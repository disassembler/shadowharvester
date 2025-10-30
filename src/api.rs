// shadowharvester/src/api.rs

use serde::Deserialize;
use reqwest;
use serde_json;

// --- CONSTANTS ---
const MOCK_PUBKEY: &str = "009236f53f5fbb1056defb64d623f7508bc1b61822016d43faf62070f1489ff5";
const MOCK_SIGNATURE: &str = "845882a30127045839001c2e057143337716055394074256b79df7fc36051802ccefc1b2d3bf2a77372b1cff16a2cfe1e9a634bfaae3c74e8c8188e6043f572295f067616464726573735839001c2e057143337716055394074256b79df7fc36051802ccefc1b2d3bf2a77372b1cff16a2cfe1e9a634bfaae3c74e8c8188e6043f572295f0a166686173686564f458b34920616772656520746f20616269646520627920746865207465726d7320616e6420636f6e646974696f6e732061732064657363726962656420696e2076657273696f6e20312d30206f6620746865204d69646e696768742073636176656e676572206d696e696e672070726f636573733a20666566653336626638653566623436313663633536386138643762613230616237306361626632653837623866383661656362393662303264383365643438665840504a01e23e3a6cefcb93901af88f9873421fa76f75f98d7d0c7b43b3bdf09676921d7ee326f3bf1061fea4c62e07b84b9fdd1e17cd5d52790a7c1a0fce99e80e";

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
}

#[derive(Debug, Deserialize)]
struct ChallengeResponse {
    pub code: String,
    pub challenge: Option<ChallengeData>,
    pub starts_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SolutionReceipt {
    #[serde(rename = "crypto_receipt")]
    pub crypto_receipt: serde_json::Value,
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

/// Performs a mock POST /register call using a fetched T&C message and placeholder key/signature data.
pub fn register_address_mock(api_url: &str, address: &str, _tc_message: &str) -> Result<(), reqwest::Error> {
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

/// Performs a mock POST /solution call.
pub fn submit_solution_mock(
    api_url: &str,
    address: &str,
    challenge_id: &str,
    nonce: &str,
) -> Result<(), reqwest::Error> {
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
        .send()?;

    let response = response.error_for_status()?;

    // Deserialize the crypto_receipt to confirm successful submission
    let _: SolutionReceipt = response.json()?;

    Ok(())
}
