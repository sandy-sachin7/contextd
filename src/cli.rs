use anyhow::Result;
use clap::Subcommand;

use crate::config::Config;
use crate::indexer::embeddings::Embedder;
use crate::storage::db::{Database, SearchOptions};

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run as a daemon (default)
    Daemon,
    /// Run as an MCP server
    Mcp,
    /// Setup models
    Setup,
    /// Query the index
    Query {
        query: String,
        /// Number of context lines to show before/after match
        #[arg(short, long, default_value = "0")]
        context: usize,
    },
}

pub async fn handle_setup(config: &Config) -> Result<()> {
    println!("Setting up model: {}", config.storage.model_type);
    println!("Target directory: {:?}", config.storage.model_path);

    crate::download::ensure_model_files(&config.storage.model_path, &config.storage.model_type)
        .await?;

    println!("Model setup complete.");
    Ok(())
}

pub async fn handle_query(config: &Config, query: &str, context_lines: usize) -> Result<()> {
    let db = Database::new(&config.storage.db_path)?;
    let embedder = Embedder::new(&config.storage)?;

    let embedding = embedder.embed(query)?;

    let options = SearchOptions {
        limit: Some(10),
        context_lines: if context_lines > 0 {
            Some(context_lines)
        } else {
            None
        },
        ..Default::default()
    };

    let results = db.search_chunks_hybrid(query, &embedding, &options)?;

    println!("Found {} results for '{}':", results.len(), query);
    for (i, res) in results.iter().enumerate() {
        println!("\n{}. {} (Score: {:.4})", i + 1, res.file_path, res.score);
        println!(
            "   {}...",
            res.content
                .replace('\n', " ")
                .chars()
                .take(100)
                .collect::<String>()
        );
    }

    Ok(())
}
