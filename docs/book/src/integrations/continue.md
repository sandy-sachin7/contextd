# Continue.dev Integration

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

Or create `.continue/mcpServers/contextd.yaml` in your project root:

```yaml
name: contextd
command: /home/you/contextd/target/release/contextd
args:
  - --mcp
  - --config
  - /home/you/contextd/contextd.toml
```

## Usage in Continue

In Continue chat, use `@Tools` to select MCP tools, or just ask naturally:

> "Use search_context to find code about semantic search with limit 3"
