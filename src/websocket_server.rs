// src/websocket_server.rs

use crate::data_types::{ChallengeResponse, ManagerCommand, WebSocketCommand, PendingSolution};
use std::sync::mpsc::{Sender, Receiver, TryRecvError};
use std::net::{TcpListener, SocketAddr, TcpStream};
use tungstenite::{accept, Message, Error as TungsteniteError};
use serde_json::{self, Value};
use std::io::ErrorKind;
use std::time::Duration;
use std::thread;


/// Starts a simple blocking WebSocket server to listen for new challenge posts.
/// Challenges received are forwarded to the Manager thread via MPSC.
pub fn start_server(
    manager_tx: Sender<ManagerCommand>,
    solution_rx: Receiver<WebSocketCommand>, // <-- NEW: Solution Receiver
    port: u16
) -> Result<(), String> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(&addr)
        .map_err(|e| format!("Failed to bind WebSocket server to {}: {}", addr, e))?;

    println!("🌐 WebSocket Server listening on ws://{}.", addr);

    // Main loop waits for a TCP connection
    loop {
        // Use a 50ms sleep to prevent 100% CPU usage while spinning and checking the solution channel
        thread::sleep(Duration::from_millis(50));

        let stream = match listener.set_nonblocking(true) {
            Ok(_) => match listener.accept() {
                Ok((s, _)) => Ok(s),
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    // Check for pending solutions while waiting for a connection
                    if let Err(e) = check_for_pending_solutions_on_disconnect(&solution_rx) {
                        return Err(e); // Fatal if the core channel disconnects
                    }
                    continue;
                }
                Err(e) => Err(format!("Incoming TCP connection failed: {}", e)),
            },
            Err(e) => Err(format!("Failed to set nonblocking listener: {}", e)),
        };

        let stream: TcpStream = stream?;
        // FIX: Set stream to NON-BLOCKING so the inner loop can check solution_rx
        stream.set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking stream: {}", e))?;


        match accept(stream) {
            Ok(mut websocket) => {
                println!("🌐 WebSocket client connected. Awaiting challenge posts...");

                // FIX: Immediately tell the Manager to sweep for pending solutions
                if manager_tx.send(ManagerCommand::SweepPendingSolutions).is_err() {
                    eprintln!("⚠️ Manager channel closed on Sweep request. Connection aborting.");
                    // Break the outer loop and return Err
                    break Err("Manager channel closed during WS connection attempt.".to_string());
                }

                // Inner loop handles open connection
                let _ = loop { // FIX: Use let _ = loop to consume the loop's result value
                    // Check for incoming challenges (from client)
                    let client_msg_result = websocket.read();

                    // Check for outgoing solutions (from Rust core)
                    match solution_rx.try_recv() {
                        Ok(WebSocketCommand::SubmitSolution(solution)) => {
                            send_solution_to_client(&mut websocket, solution);
                        }
                        Err(TryRecvError::Empty) => { /* Continue */ }
                        Err(TryRecvError::Disconnected) => {
                            eprintln!("❌ Core solution channel closed. Shutting down WS server.");
                            // Break the outer loop and return Err
                            break Err("Core solution channel closed.".to_string());
                        }
                    }

                    // Handle incoming client message
                    match client_msg_result {
                        Ok(msg) => {
                            if msg.is_text() {
                                let text = msg.to_text().unwrap();

                                match handle_incoming_challenge(text, &manager_tx) {
                                    Ok(_) => {
                                        // FIX: Wrap the acknowledgment in valid JSON
                                        let ack_json = serde_json::json!({
                                            "type": "ack",
                                            "status": "Challenge accepted."
                                        }).to_string();
                                        let _ = websocket.send(Message::Text(ack_json.into()));
                                    }
                                    Err(e) => {
                                        eprintln!("⚠️ WS Challenge Handling Error: {}", e);
                                        let _ = websocket.send(Message::Text(format!("Error: {}", e).into()));
                                    }
                                }
                            }
                        }
                        // FIX: Handle the non-blocking I/O error (no data from client)
                        Err(TungsteniteError::Io(ref io_err)) if io_err.kind() == ErrorKind::WouldBlock => {
                            /* Continue */
                        }
                        Err(e) => {
                            // Client disconnected or error occurred
                            handle_websocket_disconnect(e);
                            // FIX: Provide Ok(()) to the break statement
                            break Ok(());
                        }
                    }

                    // FIX: Add a small sleep here to prevent the thread from spinning at 100% CPU
                    thread::sleep(Duration::from_millis(5));
                };
            }
            Err(e) => {
                eprintln!("⚠️ Failed to establish WebSocket connection: {}", e);
            }
        }
    }
}

