use oc_core::Tool;
use oc_mcp::McpClient;
use std::io::Write;

const FAKE_SERVER: &str = r#"
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    m = json.loads(line)
    method = m.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {"tools": {}}}}), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": {"tools": [
            {"name": "echo", "description": "echoes input.x", "inputSchema": {"type": "object"}}
        ]}}), flush=True)
    elif method == "tools/call":
        text = "echo:" + m["params"]["arguments"]["x"]
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": {"content": [{"type": "text", "text": text}]}}), flush=True)
"#;

#[test]
fn mcp_stdio_roundtrip() {
    let dir = std::env::temp_dir().join(format!("oc_mcp_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake_mcp.py");
    std::fs::write(&script, FAKE_SERVER).unwrap();

    let client = McpClient::connect(
        &["python3".to_string(), script.to_string_lossy().into_owned()],
        "fake",
    )
    .expect("connect");

    let tools = client.list_tools().expect("list_tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert!(tools[0].description.contains("echoes"));

    let out = client.call_tool("echo", r#"{"x":"hi"}"#).expect("call_tool");
    assert_eq!(out, "echo:hi");

    client.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_tool_spec_sanitizes_name() {
    // spec names must fit OpenAI's tool-name charset
    let dir = std::env::temp_dir().join(format!("oc_mcp_spec_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake_mcp.py");
    std::fs::write(&script, FAKE_SERVER).unwrap();
    let client = McpClient::connect(
        &["python3".to_string(), script.to_string_lossy().into_owned()],
        "weird.server-1",
    )
    .unwrap();
    let tools = client.list_tools().unwrap();
    let t = oc_mcp::McpTool::new(std::sync::Arc::new(client), tools.into_iter().next().unwrap());
    let name = t.spec().function.name;
    assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{name}");
    assert!(name.len() <= 64);
    let _ = Write::flush(&mut std::io::sink());
    let _ = std::fs::remove_dir_all(&dir);
}
