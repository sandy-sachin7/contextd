use anyhow::Result;
use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{params, Connection, OptionalExtension};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Once;
use std::sync::{Arc, Mutex};
static INIT_SQLITE_VEC: Once = Once::new();

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    #[allow(clippy::missing_transmute_annotations)]
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        INIT_SQLITE_VEC.call_once(|| unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
        });
        let conn = Connection::open(path)?;

        // Enable foreign keys and WAL mode
        conn.execute("PRAGMA foreign_keys = ON;", [])?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                last_modified INTEGER NOT NULL,
                last_indexed INTEGER
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                start_offset INTEGER NOT NULL,
                end_offset INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                metadata TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_files_path ON files(path)",
            [],
        )?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding float[384]
            )",
            [],
        )?;

        // FTS5 Virtual Table
        // We use the same rowid as the chunks table for easy joining
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(content, tokenize='porter')",
            [],
        )?;

        // Query hits table for frequency ranking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS query_hits (
                file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
                hit_count INTEGER DEFAULT 0,
                last_hit INTEGER
            )",
            [],
        )?;

        Ok(())
    }

    pub fn add_or_update_file(&self, path: &str, last_modified: u64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        // Upsert file
        conn.execute(
            "INSERT INTO files (path, last_modified, last_indexed)
             VALUES (?1, ?2, NULL)
             ON CONFLICT(path) DO UPDATE SET
                last_modified = ?2,
                last_indexed = NULL",
            params![path, last_modified],
        )?;

        let id = conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;

        Ok(id)
    }

    #[allow(dead_code)]
    pub fn get_file_id(&self, path: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let id = conn
            .query_row(
                "SELECT id FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    pub fn mark_indexed(&self, file_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE files SET last_indexed = strftime('%s', 'now') WHERE id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    pub fn needs_reindexing(&self, path: &str, current_modified: u64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let last_indexed: Option<Option<u64>> = conn
            .query_row(
                "SELECT last_indexed FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;

        match last_indexed {
            Some(Some(ts)) => Ok(current_modified > ts),
            Some(None) => Ok(true), // File exists but never indexed
            None => Ok(true),       // File doesn't exist in DB
        }
    }

    pub fn clear_chunks(&self, file_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Delete from vec0 first
        conn.execute(
            "DELETE FROM chunks_vec WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        // Delete from FTS first (using subquery)
        conn.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        conn.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    pub fn add_chunk(
        &self,
        file_id: i64,
        start: u64,
        end: u64,
        content: &str,
        embedding: Option<&[f32]>,
        metadata: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        let embedding_bytes = if let Some(emb) = embedding {
            // Convert &[f32] to bytes (little endian)
            let mut bytes = Vec::with_capacity(emb.len() * 4);
            for val in emb {
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            Some(bytes)
        } else {
            None
        };

        conn.execute(
            "INSERT INTO chunks (file_id, start_offset, end_offset, content, embedding, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![file_id, start, end, content, embedding_bytes, metadata],
        )?;

        let chunk_id = conn.last_insert_rowid();

        // Insert into vec0
        if let Some(emb_bytes) = &embedding_bytes {
            conn.execute(
                "INSERT INTO chunks_vec (chunk_id, embedding) VALUES (?1, ?2)",
                params![chunk_id, emb_bytes.as_slice()],
            )?;
        }

        // Insert into FTS
        conn.execute(
            "INSERT INTO chunks_fts (rowid, content) VALUES (?1, ?2)",
            params![chunk_id, content],
        )?;
        Ok(())
    }

    /// Get database statistics
    pub fn get_stats(&self) -> Result<DbStats> {
        let conn = self.conn.lock().unwrap();

        let file_count: u64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;

        let chunk_count: u64 =
            conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;

        // Get database page count and page size for size estimate
        let page_count: u64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: u64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let db_size = page_count * page_size;

        Ok(DbStats {
            file_count,
            chunk_count,
            db_size,
        })
    }

    /// Record a search hit for a file (for frequency ranking)
    /// Call this after returning search results to boost frequently accessed files
    #[allow(dead_code)]
    pub fn record_search_hit(&self, file_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        conn.execute(
            "INSERT INTO query_hits (file_id, hit_count, last_hit)
             VALUES (?1, 1, ?2)
             ON CONFLICT(file_id) DO UPDATE SET
                hit_count = hit_count + 1,
                last_hit = ?2",
            params![file_id, now],
        )?;
        Ok(())
    }

    /// Hybrid search using RRF (Reciprocal Rank Fusion)
    pub fn search_chunks_hybrid(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let limit = options.limit.unwrap_or(10);
        let k = 60.0; // RRF constant

        // 1. Vector Search
        let vector_options = SearchOptions {
            limit: Some(50), // Fetch more for re-ranking
            start_time: options.start_time,
            end_time: options.end_time,
            file_types: options.file_types.clone(),
            paths: options.paths.clone(),
            min_score: None,
            recency_weight: options.recency_weight,
            frequency_weight: options.frequency_weight,
            context_lines: options.context_lines,
        };
        let vector_results = self.search_chunks_enhanced(query_embedding, &vector_options)?;

        // 2. FTS Search
        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT c.id, c.content, f.path, f.last_modified
                       FROM chunks_fts fts
                       JOIN chunks c ON fts.rowid = c.id
                       JOIN files f ON c.file_id = f.id
                       WHERE fts.content MATCH ?"
            .to_string();

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        // Sanitize query for FTS5
        // Escape double quotes and wrap in quotes to treat as a phrase/literal
        // This prevents syntax errors with special characters like OR, AND, etc.
        let sanitized_query = format!("\"{}\"", query_text.replace('"', "\"\""));
        params.push(Box::new(sanitized_query));

        if let Some(start) = options.start_time {
            sql.push_str(" AND f.last_modified >= ?");
            params.push(Box::new(start));
        }
        if let Some(end) = options.end_time {
            sql.push_str(" AND f.last_modified <= ?");
            params.push(Box::new(end));
        }

        sql.push_str(" ORDER BY fts.rank LIMIT 50");

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let fts_iter = stmt.query_map(params_refs.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let file_path: String = row.get(2)?;
            let last_modified: u64 = row.get(3)?;
            Ok((id, content, file_path, last_modified))
        })?;

        let mut fts_results = Vec::new();
        for res in fts_iter {
            let (id, content, file_path, last_modified) = res?;

            // Extract file extension
            let file_type = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

            // Apply file type filter
            if let Some(types) = &options.file_types {
                if !types.iter().any(|t| t.to_lowercase() == file_type) {
                    continue;
                }
            }

            // Apply path filter
            if let Some(path_filters) = &options.paths {
                if !path_filters.iter().any(|p| file_path.contains(p)) {
                    continue;
                }
            }

            fts_results.push(SearchResult {
                id,
                content,
                score: 0.0, // Placeholder
                file_path,
                file_type,
                last_modified,
                ..Default::default()
            });
        }

        // 3. RRF
        let mut scores: HashMap<i64, f32> = HashMap::new();
        let mut results_map: HashMap<i64, SearchResult> = HashMap::new();

        for (rank, res) in vector_results.iter().enumerate() {
            let score = 1.0 / (k + (rank as f32 + 1.0));
            *scores.entry(res.id).or_insert(0.0) += score;
            results_map.insert(res.id, res.clone());
        }

        for (rank, res) in fts_results.iter().enumerate() {
            let score = 1.0 / (k + (rank as f32 + 1.0));
            *scores.entry(res.id).or_insert(0.0) += score;
            results_map.entry(res.id).or_insert_with(|| res.clone());
        }

        let mut final_results: Vec<SearchResult> = results_map.into_values().collect();
        for res in &mut final_results {
            if let Some(s) = scores.get(&res.id) {
                res.score = *s;
            }
        }

        final_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        final_results.truncate(limit);

        Ok(final_results)
    }

    /// Enhanced search with file type and path filtering
    pub fn search_chunks_enhanced(
        &self,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let limit = options.limit.unwrap_or(10);
        let start_time = options.start_time;
        let end_time = options.end_time;
        let file_types = options.file_types.as_deref();
        let paths = options.paths.as_deref();
        let min_score = options.min_score;

        let conn = self.conn.lock().unwrap();

        let mut query_bytes = Vec::with_capacity(query_embedding.len() * 4);
        for val in query_embedding {
            query_bytes.extend_from_slice(&val.to_le_bytes());
        }

        let mut sql =
            "SELECT c.id, c.content, vec_distance_cosine(v.embedding, ?1) as distance, f.path, f.last_modified, f.id as file_id,
                              COALESCE(qh.hit_count, 0) as hit_count
                       FROM chunks c
                       JOIN chunks_vec v ON c.id = v.chunk_id
                       JOIN files f ON c.file_id = f.id
                       LEFT JOIN query_hits qh ON f.id = qh.file_id
                       WHERE 1=1"
                .to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        params.push(Box::new(query_bytes));

        let mut param_idx = 2;

        if let Some(start) = start_time {
            sql.push_str(&format!(" AND f.last_modified >= ?{}", param_idx));
            param_idx += 1;
            params.push(Box::new(start));
        }

        if let Some(end) = end_time {
            sql.push_str(&format!(" AND f.last_modified <= ?{}", param_idx));
            param_idx += 1;
            params.push(Box::new(end));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let chunk_iter = stmt.query_map(params_refs.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let distance: f32 = row.get(2)?;
            let file_path: String = row.get(3)?;
            let last_modified: u64 = row.get(4)?;
            let file_id: i64 = row.get(5)?;
            let hit_count: i64 = row.get(6)?;
            Ok((
                id,
                content,
                distance,
                file_path,
                last_modified,
                file_id,
                hit_count,
            ))
        })?;

        let mut scored_chunks = Vec::new();

        for chunk in chunk_iter {
            let (id, content, distance, file_path, last_modified, _file_id, hit_count) = chunk?;

            let file_type = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

            if let Some(types) = file_types {
                if !types.iter().any(|t| t.to_lowercase() == file_type) {
                    continue;
                }
            }

            if let Some(path_filters) = paths {
                if !path_filters.iter().any(|p| file_path.contains(p)) {
                    continue;
                }
            }

            // distance is typically 1 - cosine_similarity for vectors, wait no, vec_distance_cosine returns distance.
            // Let's assume score = 1.0 - distance to match previous cosine similarity logic.
            // Wait, vec_distance_cosine in sqlite-vec returns the cosine distance (0.0 means identical, up to 2.0).
            // We want similarity, which is 1.0 - distance.
            let score = 1.0 - distance;

            // Re-apply original similarity range?
            // In original it was: a*b sum, which is dot product. If vectors are normalized, dot product = cosine similarity.
            // distance = 1 - cosine_similarity. So score = 1.0 - distance is exactly dot product.

            if let Some(min) = min_score {
                if score < min {
                    continue;
                }
            }

            let recency_weight = options.recency_weight.unwrap_or(0.1);
            let recency_adjusted = if recency_weight > 0.0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let age_hours = (now.saturating_sub(last_modified)) / 3600;
                let recency_boost = 1.0 / (1.0 + (age_hours as f32 / 24.0));
                score * (1.0 - recency_weight) + recency_boost * recency_weight
            } else {
                score
            };

            let frequency_weight = options.frequency_weight.unwrap_or(0.1);
            let final_score = if frequency_weight > 0.0 && hit_count > 0 {
                let freq_boost = (hit_count as f32).ln_1p() * frequency_weight;
                recency_adjusted + freq_boost
            } else {
                recency_adjusted
            };

            scored_chunks.push(SearchResult {
                id,
                content,
                score: final_score,
                file_path,
                file_type,
                last_modified,
                ..Default::default()
            });
        }

        scored_chunks.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored_chunks.truncate(limit);

        Ok(scored_chunks)
    }
}

/// Database statistics
pub struct DbStats {
    pub file_count: u64,
    pub chunk_count: u64,
    pub db_size: u64,
}

/// Search options for enhanced chunk search
#[derive(Default)]
pub struct SearchOptions {
    pub limit: Option<usize>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub file_types: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub min_score: Option<f32>,
    /// Weight for recency boost (0.0 to 1.0, default 0.1)
    pub recency_weight: Option<f32>,
    /// Weight for frequency boost (0.0 to 1.0, default 0.1)
    pub frequency_weight: Option<f32>,
    /// Number of context lines to include before/after match (default 0)
    pub context_lines: Option<usize>,
}

/// Enhanced search result with metadata
#[derive(Clone, Default)]
pub struct SearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub file_path: String,
    pub file_type: String,
    pub last_modified: u64,
    /// Context lines before the matched content
    #[allow(dead_code)]
    pub context_before: Option<String>,
    /// Context lines after the matched content
    #[allow(dead_code)]
    pub context_after: Option<String>,
    /// Starting line number in the source file
    #[allow(dead_code)]
    pub line_start: Option<usize>,
    /// Ending line number in the source file
    #[allow(dead_code)]
    pub line_end: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_init() {
        let db = Database::new(":memory:").unwrap();
        let conn = db.conn.lock().unwrap();

        // Check tables exist
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"chunks".to_string()));
    }

    #[test]
    fn test_add_get_file() {
        let db = Database::new(":memory:").unwrap();
        let path = "/tmp/test.txt";

        let id = db.add_or_update_file(path, 100).unwrap();
        assert!(id > 0);

        let fetched_id = db.get_file_id(path).unwrap();
        assert_eq!(Some(id), fetched_id);

        let missing = db.get_file_id("/nonexistent").unwrap();
        assert_eq!(None, missing);
    }

    #[test]
    fn test_update_file() {
        let db = Database::new(":memory:").unwrap();
        let path = "/tmp/test.txt";

        let id1 = db.add_or_update_file(path, 100).unwrap();
        let id2 = db.add_or_update_file(path, 200).unwrap();

        assert_eq!(id1, id2); // ID should remain same

        let conn = db.conn.lock().unwrap();
        let last_mod: u64 = conn
            .query_row(
                "SELECT last_modified FROM files WHERE id = ?1",
                params![id1],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(last_mod, 200);
    }

    #[test]
    fn test_chunks() {
        let db = Database::new(":memory:").unwrap();
        let path = "/tmp/test.txt";
        let file_id = db.add_or_update_file(path, 100).unwrap();

        db.add_chunk(file_id, 0, 10, "chunk1", None, None).unwrap();
        db.add_chunk(file_id, 10, 20, "chunk2", None, None).unwrap();

        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count, 2);

        // Test cascade delete (simulated by clearing chunks)
        drop(conn); // unlock
        db.clear_chunks(file_id).unwrap();

        let conn = db.conn.lock().unwrap();
        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count_after, 0);
    }

    #[test]
    fn test_recency_boost() {
        let db = Database::new(":memory:").unwrap();

        // Create two files: one recent, one old
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let one_week_ago = now - (7 * 24 * 3600);

        let recent_id = db.add_or_update_file("/recent.rs", now).unwrap();
        let old_id = db.add_or_update_file("/old.rs", one_week_ago).unwrap();

        // Add chunks with identical embeddings
        let embedding: Vec<f32> = vec![1.0; 384]; // Normalized unit vector
        db.add_chunk(recent_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.add_chunk(old_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(recent_id).unwrap();
        db.mark_indexed(old_id).unwrap();

        // Search with recency boost
        let options = SearchOptions {
            limit: Some(10),
            recency_weight: Some(0.5), // Strong recency weight
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        // Recent file should rank higher
        assert_eq!(results.len(), 2);
        assert!(
            results[0].file_path.contains("recent"),
            "Recent file should rank first"
        );
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_frequency_boost() {
        let db = Database::new(":memory:").unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create two files with same modification time
        let frequent_id = db.add_or_update_file("/frequent.rs", now).unwrap();
        let rare_id = db.add_or_update_file("/rare.rs", now).unwrap();

        // Add chunks with identical embeddings
        let embedding: Vec<f32> = vec![1.0; 384];
        db.add_chunk(frequent_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.add_chunk(rare_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(frequent_id).unwrap();
        db.mark_indexed(rare_id).unwrap();

        // Simulate frequent access
        for _ in 0..10 {
            db.record_search_hit(frequent_id).unwrap();
        }

        // Search with frequency boost
        let options = SearchOptions {
            limit: Some(10),
            frequency_weight: Some(0.5), // Strong frequency weight
            recency_weight: Some(0.0),   // Disable recency boost
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        // Frequent file should rank higher
        assert_eq!(results.len(), 2);
        assert!(
            results[0].file_path.contains("frequent"),
            "Frequently queried file should rank first"
        );
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_context_lines_option() {
        let db = Database::new(":memory:").unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let file_id = db.add_or_update_file("/test.rs", now).unwrap();

        // Add chunk with embedding
        let embedding: Vec<f32> = vec![1.0; 384];
        db.add_chunk(file_id, 0, 10, "fn test() {}", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(file_id).unwrap();

        // Search with context_lines option
        let options = SearchOptions {
            limit: Some(10),
            context_lines: Some(3),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        // Verify result exists and context_lines was passed
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "/test.rs");
        // Context fields exist (populated by caller, not search)
        assert!(results[0].context_before.is_none());
        assert!(results[0].context_after.is_none());
    }

    #[test]
    fn test_fts_sanitization() {
        let db = Database::new(":memory:").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let file_id = db.add_or_update_file("/test.rs", now).unwrap();

        // Add chunk with content containing special FTS operators
        let embedding: Vec<f32> = vec![1.0; 384];
        db.add_chunk(
            file_id,
            0,
            10,
            "function with OR and AND",
            Some(&embedding),
            None,
        )
        .unwrap();
        db.mark_indexed(file_id).unwrap();

        // Search with special characters that would break raw FTS5
        // "OR" is an operator, but we want to match the literal word "OR"
        let options = SearchOptions {
            limit: Some(10),
            ..Default::default()
        };

        // This should not panic or error due to syntax
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_with_time_range() {
        let db = Database::new(":memory:").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let one_hour_ago = now - 3600;
        let two_hours_ago = now - 7200;

        let recent_id = db.add_or_update_file("/recent.rs", now).unwrap();
        let mid_id = db.add_or_update_file("/mid.rs", one_hour_ago).unwrap();
        let old_id = db.add_or_update_file("/old.rs", two_hours_ago).unwrap();

        let embedding: Vec<f32> = vec![1.0; 384];
        db.add_chunk(recent_id, 0, 10, "function a", Some(&embedding), None)
            .unwrap();
        db.add_chunk(mid_id, 0, 10, "function b", Some(&embedding), None)
            .unwrap();
        db.add_chunk(old_id, 0, 10, "function c", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(recent_id).unwrap();
        db.mark_indexed(mid_id).unwrap();
        db.mark_indexed(old_id).unwrap();

        // Filter to only mid-age files: one_hour_ago <= last_modified <= now
        let options = SearchOptions {
            limit: Some(10),
            start_time: Some(one_hour_ago),
            end_time: Some(now),
            recency_weight: Some(0.0),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        // With the bug, both ?2 and ?2 bind to the same value, so we get 1 or 0 results.
        // With the fix, we should get 2: recent.rs (now) and mid.rs (one_hour_ago).
        // old.rs (two_hours_ago) is outside the range.
        assert_eq!(
            results.len(),
            2,
            "Expected 2 files in time range [{}, {}], got {}",
            one_hour_ago,
            now,
            results.len()
        );
    }
}
