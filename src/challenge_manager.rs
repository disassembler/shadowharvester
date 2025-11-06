// src/challenge_manager.rs

use std::sync::mpsc::{Receiver, Sender};
use crate::data_types::{ManagerCommand, SubmitterCommand, ChallengeData, PendingSolution, MiningContext, DataDirMnemonic};
use std::thread;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use crate::cli::Cli;
use crate::cardano;
use super::mining;
use crate::api;
use crate::backoff::Backoff;
use std::fs;
use std::hash::{Hash, Hasher};
use crate::utils;

// Key constants for SLED state
const SLED_KEY_MINING_MODE: &str = "last_active_key_mode";
const SLED_KEY_MNEMONIC_INDEX: &str = "mnemonic_index";
const SLED_KEY_LAST_CHALLENGE: &str = "last_challenge_id";
const SLED_KEY_CHALLENGE: &str = "challenge";
const SLED_KEY_RECEIPT: &str = "receipt";

// FIX 1: Define the constant fatal error message for send failures
const SUBMITTER_SEND_FAIL: &str = "FATAL: Submitter channel closed. Submitter thread likely failed to open Sled DB.";

// Helper function to query the persistence worker and synchronously wait for the response.
fn sync_get_state(submitter_tx: &Sender<SubmitterCommand>, key: &str) -> Result<Option<String>, String> {
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    let command = SubmitterCommand::GetState(key.to_string(), response_tx);
    submitter_tx.send(command).map_err(|e| format!("Failed to send GetState command: {}", e))?;
    response_rx.recv()
        .map_err(|e| format!("Failed to receive state response: {}", e))?
        .map_err(|e| format!("Persistence worker returned error: {}", e))
}

/// Checks SLED synchronously if a receipt exists for the given address and challenge.
fn sync_check_receipt_exists(submitter_tx: &Sender<SubmitterCommand>, address: &str, challenge_id: &str) -> Result<bool, String> {
    let key = format!("{}:{}:{}", SLED_KEY_RECEIPT, address, challenge_id);
    match sync_get_state(submitter_tx, &key) {
        Ok(Some(_)) => Ok(true), // Receipt found
        Ok(None) => Ok(false), // No receipt
        Err(e) => Err(e), // Sled error
    }
}

/// Helper function to stop the currently running miner thread.
fn stop_current_miner(stop_signal: &mut Option<Arc<AtomicBool>>) {
    if let Some(signal) = stop_signal.take() {
        println!("🛑 Manager sending STOP signal to miner thread.");
        signal.store(true, Ordering::Relaxed);
    }
}

