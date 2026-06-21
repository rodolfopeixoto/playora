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
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::io::{stdout, Stdout};

type Term = Terminal<CrosstermBackend<Stdout>>;

enum Screen {
    Main,
    Portmaster,
    Update,
}

struct App {
    cfg: AgentConfig,
    screen: Screen,
    output: String,
    main_state: ListState,
    pm_state: ListState,
    pm_catalog: Option<crate::portmaster::Catalog>,
    pm_filter_rtr: bool,
    update_channel: crate::selfupdate::Channel,
}

impl App {
    fn new(cfg: AgentConfig) -> Self {
        let mut s = Self {
            cfg,
            screen: Screen::Main,
            output: String::from("Use ↑/↓ + Enter. Esc = voltar. q = sair."),
            main_state: ListState::default(),
            pm_state: ListState::default(),
            pm_catalog: None,
            pm_filter_rtr: true,
            update_channel: crate::selfupdate::Channel::Stable,
        };
        s.main_state.select(Some(0));
        s.pm_state.select(Some(0));
        s
    }
}

struct MainAction {
    label: &'static str,
    run: fn(&mut App),
}

fn act_status(app: &mut App) {
    app.output = match crate::sync::cmd_status(app.cfg.clone()) {
        Ok(_) => "status printed".into(),
        Err(e) => e.to_string(),
    };
}
fn act_hw(app: &mut App) {
    app.output = serde_json::to_string_pretty(&crate::hw::snapshot()).unwrap_or_default();
}
fn act_resources(app: &mut App) {
    app.output = serde_json::to_string_pretty(&crate::resources::sample()).unwrap_or_default();
}
fn act_sync(app: &mut App) {
    app.output = match crate::sync::cmd_sync_once(app.cfg.clone()) {
        Ok(_) => "synced".into(),
        Err(e) => e.to_string(),
    };
}
fn act_heartbeat(app: &mut App) {
    app.output = match crate::sync::cmd_heartbeat(app.cfg.clone()) {
        Ok(_) => "heartbeat queued".into(),
        Err(e) => e.to_string(),
    };
}
fn act_test_quick(app: &mut App) {
    app.output = match crate::tests::cmd_hardware_test(app.cfg.clone(), "quick", false) {
        Ok(_) => "quick test done".into(),
        Err(e) => e.to_string(),
    };
}
fn act_test_full(app: &mut App) {
    app.output = match crate::tests::cmd_hardware_test(app.cfg.clone(), "full", false) {
        Ok(_) => "full test done".into(),
        Err(e) => e.to_string(),
    };
}
fn act_scan(app: &mut App) {
    app.output = match crate::scanner::cmd_scan(app.cfg.clone()) {
        Ok(_) => "scan done".into(),
        Err(e) => e.to_string(),
    };
}
fn act_saves_pack(app: &mut App) {
    app.output = match crate::saves::cmd_pack(app.cfg.clone(), None) {
        Ok(_) => "saves packed".into(),
        Err(e) => e.to_string(),
    };
}
fn act_saves_upload(app: &mut App) {
    app.output = match crate::saves::cmd_upload(app.cfg.clone()) {
        Ok(_) => "uploaded".into(),
        Err(e) => e.to_string(),
    };
}
fn act_features(app: &mut App) {
    app.output = match crate::features::cmd_show(app.cfg.clone()) {
        Ok(_) => "features printed".into(),
        Err(e) => e.to_string(),
    };
}
fn act_systems(app: &mut App) {
    let mut s = String::new();
    for sp in playora_common::systems::SYSTEMS {
        s.push_str(&format!("{:<12} {}\n", sp.folder, sp.display_name));
    }
    app.output = s;
}
fn act_open_pm(app: &mut App) {
    app.output = "loading PortMaster catalog...".into();
    match crate::portmaster::fetch_catalog() {
        Ok(c) => {
            app.pm_catalog = Some(c);
            app.screen = Screen::Portmaster;
            app.output = "catalog loaded".into();
        }
        Err(e) => app.output = format!("catalog error: {e}"),
    }
}
fn act_open_update(app: &mut App) {
    app.screen = Screen::Update;
    app.output = format!(
        "current: v{}\nchannel: {:?}\nEnter no item 'Check & install' pra atualizar.",
        env!("CARGO_PKG_VERSION"),
        app.update_channel
    );
}
fn act_quit(_app: &mut App) {}

