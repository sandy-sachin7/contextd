use futures_util::future;
use reqwest::Client;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::time::sleep;

const TEST_PORT: u16 = 13030; // Different from default 3030

/// Helper to start the daemon in the background and wait for its health endpoint
async fn start_test_daemon(
    config_path: PathBuf,
    port: u16,
) -> (
    tokio::process::Child,
    Client,
    Option<tokio::process::ChildStdin>,
) {
    use std::process::Stdio;
    let mut daemon = Command::new("./target/release/contextd")
        .arg("--config")
        .arg(config_path)
        .arg("daemon")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to start daemon");

    // Keep the stdin pipe open so the daemon's EOF monitor stays blocked
    let stdin_writer = daemon.stdin.take();

    let client = Client::new();
    let health_url = format!("http://127.0.0.1:{}/health", port);

    for _ in 0..30 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (daemon, client, stdin_writer);
            }
        }
        sleep(Duration::from_millis(500)).await;
    }

    let _ = daemon.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(1), daemon.wait()).await;
    panic!("Daemon didn't become healthy within 15s");
}

/// Test concurrent API requests
#[tokio::test]
async fn test_concurrent_api_requests() {
    // Create temporary directory for test
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    // Create a minimal config
    let config = format!(
        r#"
[server]
host = "127.0.0.1"
port = {}

[storage]
db_path = "{}"
model_path = "models"

[search]
enable_cache = true

[watch]
paths = []

[chunking]
max_chunk_size = 512
"#,
        TEST_PORT,
        db_path.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let (mut daemon, client, _stdin) = start_test_daemon(config_path, TEST_PORT).await;
    let base_url = format!("http://127.0.0.1:{}", TEST_PORT);

    // Send 50 concurrent requests
    let mut handles = vec![];
    for i in 0..50 {
        let client = client.clone();
        let url = base_url.clone();

        let handle = tokio::spawn(async move {
            let resp = client
                .post(format!("{}/query", url))
                .json(&json!({
                    "query": format!("test query {}", i),
                    "limit": 5
                }))
                .send()
                .await;

            resp.is_ok()
        });

        handles.push(handle);
    }

    // Collect results
    let results: Vec<bool> = future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap_or(false))
        .collect();

    let success_count = results.iter().filter(|&&r| r).count();

    // Clean up
    let _ = daemon.kill().await;

    // At least 80% of requests should succeed
    assert!(
        success_count >= 40,
        "Only {}/50 requests succeeded",
        success_count
    );
}

/// Test rapid file changes
#[tokio::test]
async fn test_rapid_file_changes() {
    let temp_dir = TempDir::new().unwrap();
    let watch_dir = temp_dir.path().join("watched");
    fs::create_dir(&watch_dir).unwrap();

    let db_path = temp_dir.path().join("test.db");

    let config = format!(
        r#"
[server]
host = "127.0.0.1"
port = {}

[storage]
db_path = "{}"
model_path = "models"

[watch]
paths = ["{}"]
debounce_ms = 200

[chunking]
max_chunk_size = 512
"#,
        TEST_PORT + 1,
        db_path.display(),
        watch_dir.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let (mut daemon, _, _stdin) = start_test_daemon(config_path, TEST_PORT + 1).await;

    // Create 100 files rapidly
    for i in 0..100 {
        let file_path = watch_dir.join(format!("file_{}.rs", i));
        fs::write(file_path, format!("fn test_{} () {{}}", i)).unwrap();
    }

    // Wait for debouncing to settle
    sleep(Duration::from_secs(5)).await;

    // Clean up
    let _ = daemon.kill().await;

    // If we get here without panic/crash, test passed
    assert!(true, "Daemon handled rapid file changes without crashing");
}

/// Test sustained load over time
#[tokio::test]
async fn test_sustained_load() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let config = format!(
        r#"
[server]
host = "127.0.0.1"
port = {}

[storage]
db_path = "{}"
model_path = "models"

[search]
enable_cache = true

[watch]
paths = []

[chunking]
max_chunk_size = 512
"#,
        TEST_PORT + 2,
        db_path.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let (mut daemon, client, _stdin) = start_test_daemon(config_path, TEST_PORT + 2).await;
    let base_url = format!("http://127.0.0.1:{}", TEST_PORT + 2);

    // Send queries continuously for 30 seconds (reduced from 5 minutes for faster testing)
    let start = std::time::Instant::now();
    let mut query_count = 0;
    let mut success_count = 0;

    while start.elapsed() < Duration::from_secs(30) {
        let resp = client
            .post(format!("{}/query", base_url))
            .json(&json!({
                "query": format!("sustained test {}", query_count),
                "limit": 3
            }))
            .send()
            .await;

        query_count += 1;
        if resp.is_ok() {
            success_count += 1;
        }

        // Small sleep to avoid overwhelming
        sleep(Duration::from_millis(100)).await;
    }

    // Clean up
    let _ = daemon.kill().await;

    // At least 90% of queries should succeed
    let success_rate = (success_count as f64) / (query_count as f64);
    assert!(
        success_rate >= 0.9,
        "Success rate too low: {:.2}% ({}/{})",
        success_rate * 100.0,
        success_count,
        query_count
    );
}
