//! # "Why this date?": what decided each task's dates
//!
//! ## What this file is for
//! Every scheduled date has a reason: the task waits for a predecessor, it
//! has a constraint, it is the first task and starts with the project, the
//! calendar pushed it past a weekend… This file defines the types that
//! record that reason ([`Driver`], [`Explanation`]) and the small "keep the
//! later / keep the earlier" helpers ([`Bound`]) the passes use, so that the
//! winning reason is remembered together with the winning date.
//!
//! ## Where it fits
//! The forward and backward passes (`cpm::forward`, `cpm::backward`) and the
//! constraint, manual and summary rules fill one [`Explanation`] per task.
//! It ends up in [`TaskSchedule::explanation`](crate::TaskSchedule), and the
//! UI shows it in the "Why this date?" panel.
//!
//! ## Main ideas
//! - A date is the **latest of several lower limits** (forward) or the
//!   **earliest of several upper limits** (backward). Each limit is a
//!   [`Bound`]: a moment plus the [`Driver`] that produced it. Taking the
//!   maximum of bounds keeps the driver of the winner.
//! - **Ties**: when two limits give the same moment, a *specific* reason (a
//!   predecessor, a constraint) replaces the *generic* project start or
//!   finish ("starts with the project" is less useful than "waits for
//!   Design" when both are true). Between two links, the one with the
//!   smaller link UUID wins, so the answer does not depend on the order in
//!   which links happen to be stored. Otherwise the first one keeps its
//!   place.
//! - After the winning moment is found, the **calendar** may still move it
//!   (a start at Friday 17:00 becomes Monday 08:00). That is recorded in
//!   [`Explanation::moved_by_calendar`].
//!
//! ## Worked example: the shed's "Build the floor"
//! ```text
//! lower limits for its start:
//!   project start             Mon 1 Mar 08:00
//!   3 Buy materials FS        Fri 5 Mar 17:00
//!   4 Prepare the ground FS   Thu 11 Mar 17:00   <- the latest: the driver
//! the calendar moves Thu 17:00 to Fri 12 Mar 08:00 (moved_by_calendar = true)
//! explanation: driver = predecessor "Prepare the ground" (FS),
//!              driven_date = Thu 11 Mar 17:00, start = Fri 12 Mar 08:00
//! ```
//!
//! ## Documentation

use chrono::NaiveDateTime;
use model::{CalendarId, Constraint, DependencyId, DependencyType, Duration, TaskId};
use serde::{Deserialize, Serialize};

/// The reason behind a date.
///
/// # Variants
/// | Variant | Meaning |
/// |---|---|
/// | `ProjectStart` | Nothing holds the task back; it starts with the project |
/// | `ProjectFinish` | Nothing holds it forward; it finishes with the project (ALAP, or scheduling from the finish) |
/// | `Predecessor` | A link from an earlier task decides the date |
/// | `Successor` | A link to a later task decides the date (ALAP, or scheduling from the finish) |
/// | `Constraint` | The task's own constraint (SNET, MSO, …) decides the date |
/// | `Deadline` | The deadline decides the date (only when scheduling backward) |
/// | `ManualStart` | A manually scheduled task: the user typed the start |
/// | `Subtasks` | A summary task: its dates are rolled up from its subtasks |
///
/// # JSON
/// An object with a `"type"` field, e.g.
/// `{"type": "predecessor", "dependency": "…", "task": "…", "kind": "finishToStart"}`.
///
/// # Example
/// ```
/// use engine::Driver;
/// let reason = Driver::ProjectStart;
/// assert!(reason.is_generic());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Driver {
    /// The project start (no other limit is later).
    ProjectStart,
    /// The project finish (no other limit is earlier).
    ProjectFinish,
    /// A link from a predecessor.
    Predecessor {
        /// The link.
        dependency: DependencyId,
        /// The predecessor task.
        task: TaskId,
        /// The link type.
        kind: DependencyType,
    },
    /// A link to a successor.
    Successor {
        /// The link.
        dependency: DependencyId,
        /// The successor task.
        task: TaskId,
        /// The link type.
        kind: DependencyType,
    },
    /// The task's (or its summary task's) constraint.
    Constraint {
        /// The constraint, with its date.
        constraint: Constraint,
    },
    /// The deadline.
    Deadline {
        /// The deadline date.
        date: NaiveDateTime,
    },
    /// A manually scheduled task's typed start.
    ManualStart,
    /// A summary task's dates, rolled up from its subtasks.
    Subtasks,
}

impl Driver {
    /// `true` for the generic reasons (project start or finish), which a
    /// specific reason replaces when both give the same moment.
    pub fn is_generic(&self) -> bool {
        matches!(self, Driver::ProjectStart | Driver::ProjectFinish)
    }

