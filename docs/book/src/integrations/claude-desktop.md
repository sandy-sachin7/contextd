# Claude Desktop Integration

Add contextd to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "contextd": {
      "command": "/path/to/contextd/target/release/contextd",
      "args": ["mcp", "--config", "/path/to/contextd.toml"],
      "env": {}
    }
  }
}
```

**Config locations:**
- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Linux: `~/.config/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

## Usage in Claude

> "Search contextd for functions that handle file indexing"
> "Find code related to embedding generation, only in Rust files"
> "Use contextd to get the indexing status"
