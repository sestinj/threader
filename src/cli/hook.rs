use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, warn};

use crate::hooks::{HookEvent, HookInput, HookMessage};
use crate::storage::local::LocalStorage;

/// Handle a hook event from Claude Code.
///
/// Reads JSON input from stdin, wraps it with event type,
/// and sends to the daemon via Unix socket.
pub fn handle_hook(event: HookEvent) -> Result<()> {
    // Read hook input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .context("Failed to read hook input from stdin")?;

    let input: HookInput =
        serde_json::from_str(&input_str).context("Failed to parse hook input JSON")?;

    let msg = HookMessage {
        event,
        input,
        timestamp: Utc::now(),
    };

    // Try to send to daemon via Unix socket
    let socket_path = socket_path()?;
    match send_to_daemon(&socket_path, &msg) {
        Ok(()) => {
            debug!("Sent hook event to daemon");
        }
        Err(e) => {
            // Daemon might not be running - that's okay.
            // The data isn't lost since the transcript file persists.
            warn!("Could not reach daemon ({}), event not sent", e);
        }
    }

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
