mod task;

pub use task::TaskTool;

use anyhow::{Result, anyhow};
use oc_core::{Tool, ToolSpec};
use serde_json::Value;
use std::path::Path;

fn args(json: &str) -> Result<Value> {
    Ok(serde_json::from_str(json)?)
}

fn str_arg(v: &Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("missing string arg `{key}`"))
}

/// bash: run a shell command, returns stdout+stderr.
pub struct Bash {
    pub cwd: std::path::PathBuf,
    pub timeout_secs: u64,
}

impl Bash {
    pub fn new(cwd: impl Into<std::path::PathBuf>) -> Self {
        Self { cwd: cwd.into(), timeout_secs: 120 }
    }
}

impl Tool for Bash {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "bash",
                "description": "Run a shell command in the working directory and return combined stdout/stderr. Use for ls, git, cargo, grep, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The shell command to run" }
                    },
                    "required": ["command"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let cmd = str_arg(&args(a)?, "command")?;
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&self.cwd)
            .output()?;
        Ok(format!(
            "exit: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// read: read a text file (with line numbers).
pub struct Read;

impl Tool for Read {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read",
                "description": "Read a text file from the working directory. Returns numbered lines.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to working directory" }
                    },
                    "required": ["path"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let path = str_arg(&args(a)?, "path")?;
        let content = std::fs::read_to_string(Path::new(&path))
            .map_err(|e| anyhow!("read `{path}`: {e}"))?;
        Ok(content
            .lines()
            .enumerate()
            .map(|(i, l)| format!("{}: {l}", i + 1))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// write: create or overwrite a file.
pub struct Write;

impl Tool for Write {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "write",
                "description": "Create or overwrite a file with the given content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let v = args(a)?;
        let path = str_arg(&v, "path")?;
        let content = str_arg(&v, "content")?;
        std::fs::write(&path, content).map_err(|e| anyhow!("write `{path}`: {e}"))?;
        Ok(format!("wrote {path}"))
    }
}

/// edit: exact string replacement in a file.
pub struct Edit;

impl Tool for Edit {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit",
                "description": "Replace an exact string in a file. old must match exactly once.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old": { "type": "string" },
                        "new": { "type": "string" }
                    },
                    "required": ["path", "old", "new"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let v = args(a)?;
        let path = str_arg(&v, "path")?;
        let old = str_arg(&v, "old")?;
        let new = str_arg(&v, "new")?;
        let content = std::fs::read_to_string(&path).map_err(|e| anyhow!("read `{path}`: {e}"))?;
        let count = content.matches(&old).count();
        if count == 0 {
            return Err(anyhow!("old string not found in {path}"));
        }
        if count > 1 {
            return Err(anyhow!("old string matches {count} times in {path}; provide more context"));
        }
        std::fs::write(&path, content.replace(&old, &new)).map_err(|e| anyhow!("write `{path}`: {e}"))?;
        Ok(format!("edited {path}"))
    }
}

/// glob: find files by pattern (e.g. `**/*.rs`).
pub struct Glob;

impl Tool for Glob {
    fn spec(&self) -> ToolSpec {
        serde_json::from_value(serde_json::json!({
            "type": "function",
            "function": {
                "name": "glob",
                "description": "List files matching a pattern like '**/*.rs' or 'crates/*' (up to 200 results).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    },
                    "required": ["pattern"]
                }
            }
        }))
        .unwrap()
    }

    fn execute(&self, a: &str) -> Result<String> {
        let pattern = str_arg(&args(a)?, "pattern")?;
        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(".")
            .max_depth(12)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().to_string_lossy();
            let rel = rel.trim_start_matches("./");
            if glob_match(pattern.as_bytes(), rel.as_bytes()) {
                out.push(rel.to_string());
                if out.len() >= 200 {
                    break;
                }
            }
        }
        if out.is_empty() {
            return Ok("no matches".into());
        }
        Ok(out.join("\n"))
    }
}

/// fnmatch-style glob with `**`, `*`, `?` support.
fn glob_match(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p.starts_with(b"**") {
        let rest = if p.get(2) == Some(&b'/') { &p[3..] } else { &p[2..] };
        if glob_match(rest, s) {
            return true;
        }
        for i in 1..=s.len() {
            if glob_match(rest, &s[i..]) {
                return true;
            }
        }
        return false;
    }
    if p[0] == b'*' {
        for i in 0..=s.len() {
            if glob_match(&p[1..], &s[i..]) {
                return true;
            }
        }
        return false;
    }
    if p[0] == b'?' {
        return !s.is_empty() && glob_match(&p[1..], &s[1..]);
    }
    !s.is_empty() && p[0] == s[0] && glob_match(&p[1..], &s[1..])
}
