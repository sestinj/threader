use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};
use uuid::Uuid;

use threader::storage::local::LocalStorage;
use threader::sync::cost::read_session_cost;

// ---------------------------------------------------------------------------
// Helpers: generate synthetic JSONL transcript data
// ---------------------------------------------------------------------------

/// Build a single assistant transcript line with usage data.
fn assistant_line(request_id: &str, input: u64, output: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "requestId": request_id,
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello, world!"}],
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_read_input_tokens": input / 4,
                "cache_creation_input_tokens": input / 8
            }
        }
    })
    .to_string()
}

/// Build a non-assistant line (user message).
fn user_line() -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": "What is Rust?"}]
        }
    })
    .to_string()
}

/// Build a progress line (filler).
fn progress_line() -> String {
    serde_json::json!({
        "type": "progress",
        "progress": 0.5
    })
    .to_string()
}

/// Write a synthetic transcript JSONL file with `n` total lines.
/// Roughly 40% assistant lines, 30% user lines, 30% progress lines.
fn write_transcript(file: &mut impl Write, n: usize) {
    for i in 0..n {
        let line = match i % 10 {
            0 | 2 | 5 | 7 => {
                let rid = Uuid::new_v4().to_string();
                assistant_line(&rid, 1200 + (i as u64 % 500), 350 + (i as u64 % 200))
            }
            1 | 4 | 8 => user_line(),
            _ => progress_line(),
        };
        writeln!(file, "{}", line).unwrap();
    }
}

/// Create a temporary transcript file with `n` lines and return the NamedTempFile.
fn make_transcript(n: usize) -> NamedTempFile {
    let mut tmp = NamedTempFile::new().unwrap();
    write_transcript(&mut tmp, n);
    tmp.flush().unwrap();
    tmp
}

// ---------------------------------------------------------------------------
// Benchmark: read_session_cost
// ---------------------------------------------------------------------------

fn bench_read_session_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_session_cost");

    for size in [100, 1000, 5000] {
        let tmp = make_transcript(size);
        let path = tmp.path().to_path_buf();

        group.bench_with_input(BenchmarkId::new("lines", size), &size, |b, _| {
            b.iter(|| {
                let result = read_session_cost(&path).unwrap();
                assert!(result.is_some());
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: read_transcript_lines
// ---------------------------------------------------------------------------

fn bench_read_transcript_lines(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_transcript_lines");

    // Set up a LocalStorage instance
    let tmp_dir = TempDir::new().unwrap();
    let storage = LocalStorage::new(tmp_dir.path().to_path_buf());
    storage.init().unwrap();

    for &(total_lines, from_line) in &[
        (100, 0),     // small file, read all
        (100, 90),    // small file, read last 10
        (1000, 0),    // medium file, read all
        (1000, 900),  // medium file, read last 100
        (5000, 0),    // large file, read all
        (5000, 4900), // large file, read last 100
    ] {
        let tmp = make_transcript(total_lines);
        let path = tmp.path().to_path_buf();

        let label = format!("total={}/from={}", total_lines, from_line);
        group.bench_with_input(BenchmarkId::new("read", &label), &label, |b, _| {
            b.iter(|| {
                let (lines, total) = storage.read_transcript_lines(&path, from_line).unwrap();
                assert_eq!(total, total_lines as u64);
                assert_eq!(lines.len(), (total_lines - from_line as usize));
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_read_session_cost,
    bench_read_transcript_lines
);
criterion_main!(benches);
