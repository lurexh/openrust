use anyhow::Result;
use oc_core::message::ToolCallFunction;
use oc_core::{AssistantTurn, Chat, Message, Role, ToolCall, ToolSpec};
use serde_json::{json, Value};
use std::io::BufRead;

/// Anthropic Messages API provider (Claude models).
#[derive(Clone)]
pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    pub model: String,
    pub max_tokens: u32,
}

impl AnthropicProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 8192,
        }
    }
}

/// Map our unified messages into Anthropic `messages` + `system` params.
pub fn to_anthropic(messages: &[Message]) -> (String, Vec<Value>) {
    let mut system = String::new();
    let mut out: Vec<Value> = Vec::new();

    for m in messages {
        match m.role {
            Role::System => {
                if let Some(c) = &m.content {
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(c);
                }
            }
            Role::User => {
                let text = m.content.clone().unwrap_or_default();
                out.push(json!({"role": "user", "content": [{"type": "text", "text": text}]}));
            }
            Role::Assistant => {
                let mut blocks = Vec::new();
                if let Some(t) = &m.content {
                    if !t.is_empty() {
                        blocks.push(json!({"type": "text", "text": t}));
                    }
                }
                if let Some(calls) = &m.tool_calls {
                    for tc in calls {
                        let input: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                        blocks.push(json!({
                            "type": "tool_use", "id": tc.id,
                            "name": tc.function.name, "input": input
                        }));
                    }
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": "(no content)"}));
                }
                out.push(json!({"role": "assistant", "content": blocks}));
            }
            Role::Tool => {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content.clone().unwrap_or_default()
                });
                // merge consecutive tool results into one user message
                match out.last_mut() {
                    Some(last) if last["role"] == "user" && last["content"].as_array().map(|a| a.iter().any(|b| b["type"] == "tool_result")).unwrap_or(false) => {
                        last["content"].as_array_mut().unwrap().push(block);
                    }
                    _ => {
                        out.push(json!({"role": "user", "content": [block]}));
                    }
                }
            }
        }
    }

    (system, out)
}

/// Parse an Anthropic Messages API response into an AssistantTurn.
pub fn parse_response(v: &Value) -> AssistantTurn {
    let mut content = String::new();
    let mut calls = Vec::new();
    if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        content.push_str(t);
                    }
                }
                Some("tool_use") => {
                    calls.push(ToolCall {
                        id: b.get("id").and_then(|i| i.as_str()).unwrap_or_default().into(),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: b.get("name").and_then(|n| n.as_str()).unwrap_or_default().into(),
                            arguments: b
                                .get("input")
                                .cloned()
                                .unwrap_or(json!({}))
                                .to_string(),
                        },
                    });
                }
                _ => {}
            }
        }
    }
    AssistantTurn { content, tool_calls: calls }
}

fn tools_to_anthropic(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.function.name,
                "description": t.function.description,
                "input_schema": t.function.parameters
            })
        })
        .collect()
}

impl Chat for AnthropicProvider {
    fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn> {
        let (system, msgs) = to_anthropic(messages);
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": msgs,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools_to_anthropic(tools));
        }

        let resp = ureq::post(&format!("{}/v1/messages", self.base_url))
            .set("anthropic-version", "2023-06-01")
            .set("x-api-key", &self.api_key)
            .send_json(body)?;
        let v: Value = resp.into_json()?;
        Ok(parse_response(&v))
    }

    fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<AssistantTurn> {
        let (system, msgs) = to_anthropic(messages);
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": msgs,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools_to_anthropic(tools));
        }

        let resp = ureq::post(&format!("{}/v1/messages", self.base_url))
            .set("anthropic-version", "2023-06-01")
            .set("x-api-key", &self.api_key)
            .send_json(body)?;
        let ct = resp.content_type().to_string();
        if !ct.contains("text/event-stream") {
            let mut text = resp.into_string()?;
            text.truncate(200);
            anyhow::bail!("anthropic returned `{ct}` instead of an SSE stream: {text:?}");
        }
        let reader = std::io::BufReader::new(resp.into_reader());

        let mut content = String::new();
        // current tool call being assembled (index, id, name, args)
        let mut calls: Vec<Option<(String, String, String)>> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let data = line.strip_prefix("data: ").unwrap_or("");
            if data.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            match v.get("type").and_then(|t| t.as_str()) {
                Some("content_block_start") => {
                    let cb = &v["content_block"];
                    if cb["type"] == "tool_use" {
                        let idx = v["index"].as_u64().unwrap_or(0) as usize;
                        while calls.len() <= idx {
                            calls.push(None);
                        }
                        calls[idx] = Some((
                            cb["id"].as_str().unwrap_or_default().into(),
                            cb["name"].as_str().unwrap_or_default().into(),
                            String::new(),
                        ));
                    }
                }
                Some("content_block_delta") => {
                    let d = &v["delta"];
                    let idx = v["index"].as_u64().unwrap_or(0) as usize;
                    match d["type"].as_str() {
                        Some("text_delta") => {
                            if let Some(t) = d["text"].as_str() {
                                content.push_str(t);
                                on_delta(t);
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(a) = d["partial_json"].as_str() {
                                while calls.len() <= idx {
                                    calls.push(None);
                                }
                                if let Some(entry) = calls.get_mut(idx) {
                                    if let Some(e) = entry {
                                        e.2.push_str(a);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_stop") => break,
                _ => {}
            }
        }

        Ok(AssistantTurn {
            content,
            tool_calls: calls
                .into_iter()
                .flatten()
                .map(|(id, name, args)| ToolCall {
                    id,
                    kind: "function".into(),
                    function: ToolCallFunction { name, arguments: if args.is_empty() { "{}".into() } else { args } },
                })
                .collect(),
        })
    }
}
