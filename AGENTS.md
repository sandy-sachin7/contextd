# AGENTS.md

## Project Context

contextd is a local-first semantic context daemon for AI agents. It watches files,
indexes them via ONNX embeddings, and exposes search through REST API, CLI, and MCP.

## Repository

https://github.com/sandy-sachin7/contextd

## Build & Test

```bash
cargo build --release
cargo test                    # unit tests
cargo test --test load_test   # integration tests
cargo test --test watcher_test
python3 scripts/test_mcp_local.py  # MCP E2E tests
python3 scripts/memory_stress_test.py  # memory stress test
```

## CI

- `.github/workflows/ci.yml` — fmt check, build, test, clippy
- `.github/workflows/release.yml` — cross-compile for 5 targets on tag
- `.github/workflows/docker.yml` — Docker build + push
- `.github/workflows/audit.yml` — daily cargo-audit
- `.github/workflows/publish.yml` — crates.io publish

## Conventions

- Rust edition 2021, stable toolchain
- Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `style:`, `refactor:`
- All new code needs tests
- Document public API
- Use anyhow for error handling
