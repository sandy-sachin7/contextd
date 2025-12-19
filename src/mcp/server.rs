use crate::config::Config;
use crate::indexer::embeddings::Embedder;
use crate::storage::db::Database;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

// JSON-RPC Types
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: serde_json::Map<String, Value>,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
    instructions: String,
}

#[derive(Serialize)]
struct Tool {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Serialize)]
struct ListToolsResult {
    tools: Vec<Tool>,
}

#[derive(Serialize)]
struct Content {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Serialize)]
struct CallToolResult {
    content: Vec<Content>,
    #[serde(rename = "isError")]
    is_error: bool,
}

pub struct ContextdServer {
    db: Database,
    embedder: Arc<Embedder>,
    #[allow(dead_code)]
    config: Config,
}

impl ContextdServer {
    pub fn new(db: Database, embedder: Arc<Embedder>, config: Config) -> Self {
        Self {
            db,
            embedder,
            config,
        }
    }

    async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone();

        // Handle notifications (no id)
        if id.is_none() {
            if req.method == "notifications/initialized" || req.method == "initialized" {
                eprintln!("MCP initialized notification received");
            }
            return None;
        }

        let result = match req.method.as_str() {
            "initialize" => {
                eprintln!("MCP initialize request received");
                Ok(serde_json::to_value(InitializeResult {
                    protocol_version: "2024-11-05".to_string(),
                    capabilities: serde_json::Map::new(),
                    server_info: ServerInfo {
                        name: "contextd".to_string(),
                        version: "0.1.0".to_string(),
                    },
                    instructions: "contextd provides semantic search over your codebase. Use search_context to find relevant code and documentation.".to_string(),
                }).unwrap())
            }
            "tools/list" => {
                eprintln!("MCP tools/list request received");
                Ok(serde_json::to_value(ListToolsResult {
                    tools: vec![
                        Tool {
                            name: "search_context".to_string(),
                            description: "Search for relevant code and documentation using semantic search.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "The search query" },
                                    "limit": { "type": "integer", "description": "Max results (default 5)" },
                                    "file_types": { "type": "array", "items": { "type": "string" }, "description": "Filter by file extension" },
                                    "min_score": { "type": "number", "description": "Minimum similarity score (0.0-1.0)" }
                                },
                                "required": ["query"]
                            }),
                        },
                        Tool {
                            name: "get_status".to_string(),
                            description: "Get indexing status and statistics.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": {},
                            }),
                        },
                    ],
                }).unwrap())
            }
            "tools/call" => {
                eprintln!("MCP tools/call request received");
                if let Some(params) = req.params {
                    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = params
                        .get("arguments")
                        .unwrap_or(&serde_json::json!({}))
                        .clone();

                    match name {
                        "search_context" => {
                            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                            let limit =
                                args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
                            let min_score = args
                                .get("min_score")
                                .and_then(|v| v.as_f64())
                                .map(|v| v as f32);

                            // Parse file_types
                            let file_types =
                                args.get("file_types")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect::<Vec<_>>()
                                    });

                            eprintln!("Executing search: '{}' (limit: {})", query, limit);

                            // Embed query
                            let embedding_result = self.embedder.embed(query);

                            match embedding_result {
                                Ok(embedding) => {
                                    // Use existing search logic
                                    let options = crate::storage::db::SearchOptions {
                                        limit: Some(limit),
                                        min_score,
                                        file_types,
                                        paths: None,
                                        ..Default::default()
                                    };

                                    let results =
                                        self.db.search_chunks_enhanced(&embedding, &options);

                                    match results {
                                        Ok(hits) => {
                                            let mut text = String::new();
                                            for hit in hits {
                                                text.push_str(&format!(
                                                    "File: {}\nScore: {:.2}\n\n{}\n\n---\n\n",
                                                    hit.file_path, hit.score, hit.content
                                                ));
                                            }
                                            if text.is_empty() {
                                                text = "No results found.".to_string();
                                            }
                                            Ok(serde_json::to_value(CallToolResult {
                                                content: vec![Content {
                                                    kind: "text".to_string(),
                                                    text,
                                                }],
                                                is_error: false,
                                            })
                                            .unwrap())
                                        }
                                        Err(e) => Err(JsonRpcError {
                                            code: -32603,
                                            message: format!("Search failed: {}", e),
                                        }),
                                    }
                                }
                                Err(e) => Err(JsonRpcError {
                                    code: -32603,
                                    message: format!("Embedding failed: {}", e),
                                }),
                            }
                        }
                        "get_status" => match self.db.get_stats() {
                            Ok(stats) => {
                                let text = format!(
                                    "Indexed Files: {}\nTotal Chunks: {}\nDatabase Size: {:.2} MB",
                                    stats.file_count,
                                    stats.chunk_count,
                                    stats.db_size as f64 / 1024.0 / 1024.0
                                );
                                Ok(serde_json::to_value(CallToolResult {
                                    content: vec![Content {
                                        kind: "text".to_string(),
                                        text,
                                    }],
                                    is_error: false,
                                })
                                .unwrap())
                            }
                            Err(e) => Err(JsonRpcError {
                                code: -32603,
                                message: format!("Failed to get stats: {}", e),
                            }),
                        },
                        _ => Err(JsonRpcError {
                            code: -32601,
                            message: format!("Unknown tool: {}", name),
                        }),
                    }
                } else {
                    Err(JsonRpcError {
                        code: -32602,
                        message: "Missing params".to_string(),
                    })
                }
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
            }),
        };

        match result {
            Ok(val) => Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(val),
                error: None,
            }),
            Err(err) => Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(err),
            }),
        }
    }
}

/// Run the MCP server over stdio (manual implementation)
pub async fn run_mcp_server(db: Database, embedder: Arc<Embedder>, config: Config) {
    let server = ContextdServer::new(db, embedder, config);
    eprintln!("contextd MCP server starting on stdio (manual)...");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        // Parse request
        match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => {
                if let Some(resp) = server.handle_request(req).await {
                    let json = serde_json::to_string(&resp).unwrap();
                    eprintln!("Sending response: {}", json);
                    println!("{}", json);
                }
            }
            Err(e) => {
                eprintln!("Failed to parse JSON-RPC: {} (line: {})", e, line);
                // Send parse error
                let error_resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: "Parse error".to_string(),
                    }),
                };
                let json = serde_json::to_string(&error_resp).unwrap();
                let _ = stdout.write_all(format!("{}\n", json).as_bytes()).await;
                let _ = stdout.flush().await;
            }
        }
    }

    eprintln!("MCP server stdin closed, exiting.");
}
