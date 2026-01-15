use crate::indexer::embeddings::Embedder;
use crate::storage::db::Database;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub embedder: Arc<Embedder>,
    pub start_time: u64,
}

// ============================================================================
// Query Types
// ============================================================================

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    // Enhanced filters
    #[serde(default)]
    pub file_types: Option<Vec<String>>,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
    pub min_score: Option<f32>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub results: Vec<QueryResult>,
}

#[derive(Serialize)]
pub struct QueryResult {
    pub content: String,
    pub score: f32,
    // Enhanced metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<u64>,
}

// ============================================================================
// Health & Status Types
// ============================================================================

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub indexed_files: u64,
    pub total_chunks: u64,
    pub database_size_bytes: u64,
}

// ============================================================================
// Server Setup
// ============================================================================

pub async fn run_server(db: Database, embedder: Arc<Embedder>, host: &str, port: u16) {
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        embedder,
        start_time,
    };

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/status", get(handle_status))
        .route("/query", post(handle_query))
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("API listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// Handlers
// ============================================================================

async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = current_time() - state.start_time;
    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_secs: uptime,
    })
}

async fn handle_status(State(state): State<AppState>) -> Result<Json<StatusResponse>, StatusCode> {
    let uptime = current_time() - state.start_time;

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stats = db
        .get_stats()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(StatusResponse {
        status: "ok".to_string(),
        uptime_secs: uptime,
        indexed_files: stats.file_count,
        total_chunks: stats.chunk_count,
        database_size_bytes: stats.db_size,
    }))
}

async fn handle_query(
    State(state): State<AppState>,
    Json(payload): Json<QueryRequest>,
) -> Json<QueryResponse> {
    println!("Received query: {}", payload.query);

    // Embed query
    let embedding = match state.embedder.embed(&payload.query) {
        Ok(emb) => emb,
        Err(e) => {
            eprintln!("Embedding error: {}", e);
            return Json(QueryResponse { results: vec![] });
        }
    };

    // Search DB
    let db = state.db.lock().unwrap();

    let options = crate::storage::db::SearchOptions {
        limit: Some(payload.limit.unwrap_or(5)),
        start_time: payload.start_time,
        end_time: payload.end_time,
        file_types: payload.file_types,
        paths: payload.paths,
        min_score: payload.min_score,
        recency_weight: None,   // Use default
        frequency_weight: None, // Use default
        context_lines: None,    // Use default
    };

    let results = match db.search_chunks_enhanced(&embedding, &options) {
        Ok(res) => res
            .into_iter()
            .map(|r| QueryResult {
                content: r.content,
                score: r.score,
                file_path: Some(r.file_path),
                file_type: Some(r.file_type),
                last_modified: Some(r.last_modified),
            })
            .collect(),
        Err(e) => {
            eprintln!("Search error: {}", e);
            vec![]
        }
    };

    Json(QueryResponse { results })
}

fn current_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
