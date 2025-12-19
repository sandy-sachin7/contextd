# Claude Desktop Integration Guide

This guide explains how to integrate **contextd** with Claude Desktop as an MCP (Model Context Protocol) server.

## Overview

contextd provides semantic search capabilities to Claude Desktop through the MCP protocol. This allows Claude to:

- Search your codebase using natural language queries
- Find semantically related code and documentation
- Filter results by file type or minimum relevance score

## Prerequisites

1. **Build contextd**:
   ```bash
   cargo build --release
   ```

2. **Create a configuration file** (`contextd.toml`):
   ```toml
   [storage]
   db_path = "~/.contextd/index.db"
   model_path = "~/.contextd/models"

   [watch]
   paths = ["/path/to/your/codebase"]
   ```

3. **Download the embedding model**:
   ```bash
   mkdir -p ~/.contextd/models
   # Download all-MiniLM-L6-v2 ONNX model
   cd ~/.contextd/models
   curl -LO https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx
   curl -LO https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json
   ```

## Claude Desktop Configuration

Add contextd to your `claude_desktop_config.json`:

### macOS
```bash
# Location: ~/Library/Application Support/Claude/claude_desktop_config.json
```

### Linux
```bash
# Location: ~/.config/Claude/claude_desktop_config.json
```

### Windows
```batch
:: Location: %APPDATA%\Claude\claude_desktop_config.json
```

### Configuration

```json
{
  "mcpServers": {
    "contextd": {
      "command": "/path/to/contextd",
      "args": ["--mcp", "--config", "/path/to/contextd.toml"],
      "env": {}
    }
  }
}
```

Replace `/path/to/contextd` with the actual path to your built binary (e.g., `target/release/contextd`).

## Available Tools

### search_context

Search for relevant code or documentation by meaning.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Natural language search query |
| `limit` | number | No | Max results (default: 5) |
| `file_types` | string[] | No | Filter by extensions (e.g., ["rs", "py"]) |
| `min_score` | number | No | Minimum relevance score (0.0-1.0) |

**Example usage in Claude:**
> "Search contextd for functions that handle file indexing"
> "Find code related to embedding generation, only in Rust files"

### get_status

Get the current indexing status.

**Returns:**
- Number of indexed files
- Total content chunks
- Database size

## First-Time Setup

1. **Start contextd in daemon mode first** to build the initial index:
   ```bash
   ./contextd --config ./contextd.toml
   ```
   Wait for the initial indexing to complete.

2. **Restart Claude Desktop** after configuring `claude_desktop_config.json`.

3. **Verify the connection** by asking Claude:
   > "Use contextd to get the indexing status"

## Troubleshooting

### "Server not found" or connection issues

1. Check that the path to contextd is correct
2. Ensure the binary has execute permissions: `chmod +x contextd`
3. Verify the config file exists at the specified path

### "Database error" or empty results

1. Ensure you've run contextd in daemon mode first to build the index
2. Check that the database file exists at the configured path
3. Verify the embedding model is downloaded to the model path

### Logs

When running as an MCP server, contextd logs to stderr. Check Claude Desktop's logs for error messages.

## Architecture

```
┌─────────────────┐     stdio      ┌──────────────────┐
│  Claude Desktop │ ◄─────────────►│     contextd     │
│   (MCP Client)  │    JSON-RPC    │   (MCP Server)   │
└─────────────────┘                └────────┬─────────┘
                                            │
                                   ┌────────▼─────────┐
                                   │  SQLite + Index  │
                                   │   Embeddings     │
                                   └──────────────────┘
```
