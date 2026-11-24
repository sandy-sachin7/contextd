//! Offline runnable example demonstrating `contextd` as an embedded Rust library:
//! Tree-sitter chunking + in-memory SQLite (`sqlite-vec` KNN + FTS5) hybrid search.
//!
//! Run with:
//! ```bash
//! cargo run --example embedded_search
//! ```

use anyhow::Result;
use contextd::indexer::chunker;
use contextd::storage::db::{ChunkInput, Database, SearchOptions};

/// Tiny deterministic 16-dimensional bag-of-trigrams embedding for offline demos.
/// Production deployments use `contextd::indexer::embeddings::Embedder` (ONNX Runtime).
fn demo_embed(text: &str) -> [f32; 16] {
    let mut vec = [0.0_f32; 16];
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() < 3 {
        vec[0] = 1.0;
        return vec;
    }
    for window in bytes.windows(3) {
        let mut h: u32 = 2166136261;
        for &b in window {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        vec[(h as usize) % 16] += 1.0;
    }
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}

fn main() -> Result<()> {
    // 1. Open an in-memory schema v2 database with 16-dimensional demo vectors
    let db = Database::open(":memory:", 16, "demo-trigram-16d")?;

    // 2. Sample source files to chunk with Tree-sitter and Markdown chunkers
    let files = [
        (
            "src/auth.rs",
            "rs",
            r#"
/// Validate bearer tokens using a constant-time comparison.
pub fn verify_bearer_token(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;
    }
    expected
        .bytes()
        .zip(provided.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
"#,
        ),
        (
            "src/search.rs",
            "rs",
            r#"
/// Combine sqlite-vec KNN ranks with FTS5 keyword ranks via normalized RRF.
pub fn reciprocal_rank_fusion(vec_rank: usize, fts_rank: usize) -> f32 {
    let k = 60.0_f32;
    (1.0 / (k + vec_rank as f32)) + (1.0 / (k + fts_rank as f32))
}
"#,
        ),
        (
            "docs/security.md",
            "md",
            r#"
# Security Model

Non-loopback HTTP binds require an authentication token (`CONTEXTD_AUTH_TOKEN`).
Workspace-local configs cannot override server bind addresses or storage paths.
"#,
        ),
    ];

    // 3. Chunk and transactionally index every file
    let now_secs = 1_760_000_000_u64;
    for (path, ext, content) in files {
        let chunks = chunker::chunk_by_type(content, ext)?;
        let embeddings: Vec<[f32; 16]> = chunks.iter().map(|c| demo_embed(&c.content)).collect();
        let inputs: Vec<ChunkInput<'_>> = chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(c, emb)| ChunkInput {
                start: c.start,
                end: c.end,
                content: &c.content,
                embedding: Some(emb.as_slice()),
                metadata: c.metadata.as_deref(),
            })
            .collect();

        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        db.index_file_chunks(path, now_secs, 0, content.len() as u64, &hash, &inputs)?;
    }

    let stats = db.get_stats()?;
    println!(
        "Indexed {} files ({} chunks) into in-memory SQLite WAL database.",
        stats.file_count, stats.chunk_count
    );

    // 4. Run a hybrid semantic + FTS5 query
    let query = "constant-time bearer token validation";
    let query_vec = demo_embed(query);
    let options = SearchOptions {
        limit: Some(3),
        file_types: Some(vec!["rs".to_string(), "md".to_string()]),
        ..Default::default()
    };

    let results = db.search_chunks_hybrid(query, &query_vec, &options)?;
    println!("\nTop {} hybrid results for '{}':", results.len(), query);
    for (idx, hit) in results.iter().enumerate() {
        let preview = hit
            .content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "  {}. {} (score: {:.4}) -> {}",
            idx + 1,
            hit.file_path,
            hit.score,
            preview
        );
    }

    assert!(
        !results.is_empty(),
        "Expected at least one hybrid search match"
    );
    Ok(())
}
