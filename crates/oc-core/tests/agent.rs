use oc_core::{Agent, AssistantTurn, Chat, Event, Message, Tool, ToolSpec};
use anyhow::Result;

struct FakeChat {
    calls: std::sync::atomic::AtomicUsize,
}

impl Chat for FakeChat {
    fn chat(&self, _m: &[Message], _t: &[ToolSpec]) -> Result<AssistantTurn> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n == 0 {
            Ok(AssistantTurn {
                content: String::new(),
                tool_calls: vec![serde_json::from_value(serde_json::json!({
                    "id": "call1", "type": "function",
                    "function": {"name": "echo", "arguments": "{\"x\":1}"}
                }))?],
            })
        } else {
            Ok(AssistantTurn { content: "done".into(), tool_calls: vec![] })
        }
    }
}

struct EchoTool;

impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {"name": "echo", "description": "echo", "parameters": {"type": "object"}}
        }))
        .unwrap()
    }
    fn execute(&self, a: &str) -> Result<String> {
        Ok(format!("echoed {a}"))
    }
}

#[test]
fn agent_tool_loop() {
    let mut agent = Agent::new(
        FakeChat { calls: std::sync::atomic::AtomicUsize::new(0) },
        "sys",
        vec![Box::new(EchoTool)],
    );
    let mut tool_ran = false;
    let reply = agent
        .run("go", |ev| {
            if let Event::ToolResult { ok, .. } = ev {
                tool_ran = ok;
            }
        })
        .unwrap();
    assert_eq!(reply, "done");
    assert!(tool_ran);
    assert!(agent.history.iter().any(|m| m.role == oc_core::Role::Tool));
}
