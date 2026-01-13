# Running Robustness Tests - Quick Reference

## Prerequisites

```bash
# Ensure release binary is built for integration tests
cargo build --release

# For memory stress test only
pip install psutil
```

## Quick Verification (30 seconds)

```bash
cargo test --bin contextd
```

Expected: `test result: ok. 26 passed; 0 failed; 1 ignored`

## All Tests

### Unit Tests
```bash
cargo test --bin contextd
```

### Integration Tests
```bash
# Run each test file separately (recommended)
cargo test --test load_test
cargo test --test watcher_test

# Or run all with single thread to avoid port conflicts
cargo test --test '*' -- --test-threads=1
```

### MCP E2E Tests
```bash
python3 scripts/test_mcp_local.py
```

### Memory Stress Test
```bash
python3 scripts/memory_stress_test.py
```

See [walkthrough.md](file:///home/sachin/.gemini/antigravity/brain/3ab4aac8-7218-4a37-9603-40227ed3ca02/walkthrough.md) for detailed documentation.
