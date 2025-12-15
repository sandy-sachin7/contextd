use axum::{
    extract::{State, Json},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::storage::db::Database;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub results: Vec<QueryResult>,
}

#[derive(Serialize)]
pub struct QueryResult {
    pub content: String,
    pub source: String,
    pub score: f32,
}

pub async fn run_server(db: Database) {
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
    };

    let app = Router::new()
        .route("/query", post(handle_query))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await.unwrap();
    println!("API listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn handle_query(
    State(state): State<AppState>,
    Json(payload): Json<QueryRequest>,
) -> Json<QueryResponse> {
    // Phase 1: Mock response or basic keyword search
    // Since we don't have embeddings yet, we'll just return a placeholder
    // or maybe query the chunks table with LIKE if we want to be fancy.
    // Let's do a simple LIKE query for now.

    let _db = state.db.lock().unwrap();
    // TODO: Implement actual search in DB

    Json(QueryResponse {
        results: vec![
            QueryResult {
                content: format!("Result for: {}", payload.query),
                source: "mock".to_string(),
                score: 1.0,
            }
        ]
    })
}
