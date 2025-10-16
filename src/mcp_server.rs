use anyhow::{Context, Result};
use axum::{
    extract::{Json, State},
    http::{header, Method},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::post,
    Router,
};
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

use crate::builder::Builder;
use crate::config::{Config, ProcessType};
use crate::crash_handler::{CrashHandler, RunMode};
use crate::mode::ModeManager;
use crate::process::ProcessManager;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
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

#[derive(Clone)]
pub struct AppState {
    processes: Arc<RwLock<HashMap<String, Arc<ProcessManager>>>>,
    builder: Arc<Builder>,
    mode_manager: Arc<ModeManager>,
    crash_handlers: Arc<RwLock<HashMap<String, CrashHandler>>>,
}

impl AppState {
    pub fn new(
        _config: Config,
        processes: Arc<RwLock<HashMap<String, Arc<ProcessManager>>>>,
        builder: Arc<Builder>,
        mode_manager: Arc<ModeManager>,
        crash_handlers: Arc<RwLock<HashMap<String, CrashHandler>>>,
    ) -> Self {
        Self {
            processes,
            builder,
            mode_manager,
            crash_handlers,
        }
    }

    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone().unwrap_or(Value::Null);

        match request.method.as_str() {
            "initialize" => {
                info!("Received initialize request");
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "background-process-manager",
                            "version": "0.1.0"
                        }
                    })),
                    error: None,
                }
            }
            "tools/list" => {
                info!("Received tools/list request");
                self.handle_list_tools(id).await
            }
            "tools/call" => {
                info!("Received tools/call request");
                self.mode_manager.record_tool_call().await;
                self.handle_tool_call(id, request.params).await
            }
            _ => {
                warn!("Unknown method: {}", request.method);
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Method not found: {}", request.method),
                    }),
                }
            }
        }
    }

    async fn handle_list_tools(&self, id: Value) -> JsonRpcResponse {
        let tools = json!({
            "tools": [
                {
                    "name": "search_logs",
                    "description": "Search process logs with optional regex pattern, context lines, and head/tail limiting. Execution order: pattern matching → context expansion → head/tail limiting",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "process": {
                                "type": "string",
                                "description": "Process name"
                            },
                            "pattern": {
                                "type": "string",
                                "description": "Optional regex pattern (Rust regex syntax, case-sensitive). Examples: 'ERROR|WARN' (OR), 'started.*server' (wildcards), '\\\\d{3}' (digits). Matched lines prefixed with ' * ', context lines with '   '"
                            },
                            "context_lines": {
                                "type": "number",
                                "description": "Number of lines to show before and after each match. Only applies when pattern is provided"
                            },
                            "head": {
                                "type": "number",
                                "description": "Return only first N lines (applied after pattern/context). Mutually exclusive with tail"
                            },
                            "tail": {
                                "type": "number",
                                "description": "Return only last N lines (applied after pattern/context). Takes precedence over head if both specified"
                            },
                            "index": {
                                "type": "number",
                                "description": "Log instance index. Negative = recent (-1 most recent, -2 second-to-last), positive = absolute (0 first, 1 second). Default: -1"
                            }
                        },
                        "required": ["process"]
                    }
                },
                {
                    "name": "search_build_log",
                    "description": "Search build logs with optional regex pattern, context lines, and head/tail limiting. Execution order: pattern matching → context expansion → head/tail limiting",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "process": {
                                "type": "string",
                                "description": "Process name"
                            },
                            "pattern": {
                                "type": "string",
                                "description": "Optional regex pattern (Rust regex syntax, case-sensitive). Examples: 'ERROR|WARN' (OR), 'started.*server' (wildcards), '\\\\d{3}' (digits). Matched lines prefixed with ' * ', context lines with '   '"
                            },
                            "context_lines": {
                                "type": "number",
                                "description": "Number of lines to show before and after each match. Only applies when pattern is provided"
                            },
                            "head": {
                                "type": "number",
                                "description": "Return only first N lines (applied after pattern/context). Mutually exclusive with tail"
                            },
                            "tail": {
                                "type": "number",
                                "description": "Return only last N lines (applied after pattern/context). Takes precedence over head if both specified"
                            },
                            "index": {
                                "type": "number",
                                "description": "Log instance index. Negative = recent (-1 most recent, -2 second-to-last), positive = absolute (0 first, 1 second). Default: -1"
                            }
                        },
                        "required": ["process"]
                    }
                },
                {
                    "name": "restart",
                    "description": "Restart a process (builds first for Rust projects, then restarts). Switches back to dev mode.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "process": {
                                "type": "string",
                                "description": "Process name"
                            }
                        },
                        "required": ["process"]
                    }
                },
                {
                    "name": "get_status",
                    "description": "Get status of all processes including mode, uptime, state, and recent events",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ]
        });

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(tools),
            error: None,
        }
    }

    async fn handle_tool_call(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let params = match params {
            Some(p) => p,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing params".to_string(),
                    }),
                }
            }
        };

        let tool_name = params["name"].as_str().unwrap_or("");
        let arguments = &params["arguments"];

        let result = match tool_name {
            "search_logs" => self.tool_search_logs(arguments).await,
            "search_build_log" => self.tool_search_build_log(arguments).await,
            "restart" => self.tool_restart(arguments).await,
            "get_status" => self.tool_get_status().await,
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": content
                        }
                    ]
                })),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: format!("Tool execution error: {}", e),
                }),
            },
        }
    }

    async fn tool_search_logs(&self, args: &Value) -> Result<String> {
        let process_name = args["process"]
            .as_str()
            .context("Missing 'process' parameter")?;

        let pattern = args["pattern"].as_str();
        let context_lines = args["context_lines"].as_u64().map(|n| n as usize);
        let head = args["head"].as_u64().map(|n| n as usize);
        let tail = args["tail"].as_u64().map(|n| n as usize);
        let index = args["index"].as_i64().map(|n| n as i32);

        let processes = self.processes.read().await;
        let process = processes
            .get(process_name)
            .context(format!("Process '{}' not found", process_name))?;

        let results = process
            .logs
            .read()
            .await
            .search(index, pattern, context_lines, head, tail);

        Ok(results.join("\n"))
    }

    async fn tool_search_build_log(&self, args: &Value) -> Result<String> {
        let process_name = args["process"]
            .as_str()
            .context("Missing 'process' parameter")?;

        let pattern = args["pattern"].as_str();
        let context_lines = args["context_lines"].as_u64().map(|n| n as usize);
        let head = args["head"].as_u64().map(|n| n as usize);
        let tail = args["tail"].as_u64().map(|n| n as usize);
        let index = args["index"].as_i64().map(|n| n as i32);

        let processes = self.processes.read().await;
        let process = processes
            .get(process_name)
            .context(format!("Process '{}' not found", process_name))?;

        let results = process
            .build_logs
            .read()
            .await
            .search(index, pattern, context_lines, head, tail);

        Ok(results.join("\n"))
    }

    async fn tool_restart(&self, args: &Value) -> Result<String> {
        let process_name = args["process"]
            .as_str()
            .context("Missing 'process' parameter")?;

        let processes = self.processes.read().await;
        let process = processes
            .get(process_name)
            .context(format!("Process '{}' not found", process_name))?
            .clone();
        drop(processes);

        // Set manual restart flag to prevent crash monitor interference
        process.set_manual_restart_flag().await;

        // Switch back to dev mode on restart
        self.mode_manager.switch_to_dev().await;
        let mode = self.mode_manager.get_mode().await;

        // Build FIRST (while old process keeps running)
        let binary_path = match process.config.process_type {
            ProcessType::Rust => {
                let release = matches!(mode, RunMode::Release);
                Some(self
                    .builder
                    .build_rust(release, process.build_logs.clone())
                    .await?)
            }
            ProcessType::Npm => None,
        };

        // Now stop the old process
        process.stop().await?;

        // Start the new process
        match process.config.process_type {
            ProcessType::Rust => {
                if let Some(binary_path) = binary_path {
                    process.spawn_process(binary_path).await?;
                }
            }
            ProcessType::Npm => {
                process.spawn_npm_process().await?;
            }
        }

        // Clear manual restart flag
        process.clear_manual_restart_flag().await;

        // Reset crash handler
        let mut handlers = self.crash_handlers.write().await;
        if let Some(handler) = handlers.get_mut(process_name) {
            handler.reset_crash_count();
        }

        Ok(format!("Process '{}' restarted successfully in dev mode", process_name))
    }

    async fn tool_get_status(&self) -> Result<String> {
        let mode = self.mode_manager.get_mode().await;
        let time_until_release = self.mode_manager.get_time_until_release_mode().await;

        let mut status = format!("Mode: {:?}\n", mode);
        if let Some(time) = time_until_release {
            status.push_str(&format!(
                "Time until release mode: {} hours {} minutes\n",
                time.num_hours(),
                time.num_minutes() % 60
            ));
        } else {
            status.push_str("Currently in release mode\n");
        }
        status.push_str("\nProcesses:\n");

        let processes = self.processes.read().await;
        for (name, process) in processes.iter() {
            let state = process.state.read().await;
            status.push_str(&format!("\n  {}: {}\n", name, state.as_str()));

            if let Some(uptime) = process.get_uptime().await {
                status.push_str(&format!(
                    "    Uptime: {} hours {} minutes\n",
                    uptime.num_hours(),
                    uptime.num_minutes() % 60
                ));
            }

            let events = process.events.read().await;
            if !events.is_empty() {
                status.push_str("    Recent events:\n");
                for event in events.iter().rev().take(5) {
                    status.push_str(&format!("      - {}\n", event.description()));
                }
            }

            let handlers = self.crash_handlers.read().await;
            if let Some(handler) = handlers.get(name) {
                let crash_count = handler.get_crash_count();
                if crash_count > 0 {
                    status.push_str(&format!("    Crash count: {}\n", crash_count));
                }
            }
        }

        Ok(status)
    }
}

async fn handle_post(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Response {
    let response = state.handle_request(request).await;
    Json(response).into_response()
}

async fn handle_get(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // For now, we just send keep-alive events
    // In the future, this could be used for server-initiated messages
    let stream = stream::iter(vec![
        Ok(Event::default().comment("connected")),
    ]);

    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);

    Router::new()
        .route("/mcp", post(handle_post).get(handle_get))
        .layer(cors)
        .with_state(state)
}

pub async fn start_server(state: AppState, port: u16) -> Result<()> {
    let app = create_router(state).await;

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;

    info!("MCP HTTP server listening on http://{}/mcp", addr);

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}
