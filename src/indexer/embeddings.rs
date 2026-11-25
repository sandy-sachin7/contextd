use anyhow::{anyhow, Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::tensor::TensorElementType;
use ort::value::{Value, ValueType};
use std::sync::{Arc, Mutex, OnceLock};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

static ORT_INIT: OnceLock<Result<(), String>> = OnceLock::new();

fn ort_environment() -> Result<()> {
    ORT_INIT
        .get_or_init(|| {
            ort::init()
                .commit()
                .map(|_| ())
                .map_err(|e| format!("Failed to initialize ONNX Runtime environment: {e}"))
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

use crate::config::StorageConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolingStrategy {
    Mean,
    Cls,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    pub id: &'static str,
    pub expected_dim: usize,
    pub max_seq_len: usize,
    pub pooling: PoolingStrategy,
}

pub const SUPPORTED_MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "all-minilm-l6-v2",
        expected_dim: 384,
        max_seq_len: 512,
        pooling: PoolingStrategy::Mean,
    },
    ModelSpec {
        id: "all-minilm-l6-v2-q8",
        expected_dim: 384,
        max_seq_len: 512,
        pooling: PoolingStrategy::Mean,
    },
    ModelSpec {
        id: "all-mpnet-base-v2",
        expected_dim: 768,
        max_seq_len: 384,
        pooling: PoolingStrategy::Mean,
    },
    ModelSpec {
        id: "bge-small-en-v1.5",
        expected_dim: 384,
        max_seq_len: 512,
        pooling: PoolingStrategy::Cls,
    },
];

pub fn lookup_model_spec(model_type: &str) -> Result<ModelSpec> {
    SUPPORTED_MODELS
        .iter()
        .copied()
        .find(|m| m.id == model_type)
        .ok_or_else(|| {
            let names: Vec<&str> = SUPPORTED_MODELS.iter().map(|m| m.id).collect();
            anyhow!(
                "Unsupported model_type '{}'. Supported models: {}",
                model_type,
                names.join(", ")
            )
        })
}

/// Abstraction over text embedding so HTTP API and MCP servers can be unit-tested
/// deterministically without loading ONNX model weights.
pub trait TextEmbedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.embed(query)
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
    fn embedding_dim(&self) -> usize;
    fn model_type(&self) -> &str;
}

impl<T: TextEmbedder + ?Sized> TextEmbedder for Arc<T> {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        (**self).embed(text)
    }
    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        (**self).embed_query(query)
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        (**self).embed_batch(texts)
    }
    fn embedding_dim(&self) -> usize {
        (**self).embedding_dim()
    }
    fn model_type(&self) -> &str {
        (**self).model_type()
    }
}

pub struct Embedder {
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    hidden_size: usize,
    expects_token_type_ids: bool,
    output_name: String,
    pooling: PoolingStrategy,
    model_type: String,
}

/// Maximum chunks encoded per ONNX `session.run()` call.
/// Bounds ONNX Runtime arena memory and prevents large files from starving concurrent queries.
const MAX_EMBED_BATCH_SIZE: usize = 16;

