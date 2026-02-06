use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use sha2::{Digest, Sha256};

use crate::hooks::{SessionMeta, SyncState};
use crate::storage::local::LocalStorage;
use crate::storage::queue::UploadQueue;

#[derive(Subcommand)]
pub enum DebugCommand {
    /// Show all local sessions with sync status at a glance
    List,

    /// Show detailed info for a single session
    Inspect {
        /// Session ID (prefix match supported)
        session_id: String,
    },

    /// Compare transcript across source, local copy, and remote
    Verify {
        /// Session ID (prefix match supported)
        session_id: String,
    },

    /// Show line-by-line content differences between local and remote
    Diff {
        /// Session ID (prefix match supported)
        session_id: String,

        /// Maximum number of differences to show
        #[arg(long, default_value = "20")]
        max_diffs: usize,
    },
}

/// Convex HTTP endpoint base URL (same as uploader.rs).
fn convex_site_url() -> String {
    std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string())
}

// ANSI color helpers
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

pub async fn run(command: DebugCommand) -> Result<()> {
    match command {
        DebugCommand::List => cmd_list().await,
        DebugCommand::Inspect { session_id } => cmd_inspect(&session_id).await,
        DebugCommand::Verify { session_id } => cmd_verify(&session_id).await,
        DebugCommand::Diff {
            session_id,
            max_diffs,
        } => cmd_diff(&session_id, max_diffs).await,
    }
}

/// Resolve a (possibly partial) session ID to a full one.
fn resolve_session_id(storage: &LocalStorage, partial: &str) -> Result<String> {
    let sessions = storage.list_sessions()?;
    let matches: Vec<&String> = sessions.iter().filter(|s| s.starts_with(partial)).collect();
    match matches.len() {
        0 => bail!("No session found matching '{partial}'"),
        1 => Ok(matches[0].clone()),
        n => {
            eprintln!("Ambiguous session ID '{partial}' matches {n} sessions:");
            for m in &matches {
                eprintln!("  {m}");
            }
            bail!("Provide a longer prefix to disambiguate")
        }
    }
}

/// Count lines in a file.
fn count_lines(path: &Path) -> Result<u64> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut count = 0u64;
    for line in reader.lines() {
        line?;
        count += 1;
    }
    Ok(count)
}

/// Read all lines from a file.
fn read_lines(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    reader.lines().collect::<std::io::Result<Vec<_>>>().map_err(Into::into)
}

/// Format a duration as human-readable (e.g. "2h ago", "3d ago").
fn format_age(dt: chrono::DateTime<chrono::Utc>) -> String {
    let dur = chrono::Utc::now() - dt;
    let secs = dur.num_seconds();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

// ── list ──────────────────────────────────────────────────────────────

async fn cmd_list() -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir.clone());
    let queue = UploadQueue::new(base_dir);

    let sessions = storage.list_sessions()?;
    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    // Collect queue entries per session
    let pending = queue.list_pending().unwrap_or_default();
    let mut queue_counts: HashMap<String, usize> = HashMap::new();
    for (_, entry) in &pending {
        *queue_counts.entry(entry.session_id.clone()).or_default() += 1;
    }

    println!(
        "{BOLD}{:<38}  {:<10}  {:>6}  {:>6}  {:>6}  {:>5}  {}{RESET}",
        "Session ID", "Status", "Source", "Local", "Synced", "Queue", "Age"
    );

    let mut rows: Vec<(String, SessionMeta, Option<SyncState>)> = Vec::new();
    for sid in &sessions {
        let meta = match storage.read_meta(sid) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let sync = storage.read_sync_state(sid).ok();
        rows.push((sid.clone(), meta, sync));
    }

    // Sort by started_at descending (most recent first)
    rows.sort_by(|a, b| b.1.started_at.cmp(&a.1.started_at));

    for (sid, meta, sync) in &rows {
        let status = if meta.ended_at.is_some() {
            "finalized"
        } else {
            "active"
        };

        let source_lines = if meta.transcript_path.exists() {
            count_lines(&meta.transcript_path)
                .map(|n| n.to_string())
                .unwrap_or_else(|_| "err".into())
        } else {
            "gone".into()
        };

        let local_path = storage.transcript_path(sid);
        let local_lines = if local_path.exists() {
            count_lines(&local_path)
                .map(|n| n.to_string())
                .unwrap_or_else(|_| "err".into())
        } else {
            "-".into()
        };

        let synced = sync
            .as_ref()
            .map(|s| s.last_synced_line.to_string())
            .unwrap_or_else(|| "-".into());

        let q = queue_counts.get(sid).copied().unwrap_or(0);
        let age = format_age(meta.started_at);

        // Truncate session ID for display
        let display_id = if sid.len() > 36 {
            format!("{}...", &sid[..33])
        } else {
            sid.clone()
        };

        let status_color = if status == "active" { GREEN } else { DIM };
        println!(
            "{:<38}  {status_color}{:<10}{RESET}  {:>6}  {:>6}  {:>6}  {:>5}  {}",
            display_id, status, source_lines, local_lines, synced, q, age
        );
    }

    Ok(())
}

