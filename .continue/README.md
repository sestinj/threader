# Agent Checks Demo

This repo has four [Continue Agents](https://docs.continue.dev/agent-checks) configured in `.continue/agents/`. Here's how to demo them.

## Agents

| Agent | Trigger | What it does |
|-------|---------|-------------|
| **Fix CI** | CI failure | Auto-fixes formatting, clippy, and compilation errors. Comments on test/audit failures instead of guessing. |
| **PR Quality Gate** | Every PR | Reviews diff for `Result<_, String>` anti-pattern — suggests `anyhow::Result` or custom error types. |
| **Issue Solver** | Issue labeled `autosolve` | Opens a fix PR from `fix/autosolve-{issue-number}`. |
| **Sentry Triage** | High/critical Sentry alert | Opens a fix PR if clear, otherwise files a GitHub issue with analysis. |

## How to Demo

### 1. Fix CI (easiest — start here)

1. Open any `.rs` file and break formatting (e.g. remove a space, add an extra blank line)
2. Push to a branch and open a PR
3. CI fails on `cargo fmt --check`
4. The **Fix CI** agent picks up the failure, pushes a fix commit, and CI goes green

### 2. PR Quality Gate

1. Add a function that returns `Result<(), String>` anywhere in `src/`
2. Open a PR
3. The **PR Quality Gate** agent posts a review comment pointing out the bad error type

### 3. Issue Solver

1. Create a GitHub issue describing a small bug or improvement
2. Add the `autosolve` label
3. The **Issue Solver** agent opens a PR referencing the issue

### 4. Sentry Triage

Requires a Sentry integration. When a high/critical issue fires, the agent either opens a fix PR or files a GitHub issue with root-cause analysis.
