use shadow_harvester_lib::scavenge;
use clap::Parser;
use reqwest;

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

/// Runs the main mining loop (scavenge) and submission for a single challenge.
/// Returns true if mining should continue (only relevant for continuous mode).
fn run_single_mining_cycle(
    api_url: &str,
    mining_address: String,
    threads: u32,
    donate_to_option: Option<&String>,
    challenge_params: &api::ChallengeData,
    // NEW: Secret key is passed for dynamic donation signing
    pay_sk: &pallas_crypto::key::ed25519::SecretKey,
    pay_vk: &pallas_crypto::key::ed25519::PublicKey,
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
        if let Some(ref destination_address) = donate_to_option {

            // Generate dynamic signature for donation message
            let donation_message = format!("Assign accumulated Scavenger rights to: {}", destination_address);
            let donation_signature = cardano::cip8_sign(&pay_sk, &donation_message);

            if let Err(e) = api::donate_to(
                api_url,
                &mining_address,
                destination_address,
                &donation_signature, // Dynamic Signature
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

    // --- COMMAND HANDLERS ---

    if let Some(Commands::Challenges) = cli.command {
        // ... (Challenge Status logic remains the same) ...
        let challenge_response = api::fetch_challenge_status(&api_url)
            .map_err(|e| format!("Could not fetch challenge status: {}", e))?;
        // ... (Printing logic removed for brevity) ...
        return Ok(());
    }


    // 2. Fetch T&C message (always required for registration payload)
    let tc_response = api::fetch_tandc(&api_url)
        .map_err(|e| format!("Could not fetch T&C from API URL: {}. Details: {}", api_url, e))?;

    // 3. Conditional T&C display and acceptance check
    if !cli.accept_tos {
        // ... (T&C display logic remains the same) ...
        return Err("You must pass the '--accept-tos' flag to proceed with mining.".to_string());
    }

    // 4. Fetch Challenge Parameters (Needed for all subsequent operations)
    let challenge_params = api::get_active_challenge_data(&api_url)
        .map_err(|e| format!("Could not fetch active challenge: {}", e))?;

    // 5. Determine Operation Mode and Start Mining

    // --- Pre-extract necessary fields ---
    let cli_address_option = cli.address.as_ref();
    let donate_to_option_ref = cli.donate_to.as_ref(); // Option<&String>
    let threads = cli.threads; // Copy

    // Default mode: display info and exit if no key/destination is provided
    if cli.payment_key.is_none() && cli.donate_to.is_none() {
        // ... (INFO ONLY logic remains the same) ...
        return Ok(());
    }

    // --- MODE A: Single Run (Uses optional --payment-key or default address) ---
    if cli.payment_key.is_some() || cli.donate_to.is_none() {

        let mining_address = cli_address_option.map(String::from).ok_or_else(|| {
            "Single Run mode requires the '--address' flag to be set.".to_string()
        })?;

        // 1. Key Generation/Loading
        let (pay_sk, pay_vk, _generated_address, pub_key_hex) = cardano::generate_cardano_key_and_address();

        // 2. Registration (Dynamic Signature)
        let reg_message = tc_response.message.clone();
        let reg_signature = cardano::cip8_sign(&pay_sk, &reg_message);

        println!("\n[REGISTRATION] Attempting registration for address: {}", mining_address);
        if let Err(e) = api::register_address(
            &api_url,
            &mining_address,
            &tc_response.message,
            &reg_signature,
            &pub_key_hex,
        ) {
            eprintln!("Address registration failed: {}. Cannot start mining cycle.", e);
            return Err("Address registration failed.".to_string());
        }

        print_mining_setup(
            &api_url,
            Some(mining_address.as_str()),
            threads,
            &challenge_params
        );
        println!("MODE: SINGLE RUN");

        if run_single_mining_cycle(
            &api_url,
            mining_address,
            threads,
            donate_to_option_ref,
            &challenge_params,
            &pay_sk,
            &pay_vk,
        ) {
            println!("\nSingle run complete.");
        }

    // --- MODE B: Continuous Mining & Donation (Requires --donate-to) ---
    } else if cli.donate_to.is_some() {

        println!("\n==============================================");
        println!("⛏️  Shadow Harvester: CONTINUOUS MINING Mode");
        println!("==============================================");
        println!("Donation Target: {}", cli.donate_to.as_ref().unwrap());

        loop {
            // 1. Generate New Key Pair for this cycle
            let (pay_sk, pay_vk, generated_mining_address, pub_key_hex) = cardano::generate_cardano_key_and_address();

            // 2. Registration (Dynamic Signature)
            println!("\n[CYCLE START] Generated Address: {}", generated_mining_address);

            let reg_message = tc_response.message.clone();
            let reg_signature = cardano::cip8_sign(&pay_sk, &reg_message);

            if let Err(e) = api::register_address(
                &api_url,
                &generated_mining_address,
                &tc_response.message,
                &reg_signature,
                &pub_key_hex,
            ) {
                eprintln!("Registration failed: {}. Cannot start mining cycle.", e);
                break;
            }

            // 3. Mining and Submission
            print_mining_setup(
                &api_url,
                Some(generated_mining_address.as_str()),
                threads,
                &challenge_params
            );

            if !run_single_mining_cycle(
                &api_url,
                generated_mining_address, // Use ownership of the generated address
                threads,
                donate_to_option_ref,
                &challenge_params,
                &pay_sk,
                &pay_vk,
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

    match run_app(cli) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("FATAL ERROR: {}", e);
            std::process::exit(1);
        }
    }
}
