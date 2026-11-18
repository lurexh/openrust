use oc_core::permissions::{AllowAll, Decision, Permission};
use oc_core::{Agent, AssistantTurn, Chat, Event, Message, Tool, ToolSpec};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct OneToolChat;

impl Chat for OneToolChat {
    fn chat(&self, _m: &[Message], _t: &[ToolSpec]) -> Result<AssistantTurn> {
        Ok(AssistantTurn {
            content: String::new(),
            tool_calls: vec![serde_json::from_value(serde_json::json!({
                "id": "c1", "type": "function",
                "function": {"name": "risky", "arguments": "{}"}
            }))?],
        })
    }
}

struct RiskyTool(Arc<AtomicBool>);

impl Tool for RiskyTool {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {"name": "risky", "description": "risky", "parameters": {"type": "object"}}
        }))
        .unwrap()
    }
    fn execute(&self, _a: &str) -> Result<String> {
        self.0.store(true, Ordering::SeqCst);
        Ok("ran".into())
    }
}

struct DenyAll;
impl Permission for DenyAll {
    fn check(&self, _: &str, _: &str) -> Decision {
        Decision::Deny
    }
}

fn agent_with(ran: Arc<AtomicBool>, perm: Option<Arc<dyn Permission>>) -> Agent<OneToolChat> {
    let mut a = Agent::new(OneToolChat, "sys", vec![Box::new(RiskyTool(ran))]);
    a.permissions = perm;
    a
}

#[test]
fn denied_tool_never_executes() {
    let ran = Arc::new(AtomicBool::new(false));
    let mut agent = agent_with(Arc::clone(&ran), Some(Arc::new(DenyAll)));
    let mut denied_seen = false;
    agent.run("go", |ev| {
        if let Event::ToolResult { ok, .. } = ev {
            denied_seen |= !ok;
        }
    });
    assert!(!ran.load(Ordering::SeqCst));
    assert!(denied_seen);
}

#[test]
fn allow_all_permission_runs_tool() {
    let ran = Arc::new(AtomicBool::new(false));
    let mut agent = agent_with(Arc::clone(&ran), Some(Arc::new(AllowAll)));
    agent.run("go", |_| {});
    assert!(ran.load(Ordering::SeqCst));
}
