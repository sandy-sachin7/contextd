# Claude Desktop Integration

Add contextd to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "contextd": {
      "command": "/path/to/contextd",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

> Or run `contextd connect --tool claude-desktop` to configure this automatically. When `--config` is omitted, `contextd mcp` loads config, database, and models from platform XDG directories (`~/.config/contextd/contextd.toml`). If you pass a custom config path outside that directory and want its `[server]`/`[storage]` settings honored, add `"--config", "/path/to/contextd.toml", "--trust-config"`.

**Config locations:**
- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Linux: `~/.config/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

## Usage in Claude

> "Search contextd for functions that handle file indexing"
> "Find code related to embedding generation, only in Rust files"
> "Use contextd to get the indexing status"
