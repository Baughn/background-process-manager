use crate::log_buffer::LogBuffer;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct Builder {
    project_dir: PathBuf,
    has_direnv: bool,
}

impl Builder {
    pub fn new(project_dir: PathBuf) -> Self {
        let has_direnv = project_dir.join(".envrc").exists();
        Self {
            project_dir,
            has_direnv,
        }
    }

    pub async fn build_rust(
        &self,
        release: bool,
        build_logs: Arc<RwLock<LogBuffer>>,
    ) -> Result<PathBuf> {
        info!(
            "Building Rust project in {} mode",
            if release { "release" } else { "dev" }
        );

        // Create new build log instance
        build_logs.write().await.new_instance();

        let mut cmd = if self.has_direnv {
            let mut c = Command::new("direnv");
            c.arg("exec").arg(&self.project_dir).arg("cargo").arg("build");
            c
        } else {
            let mut c = Command::new("cargo");
            c.arg("build");
            c
        };

        if release {
            cmd.arg("--release");
        }

        cmd.current_dir(&self.project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn cargo build")?;

        // Capture stdout
        if let Some(stdout) = child.stdout.take() {
            let logs = build_logs.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[build] {}", line);
                    logs.write().await.append(line);
                }
            });
        }

        // Capture stderr (cargo outputs to stderr by default)
        if let Some(stderr) = child.stderr.take() {
            let logs = build_logs.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[build] {}", line);
                    logs.write().await.append(line);
                }
            });
        }

        let status = child.wait().await.context("Failed to wait for cargo build")?;

        if !status.success() {
            error!("Build failed with status: {}", status);
            anyhow::bail!("Build failed");
        }

        info!("Build completed successfully");

        // Find the binary name from Cargo.toml
        let binary_path = self.find_rust_binary(release)?;
        Ok(binary_path)
    }

    fn find_rust_binary(&self, release: bool) -> Result<PathBuf> {
        // Read Cargo.toml to find the package name
        let cargo_toml_path = self.project_dir.join("Cargo.toml");
        let content = std::fs::read_to_string(&cargo_toml_path)
            .context("Failed to read Cargo.toml")?;

        let cargo_toml: toml::Value = toml::from_str(&content)
            .context("Failed to parse Cargo.toml")?;

        let package_name = cargo_toml
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .context("Failed to find package name in Cargo.toml")?;

        // Convert package name to binary name (replace hyphens with underscores is not needed for the binary file itself)
        let target_dir = if release { "release" } else { "debug" };
        let binary_path = self
            .project_dir
            .join("target")
            .join(target_dir)
            .join(package_name);

        if !binary_path.exists() {
            anyhow::bail!("Binary not found at: {}", binary_path.display());
        }

        Ok(binary_path)
    }
}
