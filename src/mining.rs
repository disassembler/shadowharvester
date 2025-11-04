// src/mining.rs

use crate::api::{self, ChallengeData, parse_cli_challenge_string, Statistics};
use crate::backoff::Backoff;
use crate::cardano;
use crate::cli::{Cli, Commands};
use crate::constants::USER_AGENT;
use crate::data_types::{DataDir, DataDirMnemonic, MiningResult, FILE_NAME_RECEIPT};
use reqwest::blocking::{self, Client};
use std::ffi::OsStr;
use std::thread;
use std::time::Duration;
use chrono::{DateTime, Utc};
use std::process;
use serde_json;


// NEW STRUCT: Holds the common, validated state for the mining loops.
pub struct MiningContext<'a> {
    pub client: blocking::Client,
    pub api_url: String,
    pub tc_response: api::TandCResponse,
    pub donate_to_option: Option<&'a String>,
    pub threads: u32,
    pub cli_challenge: Option<&'a String>,
    pub data_dir: Option<&'a str>,
}

// ===============================================
// HELPER FUNCTIONS
// ===============================================

pub fn format_duration(seconds: f64) -> String {
    let s = seconds.floor() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let s = s % 60;
    format!("{}:{}:{}", h, m, s)
}

pub fn create_api_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
}

/// Polls the API for the current challenge status and handles challenge change logic.
pub fn poll_for_active_challenge(
    client: &blocking::Client,
    api_url: &str,
    current_id: &mut String,
) -> Result<Option<ChallengeData>, String> {

    let challenge_response = api::fetch_challenge_status(&client, api_url)?;

    match challenge_response.code.as_str() {
        "active" => {
            let active_params = challenge_response.challenge.unwrap();

            // Check if the ID has changed OR if we are coming from a non-active state (empty ID)
            if active_params.challenge_id != *current_id {

                if current_id.is_empty() {
                    // First time seeing this active challenge (after reset/start)
                    println!("\n✅ Active challenge found (ID: {}). Starting cycle.", active_params.challenge_id);
                } else {
                    // Actual new challenge detected
                    println!("\n🎉 New active challenge detected (ID: {}). Starting new cycle.", active_params.challenge_id);
                }

                *current_id = active_params.challenge_id.clone();
                Ok(Some(active_params))
            } else {
                // Same challenge, remains active/solved
                println!("\nℹ️ Challenge ID ({}) remains active/solved. Waiting 5 minutes for a new challenge...", active_params.challenge_id);
                thread::sleep(Duration::from_secs(5 * 60));
                Ok(None)
            }
        }
        "before" => {
            let start_time = challenge_response.starts_at.unwrap_or_default();
            println!("\n⏳ MINING IS NOT YET ACTIVE. Starts at: {}. Waiting 5 minutes...", start_time);
            *current_id = "".to_string();
            thread::sleep(Duration::from_secs(5 * 60));
            Ok(None)
        }
        "after" => {
            println!("\n🛑 MINING PERIOD HAS ENDED. Waiting 5 minutes for the next challenge...");
            *current_id = "".to_string();
            thread::sleep(Duration::from_secs(5 * 60));
            Ok(None)
        }
        _ => Err(format!("Received unexpected challenge code: {}", challenge_response.code)),
    }
}

pub fn get_challenge_params(
    client: &blocking::Client,
    api_url: &str,
    cli_challenge: Option<&String>,
    current_id: &mut String,
) -> Result<Option<ChallengeData>, String> {
    if let Some(challenge_str) = cli_challenge {
        let cli_challenge_data = parse_cli_challenge_string(challenge_str)
            .map_err(|e| format!("Challenge parameter parsing error: {}", e))?;
        let live_params = api::get_active_challenge_data(&client, api_url)
            .map_err(|e| format!("Could not fetch live challenge status (required for submission deadline/hour): {}", e))?;

        let mut fixed_challenge_params = live_params.clone();
        fixed_challenge_params.challenge_id = cli_challenge_data.challenge_id.clone();
        fixed_challenge_params.no_pre_mine_key = cli_challenge_data.no_pre_mine_key.clone();
        fixed_challenge_params.difficulty = cli_challenge_data.difficulty.clone();
        fixed_challenge_params.no_pre_mine_hour_str = cli_challenge_data.no_pre_mine_hour_str.clone();
        fixed_challenge_params.latest_submission = cli_challenge_data.latest_submission.clone();
        let current_time: DateTime<Utc> = Utc::now();
        let latest_submission_time = match DateTime::parse_from_rfc3339(&fixed_challenge_params.latest_submission) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                eprintln!("Error parsing target time: {}", e);
                process::exit(1);
            }
        };

        if fixed_challenge_params.challenge_id != *current_id {
            println!("\n⚠️ Fixed challenge specified: Using ID {} with Difficulty {}. Live polling disabled.",
                fixed_challenge_params.challenge_id, fixed_challenge_params.difficulty);
            *current_id = fixed_challenge_params.challenge_id.clone();
        }
        else if latest_submission_time < current_time {
            eprintln!("Challenge Submission expired! Exiting!");
            process::exit(1);
        }
        else {
             println!("\n⚠️ Fixed challenge ID ({}) is being re-mined.", fixed_challenge_params.challenge_id);
        }
        Ok(Some(fixed_challenge_params))
    } else {
        poll_for_active_challenge(client, api_url, current_id)
    }
}