// ── inspect ───────────────────────────────────────────────────────────

async fn cmd_inspect(partial: &str) -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir.clone());
    let queue = UploadQueue::new(base_dir);

    let sid = resolve_session_id(&storage, partial)?;
    let meta = storage.read_meta(&sid)?;
    let sync = storage.read_sync_state(&sid).ok();

    // ── Metadata ──
    println!("{BOLD}Metadata{RESET}");
    println!("  Session ID:  {sid}");
    println!(
        "  Agent:       {}",
        meta.agent.as_deref().unwrap_or("claude-code")
    );
    println!(
        "  Model:       {}",
        meta.model.as_deref().unwrap_or("unknown")
    );
    println!("  CWD:         {}", meta.cwd.as_deref().unwrap_or("-"));
    println!("  Repo:        {}", meta.repo.as_deref().unwrap_or("-"));
    println!("  Started:     {}", meta.started_at);
    println!(
        "  Ended:       {}",
        meta.ended_at
            .map(|t| t.to_string())
            .unwrap_or_else(|| "(active)".into())
    );
    if let Some(reason) = &meta.end_reason {
        println!("  End reason:  {reason}");
    }
    if let Some(cost) = meta.total_cost_usd {
        println!("  Cost:        ${cost:.4}");
    }
    if let Some(t) = meta.total_input_tokens {
        println!("  Input tkns:  {t}");
    }
    if let Some(t) = meta.total_output_tokens {
        println!("  Output tkns: {t}");
    }
    if let Some(t) = meta.total_cache_read_tokens {
        println!("  Cache read:  {t}");
    }
    if let Some(t) = meta.total_cache_creation_tokens {
        println!("  Cache create:{t}");
    }

    // ── Source transcript ──
    println!();
    println!("{BOLD}Source Transcript{RESET}");
    println!("  Path:    {}", meta.transcript_path.display());
    if meta.transcript_path.exists() {
        let size = std::fs::metadata(&meta.transcript_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let lines = count_lines(&meta.transcript_path).unwrap_or(0);
        println!("  Exists:  {GREEN}yes{RESET}");
        println!("  Size:    {} bytes", size);
        println!("  Lines:   {lines}");
    } else {
        println!("  Exists:  {RED}no (source file missing){RESET}");
    }

    // ── Local transcript ──
    let local_path = storage.transcript_path(&sid);
    println!();
    println!("{BOLD}Local Transcript{RESET}");
    println!("  Path:    {}", local_path.display());
    if local_path.exists() {
        let size = std::fs::metadata(&local_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let lines = count_lines(&local_path).unwrap_or(0);
        println!("  Size:    {} bytes", size);
        println!("  Lines:   {lines}");
    } else {
        println!("  {YELLOW}(not yet created){RESET}");
    }

    // ── Sync state ──
    println!();
    println!("{BOLD}Sync State{RESET}");
    if let Some(sync) = sync {
        println!("  Last synced line: {}", sync.last_synced_line);
        println!("  Last synced at:   {}", sync.last_synced_at);
        println!("  Upload status:    {:?}", sync.upload_status);
        println!("  Retry count:      {}", sync.retry_count);

        // How many lines behind?
        if meta.transcript_path.exists() {
            if let Ok(source_lines) = count_lines(&meta.transcript_path) {
                let behind = source_lines.saturating_sub(sync.last_synced_line);
                if behind > 0 {
                    println!("  {YELLOW}Lines behind:    {behind}{RESET}");
                } else {
                    println!("  {GREEN}Fully synced{RESET}");
                }
            }
        }
    } else {
        println!("  {YELLOW}(no sync state file){RESET}");
    }

    // ── Queue entries ──
    println!();
    println!("{BOLD}Queue Entries{RESET}");
    let pending = queue.list_pending().unwrap_or_default();
    let session_entries: Vec<_> = pending
        .iter()
        .filter(|(_, e)| e.session_id == sid)
        .collect();
    if session_entries.is_empty() {
        println!("  (none)");
    } else {
        for (path, entry) in &session_entries {
            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("?");
            println!(
                "  {:?}  lines {}-{}  attempts: {}  {}",
                entry.action, entry.lines_start, entry.lines_end, entry.attempts, filename
            );
        }
    }

    Ok(())
}

// ── verify ────────────────────────────────────────────────────────────

async fn cmd_verify(partial: &str) -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir);
    let sid = resolve_session_id(&storage, partial)?;
    let meta = storage.read_meta(&sid)?;

    let source_lines = if meta.transcript_path.exists() {
        Some(read_lines(&meta.transcript_path)?)
    } else {
        None
    };

    let local_path = storage.transcript_path(&sid);
    let local_lines = if local_path.exists() {
        Some(read_lines(&local_path)?)
    } else {
        None
    };

    // Fetch remote
    let remote_lines = match fetch_remote_transcript(&sid).await {
        Ok(lines) => Some(lines),
        Err(e) => {
            eprintln!(
                "{YELLOW}Could not fetch remote transcript: {e}{RESET}"
            );
            None
        }
    };

    println!("{BOLD}Verification for {sid}{RESET}");
    println!();

    // Source vs Local
    print!("Source vs Local:   ");
    match (&source_lines, &local_lines) {
        (Some(src), Some(loc)) => {
            print_comparison("source", src, "local", loc);
        }
        (None, _) => println!("{YELLOW}SKIP (source file missing){RESET}"),
        (_, None) => println!("{YELLOW}SKIP (local file missing){RESET}"),
    }

    // Source vs Remote
    print!("Source vs Remote:  ");
    match (&source_lines, &remote_lines) {
        (Some(src), Some(rem)) => {
            print_comparison("source", src, "remote", rem);
        }
        (None, _) => println!("{YELLOW}SKIP (source file missing){RESET}"),
        (_, None) => println!("{YELLOW}SKIP (remote unavailable){RESET}"),
    }

    // Local vs Remote
    print!("Local vs Remote:   ");
    match (&local_lines, &remote_lines) {
        (Some(loc), Some(rem)) => {
            print_comparison("local", loc, "remote", rem);
        }
        (None, _) => println!("{YELLOW}SKIP (local file missing){RESET}"),
        (_, None) => println!("{YELLOW}SKIP (remote unavailable){RESET}"),
    }

    // Duplicate detection
    println!();
    println!("{BOLD}Duplicate line detection{RESET}");
    if let Some(ref lines) = source_lines {
        let dups = detect_duplicate_line_numbers(lines);
        if dups.is_empty() {
            println!("  Source:  {GREEN}no duplicates{RESET}");
        } else {
            println!(
                "  Source:  {RED}{} duplicate lineNumbers: {:?}{RESET}",
                dups.len(),
                dups
            );
        }
    }
    if let Some(ref lines) = local_lines {
        let dups = detect_duplicate_line_numbers(lines);
        if dups.is_empty() {
            println!("  Local:   {GREEN}no duplicates{RESET}");
        } else {
            println!(
                "  Local:   {RED}{} duplicate lineNumbers: {:?}{RESET}",
                dups.len(),
                dups
            );
        }
    }
    if let Some(ref lines) = remote_lines {
        let dups = detect_duplicate_line_numbers(lines);
        if dups.is_empty() {
            println!("  Remote:  {GREEN}no duplicates{RESET}");
        } else {
            println!(
                "  Remote:  {RED}{} duplicate lineNumbers: {:?}{RESET}",
                dups.len(),
                dups
            );
        }
    }

    Ok(())
}

