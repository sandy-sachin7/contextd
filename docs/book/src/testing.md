# Testing

`contextd` includes unit, doctest, integration, MCP end-to-end, benchmark, and memory-stress suites.

## Test Suites

| Suite | Command | Coverage |
|-------|---------|----------|
| **Unit & Library Tests** | `cargo test --lib --bins --locked` | Config & trust isolation, Tree-sitter/PDF chunking & UTF-8 budgets, `safe_read` (`O_NOFOLLOW`), ignore/secret exclusions, sandboxed plugins, SQLite WAL/`sqlite-vec`/FTS5 hybrid search, HTTP API security middleware, MCP server, `connect`, `download` |
| **Doc Tests** | `cargo test --doc --locked` | Crate-level embedded library quickstart in `src/lib.rs` |
| **Integration Tests** | `cargo test --tests --locked` | `watcher_test` (file events/renames/deletes), `load_test` (concurrent HTTP API + rapid edits), `zero_config_test` (model cache & setup) |
| **Embedded Example** | `cargo run --example embedded_search` | Offline Tree-sitter + SQLite WAL hybrid search library usage |
| **MCP E2E Suite** | `CONTEXTD_BIN=./target/debug/contextd python3 scripts/test_mcp_local.py` | 17 end-to-end JSON-RPC 2.0 / MCP protocol, validation, cancellation, and edge-case checks |
| **Memory Stress** | `python3 scripts/memory_stress_test.py` | Large corpus (10K files, 1K queries) memory-bound check |
| **Benchmarks** | `cargo bench --locked` / `bash scripts/bench.sh` | Criterion benchmarks (`chunker_bench`, `db_bench`) |

## Running the Full Verification Suite

```bash
# Formatting, lints, and rustdoc
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked

# All Rust tests (unit, doc, integration)
cargo test --locked

# Offline embedded example
cargo run --example embedded_search

# MCP end-to-end tests
CONTEXTD_BIN=./target/debug/contextd python3 scripts/test_mcp_local.py

# Shell script syntax checks
bash -n scripts/install.sh scripts/playground.sh scripts/pdftotext.sh
```
