//! # .gtra file tests: the entry point of the `gtra` test target
//!
//! ## What this file is for
//! Cargo builds everything in `tests/rust/gtra/` into one test program,
//! starting from this file. It lists the test files (modules) and holds the
//! helpers they share.
//!
//! ## Where it fits
//! Declared as `[[test]] name = "gtra"` in `tests/rust/Cargo.toml`. Run it
//! with:
//! ```text
//! cargo test -p ganttora-tests --test gtra
//! ```
//!
//! ## Main ideas
//! - A `.gtra` file is a SQLite database. The `gtra` crate
//!   saves a `model::Project` into it and reads it back. These tests use its
//!   public API: `gtra::save`, `gtra::open`, `gtra::create_empty`,
//!   `gtra::open_db`, `gtra::schema` and `gtra::atomic`.
//! - Every test works in its **own temporary folder** (`tempfile::TempDir`),
//!   which is deleted when the test ends. Tests never write into the
//!   repository and never see each other's files, so they can run in
//!   parallel.
//! - The file does not store the *order* of links or calendars (they are
//!   sets in the database). `normalized` sorts both lists before comparing.
//!
//! ## The test files
//! | File | What it checks |
//! |---|---|
//! | `roundtrip.rs` | save then open gives the same project (shed, outline, calendars...) |
//! | `m1_format.rs` | format version 2: every M1 field round-trips; format-1 files upgrade |
//! | `session.rs` | ProjectSession, recovery copies, newer files, journal, error codes |
//! | `resave.rs` | saving again after edits updates the file correctly |
//! | `atomic.rs` | a save is all-or-nothing and leaves no temporary files |
//! | `blockers.rs` | blockers round trip, format 2 → 3 upgrade, bad rows reported (D23) |
//! | `versions.rs` | newer files open read-only; not-a-gtra files; empty files; migrations |
//! | `database.rs` | the rules inside the database (foreign keys, cascades) |
//! | `journal_unknown.rs` | journal rows of unknown kinds are skipped and counted |
//!
//! ## Documentation
//! - the project documentation

mod atomic;
mod blockers;
mod database;
mod journal_unknown;
mod m1_format;
mod resave;
mod roundtrip;
mod session;
mod versions;

use chrono::{DateTime, Utc};
use ganttora_tests::dt;
use gtra::{OpenedProject, SaveOptions};
use model::Project;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Makes a fresh, empty temporary folder for one test.
///
/// # Returns
/// A `TempDir`. The folder is deleted automatically when this value is
/// dropped (goes out of scope at the end of the test).
///
/// # Beginner note
/// "Dropped" is Rust's word for "thrown away". When a value is dropped, Rust
/// runs its clean-up code; for `TempDir` that deletes the folder. See book
/// chapter on ownership.
pub(crate) fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("cannot create a temporary folder")
}

/// A fixed timestamp for saves, so file metadata is the same on every run.
pub(crate) fn saved_at(when: &str) -> DateTime<Utc> {
    dt(when).and_utc()
}

/// Save options with author "Tester" at 2027-02-15 10:00 UTC.
pub(crate) fn options() -> SaveOptions {
    SaveOptions::new(saved_at("2027-02-15 10:00"), "Tester")
}

/// Saves a project and fails the test with the error message if saving fails.
///
/// # Parameters
/// - `path`: where to save.
/// - `project`: what to save.
pub(crate) fn save_ok(path: &Path, project: &Project) {
    gtra::save(path, project, &options()).unwrap_or_else(|error| panic!("save failed: {error}"));
}

/// Opens a file and fails the test with the error message if opening fails.
///
/// # Parameters
/// - `path`: the file to open.
///
/// # Returns
/// What `gtra::open` returned: the project plus notes, access and version.
pub(crate) fn open_ok(path: &Path) -> OpenedProject {
    gtra::open(path).unwrap_or_else(|error| panic!("open failed: {error}"))
}

/// A copy of the project with links and calendars sorted by ID.
///
/// # Parameters
/// - `project`: the project to copy.
///
/// # Returns
/// The sorted copy. Task order is kept, because the file *does* store it.
///
/// # Why
/// The database keeps links and calendars as unordered rows, so after a save
/// and an open they may come back in a different order. Comparing sorted
/// copies compares them "as sets".
pub(crate) fn normalized(project: &Project) -> Project {
    let mut copy = project.clone();
    copy.dependencies.sort_by_key(|d| d.id);
    copy.calendars.sort_by_key(|c| c.id);
    copy
}

/// Lists the names of all files in a folder, sorted.
///
/// # Parameters
/// - `dir`: the folder.
///
/// # Returns
/// File names (not full paths). Used to check that no temporary files
/// (`.name.gtra.<uuid>.tmp`, `-journal`) are left behind.
pub(crate) fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("cannot list the folder")
        .map(|entry| {
            entry
                .expect("cannot read a folder entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The path of `name` inside a temporary folder.
pub(crate) fn path_in(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}
