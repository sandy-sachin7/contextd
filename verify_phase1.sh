#!/bin/bash
set -e

# Build
echo "Building..."
cargo build

# Start daemon in background
echo "Starting daemon..."
./target/debug/contextd > daemon.log 2>&1 &
DAEMON_PID=$!

# Wait for startup
sleep 2

# Create test file
echo "Creating test file..."
printf "This is a test file for contextd.\n\nIt has two paragraphs." > test.txt

# Wait for indexing
sleep 2

# Query API
echo "Querying API..."
curl -v -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"query": "test", "limit": 5}' > curl_output.txt 2>&1

# Cleanup
echo "Stopping daemon..."
kill $DAEMON_PID
sleep 1

echo "Daemon Logs:"
cat daemon.log

rm test.txt

echo "Verifying DB content..."
sqlite3 contextd.db "SELECT COUNT(*) FROM chunks;"

rm contextd.db
# rm daemon.log # Keep log for debugging
echo "Done."
