# Configuration

contextd uses a TOML configuration file. By default it looks for `contextd.toml` in the
current directory, or you can specify one with `--config`.

## Example

```toml
[server]
host = "127.0.0.1"
port = 3030

[storage]
db_path = "contextd.db"
model_path = "models"
model_type = "all-minilm-l6-v2"

[watch]
paths = ["."]
debounce_ms = 2000

[search]
enable_cache = true

[plugins]
pdf = ["./scripts/pdftotext.sh"]
docx = ["pandoc", "-t", "plain"]
odt = ["pandoc", "-t", "plain"]
rtf = ["pandoc", "-t", "plain"]
epub = ["pandoc", "-t", "plain"]
html = ["pandoc", "-t", "plain"]
tex = ["pandoc", "-t", "plain"]
```

## Ignoring Files

contextd respects `.gitignore` by default. You can also create a `.contextignore` file
to exclude specific files from indexing without affecting git:

```gitignore
*.log
temp/
secret_keys.json
```
