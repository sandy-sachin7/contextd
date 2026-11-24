use crate::api;
use crate::config::Config;
use crate::indexer::embeddings::{Embedder, TextEmbedder};
use crate::indexer::ignore::{is_default_excluded, is_default_ignored_dir_name, IgnoreChecker};
use crate::indexer::{chunker, plugins, watcher};
use crate::storage::db::{ChunkInput, Database, ReindexCheck};
use anyhow::Result;
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use notify_debouncer_mini::DebounceEventResult;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

const BINARY_SNIFF_BYTES: usize = 8192;

/// Canonicalize a path even when the leaf file does not exist yet (e.g., new DB file).
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canon) = fs::canonicalize(path) {
        return canon;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        let base = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if let Ok(canon_parent) = fs::canonicalize(base) {
            return canon_parent.join(name);
        }
    }
    path.to_path_buf()
}

/// Paths that must never be indexed (the database, its WAL/SHM/journal sidecars, and model dir).
#[derive(Clone, Debug)]
struct ExcludedTargets {
    db_files: Vec<PathBuf>,
    model_dir: PathBuf,
}

impl ExcludedTargets {
    fn from_config(config: &Config) -> Self {
        let db_norm = normalize_path(&config.storage.db_path);
        let db_str = db_norm.to_string_lossy().to_string();
        let db_files = vec![
            db_norm,
            PathBuf::from(format!("{}-wal", db_str)),
            PathBuf::from(format!("{}-shm", db_str)),
            PathBuf::from(format!("{}-journal", db_str)),
        ];
        let model_dir = normalize_path(&config.storage.model_path);
        Self {
            db_files,
            model_dir,
        }
    }

    fn matches(&self, path: &Path) -> bool {
        let norm = normalize_path(path);
        if self.db_files.iter().any(|p| p == &norm) {
            return true;
        }
        if norm.starts_with(&self.model_dir) {
            return true;
        }
        false
    }
}

/// Per-path coalescing coordinator: guarantees at most one active indexing task per path
/// and re-runs once if a new edit arrives while indexing is in flight.
#[derive(Default)]
struct PathCoordinator {
    /// Maps canonical path -> `dirty` flag (true means another event arrived while in flight).
    active: Mutex<HashMap<PathBuf, bool>>,
}

impl PathCoordinator {
    fn try_claim(&self, path: &Path) -> bool {
        let mut guard = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dirty) = guard.get_mut(path) {
            *dirty = true;
            false
        } else {
            guard.insert(path.to_path_buf(), false);
            true
        }
    }

    /// Returns `true` if another event arrived while this path was being processed
    /// (caller should loop and re-process), or `false` once released.
    fn finish_or_reclaim(&self, path: &Path) -> bool {
        let mut guard = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dirty) = guard.get_mut(path) {
            if *dirty {
                *dirty = false;
                return true;
            }
        }
        guard.remove(path);
        false
    }
}

pub async fn run(config: Config, exit_on_stdin_eof: bool) -> Result<()> {
    // 0. Optional stdin EOF monitoring (only when explicitly requested, e.g. editor extensions)
    if exit_on_stdin_eof {
        tokio::spawn(async {
            let mut buf = [0u8; 1];
            use tokio::io::AsyncReadExt;
            if let Ok(0) = tokio::io::stdin().read(&mut buf).await {
                eprintln!("Stdin closed (parent exited). Shutting down daemon.");
                std::process::exit(0);
            }
        });
    }

    // 1. Ensure model files exist (auto-download if missing)
    if crate::download::ensure_model_files(&config.storage.model_path, &config.storage.model_type)
        .await?
    {
        println!("Model files were downloaded.");
    }

    // 2. Initialize Embedder (offload model load to blocking pool)
    let storage_cfg = config.storage.clone();
    let embedder: Arc<dyn TextEmbedder> =
        Arc::new(tokio::task::spawn_blocking(move || Embedder::new(&storage_cfg)).await??);
    println!(
        "Embedder initialized ({}, {} dims) from {:?}",
        embedder.model_type(),
        embedder.embedding_dim(),
        config.storage.model_path
    );

    // 3. Initialize Storage with dynamic dimension matching the active embedder
    let db_path = config.storage.db_path.clone();
    let emb_dim = embedder.embedding_dim();
    let model_type = embedder.model_type().to_string();
    let db = tokio::task::spawn_blocking(move || Database::open(&db_path, emb_dim, &model_type))
        .await??;
    println!("Database initialized at {:?}", config.storage.db_path);

    run_with_components(config, db, embedder).await
}

