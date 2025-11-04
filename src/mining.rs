// src/mining.rs

use crate::api;
use crate::data_types::{DataDir, DataDirMnemonic, MiningContext, MiningResult, ChallengeData}; // Added ChallengeData
use crate::cli::Cli;
use crate::cardano;
use crate::utils::{self, next_wallet_deriv_index_for_challenge, print_mining_setup, print_statistics, receipt_exists_for_index, run_single_mining_cycle};


// ===============================================
// MINING MODE FUNCTIONS (Core Logic Only)
// ===============================================

/// MODE A: Persistent Key Continuous Mining
pub fn run_persistent_key_mining(context: MiningContext, skey_hex: &String) -> Result<(), String> {
    let key_pair = cardano::generate_cardano_key_pair_from_skey(skey_hex);
    let mining_address = key_pair.2.to_bech32().unwrap();
    let mut final_hashes: u64;
    let mut final_elapsed: f64;
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
    let mut last_active_challenge_data: Option<ChallengeData> = None; // ADDED: Store last valid challenge data
    loop {
        let challenge_params: ChallengeData = match utils::get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(params)) => {
                last_active_challenge_data = Some(params.clone()); // Store on success
                params
            },
            Ok(None) => continue,
            Err(e) => {
                // NEW LOGIC: If a challenge ID is set AND we detect a network failure, continue mining.
                if !current_challenge_id.is_empty() && e.contains("API request failed") {
                    eprintln!("⚠️ Challenge API poll failed (Network Error): {}. Continuing mining with previous challenge parameters (ID: {})...", e, current_challenge_id);
                    // Use the last stored parameters, which must exist if current_challenge_id is set.
                    last_active_challenge_data.as_ref().cloned().ok_or_else(|| {
                        format!("FATAL LOGIC ERROR: Challenge ID {} is set but no previous challenge data was stored.", current_challenge_id)
                    })?
                } else {
                    // Otherwise, it's a critical error (non-network failure or no challenge active), so wait and retry poll.
                    eprintln!("⚠️ Critical API Error during challenge check: {}. Retrying in 1 minute...", e);
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    continue;
                }
            }
        };

        if let Some(base_dir) = context.data_dir { data_dir.save_challenge(base_dir, &challenge_params)?; }
        print_mining_setup(&context.api_url, Some(mining_address.as_str()), context.threads, &challenge_params);

        loop {
            let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                &context.client, &context.api_url, mining_address.clone(), context.threads, context.donate_to_option, &challenge_params, &key_pair,
            );
            final_hashes = total_hashes; final_elapsed = elapsed_secs;

            match result {
                MiningResult::FoundAndSubmitted((ref receipt, ref donation)) => {
                    println!("\n✅ Solution submitted. Stopping current mining.");
                    if let Some(base_dir) = context.data_dir { data_dir.save_receipt(base_dir, &challenge_params.challenge_id, receipt, donation)?; }
                    break;
                },
                MiningResult::AlreadySolved => {
                    println!("\n✅ Challenge already solved on network. Writing placeholder receipt to skip on next run.");
                    // Write a placeholder receipt on "AlreadySolved" API response
                    let placeholder_receipt = serde_json::json!({"status": "already_solved_on_network"});
                    if let Some(base_dir) = context.data_dir {
                        data_dir.save_receipt(base_dir, &challenge_params.challenge_id, &placeholder_receipt, &None)?;
                    }
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
    let mut last_active_challenge_data: Option<ChallengeData> = None; // ADDED: Store last valid challenge data

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
                last_active_challenge_data = Some(params.clone()); // Store on success
                if first_run || (context.cli_challenge.is_none() && params.challenge_id != old_challenge_id) {
                    // Create a dummy DataDir with index 0 to calculate the base path for scanning
                    let temp_data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: 0 });
                    wallet_deriv_index = next_wallet_deriv_index_for_challenge(&cli.data_dir, &params.challenge_id, &temp_data_dir)?;
                }
                last_seen_challenge_id = params.challenge_id.clone();
                params
            },
            Ok(None) => { backoff_challenge.reset(); continue; },
            Err(e) => {
                // NEW LOGIC: If a challenge ID is set AND we detect a network failure, continue mining.
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
            if let Some(base_dir) = context.data_dir {
                if receipt_exists_for_index(base_dir, &challenge_params.challenge_id, &wallet_config)? {
                    println!("\nℹ️ Index {} already has a local receipt. Skipping and checking next index.", wallet_deriv_index);
                    wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                    continue 'skip_check;
                }
            }
            break 'skip_check;
        }

        // --- 3. Key Generation, Registration, and Mining ---
        let key_pair = cardano::derive_key_pair_from_mnemonic(&mnemonic_phrase, cli.mnemonic_account, wallet_deriv_index);
        let mining_address = key_pair.2.to_bech32().unwrap();
        let data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: wallet_deriv_index });

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

        let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
            &context.client, &context.api_url, mining_address.clone(), context.threads, context.donate_to_option, &challenge_params, &key_pair,
        );

        // --- 4. Post-Mining Index Advancement ---
        match result {
            MiningResult::FoundAndSubmitted((receipt, donation)) => {
                if let Some(base_dir) = context.data_dir { data_dir.save_receipt(base_dir, &challenge_params.challenge_id, &receipt, &donation)?; }
                wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                println!("\n✅ Solution submitted. Incrementing index to {}.", wallet_deriv_index);
            },
            MiningResult::AlreadySolved => {
                // Write a placeholder receipt on "AlreadySolved" API response
                let placeholder_receipt = serde_json::json!({"status": "already_solved_on_network"});
                if let Some(base_dir) = context.data_dir {
                    data_dir.save_receipt(base_dir, &challenge_params.challenge_id, &placeholder_receipt, &None)?;
                }
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
pub fn run_ephemeral_key_mining(context: MiningContext) -> Result<(), String> {
    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: EPHEMERAL KEY MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    let mut final_hashes: u64;
    let mut final_elapsed: f64;
    let mut current_challenge_id = String::new();
    let mut last_active_challenge_data: Option<ChallengeData> = None; // ADDED: Store last valid challenge data

    loop {
        let challenge_params: ChallengeData = match utils::get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(p)) => {
                last_active_challenge_data = Some(p.clone()); // Store on success
                p
            },
            Ok(None) => continue,
            Err(e) => {
                // NEW LOGIC: If a challenge ID is set AND we detect a network failure, continue mining.
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

        let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                &context.client, &context.api_url, generated_mining_address.to_string(), context.threads, context.donate_to_option, &challenge_params, &key_pair,
            );
        final_hashes = total_hashes; final_elapsed = elapsed_secs;

        match result {
            MiningResult::FoundAndSubmitted((receipt, donation)) => {
                if let Some(base_dir) = context.data_dir { data_dir.save_receipt(base_dir, &challenge_params.challenge_id, &receipt, &donation)?; }
            }
            MiningResult::AlreadySolved => { eprintln!("Solution was already accepted by the network. Starting next cycle immediately..."); }
            MiningResult::MiningFailed => { eprintln!("Mining cycle failed. Retrying next cycle in 1 minute..."); std::thread::sleep(std::time::Duration::from_secs(60)); }
        }

        let stats_result = api::fetch_statistics(&context.client, &context.api_url, &generated_mining_address);
        print_statistics(stats_result, final_hashes, final_elapsed);
        println!("\n[CYCLE END] Starting next mining cycle immediately...");
    }
}
