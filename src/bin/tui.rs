use anyhow::{Context, Result};
use background_process_manager::tui::{App, EventHandler, McpClient};
use crossterm::{
    event::KeyCode,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;
use std::io::{stdout, Stdout};
use std::time::Duration;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn init_terminal() -> Result<Tui> {
    enable_raw_mode().context("Failed to enable raw mode")?;
    stdout()
        .execute(EnterAlternateScreen)
        .context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend).context("Failed to create terminal")?;
    Ok(terminal)
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    stdout()
        .execute(LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mcp_url = if args.len() > 1 {
        args[1].clone()
    } else {
        "http://localhost:3001/mcp".to_string()
    };

    // Initialize app
    let mut app = App::new(mcp_url.clone());
    let mut client = McpClient::new(mcp_url);

    // Initialize MCP connection
    match client.initialize().await {
        Ok(_) => {
            app.status_message = "Connected to MCP server".to_string();
        }
        Err(e) => {
            eprintln!("Failed to initialize MCP client: {}", e);
            eprintln!("Make sure the background-process-manager is running.");
            return Err(e);
        }
    }

    // Initialize terminal
    let mut terminal = init_terminal().context("Failed to initialize terminal")?;
    terminal.clear().context("Failed to clear terminal")?;

    // Create event handler with 1 second tick rate
    let mut events = EventHandler::new(Duration::from_secs(1));

    // Initial status fetch
    let _ = app.update_status(&mut client).await;

    // Main loop
    let result = run_app(&mut terminal, &mut app, &mut client, &mut events).await;

    // Restore terminal
    restore_terminal().context("Failed to restore terminal")?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        return Err(e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Tui,
    app: &mut App,
    client: &mut McpClient,
    events: &mut EventHandler,
) -> Result<()> {
    loop {
        // Render
        terminal
            .draw(|frame| background_process_manager::tui::ui::render(frame, app))
            .context("Failed to draw terminal")?;

        // Handle events
        if let Some(event) = events.next().await {
            match event {
                background_process_manager::tui::Event::Tick => {
                    // Auto-refresh status every tick
                    let _ = app.update_status(client).await;
                }
                background_process_manager::tui::Event::Key(key) => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.quit();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.select_previous_process();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.select_next_process();
                        }
                        KeyCode::Enter => {
                            let _ = app.refresh_logs(client).await;
                        }
                        KeyCode::Char('r') => {
                            let _ = app.restart_selected_process(client).await;
                        }
                        KeyCode::Char('c') => {
                            app.clear_logs();
                        }
                        _ => {}
                    }
                }
                background_process_manager::tui::Event::Resize(_, _) => {
                    // Terminal will handle resize automatically
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
