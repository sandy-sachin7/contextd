use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static MIGRATION_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct ModelDownloadManifest {
    pub model_type: &'static str,
    pub model_url: &'static str,
    pub model_sha256: &'static str,
    pub tokenizer_url: &'static str,
    pub tokenizer_sha256: &'static str,
}

/// Pinned HuggingFace commit revisions and SHA-256 digests for all supported embedding models.
pub const MODEL_MANIFESTS: &[ModelDownloadManifest] = &[
    ModelDownloadManifest {
        model_type: "all-minilm-l6-v2",
        model_url: "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/10244843eba3d9e479b27a4b81c94b56d8e9f4f2/model.onnx",
        model_sha256: "4a64cee3d4134bbdc86eed96e1a660efec58975417204ecfcf134140edb6e0e2",
        tokenizer_url: "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/10244843eba3d9e479b27a4b81c94b56d8e9f4f2/tokenizer.json",
        tokenizer_sha256: "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0",
    },
    ModelDownloadManifest {
        model_type: "all-mpnet-base-v2",
        model_url: "https://huggingface.co/sentence-transformers/all-mpnet-base-v2/resolve/e8c3b32edf5434bc2275fc9bab85f82640a19130/onnx/model.onnx",
        model_sha256: "74187b16d9c946fea252e120cfd7a12c5779d8b8b86838a2e4c56573c47941bd",
        tokenizer_url: "https://huggingface.co/sentence-transformers/all-mpnet-base-v2/resolve/e8c3b32edf5434bc2275fc9bab85f82640a19130/tokenizer.json",
        tokenizer_sha256: "b8be2c30ba5dd723a6d5ee26d013da103d5408d92ddcb23747622f9e48f1d842",
    },
    ModelDownloadManifest {
        model_type: "bge-small-en-v1.5",
        model_url: "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/onnx/model.onnx",
        model_sha256: "828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35",
        tokenizer_url: "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/tokenizer.json",
        tokenizer_sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
    },
];

pub fn lookup_manifest(model_type: &str) -> Result<ModelDownloadManifest> {
    MODEL_MANIFESTS
        .iter()
        .copied()
        .find(|m| m.model_type == model_type)
        .ok_or_else(|| {
            let supported: Vec<&str> = MODEL_MANIFESTS.iter().map(|m| m.model_type).collect();
            anyhow!(
                "Unsupported model_type '{}'. Supported models: {}",
                model_type,
                supported.join(", ")
            )
        })
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("contextd/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to build HTTP client for model downloads")
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Download a file atomically via a collision-free temp file, verifying HTTP status and optional SHA-256.
pub async fn download_file(url: &str, path: &Path, expected_sha256: Option<&str>) -> Result<()> {
    if path.exists() {
        if let Some(expected) = expected_sha256 {
            let check_path = path.to_path_buf();
            let expected_owned = expected.to_string();
            let valid =
                tokio::task::spawn_blocking(move || sha256_matches(&check_path, &expected_owned))
                    .await??;
            if valid {
                println!(
                    "File {:?} already exists and passed SHA-256 verification, skipping.",
                    path
                );
                return Ok(());
            }
            eprintln!(
                "Existing file {:?} failed SHA-256 verification; re-downloading...",
                path
            );
        } else {
            println!("File {:?} already exists, skipping.", path);
            return Ok(());
        }
    }

    let parent_dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent)?;
            parent.to_path_buf()
        }
        _ => PathBuf::from("."),
    };

    println!("Downloading {}...", url);

    let client = build_http_client()?;
    let res = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to request {}", url))?
        .error_for_status()
        .with_context(|| format!("HTTP error downloading {}", url))?;

    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
            )?
            .progress_chars("#>-"),
    );

    let prefix = format!(
        ".{}.part.",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download")
    );
    let mut temp_file = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(&parent_dir)
        .with_context(|| format!("Failed to create temp download file in {:?}", parent_dir))?;

    let download_res: Result<String> = async {
        let mut hasher = Sha256::new();
        let mut stream = res.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item.with_context(|| format!("Stream error downloading {}", url))?;
            temp_file.write_all(&chunk)?;
            hasher.update(&chunk);
            pb.inc(chunk.len() as u64);
        }

        temp_file.as_file().sync_all()?;
        Ok(hex_encode(&hasher.finalize()))
    }
    .await;

    let actual_sha256 = match download_res {
        Ok(digest) => digest,
        Err(err) => {
            drop(temp_file);
            pb.abandon_with_message("Download failed");
            return Err(err);
        }
    };

    if let Some(expected) = expected_sha256 {
        if !actual_sha256.eq_ignore_ascii_case(expected) {
            drop(temp_file);
            pb.abandon_with_message("Checksum mismatch");
            return Err(anyhow!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                url,
                expected,
                actual_sha256
            ));
        }
    }

    temp_file
        .persist(path)
        .map_err(|err| err.error)
        .with_context(|| format!("Failed to atomically move temp file to {:?}", path))?;

    pb.finish_with_message("Download complete");
    Ok(())
}

