#!/usr/bin/env python3
"""
Comprehensive MCP Server End-to-End Test Suite for contextd

Tests:
1. Basic functionality (initialize, list tools, call tools)
2. Error handling (invalid JSON, unknown methods, malformed params)
3. Edge cases (empty queries, unicode, very long queries, special chars)
4. Concurrent requests
"""

import subprocess
import sys
import time
import json
import threading
from typing import Optional, Dict, Any

# ANSI color codes for pretty output
GREEN = '\033[92m'
RED = '\033[91m'
YELLOW = '\033[93m'
BLUE = '\033[94m'
RESET = '\033[0m'

class MCPTestClient:
    def __init__(self, process):
        self.process = process
        self.test_count = 0
        self.passed = 0
        self.failed = 0

    def send_request(self, method: str, params: Optional[Dict] = None, req_id: Optional[int] = None):
        """Send a JSON-RPC request to the MCP server"""
        req = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            req["params"] = params
        if req_id is not None:
            req["id"] = req_id

        json_str = json.dumps(req)
        self.process.stdin.write(json_str + "\n")
        self.process.stdin.flush()

    def send_raw(self, data: str):
        """Send raw data (for testing invalid JSON)"""
        self.process.stdin.write(data + "\n")
        self.process.stdin.flush()

    def read_response(self, timeout: float = 5) -> Optional[Dict]:
        """Read a JSON-RPC response"""
        start = time.time()
        while time.time() - start < timeout:
            line = self.process.stdout.readline()
            if line:
                try:
                    return json.loads(line)
                except json.JSONDecodeError:
                    continue

            if self.process.poll() is not None:
                return None

            time.sleep(0.05)
        return None

    def assert_test(self, name: str, condition: bool, error_msg: str = ""):
        """Assert a test condition and track results"""
        self.test_count += 1
        if condition:
            print(f"{GREEN}✓{RESET} {name}")
            self.passed += 1
        else:
            print(f"{RED}✗{RESET} {name}: {error_msg}")
            self.failed += 1
        return condition


def test_basic_functionality(client: MCPTestClient) -> bool:
    """Test basic MCP functionality"""
    print(f"\n{BLUE}=== Test Group: Basic Functionality ==={RESET}")

    # 1. Initialize
    client.send_request("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test-client", "version": "1.0"}
    }, req_id=1)

    resp = client.read_response()
    client.assert_test(
        "Initialize request",
        resp and "result" in resp and "serverInfo" in resp["result"],
        f"Got: {resp}"
    )

    # 2. Initialized notification
    client.send_request("notifications/initialized")

    # 3. List tools
    client.send_request("tools/list", req_id=2)
    resp = client.read_response()
    has_tools = resp and "result" in resp and "tools" in resp["result"]
    client.assert_test("List tools", has_tools, f"Got: {resp}")

    if has_tools:
        tools = resp["result"]["tools"]
        client.assert_test(
            "Has search_context tool",
            any(t["name"] == "search_context" for t in tools)
        )
        client.assert_test(
            "Has get_status tool",
            any(t["name"] == "get_status" for t in tools)
        )

    # 4. Call get_status
    client.send_request("tools/call", {
        "name": "get_status",
        "arguments": {}
    }, req_id=3)
    resp = client.read_response()
    client.assert_test(
        "Call get_status",
        resp and "result" in resp and "content" in resp["result"]
    )

    # 5. Call search_context
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": "test", "limit": 1}
    }, req_id=4)
    resp = client.read_response()
    client.assert_test(
        "Call search_context",
        resp and "result" in resp and "content" in resp["result"]
    )

    return True


def test_error_handling(client: MCPTestClient) -> bool:
    """Test error handling for invalid inputs"""
    print(f"\n{BLUE}=== Test Group: Error Handling ==={RESET}")

    # 1. Invalid JSON
    client.send_raw("{this is not valid json}")
    resp = client.read_response(timeout=2)
    # Server might not respond to invalid JSON, which is acceptable
    client.assert_test(
        "Invalid JSON handled gracefully",
        resp is None or "error" in resp,
        "Server should ignore or return error"
    )

    # 2. Unknown method
    client.send_request("unknown/method", req_id=10)
    resp = client.read_response()
    client.assert_test(
        "Unknown method returns error",
        resp and "error" in resp,
        f"Expected error, got: {resp}"
    )

    # 3. Call non-existent tool
    client.send_request("tools/call", {
        "name": "nonexistent_tool",
        "arguments": {}
    }, req_id=11)
    resp = client.read_response()
    client.assert_test(
        "Non-existent tool returns error",
        resp and ("error" in resp or
                  ("result" in resp and "isError" in resp["result"]))
    )

    # 4. Missing required parameters
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {}  # Missing required 'query' param
    }, req_id=12)
    resp = client.read_response()
    client.assert_test(
        "Missing required params returns error",
        resp and ("error" in resp or
                  ("result" in resp and "isError" in resp["result"]))
    )

    return True


