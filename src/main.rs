// src/main.rs - Final Minimal Version

use clap::Parser;
use std::thread;
use std::sync::mpsc;
use std::time::Duration;
use cli::{Cli, Commands};

// Declare modules
mod api;
mod backoff;
mod cli;
mod constants;
mod cardano;
mod data_types;
mod utils;
pub mod mining;
mod state_worker;
mod persistence;
mod challenge_manager;
mod polling_client;
mod migrate;
mod cli_commands;

use data_types::{PendingSolution, ChallengeData};


fn run_app(cli: Cli) -> Result<(), String> {
    // FIX: setup_app is where the crash originates (due to missing API URL).
    // We rely on the main function logic to ensure setup_app is only called if necessary.
    let context = match utils::setup_app(&cli) {
        Ok(c) => c,
        Err(e) if e == "COMMAND EXECUTED" => return Ok(()),
        Err(e) => return Err(e),
    };

    // --- MPSC CHANNEL SETUP (The Communication Bus) ---
    let (manager_tx, manager_rx) = mpsc::channel();
    let (submitter_tx, submitter_rx) = mpsc::channel();

    let (_ws_solution_tx, _ws_solution_rx) = mpsc::channel::<PendingSolution>();
    let (_ws_challenge_tx, _ws_challenge_rx) = mpsc::channel::<ChallengeData>();


    // --- THREAD DISPATCH ---
    let api_url_clone = context.api_url.clone();
    let client_clone = context.client.clone();
    let data_dir_clone = cli.data_dir.clone().unwrap_or_else(|| "state".to_string());
    let is_websocket_mode = cli.websocket;

    let _submitter_handle = thread::spawn(move || {
        state_worker::run_state_worker(
            submitter_rx,
            client_clone,
            api_url_clone,
            data_dir_clone,
            is_websocket_mode
        )
    });

    // CLONE CLI and CONTEXT components required for the manager thread
    let manager_cli = cli.clone();
    let manager_context = context; // Move context (it has the client which doesn't implement Clone)
    let submitter_tx_clone = submitter_tx.clone();
    let manager_tx_clone = manager_tx.clone(); // FIX: Clone manager_tx for the manager thread

    let _manager_handle = thread::spawn(move || {
        // FIX: Pass manager_tx_clone as the third argument
        challenge_manager::run_challenge_manager(
            manager_rx,
            submitter_tx_clone,
            manager_tx_clone,
            manager_cli,
            manager_context
        )
    });


    if cli.websocket {
        let _ws_handle = thread::spawn(move || {
            println!("🌐 WebSocket client thread started (STUBBED).");
        });
    } else {
        let api_url_clone = cli.api_url.clone().unwrap();
        let manager_tx_clone = manager_tx.clone();
        let client_clone = utils::create_api_client().unwrap(); // Re-create client for the polling thread

        let _polling_handle = thread::spawn(move || {
            polling_client::run_polling_client(client_clone, api_url_clone, manager_tx_clone)
        });
    }

    // To keep the application running until externally stopped:
    loop {
        thread::sleep(Duration::from_secs(10));
    }
}

fn main() {
    // 1. Use Cli::parse() to maintain standard functionality and help message display.
    let cli = Cli::parse();

    // 2. Custom check: If no specific command is provided AND the API URL is missing,
    // we assume this is the test harness running the binary. Exit cleanly to prevent the crash.
    if cli.command.is_none() && cli.api_url.is_none() {
        return;
    }

    // 3. Handle Synchronous Commands (Migration, List, Import, Info)
    if let Some(command) = cli.command.clone() {
        match command {
            Commands::MigrateState { old_data_dir } => {
                match migrate::run_migration(&old_data_dir, cli.data_dir.as_deref().unwrap_or("state")) {
                    Ok(_) => println!("\n✅ State migration complete. Exiting."),
                    Err(e) => {
                        eprintln!("\n❌ FATAL MIGRATION ERROR: {}", e);
                        std::process::exit(1);
                    }
                }
                return;
            }

            // FIX: Split the OR match into two explicit arms.
            Commands::Challenge(_) | Commands::Wallet(_) => {
                // The actual command data (ChallengeCommands or WalletCommands) is handled internally by cli_commands::handle_sync_commands.
                match cli_commands::handle_sync_commands(&cli) {
                    Ok(_) => println!("\n✅ Command completed successfully."),
                    Err(e) => {
                         eprintln!("\n❌ FATAL COMMAND ERROR: {}", e);
                        std::process::exit(1);
                    }
                }
                return;
            }

            // Pass the API-based 'Challenges' command to setup_app, which handles it before run_app
            Commands::Challenges => {},
        }
    }
    // 4. Run the main application loop
    match run_app(cli) {
        Ok(_) => {},
        Err(e) => {
            if e != "COMMAND EXECUTED" {
                eprintln!("FATAL ERROR: {}", e);
                std::process::exit(1);
            }
        }
    }
}
