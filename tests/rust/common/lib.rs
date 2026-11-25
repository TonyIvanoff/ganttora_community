//! # `ganttora_tests`: shared helpers for all Rust tests
//!
//! ## What this file is for
//! This is the small helper library that every Rust test target in
//! `tests/rust/` (and the benchmark in `tests/perf/rust/`) can use. It keeps
//! the tests short and readable: instead of ten lines of setup, a test
//! writes `let project = shed();` or `dt("2027-03-01 08:00")`.
//!
//! ## Where it fits
//! ```text
//!   tests/rust/common/  (this library)
//!        ▲         ▲
//!        │         └── tests/perf/rust/schedule_10k.rs  (benchmark)
//!        └──────────── tests/rust/engine/*.rs           (engine tests)
//! ```
//! It is declared as the `[lib]` of the `ganttora-tests` crate in
//! `tests/rust/Cargo.toml`.
//!
//! ## Main ideas
//! - Helpers only build *input* (projects, dates). They never check results:
//!   checking is the job of each test, so a reader sees the expected values
//!   right in the test.
//! - Helpers use only the public API of `model` and `engine` (decision D17).
//! - When a helper gets bad input (for example a misspelled date), it stops
//!   the test with a clear message (`panic!`). In tests that is what we want:
//!   a broken test should fail loudly, not continue with wrong data.
//!
//! ## Modules
//! - [`dates`]: turn short text like `"2027-03-01 08:00"` into dates.
//! - [`builders`]: build projects: an empty one, tasks, links, the garden shed.
//! - [`generate`]: build big, repeatable projects (10,000 tasks) for
//!   performance tests.
//!
//! ## Documentation
//! - How to run the tests: the project documentation

pub mod builders;
pub mod dates;
pub mod generate;

// Re-exports: the helpers used most often, available directly as
// `ganttora_tests::shed` instead of `ganttora_tests::builders::shed`.
pub use builders::{add_task, days, hours, link, link_with_lag, project_on, row_id, shed};
pub use dates::{date, dt};
