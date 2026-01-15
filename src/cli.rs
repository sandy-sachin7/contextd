use anyhow::Result;
use clap::Subcommand;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

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
    let model_dir = &config.storage.model_path;
    let model_type = &config.storage.model_type;

    if !model_dir.exists() {
        fs::create_dir_all(model_dir)?;
    }

    println!("Setting up model: {}", model_type);
    println!("Target directory: {:?}", model_dir);

    // Define URLs based on model type
    // Using HuggingFace Optimum ONNX models
    let (model_url, tokenizer_url, description) = match model_type.as_str() {
        "all-minilm-l6-v2" => (
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
            "General-purpose embeddings (384 dim, fast)",
        ),
        "all-mpnet-base-v2" => (
            "https://huggingface.co/optimum/all-mpnet-base-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-mpnet-base-v2/resolve/main/tokenizer.json",
            "Higher quality embeddings (768 dim, recommended for code)",
        ),
        "bge-small-en-v1.5" => (
            "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx",
            "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json",
            "BGE small embeddings (384 dim, good quality/speed balance)",
        ),
        _ => (
            // Default fallback
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
            "Default: all-minilm-l6-v2 (384 dim)",
        ),
    };

    println!("Model: {} - {}", model_type, description);

    download_file(model_url, &model_dir.join("model.onnx")).await?;
    download_file(tokenizer_url, &model_dir.join("tokenizer.json")).await?;

    println!("Model setup complete.");
    Ok(())
}

async fn download_file(url: &str, path: &PathBuf) -> Result<()> {
    if path.exists() {
        println!("File {:?} already exists, skipping.", path);
        return Ok(());
    }

    println!("Downloading {}...", url);

    let res = reqwest::get(url).await?;
    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
        .progress_chars("#>-"));

    let mut file = fs::File::create(path)?;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk)?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_with_message("Download complete");
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