const MAIN_ACTIONS: &[MainAction] = &[
    MainAction {
        label: "[1] PortMaster — instalar portas (CS, Half-Life, etc.)",
        run: act_open_pm,
    },
    MainAction {
        label: "[2] Update Playora (stable/beta)",
        run: act_open_update,
    },
    MainAction {
        label: "[3] Status",
        run: act_status,
    },
    MainAction {
        label: "[4] My Console (hardware)",
        run: act_hw,
    },
    MainAction {
        label: "[5] Resource sample",
        run: act_resources,
    },
    MainAction {
        label: "[6] Sync now",
        run: act_sync,
    },
    MainAction {
        label: "[7] Heartbeat",
        run: act_heartbeat,
    },
    MainAction {
        label: "[8] Hardware test (quick)",
        run: act_test_quick,
    },
    MainAction {
        label: "[9] Hardware test (full)",
        run: act_test_full,
    },
    MainAction {
        label: "[a] Scan ROMs",
        run: act_scan,
    },
    MainAction {
        label: "[b] Saves — pack",
        run: act_saves_pack,
    },
    MainAction {
        label: "[c] Saves — upload to server",
        run: act_saves_upload,
    },
    MainAction {
        label: "[d] Features",
        run: act_features,
    },
    MainAction {
        label: "[e] Systems (folder/emulator)",
        run: act_systems,
    },
    MainAction {
        label: "[q] Quit",
        run: act_quit,
    },
];

pub fn cmd_tui(cfg: AgentConfig, _screen: Option<String>) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term: Term = Terminal::new(backend)?;
    let mut app = App::new(cfg);

    let res = run_loop(&mut term, &mut app);

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    res
}

fn run_loop(term: &mut Term, app: &mut App) -> Result<()> {
    loop {
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

            let title = match app.screen {
                Screen::Main => format!(" Playora — {} ", app.cfg.device_name),
                Screen::Portmaster => format!(
                    " Playora › PortMaster ({} entries) ",
                    app.pm_catalog.as_ref().map(|c| c.ports.len()).unwrap_or(0)
                ),
                Screen::Update => format!(" Playora › Update (v{}) ", env!("CARGO_PKG_VERSION")),
            };
            let header = Paragraph::new(Line::from(Span::styled(
                title,
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, cols[0]);

            match app.screen {
                Screen::Main => {
                    let items: Vec<ListItem> = MAIN_ACTIONS
                        .iter()
                        .map(|a| ListItem::new(a.label))
                        .collect();
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title(" menu "))
                        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                        .highlight_symbol("▶ ");
                    f.render_stateful_widget(list, cols[1], &mut app.main_state);
                }
                Screen::Portmaster => {
                    let cat = app.pm_catalog.as_ref().unwrap();
                    let items: Vec<ListItem> = cat
                        .ports
                        .iter()
                        .filter(|p| !app.pm_filter_rtr || p.attr.rtr)
                        .map(|p| {
                            let title = if p.attr.title.is_empty() {
                                p.name.clone()
                            } else {
                                p.attr.title.clone()
                            };
                            let badge = if p.attr.rtr { "[RTR]" } else { "[DATA]" };
                            ListItem::new(format!("{:<6} {}", badge, title))
                        })
                        .collect();
                    let title = if app.pm_filter_rtr {
                        " ports (ready-to-run) — F=ver todos "
                    } else {
                        " ports (all) — F=só RTR "
                    };
                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title(title))
                        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                        .highlight_symbol("▶ ");
                    f.render_stateful_widget(list, cols[1], &mut app.pm_state);
                }
                Screen::Update => {
                    let labels: Vec<ListItem> = vec![
                        ListItem::new(format!(
                            "Channel: {:?}  (Tab pra alternar stable/beta)",
                            app.update_channel
                        )),
                        ListItem::new("Check & install latest"),
                        ListItem::new("Back"),
                    ];
                    let list = List::new(labels)
                        .block(Block::default().borders(Borders::ALL).title(" update "))
                        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                        .highlight_symbol("▶ ");
                    f.render_stateful_widget(list, cols[1], &mut app.main_state);
                }
            }

            let footer = Paragraph::new(app.output.as_str())
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(" output "));
            f.render_widget(footer, cols[2]);
        })?;

        if let CtEvent::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            if matches!(k.code, KeyCode::Char('q')) {
                return Ok(());
            }
            match app.screen {
                Screen::Main => {
                    if main_handle(k.code, app) {
                        return Ok(());
                    }
                }
                Screen::Portmaster => pm_handle(k.code, app),
                Screen::Update => up_handle(k.code, app),
            }
        }
    }
}

