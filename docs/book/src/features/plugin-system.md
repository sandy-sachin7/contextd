# Plugin System

Extend indexing to additional file formats via external converter commands. Native Tree-sitter chunkers (Rust, Python, JS/JSX, TS/TSX, Go), Markdown splitting, plain text, and built-in PDF extraction (`pdf-extract`) work out of the box — plugins are only needed for extra formats like `.docx`, `.odt`, `.epub`, or `.ipynb`.

Plugins are invoked with `-- <canonical_file_path>` appended as the final arguments and must write UTF-8 text to `stdout`.

## Configuration

Configure `[plugins]` in your trusted user-global config (`~/.config/contextd/contextd.toml` on Linux, `~/Library/Application Support/contextd/contextd.toml` on macOS):

```toml
[plugins]
# External PDF override (wrapper strips the leading `--` guard before calling pdftotext)
pdf = ["./scripts/pdftotext.sh"]

# Office documents (requires pandoc)
docx = ["pandoc", "-t", "plain"]
odt = ["pandoc", "-t", "plain"]
rtf = ["pandoc", "-t", "plain"]
epub = ["pandoc", "-t", "plain"]

# Jupyter notebooks (requires jupyter nbconvert)
ipynb = ["jupyter", "nbconvert", "--to", "markdown", "--stdout"]
```

## Security & Trust Model

- **Workspace Isolation**: `[plugins]` defined in a workspace-local `./contextd.toml` (or an untrusted `--config` file) are **disabled by default** to prevent arbitrary command execution when indexing untrusted repositories. Enable explicitly via `--trust-workspace-plugins`, `--trust-config`, or `security.allow_workspace_plugins = true` in your user-global config.
- **Flag-Injection Guard**: `contextd` always inserts `--` before the canonicalized file path so filenames starting with `-` cannot be interpreted as flags. Custom wrapper scripts should strip an optional leading `--` (see `scripts/pdftotext.sh`).
- **Execution Limits**: `stdin` is bound to `/dev/null`, child processes use `kill_on_drop(true)`, execution times out after **30 seconds**, `stdout` is capped at **10 MiB**, and output must be valid UTF-8.
