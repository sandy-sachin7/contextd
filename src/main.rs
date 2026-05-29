use clap::Parser;
use std::path::PathBuf;

use contextd::cli;
use contextd::config::Config;
use contextd::daemon;
use contextd::indexer::embeddings::Embedder;
use contextd::mcp;
use contextd::storage::db::Database;
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
        Config::load(&args.config)?
    } else {
        Config::default()
    };

    match args.command.unwrap_or(cli::Commands::Daemon) {
        cli::Commands::Daemon => {
            println!("contextd starting in daemon mode...");
            daemon::run(config).await?;
        }
        cli::Commands::Mcp => {
            eprintln!("contextd starting in MCP mode...");
            let db = Database::new(&config.storage.db_path)?;
            let embedder = Arc::new(Embedder::new(&config.storage)?);
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
