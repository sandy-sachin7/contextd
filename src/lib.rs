//! # contextd
//!
//! A local-first, privacy-preserving semantic context daemon and library for AI agents.
//!
//! `contextd` watches local workspaces, parses source code and documents with Tree-sitter
//! and PDF/plugin extractors, generates embeddings locally with ONNX Runtime, stores them
//! in SQLite WAL (`sqlite-vec` KNN + `FTS5`), and serves hybrid search over MCP stdio,
//! hardened HTTP REST, and CLI.
//!
//! ## Core Modules
//!
//! - [`config`] — TOML configuration, untrusted workspace isolation, and `CONTEXTD_*` overrides.
//! - [`indexer`] — Tree-sitter/text chunking, batched ONNX embeddings, `O_NOFOLLOW` safe reads,
//!   ignore/secret filters, sandboxed external plugins, and file watching.
//! - [`storage`] — Concurrent SQLite WAL storage (`sqlite-vec` KNN + `FTS5` + normalized RRF).
//! - [`daemon`] — Concurrent indexing orchestrator, per-path coalescing, and reconciliation.
//! - [`api`] — Hardened Axum HTTP server (`/health`, `/status`, `/query`).
//! - [`mcp`] — Async JSON-RPC 2.0 / MCP stdio server (`search_context`, `get_status`).
//! - [`connect`] — Automated MCP configuration for 9 AI coding tools.
//! - [`download`] — Pinned HuggingFace model downloads with SHA-256 verification.
//!
//! ## Embedded Quickstart
//!
//! You can use [`indexer::chunker`] and [`storage::db::Database`] directly without
//! starting the daemon or downloading external models:
//!
//! ```rust
//! use contextd::indexer::chunker;
//! use contextd::storage::db::{ChunkInput, Database, SearchOptions};
//!
//! # fn main() -> anyhow::Result<()> {
//! let db = Database::open(":memory:", 4, "demo-4d")?;
//! let source = "pub fn verify_token(tok: &str) -> bool { !tok.is_empty() }\n";
//! let chunks = chunker::chunk_by_type(source, "rs")?;
//!
//! let embedding = [0.5_f32, 0.5, 0.5, 0.5];
//! let inputs: Vec<ChunkInput<'_>> = chunks
//!     .iter()
//!     .map(|c| ChunkInput {
//!         start: c.start,
//!         end: c.end,
//!         content: &c.content,
//!         embedding: Some(&embedding),
//!         metadata: c.metadata.as_deref(),
//!     })
//!     .collect();
//!
//! db.index_file_chunks("src/auth.rs", 1_700_000_000, 0, source.len() as u64, "hash1", &inputs)?;
//!
//! let opts = SearchOptions {
//!     limit: Some(5),
//!     ..Default::default()
//! };
//! let hits = db.search_chunks_hybrid("verify_token", &embedding, &opts)?;
//! assert!(!hits.is_empty());
//! # Ok(())
//! # }
//! ```

pub mod api;
pub mod cli;
pub mod config;
pub mod connect;
pub mod daemon;
pub mod download;
pub mod indexer;
pub mod mcp;
pub mod storage;
