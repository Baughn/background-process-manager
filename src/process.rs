use crate::config::ProcessConfig;
use crate::log_buffer::LogBuffer;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Idle,
    Running,
    Crashed,
}

impl ProcessState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessState::Idle => "idle",
            ProcessState::Running => "running",
            ProcessState::Crashed => "crashed",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProcessEvent {
    Started { timestamp: DateTime<Utc> },
    Crashed { timestamp: DateTime<Utc>, exit_code: Option<i32> },
}

impl ProcessEvent {
    pub fn description(&self) -> String {
        match self {
            ProcessEvent::Started { timestamp } => format!("Started at {}", timestamp),
            ProcessEvent::Crashed { timestamp, exit_code } => {
                format!("Crashed at {} (exit code: {:?})", timestamp, exit_code)
            }
        }
    }
}

pub struct ProcessManager {
    pub name: String,
    pub config: ProcessConfig,
    pub project_dir: PathBuf,
    pub state: RwLock<ProcessState>,
    pub logs: Arc<RwLock<LogBuffer>>,
    pub build_logs: Arc<RwLock<LogBuffer>>,
    pub started_at: RwLock<Option<DateTime<Utc>>>,
    pub events: RwLock<Vec<ProcessEvent>>,
    child: RwLock<Option<Child>>,
    has_direnv: bool,
}

impl ProcessManager {
    pub fn new(name: String, config: ProcessConfig, project_dir: PathBuf) -> Self {
        let has_direnv = project_dir.join(".envrc").exists();

        Self {
            name,
            config,
            project_dir,
            state: RwLock::new(ProcessState::Idle),
            logs: Arc::new(RwLock::new(LogBuffer::new())),
            build_logs: Arc::new(RwLock::new(LogBuffer::new())),
            started_at: RwLock::new(None),
            events: RwLock::new(Vec::new()),
            child: RwLock::new(None),
            has_direnv,
        }
    }

    pub async fn spawn_process(&self, binary_path: PathBuf) -> Result<()> {
        info!("Spawning process: {}", self.name);

        // Create new log instance
        self.logs.write().await.new_instance();

        let mut cmd = if self.has_direnv {
            let mut c = Command::new("direnv");
            c.arg("exec").arg(&self.project_dir).arg(&binary_path);
            c
        } else {
            Command::new(&binary_path)
        };

        // Add configured arguments
        for arg in &self.config.args {
            cmd.arg(arg);
        }

        cmd.current_dir(&self.project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().context("Failed to spawn process")?;

        // Capture stdout
        if let Some(stdout) = child.stdout.take() {
            let logs = self.logs.clone();
            let name = self.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[{}] {}", name, line);
                    logs.write().await.append(line);
                }
            });
        }

        // Capture stderr
        if let Some(stderr) = child.stderr.take() {
            let logs = self.logs.clone();
            let name = self.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{}] {}", name, line);
                    logs.write().await.append(format!("[stderr] {}", line));
                }
            });
        }

        *self.child.write().await = Some(child);
        *self.state.write().await = ProcessState::Running;
        *self.started_at.write().await = Some(Utc::now());

        self.events.write().await.push(ProcessEvent::Started {
            timestamp: Utc::now(),
        });

        info!("Process {} started successfully", self.name);
        Ok(())
    }

    pub async fn spawn_npm_process(&self) -> Result<()> {
        info!("Spawning NPM process: {}", self.name);

        // Create new log instance
        self.logs.write().await.new_instance();

        let command = &self.config.command;
        if command.is_empty() {
            anyhow::bail!("No command specified for NPM process");
        }

        let mut cmd = if self.has_direnv {
            let mut c = Command::new("direnv");
            c.arg("exec").arg(&self.project_dir);
            c.args(command);
            c
        } else {
            let mut c = Command::new(&command[0]);
            c.args(&command[1..]);
            c
        };

        cmd.current_dir(&self.project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().context("Failed to spawn NPM process")?;

        // Capture stdout
        if let Some(stdout) = child.stdout.take() {
            let logs = self.logs.clone();
            let name = self.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[{}] {}", name, line);
                    logs.write().await.append(line);
                }
            });
        }

        // Capture stderr
        if let Some(stderr) = child.stderr.take() {
            let logs = self.logs.clone();
            let name = self.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{}] {}", name, line);
                    logs.write().await.append(format!("[stderr] {}", line));
                }
            });
        }

        *self.child.write().await = Some(child);
        *self.state.write().await = ProcessState::Running;
        *self.started_at.write().await = Some(Utc::now());

        self.events.write().await.push(ProcessEvent::Started {
            timestamp: Utc::now(),
        });

        info!("NPM process {} started successfully", self.name);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping process: {}", self.name);

        let mut child = self.child.write().await;
        if let Some(ref mut child) = *child {
            // Send SIGTERM
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;

                if let Some(pid) = child.id() {
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);

                    // Wait up to 5 seconds for graceful shutdown
                    let timeout = Duration::from_secs(5);
                    let start = std::time::Instant::now();

                    while start.elapsed() < timeout {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                info!("Process {} terminated gracefully", self.name);
                                *self.state.write().await = ProcessState::Idle;
                                return Ok(());
                            }
                            _ => {
                                sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }

                    // Force kill if still running
                    warn!("Process {} did not terminate gracefully, sending SIGKILL", self.name);
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                }
            }

            #[cfg(not(unix))]
            {
                child.kill().await.context("Failed to kill process")?;
            }
        }

        *self.state.write().await = ProcessState::Idle;
        info!("Process {} stopped", self.name);
        Ok(())
    }

    pub async fn wait_for_exit(&self) -> Option<i32> {
        let mut child = self.child.write().await;
        if let Some(ref mut child) = *child {
            match child.wait().await {
                Ok(status) => {
                    let exit_code = status.code();
                    *self.state.write().await = ProcessState::Crashed;

                    self.events.write().await.push(ProcessEvent::Crashed {
                        timestamp: Utc::now(),
                        exit_code,
                    });

                    error!(
                        "Process {} exited with code {:?}",
                        self.name, exit_code
                    );
                    exit_code
                }
                Err(e) => {
                    error!("Error waiting for process {}: {}", self.name, e);
                    *self.state.write().await = ProcessState::Crashed;

                    self.events.write().await.push(ProcessEvent::Crashed {
                        timestamp: Utc::now(),
                        exit_code: None,
                    });
                    None
                }
            }
        } else {
            None
        }
    }

    pub async fn get_uptime(&self) -> Option<chrono::Duration> {
        let started = *self.started_at.read().await;
        started.map(|start| Utc::now() - start)
    }
}
