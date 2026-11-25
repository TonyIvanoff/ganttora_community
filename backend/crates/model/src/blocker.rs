//! # Blockers: unplanned delays recorded on a task
//!
//! ## What this file is for
//! Defines [`Blocker`], one unplanned delay on a task. The
//! user story behind it:
//!
//! > "I ordered wood with delivery in 5 business days. After 4 days I hear
//! > that delivery is extended by another week. This blocks my plan and
//! > makes the whole project longer."
//!
//! A blocker says: "after this much of the task's work was done, the task
//! was stuck for this long, because of this reason". The Gantt chart draws
//! the blocked time in its own colour, so the planned duration stays
//! visible next to the delay.
//!
//! ## Where it fits
//! Part of the `model` crate (data only). A [`Task`](crate::Task) holds its
//! blockers in `Task::blockers`. The engine (`engine::blockers`) makes the
//! task longer by the blocked time; the `document` crate adds, edits and
//! removes blockers with commands (so they can be undone and appear in the
//! change journal).
//!
//! ## Main ideas
//! - **The position is stored as "after X of work"**, not as a date. If a
//!   predecessor slips by two days, the task moves, and its blocker moves
//!   with it, still "after 2 days of work". A stored date would stay behind
//!   and end up outside the task. It also keeps the engine deterministic:
//!   the engine never needs to know "today".
//! - **Both lengths are working time** on the task's calendar, in minutes,
//!   like every [`Duration`] (1 day = 480 minutes on an 8-hour day).
//! - **Several blockers** on one task are applied in order of `after_work`
//!   (ties: by `id`, so the order never depends on how they are stored).
//!
//! ## Worked example: the shed's "Buy materials"
//! ```text
//! task: 3 days of work, scheduled Wed 3 Mar 2027 08:00 - Fri 5 Mar 17:00
//! blocker: after_work = 2 days (960 min), duration = 5 days (2400 min)
//!
//!   work      Wed 3 Mar 08:00 - Thu 4 Mar 17:00    (2 days of work done)
//!   blocked   Fri 5 Mar 08:00 - Thu 11 Mar 17:00   (5 working days stuck)
//!   work      Fri 12 Mar 08:00 - Fri 12 Mar 17:00  (the last day of work)
//!
//! the task now ends Fri 12 Mar 17:00: 3 + 5 = 8 working days after its start
//! ```
//!
//! ## Not in MS Project
//! MS Project has no blockers. The closest feature is *splitting* a task
//! (a gap in the bar), which has no reason and is not counted separately.
//!
//! ## Documentation

use crate::duration::Duration;
use crate::ids::BlockerId;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// One unplanned delay on a task.
///
/// # Fields
/// - `id`: unique ID of the blocker (a UUID, never a list position).
/// - `after_work`: how much of the task's **own work** was done before the
///   blocker started, in working minutes. This is the authoritative
///   position. 0 = the task was blocked before any work; a value equal to
///   (or larger than) the task's duration = the blocker comes after all the
///   work and simply extends the end. The engine clamps it to
///   `0..=duration`.
/// - `duration`: how long the task was (or will be) blocked, in working
///   minutes on the task's calendar. Must not be negative.
/// - `reason`: free text, e.g. "Wood delivery delayed by the supplier".
/// - `reported_on`: the day the user learned about the delay, if given.
///   Only information for the user and later reports; the engine ignores it.
///
/// # JSON
/// Field names in camelCase; the two durations are written like every
/// [`Duration`] (`{"minutes": 960}`):
/// ```text
/// { "id": "…", "afterWork": { "minutes": 960 }, "duration": { "minutes": 2400 },
///   "reason": "Wood delivery delayed", "reportedOn": "2027-03-04" }
/// ```
///
/// # Example
/// ```
/// use model::{Blocker, BlockerId, Duration};
/// let late_wood = Blocker::new(
///     BlockerId::new(),
///     Duration::from_days(2),
///     Duration::from_days(5),
///     "Wood delivery delayed",
/// );
/// assert_eq!(late_wood.duration.minutes(), 2400);
/// assert!(late_wood.reported_on.is_none());
/// ```
///
/// # Beginner note: `Option<NaiveDate>`
/// `Option` means "maybe there is a value": `Some(date)` or `None`. A
/// `NaiveDate` is a calendar day without a time and without a time zone.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    /// Unique ID of the blocker.
    pub id: BlockerId,
    /// Working minutes of the task's own work done before the blocker starts.
    pub after_work: Duration,
    /// How long the task is blocked, in working minutes (task calendar).
    pub duration: Duration,
    /// Why the task is blocked (free text; may be empty).
    #[serde(default)]
    pub reason: String,
    /// The day the delay was reported, if known.
    #[serde(default)]
    pub reported_on: Option<NaiveDate>,
}

impl Blocker {
    /// Creates a blocker without a "reported on" day.
    ///
    /// # Parameters
    /// - `id`: the blocker's ID (the caller makes it, so tests can use fixed IDs).
    /// - `after_work`: work done before the blocker starts.
    /// - `duration`: how long the task is blocked.
    /// - `reason`: why.
    ///
    /// # Returns
    /// The new blocker.
    ///
    /// # Example
    /// ```
    /// use model::{Blocker, BlockerId, Duration};
    /// let b = Blocker::new(BlockerId::new(), Duration::ZERO, Duration::from_days(1), "Rain");
    /// assert!(b.after_work.is_zero()); // blocked before any work started
    /// ```
    pub fn new(
        id: BlockerId,
        after_work: Duration,
        duration: Duration,
        reason: impl Into<String>,
    ) -> Self {
        Blocker {
            id,
            after_work,
            duration,
            reason: reason.into(),
            reported_on: None,
        }
    }
}