fn print_comparison(a_name: &str, a: &[String], b_name: &str, b: &[String]) {
    let a_hash = hash_lines(a);
    let b_hash = hash_lines(b);

    if a_hash == b_hash {
        println!("{GREEN}MATCH{RESET} ({} lines)", a.len());
        return;
    }

    println!("{RED}MISMATCH{RESET}");
    println!(
        "  Line counts: {} ({a_name}) vs {} ({b_name})",
        a.len(),
        b.len()
    );

    // Find first divergence
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        // Skip image-transformed lines (expected differences)
        if is_image_line(&a[i]) || is_image_line(&b[i]) {
            continue;
        }
        if a[i] != b[i] {
            println!("  First content divergence at line {}", i + 1);
            return;
        }
    }
    if a.len() != b.len() {
        println!(
            "  Content matches for first {min_len} lines; {b_name} has {} extra lines",
            b.len().saturating_sub(a.len())
        );
    }
}

fn hash_lines(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// Detect lines that share the same lineNumber field (indicates duplicates in remote).
fn detect_duplicate_line_numbers(lines: &[String]) -> Vec<u64> {
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for line in lines {
        // Try to extract a lineNumber field
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(n) = val.get("lineNumber").and_then(|v| v.as_u64()) {
                *seen.entry(n).or_default() += 1;
            }
        }
    }
    let mut dups: Vec<u64> = seen
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(n, _)| n)
        .collect();
    dups.sort();
    dups
}

