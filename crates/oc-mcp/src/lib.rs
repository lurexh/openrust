use anyhow::{Result, anyhow};
use oc_core::{Tool, ToolSpec};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct Inner {
    stdin: Mutex<ChildStdin>,
    responses: Mutex<HashMap<u64, Value>>,
    cv: Condvar,
    next_id: AtomicU64,
}

/// Minimal MCP stdio client (newline-delimited JSON-RPC).
pub struct McpClient {
    inner: Arc<Inner>,
    child: Mutex<Child>,
    pub server_name: String,
}

impl McpClient {
    pub fn connect(command: &[String], name: &str) -> Result<Self> {
        if command.is_empty() {
            return Err(anyhow!("empty command"));
        }
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

        let inner = Arc::new(Inner {
            stdin: Mutex::new(stdin),
            responses: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
            next_id: AtomicU64::new(1),
        });

        // reader thread: route responses into the map, ignore notifications
        let map = Arc::clone(&inner);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|l| l.ok()) {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v.get("method").is_some() {
                    continue; // notification, ignore
                }
                let Some(id) = v.get("id").and_then(|i| i.as_u64()) else { continue };
                map.responses.lock().unwrap().insert(id, v);
                map.cv.notify_all();
            }
        });

        let client = Self { inner, child: Mutex::new(child), server_name: name.to_string() };

        // MCP handshake
        client.request("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "openrust", "version": "0.1.0"}
        }))?;
        client.notify("notifications/initialized")?;

        Ok(client)
    }

    fn write_msg(&self, v: &Value) -> Result<()> {
        let mut stdin = self.inner.stdin.lock().unwrap();
        writeln!(stdin, "{v}")?;
        stdin.flush()?;
        Ok(())
    }

    fn notify(&self, method: &str) -> Result<()> {
        self.write_msg(&json!({"jsonrpc": "2.0", "method": method}))
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        self.write_msg(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        let mut guard = self.inner.responses.lock().unwrap();
        loop {
            if let Some(v) = guard.remove(&id) {
                drop(guard);
                if let Some(err) = v.get("error") {
                    return Err(anyhow!("mcp `{method}`: {err}"));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            let (g, timeout) = self
                .inner
                .cv
                .wait_timeout(guard, Duration::from_secs(60))
                .unwrap();
            guard = g;
            if timeout.timed_out() && !guard.contains_key(&id) {
                return Err(anyhow!("mcp `{method}`: timed out"));
            }
        }
    }

    pub fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        let result = self.request("tools/list", json!({}))?;
        let mut out = Vec::new();
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            for t in tools {
                out.push(McpToolInfo {
                    name: t.get("name").and_then(|n| n.as_str()).unwrap_or_default().into(),
                    description: t.get("description").and_then(|d| d.as_str()).unwrap_or_default().into(),
                    schema: t.get("inputSchema").cloned().unwrap_or(json!({"type": "object"})),
                });
            }
        }
        Ok(out)
    }

    pub fn call_tool(&self, name: &str, args: &str) -> Result<String> {
        let args: Value = serde_json::from_str(args).unwrap_or(json!({}));
        let result = self.request("tools/call", json!({"name": name, "arguments": args}))?;

        let is_error = result.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
        let mut text = String::new();
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            for c in content {
                if c.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }
        }
        if is_error {
            return Err(anyhow!("{text}"));
        }
        Ok(if text.is_empty() { "(empty result)".into() } else { text })
    }

    pub fn shutdown(&self) {
        let _ = self.notify("notifications/cancelled"); // best-effort
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// Bridge: exposes one MCP tool as an agent Tool.
pub struct McpTool {
    client: Arc<McpClient>,
    info: McpToolInfo,
}

impl McpTool {
    pub fn new(client: Arc<McpClient>, info: McpToolInfo) -> Self {
        Self { client, info }
    }
}

impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        let raw = format!("mcp_{}_{}", self.client.server_name, self.info.name);
        let name: String = raw
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .take(64)
            .collect();
        serde_json::from_value(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": self.info.description,
                "parameters": self.info.schema
            }
        }))
        .unwrap_or(ToolSpec {
            kind: "function".into(),
            function: oc_core::message::ToolSpecFunction {
                name,
                description: self.info.description.clone(),
                parameters: json!({"type": "object"}),
            },
        })
    }

    fn execute(&self, args_json: &str) -> Result<String> {
        self.client.call_tool(&self.info.name, args_json)
    }
}
