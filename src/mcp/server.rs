use crate::config::Config;
use crate::indexer::embeddings::TextEmbedder;
use crate::storage::db::{Database, SearchOptions};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_LIMIT: usize = 100;
const MAX_MCP_LINE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
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
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
    annotations: ToolAnnotations,
}

#[derive(Serialize)]
struct ToolAnnotations {
    title: &'static str,
    #[serde(rename = "readOnlyHint")]
    read_only_hint: bool,
    #[serde(rename = "destructiveHint")]
    destructive_hint: bool,
    #[serde(rename = "idempotentHint")]
    idempotent_hint: bool,
    #[serde(rename = "openWorldHint")]
    open_world_hint: bool,
}

#[derive(Serialize)]
struct ListToolsResult {
    tools: Vec<Tool>,
}

#[derive(Serialize)]
struct Content {
    #[serde(rename = "type")]
    kind: &'static str,
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
    embedder: Arc<dyn TextEmbedder>,
    #[allow(dead_code)]
    config: Config,
    initialized: AtomicBool,
}

impl ContextdServer {
    pub fn new(db: Database, embedder: Arc<dyn TextEmbedder>, config: Config) -> Self {
        Self {
            db,
            embedder,
            config,
            initialized: AtomicBool::new(false),
        }
    }

