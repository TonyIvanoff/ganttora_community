//! # Document tests: the entry point of the `document` test target
//!
//! ## What this file is for
//! Cargo builds everything in `tests/rust/document/` into one test program,
//! starting from this file. It lists the test files (modules) and holds the
//! small helpers they share.
//!
//! ## Where it fits
//! Declared as `[[test]] name = "document"` in `tests/rust/Cargo.toml`. Run it
//! with:
//! ```text
//! cargo test -p ganttora-tests --test document
//! ```
//!
//! ## Main ideas
//! - The `document` crate is the "open project session": it holds the project,
//!   its calculated schedule and the undo/redo history. Every edit is a
//!   `Command` (decision D16). These tests use only its public API
//!   (`Document::apply`, `undo`, `redo`, `snapshot` ...), exactly like the app.
//! - A `ChangeContext` says *who* made a change, *when*, and which user action
//!   (batch) it belongs to. The tests use fixed contexts so the results are the
//!   same on every run.
//!
//! ## The test files
//! | File | What it checks |
//! |---|---|
//! | `commands.rs` | every `Command` variant: apply, undo, redo, and its errors |
//! | `history.rs` | undo/redo rules: refused changes, no-op commands, redo cleared |
//! | `journal.rs` | change records: how many, which fields, old/new values, context |
//! | `shed.rs` | the book's example: task 2 from 5 to 6 days moves the finish |
//! | `json.rs` | the exact JSON shapes exchanged with the UI |
//! | `task_fields.rs` | M1 task-field, link and project commands, with records |
//! | `outline.rs` | `model::Outline`, indent/outdent/move, outline refusals |
//! | `calendars.rs` | calendar commands, `CalendarInUse`, undo-all |
//! | `batch.rs` | `Command::Batch`: one undo step, all or nothing; one kind of lag |
//! | `blockers.rs` | blocker commands, refusals, records, snapshot fields (D23) |
//! | `clipboard.rs` | copy/paste of task rows as tab-separated text |
//!
//! ## Documentation
//! - the project documentation

mod batch;
mod blockers;
mod calendars;
mod clipboard;
mod commands;
mod history;
mod journal;
mod json;
mod outline;
mod projects;
mod shed;
mod task_fields;

use chrono::{DateTime, Utc};
use document::{BatchId, ChangeContext, Document};
use ganttora_tests::dt;
use model::{CalendarId, Project};
use uuid::Uuid;

/// A fixed "who, when, which action" for edits in tests.
///
/// # Returns
/// A context with author "Tester", timestamp 2027-02-15 10:00 UTC and a batch
/// ID made from the number 1, so every run produces identical change records.
///
/// # Beginner note
/// In the app the context comes from the settings (the user's name) and the
/// clock. Tests must not depend on the clock, otherwise a test could pass
/// today and fail tomorrow.
pub(crate) fn ctx() -> ChangeContext {
    ctx_with("Tester", "2027-02-15 10:00", 1)
}

/// Builds a context with chosen values.
///
/// # Parameters
/// - `author`: the name written into each change record.
/// - `when`: the moment as `"YYYY-MM-DD HH:MM"`, read as UTC.
/// - `batch`: a small number turned into the batch UUID (`Uuid::from_u128`).
///
/// # Returns
/// The `ChangeContext`.
pub(crate) fn ctx_with(author: &str, when: &str, batch: u128) -> ChangeContext {
    ChangeContext {
        author: author.to_string(),
        timestamp: utc(when),
        batch_id: BatchId::from_uuid(Uuid::from_u128(batch)),
    }
}

/// Turns `"YYYY-MM-DD HH:MM"` into a UTC timestamp.
///
/// # Parameters
/// - `when`: the moment as text.
///
/// # Returns
/// The same moment as a `DateTime<Utc>` (a date and time *with* a time zone).
pub(crate) fn utc(when: &str) -> DateTime<Utc> {
    dt(when).and_utc()
}

/// Opens a project in a new document and checks that it schedules.
///
/// # Parameters
/// - `project`: the project to open (moved into the document).
///
/// # Returns
/// The new `Document`.
///
/// # Errors / edge cases
/// Fails the test if the project does not schedule, because most tests need
/// a working schedule to start from.
pub(crate) fn open_ok(project: Project) -> Document {
    let document = Document::new(project);
    if let Err(error) = document.schedule() {
        panic!("the test project does not schedule: {error}");
    }
    document
}

/// The ID of a calendar that is never part of any test project.
///
/// # Returns
/// A fixed `CalendarId` (made from the number `0xDEAD`), so failures are
/// repeatable.
pub(crate) fn missing_calendar() -> CalendarId {
    CalendarId::from_uuid(Uuid::from_u128(0xDEAD))
}

/// Breaks a project's schedule on purpose: the task in `row` (counted from 1,
/// like the book's task numbers) gets a task calendar that does not exist.
///
/// # Parameters
/// - `project`: the project to break (changed in place).
/// - `row`: the task number, 1 = first task.
///
/// # Errors / edge cases
/// After this, `engine::schedule` returns `TaskCalendarNotFound`. Tests use
/// it to check how the document behaves while a project cannot be scheduled.
/// (In M0 an SS link did this job; since M1 every link type schedules.)
///
/// # Beginner note
/// Tests sometimes need "bad" data. Choosing a mistake that the engine
/// clearly reports (a missing calendar) keeps such tests simple.
pub(crate) fn break_schedule(project: &mut Project, row: usize) {
    project.tasks[row - 1].calendar_id = Some(missing_calendar());
}