    /// The link of a `Predecessor` or `Successor` reason, if any.
    fn link(&self) -> Option<DependencyId> {
        match self {
            Driver::Predecessor { dependency, .. } | Driver::Successor { dependency, .. } => {
                Some(*dependency)
            }
            _ => None,
        }
    }

    /// Decides a tie: should `self` replace `current` when both ask for
    /// exactly the same moment? (Rules in the module notes.)
    ///
    /// # Example
    /// ```
    /// use engine::Driver;
    /// use model::Constraint;
    /// let constraint = Driver::Constraint { constraint: Constraint::AsSoonAsPossible };
    /// assert!(constraint.wins_tie_over(&Driver::ProjectStart));
    /// assert!(!Driver::ProjectStart.wins_tie_over(&constraint));
    /// ```
    pub fn wins_tie_over(&self, current: &Driver) -> bool {
        if current.is_generic() {
            return !self.is_generic();
        }
        // `if let (Some(a), Some(b)) = …` matches only when both are links.
        if let (Some(mine), Some(theirs)) = (self.link(), current.link()) {
            return mine < theirs;
        }
        false
    }
}

/// Which end of the task an [`Explanation`] is about.
///
/// # Variants
/// - `Start`: the start was decided first and the finish follows from the
///   duration (forward scheduling, the usual case).
/// - `Finish`: the finish was decided first and the start follows (ALAP
///   tasks, scheduling from the project finish, FF/SF links).
///
/// # JSON
/// `"start"` or `"finish"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskEnd {
    /// The start was decided first.
    Start,
    /// The finish was decided first.
    Finish,
}

/// The full "Why this date?" answer for one task.
///
/// # Fields
/// - `driver`: the winning reason.
/// - `end`: whether the reason fixed the start or the finish.
/// - `driven_date`: the moment the reason asked for, *before* the calendar
///   moved it to working time.
/// - `moved_by_calendar`: `true` when the calendar moved that moment (for
///   example from Friday 17:00 to Monday 08:00, or past a holiday).
/// - `calendar_id`: the calendar used for the task (task calendar or project
///   calendar).
/// - `via_summary`: when the reason belongs to a summary task above this
///   task (a link to "Phase 2" holds back every task inside it), the ID of
///   that summary task; otherwise `None`.
/// - `blocked`: the total blocked time of this task's own blockers
///   which pushed its finish later than its work alone
///   would. Zero for tasks without blockers and for summary tasks. The
///   "Why this date?" panel can say "extended by blockers: 5 days".
///   Example: "Buy materials" (3d) with a 5-day blocker finishes Fri 12 Mar
///   instead of Fri 5 Mar; `blocked` = 2400 minutes.
///
/// # JSON
/// ```text
/// { "driver": { "type": "predecessor", … }, "end": "start",
///   "drivenDate": "2027-03-11T17:00:00", "movedByCalendar": true,
///   "calendarId": "…", "viaSummary": null, "blocked": { "minutes": 0 } }
/// ```
///
/// # Example
/// ```
/// use engine::schedule;
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Dig", Duration::from_days(1)));
/// let result = schedule(&project).unwrap();
/// let why = &result.tasks[0].explanation;
/// assert_eq!(why.driver, engine::Driver::ProjectStart);
/// // The project start (Mon 00:00) was already moved to 08:00 before the
/// // passes ran, so the task itself was not moved by the calendar.
/// assert_eq!(why.driven_date.to_string(), "2027-03-01 08:00:00");
/// assert!(!why.moved_by_calendar);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    /// The winning reason.
    pub driver: Driver,
    /// Which end of the task the reason fixed.
    pub end: TaskEnd,
    /// The moment the reason asked for, before calendar adjustment.
    pub driven_date: NaiveDateTime,
    /// `true` if the calendar moved `driven_date` to another moment.
    pub moved_by_calendar: bool,
    /// The calendar the task is scheduled on.
    pub calendar_id: CalendarId,
    /// The summary task whose link or constraint this is, if any.
    pub via_summary: Option<TaskId>,
    /// Blocked time that extended this task's finish (zero = none).
    // `default`: JSON written before blockers existed still reads (as zero).
    #[serde(default)]
    pub blocked: Duration,
}

/// A limit on a date together with its reason: "not before Thu 11 Mar
/// 17:00, because of predecessor 4".
///
/// # Fields
/// - `at`: the moment.
/// - `driver`: the reason.
/// - `via_summary`: the summary task the limit was inherited from, if any.
///
/// # Example
/// ```
/// use engine::{Bound, Driver};
/// use chrono::NaiveDate;
/// let at = |h| NaiveDate::from_ymd_opt(2027, 3, 1).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let mut best = Bound::new(at(8), Driver::ProjectStart);
/// best.keep_later(Bound::new(at(13), Driver::ManualStart));
/// assert_eq!(best.at, at(13));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bound {
    /// The moment.
    pub at: NaiveDateTime,
    /// Why this moment.
    pub driver: Driver,
    /// The summary task this limit comes from, if inherited.
    pub via_summary: Option<TaskId>,
}

