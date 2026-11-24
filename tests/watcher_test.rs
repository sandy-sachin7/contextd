use anyhow::Result;
use contextd::config::Config;
use contextd::daemon;
use contextd::indexer::embeddings::TextEmbedder;
use contextd::storage::db::Database;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::sleep;

struct MockEmbedder;

impl TextEmbedder for MockEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0_f32; 384];
        for (i, byte) in text.bytes().enumerate() {
            v[(byte as usize + i * 13) % 384] += 1.0;
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-6 {
            for x in &mut v {
                *x /= norm;
            }
        } else {
            v[0] = 1.0;
        }
        Ok(v)
    }

    fn embedding_dim(&self) -> usize {
        384
    }

    fn model_type(&self) -> &str {
        "all-minilm-l6-v2"
    }
}

struct BlockingEmbedder {
    started: mpsc::Sender<()>,
    completed: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    blocked: AtomicBool,
}

impl TextEmbedder for BlockingEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        MockEmbedder.embed(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if !self.blocked.swap(true, Ordering::SeqCst) {
            let _ = self.started.send(());
            self.release
                .lock()
                .unwrap()
                .recv()
                .expect("test must release blocked embedding");
            let _ = self.completed.send(());
        }
        texts.iter().map(|text| MockEmbedder.embed(text)).collect()
    }

    fn embedding_dim(&self) -> usize {
        384
    }

    fn model_type(&self) -> &str {
        "all-minilm-l6-v2"
    }
}

async fn wait_until<F>(timeout: Duration, mut pred: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(100)).await;
    }
    pred()
}

fn spawn_test_daemon(
    temp_dir: &TempDir,
    watch_dir: &std::path::Path,
    db_path: &std::path::Path,
    customize: impl FnOnce(&mut Config),
) -> (Database, tokio::task::JoinHandle<()>) {
    let embedder: Arc<dyn TextEmbedder> = Arc::new(MockEmbedder);
    spawn_test_daemon_with_embedder(temp_dir, watch_dir, db_path, embedder, customize)
}

fn spawn_test_daemon_with_embedder(
    temp_dir: &TempDir,
    watch_dir: &std::path::Path,
    db_path: &std::path::Path,
    embedder: Arc<dyn TextEmbedder>,
    customize: impl FnOnce(&mut Config),
) -> (Database, tokio::task::JoinHandle<()>) {
    let mut config = Config::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.port = 0;
    config.storage.db_path = db_path.to_path_buf();
    config.storage.model_path = temp_dir.path().join("models");
    config.watch.paths = vec![watch_dir.to_path_buf()];
    config.watch.debounce_ms = 100;
    customize(&mut config);

    let db = Database::open(db_path, 384, "all-minilm-l6-v2").unwrap();
    let db_ret = db.clone();

    let handle = tokio::spawn(async move {
        let _ = daemon::run_with_components(config, db, embedder).await;
    });
    (db_ret, handle)
}

#[tokio::test]
async fn test_rapid_file_creation_and_db_exclusion() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    // Place DB INSIDE the watched directory to verify zero self-indexing loops!
    let db_path = watch_dir.join("contextd.db");

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    sleep(Duration::from_millis(300)).await;

    for i in 0..25 {
        fs::write(
            watch_dir.join(format!("rapid_{}.rs", i)),
            format!("pub fn rapid_{}() {{}}", i),
        )
        .unwrap();
    }
    // Also create secret, binary, and oversized files that MUST be skipped
    fs::write(watch_dir.join(".env"), "API_KEY=supersecret").unwrap();
    fs::write(watch_dir.join("private.pem"), "-----BEGIN PRIVATE KEY-----").unwrap();
    fs::write(watch_dir.join("blob.dat"), b"bin\x00data\x00here").unwrap();

    let ok = wait_until(Duration::from_secs(8), || {
        db.get_stats().unwrap().file_count == 25
    })
    .await;

    let paths = db.list_indexed_paths().unwrap();
    handle.abort();

    assert!(ok, "Expected 25 indexed files, got {}", paths.len());
    assert!(
        !paths.iter().any(|p| p.contains("contextd.db")),
        "DB file must never be self-indexed"
    );
    assert!(
        !paths
            .iter()
            .any(|p| p.ends_with(".env") || p.ends_with(".pem") || p.ends_with(".dat")),
        "Secret and binary files must be excluded: {:?}",
        paths
    );
}

