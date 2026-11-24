# Contributing to contextd

Thank you for contributing to **contextd**! Whether you are fixing a bug, improving retrieval quality, adding language chunkers, or tightening security boundaries, this guide will get you productive quickly.

Please also review our [Code of Conduct](CODE_OF_CONDUCT.md), [Architecture Guide](ARCHITECTURE.md), and [Security Policy](SECURITY.md).

## Prerequisites

- **Rust**: `1.91+` (MSRV enforced in [`Cargo.toml`](Cargo.toml); `rust-toolchain.toml` tracks `stable` with `rustfmt` and `clippy`)
- **Python 3** (optional, for MCP end-to-end tests in `scripts/test_mcp_local.py`)

## Getting Started

```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
cargo build --locked
```

## Module Map

| Path | Responsibility |
| ---- | -------------- |
| `src/main.rs` | CLI entrypoint (`clap`), global flags (`--config`, `--trust-config`, `--trust-workspace-plugins`), subcommand dispatch |
| `src/cli.rs` | `setup`, `query`, `completions`, date/timestamp parsing |
| `src/config.rs` | TOML loading, workspace trust isolation, XDG path resolution, `CONTEXTD_*` env overrides |
| `src/daemon.rs` | Initial scan (`WalkBuilder`), `notify` watcher loop, `PathCoordinator` coalescing, startup reconciliation |
| `src/indexer/` | Tree-sitter & text/PDF chunking (`chunker.rs`), ONNX batched embeddings (`embeddings.rs`), `O_NOFOLLOW` reads (`safe_read.rs`), ignore/secret filters (`ignore.rs`), sandboxed external plugins (`plugins.rs`), file watcher (`watcher.rs`) |
| `src/storage/db.rs` | SQLite WAL storage (schema v2), `r2d2` read-only pool, `sqlite-vec` KNN, FTS5, normalized RRF hybrid search |
| `src/api/mod.rs` | Hardened Axum HTTP API (`/health`, `/status`, `/query`), DNS-rebinding & Bearer auth middleware |
| `src/mcp/server.rs` | JSON-RPC 2.0 / MCP stdio server (`search_context`, `get_status`, cancellation, frame limits) |
| `src/connect.rs` | Auto-configuration for 9 AI tools with JSONC/TOML preservation and `.bak` backups |
| `src/download.rs` | Pinned HuggingFace downloads, SHA-256 verification, per-model cache directories & legacy migration |

## Verification Checklist

Before opening a pull request, run the full local verification suite:

```bash
# Formatting
cargo fmt --all -- --check

# Linting (zero warnings)
cargo clippy --all-targets -- -D warnings

# Rustdoc build (zero warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked

# Unit, doc, and integration tests
cargo test --locked

# Offline embedded library example
cargo run --example embedded_search

# MCP end-to-end test suite (17 checks)
CONTEXTD_BIN=./target/debug/contextd python3 scripts/test_mcp_local.py

# Shell script syntax checks (if scripts changed)
bash -n scripts/install.sh scripts/playground.sh scripts/pdftotext.sh
```

If you modify CLI flags or subcommands, regenerate shell completions:

```bash
cargo run -- completions bash > completions/contextd.bash
cargo run -- completions zsh  > completions/_contextd
cargo run -- completions fish > completions/contextd.fish
```

## Commit & Pull Request Conventions

We use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new user-facing capability
- `fix:` — bug fix
- `perf:` — performance improvement
- `security:` — security hardening or vulnerability fix
- `docs:` — documentation or mdBook updates
- `refactor:` — code restructuring without behavior change
- `test:` — adding or updating tests
- `chore:` — tooling, CI, or dependency maintenance

Guidelines for smooth reviews:
1. Keep PRs focused on a single concern and include unit/integration tests for all behavior changes.
2. Never log user source code or search result contents to `stdout` or `stderr`.
3. Preserve security invariants (trust boundaries, `O_NOFOLLOW`, loopback/auth gates, bounded inputs).
4. Update relevant docs in `README.md`, `ARCHITECTURE.md`, or `docs/book/src/` when changing user-facing behavior.

## Security Issues

Do **not** report security vulnerabilities in public issues or PRs. Follow [`SECURITY.md`](SECURITY.md) to submit a private GitHub Security Advisory.

## License

By contributing to `contextd`, you agree that your contributions will be licensed under the [MIT License](LICENSE).
