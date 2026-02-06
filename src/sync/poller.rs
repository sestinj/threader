use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tokio::time;
use tracing::{info, warn};

use crate::storage::local::LocalStorage;
use crate::storage::queue::UploadQueue;
use crate::sync::cost;
use crate::sync::cursor::CursorTracker;

const POLL_INTERVAL_SECS: u64 = 5;

/// Background worker that periodically polls active sessions' transcript files
/// for new lines and syncs them, so content appears before the agent turn ends.
pub struct TranscriptPoller {
    storage: LocalStorage,
    queue: UploadQueue,
}

impl TranscriptPoller {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            storage: LocalStorage::new(base_dir.clone()),
            queue: UploadQueue::new(base_dir),
        }
    }

    /// Run the polling loop. Checks active sessions for new transcript lines periodically.
    pub async fn run(&self) -> Result<()> {
        info!("Transcript poller started (interval={}s)", POLL_INTERVAL_SECS);

        loop {
            time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            self.poll_active_sessions();
        }
    }

    fn poll_active_sessions(&self) {
        let active = match self.storage.list_active_sessions() {
            Ok(a) => a,
            Err(e) => {
                warn!("Poller: failed to list active sessions: {}", e);
                return;
            }
        };

        if active.is_empty() {
            return;
        }

        let tracker = CursorTracker::new(&self.storage);

        for meta in &active {
            let session_id = &meta.session_id;
            let last_line = match tracker.get_position(session_id) {
                Ok(l) => l,
                Err(_) => continue,
            };

            let (new_lines, total_lines) =
                match self.storage.read_transcript_lines(&meta.transcript_path, last_line) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

            if new_lines.is_empty() {
                continue;
            }

            if let Err(e) = self.storage.append_transcript(session_id, &new_lines) {
                warn!("Poller: failed to append transcript for {}: {}", session_id, e);
                continue;
            }

            if let Err(e) = self.queue.enqueue_append(session_id, last_line, total_lines) {
                warn!("Poller: failed to enqueue append for {}: {}", session_id, e);
                continue;
            }

            if let Err(e) = tracker.advance(session_id, total_lines) {
                warn!("Poller: failed to advance cursor for {}: {}", session_id, e);
                continue;
            }

            info!(
                "Poller: synced {} new lines for session {} (lines {}-{})",
                new_lines.len(),
                session_id,
                last_line,
                total_lines
            );

            // Update token counts from the source transcript
            self.refresh_cost(meta);
        }
    }

    fn refresh_cost(&self, meta: &crate::hooks::SessionMeta) {
        match cost::read_session_cost(&meta.transcript_path) {
            Ok(Some(c)) => {
                if let Ok(mut m) = self.storage.read_meta(&meta.session_id) {
                    m.total_cost_usd = Some(c.total_cost_usd);
                    m.total_input_tokens = Some(c.total_input_tokens);
                    m.total_output_tokens = Some(c.total_output_tokens);
                    m.total_cache_read_tokens = Some(c.total_cache_read_tokens);
                    m.total_cache_creation_tokens = Some(c.total_cache_creation_tokens);
                    if let Err(e) = self.storage.update_meta(&m) {
                        warn!("Poller: failed to update cost meta for {}: {}", meta.session_id, e);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                warn!("Poller: failed to read cost for {}: {}", meta.session_id, e);
            }
        }
    }
}
