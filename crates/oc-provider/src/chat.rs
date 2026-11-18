use oc_core::{AssistantTurn, Chat, Message, ToolSpec};
use anyhow::Result;
use serde::Deserialize;
use ureq::json;

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallRaw>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolCallRaw {
    id: String,
    function: ToolCallFunctionRaw,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolCallFunctionRaw {
    name: String,
    arguments: Option<String>,
}

#[derive(Clone)]
pub struct OpenAiCompat {
    base_url: String,
    api_key: Option<String>,
    pub model: String,
}

impl OpenAiCompat {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>, model: impl Into<String>) -> Self {
        Self { base_url: base_url.into().trim_end_matches('/').to_string(), api_key, model: model.into() }
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }
}

impl OpenAiCompat {
    fn turn_from_response(resp: ChatResponse) -> Result<AssistantTurn> {
        let msg = resp
            .choices
            .first()
            .map(|c| &c.message)
            .ok_or_else(|| anyhow::anyhow!("empty choices"))?;

        Ok(AssistantTurn {
            content: msg.content.clone().unwrap_or_default(),
            tool_calls: msg
                .tool_calls
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|t| oc_core::ToolCall {
                    id: t.id,
                    kind: "function".into(),
                    function: oc_core::message::ToolCallFunction {
                        name: t.function.name,
                        arguments: t.function.arguments.unwrap_or_else(|| "{}".into()),
                    },
                })
                .collect(),
        })
    }
}

impl Chat for OpenAiCompat {
    fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        let mut req = ureq::post(&format!("{}/chat/completions", self.base_url))
            .set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }

        let resp: ChatResponse = req.send_json(body)?.into_json()?;
        Self::turn_from_response(resp)
    }

    fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<AssistantTurn> {
        use std::io::BufRead;

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        let mut req = ureq::post(&format!("{}/chat/completions", self.base_url))
            .set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }

        let resp = req.send_json(body)?;
        let ct = resp.content_type().to_string();

        if ct.contains("json") {
            // server ignored `stream` and answered with a plain JSON body
            let parsed: ChatResponse = resp.into_json()?;
            return Self::turn_from_response(parsed);
        }
        if !ct.contains("text/event-stream") {
            let mut text = resp.into_string()?;
            text.truncate(200);
            anyhow::bail!(
                "provider returned `{ct}` instead of an SSE stream: {text:?} — endpoint may be down or moved"
            );
        }

        let reader = std::io::BufReader::new(resp.into_reader());

        let mut content = String::new();
        let mut calls: Vec<Option<(String, String, String)>> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let data = line.strip_prefix("data: ").unwrap_or("");
            if data == "[DONE]" {
                break;
            }
            if data.is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let delta = &v["choices"][0]["delta"];
            if let Some(c) = delta["content"].as_str() {
                content.push_str(c);
                on_delta(c);
            }
            if let Some(tcs) = delta["tool_calls"].as_array() {
                for tc in tcs {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    while calls.len() <= idx {
                        calls.push(None);
                    }
                    let entry = calls[idx].get_or_insert_with(|| (String::new(), String::new(), String::new()));
                    if let Some(id) = tc["id"].as_str() {
                        entry.0 = id.into();
                    }
                    if let Some(n) = tc["function"]["name"].as_str() {
                        entry.1.push_str(n);
                    }
                    if let Some(a) = tc["function"]["arguments"].as_str() {
                        entry.2.push_str(a);
                    }
                }
            }
        }

        if content.is_empty() && calls.iter().all(|c| c.is_none()) {
            anyhow::bail!("empty streaming response: no SSE data received from provider");
        }
        Ok(AssistantTurn {
            content,
            tool_calls: calls
                .into_iter()
                .flatten()
                .map(|(id, name, args)| oc_core::ToolCall {
                    id,
                    kind: "function".into(),
                    function: oc_core::message::ToolCallFunction { name, arguments: args },
                })
                .collect(),
        })
    }
}
