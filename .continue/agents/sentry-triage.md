---
name: Sentry Triage
on:
  sentry:
    severity: [high, critical]
---

# Sentry Issue Triage

Triage high/critical Sentry issues for the Threader Rust CLI. If the fix is clear and safe, open a fix PR. Otherwise, create a GitHub issue with your analysis. Label issues with `bug` and `sentry`.
