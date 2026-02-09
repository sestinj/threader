use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, info};

/// Ensure a Claude Code session transcript exists locally so `claude --resume` can find it.
///
/// If the JSONL already exists (same-machine case), this is a no-op.
/// Otherwise, downloads the transcript from the Threader cloud and writes it
/// to the appropriate Claude Code project directory.
pub async fn hydrate_session(session_id: &str, cwd: &str) -> Result<()> {
    let project_dir = cc_project_dir(cwd)?;
    let jsonl_path = project_dir.join(format!("{session_id}.jsonl"));

    if jsonl_path.exists() {
        debug!(
            "Session transcript already exists locally: {}",
            jsonl_path.display()
        );
        return Ok(());
    }

    info!("Downloading transcript for session {session_id} from cloud...");

    let token = crate::auth::get_token()
        .await
        .context("Authentication required to download session transcript")?;

    let site_url = std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string());

    let url = format!("{site_url}/api/sessions/{session_id}/transcript");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to request transcript from cloud")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to download transcript ({status}): {text}");
    }

    let transcript = resp
        .text()
        .await
        .context("Failed to read transcript body")?;

    // Ensure project directory exists
    fs::create_dir_all(&project_dir)
        .with_context(|| format!("Failed to create project dir: {}", project_dir.display()))?;

    // Write JSONL atomically
    let tmp_path = jsonl_path.with_extension("jsonl.tmp");
    fs::write(&tmp_path, &transcript)
        .with_context(|| format!("Failed to write transcript to {}", tmp_path.display()))?;
    fs::rename(&tmp_path, &jsonl_path)
        .context("Failed to atomically move transcript into place")?;

    info!("Transcript written to {}", jsonl_path.display());

    // Update sessions-index.json
    update_sessions_index(&project_dir, session_id, cwd)?;

    Ok(())
}

/// Compute the Claude Code project directory for a given cwd.
/// CC uses `~/.claude/projects/{cwd-with-slashes-replaced-by-dashes}/`.
fn cc_project_dir(cwd: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let dir_name = cwd.replace('/', "-");
    Ok(home.join(".claude").join("projects").join(dir_name))
}

/// Append an entry to sessions-index.json so CC knows about the hydrated session.
fn update_sessions_index(project_dir: &Path, session_id: &str, cwd: &str) -> Result<()> {
    let index_path = project_dir.join("sessions-index.json");

    let mut index: serde_json::Value = if index_path.exists() {
        let data = fs::read_to_string(&index_path)?;
        serde_json::from_str(&data).unwrap_or_else(|_| default_index())
    } else {
        default_index()
    };

    let entries = index
        .as_object_mut()
        .and_then(|o| o.get_mut("entries"))
        .and_then(|e| e.as_array_mut())
        .context("sessions-index.json has unexpected format")?;

    // Don't duplicate if entry already exists
    let already_exists = entries.iter().any(|e| {
        e.get("sessionId")
            .and_then(|v| v.as_str())
            .map(|id| id == session_id)
            .unwrap_or(false)
    });

    if !already_exists {
        let jsonl_path = project_dir.join(format!("{session_id}.jsonl"));
        entries.push(serde_json::json!({
            "sessionId": session_id,
            "fullPath": jsonl_path.to_string_lossy(),
            "fileMtime": Utc::now().to_rfc3339(),
            "firstPrompt": "",
            "summary": "",
            "originalPath": cwd,
        }));

        let tmp = index_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&index)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &index_path)?;

        debug!("Updated sessions-index.json with session {session_id}");
    }

    Ok(())
}

fn default_index() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "entries": []
    })
}
