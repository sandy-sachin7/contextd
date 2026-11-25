use crate::api;
use crate::config::Config;
use crate::indexer::embeddings::{Embedder, TextEmbedder};
use crate::indexer::ignore::{is_default_excluded, is_default_ignored_dir_name, IgnoreChecker};
use crate::indexer::{chunker, plugins, safe_read, watcher};
use crate::storage::db::{ChunkInput, Database, ReindexCheck};
use anyhow::Result;
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use notify_debouncer_mini::DebounceEventResult;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

const BINARY_SNIFF_BYTES: usize = 8192;

/// Canonicalize a path even when some of its components do not exist yet.
/// Walk up to the nearest existing ancestor so that paths under a new model
/// directory resolve the same way as paths under an existing one.
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canon) = fs::canonicalize(path) {
        return canon;
    }
    let mut current = path;
    let mut tail = Vec::new();
    while let Some(name) = current.file_name() {
        tail.push(name.to_os_string());
        current = match current.parent() {
            Some(parent) => parent,
            None => break,
        };
        if let Ok(canon) = fs::canonicalize(current) {
            let mut result = canon;
            for part in tail.iter().rev() {
                result.push(part);
            }
            return result;
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

/// Only prune indexed paths that belong to a current watch root and were absent
/// from the initial scan. Index rows from other roots stay in the shared DB.
fn should_reconcile_prune(indexed: &str, roots: &[PathBuf], seen: &HashSet<String>) -> bool {
    let path = Path::new(indexed);
    roots.iter().any(|root| path.starts_with(root)) && !seen.contains(indexed)
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
    let canonical_watch_roots = Arc::new(canonical_watch_roots);

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

    let ignore_checkers: Arc<Vec<IgnoreChecker>> = Arc::new(
        config
            .watch
            .paths
            .iter()
            .map(|p| IgnoreChecker::new(p))
            .collect(),
    );

    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut scan_tasks: JoinSet<()> = JoinSet::new();

    // Offload synchronous WalkBuilder directory traversal to the blocking pool
    // and stream discovered paths into the async worker loop.
    let (scan_tx, mut scan_rx) = mpsc::channel::<PathBuf>(256);
    let walker_roots = config.watch.paths.clone();
    let walker_canonical_roots = canonical_watch_roots.clone();
    let walker_excluded = excluded.clone();
    let walker_ignore_checkers = ignore_checkers.clone();

    let walker_handle = tokio::task::spawn_blocking(move || {
        for root in &walker_roots {
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
                        if entry
                            .file_type()
                            .map(|kind| kind.is_symlink())
                            .unwrap_or(false)
                            || !path.is_file()
                        {
                            continue;
                        }
                        let norm_path = normalize_path(path);
                        if !is_under_watch_root(&norm_path, &walker_canonical_roots) {
                            continue;
                        }
                        if walker_excluded.matches(&norm_path)
                            || is_default_excluded(&norm_path, false)
                        {
                            continue;
                        }
                        if walker_ignore_checkers
                            .iter()
                            .any(|c| c.is_ignored(&norm_path, false))
                        {
                            continue;
                        }

                        if scan_tx.blocking_send(norm_path).is_err() {
                            return;
                        }
                    }
                    Err(err) => eprintln!("Error during scan: {}", err),
                }
            }
        }
    });

    // Concurrent file preparation + cross-file length-sorted ONNX batching
    let (prepared_tx, mut prepared_rx) = mpsc::channel::<PreparedFile>(64);
    let batch_db = db.clone();
    let batch_embedder = embedder.clone();
    let batch_pb = pb.clone();
    let consumer_handle = tokio::spawn(async move {
        const INITIAL_SCAN_BATCH_CHUNKS: usize = 128;
        let mut pending: Vec<PreparedFile> = Vec::new();
        let mut pending_chunks = 0usize;

        while let Some(prepared) = prepared_rx.recv().await {
            pending_chunks += prepared.chunks.len();
            pending.push(prepared);
            if pending_chunks >= INITIAL_SCAN_BATCH_CHUNKS {
                let batch = std::mem::take(&mut pending);
                let files_in_batch = batch.len() as u64;
                pending_chunks = 0;
                commit_prepared_batch(batch, batch_db.clone(), batch_embedder.clone()).await;
                batch_pb.inc(files_in_batch);
            }
        }
        if !pending.is_empty() {
            let files_in_batch = pending.len() as u64;
            commit_prepared_batch(pending, batch_db, batch_embedder).await;
            batch_pb.inc(files_in_batch);
        }
    });

    while let Some(norm_path) = scan_rx.recv().await {
        seen_paths.insert(norm_path.to_string_lossy().to_string());

        let Ok(permit) = semaphore.clone().acquire_owned().await else {
            break;
        };

        let config = config.clone();
        let db = db.clone();
        let pb = pb.clone();
        let plugin_warned = plugin_warned.clone();
        let watch_roots = canonical_watch_roots.clone();
        let prepared_tx = prepared_tx.clone();

        scan_tasks.spawn(async move {
            pb.set_message(format!(
                "Indexing {:?}",
                norm_path.file_name().unwrap_or_default()
            ));
            let prepared = prepare_file_for_indexing(
                norm_path,
                config,
                db,
                plugin_warned,
                watch_roots.as_ref().clone(),
            )
            .await;
            drop(permit);
            match prepared {
                Some(file) => {
                    let _ = prepared_tx.send(file).await;
                }
                None => pb.inc(1),
            }
        });
    }

    if let Err(err) = walker_handle.await {
        eprintln!("Initial directory walk task failed: {}", err);
    }

    while let Some(res) = scan_tasks.join_next().await {
        if let Err(err) = res {
            eprintln!("Initial scan task panicked: {}", err);
        }
    }
    drop(prepared_tx);
    if let Err(err) = consumer_handle.await {
        eprintln!("Initial scan batch consumer panicked: {}", err);
    }

    // Reconcile DB against disk: remove paths under watched roots that no longer exist or are ignored
    let reconcile_db = db.clone();
    let roots_for_reconcile = canonical_watch_roots.clone();
    let seen_for_reconcile = seen_paths;
    let pruned = tokio::task::spawn_blocking(move || -> Result<usize> {
        let indexed_paths = reconcile_db.list_indexed_paths()?;
        let mut removed = 0usize;
        for indexed in indexed_paths {
            if should_reconcile_prune(&indexed, &roots_for_reconcile, &seen_for_reconcile)
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
                        let mut symlink_paths = HashSet::new();
                        let event_paths: HashSet<PathBuf> =
                            events.into_iter().map(|event| event.path).collect();
                        for event_path in event_paths {
                            if is_symlink(&event_path) {
                                symlink_paths
                                    .insert(normalize_path_without_final_component(&event_path));
                            } else {
                                unique_paths.insert(normalize_path(&event_path));
                            }
                        }

                        for path in symlink_paths {
                            if !is_under_watch_root(&path, &canonical_watch_roots) {
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
                                canonical_watch_roots.clone(),
                            );
                        }

                        for path in unique_paths {
                            if !is_under_watch_root(&path, &canonical_watch_roots) {
                                continue;
                            }
                            if excluded.matches(&path) {
                                continue;
                            }

                            if !path.exists() {
                                schedule_index_path(
                                    path,
                                    coordinator.clone(),
                                    semaphore.clone(),
                                    config.clone(),
                                    db.clone(),
                                    embedder.clone(),
                                    plugin_warned.clone(),
                                    canonical_watch_roots.clone(),
                                );
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
                                let scan_dir = path.clone();
                                let scan_roots = canonical_watch_roots.clone();
                                let scan_excluded = excluded.clone();
                                let scan_checkers = ignore_checkers.clone();
                                let discovered = tokio::task::spawn_blocking(move || {
                                    let mut files = Vec::new();
                                    let walker = WalkBuilder::new(&scan_dir)
                                        .standard_filters(true)
                                        .add_custom_ignore_filename(".contextignore")
                                        .filter_entry(|entry| {
                                            if entry.depth() == 0 {
                                                return true;
                                            }
                                            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                                            {
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
                                        if entry
                                            .file_type()
                                            .map(|kind| kind.is_symlink())
                                            .unwrap_or(false)
                                        {
                                            continue;
                                        }
                                        let p = normalize_path(entry.path());
                                        if is_under_watch_root(&p, &scan_roots)
                                            && p.is_file()
                                            && !scan_excluded.matches(&p)
                                            && !is_default_excluded(&p, false)
                                            && !scan_checkers.iter().any(|c| c.is_ignored(&p, false))
                                        {
                                            files.push(p);
                                        }
                                    }
                                    files
                                })
                                .await;
                                match discovered {
                                    Ok(files) => {
                                        for p in files {
                                            schedule_index_path(
                                                p,
                                                coordinator.clone(),
                                                semaphore.clone(),
                                                config.clone(),
                                                db.clone(),
                                                embedder.clone(),
                                                plugin_warned.clone(),
                                                canonical_watch_roots.clone(),
                                            );
                                        }
                                    }
                                    Err(err) => {
                                        eprintln!(
                                            "Warning: directory scan task failed for {:?}: {}",
                                            path, err
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
                                canonical_watch_roots.clone(),
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

#[allow(clippy::too_many_arguments)]
fn schedule_index_path(
    path: PathBuf,
    coordinator: Arc<PathCoordinator>,
    semaphore: Arc<Semaphore>,
    config: Arc<Config>,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    plugin_warned: Arc<AtomicBool>,
    watch_roots: Arc<Vec<PathBuf>>,
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
            if !path.exists() || is_symlink(&path) {
                prune_indexed_path_and_children(&db, &path).await;
            } else if path.is_file() {
                index_file_with_roots(
                    path.clone(),
                    config.clone(),
                    db.clone(),
                    embedder.clone(),
                    plugin_warned.clone(),
                    watch_roots.as_ref().clone(),
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

async fn prune_indexed_path_and_children(db: &Database, path: &Path) {
    let db = db.clone();
    let path = path.to_string_lossy().to_string();
    let display_path = path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<usize> {
        let mut removed = 0usize;
        if db.delete_file_by_path(&path)? {
            removed += 1;
        }
        removed += db.delete_files_under_prefix(&path)?;
        Ok(removed)
    })
    .await;
    match result {
        Ok(Ok(count)) if count > 0 => {
            println!("Pruned {} indexed path(s) for {:?}", count, display_path);
        }
        Ok(Err(err)) => eprintln!("Failed to prune indexed path {:?}: {}", display_path, err),
        Err(err) => eprintln!("Failed to prune indexed path {:?}: {}", display_path, err),
        _ => {}
    }
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

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn normalize_path_without_final_component(path: &Path) -> PathBuf {
    let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    fs::canonicalize(parent)
        .map(|canonical_parent| canonical_parent.join(file_name))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn is_under_watch_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

async fn prune_indexed_file(db: &Database, path: &str) {
    let db = db.clone();
    let path = path.to_owned();
    let display_path = path.clone();
    let result = tokio::task::spawn_blocking(move || db.delete_file_by_path(&path)).await;
    let error = match result {
        Ok(Ok(_)) => return,
        Ok(Err(err)) => err.to_string(),
        Err(err) => err.to_string(),
    };
    eprintln!(
        "Failed to prune stale index for {:?}: {}",
        display_path, error
    );
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
    let watch_roots: Vec<PathBuf> = config
        .watch
        .paths
        .iter()
        .map(|root| fs::canonicalize(root).unwrap_or_else(|_| normalize_path(root)))
        .collect();
    index_file_with_roots(path, config, db, embedder, plugin_warned, watch_roots).await;
}

struct PreparedFile {
    path: PathBuf,
    path_str: String,
    modified_secs: u64,
    modified_ns: i64,
    file_size: u64,
    content_hash: String,
    file_metadata: serde_json::Value,
    chunks: Vec<chunker::Chunk>,
}

async fn index_file_with_roots(
    path: PathBuf,
    config: Arc<Config>,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    plugin_warned: Arc<AtomicBool>,
    watch_roots: Vec<PathBuf>,
) {
    if let Some(prepared) =
        prepare_file_for_indexing(path, config, db.clone(), plugin_warned, watch_roots).await
    {
        commit_prepared_batch(vec![prepared], db, embedder).await;
    }
}

async fn prepare_file_for_indexing(
    path: PathBuf,
    config: Arc<Config>,
    db: Database,
    plugin_warned: Arc<AtomicBool>,
    watch_roots: Vec<PathBuf>,
) -> Option<PreparedFile> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let path_str = path.to_string_lossy().to_string();

    // Open once beneath a configured root. Metadata and content use this handle.
    let open_path = path.clone();
    let open_res = tokio::task::spawn_blocking(move || {
        let file = safe_read::open_file_under_roots(&open_path, &watch_roots)?;
        let metadata = file.metadata()?;
        Ok::<_, anyhow::Error>((file, metadata))
    })
    .await;
    let (file, metadata) = match open_res {
        Ok(Ok(opened)) => opened,
        Ok(Err(err)) => {
            // Transient open failures must not drop a previously valid index row.
            eprintln!("Failed opening {:?} beneath watch roots: {}", path, err);
            return None;
        }
        Err(err) => {
            eprintln!("Index task failed for {:?}: {}", path, err);
            return None;
        }
    };

    let file_size = metadata.len();
    if file_size > config.watch.max_file_size {
        prune_indexed_file(&db, &path_str).await;
        return None;
    }

    let (modified_secs, modified_ns) = extract_mtime(&metadata);
    let created_secs = metadata
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Fast-path DB check via (mtime_ns, file_size)
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
            return None;
        }
        Err(e) => {
            eprintln!("Task error checking {:?}: {}", path, e);
            return None;
        }
    };

    if reindex_state == ReindexCheck::Unchanged {
        return None;
    }

    // Read through the same handle and enforce the limit even if the file grows.
    let max_bytes = config.watch.max_file_size;
    let read_res = tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(bytes)
    })
    .await;
    let raw_bytes = match read_res {
        Ok(Ok(bytes)) if bytes.len() as u64 <= config.watch.max_file_size => bytes,
        Ok(Ok(_)) => {
            prune_indexed_file(&db, &path_str).await;
            return None;
        }
        Ok(Err(e)) => {
            eprintln!("Failed reading {:?}: {}", path, e);
            return None;
        }
        Err(e) => {
            eprintln!("Index task failed reading {:?}: {}", path, e);
            return None;
        }
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
            return None;
        }
    }

    // Parse / chunk (with plugin trust enforcement)
    let active_plugin_cmd = match config.plugins.get(&ext) {
        Some(cmd) if config.plugins_allowed() => Some(cmd.clone()),
        Some(_) => {
            if !plugin_warned.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "Warning: ignoring [plugins] from untrusted workspace-local contextd.toml. \
                     Pass --trust-workspace-plugins or set security.allow_workspace_plugins = true in user config."
                );
            }
            None
        }
        None => None,
    };

    let max_chunk_size = config.chunking.max_chunk_size;
    let overlap = config.chunking.overlap;

    let chunks_result = if let Some(cmd) = active_plugin_cmd {
        let suffix = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}"))
            .unwrap_or_default();
        let snapshot_res = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
            let mut snapshot = tempfile::Builder::new().suffix(&suffix).tempfile()?;
            snapshot.write_all(&raw_bytes)?;
            snapshot.flush()?;
            Ok(snapshot)
        })
        .await;
        let snapshot = match snapshot_res {
            Ok(Ok(snapshot)) => snapshot,
            Ok(Err(error)) => {
                // Preserve last known good index on transient parser snapshot errors.
                eprintln!("Failed to create parser snapshot for {:?}: {}", path, error);
                return None;
            }
            Err(error) => {
                eprintln!("Parser snapshot task failed for {:?}: {}", path, error);
                return None;
            }
        };
        let plugin_res = plugins::run_parser(&cmd, snapshot.path()).await;
        let _ = tokio::task::spawn_blocking(move || drop(snapshot)).await;
        match plugin_res {
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
            chunker::chunk_pdf_bytes_with_limits(&raw_bytes, &pdf_path, max_chunk_size, overlap)
        })
        .await
        {
            Ok(res) => res,
            Err(e) => Err(anyhow::anyhow!("PDF chunker task failed: {}", e)),
        }
    } else {
        if looks_binary(&raw_bytes) {
            prune_indexed_file(&db, &path_str).await;
            return None;
        }
        let content = match String::from_utf8(raw_bytes) {
            Ok(s) => s,
            Err(_) => {
                prune_indexed_file(&db, &path_str).await;
                return None;
            }
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
            // Preserve last known good index on transient parser/chunking failures.
            eprintln!("Error chunking file {:?}: {}", path, e);
            return None;
        }
    };

    let file_metadata = serde_json::json!({
        "size": file_size,
        "created": created_secs,
        "modified": modified_secs,
        "extension": ext
    });

    Some(PreparedFile {
        path,
        path_str,
        modified_secs,
        modified_ns,
        file_size,
        content_hash,
        file_metadata,
        chunks,
    })
}

async fn commit_prepared_batch(
    batch: Vec<PreparedFile>,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
) {
    if batch.is_empty() {
        return;
    }

    let res = tokio::task::spawn_blocking(move || -> Result<()> {
        let total_chunks: usize = batch.iter().map(|f| f.chunks.len()).sum();
        let mut texts: Vec<&str> = Vec::with_capacity(total_chunks);
        for file in &batch {
            for chunk in &file.chunks {
                texts.push(chunk.content.as_str());
            }
        }

        let embeddings = if texts.is_empty() {
            Vec::new()
        } else {
            embedder.embed_batch(&texts)?
        };

        let mut offset = 0usize;
        for file in &batch {
            let count = file.chunks.len();
            let file_embs = &embeddings[offset..offset + count];
            offset += count;

            let merged_meta: Vec<String> = file
                .chunks
                .iter()
                .map(|chunk| {
                    let mut m = file.file_metadata.clone();
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

            let inputs: Vec<ChunkInput<'_>> = file
                .chunks
                .iter()
                .enumerate()
                .map(|(idx, chunk)| ChunkInput {
                    start: chunk.start,
                    end: chunk.end,
                    content: &chunk.content,
                    embedding: file_embs.get(idx).map(|v| v.as_slice()),
                    metadata: merged_meta.get(idx).map(|s| s.as_str()),
                })
                .collect();

            match db.index_file_chunks(
                &file.path_str,
                file.modified_secs,
                file.modified_ns,
                file.file_size,
                &file.content_hash,
                &inputs,
            ) {
                Ok(_) => println!("Indexed {} chunks for {:?}", count, file.path),
                Err(e) => {
                    // Preserve last known good index if DB commit fails.
                    eprintln!("Error indexing {:?}: {}", file.path, e);
                }
            }
        }
        Ok(())
    })
    .await;

    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("Error embedding batch: {}", e),
        Err(e) => eprintln!("Indexing batch task panicked: {}", e),
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
    fn test_reconcile_keeps_paths_outside_current_watch_roots() {
        let roots = vec![PathBuf::from("/project-b")];
        let seen = HashSet::from(["/project-b/src/main.rs".to_string()]);

        assert!(should_reconcile_prune(
            "/project-b/src/deleted.rs",
            &roots,
            &seen
        ));
        assert!(!should_reconcile_prune(
            "/project-a/src/main.rs",
            &roots,
            &seen
        ));
        assert!(!should_reconcile_prune(
            "/project-b/src/main.rs",
            &roots,
            &seen
        ));
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

    struct ToggleEmbedder {
        fail: AtomicBool,
    }

    impl TextEmbedder for ToggleEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            if self.fail.load(Ordering::Relaxed) {
                anyhow::bail!("simulated embed failure");
            }
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }

        fn embedding_dim(&self) -> usize {
            4
        }

        fn model_type(&self) -> &str {
            "test-model"
        }
    }

    #[tokio::test]
    async fn test_index_file_preserves_last_known_good_index_on_embed_failure() {
        let dir = tempfile::tempdir().unwrap();
        let watch_dir = fs::canonicalize(dir.path()).unwrap();
        let file_path = watch_dir.join("auth.rs");
        fs::write(&file_path, "pub fn verify_token() -> bool { true }\n").unwrap();

        let mut cfg = Config::default();
        cfg.watch.paths = vec![watch_dir.clone()];
        let db = Database::open(watch_dir.join("test.db"), 4, "test-model").unwrap();
        let embedder = Arc::new(ToggleEmbedder {
            fail: AtomicBool::new(false),
        });
        let cfg = Arc::new(cfg);
        let warned = Arc::new(AtomicBool::new(false));

        index_file(
            file_path.clone(),
            cfg.clone(),
            db.clone(),
            embedder.clone(),
            warned.clone(),
        )
        .await;
        assert_eq!(db.get_stats().unwrap().file_count, 1);

        // Modify file and fail embedding: previously indexed chunks must remain intact.
        embedder.fail.store(true, Ordering::Relaxed);
        fs::write(&file_path, "pub fn verify_token_v2() -> bool { false }\n").unwrap();

        index_file(file_path, cfg, db.clone(), embedder, warned).await;

        assert_eq!(db.get_stats().unwrap().file_count, 1);
        let hits = db
            .search_chunks_fts(
                "verify_token",
                &crate::storage::db::SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].content.contains("verify_token()"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_index_file_preserves_last_known_good_index_on_plugin_failure() {
        let dir = tempfile::tempdir().unwrap();
        let watch_dir = fs::canonicalize(dir.path()).unwrap();
        let file_path = watch_dir.join("notes.custom");
        fs::write(&file_path, "initial document text").unwrap();

        let mut cfg = Config::default();
        cfg.watch.paths = vec![watch_dir.clone()];
        cfg.security.allow_workspace_plugins = true;
        cfg.plugins
            .insert("custom".to_string(), vec!["cat".to_string()]);

        let db = Database::open(watch_dir.join("test.db"), 4, "test-model").unwrap();
        let embedder = Arc::new(ToggleEmbedder {
            fail: AtomicBool::new(false),
        });
        let warned = Arc::new(AtomicBool::new(false));

        index_file(
            file_path.clone(),
            Arc::new(cfg.clone()),
            db.clone(),
            embedder.clone(),
            warned.clone(),
        )
        .await;
        assert_eq!(db.get_stats().unwrap().file_count, 1);

        // Switch plugin to failing command and update file: last known good index must survive.
        cfg.plugins
            .insert("custom".to_string(), vec!["false".to_string()]);
        fs::write(&file_path, "updated document text").unwrap();

        index_file(file_path, Arc::new(cfg), db.clone(), embedder, warned).await;
        assert_eq!(db.get_stats().unwrap().file_count, 1);
        let hits = db
            .search_chunks_fts("initial", &crate::storage::db::SearchOptions::default())
            .unwrap();
        assert_eq!(hits.len(), 1);
    }
}
