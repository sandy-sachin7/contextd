# Continue.dev Integration

You can configure Continue automatically with:

```bash
contextd connect --tool continue
```

Or configure it manually in `~/.continue/config.yaml` (recommended — uses your trusted user-global `~/.config/contextd/contextd.toml` or platform defaults):

```yaml
mcpServers:
  - name: contextd
    command: /absolute/path/to/contextd
    args:
      - mcp
```

To point at a project-specific config outside your user config directory, create `.continue/mcpServers/contextd.yaml` and add `--trust-config` if you want its `[storage]` or `[plugins]` sections applied:

```yaml
name: contextd
command: /absolute/path/to/contextd
args:
  - mcp
  - --config
  - /absolute/path/to/contextd.toml
  - --trust-config
```

> Without `--trust-config`, a config file outside the user config directory is treated as untrusted: `[server]` and `[storage]` remain governed by your user-global config or platform defaults, and `[plugins]` remain disabled.

## Usage in Continue

In Continue chat, select MCP tools or ask naturally:

> "Use search_context to find code about semantic search with limit 3"
