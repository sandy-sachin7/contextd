use crate::api;
use crate::indexer::{chunker, embeddings::Embedder, plugins, watcher};
use crate::storage::db::Database;
use anyhow::Result;
use ignore::WalkBuilder;
use std::sync::{mpsc, Arc};

use crate::config::Config;

pub async fn run(config: Config) -> Result<()> {
    // 1. Initialize Storage
    let db = Database::new(&config.storage.db_path)?;
    println!("Database initialized at {:?}", config.storage.db_path);

    // 2. Initialize Embedder
    let embedder = Arc::new(Embedder::new(&config.storage.model_path)?);
    println!("Embedder initialized from {:?}", config.storage.model_path);

    // 3. Initial Scan
    println!("Performing initial scan of {:?}", config.watch.paths);
    for path in &config.watch.paths {
        let walker = WalkBuilder::new(path)
            .standard_filters(true)
            .add_custom_ignore_filename(".contextignore")
            .build();

        for result in walker {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_file() {
                        index_file(path, &config, &db, &embedder);
                    }
                }
                Err(err) => eprintln!("Error during scan: {}", err),
            }
        }
    }
    println!("Initial scan complete.");

    // 4. Start Watcher
    let (tx, rx) = mpsc::channel();
    let _watcher = watcher::watch(&config.watch.paths, tx)?;
    println!("Watching {:?}", config.watch.paths);

    // 5. Start API Server in background
    let db_clone = db.clone();
    let embedder_clone = embedder.clone();
    let host = config.server.host.clone();
    let port = config.server.port;
    tokio::spawn(async move {
        api::run_server(db_clone, embedder_clone, &host, port).await;
    });

    // Initialize Ignore Checkers for Watcher
    let ignore_checkers: Vec<crate::indexer::ignore::IgnoreChecker> = config
        .watch
        .paths
        .iter()
        .map(|p| crate::indexer::ignore::IgnoreChecker::new(p))
        .collect();

    // 6. Main Loop: Process File Events
    println!("Daemon main loop starting...");
    for result in rx {
        match result {
            Ok(events) => {
                let mut unique_paths = std::collections::HashSet::new();
                for event in events {
                    unique_paths.insert(event.path);
                }

                for path in unique_paths {
                    let is_dir = path.is_dir();
                    let is_ignored = ignore_checkers.iter().any(|c| c.is_ignored(&path, is_dir));

                    if !is_ignored && path.exists() {
                        if path.is_dir() {
                            continue;
                        }
                        // Temporary fix for infinite loop on .gitignore
                        if path.file_name().and_then(|s| s.to_str()) == Some(".gitignore") {
                            continue;
                        }
                        index_file(&path, &config, &db, &embedder);
                    }
                }
            }
            Err(e) => println!("Watch error: {:?}", e),
        }
    }

    Ok(())
}

fn index_file(path: &std::path::Path, config: &Config, db: &Database, embedder: &Arc<Embedder>) {
    // Check extension
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let chunks_result = if let Some(cmd) = config.plugins.get(ext) {
        println!("Using plugin {:?} for {:?}", cmd, path);
        plugins::run_parser(cmd, path).and_then(|content| chunker::chunk_by_type(&content, ext))
    } else if ext == "pdf" {
        chunker::chunk_pdf(path)
    } else {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        chunker::chunk_by_type(&content, ext)
    };

    if let Ok(chunks) = chunks_result {
        // Store
        let path_str = path.to_string_lossy().to_string();
        let metadata = std::fs::metadata(path).ok();
        let modified = metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Collect metadata
        let file_meta = std::fs::metadata(path).ok();
        let size = file_meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let created = file_meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let metadata_json = serde_json::json!({
            "size": size,
            "created": created,
            "modified": modified,
            "extension": ext
        })
        .to_string();

        if let Ok(file_id) = db.add_or_update_file(&path_str, modified) {
            let count = chunks.len();
            let _ = db.clear_chunks(file_id);
            for chunk in chunks {
                // Embed chunk
                let embedding = embedder.embed(&chunk.content).ok();
                let _ = db.add_chunk(
                    file_id,
                    chunk.start,
                    chunk.end,
                    &chunk.content,
                    embedding.as_deref(),
                    Some(&metadata_json),
                );
            }
            let _ = db.mark_indexed(file_id);
            println!("Indexed {} chunks for {:?}", count, path);
        }
    } else if let Err(e) = chunks_result {
        eprintln!("Error chunking file {:?}: {:?}", path, e);
    }
}
