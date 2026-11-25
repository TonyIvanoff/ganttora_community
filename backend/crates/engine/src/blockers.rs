//! # Blockers in the schedule: blocked time makes a task longer
//!
//! ## What this file is for
//! A **blocker** ([`model::Blocker`]) is an unplanned delay on a
//! task: "after 2 days of work the task was stuck for 5 days". This file
//! holds the two rules the engine needs for blockers:
//!
//! 1. [`span`]: how much working time a task occupies in the schedule,
//!    i.e. its work **plus** all blocked time. The passes place tasks with
//!    this length instead of the bare duration.
//! 2. [`segments`]: once the task's start is known, where each piece lies:
//!    work, blocked, work… The UI draws blocked pieces in their own colour.
//!
//! ## Where it fits
//! - `constraints.rs` (`place_forward`, `place_backward`) and `manual.rs`
//!   call [`span`] for every ordinary task, in **both** passes. So early and
//!   late dates, slack and the critical path all include the blocked time.
//! - `cpm/mod.rs` calls [`segments`] at the very end, on the scheduled
//!   dates, and stores them in [`TaskSchedule`](crate::TaskSchedule).
//!
//! ## Why the span is enough for the passes
//! Work and blocked time are both counted in working minutes on the **same**
//! calendar (the task's). So for the finish date it does not matter *where*
//! inside the task the blocked time sits: 2 days work + 5 days blocked +
//! 1 day work ends exactly where 8 days of plain work would end. Only the
//! drawing (the segments) needs to know the order.
//!
//! ## The walk (rules for the segments)
//! Starting at the task's scheduled start, with "work done" = 0:
//! 1. Sort the blockers by `after_work` (clamped to `0..=duration`), ties
//!    by blocker ID (determinism: the stored order never matters).
//! 2. For each blocker: first do work until "work done" reaches its
//!    `after_work` (a work segment), then the blocked time (a blocked
//!    segment).
//! 3. Then the rest of the work (a last work segment).
//!
//! Empty pieces are left out. A piece that follows another one starts at the
//! next working moment (Thu 17:00 → Fri 08:00), like a successor task does.
//!
//! ## Worked example: the shed's "Buy materials" (Standard calendar)
//! ```text
//! duration 3d, scheduled start Wed 3 Mar 2027 08:00
//! blocker: after_work 2d, duration 5d
//!
//! span = 3d + 5d = 8d  -> finish = Wed 3 Mar 08:00 + 8 working days
//!                                = Fri 12 Mar 17:00   (was Fri 5 Mar 17:00)
//! segments:
//!   work     Wed 3 Mar 08:00  - Thu 4 Mar 17:00    2d of work (after_work reached)
//!   blocked  Fri 5 Mar 08:00  - Thu 11 Mar 17:00   5d blocked
//!   work     Fri 12 Mar 08:00 - Fri 12 Mar 17:00   the remaining 1d
//! ```
//! "Buy materials" had 4 days of total slack (it had to finish by Thu 11 Mar
//! 17:00). It now finishes one working day later than that, so "Build the
//! floor" and everything after it move by **one** working day, and the shed
//! finishes Wed 24 Mar instead of Tue 23 Mar. Had the task been critical
//! (no slack), the project would have moved by all 5 days.
//!
//! The same example as code, with the whole shed project:
//! ```
//! use engine::schedule;
//! use model::{Blocker, BlockerId, Dependency, Duration, Project, Task};
//! use chrono::NaiveDate;
//! let mut shed = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
//! let rows = [("Design", 2), ("Permit", 5), ("Buy materials", 3), ("Ground", 2),
//!             ("Floor", 2), ("Walls and roof", 4), ("Paint", 2), ("Finished", 0)];
//! for (name, days) in rows {
//!     shed.tasks.push(Task::new(name, Duration::from_days(days)));
//! }
//! let id = |row: usize| shed.tasks[row].id;
//! // The links of the project documentation (rows counted from 0 here).
//! for (from, to) in [(0, 1), (0, 2), (1, 3), (2, 4), (3, 4), (4, 5), (5, 6), (6, 7)] {
//!     let link = Dependency::finish_to_start(id(from), id(to));
//!     shed.dependencies.push(link);
//! }
//! assert_eq!(schedule(&shed).unwrap().project_finish.to_string(), "2027-03-23 17:00:00");
//!
//! // The wood is late: after 2 days of work, "Buy materials" is blocked for 5 days.
//! shed.tasks[2].blockers.push(Blocker::new(
//!     BlockerId::new(), Duration::from_days(2), Duration::from_days(5), "Wood delivery delayed",
//! ));
//! let result = schedule(&shed).unwrap();
//! let buy = &result.tasks[2];
//! assert_eq!(buy.finish.to_string(), "2027-03-12 17:00:00");   // was Fri 5 Mar
//! assert_eq!(buy.duration, Duration::from_days(3));             // planned work unchanged
//! assert_eq!(buy.blocked, Duration::from_days(5));
//! assert_eq!(buy.explanation.blocked, Duration::from_days(5));
//! assert!(buy.is_critical);                                     // its 4 days of slack are used up
//! assert_eq!(buy.segments.len(), 3);                            // work, blocked, work
//! // 5 days blocked - 4 days of slack = the shed finishes 1 working day later.
//! assert_eq!(result.project_finish.to_string(), "2027-03-24 17:00:00");
//! ```
//!
//! ## Special cases
//! - **A blocker at the end** (`after_work` ≥ duration) just extends the
//!   task: all work, then the blocked time.
//! - **A milestone** (duration 0) with a blocker: the blocked time comes
//!   first, then the milestone point, which lands at the task's finish.
//! - **Summary tasks** cannot have blockers (the document refuses them); if
//!   a file contains some anyway, the engine ignores them, because summary
//!   dates are rolled up from the subtasks.
//! - **Negative values** (never let in by the document) count as zero.
//!
//! ## Not in MS Project
//! MS Project has no blockers.
//!
//! ## Documentation

