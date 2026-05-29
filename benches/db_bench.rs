use criterion::{criterion_group, criterion_main, Criterion};

use contextd::storage::db::{Database, SearchOptions};
use tempfile::TempDir;

fn bench_db_insert(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let db = Database::new(dir.path().join("test.db")).unwrap();

    let file_id = db.add_or_update_file("/test/file.rs", 1000).unwrap();

    let embedding: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();

    c.bench_function("db_insert_chunk_384d", |b| {
        b.iter(|| {
            db.add_chunk(
                file_id,
                0,
                100,
                "fn test() { println!(\"hello\"); }",
                Some(&embedding),
                Some(r#"{"type":"function"}"#),
            )
            .unwrap();
            db.clear_chunks(file_id).unwrap();
        })
    });
}

fn bench_db_search_fts(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let db = Database::new(dir.path().join("test.db")).unwrap();

    let query_embedding: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();

    for i in 0..100 {
        let file_id = db
            .add_or_update_file(&format!("/test/file_{}.rs", i), 1000 + i)
            .unwrap();
        let emb: Vec<f32> = (0..384).map(|j| ((j + i * 10) as f32) / 384.0).collect();
        db.add_chunk(
            file_id,
            0,
            100,
            &format!("fn function_{}() {{ return {}; }}", i, i),
            Some(&emb),
            None,
        )
        .unwrap();
    }

    let options = SearchOptions {
        limit: Some(10),
        start_time: None,
        end_time: None,
        file_types: None,
        paths: None,
        min_score: None,
        recency_weight: None,
        frequency_weight: None,
        context_lines: None,
    };

    c.bench_function("db_search_hybrid_100_chunks", |b| {
        b.iter(|| db.search_chunks_hybrid("function", &query_embedding, &options))
    });
}

fn bench_db_search_enhanced(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let db = Database::new(dir.path().join("test.db")).unwrap();

    let query_embedding: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();

    for i in 0..200 {
        let file_id = db
            .add_or_update_file(&format!("/test/file_{}.rs", i), 1000 + i)
            .unwrap();
        let emb: Vec<f32> = (0..384).map(|j| ((j + i * 10) as f32) / 384.0).collect();
        db.add_chunk(
            file_id,
            0,
            100,
            &format!("fn function_{}() {{ return {}; }}", i, i),
            Some(&emb),
            None,
        )
        .unwrap();
    }

    let options = SearchOptions {
        limit: Some(10),
        start_time: None,
        end_time: None,
        file_types: None,
        paths: None,
        min_score: None,
        recency_weight: None,
        frequency_weight: None,
        context_lines: None,
    };

    c.bench_function("db_search_enhanced_200_chunks_384d", |b| {
        b.iter(|| db.search_chunks_enhanced(&query_embedding, &options))
    });
}

criterion_group!(
    benches,
    bench_db_insert,
    bench_db_search_fts,
    bench_db_search_enhanced
);
criterion_main!(benches);
