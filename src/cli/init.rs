use std::fs;

use anyhow::{Context, Result};
use tracing::info;

use crate::storage::local::LocalStorage;

/// Initialize Threader: create directories and install Claude Code hooks.
pub fn run_init() -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir);
    storage.init()?;

    install_hooks()?;

    println!("Threader initialized successfully.");
    println!("Run `threader daemon` to start the daemon.");
    Ok(())
}

/// Install hooks into ~/.claude/settings.json.
fn install_hooks() -> Result<()> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let settings_path = home.join(".claude").join("settings.json");

    // Read existing settings or start fresh
    let mut settings: serde_json::Value = if settings_path.exists() {
        let data = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        fs::create_dir_all(settings_path.parent().unwrap())?;
        serde_json::json!({})
    };

    let hooks = settings
        .as_object_mut()
        .context("Settings is not an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let hooks_obj = hooks
        .as_object_mut()
        .context("hooks is not an object")?;

    // Install each hook, preserving existing hooks
    for (event, command) in [
        ("SessionStart", "threader hook session-start"),
        ("Stop", "threader hook stop"),
        ("SessionEnd", "threader hook session-end"),
    ] {
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

        // Check if we already have this hook installed
        let already_installed = event_array.iter().any(|matcher| {
            matcher
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(|c| c.starts_with("threader hook"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });

        if !already_installed {
            event_array.push(serde_json::json!({
                "hooks": [hook_entry]
            }));
            info!("Installed {} hook", event);
        }
    }

    // Write back atomically
    let tmp = settings_path.with_extension("tmp");
    let json = serde_json::to_string_pretty(&settings)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &settings_path)?;

    println!("Claude Code hooks installed.");
    Ok(())
}
