//! Terminal UI for darkOs — minimal menu over crossterm + ratatui.
//! Designed for 640x480 framebuffer fbterm/console on the R36S.

pub mod viewer;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io::stdout;

pub struct MenuItem {
    pub label: &'static str,
    pub action: Action,
}

pub enum Action {
    HwSnapshot,
    StorageReport,
    ScanRoms,
    SaveSnapshot,
    ThemeList,
    UpdateApt,
    PerfBalanced,
    PerfPerformance,
    PerfPowerSave,
    Quit,
}

pub fn run() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let items = vec![
        MenuItem {
            label: "Hardware snapshot",
            action: Action::HwSnapshot,
        },
        MenuItem {
            label: "Storage report",
            action: Action::StorageReport,
        },
        MenuItem {
            label: "Scan ROMs into DB",
            action: Action::ScanRoms,
        },
        MenuItem {
            label: "Snapshot all saves",
            action: Action::SaveSnapshot,
        },
        MenuItem {
            label: "List themes",
            action: Action::ThemeList,
        },
        MenuItem {
            label: "Check apt updates",
            action: Action::UpdateApt,
        },
        MenuItem {
            label: "Perf: powersave",
            action: Action::PerfPowerSave,
        },
        MenuItem {
            label: "Perf: balanced",
            action: Action::PerfBalanced,
        },
        MenuItem {
            label: "Perf: performance",
            action: Action::PerfPerformance,
        },
        MenuItem {
            label: "Quit",
            action: Action::Quit,
        },
    ];
    let mut state = ListState::default();
    state.select(Some(0));
    let mut last_output = String::from("ready.");

    let result = loop {
        term.draw(|f| {
            let size: Rect = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(10),
                ])
                .split(size);

            let header = Paragraph::new(Line::from(vec![
                Span::styled("darkOs ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("— R36S clone control panel"),
            ]))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            let list_items: Vec<ListItem> = items.iter().map(|i| ListItem::new(i.label)).collect();
            let list = List::new(list_items)
                .block(Block::default().borders(Borders::ALL).title(" menu "))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, chunks[1], &mut state);

            let out = Paragraph::new(last_output.as_str())
                .block(Block::default().borders(Borders::ALL).title(" output "));
            f.render_widget(out, chunks[2]);
        })?;

        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok::<(), anyhow::Error>(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1).min(items.len() - 1)));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(i) = state.selected() {
                        last_output = run_action(&items[i].action);
                        if matches!(items[i].action, Action::Quit) {
                            break Ok(());
                        }
                    }
                }
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    result
}

fn run_action(a: &Action) -> String {
    match a {
        Action::HwSnapshot => match darkos_hw::snapshot() {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("hw error: {e}"),
        },
        Action::StorageReport => {
            let roots = ["/roms", "/", "/boot"];
            let mut out = String::new();
            for r in roots {
                if let Ok(u) = darkos_storage::disk_usage(r) {
                    out.push_str(&format!(
                        "{}: used {:.0}% ({}/{} free {})\n",
                        u.path,
                        u.used_pct,
                        bytesize::ByteSize(u.used_bytes),
                        bytesize::ByteSize(u.total_bytes),
                        bytesize::ByteSize(u.free_bytes),
                    ));
                }
            }
            out
        }
        Action::ScanRoms => {
            let paths = darkos_core::Paths::default();
            let _ = paths.ensure();
            match darkos_db::Db::open(&paths.db_path) {
                Err(e) => format!("db open: {e}"),
                Ok(db) => match darkos_roms::scan_into_db(
                    std::path::Path::new(&paths.roms_dir),
                    &db,
                    false,
                ) {
                    Ok(n) => format!(
                        "indexed {n} roms\nsummary:\n{:?}",
                        db.count_roms_by_system()
                    ),
                    Err(e) => format!("scan: {e}"),
                },
            }
        }
        Action::SaveSnapshot => {
            let paths = darkos_core::Paths::default();
            match darkos_saves::snapshot(
                std::path::Path::new(&paths.roms_dir),
                std::path::Path::new(&paths.cache_dir),
            ) {
                Ok(p) => format!("snapshot at {}", p.display()),
                Err(e) => format!("snapshot error: {e}"),
            }
        }
        Action::ThemeList => match darkos_themes::list_installed() {
            Ok(v) => format!("installed themes:\n{}", v.join("\n")),
            Err(e) => format!("themes: {e}"),
        },
        Action::UpdateApt => match darkos_update::apt_update_available() {
            Ok(n) => format!("{n} apt updates available"),
            Err(e) => format!("update probe: {e}"),
        },
        Action::PerfPowerSave => apply_perf(darkos_perf::Profile::PowerSave),
        Action::PerfBalanced => apply_perf(darkos_perf::Profile::Balanced),
        Action::PerfPerformance => apply_perf(darkos_perf::Profile::Performance),
        Action::Quit => "bye".into(),
    }
}

fn apply_perf(p: darkos_perf::Profile) -> String {
    match darkos_perf::apply_profile(p) {
        Ok(()) => "perf applied".into(),
        Err(e) => format!("perf: {e}"),
    }
}
