// shadowharvester/src/cli.rs

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
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
    #[arg(long, default_value = "addr_test1qq4dl3nhr0axurgcrpun9xyp04pd2r2dwu5x7eeam98psv6dhxlde8ucclv2p46hm077ds4vzelf5565fg3ky794uhrq5up0he")]
    pub address: String,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generates a new random Ed25519 key pair and prints the corresponding payment address.
    #[command(author, version, about = "Generate new keys")]
    KeyGen,
    // Add more subcommands here as the miner evolves (e.g., Register, Submit)
}
