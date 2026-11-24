use crate::indexer::embeddings::TextEmbedder;
use crate::storage::db::{Database, SearchOptions};
use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Json, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::TimeoutLayer;

pub const MAX_BODY_BYTES: usize = 64 * 1024; // 64 KiB
pub const MAX_QUERY_LIMIT: usize = 100;
pub const MAX_QUERY_LEN: usize = 16 * 1024;
pub const REQUEST_TIMEOUT_SECS: u64 = 30;
pub const MAX_CONCURRENT_REQUESTS: usize = 64;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub embedder: Arc<dyn TextEmbedder>,
    pub start_time: Instant,
    pub auth_token: Option<Arc<String>>,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub max_results: Option<usize>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    #[serde(default)]
    pub file_types: Option<Vec<String>>,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
    pub min_score: Option<f32>,
    pub context_lines: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResponse {
    pub results: Vec<QueryResult>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResult {
    pub content: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StatusResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub indexed_files: u64,
    pub total_chunks: u64,
    pub database_size_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorResponse {
    pub error: String,
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorResponse { error: msg.into() })).into_response()
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1" | "[::1]"
    )
}

fn is_valid_loopback_host_header(host_header: &str) -> bool {
    let trimmed = host_header.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return false;
    }
    // IPv6 bracketed form: `[::1]` or `[::1]:3030`
    if let Some(rest) = trimmed.strip_prefix("[::1]") {
        return rest.is_empty()
            || rest
                .strip_prefix(':')
                .is_some_and(|p| p.parse::<u16>().is_ok());
    }
    let host_only = match trimmed.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => h,
        _ => trimmed.as_str(),
    };
    matches!(host_only, "localhost" | "127.0.0.1")
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn security_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let host_header = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if let Some(expected_token) = &state.auth_token {
        // Allow local loopback health checks without bearer token (e.g., Docker healthcheck)
        let is_local_health = path == "/health" && is_valid_loopback_host_header(host_header);
        if !is_local_health {
            let provided = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            match provided {
                Some(tok) if constant_time_eq(tok.as_bytes(), expected_token.as_bytes()) => {}
                _ => {
                    return error_response(
                        StatusCode::UNAUTHORIZED,
                        "Missing or invalid bearer token",
                    );
                }
            }
        }
    } else {
        // Unauthenticated local mode: strictly validate Host header to block DNS rebinding
        if !is_valid_loopback_host_header(host_header) {
            return error_response(
                StatusCode::FORBIDDEN,
                "Forbidden Host header (only localhost/127.0.0.1 permitted without auth_token)",
            );
        }
    }

    next.run(req).await
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/status", get(handle_status))
        .route("/query", post(handle_query))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TimeoutLayer::new(Duration::from_secs(REQUEST_TIMEOUT_SECS)))
        .layer(ConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS))
        .with_state(state)
}

pub async fn bind_server(host: &str, port: u16, auth_token: Option<&str>) -> Result<TcpListener> {
    if !is_loopback_host(host) && auth_token.is_none() {
        return Err(anyhow!(
            "Refusing to bind non-loopback address '{}' without an auth token. \
             Set server.auth_token in config or CONTEXTD_AUTH_TOKEN in the environment.",
            host
        ));
    }
    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind HTTP API server on {}", addr))?;
    println!("API listening on {}", listener.local_addr()?);
    Ok(listener)
}

pub async fn serve_on_listener(
    listener: TcpListener,
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    auth_token: Option<String>,
) -> Result<()> {
    let state = AppState {
        db: Arc::new(db),
        embedder,
        start_time: Instant::now(),
        auth_token: auth_token.map(Arc::new),
    };

    let app = build_router(state);
    axum::serve(listener, app)
        .await
        .context("HTTP API server error")?;
    Ok(())
}

pub async fn run_server(
    db: Database,
    embedder: Arc<dyn TextEmbedder>,
    host: &str,
    port: u16,
    auth_token: Option<String>,
) -> Result<()> {
    let listener = bind_server(host, port, auth_token.as_deref()).await?;
    serve_on_listener(listener, db, embedder, auth_token).await
}

async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
    })
}

async fn handle_status(State(state): State<AppState>) -> Response {
    let uptime = state.start_time.elapsed().as_secs();
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || db.get_stats()).await {
        Ok(Ok(stats)) => Json(StatusResponse {
            status: "ok".to_string(),
            uptime_secs: uptime,
            indexed_files: stats.file_count,
            total_chunks: stats.chunk_count,
            database_size_bytes: stats.db_size,
        })
        .into_response(),
        Ok(Err(e)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read database stats: {}", e),
        ),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Status worker failed: {}", e),
        ),
    }
}

