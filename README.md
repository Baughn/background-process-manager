# Background Process Manager

A specialized MCP (Model Context Protocol) server for managing development processes with zero-downtime restarts, automatic crash recovery, and intelligent dev/release mode switching.

## Features

- **Zero-downtime restarts**: Build first, then switch to the new binary
- **Automatic crash recovery**: Configurable backoff strategies for dev and release modes
- **Dev/Release mode switching**: Automatically switches to release mode after a period of inactivity
- **Direnv integration**: Automatically detects and uses `.envrc` for environment variables
- **Log management**: In-memory circular buffers with search capabilities
- **MCP protocol**: Integrates seamlessly with Claude and other MCP clients

## Use Case

This tool is designed for hobby projects and prototypes that need to stay running even when you're not actively developing them. Unlike traditional process managers, it prioritizes fast iteration during development (using `cargo run`) while still being able to switch to optimized release builds when idle.

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/background-process-manager`.

## Configuration

Create a `.mcp-run` file in your project directory:

```toml
# Port for the MCP server to listen on
mcp_port = 3001

# Time in hours before switching to release mode (optional, default: 3)
dev_timeout_hours = 3

# Wait time in seconds after a crash in dev mode (optional, default: 120)
dev_crash_wait_seconds = 120

# Initial backoff in seconds for crash recovery in release mode (optional, default: 1)
release_crash_backoff_initial_seconds = 1

# Maximum backoff in seconds for crash recovery in release mode (optional, default: 300)
release_crash_backoff_max_seconds = 300

# Define processes to manage
[process.main]
type = "rust"
args = ["--port", "8080"]

# Optional: NPM sidecar process
# [process.frontend]
# type = "npm"
# command = ["npm", "run", "dev"]
```

## Usage

### Running the Manager

```bash
background-process-manager /path/to/project
```

Or with systemd:

```ini
[Unit]
Description=Background Process Manager for MyProject
After=network.target

[Service]
Type=simple
User=your-user
WorkingDirectory=/path/to/project
ExecStart=/path/to/background-process-manager /path/to/project
Restart=always

[Install]
WantedBy=multi-user.target
```

### MCP Tools

The server exposes four MCP tools:

#### 1. `search_logs`

Search process logs with optional regex pattern and filtering.

```json
{
  "process": "main",
  "pattern": "error.*timeout",  // optional regex
  "context_lines": 2,             // optional: lines around matches
  "head": 50,                     // optional: first N lines
  "tail": 100,                    // optional: last N lines
  "index": -1                     // optional: -1 = most recent, -2 = previous, etc.
}
```

#### 2. `search_build_log`

Search build logs (same parameters as `search_logs`).

#### 3. `restart`

Restart a process. Builds first (for Rust projects), then restarts. Automatically switches back to dev mode.

```json
{
  "process": "main"
}
```

#### 4. `get_status`

Get status of all processes including mode, uptime, state, and recent events.

```json
{}
```

## How It Works

### Process Lifecycle

1. **Initial startup**: Starts in **release mode** (designed for system boot scenarios), builds with `cargo build --release` and starts the process
2. **Crash recovery**:
   - Dev mode: Waits 2 minutes (configurable) before restart, giving you time to investigate
   - Release mode: Uses sub-exponential backoff (1s, 1.5s, 2.25s, ..., up to 5 minutes)
3. **Auto-release switch**: After 3 hours (configurable) of no tool calls, rebuilds in release mode (if in dev mode)
4. **Manual restart**: When you call the `restart` tool, switches to dev mode for faster iteration

### Zero-Downtime Restart

When you call `restart`:
1. Manual restart flag is set to prevent crash monitor interference
2. Build starts in the background (while old process keeps running)
3. Once build completes, old process is stopped (SIGTERM, 5s grace period, then SIGKILL)
4. New process starts immediately
5. Manual restart flag is cleared

This means compilation time doesn't add to downtime - only the brief moment to swap processes. The manual restart flag ensures the crash monitor doesn't interfere and that the restart isn't counted as a crash.

### Direnv Support

If a `.envrc` file exists in your project directory, all commands (build, run) are wrapped with `direnv exec`.

### Logging

All output from managed processes and builds is:
- Captured to in-memory circular buffers (searchable via MCP tools)
- Passed through to stdout/stderr with `[process_name]` or `[build]` prefixes

This means logs appear in journalctl when running as a systemd service, while still being available for search through the MCP interface.

## Connecting with Claude Code

Add the MCP server to your Claude Code configuration (`~/.config/claude-code/config.json`):

```json
{
  "mcpServers": {
    "ganbot": {
      "url": "http://localhost:3001/mcp"
    }
  }
}
```

Once connected, you can use the MCP tools directly in Claude Code:
- `search_logs` - Search process logs for errors or patterns
- `search_build_log` - Check build output for compilation issues
- `restart` - Rebuild and restart your process after code changes
- `get_status` - Check current mode, uptime, and recent events

## Example: Using with ganbot

```bash
# Create configuration
cat > ~/dev/ganbot/.mcp-run << 'EOF'
mcp_port = 3001
dev_timeout_hours = 3

