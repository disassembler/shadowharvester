// shadowharvester/src/api.rs

use serde::{Deserialize, Serialize};
use reqwest;
use reqwest::blocking;
use serde_json;

// FIX: Import MOCK constants from the new module
use crate::constants::USER_AGENT;

// --- RESPONSE STRUCTS ---

#[derive(Debug, Deserialize)]
pub struct TandCResponse {
    pub version: String,
    pub content: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RegistrationReceipt {
    #[serde(rename = "registrationReceipt")]
    pub registration_receipt: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChallengeData {
    pub challenge_id: String,
    pub difficulty: String,
    #[serde(rename = "no_pre_mine")]
    pub no_pre_mine_key: String,
    #[serde(rename = "no_pre_mine_hour")]
    pub no_pre_mine_hour_str: String,
    pub latest_submission: String,
    // NEW: Fields for listing command
    pub challenge_number: u16,
    pub day: u8,
    pub issued_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeResponse { // Made struct public for use in main.rs
    pub code: String,
    pub challenge: Option<ChallengeData>,
    pub starts_at: Option<String>,
    // NEW: Fields for listing command (overall status)
    pub mining_period_ends: Option<String>,
    pub max_day: Option<u8>,
    pub total_challenges: Option<u16>,
    pub current_day: Option<u8>,
    pub next_challenge_starts_at: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct SolutionReceipt {
    #[serde(rename = "crypto_receipt")]
    pub crypto_receipt: serde_json::Value,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct DonateResponse {
    pub status: String,
    #[serde(rename = "donation_id")]
    pub donation_id: String,
}


#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    pub message: String,
    pub error: Option<String>,
    pub statusCode: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct GlobalStatistics {
    pub wallets: u32,
    pub challenges: u16,
    #[serde(rename = "total_challenges")]
    pub total_challenges: u16,
    #[serde(rename = "total_crypto_receipts")]
    pub total_crypto_receipts: u32,
    #[serde(rename = "recent_crypto_receipts")]
    pub recent_crypto_receipts: u32,
}

// NEW: Struct for the statistics under the "local" key
#[derive(Debug, Deserialize)]
pub struct LocalStatistics {
    pub crypto_receipts: u32,
    pub night_allocation: u32,
}

// NEW: Struct representing the entire JSON response from the /statistics/:address endpoint
#[derive(Debug, Deserialize)]
pub struct StatisticsApiResponse {
    pub global: GlobalStatistics,
    pub local: LocalStatistics,
}

#[derive(Debug)]
pub struct Statistics {
    // Local Address (Added by the client)
    pub local_address: String,
    // Global fields
    pub wallets: u32,
    pub challenges: u16,
    pub total_challenges: u16,
    pub total_crypto_receipts: u32,
    pub recent_crypto_receipts: u32,
    // Local fields
    pub crypto_receipts: u32,
    pub night_allocation: u32,
}
// NEW: Struct for the challenge parameters provided via CLI
#[derive(Debug, Clone)]
pub struct CliChallengeData {
    pub challenge_id: String,
    pub no_pre_mine_key: String,
    pub difficulty: String,
    pub no_pre_mine_hour_str: String,
    pub latest_submission: String,
}

// NEW FUNCTION: Parses the comma-separated CLI challenge string
pub fn parse_cli_challenge_string(challenge_str: &str) -> Result<CliChallengeData, String> {
    let parts: Vec<&str> = challenge_str.split(',').collect();

    if parts.len() != 5 {
        return Err(format!(
            "Invalid --challenge format. Expected 5 comma-separated values, found {}. Format: challenge_id,no_pre_mine,difficulty,no_pre_mine_hour,latest_submission",
            parts.len()
        ));
    }

    Ok(CliChallengeData {
        challenge_id: parts[0].trim().to_string(),
        no_pre_mine_key: parts[1].trim().to_string(),
        difficulty: parts[2].trim().to_string(),
        no_pre_mine_hour_str: parts[3].trim().to_string(),
        latest_submission: parts[4].trim().to_string(),
    })
}


// --- API FUNCTIONS ---

/// Fetches the T&C from the API, returning the full response object.
pub fn fetch_tandc(client: &blocking::Client, api_url: &str) -> Result<TandCResponse, reqwest::Error> {
    let url = format!("{}/TandC/1-0", api_url);
    println!("-> Fetching Terms and Conditions from: {}", url);

    let response = client.get(url).send()?;

    let response = response.error_for_status()?;

    response.json()
}

/// Performs the POST /register call using key/signature arguments.
// RENAMED from register_address_mock
pub fn register_address(
    client: &blocking::Client,
    api_url: &str,
    address: &str,
    _tc_message: &str,
    signature: &str,
    pubkey: &str,
) -> Result<(), reqwest::Error> {
    let url = format!(
        "{}/register/{}/{}/{}",
        api_url,
        address,
        signature,
        pubkey
    );

    println!("-> Attempting address registration for address: {}", address);

    let response = client
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send()?;

    let response = response.error_for_status()?;

    let registration_receipt: RegistrationReceipt = response.json()?;
    println!("✅ Address registered successfully.");
    println!("Receipt: {}", registration_receipt.registration_receipt);

    Ok(())
}

/// Fetches the current challenge parameters from the API.
pub fn fetch_challenge(client: &blocking::Client, api_url: &str) -> Result<ChallengeData, String> {
    let url = format!("{}/challenge", api_url);
    println!("-> Fetching current challenge from: {}", url);

    let response = client.get(url).send().map_err(|e| format!("API request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Challenge API returned non-success status: {}", response.status()));
    }

    let challenge_response: ChallengeResponse = response.json().map_err(|e| format!("JSON parsing failed: {}", e))?;

    match challenge_response.code.as_str() {
        "active" => {
            Ok(challenge_response.challenge.unwrap())
        }
        "before" => {
            let start_time = challenge_response.starts_at.unwrap_or_default();
            Err(format!("MINING IS NOT YET ACTIVE. Starts at: {}", start_time))
        }
        "after" => {
            Err("MINING PERIOD HAS ENDED.".to_string())
        }
        _ => {
            Err(format!("Received unexpected challenge code: {}", challenge_response.code))
        }
    }
}

/// Performs the POST /solution call.
pub fn submit_solution(
    client: &blocking::Client,
    api_url: &str,
    address: &str,
    challenge_id: &str,
    nonce: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/solution/{}/{}/{}",
        api_url,
        address,
        challenge_id,
        nonce
    );

    println!("-> Submitting solution (Nonce: {})", nonce);

    let response = client
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send().map_err(|e| format!("Network/Client Error: {}", e))?;

    let status = response.status();

    if status.is_success() {
        // Successful submission
        let receipt: SolutionReceipt = response.json().map_err(|e| format!("Failed to parse successful receipt JSON: {}", e))?;
        Ok(receipt.crypto_receipt)
    } else {
        // Submission failed (4xx or 5xx)
        let body_text = response.text().unwrap_or_else(|_| format!("Could not read response body for status {}", status));

        let api_error: Result<ApiErrorResponse, _> = serde_json::from_str(&body_text);

        match api_error {
            Ok(err) => {
                // API returned a structured error message (e.g., Not registered, Invalid difficulty)
                Err(format!("API Validation Failed (Status {}): {}", status.as_u16(), err.message))
            }
            Err(_) => {
                // API returned a non-structured error (e.g., plain text or unreadable JSON)
                Err(format!("HTTP Error {} with unparseable body: {}", status.as_u16(), body_text))
            }
        }
    }
}

/// Performs the POST /donate_to call.
// RENAMED from donate_to_mock
pub fn donate_to(
    client: &blocking::Client,
    api_url: &str,
    original_address: &str,
    destination_address: &str,
    donation_signature: &str,
) -> Result<String, String> {

    let url = format!(
        "{}/donate_to/{}/{}/{}",
        api_url,
        destination_address,
        original_address,
        donation_signature
    );

    println!("-> Donating funds from {} to {}", original_address, destination_address);
    println!("url: {}", &url);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&serde_json::json!({}))
        .send().map_err(|e| format!("Network/Client Error: {}", e))?;

    let status = response.status();

    if status.is_success() {
        let donation_response: DonateResponse = response.json().map_err(|e| format!("Failed to parse successful donation JSON: {}", e))?;
        println!("✅ Donation successful. Donation ID: {}", donation_response.donation_id);
        Ok(donation_response.donation_id)
    } else {
        let body_text = response.text().unwrap_or_else(|_| format!("Could not read response body for status {}", status));
        let api_error: Result<ApiErrorResponse, _> = serde_json::from_str(&body_text);

        match api_error {
            Ok(err) => {
                Err(format!("Donation Failed (Status {}): {}", status.as_u16(), err.message))
            }
            Err(_) => {
                Err(format!("HTTP Error {} with unparseable body: {}", status.as_u16(), body_text))
            }
        }
    }
}

/// Fetches the raw Challenge Response object from the API.
// NEW FUNCTION
pub fn fetch_challenge_status(client: &blocking::Client, api_url: &str) -> Result<ChallengeResponse, String> {
    let url = format!("{}/challenge", api_url);

    let response = client.get(url).send().map_err(|e| format!("API request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Challenge API returned non-success status: {}", response.status()));
    }

    let challenge_response: ChallengeResponse = response.json().map_err(|e| format!("JSON parsing failed: {}", e))?;
    Ok(challenge_response)
}

/// Fetches and validates the active challenge parameters, returning data only if active.
// RENAMED from fetch_challenge
pub fn get_active_challenge_data(client: &blocking::Client, api_url: &str) -> Result<ChallengeData, String> {
    let challenge_response = fetch_challenge_status(&client, api_url)?;

    match challenge_response.code.as_str() {
        "active" => {
            // Unwrap is safe because 'challenge' should be present when code is "active"
            Ok(challenge_response.challenge.unwrap())
        }
        "before" => {
            let start_time = challenge_response.starts_at.unwrap_or_default();
            Err(format!("MINING IS NOT YET ACTIVE. Starts at: {}", start_time))
        }
        "after" => {
            Err("MINING PERIOD HAS ENDED.".to_string())
        }
        _ => {
            Err(format!("Received unexpected challenge code: {}", challenge_response.code))
        }
    }
}


// ... (existing API FUNCTIONS)

pub fn fetch_statistics(client: &blocking::Client, api_url: &str, address: &str) -> Result<Statistics, String> {
    let url = format!("{}/statistics/{}", api_url, address);
    println!("\n📊 Fetching statistics for address: {}", address);

    let response = client.get(url)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Network/Client Error: {}", e))?;

    let status = response.status();

    if status.is_success() {
        let api_data: StatisticsApiResponse = response.json().map_err(|e| format!("JSON parsing failed: {}", e))?;

        // Transform nested API response into the desired flat Statistics struct
        Ok(Statistics {
            local_address: address.to_string(),
            wallets: api_data.global.wallets,
            challenges: api_data.global.challenges,
            total_challenges: api_data.global.total_challenges,
            total_crypto_receipts: api_data.global.total_crypto_receipts,
            recent_crypto_receipts: api_data.global.recent_crypto_receipts,
            crypto_receipts: api_data.local.crypto_receipts,
            night_allocation: api_data.local.night_allocation,
        })
    } else {
        let body_text = response.text().unwrap_or_else(|_| format!("(Could not read response body for status {})", status));
        // ... (error handling omitted)
        let api_error: Result<ApiErrorResponse, _> = serde_json::from_str(&body_text);

        match api_error {
            Ok(err) => {
                Err(format!("API Error (Status {}): {}", status.as_u16(), err.message))
            }
            Err(_) => {
                Err(format!("HTTP Error {} with unparseable body: {}", status.as_u16(), body_text))
            }
        }
    }
}
