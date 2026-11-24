# AGENTS.md

## Project Context

`contextd` (v4.0.1) is a local-first, privacy-preserving semantic context daemon and Rust library for AI agents. It watches local files, chunks them with Tree-sitter/Markdown/PDF/sandboxed plugins, embeds them via batched ONNX Runtime, stores them in SQLite WAL (`sqlite-vec` KNN + `FTS5`), and serves hybrid search over MCP stdio, hardened Axum HTTP, and CLI.

## Repository

https://github.com/sandy-sachin7/contextd

## Build & Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo test --lib --bins --locked      # unit tests
cargo test --locked                   # full suite (unit + doc + integration)
cargo run --example embedded_search   # offline library example
CONTEXTD_BIN=./target/debug/contextd python3 scripts/test_mcp_local.py  # 17 MCP E2E checks
bash -n scripts/install.sh scripts/playground.sh scripts/pdftotext.sh
```

## Architecture & Security Invariants

- **MSRV**: Rust `1.91+`, edition 2021 (`rust-toolchain.toml` tracks `stable`).
- **Storage**: Schema v2 SQLite WAL (`src/storage/db.rs`) with single writer + 8-connection read-only `r2d2` reader pool, `sqlite-vec` KNN, and Porter-stemmed `FTS5` (no in-memory LRU cache).
- **Concurrency & Watching**: `available_parallelism().clamp(2, 8)` workers, `500ms` default debounce, `PathCoordinator` per-path coalescing, `blake3` fast-skipping.
- **Trust Boundaries**: Workspace `./contextd.toml` (and untrusted `--config` without `--trust-config`) can only set `[watch]` and `[chunking]`; `[plugins]` requires explicit trust (`--trust-workspace-plugins`, `--trust-config`, or user-global `security.allow_workspace_plugins = true`).
- **HTTP & MCP Hardening**: Loopback `Host` check in unauthenticated mode, mandatory Bearer auth on non-loopback binds, default `limit = 5`, 64 KiB HTTP body cap, 16 MiB MCP line cap, `O_NOFOLLOW` / reparse-point-safe file reads (`safe_read.rs`), never log source code or search results.

## CI Workflows

- `.github/workflows/ci.yml` — fmt, clippy, rustdoc (`-D warnings`), unit tests (`cargo test --lib --bins`), integration tests, benchmark compile, Docker build
- `.github/workflows/audit.yml` — daily `cargo audit`
- `.github/workflows/release.yml` — cross-compiled multi-arch release binaries + `SHA256SUMS.txt`
- `.github/workflows/docker.yml` — GHCR image build & push
- `.github/workflows/publish.yml` — Crates.io publish
- `.github/workflows/docs.yml` — mdBook build & GitHub Pages deploy

## Conventions

- Conventional Commits: `feat:`, `fix:`, `perf:`, `security:`, `docs:`, `refactor:`, `test:`, `chore:`
- Use `anyhow` for application errors; keep all new code covered by tests and zero-warning rustdoc.
- Regenerate `completions/{contextd.bash,_contextd,contextd.fish}` via `cargo run -- completions <shell>` after any CLI change.