[process.main]
type = "rust"
args = []
EOF

# Start the manager
background-process-manager ~/dev/ganbot

# Now connect from Claude Code using the configuration above
```

## TUI (Terminal User Interface)

A built-in TUI is available for manual interaction with the MCP server:

```bash
# Run the TUI (connects to default localhost:3001)
bpm-tui

# Or specify a custom MCP server URL
bpm-tui http://localhost:3001/mcp
```

### TUI Features

The TUI provides a comprehensive dashboard with four panels:

- **Server Status** (top-left): Connection state, mode, process counts, status messages
- **Process Details** (top-right): Selected process info, uptime, events, crash count
- **Processes** (bottom-left): List of all managed processes with state indicators
- **Output** (bottom-right): Live logs from the selected process

### TUI Keyboard Shortcuts

- `▲▼` or `j/k` - Navigate process list
- `Enter` - Refresh logs for selected process
- `r` - Restart selected process (rebuilds first, then restarts)
- `c` - Clear output panel
- `q` or `Esc` - Quit

### TUI Status Indicators

- 🟢 Green `▶` - Process is running
- 🟡 Yellow `■` - Process is stopped/idle
- 🔴 Red `✗` - Process has crashed

The TUI auto-refreshes status every second and provides real-time feedback for all operations.

## Architecture

```
┌─────────────────────────────────────┐
│  MCP Client (Claude Code)           │
└─────────────┬───────────────────────┘
              │ JSON-RPC over HTTP/SSE
┌─────────────▼───────────────────────┐
│  MCP HTTP Server (port 3001)        │
│  Endpoint: /mcp                     │
│                                     │
│  Tools:                             │
│  - search_logs                      │
│  - search_build_log                 │
│  - restart                          │
│  - get_status                       │
└─────────────┬───────────────────────┘
              │
    ┌─────────┴─────────┐
    │                   │
┌───▼────────┐  ┌───────▼──────┐
│ Builder    │  │ Mode Manager │
│ - cargo    │  │ - Dev/Release│
│ - direnv   │  │ - Timeouts   │
└───┬────────┘  └──────────────┘
    │
┌───▼──────────────────────────┐
│ Process Manager              │
│ - Spawn/Stop                 │
│ - Log capture                │
│ - Crash detection            │
│ - Manual restart flag        │
└───┬──────────────────────────┘
    │
┌───▼──────────────────────────┐
│ Your Application (ganbot)    │
└──────────────────────────────┘
```

## Limitations

- Only tested on Linux (uses Unix signals)
- In-memory logs only (lost on manager restart)
- No support for process dependencies
- Basic health checking (process running = healthy)

## Future Enhancements

- Custom build commands
- Process dependency ordering
- Health check endpoints
- Persistent log storage
- Windows support
- Process resource limits

## License

MIT (or whatever license you prefer)
