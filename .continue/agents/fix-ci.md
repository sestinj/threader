---
name: Fix CI
on:
  github:
    check_run:
      conclusion: [failure]
---

# CI Failure Auto-Fixer

Fix CI failures for the Threader Rust CLI. For mechanical issues (formatting, clippy, compilation), push a fix commit. For test failures or audit issues, comment on the PR with analysis instead — don't push speculative fixes.

Run `cargo check && cargo test` before pushing to make sure you don't introduce new failures. Use commit message format: `fix: auto-fix <description>`.
