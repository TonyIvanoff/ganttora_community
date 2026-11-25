//! # Tasks: the pieces of work in a project
//!
//! ## What this file is for
//! Defines [`Task`], one row of the schedule, and the small types that
//! describe how a task is scheduled: [`TaskMode`] (automatic or manual) and
//! [`Constraint`] (ASAP, ALAP, "start no earlier than" and the others).
//!
//! ## Where it fits
//! Part of the `model` crate. A [`Project`](crate::Project) holds its tasks in
//! a list; the order of that list is the order of the rows the user sees. The
//! engine reads each task's duration, mode, constraint, calendar and deadline
//! and calculates its dates.
//!
//! ## Main ideas
//! - A task stores what the *user* decides (name, duration, constraint…).
//!   Calculated values (start, finish, slack) are **not** stored here; they
//!   are produced by the engine in its own result type. That keeps one clear
//!   owner for each value.
//! - **The outline (WBS).** Tasks are kept in one flat list in row order,
//!   and each task has an `outline_level` (1 = top level, 2 = indented once…).
//!   A task followed by a more indented task is a **summary task**: its dates
//!   are rolled up from the tasks below it. This is exactly how MS Project
//!   stores its outline. The parent of a task and its position among its
//!   siblings are *derived* from the list (see [`crate::outline`]), so there
//!   is only one source of truth.
//!
//!   ```text
//!   row  level  name              parent          position among siblings
//!   0    1      Preparation       (none)          0
//!   1    2        Design          Preparation     0
//!   2    2        Permit          Preparation     1
//!   3    1      Build             (none)          1
//!   ```
//! - A task with a zero duration is a **milestone**, a moment in time such
//!   as "Shed finished". A task can also be *marked* as a milestone with the
//!   `milestone` flag while keeping a duration, as in MS Project.
//! - **Manual scheduling**: a manually scheduled task stays where the user
//!   put it (`manual_start`); links and constraints do not move it.
//! - **Blockers**: unplanned delays recorded on the task
//!   ([`Blocker`]). They make the task longer than its planned `duration`
//!   without changing that duration.
//!
//! ## Documentation

use crate::blocker::Blocker;
use crate::duration::Duration;
use crate::ids::{CalendarId, TaskId};
use crate::risk::Risk;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// How a task's dates are decided.
///
/// # Variants
/// - `Auto`: the engine calculates the dates from links, constraints and
///   calendars (the default, called "Auto Scheduled" in MS Project).
/// - `Manual`: the task stays at the start the user gave it
///   (`Task::manual_start`). Links and constraints do not move it, although
///   it still pushes its successors.
///
/// # JSON
/// A plain string: `"auto"` or `"manual"`.
///
/// # Example
/// ```
/// use model::TaskMode;
/// assert_eq!(TaskMode::default(), TaskMode::Auto);
/// assert_eq!(serde_json::to_string(&TaskMode::Manual).unwrap(), "\"manual\"");
/// ```
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum TaskMode {
    /// Dates are calculated by the engine.
    #[default]
    Auto,
    /// Dates are typed by the user and not moved by the engine.
    Manual,
}

