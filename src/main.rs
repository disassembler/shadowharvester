use shadow_harvester_lib::scavenge;
use clap::Parser;
use reqwest;

// Declare the new API module
mod api;
// Declare the CLI module
mod cli;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    // KeyGen handling (temporarily disabled)
    match cli.command {
        Some(Commands::KeyGen) => {
             eprintln!("ERROR: The 'key-gen' command is temporarily disabled.");
             return;
        }
        None => {} // Continue to mining setup
    }


    // 1. Check for --api-url
    let api_url: String = match cli.api_url {
        Some(url) => url,
        None => {
            eprintln!("ERROR: The '--api-url' flag must be specified to connect to the Scavenger Mine API.");
            eprintln!("Example: ./shadow-harvester --api-url https://scavenger.gd.midnighttge.io");
            return;
        }
    };

    // --- API FLOW STARTS HERE ---

    // 2. Fetch T&C message (always required for registration payload)
    let tc_response = match api::fetch_tandc(&api_url) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("FATAL ERROR: Could not fetch T&C from API URL: {}. Details: {}", api_url, e);
            return;
        }
    };

    // 3. Conditional T&C display and acceptance check
    if !cli.accept_tos {
        // Display T&C only if the flag is missing
        println!("\n--- Token End-User Terms (Version {}) ---", tc_response.version);
        println!("{}", tc_response.content);
        println!("--------------------------------------------------");
        println!("Agreement Message:\n'{}'", tc_response.message);
        println!("--------------------------------------------------");

        println!("\nERROR: You must pass the '--accept-tos' flag to proceed with mining.");
        return;
    }

    // 4. Register the address (MOCK) - TEMPORARILY SKIPPED
    println!("⏩ Skipping address registration for now.");
    /*
    if let Err(e) = api::register_address_mock(&api_url, &cli.address, &tc_response.message) {
        eprintln!("FATAL ERROR: Address registration failed. Details: {}", e);
        return;
    }
    */

    // 5. Fetch Challenge Parameters
    let challenge_params = match api::fetch_challenge(&api_url) {
        Ok(params) => params,
        Err(e) => {
            // Print the custom error message and gracefully exit
            eprintln!("ERROR: {}", e);
            return;
        }
    };

    // --- MINING SETUP ---

    println!("\n==============================================");
    println!("⛏️  Shadow Harvester: Mining Started");
    println!("==============================================");
    println!("API URL: {}", api_url);
    println!("Mining Address: {}", cli.address);
    println!("Worker Threads: {}", cli.threads);
    println!("----------------------------------------------");
    println!("CHALLENGE DETAILS:");
    println!("  ID:               {}", challenge_params.challenge_id);
    println!("  Difficulty Mask:  {}", challenge_params.difficulty);
    println!("  Submission Deadline: {}", challenge_params.latest_submission);
    println!("  ROM Key (no_pre_mine): {}", challenge_params.no_pre_mine_key);
    println!("  Hash Input Hour:  {}", challenge_params.no_pre_mine_hour_str);
    println!("----------------------------------------------");

    // 6. Start Scavenging (Using dynamic challenge data)

    let found_nonce = scavenge(
        cli.address.clone(),
        challenge_params.challenge_id.clone(),
        challenge_params.difficulty.clone(),
        challenge_params.no_pre_mine_key.clone(),
        challenge_params.latest_submission.clone(),
        challenge_params.no_pre_mine_hour_str.clone(),
        cli.threads,
    );

    // 7. Submit solution if found
    if let Some(nonce) = found_nonce {
        println!("\n✅ Solution found: {}. Submitting...", nonce);
        if let Err(e) = api::submit_solution_mock(
            &api_url,
            &cli.address,
            &challenge_params.challenge_id,
            &nonce,
        ) {
            eprintln!("FATAL ERROR: Solution submission failed. Details: {}", e);
            return;
        }
        println!("🚀 Submission successful!");
    } else {
        println!("\n⚠️ Scavenging finished, but no solution was found.");
    }
}
