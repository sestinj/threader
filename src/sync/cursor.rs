use anyhow::Result;
use chrono::Utc;
use tracing::info;

use crate::hooks::UploadStatus;
use crate::storage::local::LocalStorage;

/// Manages cursor-based sync tracking for sessions.
pub struct CursorTracker<'a> {
    storage: &'a LocalStorage,
}

impl<'a> CursorTracker<'a> {
    pub fn new(storage: &'a LocalStorage) -> Self {
        Self { storage }
    }

    /// Get the current sync position for a session.
    pub fn get_position(&self, session_id: &str) -> Result<u64> {
        let state = self.storage.read_sync_state(session_id)?;
        Ok(state.last_synced_line)
    }

    /// Advance the cursor after a successful sync.
    pub fn advance(&self, session_id: &str, new_line: u64) -> Result<()> {
        let mut state = self.storage.read_sync_state(session_id)?;
        state.last_synced_line = new_line;
        state.last_synced_at = Utc::now();
        state.upload_status = UploadStatus::Synced;
        state.retry_count = 0;
        self.storage.update_sync_state(session_id, &state)?;
        info!(
            "Advanced cursor for session {} to line {}",
            session_id, new_line
        );
        Ok(())
    }

    /// Mark a sync as in progress. Used by the uploader when processing queue entries.
    #[allow(dead_code)]
    pub fn mark_in_progress(&self, session_id: &str) -> Result<()> {
        let mut state = self.storage.read_sync_state(session_id)?;
        state.upload_status = UploadStatus::InProgress;
        self.storage.update_sync_state(session_id, &state)
    }

    /// Mark a sync as failed, incrementing the retry count. Used by the uploader when processing queue entries.
    #[allow(dead_code)]
    pub fn mark_failed(&self, session_id: &str) -> Result<()> {
        let mut state = self.storage.read_sync_state(session_id)?;
        state.upload_status = UploadStatus::Failed;
        state.retry_count += 1;
        self.storage.update_sync_state(session_id, &state)
    }
}