/// A date rule on a task: "start no earlier than 8 March", "must finish on
/// 23 March" and so on. The eight types are MS Project's constraint types.
///
/// # Variants
/// | Variant | Short name | Meaning |
/// |---|---|---|
/// | `AsSoonAsPossible` | ASAP | No date rule; start as early as the links allow (default) |
/// | `AsLateAsPossible` | ALAP | Start as late as possible without delaying the next tasks |
/// | `StartNoEarlierThan` | SNET | Start on or after `date` |
/// | `StartNoLaterThan` | SNLT | Start on or before `date` |
/// | `FinishNoEarlierThan` | FNET | Finish on or after `date` |
/// | `FinishNoLaterThan` | FNLT | Finish on or before `date` |
/// | `MustStartOn` | MSO | Start exactly on `date` |
/// | `MustFinishOn` | MFO | Finish exactly on `date` |
///
/// The variants with a date *carry* the date, so a "SNET without a date"
/// cannot exist.
///
/// # Worked example
/// The shed's "Build the floor" has a predecessor that finishes Thu 11 Mar
/// 2027 17:00. With **SNET Mon 15 Mar 08:00** it starts Mon 15 Mar instead of
/// Fri 12 Mar: the later of the two rules wins.
///
/// # JSON
/// An object with a `"type"` field; the dated ones also have `"date"`:
/// ```text
/// { "type": "asSoonAsPossible" }
/// { "type": "startNoEarlierThan", "date": "2027-03-15T08:00:00" }
/// ```
///
/// # Example
/// ```
/// use model::Constraint;
/// use chrono::NaiveDate;
/// let date = NaiveDate::from_ymd_opt(2027, 3, 15).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// let snet = Constraint::StartNoEarlierThan { date };
/// assert_eq!(snet.date(), Some(date));
/// assert_eq!(Constraint::default().date(), None);
/// ```
///
/// # Beginner note: enum variants with data
/// Some variants here carry a value (`{ date }`) and some do not. Rust lets
/// each variant of an `enum` have its own fields.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Constraint {
    /// ASAP: no date rule (the default).
    #[default]
    AsSoonAsPossible,
    /// ALAP: as late as possible without delaying the following tasks.
    AsLateAsPossible,
    /// SNET: start on or after `date`.
    StartNoEarlierThan {
        /// The constraint date.
        date: NaiveDateTime,
    },
    /// SNLT: start on or before `date`.
    StartNoLaterThan {
        /// The constraint date.
        date: NaiveDateTime,
    },
    /// FNET: finish on or after `date`.
    FinishNoEarlierThan {
        /// The constraint date.
        date: NaiveDateTime,
    },
    /// FNLT: finish on or before `date`.
    FinishNoLaterThan {
        /// The constraint date.
        date: NaiveDateTime,
    },
    /// MSO: start exactly on `date`.
    MustStartOn {
        /// The constraint date.
        date: NaiveDateTime,
    },
    /// MFO: finish exactly on `date`.
    MustFinishOn {
        /// The constraint date.
        date: NaiveDateTime,
    },
}

impl Constraint {
    /// Returns the constraint date, or `None` for ASAP and ALAP.
    ///
    /// # Example
    /// ```
    /// use model::Constraint;
    /// assert_eq!(Constraint::AsLateAsPossible.date(), None);
    /// ```
    ///
    /// # Beginner note: `|` in a pattern
    /// `A { date } | B { date }` means "either A or B"; both give a `date`.
    pub fn date(&self) -> Option<NaiveDateTime> {
        match self {
            Constraint::AsSoonAsPossible | Constraint::AsLateAsPossible => None,
            Constraint::StartNoEarlierThan { date }
            | Constraint::StartNoLaterThan { date }
            | Constraint::FinishNoEarlierThan { date }
            | Constraint::FinishNoLaterThan { date }
            | Constraint::MustStartOn { date }
            | Constraint::MustFinishOn { date } => Some(*date),
        }
    }
}

