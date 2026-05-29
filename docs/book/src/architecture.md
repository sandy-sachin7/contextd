# Architecture

```
┌─────────────┐
│  File Watch │ → Debouncer → .contextignore Filter
└─────────────┘
       │
       ▼
┌─────────────┐
│   Parser    │ → Plugin System / Native Parsers
└─────────────┘
       │
       ▼
┌─────────────┐
│   Chunker   │ → Tree-sitter (Rust/Py/JS/TS/Go) / Header-based (MD) / Pages (PDF)
└─────────────┘
       │
       ▼
┌─────────────┐
│  Embedder   │ → ONNX Runtime (Local, no cloud!)
└─────────────┘
       │
       ▼
┌─────────────┐
│   Storage   │ → SQLite + FTS5 (Hybrid search)
└─────────────┘
       │
       ▼
┌─────────────┐
│ Query Layer │ → REST API / CLI / MCP Server
└─────────────┘
```

## Components

### Daemon
The orchestration layer that manages the entire indexing lifecycle:
- Initial scan with concurrent indexing (4 workers)
- File watcher with 2-second debounce
- Incremental updates (only re-indexes changed files)

### Embedder
ONNX Runtime inference pipeline:
- Tokenizes input text via HuggingFace tokenizers
- Runs ONNX model with mean pooling of `last_hidden_state`
- L2 normalization for cosine similarity
- Supports 3 model architectures (384d and 768d)

### Storage
SQLite with:
- WAL journal mode
- FTS5 virtual table for keyword search
- LRU query cache (100 entries)
- Frequency tracking table for ranking

### Query Layer
Three interfaces:
- REST API (axum, HTTP)
- CLI (terminal, one-off queries)
- MCP (stdio, JSON-RPC 2.0)
