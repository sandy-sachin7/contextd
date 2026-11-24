# MCP Server

Run `contextd` as an async JSON-RPC 2.0 / Model Context Protocol stdio server:

```bash
# Uses trusted user-global config (~/.config/contextd/contextd.toml) or platform defaults
contextd mcp

# Or point to an explicit config outside the user config dir
contextd mcp --config /path/to/contextd.toml --trust-config
```

> `contextd mcp` writes protocol diagnostics exclusively to `stderr` (never `stdout`) and never logs indexed source code or search result contents.

## Protocol & Hardening

- **Negotiated Versions**: `2025-06-18` (latest), `2025-03-26`, `2024-11-05`
- **Frame Cap**: 16 MiB max line length on `stdin`
- **Cancellation**: Honors `notifications/cancelled` via in-flight abort handles
- **Tool Annotations**: Read-only (`readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: false`)

## Available Tools

### `search_context`

Hybrid semantic (`sqlite-vec` KNN) + keyword (`FTS5`) search over your indexed codebase.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Natural language or keyword search query |
| `limit` | integer | No | Max results (default: `5`, max: `100`) |
| `file_types` | string[] | No | Filter by file extensions (e.g., `["rs", "py"]`) |
| `paths` | string[] | No | Filter by file path substrings |
| `min_score` | number | No | Minimum relevance score (`0.0`-`1.0`) |
| `start_time` | integer | No | Filter files modified at or after UNIX timestamp (seconds) |
| `end_time` | integer | No | Filter files modified at or before UNIX timestamp (seconds) |
| `context_lines` | integer | No | Surrounding source lines to include (`0`-`20`) |

### `get_status`

Get the current indexing status (`indexed_files`, `total_chunks`, `database_size_bytes`).
