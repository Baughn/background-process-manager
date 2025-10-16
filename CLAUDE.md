# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Background Process Manager is an MCP (Model Context Protocol) server for managing development processes with zero-downtime restarts, automatic crash recovery, and intelligent dev/release mode switching. It's designed for hobby projects and prototypes that need to stay running even when not actively being developed.

## Version Control

This project uses **Jujutsu** (jj), not Git. Use `jj` commands instead of `git` commands.

## Build and Test Commands

### Building
```bash
# Development build
cargo build

# Release build
cargo build --release

# With direnv (if .envrc exists)
direnv exec . cargo build
direnv exec . cargo build --release
```

### Testing
```bash
cargo test
```

### Linting
```bash
cargo clippy
```

### Running
```bash
# Main binary (requires a project directory with .mcp-run config)
cargo run -- /path/to/project

# TUI binary (for interactive testing)
cargo run --bin bpm-tui
cargo run --bin bpm-tui http://localhost:3001/mcp
```

## Architecture Overview

### Core Components

The system consists of several key modules that work together:

1. **main.rs**: Orchestrates all components, spawns crash monitors for each process, and manages automatic release mode switching
2. **mcp_server.rs**: Axum-based HTTP server implementing MCP protocol over HTTP/SSE with four tools (search_logs, search_build_log, restart, get_status)
3. **process.rs**: Manages individual process lifecycle (spawn, stop, wait_for_exit) with direnv integration and manual restart flag coordination
4. **builder.rs**: Handles Rust project building with direnv support, parses Cargo.toml to find binary paths
5. **mode.rs**: Manages dev/release mode switching based on tool call activity and configurable timeout
6. **crash_handler.rs**: Implements backoff strategies (fixed delay in dev, sub-exponential in release)
7. **log_buffer.rs**: Circular buffer with multiple instances, regex search, context lines, and head/tail limiting

### Key Architectural Patterns

**Zero-Downtime Restart Flow** (mcp_server.rs:328-384):
1. Set manual restart flag on ProcessManager to prevent crash monitor interference
2. Build new binary (while old process keeps running)
3. Stop old process (SIGTERM → 5s grace → SIGKILL)
4. Start new process
5. Clear manual restart flag
6. Reset crash handler

**Manual Restart Flag Pattern** (process.rs:358-370):
Used to prevent the automatic crash monitor from treating manual stops as crashes. The flag is checked in `wait_for_exit()` before marking state as Crashed.

**Mode Switching** (main.rs:178-218):
A background task checks every minute if dev mode has been idle (no tool calls) for longer than `dev_timeout_hours`. If so, rebuilds all Rust processes in release mode.

**Process Monitoring** (main.rs:114-176):
Each process has a dedicated tokio task that waits for exit, checks the manual restart flag, applies crash backoff, then rebuilds and restarts. This runs in a loop for the lifetime of the process.

**Log Instance Management** (log_buffer.rs:97-162):
Each process restart creates a new log instance (up to 10 kept). Supports negative indexing (-1 = most recent) and positive indexing (0 = first). Search applies pattern matching, context expansion, then head/tail limiting in that order.

**Direnv Integration** (builder.rs:38-46, process.rs:87-93):
Detected by checking for `.envrc` file. When present, all commands are wrapped with `direnv exec <project_dir> <command>`.

## Configuration

Projects managed by this tool require a `.mcp-run` file in TOML format. See README.md for the full schema. Key fields:
- `mcp_port`: HTTP server port
- `dev_timeout_hours`: Time before auto-switching to release mode
- `dev_crash_wait_seconds`: Fixed wait after crash in dev mode
- `release_crash_backoff_*`: Sub-exponential backoff parameters for release mode
- `[process.<name>]`: Process definitions with type (rust/npm), args, command

## MCP Protocol Implementation

The server implements MCP over HTTP (POST) and SSE (GET) at the `/mcp` endpoint. The protocol follows the 2024-11-05 version. Tool calls automatically record activity with ModeManager to reset the dev mode timeout.

## Development Notes

- Tracing is configured via RUST_LOG environment variable (default: info)
- All process stdout/stderr is both captured to LogBuffer and passed through with `[process_name]` or `[build]` prefixes
- Process state transitions: Idle → Running → (Crashed or Idle on manual stop)
- NixOS environment: Use nix-shell if commands are missing
