// src/migrate.rs

use crate::persistence::Persistence;
use crate::data_types::{FILE_NAME_RECEIPT, FILE_NAME_CHALLENGE, ChallengeData, PendingSolution};
use std::path::{Path, PathBuf};
use std::fs;
use serde_json::Value; // Needed to parse receipt JSON

// Key prefixes for SLED to organize data
const SLED_KEY_RECEIPT: &str = "receipt";
const SLED_KEY_CHALLENGE: &str = "challenge";
const SLED_KEY_PENDING: &str = "pending";
const SLED_KEY_MNEMONIC_INDEX: &str = "mnemonic_index"; // Key for mnemonic index state
const NONCE_HEX_LENGTH: usize = 16; // 64 bits = 16 hex characters

/// Constructs the unique key used to store a receipt in Sled.
/// Format: receipt:<ADDRESS>:<CHALLENGE_ID>
fn get_sled_receipt_key(address: &str, challenge_id: &str) -> String {
    format!("{}:{}:{}", SLED_KEY_RECEIPT, address, challenge_id)
}

/// Constructs the unique key used to store a pending solution in Sled.
/// Format: pending:<ADDRESS>:<CHALLENGE_ID>:<NONCE>
fn get_sled_pending_key(solution: &PendingSolution) -> String {
    format!("{}:{}:{}:{}", SLED_KEY_PENDING, solution.address, solution.challenge_id, solution.nonce)
}

/// Helper to extract the Cardano address from the 'preimage' string in the receipt JSON.
fn extract_address_from_preimage(receipt_json: &str) -> Result<String, String> {
    let parsed: Value = serde_json::from_str(receipt_json)
        .map_err(|e| format!("Failed to parse receipt JSON: {}", e))?;

    let preimage = parsed["preimage"].as_str()
        .ok_or_else(|| "Receipt JSON missing 'preimage' field.".to_string())?;

    // The preimage structure is [NONCE_HEX (16 chars)][ADDRESS][CHALLENGE_ID]...
    // The address starts immediately after the 16-char nonce.
    let address_start_index = NONCE_HEX_LENGTH;

    // The address ends when the Challenge ID (which starts with **) begins.
    if let Some(address_end_index) = preimage[address_start_index..].find("**") {
        let address_end_index = address_start_index + address_end_index;

        Ok(preimage[address_start_index..address_end_index].to_string())
    } else {
        Err("Could not find Challenge ID marker ('**') in preimage to delimit address.".to_string())
    }
}


/// Helper function to extract and store mnemonic path info, ignoring the Challenge ID.
fn store_mnemonic_path_info(path: &Path, persistence: &Persistence, receipt_content: &str) -> Result<(), String> {
    // 1. Get the definitive Cardano address from the receipt content.
    let known_address = extract_address_from_preimage(receipt_content)?;

    // We expect the path to be like: .../<hash>/<account>/<index>/receipt.json

    let mut components = path.components().rev().skip(1); // Skip 'receipt.json'

    // 2. Get Derivation Index (INDEX)
    let deriv_index_str = components.next().and_then(|c| c.as_os_str().to_str());
    let deriv_index: u32 = deriv_index_str
        .ok_or_else(|| "Missing derivation index in path".to_string())?
        .parse()
        .map_err(|_| "Failed to parse derivation index as u32".to_string())?;

    // 3. Get Account Index (ACCOUNT)
    let account_index_str = components.next().and_then(|c| c.as_os_str().to_str());
    let account_index: u32 = account_index_str
        .ok_or_else(|| "Missing account index in path".to_string())?
        .parse()
        .map_err(|_| "Failed to parse account index as u32".to_string())?;

    // 4. Get Mnemonic Hash (HASH)
    let mnemonic_hash = components.next().and_then(|c| c.as_os_str().to_str())
        .ok_or_else(|| "Missing mnemonic hash in path".to_string())?;

    // SLED Key format (Non-duplicative, Challenge-Agnostic): mnemonic_index:<HASH>:<ACCOUNT>:<INDEX>
    let key = format!(
        "{}:{}:{}:{}",
        SLED_KEY_MNEMONIC_INDEX,
        mnemonic_hash,
        account_index,
        deriv_index
    );

    // Value: The Cardano Address (known_address)
    let address_value = known_address;

    // Check if the key already exists before inserting (to prevent duplicates across challenges)
    if persistence.get(&key)?.is_none() {
         persistence.set(&key, &address_value)?;
         println!("    -> Saved new wallet state: {}:{}:{} -> {}", mnemonic_hash, account_index, deriv_index, address_value);
    }
    // If it exists, we just skip it silently, as requested.

    Ok(())
}


