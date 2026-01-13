# contextd

[![CI](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml/badge.svg)](https://github.com/sandy-sachin7/contextd/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![MCP Compatible](https://img.shields.io/badge/MCP-Compatible-blue)](https://modelcontextprotocol.io)

> **Local-first semantic search meets AI context** - A privacy-preserving daemon that transforms your codebase into queryable intelligence for AI assistants.

## Why contextd?

- 🔒 **100% Local** - Your code never leaves your machine
- 🤖 **MCP Native** - Universal backend for Claude, Cline, Roo Code, Continue, & more
- 🔍 **Hybrid Search** - Combines semantic understanding with keyword precision (FTS5)
- ⚡ **Lightning Fast** - Query caching and optimized indexing
- 🌍 **Polyglot** - Native support for Rust, Python, JS/TS, Go, Markdown, PDF
- 🎯 **Smart Chunking** - Tree-sitter based semantic code splitting
- 🔌 **Extensible** - Plugin system for any file format

## Quick Start

### 1. Install
```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
cargo run -- setup
cargo build --release
```

### 2. Run as Daemon
```bash
# Start the daemon (watches your configured directories)
./target/release/contextd daemon

# Or use the CLI for one-off queries
./target/release/contextd query "authentication system"
```

### 3. Connect your AI Tool

contextd works with Claude Desktop, Cline, Roo Code, and more.

**Example (Claude Desktop):**
Add to `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "contextd": {
      "command": "/path/to/contextd/target/release/contextd",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

See [MCP Integration Guide](docs/mcp-integration.md) for other tools.

## Features in Detail

### 🔍 Hybrid Search

contextd combines vector embeddings with full-text search (SQLite FTS5) for superior results:

- **Semantic**: Understands meaning and context
- **Keyword**: Fast exact matches
- **Weighted**: Automatically balances both approaches

### 🧩 Smart Code Chunking

Tree-sitter based parsing for:
- **Python**: Functions, classes, methods
- **JavaScript/TypeScript**: Functions, classes, arrow functions
- **Go**: Functions, methods, structs
- **Rust**: Functions, structs, impls, traits
- **Markdown**: Header-based sections
- **PDF**: Page-level extraction

### ⚡ Performance

- **Query Caching**: Repeated queries use cached embeddings
- **Adaptive Debouncing**: Batches file changes to avoid CPU spikes
- **Incremental Updates**: Only re-indexes changed files

### 🔌 Plugin System

Extend support to any file format:
```toml
[plugins]
docx = ["pandoc", "-t", "plain"]
ipynb = ["jupyter", "nbconvert", "--to", "markdown", "--stdout"]
```

## Usage

### Daemon Mode (Background Service)
```bash
# Start daemon with default config
contextd daemon

# With custom config
contextd daemon --config /path/to/contextd.toml
```

### CLI Mode (One-off Queries)
```bash
# Basic query
contextd query "authentication"

# With filters
contextd query "database schema" --limit 10 --min-score 0.7

# Filter by time range
contextd query "API changes" --after 2024-12-01
```

### REST API
```bash
# Query endpoint
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{
    "query": "How does auth work?",
    "limit": 5,
    "min_score": 0.5,
    "file_types": [".rs", ".py"]
  }'

# Health check
curl http://localhost:3030/health

# Status/stats
curl http://localhost:3030/status
```

### MCP Server Mode
```bash
# Run as MCP server (for Claude Desktop integration)
contextd mcp
```

## Configuration

Create `contextd.toml`:
```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "contextd.db"
model_path = "models"
model_type = "all-minilm-l6-v2"  # Configurable!

[search]
enable_cache = true
cache_ttl_seconds = 3600
hybrid_weight = 0.7  # 70% semantic, 30% keyword

[watch]
paths = ["/path/to/code", "/path/to/docs"]
debounce_ms = 200

[chunking]
max_chunk_size = 512
overlap = 50

[plugins]
docx = ["pandoc", "-t", "plain"]
py = ["cat"]  # Or use native parser
```

## Ignoring Files

contextd respects `.gitignore` by default. You can also create a `.contextignore` file to exclude specific files from indexing without affecting git:

```gitignore
# .contextignore
*.log
temp/
secret_keys.json
```

## Use Cases

### 1. AI-Powered Code Understanding
Ask Claude "Show me how authentication is implemented" and get actual code from your project.

### 2. Documentation Search
Index your Markdown docs and query them semantically: "deployment process" finds relevant sections even without exact keywords.

### 3. Research Notes
Turn your Zettelkasten or Obsidian vault into a queryable knowledge base.

### 4. Legacy Codebase Exploration
Point contextd at that scary old project and let AI help you understand it.

## Architecture
```
┌─────────────┐
│  File Watch │ → Debouncer → .contextignore Filter
└─────────────┘
       │
       ▼
┌─────────────┐
│   Parser    │ → Plugin System / Native Parsers
└─────────────┘
       │
       ▼
┌─────────────┐
│   Chunker   │ → Tree-sitter (Rust/Py/JS/TS/Go) / Header-based (MD) / Pages (PDF)
└─────────────┘
       │
       ▼
┌─────────────┐
│  Embedder   │ → ONNX Runtime (Local, no cloud!)
└─────────────┘
       │
       ▼
┌─────────────┐
│   Storage   │ → SQLite + FTS5 (Hybrid search)
└─────────────┘
       │
       ▼
┌─────────────┐
│ Query Layer │ → REST API / CLI / MCP Server
└─────────────┘
```

## Integrations

contextd works with any tool that supports the [Model Context Protocol](https://modelcontextprotocol.io):

- **Claude Desktop**: [Setup Guide](docs/mcp-integration.md#claude-desktop)
- **Cline / Roo Code**: [Setup Guide](docs/mcp-integration.md#cline--roo-code)
- **Continue**: [Setup Guide](docs/mcp-integration.md#continue)
- **Zed Editor**: (Coming soon)

### VSCode Extension
Coming soon! Track progress in [#issue-number]

### Obsidian Plugin
Community contribution welcome!

## Performance

Benchmarks on a typical codebase (10K files, ~500K LOC):

- **Initial indexing**: ~2-3 minutes
- **Query latency**: <50ms p99
- **Memory usage**: ~150MB for 100K chunks
- **Re-index on file change**: <100ms

## Comparison

| Feature | contextd | Sourcegraph | GitHub Copilot | Cursor |
|---------|----------|-------------|----------------|--------|
| Local-first | ✅ | ❌ | ❌ | ❌ |
| MCP Native | ✅ | ❌ | ❌ | ❌ |
| Hybrid Search | ✅ | ✅ | ❌ | ✅ |
| Open Source | ✅ | Partial | ❌ | ❌ |
| Self-hosted | ✅ | ✅ ($$$) | ❌ | ❌ |

## Testing

contextd v0.1.0 includes a comprehensive test suite ensuring rock-solid reliability:

### Test Coverage

- **26 Unit Tests**: Core functionality (chunking, plugins, database, config)
- **8 Integration Tests**: Load testing, file watcher reliability
- **25+ E2E Tests**: MCP protocol compliance, error handling, edge cases
- **Memory Stress Testing**: 10K files, 1K queries with profiling

### Running Tests

```bash
# Unit tests (fastest - 30s)
cargo test --bin contextd

# Integration tests
cargo test --test load_test
cargo test --test watcher_test

# MCP end-to-end tests
python3 scripts/test_mcp_local.py

# Memory stress test (requires psutil)
pip install psutil
python3 scripts/memory_stress_test.py
```

See [`tests/README.md`](tests/README.md) for detailed testing documentation.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md)

## Roadmap

- [ ] Pre-built binaries (Linux/Mac/Windows)
- [ ] VSCode extension
- [ ] Additional embedding models (CodeBERT, UniXcoder)
- [ ] Re-ranking layer (cross-encoder)
- [ ] Homebrew formula
- [ ] Docker image

## License

MIT - see [LICENSE](LICENSE)

## Acknowledgments

- Tree-sitter for AST parsing
- ONNX Runtime for local inference
- SQLite FTS5 for hybrid search
- The MCP community for the protocol

---

**Star ⭐ this repo if contextd helps you understand your code better!**
