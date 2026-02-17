---
name: Sync Correctness
description: Verify that sync logic preserves the append-only, idempotent, convergence-over-coordination guarantees defined in docs/SYNC.md.
---

# Sync Correctness

## Context

Threader's daemon replicates Claude Code transcript JSONL files to the cloud. The sync system is built on four principles documented in `docs/SYNC.md`: server-owned watermarks, idempotent writes, reading from the source transcript, and convergence over coordination. Violating any of these can cause data loss, duplicate lines, or silent sync failures — all invisible to the user since the daemon runs in the background.

## What to Check

### 1. Source of Truth Violations

The uploader must always read from the **source transcript** (`meta.transcript_path` — Claude Code's JSONL file), never from the daemon's local copy at `~/.threader/sessions/<id>/transcript.jsonl`. The local copy can diverge due to concurrent poller and hook appends.

**BAD** — reading from local storage:
```rust
let content = self.storage.read_transcript(&entry.session_id)?;
```

**GOOD** — reading from Claude Code's source file:
```rust
let content = std::fs::read_to_string(&meta.transcript_path)?;
```

Look for any new code in `src/sync/` that reads transcript content from `LocalStorage` instead of `meta.transcript_path`.

### 2. Idempotency Breaks

Every upload operation must be safe to retry. Check that:
- `upload_create` remains idempotent (server handles duplicate creates)
- `upload_append` sends `start_line` so the server can deduplicate by line number
- No code assumes "if I sent it, it was received" — network failures between send and ACK must be tolerable
- Queue entries are only removed after a successful server response, not before

**BAD** — removing from queue before confirming upload:
```rust
self.queue.remove(&path)?;
self.upload(&entry, &token).await?;
```

**GOOD** — removing only after success:
```rust
self.upload(&entry, &token).await?;
self.queue.remove(&path)?;
```

### 3. Cursor and Watermark Consistency

The cursor (`CursorTracker`) is an optimization — correctness must not depend on it. Check that:
- If the cursor is ahead of actual synced lines, the system recovers (server's watermark is authoritative)
- If the cursor is behind, duplicate sends are handled (idempotent append)
- Cursor advances only happen after successful queue enqueue, not before

Review `src/sync/cursor.rs` and `src/sync/poller.rs` for ordering: the pattern should be enqueue first, then advance cursor.

### 4. Concurrent Access Safety

The poller and hook handler can both trigger syncs for the same session. Check that:
- `SESSION_SUMMARY_CACHE` (a `Mutex<Option<HashMap>>`) is accessed safely — lock held briefly, no `.unwrap()` on lock
- No TOCTOU races in file reads (e.g., checking file length then reading — the file may have grown)
- Queue operations are atomic or tolerant of partial writes

## Key Files to Check

- `src/sync/uploader.rs` — upload logic, source transcript reads, queue processing
- `src/sync/poller.rs` — periodic transcript polling, cursor management
- `src/sync/cursor.rs` — cursor tracking and advancement
- `src/hooks/` — hook handler that enqueues sync operations
- `src/storage/queue.rs` — upload queue operations
- `docs/SYNC.md` — authoritative design principles

## Exclusions

- Server-side deduplication logic (lives in `threader-internal`, not this repo)
- Image processing in `src/sync/images.rs` (separate concern)
- Cost calculation in `src/sync/cost.rs` (metadata only, not transcript correctness)