// Recursive helper to find and migrate receipt.json files
fn migrate_receipts_recursively(
    path: &Path,
    challenge_id: &str,
    persistence: &Persistence,
    total_receipts: &mut u32
) -> Result<(), String> {
    if path.is_file() {
        if path.file_name().and_then(|s| s.to_str()) == Some(FILE_NAME_RECEIPT) {
            // Found a receipt file.
            let address_identifier = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str());

            if let Some(address) = address_identifier {
                // Attempt to read the file content
                if let Ok(receipt_content) = fs::read_to_string(path) {

                    // Construct the Sled key
                    let key = get_sled_receipt_key(address, challenge_id);

                    // Store the receipt
                    if persistence.set(&key, &receipt_content).is_ok() {
                        *total_receipts += 1;

                        // Check if this receipt is from the mnemonic path for further state storage
                        if path.to_string_lossy().contains("/mnemonic/") {
                            // If the logic fails inside, it will be skipped silently (as requested).
                            let _ = store_mnemonic_path_info(path, persistence, &receipt_content);
                        }
                    }
                }
                // If fs::read_to_string fails, or persistence.set fails, we skip silently.
            }
        }
        return Ok(());
    }

    if path.is_dir() {
        // Handle fs::read_dir result before iterating
        match fs::read_dir(path) {
            Ok(read_dir) => {
                for entry in read_dir.filter_map(|e| e.ok()) {
                    // Recurse into subdirectories (necessary for the nested Mnemonic path structure)
                    if let Err(e) = migrate_receipts_recursively(&entry.path(), challenge_id, persistence, total_receipts) {
                        // Only return error if the recursive call failed with an unexpected error
                        eprintln!("⚠️ Warning: Recursive migration failure: {}", e);
                    }
                }
            }
            Err(e) => {
                return Err(format!("Failed to read directory {}: {}", path.display(), e));
            }
        }
    }

    Ok(())
}

/// Processes the separate /pending_submissions folder and migrates solutions into Sled.
fn migrate_pending_submissions(old_data_dir: &str, persistence: &Persistence) -> Result<u32, String> {
    let pending_path = Path::new(old_data_dir).join("pending_submissions");
    if !pending_path.is_dir() {
        return Ok(0); // Directory doesn't exist, nothing to do
    }

    let mut count = 0;

    for entry in fs::read_dir(&pending_path)
        .map_err(|e| format!("Failed to read pending submissions directory: {}", e))?
        .filter_map(|e| e.ok())
    {
        let file_path = entry.path();
        if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
            let content = fs::read_to_string(&file_path)
                .map_err(|e| format!("Failed to read pending solution file: {}", e))?;

            let solution: PendingSolution = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse pending solution JSON: {}", e))?;

            // Store the full PendingSolution JSON in Sled
            let key = get_sled_pending_key(&solution);
            persistence.set(&key, &content)?;

            count += 1;
        }
    }

    Ok(count)
}


/// Runs the state migration from the old file-based structure to the new Sled database.
pub fn run_migration(old_data_dir: &str, new_data_dir: &str) -> Result<(), String> {
    println!("\n==============================================");
    println!("⚙️ Starting state migration...");
    println!("  Source (File System): {}", old_data_dir);
    println!("  Destination (Sled DB): {}", new_data_dir);
    println!("==============================================");

    // 1. Initialize SLED DB
    let sled_path = PathBuf::from(new_data_dir).join("state.sled"); // Using hardcoded sled filename
    let persistence = Persistence::open(&sled_path)
        .map_err(|e| format!("FATAL: Could not initialize Sled DB at {:?}: {}", sled_path, e))?;

    let old_base_path = Path::new(old_data_dir);

    // --- Phase 1: Migrate Receipts and Challenges ---
    let mut total_receipts = 0;
    for challenge_entry in fs::read_dir(old_base_path)
        .map_err(|e| format!("Failed to read old data directory: {}", e))?
        .filter_map(|e| e.ok())
    {
        let challenge_path = challenge_entry.path();
        if !challenge_path.is_dir() { continue; }

        let challenge_id = challenge_path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        if challenge_id.is_empty() { continue; }

        // Store CHALLENGE.JSON
        let challenge_file_path = challenge_path.join(FILE_NAME_CHALLENGE);
        if let Ok(content) = fs::read_to_string(&challenge_file_path) {
            if let Ok(data) = serde_json::from_str::<ChallengeData>(&content) {
                let key = format!("{}:{}", SLED_KEY_CHALLENGE, data.challenge_id);
                persistence.set(&key, &content)?;
                println!("  [Challenge] Saved challenge data for: {}", challenge_id);
            }
        }

        // Recursively find and store all receipts
        for mode in ["persistent", "ephemeral", "mnemonic"].iter() {
            let mode_path = challenge_path.join(mode);
            if !mode_path.is_dir() { continue; }
            // Handle fs::read_dir result before iterating
            match fs::read_dir(&mode_path) {
                Ok(read_dir) => {
                    for receipt_result in read_dir.filter_map(|e| e.ok()) {
                        if let Err(e) = migrate_receipts_recursively(&receipt_result.path(), &challenge_id, &persistence, &mut total_receipts) {
                            eprintln!("⚠️ Warning: Failed processing path {}: {}", receipt_result.path().display(), e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ Warning: Failed reading mode directory {}: {}", mode_path.display(), e);
                }
            }
        }
    }

    // --- Phase 2: Migrate Pending Solutions Queue ---
    let total_pending = migrate_pending_submissions(old_data_dir, &persistence)?;

    // 6. Close DB and finalize
    persistence.close().map_err(|e| format!("Failed to close Sled DB: {}", e))?;

    println!("\n✅ Migration SUCCESSFUL.");
    println!("  Total challenge/receipts migrated: {}", total_receipts);
    println!("  Total pending solutions migrated: {}", total_pending);

    Ok(())
}
