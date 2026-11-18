use anyhow::Result;
use crossterm::event::{self, Event as Ev, KeyCode, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use oc_core::LOGO;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::io::stdout;
use std::time::Duration;

pub mod session;

/// The homepage: opencode wordmark, "Rust Edition" beneath it,
/// version in the bottom-right corner. Returns true to start a session.
pub fn home(version: &str) -> Result<bool> {
    terminal::enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;

    let edition = Line::from(vec![
        Span::styled("Rust Edition", Style::new().magenta().bold()),
        Span::styled(format!("  ·  v{version}"), Style::new().dim()),
    ]);

    let enter = loop {
        term.draw(|f| {
            let area = f.area();

            let logo = Paragraph::new(LOGO)
                .style(Style::new().cyan().bold())
                .alignment(Alignment::Center);
            let logo_h = LOGO.lines().count() as u16;
            let y = area.height.saturating_sub(logo_h + 4) / 2;
            f.render_widget(logo, Rect { x: 0, y, width: area.width, height: logo_h });

            let edition_area = Rect { y: y + logo_h + 1, height: 1, ..Rect::new(0, 0, area.width, 1) };
            f.render_widget(Paragraph::new(edition.clone()).alignment(Alignment::Center), edition_area);

            let bottom = Rect { y: area.height - 1, width: area.width, ..Rect::new(0, 0, area.width, 1) };
            f.render_widget(
                Paragraph::new(format!("v{version}"))
                    .style(Style::new().dim())
                    .alignment(Alignment::Right),
                bottom,
            );

            let hint = Rect { y: area.height.saturating_sub(3), width: area.width, ..Rect::new(0, 0, area.width, 1) };
            f.render_widget(
                Paragraph::new("enter · new session    q · quit")
                    .style(Style::new().dim())
                    .alignment(Alignment::Center),
                hint,
            );
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Ev::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Enter => break true,
                        KeyCode::Char('q') | KeyCode::Esc => break false,
                        _ => {}
                    }
                }
            }
        }
    };

    terminal::disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(enter)
}
