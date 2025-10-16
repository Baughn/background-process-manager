use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct ServerStatus {
    pub mode: String,
    pub time_until_release: Option<String>,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub state: String,
    pub uptime: Option<String>,
    pub events: Vec<String>,
    pub crash_count: u32,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    id: u64,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub struct McpClient {
    url: String,
    client: reqwest::Client,
    next_id: u64,
}

impl McpClient {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
            next_id: 1,
        }
    }

    fn get_next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.get_next_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let response = self
            .client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request")?;

        let json_response: JsonRpcResponse = response
            .json()
            .await
            .context("Failed to parse response")?;

        if let Some(error) = json_response.error {
            anyhow::bail!("MCP error {}: {}", error.code, error.message);
        }

        json_response
            .result
            .context("No result in response")
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.send_request("initialize", Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "bpm-tui",
                "version": "0.1.0"
            }
        })))
        .await?;
        Ok(())
    }

    pub async fn get_status(&mut self) -> Result<ServerStatus> {
        let result = self
            .send_request(
                "tools/call",
                Some(json!({
                    "name": "get_status",
                    "arguments": {}
                })),
            )
            .await?;

        // Extract the text content from the response
        let text = result["content"][0]["text"]
            .as_str()
            .context("Invalid response format")?;

        // Parse the text response into structured data
        self.parse_status_text(text)
    }

    fn parse_status_text(&self, text: &str) -> Result<ServerStatus> {
        let mut lines = text.lines();
        let mut mode = String::new();
        let mut time_until_release = None;
        let mut processes = Vec::new();

        while let Some(line) = lines.next() {
            if line.starts_with("Mode:") {
                mode = line
                    .trim_start_matches("Mode:")
                    .trim()
                    .to_string();
            } else if line.starts_with("Time until release mode:") {
                time_until_release = Some(
                    line.trim_start_matches("Time until release mode:")
                        .trim()
                        .to_string(),
                );
            } else if line.starts_with("  ") && line.contains(':') && !line.contains("Uptime") && !line.contains("Recent events") && !line.contains("Crash count") {
                // This is a process line
                let parts: Vec<&str> = line.trim().splitn(2, ':').collect();
                if parts.len() == 2 {
                    let name = parts[0].trim().to_string();
                    let state = parts[1].trim().to_string();
                    let mut uptime = None;
                    let mut events = Vec::new();
                    let mut crash_count = 0;

                    // Read additional process info
                    while let Some(info_line) = lines.next() {
                        if info_line.trim().is_empty() {
                            break;
                        }
                        if info_line.starts_with("  ") && !info_line.starts_with("    ") {
                            // Next process
                            break;
                        }
                        if info_line.contains("Uptime:") {
                            uptime = Some(
                                info_line
                                    .trim()
                                    .trim_start_matches("Uptime:")
                                    .trim()
                                    .to_string(),
                            );
                        } else if info_line.contains("Recent events:") {
                            // Continue reading events
                            while let Some(event_line) = lines.next() {
                                if event_line.starts_with("      - ") {
                                    events.push(
                                        event_line
                                            .trim()
                                            .trim_start_matches("- ")
                                            .to_string(),
                                    );
                                } else {
                                    break;
                                }
                            }
                        } else if info_line.contains("Crash count:") {
                            if let Some(count_str) = info_line.trim().trim_start_matches("Crash count:").trim().parse().ok() {
                                crash_count = count_str;
                            }
                        }
                    }

                    processes.push(ProcessInfo {
                        name,
                        state,
                        uptime,
                        events,
                        crash_count,
                    });
                }
            }
        }

        Ok(ServerStatus {
            mode,
            time_until_release,
            processes,
        })
    }

    pub async fn search_logs(&mut self, process: &str, tail: Option<usize>) -> Result<String> {
        let mut args = json!({
            "process": process
        });

        if let Some(tail_count) = tail {
            args["tail"] = json!(tail_count);
        }

        let result = self
            .send_request(
                "tools/call",
                Some(json!({
                    "name": "search_logs",
                    "arguments": args
                })),
            )
            .await?;

        let text = result["content"][0]["text"]
            .as_str()
            .context("Invalid response format")?;

        Ok(text.to_string())
    }

    pub async fn restart_process(&mut self, process: &str) -> Result<String> {
        let result = self
            .send_request(
                "tools/call",
                Some(json!({
                    "name": "restart",
                    "arguments": {
                        "process": process
                    }
                })),
            )
            .await?;

        let text = result["content"][0]["text"]
            .as_str()
            .context("Invalid response format")?;

        Ok(text.to_string())
    }
}
