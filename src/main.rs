

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

// --- PLACEHOLDER DATA FOR MOCK CIP-30 SIGNATURE/PUBKEY ---
const MOCK_PUBKEY: &str = "009236f53f5fbb1056defb64d623f7508bc1b61822016d43faf62070f1489ff5";
const MOCK_SIGNATURE: &str = "845882a30127045839001c2e057143337716055394074256b79df7fc36051802ccefc1b2d3bf2a77372b1cff16a2cfe1e9a634bfaae3c74e8c8188e6043f572295f067616464726573735839001c2e057143337716055394074256b79df7fc36051802ccefc1b2d3bf2a77372b1cff16a2cfe1e9a634bfaae3c74e8c8188e6043f572295f0a166686173686564f458b34920616772656520746f20616269646520627920746865207465726d7320616e6420636f6e646974696f6e732061732064657363726962656420696e2076657273696f6e20312d30206f6620746865204d69646e696768742073636176656e676572206d696e696e672070726f636573733a20666566653336626638653566623436313663633536386138643762613230616237306361626632653837623866383661656362393662303264383365643438665840504a01e23e3a6cefcb93901af88f9873421fa76f75f98d7d0c7b43b3bdf09676921d7ee326f3bf1061fea4c62e07b84b9fdd1e17cd5d52790a7c1a0fce99e80e";

// shadowharvester/src/main.rs

// ... (existing use statements) ...

// Structs for API response deserialization
#[derive(Debug, Deserialize)]
struct TandCResponse {
    pub version: String,
    pub content: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct RegistrationReceipt {
    #[serde(rename = "registrationReceipt")]
    pub registration_receipt: serde_json::Value, // Capture the full object
}

/// Fetches the T&C from the API, returning the full response object.
fn fetch_tandc(api_url: &str) -> Result<TandCResponse, reqwest::Error> {
    let url = format!("{}/TandC/1-0", api_url);

    let client = reqwest::blocking::Client::new();
    let response = client.get(url).send()?;

    let response = response.error_for_status()?;

    response.json()
}

/// Performs a mock POST /register call using a fetched T&C message and placeholder key/signature data.
fn register_address_mock(api_url: &str, address: &str, _tc_message: &str) -> Result<(), reqwest::Error> {
    // NOTE: The CIP-30 signature is hardcoded and ignores the tc_message, but the API endpoint URL still needs the signature.
    let url = format!(
        "{}/register/{}/{}/{}",
        api_url,
        address,
        MOCK_SIGNATURE,
        MOCK_PUBKEY
    );

    println!("-> Attempting mock registration for address: {}", address);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send()?;

    let response = response.error_for_status()?;

    let registration_receipt: RegistrationReceipt = response.json()?;
    println!("✅ Address registered successfully.");
    // Display the receipt (or parts of it)
    println!("Receipt: {}", registration_receipt.registration_receipt);

    Ok(())
}

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

    // 2. Fetch T&C message (always required)
    let tc_response = match fetch_tandc(&api_url) {
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

    // 4. Register the address (MOCK)
    if let Err(e) = register_address_mock(&api_url, &cli.address, &tc_response.message) {
        eprintln!("FATAL ERROR: Address registration failed. Details: {}", e);
        return;
    }

    // --- MINING SETUP ---

    println!("API URL: {}", api_url);
    println!("Mining address: {}", cli.address);

    // 5. Start Scavenging (Using mock challenge data)

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
