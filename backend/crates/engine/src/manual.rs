//! # Manually scheduled tasks: the user decides the dates
//!
//! ## What this file is for
//! Places a **manually scheduled** task. Such a task stays exactly where the
//! user put it: links and constraints do not move it. It still pushes its
//! successors, and its late dates are still calculated (so it can show
//! negative slack when it sits too late for the plan).
//!
//! ## Where it fits
//! Called by the forward pass for every manual task, and by the backward
//! pass when the backward pass decides the dates (scheduling from the
//! finish).
//!
//! ## The rule
//! - start = `task.manual_start` (if missing: the pass's anchor, i.e. the
//!   project start in the forward pass);
//! - finish = start + duration (+ blocked time, see `blockers.rs`), counted
//!   in working time on the task's calendar. The start is **not** moved to working time: a manual task can
//!   start on a Saturday if the user says so (MS Project allows that too).
//!
//! ```text
//! manual start Sat 6 Mar 2027 10:00, duration 1 day, Standard calendar
//!   finish = 1 working day after Saturday 10:00 = Mon 8 Mar 17:00
//! ```
//!
//! ## Documentation

use crate::calendar_math::{CalendarError, add_working_time};
use crate::constraints::Placement;
use crate::cpm::TaskDates;
use crate::explain::{Bound, Driver, TaskEnd};
use chrono::NaiveDateTime;
use model::{Calendar, Task};

/// Places a manually scheduled task at its typed start.
///
/// # Parameters
/// - `task`: the task (its `manual_start` and `duration` are read).
/// - `calendar`: the task's calendar, used to count the duration.
/// - `fallback`: the start used when the task has no `manual_start`.
///
/// # Returns
/// The [`Placement`]; its explanation says `ManualStart` (or
/// `ProjectStart` when the fallback was used).
///
/// # Errors
/// A [`CalendarError`] from adding the duration.
///
/// # Example
/// ```
/// use engine::manual::place_manual;
/// use model::{Calendar, Duration, Task, TaskMode};
/// use chrono::NaiveDate;
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let mut task = Task::new("Inspection", Duration::from_days(1));
/// task.mode = TaskMode::Manual;
/// task.manual_start = Some(at(6, 10)); // a Saturday
/// let placed = place_manual(&task, &Calendar::standard(), at(1, 8)).unwrap();
/// assert_eq!(placed.dates.start, at(6, 10));
/// assert_eq!(placed.dates.finish, at(8, 17));
/// ```
pub fn place_manual(
    task: &Task,
    calendar: &Calendar,
    fallback: NaiveDateTime,
) -> Result<Placement, CalendarError> {
    let (start, driver) = match task.manual_start {
        Some(start) => (start, Driver::ManualStart),
        None => (fallback, Driver::ProjectStart),
    };
    // Work plus blocked time: a blocker extends a manual task from its
    // typed start, exactly like an automatic one.
    let finish = add_working_time(calendar, start, crate::blockers::span(task))?;
    Ok(Placement {
        dates: TaskDates { start, finish },
        explanation: Bound::new(start, driver).explain(TaskEnd::Start, start, calendar.id),
    })
}
