// src/cli.rs

use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// The base URL for the Scavenger Mine API (e.g., https://scavenger.gd.midnighttge.io)
    #[arg(long)]
    pub api_url: Option<String>,

    /// Accept the Token End User Agreement and continue mining without displaying the terms.
    #[arg(long)]
    pub accept_tos: bool,

    /// Registered Cardano address to submit solutions for.
    #[arg(long)]
    pub address: Option<String>,

    /// Number of worker threads to use for mining.
    #[arg(long, default_value_t = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(24))]
    pub threads: u32,

    /// Optional secret key (hex-encoded) to mine with.
    #[arg(long)]
    pub payment_key: Option<String>,

    /// Automatically generate a new ephemeral key pair for every mining cycle.
    #[arg(long)]
    pub ephemeral_key: bool,

    /// Cardano address (bech32) to donate all accumulated rewards to.
    #[arg(long)]
    pub donate_to: Option<String>,

    /// 24-word BIP39 mnemonic phrase for sequential address generation.
    #[arg(long)]
    pub mnemonic: Option<String>,

    #[arg(long)]
    pub mnemonic_file: Option<String>,

    #[arg(long, default_value_t = 0)]
    pub mnemonic_account: u32,

    #[arg(long, default_value_t = 0)]
    pub mnemonic_starting_index: u32,

    /// The name of the challenge to mine (e.g., D07C21). The challenge details are loaded from the Sled DB.
    #[arg(long)]
    pub challenge: Option<String>,

    /// Where to store state (like the mnemonic starting index) and receipts
    #[arg(long, default_value = ".")]
    pub data_dir: Option<String>,

    /// Enable WebSocket mode for receiving challenges and posting solutions.
    #[arg(long)]
    pub websocket: bool,
}


#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Lists the current status and details of the mining challenge (API-based check).
    #[command(author, about = "List current challenge status")]
    Challenges,

    /// Migrates old file-based state (receipts/indices) to the new Sled database.
    #[command(author, about = "Migrate old file-based state to Sled DB")]
    MigrateState {
        /// The path to the old file-based state directory (default: current directory).
        #[arg(long, default_value = ".")]
        old_data_dir: String,
    },

    /// Commands for managing stored challenges (list, import, info).
    #[command(subcommand, author, about = "Manage local challenge state (list, import, info)")]
    Challenge(ChallengeCommands),

    /// Commands for inspecting known wallet addresses and derivations.
    #[command(subcommand, author, about = "Inspect known wallet addresses")]
    Wallet(WalletCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub enum ChallengeCommands {
    /// Lists all challenge IDs stored in the local Sled database.
    List,

    /// Imports a challenge JSON file into the local Sled database for offline/custom mining.
    Import {
        /// Path to the challenge JSON file (must contain ChallengeData structure).
        #[arg(long)]
        file: String,
    },

    /// Dumps the full JSON details of a specific challenge loaded from the Sled DB.
    Info {
        /// The ID of the challenge to display (e.g., D07C21).
        #[arg(long)]
        id: String,
    },

    /// Outputs challenge details, plus local completed and pending solution counts.
    #[command(author, about = "Outputs detailed challenge stats and mining setup.")]
    Details {
        /// The ID of the challenge to display (e.g., D07C21).
        #[arg(long)]
        id: String,
    },

    /// Dumps the receipt JSON for a specific address and challenge ID.
    ReceiptInfo {
        /// The ID of the challenge (e.g., D07C21).
        #[arg(long)]
        challenge_id: String,
        /// The Cardano address associated with the receipt.
        #[arg(long)]
        address: String,
    },

    /// Dumps the JSON details of a specific pending solution.
    PendingInfo {
        /// The ID of the challenge (e.g., D07C21).
        #[arg(long)]
        challenge_id: String,
        /// The Cardano address associated with the pending solution.
        #[arg(long)]
        address: String,
        /// The nonce of the solution (16 hex chars).
        #[arg(long)]
        nonce: String,
    },
    Errors,
    Hash {
        /// The ID of the challenge (e.g., D07C21).
        #[arg(long)]
        challenge_id: String,
        /// The Cardano address associated with the receipt.
        #[arg(long)]
        address: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WalletCommands {
    /// Lists unique wallet identifiers (Mnemonic Hash:Account Index) found in the database.
    List,

    /// Lists all known addresses and derivation paths (<index>:<address>) for a specific wallet hash.
    Addresses {
        /// The unique wallet identifier (Mnemonic Hash:Account Index) to inspect (e.g., 16886378742194182050:0).
        #[arg(long)]
        wallet: String,
    },

    /// Lists all challenge IDs that a specific address has a receipt for.
    ListChallenges {
        /// The Cardano address to inspect.
        #[arg(long)]
        address: String,
    },
}
