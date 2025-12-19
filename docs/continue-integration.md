# Continue.dev MCP Integration for contextd

This guide explains how to use **contextd** with the [Continue.dev](https://continue.dev) VSCode extension.

## Prerequisites

1. **Install Continue.dev**: Search for "Continue" in VSCode extensions
2. **Build contextd**: `cargo build --release`
3. **Index your codebase** first (run once as daemon):
   ```bash
   ./target/release/contextd --config contextd.toml
   ```

## Configuration

### Option 1: Global Config (`~/.continue/config.yaml`)

Add to your `~/.continue/config.yaml`:

```yaml
mcpServers:
  - name: contextd
    command: /absolute/path/to/contextd
    args:
      - --mcp
      - --config
      - /absolute/path/to/contextd.toml
```

### Option 2: Project-Level Config

Create `.continue/mcpServers/contextd.yaml` in your project root:

```yaml
name: contextd
command: /home/you/contextd/target/release/contextd
args:
  - --mcp
  - --config
  - /home/you/contextd/contextd.toml
```

## Available Tools

Once configured, Continue.dev can use these tools:

### `search_context`
Search for relevant code by meaning.

**Parameters:**
- `query` (string): Natural language search
- `limit` (number): Max results (default: 5)
- `file_types` (string[]): Filter by extension
- `min_score` (number): Minimum relevance 0.0-1.0

**Example prompts:**
- "Search contextd for functions that handle embeddings"
- "Find code related to file watching"
- "Search for database operations in Rust files"

### `get_status`
Get indexing statistics.

**Returns:**
- Number of indexed files
- Total content chunks
- Database size

## Usage in Continue

1. Open VSCode with Continue extension
2. In Continue chat, use `@Tools` to select MCP tools
3. Or just ask naturally:
   > "Use search_context to find code about semantic search with limit 3"

## Example Config File

Full `contextd.toml` for Continue:

```toml
[storage]
db_path = "/home/you/.contextd/index.db"
model_path = "/home/you/.contextd/models"

[watch]
# Your project(s) to index
paths = [
    "/home/you/your-project/src",
    "/home/you/your-project/docs"
]

[plugins]
# Optional: external parsers for special formats
# pdf = ["pdftotext", "-"]
```

## Troubleshooting

### "Tool not found"
- Ensure contextd path is absolute
- Verify the binary has execute permissions
- Check Continue logs: `Cmd+Shift+P` → "Continue: View Logs"

### Empty search results
- Make sure you've run contextd in daemon mode first to build the index
- Check that the database file exists

### Connection errors
- Verify contextd has access to the model files
- Try running `contextd --mcp` manually to check for errors