pub fn print_statistics(stats_result: Result<Statistics, String>, total_hashes: u64, elapsed_secs: f64) {
    println!("\n==============================================");
    println!("📈 Mining Statistics Summary");
    println!("==============================================");
    let hash_rate = if elapsed_secs > 0.0 { total_hashes as f64 / elapsed_secs } else { 0.0 };
    println!("** LAST MINING CYCLE PERFORMANCE **");
    println!("  Time Elapsed: {}", format_duration(elapsed_secs));
    println!("  Total Hashes: {}", total_hashes);
    println!("  Hash Rate: {:.2} H/s", hash_rate);
    println!("----------------------------------------------");
    match stats_result {
        Ok(stats) => {
            println!("** YOUR ACCOUNT STATISTICS (Address: {}) **", stats.local_address);
            println!("  Crypto Receipts (Solutions): {}", stats.crypto_receipts);
            println!("  Night Allocation: {}", stats.night_allocation);
            println!("----------------------------------------------");
            println!("** GLOBAL STATISTICS (All Miners) **");
            println!("  NOTE: These statistics are aggregated across all wallets globally.");
            println!("  Total Wallets: {}", stats.wallets);
            println!("  Current Challenges: {}", stats.challenges);
            println!("  Total Challenges Ever: {}", stats.total_challenges);
            println!("  Total Crypto Receipts: {}", stats.total_crypto_receipts);
            println!("  Recent Crypto Receipts: {}", stats.recent_crypto_receipts);
            println!("==============================================");
        }
        Err(e) => {
            println!("** FAILED TO FETCH API STATISTICS **");
            eprintln!("  Error: {}", e);
            println!("==============================================");
        }
    }
}

pub fn run_single_mining_cycle(
    client: &blocking::Client,
    api_url: &str,
    mining_address: String,
    threads: u32,
    donate_to_option: Option<&String>,
    challenge_params: &ChallengeData,
    keypair: &cardano::KeyPairAndAddress,
) -> (MiningResult, u64, f64) {
    let (found_nonce, total_hashes, elapsed_secs) = shadow_harvester_lib::scavenge(
        mining_address.clone(),
        challenge_params.challenge_id.clone(),
        challenge_params.difficulty.clone(),
        challenge_params.no_pre_mine_key.clone(),
        challenge_params.latest_submission.clone(),
        challenge_params.no_pre_mine_hour_str.clone(),
        threads,
    );

    let mining_result = match found_nonce {
        None => {
            println!("\n⚠️ Scavenging finished, but no solution was found.");
            MiningResult::MiningFailed
        },
        Some(nonce) => {
            println!("\n✅ Solution found: {}. Submitting...", nonce);
            match api::submit_solution(
                &client, api_url, &mining_address, &challenge_params.challenge_id, &nonce,
            ) {
                Err(e) => {
                    eprintln!("⚠️ Solution submission failed. Details: {}", e);
                    if e.contains("Solution already exists") {
                        MiningResult::AlreadySolved
                    } else {
                        MiningResult::MiningFailed
                    }
                },
                Ok(receipt) => {
                    println!("🚀 Submission successful!");
                    let donation = donate_to_option.and_then(|ref destination_address| {
                        let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
                        let donation_signature = cardano::cip8_sign(keypair, &donation_message);
                        api::donate_to(
                            &client, api_url, &mining_address, destination_address, &donation_signature.0,
                        ).map_or_else(|e| {
                            eprintln!("⚠️ Donation failed. Details: {}", e);
                            None
                        }, Some)
                    });
                    MiningResult::FoundAndSubmitted((receipt, donation))
                }
            }
        },
    };
    (mining_result, total_hashes, elapsed_secs)
}

