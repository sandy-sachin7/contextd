mod api;
mod config;
mod daemon;
mod indexer;
mod mcp;
mod storage;

mod cli;

use clap::Parser;
use config::Config;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "contextd")]
#[command(about = "A local-first semantic context daemon for AI agents")]
#[command(version)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "contextd.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<cli::Commands>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let config = if args.config.exists() {
        // eprintln!("Loading config from {}", args.config.display());
        Config::load(&args.config)?
    } else {
        // eprintln!("Config not found at {}, using defaults", args.config.display());
        Config::default()
    };

    match args.command.unwrap_or(cli::Commands::Daemon) {
        cli::Commands::Daemon => {
            println!("contextd starting in daemon mode...");
            daemon::run(config).await?;
        }
        cli::Commands::Mcp => {
            eprintln!("contextd starting in MCP mode...");
            let db = storage::db::Database::new(&config.storage.db_path)?;
            let embedder = Arc::new(indexer::embeddings::Embedder::new(&config.storage)?);
            mcp::run_mcp_server(db, embedder, config).await;
        }
        cli::Commands::Setup => {
            cli::handle_setup(&config).await?;
        }
        cli::Commands::Query { query, context } => {
            cli::handle_query(&config, &query, context).await?;
        }
    }

    Ok(())
}
