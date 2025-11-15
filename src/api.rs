// src/api.rs

use reqwest::blocking;
use std::thread;
use std::time::Duration;

// FIX: Import structs from the new module location
use crate::data_types::{
    TandCResponse, RegistrationReceipt, ChallengeData, ChallengeResponse,
    SolutionReceipt, DonateResponse, Statistics, StatisticsApiResponse, CliChallengeData, ApiErrorResponse
};

// --- API FUNCTIONS ---

/// Fetches the T&C from the API, returning the full response object.
pub fn fetch_tandc(client: &blocking::Client, api_url: &str) -> Result<TandCResponse, reqwest::Error> {
    let url = format!("{}/TandC/1-0", api_url);
    println!("-> Fetching Terms and Conditions from: {}", url);

    let response = client.get(url).send()?;

    let response = response.error_for_status()?;

    response.json()
}

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


/// Performs the POST /register call using key/signature arguments.
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

/// Helper to format a detailed error message from the API response body.
fn format_detailed_api_error(err: ApiErrorResponse, status: reqwest::StatusCode) -> String {
    let mut msg = format!("(Status {}) {}", status.as_u16(), err.message);

    if let Some(e) = err.error {
        msg.push_str(&format!(" [Type: {}]", e));
    }
    if let Some(code) = err.status_code {
        msg.push_str(&format!(" [API Code: {}]", code));
    }
    msg
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
                // FIX: Use all error fields for detailed reporting
                Err(format!("API Validation Failed: {}", format_detailed_api_error(err, status)))
            }
            Err(_) => {
                // API returned a non-structured error (e.g., plain text or unreadable JSON)
                Err(format!("HTTP Error {} with unparseable body: {}", status.as_u16(), body_text))
            }
        }
    }
}

/// Performs the POST /donate_to call.
pub fn donate_to(
    client: &blocking::Client,
    api_url: &str,
    original_address: &str,
    destination_address: &str,
    donation_signature: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/donate_to/{}/{}/{}",
        api_url.trim_end_matches('/'),
        destination_address,
        original_address,
        donation_signature
    );

    // Same empty JSON body as before (explicit for logging)
    let body = serde_json::json!({});
    let mut attempt: u32 = 0;
    let max_attempts: u32 = 3;

    println!("-> Donating funds from {} to {}", original_address, destination_address);

    while attempt <= max_attempts {
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send();

        match resp {
            Ok(response) => {
                let status = response.status();
                // Read once (text may be JSON or plain)
                let text = response.text().unwrap_or_default();

                // Always log request/response for debugging
                println!("\n----------------------------------------------");
                println!("📤 Request:");
                println!("  URL : {}", url);
                println!("  Body: {}", body); // prints {}
                println!("📥 Response:");
                println!("  Status: {}", status);
                println!("  Body  : {}", text);
                println!("----------------------------------------------");

                // Treat 2xx as success; 409 as success/“already done”
                if status.is_success() || status.as_u16() == 409 {
                    // Try to parse donation_id; if absent (e.g., some 409s), return a marker
                    if let Ok(parsed) = serde_json::from_str::<DonateResponse>(&text) {
                        println!("✅ Donation successful. Donation ID: {}", parsed.donation_id);
                        return Ok(parsed.donation_id);
                    } else {
                        println!("✅ SUCCESS/ALREADY DONE (no donation_id in response JSON)");
                        return Ok("(already-done)".to_string());
                    }
                }

                // Handle common 4xx we care about with detailed JSON-parsed error if available
                match status.as_u16() {
                    400 | 404 => {
                        if let Ok(err) = serde_json::from_str::<ApiErrorResponse>(&text) {
                            return Err(format!(
                                "Donation Failed: {}",
                                format_detailed_api_error(err, status)
                            ));
                        }
                        return Err(format!(
                            "HTTP Error {} with unparseable body: {}",
                            status.as_u16(),
                            text
                        ));
                    }
                    // Retryable server / rate limiting / timeout style errors
                    s if s >= 500 || s == 429 || s == 408 => {
                        attempt = attempt.saturating_add(1);
                        if attempt > max_attempts {
                            break;
                        }
                        let wait_ms = 5000u64.saturating_mul(1u64 << (attempt - 1)); // 5s, 10s, 20s
                        eprintln!(
                            "⏳ Server {} – retry {}/{} in {}s…",
                            s,
                            attempt,
                            max_attempts,
                            wait_ms / 1000
                        );
                        thread::sleep(Duration::from_millis(wait_ms));
                        continue;
                    }
                    // Other non-retryable 4xx
                    _ => {
                        if let Ok(err) = serde_json::from_str::<ApiErrorResponse>(&text) {
                            return Err(format!(
                                "Donation Failed: {}",
                                format_detailed_api_error(err, status)
                            ));
                        }
                        return Err(format!(
                            "HTTP Error {} with unparseable body: {}",
                            status.as_u16(),
                            text
                        ));
                    }
                }
            }
            Err(e) => {
                attempt = attempt.saturating_add(1);
                let wait_ms = 5000u64.saturating_mul(1u64 << (attempt - 1)); // 5s, 10s, 20s
                eprintln!(
                    "\n----------------------------------------------\n\
                     🌐 NETWORK ERROR on attempt {}/{}\n\
                     URL : {}\nError: {}\n\
                     Retrying in {}s…\n----------------------------------------------",
                    attempt,
                    max_attempts,
                    url,
                    e,
                    wait_ms / 1000
                );
                if attempt > max_attempts {
                    break;
                }
                thread::sleep(Duration::from_millis(wait_ms));
            }
        }
    }

    Err(format!(
        "Max retries exceeded for original_address {} → destination {}",
        original_address, destination_address
    ))
}

/// Fetches the raw Challenge Response object from the API.
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
pub fn get_active_challenge_data(client: &blocking::Client, api_url: &str) -> Result<ChallengeData, String> {
    let challenge_response = fetch_challenge_status(client, api_url)?;

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
            recent_crypto_receipts: api_data.global.recent_crypto_receipts,
            total_crypto_receipts: api_data.global.total_crypto_receipts,
            crypto_receipts: api_data.local.crypto_receipts,
            night_allocation: api_data.local.night_allocation,
        })
    } else {
        let body_text = response.text().unwrap_or_else(|_| format!("(Could not read response body for status {})", status));
        let api_error: Result<ApiErrorResponse, _> = serde_json::from_str(&body_text);

        match api_error {
            Ok(err) => {
                // FIX: Use all error fields for detailed reporting
                Err(format!("API Error: {}", format_detailed_api_error(err, status)))
            }
            Err(_) => {
                Err(format!("HTTP Error {} with unparseable body: {}", status.as_u16(), body_text))
            }
        }
    }
}
