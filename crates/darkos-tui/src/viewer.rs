//! Scrollable text overlay viewer. Keybinds:
//! ↑↓ scroll · PgUp/PgDn page · Home/End jump · / search · n/N next · f snapshot · q/Esc dismiss

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io::{stdout, Read};
use std::path::{Path, PathBuf};

const FOOTER_HINT: &str =
    " ↑↓ scroll · PgUp/PgDn page · Home/End jump · / search · n/N next · f snapshot · q quit ";

pub struct ViewSource {
    pub title: String,
    pub lines: Vec<String>,
}

impl ViewSource {
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let p = path.as_ref();
        let body = std::fs::read_to_string(p)?;
        Ok(Self {
            title: format!(" {} ", p.display()),
            lines: body.lines().map(|s| s.to_string()).collect(),
        })
    }

    pub fn from_stdin() -> anyhow::Result<Self> {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(Self {
            title: " stdin ".into(),
            lines: buf.lines().map(|s| s.to_string()).collect(),
        })
    }
}

#[derive(Default)]
struct ViewerState {
    offset: usize,
    page_height: usize,
    search: Option<String>,
    search_input_active: bool,
    matches: Vec<usize>,
    cursor_match: Option<usize>,
    status: String,
}

pub fn view(source: ViewSource, snapshot_dir: Option<PathBuf>) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let mut state = ViewerState::default();
    let total = source.lines.len();

    let result = loop {
        term.draw(|f| {
            let area: Rect = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(area);

            let header = Paragraph::new(Line::from(vec![
                Span::styled("view ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&source.title),
                Span::styled(
                    format!(
                        "  ({}/{})",
                        state.offset.saturating_add(1).min(total.max(1)),
                        total
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            f.render_widget(header, chunks[0]);

            let body_area = chunks[1];
            state.page_height = body_area.height.saturating_sub(2) as usize;
            let end = (state.offset + state.page_height).min(total);
            let highlight_term = state.search.as_deref().unwrap_or("");
            let view_lines: Vec<Line> = source.lines[state.offset..end]
                .iter()
                .map(|raw| highlight_line(raw, highlight_term))
                .collect();
            let body = Paragraph::new(view_lines)
                .block(Block::default().borders(Borders::ALL))
                .wrap(Wrap { trim: false });
            f.render_widget(body, body_area);

            let footer_text = if state.search_input_active {
                format!(
                    " search: {}  (Enter confirm · Esc cancel)",
                    state.search.as_deref().unwrap_or("")
                )
            } else if !state.status.is_empty() {
                format!(" {} | {}", state.status, FOOTER_HINT)
            } else {
                FOOTER_HINT.into()
            };
            let footer = Paragraph::new(footer_text)
                .style(Style::default().fg(Color::Black).bg(Color::Gray));
            f.render_widget(footer, chunks[2]);
        })?;

        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            if state.search_input_active {
                match k.code {
                    KeyCode::Esc => {
                        state.search_input_active = false;
                        state.search = None;
                        state.matches.clear();
                        state.cursor_match = None;
                    }
                    KeyCode::Enter => {
                        state.search_input_active = false;
                        recompute_matches(&source, &mut state);
                        jump_to_match(&mut state, 0);
                    }
                    KeyCode::Backspace => {
                        if let Some(s) = state.search.as_mut() {
                            s.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        state.search.get_or_insert_with(String::new).push(c);
                    }
                    _ => {}
                }
                continue;
            }

            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok::<(), anyhow::Error>(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.offset + 1 + state.page_height <= total {
                        state.offset += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.offset = state.offset.saturating_sub(1);
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    let step = state.page_height.max(1);
                    state.offset = (state.offset + step).min(total.saturating_sub(step.max(1)));
                }
                KeyCode::PageUp => {
                    let step = state.page_height.max(1);
                    state.offset = state.offset.saturating_sub(step);
                }
                KeyCode::Home | KeyCode::Char('g') => state.offset = 0,
                KeyCode::End | KeyCode::Char('G') => {
                    state.offset = total.saturating_sub(state.page_height.max(1));
                }
                KeyCode::Char('/') => {
                    state.search_input_active = true;
                    state.search = Some(String::new());
                }
                KeyCode::Char('n') => {
                    let next = state.cursor_match.map(|i| i + 1).unwrap_or(0);
                    jump_to_match(&mut state, next);
                }
                KeyCode::Char('N') => {
                    if let Some(i) = state.cursor_match {
                        let prev = i.saturating_sub(1);
                        jump_to_match(&mut state, prev);
                    }
                }
                KeyCode::Char('f') => {
                    state.status = save_snapshot(&source, &state, snapshot_dir.as_deref())
                        .unwrap_or_else(|e| format!("snapshot failed: {e}"));
                }
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break Ok(()),
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    result
}

fn highlight_line<'a>(raw: &'a str, term: &str) -> Line<'a> {
    if term.is_empty() || !raw.contains(term) {
        return Line::from(Span::raw(raw));
    }
    let mut spans = Vec::new();
    let mut cur = 0;
    while let Some(pos) = raw[cur..].find(term) {
        let abs = cur + pos;
        if abs > cur {
            spans.push(Span::raw(&raw[cur..abs]));
        }
        spans.push(Span::styled(
            &raw[abs..abs + term.len()],
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ));
        cur = abs + term.len();
    }
    if cur < raw.len() {
        spans.push(Span::raw(&raw[cur..]));
    }
    Line::from(spans)
}

fn recompute_matches(src: &ViewSource, st: &mut ViewerState) {
    st.matches.clear();
    if let Some(term) = st.search.as_ref() {
        if term.is_empty() {
            return;
        }
        for (i, l) in src.lines.iter().enumerate() {
            if l.contains(term) {
                st.matches.push(i);
            }
        }
    }
    st.status = if st.matches.is_empty() {
        "0 matches".into()
    } else {
        format!("{} matches", st.matches.len())
    };
}

fn jump_to_match(st: &mut ViewerState, idx: usize) {
    if st.matches.is_empty() {
        return;
    }
    let wrapped = idx % st.matches.len();
    st.cursor_match = Some(wrapped);
    st.offset = st.matches[wrapped].saturating_sub(2);
}

fn save_snapshot(
    src: &ViewSource,
    st: &ViewerState,
    snapshot_dir: Option<&Path>,
) -> anyhow::Result<String> {
    let dir = snapshot_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/darkos-snapshots"));
    std::fs::create_dir_all(&dir)?;
    let ts = chrono_like_timestamp();
    let path = dir.join(format!("view-{ts}.txt"));
    let end = (st.offset + st.page_height.max(20)).min(src.lines.len());
    let slice = src.lines[st.offset..end].join("\n");
    std::fs::write(&path, slice)?;
    Ok(format!("snapshot → {}", path.display()))
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{s}")
}
