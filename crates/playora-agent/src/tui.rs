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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::io::stdout;

struct MenuAction {
    label: &'static str,
    run: fn(&AgentConfig) -> String,
}

fn act_status(cfg: &AgentConfig) -> String {
    match crate::sync::cmd_status(cfg.clone()) {
        Ok(()) => "status printed".into(),
        Err(e) => e.to_string(),
    }
}
fn act_hw(_cfg: &AgentConfig) -> String {
    serde_json::to_string_pretty(&crate::hw::snapshot()).unwrap_or_default()
}
fn act_resources(_cfg: &AgentConfig) -> String {
    serde_json::to_string_pretty(&crate::resources::sample()).unwrap_or_default()
}
fn act_sync(cfg: &AgentConfig) -> String {
    match crate::sync::cmd_sync_once(cfg.clone()) {
        Ok(()) => "synced".into(),
        Err(e) => e.to_string(),
    }
}
fn act_heartbeat(cfg: &AgentConfig) -> String {
    match crate::sync::cmd_heartbeat(cfg.clone()) {
        Ok(()) => "heartbeat queued".into(),
        Err(e) => e.to_string(),
    }
}
fn act_test_quick(cfg: &AgentConfig) -> String {
    match crate::tests::cmd_hardware_test(cfg.clone(), "quick", false) {
        Ok(()) => "quick test done".into(),
        Err(e) => e.to_string(),
    }
}
fn act_test_full(cfg: &AgentConfig) -> String {
    match crate::tests::cmd_hardware_test(cfg.clone(), "full", false) {
        Ok(()) => "full test done".into(),
        Err(e) => e.to_string(),
    }
}
fn act_scan(cfg: &AgentConfig) -> String {
    match crate::scanner::cmd_scan(cfg.clone()) {
        Ok(()) => "scan done".into(),
        Err(e) => e.to_string(),
    }
}
fn act_pm_list(_cfg: &AgentConfig) -> String {
    match crate::portmaster::fetch_catalog() {
        Ok(c) => {
            let mut s = String::new();
            for p in c.ports.iter().filter(|p| p.attr.rtr).take(20) {
                s.push_str(&format!("RTR  {}\n", p.attr.title));
            }
            for p in c.ports.iter().filter(|p| !p.attr.rtr).take(10) {
                s.push_str(&format!(" *   {}\n", p.attr.title));
            }
            s
        }
        Err(e) => e.to_string(),
    }
}
fn act_pm_installed(_cfg: &AgentConfig) -> String {
    let ports = crate::portmaster::list_installed();
    if ports.is_empty() {
        "no ports installed".into()
    } else {
        ports.join("\n")
    }
}
fn act_saves_pack(cfg: &AgentConfig) -> String {
    match crate::saves::cmd_pack(cfg.clone(), None) {
        Ok(()) => "saves packed (see /tmp)".into(),
        Err(e) => e.to_string(),
    }
}
fn act_saves_upload(cfg: &AgentConfig) -> String {
    match crate::saves::cmd_upload(cfg.clone()) {
        Ok(()) => "saves uploaded".into(),
        Err(e) => e.to_string(),
    }
}
fn act_features(cfg: &AgentConfig) -> String {
    match crate::features::cmd_show(cfg.clone()) {
        Ok(()) => "features printed".into(),
        Err(e) => e.to_string(),
    }
}
fn act_systems(_cfg: &AgentConfig) -> String {
    let mut s = String::new();
    for sp in playora_common::systems::SYSTEMS {
        s.push_str(&format!("{:<12} {}\n", sp.folder, sp.display_name));
    }
    s
}
fn act_self_update(_cfg: &AgentConfig) -> String {
    match crate::selfupdate::run("ropeixoto", "playora") {
        Ok(s) => s,
        Err(e) => e.to_string(),
    }
}
fn act_quit(_cfg: &AgentConfig) -> String {
    "bye".into()
}

const ACTIONS: &[MenuAction] = &[
    MenuAction {
        label: "[1] Status (device, sync, pending events)",
        run: act_status,
    },
    MenuAction {
        label: "[2] My Console (hardware snapshot)",
        run: act_hw,
    },
    MenuAction {
        label: "[3] Resource sample (CPU/mem now)",
        run: act_resources,
    },
    MenuAction {
        label: "[4] Sync now (send pending events to server)",
        run: act_sync,
    },
    MenuAction {
        label: "[5] Heartbeat (ping server)",
        run: act_heartbeat,
    },
    MenuAction {
        label: "[6] Hardware test (quick)",
        run: act_test_quick,
    },
    MenuAction {
        label: "[7] Hardware test (full)",
        run: act_test_full,
    },
    MenuAction {
        label: "[8] Scan ROMs and index in DB",
        run: act_scan,
    },
    MenuAction {
        label: "[9] PortMaster — list ports (Counter-Strike, Half-Life, ...)",
        run: act_pm_list,
    },
    MenuAction {
        label: "[a] PortMaster — installed",
        run: act_pm_installed,
    },
    MenuAction {
        label: "[b] Saves — pack tarball",
        run: act_saves_pack,
    },
    MenuAction {
        label: "[c] Saves — upload to server",
        run: act_saves_upload,
    },
    MenuAction {
        label: "[d] Features (show flags from server)",
        run: act_features,
    },
    MenuAction {
        label: "[e] Systems (folder/emulator/ext)",
        run: act_systems,
    },
    MenuAction {
        label: "[f] Update Playora from GitHub release",
        run: act_self_update,
    },
    MenuAction {
        label: "[q] Quit",
        run: act_quit,
    },
];

pub fn cmd_tui(cfg: AgentConfig, _screen: Option<String>) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let mut state = ListState::default();
    state.select(Some(0));
    let mut output = String::from("Use ↑/↓ + Enter. Esc/q sai.\nServer: ");
    output.push_str(&cfg.server_url);

    let res = loop {
        term.draw(|f| {
            let area = f.area();
            let cols = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Min(8),
                ])
                .split(area);

            let header = Paragraph::new(Line::from(format!(
                " Playora — {}  ({})  ",
                cfg.device_name, cfg.device_id
            )))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, cols[0]);

            let items: Vec<ListItem> = ACTIONS.iter().map(|a| ListItem::new(a.label)).collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" menu "))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, cols[1], &mut state);

            let out = Paragraph::new(output.as_str())
                .wrap(Wrap { trim: false })
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
                    state.select(Some((i + 1).min(ACTIONS.len() - 1)));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let sel = state.selected().unwrap_or(0);
                    output = (ACTIONS[sel].run)(&cfg);
                    if sel == ACTIONS.len() - 1 {
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
