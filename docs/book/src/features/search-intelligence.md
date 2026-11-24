# Search Intelligence

`contextd` layers three ranking and presentation signals on top of `sqlite-vec` KNN + `FTS5` hybrid retrieval:

## Recency Boost

Recently modified files rank higher during vector candidate scoring. The boost decays smoothly by 50% every 24 hours:

```text
recency_boost = 1.0 / (1.0 + age_hours / 24.0)
```

Default `recency_weight` in `SearchOptions` is `0.1`.

## Frequency Ranking

Files that are repeatedly matched gain a logarithmic priority boost using the `query_hits` table:

```text
frequency_boost = ln(1 + hit_count) * frequency_weight
```

Default `frequency_weight` in `SearchOptions` is `0.1`.

## Smart Context Windows

When `context_lines` (`1`-`20`) is requested via CLI (`--context`), HTTP API, or MCP (`search_context`), `contextd` reads surrounding lines and computes 1-indexed `line_start` / `line_end` bounds from disk **only for the final top-N results** after ranking and truncation.
