# contextd

A local-first, semantic context daemon for AI agents. `contextd` indexes your local files (text, markdown, PDF) and provides a semantic search API for your AI tools to retrieve relevant context.

## Features

- **Local-First**: All data stays on your machine.
- **Semantic Search**: Uses `all-MiniLM-L6-v2` (via ONNX Runtime) for vector embeddings.
- **File Watching**: Automatically re-indexes files when they change.
- **Time-Based Filtering**: Filter context by modification time.
- **Formats**: Supports `.txt`, `.md`, and `.pdf`.
- **Extensible**: Configurable via `contextd.toml` and supports external parser plugins.

## Configuration

Create a `contextd.toml` file in the working directory to configure the daemon:

```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "contextd.db"
model_path = "models"

[watch]
paths = ["."]

[plugins]
# Map file extensions to external commands
# The command receives the file path as the last argument
docx = ["pandoc", "-t", "plain"]
```

## Getting Started

### Prerequisites

- Rust (latest stable)
- `curl` (for testing)

### Build & Run

1. Clone the repository:
   ```bash
   git clone https://github.com/sandy-sachin7/contextd.git
   cd contextd
   ```

2. Download the model (if not present, the daemon will fail):
   ```bash
   ./setup_model.sh
   ```

3. Run the daemon:
   ```bash
   cargo run
   ```

### Usage

Query the API:

```bash
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"query": "your search query", "limit": 5}'
```

With time filters:

```bash
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"query": "recent changes", "start_time": 1700000000}'
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

MIT
