// src/challenge_manager.rs

use std::sync::mpsc::{Receiver, Sender};
use crate::data_types::{ManagerCommand, SubmitterCommand, ChallengeData, MiningContext, Statistics};
use std::thread;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use crate::cli::Cli;
use crate::cardano;
use super::mining;
use crate::api;
use std::fs;
use std::hash::{Hash, Hasher};
use crate::utils;

// Key constants for SLED state
const SLED_KEY_MINING_MODE: &str = "last_active_key_mode";
const SLED_KEY_MNEMONIC_INDEX: &str = "mnemonic_index";
const SLED_KEY_LAST_CHALLENGE: &str = "last_challenge_id";
const SLED_KEY_CHALLENGE: &str = "challenge";
const SLED_KEY_RECEIPT: &str = "receipt";

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
    } else if cli.mnemonic.is_some() || cli.mnemonic_file.is_some() {
        "mnemonic".to_string()
    } else {
        return Err("FATAL: No mining mode (ephemeral, payment-key, or mnemonic) configured.".to_string());
    };

    println!("⛏️ Initial Mining Mode: {}", initial_mode);
    submitter_tx.send(SubmitterCommand::SaveState(SLED_KEY_MINING_MODE.to_string(), initial_mode.clone()))
        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?; // Replaced unwrap

    // Handle fixed challenge setup if provided
    if let Some(challenge_str) = context.cli_challenge.as_ref() {
        let fixed_challenge_params = if challenge_str.contains(',') {
            // Case 1: Full 5-part challenge string provided
            let cli_challenge_data = api::parse_cli_challenge_string(challenge_str)
                .map_err(|e| format!("Challenge parameter parsing error: {}", e))?;

            let full_challenge = ChallengeData {
                challenge_id: cli_challenge_data.challenge_id.clone(),
                difficulty: cli_challenge_data.difficulty.clone(),
                no_pre_mine_key: cli_challenge_data.no_pre_mine_key.clone(),
                no_pre_mine_hour_str: cli_challenge_data.no_pre_mine_hour_str.clone(),
                latest_submission: cli_challenge_data.latest_submission.clone(),
                challenge_number: 0,
                day: 0,
                issued_at: String::new(),
            };

            // --- DEADLINE CHECK (Case 1: 5-part CLI string) ---
            utils::check_submission_deadline(full_challenge)?

        } else {
            // Case 2: Only Challenge ID provided (Lookup from Sled)
            let challenge_id = challenge_str.trim().to_string();
            let challenge_key = format!("{}:{}", SLED_KEY_CHALLENGE, challenge_id); // Key format: challenge:<ID>

            let challenge_json = sync_get_state(&submitter_tx, &challenge_key)?
                .ok_or_else(|| format!("FATAL: Fixed challenge '{}' not found in local Sled DB. Use 'challenge import' or provide a 5-part string.", challenge_id))?;

            let sled_challenge = serde_json::from_str::<ChallengeData>(&challenge_json)
                .map_err(|e| format!("Failed to deserialize challenge data from Sled: {}", e))?;

            // --- DEADLINE CHECK (Case 2: Sled Lookup) ---
            utils::check_submission_deadline(sled_challenge)?
        };


        println!("🎯 Starting with fixed challenge: {}", fixed_challenge_params.challenge_id);
        if manager_tx.send(ManagerCommand::NewChallenge(fixed_challenge_params)).is_err() {
            return Err("Failed to post initial fixed challenge to manager channel.".to_string());
        }
    }


    // Main loop: consumes commands from the central bus
    while let Ok(command) = manager_rx.recv() {
        let start_mining = |challenge: &ChallengeData| -> Result<Option<Arc<AtomicBool>>, String> {
            // 2. Determine address and key pair based on mode
            let (key_pair_and_address, mining_address) = match initial_mode.as_str() {
                "persistent" => {
                    // ... (persistent key logic remains the same)
                    let skey_hex = cli.payment_key.as_ref()
                        .ok_or_else(|| "FATAL: Persistent mode selected but key is missing.".to_string())?;
                    let kp = cardano::generate_cardano_key_pair_from_skey(skey_hex);
                    let address = kp.2.to_bech32().unwrap();

                    println!("Solving for Persistent Address: {}", address);
                    (Some(kp), address)
                }
                "mnemonic" => {
                    // ... (mnemonic logic remains the same)
                    let mnemonic = cli.mnemonic.as_ref()
                         .ok_or_else(|| "FATAL: Mnemonic mode selected but key is missing during derivation.".to_string())?;

                    let account = cli.mnemonic_account;
                    let deriv_index: u32;

                    let mnemonic_index_key = format!("{}:{}", SLED_KEY_MNEMONIC_INDEX, challenge.challenge_id);

                    if let Ok(Some(index_str)) = sync_get_state(&submitter_tx, &mnemonic_index_key) {
                        deriv_index = index_str.parse().unwrap_or(cli.mnemonic_starting_index);
                        println!("▶️ Resuming challenge {} at index {}.", challenge.challenge_id, deriv_index);
                    } else {
                        deriv_index = cli.mnemonic_starting_index;
                        println!("🟢 Starting new challenge {} at index {}.", challenge.challenge_id, deriv_index);
                    }

                    let mut current_index = deriv_index;

                    loop {
                        let temp_keypair = cardano::derive_key_pair_from_mnemonic(mnemonic, account, current_index);
                        let temp_address = temp_keypair.2.to_bech32().unwrap();

                        match sync_check_receipt_exists(&submitter_tx, &temp_address, &challenge.challenge_id) {
                            Ok(true) => {
                                println!("⏭ Skipping solved address (Index {}).", current_index);
                                current_index = current_index.wrapping_add(1);
                            }
                            Ok(false) => { break; }
                            Err(e) => {
                                eprintln!("⚠️ Sled error during receipt check: {}. Mining at index {} as fallback.", e, current_index);
                                break;
                            }
                        }
                    }

                    let final_deriv_index = current_index;

                    submitter_tx.send(SubmitterCommand::SaveState(
                        mnemonic_index_key.clone(), // Use the challenge-specific key
                        final_deriv_index.to_string())
                    ).map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                    let kp = cardano::derive_key_pair_from_mnemonic(mnemonic, account, final_deriv_index);
                    let address = kp.2.to_bech32().unwrap();

                    println!("Solving for Address Index {}: {}", final_deriv_index, address);

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
                    // ... (ephemeral key logic remains the same)
                    let kp = cardano::generate_cardano_key_and_address();
                    let address = kp.2.to_bech32().unwrap();

                    println!("Solving for Ephemeral Address: {}", address);
                    (Some(kp), address)
                }
                _ => { return Ok(None); },
            };

            // 3. Registration
            let should_contact_api = !cli.websocket; // <-- Check WS mode flag

            if key_pair_and_address.is_some() {
                // Print setup regardless of WS mode
                utils::print_mining_setup(
                    &context.api_url,
                    Some(&mining_address),
                    context.threads,
                    challenge,
                );
            }

            let stats_result: Result<Statistics, String> = if should_contact_api {
                // Only fetch statistics if NOT in WebSocket mode
                api::fetch_statistics(&context.client, &context.api_url, &mining_address)
            } else {
                // In WS mode, return a dummy error that the match block below will handle gracefully.
                Err("WebSocket mode: API contact skipped.".to_string())
            };

            if let Some((_key_pair, pubkey, address_obj)) = key_pair_and_address.as_ref() {
                let reg_message = context.tc_response.message.clone();
                let address_str = address_obj.to_bech32().unwrap();
                let reg_signature = cardano::cip8_sign(key_pair_and_address.as_ref().unwrap(), &reg_message);

                // Handle conditional registration and stats print
                match stats_result {
                    Ok(ref stats) => { // Stats successfully fetched (implies HTTP mode)
                         println!("📋 Address {} is already registered (Receipts: {}). Skipping registration.", address_str, stats.crypto_receipts);
                    },
                    Err(ref e) if e == "WebSocket mode: API contact skipped." => { // Handle WS skip gracefully
                        println!("📋 Address registration and statistics fetch skipped (WebSocket Mode).");
                    }
                    Err(_) => {
                        // Stats fetch failed (only happens in HTTP mode). Attempt registration.
                        if let Err(reg_e) = api::register_address(
                            &context.client, &context.api_url, &address_str, &reg_message, &reg_signature.0, &hex::encode(pubkey.as_ref()),
                        ) {
                            eprintln!("⚠️ Address registration failed for {}: {}. Continuing attempt to mine...", address_str, reg_e);
                        } else {
                            println!("📋 Address registered successfully: {}", address_str);
                            // Re-fetch stats after successful registration, discarding the result with `let _ = ...`
                            let _ = api::fetch_statistics(&context.client, &context.api_url, &address_str);
                        }
                    }
                }

                // 4. Execute synchronous Donation API call if configured
                if let Some(donation_address) = context.donate_to_option.as_ref() {
                    let donation_message = format!("Assign accumulated Scavenger rights to: {}", donation_address);

                    // Generate the signature for the donation message using the current key pair
                    let (donation_signature, _) = cardano::cip8_sign(key_pair_and_address.as_ref().unwrap(), &donation_message);

                    println!("🚀 Attempting synchronous donation for {}...", mining_address);
                    match api::donate_to(
                        &context.client,
                        &context.api_url,
                        &mining_address,
                        donation_address,
                        &donation_signature,
                    ) {
                        Ok(id) => println!("✅ Donation initiated successfully. ID: {}", id),
                        Err(e) => eprintln!("⚠️ Donation failed (manager attempt): {}", e),
                    }
                }
            }

            // 5. Spawn new miner threads
            Ok(match key_pair_and_address {
                Some(_) => {
                    let stop_signal = Some(mining::spawn_miner_workers(challenge.clone(), context.threads, mining_address.clone(), manager_tx.clone())
                        .map_err(|e| format!("❌ Failed to spawn miner workers: {}", e))?);
                    println!("⛏️ Started mining for address: {}", mining_address);
                    stop_signal
                },
                None => None,
            })
        };

        let cycle_result: Result<(), String> = (|| {
            match command {
                ManagerCommand::NewChallenge(challenge) => {
                    // 1. Check if this is the same challenge we just processed
                    let is_duplicate = current_challenge.as_ref().is_some_and(|c| c.challenge_id == challenge.challenge_id);

                    if is_duplicate {
                        // Stop from re-starting unnecessarily
                        println!("🎯 Challenge {} is the same. Waiting for miner to stop/exit.", challenge.challenge_id);
                        return Ok(());
                    }


                    // Save ChallengeData to Sled DB
                    let challenge_key = format!("{}:{}", SLED_KEY_CHALLENGE, challenge.challenge_id);
                    let challenge_json = serde_json::to_string(&challenge)
                        .map_err(|e| format!("Failed to serialize challenge data: {}", e))?;
                    submitter_tx.send(SubmitterCommand::SaveState(challenge_key, challenge_json))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                    submitter_tx.send(SubmitterCommand::SaveState(SLED_KEY_LAST_CHALLENGE.to_string(), challenge.challenge_id.clone()))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                    let is_mining = current_challenge.is_some();
                    current_challenge = Some(challenge.clone());

                    if !is_mining {
                        current_stop_signal = start_mining(&challenge)?;
                    }

                    Ok(())
                }

                ManagerCommand::SolutionFound(solution, total_hashes, elapsed_secs) => {
                    // 1. Stop the current mining cycle to prevent further hashing
                    stop_current_miner(&mut current_stop_signal);

                    // 2. Queue for submission (State Worker handles network submission and receipt saving)
                    submitter_tx.send(SubmitterCommand::SubmitSolution(solution.clone()))
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;

                    // 3. Print final statistics before advancing index and triggering restart
                    let address = solution.address.clone();

                    // Stats fetch is still needed here for printing, but we must check WS mode
                    let stats_result = if !cli.websocket { // Check WS mode flag
                        api::fetch_statistics(&context.client, &context.api_url, &address)
                    } else {
                        // Return dummy error in WS mode to avoid API contact
                        Err("WebSocket mode: API contact skipped.".to_string())
                    };

                    // Use a safe match statement instead of unwrap_err() on Result
                    match stats_result {
                        Ok(stats) => {
                            // Stats were successfully fetched (HTTP mode)
                            utils::print_statistics(Ok(stats), total_hashes, elapsed_secs);
                        }
                        Err(e) if e == "WebSocket mode: API contact skipped." => {
                            // Stats were intentionally skipped (WS mode)
                            println!("📈 Statistics printing skipped (WebSocket Mode).");
                        }
                        Err(e) => {
                            // A real error occurred during stats fetch (HTTP mode)
                            utils::print_statistics(Err(e), total_hashes, elapsed_secs);
                        }
                    }

                    // Add a small delay to ensure the statistics are printed/flushed before the next cycle's output starts.
                    thread::sleep(Duration::from_millis(500));

                    // 4. Handle Mnemonic Index Advancement (for next cycle)
                    // This is not really needed because `start_mining()` skips already solved indices,
                    // but it leads to better looking logs and a tiny speedup if we do the advancement for it.
                    if initial_mode == "mnemonic" {

                        // Construct the challenge-specific key
                        let challenge_id = current_challenge.as_ref().map(|c| c.challenge_id.clone())
                            .ok_or_else(|| "FATAL: Solution found but challenge context missing.".to_string())?;
                        let mnemonic_index_key = format!("{}:{}", SLED_KEY_MNEMONIC_INDEX, challenge_id);


                        // Get and advance the index using the challenge-specific key
                        if let Ok(Some(index_str)) = sync_get_state(&submitter_tx, &mnemonic_index_key) {
                            if let Ok(mut index) = index_str.parse::<u32>() {
                                index = index.wrapping_add(1);

                                // Save the advanced index back to the challenge-specific key
                                submitter_tx.send(SubmitterCommand::SaveState(mnemonic_index_key, index.to_string()))
                                    .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                            }
                        }
                    }

                    current_stop_signal = start_mining(current_challenge.as_ref().unwrap())?;

                    Ok(())
                }

                ManagerCommand::Shutdown => {
                    println!("🚨 Manager received shutdown signal. Stopping miner and exiting.");
                    stop_current_miner(&mut current_stop_signal);
                    submitter_tx.send(SubmitterCommand::Shutdown)
                        .map_err(|_| SUBMITTER_SEND_FAIL.to_string())?;
                    Err("Manager received Shutdown command.".to_string())// Signal main thread to exit gracefully
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

            // Check for the specific fatal Sled error and exit the Manager thread if found.
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
