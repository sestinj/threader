use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, warn};

use crate::hooks::{QueueAction, QueueEntry};

/// Persistent file-based upload queue.
///
/// Each entry is stored as a JSON file in `~/.threader/queue/pending/`.
/// Filename format: `{timestamp_millis}_{session_id}.json`
pub struct UploadQueue {
    pending_dir: PathBuf,
}

impl UploadQueue {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            pending_dir: base_dir.join("queue").join("pending"),
        }
    }

    /// Enqueue a new upload action.
    pub fn enqueue(&self, entry: &QueueEntry) -> Result<()> {
        fs::create_dir_all(&self.pending_dir)?;

        let filename = format!(
            "{}_{}.json",
            entry.created_at.timestamp_millis(),
            entry.session_id
        );
        let path = self.pending_dir.join(&filename);
        let tmp = path.with_extension("tmp");

        let json = serde_json::to_string_pretty(entry)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &path)?;

        debug!("Enqueued {:?} for session {}", entry.action, entry.session_id);
        Ok(())
    }

    /// List all pending entries, sorted by creation time (oldest first).
    pub fn list_pending(&self) -> Result<Vec<(PathBuf, QueueEntry)>> {
        if !self.pending_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<(PathBuf, QueueEntry)> = Vec::new();

        for dir_entry in fs::read_dir(&self.pending_dir)? {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(data) => match serde_json::from_str::<QueueEntry>(&data) {
                    Ok(entry) => entries.push((path, entry)),
                    Err(e) => warn!("Skipping malformed queue entry {}: {}", path.display(), e),
                },
                Err(e) => warn!("Failed to read queue entry {}: {}", path.display(), e),
            }
        }

        // Sort by creation time
        entries.sort_by_key(|(_, e)| e.created_at);
        Ok(entries)
    }

    /// Remove a completed queue entry.
    pub fn remove(&self, path: &PathBuf) -> Result<()> {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove queue entry: {}", path.display()))?;
        debug!("Removed queue entry: {}", path.display());
        Ok(())
    }

    /// Update a queue entry (e.g., increment attempts).
    pub fn update(&self, path: &PathBuf, entry: &QueueEntry) -> Result<()> {
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(entry)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Create a queue entry for a new session.
    pub fn enqueue_create(&self, session_id: &str) -> Result<()> {
        self.enqueue(&QueueEntry {
            session_id: session_id.to_string(),
            action: QueueAction::Create,
            lines_start: 0,
            lines_end: 0,
            created_at: Utc::now(),
            attempts: 0,
        })
    }

    /// Create a queue entry for appending transcript lines.
    pub fn enqueue_append(&self, session_id: &str, start: u64, end: u64) -> Result<()> {
        self.enqueue(&QueueEntry {
            session_id: session_id.to_string(),
            action: QueueAction::Append,
            lines_start: start,
            lines_end: end,
            created_at: Utc::now(),
            attempts: 0,
        })
    }

    /// Create a queue entry for finalizing a session.
    pub fn enqueue_finalize(&self, session_id: &str) -> Result<()> {
        self.enqueue(&QueueEntry {
            session_id: session_id.to_string(),
            action: QueueAction::Finalize,
            lines_start: 0,
            lines_end: 0,
            created_at: Utc::now(),
            attempts: 0,
        })
    }

    /// Count pending entries.
    pub fn pending_count(&self) -> Result<usize> {
        Ok(self.list_pending()?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_queue() -> (UploadQueue, TempDir) {
        let tmp = TempDir::new().unwrap();
        let queue = UploadQueue::new(tmp.path().to_path_buf());
        (queue, tmp)
    }

    #[test]
    fn test_enqueue_and_list() {
        let (queue, _tmp) = test_queue();

        queue.enqueue_create("session-1").unwrap();
        queue.enqueue_append("session-1", 0, 10).unwrap();

        let entries = queue.list_pending().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1.action, QueueAction::Create);
        assert_eq!(entries[1].1.action, QueueAction::Append);
    }

    #[test]
    fn test_remove_entry() {
        let (queue, _tmp) = test_queue();

        queue.enqueue_create("session-1").unwrap();
        let entries = queue.list_pending().unwrap();
        assert_eq!(entries.len(), 1);

        queue.remove(&entries[0].0).unwrap();
        let entries = queue.list_pending().unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_empty_queue() {
        let (queue, _tmp) = test_queue();
        let entries = queue.list_pending().unwrap();
        assert!(entries.is_empty());
        assert_eq!(queue.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_enqueue_finalize() {
        let (queue, _tmp) = test_queue();
        queue.enqueue_finalize("session-1").unwrap();
        let entries = queue.list_pending().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.action, QueueAction::Finalize);
    }
}
