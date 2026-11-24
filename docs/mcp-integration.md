# MCP Integration Guide

This guide explains how to integrate **contextd** with Claude Desktop, Claude Code, Cursor, VS Code, Codex, OpenCode, Continue, Copilot CLI, and Antigravity via the Model Context Protocol (MCP).

## Automatic Setup (Recommended)

```bash
# Download the default embedding model (stored in platform data dir)
contextd setup

# Configure all detected AI tools (user scope, creates .bak backups, preserves JSONC/TOML)
contextd connect --all

# Or configure project-local configs or a single tool
contextd connect --all --scope project
contextd connect --tool claude-desktop
```

## Prerequisites

1. **Install or build contextd**:
   ```bash
   cargo build --release
   ```

2. **Create a user config** (`~/.config/contextd/contextd.toml` on Linux, `~/Library/Application Support/contextd/contextd.toml` on macOS):
   ```toml
   [storage]
   db_path = "~/.local/share/contextd/contextd.db"
   model_path = "~/.local/share/contextd/models"

   [watch]
   paths = ["/path/to/your/codebase"]
   ```

3. **Download the embedding model**:
   ```bash
   contextd setup
   ```

## Manual Claude Desktop Configuration

- **macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Linux:** `~/.config/Claude/claude_desktop_config.json`
- **Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "contextd": {
      "command": "/path/to/contextd",
      "args": ["mcp"]
    }
  }
}
```

> Note: When `--config` is omitted, `contextd mcp` resolves config, database, and model paths from platform XDG directories so it works reliably regardless of the client's working directory. `--config` is a global flag and can appear before or after `mcp`; if you pass a config path outside your user config directory and want its `[storage]`/`[server]` settings honored, also include `--trust-config` (e.g. `["mcp", "--config", "/path/to/contextd.toml", "--trust-config"]`).

## Available Tools

### `search_context`

Hybrid semantic + keyword (FTS5) search over your indexed codebase.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Natural language or keyword search query |
| `limit` | integer | No | Max results (default: 5, max: 100) |
| `file_types` | string[] | No | Filter by extensions (e.g., `["rs", "py"]`) |
| `paths` | string[] | No | Filter by path substrings |
| `min_score` | number | No | Minimum relevance score (`0.0`-`1.0`) |
| `start_time` | integer | No | Filter files modified at or after UNIX timestamp (seconds) |
| `end_time` | integer | No | Filter files modified at or before UNIX timestamp (seconds) |
| `context_lines` | integer | No | Surrounding source lines to include (`0`-`20`) |

### `get_status`

Returns indexed file count, total chunks, and database size.

## First-Time Setup

1. Start `contextd daemon` to build and maintain the index.
2. Restart your MCP client after running `contextd connect --all`.
3. Ask your AI assistant to call `get_status` or `search_context`.

## Logs

`contextd mcp` writes protocol diagnostics to `stderr` only (never stdout, and never logs source code or search result contents).
