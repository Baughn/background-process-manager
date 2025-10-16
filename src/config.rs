use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub mcp_port: u16,
    #[serde(default = "default_dev_timeout_hours")]
    pub dev_timeout_hours: u64,
    #[serde(default = "default_dev_crash_wait_seconds")]
    pub dev_crash_wait_seconds: u64,
    #[serde(default = "default_release_crash_backoff_initial_seconds")]
    pub release_crash_backoff_initial_seconds: u64,
    #[serde(default = "default_release_crash_backoff_max_seconds")]
    pub release_crash_backoff_max_seconds: u64,
    #[serde(default)]
    pub process: HashMap<String, ProcessConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessConfig {
    #[serde(rename = "type")]
    pub process_type: ProcessType,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessType {
    Rust,
    Npm,
}

fn default_dev_timeout_hours() -> u64 {
    3
}

fn default_dev_crash_wait_seconds() -> u64 {
    120
}

fn default_release_crash_backoff_initial_seconds() -> u64 {
    1
}

fn default_release_crash_backoff_max_seconds() -> u64 {
    300
}

impl Config {
    pub fn load(project_dir: &Path) -> Result<Self> {
        let config_path = project_dir.join(".mcp-run");
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

        // Validate that we have at least one process
        if config.process.is_empty() {
            anyhow::bail!("No processes defined in configuration");
        }

        // Validate process configurations
        for (name, proc_config) in &config.process {
            match proc_config.process_type {
                ProcessType::Rust => {
                    // For Rust, args are optional
                }
                ProcessType::Npm => {
                    // For NPM, command is required
                    if proc_config.command.is_empty() {
                        anyhow::bail!("Process '{}' is type 'npm' but has no command specified", name);
                    }
                }
            }
        }

        Ok(config)
    }
}
