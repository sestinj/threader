pub mod claude_code;
pub mod cursor;

use anyhow::Result;

use crate::hooks::HookInput;

/// Trait implemented by each supported coding agent.
pub trait Agent: Send + Sync {
    /// Machine-readable identifier, e.g. "claude-code", "cursor".
    fn name(&self) -> &str;

    /// Human-readable display name, e.g. "Claude Code", "Cursor".
    fn display_name(&self) -> &str;

    /// Install hooks into this agent's configuration so it calls
    /// `threader hook <agent-name> <event>` on session events.
    fn install(&self, threader_cmd: &str) -> Result<()>;

    /// Remove threader hooks from this agent's configuration.
    #[allow(dead_code)]
    fn uninstall(&self) -> Result<()>;

    /// Check whether threader hooks are currently installed for this agent.
    #[allow(dead_code)]
    fn is_installed(&self) -> Result<bool>;

    /// Detect whether this agent is installed on the system.
    fn detect(&self) -> bool;

    /// Parse this agent's hook stdin JSON into the common HookInput.
    fn parse_hook_input(&self, raw: &str) -> Result<HookInput>;
}

/// All known agents, in display order.
pub fn all_agents() -> Vec<Box<dyn Agent>> {
    vec![
        Box::new(claude_code::ClaudeCodeAgent),
        Box::new(cursor::CursorAgent),
    ]
}

/// Look up an agent by name.
pub fn get_agent(name: &str) -> Option<Box<dyn Agent>> {
    all_agents().into_iter().find(|a| a.name() == name)
}

/// Resolve the absolute path to the threader binary, preferring ~/.local/bin/threader.
pub fn resolve_threader_cmd() -> Result<String> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let threader_bin = home.join(".local").join("bin").join("threader");
    if threader_bin.exists() {
        Ok(threader_bin
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 path to threader binary"))?
            .to_string())
    } else {
        Ok("threader".to_string())
    }
}
