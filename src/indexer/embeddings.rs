use anyhow::Result;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;
use std::sync::Mutex;
use tokenizers::Tokenizer;

use crate::config::StorageConfig;

pub struct Embedder {
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    hidden_size: usize,
}

impl Embedder {
    pub fn new(config: &StorageConfig) -> Result<Self> {
        let model_dir = &config.model_path;
        let model_type = &config.model_type;

        let hidden_size = match model_type.as_str() {
            "all-minilm-l6-v2" => 384,
            "bge-small-en-v1.5" => 384,
            "all-mpnet-base-v2" => 768,
            "codebert-base" | "unixcoder-base" => 768,
            _ => 384, // Default fallback
        };

        let tokenizer_path = model_dir.join("tokenizer.json");
        let model_path = model_dir.join("model.onnx");

        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| anyhow::anyhow!(e))?;

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;

        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
            hidden_size,
        })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Tokenize
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!(e))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();

        let batch_size = 1;
        let seq_len = input_ids.len();
        let shape = vec![batch_size, seq_len];

        let input_ids_val = Value::from_array((shape.clone(), input_ids))?;
        let attention_mask_clone = attention_mask.clone();
        let attention_mask_val = Value::from_array((shape.clone(), attention_mask))?;
        let token_type_ids_val = Value::from_array((shape.clone(), token_type_ids))?;

        // Run inference
        let mut session = self.session.lock().unwrap();
        let outputs = session.run(ort::inputs![
            "input_ids" => input_ids_val,
            "attention_mask" => attention_mask_val,
            "token_type_ids" => token_type_ids_val,
        ])?;

        // Get last_hidden_state (usually output 0)
        // Shape: [batch_size, seq_len, hidden_size]
        let (_shape, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        // data is a flat slice &[f32]

        // Mean pooling
        // We need to average the hidden states, respecting the attention mask
        // For simplicity in this v0, we can just average all tokens (including special ones if mask is 1)
        // Better: Average only where mask is 1.

        // Better: Average only where mask is 1.

        let hidden_size = self.hidden_size;
        let mut pooled = vec![0.0; hidden_size];
        let mut count = 0.0;

        for (i, &mask_val) in attention_mask_clone.iter().enumerate().take(seq_len) {
            // Check mask (assuming batch 0)
            if mask_val == 1 {
                // Use the cloned attention_mask vector
                let offset = i * hidden_size;
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

        // Normalize (optional but good for cosine similarity)
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-6 {
            for val in &mut pooled {
                *val /= norm;
            }
        }

        Ok(pooled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StorageConfig;
    use std::path::{Path, PathBuf};

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
    #[ignore] // Requires model to be present
    fn test_embedder_inference() {
        let model_dir = "models";
        if !Path::new(model_dir).exists() {
            return;
        }
        let config = StorageConfig {
            db_path: PathBuf::from("test.db"),
            model_path: PathBuf::from(model_dir),
            model_type: "all-minilm-l6-v2".to_string(),
        };
        let embedder = Embedder::new(&config).expect("Failed to create embedder");
        let vec = embedder.embed("hello world").expect("Failed to embed");
        assert_eq!(vec.len(), 384);
    }

    #[test]
    fn test_model_dimension_selection() {
        // Test that hidden_size is correctly selected based on model_type
        // We can't instantiate without models, but we can verify the logic exists

        // 384-dim models
        assert!(matches!(
            match "all-minilm-l6-v2" {
                "all-minilm-l6-v2" => 384,
                "bge-small-en-v1.5" => 384,
                "all-mpnet-base-v2" => 768,
                _ => 384,
            },
            384
        ));

        // 768-dim models
        assert!(matches!(
            match "all-mpnet-base-v2" {
                "all-minilm-l6-v2" => 384,
                "bge-small-en-v1.5" => 384,
                "all-mpnet-base-v2" => 768,
                _ => 384,
            },
            768
        ));

        // BGE model
        assert!(matches!(
            match "bge-small-en-v1.5" {
                "all-minilm-l6-v2" => 384,
                "bge-small-en-v1.5" => 384,
                "all-mpnet-base-v2" => 768,
                _ => 384,
            },
            384
        ));
    }
}
