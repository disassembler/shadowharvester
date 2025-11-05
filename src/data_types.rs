// src/data_types.rs

use std::hash::{Hash, Hasher, DefaultHasher};
use std::path::PathBuf;
use std::io::Write;
use reqwest::blocking;
use serde::{Deserialize, Serialize};

// ===============================================
// API RESPONSE STRUCTS (Moved from src/api.rs)
// ===============================================

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
    // Fields for listing command
    pub challenge_number: u16,
    pub day: u8,
    pub issued_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeResponse {
    pub code: String,
    pub challenge: Option<ChallengeData>,
    pub starts_at: Option<String>,
    // Fields for listing command (overall status)
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
pub struct ApiErrorResponse {
    pub message: String,
    pub error: Option<String>,
    // FIX: Change to snake_case and use rename attribute for deserialization
    #[serde(rename = "statusCode")]
    pub status_code: Option<u16>,
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

// Struct for the statistics under the "local" key
#[derive(Debug, Deserialize)]
pub struct LocalStatistics {
    pub crypto_receipts: u32,
    pub night_allocation: u32,
}

// Struct representing the entire JSON response from the /statistics/:address endpoint
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
// Struct for the challenge parameters provided via CLI
#[derive(Debug, Clone)]
pub struct CliChallengeData {
    pub challenge_id: String,
    pub no_pre_mine_key: String,
    pub difficulty: String,
    pub no_pre_mine_hour_str: String,
    pub latest_submission: String,
}

// ===============================================
// CORE APPLICATION STRUCTS
// ===============================================

// Holds the common, validated state for the mining loops.
#[derive(Debug)]
pub struct MiningContext<'a> {
    pub client: blocking::Client,
    pub api_url: String,
    // FIX: Use the struct from its new location
    pub tc_response: TandCResponse,
    pub donate_to_option: Option<&'a String>,
    pub threads: u32,
    pub cli_challenge: Option<&'a String>,
    pub data_dir: Option<&'a str>,
}


// Holds the data needed to submit a solution later.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PendingSolution {
    pub address: String,
    pub challenge_id: String,
    pub nonce: String,
    pub donation_address: Option<String>, // RE-ADDED this field
}

// Define a result type for the mining cycle
#[derive(Debug, PartialEq)]
pub enum MiningResult {
    FoundAndQueued, // Solution found and saved to local queue
    #[allow(dead_code)] // The submitter thread produces this result conceptually when processing a queue item, but the miner never constructs it.
    AlreadySolved, // The solution was successfully submitted by someone else
    MiningFailed,  // General mining or submission error (e.g., hash not found, transient API error)
}

// --- DataDir Structures and Constants ---
pub const FILE_NAME_CHALLENGE: &str = "challenge.json";
pub const FILE_NAME_RECEIPT: &str = "receipt.json";
pub const FILE_NAME_FOUND_SOLUTION: &str = "found.json"; // (Crash recovery file)