pub fn model_cache_dir(model_root: &Path, model_type: &str) -> std::path::PathBuf {
    model_root.join(model_type)
}

pub fn model_files_exist(model_root: &Path, model_type: &str) -> bool {
    let model_dir = model_cache_dir(model_root, model_type);
    let onnx_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");
    onnx_path.exists() && tokenizer_path.exists()
}

fn migrate_legacy_cache(
    model_root: &Path,
    model_dir: &Path,
    manifest: &ModelDownloadManifest,
) -> Result<()> {
    migrate_legacy_file(model_root, model_dir, "model.onnx", manifest.model_sha256)?;
    migrate_legacy_file(
        model_root,
        model_dir,
        "tokenizer.json",
        manifest.tokenizer_sha256,
    )?;
    Ok(())
}

fn migrate_legacy_file(
    model_root: &Path,
    model_dir: &Path,
    file_name: &str,
    expected_sha256: &str,
) -> Result<bool> {
    let legacy_path = model_root.join(file_name);
    let cache_path = model_dir.join(file_name);
    if cache_path.exists() || !legacy_path.is_file() {
        return Ok(false);
    }

    let mut source = File::open(&legacy_path)
        .with_context(|| format!("Failed to open legacy model file {:?}", legacy_path))?;
    let (temp_path, mut target) = create_migration_file(model_dir, file_name)?;

    let result = (|| -> Result<bool> {
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            target.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
        }

        let actual_sha256 = hex_encode(&hasher.finalize());
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Ok(false);
        }

        target.sync_all()?;
        install_migrated_file(&temp_path, &cache_path, expected_sha256)
    })();

    if !matches!(&result, Ok(true)) {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn install_migrated_file(
    temp_path: &Path,
    cache_path: &Path,
    expected_sha256: &str,
) -> Result<bool> {
    match fs::rename(temp_path, cache_path) {
        Ok(()) => Ok(true),
        Err(_rename_error) if sha256_matches(cache_path, expected_sha256)? => {
            fs::remove_file(temp_path)?;
            Ok(true)
        }
        Err(rename_error) => Err(rename_error)
            .with_context(|| format!("Failed to install migrated model file {:?}", cache_path)),
    }
}

fn sha256_matches(path: &Path, expected_sha256: &str) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_encode(&hasher.finalize()).eq_ignore_ascii_case(expected_sha256))
}

fn create_migration_file(model_dir: &Path, file_name: &str) -> Result<(PathBuf, File)> {
    for _ in 0..10 {
        let sequence = MIGRATION_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!(".{}.migrate.{}.{}", file_name, std::process::id(), sequence);
        let temp_path = model_dir.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }

    Err(anyhow!(
        "Could not create a temporary migration file in {:?}",
        model_dir
    ))
}