/// One task (one row) of a project.
///
/// # Fields
/// - `id`: unique ID, used by dependencies to point at this task.
/// - `name`: what the task is called, e.g. "Design the shed".
/// - `duration`: working time needed; zero for a milestone. Must not be
///   negative (the engine rejects negative durations). Ignored for summary
///   tasks, whose duration is rolled up from their subtasks.
/// - `outline_level`: indentation level in the outline, 1 = top level.
/// - `mode`: automatic or manual scheduling.
/// - `manual_start`: where a manually scheduled task starts. `None` for
///   automatic tasks (and ignored by the engine for them).
/// - `milestone`: shows the task as a milestone (a diamond) even when its
///   duration is not zero. A zero-duration task is always a milestone.
/// - `constraint`: the date rule (ASAP by default).
/// - `deadline`: a target finish. It does not move the task, but a task that
///   finishes after its deadline gets less (or negative) slack.
/// - `calendar_id`: a task calendar; `None` means "use the project calendar".
/// - `notes`: free text notes.
/// - `hyperlink`: a web address or file path attached to the task; empty
///   means none.
/// - `blockers`: unplanned delays on this task ([`Blocker`]), in no
///   particular order (the engine sorts them). Summary tasks never have
///   blockers (the document refuses them).
///
/// # JSON
/// Field names in camelCase (`outlineLevel`, `manualStart`, `calendarId`).
/// Fields added in M1 have default values, so JSON written by M0 (with only
/// `id`, `name`, `duration`, `outlineLevel`) still reads correctly.
/// `blockers` is left out of the JSON when the list is empty, so a task
/// without blockers looks exactly as it did before blockers existed.
///
/// # Example
/// ```
/// use model::{Duration, Task};
/// let design = Task::new("Design the shed", Duration::from_days(2));
/// assert_eq!(design.duration.minutes(), 960);
/// assert_eq!(design.outline_level, 1);
/// assert!(design.notes.is_empty());
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique ID of the task.
    pub id: TaskId,
    /// Task name shown in the grid.
    pub name: String,
    /// Working time the task needs. Zero means milestone.
    pub duration: Duration,
    /// Outline (indent) level; 1 is the top level.
    pub outline_level: u32,
    /// Automatic or manual scheduling.
    // `#[serde(default)]`: if the field is missing in JSON, use the type's
    // default value (here `TaskMode::Auto`) instead of failing.
    #[serde(default)]
    pub mode: TaskMode,
    /// Start of a manually scheduled task (ignored for automatic tasks).
    #[serde(default)]
    pub manual_start: Option<NaiveDateTime>,
    /// Show as a milestone even with a non-zero duration.
    #[serde(default)]
    pub milestone: bool,
    /// The date rule of the task (ASAP by default).
    #[serde(default)]
    pub constraint: Constraint,
    /// Target finish; missing it gives negative slack, but does not move the task.
    #[serde(default)]
    pub deadline: Option<NaiveDateTime>,
    /// Task calendar; `None` = the project calendar.
    #[serde(default)]
    pub calendar_id: Option<CalendarId>,
    /// Free text notes.
    #[serde(default)]
    pub notes: String,
    /// A web address or file path; empty = none.
    #[serde(default)]
    pub hyperlink: String,
    /// Unplanned delays on this task.
    // `skip_serializing_if`: an empty list is not written to JSON at all.
    // `Vec::is_empty` is the small function serde calls to decide that.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<Blocker>,
    /// How much of the task is done, in percent (0 to 100). Ignored on
    /// summary tasks: theirs is calculated from their subtasks.
    #[serde(default, skip_serializing_if = "is_default")]
    pub percent_complete: u8,
    /// This row is a *project*: a top-level row that holds tasks (the project documentation
    /// D25). One file can hold several, on the same timeline.
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_project: bool,
    /// What could go wrong, in the order it was added. Unlike a blocker, a
    /// risk has not happened, so it never moves dates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<Risk>,
    /// The finish saved by "Set plan". It stays when the
    /// schedule moves, so the two can be compared. Never moves dates.
    #[serde(default, skip_serializing_if = "is_default")]
    pub planned_finish: Option<NaiveDateTime>,
    /// When the work really finished; `None` while it is not 100 % done.
    /// Never moves dates.
    #[serde(default, skip_serializing_if = "is_default")]
    pub actual_finish: Option<NaiveDateTime>,
}

