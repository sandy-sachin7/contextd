use crate::api;
use crate::indexer::{chunker, embeddings::Embedder, plugins, watcher};
use crate::storage::db::Database;
use anyhow::Result;
use ignore::WalkBuilder;
use std::sync::{mpsc, Arc};

use crate::config::Config;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Semaphore;

pub async fn run(config: Config) -> Result<()> {
    // 1. Initialize Storage
    let db = Database::new(&config.storage.db_path)?;
    println!("Database initialized at {:?}", config.storage.db_path);

    // 2. Initialize Embedder
    let embedder = Arc::new(Embedder::new(&config.storage)?);
    println!("Embedder initialized from {:?}", config.storage.model_path);

    let config = Arc::new(config);
    let semaphore = Arc::new(Semaphore::new(4)); // Limit concurrency

    // 3. Initial Scan
    println!("Performing initial scan of {:?}", config.watch.paths);
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}")?);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

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
                        let config = config.clone();
                        let db = db.clone();
                        let embedder = embedder.clone();
                        let path = path.to_path_buf();
                        let semaphore = semaphore.clone();
                        let pb = pb.clone();

                        // Acquire permit before spawning to limit active tasks
                        // For initial scan, we want backpressure
                        let permit = semaphore.acquire_owned().await.unwrap();

                        tokio::spawn(async move {
                            pb.set_message(format!(
                                "Indexing {:?}",
                                path.file_name().unwrap_or_default()
                            ));
                            index_file(path, config, db, embedder).await;
                            drop(permit);
                            pb.inc(1);
                        });
                    }
                }
                Err(err) => eprintln!("Error during scan: {}", err),
            }
        }
    }
    pb.finish_with_message("Initial scan complete.");

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

                        let config = config.clone();
                        let db = db.clone();
                        let embedder = embedder.clone();
                        let path = path.to_path_buf();
                        let semaphore = semaphore.clone();

                        tokio::spawn(async move {
                            // Acquire permit inside spawn for watcher events to avoid blocking the loop
                            // (Though blocking loop is also fine for backpressure, but let's be non-blocking for events)
                            let _permit = semaphore.acquire_owned().await.unwrap();
                            index_file(path, config, db, embedder).await;
                        });
                    }
                }
            }
            Err(e) => println!("Watch error: {:?}", e),
        }
    }

    Ok(())
}

async fn index_file(
    path: std::path::PathBuf,
    config: Arc<Config>,
    db: Database,
    embedder: Arc<Embedder>,
) {
    // Check extension
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Check if needs reindexing
    let metadata = std::fs::metadata(&path).ok();
    let modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let path_str = path.to_string_lossy().to_string();
    if let Ok(false) = db.needs_reindexing(&path_str, modified) {
        // println!("Skipping {:?} (unchanged)", path);
        return;
    }

    let chunks_result = if let Some(cmd) = config.plugins.get(ext) {
        println!("Using plugin {:?} for {:?}", cmd, path);
        match plugins::run_parser(cmd, &path).await {
            Ok(content) => chunker::chunk_by_type(&content, ext),
            Err(e) => Err(e),
        }
    } else if ext == "pdf" {
        chunker::chunk_pdf(&path)
    } else {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        chunker::chunk_by_type(&content, ext)
    };

    if let Ok(chunks) = chunks_result {
        // Store
        let path_str = path.to_string_lossy().to_string();
        let metadata = std::fs::metadata(&path).ok();
        let modified = metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Collect metadata
        let file_meta = std::fs::metadata(&path).ok();
        let size = file_meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let created = file_meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_metadata = serde_json::json!({
            "size": size,
            "created": created,
            "modified": modified,
            "extension": ext
        });

        if let Ok(file_id) = db.add_or_update_file(&path_str, modified) {
            let count = chunks.len();
            let _ = db.clear_chunks(file_id);
            for chunk in chunks {
                // Merge chunk metadata if present
                let mut final_metadata = file_metadata.clone();
                if let Some(cm) = &chunk.metadata {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(cm) {
                        if let Some(obj) = final_metadata.as_object_mut() {
                            if let Some(parsed_obj) = parsed.as_object() {
                                for (k, v) in parsed_obj {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                }

                // Embed chunk
                let embedding = embedder.embed(&chunk.content).ok();
                let _ = db.add_chunk(
                    file_id,
                    chunk.start,
                    chunk.end,
                    &chunk.content,
                    embedding.as_deref(),
                    Some(&final_metadata.to_string()),
                );
            }
            let _ = db.mark_indexed(file_id);
            println!("Indexed {} chunks for {:?}", count, path);
        }
    } else if let Err(e) = chunks_result {
        eprintln!("Error chunking file {:?}: {:?}", path, e);
    }
}