impl Bound {
    /// Creates a bound that is not inherited from a summary task.
    ///
    /// # Parameters
    /// - `at`: the moment.
    /// - `driver`: the reason.
    pub fn new(at: NaiveDateTime, driver: Driver) -> Self {
        Bound {
            at,
            driver,
            via_summary: None,
        }
    }

    /// Replaces `self` with `other` when `other` is **later** (used for
    /// lower limits: the latest one wins). On a tie, a specific reason
    /// replaces a generic one (see the module notes).
    ///
    /// # Parameters
    /// - `other`: the candidate limit.
    pub fn keep_later(&mut self, other: Bound) {
        if other.at > self.at || (other.at == self.at && other.driver.wins_tie_over(&self.driver)) {
            *self = other;
        }
    }

    /// Replaces `self` with `other` when `other` is **earlier** (used for
    /// upper limits: the earliest one wins). Ties as in [`Bound::keep_later`].
    ///
    /// # Parameters
    /// - `other`: the candidate limit.
    pub fn keep_earlier(&mut self, other: Bound) {
        if other.at < self.at || (other.at == self.at && other.driver.wins_tie_over(&self.driver)) {
            *self = other;
        }
    }

    /// Replaces `self` with `other` unconditionally (used for "must start
    /// on" / "must finish on", which win over every link).
    ///
    /// # Parameters
    /// - `other`: the new limit.
    pub fn force(&mut self, other: Bound) {
        *self = other;
    }

    /// Builds the [`Explanation`] for a date decided by this bound.
    ///
    /// # Parameters
    /// - `end`: which end of the task the bound fixed.
    /// - `placed`: where that end really landed after calendar adjustment.
    /// - `calendar_id`: the task's calendar.
    ///
    /// # Returns
    /// The explanation; `moved_by_calendar` is `placed != self.at`;
    /// `blocked` is zero (the scheduler fills it in at the end, see
    /// `cpm::run`).
    pub fn explain(
        &self,
        end: TaskEnd,
        placed: NaiveDateTime,
        calendar_id: CalendarId,
    ) -> Explanation {
        Explanation {
            driver: self.driver,
            end,
            driven_date: self.at,
            moved_by_calendar: placed != self.at,
            calendar_id,
            via_summary: self.via_summary,
            blocked: Duration::ZERO,
        }
    }
}

/// The two optional limits a summary task passes down to its subtasks:
/// a limit on their starts and a limit on their finishes.
///
/// In the forward pass these are *lower* limits ("no subtask of Phase 2 may
/// start before Phase 1 finishes"); in the backward pass they are *upper*
/// limits ("no subtask of Phase 1 may finish after Phase 2 must start").
///
/// # Fields
/// - `start`: the limit on starts, if any.
/// - `finish`: the limit on finishes, if any.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Limits {
    /// Limit on the start, if any.
    pub start: Option<Bound>,
    /// Limit on the finish, if any.
    pub finish: Option<Bound>,
}

impl Limits {
    /// Adds a lower limit on the start (keeps the later one).
    pub fn later_start(&mut self, bound: Bound) {
        merge(&mut self.start, bound, Bound::keep_later);
    }

    /// Adds a lower limit on the finish (keeps the later one).
    pub fn later_finish(&mut self, bound: Bound) {
        merge(&mut self.finish, bound, Bound::keep_later);
    }

    /// Adds an upper limit on the start (keeps the earlier one).
    pub fn earlier_start(&mut self, bound: Bound) {
        merge(&mut self.start, bound, Bound::keep_earlier);
    }

    /// Adds an upper limit on the finish (keeps the earlier one).
    pub fn earlier_finish(&mut self, bound: Bound) {
        merge(&mut self.finish, bound, Bound::keep_earlier);
    }
}

/// Puts `bound` into an empty slot, or combines it with the bound already
/// there using `keep` (`keep_later` or `keep_earlier`).
///
/// # Beginner note: functions as parameters
/// `keep: fn(&mut Bound, Bound)` is a parameter whose value is a
/// *function*. The caller passes `Bound::keep_later` or
/// `Bound::keep_earlier`, so one helper serves both directions.
fn merge(slot: &mut Option<Bound>, bound: Bound, keep: fn(&mut Bound, Bound)) {
    match slot {
        Some(existing) => keep(existing, bound),
        None => *slot = Some(bound),
    }
}