    /// Parse and handle a single JSON-RPC 2.0 line. Returns `Some(json_line)` for requests
    /// (and parse/validation errors), or `None` for notifications.
    pub async fn handle_raw_line(&self, line: &str) -> Option<String> {
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("MCP JSON-RPC parse error: {}", e);
                let resp = JsonRpcResponse::err(Value::Null, -32700, "Parse error");
                return serde_json::to_string(&resp).ok();
            }
        };

        let resp = self.handle_value(parsed).await?;
        serde_json::to_string(&resp).ok()
    }

    async fn handle_value(&self, value: Value) -> Option<JsonRpcResponse> {
        let Some(obj) = value.as_object() else {
            return Some(JsonRpcResponse::err(
                Value::Null,
                -32600,
                "Invalid Request: expected JSON object",
            ));
        };

        let has_id = obj.contains_key("id");
        let id = obj.get("id").cloned().unwrap_or(Value::Null);

        // Strict JSON-RPC 2.0 validation
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return if has_id {
                Some(JsonRpcResponse::err(
                    id,
                    -32600,
                    "Invalid Request: jsonrpc must be \"2.0\"",
                ))
            } else {
                Some(JsonRpcResponse::err(
                    Value::Null,
                    -32600,
                    "Invalid Request: jsonrpc must be \"2.0\"",
                ))
            };
        }

        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return Some(JsonRpcResponse::err(
                id,
                -32600,
                "Invalid Request: missing string method",
            ));
        };

        let params = obj.get("params").cloned();

        // Distinguish omitted `id` (notification) from explicit `"id": null` (request)
        if !has_id {
            match method {
                "notifications/initialized" | "initialized" => {
                    self.initialized.store(true, Ordering::SeqCst);
                    eprintln!("MCP notification: initialized");
                }
                "notifications/cancelled" => {
                    let req_id = params
                        .as_ref()
                        .and_then(|p| p.get("requestId"))
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    eprintln!("MCP notification: cancelled request {}", req_id);
                }
                _ => {
                    eprintln!("MCP notification ignored: {}", method);
                }
            }
            return None;
        }

        // Validate id type per JSON-RPC 2.0 (string, number, or null)
        if !(id.is_string() || id.is_number() || id.is_null()) {
            return Some(JsonRpcResponse::err(
                Value::Null,
                -32600,
                "Invalid Request: id must be string, number, or null",
            ));
        }

        let result = match method {
            "initialize" => self.handle_initialize(params),
            "ping" => Ok(json!({})),
            "tools/list" => {
                if !self.initialized.load(Ordering::SeqCst) {
                    Err(JsonRpcError {
                        code: -32002,
                        message: "Server not initialized. Call initialize first.".to_string(),
                    })
                } else {
                    self.handle_tools_list()
                }
            }
            "tools/call" => {
                if !self.initialized.load(Ordering::SeqCst) {
                    Err(JsonRpcError {
                        code: -32002,
                        message: "Server not initialized. Call initialize first.".to_string(),
                    })
                } else {
                    self.handle_tools_call(params).await
                }
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", method),
            }),
        };

        match result {
            Ok(val) => Some(JsonRpcResponse::ok(id, val)),
            Err(err) => Some(JsonRpcResponse::err(id, err.code, err.message)),
        }
    }

    fn handle_initialize(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let requested_version = params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(Value::as_str);

        let negotiated = match requested_version {
            Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v.to_string(),
            _ => LATEST_PROTOCOL_VERSION.to_string(),
        };

        self.initialized.store(true, Ordering::SeqCst);
        eprintln!("MCP initialized (protocolVersion={})", negotiated);

        let mut caps = serde_json::Map::new();
        caps.insert("tools".to_string(), json!({ "listChanged": false }));

        serde_json::to_value(InitializeResult {
            protocol_version: negotiated,
            capabilities: caps,
            server_info: ServerInfo {
                name: "contextd",
                version: env!("CARGO_PKG_VERSION"),
            },
            instructions: "contextd provides local hybrid semantic and keyword search over your indexed codebase. Use search_context to find relevant code and documentation.".to_string(),
        })
        .map_err(|e| JsonRpcError {
            code: -32603,
            message: format!("Serialization error: {}", e),
        })
    }

    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        serde_json::to_value(ListToolsResult {
            tools: vec![
                Tool {
                    name: "search_context",
                    description: "Hybrid semantic + keyword search over your local indexed codebase. Finds relevant functions, classes, documentation, and snippets.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The search query"
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 100,
                                "description": "Max results (default 5, max 100)"
                            },
                            "file_types": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Filter by file extensions (e.g. [\"rs\", \"py\"])"
                            },
                            "paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Filter by file path substrings"
                            },
                            "min_score": {
                                "type": "number",
                                "minimum": 0.0,
                                "maximum": 1.0,
                                "description": "Minimum relevance score (0.0-1.0)"
                            },
                            "start_time": {
                                "type": "integer",
                                "description": "Filter files modified at or after this UNIX timestamp (seconds)"
                            },
                            "end_time": {
                                "type": "integer",
                                "description": "Filter files modified at or before this UNIX timestamp (seconds)"
                            },
                            "context_lines": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 20,
                                "description": "Lines of surrounding source context to include"
                            }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }),
                    annotations: ToolAnnotations {
                        title: "Search Context",
                        read_only_hint: true,
                        destructive_hint: false,
                        idempotent_hint: true,
                        open_world_hint: false,
                    },
                },
                Tool {
                    name: "get_status",
                    description: "Get current indexing status: number of indexed files, total chunks, and database size.",
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                    annotations: ToolAnnotations {
                        title: "Get Status",
                        read_only_hint: true,
                        destructive_hint: false,
                        idempotent_hint: true,
                        open_world_hint: false,
                    },
                },
            ],
        })
        .map_err(|e| JsonRpcError {
            code: -32603,
            message: format!("Serialization error: {}", e),
        })
    }

    async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let Some(params_obj) = params.as_ref().and_then(Value::as_object) else {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid params: expected object".to_string(),
            });
        };

        let Some(name) = params_obj.get("name").and_then(Value::as_str) else {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid params: missing string 'name'".to_string(),
            });
        };

        let args_val = params_obj
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let Some(args) = args_val.as_object() else {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid params: 'arguments' must be an object".to_string(),
            });
        };

        match name {
            "search_context" => self.call_search_context(args).await,
            "get_status" => self.call_get_status(args).await,
            _ => Err(JsonRpcError {
                code: -32602,
                message: format!("Unknown tool: {}", name),
            }),
        }
    }

    async fn call_search_context(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<Value, JsonRpcError> {
        const ALLOWED_KEYS: &[&str] = &[
            "query",
            "limit",
            "file_types",
            "paths",
            "min_score",
            "start_time",
            "end_time",
            "context_lines",
        ];

        for key in args.keys() {
            if !ALLOWED_KEYS.contains(&key.as_str()) {
                return Err(JsonRpcError {
                    code: -32602,
                    message: format!("Unexpected argument '{}' for search_context", key),
                });
            }
        }

        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid params: 'query' must be a string".to_string(),
            });
        };

        let limit = match args.get("limit") {
            None => 5,
            Some(v) => match v.as_u64() {
                Some(n) => (n as usize).min(MAX_LIMIT),
                None => {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: 'limit' must be a non-negative integer"
                            .to_string(),
                    })
                }
            },
        };

        let min_score = match args.get("min_score") {
            None => None,
            Some(v) => match v.as_f64() {
                Some(f) if f.is_finite() && (0.0..=1.0).contains(&f) => Some(f as f32),
                _ => {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: 'min_score' must be a number in [0.0, 1.0]"
                            .to_string(),
                    })
                }
            },
        };

        let file_types = parse_string_array_arg(args, "file_types")?;
        let paths = parse_string_array_arg(args, "paths")?;
        let start_time = parse_u64_arg(args, "start_time")?;
        let end_time = parse_u64_arg(args, "end_time")?;
        let context_lines = parse_u64_arg(args, "context_lines")?.map(|n| (n as usize).min(20));

        let options = SearchOptions {
            limit: Some(limit),
            start_time,
            end_time,
            file_types,
            paths,
            min_score,
            recency_weight: None,
            frequency_weight: None,
            context_lines,
        };

        let query_owned = query.to_string();
        let db = self.db.clone();
        let embedder = self.embedder.clone();

        let search_res = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let embedding = embedder.embed(&query_owned)?;
            db.search_chunks_hybrid(&query_owned, &embedding, &options)
        })
        .await;

        let tool_result = match search_res {
            Ok(Ok(hits)) => {
                // Record search hits non-blockingly for frequency ranking
                let hit_file_ids: Vec<i64> = hits.iter().map(|h| h.file_id).collect();
                if !hit_file_ids.is_empty() {
                    let hit_db = self.db.clone();
                    tokio::spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            hit_db.record_search_hits(&hit_file_ids)
                        })
                        .await;
                    });
                }

                let mut text = String::new();
                for hit in hits {
                    text.push_str(&format!(
                        "File: {}\nScore: {:.3}\n",
                        hit.file_path, hit.score
                    ));
                    if let (Some(ls), Some(le)) = (hit.line_start, hit.line_end) {
                        text.push_str(&format!("Lines: {}-{}\n", ls, le));
                    }
                    text.push('\n');
                    if let Some(before) = &hit.context_before {
                        text.push_str(before);
                        text.push('\n');
                    }
                    text.push_str(&hit.content);
                    if let Some(after) = &hit.context_after {
                        text.push('\n');
                        text.push_str(after);
                    }
                    text.push_str("\n\n---\n\n");
                }
                if text.is_empty() {
                    text = "No results found.".to_string();
                }

                CallToolResult {
                    content: vec![Content { kind: "text", text }],
                    is_error: false,
                }
            }
            Ok(Err(e)) => CallToolResult {
                content: vec![Content {
                    kind: "text",
                    text: format!("Search execution failed: {}", e),
                }],
                is_error: true,
            },
            Err(e) => CallToolResult {
                content: vec![Content {
                    kind: "text",
                    text: format!("Search worker failed: {}", e),
                }],
                is_error: true,
            },
        };

        serde_json::to_value(tool_result).map_err(|e| JsonRpcError {
            code: -32603,
            message: format!("Serialization error: {}", e),
        })
    }

    async fn call_get_status(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<Value, JsonRpcError> {
        if !args.is_empty() {
            return Err(JsonRpcError {
                code: -32602,
                message: "get_status accepts no arguments".to_string(),
            });
        }

        let db = self.db.clone();
        let stats_res = tokio::task::spawn_blocking(move || db.get_stats()).await;

        let tool_result = match stats_res {
            Ok(Ok(stats)) => CallToolResult {
                content: vec![Content {
                    kind: "text",
                    text: format!(
                        "Indexed Files: {}\nTotal Chunks: {}\nDatabase Size: {:.2} MB",
                        stats.file_count,
                        stats.chunk_count,
                        stats.db_size as f64 / 1024.0 / 1024.0
                    ),
                }],
                is_error: false,
            },
            Ok(Err(e)) => CallToolResult {
                content: vec![Content {
                    kind: "text",
                    text: format!("Failed to get stats: {}", e),
                }],
                is_error: true,
            },
            Err(e) => CallToolResult {
                content: vec![Content {
                    kind: "text",
                    text: format!("Stats worker failed: {}", e),
                }],
                is_error: true,
            },
        };

        serde_json::to_value(tool_result).map_err(|e| JsonRpcError {
            code: -32603,
            message: format!("Serialization error: {}", e),
        })
    }
}

