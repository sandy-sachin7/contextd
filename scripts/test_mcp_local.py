#!/usr/bin/env python3
"""
MCP Server End-to-End Test Suite for contextd.

Requires a built binary (default: ./target/release/contextd, override with CONTEXTD_BIN)
and downloaded model files.
"""

import json
import os
import subprocess
import sys
import time
from typing import Dict, Optional

GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
BLUE = "\033[94m"
RESET = "\033[0m"


class MCPTestClient:
    def __init__(self, process):
        self.process = process
        self.test_count = 0
        self.passed = 0
        self.failed = 0

    def send_request(self, method: str, params: Optional[Dict] = None, req_id: Optional[int] = None):
        req = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            req["params"] = params
        if req_id is not None:
            req["id"] = req_id
        self.process.stdin.write(json.dumps(req) + "\n")
        self.process.stdin.flush()

    def send_raw(self, data: str):
        self.process.stdin.write(data + "\n")
        self.process.stdin.flush()

    def read_response(self, timeout: float = 5.0) -> Optional[Dict]:
        start = time.time()
        while time.time() - start < timeout:
            line = self.process.stdout.readline()
            if line:
                return json.loads(line)
            if self.process.poll() is not None:
                return None
            time.sleep(0.05)
        return None

    def assert_test(self, name: str, condition: bool, error_msg: str = ""):
        self.test_count += 1
        if condition:
            print(f"{GREEN}✓{RESET} {name}")
            self.passed += 1
        else:
            print(f"{RED}✗{RESET} {name}: {error_msg}")
            self.failed += 1
        return condition


def test_basic_functionality(client: MCPTestClient):
    print(f"\n{BLUE}=== Test Group: Basic Functionality ==={RESET}")

    client.send_request(
        "initialize",
        {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"},
        },
        req_id=1,
    )
    resp = client.read_response()
    client.assert_test(
        "Initialize returns mutually-exclusive result",
        bool(resp and "result" in resp and "error" not in resp and "serverInfo" in resp["result"]),
        f"Got: {resp}",
    )

    client.send_request("notifications/initialized")

    client.send_request("ping", req_id=100)
    resp = client.read_response()
    client.assert_test(
        "Ping returns empty object result",
        bool(resp and resp.get("result") == {} and "error" not in resp),
        f"Got: {resp}",
    )

    client.send_request("tools/list", req_id=2)
    resp = client.read_response()
    has_tools = bool(resp and "result" in resp and "tools" in resp["result"])
    client.assert_test("List tools", has_tools, f"Got: {resp}")

    if has_tools:
        tools = resp["result"]["tools"]
        client.assert_test("Has search_context tool", any(t["name"] == "search_context" for t in tools))
        client.assert_test("Has get_status tool", any(t["name"] == "get_status" for t in tools))

    client.send_request("tools/call", {"name": "get_status", "arguments": {}}, req_id=3)
    resp = client.read_response()
    client.assert_test(
        "Call get_status",
        bool(resp and "result" in resp and resp["result"].get("isError") is False),
        f"Got: {resp}",
    )

    client.send_request(
        "tools/call",
        {"name": "search_context", "arguments": {"query": "test", "limit": 1}},
        req_id=4,
    )
    resp = client.read_response()
    client.assert_test(
        "Call search_context",
        bool(resp and "result" in resp and resp["result"].get("isError") is False),
        f"Got: {resp}",
    )


def test_error_handling(client: MCPTestClient):
    print(f"\n{BLUE}=== Test Group: Error Handling ==={RESET}")

    client.send_raw("{this is not valid json}")
    resp = client.read_response(timeout=2)
    client.assert_test(
        "Invalid JSON returns -32700 error without result",
        bool(resp and "error" in resp and "result" not in resp and resp["error"]["code"] == -32700),
        f"Got: {resp}",
    )

    client.send_request("unknown/method", req_id=10)
    resp = client.read_response()
    client.assert_test(
        "Unknown method returns -32601",
        bool(resp and "error" in resp and resp["error"]["code"] == -32601),
        f"Got: {resp}",
    )

    client.send_request("tools/call", {"name": "nonexistent_tool", "arguments": {}}, req_id=11)
    resp = client.read_response()
    client.assert_test(
        "Non-existent tool returns -32602",
        bool(resp and "error" in resp and resp["error"]["code"] == -32602),
        f"Got: {resp}",
    )

    client.send_request("tools/call", {"name": "search_context", "arguments": {}}, req_id=12)
    resp = client.read_response()
    client.assert_test(
        "Missing required query returns -32602",
        bool(resp and "error" in resp and resp["error"]["code"] == -32602),
        f"Got: {resp}",
    )


def test_edge_cases(client: MCPTestClient):
    print(f"\n{BLUE}=== Test Group: Edge Cases ==={RESET}")

    cases = [
        ("Empty query", {"query": ""}, 20),
        ("Unicode query", {"query": "测试 🚀 émoji", "limit": 1}, 21),
        ("Very long query", {"query": "test " * 500, "limit": 1}, 22),
        ("SQL injection string", {"query": "'; DROP TABLE chunks; --", "limit": 1}, 23),
        ("Zero limit", {"query": "test", "limit": 0}, 25),
        ("Very large limit capped", {"query": "test", "limit": 99999}, 26),
    ]
    for label, args, rid in cases:
        client.send_request("tools/call", {"name": "search_context", "arguments": args}, req_id=rid)
        resp = client.read_response(timeout=10)
        client.assert_test(
            label,
            bool(resp and "result" in resp and resp["result"].get("isError") is False),
            f"Got: {resp}",
        )


def main():
    binary = os.environ.get("CONTEXTD_BIN", "./target/release/contextd")
    process = subprocess.Popen(
        [binary, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    time.sleep(0.3)
    if process.poll() is not None:
        print(f"{RED}MCP server exited prematurely:{RESET}\n{process.stderr.read()}")
        return 1

    client = MCPTestClient(process)
    try:
        test_basic_functionality(client)
        test_error_handling(client)
        test_edge_cases(client)

        print(f"\nTotal: {client.test_count}, Passed: {client.passed}, Failed: {client.failed}")
        return 0 if client.failed == 0 else 1
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()


if __name__ == "__main__":
    sys.exit(main())
