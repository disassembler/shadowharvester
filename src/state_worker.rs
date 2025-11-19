// src/state_worker.rs

use crate::data_types::{PendingSolution, SubmitterCommand, WebSocketCommand, ChallengeData};
use crate::backoff::Backoff;
use reqwest::blocking::Client;
use std::path::PathBuf;
use std::thread;
use crate::persistence::Persistence;
use std::sync::mpsc::{Receiver, Sender};
use crate::api;
use std::sync::Arc;
use serde_json::{self};
use crate::utils::check_submission_deadline; // Need this for expiration check
use std::collections::HashMap; // Need this for challenge cache


// CONSTANTS
const SLED_DB_PATH: &str = "state.sled";
// Key prefixes for SLED
const SLED_KEY_RECEIPT: &str = "receipt";
const SLED_KEY_PENDING: &str = "pending";
const SLED_KEY_CHALLENGE: &str = "challenge";


/// Constructs the unique key used to store a pending solution in Sled.
/// Format: pending:<ADDRESS>:<CHALLENGE_ID>:<NONCE>
fn get_sled_pending_key(solution: &PendingSolution) -> String {
    format!("{}:{}:{}:{}", SLED_KEY_PENDING, solution.address, solution.challenge_id, solution.nonce)
}

/// Constructs the unique key used to store a receipt in Sled.
/// Format: receipt:<ADDRESS>:<CHALLENGE_ID>
fn get_sled_receipt_key(address: &str, challenge_id: &str) -> String {
    format!("{}:{}:{}", SLED_KEY_RECEIPT, address, challenge_id)
}

/// Attempts to submit a solution to the API with exponential backoff and saves the receipt on success.
/// Assumes the solution has already been saved to Sled's pending queue by the caller (run_state_worker).
/// Returns an error string that may start with "PERMANENT_ERROR:" if the failure is non-recoverable.
fn run_blocking_submission(
    client: &Client,
    api_url: &str,
    persistence: &Persistence,
    solution: PendingSolution, // Takes ownership of solution
) -> Result<(), String> {
    let mut backoff = Backoff::new(5, 300, 2.0); // 5s min, 300s max, 2.0 factor
    let pending_key = get_sled_pending_key(&solution);

    // NOTE: The solution is now assumed to be in SLED's pending queue upon entry.

    loop {
        match api::submit_solution(client, api_url, &solution.address, &solution.challenge_id, &solution.nonce) {
            Ok(receipt_json) => {
                println!("🚀 HTTP Submitter Success: Solution for {} submitted.", solution.address);

                // 1. On success: Save final receipt to SLED
                let receipt_key = get_sled_receipt_key(&solution.address, &solution.challenge_id);
                let receipt_content = serde_json::to_string(&receipt_json)
                    .map_err(|e| format!("Failed to serialize receipt JSON: {}", e))?;

                if let Err(e) = persistence.set(&receipt_key, &receipt_content) {
                    eprintln!("⚠️ WARNING: Submission successful, but failed to save receipt to SLED: {}", e);
                } else {
                    println!("📦 Receipt saved to SLED: {}", receipt_key);
                }

                // 2. Delete from SLED pending queue
                if let Err(e) = persistence.db.remove(&pending_key) {
                    eprintln!("⚠️ WARNING: Submission successful, but failed to remove pending entry from SLED: {}", e);
                }

                return Ok(());
            }
            Err(e) => {
                // FIX: Check for the nonce consumed/exists error.
                let is_nonce_consumed = e.contains("Solution already submitted") || e.contains("Solution already exists");
                let is_deadline_past = e.contains("Submission window closed");

                if is_nonce_consumed {
                    // CRITICAL: Solution is consumed. Set a marker receipt to prevent re-mining this address.
                    let solved_marker_key = get_sled_receipt_key(&solution.address, &solution.challenge_id);
                    let solved_marker_json = serde_json::json!({
                        "status": "solved_by_network",
                        "challenge_id": solution.challenge_id,
                        "address": solution.address,
                        "note": "Solution consumed by network; no receipt recovered."
                    }).to_string();

                    let _ = persistence.set(&solved_marker_key, &solved_marker_json)
                        .map(|_| println!("✅ Solution confirmed solved by network. Marker set in DB: {}", solved_marker_key))
                        .map_err(|e_set| eprintln!("⚠️ WARNING: Solution consumed, but failed to set SOLVED marker in SLED: {}", e_set));

                    // Always delete from pending queue and mark as a permanent error to exit retry loop.
                    let _ = persistence.db.remove(&pending_key);

                    return Err(format!("PERMANENT_ERROR: Solution consumed by network: {}", e));
                }

                else if is_deadline_past {

                    // The item remains in SLED's pending queue for manual inspection/removal
                    eprintln!("⚠️ HTTP Submission failed: {}. Exiting submission thread because deadline has passed", e);
                    // The thread will exit gracefully, leaving the solution in the pending queue
                    return Err(format!("PERMANENT_ERROR: Deadline passed: {}", e));
                }

                // All other errors (registration/difficulty mismatch, 5xx) trigger retry.
                if backoff.cur >= backoff.max { // Check against max *before* sleeping
                    eprintln!("❌ Max retries reached for solution submission. Keeping in pending queue.");
                    return Err(format!("Submission failed after max backoff: {}", e));
                }

                eprintln!("⚠️ HTTP Submission failed: {}. Retrying with backoff...", e);
                backoff.sleep();
            }
        }
    }
}

