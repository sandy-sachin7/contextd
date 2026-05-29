# Getting Started

## Install

### One-line installer (Linux/macOS)

```bash
curl -sSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.sh | sh
```

### Homebrew (macOS/Linux)

```bash
brew install sandy-sachin7/tap/contextd
```

### Docker

```bash
docker run -v $PWD:/workspace ghcr.io/sandy-sachin7/contextd
```

### From source

```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
cargo run -- setup
cargo build --release
```

## Run as Daemon

```bash
# Start the daemon (watches your configured directories)
./target/release/contextd daemon

# Or use the CLI for one-off queries
./target/release/contextd query "authentication system"
```

## Connect your AI Tool

contextd works with Claude Desktop, Cline, Roo Code, and more.
See the [Integrations](integrations.md) section for setup guides.
