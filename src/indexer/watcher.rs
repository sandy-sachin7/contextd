use crate::indexer::ignore::is_default_ignored_dir_name;
use ignore::WalkBuilder;
use notify_debouncer_mini::notify::{self, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

/// Watch configured root paths with debouncing while skipping heavy ignored directories
/// (`.git`, `node_modules`, `target`, `.venv`, etc.) to prevent `inotify` watch exhaustion.
pub fn watch(
    paths: &[PathBuf],
    tx: Sender<DebounceEventResult>,
    debounce_ms: u64,
) -> notify::Result<Debouncer<notify::RecommendedWatcher>> {
    let debounce = Duration::from_millis(debounce_ms.max(50));
    let mut debouncer = new_debouncer(debounce, move |res: DebounceEventResult| {
        let _ = tx.blocking_send(res);
    })?;

    for root in paths {
        watch_dir_tree(&mut debouncer, root)?;
    }

    Ok(debouncer)
}

/// Register non-recursive watches on `root` and every non-ignored subdirectory underneath it.
pub fn watch_dir_tree(
    debouncer: &mut Debouncer<notify::RecommendedWatcher>,
    root: &Path,
) -> notify::Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .add_custom_ignore_filename(".contextignore")
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if is_default_ignored_dir_name(name) {
                        return false;
                    }
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            if let Err(err) = debouncer
                .watcher()
                .watch(entry.path(), RecursiveMode::NonRecursive)
            {
                eprintln!(
                    "Warning: failed to watch directory {:?}: {}",
                    entry.path(),
                    err
                );
            }
        }
    }

    Ok(())
}
