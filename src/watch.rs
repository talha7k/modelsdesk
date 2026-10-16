use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::config::Config;
use crate::output;
use crate::providers::{self, ServerStatus};
use crate::server::{self};

pub async fn animated_boot(cfg: &Config) -> Result<()> {
    let mp = MultiProgress::new();
    let sty = ProgressStyle::with_template("{spinner:.green} {msg}")?;
    let steps = [
        "resolving active model",
        "probing provider binary",
        "launching server process",
        "waiting for port",
        "health check",
    ];
    let mut bars = Vec::new();
    for step in steps {
        let pb = mp.add(ProgressBar::new_spinner());
        pb.set_style(sty.clone());
        pb.set_message(step.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        bars.push(pb);
    }
    let entry = cfg.active_entry()?;
    bars[0].finish_with_message(output::ok("active model resolved"));
    let provider = providers::for_entry(&entry);
    provider.binary()?;
    bars[1].finish_with_message(output::ok("provider binary found"));
    server::start(cfg, &entry, false).await?;
    bars[2].finish_with_message(output::ok("server process launched"));
    let client = server::http_client();
    let mut healthy = None;
    for _ in 0..40 {
        if !matches!(server::server_status(cfg), ServerStatus::Stopped) {
            match provider.health(&client, &cfg.host, cfg.port).await {
                Ok(rep) if rep.ok => {
                    healthy = Some(rep);
                    break;
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bars[3].finish_with_message(output::ok("port listening"));
    match healthy {
        Some(rep) => bars[4].finish_with_message(output::ok(&format!(
            "healthy — {} ({})",
            rep.model.as_deref().unwrap_or("?"),
            rep.mode.as_deref().unwrap_or("?")
        ))),
        None => bars[4].finish_with_message(output::warn("up but health not ok yet — see logs")),
    }
    Ok(())
}

pub async fn live_console(cfg: &Config) -> Result<()> {
    use ratatui::prelude::*;
    use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

    let mut terminal = ratatui::init();
    let client = server::http_client();
    let tick = Duration::from_millis(500);
    let mut log_lines: Vec<String> = Vec::new();
    let mut last_log_len = 0usize;

    loop {
        let status = server::server_status(cfg);
        let health = match &status {
            ServerStatus::Running { .. } => {
                let entry = cfg
                    .active_entry()
                    .unwrap_or_else(|_| crate::config::ModelEntry {
                        name: String::new(),
                        provider: "mlx-dspark".to_string(),
                        repo: String::new(),
                        drafter: None,
                        kv_bits: 4,
                        context_window: None,
                        default_temperature: None,
                        mode: None,
                    });
                let provider = providers::for_entry(&entry);
                provider.health(&client, &cfg.host, cfg.port).await.ok()
            }
            ServerStatus::Stopped => None,
        };
        if let Ok(raw) = std::fs::read_to_string(crate::platform::log_path()) {
            let lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
            if lines.len() != last_log_len {
                last_log_len = lines.len();
                log_lines = lines
                    .iter()
                    .rev()
                    .take(200)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
            }
        }
        let status_text = match &status {
            ServerStatus::Running { pid, port } => {
                let h = health
                    .as_ref()
                    .map(|h| {
                        format!(
                            " {} {}",
                            h.model.as_deref().unwrap_or("?"),
                            h.mode
                                .as_deref()
                                .map(|m| format!("({m})"))
                                .unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                let ok_mark = health.as_ref().is_some_and(|h| h.ok);
                format!(
                    "{} pid={pid} :{port}{h}",
                    if ok_mark {
                        "● healthy"
                    } else {
                        "◐ starting"
                    }
                )
            }
            ServerStatus::Stopped => "○ stopped".to_string(),
        };
        let items: Vec<ListItem> = log_lines
            .iter()
            .rev()
            .take(20)
            .rev()
            .map(|l| ListItem::new(l.clone()))
            .collect();
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(4),
                    Constraint::Length(3),
                ])
                .split(area);
            frame.render_widget(
                Paragraph::new(status_text.clone()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("modeldesk :{} (q to quit)", cfg.port)),
                ),
                chunks[0],
            );
            frame.render_widget(
                List::new(items.clone()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("server log (tail)"),
                ),
                chunks[1],
            );
            frame.render_widget(
                Paragraph::new(
                    "start/stop/restart/health/models/use/logs work beside this console",
                )
                .block(Block::default().borders(Borders::ALL).title("hint")),
                chunks[2],
            );
        })?;
        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }
    ratatui::restore();
    Ok(())
}
