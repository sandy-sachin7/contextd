# Embedding Models Guide

contextd now supports multiple embedding models for different use cases. Choose the model that best fits your needs.

## Available Models

| Model | Dimensions | Size | Best For |
|-------|------------|------|----------|
| `all-minilm-l6-v2` (default) | 384 | ~80MB | General purpose, fast inference |
| `all-mpnet-base-v2` | 768 | ~420MB | Higher quality, recommended for code |
| `bge-small-en-v1.5` | 384 | ~130MB | Good quality/speed balance |

## Configuration

Set the model in your `contextd.toml`:

```toml
[storage]
db_path = "contextd.db"
model_path = "models"
model_type = "all-mpnet-base-v2"  # Change this
```

## Switching Models

1. Update `model_type` in your config
2. Run setup to download the new model:
   ```bash
   contextd setup
   ```
3. Re-index your files (optional, but recommended for best results)

## Model Details

### all-minilm-l6-v2 (Default)
- **Dimensions**: 384
- **Speed**: ⚡ Fast
- **Quality**: Good for general text
- **Best for**: Quick searches, general documentation

### all-mpnet-base-v2
- **Dimensions**: 768
- **Speed**: Medium
- **Quality**: High quality embeddings
- **Best for**: Code search, technical documentation

### bge-small-en-v1.5
- **Dimensions**: 384
- **Speed**: Fast
- **Quality**: Better than MiniLM for retrieval tasks
- **Best for**: Balanced quality/performance

## Custom Models

You can add custom ONNX models by:
1. Place `model.onnx` and `tokenizer.json` in the models directory
2. Add the hidden_size mapping in `src/indexer/embeddings.rs`
