---
name: Error Handling Patterns
description: Ensure consistent error handling with proper context propagation, no silent failures, and correct use of anyhow vs thiserror.
---

# Error Handling Patterns

## Context

Threader uses two error handling crates: `thiserror` for domain-specific error types (e.g., `AuthError` in `src/auth/mod.rs`) and `anyhow` for general error propagation. As a background daemon, silent failures are especially dangerous — users won't see errors unless they check logs. The project enforces `#[warn(clippy::unwrap_used)]` to prevent panics, but this doesn't catch swallowed errors or missing context.

## What to Check

### 1. Silent Error Swallowing

Errors must not be silently ignored. Every error should be logged, propagated, or explicitly documented as intentionally discarded.

**BAD** — silently ignoring a result:
```rust
let _ = self.storage.update_meta(&m);
```

**BAD** — using `.ok()` without logging:
```rust
let content = fs::read_to_string(&path).ok()?;
```

**GOOD** — logging before discarding:
```rust
if let Err(e) = self.storage.update_meta(&m) {
    warn!("Failed to update meta for {}: {}", session_id, e);
}
```

**GOOD** — propagating with context:
```rust
let content = fs::read_to_string(&path)
    .with_context(|| format!("Failed to read transcript for {}", session_id))?;
```

Note: `.ok()` is acceptable for truly optional reads (e.g., `read_session_summary_uncached` where the index file may legitimately not exist), but new code should prefer explicit handling.

### 2. Error Context in anyhow Chains

When using `anyhow::Result`, errors should include enough context for debugging from logs alone. The daemon runs in the background, so stack traces aren't visible to users.

**BAD** — bare `?` with no context:
```rust
let meta = self.storage.read_meta(&entry.session_id)?;
```

**GOOD** — adding session context:
```rust
let meta = self.storage.read_meta(&entry.session_id)
    .with_context(|| format!("reading meta for session {}", entry.session_id))?;
```

Focus on new code in `src/sync/`, `src/daemon/`, and `src/hooks/` where errors propagate through multiple layers.

### 3. thiserror vs anyhow Boundaries

Domain errors (`AuthError`, etc.) should use `thiserror` with meaningful variants. Generic errors should use `anyhow`. Check that:
- New error types in public module APIs use `thiserror` (like `AuthError` in `src/auth/mod.rs`)
- Internal helper functions use `anyhow::Result` with `.context()`
- No `anyhow::anyhow!("some string")` where a proper error variant would be clearer

**BAD** — stringly-typed domain error:
```rust
Err(anyhow::anyhow!("not logged in"))
```

**GOOD** — typed domain error:
```rust
Err(AuthError::NotLoggedIn)
```

### 4. Daemon Resilience

The daemon must never crash from a single session's error. Check that:
- Loop bodies in `src/sync/poller.rs` and `src/sync/uploader.rs` catch errors per-session and continue processing other sessions
- `tokio::spawn` tasks have error handling at the top level
- Panics are caught at task boundaries (or the code avoids panicking)

**BAD** — one session error kills the loop:
```rust
for session in sessions {
    self.process(session)?; // exits loop on first error
}
```

**GOOD** — per-session error handling:
```rust
for session in sessions {
    if let Err(e) = self.process(session) {
        warn!("Error processing session {}: {}", session.id, e);
    }
}
```

## Key Files to Check

- `src/sync/uploader.rs` — upload error handling and retry logic
- `src/sync/poller.rs` — polling loop resilience
- `src/daemon/` — daemon startup and task spawning
- `src/auth/mod.rs` — `AuthError` as the reference pattern for `thiserror`
- `src/hooks/` — hook event processing
- `src/cli/` — CLI-facing error messages (should be user-friendly)

## Exclusions

- Existing `.ok()` usage in `read_session_summary_uncached` (intentional for optional data)
- Test code error handling patterns
- Build scripts or tooling
