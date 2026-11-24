# REST API

The REST API is served by the daemon on the configured `host:port` (default `127.0.0.1:3030`).

## Security & Limits

- **Loopback DNS-Rebinding Protection**: In unauthenticated loopback mode, requests must include a loopback `Host` header (`localhost`, `127.0.0.1`, or `[::1]`).
- **Bearer Authentication**: Binding a non-loopback host (e.g., `0.0.0.0`) requires `server.auth_token` or `CONTEXTD_AUTH_TOKEN`. When configured, pass `Authorization: Bearer <token>` on `/status` and `/query` (loopback `/health` remains open for healthchecks).
- **Resource Caps**: 64 KiB max request body, 16 KiB max query string, 30-second request timeout, and 64 max concurrent requests.

## Health

```bash
curl http://127.0.0.1:3030/health
```

Response:

```json
{
  "status": "ok",
  "uptime_secs": 3600
}
```

## Status

```bash
curl http://127.0.0.1:3030/status
```

Response:

```json
{
  "status": "ok",
  "uptime_secs": 3600,
  "indexed_files": 1500,
  "total_chunks": 45000,
  "database_size_bytes": 52428800
}
```

## Query

```bash
curl -X POST http://127.0.0.1:3030/query \
  -H "Content-Type: application/json" \
  -d '{
    "query": "How does auth work?",
    "limit": 5,
    "min_score": 0.5,
    "file_types": ["rs", "py"],
    "context_lines": 2
  }'
```

Parameters:

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Search query (max 16 KiB) |
| `limit` | number | No | Max results (default: `5`, max: `100`) |
| `max_results` | number | No | Additional response cap (`min(limit, max_results)`, max: `100`) |
| `min_score` | number | No | Minimum relevance score (`0.0`-`1.0`, finite) |
| `file_types` | string[] | No | Filter by file extensions (e.g., `["rs", "py"]`) |
| `paths` | string[] | No | Filter by file path substrings |
| `start_time` | number | No | Filter by earliest modification time (UNIX seconds) |
| `end_time` | number | No | Filter by latest modification time (UNIX seconds) |
| `context_lines` | number | No | Surrounding source lines to include (`0`-`20`) |
