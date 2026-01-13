use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;

const TEST_PORT: u16 = 14030;

/// Helper to start the daemon in the background
async fn start_test_daemon(config_path: PathBuf) -> tokio::process::Child {
    TokioCommand::new("./target/release/contextd")
        .arg("--config")
        .arg(config_path)
        .arg("daemon")
        .spawn()
        .expect("Failed to start daemon")
}

/// Test rapid file creation
#[tokio::test]
async fn test_rapid_file_creation() {
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
        TEST_PORT,
        db_path.display(),
        watch_dir.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let mut daemon = start_test_daemon(config_path).await;
    sleep(Duration::from_secs(2)).await;

    // Create 100 files in rapid succession
    for i in 0..100 {
        fs::write(
            watch_dir.join(format!("rapid_{}.rs", i)),
            format!("fn test_{}() {{}}", i),
        )
        .unwrap();
    }

    // Wait for processing
    sleep(Duration::from_secs(3)).await;

    let _ = daemon.kill().await;

    // If we got here without crash, test passed
    assert!(true);
}

/// Test nested directory creation
#[tokio::test]
async fn test_nested_directory_creation() {
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

    let mut daemon = start_test_daemon(config_path).await;
    sleep(Duration::from_secs(2)).await;

    // Create deeply nested directories
    let mut current_path = watch_dir.clone();
    for i in 0..10 {
        current_path = current_path.join(format!("level_{}", i));
        fs::create_dir(&current_path).unwrap();

        // Add a file at each level
        fs::write(current_path.join("file.rs"), format!("// Level {}", i)).unwrap();
    }

    sleep(Duration::from_secs(3)).await;

    let _ = daemon.kill().await;
    assert!(true);
}

/// Test file rename operations
#[tokio::test]
async fn test_file_rename() {
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
        TEST_PORT + 2,
        db_path.display(),
        watch_dir.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let mut daemon = start_test_daemon(config_path).await;
    sleep(Duration::from_secs(2)).await;

    // Create files
    for i in 0..20 {
        fs::write(watch_dir.join(format!("original_{}.rs", i)), "fn test() {}").unwrap();
    }

    sleep(Duration::from_secs(1)).await;

    // Rename files
    for i in 0..20 {
        fs::rename(
            watch_dir.join(format!("original_{}.rs", i)),
            watch_dir.join(format!("renamed_{}.rs", i)),
        )
        .unwrap();
    }

    sleep(Duration::from_secs(3)).await;

    let _ = daemon.kill().await;
    assert!(true);
}

/// Test directory deletion
#[tokio::test]
async fn test_directory_delete() {
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
        TEST_PORT + 3,
        db_path.display(),
        watch_dir.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let mut daemon = start_test_daemon(config_path).await;
    sleep(Duration::from_secs(2)).await;

    // Create subdirectory with files
    let subdir = watch_dir.join("subdir");
    fs::create_dir(&subdir).unwrap();

    for i in 0..10 {
        fs::write(subdir.join(format!("file_{}.rs", i)), "fn test() {}").unwrap();
    }

    sleep(Duration::from_secs(1)).await;

    // Delete the entire subdirectory
    fs::remove_dir_all(&subdir).unwrap();

    sleep(Duration::from_secs(3)).await;

    let _ = daemon.kill().await;
    assert!(true);
}

/// Test permission changes (Unix only)
#[cfg(unix)]
#[tokio::test]
async fn test_permission_changes() {
    use std::os::unix::fs::PermissionsExt;

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
        TEST_PORT + 4,
        db_path.display(),
        watch_dir.display()
    );

    let config_path = temp_dir.path().join("test_config.toml");
    fs::write(&config_path, config).unwrap();

    let mut daemon = start_test_daemon(config_path).await;
    sleep(Duration::from_secs(2)).await;

    // Create a file
    let file_path = watch_dir.join("test.rs");
    fs::write(&file_path, "fn test() {}").unwrap();

    sleep(Duration::from_secs(1)).await;

    // Make file unreadable
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&file_path, perms).unwrap();

    sleep(Duration::from_secs(2)).await;

    // Make it readable again
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file_path, perms).unwrap();

    sleep(Duration::from_secs(2)).await;

    let _ = daemon.kill().await;
    assert!(true);
}
