use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

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

/// Download a file atomically via a `.part` temp file, verifying HTTP status and optional SHA-256.
pub async fn download_file(url: &str, path: &Path, expected_sha256: Option<&str>) -> Result<()> {
    if path.exists() {
        println!("File {:?} already exists, skipping.", path);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

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

    let part_name = format!(
        "{}.part.{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download"),
        std::process::id()
    );
    let part_path = path.with_file_name(part_name);

    let download_res: Result<String> = async {
        let mut file = File::create(&part_path)
            .with_context(|| format!("Failed to create temp download file {:?}", part_path))?;
        let mut hasher = Sha256::new();
        let mut stream = res.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item.with_context(|| format!("Stream error downloading {}", url))?;
            file.write_all(&chunk)?;
            hasher.update(&chunk);
            pb.inc(chunk.len() as u64);
        }

        file.sync_all()?;
        Ok(hex_encode(&hasher.finalize()))
    }
    .await;

    let actual_sha256 = match download_res {
        Ok(digest) => digest,
        Err(err) => {
            let _ = fs::remove_file(&part_path);
            pb.abandon_with_message("Download failed");
            return Err(err);
        }
    };

    if let Some(expected) = expected_sha256 {
        if !actual_sha256.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&part_path);
            pb.abandon_with_message("Checksum mismatch");
            return Err(anyhow!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                url,
                expected,
                actual_sha256
            ));
        }
    }

    fs::rename(&part_path, path)
        .with_context(|| format!("Failed to atomically move {:?} to {:?}", part_path, path))?;

    pb.finish_with_message("Download complete");
    Ok(())
}

pub fn model_files_exist(model_dir: &Path) -> bool {
    let onnx_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");
    onnx_path.exists() && tokenizer_path.exists()
}

pub async fn ensure_model_files(model_dir: &Path, model_type: &str) -> Result<bool> {
    let manifest = lookup_manifest(model_type)?;

    if !model_dir.exists() {
        fs::create_dir_all(model_dir)?;
    }

    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        return Ok(false);
    }

    println!("Model files missing. Downloading {}...", model_type);

    if !model_path.exists() {
        download_file(manifest.model_url, &model_path, Some(manifest.model_sha256)).await?;
    }
    if !tokenizer_path.exists() {
        download_file(
            manifest.tokenizer_url,
            &tokenizer_path,
            Some(manifest.tokenizer_sha256),
        )
        .await?;
    }

    println!("Model download complete.");
    Ok(true)
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
    }
}
