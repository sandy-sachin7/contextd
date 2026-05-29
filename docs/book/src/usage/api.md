# REST API

The REST API is served by the daemon on the configured host:port (default `127.0.0.1:3030`).

## Health

```bash
curl http://localhost:3030/health
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
curl http://localhost:3030/status
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
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{
    "query": "How does auth work?",
    "limit": 5,
    "min_score": 0.5,
    "file_types": [".rs", ".py"]
  }'
```

Parameters:

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Search query |
| `limit` | number | No | Max results (default: 10) |
| `min_score` | number | No | Minimum relevance score (0.0-1.0) |
| `file_types` | string[] | No | Filter by extensions |
| `paths` | string[] | No | Filter by file path patterns |
| `start_time` | number | No | Filter by earliest modification time (unix ts) |
| `end_time` | number | No | Filter by latest modification time (unix ts) |
