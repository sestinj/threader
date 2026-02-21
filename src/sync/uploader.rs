use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::auth;
use crate::hooks::{QueueAction, QueueEntry};
use crate::storage::local::LocalStorage;
use crate::storage::queue::UploadQueue;
use crate::sync::images::{line_has_images, ImageProcessor};

const UPLOAD_INTERVAL_SECS: u64 = 10;
const MAX_RETRY_ATTEMPTS: u32 = 360;

/// Convex HTTP endpoint base URL.
/// Can be overridden via the THREADER_CONVEX_SITE_URL env var.
fn convex_site_url() -> String {
    std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string())
}

/// Cache for session summaries to avoid re-reading sessions-index.json on every upload.
/// Key: session_id, Value: cached summary (or None if not found/empty).
static SESSION_SUMMARY_CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// Maximum number of entries in the session summary cache.
const MAX_SUMMARY_CACHE_SIZE: usize = 500;

/// Read the session summary from Claude Code's sessions-index.json.
///
/// The file lives in the same directory as the transcript JSONL and contains
/// an `entries` array with `sessionId` and `summary` fields.
///
/// Results are cached per-session to avoid repeated disk I/O during frequent
/// append uploads. The cache is never cleared since summaries don't change.
fn read_session_summary(transcript_path: &Path, session_id: &str) -> Option<String> {
    // Check cache first
    {
        let guard = SESSION_SUMMARY_CACHE.lock().ok()?;
        if let Some(cache) = guard.as_ref() {
            if let Some(cached) = cache.get(session_id) {
                return cached.clone();
            }
        }
    }

    // Read from disk
    let summary = read_session_summary_uncached(transcript_path, session_id);

    // Cache the result (even if None, to avoid repeated lookups)
    if let Ok(mut guard) = SESSION_SUMMARY_CACHE.lock() {
        let cache = guard.get_or_insert_with(HashMap::new);
        // Clear cache if it gets too large (simple eviction strategy)
        if cache.len() >= MAX_SUMMARY_CACHE_SIZE {
            cache.clear();
        }
        cache.insert(session_id.to_string(), summary.clone());
    }

    summary
}