/// Helper to ensure no solutions are missed while no client is connected
fn check_for_pending_solutions_on_disconnect(solution_rx: &Receiver<WebSocketCommand>) -> Result<(), String> {
    match solution_rx.try_recv() {
        Ok(WebSocketCommand::SubmitSolution(solution)) => {
            // NOTE: Since the solution is received here, it has already been consumed from the MPSC buffer.
            // The logic would require persisting it to SLED in the WS server if the client is not connected,
            // but the Submitter thread already does this (by keeping it in the pending queue).
            let pending_key = format!("{}:{}", solution.address, solution.challenge_id);
            println!("⚠️ Found solution for {} in queue, but no WebSocket client is connected. The solution will be resent immediately upon client reconnection.", pending_key);
            // Since this is just a loss of the current MPSC send, we let the Submitter handle retries or rely on the client reconnecting.
            Ok(())
        }
        Err(TryRecvError::Disconnected) => {
            Err("Core solution channel closed.".to_string())
        }
        _ => Ok(())
    }
}

fn send_solution_to_client(websocket: &mut tungstenite::WebSocket<TcpStream>, solution: PendingSolution) {
    let payload = serde_json::to_string(&solution)
        .map_err(|e| format!("Failed to serialize solution: {}", e))
        .unwrap_or_else(|e| {
            eprintln!("Fatal: Solution serialization failed: {}", e);
            "{}".to_string()
        });

    let solution_value: Value = serde_json::from_str(&payload).unwrap_or_default();

    // Prefix the message so the Tampermonkey script knows it's a solution and not a challenge
    // We send the raw payload string in the 'data' field.
    let final_payload = serde_json::json!({
        "type": "solution",
        "data": solution_value,
    }).to_string();

    match websocket.send(Message::Text(final_payload.into())) {
        Ok(_) => println!("🚀 Sent solution for {} to client via WebSocket.", solution.challenge_id),
        Err(e) => eprintln!("⚠️ Failed to send solution over WebSocket: {}", e),
    }
}

fn handle_websocket_disconnect(e: TungsteniteError) {
    // ... (logic remains the same)
    match e {
        TungsteniteError::ConnectionClosed | TungsteniteError::Protocol(_) | TungsteniteError::Url(_) => {
            println!("🌐 WebSocket client disconnected or protocol error: {}", e);
        }
        TungsteniteError::Io(ref io_err) => {
            match io_err.kind() {
                ErrorKind::ConnectionReset | ErrorKind::BrokenPipe => {
                    println!("🌐 WebSocket client disconnected gracefully (IO error: {}).", io_err);
                }
                _ => {
                    eprintln!("⚠️ WebSocket read IO error: {}", io_err);
                }
            }
        }
        _ => {
            eprintln!("⚠️ WebSocket read error: {}", e);
        }
    }
}

fn handle_incoming_challenge(json_payload: &str, manager_tx: &Sender<ManagerCommand>) -> Result<(), String> {
    // ... (logic remains the same)
    let challenge_response: ChallengeResponse = serde_json::from_str(json_payload)
        .map_err(|e| format!("Failed to parse JSON payload as ChallengeResponse: {}", e))?;

    match challenge_response.code.as_str() {
        "active" => {
            if let Some(challenge_data) = challenge_response.challenge {
                println!("🌐 Received new ACTIVE challenge {} via WebSocket. Forwarding to Manager.", challenge_data.challenge_id);
                manager_tx.send(ManagerCommand::NewChallenge(challenge_data))
                    .map_err(|_| "Manager channel closed (Manager thread crashed or shut down).".to_string())?;
                Ok(())
            } else {
                Err("Received 'active' status but challenge data is missing.".to_string())
            }
        }
        "before" => Err(format!("Received challenge status 'before' (starts at: {:?}). Ignoring.", challenge_response.starts_at)),
        "after" => Err("Received challenge status 'after'. Mining period has ended. Ignoring.".to_string()),
        _ => Err(format!("Received unknown challenge status code: {}", challenge_response.code)),
    }
}
