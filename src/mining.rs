// src/mining.rs

use crate::api;
use crate::data_types::{DataDir, DataDirMnemonic, MiningContext, MiningResult, ChallengeData, PendingSolution, FILE_NAME_FOUND_SOLUTION, is_solution_pending_in_queue, FILE_NAME_RECEIPT};
use crate::cli::Cli;
use crate::cardano;
use crate::utils::{self, next_wallet_deriv_index_for_challenge, print_mining_setup, print_statistics, receipt_exists_for_index, run_single_mining_cycle};
use std::{fs};

// ===============================================
// SOLUTION RECOVERY FUNCTION
// ===============================================

/// Checks the local storage for any solution that was found but not yet queued
/// and queues it if found.
fn check_for_unsubmitted_solutions(base_dir: &str, challenge_id: &str, mining_address: &str, data_dir_variant: &DataDir) -> Result<(), String> {
    // Determine the base path for the specific wallet/challenge
    let mut path = data_dir_variant.receipt_dir(base_dir, challenge_id)?;
    path.push(FILE_NAME_FOUND_SOLUTION);

    if path.exists() {
        println!("\n⚠️ Recovery file detected at {:?}. Recovering solution...", path);

        let solution_json = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read recovery file {:?}: {}", path, e))?;

        let pending_solution: PendingSolution = serde_json::from_str(&solution_json)
            .map_err(|e| format!("Failed to parse recovery solution JSON {:?}: {}", path, e))?;

        // 1. Save to the main submission queue
        if let Err(e) = data_dir_variant.save_pending_solution(base_dir, &pending_solution) {
            return Err(format!("FATAL RECOVERY ERROR: Could not queue recovered solution: {}", e));
        }

        // 2. Delete the recovery file
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("WARNING: Successfully queued recovered solution but FAILED TO DELETE RECOVERY FILE {:?}: {}", path, e);
        } else {
            println!("✅ Successfully recovered and queued solution for address {} / challenge {}.", mining_address, challenge_id);
        }
    }
    Ok(())
}

// ===============================================
// MINING MODE FUNCTIONS (Core Logic Only)
// ===============================================

/// MODE A: Persistent Key Continuous Mining
#[allow(unused_assignments)] // Suppress warnings for final_hashes/final_elapsed assignments
pub fn run_persistent_key_mining(context: MiningContext, skey_hex: &String) -> Result<(), String> {
    let key_pair = cardano::generate_cardano_key_pair_from_skey(skey_hex);
    let mining_address = key_pair.2.to_bech32().unwrap();
    let mut final_hashes: u64 = 0;
    let mut final_elapsed: f64 = 0.0;
    let reg_message = context.tc_response.message.clone();
    let data_dir = DataDir::Persistent(&mining_address);

    println!("\n[REGISTRATION] Attempting initial registration for address: {}", mining_address);
    let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);
    if let Err(e) = api::register_address(
        &context.client, &context.api_url, &mining_address, &context.tc_response.message, &reg_signature.0, &hex::encode(key_pair.1.as_ref()),
    ) {
        eprintln!("Address registration failed: {}. Cannot start mining.", e);
        return Err("Address registration failed.".to_string());
    }

    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: PERSISTENT KEY MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    let mut current_challenge_id = String::new();
    let mut last_active_challenge_data: Option<ChallengeData> = None;
    loop {
        let challenge_params = match utils::get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(params)) => {
                last_active_challenge_data = Some(params.clone());
                params
            },
            Ok(None) => continue,
            Err(e) => {
                // If a challenge ID is set AND we detect a network failure, continue mining.
                if !current_challenge_id.is_empty() && e.contains("API request failed") {
                    eprintln!("⚠️ Challenge API poll failed (Network Error): {}. Continuing mining with previous challenge parameters (ID: {})...", e, current_challenge_id);
                    last_active_challenge_data.as_ref().cloned().ok_or_else(|| {
                        format!("FATAL LOGIC ERROR: Challenge ID {} is set but no previous challenge data was stored.", current_challenge_id)
                    })?
                } else {
                    eprintln!("⚠️ Critical API Error during challenge check: {}. Retrying in 1 minute...", e);
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    continue;
                }
            }
        };

        // Check for unsubmitted solutions from previous run
        if let Some(base_dir) = context.data_dir {
            check_for_unsubmitted_solutions(base_dir, &challenge_params.challenge_id, &mining_address, &data_dir)?;
        }

        if let Some(base_dir) = context.data_dir { data_dir.save_challenge(base_dir, &challenge_params)?; }
        print_mining_setup(&context.api_url, Some(mining_address.as_str()), context.threads, &challenge_params);

        loop {
            // UPDATED CALL: Removed client and api_url
            let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                mining_address.clone(), context.threads, context.donate_to_option, &challenge_params, context.data_dir,
            );
            final_hashes = total_hashes; final_elapsed = elapsed_secs;

            match result {
                MiningResult::FoundAndQueued => {
                    if let Some(ref destination_address) = context.donate_to_option {
                        let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
                        let donation_signature = cardano::cip8_sign(&key_pair, &donation_message);

                        // Intentionally perform donation attempt synchronously here.
                        match api::donate_to(
                            &context.client, &context.api_url, &mining_address, destination_address, &donation_signature.0,
                        ) {
                            Ok(id) => println!("🚀 Donation initiated successfully. ID: {}", id),
                            Err(e) => eprintln!("⚠️ Donation failed (synchronous attempt): {}", e),
                        }
                    }

                    println!("\n✅ Solution queued. Continuing mining immediately.");
                    // Continue the loop on the same address.
                },
                MiningResult::AlreadySolved => {
                    println!("\n✅ Challenge already solved on network. Stopping current mining.");
                    // Solution saved by submitter/already exists, so check for a new challenge.
                    break;
                }
                MiningResult::MiningFailed => {
                    eprintln!("\n⚠️ Mining cycle failed. Checking if challenge is still valid before retrying...");
                    if context.cli_challenge.is_none() {
                        match api::get_active_challenge_data(&context.client,&context.api_url) {
                            Ok(active_params) if active_params.challenge_id == current_challenge_id => {
                                eprintln!("Challenge is still valid. Retrying mining cycle in 1 minute...");
                                std::thread::sleep(std::time::Duration::from_secs(60));
                            },
                            Ok(_) | Err(_) => {
                                eprintln!("Challenge appears to have changed or API is unreachable. Stopping current mining and checking for new challenge...");
                                break;
                            }
                        }
                    } else {
                        eprintln!("Fixed challenge. Retrying mining cycle in 1 minute...");
                        std::thread::sleep(std::time::Duration::from_secs(60));
                    }
                }
            }
        }
        let stats_result = api::fetch_statistics(&context.client, &context.api_url, &mining_address);
        print_statistics(stats_result, final_hashes, final_elapsed);
    }
}


