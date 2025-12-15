use crate::api;
use crate::indexer::{chunker, embeddings::Embedder, watcher};
use crate::storage::db::Database;
use anyhow::Result;
use std::path::Path;
use std::sync::{mpsc, Arc};

pub async fn run() -> Result<()> {
    // 1. Initialize Storage
    let db_path = "contextd.db";
    let db = Database::new(db_path)?;
    println!("Database initialized at {}", db_path);

    // 2. Initialize Embedder
    let model_dir = "models";
    let embedder = Arc::new(Embedder::new(model_dir)?);
    println!("Embedder initialized from {}", model_dir);

    // 3. Start Watcher
    let (tx, rx) = mpsc::channel();
    let watch_path = Path::new("."); // Watch current dir for now
    let _watcher = watcher::watch(watch_path, tx)?;
    println!("Watching {}", watch_path.display());

    // 4. Start API Server in background
    let db_clone = db.clone();
    let embedder_clone = embedder.clone();
    tokio::spawn(async move {
        api::run_server(db_clone, embedder_clone).await;
    });

    // 5. Main Loop: Process File Events
    println!("Daemon main loop starting...");
    for event in rx {
        match event {
            Ok(event) => {
                for path in event.paths {
                    if path.exists() {
                        // Check extension
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        if !["txt", "md", "pdf"].contains(&ext) {
                            continue;
                        }

                        println!("Processing {:?}", path);

                        // Chunk
                        let chunks_result = if ext == "pdf" {
                            chunker::chunk_pdf(&path)
                        } else {
                            let content = std::fs::read_to_string(&path).unwrap_or_default();
                            chunker::chunk_text(&content)
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
                                    );
                                }
                                let _ = db.mark_indexed(file_id);
                                println!("Indexed {} chunks for {:?}", count, path);
                            }
                        }
                    }
                }
            }
            Err(e) => println!("Watch error: {:?}", e),
        }
    }

    Ok(())
}
