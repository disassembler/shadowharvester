use shadow_harvester_lib::scavenge;
use clap::Parser;
use serde::Deserialize;
use reqwest;

// Declare the CLI module
mod cli;
use cli::{Cli, Commands};

const NB_THREADS: u32 = 10;

// API Test Vector Data (Used as mock challenge data)
const CHALLENGE_ID: &str = "**D07C10";
const DIFFICULTY: &str = "000FFFFF";
const NO_PRE_MINE_KEY: &str = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011";
const LATEST_SUBMISSION: &str = "2025-10-19T08:59:59.000Z";
const NO_PRE_MINE_HOUR: &str = "509681483";


// Structs for API response deserialization
#[derive(Debug, Deserialize)]
struct TandCResponse {
    version: String,
    content: String,
    message: String,
}

/// Fetches the T&C from the API and prints them to the console.
fn fetch_and_display_tandc(api_url: &str) -> Result<(), reqwest::Error> {
    let url = format!("{}/TandC/1-0", api_url);
    println!("-> Fetching Terms and Conditions from: {}", url);

    let client = reqwest::blocking::Client::new();
    let response = client.get(url).send()?;

    if !response.status().is_success() {
        eprintln!("Error: Failed to fetch T&C. Status: {}", response.status());
        return Err(response.error_for_status().unwrap_err());
    }

    let tc: TandCResponse = response.json()?;

    println!("\n--- Token End-User Terms (Version {}) ---", tc.version);
    println!("{}", tc.content);
    println!("--------------------------------------------------");
    println!("Agreement Message:\n'{}'", tc.message);
    println!("--------------------------------------------------");

    println!("\nERROR: You must pass the '--accept-tos' flag to proceed with mining.");

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    if let Some(Commands::KeyGen) = cli.command {
        // Call the key generation function from the new module
        //cardano::generate_cardano_key_and_address();
        return; // Exit after key generation
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

    // 2. Check for --accept-tos
    if !cli.accept_tos {
        match fetch_and_display_tandc(&api_url) {
            Ok(_) => return, // T&C displayed, terminate.
            Err(e) => {
                eprintln!("FATAL ERROR: Could not fetch T&C from API URL: {}. Ensure the URL is correct and the server is running.", api_url);
                eprintln!("Details: {}", e);
                return;
            }
        }
    }

    println!("API URL: {}", api_url);
    println!("Mining address: {}", cli.address);

    // 3. Start Scavenging (Using mock data for the challenge)

    scavenge(
        cli.address,
        CHALLENGE_ID.to_string(),
        DIFFICULTY.to_string(),
        NO_PRE_MINE_KEY.to_string(),
        LATEST_SUBMISSION.to_string(),
        NO_PRE_MINE_HOUR.to_string(),
        NB_THREADS,
    );
}
