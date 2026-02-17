---
name: Security & Auth Review
description: Check token handling, credential storage, secret leakage prevention, and crypto usage for security best practices.
---

# Security & Auth Review

## Context

Threader handles OAuth tokens (WorkOS device flow), encrypted credential storage (XChaCha20-Poly1305 + Argon2), and transmits user session data to the cloud. The daemon runs continuously in the background, so security issues persist silently. Credentials are stored encrypted at `~/.local/share/threader/daemon/credentials.enc` with machine-ID-derived keys.

## What to Check

### 1. Token and Secret Leakage in Logs

Tokens, credentials, and session content must never appear in log output. The project uses `tracing` for structured logging.

**BAD** — logging the token value:
```rust
info!("Using token: {}", token);
debug!("Auth response: {:?}", auth_response);
```

**GOOD** — logging only metadata:
```rust
debug!("Got auth token (len={})", token.len());
info!("Auth token refreshed for user {}", creds.user_id);
```

Check all `tracing::` calls (`debug!`, `info!`, `warn!`, `error!`) in `src/auth/`, `src/sync/uploader.rs`, and `src/daemon/` for accidental secret logging. Also check that `Debug` derives on structs containing tokens don't leak values (e.g., `Credentials` in `src/auth/mod.rs` derives `Debug`).

### 2. Credential Storage Security

Credentials are encrypted with XChaCha20-Poly1305 using a key derived from the machine ID via Argon2. Check that:
- The encryption key is never written to disk or logged
- File permissions are set to `0o600` on Unix after writing (`src/auth/storage.rs:150-154`)
- Nonces are generated from `OsRng` (cryptographically secure), never reused or predictable
- Plaintext credentials are not left in memory longer than necessary (no intermediate temp files)

**BAD** — writing plaintext credentials:
```rust
fs::write(&path, &json)?; // plaintext on disk
```

**GOOD** — encrypt before writing:
```rust
let encrypted = encrypt(json.as_bytes(), &key)?;
fs::write(&path, &encrypted)?;
```

### 3. Network Security

All HTTP requests must use HTTPS. The project uses `reqwest` with `rustls-tls`.

Check that:
- No hardcoded `http://` URLs (only `https://`)
- Environment variable overrides (`THREADER_CONVEX_SITE_URL`, `THREADER_WORKOS_CLIENT_ID`) don't downgrade to HTTP without warning
- Bearer tokens are sent via `.bearer_auth()`, not in URL query parameters or custom headers
- Response bodies from failed auth requests are not re-exposed to users in a way that leaks server internals

**BAD** — token in URL:
```rust
let url = format!("{}/api/sessions?token={}", base_url, token);
```

**GOOD** — token in Authorization header:
```rust
self.client.post(&url).bearer_auth(token).json(&body).send().await?;
```

### 4. Auth Flow Correctness

The device flow (`src/auth/device_flow.rs`) and token refresh (`src/auth/mod.rs`) must handle edge cases securely:
- Expired tokens must trigger re-auth, not fail silently
- Refresh token rotation: after refresh, the old refresh token must be replaced with the new one
- JWT parsing (`parse_jwt_expiry`) must not trust unverified claims for security decisions beyond caching — currently used only for expiry check, which is appropriate
- The `client_id` is not a secret (it's a public identifier), but API keys or secrets must never be hardcoded

### 5. Input Validation at System Boundaries

Data from external sources (Claude Code transcript files, server responses, environment variables) should be validated:
- Transcript JSONL lines should be valid JSON before sending to the server
- Server error responses should not be blindly displayed to users (may contain internal details)
- File paths from `meta.transcript_path` should be validated to prevent path traversal

## Key Files to Check

- `src/auth/mod.rs` — token management, JWT parsing, refresh flow
- `src/auth/storage.rs` — encrypted credential storage, key derivation
- `src/auth/device_flow.rs` — OAuth device flow
- `src/sync/uploader.rs` — bearer token usage, HTTPS endpoints
- `src/main.rs` — Sentry DSN and telemetry configuration
- `install.sh` — installation script (curl-pipe-bash pattern)

## Exclusions

- Server-side auth validation (lives in `threader-internal`)
- Sentry telemetry data (controlled by `THREADER_NO_TELEMETRY=1`, not a credential concern)
- Public values like `DEFAULT_CLIENT_ID` (WorkOS client IDs are public by design)