impl Embedder {
    pub fn new(config: &StorageConfig) -> Result<Self> {
        let spec = lookup_model_spec(&config.model_type)?;
        let model_dir = crate::download::model_cache_dir(&config.model_path, &config.model_type);

        let tokenizer_path = model_dir.join("tokenizer.json");
        let model_path = model_dir.join("model.onnx");

        if !tokenizer_path.exists() {
            return Err(anyhow!("Tokenizer file not found at {:?}", tokenizer_path));
        }
        if !model_path.exists() {
            return Err(anyhow!("ONNX model file not found at {:?}", model_path));
        }

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow!("Failed to load tokenizer {:?}: {}", tokenizer_path, e))?;

        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: spec.max_seq_len,
                ..Default::default()
            }))
            .map_err(|e| anyhow!("Failed to configure tokenizer truncation: {}", e))?;

        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));

        let intra_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(2);

        ort_environment()?;
        let session = Session::builder()
            .map_err(|e| anyhow!("ONNX SessionBuilder creation failed: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("ONNX optimization level failed: {}", e))?
            .with_intra_threads(intra_threads)
            .map_err(|e| anyhow!("ONNX intra-threads config failed: {}", e))?
            .commit_from_file(&model_path)
            .map_err(|e| anyhow!("Failed to load ONNX model {:?}: {}", model_path, e))?;

        // Dynamically inspect ONNX inputs & outputs
        let expects_token_type_ids = session
            .inputs
            .iter()
            .any(|input| input.name == "token_type_ids");

        let (output_name, inferred_dim) = inspect_output_spec(&session, spec.expected_dim);

        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
            hidden_size: inferred_dim,
            expects_token_type_ids,
            output_name,
            pooling: spec.pooling,
            model_type: spec.id.to_string(),
        })
    }

    pub fn embedding_dim(&self) -> usize {
        self.hidden_size
    }

    pub fn model_type(&self) -> &str {
        &self.model_type
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text])?;
        results
            .pop()
            .ok_or_else(|| anyhow!("Empty embedding batch result"))
    }

    /// Embed a search query, applying the recommended retrieval instruction prefix for BGE models.
    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        if self.model_type.starts_with("bge-") {
            let prefixed = format!(
                "Represent this sentence for searching relevant passages: {}",
                query
            );
            self.embed(&prefixed)
        } else {
            self.embed(query)
        }
    }

    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if texts.len() <= MAX_EMBED_BATCH_SIZE {
            return self.embed_micro_batch(texts);
        }

        // Sort indices by byte length so micro-batches group similar-length chunks and avoid
        // quadratic self-attention padding waste when short and long chunks share a batch.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].len());

        let mut results: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
        let mut batch_texts: Vec<&str> = Vec::with_capacity(MAX_EMBED_BATCH_SIZE);

        for idx_chunk in order.chunks(MAX_EMBED_BATCH_SIZE) {
            batch_texts.clear();
            for &idx in idx_chunk {
                batch_texts.push(texts[idx]);
            }
            let batch_embs = self.embed_micro_batch(&batch_texts)?;
            for (&orig_idx, emb) in idx_chunk.iter().zip(batch_embs) {
                results[orig_idx] = emb;
            }
        }
        Ok(results)
    }

    fn embed_micro_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow!("Tokenization failed: {}", e))?;

        let batch_size = encodings.len();
        let seq_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);

        if seq_len == 0 {
            return Ok(vec![vec![0.0; self.hidden_size]; batch_size]);
        }

        let mut input_ids = Vec::with_capacity(batch_size * seq_len);
        let mut attention_mask = Vec::with_capacity(batch_size * seq_len);
        let mut token_type_ids = Vec::with_capacity(batch_size * seq_len);

        for enc in &encodings {
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            let types = enc.get_type_ids();
            for i in 0..seq_len {
                input_ids.push(*ids.get(i).unwrap_or(&0) as i64);
                attention_mask.push(*mask.get(i).unwrap_or(&0) as i64);
                token_type_ids.push(*types.get(i).unwrap_or(&0) as i64);
            }
        }

        let shape = vec![batch_size, seq_len];
        let input_ids_val = Value::from_array((shape.clone(), input_ids))?;
        let attention_mask_val = Value::from_array((shape.clone(), attention_mask.clone()))?;

        // Hold session lock ONLY for inference and tensor extraction, then drop immediately
        let (out_shape, raw_data): (Vec<usize>, Vec<f32>) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow!("ONNX session mutex poisoned"))?;

            let outputs = if self.expects_token_type_ids {
                let token_type_ids_val = Value::from_array((shape, token_type_ids))?;
                session.run(ort::inputs![
                    "input_ids" => input_ids_val,
                    "attention_mask" => attention_mask_val,
                    "token_type_ids" => token_type_ids_val,
                ])?
            } else {
                session.run(ort::inputs![
                    "input_ids" => input_ids_val,
                    "attention_mask" => attention_mask_val,
                ])?
            };

            let (tensor_shape, slice) = outputs[self.output_name.as_str()]
                .try_extract_tensor::<f32>()
                .context("Failed to extract f32 output tensor from ONNX session")?;

            let dims: Vec<usize> = tensor_shape.iter().map(|&d| d as usize).collect();
            (dims, slice.to_vec())
        };

        // Pool & L2-normalize outside the session lock
        pool_and_normalize(
            &raw_data,
            &out_shape,
            &attention_mask,
            batch_size,
            seq_len,
            self.hidden_size,
            self.pooling,
        )
    }
}

impl TextEmbedder for Embedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Embedder::embed(self, text)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        Embedder::embed_query(self, query)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Embedder::embed_batch(self, texts)
    }

    fn embedding_dim(&self) -> usize {
        self.hidden_size
    }

    fn model_type(&self) -> &str {
        &self.model_type
    }
}

fn inspect_output_spec(session: &Session, fallback_dim: usize) -> (String, usize) {
    let output = session
        .outputs
        .iter()
        .find(|o| o.name == "last_hidden_state")
        .or_else(|| session.outputs.first());

    match output {
        Some(outlet) => {
            let dim = match &outlet.output_type {
                ValueType::Tensor {
                    ty: TensorElementType::Float32,
                    shape,
                    ..
                } => shape
                    .last()
                    .copied()
                    .filter(|&d| d > 0)
                    .map(|d| d as usize)
                    .unwrap_or(fallback_dim),
                _ => fallback_dim,
            };
            (outlet.name.clone(), dim)
        }
        None => ("last_hidden_state".to_string(), fallback_dim),
    }
}

