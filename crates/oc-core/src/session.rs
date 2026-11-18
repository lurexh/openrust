use crate::message::Message;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Stable id for a working directory (hash of its path).
fn session_id(cwd: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in cwd.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Shared app data dir: `~/.local/share/openrust` (sessions, memory, ...).
pub fn data_dir() -> Result<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let mut p = home;
    p.push(".local");
    p.push("share");
    p.push("openrust");
    Ok(p)
}

fn sessions_dir() -> Option<PathBuf> {
    let mut p = std::env::home_dir()?;
    p.push(".local/share/openrust/sessions");
    Some(p)
}

fn path_in(base: &Path, cwd: &Path) -> PathBuf {
    base.join(format!("{}.json", session_id(cwd)))
}

pub fn load_history(cwd: &Path) -> Vec<Message> {
    sessions_dir()
        .map(|b| load_history_in(&b, cwd))
        .unwrap_or_default()
}

pub fn save_history(cwd: &Path, history: &[Message]) {
    if let Some(b) = sessions_dir() {
        save_history_in(&b, cwd, history);
    }
}

pub fn clear_history(cwd: &Path) {
    if let Some(b) = sessions_dir() {
        clear_history_in(&b, cwd);
    }
}

pub fn load_history_in(base: &Path, cwd: &Path) -> Vec<Message> {
    std::fs::read_to_string(path_in(base, cwd))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_history_in(base: &Path, cwd: &Path, history: &[Message]) {
    let _ = std::fs::create_dir_all(base);
    if let Ok(json) = serde_json::to_string(history) {
        let _ = std::fs::write(path_in(base, cwd), json);
    }
}

pub fn clear_history_in(base: &Path, cwd: &Path) {
    let _ = std::fs::remove_file(path_in(base, cwd));
}
