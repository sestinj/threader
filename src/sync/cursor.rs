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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::SessionMeta;
    use crate::storage::local::LocalStorage;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup() -> (LocalStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = LocalStorage::new(tmp.path().to_path_buf());
        storage.init().unwrap();
        (storage, tmp)
    }

    fn create_test_session(storage: &LocalStorage, id: &str) {
        let meta = SessionMeta {
            session_id: id.to_string(),
            transcript_path: PathBuf::from("/tmp/test.jsonl"),
            cwd: None,
            model: None,
            repo: None,
            agent: None,
            started_at: Utc::now(),
            ended_at: None,
            end_reason: None,
            tags: vec![],
            total_cost_usd: None,
            total_input_tokens: None,
            total_output_tokens: None,
            total_cache_read_tokens: None,
            total_cache_creation_tokens: None,
        };
        storage.create_session(&meta).unwrap();
    }

    #[test]
    fn advance_and_get_position_round_trip() {
        let (storage, _tmp) = setup();
        create_test_session(&storage, "test-session");

        let tracker = CursorTracker::new(&storage);

        // Initial position is 0
        assert_eq!(tracker.get_position("test-session").unwrap(), 0);

        // Advance to 42
        tracker.advance("test-session", 42).unwrap();
        assert_eq!(tracker.get_position("test-session").unwrap(), 42);

        // Advance further
        tracker.advance("test-session", 100).unwrap();
        assert_eq!(tracker.get_position("test-session").unwrap(), 100);
    }

    #[test]
    fn advance_updates_sync_state_fields() {
        let (storage, _tmp) = setup();
        create_test_session(&storage, "test-session");

        let tracker = CursorTracker::new(&storage);

        // Mark as failed first to set non-default values
        tracker.mark_failed("test-session").unwrap();
        let state = storage.read_sync_state("test-session").unwrap();
        assert_eq!(state.upload_status, UploadStatus::Failed);
        assert_eq!(state.retry_count, 1);

        // Advance should reset status and retry_count
        tracker.advance("test-session", 10).unwrap();
        let state = storage.read_sync_state("test-session").unwrap();
        assert_eq!(state.last_synced_line, 10);
        assert_eq!(state.upload_status, UploadStatus::Synced);
        assert_eq!(state.retry_count, 0);
    }
}
