use crate::config::{ANON_BASE_URL, ANON_DEFAULT_MODEL};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// List models from an OpenAI-compatible /models endpoint, filtered to free ones.
pub fn list_free_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<ModelInfo>> {
    let mut req = ureq::get(&format!("{}/models", base_url.trim_end_matches('/')));
    if let Some(key) = api_key {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let v: serde_json::Value = serde_json::from_str(req.call()?.into_string()?.as_str())?;
    let mut out = Vec::new();
    if let Some(list) = v.get("data").and_then(|d| d.as_array()) {
        for m in list {
            let free = m
                .get("cost")
                .and_then(|c| c.get("text"))
                .map(|t| {
                    let price = |k: &str| {
                        t.get(k).and_then(|v| v.get("price")).and_then(|p| p.as_f64()).unwrap_or(f64::INFINITY)
                    };
                    price("input") == 0.0 && price("output") == 0.0
                })
                .unwrap_or(false);
            if free {
                out.push(ModelInfo {
                    id: m.get("id").and_then(|v| v.as_str()).unwrap_or_default().into(),
                    name: m.get("name").and_then(|v| v.as_str()).unwrap_or_default().into(),
                    description: m.get("description").and_then(|v| v.as_str()).unwrap_or_default().into(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

impl ModelInfo {
    pub fn list_free(config: &crate::config::Config) -> anyhow::Result<Vec<ModelInfo>> {
        list_free_models(&config.base_url, config.api_key.as_deref())
    }
}

#[allow(dead_code)]
fn ref_defaults() -> (&'static str, &'static str) {
    (ANON_BASE_URL, ANON_DEFAULT_MODEL)
}
