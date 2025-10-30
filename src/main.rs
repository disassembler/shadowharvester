use shadow_harvester_lib::scavenge;
use clap::Parser;
use reqwest;

// Declare the new API module
mod api;
mod cardano;
mod cli;
use cli::{Cli, Commands};
// Declare the constants module
mod constants;

/// Runs the main mining loop (scavenge) and submission for a single challenge.
/// Returns true if mining should continue (only relevant for continuous mode).
// FIX: Updated signature to take individual, non-moved arguments (threads and donate_to are now explicit).
fn run_single_mining_cycle(
    api_url: &str,
    mining_address: String,
    threads: u32,
    donate_to_option: Option<&String>,
    challenge_params: &api::ChallengeData,
) -> bool {
    let found_nonce = scavenge(
        mining_address.clone(),
        challenge_params.challenge_id.clone(),
        challenge_params.difficulty.clone(),
        challenge_params.no_pre_mine_key.clone(),
        challenge_params.latest_submission.clone(),
        challenge_params.no_pre_mine_hour_str.clone(),
        threads, // Use threads directly
    );

    if let Some(nonce) = found_nonce {
        println!("\n✅ Solution found: {}. Submitting...", nonce);

        // 1. Submit solution
        if let Err(e) = api::submit_solution(
            api_url,
            &mining_address,
            &challenge_params.challenge_id,
            &nonce,
        ) {
            eprintln!("FATAL ERROR: Solution submission failed. Details: {}", e);
            return false;
        }
        println!("🚀 Submission successful!");

        // 2. Handle donation if required
        if let Some(destination_address) = donate_to_option {
            if let Err(e) = api::donate_to(
                api_url,
                &mining_address,
                destination_address,
                constants::DONATE_MESSAGE_SIG, // PASSING MOCK SIG
            ) {
                eprintln!("FATAL ERROR: Donation failed. Details: {}", e);
                return false;
            }
        }
        return true; // Single run successful, continue only if in continuous mode
    } else {
        println!("\n⚠️ Scavenging finished, but no solution was found.");
        return false; // Cannot continue if no solution was found in this cycle
    }
}

/// Runs the main application logic based on CLI flags.
fn run_app(cli: Cli) -> Result<(), String> {
    // 1. Check for --api-url
    // NOTE: This move makes cli partially moved.
    let api_url: String = match cli.api_url {
        Some(url) => url,
        None => {
            return Err("The '--api-url' flag must be specified to connect to the Scavenger Mine API.".to_string());
        }
    };

    // 2. Fetch T&C message (always required for registration payload)
    let tc_response = api::fetch_tandc(&api_url)
        .map_err(|e| format!("Could not fetch T&C from API URL: {}. Details: {}", api_url, e))?;

    // 3. Conditional T&C display and acceptance check
    if !cli.accept_tos {
        // Display T&C only if the flag is missing
        println!("\n--- Token End-User Terms (Version {}) ---", tc_response.version);
        println!("{}", tc_response.content);
        println!("--------------------------------------------------");
        println!("Agreement Message:\n'{}'", tc_response.message);
        return Err("You must pass the '--accept-tos' flag to proceed with mining.".to_string());
    }

    // 4. Fetch Challenge Parameters (Needed for all subsequent operations)
    let challenge_params = api::fetch_challenge(&api_url)
        .map_err(|e| format!("Could not fetch active challenge: {}", e))?;

    // 5. Determine Operation Mode and Start Mining

    // Default mode: display info and exit if no key/destination is provided
    if cli.payment_key.is_none() && cli.donate_to.is_none() {
        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: INFO ONLY Mode");
        println!("==============================================");
        println!("Mining Address: {}", cli.address);
        println!("Worker Threads: {}", cli.threads);
        println!("CHALLENGE ID: {}", challenge_params.challenge_id);
        println!("----------------------------------------------");
        println!("NOTE: No secret key or continuous donation target specified.");
        println!("      Pass '--payment-key <HEX>' for a single run, or");
        println!("      '--donate-to <ADDR>' for continuous mining with new keys.");
        return Ok(());
    }

    // --- Pre-extract necessary fields as they are needed multiple times ---
    let mining_address = cli.address.clone();
    let donate_to_option_ref = cli.donate_to.as_ref(); // Option<&String>
    let threads = cli.threads; // Copy

    // --- MODE A: Single Run (Uses optional --payment-key or default address) ---
    if cli.payment_key.is_some() || cli.donate_to.is_none() {

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: SINGLE RUN Mode");
        println!("==============================================");
        println!("Mining Address: {}", mining_address);
        println!("Worker Threads: {}", threads);
        println!("CHALLENGE ID: {}", challenge_params.challenge_id);
        println!("----------------------------------------------");

        // Note: Registration logic is still mock/skipped here as per previous steps.

        if run_single_mining_cycle(
            &api_url,
            mining_address,
            threads,
            donate_to_option_ref,
            &challenge_params
        ) {
            println!("\nSingle run complete.");
        }

    // --- MODE B: Continuous Mining & Donation (Requires --donate-to) ---
    } else if cli.donate_to.is_some() {
        // Continuous mode: Generate new key, register, mine, donate. Loop indefinitely.

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: CONTINUOUS MINING Mode");
        println!("==============================================");
        println!("Donation Target: {}", cli.donate_to.as_ref().unwrap());
        println!("Worker Threads: {}", threads);
        println!("CHALLENGE ID: {}", challenge_params.challenge_id);
        println!("----------------------------------------------");

        loop {
            // NOTE: In a real implementation, you would call cardano::generate_new_key() here,
            // register the new address, and then mine with it.
            let generated_mining_address = cli.address.clone();

            println!("\n[CYCLE START] Mining with temporary address: {}", generated_mining_address);

            if !run_single_mining_cycle(
                &api_url,
                generated_mining_address,
                threads,
                donate_to_option_ref,
                &challenge_params
            ) {
                // If mining or submission fails, break the continuous loop
                eprintln!("Critical failure in cycle. Terminating continuous mining.");
                break;
            }

            // Wait before starting the next key generation cycle (MOCK: No actual wait implemented)
            println!("\n[CYCLE END] Starting next mining cycle immediately...");
        }
    }

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    if let Some(Commands::KeyGen) = cli.command {
        cardano::generate_cardano_key_and_address();
        return;
    }

    match run_app(cli) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("FATAL ERROR: {}", e);
            std::process::exit(1);
        }
    }
}