/// The main orchestration loop, replacing the old core logic in src/mining.rs.
pub fn run_challenge_manager(
    // Receives commands from network/miner threads
    manager_rx: Receiver<ManagerCommand>,
    // Sends commands to the Submitter/Persistence thread
    submitter_tx: Sender<SubmitterCommand>,
    // Pass the Manager's own Sender (manager_tx) for self-posting tasks (like fixed challenges)
    manager_tx: Sender<ManagerCommand>,
    // The CLI context needed for configuration
    mut cli: Cli,
    context: MiningContext,
) -> Result<(), String> {
    println!("🟢 Challenge Manager thread started.");

    // State maintained by the Manager
    let mut current_stop_signal: Option<Arc<AtomicBool>> = None;
    let mut current_challenge: Option<ChallengeData> = None;
    let mut last_processed_address: Option<String> = None;

    // Initial State Setup: Load Mnemonic from File
    if cli.mnemonic.is_none() {
        if let Some(file_path) = cli.mnemonic_file.as_ref() {
            match fs::read_to_string(file_path) {
                Ok(content) => {
                    // Trim whitespace and update cli.mnemonic
                    cli.mnemonic = Some(content.trim().to_string());
                }
                Err(e) => {
                    // CRITICAL FAILURE: Cannot proceed if mnemonic file is specified but unreadable.
                    eprintln!("🚨 Failed to read mnemonic file {}: {}", file_path, e);
                    return Err("Mnemonic file read error.".to_string());
                }
            }
        }
    }

    // Determine the mining mode.
    let initial_mode = if cli.ephemeral_key {
        "ephemeral".to_string()
    } else if cli.payment_key.is_some() {
        "persistent".to_string()
    } else if cli.mnemonic.is_some() {
        "mnemonic".to_string()
    } else {
        return Err("FATAL: No mining mode (ephemeral, payment-key, or mnemonic) configured.".to_string());
    };

    println!("⛏️ Initial Mining Mode: {}", initial_mode);
    submitter_tx.send(SubmitterCommand::SaveState(SLED_KEY_MINING_MODE.to_string(), initial_mode.clone()))
        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?; // Replaced unwrap

    // If running in fixed challenge mode, immediately fetch and post the challenge
    if let Some(challenge_str) = context.cli_challenge.as_ref() {
        // FIX: Borrow the owned String by using &
        let cli_challenge_data = api::parse_cli_challenge_string(challenge_str)
            .map_err(|e| format!("Challenge parameter parsing error: {}", e))?;

        let fixed_challenge_params = ChallengeData {
            challenge_id: cli_challenge_data.challenge_id.clone(),
            difficulty: cli_challenge_data.difficulty.clone(),
            no_pre_mine_key: cli_challenge_data.no_pre_mine_key.clone(),
            no_pre_mine_hour_str: cli_challenge_data.no_pre_mine_hour_str.clone(),
            latest_submission: cli_challenge_data.latest_submission.clone(),
            challenge_number: 0,
            day: 0,
            issued_at: String::new(),
        };

        println!("🎯 Starting with fixed challenge: {}", fixed_challenge_params.challenge_id);
        if manager_tx.send(ManagerCommand::NewChallenge(fixed_challenge_params)).is_err() {
            return Err("Failed to post initial fixed challenge to manager channel.".to_string());
        }
    }


    // Main loop: consumes commands from the central bus
    while let Ok(command) = manager_rx.recv() {

        // Use a block to contain the Result and perform graceful error handling
        let cycle_result: Result<(), String> = (|| {
            match command {
                ManagerCommand::NewChallenge(challenge) => {
                    // 1. Stop current mining if active
                    stop_current_miner(&mut current_stop_signal);

                    // Check if this is the same challenge we just processed
                    let is_duplicate = current_challenge.as_ref().map_or(false, |c| c.challenge_id == challenge.challenge_id);

                    if is_duplicate {
                        if initial_mode != "mnemonic" {
                            // Stop persistent/ephemeral mode from re-starting unnecessarily
                            println!("🎯 Challenge {} is the same. Waiting for miner to stop/exit.", challenge.challenge_id);
                            return Ok(());
                        } else {
                            // FIX: Mnemonic mode must re-run derivation to skip solved index. Log and proceed.
                            println!("♻️ Restarting Mnemonic cycle to derive next address.");
                        }
                    }

                    current_challenge = Some(challenge.clone());

                    // FIX 2: Save ChallengeData to Sled DB
                    let challenge_key = format!("{}:{}", SLED_KEY_CHALLENGE, challenge.challenge_id);
                    let challenge_json = serde_json::to_string(&challenge)
                        .map_err(|e| format!("Failed to serialize challenge data: {}", e))?;
                    submitter_tx.send(SubmitterCommand::SaveState(challenge_key, challenge_json))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                    submitter_tx.send(SubmitterCommand::SaveState(SLED_KEY_LAST_CHALLENGE.to_string(), challenge.challenge_id.clone()))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;


                    // 2. Determine address and key pair based on mode
                    let (key_pair_and_address, mining_address) = match initial_mode.as_str() {
                        "persistent" => {
                            let skey_hex = cli.payment_key.as_ref()
                                .ok_or_else(|| "FATAL: Persistent mode selected but key is missing.".to_string())?;
                            let kp = cardano::generate_cardano_key_pair_from_skey(skey_hex);
                            let address = kp.2.to_bech32().unwrap();

                            // FIX: Output the mode and address
                            println!("Solving for Persistent Address: {}", address);

                            (Some(kp), address)
                        }
                        "mnemonic" => {
                            let mnemonic = cli.mnemonic.as_ref()
                                 .ok_or_else(|| "FATAL: Mnemonic mode selected but key is missing during derivation.".to_string())?;

                            let account = cli.mnemonic_account;
                            let mut deriv_index: u32;

                            // FIX 3: Generate the challenge-specific key for the next index
                            let mnemonic_index_key = format!("{}:{}", SLED_KEY_MNEMONIC_INDEX, challenge.challenge_id);

                            // 1. Get last saved index for THIS challenge ID.
                            if let Ok(Some(index_str)) = sync_get_state(&submitter_tx, &mnemonic_index_key) {
                                // If found in SLED, use the saved index.
                                deriv_index = index_str.parse().unwrap_or(cli.mnemonic_starting_index);
                                println!("▶️ Resuming challenge {} at index {}.", challenge.challenge_id, deriv_index);
                            } else {
                                // If not found, use the CLI starting index.
                                deriv_index = cli.mnemonic_starting_index;
                                println!("🟢 Starting new challenge {} at index {}.", challenge.challenge_id, deriv_index);
                            }

                            let mut current_index = deriv_index;

                            // 2. Loop to skip indices that already have a RECEIPT for this challenge
                            loop {
                                let temp_keypair = cardano::derive_key_pair_from_mnemonic(mnemonic, account, current_index);
                                let temp_address = temp_keypair.2.to_bech32().unwrap();

                                // FIX: Check only for receipt. Failed or Pending solutions are *not* receipts
                                // and should be re-run if a new challenge starts.
                                match sync_check_receipt_exists(&submitter_tx, &temp_address, &challenge.challenge_id) {
                                    Ok(true) => {
                                        // Skip: Receipt found, index already solved.
                                        println!("⏭ Skipping solved address (Index {}).", current_index);
                                        current_index = current_index.wrapping_add(1);
                                    }
                                    Ok(false) => {
                                        // Found a clean index. Break the loop and use this index.
                                        break;
                                    }
                                    Err(e) => {
                                        // Sled error, log and use the current index as a safe fallback
                                        eprintln!("⚠️ Sled error during receipt check: {}. Mining at index {} as fallback.", e, current_index);
                                        break;
                                    }
                                }
                            }

                            let final_deriv_index = current_index;

                            // FIX 4: Save the final, clean index back to the challenge-specific key
                            submitter_tx.send(SubmitterCommand::SaveState(
                                mnemonic_index_key.clone(), // Use the challenge-specific key
                                final_deriv_index.to_string())
                            ).map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                            let kp = cardano::derive_key_pair_from_mnemonic(mnemonic, account, final_deriv_index);
                            let address = kp.2.to_bech32().unwrap();

                            // FIX: Output the index and address here
                            println!("Solving for Address Index {}: {}", final_deriv_index, address);

                            // FIX 5: Save the Mnemonic Address/Path to Sled for `shadow-harvester wallet` commands
                            let mnemonic_hash = {
                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                mnemonic.hash(&mut hasher);
                                hasher.finish()
                            };
                            let wallet_key = format!(
                                "{}:{}:{}:{}",
                                SLED_KEY_MNEMONIC_INDEX,
                                mnemonic_hash,
                                account,
                                final_deriv_index
                            );
                            submitter_tx.send(SubmitterCommand::SaveState(wallet_key, address.clone()))
                                .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                            (Some(kp), address)
                        }
                        "ephemeral" => {
                            let kp = cardano::generate_cardano_key_and_address();
                            let address = kp.2.to_bech32().unwrap();

                            // FIX: Output the mode and address
                            println!("Solving for Ephemeral Address: {}", address);

                            (Some(kp), address)
                        }
                        _ => {
                            // Should be unreachable.
                            return Ok(());
                        },
                    };

                    // 3. Registration
                    if key_pair_and_address.is_some() {
                        let challenge_data = current_challenge.as_ref().unwrap();
                        let address_str = mining_address.as_str();

                        // FIX: Print Mining Cycle Setup (using utils::print_mining_setup)
                        utils::print_mining_setup(
                            &context.api_url,
                            Some(address_str),
                            context.threads,
                            challenge_data
                        );
                    }

                    let mut stats_result = api::fetch_statistics(&context.client, &context.api_url, &mining_address);

                    if let Some((_, pubkey, address_obj)) = key_pair_and_address.as_ref() {
                        let reg_message = context.tc_response.message.clone();
                        let address_str = address_obj.to_bech32().unwrap();
                        let reg_signature = cardano::cip8_sign(key_pair_and_address.as_ref().unwrap(), &reg_message);

                        // FIX: Handle stats fetch and print registration status
                        match stats_result {
                            Ok(ref stats) => { // Use ref to borrow stats
                                 println!("📋 Address {} is already registered (Receipts: {}). Skipping registration.", address_str, stats.crypto_receipts);
                            },
                            Err(_) => {
                                if let Err(e) = api::register_address(
                                    &context.client, &context.api_url, &address_str, &reg_message, &reg_signature.0, &hex::encode(pubkey.as_ref()),
                                ) {
                                    eprintln!("⚠️ Address registration failed for {}: {}. Continuing attempt to mine...", address_str, e);
                                } else {
                                    println!("📋 Address registered successfully: {}", address_str);
                                    // Re-fetch stats after successful registration, overwriting the old stats_result
                                    stats_result = api::fetch_statistics(&context.client, &context.api_url, &address_str);
                                }
                            }
                        }
                    }

                    // 4. Spawn new miner threads
                    if key_pair_and_address.is_some() {
                        match mining::spawn_miner_workers(challenge.clone(), context.threads, mining_address.clone(), manager_tx.clone()) {
                            Ok(signal) => {
                                current_stop_signal = Some(signal);
                                last_processed_address = Some(mining_address.clone());

                                // FIX: REMOVED THE UNWANTED INITIAL CALL TO print_statistics HERE:
                                // utils::print_statistics(stats_result, 0, 0.0);

                                println!("⛏️ Started mining for address: {}", last_processed_address.as_ref().unwrap());
                            }
                            Err(e) => eprintln!("❌ Failed to spawn miner workers: {}", e),
                        }
                    }

                    Ok(())
                }

                ManagerCommand::SolutionFound(mut solution, total_hashes, elapsed_secs) => {
                    // 1. Stop the current mining cycle to prevent further hashing
                    stop_current_miner(&mut current_stop_signal);

                    // 2. Add donation address to the solution if configured (Submitter needs this)
                    solution.donation_address = context.donate_to_option.clone();

                    // 3. Queue for submission (State Worker handles network submission and receipt saving)
                    submitter_tx.send(SubmitterCommand::SubmitSolution(solution.clone()))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                    // 5. Print final statistics before advancing index and triggering restart
                    let address = solution.address.clone();
                    let stats_result = api::fetch_statistics(&context.client, &context.api_url, &address);

                    // FIX: This is the ONLY place we call print_statistics for a successful cycle.
                    utils::print_statistics(stats_result, total_hashes, elapsed_secs);

                    // FIX 6: Add a small delay to ensure the statistics are printed/flushed before the next cycle's output starts.
                    thread::sleep(Duration::from_millis(500));

                    // 4. Handle Mnemonic Index Advancement (for next cycle)
                    if initial_mode == "mnemonic" {

                        // FIX 7: Construct the challenge-specific key
                        let challenge_id = current_challenge.as_ref().map(|c| c.challenge_id.clone())
                            .ok_or_else(|| "FATAL: Solution found but challenge context missing.".to_string())?;
                        let mnemonic_index_key = format!("{}:{}", SLED_KEY_MNEMONIC_INDEX, challenge_id);


                        // FIX 8: Get and advance the index using the challenge-specific key
                        if let Ok(Some(index_str)) = sync_get_state(&submitter_tx, &mnemonic_index_key) {
                            if let Ok(mut index) = index_str.parse::<u32>() {
                                index = index.wrapping_add(1);

                                // FIX 9: Save the advanced index back to the challenge-specific key
                                submitter_tx.send(SubmitterCommand::SaveState(mnemonic_index_key, index.to_string()))
                                    .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                            }
                        }

                        // FIX: Self-trigger the next cycle immediately to pick up the new index/address.
                        if let Some(challenge_data) = current_challenge.clone() {
                            manager_tx.send(ManagerCommand::NewChallenge(challenge_data)).unwrap();
                        }
                    }

                    Ok(())
                }

                ManagerCommand::Shutdown => {
                    println!("🚨 Manager received shutdown signal. Stopping miner and exiting.");
                    stop_current_miner(&mut current_stop_signal);
                    submitter_tx.send(SubmitterCommand::Shutdown)
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                    return Err("Manager received Shutdown command.".to_string()); // Signal main thread to exit gracefully
                }
            }
        })(); // End of immediate invocation block

        // If an error occurred inside the block (e.g., failed mnemonic lookup), print it clearly
        // and then continue the loop, waiting for the next command.
        if let Err(e) = cycle_result {
            // Only stop the entire application if the error is the explicit Shutdown signal
            if e == "Manager received Shutdown command." {
                return Ok(()); // Allow the thread to exit cleanly
            }

            // FIX 10: Check for the specific fatal Sled error and exit the Manager thread if found.
            if e.contains(SUBMITTER_SEND_FAIL) {
                eprintln!("❌ Manager Cycle Failed (FATAL): {}", e);
                // Propagate the error out of run_challenge_manager, forcing the application to exit.
                return Err(e);
            }

            eprintln!("❌ Manager Cycle Failed (Non-Fatal): {}", e);

            // To be extra cautious, stop current mining if an error occurred in the cycle
            stop_current_miner(&mut current_stop_signal);
        }
    }

    Ok(())
}
