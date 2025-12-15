use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

use std::path::PathBuf;

pub fn watch(paths: &[PathBuf], tx: Sender<notify::Result<Event>>) -> notify::Result<RecommendedWatcher> {
    // Watcher configuration
    let mut watcher = notify::recommended_watcher(tx)?;
    for path in paths {
        watcher.watch(path, RecursiveMode::Recursive)?;
    }

    Ok(watcher)
}
