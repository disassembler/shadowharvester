// src/cli_commands.rs

use crate::cli::{Cli, Commands, ChallengeCommands, WalletCommands};
use crate::persistence::Persistence;
use crate::data_types::{ChallengeData, FailedSolution}; // FIX: Import FailedSolution
use std::path::PathBuf;
use std::fs;
use std::collections::{HashSet, HashMap}; // FIX: Import HashMap
use crate::data_types::SLED_KEY_FAILED_SOLUTION;

// Key prefixes for SLED to organize data
const SLED_KEY_CHALLENGE: &str = "challenge";
const SLED_KEY_RECEIPT: &str = "receipt";
const SLED_KEY_PENDING: &str = "pending";
const SLED_KEY_MNEMONIC_INDEX: &str = "mnemonic_index";
const SLED_DB_FILENAME: &str = "state.sled";

/// Handles all synchronous persistence-related commands (List, Import, Info, ReceiptInfo, PendingInfo, Wallet).
/// These commands run before the main application loop starts.
pub fn handle_sync_commands(cli: &Cli) -> Result<(), String> {

    // 1. Initialize Sled DB based on CLI data_dir
    let db_path = PathBuf::from(cli.data_dir.as_deref().unwrap_or("state")).join(SLED_DB_FILENAME);
    let persistence = Persistence::open(&db_path)
        .map_err(|e| format!("FATAL: Could not open Sled DB at {}: {}", db_path.display(), e))?;

    if let Some(command) = cli.command.clone() {
        match command {
            Commands::Challenge(cmd) => {
                match cmd {
                    ChallengeCommands::List => {
                        println!("\n==============================================");
                        println!("Stored Challenge IDs and Solutions");
                        println!("==============================================");

                        // 1. Calculate receipt counts for all challenges
                        let mut challenge_receipt_counts = HashMap::new();
                        let completed_prefix_base = format!("{}:", SLED_KEY_RECEIPT);

                        // Iterate over all receipts
                        for entry_result in persistence.db.scan_prefix(completed_prefix_base.as_bytes()) {
                            match entry_result {
                                Ok((key_ivec, _value_ivec)) => {
                                    let key = String::from_utf8_lossy(&key_ivec);
                                    // Key format: receipt:<ADDRESS>:<CHALLENGE_ID>
                                    let parts: Vec<&str> = key.split(':').collect();

                                    // parts[2] is CHALLENGE_ID
                                    if parts.len() == 3 {
                                        let challenge_id = parts[2].to_string();
                                        // Increment count for this challenge ID
                                        *challenge_receipt_counts.entry(challenge_id).or_insert(0) += 1;
                                    }
                                }
                                Err(e) => {
                                    // Handle iteration failure
                                    return Err(format!("Sled receipt iteration error: {}", e));
                                }
                            }
                        }

                        // 2. Iterate over stored challenge IDs and print with count
                        let mut found = false;
                        let iter = persistence.db.scan_prefix(format!("{}:", SLED_KEY_CHALLENGE).as_bytes());

                        for entry_result in iter {
                            match entry_result {
                                Ok((key_ivec, _value_ivec)) => {
                                    let key = String::from_utf8_lossy(&key_ivec);
                                    if let Some(challenge_id) = key.strip_prefix(format!("{}:", SLED_KEY_CHALLENGE).as_str()) {
                                        // Get the count, defaulting to 0
                                        let count = challenge_receipt_counts.get(challenge_id).unwrap_or(&0);
                                        // Print in a formatted way
                                        println!("{:<20} Solutions: {}", challenge_id, count);
                                        found = true;
                                    }
                                }
                                Err(e) => {
                                    // Handle iteration failure
                                    return Err(format!("Sled challenge iteration error: {}", e));
                                }
                            }
                        }

                        if !found {
                            println!("No challenges found in local state.");
                        }
                        println!("==============================================");
                        Ok(())
                    }
                    ChallengeCommands::Import { file } => {
                        let content = fs::read_to_string(&file)
                            .map_err(|e| format!("Failed to read challenge file {}: {}", file, e))?;
                        let challenge_data: ChallengeData = serde_json::from_str(&content)
                            .map_err(|e| format!("Failed to parse JSON file {}: {}", file, e))?;

                        let key = format!("{}:{}", SLED_KEY_CHALLENGE, challenge_data.challenge_id);
                        persistence.set(&key, &content)?;

                        println!("✅ Challenge '{}' imported successfully into Sled DB.", challenge_data.challenge_id);
                        Ok(())
                    }
                    ChallengeCommands::Info { id } => {
                        let key = format!("{}:{}", SLED_KEY_CHALLENGE, id);
                        match persistence.get(&key)? {
                            Some(json) => {
                                println!("\n==============================================");
                                println!("Challenge Details: {}", id);
                                println!("==============================================");
                                println!("{}", json);
                                Ok(())
                            }
                            None => {
                                Err(format!("Challenge ID '{}' not found in Sled DB.", id))
                            }
                        }
                    }
                    ChallengeCommands::Details { id } => {
                        let key = format!("{}:{}", SLED_KEY_CHALLENGE, id);
                        let json = persistence.get(&key)?.ok_or_else(|| format!("Challenge ID '{}' not found in Sled DB.", id))?;
                        let challenge_data: ChallengeData = serde_json::from_str(&json)
                            .map_err(|e| format!("Failed to deserialize challenge data: {}", e))?;

                        // --- Aggregation: FIX Logic to count SPECIFICALLY for this challenge ID ---

                        // Completed Key format: receipt:<ADDRESS>:<ID>
                        let completed_prefix_base = format!("{}:", SLED_KEY_RECEIPT);
                        let mut completed_count = 0;

                        // Iterate over all receipts and manually filter by CHALLENGE_ID
                        for entry_result in persistence.db.scan_prefix(completed_prefix_base.as_bytes()) {
                            if let Ok((key_ivec, _value_ivec)) = entry_result {
                                let key = String::from_utf8_lossy(&key_ivec);
                                // The key is receipt:<ADDRESS>:<CHALLENGE_ID>
                                let parts: Vec<&str> = key.split(':').collect();
                                // parts[2] is CHALLENGE_ID
                                if parts.len() == 3 && parts[2] == id {
                                    completed_count += 1;
                                }
                            }
                            else if let Err(e) = entry_result {
                                return Err(format!("Sled iteration error: {}", e));
                            }
                        }

                        // Pending Key format: pending:<ADDRESS>:<ID>:<NONCE>
                        let pending_prefix_base = format!("{}:", SLED_KEY_PENDING);
                        let mut pending_count = 0;

                        // Iterate over all pending solutions and manually filter by CHALLENGE_ID
                        for entry_result in persistence.db.scan_prefix(pending_prefix_base.as_bytes()) {
                            if let Ok((key_ivec, _value_ivec)) = entry_result {
                                let key = String::from_utf8_lossy(&key_ivec);
                                // The key is pending:<ADDRESS>:<CHALLENGE_ID>:<NONCE>
                                let parts: Vec<&str> = key.split(':').collect();
                                // parts[2] is CHALLENGE_ID
                                if parts.len() == 4 && parts[2] == id {
                                    pending_count += 1;
                                }
                            }
                            else if let Err(e) = entry_result {
                                return Err(format!("Sled iteration error: {}", e));
                            }
                        }

                        // --- Output ---
                        println!("\n==============================================");
                        println!("⛏️  Challenge Details: {}", id);
                        println!("==============================================");
                        println!("  ID:               {}", challenge_data.challenge_id);
                        println!("  Day:              {}", challenge_data.day);
                        println!("  Difficulty Mask:  {}", challenge_data.difficulty);
                        println!("  Submission Deadline: {}", challenge_data.latest_submission);
                        println!("  ROM Key:          {}", challenge_data.no_pre_mine_key);
                        println!("  Hash Input Hour:  {}", challenge_data.no_pre_mine_hour_str);
                        println!("----------------------------------------------");
                        println!("  Local Completed Solutions: {}", completed_count);
                        println!("  Local Pending Submissions: {}", pending_count);
                        println!("==============================================");

                        Ok(())
                    }
                    ChallengeCommands::ReceiptInfo { challenge_id, address } => {
                        // Key format: receipt:<ADDRESS>:<CHALLENGE_ID>
                        let key = format!("{}:{}:{}", SLED_KEY_RECEIPT, address, challenge_id);
                        match persistence.get(&key)? {
                            Some(json) => {
                                println!("\n==============================================");
                                println!("Receipt Info: {} for {}", challenge_id, address);
                                println!("==============================================");
                                println!("{}", json);
                                Ok(())
                            }
                            None => {
                                Err(format!("Receipt not found for Challenge ID '{}' and Address '{}'.", challenge_id, address))
                            }
                        }
                    }
                    ChallengeCommands::PendingInfo { challenge_id, address, nonce } => {
                        // Key format: pending:<ADDRESS>:<CHALLENGE_ID>:<NONCE>
                        let key = format!("{}:{}:{}:{}", SLED_KEY_PENDING, address, challenge_id, nonce);

                        match persistence.get(&key)? {
                            Some(json) => {
                                println!("\n==============================================");
                                println!("Pending Solution: {} for {}", nonce, address);
                                println!("==============================================");
                                println!("{}", json);
                                Ok(())
                            }
                            None => {
                                Err(format!("Pending solution not found for Nonce '{}', Challenge '{}', and Address '{}'.", nonce, challenge_id, address))
                            }
                        }
                    }
                    ChallengeCommands::Errors => {
                        println!("\n==============================================");
                        println!("Stored Permanent Submission Errors");
                        println!("==============================================");

                        let prefix = format!("{}:", SLED_KEY_FAILED_SOLUTION);
                        let mut found = false;

                        // Scan Sled for the failed solution prefix
                        for entry_result in persistence.db.scan_prefix(prefix.as_bytes()) {
                            match entry_result {
                                Ok((_key_ivec, value_ivec)) => {
                                    let error_json = String::from_utf8_lossy(&value_ivec);

                                    // Print the entire stored JSON object
                                    println!("{}", error_json);
                                    println!("----------------------------------------------");
                                    found = true;
                                }
                                Err(e) => {
                                    return Err(format!("Sled iteration error while dumping errors: {}", e));
                                }
                            }
                        }

                        if !found {
                            println!("No permanent submission errors found in local state.");
                        }
                        println!("==============================================");
                        Ok(())
                    }
                    ChallengeCommands::Hash { challenge_id, address } => {
                        // Import necessary library functions
                        use shadow_harvester_lib::{Rom, RomGenerationType, hash};

                        const MB: usize = 1024 * 1024;
                        const GB: usize = 1024 * MB;
                        const NONCE_HEX_LENGTH: usize = 16;
                        const NB_LOOPS: u32 = 8;
                        const NB_INSTRS: u32 = 256;

                        let source: &str;
                        let preimage_str: String;
                        let stored_hash: Option<String>; // Hash found in the FailedSolution record

                        let key_challenge = format!("{}:{}", SLED_KEY_CHALLENGE, challenge_id);
                        let key_receipt = format!("{}:{}:{}", SLED_KEY_RECEIPT, address, challenge_id);
                        let prefix_error = format!("{}:{}:{}:", SLED_KEY_FAILED_SOLUTION, address, challenge_id);


                        // 1. Get Challenge Data (needed for ROM and preimage)
                        let challenge_json = persistence.get(&key_challenge)?
                            .ok_or_else(|| format!("Challenge ID '{}' not found in Sled DB.", challenge_id))?;
                        let challenge_data: ChallengeData = serde_json::from_str(&challenge_json)
                            .map_err(|e| format!("Failed to deserialize challenge data: {}", e))?;

                        // 2. Try to get Receipt or Error Record
                        if let Some(receipt_json_value) = persistence.get(&key_receipt)? {
                            // --- FOUND RECEIPT ---
                            source = "Receipt (Successful Submission)";
                            let full_receipt: serde_json::Value = serde_json::from_str(&receipt_json_value)
                                .map_err(|e| format!("Failed to parse receipt JSON from Sled: {}", e))?;

                            preimage_str = full_receipt.get("preimage")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .ok_or_else(|| "Receipt JSON missing 'preimage' string field.".to_string())?;

                            stored_hash = None; // Receipt does not store the hash output
                        }
                        else if let Some(error_entry) = persistence.db.scan_prefix(prefix_error.as_bytes()).next().and_then(|r| r.ok()) {
                            // --- FOUND ERROR RECORD ---
                            source = "Error Record (Non-Recoverable Failure)";
                            let error_json = String::from_utf8_lossy(&error_entry.1);

                            let failed_solution: FailedSolution = serde_json::from_str(&error_json)
                                .map_err(|e| format!("Failed to deserialize Error JSON: {}", e))?;

                            preimage_str = failed_solution.preimage;
                            stored_hash = Some(failed_solution.hash_output);
                        }
                        else {
                            return Err(format!("Neither a Receipt nor a permanent Error Record found for challenge '{}' and address '{}'.", challenge_id, address));
                        }

                        let nonce_hex = preimage_str.get(0..NONCE_HEX_LENGTH)
                            .ok_or_else(|| "Preimage is too short to extract 16-char nonce.".to_string())?;

                        // 3. Initialize ROM
                        let rom = Rom::new(
                            challenge_data.no_pre_mine_key.as_bytes(),
                            RomGenerationType::TwoStep {
                                pre_size: 16 * MB,
                                mixing_numbers: 4,
                            },
                            GB,
                        );

                        // 4. Compute the Hash
                        let h = hash(preimage_str.as_bytes(), &rom, NB_LOOPS, NB_INSTRS);


                        // 5. Output Result
                        println!("\n==============================================");
                        println!("Hash Verification for Challenge: {}", challenge_id);
                        println!("  Source: {}", source);
                        println!("==============================================");
                        println!("Address: {}", address);
                        println!("Nonce: {}", nonce_hex);
                        println!("Difficulty Mask: {}", challenge_data.difficulty);
                        println!("Reconstructed Preimage (Full): {}", preimage_str);
                        println!("----------------------------------------------");
                        println!("ROM Key: {}", challenge_data.no_pre_mine_key);
                        println!("ROM Digest: {}", hex::encode(rom.digest.0));
                        println!("Computed Final Hash (Blake2b-512):");
                        println!("{}", hex::encode(h));

                        if let Some(stored_hash) = stored_hash {
                            println!("----------------------------------------------");
                            println!("Stored Hash (from Error Record):");
                            println!("{}", stored_hash);
                            if stored_hash == hex::encode(h) {
                                println!("✅ Stored Hash MATCHES Computed Hash.");
                            } else {
                                println!("❌ Stored Hash DOES NOT MATCH Computed Hash. Logic error or data corruption.");
                            }
                        }
                        println!("==============================================");

                        Ok(())
                    }
                }
            }
            Commands::Wallet(cmd) => {
                match cmd {
                    WalletCommands::List => {
                        println!("\n==============================================");
                        println!("Stored Wallet Identifiers (Hash:Account)");
                        println!("==============================================");

                        let mut identifiers = HashSet::new();
                        let prefix = format!("{}:", SLED_KEY_MNEMONIC_INDEX);

                        let iter = persistence.db.scan_prefix(prefix.as_bytes());

                        for entry_result in iter {
                            match entry_result {
                                Ok((key_ivec, _value_ivec)) => {
                                    let key = String::from_utf8_lossy(&key_ivec);

                                    // Key format: mnemonic_index:<HASH>:<ACCOUNT>:<INDEX>
                                    let parts: Vec<&str> = key.split(':').collect();

                                    // Need to confirm key starts with prefix and has enough parts
                                    if parts.len() >= 3 && parts[0] == SLED_KEY_MNEMONIC_INDEX {
                                        // Identifier is HASH:ACCOUNT
                                        let identifier = format!("{}:{}", parts[1], parts[2]);
                                        identifiers.insert(identifier);
                                    }
                                }
                                Err(e) => {
                                    return Err(format!("Sled iteration error: {}", e));
                                }
                            }
                        }

                        if identifiers.is_empty() {
                            println!("No wallet identifiers found in local state.");
                        } else {
                            for id in identifiers {
                                println!("{}", id);
                            }
                        }
                        println!("==============================================");
                        Ok(())
                    }

                    WalletCommands::Addresses { wallet } => {
                        let parts: Vec<&str> = wallet.split(':').collect();
                        if parts.len() != 2 {
                             return Err("Invalid wallet format. Expected <Hash>:<AccountIndex> (e.g., 16886378742194182050:0)".to_string());
                        }
                        let (hash, account) = (parts[0], parts[1]);

                        println!("\n==============================================");
                        println!("Addresses for Wallet: {} (Account {})", hash, account);
                        println!("==============================================");

                        let prefix = format!("{}:{}:{}:", SLED_KEY_MNEMONIC_INDEX, hash, account);
                        let mut addresses_found = false;

                        let iter = persistence.db.scan_prefix(prefix.as_bytes());

                        for entry_result in iter { // Iterates over Result<(IVec, IVec), E>
                            match entry_result {
                                Ok((key_ivec, value_ivec)) => {
                                    let key = String::from_utf8_lossy(&key_ivec);
                                    let address = String::from_utf8_lossy(&value_ivec);

                                    // Key format: mnemonic_index:HASH:ACCOUNT:INDEX
                                    let key_parts: Vec<&str> = key.split(':').collect();

                                    // We know length must be 4 based on key format
                                    if key_parts.len() == 4 {
                                        let index = key_parts[3];

                                        // Output format: <INDEX>:<ADDRESS>
                                        println!("{}: {}", index, address);
                                        addresses_found = true;
                                    }
                                }
                                Err(e) => {
                                    return Err(format!("Sled iteration error: {}", e));
                                }
                            }
                        }

                        if !addresses_found {
                            println!("No addresses found for this wallet identifier.");
                        }
                        println!("==============================================");
                        Ok(())
                    }

                    WalletCommands::ListChallenges { address } => {
                        println!("\n==============================================");
                        println!("Completed Challenges for Address: {}", address);
                        println!("==============================================");

                        // Key format: receipt:<ADDRESS>:<ID>
                        let prefix = format!("{}:{}:", SLED_KEY_RECEIPT, address);
                        let mut challenges_found = false;

                        let iter = persistence.db.scan_prefix(prefix.as_bytes());

                        for entry_result in iter {
                            if let Ok((key_ivec, _value_ivec)) = entry_result {
                                let key = String::from_utf8_lossy(&key_ivec);
                                // Key format: receipt:<ADDRESS>:<CHALLENGE_ID>
                                let parts: Vec<&str> = key.split(':').collect();

                                if parts.len() == 3 && parts[0] == SLED_KEY_RECEIPT {
                                    println!("{}", parts[2]); // parts[2] is the CHALLENGE_ID
                                    challenges_found = true;
                                }
                            } else {
                                // If the iteration itself fails, return the error.
                                return Err(format!("Sled iteration error: {}", entry_result.unwrap_err()));
                            }
                        }

                        if !challenges_found {
                            println!("No completed challenges found for this address.");
                        }
                        println!("==============================================");
                        Ok(())
                    }
                }
            }
            _ => return Err("Invalid command passed to handle_persistence_commands.".to_string()),
        }
    } else {
        // This case should not be reachable if logic in main.rs is correct,
        // but acts as a fallback.
        Err("Invalid command passed to handle_persistence_commands.".to_string())
    }?; // Use ? to propagate the error from the match block

    persistence.close().map_err(|e| format!("Failed to close Sled DB: {}", e))?;
    Ok(())
}
