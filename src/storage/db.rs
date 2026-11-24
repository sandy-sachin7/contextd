use anyhow::{anyhow, Context, Result};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{SystemTime, UNIX_EPOCH};

static INIT_SQLITE_VEC: Once = Once::new();

pub const SCHEMA_VERSION: &str = "2";
pub const DEFAULT_EMBEDDING_DIM: usize = 384;
pub const DEFAULT_MODEL_TYPE: &str = "all-minilm-l6-v2";

fn ensure_sqlite_vec_registered() {
    #[allow(clippy::missing_transmute_annotations)]
    INIT_SQLITE_VEC.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}

fn encode_f32_le(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for val in values {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

fn escape_sql_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn extract_extension(file_path: &str) -> String {
    Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn normalize_extension_filter(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_lowercase()
}

/// Tokenize a free-text query into sanitized FTS5 OR-joined quoted terms.
/// Returns `None` if no searchable tokens exist.
pub fn build_fts5_query(query_text: &str) -> Option<String> {
    let tokens: Vec<String> = query_text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .take(32)
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" OR "))
    }
}

/// Extract context lines around a chunk's byte range from a file on disk.
fn populate_context_lines(
    file_path: &str,
    start_offset: usize,
    end_offset: usize,
    context_lines: usize,
) -> Option<(Option<String>, Option<String>, usize, usize)> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let clamped_start = start_offset.min(content.len());
    let clamped_end = end_offset.min(content.len()).max(clamped_start);

    // Ensure char boundaries for UTF-8 safety
    let safe_start = content.floor_char_boundary(clamped_start);
    let safe_end = content.floor_char_boundary(clamped_end);

    let start_line_idx = content[..safe_start].matches('\n').count();
    let end_line_idx = if safe_end > safe_start {
        let slice = &content[safe_start..safe_end];
        let trimmed = slice.strip_suffix('\n').unwrap_or(slice);
        start_line_idx + trimmed.matches('\n').count()
    } else {
        start_line_idx
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let line_start = start_line_idx.min(lines.len() - 1);
    let line_end = end_line_idx.min(lines.len() - 1);

    let before = if line_start > 0 {
        let from = line_start.saturating_sub(context_lines);
        Some(lines[from..line_start].join("\n"))
    } else {
        None
    };

    let after = if line_end + 1 < lines.len() {
        let to = (line_end + 1 + context_lines).min(lines.len());
        Some(lines[line_end + 1..to].join("\n"))
    } else {
        None
    };

    Some((before, after, line_start + 1, line_end + 1))
}

/// Input payload for batch transactional chunk insertion.
pub struct ChunkInput<'a> {
    pub start: u64,
    pub end: u64,
    pub content: &'a str,
    pub embedding: Option<&'a [f32]>,
    pub metadata: Option<&'a str>,
}

/// Result of checking whether a file needs reindexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReindexCheck {
    Unchanged,
    CheckHash(String),
    NeedsIndex,
}

enum ReadConn<'a> {
    Pooled(PooledConnection<SqliteConnectionManager>),
    Locked(MutexGuard<'a, Connection>),
}

impl Deref for ReadConn<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        match self {
            ReadConn::Pooled(c) => c,
            ReadConn::Locked(g) => g,
        }
    }
}

#[derive(Clone)]
pub struct Database {
    writer: Arc<Mutex<Connection>>,
    readers: Option<Pool<SqliteConnectionManager>>,
    embedding_dim: usize,
    model_type: String,
}

