use std::fs;
use std::path::PathBuf;
use std::process::Command;

const CONTEXTD_BIN: &str = env!("CARGO_BIN_EXE_contextd");

/// Test that ensure_model_files returns Ok(false) when all files exist.
#[test]
fn test_ensure_model_files_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("models");
    fs::create_dir_all(&model_path).unwrap();

    fs::write(model_path.join("model.onnx"), b"dummy model").unwrap();
    fs::write(model_path.join("tokenizer.json"), b"{}").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files(
        &model_path,
        "all-minilm-l6-v2",
    ));
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

/// Test that ensure_model_files rejects unknown model types immediately without network access.
#[test]
fn test_ensure_model_files_rejects_unknown_model() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("models");

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files(
        &model_path,
        "definitely-unknown-model",
    ));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unsupported model_type"));
}

/// Test that Embedder::new fails gracefully with non-existent model path.
#[test]
fn test_embedder_fails_without_model() {
    use contextd::config::StorageConfig;
    use contextd::indexer::embeddings::Embedder;

    let config = StorageConfig {
        db_path: PathBuf::from(":memory:"),
        model_path: PathBuf::from("i_do_not_exist_xyz"),
        model_type: "all-minilm-l6-v2".to_string(),
    };

    let err = match Embedder::new(&config) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("Expected error but got Ok"),
    };
    assert!(
        err.contains("model.onnx")
            || err.contains("No such file")
            || err.contains("not found")
            || err.contains("tokenizer.json"),
        "Unexpected error: {}",
        err
    );
}

/// Test CLI binary: --version, completions generation, and global --config flag placement.
#[test]
fn test_cli_binary_global_config_and_completions() {
    // 1. --version works
    let out = Command::new(CONTEXTD_BIN)
        .arg("--version")
        .output()
        .expect("Failed to execute contextd binary");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("contextd"));

    // 2. Shell completions work for bash/zsh/fish
    let out = Command::new(CONTEXTD_BIN)
        .args(["completions", "bash"])
        .output()
        .expect("Failed to execute completions");
    assert!(out.status.success());
    let comp = String::from_utf8_lossy(&out.stdout);
    assert!(comp.contains("_contextd"));

    // 3. Global `--config` accepted AFTER subcommand (`contextd mcp --config <path>`)
    //    and returns an error when the explicitly supplied config path does not exist.
    let out = Command::new(CONTEXTD_BIN)
        .args(["mcp", "--config", "/nonexistent/contextd_missing.toml"])
        .output()
        .expect("Failed to execute contextd mcp --config");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Failed to read config") || stderr.contains("contextd_missing.toml"),
        "Expected config error, got: {}",
        stderr
    );
}
