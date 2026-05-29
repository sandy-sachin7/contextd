# Hybrid Search

contextd combines vector embeddings with full-text search (SQLite FTS5) for superior results:

- **Semantic**: Understands meaning and context via ONNX embeddings
- **Keyword**: Fast exact matches via FTS5 with porter tokenizer
- **Weighted**: Reciprocal Rank Fusion (RRF) automatically balances both approaches

### Search Ranking

The hybrid search uses **Reciprocal Rank Fusion (RRF)** with k=60:

1. Vector search (cosine similarity) - fetches top 50
2. FTS5 keyword search - fetches top 50
3. RRF combines both ranks
4. Final score = semantic score * (1 - recency_weight) + recency_boost * recency_weight + ln(1+hit_count) * frequency_weight

Default weights: recency=0.1, frequency=0.1
