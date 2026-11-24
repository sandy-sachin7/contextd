use clap::{CommandFactory, Parser};
use std::path::PathBuf;
use std::sync::Arc;

use contextd::cli::{self, QueryCliOptions};
use contextd::config::Config;
use contextd::daemon;
use contextd::indexer::embeddings::{Embedder, TextEmbedder};
use contextd::mcp;
use contextd::storage::db::Database;

#[derive(Parser, Debug)]
#[command(name = "contextd")]
#[command(about = "A local-first semantic context daemon for AI agents")]
#[command(version)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Explicitly trust an --config file outside the user config directory
    /// (allows it to configure [server], [storage], and [plugins])
    #[arg(long, global = true, default_value_t = false)]
    trust_config: bool,

    /// Explicitly trust and execute [plugins] from a workspace-local contextd.toml
    #[arg(long, global = true, default_value_t = false)]
    trust_workspace_plugins: bool,

    #[command(subcommand)]
    command: Option<cli::Commands>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Handle shell completions immediately without loading config
    if let Some(cli::Commands::Completions { shell }) = args.command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
        return Ok(());
    }

    let config = Config::load_resolved_with_trust(
        args.config.as_deref(),
        args.trust_config,
        args.trust_workspace_plugins,
    )?;

    match args.command.unwrap_or(cli::Commands::Daemon {
        exit_on_stdin_eof: false,
    }) {
        cli::Commands::Daemon { exit_on_stdin_eof } => {
            println!("contextd starting in daemon mode...");
            daemon::run(config, exit_on_stdin_eof).await?;
        }
        cli::Commands::Mcp => {
            eprintln!("contextd starting in MCP mode...");
            // Migrate legacy flat caches before the existence check so an
            // upgrade does not require a separate setup or daemon run. Never
            // download here: stdout carries JSON-RPC frames.
            if let Err(err) = contextd::download::migrate_legacy_model_cache(
                &config.storage.model_path,
                &config.storage.model_type,
            ) {
                eprintln!("Legacy model cache migration failed: {}", err);
            }
            if !contextd::download::model_files_exist(
                &config.storage.model_path,
                &config.storage.model_type,
            ) {
                eprintln!(
                    "Models not found in {:?}. Run 'contextd setup' or 'contextd daemon' first.",
                    config.storage.model_path
                );
                std::process::exit(1);
            }

            let storage_cfg = config.storage.clone();
            let (db, embedder): (Database, Arc<dyn TextEmbedder>) =
                tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                    let embedder = Embedder::new(&storage_cfg)?;
                    let db = Database::open(
                        &storage_cfg.db_path,
                        embedder.embedding_dim(),
                        embedder.model_type(),
                    )?;
                    Ok((db, Arc::new(embedder) as Arc<dyn TextEmbedder>))
                })
                .await??;

            mcp::run_mcp_server(db, embedder, config).await;
        }
        cli::Commands::Setup => {
            cli::handle_setup(&config).await?;
        }
        cli::Commands::Query {
            query,
            context,
            limit,
            min_score,
            after,
            before,
            file_types,
            paths,
        } => {
            cli::handle_query(
                &config,
                &query,
                QueryCliOptions {
                    context_lines: context,
                    limit,
                    min_score,
                    after,
                    before,
                    file_types,
                    paths,
                },
            )
            .await?;
        }
        cli::Commands::Connect { all, scope, tool } => {
            contextd::connect::handle_connect(all, scope, tool.as_deref()).await?;
        }
        cli::Commands::Completions { .. } => unreachable!(),
    }

    Ok(())
}
