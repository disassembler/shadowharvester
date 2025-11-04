// src/main.rs - Final Minimal Version

use clap::Parser;

// Declare modules
mod api;
mod backoff;
mod cli;
mod constants;
mod cardano;
mod data_types; // New module for core types and file persistence logic
mod mining;     // New module for all application setup and mining mode logic

use mining::{setup_app, run_persistent_key_mining, run_mnemonic_sequential_mining, run_new_key_per_cycle_mining, print_mining_setup};
use cli::Cli;
use api::get_active_challenge_data; // Needed for the info-only mode


/// Runs the main application logic based on CLI flags.
fn run_app(cli: Cli) -> Result<(), String> {
    let context = match setup_app(&cli) {
        Ok(c) => c,
        // Exit the app if a command like 'Challenges' was run successfully
        Err(e) if e == "COMMAND EXECUTED" => return Ok(()),
        Err(e) => return Err(e),
    };

    // --- Pre-extract mnemonic logic ---
    let mnemonic: Option<String> = if let Some(mnemonic) = cli.mnemonic.clone() {
        Some(mnemonic)
    } else if let Some(mnemonic_file) = cli.mnemonic_file.clone() {
        Some(std::fs::read_to_string(mnemonic_file)
            .map_err(|e| format!("Could not read mnemonic from file: {}", e))?)
    } else {
        None
    };

    // 1. Default mode: display info and exit
    if cli.payment_key.is_none() && cli.donate_to.is_none() && mnemonic.is_none() && cli.challenge.is_none() {
        // Fetch challenge for info display
        match get_active_challenge_data(&context.client, &context.api_url) {
            Ok(challenge_params) => {
                 print_mining_setup(
                    &context.api_url,
                    cli.address.as_deref(),
                    context.threads,
                    &challenge_params
                );
            },
            Err(e) => eprintln!("Could not fetch active challenge for info display: {}", e),
        };
        println!("MODE: INFO ONLY. Provide '--payment-key', '--mnemonic', '--mnemonic-file', '--donate-to', or '--challenge' to begin mining.");
        return Ok(())
    }

    // 2. Determine Operation Mode and Start Mining
    if let Some(skey_hex) = cli.payment_key.as_ref() {
        run_persistent_key_mining(&cli, context, skey_hex)
    }
    else if let Some(mnemonic_phrase) = mnemonic {
        run_mnemonic_sequential_mining(&cli, context, mnemonic_phrase)
    }
    else if cli.donate_to.is_some() {
        run_new_key_per_cycle_mining(context)
    } else {
        // This should be unreachable due to the check above
        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();

    match run_app(cli) {
        Ok(_) => {},
        Err(e) => {
            if e != "COMMAND EXECUTED" { // Don't print fatal error if a command ran successfully
                eprintln!("FATAL ERROR: {}", e);
                std::process::exit(1);
            }
        }
    }
}
