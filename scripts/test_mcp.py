#!/usr/bin/env python3
"""
Test script for contextd MCP server.
Tests the MCP protocol communication via stdio.
"""

import subprocess
import json
import sys

def send_request(process, method, params=None, id=1):
    """Send a JSON-RPC request to the MCP server."""
    request = {
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    }
    if params:
        request["params"] = params

    message = json.dumps(request)
    # MCP uses content-length header format
    header = f"Content-Length: {len(message)}\r\n\r\n"
    full_message = header + message

    process.stdin.write(full_message.encode())
    process.stdin.flush()

    # Read response
    response_header = b""
    while b"\r\n\r\n" not in response_header:
        chunk = process.stdout.read(1)
        if not chunk:
            break
        response_header += chunk

    # Parse content length
    header_str = response_header.decode()
    content_length = 0
    for line in header_str.split("\r\n"):
        if line.startswith("Content-Length:"):
            content_length = int(line.split(":")[1].strip())

    # Read body
    body = process.stdout.read(content_length).decode()
    return json.loads(body)


def main():
    print("=== Testing contextd MCP Server ===\n")

    # Start the MCP server
    print("Starting contextd in MCP mode...")
    process = subprocess.Popen(
        ["./target/release/contextd", "--mcp", "--config", "contextd.toml"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        # Test 1: Initialize
        print("\n1. Testing initialize...")
        response = send_request(process, "initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })
        print(f"   Server: {response.get('result', {}).get('serverInfo', {})}")

        # Send initialized notification
        send_request(process, "notifications/initialized", {})

        # Test 2: List tools
        print("\n2. Listing available tools...")
        response = send_request(process, "tools/list", {}, id=2)
        tools = response.get("result", {}).get("tools", [])
        for tool in tools:
            print(f"   - {tool.get('name')}: {tool.get('description', '')[:60]}...")

        # Test 3: Call get_status
        print("\n3. Testing get_status tool...")
        response = send_request(process, "tools/call", {
            "name": "get_status",
            "arguments": {}
        }, id=3)
        result = response.get("result", {})
        if "content" in result:
            for content in result["content"]:
                print(f"   {content.get('text', '')}")

        # Test 4: Call search_context
        print("\n4. Testing search_context tool...")
        response = send_request(process, "tools/call", {
            "name": "search_context",
            "arguments": {
                "query": "semantic search embedding",
                "limit": 2
            }
        }, id=4)
        result = response.get("result", {})
        if "content" in result:
            for content in result["content"]:
                text = content.get("text", "")
                # Truncate long output
                if len(text) > 200:
                    text = text[:200] + "..."
                print(f"   {text}")

        print("\n=== All tests completed! ===")

    except Exception as e:
        print(f"Error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        process.terminate()
        process.wait()


if __name__ == "__main__":
    main()
