use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;
use url::Url;

use super::hydrate;
use super::terminal;

/// Handle a `threader://resume/<sessionId>?cwd=<path>` deep link URL.
///
/// Since this runs from a URL handler (no terminal attached), all errors are
/// shown as native macOS dialogs via osascript.
pub async fn handle_url(raw_url: &str) -> Result<()> {
    if let Err(e) = handle_url_inner(raw_url).await {
        show_error_dialog(&format!("Threader Resume Error\n\n{e}"));
        anyhow::bail!("{e}");
    }
    Ok(())
}

async fn handle_url_inner(raw_url: &str) -> Result<()> {
    let url = Url::parse(raw_url).context("Invalid URL")?;

    // Extract session ID from path: threader://resume/<sessionId>
    let path_segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.collect())
        .unwrap_or_default();

    let session_id = path_segments
        .first()
        .filter(|s| !s.is_empty())
        .context("Missing session ID in URL")?;

    // Extract cwd from query params
    let cwd = url
        .query_pairs()
        .find(|(k, _)| k == "cwd")
        .map(|(_, v)| v.into_owned())
        .context("Missing 'cwd' query parameter in URL")?;

    // Validate cwd exists on disk
    if !Path::new(&cwd).is_dir() {
        anyhow::bail!(
            "The working directory does not exist on this machine:\n{cwd}\n\n\
             This session was started in a directory that doesn't exist locally."
        );
    }

    // Check claude binary is available
    let claude_exists = Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !claude_exists {
        anyhow::bail!(
            "Claude Code CLI not found.\n\n\
             Install it from https://docs.anthropic.com/en/docs/claude-code"
        );
    }

    // Hydrate session transcript (downloads from cloud if not present locally)
    info!("Hydrating session {session_id} for cwd {cwd}");
    hydrate::hydrate_session(session_id, &cwd).await?;

    // Detect terminal and launch
    let term = terminal::detect_terminal();
    let command = format!("claude --resume {session_id}");
    info!("Opening {:?} with: {command}", term);
    terminal::open_in_terminal(term, &cwd, &command)?;

    Ok(())
}

/// Show a native macOS error dialog (since there's no terminal for stderr).
fn show_error_dialog(message: &str) {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"display dialog "{escaped}" with title "Threader" buttons {{"OK"}} default button "OK" with icon stop"#
    );
    let _ = Command::new("osascript").args(["-e", &script]).output();
}
