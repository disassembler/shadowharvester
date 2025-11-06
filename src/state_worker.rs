// src/state_worker.rs

use crate::data_types::{PendingSolution, SubmitterCommand};
use crate::backoff::Backoff;
use reqwest::blocking::Client;
use std::path::PathBuf;
use std::thread;
use crate::persistence::Persistence;
use std::sync::mpsc::Receiver;
use crate::api;
use std::sync::Arc; // FIX: Added Arc for thread safety


// CONSTANTS
const SLED_DB_PATH: &str = "state.sled";
// Key prefixes for SLED
const SLED_KEY_RECEIPT: &str = "receipt";
const SLED_KEY_PENDING: &str = "pending";


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
/// Returns an error string that may start with "PERMANENT_ERROR:" if the failure is non-recoverable.
fn run_blocking_submission(
    client: &Client,
    api_url: &str,
    persistence: &Persistence,
    solution: PendingSolution, // Takes ownership of solution
) -> Result<(), String> {
    let mut backoff = Backoff::new(5, 300, 2.0); // 5s min, 300s max, 2.0 factor
    let pending_key = get_sled_pending_key(&solution);

    // 1. Initial Save to SLED pending queue (Ensures crash resilience)
    let solution_json = serde_json::to_string(&solution)
        .map_err(|e| format!("Failed to serialize pending solution: {}", e))?;

    if let Err(e) = persistence.set(&pending_key, &solution_json) {
        return Err(format!("FATAL: Failed to save pending solution to SLED: {}", e));
    }
    println!("📦 Solution queued to SLED pending table: {}", pending_key);

    loop {
        match api::submit_solution(client, api_url, &solution.address, &solution.challenge_id, &solution.nonce) {
            Ok(receipt_json) => {
                println!("🚀 HTTP Submitter Success: Solution for {} submitted.", solution.address);

                // 2. On success: Save final receipt to SLED
                let receipt_key = get_sled_receipt_key(&solution.address, &solution.challenge_id);
                let receipt_content = serde_json::to_string(&receipt_json)
                    .map_err(|e| format!("Failed to serialize receipt JSON: {}", e))?;

                if let Err(e) = persistence.set(&receipt_key, &receipt_content) {
                    eprintln!("⚠️ WARNING: Submission successful, but failed to save receipt to SLED: {}", e);
                } else {
                    println!("📦 Receipt saved to SLED: {}", receipt_key);
                }

                // 3. Delete from SLED pending queue
                if let Err(e) = persistence.db.remove(&pending_key) {
                    eprintln!("⚠️ WARNING: Submission successful, but failed to remove pending entry from SLED: {}", e);
                }

                return Ok(());
            }
            Err(e) => {
                // FIX: Check for the nonce consumed/exists error.
                let is_nonce_consumed = e.contains("Solution already submitted") || e.contains("Solution already exists");

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

                // All other errors (registration/difficulty mismatch, 5xx) trigger retry.
                if backoff.cur > backoff.max {
                    eprintln!("❌ Max retries reached for solution submission. Keeping in pending queue.");
                    return Err(format!("Submission failed after max backoff: {}", e));
                }

                eprintln!("⚠️ HTTP Submission failed: {}. Retrying with backoff...", e);
                backoff.sleep();
            }
        }
    }
}

/// FIX: Decouples the blocking network call from the main worker loop.
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

                // CRITICAL: Since run_blocking_submission handles logging and removing from pending queue on PERMANENT_ERROR,
                // we only need to log the high-level failure here.
                println!("❌ Submission Permanent Failure in background: {}", error_message_val);
            }
        }
    });
}


pub fn run_state_worker(
    // Receives commands from the Manager thread
    submitter_rx: Receiver<SubmitterCommand>,
    // Arguments needed for network communication (if in HTTP mode)
    client: Client,
    api_url: String,
    data_dir_base: String,
    is_websocket_mode: bool,
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
                if !is_websocket_mode {
                    // FIX: Spawn a non-blocking thread to handle the submission and retry logic.
                    spawn_submission_handler(
                        submission_client.clone(),
                        submission_api_url.clone(),
                        persistence.clone(),
                        solution, // Move solution into handler
                    );
                } else {
                    println!("⚠️ Submission Command Ignored: WebSocket mode is active. Manager should post to WS channel.");
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