/// MODE B: Mnemonic Sequential Mining
pub fn run_mnemonic_sequential_mining(cli: &Cli, context: MiningContext, mnemonic_phrase: String) -> Result<(), String> {
    let reg_message = context.tc_response.message.clone();
    let mut wallet_deriv_index: u32 = 0;
    let mut first_run = true;
    let mut max_registered_index = None;
    let mut backoff_challenge = crate::backoff::Backoff::new(5, 300, 2.0);
    let mut backoff_reg = crate::backoff::Backoff::new(5, 300, 2.0);
    let mut last_seen_challenge_id = String::new();
    let mut current_challenge_id = String::new();
    let mut last_active_challenge_data: Option<ChallengeData> = None;

    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: MNEMONIC SEQUENTIAL MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    loop {
        // --- 1. Challenge Discovery and Initial Index Reset ---
        backoff_challenge.reset();
        let old_challenge_id = last_seen_challenge_id.clone();
        current_challenge_id.clear();

        let challenge_params: ChallengeData = match utils::get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(params)) => {
                backoff_challenge.reset();
                last_active_challenge_data = Some(params.clone());
                if first_run || (context.cli_challenge.is_none() && params.challenge_id != old_challenge_id) {
                    // Create a dummy DataDir with index 0 to calculate the base path for scanning
                    let temp_data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: 0 });

                    let next_index_from_receipts = next_wallet_deriv_index_for_challenge(&cli.data_dir, &params.challenge_id, &temp_data_dir)?;

                    // FIX: Take the maximum of the index derived from receipts and the CLI starting index.
                    wallet_deriv_index = next_index_from_receipts.max(cli.mnemonic_starting_index);
                }
                last_seen_challenge_id = params.challenge_id.clone();
                params
            },
            Ok(None) => { backoff_challenge.reset(); continue; },
            Err(e) => {
                // If a challenge ID is set AND we detect a network failure, continue mining.
                if !current_challenge_id.is_empty() && e.contains("API request failed") {
                    eprintln!("⚠️ Challenge API poll failed (Network Error): {}. Continuing mining with previous challenge parameters (ID: {})...", e, current_challenge_id);
                    backoff_challenge.reset();
                    last_active_challenge_data.as_ref().cloned().ok_or_else(|| {
                        format!("FATAL LOGIC ERROR: Challenge ID {} is set but no previous challenge data was stored.", current_challenge_id)
                    })?
                } else {
                    eprintln!("⚠️ Critical API Error during challenge polling: {}. Retrying with exponential backoff...", e);
                    backoff_challenge.sleep();
                    continue;
                }
            }
        };
        first_run = false;

        // Save challenge details
        let temp_data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: 0 });
        if let Some(base_dir) = context.data_dir { temp_data_dir.save_challenge(base_dir, &challenge_params)?; }

        // --- 2. Continuous Index Skip Check ---
        // This loop ensures we skip indices with existing receipts, even if the index hasn't changed.
        'skip_check: loop {
            let wallet_config = DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: wallet_deriv_index };
            let data_dir = DataDir::Mnemonic(wallet_config); // Full DataDir for recovery check

            // Get the temporary mining address for this index (needed for queue file lookup/recovery)
            let mining_address_temp = cardano::derive_key_pair_from_mnemonic(&mnemonic_phrase, cli.mnemonic_account, wallet_deriv_index).2.to_bech32().unwrap();

            // Check for unsubmitted solutions (recovery file or pending queue)
            if let Some(base_dir) = context.data_dir {
                if wallet_deriv_index >= cli.mnemonic_starting_index {
                    // 1. Check for crash recovery file (found.json)
                    check_for_unsubmitted_solutions(base_dir, &challenge_params.challenge_id, &mining_address_temp, &data_dir)?;

                    // 2. Check if a solution for this address/challenge is already in the pending queue
                    if is_solution_pending_in_queue(base_dir, &mining_address_temp, &challenge_params.challenge_id)? {
                        println!("\nℹ️ Index {} has a pending submission in the queue. Skipping and checking next index.", wallet_deriv_index);
                        wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                        continue 'skip_check;
                    }
                }
            }

            // --- Final Receipt Check (Multi-Path Resumption) ---
            if let Some(base_dir) = context.data_dir {
                // 1. Check Correct Mnemonic Path (where it should be)
                if receipt_exists_for_index(base_dir, &challenge_params.challenge_id, &wallet_config)? {
                    println!("\nℹ️ Index {} already has a local receipt (Mnemonic path). Skipping.", wallet_deriv_index);
                    wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                    continue 'skip_check;
                }

                // 2. Check INCORRECT Persistent Path (where submitter currently writes receipts due to heuristic)
                let mut persistent_path = data_dir.challenge_dir(base_dir, &challenge_params.challenge_id)?;
                persistent_path.push("persistent");
                persistent_path.push(&mining_address_temp); // The address derived for this index
                persistent_path.push(FILE_NAME_RECEIPT);

                if persistent_path.exists() {
                    println!("\n⚠️ Index {} found receipt in Persistent path (Submitter heuristic failure). Skipping.", wallet_deriv_index);
                    wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                    continue 'skip_check;
                }
            }

            // If none of the above conditions met, we break and mine.
            break 'skip_check;
        }

        // --- 3. Key Generation, Registration, and Mining ---
        let key_pair = cardano::derive_key_pair_from_mnemonic(&mnemonic_phrase, cli.mnemonic_account, wallet_deriv_index);
        let mining_address = key_pair.2.to_bech32().unwrap();

        println!("\n[CYCLE START] Deriving Address Index {}: {}", wallet_deriv_index, mining_address);
        if match max_registered_index { Some(idx) => wallet_deriv_index > idx, None => true } {
            let stats_result = api::fetch_statistics(&context.client, &context.api_url, &mining_address);
            match stats_result {
                Ok(stats) => { println!("  Crypto Receipts (Solutions): {}", stats.crypto_receipts); println!("  Night Allocation: {}", stats.night_allocation); }
                Err(_) => {
                    let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);
                    if let Err(e) = api::register_address(&context.client, &context.api_url, &mining_address, &reg_message, &reg_signature.0, &hex::encode(key_pair.1.as_ref())) {
                        eprintln!("Registration failed: {}. Retrying with exponential backoff...", e); backoff_reg.sleep(); continue;
                    }
                }
            }
            max_registered_index = Some(wallet_deriv_index); backoff_reg.reset();
        }

        print_mining_setup(&context.api_url, Some(mining_address.as_str()), context.threads, &challenge_params);

        // UPDATED CALL: Removed client and api_url
        let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
            mining_address.clone(), context.threads, context.donate_to_option, &challenge_params, context.data_dir,
        );

        // --- 4. Post-Mining Index Advancement ---
        match result {
            MiningResult::FoundAndQueued => {
                if let Some(ref destination_address) = context.donate_to_option {
                    // key_pair is available locally in this loop scope
                    let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
                    let donation_signature = cardano::cip8_sign(&key_pair, &donation_message);

                    // Attempt donation synchronously. Ignore result here to keep the main flow clean.
                    match api::donate_to(
                        &context.client, &context.api_url, &mining_address, destination_address, &donation_signature.0,
                    ) {
                        Ok(id) => println!("🚀 Donation initiated successfully. ID: {}", id),
                        Err(e) => eprintln!("⚠️ Donation failed (synchronous attempt): {}", e),
                    }
                }

                wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                println!("\n✅ Solution queued. Incrementing index to {}.", wallet_deriv_index);
            },
            MiningResult::AlreadySolved => {
                // This scenario means the submitter/API reported it was already solved
                wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                println!("\n✅ Challenge already solved. Incrementing index to {}.", wallet_deriv_index);
            }
            MiningResult::MiningFailed => {
                eprintln!("\n⚠️ Mining cycle failed. Retrying with the SAME index {}.", wallet_deriv_index);
            }
        }
        let stats_result = api::fetch_statistics(&context.client, &context.api_url, &mining_address);
        print_statistics(stats_result, total_hashes, elapsed_secs);
    }
}