#[tokio::test]
async fn test_nested_directory_creation() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("nested.db");

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    sleep(Duration::from_millis(300)).await;

    let mut current_path = watch_dir.clone();
    for i in 0..5 {
        current_path = current_path.join(format!("level_{}", i));
        fs::create_dir(&current_path).unwrap();
        fs::write(
            current_path.join("file.rs"),
            format!("pub fn level_{}() {{}}", i),
        )
        .unwrap();
    }

    let ok = wait_until(Duration::from_secs(8), || {
        db.get_stats().unwrap().file_count == 5
    })
    .await;

    let count = db.get_stats().unwrap().file_count;
    handle.abort();
    assert!(ok, "Expected all 5 nested files indexed, got {}", count);
}

#[tokio::test]
async fn test_file_rename_prunes_old_and_indexes_new() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("rename.db");

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    sleep(Duration::from_millis(300)).await;

    for i in 0..5 {
        fs::write(
            watch_dir.join(format!("original_{}.rs", i)),
            format!("pub fn orig_{}() {{}}", i),
        )
        .unwrap();
    }

    assert!(
        wait_until(Duration::from_secs(6), || {
            db.get_stats().unwrap().file_count == 5
        })
        .await
    );

    for i in 0..5 {
        fs::rename(
            watch_dir.join(format!("original_{}.rs", i)),
            watch_dir.join(format!("renamed_{}.rs", i)),
        )
        .unwrap();
    }

    let ok = wait_until(Duration::from_secs(8), || {
        let paths = db.list_indexed_paths().unwrap();
        paths.len() == 5
            && paths.iter().all(|p| p.contains("renamed_"))
            && !paths.iter().any(|p| p.contains("original_"))
    })
    .await;

    let paths = db.list_indexed_paths().unwrap();
    handle.abort();
    assert!(
        ok,
        "Expected 5 renamed files and 0 originals, got {:?}",
        paths
    );
}

#[tokio::test]
async fn test_directory_delete_prunes_all_indexed_children() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("dir_delete.db");

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    sleep(Duration::from_millis(300)).await;

    let subdir = watch_dir.join("subdir");
    fs::create_dir(&subdir).unwrap();
    for i in 0..5 {
        fs::write(
            subdir.join(format!("file_{}.rs", i)),
            format!("pub fn sub_{}() {{}}", i),
        )
        .unwrap();
    }

    assert!(
        wait_until(Duration::from_secs(6), || {
            db.get_stats().unwrap().file_count == 5
        })
        .await
    );

    fs::remove_dir_all(&subdir).unwrap();

    let ok = wait_until(Duration::from_secs(8), || {
        let stats = db.get_stats().unwrap();
        stats.file_count == 0 && stats.chunk_count == 0
    })
    .await;

    let stats = db.get_stats().unwrap();
    handle.abort();
    assert!(
        ok,
        "Expected deleted dir files & chunks pruned, got files={} chunks={}",
        stats.file_count, stats.chunk_count
    );
}

#[tokio::test]
async fn test_untrusted_workspace_plugins_are_not_executed() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("plugins.db");
    let pwn_marker = temp_dir.path().join("pwned.txt");
    let marker_str = pwn_marker.to_string_lossy().to_string();

    fs::write(watch_dir.join("demo.custom"), "plain text content").unwrap();

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, move |cfg| {
        cfg.is_workspace_config = true;
        cfg.security.allow_workspace_plugins = false;
        let mut plugins = HashMap::new();
        plugins.insert(
            "custom".to_string(),
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("touch '{}' && echo pwned", marker_str),
            ],
        );
        cfg.plugins = plugins;
    });

    assert!(
        wait_until(Duration::from_secs(6), || {
            db.get_stats().unwrap().file_count == 1
        })
        .await
    );

    handle.abort();
    assert!(
        !pwn_marker.exists(),
        "Untrusted workspace plugin command MUST NOT execute"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_symlinked_file_outside_watch_root_is_not_indexed() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    let outside_dir = temp_dir.path().join("outside");
    fs::create_dir(&watch_dir).unwrap();
    fs::create_dir(&outside_dir).unwrap();

    let outside_file = outside_dir.join("external-secret.rs");
    fs::write(&outside_file, "fn outside_secret_marker() {}").unwrap();

    let db_path = temp_dir.path().join("symlink.db");
    let (db, first_handle) = spawn_test_daemon(&temp_dir, &outside_dir, &db_path, |_| {});
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&outside_file.to_string_lossy().to_string()))
                .unwrap_or(false)
        })
        .await
    );
    first_handle.abort();

    let link_path = watch_dir.join("linked.rs");
    symlink(&outside_file, &link_path).unwrap();

    // Use a fresh database: a daemon for one workspace must not index symlink
    // targets into its own index, regardless of what another workspace indexed.
    let watched_db_path = temp_dir.path().join("symlink-watched.db");
    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &watched_db_path, |_| {});
    sleep(Duration::from_millis(500)).await;

    let late_external_file = outside_dir.join("late-external.rs");
    fs::write(&late_external_file, "fn late_outside_secret_marker() {}").unwrap();
    let late_link = watch_dir.join("late-linked.rs");
    symlink(&late_external_file, &late_link).unwrap();
    sleep(Duration::from_secs(1)).await;

    let paths = db.list_indexed_paths().unwrap();
    let emb = MockEmbedder
        .embed("outside_secret_marker late_outside_secret_marker")
        .unwrap();
    let hits = db
        .search_chunks_hybrid(
            "outside_secret_marker late_outside_secret_marker",
            &emb,
            &Default::default(),
        )
        .unwrap();
    handle.abort();

    assert!(paths.is_empty(), "Symlink target was indexed: {:?}", paths);
    assert!(hits.is_empty(), "Symlink target was searchable: {:?}", hits);
}

