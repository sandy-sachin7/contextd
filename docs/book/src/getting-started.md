# Getting Started

## Install

### One-line installer (Linux/macOS, SHA-256 verified)

```bash
curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.sh | bash
```

### Cargo (Crates.io, Rust 1.95+)

```bash
cargo install contextd --locked
```

### Windows (PowerShell, SHA-256 verified)

```powershell
irm https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.ps1 | iex
```

### Homebrew (macOS/Linux)

```bash
brew install --HEAD --build-from-source ./homebrew/contextd.rb
```

### Docker

The container binds `0.0.0.0:3030` inside the container, so `CONTEXTD_AUTH_TOKEN` is required:

```bash
docker run --rm -p 127.0.0.1:3030:3030 \
  -e CONTEXTD_AUTH_TOKEN=replace-with-strong-token \
  -v "$PWD:/workspace:ro" \
  ghcr.io/sandy-sachin7/contextd
```

### From source

```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
cargo build --release --locked
./target/release/contextd setup
```

## Run as Daemon

```bash
# Download and verify the default embedding model
contextd setup

# Start the daemon (watches your configured directories and serves HTTP on 127.0.0.1:3030)
contextd daemon

# Or run one-off hybrid queries from the CLI
contextd query "authentication system" --limit 10 --context 2
```

## Connect your AI Tool

Auto-configure all detected AI tools (Claude Desktop, Claude Code, Cursor, VS Code Copilot, Copilot CLI, OpenCode, Continue, Antigravity, Codex):

```bash
contextd connect --all
```

See the [Integrations](integrations.md) section for manual setup guides.
