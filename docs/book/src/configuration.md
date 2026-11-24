# Configuration

contextd uses a TOML configuration file. Resolution order:

1. Explicit `--config <path>` CLI flag (global across all subcommands)
2. Workspace-local `./contextd.toml`
3. User-global config (`~/.config/contextd/contextd.toml` on Linux, `~/Library/Application Support/contextd/contextd.toml` on macOS)
4. Platform defaults (`~/.local/share/contextd/contextd.db` and `~/.local/share/contextd/models`)

## Workspace config trust

A workspace-local `./contextd.toml` is untrusted. Only `[watch]` and `[chunking]`
apply from it. The `[server]` and `[storage]` sections come from the user-global
config, or from platform defaults when no user-global config exists. `[plugins]`
requires `allow_workspace_plugins = true` in the user-global config or the
`--trust-workspace-plugins` CLI flag.

To change network or storage settings, use the user-global config or an
environment override below. An explicit `--config <path>` outside the user
config directory is also treated as untrusted for those sections unless you
explicitly pass `--trust-config` (which allows `[server]`, `[storage]`, and
`[plugins]` from that file).

## Environment Variable Overrides

| Variable | Description |
|----------|-------------|
| `CONTEXTD_HOST` | HTTP API bind host (default `127.0.0.1`) |
| `CONTEXTD_PORT` | HTTP API bind port (default `3030`) |
| `CONTEXTD_AUTH_TOKEN` | Bearer token (required when binding non-loopback hosts) |
| `CONTEXTD_DB_PATH` | SQLite database path |
| `CONTEXTD_MODEL_PATH` | Root directory for model-specific cache folders |
| `CONTEXTD_MODEL_TYPE` | `all-minilm-l6-v2`, `all-mpnet-base-v2`, or `bge-small-en-v1.5` |
| `CONTEXTD_WATCH_PATHS` | Colon/semicolon/comma-separated list of directories to watch |
| `CONTEXTD_MAX_FILE_SIZE` | Maximum file size in bytes (default `2097152`) |

## Example

```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "~/.local/share/contextd/contextd.db"
model_path = "~/.local/share/contextd/models"
model_type = "all-minilm-l6-v2"

[watch]
paths = ["."]
debounce_ms = 500
max_file_size = 2097152

[chunking]
max_chunk_size = 1500
overlap = 150

[security]
allow_workspace_plugins = false

[plugins]
docx = ["pandoc", "-t", "plain"]
```

## Ignoring Files

contextd automatically excludes secrets (`.env*`, `*.pem`, `*.key`, `id_rsa`), binary files, and heavy build directories (`.git/`, `node_modules/`, `target/`, `.venv/`, `__pycache__/`, `dist/`, `build/`). It also respects `.gitignore` and `.contextignore`.
