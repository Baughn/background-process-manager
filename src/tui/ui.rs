use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, ConnectionState};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // Top panels
            Constraint::Percentage(57), // Bottom panels
            Constraint::Min(3),          // Keyboard shortcuts
        ])
        .split(frame.area());

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_server_status(frame, app, top_chunks[0]);
    render_process_details(frame, app, top_chunks[1]);
    render_processes(frame, app, bottom_chunks[0]);
    render_output(frame, app, bottom_chunks[1]);
    render_keyboard_shortcuts(frame, chunks[2]);
}

fn render_server_status(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Server Status")
        .title_style(Style::default().fg(Color::Cyan))
        .borders(Borders::ALL);

    let (connection_indicator, connection_color) = match app.connection_state {
        ConnectionState::Connected => ("● Connected", Color::Green),
        ConnectionState::Connecting => ("● Connecting", Color::Yellow),
        ConnectionState::Disconnected => ("● Disconnected", Color::Red),
        ConnectionState::Error => ("● Error", Color::Red),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(connection_indicator, Style::default().fg(connection_color).bold()),
            Span::raw(" | Port: "),
            Span::styled(
                app.mcp_url.split(':').last().unwrap_or("3001").trim_end_matches("/mcp"),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    if let Some(ref status) = app.server_status {
        if let Some(last_update) = app.last_update {
            lines.push(Line::from(vec![
                Span::raw("Last update: "),
                Span::styled(
                    last_update.format("%H:%M:%S").to_string(),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }

        lines.push(Line::from(vec![
            Span::raw("Mode: "),
            Span::styled(
                status.mode.clone(),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        if let Some(ref time_until_release) = status.time_until_release {
            lines.push(Line::from(vec![
                Span::raw("Time until release: "),
                Span::styled(
                    time_until_release.clone(),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }

        let (running, stopped, errored) = app.get_process_counts();
        lines.push(Line::from(vec![
            Span::raw("Processes: "),
            Span::styled(
                format!("{}", status.processes.len()),
                Style::default().fg(Color::White),
            ),
            Span::raw(" ("),
            Span::styled(
                format!("{} running", running),
                Style::default().fg(Color::Green),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} stopped", stopped),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} errored", errored),
                Style::default().fg(Color::Red),
            ),
            Span::raw(")"),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No status available",
            Style::default().fg(Color::Gray).italic(),
        )));
    }

    if !app.status_message.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            &app.status_message,
            Style::default().fg(Color::Yellow),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_process_details(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Process Details")
        .title_style(Style::default().fg(Color::Cyan))
        .borders(Borders::ALL);

    let content = if let Some(process) = app.get_selected_process() {
        let state_color = match process.state.to_lowercase().as_str() {
            s if s.contains("running") => Color::Green,
            s if s.contains("stopped") || s.contains("idle") => Color::Yellow,
            s if s.contains("crashed") => Color::Red,
            _ => Color::Gray,
        };

        let mut lines = vec![
            Line::from(vec![
                Span::raw("Name: "),
                Span::styled(
                    process.name.clone(),
                    Style::default().fg(Color::Cyan).bold(),
                ),
            ]),
            Line::from(vec![
                Span::raw("State: "),
                Span::styled(
                    process.state.clone(),
                    Style::default().fg(state_color).bold(),
                ),
            ]),
        ];

        if let Some(ref uptime) = process.uptime {
            lines.push(Line::from(vec![
                Span::raw("Uptime: "),
                Span::styled(
                    uptime.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        if process.crash_count > 0 {
            lines.push(Line::from(vec![
                Span::raw("Crash count: "),
                Span::styled(
                    format!("{}", process.crash_count),
                    Style::default().fg(Color::Red).bold(),
                ),
            ]));
        }

        if !process.events.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Recent events:",
                Style::default().fg(Color::Cyan),
            )));
            for event in process.events.iter().take(5) {
                lines.push(Line::from(vec![
                    Span::raw("  • "),
                    Span::styled(
                        event.clone(),
                        Style::default().fg(Color::Gray),
                    ),
                ]));
            }
        }

        Text::from(lines)
    } else {
        Text::from(Span::styled(
            "No process selected.",
            Style::default().fg(Color::Gray).italic(),
        ))
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_processes(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Processes")
        .title_style(Style::default().fg(Color::Cyan))
        .borders(Borders::ALL);

    let items: Vec<ListItem> = if let Some(ref status) = app.server_status {
        if status.processes.is_empty() {
            vec![ListItem::new(Span::styled(
                "No processes running.",
                Style::default().fg(Color::Gray).italic(),
            ))]
        } else {
            status
                .processes
                .iter()
                .enumerate()
                .map(|(i, process)| {
                    let state_color = match process.state.to_lowercase().as_str() {
                        s if s.contains("running") => Color::Green,
                        s if s.contains("stopped") || s.contains("idle") => Color::Yellow,
                        s if s.contains("crashed") => Color::Red,
                        _ => Color::Gray,
                    };

                    let icon = match process.state.to_lowercase().as_str() {
                        s if s.contains("running") => "▶",
                        s if s.contains("stopped") || s.contains("idle") => "■",
                        s if s.contains("crashed") => "✗",
                        _ => "?",
                    };

                    let mut style = Style::default();
                    if Some(i) == app.selected_process_index {
                        style = style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                    }

                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{} ", icon), Style::default().fg(state_color)),
                        Span::styled(
                            format!("{} ", process.name),
                            style.fg(Color::White),
                        ),
                        Span::styled(
                            format!("({})", process.state),
                            style.fg(state_color),
                        ),
                    ]))
                    .style(style)
                })
                .collect()
        }
    } else {
        vec![ListItem::new(Span::styled(
            "Connecting...",
            Style::default().fg(Color::Gray).italic(),
        ))]
    };

    let list = List::new(items).block(block);

    frame.render_widget(list, area);
}

fn render_output(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Output")
        .title_style(Style::default().fg(Color::Cyan))
        .borders(Borders::ALL);

    let content = if app.logs.is_empty() {
        if app.get_selected_process().is_some() {
            Text::from(Span::styled(
                "No logs available. Press Enter to refresh.",
                Style::default().fg(Color::Gray).italic(),
            ))
        } else {
            Text::from(Span::styled(
                "No process selected.",
                Style::default().fg(Color::Gray).italic(),
            ))
        }
    } else {
        Text::from(app.logs.as_str())
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((0, 0));

    frame.render_widget(paragraph, area);
}

fn render_keyboard_shortcuts(frame: &mut Frame, area: Rect) {
    let shortcuts = vec![
        ("▲▼", "Navigate"),
        ("⏎", "View Output"),
        ("r", "Restart"),
        ("c", "Clear"),
        ("q", "Quit"),
    ];

    let spans: Vec<Span> = shortcuts
        .iter()
        .flat_map(|(key, desc)| {
            vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::styled(
                    format!("{} ", desc),
                    Style::default().fg(Color::White),
                ),
                Span::raw("| "),
            ]
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(paragraph, area);
}
