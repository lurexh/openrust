use anyhow::Result;
use crossterm::event::{self, Event as Ev, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use oc_core::permissions::Decision;
use oc_core::{Agent, Chat, Event as AgentEvent, InteractivePermission};
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};
use std::collections::HashSet;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

enum UiEv {
    Delta(String),
    ToolCall(String, String),
    ToolResult(String, bool),
    Permission(String, String, mpsc::Sender<Decision>),
    Note(String),
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    User,
    Assistant,
    Tool,
    Note,
}

#[derive(Debug)]
struct Msg {
    role: Role,
    text: String,
}

struct Ui {
    msgs: Vec<Msg>,
    streaming: Option<String>,
    input: String,
    offset: usize,
    auto_scroll: bool,
    busy: bool,
    model: String,
    version: String,
    status: String,
    pending: Option<Pending>,
}

struct Pending {
    tool: String,
    args: String,
    resp: mpsc::Sender<Decision>,
}

impl Ui {
    fn new(model: String, version: String, resumed: usize) -> Self {
        let mut u = Self {
            msgs: Vec::new(),
            streaming: None,
            input: String::new(),
            offset: 0,
            auto_scroll: true,
            busy: false,
            model,
            version,
            status: String::new(),
            pending: None,
        };
        if resumed > 0 {
            u.note(format!("[session resumed — {resumed} messages]"));
        }
        u
    }

    fn push(&mut self, role: Role, text: impl Into<String>) {
        self.msgs.push(Msg { role, text: text.into() });
    }

    fn note(&mut self, text: impl Into<String>) {
        self.push(Role::Note, text);
    }

    fn flush_stream(&mut self) {
        if let Some(s) = self.streaming.take() {
            if !s.is_empty() {
                self.push(Role::Assistant, s);
            }
        }
    }

    fn wrapped_count(&self, text: &str, w: u16) -> usize {
        let disp = UnicodeWidthStr::width(text) as usize;
        let w = w.max(1) as usize;
        disp.saturating_sub(1) / w + 1
    }

    fn total_lines(&self, w: u16) -> usize {
        let mut n = self.msgs.iter().map(|m| self.wrapped_count(&m.text, w)).sum::<usize>();
        if let Some(s) = &self.streaming {
            n += self.wrapped_count(s, w);
        }
        n
    }
}

