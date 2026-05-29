# CLI

## Daemon Mode (Background Service)

```bash
contextd daemon
contextd daemon --config /path/to/contextd.toml
```

## CLI Mode (One-off Queries)

```bash
contextd query "authentication"
contextd query "database schema" --limit 10 --min-score 0.7
contextd query "API changes" --after 2024-12-01
```

## Setup Mode

Downloads the embedding model from HuggingFace:

```bash
contextd setup
```

Supports three models: `all-minilm-l6-v2` (default), `all-mpnet-base-v2`, `bge-small-en-v1.5`.