/// Read session summary without caching.
fn read_session_summary_uncached(transcript_path: &Path, session_id: &str) -> Option<String> {
    let project_dir = transcript_path.parent()?;
    let index_path = project_dir.join("sessions-index.json");
    let content = fs::read_to_string(&index_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let entries = parsed.get("entries")?.as_array()?;
    for entry in entries {
        if entry.get("sessionId")?.as_str()? == session_id {
            let summary = entry.get("summary")?.as_str()?;
            if !summary.is_empty() {
                return Some(summary.to_string());
            }
        }
    }
    None
}

/// Background worker that processes the upload queue.
pub struct BackgroundUploader {
    queue: UploadQueue,
    storage: LocalStorage,
    client: reqwest::Client,
}

impl BackgroundUploader {
    pub fn new(queue: UploadQueue, base_dir: PathBuf) -> Self {
        Self {
            queue,
            storage: LocalStorage::new(base_dir),
            client: reqwest::Client::new(),
        }
    }

    /// Run the upload loop. Processes pending entries periodically.
    pub async fn run(&self) -> Result<()> {
        info!("Background uploader started");

        loop {
            if let Err(e) = self.process_queue().await {
                error!("Error processing upload queue: {}", e);
            }
            time::sleep(Duration::from_secs(UPLOAD_INTERVAL_SECS)).await;
        }
    }

    async fn process_queue(&self) -> Result<()> {
        let entries = self.queue.list_pending()?;
        if entries.is_empty() {
            return Ok(());
        }

        debug!("Processing {} pending uploads", entries.len());

        // Get auth token once per batch
        let token = match auth::get_token().await {
            Ok(t) => {
                debug!("Got auth token (len={})", t.len());
                t
            }
            Err(e) => {
                warn!("Cannot upload: auth error: {}", e);
                return Ok(());
            }
        };

        for (path, mut entry) in entries {
            if entry.attempts >= MAX_RETRY_ATTEMPTS {
                warn!(
                    "Moving queue entry for session {} to dead-letter after {} attempts",
                    entry.session_id, entry.attempts
                );
                self.move_to_failed(&path)?;
                continue;
            }

            match self.upload(&entry, &token).await {
                Ok(()) => {
                    debug!(
                        "Uploaded {:?} for session {}",
                        entry.action, entry.session_id
                    );
                    self.queue.remove(&path)?;
                }
                Err(e) => {
                    warn!(
                        "Upload failed for session {} (attempt {}): {}",
                        entry.session_id,
                        entry.attempts + 1,
                        e
                    );
                    entry.attempts += 1;
                    self.queue.update(&path, &entry)?;
                }
            }
        }

        Ok(())
    }

    /// Move a failed queue entry to the dead-letter directory instead of deleting it.
    fn move_to_failed(&self, path: &PathBuf) -> Result<()> {
        let failed_dir = path
            .parent()
            .context("Queue entry has no parent dir")?
            .parent()
            .context("Queue pending dir has no parent")?
            .join("failed");
        fs::create_dir_all(&failed_dir)?;

        let filename = path.file_name().context("Queue entry has no filename")?;
        let dest = failed_dir.join(filename);
        fs::rename(path, &dest)?;
        debug!("Moved failed entry to {}", dest.display());
        Ok(())
    }

    /// Upload a single queue entry to the Convex HTTP endpoint.
    async fn upload(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        match entry.action {
            QueueAction::Create => self.upload_create(entry, token).await,
            QueueAction::Append => {
                // Ensure session exists on server (idempotent create)
                self.upload_create(entry, token).await?;
                self.upload_append(entry, token).await
            }
            QueueAction::Finalize => {
                // Ensure session exists on server (idempotent create)
                self.upload_create(entry, token).await?;
                self.upload_finalize(entry, token).await
            }
        }
    }

    async fn upload_create(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        let meta = self.storage.read_meta(&entry.session_id)?;

        // Build body, omitting None fields (Convex rejects null for optional string fields)
        let mut body = serde_json::json!({
            "session_id": meta.session_id,
            "started_at": meta.started_at.to_rfc3339(),
        });
        if let Some(ref model) = meta.model {
            body["model"] = serde_json::json!(model);
        }
        if let Some(ref cwd) = meta.cwd {
            body["cwd"] = serde_json::json!(cwd);
        }
        if let Some(ref agent) = meta.agent {
            body["agent"] = serde_json::json!(agent);
        }
        if let Some(ref repo) = meta.repo {
            body["repo"] = serde_json::json!(repo);
        }

        // Include client-generated share slug if available
        if let Ok(base) = LocalStorage::default_base_dir() {
            let slug_path = base.join("share-slugs").join(&entry.session_id);
            if let Ok(slug) = fs::read_to_string(&slug_path) {
                let slug = slug.trim();
                if !slug.is_empty() {
                    body["share_slug"] = serde_json::json!(slug);
                }
            }
        }

        let url = format!("{}/api/sessions", convex_site_url());
        info!("Creating session {} at {}", entry.session_id, url);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Create session failed ({}): {}", status, text);
        }

        info!("Created session {} on server", entry.session_id);
        Ok(())
    }

    async fn upload_append(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        // Read from the source transcript (Claude Code's file), not the local copy.
        // The local copy can diverge due to concurrent poller + hook appends.
        let meta = self.storage.read_meta(&entry.session_id)?;
        let transcript_path = &meta.transcript_path;
        let content = std::fs::read_to_string(transcript_path).with_context(|| {
            format!("Failed to read source transcript for {}", entry.session_id)
        })?;

        let all_lines: Vec<&str> = content.lines().collect();

        // Extract the range of lines to send
        let start = entry.lines_start as usize;
        let end = (entry.lines_end as usize).min(all_lines.len());
        if start >= all_lines.len() || start >= end {
            debug!("No lines to append for session {}", entry.session_id);
            return Ok(());
        }

        let lines_to_send: Vec<&str> = all_lines[start..end].to_vec();

        // Process lines through image processor — rewrite image blocks with URLs
        let image_processor = ImageProcessor::new(self.client.clone(), convex_site_url());
        let mut processed_lines: Vec<String> = Vec::with_capacity(lines_to_send.len());
        for line in &lines_to_send {
            if line_has_images(line) {
                let processed = image_processor
                    .process_line(line, &entry.session_id, token)
                    .await;
                processed_lines.push(processed);
            } else {
                processed_lines.push(line.to_string());
            }
        }

        let lines_str = processed_lines.join("\n");
        let line_count = processed_lines.len();
        let mut body = serde_json::json!({
            "lines": lines_str,
            "line_count": line_count,
            "start_line": start,
        });
        if let Some(cost) = meta.total_cost_usd {
            body["total_cost_usd"] = serde_json::json!(cost);
        }
        if let Some(tokens) = meta.total_input_tokens {
            body["total_input_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_output_tokens {
            body["total_output_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_read_tokens {
            body["total_cache_read_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_creation_tokens {
            body["total_cache_creation_tokens"] = serde_json::json!(tokens);
        }
        if let Some(title) = read_session_summary(transcript_path, &entry.session_id) {
            body["title"] = serde_json::json!(title);
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/sessions/{}/append",
                convex_site_url(),
                entry.session_id
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Append lines failed ({}): {}", status, text);
        }

        debug!(
            "Appended {} lines for session {}",
            line_count, entry.session_id
        );
        Ok(())
    }

    async fn upload_finalize(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        let meta = self.storage.read_meta(&entry.session_id)?;

        let mut body = serde_json::json!({});
        if let Some(ref reason) = meta.end_reason {
            body["end_reason"] = serde_json::json!(reason);
        }
        if let Some(ended_at) = meta.ended_at {
            body["ended_at"] = serde_json::json!(ended_at.to_rfc3339());
        }
        if let Some(cost) = meta.total_cost_usd {
            body["total_cost_usd"] = serde_json::json!(cost);
        }
        if let Some(tokens) = meta.total_input_tokens {
            body["total_input_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_output_tokens {
            body["total_output_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_read_tokens {
            body["total_cache_read_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_creation_tokens {
            body["total_cache_creation_tokens"] = serde_json::json!(tokens);
        }
        if let Some(title) = read_session_summary(&meta.transcript_path, &entry.session_id) {
            body["title"] = serde_json::json!(title);
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/sessions/{}/finalize",
                convex_site_url(),
                entry.session_id
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Finalize session failed ({}): {}", status, text);
        }

        debug!("Finalized session {} on server", entry.session_id);
        Ok(())
    }
}
