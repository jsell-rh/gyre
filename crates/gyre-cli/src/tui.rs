use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    DefaultTerminal,
};
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::ws::WsClient;

#[derive(Clone, PartialEq)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

impl ConnectionStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Connecting => Color::Yellow,
            Self::Connected => Color::Green,
            Self::Disconnected => Color::Red,
        }
    }
}

struct AppState {
    status: ConnectionStatus,
    last_ping_ms: Option<u64>,
}

pub async fn run(server_url: String, token: String) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let terminal = ratatui::init();

    let result = run_app(terminal, server_url, token).await;

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    ratatui::restore();
    result
}

async fn run_app(mut terminal: DefaultTerminal, server_url: String, token: String) -> Result<()> {
    let state = Arc::new(Mutex::new(AppState {
        status: ConnectionStatus::Connecting,
        last_ping_ms: None,
    }));

    // Spawn background task for WS connection and pinging
    let state_bg = Arc::clone(&state);
    let url_bg = server_url.clone();
    let token_bg = token.clone();
    tokio::spawn(async move {
        let client = WsClient::new(url_bg, token_bg);
        match client.connect_and_auth().await {
            Ok(mut ws) => {
                {
                    let mut s = state_bg.lock().unwrap();
                    s.status = ConnectionStatus::Connected;
                }
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    match client.ping(&mut ws).await {
                        Ok(rtt) => {
                            let mut s = state_bg.lock().unwrap();
                            s.last_ping_ms = Some(rtt);
                        }
                        Err(_) => {
                            let mut s = state_bg.lock().unwrap();
                            s.status = ConnectionStatus::Disconnected;
                            break;
                        }
                    }
                }
            }
            Err(_) => {
                let mut s = state_bg.lock().unwrap();
                s.status = ConnectionStatus::Disconnected;
            }
        }
    });

    let version = env!("CARGO_PKG_VERSION");

    loop {
        let (status, last_ping) = {
            let s = state.lock().unwrap();
            (s.status.clone(), s.last_ping_ms)
        };

        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(area);

            // Header
            let header = Paragraph::new(format!("Gyre CLI v{version}"))
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(Color::Cyan));
            frame.render_widget(header, chunks[0]);

            // Connection status
            let status_text = Line::from(vec![
                Span::raw("Status: "),
                Span::styled(status.label(), Style::default().fg(status.color())),
            ]);
            let status_widget = Paragraph::new(status_text)
                .block(Block::default().borders(Borders::ALL).title("Connection"));
            frame.render_widget(status_widget, chunks[1]);

            // Ping
            let ping_text = match last_ping {
                Some(ms) => format!("Last ping RTT: {ms}ms"),
                None => "Last ping RTT: --".to_string(),
            };
            let ping_widget = Paragraph::new(ping_text)
                .block(Block::default().borders(Borders::ALL).title("Ping"));
            frame.render_widget(ping_widget, chunks[2]);

            // Help
            let help =
                Paragraph::new("Press q to quit").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, chunks[3]);
        })?;

        // Poll events with 100ms timeout so the UI refreshes
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    Ok(())
}
