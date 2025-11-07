// src/main.rs - Final Minimal Version

use clap::Parser;
use std::thread;
use std::sync::mpsc;
use std::time::Duration;
use cli::{Cli, Commands};
use crate::data_types::WebSocketCommand;

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
mod websocket_server;

use data_types::{PendingSolution, ChallengeData};


fn run_app(cli: Cli) -> Result<(), String> {
    // setup_app is where the crash originates (due to missing API URL).
    // We rely on the main function logic to ensure setup_app is only called if necessary.
    let context = match utils::setup_app(&cli) {
        Ok(c) => c,
        Err(e) if e == "COMMAND EXECUTED" => return Ok(()),
        Err(e) => return Err(e),
    };

    // Client Clone 1 & API URL Clone 1: For Submitter Thread (state_worker)
    let submitter_client = context.client.clone();
    let submitter_api_url = context.api_url.clone();

    // Client Clone 2 & API URL Clone 2: For Polling Thread
    let polling_client = context.client.clone();
    let polling_api_url = context.api_url.clone();

    // --- MPSC CHANNEL SETUP (The Communication Bus) ---
    let (manager_tx, manager_rx) = mpsc::channel();
    let (submitter_tx, submitter_rx) = mpsc::channel();
    let (ws_tx, ws_rx) = mpsc::channel();

    let (_ws_solution_tx, _ws_solution_rx) = mpsc::channel::<PendingSolution>();
    let (_ws_challenge_tx, _ws_challenge_rx) = mpsc::channel::<ChallengeData>();


    // --- THREAD DISPATCH ---
    let data_dir_clone = cli.data_dir.clone().unwrap_or_else(|| "state".to_string());
    let is_websocket_mode = cli.websocket;

    let ws_tx_for_submitter = ws_tx.clone(); // Clone for Submitter thread
    let _submitter_handle = thread::spawn(move || {
        let result = state_worker::run_state_worker(
            submitter_rx,
            submitter_client, // Use cloned client
            submitter_api_url, // Use cloned api_url
            data_dir_clone,
            is_websocket_mode,
            ws_tx_for_submitter, // <-- NEW: Pass ws_tx
        );
        if let Err(e) = result {
            eprintln!("❌ FATAL THREAD ERROR: Submitter failed: {}", e);
            std::process::exit(1);
        }
    });


    // Manager Thread - Log error if it fails
    let manager_cli = cli.clone();
    let manager_context = context; // context is moved here
    let submitter_tx_clone = submitter_tx.clone();
    let manager_tx_clone = manager_tx.clone();

    let _manager_handle = thread::spawn(move || {
        let result = challenge_manager::run_challenge_manager(
            manager_rx,
            submitter_tx_clone,
            manager_tx_clone,
            manager_cli,
            manager_context
        );
        if let Err(e) = result {
            eprintln!("❌ FATAL THREAD ERROR: Manager failed: {}", e);
            std::process::exit(1);
        }
    });


    // Polling / WebSocket Thread Dispatch - Log error if it fails
    if cli.websocket {
        let ws_port = cli.ws_port;
        let manager_tx_clone = manager_tx.clone();

        let _ws_server_handle = thread::spawn(move || {
            let result = websocket_server::start_server(manager_tx_clone, ws_rx, ws_port);
            if let Err(e) = result {
                eprintln!("❌ FATAL THREAD ERROR: WebSocket Server failed: {}", e);
                std::process::exit(1);
            }
        });
    } else if cli.challenge.is_none() {
        // Start dedicated HTTP Polling Client
        let manager_tx_clone = manager_tx.clone();

        let _polling_handle = thread::spawn(move || {
            let result = polling_client::run_polling_client(polling_client, polling_api_url, manager_tx_clone);
            if let Err(e) = result {
                eprintln!("❌ FATAL THREAD ERROR: Polling Client failed: {}", e);
                std::process::exit(1);
            }
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
    if cli.command.is_none() && cli.api_url.is_none() && !cli.websocket {
        eprintln!("❌ FATAL ERROR: must pass --api-url or --websocket or a CLI command");
        std::process::exit(1);
        return;
    }

    // 3. Handle Synchronous Commands (Migration, List, Import, Info, Db)
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

            Commands::Challenge(_) | Commands::Wallet(_) | Commands::Db(_) => {
                // The actual command data (ChallengeCommands, WalletCommands, or DbCommands) is handled internally by cli_commands::handle_sync_commands.
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
            // FIX: Ensure all setup errors are printed here before final exit
            if e != "COMMAND EXECUTED" {
                eprintln!("FATAL ERROR: {}", e);
                std::process::exit(1);
            }
        }
    }
}
