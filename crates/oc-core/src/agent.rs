use crate::message::{Message, ToolSpec};
use crate::permissions::{Decision, Permission};
use crate::provider::Chat;
use anyhow::{Result, anyhow};

pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn execute(&self, args_json: &str) -> Result<String>;
}

pub struct Agent<C: Chat> {
    pub chat: C,
    pub tools: Vec<Box<dyn Tool>>,
    pub history: Vec<Message>,
    pub max_steps: usize,
    pub permissions: Option<std::sync::Arc<dyn Permission>>,
    pub plan_mode: bool,
}

pub enum Event<'a> {
    Delta(&'a str),
    ToolCall { name: &'a str, args: &'a str },
    ToolResult { name: &'a str, ok: bool },
    Reply(&'a str),
}

impl<C: Chat> Agent<C> {
    pub fn new(chat: C, system: impl Into<String>, tools: Vec<Box<dyn Tool>>) -> Self {
        Self {
            chat,
            tools,
            history: vec![Message::system(system)],
            max_steps: 12,
            permissions: None,
            plan_mode: false,
        }
    }

    pub fn set_system(&mut self, system: impl Into<String>) {
        self.history[0] = Message::system(system);
    }

    pub fn reset(&mut self) {
        self.history.truncate(1);
    }

    pub fn swap_chat(&mut self, chat: C) {
        self.chat = chat;
    }

    pub fn run(&mut self, input: &str, mut on_event: impl FnMut(Event)) -> Result<String> {
        self.history.push(Message::user(input));
        let mut specs: Vec<ToolSpec> = self.tools.iter().map(|t| t.spec()).collect();
        if self.plan_mode {
            specs.retain(|s| crate::permissions::is_safe(&s.function.name));
        }

        for _ in 0..self.max_steps {
            let streamed = std::cell::Cell::new(false);
            let turn = match self.chat.chat_stream(&self.history, &specs, &mut |d| {
                streamed.set(true);
                on_event(Event::Delta(d));
            }) {
                Ok(t) => t,
                Err(e) if streamed.get() => {
                    return Err(anyhow::anyhow!("stream interrupted: {e}"));
                }
                Err(e) => match self.chat.chat(&self.history, &specs) {
                    Ok(t) => t,
                    Err(e2) => return Err(anyhow::anyhow!("stream: {e}; non-stream: {e2}")),
                },
            };

            if turn.tool_calls.is_empty() {
                self.history.push(Message::assistant(turn.content.clone(), vec![]));
                on_event(Event::Reply(&turn.content));
                return Ok(turn.content);
            }

            self.history
                .push(Message::assistant(turn.content.clone(), turn.tool_calls.clone()));

            for tc in turn.tool_calls {
                on_event(Event::ToolCall { name: &tc.function.name, args: &tc.function.arguments });

                let denied = self
                    .permissions
                    .as_ref()
                    .map(|p| matches!(p.check(&tc.function.name, &tc.function.arguments), Decision::Deny))
                    .unwrap_or(false);

                let (result, ok) = if denied {
                    (format!("user denied permission to run {}", tc.function.name), false)
                } else {
                    match self.tools.iter().find(|t| t.spec().function.name == tc.function.name) {
                        Some(tool) => match tool.execute(&tc.function.arguments) {
                            Ok(out) => (out, true),
                            Err(e) => (format!("error: {e}"), false),
                        },
                        None => (format!("error: unknown tool `{}`", tc.function.name), false),
                    }
                };
                on_event(Event::ToolResult { name: &tc.function.name, ok });
                self.history.push(Message::tool(tc.id, tc.function.name, result));
            }
        }

        Err(anyhow!("agent exceeded {} steps", self.max_steps))
    }
}
