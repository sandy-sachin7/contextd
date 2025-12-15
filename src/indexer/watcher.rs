use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;


pub fn watch(path: &Path, tx: Sender<Event>) -> notify::Result<RecommendedWatcher> {
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();

    let mut watcher = RecommendedWatcher::new(notify_tx, Config::default())?;

    watcher.watch(path, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        for res in notify_rx {
            match res {
                Ok(event) => {
                    // Simple debounce/filter could go here, but for Phase 1 just forward
                    let _ = tx.send(event);
                }
                Err(e) => println!("watch error: {:?}", e),
            }
        }
    });

    Ok(watcher)
}
