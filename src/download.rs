use anyhow::Result;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub async fn download_file(url: &str, path: &PathBuf) -> Result<()> {
    if path.exists() {
        println!("File {:?} already exists, skipping.", path);
        return Ok(());
    }

    println!("Downloading {}...", url);

    let res = reqwest::get(url).await?;
    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
            )?
            .progress_chars("#>-"),
    );

    let mut file = fs::File::create(path)?;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk)?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_with_message("Download complete");
    Ok(())
}

pub async fn ensure_model_files(model_dir: &PathBuf, model_type: &str) -> Result<bool> {
    if !model_dir.exists() {
        fs::create_dir_all(model_dir)?;
    }

    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        return Ok(false);
    }

    let (model_url, tokenizer_url) = match model_type {
        "all-minilm-l6-v2" => (
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
        ),
        "all-mpnet-base-v2" => (
            "https://huggingface.co/optimum/all-mpnet-base-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-mpnet-base-v2/resolve/main/tokenizer.json",
        ),
        "bge-small-en-v1.5" => (
            "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx",
            "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json",
        ),
        _ => (
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/model.onnx",
            "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
        ),
    };

    println!("Model files missing. Downloading {}...", model_type);

    if !model_path.exists() {
        download_file(model_url, &model_path).await?;
    }
    if !tokenizer_path.exists() {
        download_file(tokenizer_url, &tokenizer_path).await?;
    }

    println!("Model download complete.");
    Ok(true)
}
