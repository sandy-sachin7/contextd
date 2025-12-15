use crate::api;
use crate::indexer::{chunker, watcher};
use crate::storage::db::Database;
use anyhow::Result;
use notify::Event;
use std::path::Path;
use std::sync::mpsc;

pub async fn run() -> Result<()> {
    // 1. Initialize Storage
    let db_path = "contextd.db";
    let db = Database::new(db_path)?;
    println!("Database initialized at {}", db_path);

    // 2. Start Watcher
    let (tx, rx) = mpsc::channel();
    let watch_path = Path::new("."); // Watch current dir for now
    let _watcher = watcher::watch(watch_path, tx)?;
    println!("Watching {}", watch_path.display());

    // 3. Start API Server in background
    let db_clone = db.clone();
    tokio::spawn(async move {
        api::run_server(db_clone).await;
    });

    // 4. Main Loop: Process File Events
    println!("Daemon main loop starting...");
    for event in rx {
        let Event { paths, kind, .. } = event;
        println!("Event: {:?} {:?}", kind, paths);
        for path in paths {
            if path.extension().map_or(false, |ext| ext == "txt" || ext == "md") {
                println!("Processing file: {:?}", path);

                // Read file
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // Chunk
                    if let Ok(chunks) = chunker::chunk_text(&content) {
                        // Store
                        let path_str = path.to_string_lossy().to_string();
                        if let Ok(file_id) = db.add_or_update_file(&path_str, 0) { // TODO: real timestamp
                            let count = chunks.len();
                            let _ = db.clear_chunks(file_id);
                            for chunk in chunks {
                                let _ = db.add_chunk(file_id, chunk.start, chunk.end, &chunk.content);
                            }
                            let _ = db.mark_indexed(file_id);
                            println!("Indexed {} chunks for {:?}", count, path);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
