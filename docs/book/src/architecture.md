# Architecture

```
┌──────────────────┐
│  Watcher + Walk  │ → 500ms Debounce → Default Secret/Build Exclusions + .gitignore/.contextignore
└──────────────────┘
         │
         ▼
┌──────────────────┐
│  safe_read +     │ → O_NOFOLLOW / Reparse-Point Guard → Binary Sniff (8 KiB) → blake3 Hash
│  PathCoordinator │
└──────────────────┘
         │
         ▼
┌──────────────────┐
│  Parser/Chunker  │ → Tree-sitter (Rust/Py/JS/TS/Go) / Markdown / PDF / Sandboxed Plugins
└──────────────────┘
         │
         ▼
┌──────────────────┐
│ Batched Embedder │ → ONNX Runtime (SHA-256 Pinned Models, Mean/CLS Pooling, L2 Norm)
└──────────────────┘
         │
         ▼
┌──────────────────┐
│  SQLite WAL v2   │ → Transactional Writer + 8-Conn Reader Pool + sqlite-vec KNN + FTS5
└──────────────────┘
         │
         ▼
┌──────────────────┐
│   Query Layer    │ → MCP stdio (2025-06-18) / Hardened Axum HTTP / CLI
└──────────────────┘
```

## Components

### Daemon (`src/daemon.rs`)
The orchestration layer that manages the indexing lifecycle:
- Initial scan streamed from `WalkBuilder` with bounded worker concurrency (`available_parallelism().clamp(2, 8)`)
- Startup reconciliation that prunes deleted files under active watch roots
- File watcher via `notify-debouncer-mini` with `500ms` default debounce (`watch.debounce_ms`)
- Per-path coalescing (`PathCoordinator`) and `blake3` + `(mtime_ns, file_size)` fast-skipping for incremental updates
- Symlink-safe regular file reads via `safe_read` (`O_NOFOLLOW` on Unix, reparse-point checks on Windows)

### Embedder (`src/indexer/embeddings.rs`, `src/download.rs`)
Local ONNX Runtime inference pipeline:
- Tokenizes input text via HuggingFace `tokenizers`
- Batched ONNX Runtime inference (`embed_batch`) with Mean pooling (`all-minilm-l6-v2`, `all-mpnet-base-v2`) or CLS pooling (`bge-small-en-v1.5`)
- L2 normalization for cosine similarity across 384d and 768d models
- Pinned HuggingFace commit downloads verified by SHA-256 into per-model cache subdirectories

### Storage (`src/storage/db.rs`)
SQLite schema v2 in WAL mode:
- Single transactional writer (`Mutex<Connection>`) + 8-connection read-only `r2d2` reader pool (`PRAGMA query_only = ON`)
- `chunks_vec` (`sqlite-vec` KNN virtual table) + `chunks_fts` (Porter-stemmed FTS5 virtual table)
- Normalized Reciprocal Rank Fusion (RRF) with progressive filter over-fetching
- `meta` table validating `schema_version`, `model_type`, and `embedding_dim` on open
- `query_hits` table for logarithmic frequency ranking

### Query Layer (`src/api/mod.rs`, `src/mcp/server.rs`, `src/cli.rs`)
Three async interfaces:
- **HTTP REST API**: Hardened Axum server (loopback `Host` validation, mandatory Bearer auth on non-loopback binds, 64 KiB body cap, 30s timeout, 64 concurrent requests)
- **MCP Server**: Async stdio JSON-RPC 2.0 server (`2025-06-18`, 16 MiB frame cap, `notifications/cancelled`)
- **CLI**: One-off hybrid search (`contextd query`) and AI tool auto-configuration (`contextd connect`)

See [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) for the full design and trust-boundary documentation.
