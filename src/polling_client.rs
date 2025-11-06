// src/polling_client.rs

use crate::api;
use crate::data_types::ManagerCommand;
use reqwest::blocking::Client;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

// Note: This duration is 5 minutes to prevent spamming the API when no new challenge is found.
const POLLING_INTERVAL_SECS: u64 = 5 * 60;

pub fn run_polling_client(
    client: Client,
    api_url: String,
    manager_tx: Sender<ManagerCommand>,
) -> Result<(), String> {
    println!("🌍 HTTP Polling thread started. Polling every {} seconds.", POLLING_INTERVAL_SECS);

    let mut current_challenge_id = String::new();

    loop {
        // Use a blocking API client to check the challenge status
        let result = api::fetch_challenge_status(&client, &api_url);

        match result {
            Ok(challenge_response) => {
                match challenge_response.code.as_str() {
                    "active" => {
                        // The 'challenge' field is guaranteed to be present when code is "active"
                        let active_params = challenge_response.challenge.unwrap();

                        if active_params.challenge_id != current_challenge_id {
                            println!("🌍 Poller found NEW active challenge: {}. Notifying manager.", active_params.challenge_id);

                            // Send the new challenge to the Manager thread
                            if manager_tx.send(ManagerCommand::NewChallenge(active_params.clone())).is_err() {
                                eprintln!("⚠️ Manager channel closed. Shutting down polling.");
                                return Ok(());
                            }
                            current_challenge_id = active_params.challenge_id;
                        }
                    }
                    "before" | "after" => {
                         // Non-active states, reset the tracked ID if a challenge was previously active
                         if !current_challenge_id.is_empty() {
                            println!("🌍 Challenge ended. Resetting ID.");
                            current_challenge_id.clear();
                        }
                    }
                    _ => {
                        eprintln!("⚠️ Poller received unexpected challenge code: {}", challenge_response.code);
                    }
                }
            }
            Err(e) => {
                eprintln!("⚠️ Poller API request failed: {}. Retrying after sleep.", e);
            }
        }

        // Sleep before the next poll
        thread::sleep(Duration::from_secs(POLLING_INTERVAL_SECS));
    }
}
