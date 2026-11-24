<div align="center">
  <img src="https://raw.githubusercontent.com/sandy-sachin7/contextd/main/assets/logo.png" alt="contextd logo" width="120" />

  <h1>contextd</h1>
  <p><b>Local-first semantic search meets AI context.</b></p>
  <p><i>A privacy-preserving daemon that transforms your codebase into queryable intelligence for AI assistants.</i></p>

  [![CI](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml/badge.svg)](https://github.com/sandy-sachin7/contextd/actions)
  [![Release](https://img.shields.io/github/v/release/sandy-sachin7/contextd)](https://github.com/sandy-sachin7/contextd/releases)
  [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
  [![MCP Compatible](https://img.shields.io/badge/MCP-Compatible-blue)](https://modelcontextprotocol.io)
  [![VSCode Marketplace](https://img.shields.io/badge/VSCode-Marketplace-blue)](https://marketplace.visualstudio.com/items?itemName=SanthoshSachin.contextd-semantic-search)
</div>

<hr/>

## Why contextd?

- 🔒 **100% Local** - Your code never leaves your machine
- 🤖 **MCP Native** - JSON-RPC 2.0 / MCP backend for Claude Desktop, Claude Code, Cursor, VS Code, Codex, OpenCode, Continue, Copilot CLI, & Antigravity
- 🔍 **Hybrid Search** - Combines `sqlite-vec` KNN semantic embeddings with tokenized SQLite FTS5 keyword search (normalized RRF) across CLI, HTTP API, and MCP
- ⚡ **Concurrent WAL Storage** - Dedicated WAL read-only connection pool + transactional writes and batched ONNX inference
- 🌍 **Polyglot** - Tree-sitter semantic chunking for Rust, Python, JS/JSX, TS/TSX, Go, plus Markdown and PDF
- 🛡️ **Secure by Default** - DNS-rebinding protection, optional bearer auth, secret/binary file exclusions, SHA-256 verified model downloads, and workspace plugin trust gating

## Quick Start

### 0. Playground (Try in 30 Seconds)

```bash
curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/playground.sh | bash
```

### 1. Install

**One-line installer (Linux/macOS, with SHA-256 verification):**
```bash
curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.sh | bash
```

**Homebrew (macOS/Linux):**
```bash
brew install --build-from-source ./homebrew/contextd.rb
```

**Docker:**
```bash
docker run -p 127.0.0.1:3030:3030 \
  -e CONTEXTD_AUTH_TOKEN=replace-with-strong-token \
  -v "$PWD:/workspace:ro" \
  ghcr.io/sandy-sachin7/contextd
```

**From source:**
```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
cargo build --release --locked
./target/release/contextd setup
```

### 2. Run as Daemon
```bash
# Start the daemon (watches your configured directories)
contextd daemon

# One-off hybrid queries from the CLI
contextd query "authentication system" --limit 10 --min-score 0.5 --context 2
```

### 3. Connect your AI Tools (Automatic)

```bash
# Auto-detect AI tools and configure user-global MCP settings
contextd connect --all

# Or configure project-local settings / a single tool
contextd connect --all --scope project
contextd connect --tool claude-desktop
```

contextd supports **9 AI tools** out of the box:

| Tool | ID | Config Format | Detection |
|------|----|--------------|-----------|
| **Claude Desktop** | `claude-desktop` | Object (`mcpServers`) | App / config dir |
| **Claude Code** | `claude-code` | Object (`mcpServers`) | `claude` binary or `~/.claude.json` |
| **Cursor** | `cursor` | Object (`mcpServers`) | `cursor` binary or `~/.cursor/` |
| **VS Code Copilot** | `vscode-copilot` | Object (`servers`) | `code` / `code-insiders` or `.vscode/` |
| **Copilot CLI** | `copilot-cli` | Object (`mcpServers`) | `copilot` / `github-copilot-cli` in PATH |
| **OpenCode** | `opencode` | Object (`mcp` + `$schema`) | `opencode` binary or `~/.config/opencode/` |
| **Continue** | `continue` | Array (`mcpServers[]`) | `~/.continue/` directory |
| **Antigravity (agy)** | `antigravity` | Object (`mcpServers`) | `agy` binary in PATH |
| **Codex** | `codex` | TOML (`[mcp_servers.contextd]`) | `codex` binary or `~/.codex/config.toml` |

Each `contextd connect` run:
1. Parses JSON/JSONC (preserving comments/trailing commas via JSON5) and TOML (preserving comments via `toml_edit`)
2. Refuses to overwrite if an existing config has syntax errors
3. Writes a `.bak` backup and atomically replaces the target file

See [MCP Integration Guide](docs/mcp-integration.md) for manual setup.

## Usage

### Daemon Mode
```bash
contextd daemon
contextd daemon --config /path/to/contextd.toml
# Optional: exit automatically if parent stdin closes (editor integrations)
contextd daemon --exit-on-stdin-eof
```

### CLI Query Mode
```bash
contextd query "authentication"
contextd query "database schema" --limit 10 --min-score 0.5 --file-types rs,sql
contextd query "API changes" --after 2024-12-01 --before 2026-12-31 --context 3
```

### Shell Completions
```bash
contextd completions bash > ~/.local/share/bash-completion/completions/contextd
contextd completions zsh  > ~/.zfunc/_contextd
contextd completions fish > ~/.config/fish/completions/contextd.fish
```

### REST API
```bash
# Hybrid search endpoint
curl -X POST http://127.0.0.1:3030/query \
  -H "Content-Type: application/json" \
  -d '{
    "query": "How does auth work?",
    "limit": 5,
    "min_score": 0.5,
    "file_types": ["rs", "py"],
    "context_lines": 2
  }'

# Health check & status
curl http://127.0.0.1:3030/health
curl http://127.0.0.1:3030/status
```

### MCP Server Mode
```bash
contextd mcp
contextd mcp --config /path/to/contextd.toml
```

## Configuration

See [Configuration Guide](docs/book/src/configuration.md) and [`contextd.example.toml`](contextd.example.toml).

### Available Embedding Models

| Model | Dimensions | Pooling | Best For |
|-------|------------|---------|----------|
| `all-minilm-l6-v2` (default) | 384 | Mean | General purpose, fast |
| `all-mpnet-base-v2` | 768 | Mean | Higher quality, recommended for code |
| `bge-small-en-v1.5` | 384 | CLS | Quality/speed balance for retrieval |

All downloads use pinned HuggingFace commit revisions verified by SHA-256.

## Testing

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

See [`tests/README.md`](tests/README.md) for details.

## License

MIT - see [LICENSE](LICENSE)