impl Database {
    /// Open or initialize a database using the default 384-dim `all-minilm-l6-v2` schema.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open(path, DEFAULT_EMBEDDING_DIM, DEFAULT_MODEL_TYPE)
    }

    /// Open or initialize a database with an explicit embedding dimension and model type.
    /// Detects mismatches against existing database metadata cleanly.
    pub fn open<P: AsRef<Path>>(path: P, embedding_dim: usize, model_type: &str) -> Result<Self> {
        if embedding_dim == 0 {
            return Err(anyhow!("Embedding dimension must be greater than 0"));
        }

        ensure_sqlite_vec_registered();

        let path_ref = path.as_ref();
        let is_memory = path_ref == Path::new(":memory:");

        if !is_memory {
            if let Some(parent) = path_ref.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create database directory {:?}", parent)
                    })?;
                }
            }
        }

        let conn = Connection::open(path_ref)
            .with_context(|| format!("Failed to open SQLite database {:?}", path_ref))?;

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;

        let mut db = Self {
            writer: Arc::new(Mutex::new(conn)),
            readers: None,
            embedding_dim,
            model_type: model_type.to_string(),
        };

        db.init()?;

        if !is_memory {
            let manager = SqliteConnectionManager::file(path_ref)
                .with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
                .with_init(|c| {
                    c.execute_batch(
                        "PRAGMA query_only = ON;
                         PRAGMA busy_timeout = 5000;",
                    )
                });
            let pool = Pool::builder()
                .max_size(8)
                .build(manager)
                .context("Failed to initialize SQLite WAL reader pool")?;
            db.readers = Some(pool);
        }

        Ok(db)
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    pub fn model_type(&self) -> &str {
        &self.model_type
    }

    fn lock_writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| anyhow!("Database writer mutex poisoned"))
    }

    fn read_conn(&self) -> Result<ReadConn<'_>> {
        if let Some(pool) = &self.readers {
            let conn = pool
                .get()
                .context("Failed to acquire read connection from SQLite WAL pool")?;
            Ok(ReadConn::Pooled(conn))
        } else {
            Ok(ReadConn::Locked(self.lock_writer()?))
        }
    }

    fn init(&self) -> Result<()> {
        let conn = self.lock_writer()?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        // Check existing metadata for model / dimension mismatches
        let stored_dim: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'embedding_dim'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let stored_model: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'model_type'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(dim_str) = stored_dim {
            let existing_dim: usize = dim_str.parse().unwrap_or(0);
            if existing_dim != self.embedding_dim {
                return Err(anyhow!(
                    "Database embedding dimension mismatch: database uses {} dims (model '{}'), \
                     but configured model '{}' uses {} dims. Remove or recreate the database file to switch models.",
                    existing_dim,
                    stored_model.as_deref().unwrap_or("unknown"),
                    self.model_type,
                    self.embedding_dim
                ));
            }
        }
        if let Some(ref existing_model) = stored_model {
            if existing_model != &self.model_type {
                return Err(anyhow!(
                    "Database model mismatch: database was indexed with '{}', \
                     but configured model is '{}'. Remove or recreate the database file to switch models.",
                    existing_model,
                    self.model_type
                ));
            }
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                last_modified INTEGER NOT NULL,
                last_modified_ns INTEGER NOT NULL DEFAULT 0,
                file_size INTEGER NOT NULL DEFAULT 0,
                content_hash TEXT NOT NULL DEFAULT '',
                last_indexed INTEGER
            )",
            [],
        )?;

        // Migrate v1 `files` table if columns are missing
        let existing_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(files)")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        if !existing_cols.iter().any(|c| c == "last_modified_ns") {
            conn.execute(
                "ALTER TABLE files ADD COLUMN last_modified_ns INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !existing_cols.iter().any(|c| c == "file_size") {
            conn.execute(
                "ALTER TABLE files ADD COLUMN file_size INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !existing_cols.iter().any(|c| c == "content_hash") {
            conn.execute(
                "ALTER TABLE files ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                start_offset INTEGER NOT NULL,
                end_offset INTEGER NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_files_path ON files(path)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id)",
            [],
        )?;

        let vec_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding float[{}] distance_metric=cosine
            )",
            self.embedding_dim
        );
        conn.execute(&vec_sql, [])?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(content, tokenize='porter')",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS query_hits (
                file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
                hit_count INTEGER DEFAULT 0,
                last_hit INTEGER
            )",
            [],
        )?;

        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('model_type', ?1)",
            params![self.model_type],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('embedding_dim', ?1)",
            params![self.embedding_dim.to_string()],
        )?;

        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.read_conn()?;
        let val = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(val)
    }

    /// Transactionally upsert a file, replace all of its chunks/vectors/FTS rows,
    /// and mark it indexed in a single SQLite transaction.
    pub fn index_file_chunks(
        &self,
        path: &str,
        last_modified: u64,
        last_modified_ns: i64,
        file_size: u64,
        content_hash: &str,
        chunks: &[ChunkInput<'_>],
    ) -> Result<i64> {
        for chunk in chunks {
            if let Some(emb) = chunk.embedding {
                if emb.len() != self.embedding_dim {
                    return Err(anyhow!(
                        "Chunk embedding dimension {} does not match database dimension {}",
                        emb.len(),
                        self.embedding_dim
                    ));
                }
            }
        }

        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO files (path, last_modified, last_modified_ns, file_size, content_hash, last_indexed)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)
             ON CONFLICT(path) DO UPDATE SET
                last_modified = excluded.last_modified,
                last_modified_ns = excluded.last_modified_ns,
                file_size = excluded.file_size,
                content_hash = excluded.content_hash,
                last_indexed = NULL",
            params![
                path,
                last_modified as i64,
                last_modified_ns,
                file_size as i64,
                content_hash
            ],
        )?;

        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                r.get(0)
            })?;

        tx.execute(
            "DELETE FROM chunks_vec WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        {
            let mut insert_chunk = tx.prepare_cached(
                "INSERT INTO chunks (file_id, start_offset, end_offset, content, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut insert_vec =
                tx.prepare_cached("INSERT INTO chunks_vec (chunk_id, embedding) VALUES (?1, ?2)")?;
            let mut insert_fts =
                tx.prepare_cached("INSERT INTO chunks_fts (rowid, content) VALUES (?1, ?2)")?;

            for chunk in chunks {
                insert_chunk.execute(params![
                    file_id,
                    chunk.start as i64,
                    chunk.end as i64,
                    chunk.content,
                    chunk.metadata
                ])?;
                let chunk_id = tx.last_insert_rowid();

                if let Some(emb) = chunk.embedding {
                    let emb_bytes = encode_f32_le(emb);
                    insert_vec.execute(params![chunk_id, emb_bytes.as_slice()])?;
                }
                insert_fts.execute(params![chunk_id, chunk.content])?;
            }
        }

        tx.execute(
            "UPDATE files SET last_indexed = strftime('%s', 'now') WHERE id = ?1",
            params![file_id],
        )?;

        tx.commit()?;
        Ok(file_id)
    }

    /// Update only `(last_modified, last_modified_ns, file_size)` when content hash matched.
    pub fn touch_file_metadata(
        &self,
        path: &str,
        last_modified: u64,
        last_modified_ns: i64,
        file_size: u64,
    ) -> Result<()> {
        let conn = self.lock_writer()?;
        conn.execute(
            "UPDATE files
             SET last_modified = ?2, last_modified_ns = ?3, file_size = ?4
             WHERE path = ?1",
            params![
                path,
                last_modified as i64,
                last_modified_ns,
                file_size as i64
            ],
        )?;
        Ok(())
    }

    /// Transactionally delete a file and all associated vector, FTS, chunk, and hit rows.
    pub fn delete_file_by_path(&self, path: &str) -> Result<bool> {
        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;

        let file_id: Option<i64> = tx
            .query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                r.get(0)
            })
            .optional()?;

        let Some(fid) = file_id else {
            return Ok(false);
        };

        tx.execute(
            "DELETE FROM chunks_vec WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![fid],
        )?;
        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![fid],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![fid])?;
        tx.execute("DELETE FROM query_hits WHERE file_id = ?1", params![fid])?;
        tx.execute("DELETE FROM files WHERE id = ?1", params![fid])?;

        tx.commit()?;
        Ok(true)
    }

    /// Transactionally delete all indexed files under a directory prefix.
    pub fn delete_files_under_prefix(&self, dir_prefix: &str) -> Result<usize> {
        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;

        let normalized = dir_prefix.trim_end_matches(['/', '\\']);
        let pattern_unix = format!("{}/%", escape_sql_like(normalized));
        let pattern_win = format!("{}\\%", escape_sql_like(normalized));

        let file_ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM files
                 WHERE path = ?1
                    OR path LIKE ?2 ESCAPE '\\'
                    OR path LIKE ?3 ESCAPE '\\'",
            )?;
            let rows = stmt
                .query_map(params![normalized, pattern_unix, pattern_win], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        let count = file_ids.len();
        for fid in file_ids {
            tx.execute(
                "DELETE FROM chunks_vec WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
                params![fid],
            )?;
            tx.execute(
                "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
                params![fid],
            )?;
            tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![fid])?;
            tx.execute("DELETE FROM query_hits WHERE file_id = ?1", params![fid])?;
            tx.execute("DELETE FROM files WHERE id = ?1", params![fid])?;
        }

        tx.commit()?;
        Ok(count)
    }

    /// List all indexed file paths for startup reconciliation.
    pub fn list_indexed_paths(&self) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn.prepare("SELECT path FROM files")?;
        let paths = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(paths)
    }

    pub fn add_or_update_file(&self, path: &str, last_modified: u64) -> Result<i64> {
        let conn = self.lock_writer()?;

        conn.execute(
            "INSERT INTO files (path, last_modified, last_indexed)
             VALUES (?1, ?2, NULL)
             ON CONFLICT(path) DO UPDATE SET
                last_modified = ?2,
                last_indexed = NULL",
            params![path, last_modified as i64],
        )?;

        let id = conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;

        Ok(id)
    }

    pub fn get_file_id(&self, path: &str) -> Result<Option<i64>> {
        let conn = self.read_conn()?;
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
        let conn = self.lock_writer()?;
        conn.execute(
            "UPDATE files SET last_indexed = strftime('%s', 'now') WHERE id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    /// Fast metadata check using `(mtime_ns, file_size)` and stored `blake3` hash.
    pub fn check_reindex_state(
        &self,
        path: &str,
        mtime_ns: i64,
        file_size: u64,
    ) -> Result<ReindexCheck> {
        let conn = self.read_conn()?;
        let row: Option<(i64, i64, String, Option<i64>)> = conn
            .query_row(
                "SELECT last_modified_ns, file_size, content_hash, last_indexed
                 FROM files WHERE path = ?1",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        match row {
            None => Ok(ReindexCheck::NeedsIndex),
            Some((_, _, _, None)) => Ok(ReindexCheck::NeedsIndex),
            Some((stored_ns, stored_size, stored_hash, Some(_))) => {
                if !stored_hash.is_empty()
                    && stored_ns == mtime_ns
                    && stored_size == file_size as i64
                {
                    Ok(ReindexCheck::Unchanged)
                } else if !stored_hash.is_empty() {
                    Ok(ReindexCheck::CheckHash(stored_hash))
                } else {
                    Ok(ReindexCheck::NeedsIndex)
                }
            }
        }
    }

    pub fn needs_reindexing(&self, path: &str, current_modified: u64) -> Result<bool> {
        let conn = self.read_conn()?;
        let row: Option<(i64, Option<i64>)> = conn
            .query_row(
                "SELECT last_modified, last_indexed FROM files WHERE path = ?1",
                params![path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        match row {
            Some((stored_mtime, Some(_))) => Ok(current_modified as i64 != stored_mtime),
            Some((_, None)) | None => Ok(true),
        }
    }

    pub fn clear_chunks(&self, file_id: i64) -> Result<()> {
        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM chunks_vec WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;
        tx.commit()?;
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
        if let Some(emb) = embedding {
            if emb.len() != self.embedding_dim {
                return Err(anyhow!(
                    "Embedding dimension {} does not match database dimension {}",
                    emb.len(),
                    self.embedding_dim
                ));
            }
        }

        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO chunks (file_id, start_offset, end_offset, content, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_id, start as i64, end as i64, content, metadata],
        )?;

        let chunk_id = tx.last_insert_rowid();

        if let Some(emb) = embedding {
            let emb_bytes = encode_f32_le(emb);
            tx.execute(
                "INSERT INTO chunks_vec (chunk_id, embedding) VALUES (?1, ?2)",
                params![chunk_id, emb_bytes.as_slice()],
            )?;
        }

        tx.execute(
            "INSERT INTO chunks_fts (rowid, content) VALUES (?1, ?2)",
            params![chunk_id, content],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Get database statistics
    pub fn get_stats(&self) -> Result<DbStats> {
        let conn = self.read_conn()?;

        let file_count: u64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let chunk_count: u64 =
            conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        let page_count: u64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: u64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let db_size = page_count * page_size;

        Ok(DbStats {
            file_count,
            chunk_count,
            db_size,
        })
    }

    /// Record a search hit for a file (for frequency ranking).
    pub fn record_search_hit(&self, file_id: i64) -> Result<()> {
        self.record_search_hits(&[file_id])
    }

    /// Batch-record search hits for returned files in a single transaction.
    pub fn record_search_hits(&self, file_ids: &[i64]) -> Result<()> {
        if file_ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock_writer()?;
        let tx = conn.transaction()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO query_hits (file_id, hit_count, last_hit)
                 VALUES (?1, 1, ?2)
                 ON CONFLICT(file_id) DO UPDATE SET
                    hit_count = hit_count + 1,
                    last_hit = ?2",
            )?;
            for &fid in file_ids {
                if fid > 0 {
                    stmt.execute(params![fid, now])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Hybrid search combining sqlite-vec KNN and tokenized FTS5 via normalized RRF.
    pub fn search_chunks_hybrid(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let limit = options.limit.unwrap_or(10);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let k = 60.0_f32;
        let candidate_limit = (limit * 5).clamp(50, 200);

        // 1. Vector KNN Search (without context_lines yet — populate only for final top-N!)
        let vector_options = SearchOptions {
            limit: Some(candidate_limit),
            start_time: options.start_time,
            end_time: options.end_time,
            file_types: options.file_types.clone(),
            paths: options.paths.clone(),
            min_score: None,
            recency_weight: options.recency_weight,
            frequency_weight: options.frequency_weight,
            context_lines: None,
        };
        let vector_results = self.search_chunks_enhanced(query_embedding, &vector_options)?;

        // 2. Tokenized FTS5 Search with SQL filters applied BEFORE LIMIT
        let mut fts_results = Vec::new();
        if let Some(fts_query) = build_fts5_query(query_text) {
            let conn = self.read_conn()?;
            let mut sql = String::from(
                "SELECT c.id, c.content, f.path, f.last_modified, f.id, c.start_offset, c.end_offset
                 FROM chunks_fts fts
                 JOIN chunks c ON fts.rowid = c.id
                 JOIN files f ON c.file_id = f.id
                 WHERE fts.content MATCH ?",
            );

            let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            sql_params.push(Box::new(fts_query));

            append_file_filters(&mut sql, &mut sql_params, options);

            sql.push_str(" ORDER BY fts.rank LIMIT ?");
            sql_params.push(Box::new(candidate_limit as i64));

            let mut stmt = conn.prepare(&sql)?;
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                sql_params.iter().map(|p| p.as_ref()).collect();

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)? as u64,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)? as u64,
                        row.get::<_, i64>(6)? as u64,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            for (id, content, file_path, last_modified, file_id, start_offset, end_offset) in rows {
                let file_type = extract_extension(&file_path);
                fts_results.push(SearchResult {
                    id,
                    file_id,
                    content,
                    score: 0.0,
                    file_path,
                    file_type,
                    last_modified,
                    start_offset,
                    end_offset,
                    ..Default::default()
                });
            }
        }

        // 3. Normalized Reciprocal Rank Fusion (RRF) into [0.0, 1.0]
        let mut rrf_scores: HashMap<i64, f32> = HashMap::new();
        let mut vec_similarity: HashMap<i64, f32> = HashMap::new();
        let mut has_fts_hit: HashMap<i64, bool> = HashMap::new();
        let mut results_map: HashMap<i64, SearchResult> = HashMap::new();

        for (rank, res) in vector_results.iter().enumerate() {
            let rrf = 1.0 / (k + (rank as f32 + 1.0));
            *rrf_scores.entry(res.id).or_insert(0.0) += rrf;
            vec_similarity.insert(res.id, res.score);
            results_map.insert(res.id, res.clone());
        }

        for (rank, res) in fts_results.iter().enumerate() {
            let rrf = 1.0 / (k + (rank as f32 + 1.0));
            *rrf_scores.entry(res.id).or_insert(0.0) += rrf;
            has_fts_hit.insert(res.id, true);
            results_map.entry(res.id).or_insert_with(|| res.clone());
        }

        let max_possible = (if !vector_results.is_empty() {
            1.0 / (k + 1.0)
        } else {
            0.0
        }) + (if !fts_results.is_empty() {
            1.0 / (k + 1.0)
        } else {
            0.0
        });

        let mut final_results: Vec<SearchResult> = results_map
            .into_values()
            .filter_map(|mut res| {
                let raw_rrf = rrf_scores.get(&res.id).copied().unwrap_or(0.0);
                let norm_rrf = if max_possible > 0.0 {
                    (raw_rrf / max_possible).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // Blend normalized RRF with vector similarity when available so score reflects
                // both rank consensus and actual semantic relevance.
                let score = match (
                    vec_similarity.get(&res.id),
                    has_fts_hit.contains_key(&res.id),
                ) {
                    (Some(&vsim), true) => {
                        ((norm_rrf + vsim.clamp(0.0, 1.0)) * 0.5).clamp(0.0, 1.0)
                    }
                    (Some(&vsim), false) => {
                        if !fts_results.is_empty() {
                            (norm_rrf * 0.5 + vsim.clamp(0.0, 1.0) * 0.5).clamp(0.0, 1.0)
                        } else {
                            vsim.clamp(0.0, 1.0)
                        }
                    }
                    (None, true) => norm_rrf,
                    (None, false) => 0.0,
                };

                if let Some(min) = options.min_score {
                    if score < min {
                        return None;
                    }
                }
                res.score = score;
                Some(res)
            })
            .collect();

        final_results.sort_by(|a, b| {
            let sa = if a.score.is_nan() { 0.0 } else { a.score };
            let sb = if b.score.is_nan() { 0.0 } else { b.score };
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        final_results.truncate(limit);

        if let Some(ctx_lines) = options.context_lines.filter(|&n| n > 0) {
            for res in &mut final_results {
                if let Some((before, after, line_start, line_end)) = populate_context_lines(
                    &res.file_path,
                    res.start_offset as usize,
                    res.end_offset as usize,
                    ctx_lines,
                ) {
                    res.context_before = before;
                    res.context_after = after;
                    res.line_start = Some(line_start);
                    res.line_end = Some(line_end);
                }
            }
        }

        Ok(final_results)
    }

    /// Vector KNN search powered by `sqlite-vec` (`WHERE embedding MATCH ?1 AND k = ?2`).
    /// Never loads full table contents into RAM — only fetches metadata & content for top-K IDs.
    pub fn search_chunks_enhanced(
        &self,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let limit = options.limit.unwrap_or(10);
        if limit == 0 {
            return Ok(Vec::new());
        }
        if query_embedding.len() != self.embedding_dim {
            return Err(anyhow!(
                "Query embedding dimension {} does not match database dimension {}",
                query_embedding.len(),
                self.embedding_dim
            ));
        }

        const MAX_FILTERED_KNN_K: usize = 4096;

        let has_file_filters = options.start_time.is_some()
            || options.end_time.is_some()
            || options.file_types.as_ref().is_some_and(|v| !v.is_empty())
            || options.paths.as_ref().is_some_and(|v| !v.is_empty());
        let has_filters = has_file_filters || options.min_score.is_some();

        let mut knn_k: usize = if has_filters {
            (limit.saturating_mul(10)).clamp(64, 512)
        } else {
            (limit.saturating_mul(3)).clamp(20, 256)
        };

        let query_bytes = encode_f32_le(query_embedding);

        struct CandidateRow {
            id: i64,
            content: String,
            file_path: String,
            last_modified: u64,
            file_id: i64,
            hit_count: i64,
            start_offset: u64,
            end_offset: u64,
        }

        let conn = self.read_conn()?;
        let mut distance_by_id: HashMap<i64, f32>;
        let mut raw_rows: Vec<CandidateRow>;

        // Progressive over-fetch when file/path/time filters are active: expand k
        // up to MAX_FILTERED_KNN_K if filtered results fall short of `limit`.
        loop {
            // Step 1: Real sqlite-vec KNN index lookup — returns ONLY (chunk_id, distance)
            let knn_candidates: Vec<(i64, f32)> = {
                let mut stmt = conn.prepare(
                    "SELECT chunk_id, distance
                     FROM chunks_vec
                     WHERE embedding MATCH ?1 AND k = ?2
                     ORDER BY distance",
                )?;
                let rows = stmt
                    .query_map(params![query_bytes.as_slice(), knn_k as i64], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, f32>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };

            if knn_candidates.is_empty() {
                return Ok(Vec::new());
            }

            let exhausted = knn_candidates.len() < knn_k;
            let below_min_score_frontier = options.min_score.is_some_and(|min| {
                knn_candidates
                    .last()
                    .is_some_and(|&(_, dist)| (1.0 - dist) < min)
            });

            distance_by_id = knn_candidates.iter().copied().collect();

            // Step 2: Fetch content & file metadata ONLY for the top-K candidate chunk IDs
            let placeholders = std::iter::repeat_n("?", knn_candidates.len())
                .collect::<Vec<_>>()
                .join(", ");
            let mut sql = format!(
                "SELECT c.id, c.content, f.path, f.last_modified, f.id,
                        COALESCE(qh.hit_count, 0), c.start_offset, c.end_offset
                 FROM chunks c
                 JOIN files f ON c.file_id = f.id
                 LEFT JOIN query_hits qh ON f.id = qh.file_id
                 WHERE c.id IN ({})",
                placeholders
            );

            let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> =
                Vec::with_capacity(knn_candidates.len() + 8);
            for &(cid, _) in &knn_candidates {
                sql_params.push(Box::new(cid));
            }

            append_file_filters(&mut sql, &mut sql_params, options);

            raw_rows = {
                let mut stmt = conn.prepare(&sql)?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    sql_params.iter().map(|p| p.as_ref()).collect();
                let rows = stmt
                    .query_map(params_refs.as_slice(), |row| {
                        Ok(CandidateRow {
                            id: row.get(0)?,
                            content: row.get(1)?,
                            file_path: row.get(2)?,
                            last_modified: row.get::<_, i64>(3)? as u64,
                            file_id: row.get(4)?,
                            hit_count: row.get(5)?,
                            start_offset: row.get::<_, i64>(6)? as u64,
                            end_offset: row.get::<_, i64>(7)? as u64,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };

            let qualified_count = match options.min_score {
                Some(min) => raw_rows
                    .iter()
                    .filter(|r| {
                        let dist = distance_by_id.get(&r.id).copied().unwrap_or(1.0);
                        (1.0 - dist) >= min
                    })
                    .count(),
                None => raw_rows.len(),
            };

            if !has_file_filters
                || qualified_count >= limit
                || exhausted
                || below_min_score_frontier
                || knn_k >= MAX_FILTERED_KNN_K
            {
                break;
            }

            knn_k = knn_k.saturating_mul(4).min(MAX_FILTERED_KNN_K);
        }

        drop(conn);

        // Hoist SystemTime::now() outside the scoring loop
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let recency_weight = options.recency_weight.unwrap_or(0.1);
        let frequency_weight = options.frequency_weight.unwrap_or(0.1);

        let mut scored_chunks = Vec::with_capacity(raw_rows.len());

        for CandidateRow {
            id,
            content,
            file_path,
            last_modified,
            file_id,
            hit_count,
            start_offset,
            end_offset,
        } in raw_rows
        {
            let distance = distance_by_id.get(&id).copied().unwrap_or(1.0);
            let score = 1.0 - distance;

            if let Some(min) = options.min_score {
                if score < min {
                    continue;
                }
            }

            let file_type = extract_extension(&file_path);

            let recency_adjusted = if recency_weight > 0.0 {
                let age_hours = now.saturating_sub(last_modified) / 3600;
                let recency_boost = 1.0 / (1.0 + (age_hours as f32 / 24.0));
                score * (1.0 - recency_weight) + recency_boost * recency_weight
            } else {
                score
            };

            let final_score = if frequency_weight > 0.0 && hit_count > 0 {
                let freq_boost = (hit_count as f32).ln_1p() * frequency_weight;
                recency_adjusted + freq_boost
            } else {
                recency_adjusted
            };

            scored_chunks.push(SearchResult {
                id,
                file_id,
                content,
                score: final_score,
                file_path,
                file_type,
                last_modified,
                start_offset,
                end_offset,
                ..Default::default()
            });
        }

        scored_chunks.sort_by(|a, b| {
            let sa = if a.score.is_nan() { 0.0 } else { a.score };
            let sb = if b.score.is_nan() { 0.0 } else { b.score };
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        scored_chunks.truncate(limit);

        if let Some(ctx_lines) = options.context_lines.filter(|&n| n > 0) {
            for res in &mut scored_chunks {
                if let Some((before, after, line_start, line_end)) = populate_context_lines(
                    &res.file_path,
                    res.start_offset as usize,
                    res.end_offset as usize,
                    ctx_lines,
                ) {
                    res.context_before = before;
                    res.context_after = after;
                    res.line_start = Some(line_start);
                    res.line_end = Some(line_end);
                }
            }
        }

        Ok(scored_chunks)
    }
}

fn append_file_filters(
    sql: &mut String,
    sql_params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    options: &SearchOptions,
) {
    if let Some(start) = options.start_time {
        sql.push_str(" AND f.last_modified >= ?");
        sql_params.push(Box::new(start as i64));
    }

    if let Some(end) = options.end_time {
        sql.push_str(" AND f.last_modified <= ?");
        sql_params.push(Box::new(end as i64));
    }

    if let Some(types) = &options.file_types {
        let normalized: Vec<String> = types
            .iter()
            .map(|t| normalize_extension_filter(t))
            .filter(|t| !t.is_empty())
            .collect();
        if !normalized.is_empty() {
            let clauses = std::iter::repeat_n("LOWER(f.path) LIKE ? ESCAPE '\\'", normalized.len())
                .collect::<Vec<_>>()
                .join(" OR ");
            sql.push_str(&format!(" AND ({})", clauses));
            for ext in normalized {
                sql_params.push(Box::new(format!("%.{}", escape_sql_like(&ext))));
            }
        }
    }

    if let Some(path_filters) = &options.paths {
        let non_empty: Vec<&String> = path_filters.iter().filter(|p| !p.is_empty()).collect();
        if !non_empty.is_empty() {
            let clauses = std::iter::repeat_n("f.path LIKE ? ESCAPE '\\'", non_empty.len())
                .collect::<Vec<_>>()
                .join(" OR ");
            sql.push_str(&format!(" AND ({})", clauses));
            for pat in non_empty {
                sql_params.push(Box::new(format!("%{}%", escape_sql_like(pat))));
            }
        }
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DbStats {
    pub file_count: u64,
    pub chunk_count: u64,
    pub db_size: u64,
}

/// Search options for enhanced / hybrid chunk search
#[derive(Default, Clone, Debug)]
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
#[derive(Clone, Default, Debug)]
pub struct SearchResult {
    pub id: i64,
    pub file_id: i64,
    pub content: String,
    pub score: f32,
    pub file_path: String,
    pub file_type: String,
    pub last_modified: u64,
    pub start_offset: u64,
    pub end_offset: u64,
    /// Context lines before the matched content
    pub context_before: Option<String>,
    /// Context lines after the matched content
    pub context_after: Option<String>,
    /// Starting 1-based line number in the source file
    pub line_start: Option<usize>,
    /// Ending 1-based line number in the source file
    pub line_end: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_vec(dim: usize, hot_idx: usize) -> Vec<f32> {
        let mut v = vec![0.0_f32; dim];
        v[hot_idx % dim] = 1.0;
        v
    }

    #[test]
    fn test_database_init_and_meta() {
        let db = Database::new(":memory:").unwrap();
        assert_eq!(db.get_meta("schema_version").unwrap().as_deref(), Some("2"));
        assert_eq!(
            db.get_meta("embedding_dim").unwrap().as_deref(),
            Some("384")
        );
        assert_eq!(
            db.get_meta("model_type").unwrap().as_deref(),
            Some("all-minilm-l6-v2")
        );

        let conn = db.lock_writer().unwrap();
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
        assert!(tables.contains(&"meta".to_string()));

        // Verify redundant chunks.embedding BLOB column was removed
        let mut cols_stmt = conn.prepare("PRAGMA table_info(chunks)").unwrap();
        let cols: Vec<String> = cols_stmt
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(!cols.contains(&"embedding".to_string()));
    }

    #[test]
    fn test_model_dimension_mismatch_detected() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("dim_test.db");

        {
            let db = Database::open(&db_path, 768, "all-mpnet-base-v2").unwrap();
            let emb = unit_vec(768, 0);
            db.index_file_chunks(
                "/a.rs",
                100,
                100_000,
                10,
                "hash1",
                &[ChunkInput {
                    start: 0,
                    end: 10,
                    content: "fn alpha() {}",
                    embedding: Some(&emb),
                    metadata: None,
                }],
            )
            .unwrap();
        }

        // Opening with wrong dimension or model must return a clear error
        let err = Database::open(&db_path, 384, "all-minilm-l6-v2")
            .err()
            .expect("Expected dimension mismatch error");
        assert!(err.to_string().contains("dimension mismatch"));

        // Opening with matching 768-dim model works and uses WAL reader pool
        let db = Database::open(&db_path, 768, "all-mpnet-base-v2").unwrap();
        let res = db
            .search_chunks_hybrid("alpha", &unit_vec(768, 0), &SearchOptions::default())
            .unwrap();
        assert_eq!(res.len(), 1);
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
    fn test_transactional_delete_cleans_all_tables() {
        let db = Database::new(":memory:").unwrap();
        let emb = unit_vec(384, 1);

        db.index_file_chunks(
            "/repo/src/auth.rs",
            100,
            100_000,
            25,
            "hash_auth",
            &[ChunkInput {
                start: 0,
                end: 25,
                content: "fn verify_jwt_token() {}",
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();

        assert_eq!(db.get_stats().unwrap().file_count, 1);
        assert_eq!(db.get_stats().unwrap().chunk_count, 1);

        let deleted = db.delete_file_by_path("/repo/src/auth.rs").unwrap();
        assert!(deleted);
        assert_eq!(db.get_stats().unwrap().file_count, 0);
        assert_eq!(db.get_stats().unwrap().chunk_count, 0);

        let results = db
            .search_chunks_hybrid("verify_jwt_token", &emb, &SearchOptions::default())
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_recency_boost() {
        let db = Database::new(":memory:").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let one_week_ago = now - (7 * 24 * 3600);

        let recent_id = db.add_or_update_file("/recent.rs", now).unwrap();
        let old_id = db.add_or_update_file("/old.rs", one_week_ago).unwrap();

        let embedding = unit_vec(384, 0);
        db.add_chunk(recent_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.add_chunk(old_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(recent_id).unwrap();
        db.mark_indexed(old_id).unwrap();

        let options = SearchOptions {
            limit: Some(10),
            recency_weight: Some(0.5),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].file_path.contains("recent"));
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_frequency_boost() {
        let db = Database::new(":memory:").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let frequent_id = db.add_or_update_file("/frequent.rs", now).unwrap();
        let rare_id = db.add_or_update_file("/rare.rs", now).unwrap();

        let embedding = unit_vec(384, 0);
        db.add_chunk(frequent_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.add_chunk(rare_id, 0, 10, "function test", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(frequent_id).unwrap();
        db.mark_indexed(rare_id).unwrap();

        for _ in 0..10 {
            db.record_search_hit(frequent_id).unwrap();
        }

        let options = SearchOptions {
            limit: Some(10),
            frequency_weight: Some(0.5),
            recency_weight: Some(0.0),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].file_path.contains("frequent"));
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_context_lines_extraction() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.rs");
        let content = "line 1\nline 2\nfn target() {\n    42\n}\nline 6\nline 7\n";
        std::fs::write(&file_path, content).unwrap();

        let start = content.find("fn target").unwrap() as u64;
        let end = start + "fn target() {\n    42\n}".len() as u64;

        let db = Database::new(":memory:").unwrap();
        let emb = unit_vec(384, 2);
        db.index_file_chunks(
            &file_path.to_string_lossy(),
            100,
            100_000,
            content.len() as u64,
            "hash_ctx",
            &[ChunkInput {
                start,
                end,
                content: "fn target() {\n    42\n}",
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();

        let options = SearchOptions {
            limit: Some(5),
            context_lines: Some(2),
            ..Default::default()
        };
        let results = db.search_chunks_hybrid("target", &emb, &options).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_start, Some(3));
        assert_eq!(results[0].line_end, Some(5));
        assert_eq!(results[0].context_before.as_deref(), Some("line 1\nline 2"));
        assert_eq!(results[0].context_after.as_deref(), Some("line 6\nline 7"));
    }

    #[test]
    fn test_fts_sanitization_and_hybrid_search() {
        let db = Database::new(":memory:").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let emb_a = unit_vec(384, 0);
        let emb_b = unit_vec(384, 1);

        db.index_file_chunks(
            "/src/auth.rs",
            now,
            1,
            30,
            "h1",
            &[ChunkInput {
                start: 0,
                end: 30,
                content: "function with OR and AND authentication handler",
                embedding: Some(&emb_a),
                metadata: None,
            }],
        )
        .unwrap();
        db.index_file_chunks(
            "/docs/README",
            now,
            1,
            20,
            "h2",
            &[ChunkInput {
                start: 0,
                end: 20,
                content: "unrelated deployment notes",
                embedding: Some(&emb_b),
                metadata: None,
            }],
        )
        .unwrap();

        // FTS5 special operators & quotes must not error, and non-contiguous terms must match
        let options = SearchOptions {
            limit: Some(10),
            file_types: Some(vec![".rs".to_string()]),
            min_score: Some(0.5),
            ..Default::default()
        };
        let results = db
            .search_chunks_hybrid(
                "authentication \"OR\" AND handler; DROP TABLE",
                &emb_a,
                &options,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "/src/auth.rs");
        assert_eq!(results[0].file_type, "rs");
        assert!(results[0].score >= 0.5 && results[0].score <= 1.0);

        // README without extension must have empty file_type, not "readme"
        let readme_res = db
            .search_chunks_hybrid("deployment", &emb_b, &SearchOptions::default())
            .unwrap();
        assert_eq!(readme_res[0].file_type, "");
    }

    #[test]
    fn test_search_with_time_range() {
        let db = Database::new(":memory:").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let one_hour_ago = now - 3600;
        let two_hours_ago = now - 7200;

        let recent_id = db.add_or_update_file("/recent.rs", now).unwrap();
        let mid_id = db.add_or_update_file("/mid.rs", one_hour_ago).unwrap();
        let old_id = db.add_or_update_file("/old.rs", two_hours_ago).unwrap();

        let embedding = unit_vec(384, 0);
        db.add_chunk(recent_id, 0, 10, "function a", Some(&embedding), None)
            .unwrap();
        db.add_chunk(mid_id, 0, 10, "function b", Some(&embedding), None)
            .unwrap();
        db.add_chunk(old_id, 0, 10, "function c", Some(&embedding), None)
            .unwrap();
        db.mark_indexed(recent_id).unwrap();
        db.mark_indexed(mid_id).unwrap();
        db.mark_indexed(old_id).unwrap();

        let options = SearchOptions {
            limit: Some(10),
            start_time: Some(one_hour_ago),
            end_time: Some(now),
            recency_weight: Some(0.0),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&embedding, &options).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_change_detection_mtime_size_hash() {
        let db = Database::new(":memory:").unwrap();
        let emb = unit_vec(384, 3);
        db.index_file_chunks(
            "/repo/a.rs",
            100,
            100_000_000_123,
            42,
            "blake3_abc",
            &[ChunkInput {
                start: 0,
                end: 10,
                content: "fn a() {}",
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();

        assert_eq!(
            db.check_reindex_state("/repo/a.rs", 100_000_000_123, 42)
                .unwrap(),
            ReindexCheck::Unchanged
        );
        // Same second, different nanosecond (or git checkout older mtime) triggers hash check
        assert_eq!(
            db.check_reindex_state("/repo/a.rs", 50_000_000_000, 42)
                .unwrap(),
            ReindexCheck::CheckHash("blake3_abc".to_string())
        );
        assert_eq!(
            db.check_reindex_state("/repo/missing.rs", 100, 42).unwrap(),
            ReindexCheck::NeedsIndex
        );
    }

    #[test]
    fn test_filtered_knn_progressive_overfetch_recalls_beyond_initial_window() {
        let db = Database::new(":memory:").unwrap();
        let query_emb = unit_vec(384, 0);

        // Insert 100 `.md` chunks that exactly match query_emb (distance 0.0),
        // filling more than the initial knn_k=64 window for limit=3.
        for i in 0..100 {
            let path = format!("/docs/note_{i}.md");
            db.index_file_chunks(
                &path,
                1_700_000_000,
                1_700_000_000_000_000_000,
                16,
                &format!("md_hash_{i}"),
                &[ChunkInput {
                    start: 0,
                    end: 16,
                    content: "markdown filler",
                    embedding: Some(&query_emb),
                    metadata: None,
                }],
            )
            .unwrap();
        }

        // Insert 3 `.rs` chunks with slightly lower similarity (orthogonal dim)
        let mut rs_emb = vec![0.0_f32; 384];
        rs_emb[0] = 0.6;
        rs_emb[1] = 0.8;
        for i in 0..3 {
            let path = format!("/src/mod_{i}.rs");
            db.index_file_chunks(
                &path,
                1_700_000_000,
                1_700_000_000_000_000_000,
                20,
                &format!("rs_hash_{i}"),
                &[ChunkInput {
                    start: 0,
                    end: 20,
                    content: "fn rust_target() {}",
                    embedding: Some(&rs_emb),
                    metadata: None,
                }],
            )
            .unwrap();
        }

        let options = SearchOptions {
            limit: Some(3),
            file_types: Some(vec!["rs".to_string()]),
            recency_weight: Some(0.0),
            frequency_weight: Some(0.0),
            ..Default::default()
        };
        let results = db.search_chunks_enhanced(&query_emb, &options).unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.file_type == "rs"));
    }
}
