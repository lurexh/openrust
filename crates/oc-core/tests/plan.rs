use oc_core::{Agent, AssistantTurn, Chat, Message, Tool, ToolSpec};
use anyhow::Result;
use std::sync::Mutex;

struct RecordingChat {
    tool_counts: Mutex<Vec<usize>>,
}

impl Chat for RecordingChat {
    fn chat(&self, _m: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn> {
        self.tool_counts.lock().unwrap().push(tools.len());
        Ok(AssistantTurn { content: "ok".into(), tool_calls: vec![] })
    }
}

struct SimpleTool {
    name: &'static str,
}

impl Tool for SimpleTool {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {"name": self.name, "description": "t", "parameters": {"type": "object"}}
        }))
        .unwrap()
    }
    fn execute(&self, _a: &str) -> Result<String> {
        Ok("ok".into())
    }
}

#[test]
fn plan_mode_hides_unsafe_tools() {
    let chat = RecordingChat { tool_counts: Mutex::new(Vec::new()) };
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(SimpleTool { name: "read" }),
        Box::new(SimpleTool { name: "glob" }),
        Box::new(SimpleTool { name: "bash" }),
        Box::new(SimpleTool { name: "write" }),
        Box::new(SimpleTool { name: "task" }),
    ];
    let mut agent = Agent::new(chat, "sys", tools);
    agent.plan_mode = true;
    agent.run("plan please", |_| {}).unwrap();
    let counts = agent.chat.tool_counts.lock().unwrap();
    assert_eq!(counts.last(), Some(&2), "only read+glob visible in plan mode");
}

#[test]
fn normal_mode_shows_all_tools() {
    let chat = RecordingChat { tool_counts: Mutex::new(Vec::new()) };
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(SimpleTool { name: "read" }),
        Box::new(SimpleTool { name: "glob" }),
        Box::new(SimpleTool { name: "bash" }),
        Box::new(SimpleTool { name: "write" }),
        Box::new(SimpleTool { name: "task" }),
    ];
    let mut agent = Agent::new(chat, "sys", tools);
    agent.run("go", |_| {}).unwrap();
    let counts = agent.chat.tool_counts.lock().unwrap();
    assert_eq!(counts.last(), Some(&5));
}
