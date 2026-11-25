//! # Engine tests: the entry point of the `engine` test target
//!
//! ## What this file is for
//! Cargo builds everything in `tests/rust/engine/` into one test program,
//! starting from this file. It lists the test files (modules) and holds two
//! tiny helpers that all of them share.
//!
//! ## Where it fits
//! Declared as `[[test]] name = "engine"` in `tests/rust/Cargo.toml`. Run it
//! with:
//! ```text
//! cargo test -p ganttora-tests --test engine
//! ```
//!
//! ## Main ideas
//! - The engine is tested **only through its public API**: `engine::schedule`
//!   for whole projects and `engine::calendar_math` for date arithmetic. This
//!   is decision D17: tests use the engine exactly the way the app does, so
//!   the engine's insides can be rewritten freely as long as the answers stay
//!   right.
//! - Expected dates come from the book (sections 2.3 and 2.4, chapter 3) and
//!   from working them out by hand on the standard calendar: Monday to Friday,
//!   08:00–12:00 and 13:00–17:00, 8 working hours a day.
//!
//! ## The test files
//! | File | What it checks |
//! |---|---|
//! | `basics.rs` | empty project, one task, independent tasks, looking up results |
//! | `shed.rs` | the garden shed from the book: every date, slack, critical path |
//! | `calendars.rs` | holidays, weekends, project start outside working time |
//! | `lags.rs` | waiting time (lag) and overlap (lead) on links |
//! | `milestones.rs` | zero-duration tasks |
//! | `errors.rs` | every way scheduling can fail, and the error it gives |
//! | `determinism.rs` | same input, same output, whatever the input order |
//! | `calendar_math.rs` | adding and subtracting working time, counting it |
//! | `size.rs` | a generated 10,000-task project schedules correctly |
//! | `link_types.rs` | FS, SS, FF, SF links with time and percentage lags (M1) |
//! | `constraints.rs` | the 8 constraints, deadlines, negative slack (M1) |
//! | `task_calendars.rs` | task calendars and calendar exceptions (M1) |
//! | `summaries.rs` | summary roll-up, links into/out of summaries (M1) |
//! | `manual_tasks.rs` | manually scheduled tasks (M1) |
//! | `alap.rs` | "as late as possible" and scheduling from the finish (M1) |
//! | `explain.rs` | "Why this date?": every driver, tie-breaking (M1) |
//! | `blockers.rs` | blockers: blocked time, segments, slack (D23) |
//! | `version.rs` | `ENGINE_VERSION`; loops are found before calendar errors |
//!
//! ## Documentation
//! - the project documentation

// Each `mod name;` line pulls in the file `name.rs` from this folder.
mod alap;
mod basics;
mod blockers;
mod calendar_math;
mod calendars;
mod constraints;
mod determinism;
mod errors;
mod explain;
mod lags;
mod link_types;
mod manual_tasks;
mod milestones;
mod projects;
mod shed;
mod size;
mod summaries;
mod task_calendars;
mod version;

use engine::{ScheduleResult, TaskSchedule, schedule};
use model::{Project, TaskId};

/// Schedules a project and stops the test if the engine returns an error.
///
/// # Parameters
/// - `project`: the project to schedule.
///
/// # Returns
/// The engine's result.
///
/// # Errors / edge cases
/// If scheduling fails, the test fails with the engine's error message, so
/// the reader sees *why* (for example "circular dependency: A -> B -> A").
pub(crate) fn schedule_ok(project: &Project) -> ScheduleResult {
    // `unwrap_or_else` runs the closure only in the error case.
    schedule(project).unwrap_or_else(|error| panic!("scheduling failed: {error}"))
}

/// Finds one task's calculated dates in a result.
///
/// # Parameters
/// - `result`: what the engine returned.
/// - `id`: the task to look up.
///
/// # Returns
/// The task's [`TaskSchedule`] (early/late dates, slack, critical flag).
///
/// # Errors / edge cases
/// A missing task fails the test: every task must appear in the result.
pub(crate) fn dates_of(result: &ScheduleResult, id: TaskId) -> &TaskSchedule {
    result
        .task(id)
        .unwrap_or_else(|| panic!("task {id} is missing from the schedule result"))
}
