# Continue.dev MCP Integration for contextd

This guide explains how to use **contextd** with the [Continue.dev](https://continue.dev) IDE extension.

## Automatic Setup (Recommended)

```bash
contextd setup
contextd connect --tool continue
```

`contextd connect` configures `~/.continue/config.json` (or `.continue/` with `--scope project`), creates a `.bak` backup, and launches `contextd mcp` using your user-global configuration.

## Prerequisites

1. **Install Continue.dev** in VS Code or JetBrains.
2. **Install or build contextd**: `cargo install contextd --locked` (or `cargo build --release --locked`).
3. **Index your codebase** by running `contextd daemon` once (or keeping it running in the background).

## Manual Configuration

### Option 1: User-Global Config (Recommended)

When `--config` is omitted, `contextd mcp` loads your trusted user-global config (`~/.config/contextd/contextd.toml` on Linux, `~/Library/Application Support/contextd/contextd.toml` on macOS) or platform defaults so it works regardless of the IDE's working directory.

Add to `~/.continue/config.yaml`:

```yaml
mcpServers:
  - name: contextd
    command: /absolute/path/to/contextd
    args:
      - mcp
```

### Option 2: Explicit Config Path

If you pass an explicit `--config` located **outside** the user config directory (for example, inside a project folder), `contextd` treats it as untrusted by default and ignores its `[server]`, `[storage]`, and `[plugins]` sections. Pass `--trust-config` only if you own and trust that config file:

```yaml
# .continue/mcpServers/contextd.yaml
name: contextd
command: /absolute/path/to/contextd
args:
  - mcp
  - --config
  - /absolute/path/to/contextd.toml
  - --trust-config
```

## Available Tools

### `search_context`
Hybrid semantic (`sqlite-vec`) + keyword (`FTS5`) search over your indexed codebase.

**Parameters:**
- `query` (string, required): Natural language or keyword search query
- `limit` (integer): Max results (default: `5`, max: `100`)
- `file_types` (string[]): Filter by extension (e.g., `["rs", "py"]`)
- `paths` (string[]): Filter by path substrings
- `min_score` (number): Minimum relevance score (`0.0`-`1.0`)
- `start_time` / `end_time` (integer): UNIX timestamp bounds (seconds)
- `context_lines` (integer): Surrounding source lines (`0`-`20`)

### `get_status`
Returns indexed file count, total chunks, and database size.

## Usage in Continue

1. Open VS Code with the Continue extension.
2. In Continue chat, select the `contextd` MCP tools or ask naturally:
   > "Use search_context to find code about semantic search with limit 3"

## Example User-Global Config (`~/.config/contextd/contextd.toml`)

```toml
[storage]
db_path = "~/.local/share/contextd/contextd.db"
model_path = "~/.local/share/contextd/models"
model_type = "all-minilm-l6-v2"

[watch]
paths = [
    "/home/you/your-project/src",
    "/home/you/your-project/docs"
]
```

## Troubleshooting

- **"Tool not found"**: Ensure the `contextd` path is absolute and executable, and check Continue logs (`Cmd/Ctrl+Shift+P` -> "Continue: View Logs").
- **Empty search results**: Make sure `contextd daemon` has indexed your workspace into the same `db_path`.
- **Connection errors**: Verify model files exist (`contextd setup`) and test manually with `contextd mcp`. Diagnostics are written exclusively to `stderr`.
