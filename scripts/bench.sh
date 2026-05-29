#!/usr/bin/env bash
set -euo pipefail

echo "=== contextd Benchmarks ==="
echo ""

if ! cargo criterion --version &>/dev/null 2>&1; then
    echo "Installing cargo-criterion..."
    cargo install cargo-criterion --locked
fi

echo "=== Chunker Benchmarks ==="
cargo criterion --bench chunker_bench 2>&1

echo ""
echo "=== Database Benchmarks ==="
cargo criterion --bench db_bench 2>&1

echo ""
echo "=== All benchmarks complete ==="
echo "HTML reports available in target/criterion/"