pub fn print_mining_setup(
    api_url: &str,
    address: Option<&str>,
    threads: u32,
    challenge_params: &ChallengeData,
) {
    let address_display = address.unwrap_or("[Not Set / Continuous Generation]");
    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: Mining Cycle Setup");
    println!("==============================================");
    println!("API URL: {}", api_url);
    println!("Mining Address: {}", address_display);
    println!("Worker Threads: {}", threads);
    println!("----------------------------------------------");
    println!("CHALLENGE DETAILS:");
    println!("  ID:               {}", challenge_params.challenge_id);
    println!("  Difficulty Mask:  {}", challenge_params.difficulty);
    println!("  Submission Deadline: {}", challenge_params.latest_submission);
    println!("  ROM Key (no_pre_mine): {}", challenge_params.no_pre_mine_key);
    println!("  Hash Input Hour:  {}", challenge_params.no_pre_mine_hour_str);
    println!("----------------------------------------------");
}

// New function to check if a specific index already has a receipt
pub fn receipt_exists_for_index(base_dir: &str, challenge_id: &str, wallet_config: &DataDirMnemonic) -> Result<bool, String> {
    let data_dir = DataDir::Mnemonic(*wallet_config);
    let mut path = data_dir.receipt_dir(base_dir, challenge_id)?;
    path.push(FILE_NAME_RECEIPT);
    Ok(path.exists())
}

pub fn next_wallet_deriv_index_for_challenge(
    base_dir: &Option<String>,
    challenge_id: &str,
    data_dir_for_path: &DataDir
) -> Result<u32, String> {
    const START_INDEX: u32 = 0;
    Ok(if let Some(data_base_dir) = base_dir {
        let temp_data_dir_mnemonic = match data_dir_for_path {
            DataDir::Mnemonic(wallet) => DataDir::Mnemonic(DataDirMnemonic {
                mnemonic: wallet.mnemonic,
                account: wallet.account,
                deriv_index: 0,
            }),
            _ => return Err("next_wallet_deriv_index_for_challenge called with non-Mnemonic DataDir".to_string()),
        };

        let mut account_dir = temp_data_dir_mnemonic.receipt_dir(data_base_dir, challenge_id)?;
        account_dir.pop();

        let mut parsed_indices: Vec<u32> = std::fs::read_dir(&account_dir)
            .map_err(|e| format!("Could not read the mnemonic's account dir: {}", e))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.file_stem().and_then(OsStr::to_str).and_then(|s| s.parse::<u32>().ok()).is_some())
            .filter(|path| {
                let mut receipt_path = path.clone();
                receipt_path.push(FILE_NAME_RECEIPT);
                match std::fs::exists(&receipt_path) {
                    Err(e) => {
                        eprintln!("Could not check for receipt at {:?}: {}", path, e);
                        true
                    },
                    Ok(exists) => exists,
                }
            })
            .filter_map(|path| path.file_stem().and_then(OsStr::to_str).and_then(|s| s.parse::<u32>().ok()))
            .collect();
        parsed_indices.sort();

        if parsed_indices.is_empty() {
            eprintln!("no highest index: using {}", START_INDEX);
            START_INDEX
        } else {
            let mut expected_index = START_INDEX;
            for &index in parsed_indices.iter() {
                if index > expected_index {
                    // Gap found: an index is missing a receipt. Return the missing index.

                    let highest_continuous_index_display = if expected_index > 0 {
                        expected_index.wrapping_sub(1).to_string()
                    } else {
                        "N/A".to_string()
                    };

                    eprintln!("Gap found in receipts. Highest continuous index is {}. Retrying missing index {}.", highest_continuous_index_display, expected_index);
                    return Ok(expected_index);
                }
                expected_index = index.wrapping_add(1);
            }
            expected_index
        }
    } else {
        START_INDEX
    })
}

// ===============================================
// CORE DISPATCHER AND SETUP FUNCTION
// ===============================================

