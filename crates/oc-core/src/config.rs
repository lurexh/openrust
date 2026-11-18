use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

pub const ANON_BASE_URL: &str = "https://opencode.ai/inference/openai/v1";
pub const ANON_DEFAULT_MODEL: &str = "glm-5.3-flash-free";

#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub mcp_servers: Vec<McpServer>,
    /// "ask" (default) or "allow"
    pub permission_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServer {
    pub name: Option<String>,
    pub command: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    mcp_servers: Option<Vec<McpServer>>,
    permissions: Option<PermissionsCfg>,
}

#[derive(Debug, Default, Deserialize)]
struct PermissionsCfg {
    mode: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let file: FileConfig = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let api_key = std::env::var("OPENCODE_API_KEY").ok().or(file.api_key);

        // provider-specific keyed defaults (GitHub Models inference is retired)
        let keyed_default_base = if std::env::var("OPENROUTER_API_KEY").is_ok() {
            "https://openrouter.ai/api/v1"
        } else if std::env::var("OPENAI_API_KEY").is_ok() {
            "https://api.openai.com/v1"
        } else {
            "https://models.github.ai/inference"
        };

        let base_url = std::env::var("OPENCODE_BASE_URL")
            .ok()
            .or(file.base_url)
            .unwrap_or(if api_key.is_some() {
                keyed_default_base.into()
            } else {
                ANON_BASE_URL.into()
            });

        let model = std::env::var("OPENCODE_MODEL")
            .ok()
            .or(file.model)
            .unwrap_or(if api_key.is_some() {
                "openai/gpt-4o-mini".into()
            } else {
                ANON_DEFAULT_MODEL.into()
            });

        let permission_mode = file
            .permissions
            .and_then(|p| p.mode)
            .unwrap_or_else(|| "ask".into());

        Ok(Self {
            base_url,
            api_key,
            model,
            mcp_servers: file.mcp_servers.unwrap_or_default(),
            permission_mode,
        })
    }
    pub fn anonymous(&self) -> bool {
        self.api_key.is_none() && self.base_url.starts_with(ANON_BASE_URL)
    }

    fn path() -> Option<PathBuf> {
        let mut p = std::env::home_dir()?;
        p.push(".config/openrust/config.json");
        p.exists().then_some(p)
    }
}
