# MCP Server

Run as MCP stdio server:

```bash
contextd mcp
contextd mcp --config /path/to/contextd.toml
```

## Available Tools

### search_context

Search for relevant code or documentation by meaning.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Natural language search query |
| `limit` | number | No | Max results (default: 5) |
| `file_types` | string[] | No | Filter by file extensions |
| `min_score` | number | No | Minimum relevance score (0.0-1.0) |

### get_status

Get the current indexing status (indexed files, chunks, DB size).