/// Run the daemon loop with pre-initialized `Database` and `TextEmbedder` (also used in tests).
pub async fn run_with_components(
    config: Config,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
) -> Result<()> {
    let config = Arc::new(config);
    let excluded = Arc::new(ExcludedTargets::from_config(&config));
    let plugin_warned = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(PathCoordinator::default());
    let concurrency = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4);
    let semaphore = Arc::new(Semaphore::new(concurrency));

    let canonical_watch_roots: Vec<PathBuf> = config
        .watch
        .paths
        .iter()
        .map(|p| normalize_path(p))
        .collect();

    // 4. Start API Server in background (validating bind config up front)
    let api_db = db.clone();
    let api_embedder = embedder.clone();
    let api_host = config.server.host.clone();
    let api_port = config.server.port;
    let api_token = config.server.auth_token.clone();
    let api_listener = api::bind_server(&api_host, api_port, api_token.as_deref()).await?;
    let api_handle = tokio::spawn(async move {
        if let Err(err) =
            api::serve_on_listener(api_listener, api_db, api_embedder, api_token).await
        {
            eprintln!("API server stopped with error: {}", err);
        }
    });

    // 5. Initial Scan (await all tasks in JoinSet before logging completion)
    println!("Performing initial scan of {:?}", config.watch.paths);
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}")?);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let ignore_checkers: Vec<IgnoreChecker> = config
        .watch
        .paths
        .iter()
        .map(|p| IgnoreChecker::new(p))
        .collect();

    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut scan_tasks: JoinSet<()> = JoinSet::new();

    for root in &config.watch.paths {
        if !root.exists() {
            continue;
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

        for result in walker {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let norm_path = normalize_path(path);
                    if excluded.matches(&norm_path) || is_default_excluded(&norm_path, false) {
                        continue;
                    }
                    if ignore_checkers
                        .iter()
                        .any(|c| c.is_ignored(&norm_path, false))
                    {
                        continue;
                    }

                    seen_paths.insert(norm_path.to_string_lossy().to_string());

                    let Ok(permit) = semaphore.clone().acquire_owned().await else {
                        break;
                    };

                    let config = config.clone();
                    let db = db.clone();
                    let embedder = embedder.clone();
                    let pb = pb.clone();
                    let plugin_warned = plugin_warned.clone();

                    scan_tasks.spawn(async move {
                        pb.set_message(format!(
                            "Indexing {:?}",
                            norm_path.file_name().unwrap_or_default()
                        ));
                        index_file(norm_path, config, db, embedder, plugin_warned).await;
                        drop(permit);
                        pb.inc(1);
                    });
                }
                Err(err) => eprintln!("Error during scan: {}", err),
            }
        }
    }

    while let Some(res) = scan_tasks.join_next().await {
        if let Err(err) = res {
            eprintln!("Initial scan task panicked: {}", err);
        }
    }

    // Reconcile DB against disk: remove paths under watched roots that no longer exist or are ignored
    let reconcile_db = db.clone();
    let roots_for_reconcile = canonical_watch_roots.clone();
    let seen_for_reconcile = seen_paths;
    let pruned = tokio::task::spawn_blocking(move || -> Result<usize> {
        let indexed_paths = reconcile_db.list_indexed_paths()?;
        let mut removed = 0usize;
        for indexed in indexed_paths {
            let p = Path::new(&indexed);
            let under_watch = roots_for_reconcile.iter().any(|r| p.starts_with(r));
            if under_watch
                && !seen_for_reconcile.contains(&indexed)
                && reconcile_db.delete_file_by_path(&indexed)?
            {
                removed += 1;
            }
        }
        Ok(removed)
    })
    .await??;

    if pruned > 0 {
        println!(
            "Reconciled index: pruned {} removed/ignored file(s).",
            pruned
        );
    }
    pb.finish_with_message("Initial scan complete.");

    // 6. Start async Watcher (tokio::sync::mpsc — never blocks Tokio worker threads!)
    let (tx, mut rx) = mpsc::channel::<DebounceEventResult>(1024);
    let mut debouncer = watcher::watch(&config.watch.paths, tx, config.watch.debounce_ms)?;
    println!("Watching {:?}", config.watch.paths);

    // 7. Main Event Loop
    println!("Daemon main loop starting...");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Received shutdown signal. Stopping daemon...");
                break;
            }
            maybe_result = rx.recv() => {
                let Some(result) = maybe_result else {
                    break;
                };
                match result {
                    Ok(events) => {
                        let mut unique_paths = HashSet::new();
                        for event in events {
                            unique_paths.insert(normalize_path(&event.path));
                        }

                        for path in unique_paths {
                            if excluded.matches(&path) {
                                continue;
                            }

                            if !path.exists() {
                                // File or directory was deleted / moved away — prune from DB
                                let db_del = db.clone();
                                let path_str = path.to_string_lossy().to_string();
                                tokio::spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || -> Result<usize> {
                                        let mut removed = 0usize;
                                        if db_del.delete_file_by_path(&path_str)? {
                                            removed += 1;
                                        }
                                        removed += db_del.delete_files_under_prefix(&path_str)?;
                                        Ok(removed)
                                    })
                                    .await;
                                    match res {
                                        Ok(Ok(n)) if n > 0 => {
                                            println!("Pruned {} deleted path(s) for {:?}", n, path);
                                        }
                                        Ok(Err(e)) => eprintln!("Error pruning {:?}: {}", path, e),
                                        Err(e) => eprintln!("Prune task error: {}", e),
                                        _ => {}
                                    }
                                });
                                continue;
                            }

                            let is_dir = path.is_dir();
                            if is_default_excluded(&path, is_dir)
                                || ignore_checkers.iter().any(|c| c.is_ignored(&path, is_dir))
                            {
                                continue;
                            }

                            if is_dir {
                                // Dynamically watch newly created non-ignored directories & scan any files inside
                                if let Err(e) = watcher::watch_dir_tree(&mut debouncer, &path) {
                                    eprintln!("Warning: failed watching new dir {:?}: {}", path, e);
                                }
                                let walker = WalkBuilder::new(&path)
                                    .standard_filters(true)
                                    .add_custom_ignore_filename(".contextignore")
                                    .build();
                                for entry in walker.flatten() {
                                    let p = normalize_path(entry.path());
                                    if p.is_file()
                                        && !excluded.matches(&p)
                                        && !is_default_excluded(&p, false)
                                        && !ignore_checkers.iter().any(|c| c.is_ignored(&p, false))
                                    {
                                        schedule_index_path(
                                            p,
                                            coordinator.clone(),
                                            semaphore.clone(),
                                            config.clone(),
                                            db.clone(),
                                            embedder.clone(),
                                            plugin_warned.clone(),
                                        );
                                    }
                                }
                                continue;
                            }

                            schedule_index_path(
                                path,
                                coordinator.clone(),
                                semaphore.clone(),
                                config.clone(),
                                db.clone(),
                                embedder.clone(),
                                plugin_warned.clone(),
                            );
                        }
                    }
                    Err(e) => eprintln!("Watch error: {:?}", e),
                }
            }
        }
    }

    api_handle.abort();
    Ok(())
}