/// Handles the initial setup, argument validation, T&C, and pre-mining command dispatch.
/// Returns the necessary context for the main mining loop functions.
pub fn setup_app<'a>(cli: &'a Cli) -> Result<MiningContext<'a>, String> {
    // 1. Check for --api-url
    let api_url: String = match cli.api_url.clone() {
        Some(url) => url,
        None => {
            return Err("The '--api-url' flag must be specified to connect to the Scavenger Mine API.".to_string());
        }
    };

    // 2. Check for argument conflicts
    if cli.mnemonic.is_some() && cli.mnemonic_file.is_some() {
        return Err("Cannot use both '--mnemonic' and '--mnemonic-file' flags simultaneously.".to_string());
    }
    if cli.payment_key.is_some() && (cli.mnemonic.is_some() || cli.mnemonic_file.is_some()) {
        return Err("Cannot use both '--payment-key' and '--mnemonic' or '--mnemonic-file' flags simultaneously.".to_string());
    }

    let client = create_api_client()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // --- COMMAND HANDLERS ---
    if let Some(Commands::Challenges) = cli.command {
        let challenge_response = api::fetch_challenge_status(&client, &api_url)
            .map_err(|e| format!("Could not fetch challenge status: {}", e))?;
        println!("Challenge status fetched: {:?}", challenge_response);
        // We use a specific error string to signal successful execution and exit in run_app
        return Err("COMMAND EXECUTED".to_string());
    }

    // 3. Fetch T&C message (always required for registration payload)
    let tc_response = match api::fetch_tandc(&client, &api_url) {
        Ok(t) => t,
        Err(e) => return Err(format!("Could not fetch T&C from API URL: {}. Details: {}", api_url, e)),
    };

    // 4. Conditional T&C display and acceptance check
    if !cli.accept_tos {
        println!("Terms and Conditions (Version {}):", tc_response.version);
        println!("{}", tc_response.content);
        return Err("You must pass the '--accept-tos' flag to proceed with mining.".to_string());
    }

    Ok(MiningContext {
        client,
        api_url,
        tc_response,
        donate_to_option: cli.donate_to.as_ref(),
        threads: cli.threads,
        cli_challenge: cli.challenge.as_ref(),
        data_dir: cli.data_dir.as_deref(),
    })
}


// ===============================================
// MINING MODE FUNCTIONS
// ===============================================

