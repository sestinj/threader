use anyhow::Result;
use tracing::warn;

use crate::agents;
use crate::storage::local::LocalStorage;

/// Initialize Threader: create directories and install hooks for all detected agents.
pub fn run_init() -> Result<()> {
    init_core()?;

    println!("\nRun `threader start` to start the daemon.");
    Ok(())
}

/// Core initialization logic. Idempotent — safe to call on every update or start.
pub fn init_core() -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir);
    storage.init()?;

    install_hooks()?;

    Ok(())
}

/// Detect all installed coding agents and install hooks for each.
pub fn install_hooks() -> Result<()> {
    let threader_cmd = agents::resolve_threader_cmd()?;
    let all = agents::all_agents();

    println!("Detecting coding agents...");

    let mut connected = 0;

    for agent in &all {
        if agent.detect() {
            match agent.install(&threader_cmd) {
                Ok(()) => {
                    println!(
                        "  \u{2713} {} \u{2014} hooks installed",
                        agent.display_name()
                    );
                    connected += 1;
                }
                Err(e) => {
                    warn!("Failed to install hooks for {}: {}", agent.name(), e);
                    println!(
                        "  \u{2717} {} \u{2014} hook installation failed: {}",
                        agent.display_name(),
                        e
                    );
                }
            }
        } else {
            println!("  \u{00b7} {} \u{2014} not detected", agent.display_name());
        }
    }

    if connected > 0 {
        println!(
            "\nThreader is ready. {} agent{} connected.",
            connected,
            if connected == 1 { "" } else { "s" }
        );
    } else {
        println!(
            "\nNo coding agents detected. Install a supported agent and run `threader init` again."
        );
    }

    Ok(())
}
