use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable foreign keys and WAL mode
        conn.execute("PRAGMA foreign_keys = ON;", [])?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))?;

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

    pub fn clear_chunks(&self, file_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
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

        // Build query with optional filters
        let mut sql = "SELECT c.content, c.embedding, f.path, f.last_modified
                       FROM chunks c
                       JOIN files f ON c.file_id = f.id
                       WHERE c.embedding IS NOT NULL"
            .to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_time {
            sql.push_str(" AND f.last_modified >= ?");
            params.push(Box::new(start));
        }

        if let Some(end) = end_time {
            sql.push_str(" AND f.last_modified <= ?");
            params.push(Box::new(end));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let chunk_iter = stmt.query_map(params_refs.as_slice(), |row| {
            let content: String = row.get(0)?;
            let embedding_blob: Vec<u8> = row.get(1)?;
            let file_path: String = row.get(2)?;
            let last_modified: u64 = row.get(3)?;
            Ok((content, embedding_blob, file_path, last_modified))
        })?;

        let mut scored_chunks = Vec::new();

        for chunk in chunk_iter {
            let (content, embedding_blob, file_path, last_modified) = chunk?;

            // Extract file extension
            let file_type = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

            // Apply file type filter
            if let Some(types) = file_types {
                if !types.iter().any(|t| t.to_lowercase() == file_type) {
                    continue;
                }
            }

            // Apply path filter
            if let Some(path_filters) = paths {
                if !path_filters.iter().any(|p| file_path.contains(p)) {
                    continue;
                }
            }

            // Convert bytes back to Vec<f32>
            let embedding: Vec<f32> = embedding_blob
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                .collect();

            if embedding.len() != query_embedding.len() {
                continue;
            }

            // Cosine similarity
            let score: f32 = embedding
                .iter()
                .zip(query_embedding)
                .map(|(a, b)| a * b)
                .sum();

            // Apply minimum score filter
            if let Some(min) = min_score {
                if score < min {
                    continue;
                }
            }

            scored_chunks.push(SearchResult {
                content,
                score,
                file_path,
                file_type,
                last_modified,
            });
        }

        // Sort by score descending
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
}

/// Enhanced search result with metadata
pub struct SearchResult {
    pub content: String,
    pub score: f32,
    pub file_path: String,
    pub file_type: String,
    pub last_modified: u64,
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
}
