use anyhow::Result;
use contextd::api::{self, QueryResponse, StatusResponse};
use contextd::config::Config;
use contextd::daemon;
use contextd::indexer::embeddings::TextEmbedder;
use contextd::storage::db::{ChunkInput, Database};
use futures_util::future;
use reqwest::Client;
use serde_json::json;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::sleep;

struct DeterministicEmbedder;

impl TextEmbedder for DeterministicEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0_f32; 384];
        for (i, byte) in text.bytes().enumerate() {
            v[(byte as usize + i * 7) % 384] += 1.0;
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

/// Test 50 concurrent API requests hitting the WAL reader pool + hybrid search.
#[tokio::test]
async fn test_concurrent_api_requests() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("load_concurrent.db");
    let db = Database::open(&db_path, 384, "all-minilm-l6-v2").unwrap();
    let embedder: Arc<dyn TextEmbedder> = Arc::new(DeterministicEmbedder);

    for i in 0..30 {
        let content = format!("pub fn concurrent_handler_{}() -> usize {{ {} }}", i, i);
        let emb = embedder.embed(&content).unwrap();
        db.index_file_chunks(
            &format!("/repo/src/mod_{}.rs", i),
            1700000000 + i as u64,
            (1700000000 + i as i64) * 1_000_000_000,
            content.len() as u64,
            &format!("hash_{}", i),
            &[ChunkInput {
                start: 0,
                end: content.len() as u64,
                content: &content,
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();
    }

    let listener = api::bind_server("127.0.0.1", 0, None).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        let _ = api::serve_on_listener(listener, db, embedder, None).await;
    });

    let client = Client::new();
    let base_url = format!("http://{}", addr);

    let mut handles = Vec::new();
    for i in 0..50 {
        let client = client.clone();
        let url = base_url.clone();
        handles.push(tokio::spawn(async move {
            let resp = client
                .post(format!("{}/query", url))
                .json(&json!({
                    "query": format!("concurrent_handler_{}", i % 30),
                    "limit": 5
                }))
                .send()
                .await
                .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let body: QueryResponse = resp.json().await.ok()?;
            Some(!body.results.is_empty())
        }));
    }

    let results: Vec<bool> = future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.ok().flatten().unwrap_or(false))
        .collect();

    server_handle.abort();

    let success_count = results.iter().filter(|&&ok| ok).count();
    assert_eq!(
        success_count, 50,
        "All 50 concurrent hybrid queries must succeed with results, got {}/50",
        success_count
    );
}

/// Test rapid creation of 50 files watched by the daemon and verify they are indexed and queryable.
#[tokio::test]
async fn test_rapid_file_changes() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();
    let db_path = temp_dir.path().join("rapid.db");

    let mut config = Config::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.port = 0; // Ephemeral port
    config.storage.db_path = db_path.clone();
    config.storage.model_path = temp_dir.path().join("models");
    config.watch.paths = vec![watch_dir.clone()];
    config.watch.debounce_ms = 100;

    let db = Database::open(&db_path, 384, "all-minilm-l6-v2").unwrap();
    let db_check = db.clone();
    let embedder: Arc<dyn TextEmbedder> = Arc::new(DeterministicEmbedder);

    let daemon_handle = tokio::spawn(async move {
        let _ = daemon::run_with_components(config, db, embedder).await;
    });

    sleep(Duration::from_millis(300)).await;

    for i in 0..50 {
        let file_path = watch_dir.join(format!("file_{}.rs", i));
        fs::write(
            file_path,
            format!("pub fn rapid_fn_{}() -> i32 {{ {} }}", i, i),
        )
        .unwrap();
    }

    // Poll until all 50 files are indexed (up to 10s)
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut final_count = 0;
    while Instant::now() < deadline {
        final_count = db_check.get_stats().unwrap().file_count;
        if final_count >= 50 {
            break;
        }
        sleep(Duration::from_millis(150)).await;
    }

    daemon_handle.abort();
    assert_eq!(
        final_count, 50,
        "Expected all 50 rapidly created files to be indexed, got {}",
        final_count
    );
}

/// Test sustained query + status throughput over 2 seconds with zero errors.
#[tokio::test]
async fn test_sustained_load() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("sustained.db");
    let db = Database::open(&db_path, 384, "all-minilm-l6-v2").unwrap();
    let embedder: Arc<dyn TextEmbedder> = Arc::new(DeterministicEmbedder);

    let content = "pub fn sustained_search_target() { /* indexed payload */ }";
    let emb = embedder.embed(content).unwrap();
    db.index_file_chunks(
        "/repo/sustained.rs",
        1_700_000_000,
        1_700_000_000_000_000_000,
        content.len() as u64,
        "sustained_hash",
        &[ChunkInput {
            start: 0,
            end: content.len() as u64,
            content,
            embedding: Some(&emb),
            metadata: None,
        }],
    )
    .unwrap();

    let listener = api::bind_server("127.0.0.1", 0, None).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        let _ = api::serve_on_listener(listener, db, embedder, None).await;
    });

    let client = Client::new();
    let base_url = format!("http://{}", addr);

    let start = Instant::now();
    let mut query_count = 0u32;
    let mut success_count = 0u32;

    while start.elapsed() < Duration::from_secs(2) {
        let resp = client
            .post(format!("{}/query", base_url))
            .json(&json!({
                "query": "sustained_search_target",
                "limit": 3
            }))
            .send()
            .await;

        query_count += 1;
        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(parsed) = r.json::<QueryResponse>().await {
                    if !parsed.results.is_empty() {
                        success_count += 1;
                    }
                }
            }
        }
        sleep(Duration::from_millis(25)).await;
    }

    let status: StatusResponse = client
        .get(format!("{}/status", base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    server_handle.abort();

    assert_eq!(status.indexed_files, 1);
    assert!(query_count >= 10);
    assert_eq!(
        success_count, query_count,
        "Expected 100% sustained query success, got {}/{}",
        success_count, query_count
    );
}