fn schedule_index_path(
    path: PathBuf,
    coordinator: Arc<PathCoordinator>,
    semaphore: Arc<Semaphore>,
    config: Arc<Config>,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    plugin_warned: Arc<AtomicBool>,
) {
    if !coordinator.try_claim(&path) {
        // Already indexing this path; marked dirty so the in-flight task will re-run it.
        return;
    }

    tokio::spawn(async move {
        loop {
            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                break;
            };
            if path.exists() && path.is_file() {
                index_file(
                    path.clone(),
                    config.clone(),
                    db.clone(),
                    embedder.clone(),
                    plugin_warned.clone(),
                )
                .await;
            }
            drop(permit);

            if !coordinator.finish_or_reclaim(&path) {
                break;
            }
        }
    });
}

fn extract_mtime(meta: &fs::Metadata) -> (u64, i64) {
    match meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        Some(dur) => (dur.as_secs(), dur.as_nanos().min(i64::MAX as u128) as i64),
        None => (0, 0),
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)];
    sample.contains(&0u8)
}

pub async fn index_file(
    path: PathBuf,
    config: Arc<Config>,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    plugin_warned: Arc<AtomicBool>,
) {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let path_str = path.to_string_lossy().to_string();

    // 1. Metadata & file-size cap check (offloaded to blocking pool)
    let meta_path = path.clone();
    let meta_res = tokio::task::spawn_blocking(move || fs::metadata(&meta_path)).await;
    let metadata = match meta_res {
        Ok(Ok(m)) => m,
        _ => return, // File vanished or unreadable
    };

    let file_size = metadata.len();
    if file_size > config.watch.max_file_size {
        return;
    }

    let (modified_secs, modified_ns) = extract_mtime(&metadata);
    let created_secs = metadata
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 2. Fast-path DB check via (mtime_ns, file_size)
    let check_db = db.clone();
    let check_path = path_str.clone();
    let reindex_state = match tokio::task::spawn_blocking(move || {
        check_db.check_reindex_state(&check_path, modified_ns, file_size)
    })
    .await
    {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => {
            eprintln!("DB reindex check error for {:?}: {}", path, e);
            return;
        }
        Err(e) => {
            eprintln!("Task error checking {:?}: {}", path, e);
            return;
        }
    };

    if reindex_state == ReindexCheck::Unchanged {
        return;
    }

    // 3. Read raw bytes + blake3 hash check (and binary NUL check for text files)
    let read_path = path.clone();
    let raw_bytes = match tokio::task::spawn_blocking(move || fs::read(&read_path)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            eprintln!("Failed reading {:?}: {}", path, e);
            return;
        }
        Err(_) => return,
    };

    let content_hash = blake3::hash(&raw_bytes).to_hex().to_string();

    if let ReindexCheck::CheckHash(ref stored_hash) = reindex_state {
        if stored_hash == &content_hash {
            let touch_db = db.clone();
            let touch_path = path_str.clone();
            let _ = tokio::task::spawn_blocking(move || {
                touch_db.touch_file_metadata(&touch_path, modified_secs, modified_ns, file_size)
            })
            .await;
            return;
        }
    }

    // 4. Parse / chunk (with plugin trust enforcement)
    let plugin_cmd = config.plugins.get(&ext).cloned();
    let use_plugin = if plugin_cmd.is_some() {
        if config.plugins_allowed() {
            true
        } else {
            if !plugin_warned.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "Warning: ignoring [plugins] from untrusted workspace-local contextd.toml. \
                     Pass --trust-workspace-plugins or set security.allow_workspace_plugins = true in user config."
                );
            }
            false
        }
    } else {
        false
    };

    let max_chunk_size = config.chunking.max_chunk_size;
    let overlap = config.chunking.overlap;

    let chunks_result = if use_plugin {
        let cmd = plugin_cmd.unwrap();
        match plugins::run_parser(&cmd, &path).await {
            Ok(content) => {
                let ext_clone = ext.clone();
                match tokio::task::spawn_blocking(move || {
                    chunker::chunk_by_type_with_limits(
                        &content,
                        &ext_clone,
                        max_chunk_size,
                        overlap,
                    )
                })
                .await
                {
                    Ok(res) => res,
                    Err(e) => Err(anyhow::anyhow!("Chunker task failed: {}", e)),
                }
            }
            Err(e) => Err(e),
        }
    } else if ext == "pdf" {
        let pdf_path = path.clone();
        match tokio::task::spawn_blocking(move || {
            let raw_chunks = chunker::chunk_pdf(&pdf_path)?;
            Ok(chunker::enforce_chunk_budget(
                raw_chunks,
                max_chunk_size,
                overlap,
            ))
        })
        .await
        {
            Ok(res) => res,
            Err(e) => Err(anyhow::anyhow!("PDF chunker task failed: {}", e)),
        }
    } else {
        if looks_binary(&raw_bytes) {
            return;
        }
        let content = match String::from_utf8(raw_bytes) {
            Ok(s) => s,
            Err(_) => return, // Skip non-UTF-8 binary files
        };
        let ext_clone = ext.clone();
        match tokio::task::spawn_blocking(move || {
            chunker::chunk_by_type_with_limits(&content, &ext_clone, max_chunk_size, overlap)
        })
        .await
        {
            Ok(res) => res,
            Err(e) => Err(anyhow::anyhow!("Chunker task failed: {}", e)),
        }
    };

    let chunks = match chunks_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error chunking file {:?}: {}", path, e);
            return;
        }
    };

    let file_metadata = serde_json::json!({
        "size": file_size,
        "created": created_secs,
        "modified": modified_secs,
        "extension": ext
    });

    // 5. Batch-embed + commit all chunks in a single SQLite transaction (offloaded to blocking pool)
    let commit_path = path_str.clone();
    let commit_hash = content_hash;
    let commit_db = db.clone();
    let commit_embedder = embedder.clone();

    let res = tokio::task::spawn_blocking(move || -> Result<usize> {
        let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings: Vec<Vec<f32>> = if texts.is_empty() {
            Vec::new()
        } else {
            let mut all = Vec::with_capacity(texts.len());
            for batch in texts.chunks(32) {
                let batch_embs = commit_embedder.embed_batch(batch)?;
                all.extend(batch_embs);
            }
            all
        };

        let merged_meta: Vec<String> = chunks
            .iter()
            .map(|chunk| {
                let mut m = file_metadata.clone();
                if let Some(cm) = &chunk.metadata {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(cm) {
                        if let (Some(obj), Some(parsed_obj)) =
                            (m.as_object_mut(), parsed.as_object())
                        {
                            for (k, v) in parsed_obj {
                                obj.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                m.to_string()
            })
            .collect();

        let inputs: Vec<ChunkInput<'_>> = chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| ChunkInput {
                start: chunk.start,
                end: chunk.end,
                content: &chunk.content,
                embedding: embeddings.get(idx).map(|v| v.as_slice()),
                metadata: merged_meta.get(idx).map(|s| s.as_str()),
            })
            .collect();

        commit_db.index_file_chunks(
            &commit_path,
            modified_secs,
            modified_ns,
            file_size,
            &commit_hash,
            &inputs,
        )?;
        Ok(chunks.len())
    })
    .await;

    match res {
        Ok(Ok(count)) => println!("Indexed {} chunks for {:?}", count, path),
        Ok(Err(e)) => eprintln!("Error indexing {:?}: {}", path, e),
        Err(e) => eprintln!("Indexing task panicked for {:?}: {}", path, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_excluded_targets_handles_relative_db_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.storage.db_path = dir.path().join("contextd.db");
        cfg.storage.model_path = dir.path().join("models");

        let excluded = ExcludedTargets::from_config(&cfg);
        assert!(excluded.matches(&dir.path().join("contextd.db")));
        assert!(excluded.matches(&dir.path().join("contextd.db-wal")));
        assert!(excluded.matches(&dir.path().join("contextd.db-shm")));
        assert!(excluded.matches(&dir.path().join("models/model.onnx")));
        assert!(!excluded.matches(&dir.path().join("src/main.rs")));
    }

    #[test]
    fn test_binary_sniff() {
        assert!(looks_binary(b"hello\x00world"));
        assert!(!looks_binary(b"fn main() { println!(\"ok\"); }\n"));
    }

    #[test]
    fn test_path_coordinator_coalesces_concurrent_events() {
        let coord = PathCoordinator::default();
        let p = PathBuf::from("/tmp/foo.rs");
        assert!(coord.try_claim(&p));
        // Second event while in flight is coalesced
        assert!(!coord.try_claim(&p));
        // Finishing sees dirty flag and reclaims once
        assert!(coord.finish_or_reclaim(&p));
        // No more events -> releases cleanly
        assert!(!coord.finish_or_reclaim(&p));
        // Subsequent event can claim again
        assert!(coord.try_claim(&p));
    }
}
