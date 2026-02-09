---
name: PR Quality Gate
---

Review the PR diff for **improper error types**: functions that return `Result<_, String>` or use `String` as an error type instead of a proper error type (e.g., `anyhow::Error`, a custom enum with `thiserror`).

If you find violations, post a review comment listing each one with the file and line, and suggest using `anyhow::Result` or defining a domain error type.

If the diff looks clean, post a brief approval comment.
