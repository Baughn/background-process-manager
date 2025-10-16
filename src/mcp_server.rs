use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

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

pub struct McpServer {
    config: Config,
    processes: Arc<RwLock<HashMap<String, Arc<ProcessManager>>>>,
    builder: Arc<Builder>,
    mode_manager: Arc<ModeManager>,
    crash_handlers: Arc<RwLock<HashMap<String, CrashHandler>>>,
}

impl McpServer {
    pub fn new(
        config: Config,
        processes: Arc<RwLock<HashMap<String, Arc<ProcessManager>>>>,
        builder: Arc<Builder>,
        mode_manager: Arc<ModeManager>,
        crash_handlers: Arc<RwLock<HashMap<String, CrashHandler>>>,
    ) -> Self {
        Self {
            config,
            processes,
            builder,
            mode_manager,
            crash_handlers,
        }
    }

    pub async fn start(&self, port: u16) -> Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .context("Failed to bind to port")?;

        info!("MCP server listening on port {}", port);

        loop {
            let (stream, addr) = listener.accept().await?;
            info!("New connection from {}", addr);

            let server = self.clone_for_connection();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(stream).await {
                    error!("Error handling connection: {}", e);
                }
            });
        }
    }

    fn clone_for_connection(&self) -> Self {
        Self {
            config: self.config.clone(),
            processes: self.processes.clone(),
            builder: self.builder.clone(),
            mode_manager: self.mode_manager.clone(),
            crash_handlers: self.crash_handlers.clone(),
        }
    }

    async fn handle_connection(&self, stream: TcpStream) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(req) => req,
                Err(e) => {
                    error!("Failed to parse request: {}", e);
                    continue;
                }
            };

            let response = self.handle_request(request).await;

            let response_json = serde_json::to_string(&response)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }

        Ok(())
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
                    "description": "Search process logs with optional regex pattern, context lines, and head/tail limiting",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "process": {
                                "type": "string",
                                "description": "Process name"
                            },
                            "pattern": {
                                "type": "string",
                                "description": "Optional regex pattern to search for"
                            },
                            "context_lines": {
                                "type": "number",
                                "description": "Number of context lines around matches"
                            },
                            "head": {
                                "type": "number",
                                "description": "Return only first N lines"
                            },
                            "tail": {
                                "type": "number",
                                "description": "Return only last N lines"
                            },
                            "index": {
                                "type": "number",
                                "description": "Log instance index (negative for recent, e.g. -1 = most recent)"
                            }
                        },
                        "required": ["process"]
                    }
                },
                {
                    "name": "search_build_log",
                    "description": "Search build logs with optional regex pattern, context lines, and head/tail limiting",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "process": {
                                "type": "string",
                                "description": "Process name"
                            },
                            "pattern": {
                                "type": "string",
                                "description": "Optional regex pattern to search for"
                            },
                            "context_lines": {
                                "type": "number",
                                "description": "Number of context lines around matches"
                            },
                            "head": {
                                "type": "number",
                                "description": "Return only first N lines"
                            },
                            "tail": {
                                "type": "number",
                                "description": "Return only last N lines"
                            },
                            "index": {
                                "type": "number",
                                "description": "Log instance index (negative for recent, e.g. -1 = most recent)"
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

        // Switch back to dev mode on restart
        self.mode_manager.switch_to_dev().await;
        let mode = self.mode_manager.get_mode().await;

        // Stop the current process
        process.stop().await?;

        // Build if Rust
        match process.config.process_type {
            ProcessType::Rust => {
                let release = matches!(mode, RunMode::Release);
                let binary_path = self
                    .builder
                    .build_rust(release, process.build_logs.clone())
                    .await?;

                process.spawn_process(binary_path).await?;
            }
            ProcessType::Npm => {
                process.spawn_npm_process().await?;
            }
        }

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
