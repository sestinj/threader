---
name: Issue Solver
on:
  github:
    issues:
      types: [labeled]
      labels: [autosolve]
---

# Issue Solver

Automatically fix issues labeled `autosolve` in the Threader Rust CLI. Open a fix PR from a branch named `fix/autosolve-{issue-number}` that references `Fixes #{number}`.
