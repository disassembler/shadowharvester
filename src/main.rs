use shadow_harvester_lib::scavenge;
use clap::Parser;
use reqwest;
use reqwest::blocking;
use std::thread;
use std::time::Duration;
use reqwest::blocking::Client;
use crate::constants::USER_AGENT;
use api::{ChallengeResponse, ChallengeData, Statistics};

// Declare the new API module
mod api;
// Declare the CLI module
mod cli;
use cli::{Cli, Commands};
// Declare the constants module
mod constants;
// Declare the cardano module
mod cardano;
use cardano::KeyPairAndAddress; // Import the tuple type

// NEW: Define a result type for the mining cycle
#[derive(Debug, PartialEq)]
enum MiningResult {
    FoundAndSubmitted,
    AlreadySolved, // The solution was successfully submitted by someone else
    MiningFailed,  // General mining or submission error (e.g., hash not found, transient API error)
}

fn format_duration(seconds: f64) -> String {
    let s = seconds.floor() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let s = s % 60;
    format!("{}:{}:{}", h, m, s)
}

fn create_api_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent(USER_AGENT) // Set the custom User-Agent
        .build()
}

fn poll_for_active_challenge(
    client: &blocking::Client,
    api_url: &str,
    current_id: &mut String, // Mutate the current ID to track changes
) -> Result<Option<api::ChallengeData>, String> {

    // Poll the overall status first
    let challenge_response = api::fetch_challenge_status(&client, api_url)?;

    match challenge_response.code.as_str() {
        "active" => {
            let active_params = challenge_response.challenge.unwrap();

            // Check if challenge ID has changed
            if active_params.challenge_id != *current_id {
                println!("\n🎉 New active challenge detected (ID: {}). Starting new cycle.", active_params.challenge_id);
                // Update the current_id
                *current_id = active_params.challenge_id.clone();
                // Return the new challenge to start mining immediately.
                Ok(Some(active_params))
            } else {
                // Same challenge, still active (meaning we previously solved it or failed to find it).
                // Enforce a 5-minute wait, then return None to re-poll.
                println!("\nℹ️ Challenge ID ({}) remains active/solved. Waiting 5 minutes for a new challenge...", active_params.challenge_id);
                thread::sleep(Duration::from_secs(5 * 60));

                // Return None, telling the outer loop to just restart polling.
                Ok(None)
            }
        }
        "before" => {
            let start_time = challenge_response.starts_at.unwrap_or_default();
            println!("\n⏳ MINING IS NOT YET ACTIVE. Starts at: {}. Waiting 5 minutes...", start_time);
            *current_id = "".to_string(); // Reset ID
            thread::sleep(Duration::from_secs(5 * 60));
            Ok(None)
        }
        "after" => {
            println!("\n🛑 MINING PERIOD HAS ENDED. Waiting 5 minutes for the next challenge...");
            *current_id = "".to_string(); // Reset ID
            thread::sleep(Duration::from_secs(5 * 60));
            Ok(None)
        }
        _ => {
            Err(format!("Received unexpected challenge code: {}", challenge_response.code))
        }
    }
}

