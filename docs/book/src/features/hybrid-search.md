# Hybrid Search

`contextd` combines `sqlite-vec` KNN vector similarity with full-text keyword search (`SQLite FTS5`) across CLI, HTTP API, and MCP:

- **Semantic (`sqlite-vec`)**: Cosine similarity via local ONNX embeddings (`WHERE embedding MATCH ?1 AND k = ?2`), with progressive `k` expansion when file, path, or time filters are active.
- **Keyword (`FTS5`)**: Sanitized, tokenized `OR` queries using the Porter stemming tokenizer (`porter unicode61`), with SQL filters applied before `LIMIT`.
- **Normalized Fusion**: Reciprocal Rank Fusion (`k = 60`) normalized to `[0.0, 1.0]` and blended with semantic cosine similarity.

## Search Ranking Pipeline

1. **Vector KNN** fetches top candidates (`clamp(limit * 5, 50, 200)`) via `sqlite-vec` and applies recency decay (default weight `0.1`) and logarithmic frequency boost (default weight `0.1`).
2. **FTS5 Keyword Search** fetches top keyword matches with SQL filters applied prior to `LIMIT`.
3. **Normalized RRF** combines both rank lists into `[0.0, 1.0]` and blends rank consensus with vector similarity.
4. **Filtering & Truncation** applies `min_score`, sorts deterministically (mapping `NaN` to `0.0`), truncates to `limit`, and resolves optional `context_lines` only for the final results.
