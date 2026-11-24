use std::fs;
use std::path::PathBuf;
use std::process::Command;

const CONTEXTD_BIN: &str = env!("CARGO_BIN_EXE_contextd");

/// Test that ensure_model_files returns Ok(false) when verified files exist on disk,
/// and refuses/redownloads when cached files fail SHA-256 verification.
#[test]
fn test_ensure_model_files_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("models");
    let model_cache = contextd::download::model_cache_dir(&model_path, "all-minilm-l6-v2");
    fs::create_dir_all(&model_cache).unwrap();

    fs::write(model_cache.join("model.onnx"), b"dummy model").unwrap();
    fs::write(model_cache.join("tokenizer.json"), b"{}").unwrap();

    // SHA-256("dummy model") and SHA-256("{}")
    let manifest = contextd::download::ModelDownloadManifest {
        model_type: "all-minilm-l6-v2",
        model_url: "http://127.0.0.1:1/unreachable-model.onnx",
        model_sha256: "0f07f97f8766046062e841e4eb059393462b0fe4d3c05e056430cb0a6a1e1733",
        tokenizer_url: "http://127.0.0.1:1/unreachable-tokenizer.json",
        tokenizer_sha256: "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files_with_manifest(
        &model_path,
        &manifest,
    ));
    assert!(result.is_ok(), "unexpected error: {:?}", result);
    assert!(!result.unwrap());
    assert!(contextd::download::model_files_exist(
        &model_path,
        "all-minilm-l6-v2"
    ));
    assert!(!contextd::download::model_files_exist(
        &model_path,
        "all-mpnet-base-v2"
    ));

    // Corrupted cached file must NOT be silently skipped (attempts re-download)
    fs::write(model_cache.join("model.onnx"), b"corrupted").unwrap();
    let corrupt_res = rt.block_on(contextd::download::ensure_model_files_with_manifest(
        &model_path,
        &manifest,
    ));
    assert!(corrupt_res.is_err());
}

#[test]
fn test_model_cache_isolated_by_model_type() {
    let root = PathBuf::from("models");
    let minilm = contextd::download::model_cache_dir(&root, "all-minilm-l6-v2");
    let mpnet = contextd::download::model_cache_dir(&root, "all-mpnet-base-v2");

    assert_ne!(minilm, mpnet);
    assert!(minilm.ends_with("all-minilm-l6-v2"));
    assert!(mpnet.ends_with("all-mpnet-base-v2"));
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
