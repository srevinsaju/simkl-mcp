//! simkl-mcp: an MCP server for Simkl.
//!
//! `serve` runs the Streamable HTTP server with its own OAuth authorization
//! server in front of Simkl.

mod auth;
mod config;
mod http;
mod server;
mod tools;
mod views;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the Streamable HTTP server.
    Serve,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Serve => anyhow::bail!("serve is not implemented yet"),
    }
}