/// Migrate a legacy flat model cache into the per-model layout without
/// downloading. Callers with a stdout protocol (MCP) use this to avoid emitting
/// download progress into the data stream.
pub fn migrate_legacy_model_cache(model_root: &Path, model_type: &str) -> Result<()> {
    let manifest = lookup_manifest(model_type)?;
    let model_dir = model_cache_dir(model_root, model_type);
    if !model_dir.exists() {
        fs::create_dir_all(&model_dir)?;
    }
    migrate_legacy_cache(model_root, &model_dir, &manifest)
}

pub async fn ensure_model_files(model_root: &Path, model_type: &str) -> Result<bool> {
    let manifest = lookup_manifest(model_type)?;
    ensure_model_files_with_manifest(model_root, &manifest).await
}

pub async fn ensure_model_files_with_manifest(
    model_root: &Path,
    manifest: &ModelDownloadManifest,
) -> Result<bool> {
    let model_dir = model_cache_dir(model_root, manifest.model_type);

    if !model_dir.exists() {
        fs::create_dir_all(&model_dir)?;
    }

    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    migrate_legacy_cache(model_root, &model_dir, manifest)?;

    let check_model = model_path.clone();
    let check_tokenizer = tokenizer_path.clone();
    let model_sha = manifest.model_sha256.to_string();
    let tokenizer_sha = manifest.tokenizer_sha256.to_string();
    let (model_valid, tokenizer_valid) =
        tokio::task::spawn_blocking(move || -> Result<(bool, bool)> {
            let m_ok = sha256_matches(&check_model, &model_sha)?;
            let t_ok = sha256_matches(&check_tokenizer, &tokenizer_sha)?;
            Ok((m_ok, t_ok))
        })
        .await??;

    if model_valid && tokenizer_valid {
        return Ok(false);
    }

    println!(
        "Model files missing or corrupted. Downloading {}...",
        manifest.model_type
    );

    let mut downloaded = false;
    if !model_valid {
        download_file(manifest.model_url, &model_path, Some(manifest.model_sha256)).await?;
        downloaded = true;
    }
    if !tokenizer_valid {
        download_file(
            manifest.tokenizer_url,
            &tokenizer_path,
            Some(manifest.tokenizer_sha256),
        )
        .await?;
        downloaded = true;
    }

    println!("Model download complete.");
    Ok(downloaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_lookup() {
        assert!(lookup_manifest("all-minilm-l6-v2").is_ok());
        assert!(lookup_manifest("all-mpnet-base-v2").is_ok());
        assert!(lookup_manifest("bge-small-en-v1.5").is_ok());
        assert!(lookup_manifest("nonexistent-model").is_err());

        for m in MODEL_MANIFESTS {
            assert_eq!(m.model_sha256.len(), 64);
            assert_eq!(m.tokenizer_sha256.len(), 64);
            assert!(!m.model_url.contains("/resolve/main/"));
            assert!(!m.tokenizer_url.contains("/resolve/main/"));
        }
    }

    #[test]
    fn test_migrates_legacy_files_when_each_hash_matches() {
        let root = tempfile::tempdir().unwrap();
        let model_type = "all-minilm-l6-v2";
        let cache = model_cache_dir(root.path(), model_type);
        fs::create_dir_all(&cache).unwrap();

        let model = b"legacy model bytes";
        let tokenizer = b"legacy tokenizer bytes";
        fs::write(root.path().join("model.onnx"), model).unwrap();
        fs::write(root.path().join("tokenizer.json"), tokenizer).unwrap();
        let manifest = ModelDownloadManifest {
            model_type,
            model_url: "",
            model_sha256: "a14ed6bb1203af75a250622a3b57e6b417ec7adcb33421dde26c35b693161ab4",
            tokenizer_url: "",
            tokenizer_sha256: "742d6ac24d23e59976d6ad1924809c885bfee8bc4231fde40375c76a412bd4bb",
        };

        migrate_legacy_cache(root.path(), &cache, &manifest).unwrap();

        assert_eq!(fs::read(cache.join("model.onnx")).unwrap(), model);
        assert_eq!(fs::read(cache.join("tokenizer.json")).unwrap(), tokenizer);
        assert_eq!(fs::read(root.path().join("model.onnx")).unwrap(), model);
        assert_eq!(
            fs::read(root.path().join("tokenizer.json")).unwrap(),
            tokenizer
        );
    }

    #[test]
    fn test_does_not_migrate_legacy_file_with_mismatched_hash() {
        let root = tempfile::tempdir().unwrap();
        let model_type = "all-minilm-l6-v2";
        let cache = model_cache_dir(root.path(), model_type);
        fs::create_dir_all(&cache).unwrap();

        let legacy_model = b"different model bytes";
        let legacy_tokenizer = b"matching tokenizer bytes";
        fs::write(root.path().join("model.onnx"), legacy_model).unwrap();
        fs::write(root.path().join("tokenizer.json"), legacy_tokenizer).unwrap();

        let manifest = ModelDownloadManifest {
            model_type,
            model_url: "",
            model_sha256: "cf6753cf2e59e2dae95efb82fec4b74c141d851ed55ae7803910c39823e189b3",
            tokenizer_url: "",
            tokenizer_sha256: "7e0b1d3544e4f54f1fea18398015eb6f79e1bf900b5b4ceb51c5112686fc6d80",
        };

        migrate_legacy_cache(root.path(), &cache, &manifest).unwrap();

        assert!(!cache.join("model.onnx").exists());
        assert_eq!(
            fs::read(root.path().join("model.onnx")).unwrap(),
            legacy_model
        );
        assert_eq!(
            fs::read(cache.join("tokenizer.json")).unwrap(),
            legacy_tokenizer
        );
        assert!(fs::read_dir(&cache).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".migrate.")
        }));
    }

    #[test]
    fn test_accepts_matching_model_cache_winner() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("model.onnx");
        let temp_path = dir.path().join(".model.onnx.migrate.test");
        let bytes = b"already installed model bytes";
        let expected_sha256 = hex_encode(&Sha256::digest(bytes));
        fs::write(&cache_path, bytes).unwrap();
        fs::write(&temp_path, bytes).unwrap();

        assert!(install_migrated_file(&temp_path, &cache_path, &expected_sha256).unwrap());
        assert_eq!(fs::read(&cache_path).unwrap(), bytes);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_download_rejects_http_error_and_leaves_no_file() {
        use axum::{http::StatusCode, routing::get, Router};
        let app = Router::new().route("/404", get(|| async { StatusCode::NOT_FOUND }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.onnx");
        let res = download_file(&format!("http://{}/404", addr), &dest, None).await;
        assert!(res.is_err());
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn test_download_verifies_sha256_and_atomic_rename() {
        use axum::{routing::get, Router};
        let payload = b"hello verified model bytes";
        let expected_hex = hex_encode(&Sha256::digest(payload));

        let app = Router::new().route("/ok", get(move || async move { payload.as_slice() }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = tempfile::tempdir().unwrap();

        // Wrong checksum must fail and NOT create destination file
        let bad_dest = dir.path().join("bad.bin");
        let bad_res = download_file(
            &format!("http://{}/ok", addr),
            &bad_dest,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        )
        .await;
        assert!(bad_res.is_err());
        assert!(!bad_dest.exists());

        // Matching checksum succeeds atomically
        let good_dest = dir.path().join("good.bin");
        download_file(
            &format!("http://{}/ok", addr),
            &good_dest,
            Some(&expected_hex),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&good_dest).unwrap(), payload);

        // Corrupted pre-existing file on disk must be detected and re-downloaded
        fs::write(&good_dest, b"corrupted partial bytes").unwrap();
        download_file(
            &format!("http://{}/ok", addr),
            &good_dest,
            Some(&expected_hex),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&good_dest).unwrap(), payload);
    }
}
