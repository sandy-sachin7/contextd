use notify_debouncer_mini::notify::{self, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

pub fn watch(
    paths: &[PathBuf],
    tx: Sender<DebounceEventResult>,
) -> notify::Result<Debouncer<notify::RecommendedWatcher>> {
    // Watcher configuration with 2000ms debounce
    let mut debouncer = new_debouncer(Duration::from_millis(2000), tx)?;

    for path in paths {
        debouncer.watcher().watch(path, RecursiveMode::Recursive)?;
    }

    Ok(debouncer)
}
