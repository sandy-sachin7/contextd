# Changelog

## [3.1.2] - 2026-07-02

### Fixed
- `end_time` parameter binding in `search_chunks_enhanced` (both bounds now use distinct param indices).
- System clock `unwrap()` replaced with `unwrap_or_default()` to prevent mutex poisoning.
- NaN scores in sorting mapped to 0.0 for deterministic ordering.

### Changed
- Mutex critical section in `search_chunks_enhanced` shortened: rows collected inside lock, scoring outside.
- Redundant outer `Mutex<Database>` removed from `AppState` — API handlers access `Database` directly.
- Added index on `chunks.file_id` for faster chunk lookups and joins.

### Added
- MCP server: tool annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`).
- MCP server: `tools.listChanged` capability declaration.
- MCP server: `additionalProperties: false` on tool input schemas.
- API: `max_results` field on `QueryRequest` for capping response size independently of search limit.

## [3.1.1] - 2026-06-29

### Fixed
- Security: suppressed lopdf stack overflow advisory (RUSTSEC-2026-0187) in cargo-audit CI.
- CI: pinned audit workflow dependency resolution to prevent cascading upgrade failures.

## [3.0.3] - 2025-05-XX

### Changed
- Reformatted `db.add_chunk` call for improved readability.

## [3.0.2] - 2025-05-XX

Same as 3.0.1 (version bump).

## [3.0.1] - 2025-05-XX

### Fixed
- Security: upgraded `lru` dependency and sanitized FTS queries to prevent injection.

## [3.0.0] - 2025-05-XX

### Added
- Multiple embedding model support: `all-minilm-l6-v2`, `all-mpnet-base-v2`, `bge-small-en-v1.5`.
- CLI `query` subcommand for one-off semantic queries.
- Download progress indicator during `contextd setup`.

### Changed
- Switched to clap derive for CLI argument parsing.
- Hybrid search (vector + FTS5) with Reciprocal Rank Fusion.
- Query caching via LRU cache (100 entries).

## [2.0.0] - 2025-05-XX

### Added
- Smart context windows in search results.
- Configurable context line count in query CLI.
- `bge-small-en-v1.5` and `all-mpnet-base-v2` embedding models.

## [1.1.0] - 2025-05-XX

### Added
- Frequency ranking to search results (tracks query hit counts).
- `query_hits` table in SQLite schema.

## [1.0.0] - 2025-05-XX

### Added
- Recency boost to search ranking (time-decay over 24h).
- Initial stable release.

## [0.2.0] - 2025-05-XX

### Fixed
- Cross-compilation for aarch64 Linux (switched to rustls-tls).
- Removed Windows build target (incompatible dependencies at the time).

## [0.1.1] - 2025-05-XX

### Added
- Comprehensive test suite (unit, integration, MCP E2E, memory stress).
- CI pipeline with cargo fmt, clippy, test, integration test compilation.
- Multi-arch release workflow (x86_64/aarch64 Linux/macOS).
- Docker support with multi-stage build and docker-compose.
- Homebrew formula.
- MCP server with `search_context` and `get_status` tools.
- Plugin system for external parser commands (PDF, docx, etc.).
- Tree-sitter semantic chunking for Rust, Python, JavaScript, TypeScript, Go.
- CLI subcommands: daemon, mcp, setup, query.

### Changed
- Switched to manual JSON-RPC MCP implementation (removed `rmcp` dependency).

## [0.1.0] - 2025-05-XX

### Added
- Initial release: file watcher, SQLite storage, ONNX embeddings, basic REST API.
- Rust and Markdown tree-sitter chunking.
- `.contextignore` file filtering.
- PDF text extraction.
