mod api;
mod config;
mod daemon;
mod indexer;
mod mcp;
mod storage;

use clap::Parser;
use config::Config;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "contextd")]
#[command(about = "A local-first semantic context daemon for AI agents")]
#[command(version)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "contextd.toml")]
    config: PathBuf,

    /// Run as an MCP server (for Claude Desktop integration)
    #[arg(long)]
    mcp: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let config = if args.config.exists() {
        eprintln!("Loading config from {}", args.config.display());
        Config::load(&args.config)?
    } else {
        eprintln!(
            "Config not found at {}, using defaults",
            args.config.display()
        );
        Config::default()
    };

    if args.mcp {
        // Run as MCP server (stdio mode for Claude Desktop)
        eprintln!("contextd starting in MCP mode...");

        // Initialize components
        let db = storage::db::Database::new(&config.storage.db_path)?;
        let embedder = Arc::new(indexer::embeddings::Embedder::new(
            &config.storage.model_path,
        )?);

        mcp::run_mcp_server(db, embedder, config).await;
    } else {
        // Run as daemon with REST API
        println!("contextd starting in daemon mode...");
        daemon::run(config).await?;
    }

    Ok(())
}
