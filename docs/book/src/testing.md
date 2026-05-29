# Testing

contextd includes a comprehensive test suite:

## Unit Tests (28+)

| Module | Tests | Coverage |
|--------|-------|----------|
| config | 2 | Defaults, TOML loading |
| chunker | 9 | Text, Rust, Python, JS, TS, Go, Markdown, PDF |
| embeddings | 3 | Creation, inference, dimensions |
| ignore | 1 | .contextignore patterns |
| plugins | 8 | Echo, failure, timeout, binary, large output |
| db | 13 | CRUD, recency, frequency, FTS sanitization |

## Integration Tests

| File | Tests | What it tests |
|------|-------|---------------|
| `watcher_test.rs` | 5 | Rapid file creation, nested directories, renames, deletes, permissions |
| `load_test.rs` | 3 | 50 concurrent API requests, 100 rapid file changes, sustained load for 30s |

## E2E / Script Tests

| Script | Tests | What it tests |
|--------|-------|---------------|
| `test_mcp_local.py` | 25+ | Basic, errors, edge cases (unicode, SQL injection, null bytes), concurrent |
| `verify_mcp.mjs` | 3 | SDK-based: connect, list tools, call tools |
| `memory_stress_test.py` | 1 | 10K files, 1K queries, <500MB memory |

## Running Tests

```bash
# Unit tests
cargo test --bin contextd

# Integration tests
cargo test --test load_test
cargo test --test watcher_test

# MCP end-to-end
python3 scripts/test_mcp_local.py

# Memory stress test
pip install psutil
python3 scripts/memory_stress_test.py
```
