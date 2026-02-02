use anyhow::{Context, Result};

use super::Agent;
use crate::hooks::HookInput;

pub struct CursorAgent;

impl Agent for CursorAgent {
    fn name(&self) -> &str {
        "cursor"
    }

    fn display_name(&self) -> &str {
        "Cursor"
    }

    fn install(&self, _threader_cmd: &str) -> Result<()> {
        // TODO: Implement Cursor hook installation
        // Cursor hooks go in .cursor/hooks.json
        anyhow::bail!("Cursor hook installation not yet implemented")
    }

    fn uninstall(&self) -> Result<()> {
        // TODO: Implement Cursor hook uninstallation
        Ok(())
    }

    fn is_installed(&self) -> Result<bool> {
        // TODO: Check .cursor/hooks.json for threader hooks
        Ok(false)
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|home| home.join(".cursor").exists())
            .unwrap_or(false)
    }

    fn parse_hook_input(&self, raw: &str) -> Result<HookInput> {
        // TODO: Parse Cursor's native hook format into HookInput
        // For now, try the same JSON format as Claude Code
        serde_json::from_str(raw).context("Failed to parse Cursor hook input JSON")
    }
}
