// shadowharvester/src/cli.rs

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
    #[arg(long, default_value_t = 24)]
    pub threads: u32,

    /// NEW: Optional secret key (hex-encoded) to mine with. If passed, only solves once.
    #[arg(long)]
    pub payment_key: Option<String>,

    /// NEW: Cardano address (bech32) to donate all accumulated rewards to.
    #[arg(long)]
    pub donate_to: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// NEW: Lists the current status and details of the mining challenge.
    #[command(author, about = "List current challenge status")]
    Challenges,
}
