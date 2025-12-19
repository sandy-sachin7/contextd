# contextd

[![CI](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml/badge.svg)](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**contextd** is a local-first, semantic context daemon designed to empower AI agents with deep understanding of your codebase and documents. It runs silently in the background, indexing your files into vector embeddings, and provides a high-performance API for semantic retrieval.

Unlike cloud-based solutions, `contextd` keeps all your data on your machine, ensuring privacy and zero latency.

## 🚀 Features

-   **🔒 Local-First & Private**: Your data never leaves your machine. Embeddings are generated locally using ONNX Runtime.
-   **🧠 Semantic Search**: Powered by `all-MiniLM-L6-v2`, enabling your AI to find relevant context by meaning, not just keywords.
-   **⚡ Real-Time Indexing**: Watches your file system for changes and updates the index instantly (with adaptive debouncing).
-   **🔍 Initial Scan**: Automatically indexes existing files in watched directories on startup.
-   **🧩 Semantic Chunking**: Smart splitting for:
    -   **Rust**: Function/Struct-level chunking via Tree-sitter.
    -   **Markdown**: Header-based section splitting.
    -   **PDF**: Page-level splitting.
-   **📂 Multi-Format Support**: Native support for `.txt`, `.md`, `.pdf`, and `.rs`.
-   **🤖 MCP Server**: Native support for the [Model Context Protocol](https://modelcontextprotocol.io/), allowing Claude Desktop and other AI assistants to directly search your codebase.
-   **🔌 Extensible Plugin System**: Add support for any file type (DOCX, EPUB, etc.) via external command-line parsers.
-   **🛡️ Systems Brutality**: Built for robustness with adaptive debouncing, `.contextignore` support, and efficient resource usage.
-   **⚙️ Highly Configurable**: Customize everything via `contextd.toml`.

## 📦 Installation

### Prerequisites

-   **Rust**: Latest stable version ([Install Rust](https://rustup.rs/))
-   **Build Tools**: `build-essential` (Linux) or equivalent.

### Build from Source

```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
./setup_model.sh  # Downloads the ONNX model
cargo build --release
```

The binary will be located at `./target/release/contextd`.

## 🏃 Usage

### 1. Configuration

Create a `contextd.toml` file in your working directory (or rely on defaults).

```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "contextd.db"
model_path = "models"

[watch]
paths = ["/path/to/your/notes", "/path/to/your/code"]

[plugins]
# Map file extensions to external commands
# The command receives the file path as the last argument
docx = ["pandoc", "-t", "plain"]
```

### 2. Ignoring Files

Create a `.contextignore` file in your watched directory to exclude specific files or patterns. It uses standard `.gitignore` syntax.

```gitignore
# .contextignore
node_modules/
target/
*.tmp
secret_*.txt
```

### 3. Running the Daemon (Background Indexer)

```bash
./target/release/contextd
```

You should see logs indicating the server has started and is watching your directories.

### 4. 🤖 MCP Server Integration (Claude / Continue)

contextd can run as an MCP server, allowing AI assistants to directly query your codebase.

**Run in MCP Mode:**
```bash
./target/release/contextd --mcp --config /path/to/contextd.toml
```

**Integration Guides:**
-   [Claude Desktop Integration](docs/claude-integration.md)
-   [Continue.dev Integration](docs/continue-integration.md)

**Quick Claude Config (`claude_desktop_config.json`):**
```json
{
  "mcpServers": {
    "contextd": {
      "command": "/absolute/path/to/contextd",
      "args": ["--mcp", "--config", "/absolute/path/to/contextd.toml"]
    }
  }
}
```

### 5. API Usage (REST)

The daemon exposes a REST API for querying context.

**Endpoint**: `POST /query`

**Request**:
```json
{
  "query": "How does the authentication system work?",
  "limit": 5,
  "start_time": 1700000000,
  "end_time": 1720000000
}
```

**Response**:
```json
{
  "results": [
    {
      "content": "The auth system uses JWT tokens...",
      "score": 0.89
    }
  ]
}
```

**Example with curl**:
```bash
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"query": "database schema", "limit": 3}'
```

---

## 🧩 Plugin System

`contextd` is designed to be extensible. You can add support for **any file format** by defining plugins in `contextd.toml`. A plugin is simply an external command that takes a file path as its last argument and outputs text to `stdout`.

### How Plugins Work

```
File → Plugin Command → Text Output → Chunker → Embedder → Database
```

The plugin command receives the file path as the **last argument** and must output extracted text to stdout.

---

## 📚 Recommended Plugins

Below is a comprehensive list of recommended plugins for indexing any codebase or document format.

### Programming Languages

Use `cat` for simple text extraction. For semantic chunking, prefer native Tree-sitter support (currently Rust only).

```toml
[plugins]
# Python
py = ["cat"]

# JavaScript / TypeScript
js = ["cat"]
ts = ["cat"]
jsx = ["cat"]
tsx = ["cat"]

# Go
go = ["cat"]

# Java
java = ["cat"]

# C / C++
c = ["cat"]
cpp = ["cat"]
cc = ["cat"]
h = ["cat"]
hpp = ["cat"]

# C#
cs = ["cat"]

# Ruby
rb = ["cat"]

# PHP
php = ["cat"]

# Swift
swift = ["cat"]

# Kotlin
kt = ["cat"]
kts = ["cat"]

# Scala
scala = ["cat"]

# Lua
lua = ["cat"]

# Shell Scripts
sh = ["cat"]
bash = ["cat"]
zsh = ["cat"]

# SQL
sql = ["cat"]

# R
r = ["cat"]

# Julia
jl = ["cat"]

# Haskell
hs = ["cat"]

# Elixir
ex = ["cat"]
exs = ["cat"]

# Clojure
clj = ["cat"]
cljs = ["cat"]

# Zig
zig = ["cat"]

# Nim
nim = ["cat"]

# V
v = ["cat"]
```

---

### Document Formats

#### PDF Files

For **complex PDFs** with LaTeX equations, tables, and diagrams:

| Tool | Quality | Install | Plugin Config |
|------|---------|---------|---------------|
| **Marker** 🏆 | Excellent (preserves LaTeX) | `pip install marker-pdf` | `pdf = ["marker_single", "--output_format", "markdown"]` |
| **pdftotext** | Good (text only) | `apt install poppler-utils` | `pdf = ["pdftotext", "-layout", "-"]` |
| **Docling** | Good (structured) | `pip install docling` | `pdf = ["docling", "--to", "md"]` |

**Using pdftotext** (included in `poppler-utils`):

Since contextd appends the file path as the last argument, pdftotext needs a wrapper script:

```bash
# scripts/pdftotext.sh (already included in this repo)
#!/bin/bash
pdftotext -layout "$1" -
```

```toml
[plugins]
# pdftotext via wrapper script (text extraction, good for most documents)
pdf = ["./scripts/pdftotext.sh"]
```

**Using Marker** (for research papers with LaTeX equations):

```bash
pip install marker-pdf
```

```toml
[plugins]
# Marker (best quality, preserves LaTeX equations)
pdf = ["marker_single", "--output_format", "markdown"]
```

> **🏆 Shoutout to [Marker](https://github.com/VikParuchuri/marker)**: An excellent open-source tool for converting PDFs to Markdown with LaTeX equation preservation. Perfect for indexing research papers!

---

#### Office Documents

Requires [Pandoc](https://pandoc.org/) - the universal document converter.

```bash
# Install Pandoc
sudo apt install pandoc        # Debian/Ubuntu
brew install pandoc            # macOS
```

```toml
[plugins]
# Microsoft Word
docx = ["pandoc", "-t", "plain"]
doc = ["catdoc"]  # For legacy .doc files (apt install catdoc)

# OpenDocument
odt = ["pandoc", "-t", "plain"]

# Rich Text Format
rtf = ["pandoc", "-t", "plain"]

# EPUB (eBooks)
epub = ["pandoc", "-t", "plain"]

# HTML
html = ["pandoc", "-t", "plain"]
htm = ["pandoc", "-t", "plain"]
```

> **🏆 Shoutout to [Pandoc](https://pandoc.org/)**: The Swiss Army knife for document conversion. Supports 40+ formats!

---

#### LaTeX Source Files

```toml
[plugins]
# LaTeX source (index as-is for semantic search on equations)
tex = ["cat"]
```

---

### Data & Config Formats

```toml
[plugins]
# JSON
json = ["cat"]

# YAML
yaml = ["cat"]
yml = ["cat"]

# TOML
toml = ["cat"]

# XML
xml = ["cat"]

# CSV (consider jq for structured extraction)
csv = ["cat"]

# Environment files
env = ["cat"]

# INI configs
ini = ["cat"]

# Protobuf definitions
proto = ["cat"]

# GraphQL schemas
graphql = ["cat"]
gql = ["cat"]
```

---

### Jupyter Notebooks

```bash
# Install nbconvert
pip install jupyter nbconvert
```

```toml
[plugins]
# Jupyter Notebooks (extract to markdown)
ipynb = ["jupyter", "nbconvert", "--to", "markdown", "--stdout"]
```

> **🏆 Shoutout to [Jupyter](https://jupyter.org/)**: The de facto standard for interactive computing.

---

### Complete Example Configuration

Here's a comprehensive `contextd.toml` for indexing a full-stack polyglot codebase:

```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "contextd.db"
model_path = "models"

[watch]
paths = [
    "/home/user/projects/myapp",
    "/home/user/notes"
]

[plugins]
# === Programming Languages ===
py = ["cat"]
js = ["cat"]
ts = ["cat"]
jsx = ["cat"]
tsx = ["cat"]
go = ["cat"]
java = ["cat"]
c = ["cat"]
cpp = ["cat"]
h = ["cat"]
cs = ["cat"]
rb = ["cat"]
php = ["cat"]
swift = ["cat"]
kt = ["cat"]
scala = ["cat"]
lua = ["cat"]
sh = ["cat"]
sql = ["cat"]

# === Documents ===
pdf = ["pdftotext", "-layout"]
docx = ["pandoc", "-t", "plain"]
odt = ["pandoc", "-t", "plain"]
epub = ["pandoc", "-t", "plain"]
html = ["pandoc", "-t", "plain"]
tex = ["cat"]

# === Data Formats ===
json = ["cat"]
yaml = ["cat"]
yml = ["cat"]
xml = ["cat"]
csv = ["cat"]
proto = ["cat"]
graphql = ["cat"]

# === Notebooks ===
# ipynb = ["jupyter", "nbconvert", "--to", "markdown", "--stdout"]
```

---

## 🏗️ Architecture

1.  **Watcher**: Uses `notify-debouncer-mini` to listen for file system events. It batches rapid changes to avoid CPU spikes.
2.  **Filter**: Checks `.contextignore` and `.gitignore` to skip irrelevant files.
3.  **Parser**: Routes files to the appropriate parser (native or plugin) based on extension.
4.  **Chunker**: Splits text into semantic chunks (paragraph-based for plugins, AST-based for native formats).
5.  **Embedder**: Generates vector embeddings using the local ONNX model.
6.  **Storage**: Stores chunks and embeddings in SQLite.
7.  **API**: Serves semantic search queries via `axum`.

---

## 🙏 Acknowledgments

This project wouldn't be possible without these amazing open-source tools:

| Tool | Purpose | Link |
|------|---------|------|
| **Marker** | PDF → Markdown with LaTeX | [github.com/VikParuchuri/marker](https://github.com/VikParuchuri/marker) |
| **Pandoc** | Universal document converter | [pandoc.org](https://pandoc.org/) |
| **Tree-sitter** | Semantic code parsing | [tree-sitter.github.io](https://tree-sitter.github.io/) |
| **all-MiniLM-L6-v2** | Sentence embeddings | [Hugging Face](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) |
| **ONNX Runtime** | Local ML inference | [onnxruntime.ai](https://onnxruntime.ai/) |
| **poppler-utils** | PDF text extraction | [poppler.freedesktop.org](https://poppler.freedesktop.org/) |

---

## 🤝 Contributing

I welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

1.  Fork the repo.
2.  Create a feature branch.
3.  Commit your changes.
4.  Push to the branch.
5.  Create a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
