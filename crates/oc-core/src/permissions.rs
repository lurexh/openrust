/// Tool permission gating.
pub enum Decision {
    Allow,
    Deny,
}

pub trait Permission: Send + Sync {
    fn check(&self, tool: &str, args: &str) -> Decision;
}

/// Handler backed by a user-provided closure (interactive UIs, config, tests).
pub struct InteractivePermission {
    pub ask: Box<dyn Fn(&str, &str) -> Decision + Send + Sync>,
}

impl Permission for InteractivePermission {
    fn check(&self, tool: &str, args: &str) -> Decision {
        (self.ask)(tool, args)
    }
}

/// Tools that never need approval (read-only).
pub fn is_safe(tool: &str) -> bool {
    matches!(tool, "read" | "glob")
}

/// Allow everything (default when no permission handler is set).
pub struct AllowAll;

impl Permission for AllowAll {
    fn check(&self, _: &str, _: &str) -> Decision {
        Decision::Allow
    }
}
