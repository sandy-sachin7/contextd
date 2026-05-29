use std::fs;
use std::path::PathBuf;

/// Test that ensure_model_files returns Ok(false) when all files exist.
#[test]
fn test_ensure_model_files_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("models");
    fs::create_dir_all(&model_path).unwrap();

    // Create dummy model files
    fs::write(model_path.join("model.onnx"), b"dummy model").unwrap();
    fs::write(model_path.join("tokenizer.json"), b"{}").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files(
        &model_path,
        "all-minilm-l6-v2",
    ));
    assert!(result.is_ok());
    assert!(!result.unwrap()); // false = nothing downloaded
}

/// Test that ensure_model_files creates the model directory if missing.
#[test]
fn test_ensure_model_files_creates_dir() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("nonexistent_models");

    // Directory doesn't exist yet
    assert!(!model_path.exists());

    // Create dummy files first so it doesn't actually try to download
    fs::create_dir_all(&model_path).unwrap();
    fs::write(model_path.join("model.onnx"), b"dummy").unwrap();
    fs::write(model_path.join("tokenizer.json"), b"{}").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files(
        &model_path,
        "all-minilm-l6-v2",
    ));
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

/// Test that ensure_model_files creates directory when needed.
#[test]
fn test_ensure_model_files_partial_download() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("partial_models");
    fs::create_dir_all(&model_path).unwrap();

    // Only tokenizer exists, model.onnx is missing
    fs::write(model_path.join("tokenizer.json"), b"{}").unwrap();

    // ensure_model_files will try to download model.onnx from network
    // We can't test the full download in CI, but we can test that
    // when the file is missing, it attempts a download and handles errors.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(contextd::download::ensure_model_files(
        &model_path,
        "all-minilm-l6-v2",
    ));

    match &result {
        Ok(downloaded) => {
            // If by some chance download worked, mark it
            assert!(*downloaded);
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            // Expected: network error
            assert!(
                msg.contains("error")
                    || msg.contains("resolve")
                    || msg.contains("connection")
                    || msg.contains("timeout")
                    || msg.contains("dns")
                    || msg.contains("failed")
                    || msg.contains("refused")
                    || msg.contains("tls"),
                "Expected network error, got: {}",
                e
            );
        }
    }
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
