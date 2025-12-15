use crate::indexer::embeddings::Embedder;
use crate::storage::db::Database;
use axum::{
    extract::{Json, State},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Database>>,
    embedder: Arc<Embedder>,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub start_time: Option<u64>, // Unix timestamp
    pub end_time: Option<u64>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub results: Vec<QueryResult>,
}

#[derive(Serialize)]
pub struct QueryResult {
    pub content: String,
    pub score: f32,
}

pub async fn run_server(db: Database, embedder: Arc<Embedder>, host: &str, port: u16) {
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        embedder,
    };

    let app = Router::new()
        .route("/query", post(handle_query))
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("API listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
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
    let limit = payload.limit.unwrap_or(5);
    let db = state.db.lock().unwrap();
    let results = match db.search_chunks(&embedding, limit, payload.start_time, payload.end_time) {
        Ok(res) => res
            .into_iter()
            .map(|(content, score)| QueryResult { content, score })
            .collect(),
        Err(e) => {
            eprintln!("Search error: {}", e);
            vec![]
        }
    };

    Json(QueryResponse { results })
}
