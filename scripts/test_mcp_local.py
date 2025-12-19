import subprocess
import sys
import time
import json

def send_request(process, method, params=None, req_id=None):
    req = {
        "jsonrpc": "2.0",
        "method": method,
    }
    if params is not None:
        req["params"] = params
    if req_id is not None:
        req["id"] = req_id

    json_str = json.dumps(req)
    process.stdin.write(json_str + "\n")
    process.stdin.flush()
    print(f"\n[Client] Sent {method}: {json_str}")

def read_response(process, timeout=5):
    start = time.time()
    while time.time() - start < timeout:
        line = process.stdout.readline()
        if line:
            print(f"[Client] Received: {line.strip()}")
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                print("[Client] Failed to decode JSON")

        if process.poll() is not None:
            print(f"[Client] Process exited with {process.returncode}")
            return None

        # Non-blocking check for stderr
        # (Simplified: in real app use threads/select, here we rely on line buffering)

        time.sleep(0.1)
    print("[Client] Timeout waiting for response")
    return None

def main():
    print("=== Contextd MCP Verification (Python) ===")
    process = subprocess.Popen(
        ["./target/release/contextd", "--mcp", "--config", "contextd.toml"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr, # Forward stderr to console
        text=True,
        bufsize=1 # Line buffered
    )

    try:
        # 1. Initialize
        send_request(process, "initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "verify-py", "version": "1.0"}
        }, req_id=1)

        resp = read_response(process)
        if not resp or "result" not in resp:
            print("FAILED: Initialize failed")
            return

        print("SUCCESS: Initialized")

        # 2. Initialized Notification
        send_request(process, "notifications/initialized")
        # No response expected

        # 3. List Tools
        send_request(process, "tools/list", req_id=2)
        resp = read_response(process)
        if not resp or "result" not in resp:
            print("FAILED: List tools failed")
            return

        tools = resp["result"]["tools"]
        print(f"SUCCESS: Found {len(tools)} tools")
        for t in tools:
            print(f"  - {t['name']}")

        # 4. Call get_status
        send_request(process, "tools/call", {
            "name": "get_status",
            "arguments": {}
        }, req_id=3)
        resp = read_response(process)
        if not resp or "result" not in resp:
            print("FAILED: get_status failed")
            return

        print("SUCCESS: get_status result:")
        print(resp["result"]["content"][0]["text"])

        # 5. Call search_context
        send_request(process, "tools/call", {
            "name": "search_context",
            "arguments": {
                "query": "semantic search",
                "limit": 1
            }
        }, req_id=4)
        resp = read_response(process)
        if not resp or "result" not in resp:
            print("FAILED: search_context failed")
            return

        print("SUCCESS: search_context result:")
        print(resp["result"]["content"][0]["text"])

    finally:
        process.terminate()
        process.wait()

if __name__ == "__main__":
    main()
