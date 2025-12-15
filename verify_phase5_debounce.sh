#!/bin/bash
set -e

# Build
cargo build

# Cleanup
pkill contextd || true
rm -f debounce_test.txt contextd_p5_debounce.db* output_debounce.txt

# Create config
cat > contextd.toml <<EOF
[server]
host = "127.0.0.1"
port = 3070

[storage]
db_path = "contextd_p5_debounce.db"
model_path = "models"

[watch]
paths = ["."]
EOF

# Run daemon
echo "Starting daemon..."
./target/debug/contextd > /tmp/output_debounce.txt 2>&1 &
PID=$!

# Wait for startup
sleep 5

# Simulate rapid updates
echo "Rapid updates..."
for i in {1..5}; do
    echo "Update $i" >> debounce_test.txt
    # No sleep or very short sleep
done

# Wait for debounce timeout (2000ms) + processing
sleep 3

# Check output
COUNT=$(grep "Indexed .* chunks for .*debounce_test.txt" /tmp/output_debounce.txt | wc -l)
echo "Indexing count: $COUNT"

# We expect fewer than 5, ideally 1 or 2
if [ "$COUNT" -lt 5 ] && [ "$COUNT" -gt 0 ]; then
    echo "Debounce Verification PASSED"
else
    echo "Debounce Verification FAILED (Count: $COUNT)"
    cat /tmp/output_debounce.txt
    kill $PID
    exit 1
fi

# Cleanup
kill $PID
rm contextd.toml debounce_test.txt contextd_p5_debounce.db* /tmp/output_debounce.txt