/// `true` if a value is its type's default (0, `false`…). serde calls it to
/// leave such fields out of the JSON.
///
/// # Beginner note
/// `T: Default + PartialEq` means: any type that has a default value and can
/// be compared.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl Task {
    /// Creates a top-level, automatically scheduled ASAP task with a new
    /// random ID.
    ///
    /// # Parameters
    /// - `name`: the task name.
    /// - `duration`: the working time it needs (`Duration::ZERO` for a
    ///   milestone).
    ///
    /// # Returns
    /// The new task, at outline level 1, with no notes, no deadline and no
    /// task calendar.
    ///
    /// # Example
    /// ```
    /// use model::{Duration, Task};
    /// let done = Task::new("Shed finished", Duration::ZERO);
    /// assert!(done.duration.is_zero()); // a milestone
    /// ```
    pub fn new(name: impl Into<String>, duration: Duration) -> Self {
        Task::with_id(TaskId::new(), name, duration)
    }

    /// Like [`Task::new`], but with a given ID instead of a random one. Used
    /// when a task is read back from a file, and in tests that need
    /// repeatable IDs.
    ///
    /// # Parameters
    /// - `id`: the task's ID.
    /// - `name`: the task name.
    /// - `duration`: the working time it needs.
    ///
    /// # Returns
    /// The new task with default values for every other field.
    ///
    /// # Example
    /// ```
    /// use model::{Duration, Task, TaskId};
    /// use uuid::Uuid;
    /// let id = TaskId::from_uuid(Uuid::from_u128(7));
    /// let task = Task::with_id(id, "Paint", Duration::from_days(2));
    /// assert_eq!(task.id, id);
    /// ```
    pub fn with_id(id: TaskId, name: impl Into<String>, duration: Duration) -> Self {
        Task {
            id,
            name: name.into(),
            duration,
            outline_level: 1,
            mode: TaskMode::Auto,
            manual_start: None,
            milestone: false,
            constraint: Constraint::AsSoonAsPossible,
            deadline: None,
            calendar_id: None,
            notes: String::new(),
            hyperlink: String::new(),
            blockers: Vec::new(),
            percent_complete: 0,
            is_project: false,
            risks: Vec::new(),
            planned_finish: None,
            actual_finish: None,
        }
    }

    /// Returns the same task at another outline level. Handy for building
    /// outlines in examples: `Task::new("Design", d).at_level(2)`.
    ///
    /// # Parameters
    /// - `level`: the outline level, 1 = top level.
    ///
    /// # Example
    /// ```
    /// use model::{Duration, Task};
    /// assert_eq!(Task::new("Design", Duration::from_days(2)).at_level(2).outline_level, 2);
    /// ```
    pub fn at_level(mut self, level: u32) -> Self {
        self.outline_level = level;
        self
    }

    /// Makes a project row: a top-level row of zero duration that will hold
    /// tasks.
    ///
    /// # Parameters
    /// - `name`: the project's name.
    ///
    /// # Example
    /// ```
    /// let row = model::Task::project_row("House");
    /// assert!(row.is_project && row.outline_level == 1);
    /// ```
    pub fn project_row(name: impl Into<String>) -> Self {
        Task {
            is_project: true,
            ..Task::new(name, Duration::ZERO)
        }
    }

    /// Returns the same task with another constraint.
    ///
    /// # Parameters
    /// - `constraint`: the new date rule.
    ///
    /// # Example
    /// ```
    /// use model::{Constraint, Duration, Task};
    /// let t = Task::new("Paint", Duration::from_days(2)).with_constraint(Constraint::AsLateAsPossible);
    /// assert_eq!(t.constraint, Constraint::AsLateAsPossible);
    /// ```
    pub fn with_constraint(mut self, constraint: Constraint) -> Self {
        self.constraint = constraint;
        self
    }

    /// `true` when the task is shown as a milestone: either its duration is
    /// zero or it is marked with the `milestone` flag.
    ///
    /// # Example
    /// ```
    /// use model::{Duration, Task};
    /// assert!(Task::new("Done", Duration::ZERO).is_milestone());
    /// assert!(!Task::new("Paint", Duration::from_days(2)).is_milestone());
    /// ```
    pub fn is_milestone(&self) -> bool {
        self.milestone || self.duration.is_zero()
    }

    /// The total blocked time of the task: the sum of its blockers'
    /// durations. Negative durations (which the document never lets in)
    /// count as zero.
    ///
    /// # Returns
    /// The blocked working time; zero when the task has no blockers.
    ///
    /// # Example
    /// ```
    /// use model::{Blocker, BlockerId, Duration, Task};
    /// let mut buy = Task::new("Buy materials", Duration::from_days(3));
    /// buy.blockers.push(Blocker::new(BlockerId::new(), Duration::from_days(2), Duration::from_days(5), "Late wood"));
    /// buy.blockers.push(Blocker::new(BlockerId::new(), Duration::ZERO, Duration::from_days(1), "Rain"));
    /// assert_eq!(buy.blocked_time(), Duration::from_days(6));
    /// ```
    pub fn blocked_time(&self) -> Duration {
        let minutes: i64 = self
            .blockers
            .iter()
            .map(|b| b.duration.minutes().max(0))
            .sum();
        Duration::from_minutes(minutes)
    }
}
