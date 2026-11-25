//! # Benchmark: scheduling 10,000 tasks
//!
//! ## What this file is for
//! Measures how long `engine::schedule` takes for big generated projects.
//! The M0 target is that a 10,000-task project schedules quickly
//! enough to recalculate after every edit.
//!
//! ## Where it fits
//! Declared as `[[bench]] name = "schedule_10k"` in `tests/rust/Cargo.toml`
//! (it reuses the project generators of `tests/rust/common/`). Run it from
//! the repository root with:
//! ```text
//! cargo bench -p ganttora-tests
//! ```
//! Benchmarks always build in optimised ("release") mode, so the times are
//! close to what users will see.
//!
//! ## Main ideas
//! - **Criterion** runs each case many times, throws away warm-up runs and
//!   prints the typical time with a range, for example
//!   `time: [11.2 ms 11.4 ms 11.6 ms]` (low estimate, best estimate, high
//!   estimate).
//! - Three cases:
//!   - `layered_10k`: a realistic project, 200 phases × 50 tasks, about
//!     15,000 links, about 2–3 years long.
//!   - `stress/chain_10k`: one chain of 10,000 tasks lasting over 200 years.
//!     Every date step stays short (one task's duration), so it is fast.
//!   - `stress/wide_slack_10k`: one 10-year task plus 9,999 unlinked 1-day
//!     tasks. Each short task has about 10 years of slack, and the M0 engine
//!     counts slack by walking the calendar day by day, so this is the
//!     worst case: it shows how much the day-by-day walking costs.
//! - `black_box` tells the compiler "pretend you don't know this value", so
//!   it cannot skip work whose result is never used.

use criterion::{Criterion, criterion_group, criterion_main};
use ganttora_tests::generate::{chain_project, layered_project, wide_slack_project};
use std::hint::black_box;

/// Defines the benchmark cases.
///
/// # Parameters
/// - `c`: Criterion's runner; each `bench_function` call adds one case.
fn bench_schedule(c: &mut Criterion) {
    // Build the projects once, outside the timed part: we measure only the
    // engine, not the generator.
    let layered = layered_project(200, 50, 2027);
    let chain = chain_project(10_000, 2027);
    // 2,600 working days = 10 years of 260 working days.
    let wide = wide_slack_project(10_000, 2_600);

    // `iter` runs the closure many times and times it.
    c.bench_function("layered_10k", |b| {
        b.iter(|| engine::schedule(black_box(&layered)))
    });

    // Stress cases can be slow, so fewer samples (10 is Criterion's minimum).
    let mut group = c.benchmark_group("stress");
    group.sample_size(10);
    group.bench_function("chain_10k", |b| {
        b.iter(|| engine::schedule(black_box(&chain)))
    });
    group.bench_function("wide_slack_10k", |b| {
        b.iter(|| engine::schedule(black_box(&wide)))
    });
    group.finish();
}

// These two macros write the `main` function for us: Criterion's own
// runner instead of Rust's test runner (hence `harness = false`).
criterion_group!(benches, bench_schedule);
criterion_main!(benches);
