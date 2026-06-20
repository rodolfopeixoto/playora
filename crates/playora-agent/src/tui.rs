use anyhow::Result;
use crossterm::{
    event::{self, Event as CtEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use playora_common::AgentConfig;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io::stdout;

pub fn cmd_tui(cfg: AgentConfig, _screen: Option<String>) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let labels: Vec<&str> = vec![
        "[1] Status",
        "[2] My Console (hardware)",
        "[3] Resource sample",
        "[4] Sync now",
        "[5] Heartbeat",
        "[6] Hardware test (quick)",
        "[7] Hardware test (full)",
        "[8] Scan ROMs",
        "[9] Features (show)",
        "[a] Features (fetch from server)",
        "[b] Catalog (legal items)",
        "[c] Saves: pack tarball",
        "[d] Saves: upload to server",
        "[e] Sources (ROM mirrors)",
        "[f] Systems (folder/emulator/ext)",
        "[g] Self-update from GitHub",
        "[q] Quit",
    ];
    let mut state = ListState::default();
    state.select(Some(0));
    let mut output = String::from("ready.");

    let res = loop {
        term.draw(|f| {
            let area = f.area();
            let cols = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(10),
                ])
                .split(area);

            let header = Paragraph::new(Line::from(format!(" Playora — {} ", cfg.device_name)))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, cols[0]);

            let items: Vec<ListItem> = labels.iter().map(|l| ListItem::new(*l)).collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" menu "))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, cols[1], &mut state);

            let out = Paragraph::new(output.as_str())
                .block(Block::default().borders(Borders::ALL).title(" output "));
            f.render_widget(out, cols[2]);
        })?;

        if let CtEvent::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok::<(), anyhow::Error>(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1).min(labels.len() - 1)));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    output = run(&cfg, state.selected().unwrap_or(0));
                    if state.selected() == Some(labels.len() - 1) {
                        break Ok(());
                    }
                }
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    res
}

fn run(cfg: &AgentConfig, sel: usize) -> String {
    match sel {
        0 => match crate::sync::cmd_status(cfg.clone()) {
            Ok(()) => "status printed".into(),
            Err(e) => e.to_string(),
        },
        1 => serde_json::to_string_pretty(&crate::hw::snapshot()).unwrap_or_default(),
        2 => serde_json::to_string_pretty(&crate::resources::sample()).unwrap_or_default(),
        3 => match crate::sync::cmd_sync_once(cfg.clone()) {
            Ok(()) => "synced".into(),
            Err(e) => e.to_string(),
        },
        4 => match crate::sync::cmd_heartbeat(cfg.clone()) {
            Ok(()) => "heartbeat queued".into(),
            Err(e) => e.to_string(),
        },
        5 => match crate::tests::cmd_hardware_test(cfg.clone(), "quick", false) {
            Ok(()) => "quick test done".into(),
            Err(e) => e.to_string(),
        },
        6 => match crate::scanner::cmd_scan(cfg.clone()) {
            Ok(()) => "scan done".into(),
            Err(e) => e.to_string(),
        },
        7 => match crate::features::cmd_show(cfg.clone()) {
            Ok(()) => "features printed".into(),
            Err(e) => e.to_string(),
        },
        8 => match crate::catalog::cmd_list(cfg.clone(), false) {
            Ok(()) => "catalog printed".into(),
            Err(e) => e.to_string(),
        },
        _ => "bye".into(),
    }
}
