// src/data_types.rs

use serde_json;
use std::hash::{Hash, Hasher, DefaultHasher};
use std::path::PathBuf;
use std::ffi::OsStr;
use crate::api::ChallengeData;
use std::io::Write; // Added for file flushing

// NEW: Define a result type for the mining cycle
#[derive(Debug, PartialEq)]
pub enum MiningResult {
    FoundAndSubmitted((serde_json::Value, Option<String>)),
    AlreadySolved, // The solution was successfully submitted by someone else
    MiningFailed,  // General mining or submission error (e.g., hash not found, transient API error)
}

// --- DataDir Structures and Constants ---
pub const FILE_NAME_CHALLENGE: &str = "challenge.json";
pub const FILE_NAME_RECEIPT: &str = "receipt.json";
pub const FILE_NAME_DONATION: &str = "donation.txt";


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

impl<'a> DataDir<'a> {
    pub fn challenge_dir(&'a self, base_dir: &str, challenge_id: &str) -> Result<PathBuf, String> {
        let mut path = PathBuf::from(base_dir);
        path.push(challenge_id);
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

                path.push(&wallet.account.to_string());

                path.push(&wallet.deriv_index.to_string());
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

    pub fn save_receipt(&self, base_dir: &str, challenge_id: &str, receipt: &serde_json::Value, donation: &Option<String>) -> Result<(), String> {
        let mut path = self.receipt_dir(base_dir, challenge_id)?;
        path.push(FILE_NAME_RECEIPT);

        let receipt_json = receipt.to_string();

        // FIX: Use explicit file handling and sync to guarantee persistence.
        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Could not create {}: {}", FILE_NAME_RECEIPT, e))?;

        file.write_all(receipt_json.as_bytes())
            .map_err(|e| format!("Could not write to {}: {}", FILE_NAME_RECEIPT, e))?;

        // CRITICAL: Force the OS to write the data to disk now.
        file.sync_all()
            .map_err(|e| format!("Could not sync {}: {}", FILE_NAME_RECEIPT, e))?;

        if let Some(donation_id) = donation {
            path.pop();
            path.push(FILE_NAME_DONATION);

            std::fs::write(&path, &donation_id)
                .map_err(|e| format!("Could not write {}: {}", FILE_NAME_DONATION, e))?;
        }

        Ok(())
    }
}