pub fn run<C: Chat + 'static>(mut agent: Agent<C>, cwd: PathBuf, model: String, version: String) -> Result<()> {
    let resumed = agent.history.len().saturating_sub(1);

    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;

    let (in_tx, in_rx) = mpsc::channel::<String>();
    let (ev_tx, ev_rx) = mpsc::channel::<UiEv>();

    // permission handler: asks the UI, caches "always allow" per tool
    struct Shared {
        tx: mpsc::Sender<UiEv>,
        cache: Mutex<HashSet<String>>,
    }
    let shared = Arc::new(Shared { tx: ev_tx.clone(), cache: Mutex::new(HashSet::new()) });
    let s2 = Arc::clone(&shared);
    agent.permissions = Some(Arc::new(InteractivePermission {
        ask: Box::new(move |tool: &str, args: &str| -> Decision {
            if oc_core::permissions::is_safe(tool) {
                return Decision::Allow;
            }
            if s2.cache.lock().unwrap().contains(tool) {
                return Decision::Allow;
            }
            let (rtx, rrx) = mpsc::channel::<Decision>();
            let _ = s2.tx.send(UiEv::Permission(tool.to_string(), args.to_string(), rtx));
            rrx.recv().unwrap_or(Decision::Deny)
        }),
    }));

    std::thread::spawn(move || {
        let normal_system = agent.history.first().and_then(|m| m.content.clone()).unwrap_or_default();
        let plan_system = format!(
            "{normal_system} You are in PLAN MODE: research the codebase and propose a concrete \
             implementation plan. Read-only tools only — do not modify anything. End with the plan \
             as a numbered list."
        );
        while let Ok(input) = in_rx.recv() {
            match input.as_str() {
                "/plan" => {
                    agent.plan_mode = !agent.plan_mode;
                    if agent.plan_mode {
                        agent.set_system(plan_system.clone());
                        let _ = ev_tx.send(UiEv::Note("plan mode ON — read-only tools; agent proposes a plan".into()));
                    } else {
                        agent.set_system(normal_system.clone());
                        let _ = ev_tx.send(UiEv::Note("plan mode OFF — full tools restored".into()));
                    }
                    continue;
                }
                "/reset" => {
                    agent.reset();
                    oc_core::session::clear_history(&cwd);
                    let _ = ev_tx.send(UiEv::Note("session cleared".into()));
                    continue;
                }
                _ => {}
            }
            let tx = ev_tx.clone();
            let _ = agent.run(&input, |ev| {
                let _ = match ev {
                    AgentEvent::Delta(d) => tx.send(UiEv::Delta(d.to_string())),
                    AgentEvent::ToolCall { name, args } => {
                        tx.send(UiEv::ToolCall(name.to_string(), args.to_string()))
                    }
                    AgentEvent::ToolResult { name, ok } => tx.send(UiEv::ToolResult(name.to_string(), ok)),
                    AgentEvent::Reply(_) => Ok(()),
                };
            });
            oc_core::session::save_history(&cwd, &agent.history);
            let _ = ev_tx.send(UiEv::Done);
        }
    });

    let mut ui = Ui::new(model, version, resumed);

    loop {
        // drain events
        while let Ok(ev) = ev_rx.try_recv() {
            match ev {
                UiEv::Delta(d) => {
                    ui.streaming.get_or_insert_with(String::new).push_str(&d);
                }
                UiEv::ToolCall(name, args) => {
                    ui.flush_stream();
                    ui.push(Role::Tool, format!("· {name} {args}"));
                }
                UiEv::ToolResult(name, ok) => {
                    ui.push(Role::Tool, format!("  ↳ {name} {}", if ok { "ok" } else { "failed" }));
                }
                UiEv::Done => {
                    ui.flush_stream();
                    ui.busy = false;
                    ui.status.clear();
                }
                UiEv::Permission(tool, args, resp) => {
                    ui.flush_stream();
                    ui.pending = Some(Pending { tool, args, resp });
                }
                UiEv::Note(text) => {
                    ui.note(text);
                }
            }
        }

        let area_w = term.size()?.width;
        let total = ui.total_lines(area_w.saturating_sub(2));
        let view_h = (term.size()?.height.saturating_sub(4)) as usize;
        if ui.auto_scroll {
            ui.offset = total.saturating_sub(view_h);
        }
        ui.offset = ui.offset.min(total.saturating_sub(view_h).max(0));

        term.draw(|f| {
            let area = f.area();

            // header
            let header = Line::from(vec![
                Span::styled("openrust", Style::new().cyan().bold()),
                Span::styled(" Rust Edition", Style::new().magenta()),
                Span::styled(format!("  v{}", ui.version), Style::new().dim()),
                Span::styled(format!("  ·  {}  ·", ui.model), Style::new().dim()),
                Span::styled(
                    if ui.busy { format!(" {} ", ui.status) } else { String::new() },
                    Style::new().yellow(),
                ),
            ]);
            f.render_widget(Paragraph::new(header), Rect { x: 0, y: 0, width: area.width, height: 1 });

            // chat body
            let mut lines: Vec<Line> = Vec::new();
            for m in &ui.msgs {
                let style = match m.role {
                    Role::User => Style::new().blue().bold(),
                    Role::Assistant => Style::new(),
                    Role::Tool => Style::new().dim(),
                    Role::Note => Style::new().dim().italic(),
                };
                for l in m.text.split('\n') {
                    lines.push(Line::styled(l.to_string(), style));
                }
                lines.push(Line::raw(""));
            }
            if let Some(s) = &ui.streaming {
                for l in s.split('\n') {
                    lines.push(Line::raw(l.to_string()));
                }
            }

            let body = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((ui.offset as u16, 0));
            let body_area = Rect {
                x: 0,
                y: 1,
                width: area.width,
                height: area.height.saturating_sub(3),
            };
            f.render_widget(body, body_area);

            // input or permission prompt
            let input_area = Rect { x: 0, y: area.height - 2, width: area.width, height: 1 };
            if let Some(p) = &ui.pending {
                let short: String = p.args.chars().take(60).collect();
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("? ", Style::new().yellow().bold()),
                        Span::styled(p.tool.clone(), Style::new().yellow().bold()),
                        Span::raw(format!(" {short}  ")),
                        Span::styled("[y] allow  [n] deny  [a] always", Style::new().dim()),
                    ])),
                    input_area,
                );
            } else {
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("> ", Style::new().green().bold()),
                        Span::raw(ui.input.clone()),
                        Span::styled("█", Style::new().dim()),
                    ])),
                    input_area,
                );
            }
        })?;

        if event::poll(Duration::from_millis(80))? {
            match event::read()? {
                Ev::Key(k) if k.kind == KeyEventKind::Press => {
                    if let Some(p) = ui.pending.take() {
                        // permission modal captures all keys
                        match k.code {
                            KeyCode::Char('y') | KeyCode::Enter => {
                                let _ = p.resp.send(Decision::Allow);
                            }
                            KeyCode::Char('a') => {
                                shared.cache.lock().unwrap().insert(p.tool.clone());
                                let _ = p.resp.send(Decision::Allow);
                            }
                            _ => {
                                let _ = p.resp.send(Decision::Deny);
                            }
                        }
                        continue;
                    }
                    match (k.modifiers, k.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => break,
                    (_, KeyCode::Enter) => {
                        let text = ui.input.trim().to_string();
                        if !text.is_empty() {
                            ui.input.clear();
                            ui.auto_scroll = true;
                            if text.starts_with('/') {
                                // command: don't echo as user message
                                let _ = in_tx.send(text);
                            } else {
                                ui.flush_stream();
                                ui.push(Role::User, format!("you  {}", text));
                                ui.busy = true;
                                ui.status = "thinking…".into();
                                let _ = in_tx.send(text);
                            }
                        }
                    }
                    (_, KeyCode::Backspace) => {
                        ui.input.pop();
                    }
                    (_, KeyCode::Up) => {
                        ui.offset = ui.offset.saturating_sub(1);
                        ui.auto_scroll = false;
                    }
                    (_, KeyCode::Down) => {
                        ui.offset += 1;
                        if ui.offset >= total.saturating_sub(view_h) {
                            ui.auto_scroll = true;
                        }
                    }
                    (_, KeyCode::PageUp) => {
                        ui.offset = ui.offset.saturating_sub(view_h);
                        ui.auto_scroll = false;
                    }
                    (_, KeyCode::PageDown) => {
                        ui.offset += view_h;
                        if ui.offset >= total.saturating_sub(view_h) {
                            ui.auto_scroll = true;
                        }
                    }
                    (_, KeyCode::Char(c)) => {
                        ui.input.push(c);
                    }
                    _ => {}
                    }
                }
                Ev::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        ui.offset = ui.offset.saturating_sub(3);
                        ui.auto_scroll = false;
                    }
                    MouseEventKind::ScrollDown => {
                        ui.offset += 3;
                        if ui.offset >= total.saturating_sub(view_h) {
                            ui.auto_scroll = true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // cleanup
    terminal::disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
    std::process::exit(0);
}
