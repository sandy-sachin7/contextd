# Running contextd Tests

## Full Rust Test Suite

```bash
# Run all unit and integration tests (no network or pre-downloaded models required)
cargo test

# Formatting and lint checks
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Targeted Test Suites

### Unit Tests
```bash
cargo test --lib
```

### Integration Tests
```bash
cargo test --test load_test
cargo test --test watcher_test
cargo test --test zero_config_test
```

### MCP End-to-End Tests (requires downloaded model)
```bash
cargo build --release
cargo run --release -- setup
python3 scripts/test_mcp_local.py
```

### Memory Stress Test (requires `psutil`)
```bash
pip install psutil
python3 scripts/memory_stress_test.py
```