use crate::calendar_math::{CalendarError, add_working_time, snap_forward};
use chrono::NaiveDateTime;
use model::{Blocker, BlockerId, Calendar, Duration, Task};
use serde::{Deserialize, Serialize};

/// What one piece of a task's bar is: work or blocked time.
///
/// # Variants
/// - `Work`: the task's own work.
/// - `Blocked { blocker }`: the task is stuck; `blocker` says which
///   [`Blocker`] caused it (so the UI can show its reason).
///
/// # JSON
/// An object with a `"type"` field:
/// ```text
/// { "type": "work" }
/// { "type": "blocked", "blocker": "6f1c…" }
/// ```
///
/// # Example
/// ```
/// use engine::SegmentKind;
/// use model::BlockerId;
/// let piece = SegmentKind::Blocked { blocker: BlockerId::new() };
/// assert_ne!(piece, SegmentKind::Work);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SegmentKind {
    /// The task's own work.
    Work,
    /// Blocked time caused by one blocker.
    Blocked {
        /// The blocker that causes this piece.
        blocker: BlockerId,
    },
}

/// One piece of a task's bar: from `start` to `finish`, work or blocked.
///
/// # Fields
/// - `start`: where the piece begins.
/// - `finish`: where it ends.
/// - `kind`: work or blocked ([`SegmentKind`]).
///
/// # JSON
/// ```text
/// { "start": "2027-03-05T08:00:00", "finish": "2027-03-11T17:00:00",
///   "kind": { "type": "blocked", "blocker": "6f1c…" } }
/// ```
///
/// # Example
/// See [`segments`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Start of the piece.
    pub start: NaiveDateTime,
    /// Finish of the piece.
    pub finish: NaiveDateTime,
    /// Work or blocked.
    pub kind: SegmentKind,
}

/// The working time a task occupies in the schedule: its own duration plus
/// all its blocked time.
///
/// # Parameters
/// - `task`: the task (its `duration` and `blockers` are read).
///
/// # Returns
/// `duration + blocked time`. For a task without blockers this is just its
/// duration, so nothing changes for projects that have no blockers.
///
/// # Example
/// ```
/// use engine::blockers::span;
/// use model::{Blocker, BlockerId, Duration, Task};
/// let mut buy = Task::new("Buy materials", Duration::from_days(3));
/// assert_eq!(span(&buy), Duration::from_days(3));
/// buy.blockers.push(Blocker::new(BlockerId::new(), Duration::from_days(2), Duration::from_days(5), "Late wood"));
/// assert_eq!(span(&buy), Duration::from_days(8)); // 3d work + 5d blocked
/// ```
pub fn span(task: &Task) -> Duration {
    Duration::from_minutes(task.duration.minutes() + task.blocked_time().minutes())
}

