# contextd

**contextd** is a local-first, privacy-preserving semantic context daemon for AI coding agents.
It watches your local workspaces, chunks source code and documents with Tree-sitter and PDF/plugin extractors, generates ONNX embeddings locally, stores vectors and full-text indexes in SQLite WAL (`sqlite-vec` + `FTS5`), and exposes hybrid retrieval over **MCP stdio**, **HTTP REST**, and **CLI**.

## Why contextd?

- **100% Local** — Your code and embeddings never leave your machine
- **MCP Native (`2025-06-18`)** — One-command auto-setup (`contextd connect --all`) for 9 AI tools including Claude Desktop, Claude Code, Cursor, VS Code Copilot, Codex, OpenCode, Continue, Copilot CLI, and Antigravity
- **Hybrid Search** — Combines `sqlite-vec` KNN vector similarity with tokenized SQLite `FTS5` keyword search via normalized Reciprocal Rank Fusion (RRF)
- **Concurrent WAL Storage** — Dedicated 8-connection read-only `r2d2` reader pool, transactional writes, `blake3` incremental change detection, and batched ONNX inference
- **Polyglot Smart Chunking** — Tree-sitter AST splitting for Rust, Python, JS/JSX, TS/TSX, and Go, plus Markdown headers, built-in PDF extraction, and timeout- and output-capped external format parser subprocesses
- **Hardened by Default** — Loopback DNS-rebinding checks, mandatory Bearer auth on non-loopback binds, untrusted workspace config isolation, `O_NOFOLLOW` reads, secret/binary exclusions, and SHA-256 verified model downloads