fn pool_and_normalize(
    data: &[f32],
    out_shape: &[usize],
    attention_mask: &[i64],
    batch_size: usize,
    seq_len: usize,
    expected_hidden_size: usize,
    pooling: PoolingStrategy,
) -> Result<Vec<Vec<f32>>> {
    let hidden_size = out_shape.last().copied().unwrap_or(expected_hidden_size);
    if hidden_size == 0 {
        return Err(anyhow!("ONNX model returned zero hidden_size"));
    }

    let mut results = Vec::with_capacity(batch_size);

    // Handle pre-pooled 2D outputs `[batch_size, hidden_size]` as well as 3D `[batch_size, seq_len, hidden_size]`
    if out_shape.len() == 2 {
        for b in 0..batch_size {
            let start = b * hidden_size;
            let end = start + hidden_size;
            if end > data.len() {
                return Err(anyhow!("ONNX 2D output tensor shorter than expected"));
            }
            let mut vec = data[start..end].to_vec();
            l2_normalize(&mut vec);
            results.push(vec);
        }
        return Ok(results);
    }

    let expected_len = batch_size * seq_len * hidden_size;
    if data.len() < expected_len {
        return Err(anyhow!(
            "ONNX 3D output tensor too short: got {}, expected at least {}",
            data.len(),
            expected_len
        ));
    }

    for b in 0..batch_size {
        let mut pooled = vec![0.0_f32; hidden_size];
        let batch_base = b * seq_len * hidden_size;

        match pooling {
            PoolingStrategy::Cls => {
                // Use [CLS] token embedding (token index 0)
                pooled.copy_from_slice(&data[batch_base..batch_base + hidden_size]);
            }
            PoolingStrategy::Mean => {
                let mask_base = b * seq_len;
                let mut count = 0.0_f32;
                for i in 0..seq_len {
                    if attention_mask[mask_base + i] == 1 {
                        let offset = batch_base + i * hidden_size;
                        for j in 0..hidden_size {
                            pooled[j] += data[offset + j];
                        }
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    for val in &mut pooled {
                        *val /= count;
                    }
                }
            }
        }

        l2_normalize(&mut pooled);
        results.push(pooled);
    }

    Ok(results)
}

fn l2_normalize(vec: &mut [f32]) {
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for val in vec.iter_mut() {
            *val /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_embedder_creation_fails_without_model() {
        let config = StorageConfig {
            db_path: PathBuf::from("test.db"),
            model_path: PathBuf::from("non_existent_path"),
            model_type: "all-minilm-l6-v2".to_string(),
        };
        let result = Embedder::new(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_model_type_rejected() {
        assert!(lookup_model_spec("all-minilm-l6-v2").is_ok());
        assert_eq!(
            lookup_model_spec("all-mpnet-base-v2").unwrap().expected_dim,
            768
        );
        assert_eq!(
            lookup_model_spec("bge-small-en-v1.5").unwrap().pooling,
            PoolingStrategy::Cls
        );
        let err = lookup_model_spec("codebert-base").unwrap_err();
        assert!(err.to_string().contains("Unsupported model_type"));
    }

    #[test]
    fn test_pooling_mean_vs_cls() {
        // Batch 1, seq_len 2, hidden_size 2
        // Token 0 ([CLS]) = [3.0, 0.0], Token 1 = [0.0, 4.0]
        let data = vec![3.0, 0.0, 0.0, 4.0];
        let mask = vec![1, 1];

        let cls =
            pool_and_normalize(&data, &[1, 2, 2], &mask, 1, 2, 2, PoolingStrategy::Cls).unwrap();
        assert!((cls[0][0] - 1.0).abs() < 1e-5);
        assert!(cls[0][1].abs() < 1e-5);

        let mean =
            pool_and_normalize(&data, &[1, 2, 2], &mask, 1, 2, 2, PoolingStrategy::Mean).unwrap();
        // Mean = [1.5, 2.0], norm = 2.5 -> [0.6, 0.8]
        assert!((mean[0][0] - 0.6).abs() < 1e-5);
        assert!((mean[0][1] - 0.8).abs() < 1e-5);
    }

    #[test]
    #[ignore] // Requires model files on disk
    fn test_embedder_inference() {
        let model_root = PathBuf::from("models");
        let model_type = "all-minilm-l6-v2";
        if !crate::download::model_files_exist(&model_root, model_type) {
            return;
        }
        let config = StorageConfig {
            db_path: PathBuf::from("test.db"),
            model_path: model_root,
            model_type: model_type.to_string(),
        };
        let embedder = Embedder::new(&config).expect("Failed to create embedder");
        let vec = embedder.embed("hello world").expect("Failed to embed");
        assert_eq!(vec.len(), 384);
    }
}