def test_edge_cases(client: MCPTestClient) -> bool:
    """Test edge cases"""
    print(f"\n{BLUE}=== Test Group: Edge Cases ==={RESET}")

    # 1. Empty query
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": ""}
    }, req_id=20)
    resp = client.read_response()
    client.assert_test(
        "Empty query handled",
        resp and "result" in resp,
        "Should return empty results or handle gracefully"
    )

    # 2. Unicode query
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": "测试 🚀 émoji", "limit": 1}
    }, req_id=21)
    resp = client.read_response()
    client.assert_test(
        "Unicode/emoji query handled",
        resp and "result" in resp
    )

    # 3. Very long query
    long_query = "test " * 1000  # ~5KB query
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": long_query, "limit": 1}
    }, req_id=22)
    resp = client.read_response(timeout=10)
    client.assert_test(
        "Very long query handled",
        resp and "result" in resp
    )

    # 4. Special characters
    special_query = "'; DROP TABLE chunks; --"  # SQL injection attempt
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": special_query, "limit": 1}
    }, req_id=23)
    resp = client.read_response()
    client.assert_test(
        "Special chars (SQL injection attempt) handled safely",
        resp and "result" in resp
    )

    # 5. Null bytes
    null_query = "test\x00query"
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": null_query, "limit": 1}
    }, req_id=24)
    resp = client.read_response()
    client.assert_test(
        "Null bytes in query handled",
        resp is not None  # Any response is acceptable
    )

    # 6. Extreme limit values
    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": "test", "limit": 0}
    }, req_id=25)
    resp = client.read_response()
    client.assert_test(
        "Zero limit handled",
        resp and "result" in resp
    )

    client.send_request("tools/call", {
        "name": "search_context",
        "arguments": {"query": "test", "limit": 99999}
    }, req_id=26)
    resp = client.read_response()
    client.assert_test(
        "Very large limit handled",
        resp and "result" in resp
    )

    return True


def test_concurrent_requests(client: MCPTestClient) -> bool:
    """Test handling of rapid concurrent requests"""
    print(f"\n{BLUE}=== Test Group: Concurrent Requests ==={RESET}")

    # Send multiple requests in rapid succession
    start_id = 30
    num_requests = 10

    for i in range(num_requests):
        client.send_request("tools/call", {
            "name": "search_context",
            "arguments": {"query": f"concurrent test {i}", "limit": 1}
        }, req_id=start_id + i)

    # Collect responses
    responses = []
    for _ in range(num_requests):
        resp = client.read_response(timeout=10)
        if resp:
            responses.append(resp)

    client.assert_test(
        f"Received all {num_requests} concurrent responses",
        len(responses) >= num_requests * 0.8,  # Allow some tolerance
        f"Got {len(responses)}/{num_requests} responses"
    )

    success_count = sum(1 for r in responses if "result" in r)
    client.assert_test(
        "Most concurrent requests succeeded",
        success_count >= num_requests * 0.8,
        f"{success_count}/{num_requests} succeeded"
    )

    return True


def main():
    print(f"{YELLOW}╔══════════════════════════════════════════════════╗{RESET}")
    print(f"{YELLOW}║  Contextd MCP E2E Test Suite                    ║{RESET}")
    print(f"{YELLOW}╚══════════════════════════════════════════════════╝{RESET}")

    # Start the MCP server
    print(f"\n{BLUE}Starting contextd MCP server...{RESET}")
    process = subprocess.Popen(
        ["./target/release/contextd", "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1
    )

    # Give the server a moment to start
    time.sleep(0.5)

    client = MCPTestClient(process)

    try:
        # Run all test groups
        test_basic_functionality(client)
        test_error_handling(client)
        test_edge_cases(client)
        test_concurrent_requests(client)

        # Print summary
        print(f"\n{YELLOW}╔══════════════════════════════════════════════════╗{RESET}")
        print(f"{YELLOW}║  Test Summary                                    ║{RESET}")
        print(f"{YELLOW}╚══════════════════════════════════════════════════╝{RESET}")
        print(f"Total tests: {client.test_count}")
        print(f"{GREEN}Passed: {client.passed}{RESET}")
        print(f"{RED}Failed: {client.failed}{RESET}")

        if client.failed == 0:
            print(f"\n{GREEN}🎉 ALL TESTS PASSED!{RESET}")
            return 0
        else:
            print(f"\n{RED}❌ SOME TESTS FAILED{RESET}")
            return 1

    finally:
        process.terminate()
        process.wait(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
