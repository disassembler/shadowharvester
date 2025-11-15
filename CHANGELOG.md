# 📦 Changelog for Shadow Harvester v0.3.0

This release focuses on significantly expanding the application's debugging, testing, and operational capabilities by implementing a powerful local **Mock API** and finalizing the infrastructure for **WebSocket** and mass donation operations.

## 🛡️ Operational Resilience and Testing Infrastructure

* **Mock API Integration (`--mock-api-port`):** A fully functional local Mock Scavenger API server has been implemented using the `warp` framework. This allows developers to run comprehensive integration tests and debug the submission, registration, and polling logic without contacting the live upstream API.
* **API Client Hardening:** Improved error handling in `src/api.rs` to better parse detailed JSON error messages and status codes from the upstream API during registration and submission attempts.
* **Fatal Error Handling:** The application now correctly terminates the entire process and prints a `FATAL THREAD ERROR` message if any critical worker thread (Manager, Submitter, Polling) fails.
* **Challenge Expiration Enforcement:** Added logic to immediately halt the mining process if a challenge submission deadline has been passed, preventing wasted hashing time.

## 🚀 Wallet and Advanced Submission Features

* **Mass Donation Sweep (`wallet donate-all`):** Implemented a comprehensive command to automate the donation process for Mnemonic wallets.
    * Supports base and enterprise address derivation.
    * Includes a **404 tolerance window** to gracefully stop sweeping when unregistered indices are encountered.
    * Handles network retries, `ALREADY MAPPED` (409), and bad signature errors.
* **Challenge ID Simplification:** The main `--challenge` flag now accepts a simple **Challenge ID** string (e.g., `D07C21`). The application automatically looks up the full parameters from the local Sled database, simplifying command usage.

## 💻 New CLI and Debugging Tools

The synchronous CLI toolkit has been expanded to support state inspection and data management:

### Database Management (`shadow-harvester db`)

| Command | Functionality |
| :--- | :--- |
| `db export --file <path>` | Dumps the entire contents of the Sled database to a portable JSON file. |
| `db import --file <path>` | Imports data from a JSON backup file, using a non-duplicative logic to ensure existing keys are not overwritten. |

### Challenge & Wallet Inspection

| Command | Functionality |
| :--- | :--- |
| `challenge details --id <ID>` | Outputs structured challenge details (ROM Key, Deadline) alongside local counts of **completed solutions** and **pending submissions**. |
| `wallet list` | Lists unique wallet identifiers (`<Mnemonic Hash>:<Account Index>`) stored in the database. |
| `wallet addresses --wallet <hash:account>` | Lists all known derived Cardano addresses and their corresponding payment indexes for a given wallet. |

# Changelog for Shadow Harvester v0.2.0

This release marks a fundamental architectural shift, moving the miner from a fragile,
synchronous model to a robust, multi-threaded, reactive system. The core focus was
decoupling hashing from network latency, improving crash resilience,
and transitioning to a modern database for state management.

## ⚠️ IMPORTANT: STATE MIGRATION REQUIRED

Since this release moves from file-based state (receipts, indices) to an embedded
Sled database, **all users must run the migration tool once** to preserve their
history and resume mnemonic mining correctly.

```bash
shadow-harvester migrate-state --old-data-dir ./ --data-dir <your_sled_path>
```


* **Failure to run this command** will result in the miner starting over at index 0
and ignoring any previously completed challenge progress.

## 🚀 Major Architectural Overhaul

* **Reactive, Multi-Threaded Core:** The application logic is refactored into a reactive, channel-based (mPSC) architecture. This cleanly separates responsibilities among dedicated worker threads: **Challenge Manager**, **State Worker**, and **Network Polling**.
* **Sled Database Integration:** All critical state data (receipts, challenge data, and mnemonic progress) has been migrated from the legacy file-based system to a fast, crash-safe, embedded database (**Sled**).
* **Asynchronous Submission Queue:** Hashing is decoupled from network I/O. Solutions are now written to a local disk queue, and a dedicated **State Worker** thread handles submission retries in the background, preventing API instability from stopping the mining process.

## 🛡️ Resilience and State Management

* **Crash Recovery Implemented:** Solutions found by the miner are saved using an **atomic, two-stage persistence** process (local file -> Sled queue). This ensures no valid nonce is lost if the miner crashes or loses power.
* **Robust Mnemonic Resumption:** The logic for Mnemonic Sequential Mining now correctly recovers the last processed index by reading addresses and derivation paths from the Sled database, guaranteeing continuity.
* **Server Stability:** The submitter thread now correctly identifies and retries transient **5xx Internal Server Errors** instead of treating them as fatal validation failures.
* **Donation Tracking Removed:** All logic for submitting and tracking donation transactions was removed, simplifying the core architecture.

## 💻 New CLI Commands and Debugging Tools

All commands requiring state access are now nested under `challenge` or `wallet`, providing precise inspection capabilities.

### Challenge State Commands (`shadow-harvester challenge`)

| Command | Functionality |
| :--- | :--- |
| `challenge list` | Lists all unique Challenge IDs stored in the local database. |
| `challenge info --id <ID>` | Dumps the full JSON details for a specific challenge configuration. |
| `challenge details --id <ID>` | Outputs structured mining details (ROM Key, Difficulty) along with aggregated local counts of **completed solutions** and **pending submissions**. |
| `challenge import --file <path>` | Imports a custom challenge JSON file into the Sled database for testing or custom mining. |
| `challenge receipt-info` | Dumps the full JSON receipt stored for a specific `challenge_id` and `address`. |
| `challenge pending-info` | Dumps the full JSON for a specific pending solution waiting in the submission queue. |

### Wallet State Commands (`shadow-harvester wallet`)

| Command | Functionality |
| :--- | :--- |
| `wallet list` | Lists unique wallet identifiers (`<Mnemonic Hash>:<Account Index>`) found in the database. |
| `wallet addresses --wallet <hash:account>` | Lists all known Cardano addresses and their derivation indexes associated with that wallet identifier. |
| `wallet list-challenges --address <addr>` | Lists all Challenge IDs for which the specified address has a successfully completed receipt. |
