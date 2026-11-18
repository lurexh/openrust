use oc_core::{Message, Role};
use oc_provider::{parse_response, to_anthropic};
use serde_json::json;

#[test]
fn system_and_user_mapping() {
    let msgs = vec![Message::system("be brief"), Message::user("hi")];
    let (system, out) = to_anthropic(&msgs);
    assert_eq!(system, "be brief");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["role"], "user");
    assert_eq!(out[0]["content"][0]["text"], "hi");
}

#[test]
fn tool_calls_and_results_mapping() {
    let call = serde_json::from_value(json!({
        "id": "t1", "type": "function",
        "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}
    }))
    .unwrap();
    let msgs = vec![
        Message::user("list files"),
        Message::assistant("", vec![call]),
        Message::tool("t1", "bash", "a.rs\nb.rs"),
        Message::tool("t2", "bash", "more"),
    ];
    let (_, out) = to_anthropic(&msgs);
    assert_eq!(out.len(), 3);
    // assistant carries tool_use
    assert_eq!(out[1]["content"][0]["type"], "tool_use");
    assert_eq!(out[1]["content"][0]["name"], "bash");
    assert_eq!(out[1]["content"][0]["input"]["command"], "ls");
    // two consecutive tool results merge into ONE user message
    assert_eq!(out[2]["role"], "user");
    assert_eq!(out[2]["content"].as_array().unwrap().len(), 2);
    assert_eq!(out[2]["content"][0]["tool_use_id"], "t1");
    assert_eq!(out[2]["content"][1]["tool_use_id"], "t2");
}

#[test]
fn parse_response_text_and_tools() {
    let v = json!({
        "content": [
            {"type": "text", "text": "running "},
            {"type": "tool_use", "id": "t9", "name": "read", "input": {"path": "x.rs"}}
        ]
    });
    let turn = parse_response(&v);
    assert_eq!(turn.content, "running ");
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.tool_calls[0].id, "t9");
    assert_eq!(turn.tool_calls[0].function.name, "read");
    assert!(turn.tool_calls[0].function.arguments.contains("\"path\""));
}

#[test]
fn parse_response_empty() {
    let turn = parse_response(&json!({"content": []}));
    assert_eq!(turn.content, "");
    assert!(turn.tool_calls.is_empty());
}

#[test]
fn role_serialization_matches_unified_format() {
    let m = Message::tool("id1", "bash", "out");
    assert_eq!(m.role, Role::Tool);
}
