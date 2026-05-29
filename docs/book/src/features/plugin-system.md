# Plugin System

Extend support to any file format via external command plugins. Plugins receive the
file path as the last argument and should output parsed text to stdout.

## Configuration

```toml
[plugins]
pdf = ["./scripts/pdftotext.sh"]
docx = ["pandoc", "-t", "plain"]
py = ["cat"]
```

## Built-in Support

40+ file extension mappings are provided in the example config, including:

- Documents: pdf, docx, odt, rtf, epub, html, tex
- Data: json, yaml, xml, csv, env, ini, proto, graphql
- Scripts: sh, bash, sql, lua
- Notebooks: ipynb (via jupyter nbconvert)

## Security

- 30-second timeout per plugin execution
- UTF-8 output validation
- Output size limits
