use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

pub fn watch(path: &Path, tx: Sender<notify::Result<Event>>) -> notify::Result<RecommendedWatcher> {
    // Watcher configuration
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(path, RecursiveMode::Recursive)?;

    Ok(watcher)
}
