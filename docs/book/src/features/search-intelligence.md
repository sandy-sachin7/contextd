# Search Intelligence

contextd includes three advanced ranking signals on top of semantic search:

## Recency Boost

Recently modified files rank higher. The boost decays by 50% every 24 hours.
Configurable via `search.recency_weight` (default: 0.1).

## Frequency Ranking

Frequently queried files get priority. Tracks hit counts per file with
logarithmic scaling. Configurable via `search.frequency_weight` (default: 0.1).

## Query Caching

Repeated queries (same embedding) use a 100-entry LRU cache for instant results.
Only applies to unfiltered queries.
