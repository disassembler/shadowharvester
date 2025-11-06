// src/utils.rs

use crate::api;
use crate::constants::USER_AGENT;
use crate::data_types::{
    DataDir, DataDirMnemonic, MiningContext, MiningResult, FILE_NAME_RECEIPT,
    ChallengeData, Statistics, TandCResponse, ChallengeResponse, PendingSolution, FILE_NAME_FOUND_SOLUTION
};
use reqwest::blocking::{self, Client};
use std::ffi::OsStr;
use std::thread;
use std::time::Duration;
use chrono::{DateTime, Utc};
use std::process;

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

/// Helper to print non-active challenge status
fn print_non_active_status(response: &ChallengeResponse) {
    println!("\n==============================================");
    println!("⏰ Challenge Status: {}", response.code.to_uppercase());
    println!("==============================================");

    if let Some(day) = response.current_day {
        println!("Current Mining Day: {} / {}", day, response.max_day.unwrap_or(0));
    } else if let Some(max_day) = response.max_day {
         println!("Total Mining Days: {}", max_day);
    }

    if let Some(ends) = &response.mining_period_ends {
        println!("Mining Period Ends: {}", ends);
    }
    if let Some(total) = response.total_challenges {
        println!("Total Challenges (All Days): {}", total);
    }

    if response.code == "before" {
        if let Some(starts) = &response.starts_at {
            println!("Challenge Starts At: {}", starts);
        }
        if let Some(next_starts) = &response.next_challenge_starts_at {
            println!("Next Challenge Starts At: {}", next_starts);
        }
    }
    println!("----------------------------------------------");
}


/// Polls the API for the current challenge status and handles challenge change logic.
pub fn poll_for_active_challenge(
    client: &blocking::Client,
    api_url: &str,
    current_id: &mut String,
) -> Result<Option<ChallengeData>, String> {

    let challenge_response = api::fetch_challenge_status(client, api_url)?;

    match challenge_response.code.as_str() {
        "active" => {
            let active_params = challenge_response.challenge.unwrap();

            if active_params.challenge_id != *current_id {

                if current_id.is_empty() {
                    println!("\n✅ Active challenge found (ID: {}). Starting cycle.", active_params.challenge_id);
                } else {
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
            print_non_active_status(&challenge_response);
            println!("⏳ MINING IS NOT YET ACTIVE. Waiting 5 minutes...");
            *current_id = "".to_string();
            thread::sleep(Duration::from_secs(5 * 60));
            Ok(None)
        }
        "after" => {
            print_non_active_status(&challenge_response);
            println!("🛑 MINING PERIOD HAS ENDED. Waiting 5 minutes for the next challenge...");
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
        let cli_challenge_data = api::parse_cli_challenge_string(challenge_str)
            .map_err(|e| format!("Challenge parameter parsing error: {}", e))?;
        let live_params = api::get_active_challenge_data(client, api_url)
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
    mining_address: String,
    threads: u32,
    donate_to_option: Option<&String>,
    challenge_params: &ChallengeData,
    data_dir_base: Option<&str>,
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
            println!("\n✅ Solution found: {}. Saving solution to temporary storage...", nonce);

            // SIMPLIFIED PendingSolution
            let pending_solution = PendingSolution {
                address: mining_address.clone(),
                challenge_id: challenge_params.challenge_id.clone(),
                nonce: nonce.clone(),
                donation_address: donate_to_option.cloned(),
                // FIX: Add placeholder values for the new fields (synchronous function cannot capture full context)
                preimage: "Legacy_Preimage_Not_Captured_Sync_Mode".to_string(),
                hash_output: "Legacy_Hash_Not_Captured_Sync_Mode".to_string(),
            };


            // CRITICAL STEP 1: Save to a temporary 'found' file first for crash recovery
            if let Some(base_dir) = data_dir_base {
                let temp_data_dir = DataDir::Ephemeral(&mining_address);
                if let Err(e) = temp_data_dir.save_found_solution(base_dir, &challenge_params.challenge_id, &pending_solution) {
                     eprintln!("FATAL: Solution found but could not save recovery file {}: {}", FILE_NAME_FOUND_SOLUTION, e);
                     return (MiningResult::MiningFailed, total_hashes, elapsed_secs);
                }
            } else {
                // If no data_dir is set, the solution is lost.
                eprintln!("FATAL: Solution found but no data_dir specified. Solution lost.");
                return (MiningResult::MiningFailed, total_hashes, elapsed_secs);
            }

            // CRITICAL STEP 2: Move from temporary file to persistent queue
            if let Some(base_dir) = data_dir_base {
                let temp_data_dir = DataDir::Ephemeral(&mining_address);
                if let Err(e) = temp_data_dir.save_pending_solution(base_dir, &pending_solution) {
                     eprintln!("FATAL: Solution found but could not save to queue: {}", e);
                     // If queue save fails, the recovery file is still there, so we return MiningFailed.
                     return (MiningResult::MiningFailed, total_hashes, elapsed_secs);
                }

                // CRITICAL STEP 3: If save to queue is successful, delete the temporary file
                if let Err(e) = temp_data_dir.delete_found_solution(base_dir, &challenge_params.challenge_id) {
                    eprintln!("WARNING: Failed to delete recovery file {}: {}", FILE_NAME_FOUND_SOLUTION, e);
                }

                println!("🚀 Solution queued successfully. Mining continues.");
            }
            // else case is handled above and returns MiningFailed

            MiningResult::FoundAndQueued
        }
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
    println!("  Day:              {}", challenge_params.day);
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
pub fn setup_app(cli: &crate::cli::Cli) -> Result<MiningContext, String> {
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

    // Ephemeral key conflicts with payment key and mnemonic
    if cli.ephemeral_key {
        if cli.payment_key.is_some() {
             return Err("Cannot use '--ephemeral-key' with '--payment-key' simultaneously.".to_string());
        }
        if cli.mnemonic.is_some() || cli.mnemonic_file.is_some() {
             return Err("Cannot use '--ephemeral-key' with '--mnemonic' or '--mnemonic-file' simultaneously.".to_string());
        }
    } else {
        // Existing check for payment_key vs mnemonic, now only run if not ephemeral mode
        if cli.payment_key.is_some() && (cli.mnemonic.is_some() || cli.mnemonic_file.is_some()) {
            return Err("Cannot use both '--payment-key' and '--mnemonic' or '--mnemonic-file' flags simultaneously.".to_string());
        }
    }

    let client = create_api_client()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // --- COMMAND HANDLERS ---
    if let Some(crate::cli::Commands::Challenges) = cli.command {
        let challenge_response = api::fetch_challenge_status(&client, &api_url)
            .map_err(|e| format!("Could not fetch challenge status: {}", e))?;
        // FIX: Print full detailed status info from the ChallengeResponse object
        print_non_active_status(&challenge_response);
        println!("Challenge status fetched: {:?}", challenge_response);
        // We use a specific error string to signal successful execution and exit in run_app
        return Err("COMMAND EXECUTED".to_string());
    }

    // 3. Fetch T&C message (always required for registration payload)
    let tc_response: TandCResponse = match api::fetch_tandc(&client, &api_url) {
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
        donate_to_option: cli.donate_to.clone(),
        threads: cli.threads,
        cli_challenge: cli.challenge.clone(),
        data_dir: cli.data_dir.clone(),
    })
}