#[derive(Debug, Clone, Copy)]
pub enum DataDir<'a> {
    Persistent(&'a str),
    Ephemeral(&'a str),
    Mnemonic(DataDirMnemonic<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct DataDirMnemonic<'a> {
    pub mnemonic: &'a str,
    pub account: u32,
    pub deriv_index: u32,
}

fn normalize_challenge_id(challenge_id: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        // Directories with '*' are not supported on windows
        challenge_id.replace("*", "")
    }
    #[cfg(not(target_os = "windows"))]
    {
        challenge_id.to_string()
    }
}

impl<'a> DataDir<'a> {
    pub fn challenge_dir(&'a self, base_dir: &str, challenge_id: &str) -> Result<PathBuf, String> {
        let mut path = PathBuf::from(base_dir);
        path.push(normalize_challenge_id(challenge_id));
        Ok(path)
    }

    pub fn receipt_dir(&'a self, base_dir: &str, challenge_id: &str) -> Result<PathBuf, String> {
        let mut path = self.challenge_dir(base_dir, challenge_id)?;

        match self {
            DataDir::Persistent(mining_address) => {
                path.push("persistent");
                path.push(mining_address);
            },
            DataDir::Ephemeral(mining_address) => {
                path.push("ephemeral");
                path.push(mining_address);
            },
            DataDir::Mnemonic(wallet) => {
                path.push("mnemonic");

                let mnemonic_hash = {
                    let mut hasher = DefaultHasher::new();
                    wallet.mnemonic.hash(&mut hasher);
                    hasher.finish()
                };
                path.push(mnemonic_hash.to_string());

                path.push(wallet.account.to_string());

                path.push(wallet.deriv_index.to_string());
            }
        }

        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Could not create challenge directory: {}", e))?;

        Ok(path)
    }

    pub fn save_challenge(&self, base_dir: &str, challenge: &ChallengeData) -> Result<(), String> {
        let mut path = self.challenge_dir(base_dir, &challenge.challenge_id)?;
        path.push(FILE_NAME_CHALLENGE);

        let challenge_json = serde_json::to_string(challenge)
            .map_err(|e| format!("Could not serialize challenge {}: {}", &challenge.challenge_id, e))?;

        std::fs::write(&path, challenge_json)
            .map_err(|e| format!("Could not write {}: {}", FILE_NAME_CHALLENGE, e))?;

        Ok(())
    }

    // Only saves the receipt, donation logic removed
    pub fn save_receipt(&self, base_dir: &str, challenge_id: &str, receipt: &serde_json::Value) -> Result<(), String> {
        let mut path = self.receipt_dir(base_dir, challenge_id)?;
        path.push(FILE_NAME_RECEIPT);

        let receipt_json = receipt.to_string();

        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Could not create {}: {}", FILE_NAME_RECEIPT, e))?;

        file.write_all(receipt_json.as_bytes())
            .map_err(|e| format!("Could not write to {}: {}", FILE_NAME_RECEIPT, e))?;

        file.sync_all()
            .map_err(|e| format!("Could not sync {}: {}", FILE_NAME_RECEIPT, e))?;

        // Donation file logic is intentionally removed here.

        Ok(())
    }

    // Saves a PendingSolution to the queue directory
    pub fn save_pending_solution(&self, base_dir: &str, solution: &PendingSolution) -> Result<(), String> {
        let mut path = PathBuf::from(base_dir);
        path.push("pending_submissions"); // Dedicated directory for the queue
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Could not create pending_submissions directory: {}", e))?;

        // Use a unique file name based on challenge, address, and nonce
        path.push(format!("{}_{}_{}.json", solution.address, normalize_challenge_id(&solution.challenge_id), solution.nonce));

        let solution_json = serde_json::to_string(solution)
            .map_err(|e| format!("Could not serialize pending solution: {}", e))?;

        std::fs::write(&path, solution_json)
            .map_err(|e| format!("Could not write pending solution file: {}", e))?;

        Ok(())
    }

    // Saves the temporary file indicating a solution was found but not queued/submitted
    pub fn save_found_solution(&self, base_dir: &str, challenge_id: &str, solution: &PendingSolution) -> Result<(), String> {
        let mut path = self.receipt_dir(base_dir, challenge_id)?; // Use receipt dir for local persistence
        path.push(FILE_NAME_FOUND_SOLUTION);

        let solution_json = serde_json::to_string(solution)
            .map_err(|e| format!("Could not serialize found solution: {}", e))?;

        // Use explicit file handling to guarantee persistence before returning success
        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Could not create {}: {}", FILE_NAME_FOUND_SOLUTION, e))?;

        file.write_all(solution_json.as_bytes())
            .map_err(|e| format!("Could not write to {}: {}", FILE_NAME_FOUND_SOLUTION, e))?;

        file.sync_all()
            .map_err(|e| format!("Could not sync {}: {}", FILE_NAME_FOUND_SOLUTION, e))?;

        Ok(())
    }

    // Removes the temporary file
    pub fn delete_found_solution(&self, base_dir: &str, challenge_id: &str) -> Result<(), String> {
        let mut path = self.receipt_dir(base_dir, challenge_id)?;
        path.push(FILE_NAME_FOUND_SOLUTION);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete {}: {}", FILE_NAME_FOUND_SOLUTION, e))?;
        }
        Ok(())
    }
}

// Checks if an address/challenge has a pending submission file in the queue dir
pub fn is_solution_pending_in_queue(base_dir: &str, address: &str, challenge_id: &str) -> Result<bool, String> {
    use std::path::PathBuf;

    let mut path = PathBuf::from(base_dir);
    path.push("pending_submissions");

    // Scan for any file that matches the address and challenge ID prefix
    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Some(filename) = entry.file_name().to_str() {
                // Check if the filename starts with the required prefix and is a JSON file
                // The filename format is: address_challenge_id_nonce.json
                if filename.starts_with(&format!("{}_{}_", address, normalize_challenge_id(&challenge_id))) && filename.ends_with(".json") {
                    return Ok(true);
                }
            }
        }
    }
    // If the directory doesn't exist or no matching file is found
    Ok(false)
}
