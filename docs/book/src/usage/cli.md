# CLI

## Daemon Mode (Background Service)

```bash
contextd daemon
contextd daemon --config /path/to/contextd.toml --trust-config
# Optional: exit when stdin closes (useful when spawned by an editor extension)
contextd daemon --exit-on-stdin-eof
```

## Query Mode (One-off Hybrid Queries)

```bash
contextd query "authentication"
contextd query "database schema" --limit 10 --min-score 0.5 --file-types rs,sql --context 2
contextd query "API changes" --paths src/,docs/ --after 2025-01-01 --before 2026-12-31
```

## Setup Mode

Downloads and SHA-256 verifies the configured embedding model from pinned HuggingFace revisions:

```bash
contextd setup
```

Supports `all-minilm-l6-v2` (default, 384d), `all-mpnet-base-v2` (768d), and `bge-small-en-v1.5` (384d).

## Connect Mode (AI Tool Auto-Configuration)

```bash
contextd connect --all
contextd connect --all --scope project
contextd connect --tool claude-desktop
```

## Shell Completions

```bash
contextd completions bash > ~/.local/share/bash-completion/completions/contextd
contextd completions zsh  > ~/.zfunc/_contextd
contextd completions fish > ~/.config/fish/completions/contextd.fish
```
