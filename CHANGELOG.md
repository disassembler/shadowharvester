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
