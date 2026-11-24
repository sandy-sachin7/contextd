use anyhow::Result;
use contextd::config::Config;
use contextd::daemon;
use contextd::indexer::embeddings::TextEmbedder;
use contextd::storage::db::Database;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
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
    let embedder: Arc<dyn TextEmbedder> = Arc::new(MockEmbedder);

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
