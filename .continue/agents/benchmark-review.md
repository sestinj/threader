---
name: Benchmark Review
---

Check if this PR introduces new performance-critical code paths that should have benchmarks. Look for: tight loops, per-line processing of large files, repeated JSON parsing, network-bound retry loops, or anything in the sync/upload hot path. If you find code that could benefit from a criterion benchmark, suggest adding one in `benches/sync_benchmarks.rs`.
