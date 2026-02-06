# Transcript Sync Design

The daemon replicates an append-only log (a Claude Code transcript JSONL file) from the local machine to the Threader cloud (Convex + R2).

## Design Principles

### 1. Server owns the watermark

The server's max `lineNumber` per session is the source of truth for "what it already has." The client-side cursor is an optimization to avoid re-sending known lines, but correctness does not depend on it. If the cursor drifts, the server catches duplicates.

### 2. Idempotent writes

Every `appendLines` call is safe to retry. The server queries its highest existing `lineNumber` and skips lines at or below that watermark. This means:
- Crash between enqueue and cursor advance? Safe — duplicate send is a no-op.
- Poller and hook both sync the same range? Safe — the second send is a no-op.
- Daemon restart resends the entire transcript? Safe — all existing lines are skipped.

### 3. Read from source of truth

The uploader reads from the **source transcript** (Claude Code's JSONL file at `meta.transcript_path`), not the daemon's local copy at `~/.threader/sessions/<id>/transcript.jsonl`. The local copy can diverge if both the poller and hook handler append concurrently. The source transcript is written by Claude Code exclusively and is always correct.

### 4. Convergence over coordination

Rather than preventing duplicate sends with locks and mutexes, duplicates are made harmless via server-side dedup. This is simpler and more robust — any component can send any range of lines at any time, and the system converges to the correct state.

## Data Flow

```
Claude Code JSONL  ──→  Daemon (poller/hook)  ──→  Convex (appendLines)  ──→  R2 (archival)
    (source)               (reader)                 (dedup + store)           (collectLines dedup)
```

### Source transcript (Claude Code)
- Written exclusively by Claude Code
- Append-only JSONL file at `~/.claude/projects/<slug>/<session-id>.jsonl`
- The daemon never writes to this file

### Daemon
- **Poller**: Periodically reads source transcript, computes diff from cursor, enqueues new lines
- **Hook handler**: Triggered by Claude Code hooks, enqueues new lines immediately
- **Uploader**: Processes queue entries, reads from source transcript, sends to server
- **Cursor**: Tracks last-synced line per session (optimization, not correctness)

### Convex (server)
- **`appendLines`**: Accepts lines with lineNumbers, queries max existing lineNumber, inserts only new lines
- **`collectLines`**: Deduplicates by lineNumber before archival to R2 (defense-in-depth)

### R2 (archival)
- Stores the full transcript as a single object after session finalization
- Read path falls back to R2 when transcript lines have been cleaned up from Convex

## Invariants

1. **No duplicate lines in R2**: `collectLines` deduplicates before archival
2. **lineCount reflects actual rows**: `appendLines` increments by the number of actually-inserted lines
3. **messageCount counts real messages**: Only incremented for lines that pass the watermark check
4. **Source transcript is never modified**: The daemon is a read-only consumer of Claude Code's file