/// MODE C: Ephemeral Key Per Cycle Mining
#[allow(unused_assignments)] // Suppress warnings for final_hashes/final_elapsed assignments
pub fn run_ephemeral_key_mining(context: MiningContext) -> Result<(), String> {
    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: EPHEMERAL KEY MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    let mut final_hashes: u64 = 0;
    let mut final_elapsed: f64 = 0.0;
    let mut current_challenge_id = String::new();
    let mut last_active_challenge_data: Option<ChallengeData> = None;

    loop {
        let challenge_params: ChallengeData = match utils::get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(p)) => {
                last_active_challenge_data = Some(p.clone());
                p
            },
            Ok(None) => continue,
            Err(e) => {
                // If a challenge ID is set AND we detect a network failure, continue mining.
                if !current_challenge_id.is_empty() && e.contains("API request failed") {
                    eprintln!("⚠️ Challenge API poll failed (Network Error): {}. Continuing mining with previous challenge parameters (ID: {})...", e, current_challenge_id);
                    last_active_challenge_data.as_ref().cloned().ok_or_else(|| {
                        format!("FATAL LOGIC ERROR: Challenge ID {} is set but no previous challenge data was stored.", current_challenge_id)
                    })?
                } else {
                    eprintln!("⚠️ Could not fetch active challenge (Ephemeral Key Mode): {}. Retrying in 5 minutes...", e);
                    std::thread::sleep(std::time::Duration::from_secs(5 * 60));
                    continue;
                }
            }
        };

        let key_pair = cardano::generate_cardano_key_and_address();
        let generated_mining_address = key_pair.2.to_bech32().unwrap();
        let data_dir = DataDir::Ephemeral(&generated_mining_address);

        if let Some(base_dir) = context.data_dir { data_dir.save_challenge(base_dir, &challenge_params)?; }
        println!("\n[CYCLE START] Generated Address: {}", generated_mining_address);

        let reg_message = context.tc_response.message.clone();
        let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);

        if let Err(e) = api::register_address(&context.client, &context.api_url, &generated_mining_address, &context.tc_response.message, &reg_signature.0, &hex::encode(key_pair.1.as_ref())) {
            eprintln!("Registration failed: {}. Retrying in 5 minutes...", e); std::thread::sleep(std::time::Duration::from_secs(5 * 60)); continue;
        }

        print_mining_setup(&context.api_url, Some(&generated_mining_address.to_string()), context.threads, &challenge_params);

        // UPDATED CALL: Removed client and api_url
        let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                generated_mining_address.to_string(), context.threads, context.donate_to_option, &challenge_params, context.data_dir,
            );
        final_hashes = total_hashes; final_elapsed = elapsed_secs;

        match result {
            MiningResult::FoundAndQueued => {
                if let Some(ref destination_address) = context.donate_to_option {
                    // key_pair is available locally in this loop scope
                    let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
                    let donation_signature = cardano::cip8_sign(&key_pair, &donation_message);

                    // Attempt donation synchronously. Ignore result here to keep the main thread fast.
                    match api::donate_to(
                        &context.client, &context.api_url, &generated_mining_address, destination_address, &donation_signature.0,
                    ) {
                        Ok(id) => println!("🚀 Donation initiated successfully. ID: {}", id),
                        Err(e) => eprintln!("⚠️ Donation failed (synchronous attempt): {}", e),
                    }
                }
                eprintln!("Solution queued. Starting next cycle immediately...");
            }
            MiningResult::AlreadySolved => { eprintln!("Solution was already accepted by the network. Starting next cycle immediately..."); }
            MiningResult::MiningFailed => { eprintln!("Mining cycle failed. Retrying next cycle in 1 minute..."); std::thread::sleep(std::time::Duration::from_secs(60)); }
        }

        let stats_result = api::fetch_statistics(&context.client, &context.api_url, &generated_mining_address);
        print_statistics(stats_result, final_hashes, final_elapsed);
        println!("\n[CYCLE END] Starting next mining cycle immediately...");
    }
}
