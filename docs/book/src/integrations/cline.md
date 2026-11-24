# Cline / Roo Code (VSCode)

1. Open the MCP Servers settings (via the "MCP Servers" tab or command palette).
2. Add a new server:
   - **Name**: `contextd`
   - **Command**: `/path/to/contextd`
   - **Args**: `mcp`

> When `--config` is omitted, `contextd mcp` resolves your user-global config, database, and models from platform XDG directories. If you pass a custom config file outside your user config directory and want its `[storage]`/`[server]` settings honored, pass `mcp`, `--config`, `/path/to/contextd.toml`, `--trust-config`.
