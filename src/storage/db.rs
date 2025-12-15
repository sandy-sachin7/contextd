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

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON;", [])?;

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

    pub fn get_file_id(&self, path: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let id = conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        ).optional()?;
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
        conn.execute(
            "DELETE FROM chunks WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    pub fn add_chunk(&self, file_id: i64, start: u64, end: u64, content: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks (file_id, start_offset, end_offset, content, embedding, metadata)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
            params![file_id, start, end, content],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_init() {
        let db = Database::new(":memory:").unwrap();
        let conn = db.conn.lock().unwrap();

        // Check tables exist
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'").unwrap();
        let tables: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap()
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
        let last_mod: u64 = conn.query_row(
            "SELECT last_modified FROM files WHERE id = ?1",
            params![id1],
            |row| row.get(0)
        ).unwrap();

        assert_eq!(last_mod, 200);
    }

    #[test]
    fn test_chunks() {
        let db = Database::new(":memory:").unwrap();
        let path = "/tmp/test.txt";
        let file_id = db.add_or_update_file(path, 100).unwrap();

        db.add_chunk(file_id, 0, 10, "chunk1").unwrap();
        db.add_chunk(file_id, 10, 20, "chunk2").unwrap();

        let conn = db.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id = ?1",
            params![file_id],
            |row| row.get(0)
        ).unwrap();

        assert_eq!(count, 2);

        // Test cascade delete (simulated by clearing chunks)
        drop(conn); // unlock
        db.clear_chunks(file_id).unwrap();

        let conn = db.conn.lock().unwrap();
        let count_after: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id = ?1",
            params![file_id],
            |row| row.get(0)
        ).unwrap();

        assert_eq!(count_after, 0);
    }
}