fn print_statistics(stats_result: Result<Statistics, String>, total_hashes: u64, elapsed_secs: f64) {
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


/// Runs the main mining loop (scavenge) and submission for a single challenge.
/// Returns a MiningResult indicating success, failure, or if the challenge was already solved.
fn run_single_mining_cycle(
    client: &blocking::Client,
    api_url: &str,
    mining_address: String,
    threads: u32,
    donate_to_option: Option<&String>,
    challenge_params: &api::ChallengeData,
    keypair: &cardano::KeyPairAndAddress,
) -> (MiningResult, u64, f64) {
    let (found_nonce, total_hashes, elapsed_secs) = scavenge(
        mining_address.clone(),
        challenge_params.challenge_id.clone(),
        challenge_params.difficulty.clone(),
        challenge_params.no_pre_mine_key.clone(),
        challenge_params.latest_submission.clone(),
        challenge_params.no_pre_mine_hour_str.clone(),
        threads,
    );

    let mut mining_result = MiningResult::MiningFailed;

    if let Some(nonce) = found_nonce {
        println!("\n✅ Solution found: {}. Submitting...", nonce);

        // 1. Submit solution
        if let Err(e) = api::submit_solution(
            &client,
            api_url,
            &mining_address,
            &challenge_params.challenge_id,
            &nonce,
        ) {
            eprintln!("⚠️ Solution submission failed. Details: {}", e);

            // Check for the specific "already solved" error
            if e.contains("Solution already exists") {
                mining_result = MiningResult::AlreadySolved;
            }

            // Treat other submission errors as general failure
            mining_result = MiningResult::MiningFailed;
        }
        println!("🚀 Submission successful!");

        // 2. Handle donation if required
        if let Some(ref destination_address) = donate_to_option {

            // Generate dynamic signature for donation message
            let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
            let donation_signature = cardano::cip8_sign(keypair, &donation_message);

            if let Err(e) = api::donate_to(
                &client,
                api_url,
                &mining_address,
                destination_address,
                &donation_signature.0,
            ) {
                eprintln!("⚠️ Donation failed. Details: {}", e);
            }
        }
        mining_result = MiningResult::FoundAndSubmitted; // Single run successful
    } else {
        println!("\n⚠️ Scavenging finished, but no solution was found.");
        mining_result = MiningResult::MiningFailed; // Nonce not found is a general failure
    }
    (mining_result, total_hashes, elapsed_secs)
}

/// Prints a detailed summary of the current challenge and mining setup.
fn print_mining_setup(
    api_url: &str,
    address: Option<&str>,
    threads: u32,
    challenge_params: &api::ChallengeData,
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


/// Runs the main application logic based on CLI flags.
fn run_app(cli: Cli) -> Result<(), String> {
    // 1. Check for --api-url
    let api_url: String = match cli.api_url {
        Some(url) => url,
        None => {
            return Err("The '--api-url' flag must be specified to connect to the Scavenger Mine API.".to_string());
        }
    };

    if cli.payment_key.is_some() && cli.mnemonic.is_some() {
        return Err("Cannot use both '--payment-key' and '--mnemonic' flags simultaneously.".to_string());
    }

    let client = create_api_client()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // --- COMMAND HANDLERS ---
    if let Some(Commands::Challenges) = cli.command {
        let challenge_response = api::fetch_challenge_status(&client, &api_url)
            .map_err(|e| format!("Could not fetch challenge status: {}", e))?;
        println!("Challenge status fetched: {:?}", challenge_response);
        return Ok(());
    }

    // 2. Fetch T&C message (always required for registration payload)
    let tc_response = match api::fetch_tandc(&client, &api_url) {
        Ok(t) => t,
        Err(e) => return Err(format!("Could not fetch T&C from API URL: {}. Details: {}", api_url, e)),
    };

    // 3. Conditional T&C display and acceptance check
    if !cli.accept_tos {
        println!("Terms and Conditions (Version {}):", tc_response.version);
        println!("{}", tc_response.content);
        return Err("You must pass the '--accept-tos' flag to proceed with mining.".to_string());
    }

    // --- Pre-extract necessary fields ---
    let donate_to_option_ref = cli.donate_to.as_ref();
    let threads = cli.threads;

    // 4. Default mode: display info and exit
    if cli.payment_key.is_none() && cli.donate_to.is_none() && cli.mnemonic.is_none() { // MODIFIED: Check for mnemonic too
        // Fetch challenge for info display
        match api::get_active_challenge_data(&client, &api_url) {
            Ok(challenge_params) => {
                 print_mining_setup(
                    &api_url,
                    cli.address.as_deref(),
                    threads,
                    &challenge_params
                );
            },
            Err(e) => eprintln!("Could not fetch active challenge for info display: {}", e),
        };
        println!("MODE: INFO ONLY. Provide '--payment-key', '--mnemonic', or '--donate-to' to begin mining.");
        return Ok(())
    }

    // 5. Determine Operation Mode and Start Mining

    // --- MODE A: Persistent Key Continuous Mining (User's request) ---
    if let Some(skey_hex) = cli.payment_key.as_ref() {

        // Key Generation/Loading (Fatal if key is invalid)
        let key_pair = cardano::generate_cardano_key_pair_from_skey(skey_hex);
        let mining_address = key_pair.2.to_bech32().unwrap();
        let mut final_hashes: u64 = 0;
        let mut final_elapsed: f64 = 0.0;
        let reg_message = tc_response.message.clone();

        println!("\n[REGISTRATION] Attempting initial registration for address: {}", mining_address);

        // Initial Registration (Fatal if first registration fails)
        let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);
        if let Err(e) = api::register_address(
            &client,
            &api_url,
            &mining_address,
            &tc_response.message,
            &reg_signature.0,
            &hex::encode(&key_pair.1.as_ref()),
        ) {
            eprintln!("Address registration failed: {}. Cannot start mining.", e);
            return Err("Address registration failed.".to_string());
        }

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: PERSISTENT KEY CONTINUOUS MINING Mode");
        println!("==============================================");
        if cli.donate_to.is_some() {
            println!("Donation Target: {}", cli.donate_to.as_ref().unwrap());
        }

        let mut current_challenge_id = String::new();

        // Outer Polling loop (robustly checks for challenge changes every 5 minutes)
        loop {
            // Poll for a new/active challenge. This function handles the 5-minute wait
            // if the challenge is inactive OR if the same active ID is detected.
            let challenge_params = match poll_for_active_challenge(&client, &api_url, &mut current_challenge_id) {
                Ok(Some(params)) => params,
                Ok(None) => {
                    // Loop continues to poll again after the sleep handled inside the function.
                    continue;
                }
                Err(e) => {
                    eprintln!("⚠️ Critical API Error during challenge polling: {}. Retrying in 5 minutes...", e);
                    thread::sleep(Duration::from_secs(5 * 60));
                    continue;
                }
            };

            // New active challenge found, start mining
            print_mining_setup(
                &api_url,
                Some(mining_address.as_str()),
                threads,
                &challenge_params
            );

            // Inner mining/submission loop for robustness
            loop {
                let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                    &client,
                    &api_url,
                    mining_address.clone(),
                    threads,
                    donate_to_option_ref,
                    &challenge_params,
                    &key_pair,
                );

                final_hashes = total_hashes;
                final_elapsed = elapsed_secs;

                match result {
                    MiningResult::FoundAndSubmitted | MiningResult::AlreadySolved => {
                        println!("\n✅ Solution submitted or challenge already solved. Stopping current mining.");
                        // Break inner loop, outer loop restarts and poll_for_active_challenge will enforce the 5-minute wait.
                        break;
                    }
                    MiningResult::MiningFailed => {
                        // Mining/Submission failed (e.g., hash not found in time, general API error)
                        eprintln!("\n⚠️ Mining cycle failed. Checking if challenge is still valid before retrying...");

                        // Check if the challenge is still active and the same
                        match api::get_active_challenge_data(&client,&api_url) {
                            Ok(active_params) if active_params.challenge_id == current_challenge_id => {
                                eprintln!("Challenge is still valid. Retrying mining cycle in 1 minute...");
                                thread::sleep(Duration::from_secs(60));
                                // The inner loop continues to retry mining the same challenge
                            },
                            Ok(_) | Err(_) => {
                                // Challenge either changed or ended, or we can't connect to API
                                eprintln!("Challenge appears to have changed or API is unreachable. Stopping current mining and polling for new challenge...");
                                break; // Break the inner loop, go back to polling for new challenge.
                            }
                        }
                    }
                }
            } // END of Inner Loop
            let stats_result = api::fetch_statistics(&client, &api_url, &mining_address);
            print_statistics(stats_result, final_hashes, final_elapsed);
        }
    }

    if let Some(mnemonic_phrase) = cli.mnemonic.as_ref() {

        let reg_message = tc_response.message.clone();
        let mut wallet_deriv_index: u32 = 0; // Start at derivation index 0
        let mut current_challenge_id = String::new();
        let mut next_challenge_id = String::new();
        let mut max_registered_index = 0;

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: MNEMONIC SEQUENTIAL MINING Mode");
        println!("==============================================");
        if cli.donate_to.is_some() {
            println!("Donation Target: {}", cli.donate_to.as_ref().unwrap());
        }

        // Outer Polling loop (robustly checks for challenge changes)
        loop {
            next_challenge_id = String::new();
            // Poll for a new/active challenge. This function handles the 5-minute wait
            let challenge_params = match poll_for_active_challenge(&client, &api_url, &mut next_challenge_id) {
                Ok(Some(params)) => params,
                Ok(None) => continue, // Continue polling
                Err(e) => {
                    eprintln!("⚠️ Critical API Error during challenge polling: {}. Retrying in 5 minutes...", e);
                    thread::sleep(Duration::from_secs(5 * 60));
                    continue;
                }
            };
            if next_challenge_id != current_challenge_id {
                current_challenge_id = next_challenge_id;
                wallet_deriv_index = 0;
            }

            // 1. Generate New Key Pair using Mnemonic and Index
            let key_pair = cardano::derive_key_pair_from_mnemonic(mnemonic_phrase, wallet_deriv_index);
            let mining_address = key_pair.2.to_bech32().unwrap();

            // 2. Initial Registration (New address must be registered every cycle)
            println!("\n[CYCLE START] Deriving Address Index {}: {}", wallet_deriv_index, mining_address);

            // Only register if we haven't already in this session for this address
            if wallet_deriv_index > max_registered_index {
                let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);
                if let Err(e) = api::register_address(
                    &client,
                    &api_url,
                    &mining_address,
                    &reg_message,
                    &reg_signature.0,
                    &hex::encode(&key_pair.1.as_ref()),
                ) {
                    eprintln!("Registration failed: {}. Retrying in 5 minutes...", e);
                    thread::sleep(Duration::from_secs(5 * 60));
                    continue; // Skip this cycle and try polling again
                }
                max_registered_index = wallet_deriv_index;
            }


            // 3. Mining and Submission
            print_mining_setup(
                &api_url,
                Some(mining_address.as_str()),
                threads,
                &challenge_params
            );

            // Capture metrics for this specific cycle
            let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                &client,
                &api_url,
                mining_address.clone(),
                threads,
                donate_to_option_ref,
                &challenge_params,
                &key_pair,
            );

            // ⭐ CRITICAL: Increment the index only if mining was successful or already solved.
            match result {
                MiningResult::FoundAndSubmitted | MiningResult::AlreadySolved => {
                    wallet_deriv_index = wallet_deriv_index.wrapping_add(1);
                    println!("\n✅ Cycle complete. Incrementing index to {}.", wallet_deriv_index);
                    // No need for a 5-minute wait here, as poll_for_active_challenge handles it.
                }
                MiningResult::MiningFailed => {
                    eprintln!("\n⚠️ Mining cycle failed. Retrying in 1 minute with the SAME index {}.", wallet_deriv_index);
                    thread::sleep(Duration::from_secs(60));
                }
            }

            // The outer loop restarts, polling for the next challenge (or waiting if current is still active)
        }
    }

    else if cli.donate_to.is_some() {

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: CONTINUOUS MINING (New Key Per Cycle) Mode");
        println!("==============================================");
        println!("Donation Target: {}", cli.donate_to.as_ref().unwrap());

        let mut final_hashes: u64 = 0;
        let mut final_elapsed: f64 = 0.0;

        // Continuous loop for generating a new key, registering, and mining
        loop {
            // Robustly fetch active challenge data
            let challenge_params = match api::get_active_challenge_data(&client, &api_url) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("⚠️ Could not fetch active challenge (New Key Mode): {}. Retrying in 5 minutes...", e);
                    thread::sleep(Duration::from_secs(5 * 60));
                    continue;
                }
            };

            // 1. Generate New Key Pair for this cycle
            let key_pair = cardano::generate_cardano_key_and_address();
            let generated_mining_address = key_pair.2.to_bech32().unwrap();

            // 2. Registration (Dynamic Signature)
            println!("\n[CYCLE START] Generated Address: {}", generated_mining_address);

            let reg_message = tc_response.message.clone();
            let reg_signature = cardano::cip8_sign(&key_pair, &reg_message);

            if let Err(e) = api::register_address(
                &client,
                &api_url,
                &generated_mining_address,
                &tc_response.message,
                &reg_signature.0,
                &hex::encode(&key_pair.1.as_ref()),
            ) {
                eprintln!("Registration failed: {}. Retrying in 5 minutes...", e);
                thread::sleep(Duration::from_secs(5 * 60));
                continue;
            }

            // 3. Mining and Submission
            print_mining_setup(
                &api_url,
                Some(&generated_mining_address.to_string()),
                threads,
                &challenge_params
            );

            // Use the new MiningResult for robustness in this mode too
            let (result, total_hashes, elapsed_secs) = run_single_mining_cycle(
                    &client,
                    &api_url,
                    generated_mining_address.to_string(),
                    threads,
                    donate_to_option_ref,
                    &challenge_params,
                    &key_pair,
                );
            final_hashes = total_hashes;
            final_elapsed = elapsed_secs;

            match result {
                MiningResult::FoundAndSubmitted => {
                    // Success, continue immediately with the next key generation cycle
                }
                MiningResult::AlreadySolved => {
                    eprintln!("Solution was already accepted by the network. Starting next cycle immediately...");
                    // No need to sleep 5 minutes, just start the next cycle immediately
                }
                MiningResult::MiningFailed => {
                    eprintln!("Mining cycle failed. Retrying next cycle in 1 minute...");
                    thread::sleep(Duration::from_secs(60));
                }
            }

            let stats_result = api::fetch_statistics(&client, &api_url, &generated_mining_address);
            print_statistics(stats_result, final_hashes, final_elapsed);
            // In this mode, we just start the next cycle with a new key immediately.
            println!("\n[CYCLE END] Starting next mining cycle immediately...");
        }
    } else {
        // This is unreachable because the `if/else if` chain covers all cases
        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();

    match run_app(cli) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("FATAL ERROR: {}", e);
            std::process::exit(1);
        }
    }
}
