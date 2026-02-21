use std::fs;
use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, warn};

use crate::hooks::{HookEvent, HookInput, HookMessage};
use crate::storage::local::LocalStorage;

/// Handle a hook event from a coding agent.
///
/// Reads JSON input from stdin, wraps it with event type and agent name,
/// and sends to the daemon via Unix socket.
pub fn handle_hook(event: HookEvent, agent: &str) -> Result<()> {
    // Read hook input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .context("Failed to read hook input from stdin")?;

    // Use the agent's parser if available, otherwise fall back to default
    let mut input: HookInput = match crate::agents::get_agent(agent) {
        Some(a) => a.parse_hook_input(&input_str)?,
        None => serde_json::from_str(&input_str).context("Failed to parse hook input JSON")?,
    };

    // Resolve repo from git remote if not already set
    if input.repo.is_none() {
        if let Some(ref cwd) = input.cwd {
            input.repo = crate::git::resolve_repo(cwd);
        }
    }

    // Write/clean PID-to-session mapping for `threader share`
    if matches!(event, HookEvent::SessionStart) {
        if let Some(claude_pid) = crate::process::find_claude_ancestor_pid() {
            let base = LocalStorage::default_base_dir()?;
            let pid_dir = base.join("pid-sessions");
            fs::create_dir_all(&pid_dir)?;
            fs::write(pid_dir.join(claude_pid.to_string()), &input.session_id)?;
        }

        // Generate a share slug and display the session URL
        if let Ok(base) = LocalStorage::default_base_dir() {
            let slug = nanoid::nanoid!(12);
            let slug_dir = base.join("share-slugs");
            if fs::create_dir_all(&slug_dir).is_ok() {
                let _ = fs::write(slug_dir.join(&input.session_id), &slug);
            }
            let url = format!("\u{1f9f5} https://threader.sh/s/{}", slug);
            // Output JSON to stdout so Claude Code displays it via systemMessage
            println!(
                "{}",
                serde_json::json!({
                    "hookSpecificOutput": { "hookEventName": "SessionStart" },
                    "systemMessage": url
                })
            );
        }
    }
    if matches!(event, HookEvent::SessionEnd) {
        if let Some(claude_pid) = crate::process::find_claude_ancestor_pid() {
            let base = LocalStorage::default_base_dir()?;
            let _ = fs::remove_file(base.join("pid-sessions").join(claude_pid.to_string()));
        }
    }

    let msg = HookMessage {
        event,
        input,
        timestamp: Utc::now(),
        agent: Some(agent.to_string()),
    };

    // Try to send to daemon via Unix socket
    let socket_path = socket_path()?;
    match send_to_daemon(&socket_path, &msg) {
        Ok(()) => {
            debug!("Sent hook event to daemon");
        }
        Err(e) => {
            // Daemon not reachable — spool the message for later replay
            warn!("Could not reach daemon ({}), spooling event", e);
            if let Err(spool_err) = spool_message(&msg) {
                warn!("Failed to spool message: {}", spool_err);
            }
        }
    }

    Ok(())
}

/// Write a HookMessage to the spool directory for later replay by the daemon.
fn spool_message(msg: &HookMessage) -> Result<()> {
    let base = LocalStorage::default_base_dir()?;
    let spool_dir = base.join("spool");
    fs::create_dir_all(&spool_dir)?;

    let filename = format!(
        "{}_{}.json",
        msg.timestamp.timestamp_millis(),
        msg.input.session_id
    );
    let path = spool_dir.join(&filename);
    let tmp = path.with_extension("tmp");

    let json = serde_json::to_string_pretty(msg)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;

    debug!("Spooled event to {}", path.display());
    Ok(())
}

fn socket_path() -> Result<PathBuf> {
    let base = LocalStorage::default_base_dir()?;
    Ok(base.join("threader.sock"))
}

fn send_to_daemon(socket_path: &PathBuf, msg: &HookMessage) -> Result<()> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("Failed to connect to {}", socket_path.display()))?;

    serde_json::to_writer(&mut stream, msg).context("Failed to write hook message")?;

    // Shutdown write side so daemon knows we're done
    stream.shutdown(std::net::Shutdown::Write)?;

    Ok(())
}
