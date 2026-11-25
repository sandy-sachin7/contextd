# Embedding Models Guide

contextd supports multiple local ONNX embedding models for different speed/quality trade-offs.

## Available Models

| Model | Dimensions | Pooling | Best For |
|-------|------------|---------|----------|
| `all-minilm-l6-v2` (default) | 384 | Mean | General purpose FP32 (~91 MB) |
| `all-minilm-l6-v2-q8` | 384 | Mean | Quantized int8 (~22 MB), faster CPU indexing and lower cold query latency |
| `all-mpnet-base-v2` | 768 | Mean | Higher quality, recommended for code |
| `bge-small-en-v1.5` | 384 | CLS | Balanced quality/speed for retrieval |

All supported models are downloaded from pinned HuggingFace commit revisions and verified with SHA-256 checksums before installation.

Each model uses its own subdirectory under `model_path`. A model switch loads the selected model's files and keeps the other model caches.

## Configuration

Set the model in your `contextd.toml` (or via `CONTEXTD_MODEL_TYPE`):

```toml
[storage]
model_type = "all-mpnet-base-v2"
```

## Switching Models

1. Update `model_type` in your config.
2. Run setup to download and verify the model files:
   ```bash
   contextd setup
   ```
3. Remove or point `db_path` at a new SQLite database file and restart `contextd daemon`. contextd records `model_type` and `embedding_dim` in the database `meta` table and will fail fast with a clear error if an existing database does not match the active model.
