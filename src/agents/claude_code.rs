use std::fs;

use anyhow::{Context, Result};
use tracing::info;

use super::Agent;
use crate::hooks::HookInput;

pub struct ClaudeCodeAgent;

impl Agent for ClaudeCodeAgent {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn install(&self, threader_cmd: &str) -> Result<()> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let settings_path = home.join(".claude").join("settings.json");

        // Read existing settings or start fresh
        let mut settings: serde_json::Value = if settings_path.exists() {
            let data = fs::read_to_string(&settings_path)?;
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
        } else {
            if let Some(parent) = settings_path.parent() {
                fs::create_dir_all(parent)?;
            }
            serde_json::json!({})
        };

        let hooks = settings
            .as_object_mut()
            .context("Settings is not an object")?
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}));

        let hooks_obj = hooks.as_object_mut().context("hooks is not an object")?;

        // Install each hook, preserving existing hooks
        for (event, subcommand) in [
            ("SessionStart", "hook claude-code session-start"),
            ("Stop", "hook claude-code stop"),
            ("SessionEnd", "hook claude-code session-end"),
        ] {
            let command = format!("{} {}", threader_cmd, subcommand);
            let hook_entry = serde_json::json!({
                "type": "command",
                "command": command
            });

            let event_hooks = hooks_obj
                .entry(event)
                .or_insert_with(|| serde_json::json!([]));

            let event_array = event_hooks
                .as_array_mut()
                .context(format!("{} hooks is not an array", event))?;

            // Remove any existing threader hook entries (handles duplicates and stale paths)
            event_array.retain(|matcher| {
                let is_threader = matcher
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains("threader hook"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                !is_threader
            });

            // Add fresh hook entry with the current binary path
            event_array.push(serde_json::json!({
                "hooks": [hook_entry]
            }));
            info!("Installed Claude Code {} hook", event);
        }

        // Write back atomically
        let tmp = settings_path.with_extension("tmp");
        let json = serde_json::to_string_pretty(&settings)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &settings_path)?;

        Ok(())
    }

    fn uninstall(&self) -> Result<()> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let settings_path = home.join(".claude").join("settings.json");

        if !settings_path.exists() {
            return Ok(());
        }

        let data = fs::read_to_string(&settings_path)?;
        let mut settings: serde_json::Value =
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}));

        if let Some(hooks_obj) = settings
            .as_object_mut()
            .and_then(|s| s.get_mut("hooks"))
            .and_then(|h| h.as_object_mut())
        {
            for event in ["SessionStart", "Stop", "SessionEnd"] {
                if let Some(event_hooks) = hooks_obj.get_mut(event).and_then(|h| h.as_array_mut()) {
                    event_hooks.retain(|matcher| {
                        let is_threader = matcher
                            .get("hooks")
                            .and_then(|h| h.as_array())
                            .map(|hooks| {
                                hooks.iter().any(|h| {
                                    h.get("command")
                                        .and_then(|c| c.as_str())
                                        .map(|c| c.contains("threader hook"))
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false);
                        !is_threader
                    });
                }
            }
        }

        let tmp = settings_path.with_extension("tmp");
        let json = serde_json::to_string_pretty(&settings)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &settings_path)?;

        Ok(())
    }

    fn is_installed(&self) -> Result<bool> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let settings_path = home.join(".claude").join("settings.json");

        if !settings_path.exists() {
            return Ok(false);
        }

        let data = fs::read_to_string(&settings_path)?;
        let settings: serde_json::Value =
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}));

        let has_hook = settings
            .get("hooks")
            .and_then(|h| h.get("SessionStart"))
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter().any(|matcher| {
                    matcher
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|hooks| {
                            hooks.iter().any(|h| {
                                h.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c.contains("threader hook"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        Ok(has_hook)
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|home| home.join(".claude").exists())
            .unwrap_or(false)
    }

    fn parse_hook_input(&self, raw: &str) -> Result<HookInput> {
        serde_json::from_str(raw).context("Failed to parse Claude Code hook input JSON")
    }
}
