import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

async function main() {
  console.log("=== Contextd MCP Server Verification ===");

  // 1. Connect to server
  console.log("[Test] Launching contextd...");
  const transport = new StdioClientTransport({
    command: "./target/release/contextd",
    args: ["--mcp", "--config", "contextd.toml"],
  });

  const client = new Client(
    {
      name: "verify-client",
      version: "1.0.0",
    },
    {
      capabilities: {},
    }
  );

  try {
    await client.connect(transport);
    console.log("[Test] Connected to server!");

    // 2. List tools
    console.log("\n[Test] Listing tools...");
    const tools = await client.listTools();
    console.log(`[Test] Found ${tools.tools.length} tools:`);
    tools.tools.forEach((t) => console.log(`  - ${t.name}: ${t.description}`));

    // 3. Call get_status
    console.log("\n[Test] Calling get_status...");
    const status = await client.callTool({
      name: "get_status",
      arguments: {},
    });

    const statusText = status.content[0].text;
    console.log("[Test] Status Result:\n" + statusText);

    // 4. Call search_context
    console.log("\n[Test] Calling search_context (query: 'semantic search')...");
    const search = await client.callTool({
      name: "search_context",
      arguments: {
        query: "semantic search",
        limit: 1,
      },
    });

    const searchText = search.content[0].text;
    console.log("[Test] Search Result:\n" + searchText);

    console.log("\n=== Verification SUCCESS ===");

  } catch (error) {
    console.error("\n=== Verification FAILED ===");
    console.error(error);
    process.exit(1);
  } finally {
    await client.close();
  }
}

main();