async fn handle_query(
    State(state): State<AppState>,
    Json(payload): Json<QueryRequest>,
) -> Response {
    if payload.query.len() > MAX_QUERY_LEN {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!("Query exceeds maximum length of {} bytes", MAX_QUERY_LEN),
        );
    }

    if let Some(min_score) = payload.min_score {
        if !min_score.is_finite() || !(0.0..=1.0).contains(&min_score) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "min_score must be a finite number between 0.0 and 1.0",
            );
        }
    }

    let limit = payload.limit.unwrap_or(5).min(MAX_QUERY_LIMIT);
    let max_results = payload.max_results.unwrap_or(limit).min(MAX_QUERY_LIMIT);
    let effective_limit = limit.min(max_results);

    let options = SearchOptions {
        limit: Some(effective_limit),
        start_time: payload.start_time,
        end_time: payload.end_time,
        file_types: payload.file_types,
        paths: payload.paths,
        min_score: payload.min_score,
        recency_weight: None,
        frequency_weight: None,
        context_lines: payload.context_lines.map(|c| c.min(20)),
    };

    let query_text = payload.query;
    let db = state.db.clone();
    let embedder = state.embedder.clone();

    let search_res =
        tokio::task::spawn_blocking(move || -> Result<Vec<crate::storage::db::SearchResult>> {
            let embedding = embedder.embed(&query_text)?;
            db.search_chunks_hybrid(&query_text, &embedding, &options)
        })
        .await;

    let hits = match search_res {
        Ok(Ok(hits)) => hits,
        Ok(Err(e)) => {
            eprintln!("Query execution error: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Search execution failed: {}", e),
            );
        }
        Err(e) => {
            eprintln!("Query worker join error: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Search worker failed unexpectedly",
            );
        }
    };

    // Record search hits non-blockingly for frequency ranking
    let hit_file_ids: Vec<i64> = hits.iter().map(|h| h.file_id).collect();
    if !hit_file_ids.is_empty() {
        let hit_db = state.db.clone();
        tokio::spawn(async move {
            let _ =
                tokio::task::spawn_blocking(move || hit_db.record_search_hits(&hit_file_ids)).await;
        });
    }

    let results = hits
        .into_iter()
        .map(|r| QueryResult {
            content: r.content,
            score: r.score,
            file_path: Some(r.file_path),
            file_type: Some(r.file_type),
            last_modified: Some(r.last_modified),
            context_before: r.context_before,
            context_after: r.context_after,
            line_start: r.line_start,
            line_end: r.line_end,
        })
        .collect();

    Json(QueryResponse { results }).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::ChunkInput;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct MockEmbedder {
        fail: bool,
    }

    impl TextEmbedder for MockEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            if self.fail {
                return Err(anyhow!("mock embedder failure"));
            }
            let mut v = vec![0.0_f32; 384];
            let idx = text.len() % 384;
            v[idx] = 1.0;
            Ok(v)
        }
        fn embedding_dim(&self) -> usize {
            384
        }
        fn model_type(&self) -> &str {
            "all-minilm-l6-v2"
        }
    }

    fn make_state(auth_token: Option<&str>, fail_embed: bool) -> AppState {
        let db = Database::new(":memory:").unwrap();
        let mut emb = vec![0.0_f32; 384];
        emb["auth".len() % 384] = 1.0;
        db.index_file_chunks(
            "/repo/src/auth.rs",
            1_700_000_000,
            1_700_000_000_000_000_000,
            40,
            "hash1",
            &[ChunkInput {
                start: 0,
                end: 40,
                content: "pub fn verify_auth_token() -> bool { true }",
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();

        AppState {
            db: Arc::new(db),
            embedder: Arc::new(MockEmbedder { fail: fail_embed }),
            start_time: Instant::now(),
            auth_token: auth_token.map(|s| Arc::new(s.to_string())),
        }
    }

    #[tokio::test]
    async fn test_dns_rebinding_host_header_rejected() {
        let app = build_router(make_state(None, false));
        let req = Request::builder()
            .uri("/health")
            .header(header::HOST, "evil.attacker.com:3030")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_bearer_auth_enforced_and_local_health_allowed() {
        let app = build_router(make_state(Some("secret-token-123"), false));

        // Unauthorized status request fails
        let req = Request::builder()
            .uri("/status")
            .header(header::HOST, "localhost:3030")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Authorized status request succeeds even with non-loopback Host
        let req = Request::builder()
            .uri("/status")
            .header(header::HOST, "contextd.internal:3030")
            .header(header::AUTHORIZATION, "Bearer secret-token-123")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Local healthcheck works without token
        let req = Request::builder()
            .uri("/health")
            .header(header::HOST, "localhost:3030")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_query_returns_500_on_backend_error_never_200() {
        let app = build_router(make_state(None, true));
        let req = Request::builder()
            .method("POST")
            .uri("/query")
            .header(header::HOST, "127.0.0.1:3030")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"query":"auth","limit":5}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_query_hybrid_success_and_body_limit() {
        let app = build_router(make_state(None, false));

        // Successful hybrid query
        let req = Request::builder()
            .method("POST")
            .uri("/query")
            .header(header::HOST, "127.0.0.1:3030")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"query":"auth","limit":500}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let parsed: QueryResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(
            parsed.results[0].file_path.as_deref(),
            Some("/repo/src/auth.rs")
        );

        // Oversized body (> 64 KiB) rejected with 413 Payload Too Large
        let huge = "a".repeat(70 * 1024);
        let req = Request::builder()
            .method("POST")
            .uri("/query")
            .header(header::HOST, "127.0.0.1:3030")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"query":"{}"}}"#, huge)))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_refuses_non_loopback_without_auth_token() {
        let res = bind_server("0.0.0.0", 0, None).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("auth token"));
    }
}
