# contextd

[![CI](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml/badge.svg)](https://github.com/sandy-sachin7/contextd/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**contextd** is a local-first, semantic context daemon designed to empower AI agents with deep understanding of your codebase and documents. It runs silently in the background, indexing your files into vector embeddings, and provides a high-performance API for semantic retrieval.

Unlike cloud-based solutions, `contextd` keeps all your data on your machine, ensuring privacy and zero latency.

## 🚀 Features

-   **🔒 Local-First & Private**: Your data never leaves your machine. Embeddings are generated locally using ONNX Runtime.
-   **🧠 Semantic Search**: Powered by `all-MiniLM-L6-v2`, enabling your AI to find relevant context by meaning, not just keywords.
-   **⚡ Real-Time Indexing**: Watches your file system for changes and updates the index instantly (with adaptive debouncing).
-   **📂 Multi-Format Support**: Native support for `.txt`, `.md`, and `.pdf`.
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
rs = ["cat"] # Example: Treat Rust files as plain text
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

### 3. Running the Daemon

```bash
./target/release/contextd
```

You should see logs indicating the server has started and is watching your directories.

### 4. API Usage

The daemon exposes a REST API for querying context.

**Endpoint**: `POST /query`

**Request**:
```json
{
  "query": "How does the authentication system work?",
  "limit": 5,
  "start_time": 1700000000, // Optional: Filter by modification time
  "end_time": 1720000000    // Optional
}
```

**Response**:
```json
[
  {
    "content": "The auth system uses JWT tokens...",
    "score": 0.89,
    "metadata": {
      "path": "/src/auth.rs",
      "last_modified": 1710000000
    }
  },
  ...
]
```

**Example with curl**:
```bash
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"query": "database schema", "limit": 3}'
```

## 🧩 Plugin System

`contextd` is designed to be extensible. You can add support for any file format by defining a plugin in `contextd.toml`.

A plugin is simply an external command that takes a file path as its last argument and outputs text to `stdout`.

**Example: Indexing Python files using `cat`**
```toml
[plugins]
py = ["cat"]
```

**Example: Indexing DOCX using `pandoc`**
```toml
[plugins]
docx = ["pandoc", "-t", "plain"]
```

## 🏗️ Architecture

1.  **Watcher**: Uses `notify-debouncer-mini` to listen for file system events. It batches rapid changes to avoid CPU spikes.
2.  **Filter**: Checks `.contextignore` and `.gitignore` to skip irrelevant files.
3.  **Parser**: Routes files to the appropriate parser (native or plugin) based on extension.
4.  **Chunker**: Splits text into semantic chunks (currently paragraph-based).
5.  **Embedder**: Generates vector embeddings using the local ONNX model.
6.  **Storage**: Stores chunks and embeddings in SQLite (with `vector0` extension or blob storage).
7.  **API**: Serves semantic search queries via `axum`.

## 🤝 Contributing

I welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

1.  Fork the repo.
2.  Create a feature branch.
3.  Commit your changes.
4.  Push to the branch.
5.  Create a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
