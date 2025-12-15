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