/// The blockers of a task in the order they happen: by `after_work`
/// (clamped to `0..=duration`), ties by ID. Blockers of zero length are
/// left out (they do not change anything).
///
/// # Beginner note: `sort_by_key` and tuples
/// `(a, b)` is a *tuple*: two values in one. Rust compares tuples item by
/// item, so sorting by `(position, id)` sorts by position first and uses
/// the ID only to break ties.
fn ordered(task: &Task) -> Vec<(Duration, &Blocker)> {
    let work = task.duration.minutes().max(0);
    let mut list: Vec<(Duration, &Blocker)> = task
        .blockers
        .iter()
        .filter(|b| b.duration.minutes() > 0)
        .map(|b| {
            (
                Duration::from_minutes(b.after_work.minutes().clamp(0, work)),
                b,
            )
        })
        .collect();
    list.sort_by_key(|(at, b)| (*at, b.id));
    list
}

/// Splits a task into its work and blocked pieces, starting at `start`.
///
/// # What it does
/// The walk described in the module notes: work up to each blocker's
/// `after_work`, then that blocker's time, then the rest of the work.
///
/// # Parameters
/// - `task`: the task (duration and blockers).
/// - `calendar`: the task's calendar; all lengths are working time on it.
/// - `start`: the task's scheduled start.
///
/// # Returns
/// The pieces in time order. **Empty** when the task has no blockers (the
/// whole bar is work; the UI needs no pieces then). The last piece ends at
/// `start + span(task)`, which is the task's finish.
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic (in practice only for dates
/// out of range).
///
/// # Example: the worked example of the module notes
/// ```
/// use engine::blockers::segments;
/// use engine::SegmentKind;
/// use model::{Blocker, BlockerId, Calendar, Duration, Task};
/// use chrono::NaiveDate;
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let mut buy = Task::new("Buy materials", Duration::from_days(3));
/// let late_wood = BlockerId::new();
/// buy.blockers.push(Blocker::new(late_wood, Duration::from_days(2), Duration::from_days(5), "Late wood"));
///
/// let pieces = segments(&buy, &Calendar::standard(), at(3, 8)).unwrap();
/// assert_eq!(pieces.len(), 3);
/// assert_eq!((pieces[0].start, pieces[0].finish), (at(3, 8), at(4, 17)));   // work
/// assert_eq!((pieces[1].start, pieces[1].finish), (at(5, 8), at(11, 17)));  // blocked
/// assert_eq!(pieces[1].kind, SegmentKind::Blocked { blocker: late_wood });
/// assert_eq!((pieces[2].start, pieces[2].finish), (at(12, 8), at(12, 17))); // work
/// ```
pub fn segments(
    task: &Task,
    calendar: &Calendar,
    start: NaiveDateTime,
) -> Result<Vec<Segment>, CalendarError> {
    let blockers = ordered(task);
    if blockers.is_empty() {
        return Ok(Vec::new());
    }
    let mut walk = Walk {
        calendar,
        cursor: start,
        pieces: Vec::new(),
    };
    let mut work_done = Duration::ZERO;
    for (at, blocker) in blockers {
        // Work until this blocker's position is reached…
        let work = Duration::from_minutes(at.minutes() - work_done.minutes());
        walk.piece(work, SegmentKind::Work)?;
        work_done = at;
        // …then the blocked time.
        walk.piece(
            blocker.duration,
            SegmentKind::Blocked {
                blocker: blocker.id,
            },
        )?;
    }
    // The rest of the work after the last blocker.
    let rest = Duration::from_minutes(task.duration.minutes().max(0) - work_done.minutes());
    walk.piece(rest, SegmentKind::Work)?;
    Ok(walk.pieces)
}

/// The state of the walk in [`segments`]: where we are and what has been
/// cut so far.
struct Walk<'a> {
    /// The task's calendar.
    calendar: &'a Calendar,
    /// The moment the next piece starts from.
    cursor: NaiveDateTime,
    /// The pieces cut so far.
    pieces: Vec<Segment>,
}

impl Walk<'_> {
    /// Cuts the next piece of `length` working minutes (nothing for a
    /// length of zero) and moves the cursor to its end.
    ///
    /// The first piece starts exactly at the task's start (a manual task may
    /// start on a Saturday). Every later piece starts at the next working
    /// moment: a piece cannot start at Thu 17:00; it starts Fri 08:00.
    fn piece(&mut self, length: Duration, kind: SegmentKind) -> Result<(), CalendarError> {
        if length.minutes() <= 0 {
            return Ok(());
        }
        let start = if self.pieces.is_empty() {
            self.cursor
        } else {
            snap_forward(self.calendar, self.cursor)?
        };
        let finish = add_working_time(self.calendar, start, length)?;
        self.pieces.push(Segment {
            start,
            finish,
            kind,
        });
        self.cursor = finish;
        Ok(())
    }
}