/// MODE A: Persistent Key Continuous Mining
pub fn run_persistent_key_mining(cli: &Cli, context: MiningContext, skey_hex: &String) -> Result<(), String> {
    let key_pair = cardano::generate_cardano_key_pair_from_skey(skey_hex);
    let mining_address = key_pair.2.to_bech32().unwrap();
    let mut final_hashes: u64 = 0;
    let mut final_elapsed: f64 = 0.0;
    let reg_message = context.tc_response.message.clone();
    let data_dir = DataDir::Persistent(&mining_address);

    println!("\n[REGISTRATION] Attempting initial registration for address: {}", mining_address);
    let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);
    if let Err(e) = api::register_address(
        &context.client, &context.api_url, &mining_address, &context.tc_response.message, &reg_signature.0, &hex::encode(&key_pair.1.as_ref()),
    ) {
        eprintln!("Address registration failed: {}. Cannot start mining.", e);
        return Err("Address registration failed.".to_string());
    }

    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: PERSISTENT KEY MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    let mut current_challenge_id = String::new();
    loop {
        let challenge_params = match get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(params)) => params,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("⚠️ Critical API Error during challenge check: {}. Retrying in 1 minute...", e);
                thread::sleep(Duration::from_secs(60));
                continue;
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
                                thread::sleep(Duration::from_secs(60));
                            },
                            Ok(_) | Err(_) => {
                                eprintln!("Challenge appears to have changed or API is unreachable. Stopping current mining and checking for new challenge...");
                                break;
                            }
                        }
                    } else {
                        eprintln!("Fixed challenge. Retrying mining cycle in 1 minute...");
                        thread::sleep(Duration::from_secs(60));
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
    let mut backoff_challenge = Backoff::new(5, 300, 2.0);
    let mut backoff_reg = Backoff::new(5, 300, 2.0);
    let mut last_seen_challenge_id = String::new();
    let mut current_challenge_id = String::new();

    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: MNEMONIC SEQUENTIAL MINING Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    if context.donate_to_option.is_some() { println!("Donation Target: {}", context.donate_to_option.unwrap()); }

    loop {
        // --- 1. Challenge Discovery and Initial Index Reset ---
        backoff_challenge.reset();
        let old_challenge_id = last_seen_challenge_id.clone();
        current_challenge_id.clear();

        let challenge_params = match get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(params)) => {
                backoff_challenge.reset();
                if first_run || (context.cli_challenge.is_none() && params.challenge_id != old_challenge_id) {
                    // Create a dummy DataDir with index 0 to calculate the base path for scanning
                    let temp_data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: 0 });
                    wallet_deriv_index = next_wallet_deriv_index_for_challenge(&cli.data_dir, &params.challenge_id, &temp_data_dir)?;
                }
                last_seen_challenge_id = params.challenge_id.clone();
                params
            },
            Ok(None) => { backoff_challenge.reset(); continue; },
            Err(e) => { eprintln!("⚠️ Critical API Error during challenge polling: {}. Retrying with exponential backoff...", e); backoff_challenge.sleep(); continue; }
        };
        first_run = false;

        // Save challenge details
        let temp_data_dir = DataDir::Mnemonic(DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: 0 });
        if let Some(base_dir) = context.data_dir { temp_data_dir.save_challenge(base_dir, &challenge_params)?; }

        // --- 2. Continuous Index Skip Check (NEW LOGIC) ---
        // This loop advances the index past any indices that have receipts but were not accounted
        // for in the initial gap check (e.g., if another process solved a future index).
        'skip_check: loop {
            let wallet_config = DataDirMnemonic { mnemonic: &mnemonic_phrase, account: cli.mnemonic_account, deriv_index: wallet_deriv_index };
            if let Some(base_dir) = context.data_dir.as_deref() {
                if receipt_exists_for_index(base_dir, &challenge_params.challenge_id, &wallet_config)? {
                    println!("\nℹ️ Index {} already has a local receipt. Skipping and checking next index.", wallet_deriv_index);
                    wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                    continue 'skip_check;
                }
            }
            // If no base_dir or no receipt, break the skip check loop to proceed to mining
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
                    if let Err(e) = api::register_address(&context.client, &context.api_url, &mining_address, &reg_message, &reg_signature.0, &hex::encode(&key_pair.1.as_ref())) {
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

/// MODE C: New Key Per Cycle Mining
pub fn run_new_key_per_cycle_mining(context: MiningContext) -> Result<(), String> {
    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: CONTINUOUS MINING (New Key Per Cycle) Mode ({})", if context.cli_challenge.is_some() { "FIXED CHALLENGE" } else { "DYNAMIC POLLING" });
    println!("==============================================");
    println!("Donation Target: {}", context.donate_to_option.unwrap());

    let mut final_hashes: u64 = 0;
    let mut final_elapsed: f64 = 0.0;
    let mut current_challenge_id = String::new();

    loop {
        let challenge_params = match get_challenge_params(&context.client, &context.api_url, context.cli_challenge, &mut current_challenge_id) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("⚠️ Could not fetch active challenge (New Key Mode): {}. Retrying in 5 minutes...", e);
                thread::sleep(Duration::from_secs(5 * 60));
                continue;
            }
        };

        let key_pair = cardano::generate_cardano_key_and_address();
        let generated_mining_address = key_pair.2.to_bech32().unwrap();
        let data_dir = DataDir::Ephemeral(&generated_mining_address);

        if let Some(base_dir) = context.data_dir { data_dir.save_challenge(base_dir, &challenge_params)?; }
        println!("\n[CYCLE START] Generated Address: {}", generated_mining_address);

        let reg_message = context.tc_response.message.clone();
        let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);

        if let Err(e) = api::register_address(&context.client, &context.api_url, &generated_mining_address, &context.tc_response.message, &reg_signature.0, &hex::encode(&key_pair.1.as_ref())) {
            eprintln!("Registration failed: {}. Retrying in 5 minutes...", e); thread::sleep(Duration::from_secs(5 * 60)); continue;
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
            MiningResult::MiningFailed => { eprintln!("Mining cycle failed. Retrying next cycle in 1 minute..."); thread::sleep(Duration::from_secs(60)); }
        }

        let stats_result = api::fetch_statistics(&context.client, &context.api_url, &generated_mining_address);
        print_statistics(stats_result, final_hashes, final_elapsed);
        println!("\n[CYCLE END] Starting next mining cycle immediately...");
    }
}
