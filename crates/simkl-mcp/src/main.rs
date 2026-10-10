//! simkl-mcp: an MCP server for Simkl.
//!
//! `serve` runs the remote Streamable HTTP server with its own OAuth
//! authorization server in front of Simkl. `stdio` runs locally for a single
//! user who signs in with Simkl's device flow.

mod auth;
mod config;
mod http;
mod server;
mod stdio;
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
    /// Run the remote Streamable HTTP server.
    Serve,
    /// Run over stdio for a single local user.
    Stdio,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Serve => anyhow::bail!("serve is not implemented yet"),
        Command::Stdio => anyhow::bail!("stdio is not implemented yet"),
    }
}
