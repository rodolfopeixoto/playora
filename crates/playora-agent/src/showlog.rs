use anyhow::Result;
use crossterm::{
    event::{self, Event as CtEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io::stdout;
use std::path::Path;

pub fn cmd(file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| format!("(error reading {}: {e})", file.display()));
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;
    let mut scroll: u16 = 0;

    let res = loop {
        term.draw(|f| {
            let area = f.area();
            let cols = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(3),
                ])
                .split(area);

            let header = Paragraph::new(Line::from(format!(" Playora — {} ", file.display())))
                .style(
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Cyan),
                )
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, cols[0]);

            let body = Paragraph::new(text.as_str())
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0))
                .block(Block::default().borders(Borders::ALL).title(" output "));
            f.render_widget(body, cols[1]);

            let footer = Paragraph::new(Line::from(" ↑/↓ scroll  •  START/B/Q exit "))
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, cols[2]);
        })?;

        if let CtEvent::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Char('q')
                | KeyCode::Esc
                | KeyCode::Enter
                | KeyCode::Char(' ')
                | KeyCode::Char('b')
                | KeyCode::Backspace => break Ok::<(), anyhow::Error>(()),
                KeyCode::Down | KeyCode::Char('j') => scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => scroll = scroll.saturating_add(10),
                KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                _ => {}
            }
        }
    };
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    res
}