fn main_handle(code: KeyCode, app: &mut App) -> bool {
    match code {
        KeyCode::Esc => return false,
        KeyCode::Down | KeyCode::Char('j') => {
            let i = app.main_state.selected().unwrap_or(0);
            app.main_state
                .select(Some((i + 1).min(MAIN_ACTIONS.len() - 1)));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let i = app.main_state.selected().unwrap_or(0);
            app.main_state.select(Some(i.saturating_sub(1)));
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let i = app.main_state.selected().unwrap_or(0);
            (MAIN_ACTIONS[i].run)(app);
            if i == MAIN_ACTIONS.len() - 1 {
                return true;
            }
        }
        _ => {}
    }
    false
}

fn pm_handle(code: KeyCode, app: &mut App) {
    let cat = app.pm_catalog.as_ref().unwrap().clone();
    let filtered: Vec<_> = cat
        .ports
        .iter()
        .filter(|p| !app.pm_filter_rtr || p.attr.rtr)
        .collect();
    match code {
        KeyCode::Esc => {
            app.screen = Screen::Main;
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.pm_filter_rtr = !app.pm_filter_rtr;
            app.pm_state.select(Some(0));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let i = app.pm_state.selected().unwrap_or(0);
            app.pm_state
                .select(Some((i + 1).min(filtered.len().saturating_sub(1))));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let i = app.pm_state.selected().unwrap_or(0);
            app.pm_state.select(Some(i.saturating_sub(1)));
        }
        KeyCode::Enter => {
            if let Some(i) = app.pm_state.selected() {
                if let Some(entry) = filtered.get(i) {
                    let name = if entry.attr.title.is_empty() {
                        entry.name.clone()
                    } else {
                        entry.attr.title.clone()
                    };
                    app.output = format!("installing {name} ...");
                    let res = crate::portmaster::install(entry, |dl, total| {
                        if let Some(t) = total {
                            let pct = (dl as f64 / t as f64) * 100.0;
                            let _ = std::io::Write::write_all(
                                &mut std::io::stderr(),
                                format!("\r{pct:.0}% ").as_bytes(),
                            );
                        }
                    });
                    app.output = match res {
                        Ok(r) => format!(
                            "installed {} ({} files){}",
                            r.port_name,
                            r.installed_files,
                            if r.requires_data {
                                " — NEEDS game data files in /roms/ports/<port>/"
                            } else {
                                ""
                            }
                        ),
                        Err(e) => format!("install error: {e}"),
                    };
                }
            }
        }
        _ => {}
    }
}

fn up_handle(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc => {
            app.screen = Screen::Main;
            app.main_state.select(Some(0));
        }
        KeyCode::Tab => {
            app.update_channel = match app.update_channel {
                crate::selfupdate::Channel::Stable => crate::selfupdate::Channel::Beta,
                crate::selfupdate::Channel::Beta => crate::selfupdate::Channel::Stable,
            };
            app.output = format!("channel agora: {:?}", app.update_channel);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let i = app.main_state.selected().unwrap_or(0);
            app.main_state.select(Some((i + 1).min(2)));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let i = app.main_state.selected().unwrap_or(0);
            app.main_state.select(Some(i.saturating_sub(1)));
        }
        KeyCode::Enter => {
            let i = app.main_state.selected().unwrap_or(0);
            if i == 1 {
                app.output = "fetching update...".into();
                app.output = match crate::selfupdate::run_channel(
                    "ropeixoto",
                    "playora",
                    app.update_channel,
                ) {
                    Ok(s) => s,
                    Err(e) => e.to_string(),
                };
            } else if i == 2 {
                app.screen = Screen::Main;
                app.main_state.select(Some(0));
            }
        }
        _ => {}
    }
}
