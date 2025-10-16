use anyhow::Result;
use chrono::Local;

use super::mcp_client::{McpClient, ProcessInfo, ServerStatus};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

pub struct App {
    pub should_quit: bool,
    pub connection_state: ConnectionState,
    pub mcp_url: String,
    pub server_status: Option<ServerStatus>,
    pub selected_process_index: Option<usize>,
    pub logs: String,
    pub status_message: String,
    pub last_update: Option<chrono::DateTime<Local>>,
}

impl App {
    pub fn new(mcp_url: String) -> Self {
        Self {
            should_quit: false,
            connection_state: ConnectionState::Disconnected,
            mcp_url,
            server_status: None,
            selected_process_index: None,
            logs: String::new(),
            status_message: String::new(),
            last_update: None,
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn select_next_process(&mut self) {
        if let Some(ref status) = self.server_status {
            if status.processes.is_empty() {
                return;
            }

            let len = status.processes.len();
            self.selected_process_index = Some(match self.selected_process_index {
                Some(i) => (i + 1) % len,
                None => 0,
            });
        }
    }

    pub fn select_previous_process(&mut self) {
        if let Some(ref status) = self.server_status {
            if status.processes.is_empty() {
                return;
            }

            let len = status.processes.len();
            self.selected_process_index = Some(match self.selected_process_index {
                Some(i) => {
                    if i == 0 {
                        len - 1
                    } else {
                        i - 1
                    }
                }
                None => len - 1,
            });
        }
    }

    pub fn get_selected_process(&self) -> Option<&ProcessInfo> {
        if let (Some(ref status), Some(index)) = (&self.server_status, self.selected_process_index)
        {
            status.processes.get(index)
        } else {
            None
        }
    }

    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    pub async fn update_status(&mut self, client: &mut McpClient) -> Result<()> {
        self.connection_state = ConnectionState::Connecting;

        match client.get_status().await {
            Ok(status) => {
                // Adjust selected index if needed
                if let Some(index) = self.selected_process_index {
                    if index >= status.processes.len() {
                        self.selected_process_index = if status.processes.is_empty() {
                            None
                        } else {
                            Some(status.processes.len() - 1)
                        };
                    }
                }

                self.server_status = Some(status);
                self.connection_state = ConnectionState::Connected;
                self.last_update = Some(Local::now());
                Ok(())
            }
            Err(e) => {
                self.connection_state = ConnectionState::Error;
                self.status_message = format!("Error: {}", e);
                Err(e)
            }
        }
    }

    pub async fn refresh_logs(&mut self, client: &mut McpClient) -> Result<()> {
        if let Some(process) = self.get_selected_process() {
            match client.search_logs(&process.name, Some(100)).await {
                Ok(logs) => {
                    self.logs = logs;
                    Ok(())
                }
                Err(e) => {
                    self.status_message = format!("Error fetching logs: {}", e);
                    Err(e)
                }
            }
        } else {
            Ok(())
        }
    }

    pub async fn restart_selected_process(&mut self, client: &mut McpClient) -> Result<()> {
        if let Some(process) = self.get_selected_process() {
            let process_name = process.name.clone();
            self.status_message = format!("Restarting {}...", process_name);

            match client.restart_process(&process_name).await {
                Ok(msg) => {
                    self.status_message = msg;
                    // Refresh status immediately
                    let _ = self.update_status(client).await;
                    Ok(())
                }
                Err(e) => {
                    self.status_message = format!("Error restarting {}: {}", process_name, e);
                    Err(e)
                }
            }
        } else {
            self.status_message = "No process selected".to_string();
            Ok(())
        }
    }

    pub fn get_process_counts(&self) -> (usize, usize, usize) {
        if let Some(ref status) = self.server_status {
            let running = status
                .processes
                .iter()
                .filter(|p| p.state.to_lowercase().contains("running"))
                .count();
            let stopped = status
                .processes
                .iter()
                .filter(|p| {
                    let state = p.state.to_lowercase();
                    state.contains("stopped") || state.contains("idle")
                })
                .count();
            let errored = status
                .processes
                .iter()
                .filter(|p| p.state.to_lowercase().contains("crashed"))
                .count();
            (running, stopped, errored)
        } else {
            (0, 0, 0)
        }
    }
}
