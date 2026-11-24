# Changelog

All notable changes to `contextd` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [4.0.1] - 2026-11-25

### Security & Compatibility
- Upgraded `rusqlite` to 0.42 and `r2d2_sqlite` to 0.37, moving `libsqlite3-sys` to 0.40 and clearing `RUSTSEC-2026-0459`.
- Pinned `ort` to 2.0.0-rc.10 so the x86_64-apple-darwin release again finds prebuilt ONNX Runtime binaries; adapted embedding code to that API. Disabled `copy-dylibs` because ort-sys rc.10 fails to copy DirectML.dll on Windows; the static ONNX Runtime link is unaffected.
- Raised MSRV to Rust 1.95 and switched the Intel macOS release job to a native `macos-15-intel` runner.

## [4.0.0] - 2026-11-24

### Breaking Security & Compatibility Changes
- **Mandatory Authentication on Non-Loopback Binds**: `contextd daemon` now refuses to bind any non-loopback address (e.g., `0.0.0.0`) unless `server.auth_token` or `CONTEXTD_AUTH_TOKEN` is configured. Unauthenticated loopback mode enforces strict `Host` header validation to block DNS-rebinding attacks.
- **Untrusted Workspace Configuration Isolation**: Workspace-local `./contextd.toml` files (and explicit `--config` files outside the user config directory) are now treated as untrusted. Only `[watch]` and `[chunking]` apply from them; `[server]` and `[storage]` inherit from the user-global config or platform defaults, and `[plugins]` execution is blocked by default.
- **Explicit Trust Flags**: Added global `--trust-config` (trusts an explicit `--config` path outside the user config directory) and `--trust-workspace-plugins` (explicitly allows `[plugins]` from a workspace config).
- **Schema v2 & Per-Model Cache Directories**: Upgraded SQLite storage to schema v2 (`meta` table recording `schema_version`, `model_type`, and `embedding_dim`, plus `sqlite-vec` KNN virtual table `chunks_vec`). Embedding models are now stored in per-model subdirectories under `storage.model_path` with automatic legacy flat-cache migration; databases created with a different model or v1 schema require re-indexing.
- **MSRV**: Raised minimum supported Rust version to **Rust 1.91** (`rust-version = "1.91"`).

### Architecture & Performance
- **Concurrent SQLite WAL Storage**: Added an 8-connection read-only `r2d2` reader pool alongside a single transactional writer, eliminating reader/writer lock contention during indexing and concurrent queries.
- **True Vector KNN + Normalized Hybrid RRF**: Replaced full-table vector scans with `sqlite-vec` KNN (`WHERE embedding MATCH ?1 AND k = ?2`), progressive filter over-fetching, tokenized SQLite FTS5 keyword search with SQL pre-filtering, and normalized `[0.0, 1.0]` Reciprocal Rank Fusion. Removed the legacy in-memory LRU query cache.
- **Incremental Indexing Pipeline**: Added `blake3` content hashing + `(mtime_ns, file_size)` fast-skipping, per-path coalescing (`PathCoordinator`), bounded worker concurrency (`available_parallelism().clamp(2, 8)`), `500ms` default debounce, startup stale-file reconciliation scoped to active watch roots, and batched ONNX Runtime inference (`embed_batch`).
- **Chunking & Safe Ingestion**: Added UTF-8-safe chunk budget enforcement (`enforce_chunk_budget`), `O_NOFOLLOW` / Windows reparse-point-safe file reading (`safe_read`), 8 KiB binary sniffing, default secret/key/build-directory exclusions, and `--` flag-injection guards in external plugin execution.
- **Library Crate & Offline Example**: Added crate-level rustdoc documentation in `src/lib.rs` and an offline runnable `examples/embedded_search.rs`.

### MCP, HTTP API & `connect` Hardening
- **MCP Server (`2025-06-18`)**: Negotiates `2025-06-18` / `2025-03-26` / `2024-11-05`, enforces a 16 MiB JSON-RPC frame cap, supports `notifications/cancelled` with in-flight abort handles, offloads blocking work off the async reactor, and logs diagnostics exclusively to `stderr`.
- **Hardened Axum HTTP API**: Added constant-time Bearer auth, loopback `Host` header checks, 64 KiB body limit, 30s request timeout, 64-request concurrency cap, input validation (`query` length, `min_score` finite range, `limit` clamped to 100, `context_lines` clamped to 20), and default `limit = 5`.
- **Automated AI Tool Setup (`contextd connect`)**: Supports 9 AI tools (`claude-desktop`, `claude-code`, `cursor`, `vscode-copilot`, `copilot-cli`, `opencode`, `continue`, `antigravity`, `codex`) with JSONC (`json5`) and comment-preserving TOML (`toml_edit`) updates, syntax validation, `.bak` backups, and atomic file replacement.

### Distribution, Security & CI
- **Supply-Chain Integrity**: Pinned HuggingFace model commit revisions with SHA-256 verification before install; updated `scripts/install.sh`, `scripts/install.ps1`, and `scripts/playground.sh` to verify `SHA256SUMS.txt`.
- **Docker & Scripts**: Hardened multi-stage `Dockerfile` (non-root user, loopback healthcheck, token-gated `0.0.0.0` bind), fixed `scripts/pdftotext.sh` `--` separator handling, and isolated `scripts/playground.sh` with `--trust-config`.
- **CI & Docs**: Added `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` to CI, updated shell completions (`bash`, `zsh`, `fish`), added `ARCHITECTURE.md`, `.github/dependabot.yml`, `.github/ISSUE_TEMPLATE/config.yml`, and refreshed all mdBook and integration guides.

## [3.1.3] - 2026-07-02

### Fixed
- Flaky load tests: daemon stdin pipe management and health-based startup wait.
- Cargo.lock drift after formatting fixes.

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
