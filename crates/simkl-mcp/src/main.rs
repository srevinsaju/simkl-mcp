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

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::auth::Auth;
use crate::config::{Config, Secrets};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the Streamable HTTP server.
    Serve {
        /// A TOML configuration file; `SIMKL_MCP_*` environment variables
        /// override it.
        #[arg(long, env = "SIMKL_MCP_CONFIG")]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    match Cli::parse().command {
        Command::Serve { config } => serve(Config::load(config.as_deref())?).await,
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let secrets = Secrets::from_env()?;
    let auth = Arc::new(
        Auth::open(
            &config.public_url,
            &config.resource(),
            &config.database,
            config.client.client_id.clone(),
            secrets.simkl_client_secret,
            secrets.vault_key,
            config.auth,
        )
        .await?,
    );
    auth.spawn_sweeper();
    let simkl = simkl_api::Client::new(config.client.clone())?;
    let app = http::router(&config, auth, simkl)?;

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(bind = %config.bind, public_url = %config.public_url, "serving");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
