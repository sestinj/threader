use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, info};

use crate::hooks::{SessionMeta, SyncState, UploadStatus};

/// Manages local storage under ~/.threader/
pub struct LocalStorage {
    base_dir: PathBuf,
}

impl LocalStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Returns the default base directory (~/.threader/).
    pub fn default_base_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".threader"))
    }

    /// Initialize the directory structure.
    pub fn init(&self) -> Result<()> {
        let dirs = [
            self.base_dir.clone(),
            self.sessions_dir(),
            self.queue_dir(),
            self.pending_dir(),
            self.logs_dir(),
        ];
        for dir in &dirs {
            fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
        }
        info!("Initialized storage at {}", self.base_dir.display());
        Ok(())
    }

    fn sessions_dir(&self) -> PathBuf {
        self.base_dir.join("sessions")
    }

    fn queue_dir(&self) -> PathBuf {
        self.base_dir.join("queue")
    }

    fn pending_dir(&self) -> PathBuf {
        self.base_dir.join("queue").join("pending")
    }

    fn logs_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    pub fn meta_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("meta.json")
    }

    pub fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("transcript.jsonl")
    }

    pub fn sync_state_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("sync_state.json")
    }

    pub fn socket_path(&self) -> PathBuf {
        self.base_dir.join("threader.sock")
    }

    /// Create a new session directory and write initial metadata.
    pub fn create_session(&self, meta: &SessionMeta) -> Result<()> {
        let dir = self.session_dir(&meta.session_id);
        fs::create_dir_all(&dir)?;

        self.write_json_atomic(&self.meta_path(&meta.session_id), meta)?;

        let sync_state = SyncState {
            last_synced_line: 0,
            last_synced_at: Utc::now(),
            upload_status: UploadStatus::Pending,
            retry_count: 0,
        };
        self.write_json_atomic(&self.sync_state_path(&meta.session_id), &sync_state)?;

        // Create empty transcript file
        fs::write(self.transcript_path(&meta.session_id), "")?;

        info!("Created session: {}", meta.session_id);
        Ok(())
    }

    /// Read session metadata.
    pub fn read_meta(&self, session_id: &str) -> Result<SessionMeta> {
        let path = self.meta_path(session_id);
        let data = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read meta: {}", path.display()))?;
        serde_json::from_str(&data).context("Failed to parse meta.json")
    }

    /// Update session metadata.
    pub fn update_meta(&self, meta: &SessionMeta) -> Result<()> {
        self.write_json_atomic(&self.meta_path(&meta.session_id), meta)
    }

    /// Read sync state for a session.
    pub fn read_sync_state(&self, session_id: &str) -> Result<SyncState> {
        let path = self.sync_state_path(session_id);
        let data = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read sync state: {}", path.display()))?;
        serde_json::from_str(&data).context("Failed to parse sync_state.json")
    }

    /// Update sync state for a session.
    pub fn update_sync_state(&self, session_id: &str, state: &SyncState) -> Result<()> {
        self.write_json_atomic(&self.sync_state_path(session_id), state)
    }

    /// Read new lines from the source transcript file starting at `from_line`.
    /// Returns the lines and the new total line count.
    pub fn read_transcript_lines(
        &self,
        source_path: &Path,
        from_line: u64,
    ) -> Result<(Vec<String>, u64)> {
        let file = fs::File::open(source_path)
            .with_context(|| format!("Failed to open transcript: {}", source_path.display()))?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        let mut total = 0u64;

        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            total = i as u64 + 1;
            if total > from_line {
                lines.push(line);
            }
        }

        debug!(
            "Read {} new lines from {} (starting at line {})",
            lines.len(),
            source_path.display(),
            from_line
        );
        Ok((lines, total))
    }

    /// Append lines to the local transcript copy.
    pub fn append_transcript(&self, session_id: &str, lines: &[String]) -> Result<()> {
        use std::io::Write;

        let path = self.transcript_path(session_id);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        for line in lines {
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }

    /// Write JSON atomically using temp file + rename.
    fn write_json_atomic<T: serde::Serialize>(&self, path: &Path, data: &T) -> Result<()> {
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(data)?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// List all session IDs.
    pub fn list_sessions(&self) -> Result<Vec<String>> {
        let dir = self.sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    sessions.push(name.to_string());
                }
            }
        }
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::io::Write;
    use tempfile::TempDir;

    fn test_storage() -> (LocalStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = LocalStorage::new(tmp.path().to_path_buf());
        storage.init().unwrap();
        (storage, tmp)
    }

    fn test_meta() -> SessionMeta {
        SessionMeta {
            session_id: "test-session-1".to_string(),
            transcript_path: PathBuf::from("/tmp/transcript.jsonl"),
            cwd: Some("/home/user/project".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            started_at: Utc::now(),
            ended_at: None,
            end_reason: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_init_creates_directories() {
        let (storage, tmp) = test_storage();
        assert!(tmp.path().join("sessions").is_dir());
        assert!(tmp.path().join("queue").join("pending").is_dir());
        assert!(tmp.path().join("logs").is_dir());
        drop(storage);
    }

    #[test]
    fn test_create_and_read_session() {
        let (storage, _tmp) = test_storage();
        let meta = test_meta();

        storage.create_session(&meta).unwrap();

        let read_meta = storage.read_meta(&meta.session_id).unwrap();
        assert_eq!(read_meta.session_id, meta.session_id);
        assert_eq!(read_meta.cwd, meta.cwd);

        let sync = storage.read_sync_state(&meta.session_id).unwrap();
        assert_eq!(sync.last_synced_line, 0);
        assert_eq!(sync.upload_status, UploadStatus::Pending);
    }

    #[test]
    fn test_read_transcript_lines() {
        let (storage, _tmp) = test_storage();

        // Create a temporary transcript file
        let transcript_dir = _tmp.path().join("transcripts");
        fs::create_dir_all(&transcript_dir).unwrap();
        let transcript_path = transcript_dir.join("test.jsonl");
        {
            let mut f = fs::File::create(&transcript_path).unwrap();
            writeln!(f, r#"{{"type":"message","content":"hello"}}"#).unwrap();
            writeln!(f, r#"{{"type":"message","content":"world"}}"#).unwrap();
            writeln!(f, r#"{{"type":"message","content":"foo"}}"#).unwrap();
        }

        // Read all lines
        let (lines, total) = storage.read_transcript_lines(&transcript_path, 0).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(total, 3);

        // Read from line 2 onward
        let (lines, total) = storage.read_transcript_lines(&transcript_path, 2).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(total, 3);
        assert!(lines[0].contains("foo"));
    }

    #[test]
    fn test_append_transcript() {
        let (storage, _tmp) = test_storage();
        let meta = test_meta();
        storage.create_session(&meta).unwrap();

        let lines = vec![
            r#"{"type":"message","content":"hello"}"#.to_string(),
            r#"{"type":"message","content":"world"}"#.to_string(),
        ];
        storage.append_transcript(&meta.session_id, &lines).unwrap();

        let content = fs::read_to_string(storage.transcript_path(&meta.session_id)).unwrap();
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn test_update_meta() {
        let (storage, _tmp) = test_storage();
        let mut meta = test_meta();
        storage.create_session(&meta).unwrap();

        meta.ended_at = Some(Utc::now());
        meta.end_reason = Some("user_exit".to_string());
        storage.update_meta(&meta).unwrap();

        let read = storage.read_meta(&meta.session_id).unwrap();
        assert!(read.ended_at.is_some());
        assert_eq!(read.end_reason.unwrap(), "user_exit");
    }
}