fn parse_string_array_arg(
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<Vec<String>>, JsonRpcError> {
    let Some(val) = args.get(key) else {
        return Ok(None);
    };
    let Some(arr) = val.as_array() else {
        return Err(JsonRpcError {
            code: -32602,
            message: format!("Invalid params: '{}' must be an array of strings", key),
        });
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(s) = item.as_str() else {
            return Err(JsonRpcError {
                code: -32602,
                message: format!("Invalid params: '{}' items must be strings", key),
            });
        };
        out.push(s.to_string());
    }
    Ok(Some(out))
}

fn parse_u64_arg(
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<u64>, JsonRpcError> {
    let Some(val) = args.get(key) else {
        return Ok(None);
    };
    match val.as_u64() {
        Some(n) => Ok(Some(n)),
        None => Err(JsonRpcError {
            code: -32602,
            message: format!("Invalid params: '{}' must be a non-negative integer", key),
        }),
    }
}

/// Run the MCP server over stdio using a single unified async stdout writer.
pub async fn run_mcp_server(db: Database, embedder: Arc<dyn TextEmbedder>, config: Config) {
    let server = ContextdServer::new(db, embedder, config);
    eprintln!(
        "contextd MCP server v{} starting on stdio...",
        env!("CARGO_PKG_VERSION")
    );

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();

    loop {
        // Read at most one byte over the cap. An oversized frame is discarded
        // up to its newline, so a client cannot exhaust memory with one line.
        let mut buf = Vec::new();
        let mut limited = (&mut reader).take(MAX_MCP_LINE_BYTES + 1);
        let read = limited.read_until(b'\n', &mut buf).await.unwrap_or(0);
        drop(limited);
        if read == 0 {
            break;
        }
        if !buf.ends_with(b"\n") {
            let mut discarded = Vec::new();
            let _ = reader.read_until(b'\n', &mut discarded).await;
            eprintln!("Dropping oversized MCP request line");
            continue;
        }
        buf.pop();
        let Ok(line) = std::str::from_utf8(&buf) else {
            eprintln!("Dropping MCP request line that is not valid UTF-8");
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }

        if let Some(mut resp_json) = server.handle_raw_line(line).await {
            resp_json.push('\n');
            if let Err(e) = stdout.write_all(resp_json.as_bytes()).await {
                if e.kind() == ErrorKind::BrokenPipe {
                    eprintln!("MCP stdout pipe closed, exiting cleanly.");
                    return;
                }
                eprintln!("MCP stdout write error: {}", e);
                return;
            }
            if let Err(e) = stdout.flush().await {
                if e.kind() == ErrorKind::BrokenPipe {
                    eprintln!("MCP stdout pipe closed, exiting cleanly.");
                    return;
                }
                eprintln!("MCP stdout flush error: {}", e);
                return;
            }
        }
    }

    eprintln!("MCP server stdin closed, exiting.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::ChunkInput;

    struct MockEmbedder {
        fail: bool,
    }

    impl TextEmbedder for MockEmbedder {
        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            if self.fail {
                return Err(anyhow::anyhow!("inference error"));
            }
            let mut v = vec![0.0_f32; 384];
            v[text.len() % 384] = 1.0;
            Ok(v)
        }
        fn embedding_dim(&self) -> usize {
            384
        }
        fn model_type(&self) -> &str {
            "all-minilm-l6-v2"
        }
    }

    fn make_server(fail_embed: bool) -> ContextdServer {
        let db = Database::new(":memory:").unwrap();
        let mut emb = vec![0.0_f32; 384];
        emb["jwt auth".len() % 384] = 1.0;
        db.index_file_chunks(
            "/src/auth.rs",
            1_700_000_000,
            1_700_000_000_000_000_000,
            30,
            "hash_jwt",
            &[ChunkInput {
                start: 0,
                end: 30,
                content: "fn validate_jwt_auth() -> bool { true }",
                embedding: Some(&emb),
                metadata: None,
            }],
        )
        .unwrap();
        ContextdServer::new(
            db,
            Arc::new(MockEmbedder { fail: fail_embed }),
            Config::default(),
        )
    }

    #[tokio::test]
    async fn test_jsonrpc_exclusivity_lifecycle_and_version_negotiation() {
        let srv = make_server(false);

        // Calling tools/list before initialize is gated (-32002)
        let raw = srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("result").is_none(), "Error response must omit result");
        assert_eq!(v["error"]["code"], -32002);

        // Ping works anytime
        let raw = srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("error").is_none(), "Success response must omit error");
        assert_eq!(v["result"], json!({}));

        // Initialize negotiates supported protocol version and reports Cargo version
        let raw = srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(
            v["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(v["result"]["capabilities"]["tools"]["listChanged"], false);

        // Notifications return None (no response)
        assert!(srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await
            .is_none());
        assert!(srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}"#
            )
            .await
            .is_none());

        // tools/list returns openWorldHint: false
        let raw = srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        for t in tools {
            assert_eq!(t["annotations"]["openWorldHint"], false);
        }
    }

    #[tokio::test]
    async fn test_mcp_validation_hybrid_search_and_execution_errors() {
        let srv = make_server(false);
        let _ = srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
            .await;

        // Invalid jsonrpc version rejected (-32600)
        let raw = srv
            .handle_raw_line(r#"{"jsonrpc":"1.0","id":2,"method":"ping"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["error"]["code"], -32600);

        // Explicit id: null is treated as request, not notification
        let raw = srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["result"], json!({}));

        // Missing required query -> -32602
        let raw = srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_context","arguments":{}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["error"]["code"], -32602);

        // Unknown additional property -> -32602
        let raw = srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_context","arguments":{"query":"auth","bogus":1}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["error"]["code"], -32602);

        // Unknown tool -> -32602
        let raw = srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"no_such_tool","arguments":{}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["error"]["code"], -32602);

        // Valid hybrid search with paths filter
        let raw = srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"search_context","arguments":{"query":"jwt auth","paths":["/src/"]}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["result"]["isError"], false);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("validate_jwt_auth"));

        // Tool execution error returns CallToolResult { isError: true }
        let failing_srv = make_server(true);
        let _ = failing_srv
            .handle_raw_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
            .await;
        let raw = failing_srv
            .handle_raw_line(
                r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"search_context","arguments":{"query":"jwt"}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["isError"], true);
    }
}
