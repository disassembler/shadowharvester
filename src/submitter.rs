// src/submitter.rs

use crate::data_types::{PendingSolution, DataDir};
use crate::api;
use crate::backoff::Backoff;
use reqwest::blocking::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

// CONSTANTS for the submitter loop
const SUBMISSION_INTERVAL_SECS: u64 = 5;
const QUEUE_BASE_DIR: &str = "pending_submissions";

pub fn run_submitter_thread(client: Client, api_url: String, data_dir_base: String) -> Result<(), String> {
    println!("📦 Starting background submission queue monitor.");
    let queue_path = PathBuf::from(&data_dir_base).join(QUEUE_BASE_DIR);

    if !queue_path.exists() {
        if let Err(e) = fs::create_dir_all(&queue_path) {
            return Err(format!("Failed to create submission queue directory: {}", e));
        }
    }

    loop {
        // --- 1. Scan for pending solution files ---
        let mut processed_submission = false;
        match fs::read_dir(&queue_path) {
            Ok(entries) => {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                        // Attempt to process the file, break on success to immediately check the next one
                        if process_pending_solution(&client, &api_url, &path, &data_dir_base).is_ok() {
                            processed_submission = true;
                            break;
                        }
                    }
                }
            },
            Err(e) => eprintln!("Error reading submission queue directory: {}", e),
        }

        // --- 2. Sleep based on activity ---
        if !processed_submission {
            thread::sleep(Duration::from_secs(SUBMISSION_INTERVAL_SECS));
        }
    }
}

fn process_pending_solution(client: &Client, api_url: &str, file_path: &Path, data_dir_base: &str) -> Result<(), String> {
    // --- 1. Load the pending solution ---
    let solution_json = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read pending solution file {:?}: {}", file_path, e))?;

    let solution: PendingSolution = serde_json::from_str(&solution_json)
        .map_err(|e| format!("Failed to parse pending solution JSON {:?}: {}", file_path, e))?;

    println!("\n📦 Attempting to submit queued solution for Challenge ID {} (Nonce: {})...", solution.challenge_id, solution.nonce);

    // --- 2. Submission Retry Loop (with Backoff) ---
    let mut backoff = Backoff::new(5, 300, 2.0); // min 5s, max 300s, 2.0 factor
    let mut final_receipt: Option<serde_json::Value> = None;
    let mut submission_success = false;
    let mut non_recoverable_error = false;

    // Retry indefinitely on network errors, but break on API validation errors
    loop {
        match api::submit_solution(
            client, api_url, &solution.address, &solution.challenge_id, &solution.nonce,
        ) {
            Ok(receipt) => {
                final_receipt = Some(receipt);
                submission_success = true;
                break;
            },
            Err(e) if e.contains("Network/Client Error") => {
                eprintln!("⚠️ Solution submission failed (Network Error): {}. Retrying...", e);
                backoff.sleep();
                continue;
            },
            Err(e) => {
                // Determine if this is a recoverable 5xx error or a non-recoverable 4xx validation error.
                // We check for 50x or 51x (to be safe) status codes, which indicate server-side failure.
                let is_5xx_server_error = e.contains("(Status 50") || e.contains("(Status 51");

                if is_5xx_server_error {
                    eprintln!("⚠️ Solution submission failed (Server Error, possibly transient): {}. Retrying with backoff...", e);
                    backoff.sleep();
                    continue;
                } else {
                    // Treat 4xx errors (API Validation, Already Solved, Challenge Expired, etc.) as non-recoverable.
                    eprintln!("❌ Non-recoverable API Submission Error. Deleting from queue. Details: {}", e);
                    non_recoverable_error = true;
                    break;
                }
            }
        }
    }

    if submission_success {
        // Submission Success Confirmation
        println!("🚀 Successfully submitted solution for Index {} (Challenge: {})", solution.address, solution.challenge_id);

        // --- 3. Save Receipt and Clean Up ---
        let receipt = final_receipt.unwrap();

        // Determine the correct DataDir variant for saving the receipt
        // Heuristic: differentiate Ephemeral from Persistent/Mnemonic based on address string.
        let data_dir_instance = if solution.address.starts_with("addr_vk") {
            DataDir::Ephemeral(&solution.address)
        } else {
            // Use Persistent as a default for address-based pathing (covers both Persistent and Mnemonic key modes' final address structure)
            DataDir::Persistent(&solution.address)
        };

        // Call simplified save_receipt function (no donation ID)
        if let Err(e) = data_dir_instance.save_receipt(data_dir_base, &solution.challenge_id, &receipt) {
            eprintln!("FATAL: Successfully submitted but FAILED TO SAVE LOCAL RECEIPT: {}. The solution was accepted by the server. Delete the pending file manually to prevent re-submission.", e);
            // Even though local save failed, the server has the solution. We must remove the pending file.
        }

        // Delete the pending solution file after successful API submission and (attempted) local receipt save
        if let Err(e) = fs::remove_file(file_path) {
            eprintln!("⚠️ WARNING: Successfully submitted solution but FAILED TO DELETE PENDING FILE {:?}: {}. This file may be resubmitted.", file_path, e);
        }

        Ok(())
    } else if non_recoverable_error {
        // Non-recoverable error, clean up the file
         if let Err(e) = fs::remove_file(file_path) {
            eprintln!("⚠️ WARNING: Received unrecoverable submission error but FAILED TO DELETE PENDING FILE {:?}: {}.", file_path, e);
        }
        Err(format!("Non-recoverable error processing {:?}", file_path))
    } else {
        // Should be unreachable if the loop logic is correct, but indicates a break without success/fatal error.
        Err("Submission thread encountered unexpected state.".to_string())
    }
}
