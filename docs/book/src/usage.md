# Usage

`contextd` provides six subcommands: `daemon`, `mcp`, `setup`, `query`, `connect`, and `completions`.

- **`daemon`** (default) — Runs as a background service, watches configured paths, maintains the SQLite WAL index, and serves the hardened HTTP REST API.
- **`mcp`** — Runs as an async JSON-RPC 2.0 / MCP stdio server for AI tool integrations.
- **`setup`** — Downloads and SHA-256 verifies the configured ONNX embedding model.
- **`query`** — Executes one-off hybrid semantic + FTS5 searches directly from the terminal.
- **`connect`** — Auto-detects and configures MCP settings for 9 AI tools with `.bak` backups.
- **`completions`** — Generates `bash`, `zsh`, or `fish` shell completion scripts.

## Global Flags

These flags work before or after any subcommand:

- `-c, --config <PATH>` — Explicit TOML configuration file path.
- `--trust-config` — Explicitly trust a `--config` file located outside the user config directory (allows `[server]`, `[storage]`, and `[plugins]`).
- `--trust-workspace-plugins` — Explicitly trust and execute `[plugins]` from a workspace-local `./contextd.toml`.
