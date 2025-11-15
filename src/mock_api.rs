use warp::{Filter, Rejection, Reply, http::StatusCode};
use serde_json::json;
use std::thread;
use std::sync::{Arc, RwLock};
use tokio::runtime;
use tokio::time::{self, Duration as TokioDuration};
use chrono::{Utc, Duration, DateTime};

// --- MOCK CONSTANTS ---
const MOCK_REGISTRATION_MESSAGE: &str = "MOCK_REGISTRATION_MESSAGE_FOR_TESTS";
const MOCK_DIFFICULTY: &str = "000FFFFF";
const MOCK_NO_PRE_MINE: &str = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011";
const MOCK_NO_PRE_MINE_HOUR: &str = "416194743";

// --- STATE STRUCTURES ---

#[derive(Debug, Clone)]
struct ChallengeState {
    challenge_id: String,
    difficulty: String,
    no_pre_mine: String,
    no_pre_mine_hour: String,
    issued_at: String,
    latest_submission: String,
    challenge_number: u32,
}

// Global shared state types
type SharedState = Arc<RwLock<ChallengeState>>;
type MockReceipts = Arc<RwLock<u32>>;

fn initial_challenge_state() -> ChallengeState {
    ChallengeState {
        challenge_id: "TESTC01".to_string(),
        difficulty: MOCK_DIFFICULTY.to_string(),
        no_pre_mine: MOCK_NO_PRE_MINE.to_string(),
        no_pre_mine_hour: MOCK_NO_PRE_MINE_HOUR.to_string(),
        issued_at: Utc::now().to_rfc3339(),
        latest_submission: (Utc::now() + Duration::seconds(30)).to_rfc3339(), // Initial challenge lasts 30s
        challenge_number: 1,
    }
}

// --- FILTER HELPERS ---

// Filter to provide the shared challenge state
fn with_state(state: SharedState) -> impl Filter<Extract = (SharedState,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

// Filter to provide the shared receipts state
fn with_receipts(receipts: MockReceipts) -> impl Filter<Extract = (MockReceipts,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || receipts.clone())
}

// --- UPDATER TASK ---

async fn challenge_updater_task(state: SharedState) {
    let mut interval = time::interval(TokioDuration::from_secs(30));

    let mut challenge_counter: u32 = state.read().unwrap().challenge_number;

    // Skip the first tick
    interval.tick().await;

    loop {
        interval.tick().await;

        // --- 2-CYCLE TEST LOGIC: Stop after TESTC02 ---
        // Challenge 1 is set at start.
        // First tick sets Challenge 2 (TESTC02).
        if challenge_counter >= 2 {
            // Second tick (the third cycle overall): EXPIRE IT.
            let mut writable_state = state.write().unwrap();

            // Set the submission deadline far in the past.
            let expired_time = Utc::now() - Duration::minutes(5);

            // NOTE: Keep the challenge ID as the last issued one (TESTC02) but mark it expired.
            writable_state.latest_submission = expired_time.to_rfc3339();

            println!("\n🛑 [Mock API] Challenge **EXPIRED**:");
            println!("   ID: {} | Deadline set to: {}\n", writable_state.challenge_id, writable_state.latest_submission);

            // If you want it to run indefinitely, remove the 'continue' and let it issue the next challenge.
            continue;
        }

        challenge_counter += 1;

        let now = Utc::now();
        let issued_at = now;
        let latest_submission = now + Duration::seconds(30);

        let new_id = format!("TESTC{:02}", challenge_counter);

        // Acquire the write lock and update the state
        let mut writable_state = state.write().unwrap();
        writable_state.challenge_id = new_id;
        writable_state.challenge_number = challenge_counter;
        writable_state.issued_at = issued_at.to_rfc3339();
        writable_state.latest_submission = latest_submission.to_rfc3339();

        println!("\n⏰ [Mock API] New Challenge Issued:");
        println!("   ID: {} | Expires: {}\n", writable_state.challenge_id, writable_state.latest_submission);
    }
}


// --- MOCK ENDPOINT HANDLERS ---

// GET /api/TandC/1-0
async fn tandc_handler() -> Result<impl Reply, Rejection> {
    Ok(warp::reply::json(&json!({
        "version": "MOCK-1.0",
        "content": "Mock Terms & Conditions for local testing.",
        "message": MOCK_REGISTRATION_MESSAGE,
    })))
}

// GET /api/challenge
async fn challenge_status_handler(state: SharedState) -> Result<impl Reply, Rejection> {
    let readable_state = state.read().unwrap();

    let end_time_str = readable_state.latest_submission.clone();

    // Check if the current time is past the deadline
    let deadline: DateTime<Utc> = end_time_str.parse::<DateTime<Utc>>()
        .unwrap_or_else(|_| panic!("Failed to parse deadline time in handler."));

    let is_active = Utc::now() < deadline;
    let status_code = if is_active { "active" } else { "inactive" };

    // Calculate next start time
    let next_start = if is_active {
        (deadline + Duration::seconds(30)).to_rfc3339()
    } else {
        end_time_str.clone()
    };

    Ok(warp::reply::json(&json!({
        "code": status_code, // DYNAMIC STATUS
        "challenge": {
            "challenge_id": readable_state.challenge_id,
            "difficulty": readable_state.difficulty,
            "no_pre_mine": readable_state.no_pre_mine,
            "no_pre_mine_hour": readable_state.no_pre_mine_hour,
            "latest_submission": end_time_str,
            "challenge_number": readable_state.challenge_number,
            "day": readable_state.challenge_number,
            "issued_at": readable_state.issued_at,
        },
        "mining_period_ends": end_time_str,
        "max_day": 1,
        "total_challenges": readable_state.challenge_number,
        "current_day": readable_state.challenge_number,
        "next_challenge_starts_at": next_start,
    })))
}

