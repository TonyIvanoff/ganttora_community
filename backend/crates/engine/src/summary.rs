//! # Summary tasks: dates rolled up from their subtasks
//!
//! ## What this file is for
//! A **summary task** (a task with indented tasks below it) has no dates of
//! its own: it starts when its earliest subtask starts and finishes when its
//! latest subtask finishes. This file does that "roll-up".
//!
//! ## Where it fits
//! The forward pass rolls up early dates (at the summary's own node), the
//! backward pass late dates (at its gate), and [`crate::cpm::run`] rolls up
//! the final task dates after ALAP/ASAP tasks have moved
//! ([`roll_up_scheduled`]).
//!
//! ## Worked example
//! ```text
//! Preparation (summary)      Mon 1 Mar 08:00  -  Tue 9 Mar 17:00   <- rolled up
//!   Design     Mon 1 Mar 08:00 - Tue 2 Mar 17:00
//!   Permit     Wed 3 Mar 08:00 - Tue 9 Mar 17:00
//!   Buy        Wed 3 Mar 08:00 - Fri 5 Mar 17:00
//! start = earliest subtask start = Mon 1 Mar 08:00
//! finish = latest subtask finish = Tue 9 Mar 17:00
//! duration = working time in between = 7 days (on the project calendar)
//! ```
//!
//! ## Documentation

use crate::cpm::TaskDates;
use crate::explain::{Bound, Driver, Explanation, TaskEnd};
use chrono::NaiveDateTime;
use model::{CalendarId, Outline};

/// Rolls up a list of subtask dates: earliest start, latest finish.
///
/// # Parameters
/// - `children`: the dates of the subtasks (any iterator).
///
/// # Returns
/// `Some(rolled-up dates)`, or `None` if there were no subtasks.
///
/// # Example
/// ```
/// use engine::summary::roll_up;
/// use engine::cpm::TaskDates;
/// use chrono::NaiveDate;
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let children = [
///     TaskDates { start: at(1, 8), finish: at(2, 17) },
///     TaskDates { start: at(3, 8), finish: at(9, 17) },
/// ];
/// let rolled = roll_up(children.into_iter()).unwrap();
/// assert_eq!((rolled.start, rolled.finish), (at(1, 8), at(9, 17)));
/// ```
///
/// # Beginner note: `impl Iterator`
/// `impl Iterator<Item = TaskDates>` accepts anything that hands out
/// `TaskDates` one by one: a list, a filtered list, a mapped list… without
/// building a new `Vec` first.
pub fn roll_up(children: impl Iterator<Item = TaskDates>) -> Option<TaskDates> {
    // `fold` walks the items, carrying a running result (here: the dates so
    // far, or None before the first item).
    children.fold(None, |acc: Option<TaskDates>, d| {
        Some(match acc {
            None => d,
            Some(a) => TaskDates {
                start: a.start.min(d.start),
                finish: a.finish.max(d.finish),
            },
        })
    })
}

/// Rolls up the current task dates of every ordinary task inside a summary
/// task's block of rows.
///
/// # Parameters
/// - `outline`: the outline.
/// - `summary`: the summary task's row.
/// - `scheduled`: the current dates of all rows.
///
/// # Returns
/// The rolled-up dates (the summary's own entry if it somehow had no
/// ordinary task below it).
pub fn scheduled_roll_up(outline: &Outline, summary: usize, scheduled: &[TaskDates]) -> TaskDates {
    let leaves = (summary + 1..outline.subtree_end(summary))
        .filter(|&row| !outline.is_summary(row))
        .map(|row| scheduled[row]);
    roll_up(leaves).unwrap_or(scheduled[summary])
}

/// Rolls up every summary task's dates from the current task dates, in
/// place (after ALAP/ASAP tasks have moved).
///
/// # Parameters
/// - `outline`: the outline.
/// - `scheduled`: the dates of all rows; summary rows are overwritten.
pub fn roll_up_scheduled(outline: &Outline, scheduled: &mut [TaskDates]) {
    // Bottom-up (last row first), so a summary inside a summary is already
    // rolled up when its parent is. Direct children are enough then.
    for row in (0..scheduled.len()).rev() {
        if !outline.is_summary(row) {
            continue;
        }
        let children = (row + 1..outline.subtree_end(row))
            .filter(|&r| outline.parent(r) == Some(row))
            .map(|r| scheduled[r]);
        if let Some(rolled) = roll_up(children) {
            scheduled[row] = rolled;
        }
    }
}

/// The explanation of a summary task: "rolled up from its subtasks".
///
/// # Parameters
/// - `at`: the rolled-up moment shown as the driven date.
/// - `end`: which end it is.
/// - `calendar_id`: the project calendar (summary tasks are measured on it).
pub fn summary_explanation(
    at: NaiveDateTime,
    end: TaskEnd,
    calendar_id: CalendarId,
) -> Explanation {
    Bound::new(at, Driver::Subtasks).explain(end, at, calendar_id)
}