#[cfg(unix)]
#[tokio::test]
async fn test_replacing_indexed_file_with_outside_symlink_prunes_old_path() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    let outside_dir = temp_dir.path().join("outside");
    fs::create_dir(&watch_dir).unwrap();
    fs::create_dir(&outside_dir).unwrap();

    let file_path = watch_dir.join("replace.rs");
    let outside_file = outside_dir.join("outside.rs");
    fs::write(&file_path, "fn indexed_before_symlink() {}").unwrap();
    fs::write(&outside_file, "fn outside_symlink_target() {}").unwrap();
    let path_key = file_path
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let db_path = temp_dir.path().join("symlink_replacement.db");
    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::remove_file(&file_path).unwrap();
    symlink(&outside_file, &file_path).unwrap();
    let pruned = wait_until(Duration::from_secs(6), || {
        db.list_indexed_paths()
            .map(|paths| !paths.contains(&path_key))
            .unwrap_or(false)
    })
    .await;
    let paths = db.list_indexed_paths().unwrap();
    let emb = MockEmbedder
        .embed("indexed_before_symlink outside_symlink_target")
        .unwrap();
    let hits = db
        .search_chunks_hybrid(
            "indexed_before_symlink outside_symlink_target",
            &emb,
            &Default::default(),
        )
        .unwrap();
    handle.abort();

    assert!(
        pruned,
        "Replacing a file with a symlink left its old index row"
    );
    assert!(
        paths.is_empty(),
        "Unexpected indexed paths remain: {:?}",
        paths
    );
    assert!(
        hits.is_empty(),
        "Old file content remains searchable: {:?}",
        hits
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_inflight_index_is_pruned_after_symlink_replacement() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    let outside_dir = temp_dir.path().join("outside");
    fs::create_dir(&watch_dir).unwrap();
    fs::create_dir(&outside_dir).unwrap();

    let file_path = watch_dir.join("inflight.rs");
    let outside_file = outside_dir.join("outside.rs");
    fs::write(&outside_file, "fn outside_target_content() {}").unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let embedder: Arc<dyn TextEmbedder> = Arc::new(BlockingEmbedder {
        started: started_tx,
        completed: completed_tx,
        release: Mutex::new(release_rx),
        blocked: AtomicBool::new(false),
    });
    let db_path = temp_dir.path().join("inflight_symlink.db");
    let (db, handle) =
        spawn_test_daemon_with_embedder(&temp_dir, &watch_dir, &db_path, embedder, |_| {});
    sleep(Duration::from_millis(300)).await;
    fs::write(&file_path, "fn old_indexed_content() {}").unwrap();
    let path_key = file_path
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let indexing_started = tokio::task::spawn_blocking(move || {
        started_rx.recv_timeout(Duration::from_secs(6)).is_ok()
    })
    .await
    .unwrap();
    assert!(
        indexing_started,
        "Index task did not reach the embedding gate"
    );

    fs::remove_file(&file_path).unwrap();
    symlink(&outside_file, &file_path).unwrap();
    sleep(Duration::from_millis(500)).await;
    release_tx.send(()).unwrap();

    let embedding_completed = tokio::task::spawn_blocking(move || {
        completed_rx.recv_timeout(Duration::from_secs(6)).is_ok()
    })
    .await
    .unwrap();
    assert!(embedding_completed, "Blocked embedding did not resume");

    let settled = wait_until(Duration::from_secs(6), || {
        db.list_indexed_paths()
            .map(|paths| !paths.contains(&path_key))
            .unwrap_or(false)
    })
    .await;
    let paths = db.list_indexed_paths().unwrap();
    let emb = MockEmbedder
        .embed("old_indexed_content outside_target_content")
        .unwrap();
    let hits = db
        .search_chunks_hybrid(
            "old_indexed_content outside_target_content",
            &emb,
            &Default::default(),
        )
        .unwrap();
    handle.abort();

    assert!(settled, "Old path remained indexed after the symlink event");
    assert!(
        paths.is_empty(),
        "Unexpected indexed paths remain: {:?}",
        paths
    );
    assert!(
        hits.is_empty(),
        "Stale content remains searchable: {:?}",
        hits
    );
}