/// Check if a transcript line likely contains image data (base64 or image URL).
fn is_image_line(line: &str) -> bool {
    // Lines with base64 image data or rewritten image URLs
    line.contains("\"type\":\"image\"")
        || line.contains("\"mediaType\":\"image/")
        || line.contains("data:image/")
}

// ── diff ──────────────────────────────────────────────────────────────

async fn cmd_diff(partial: &str, max_diffs: usize) -> Result<()> {
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir);
    let sid = resolve_session_id(&storage, partial)?;

    let local_path = storage.transcript_path(&sid);
    if !local_path.exists() {
        bail!("Local transcript not found: {}", local_path.display());
    }
    let local_lines = read_lines(&local_path)?;

    let remote_lines = fetch_remote_transcript(&sid).await?;

    if local_lines == remote_lines {
        println!("{GREEN}Local and remote transcripts are identical ({} lines){RESET}", local_lines.len());
        return Ok(());
    }

    println!("{BOLD}--- local{RESET}");
    println!("{BOLD}+++ remote{RESET}");
    println!();

    let mut diff_count = 0;

    let mut i = 0;
    let mut j = 0;

    while i < local_lines.len() || j < remote_lines.len() {
        if diff_count >= max_diffs {
            println!(
                "{DIM}... (more differences not shown, use --max-diffs to increase){RESET}"
            );
            break;
        }

        match (local_lines.get(i), remote_lines.get(j)) {
            (Some(l), Some(r)) if l == r => {
                // Lines match, skip (show context around diffs)
                i += 1;
                j += 1;
            }
            (Some(l), Some(r)) => {
                // Check if this is an expected image transformation diff
                if is_image_line(l) || is_image_line(r) {
                    println!(
                        "{DIM}@@ line {} (local) / {} (remote) @@ [image transform - expected]{RESET}",
                        i + 1,
                        j + 1
                    );
                    i += 1;
                    j += 1;
                    continue;
                }

                // Check if remote has a duplicate of the local line
                if j + 1 < remote_lines.len() && remote_lines[j] == remote_lines[j + 1] && remote_lines[j] == *l {
                    println!(
                        "{BOLD}@@ line {} (remote) @@{RESET}",
                        j + 1
                    );
                    println!("  {}", truncate_line(l, 120));
                    println!(
                        "{GREEN}+ {}   (duplicate in remote){RESET}",
                        truncate_line(&remote_lines[j + 1], 120)
                    );
                    i += 1;
                    j += 2;
                    diff_count += 1;
                    continue;
                }

                // Generic mismatch
                println!(
                    "{BOLD}@@ line {} (local) / {} (remote) @@{RESET}",
                    i + 1,
                    j + 1
                );
                println!("{RED}- {}{RESET}", truncate_line(l, 120));
                println!("{GREEN}+ {}{RESET}", truncate_line(r, 120));
                i += 1;
                j += 1;
                diff_count += 1;
            }
            (Some(l), None) => {
                println!(
                    "{BOLD}@@ line {} (local only) @@{RESET}",
                    i + 1
                );
                println!("{RED}- {}{RESET}", truncate_line(l, 120));
                i += 1;
                diff_count += 1;
            }
            (None, Some(r)) => {
                println!(
                    "{BOLD}@@ line {} (remote only) @@{RESET}",
                    j + 1
                );
                println!("{GREEN}+ {}{RESET}", truncate_line(r, 120));
                j += 1;
                diff_count += 1;
            }
            (None, None) => break,
        }
    }

    println!();
    println!(
        "Summary: {} local lines, {} remote lines, {} difference(s) shown",
        local_lines.len(),
        remote_lines.len(),
        diff_count
    );

    Ok(())
}

fn truncate_line(line: &str, max: usize) -> String {
    if line.len() <= max {
        line.to_string()
    } else {
        format!("{}...", &line[..max])
    }
}

// ── remote fetch ──────────────────────────────────────────────────────

async fn fetch_remote_transcript(session_id: &str) -> Result<Vec<String>> {
    let token = crate::auth::get_token()
        .await
        .map_err(|e| anyhow::anyhow!("Auth error: {e}"))?;

    let url = format!(
        "{}/api/sessions/{}/transcript",
        convex_site_url(),
        session_id
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to fetch remote transcript")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("Remote fetch failed ({status}): {text}");
    }

    let body: serde_json::Value = resp.json().await.context("Failed to parse remote response")?;

    // Expected response: { "lines": [...], "total": N }
    let lines = body
        .get("lines")
        .and_then(|v| v.as_array())
        .context("Remote response missing 'lines' array")?;

    let result: Vec<String> = lines
        .iter()
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string())
        })
        .collect();

    Ok(result)
}
