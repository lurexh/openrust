use crate::{Bash, Edit, Glob, Read, Write};
use anyhow::{Result, anyhow};
use oc_core::{Agent, Chat, Tool, ToolSpec};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

fn args(json: &str) -> Result<Value> {
    Ok(serde_json::from_str(json)?)
}

fn str_arg(v: &Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("missing string arg `{key}`"))
}

/// task: spawn a focused subagent with its own context window and tools.
/// The subagent cannot spawn further subagents (depth 1).
pub struct TaskTool {
    pub chat: Arc<dyn Chat>,
    pub cwd: PathBuf,
    pub base_system: String,
}

impl TaskTool {
    pub fn new(chat: Arc<dyn Chat>, cwd: impl Into<PathBuf>, base_system: impl Into<String>) -> Self {
        Self { chat, cwd: cwd.into(), base_system: base_system.into() }
    }
}

impl Tool for TaskTool {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "task",
                "description": "Spawn a focused subagent with its own context and tools to handle a self-contained task (research, multi-file refactor, exploration). It returns a summary of its work.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "description": { "type": "string", "description": "Short description of the task" },
                        "prompt": { "type": "string", "description": "Full instructions for the subagent" }
                    },
                    "required": ["description", "prompt"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let v = args(a)?;
        let description = str_arg(&v, "description")?;
        let prompt = str_arg(&v, "prompt")?;

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(Bash::new(self.cwd.clone())),
            Box::new(Read),
            Box::new(Write),
            Box::new(Edit),
            Box::new(Glob),
        ];

        let mut child = Agent::new(
            Arc::clone(&self.chat),
            format!(
                "You are a focused subagent. Your task: {description}\n{}",
                self.base_system
            ),
            tools,
        );
        child.max_steps = 10;

        let reply = child.run(&prompt, |_ev| {})?;
        Ok(if reply.is_empty() { "(subagent returned nothing)".into() } else { reply })
    }
}
