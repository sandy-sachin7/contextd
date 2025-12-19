//! MCP Server implementation for contextd
//!
//! Exposes semantic search tools to AI assistants via the Model Context Protocol.

use crate::config::Config;
use crate::indexer::embeddings::Embedder;
use crate::storage::db::{Database, SearchOptions};
use rmcp::{
    handler::server::tool::ToolRouter,
    model::{CallToolResult, Content, ServerInfo, Implementation},
    tool, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// MCP Server for contextd semantic search
#[derive(Clone)]
pub struct ContextdServer {
    db: Arc<Mutex<Database>>,
    embedder: Arc<Embedder>,
    #[allow(dead_code)]
    config: Arc<Config>,
    #[allow(dead_code)] // Used internally by rmcp macros
    tool_router: ToolRouter<Self>,
}

impl ContextdServer {
    pub fn new(db: Database, embedder: Arc<Embedder>, config: Config) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            embedder,
            config: Arc::new(config),
            tool_router: Self::tool_router(),
        }
    }
}

// ============================================================================
// Tool Parameters
// ============================================================================

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchContextParams {
    /// The search query - can be natural language describing what you're looking for
    pub query: String,
    /// Maximum number of results to return (default: 5)
    #[serde(default)]
    pub limit: Option<usize>,
    /// Filter by file extensions (e.g., ["rs", "py", "ts"])
    #[serde(default)]
    pub file_types: Option<Vec<String>>,
    /// Minimum relevance score (0.0-1.0, default: 0.0)
    #[serde(default)]
    pub min_score: Option<f32>,
}

// ============================================================================
// Tool Implementations
// ============================================================================

#[tool_router]
impl ContextdServer {
    /// Search for relevant code or documentation using semantic similarity.
    #[tool(
        name = "search_context",
        description = "Search for relevant code or documentation by meaning. Returns semantically similar content chunks from the indexed codebase."
    )]
    async fn search_context(
        &self,
        params: rmcp::handler::server::wrapper::Parameters<SearchContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;

        let embedding = self
            .embedder
            .embed(&params.query)
            .map_err(|e| McpError::internal_error(format!("Embedding error: {}", e), None))?;

        let options = SearchOptions {
            limit: Some(params.limit.unwrap_or(5)),
            file_types: params.file_types,
            min_score: params.min_score,
            ..Default::default()
        };

        let db = self.db.lock().map_err(|e| {
            McpError::internal_error(format!("Database lock error: {}", e), None)
        })?;

        let results = db
            .search_chunks_enhanced(&embedding, &options)
            .map_err(|e| McpError::internal_error(format!("Search error: {}", e), None))?;

        // Format results as text content
        let mut content_parts = Vec::new();
        for (i, result) in results.iter().enumerate() {
            content_parts.push(Content::text(format!(
                "--- Result {} (score: {:.2}) ---\nFile: {}\n\n{}\n",
                i + 1,
                result.score,
                result.file_path,
                result.content
            )));
        }

        if content_parts.is_empty() {
            content_parts.push(Content::text("No relevant content found."));
        }

        Ok(CallToolResult::success(content_parts))
    }

    /// Get the current indexing status of contextd.
    #[tool(
        name = "get_status",
        description = "Get the current indexing status including file count, chunk count, and database size."
    )]
    async fn get_status(&self) -> Result<CallToolResult, McpError> {
        let db = self.db.lock().map_err(|e| {
            McpError::internal_error(format!("Database lock error: {}", e), None)
        })?;

        let stats = db
            .get_stats()
            .map_err(|e| McpError::internal_error(format!("Stats error: {}", e), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Indexed files: {}\nTotal chunks: {}\nDatabase size: {} bytes",
            stats.file_count, stats.chunk_count, stats.db_size
        ))]))
    }
}

// ============================================================================
// ServerHandler Implementation
// ============================================================================

impl ServerHandler for ContextdServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: Default::default(),
            server_info: Implementation {
                name: "contextd".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some("contextd provides semantic search over your codebase. Use search_context to find relevant code and documentation.".into()),
        }
    }
}

// ============================================================================
// Server Runner
// ============================================================================

/// Run the MCP server over stdio (for Claude Desktop integration)
pub async fn run_mcp_server(db: Database, embedder: Arc<Embedder>, config: Config) {
    use rmcp::transport::io::stdio;

    let server = ContextdServer::new(db, embedder, config);

    eprintln!("contextd MCP server starting on stdio...");

    let transport = stdio();

    if let Err(e) = rmcp::serve_server(server, transport).await {
        eprintln!("MCP server error: {}", e);
    }
}
