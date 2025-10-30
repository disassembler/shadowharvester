// shadowharvester/src/api.rs

use serde::Deserialize;
use reqwest;
use serde_json;

// FIX: Import MOCK constants from the new module
use crate::constants::{MOCK_PUBKEY, MOCK_SIGNATURE, DONATE_MESSAGE_SIG};

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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct SolutionReceipt {
    #[serde(rename = "crypto_receipt")]
    pub crypto_receipt: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct DonateResponse {
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


// --- API FUNCTIONS ---

/// Fetches the T&C from the API, returning the full response object.
pub fn fetch_tandc(api_url: &str) -> Result<TandCResponse, reqwest::Error> {
    let url = format!("{}/TandC/1-0", api_url);
    println!("-> Fetching Terms and Conditions from: {}", url);

    let client = reqwest::blocking::Client::new();
    let response = client.get(url).send()?;

    let response = response.error_for_status()?;

    response.json()
}

/// Performs the POST /register call using key/signature arguments.
// RENAMED from register_address_mock
pub fn register_address(
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

    let client = reqwest::blocking::Client::new();
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
pub fn fetch_challenge(api_url: &str) -> Result<ChallengeData, String> {
    let url = format!("{}/challenge", api_url);
    println!("-> Fetching current challenge from: {}", url);

    let client = reqwest::blocking::Client::new();
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
    api_url: &str,
    address: &str,
    challenge_id: &str,
    nonce: &str,
) -> Result<(), String> {
    let url = format!(
        "{}/solution/{}/{}/{}",
        api_url,
        address,
        challenge_id,
        nonce
    );

    println!("-> Submitting solution (Nonce: {})", nonce);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send().map_err(|e| format!("Network/Client Error: {}", e))?;

    let status = response.status();

    if status.is_success() {
        // Successful submission
        let _: SolutionReceipt = response.json().map_err(|e| format!("Failed to parse successful receipt JSON: {}", e))?;
        Ok(())
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
    api_url: &str,
    original_address: &str,
    destination_address: &str,
    donation_signature: &str,
) -> Result<(), String> {

    let url = format!(
        "{}/donate_to/{}/{}/{}",
        api_url,
        destination_address,
        original_address,
        donation_signature
    );

    println!("-> Donating funds from {} to {}", original_address, destination_address);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send().map_err(|e| format!("Network/Client Error: {}", e))?;

    let status = response.status();

    if status.is_success() {
        let donation_response: DonateResponse = response.json().map_err(|e| format!("Failed to parse successful donation JSON: {}", e))?;
        println!("✅ Donation successful. Donation ID: {}", donation_response.donation_id);
        Ok(())
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
pub fn fetch_challenge_status(api_url: &str) -> Result<ChallengeResponse, String> {
    let url = format!("{}/challenge", api_url);

    let client = reqwest::blocking::Client::new();
    let response = client.get(url).send().map_err(|e| format!("API request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Challenge API returned non-success status: {}", response.status()));
    }

    let challenge_response: ChallengeResponse = response.json().map_err(|e| format!("JSON parsing failed: {}", e))?;
    Ok(challenge_response)
}

/// Fetches and validates the active challenge parameters, returning data only if active.
// RENAMED from fetch_challenge
pub fn get_active_challenge_data(api_url: &str) -> Result<ChallengeData, String> {
    let challenge_response = fetch_challenge_status(api_url)?;

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