// POST /api/register/{address}/{signature}/{pubkey}
async fn register_handler(
    _address: String,
    _signature: String,
    _pubkey: String,
) -> Result<impl Reply, Rejection> {
    Ok(warp::reply::with_status(
        warp::reply::json(&json!({
            "status": "success",
            "registrationReceipt": "MOCK_REGISTRATION_RECEIPT"
        })),
        StatusCode::OK,
    ))
}

// POST /api/solution/{address}/{challenge_id}/{nonce}
async fn submit_solution_handler(
    nonce: String,
    address: String,
    challenge_id: String,
    receipts: MockReceipts,
    challenge_state: SharedState,
) -> Result<impl Reply, Rejection> {
    let state = challenge_state.read().unwrap();

    // --- DEADLINE CHECK IMPLEMENTATION ---
    let deadline: DateTime<Utc> = match state.latest_submission.parse::<DateTime<Utc>>() {
        Ok(dt) => dt,
        // If deadline can't be parsed, reject as an internal server issue or treat as expired
        Err(_) => return Ok(warp::reply::with_status(
            warp::reply::json(&json!({"status": "error", "message": "Internal deadline parse error."})),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    if Utc::now() > deadline {
        println!("❌ [Mock API] Submission rejected for expired challenge: {}", state.challenge_id);

        return Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "status": "error",
                "message": "Submission window closed", // <-- **UPDATED ERROR MESSAGE**
                "error_code": "CHALLENGE_EXPIRED"
            })),
            StatusCode::BAD_REQUEST,
        ));
    }
    // --- END DEADLINE CHECK ---

    // Increment the mock receipts count
    *receipts.write().unwrap() += 1;

    // ... (rest of the success logic remains the same) ...
    let mock_preimage = format!(
        "{}{}{}{}9cf4f6c96afbd4c0980fedddd53b0619b7c46e46f100c7f046db64d27acf6e7e2025-11-08T15:59:59.000Z892612581",
        nonce,
        address,
        challenge_id,
        MOCK_DIFFICULTY
    );
    let mock_signature = "a3904cbab0e5fcba67c75454a8976902de87ea79bcd33a554b686a1e7151958be207211ed25762d366ac3b1326fe56882c391b55ad1f6fde8539864a087ad04";
    let mock_timestamp = "2025-11-07T16:03:27.352Z";

    // Return the SolutionReceipt structure
    Ok(warp::reply::with_status(
        warp::reply::json(&json!({
            "status": "success",
            "crypto_receipt": {
                "preimage": mock_preimage,
                "signature": mock_signature,
                "timestamp": mock_timestamp,
            }
        })),
        StatusCode::OK,
    ))
}

// GET /api/statistics/{address}
async fn statistics_handler(_address: String, receipts: MockReceipts) -> Result<impl Reply, Rejection> {
    let receipt_count = *receipts.read().unwrap();

    Ok(warp::reply::json(&json!({
        "global": {
            "wallets": 100,
            "challenges": 1,
            "total_challenges": 1,
            "total_crypto_receipts": receipt_count + 1000,
            "recent_crypto_receipts": 10,
        },
        "local": {
            "crypto_receipts": receipt_count,
            "night_allocation": 1000000,
        }
    })))
}


// --- CORE SERVER STARTUP ---

pub fn start_mock_server_thread(port: u16) {
    let bind_addr = format!("127.0.0.1:{}", port);
    let address_clone = bind_addr.clone();

    println!("\n==============================================");
    println!("🧪 Starting Mock Scavenger API Server...");
    println!("   Bind Address: http://{}", bind_addr);
    println!("   API Base Path: /api");
    println!("==============================================\n");

    thread::spawn(move || {
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime for mock server.");

        // --- Initialize Shared States to CLEAN STATE ---
        let challenge_state = Arc::new(RwLock::new(initial_challenge_state()));
        let receipts_state: MockReceipts = Arc::new(RwLock::new(0));

        let initial_id = challenge_state.read().unwrap().challenge_id.clone();
        println!("🗑️ [Mock API] State initialized to clean slate ({}, Receipts: 0).", initial_id);

        rt.block_on(async {
            // 1. Spawn the continuous challenge updater task
            tokio::spawn(challenge_updater_task(challenge_state.clone()));

            // 2. Define Filters
            let state_filter = with_state(challenge_state.clone());
            let receipts_filter = with_receipts(receipts_state.clone());

            // Define the /api base filter
            let api_base = warp::path("api");

            // 3. Define all routes (all routes require the api_base filter)
            let tandc_route = api_base.clone()
                .and(warp::path!("TandC" / "1-0"))
                .and(warp::get())
                .and_then(tandc_handler);

            let challenge_route = api_base.clone()
                .and(warp::path("challenge"))
                .and(warp::get())
                .and(state_filter.clone())
                .and_then(challenge_status_handler);

            let register_route = api_base.clone()
                .and(warp::path!("register" / String / String / String))
                .and(warp::post())
                .and_then(register_handler);

            let solution_route = api_base.clone()
                .and(warp::path!("solution" / String / String / String))
                .and(warp::post())
                .and(receipts_filter.clone())
                .and(state_filter.clone())
                .and_then(submit_solution_handler);

            let statistics_route = api_base
                .and(warp::path!("statistics" / String))
                .and(warp::get())
                .and(receipts_filter.clone())
                .and_then(statistics_handler);

            // 4. Combine all routes with .or()
            let routes = tandc_route
                .or(challenge_route)
                .or(register_route)
                .or(solution_route)
                .or(statistics_route);

            // 5. Start the server
            warp::serve(routes)
                .run(address_clone.parse::<std::net::SocketAddr>().unwrap())
                .await;
        });
    });
}