/// Decouples the blocking network call from the main worker loop.
fn spawn_submission_handler(
    client: Client,
    api_url: String,
    persistence: Arc<Persistence>, // Use Arc<Persistence>
    solution: PendingSolution,
) {
    thread::spawn(move || {
        // We clone the client and move the persistence Arc and the solution into the thread
        if let Err(e) = run_blocking_submission(&client, &api_url, &persistence, solution) {
            // Log non-recoverable errors but allow the thread to exit.
            if e.starts_with("PERMANENT_ERROR") {
                let error_message_val = e.strip_prefix("PERMANENT_ERROR: ").unwrap_or(&e).to_string();

                // If run_blocking_submission returned a permanent error, it already handled removing from the pending queue
                // or setting a solved marker. We just log the high-level failure here.
                println!("❌ Submission Permanent Failure in background: {}", error_message_val);
            }
        }
    });
}

// --- NEW SWEEP IMPLEMENTATION ---

fn sweep_pending_solutions(persistence: &Arc<Persistence>, ws_tx: &Sender<WebSocketCommand>) -> Result<(), String> {
    println!("\n🧹 Starting sweep for unsubmitted solutions in SLED pending queue...");

    let pending_prefix = format!("{}:", SLED_KEY_PENDING);
    let challenge_prefix = format!("{}:", SLED_KEY_CHALLENGE);
    let mut sent_count = 0;

    // 1. Collect all valid ChallengeData objects for expiration check
    let mut challenge_data_cache: HashMap<String, ChallengeData> = HashMap::new();

    // Iterate over challenge entries
    for entry_result in persistence.db.scan_prefix(challenge_prefix.as_bytes()) {
        match entry_result {
            Ok((key_ivec, value_ivec)) => {
                let key = String::from_utf8_lossy(&key_ivec);
                if let Some(challenge_id) = key.strip_prefix(challenge_prefix.as_str()) {
                    if let Ok(data) = serde_json::from_slice::<ChallengeData>(&value_ivec) {
                        challenge_data_cache.insert(challenge_id.to_string(), data);
                    }
                }
            },
            Err(e) => eprintln!("⚠️ SLED error during challenge cache: {}", e),
        }
    }

    // 2. Iterate over all pending solutions
    for entry_result in persistence.db.scan_prefix(pending_prefix.as_bytes()) {
        match entry_result {
            Ok((key_ivec, value_ivec)) => {
                let pending_key_str = String::from_utf8_lossy(&key_ivec);
                if let Ok(solution) = serde_json::from_slice::<PendingSolution>(&value_ivec) {

                    // 3. Check Expiration
                    let is_expired = match challenge_data_cache.get(&solution.challenge_id) {
                        Some(challenge) => {
                            // check_submission_deadline returns Err(String) if expired
                            if let Err(e) = check_submission_deadline(challenge.clone()) {
                                println!("⚠️ Solution for {} is expired. Removing from pending queue: {}", solution.challenge_id, e);
                                // Delete the expired solution from the pending queue
                                let _ = persistence.db.remove(key_ivec); // FIX E0277: Use the IVec key
                                true
                            } else {
                                false
                            }
                        },
                        None => {
                            // Can't find challenge data, assume it's still good for now
                            println!("⚠️ Cannot find ChallengeData for {}. Assuming non-expired and attempting submit.", solution.challenge_id);
                            false
                        }
                    };

                    // 4. Submit if not expired
                    if !is_expired {
                        // Send the solution to the WebSocket Server thread
                        if ws_tx.send(WebSocketCommand::SubmitSolution(solution)).is_err() {
                            // If the channel is disconnected, the WS server is down. Stop the sweep.
                            return Err("WebSocket channel closed during sweep.".to_string());
                        }
                        sent_count += 1;
                    }

                } else {
                    eprintln!("⚠️ Failed to parse PendingSolution for key: {}", pending_key_str);
                    // Consider deleting bad data, but we'll leave it for manual inspection for now.
                }
            },
            Err(e) => {
                eprintln!("⚠️ SLED error during pending sweep iteration: {}", e);
            }
        }
    }

    println!("🧹 Sweep complete. Sent {} pending solutions to WebSocket client.", sent_count);
    Ok(())
}