#[tokio::test]
async fn test_changed_unindexable_file_prunes_old_chunks() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("unindexable.db");
    let file_path = watch_dir.join("changing.rs");
    let path_key = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.clone());
    let path_key = path_key.to_string_lossy().to_string();

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |cfg| {
        cfg.watch.max_file_size = 128;
    });
    sleep(Duration::from_millis(300)).await;

    let valid = b"pub fn changing_file() { let marker = 7; }";
    fs::write(&file_path, valid).unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, b"binary\0content").unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| !paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, valid).unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, vec![b'x'; 129]).unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| !paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, valid).unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, [b"invalid utf8 ".as_slice(), &[0xff]].concat()).unwrap();
    let cleared = wait_until(Duration::from_secs(6), || {
        db.list_indexed_paths()
            .map(|paths| !paths.contains(&path_key))
            .unwrap_or(false)
    })
    .await;
    let stats = db.get_stats().unwrap();
    handle.abort();

    assert!(cleared, "Invalid UTF-8 content left stale chunks indexed");
    assert_eq!(stats.chunk_count, 0);
}

#[cfg(unix)]
#[tokio::test]
async fn test_parser_failure_prunes_stale_chunks() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("parser_failure.db");
    let file_path = watch_dir.join("note.custom");
    let path_key = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.clone());
    let path_key = path_key.to_string_lossy().to_string();

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |cfg| {
        cfg.plugins.insert(
            "custom".to_string(),
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "grep -q reject \"$1\" && exit 1; cat \"$1\"".to_string(),
            ],
        );
    });
    sleep(Duration::from_millis(300)).await;

    fs::write(&file_path, "healthy parser content").unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            db.list_indexed_paths()
                .map(|paths| paths.contains(&path_key))
                .unwrap_or(false)
        })
        .await
    );

    fs::write(&file_path, "reject parser content").unwrap();
    let cleared = wait_until(Duration::from_secs(6), || {
        db.list_indexed_paths()
            .map(|paths| !paths.contains(&path_key))
            .unwrap_or(false)
    })
    .await;
    let stats = db.get_stats().unwrap();
    handle.abort();

    assert!(cleared, "Parser failure left stale chunks indexed");
    assert_eq!(stats.chunk_count, 0);
}

#[cfg(unix)]
#[tokio::test]
async fn test_permission_changes() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("perms.db");

    let (db, handle) = spawn_test_daemon(&temp_dir, &watch_dir, &db_path, |_| {});
    sleep(Duration::from_millis(300)).await;

    let file_path = watch_dir.join("perm_test.rs");
    fs::write(&file_path, "pub fn version_one() {}").unwrap();

    assert!(
        wait_until(Duration::from_secs(6), || {
            db.get_stats().unwrap().file_count == 1
        })
        .await
    );

    // Make unreadable briefly (daemon must survive without crashing)
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&file_path, perms).unwrap();
    sleep(Duration::from_millis(300)).await;

    // Make readable again and update content
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file_path, perms).unwrap();
    fs::write(&file_path, "pub fn version_two_updated() {}").unwrap();

    let ok = wait_until(Duration::from_secs(8), || {
        let emb = MockEmbedder.embed("version_two_updated").unwrap();
        let hits = db
            .search_chunks_hybrid("version_two_updated", &emb, &Default::default())
            .unwrap();
        hits.iter()
            .any(|h| h.content.contains("version_two_updated"))
    })
    .await;

    handle.abort();
    assert!(
        ok,
        "Daemon should re-index file once permissions are restored"
    );
}
