mod builder;
mod config;
mod crash_handler;
mod log_buffer;
mod mcp_server;
mod mode;
mod process;

use anyhow::Result;
use builder::Builder;
use config::{Config, ProcessType};
use crash_handler::{CrashHandler, RunMode};
use mcp_server::{AppState, start_server};
use mode::ModeManager;
use process::ProcessManager;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Parse CLI arguments
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <project-directory>", args[0]);
        std::process::exit(1);
    }

    let project_dir = PathBuf::from(&args[1]);
    if !project_dir.exists() {
        eprintln!("Project directory does not exist: {}", project_dir.display());
        std::process::exit(1);
    }

    info!("Starting background-process-manager for {}", project_dir.display());

    // Load configuration
    let config = Config::load(&project_dir)?;
    info!("Loaded configuration: {} processes", config.process.len());

    // Initialize shared state
    let builder = Arc::new(Builder::new(project_dir.clone()));
    let mode_manager = Arc::new(ModeManager::new(config.dev_timeout_hours));
    let processes: Arc<RwLock<HashMap<String, Arc<ProcessManager>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let crash_handlers: Arc<RwLock<HashMap<String, CrashHandler>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Initialize processes
    {
        let mut procs = processes.write().await;
        let mut handlers = crash_handlers.write().await;

        for (name, proc_config) in &config.process {
            let pm = Arc::new(ProcessManager::new(
                name.clone(),
                proc_config.clone(),
                project_dir.clone(),
            ));
            procs.insert(name.clone(), pm);

            let handler = CrashHandler::new(
                config.dev_crash_wait_seconds,
                config.release_crash_backoff_initial_seconds,
                config.release_crash_backoff_max_seconds,
            );
            handlers.insert(name.clone(), handler);
        }
    }

    // Start all processes
    info!("Starting all processes...");
    let procs = processes.read().await;
    for (name, process) in procs.iter() {
        let mode = mode_manager.get_mode().await;
        let release = matches!(mode, RunMode::Release);

        match process.config.process_type {
            ProcessType::Rust => {
                info!("Building and starting Rust process: {}", name);
                match builder.build_rust(release, process.build_logs.clone()).await {
                    Ok(binary_path) => {
                        if let Err(e) = process.spawn_process(binary_path).await {
                            error!("Failed to start process {}: {}", name, e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to build process {}: {}", name, e);
                    }
                }
            }
            ProcessType::Npm => {
                info!("Starting NPM process: {}", name);
                if let Err(e) = process.spawn_npm_process().await {
                    error!("Failed to start process {}: {}", name, e);
                }
            }
        }
    }
    drop(procs);

    // Spawn crash monitors for each process
    for (name, _) in config.process.iter() {
        let name = name.clone();
        let processes = processes.clone();
        let builder = builder.clone();
        let mode_manager = mode_manager.clone();
        let crash_handlers = crash_handlers.clone();

        tokio::spawn(async move {
            loop {
                let process = {
                    let procs = processes.read().await;
                    procs.get(&name).cloned()
                };

                if let Some(process) = process {
                    // Wait for process to exit
                    process.wait_for_exit().await;

                    // Check if this is a manual restart - if so, skip the automatic restart logic
                    if process.is_manual_restart_in_progress().await {
                        info!("Process {} stopped for manual restart, skipping automatic restart", name);
                        continue;
                    }

                    // Get crash handler and wait before restart
                    let mode = mode_manager.get_mode().await;
                    {
                        let mut handlers = crash_handlers.write().await;
                        if let Some(handler) = handlers.get_mut(&name) {
                            handler.wait_before_restart(mode).await;
                        }
                    }

                    // Rebuild and restart
                    info!("Restarting process: {}", name);
                    let release = matches!(mode, RunMode::Release);

                    match process.config.process_type {
                        ProcessType::Rust => {
                            match builder.build_rust(release, process.build_logs.clone()).await {
                                Ok(binary_path) => {
                                    if let Err(e) = process.spawn_process(binary_path).await {
                                        error!("Failed to restart process {}: {}", name, e);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to build process {}: {}", name, e);
                                }
                            }
                        }
                        ProcessType::Npm => {
                            if let Err(e) = process.spawn_npm_process().await {
                                error!("Failed to restart process {}: {}", name, e);
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        });
    }

    // Spawn mode checker
    let mode_manager_clone = mode_manager.clone();
    let processes_clone = processes.clone();
    let builder_clone = builder.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(60)); // Check every minute
        loop {
            interval.tick().await;

            if mode_manager_clone.should_switch_to_release().await {
                info!("Switching to release mode");
                mode_manager_clone.switch_to_release().await;

                // Rebuild and restart all processes in release mode
                let procs = processes_clone.read().await;
                for (name, process) in procs.iter() {
                    if process.config.process_type == ProcessType::Rust {
                        info!("Rebuilding {} in release mode", name);

                        // Stop process
                        if let Err(e) = process.stop().await {
                            error!("Failed to stop process {}: {}", name, e);
                            continue;
                        }

                        // Build in release mode
                        match builder_clone.build_rust(true, process.build_logs.clone()).await {
                            Ok(binary_path) => {
                                if let Err(e) = process.spawn_process(binary_path).await {
                                    error!("Failed to start process {} in release mode: {}", name, e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to build process {} in release mode: {}", name, e);
                            }
                        }
                    }
                }
            }
        }
    });

    // Start MCP server
    let app_state = AppState::new(
        config.clone(),
        processes.clone(),
        builder.clone(),
        mode_manager.clone(),
        crash_handlers.clone(),
    );

    info!("Starting MCP HTTP server on port {}", config.mcp_port);
    start_server(app_state, config.mcp_port).await?;

    Ok(())
}