// --- END NEW SWEEP IMPLEMENTATION ---


pub fn run_state_worker(
    // Receives commands from the Manager thread
    submitter_rx: Receiver<SubmitterCommand>,
    // Arguments needed for network communication (if in HTTP mode)
    client: Client,
    api_url: String,
    data_dir_base: String,
    is_websocket_mode: bool,
    ws_tx: Sender<WebSocketCommand>, // Added ws_tx
) -> Result<(), String> {
    println!("📦 Starting persistence and submission thread (SLED DB).");

    // FIX: Persistence must be wrapped in Arc for thread safety when cloning it into submission handlers.
    let persistence = Arc::new(Persistence::open(PathBuf::from(&data_dir_base).join(SLED_DB_PATH))
        .map_err(|e| format!("FATAL: Could not initialize SLED database. Is another process running and locking the DB? Details: {}", e))?);

    // Clone client and API URL for submission handlers
    let submission_client = client;
    let submission_api_url = api_url;


    // 2. Main Command Loop
    while let Ok(command) = submitter_rx.recv() {
        match command {
            SubmitterCommand::SaveState(key, value) => {
                if let Err(e) = persistence.set(&key, &value) {
                    eprintln!("⚠️ Persistence Error: Failed to save state key '{}': {}", key, e);
                }
            }
            SubmitterCommand::GetState(key, response_tx) => {
                // Synchronous SLED lookup (FAST operation, safe to run directly)
                let result = persistence.get(&key);
                // We don't panic if the response channel is closed, only if the Sled op failed
                if response_tx.send(result).is_err() {
                    eprintln!("⚠️ Warning: Failed to send Sled response back for key '{}'. Manager thread may be dead.", key);
                }
            }
            SubmitterCommand::SubmitSolution(solution) => {

                // --- CRITICAL FIX: Save solution to SLED pending queue for crash resilience (All Modes) ---
                let pending_key = get_sled_pending_key(&solution);
                let solution_json = match serde_json::to_string(&solution) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("FATAL: Failed to serialize pending solution for SLED: {}", e);
                        continue;
                    }
                };

                if let Err(e) = persistence.set(&pending_key, &solution_json) {
                    eprintln!("FATAL: Failed to save pending solution to SLED: {}", e);
                    continue;
                }
                println!("📦 Solution queued to SLED pending table: {}", pending_key);

                if !is_websocket_mode {
                    // HTTP MODE: Spawn a non-blocking thread to handle the submission and retry logic.
                    // run_blocking_submission now assumes it's already in SLED and removes on success/permanent failure.
                    spawn_submission_handler(
                        submission_client.clone(),
                        submission_api_url.clone(),
                        persistence.clone(),
                        solution, // Move solution into handler
                    );
                } else {
                    // WS MODE: Forward solution to the WebSocket server thread.
                    // It remains in SLED until the browser submission is manually confirmed/removed later.
                    if let Err(e) = ws_tx.send(WebSocketCommand::SubmitSolution(solution)) {
                        eprintln!("❌ FATAL ERROR: Failed to forward solution to WebSocket server: {}", e);
                    }
                    println!("🚀 Solution queued to be sent via WebSocket.");
                }
            }
            SubmitterCommand::SweepPendingSolutions => {
                if is_websocket_mode {
                    // Execute the sweep logic, which sends solutions via ws_tx
                    if let Err(e) = sweep_pending_solutions(&persistence, &ws_tx) {
                        eprintln!("❌ FATAL SWEEP ERROR: {}", e);
                        // If the error is due to a closed channel, the thread must shut down.
                        if e.contains("WebSocket channel closed") {
                            break;
                        }
                    }
                } else {
                    // Ignore sweep command if not in WS mode
                    println!("Sweep command received but ignored (Not in WebSocket mode).");
                }
            }
            SubmitterCommand::Shutdown => {
                // FIX: Unwrap Arc to close the underlying Sled DB
                match Arc::try_unwrap(persistence) {
                    Ok(p) => if let Err(e) = p.close() { eprintln!("⚠️ Error flushing SLED DB on shutdown: {}", e); },
                    Err(_) => eprintln!("⚠️ Error: Could not unwrap Persistence Arc on shutdown. Submission threads may still be alive."),
                }
                println!("📦 Submitter thread shutting down.");
                break;
            }
        }
    }

    Ok(())
}
