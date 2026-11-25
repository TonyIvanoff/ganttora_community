//! # Crate `engine`: Ganttora's scheduling engine
//!
//! ## What this crate is for
//! The engine takes a project (tasks, durations, dependencies, a calendar)
//! and calculates **when** each task can happen: its early and late start
//! and finish, its slack, and whether it is on the critical path. It is the
//! core asset of Ganttora.
//!
//! ## Where it fits
//! ```text
//!   model::Project ──► engine::schedule() ──► ScheduleResult
//!   (what the user typed)                     (calculated dates)
//! ```
//! The `document` crate calls [`schedule`] after every edit; the UI shows the
//! result. The engine never stores anything itself.
//!
//! ## Main ideas
//! - **Deterministic**: the same project always gives exactly the same
//!   result. There is no randomness, no clock reading, and no `HashMap`
//!   whose iteration order could change between runs. Ties are broken by
//!   the order of the tasks in the project.
//! - **No I/O**: the engine never reads or writes files, the network or the
//!   screen. It is a *pure function*: input in, result out. That makes it
//!   easy to test and safe to run anywhere.
//! - **The critical path method (CPM)**, exactly as you did it by hand in
//!   the project documentation: sort the tasks (graph), forward pass, backward pass,
//!   slack, critical tasks. See [`cpm`].
//!
//! ## What the engine supports (M1)
//! - All four link types (FS, SS, FF, SF) with a lag or lead in working
//!   time or as a percentage of the predecessor's duration.
//! - Constraints (ASAP, ALAP, SNET, SNLT, FNET, FNLT, MSO, MFO) and
//!   deadlines, with negative slack when rules conflict ([`constraints`]).
//! - A project calendar and task calendars, with working times, holidays and
//!   exceptions ([`calendar_math`]).
//! - Summary tasks, whose dates roll up from their subtasks ([`summary`]).
//! - Manually scheduled tasks ([`manual`]).
//! - Blockers: unplanned delays that make a task longer ([`blockers`],
//!   the project documentation).
//! - Scheduling from the project start or from the project finish.
//! - "Why this date?": every task's dates come with an [`Explanation`]
//!   ([`explain`]).
//!
//! ## Documentation

pub mod blockers;
pub mod calendar_math;
pub mod constraints;
pub mod cpm;
pub mod explain;
pub mod manual;
pub mod summary;

pub use blockers::{Segment, SegmentKind};
pub use explain::{Bound, Driver, Explanation, Limits, TaskEnd};

/// The version of the scheduling engine, e.g. `"0.1.0"`.
///
/// # What it is for
/// The `.gtra` file stores it in `meta.engine_version` when it is saved, so
/// that a later Ganttora can tell which engine calculated the dates in a
/// file (and, for example, warn when a newer engine would give different
/// dates).
///
/// # Where the value comes from
/// `env!("CARGO_PKG_VERSION")` is filled in by the compiler with the
/// `version` from this crate's `Cargo.toml` (the workspace version). Nobody
/// has to remember to update it by hand.
///
/// # Example
/// ```
/// assert!(!engine::ENGINE_VERSION.is_empty());
/// assert_eq!(engine::ENGINE_VERSION.split('.').count(), 3); // "major.minor.patch"
/// ```
///
/// # Beginner note: `env!`
/// `env!` is a *macro* (code that writes code, ending in `!`). It reads an
/// environment variable **while the program is compiled**, not while it
/// runs, so the text is baked into the program.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

use calendar_math::CalendarError;
use chrono::NaiveDateTime;
use model::{CalendarId, DependencyId, DependencyType, Duration, Project, TaskId};
use serde::{Deserialize, Serialize};

/// The calculated dates of one task.
///
/// # Fields
/// - `task_id`: which task these dates belong to.
/// - `start` / `finish`: the task's **scheduled** dates, the ones shown in
///   the grid and drawn in the Gantt chart. Usually the early dates; the
///   late dates for ALAP tasks and when scheduling from the finish; the
///   typed dates for manual tasks; the rolled-up dates for summary tasks.
/// - `duration`: the working duration (for a summary task: rolled up, the
///   working time between its start and finish on the project calendar).
/// - `early_start` / `early_finish`: the earliest the task can start and
///   finish (forward pass).
/// - `late_start` / `late_finish`: the latest it can start and finish
///   without delaying the project finish (backward pass).
/// - `total_slack`: how much the task can slip without delaying the project
///   finish, in working time. Negative when a constraint or deadline cannot
///   be met.
/// - `free_slack`: how much it can slip without delaying *any successor*.
/// - `is_critical`: `true` when total slack is zero or less.
/// - `is_summary`: `true` for a summary task.
/// - `missed_deadline`: `true` when the task finishes after its deadline.
/// - `explanation`: "Why this date?" ([`Explanation`]).
/// - `blocked`: the task's total blocked time ([`blockers`]); zero when it
///   has no blockers. `start`/`finish` (and the early and late dates)
///   already include it; `duration` does not (it stays the planned work).
/// - `segments`: the pieces of the bar, work and blocked, in time order,
///   on the scheduled dates ([`blockers::segments`]). **Empty** for a task
///   without blockers and for summary tasks.
///
/// # JSON
/// Field names in camelCase (`taskId`, `earlyStart`, `totalSlack`…).
///
/// # Example
/// ```
/// use engine::schedule;
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Only task", Duration::from_days(1)));
/// let result = schedule(&project).unwrap();
/// let only = &result.tasks[0];
/// assert!(only.is_critical);
/// assert_eq!(only.early_finish.to_string(), "2027-03-01 17:00:00");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSchedule {
    /// The task these dates belong to.
    pub task_id: TaskId,
    /// Scheduled start (shown in the grid and the Gantt chart).
    pub start: NaiveDateTime,
    /// Scheduled finish.
    pub finish: NaiveDateTime,
    /// Working duration (rolled up for summary tasks).
    pub duration: Duration,
    /// Earliest possible start.
    pub early_start: NaiveDateTime,
    /// Earliest possible finish.
    pub early_finish: NaiveDateTime,
    /// Latest start that does not delay the project.
    pub late_start: NaiveDateTime,
    /// Latest finish that does not delay the project.
    pub late_finish: NaiveDateTime,
    /// Working time the task can slip without delaying the project finish.
    pub total_slack: Duration,
    /// Working time the task can slip without delaying any successor.
    pub free_slack: Duration,
    /// `true` if the task is on the critical path (total slack <= 0).
    pub is_critical: bool,
    /// `true` for a summary task.
    pub is_summary: bool,
    /// `true` when the scheduled finish is after the deadline.
    pub missed_deadline: bool,
    /// Why the task got its dates.
    pub explanation: Explanation,
    /// Total blocked time (zero = no blockers).
    #[serde(default)]
    pub blocked: Duration,
    /// Work and blocked pieces of the bar (empty = no blockers).
    #[serde(default)]
    pub segments: Vec<Segment>,
}

/// Everything the engine calculated for a project.
///
/// # Fields
/// - `project_start`: the first working moment of the project (the project
///   start, moved forward to working time; when scheduling from the finish,
///   the earliest task start).
/// - `project_finish`: the latest finish of all tasks (when scheduling from
///   the finish, the fixed finish moved back to working time); equal to
///   `project_start` when there are no tasks.
/// - `tasks`: one [`TaskSchedule`] per task, **in the same order as
///   `project.tasks`**, so `result.tasks[i]` belongs to `project.tasks[i]`.
///
/// # Example
/// ```
/// use engine::schedule;
/// use model::Project;
/// use chrono::NaiveDate;
/// let project = Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let result = schedule(&project).unwrap();
/// assert!(result.tasks.is_empty());
/// assert_eq!(result.project_finish, result.project_start);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResult {
    /// First working moment of the project.
    pub project_start: NaiveDateTime,
    /// Finish of the whole project (latest early finish).
    pub project_finish: NaiveDateTime,
    /// Calculated dates, in project task order.
    pub tasks: Vec<TaskSchedule>,
}

impl ScheduleResult {
    /// Finds the calculated dates of one task by its ID.
    ///
    /// # Parameters
    /// - `task_id`: the task to look up.
    ///
    /// # Returns
    /// `Some(&TaskSchedule)` if the task is in the result, `None` otherwise.
    ///
    /// # Example
    /// ```
    /// use engine::schedule;
    /// use model::{Duration, Project, Task, TaskId};
    /// use chrono::NaiveDate;
    /// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// let task = Task::new("Dig", Duration::from_days(1));
    /// let id = task.id;
    /// project.tasks.push(task);
    /// let result = schedule(&project).unwrap();
    /// assert!(result.task(id).is_some());
    /// assert!(result.task(TaskId::new()).is_none());
    /// ```
    ///
    /// # Beginner note: `Option`
    /// `Option<T>` is either `Some(value)` or `None` (nothing). Rust uses it
    /// instead of "null", so you can never forget to handle the "not found"
    /// case.
    pub fn task(&self, task_id: TaskId) -> Option<&TaskSchedule> {
        self.tasks.iter().find(|t| t.task_id == task_id)
    }

    /// Lists the critical tasks, in project task order.
    ///
    /// # Returns
    /// The IDs of all tasks whose `is_critical` is `true`.
    ///
    /// # Example
    /// See the `shed` example: `cargo run -p engine --example shed`.
    pub fn critical_tasks(&self) -> Vec<TaskId> {
        cpm::critical::critical_tasks(&self.tasks)
    }
}

/// Everything that can stop the engine from producing a schedule.
///
/// # Variants
/// - `CalendarNotFound`: `project.calendar_id` names no calendar in
///   `project.calendars`.
/// - `Calendar`: the calendar is unusable (see [`CalendarError`]).
/// - `DuplicateTaskId`: two tasks share one ID.
/// - `NegativeDuration`: a task has a duration below zero.
/// - `TaskCalendarNotFound`: a task names a calendar that does not exist.
/// - `UnknownTask`: a dependency points at a task that does not exist.
/// - `UnsupportedDependencyType`: **no longer returned since M1** (all four
///   link types are supported). Kept only so older code that matches on it
///   still compiles; it will be removed.
/// - `Cycle`: the dependencies go round in a circle; lists the tasks. A link
///   between a summary task and one of its own subtasks is a circle too.
/// - `ProjectFinishMissing`: the project is scheduled from its finish, but
///   has no finish date.
///
/// # JSON (what the UI receives)
/// A stable error **code** plus details, never an English sentence; the UI
/// translates the code (no hard-coded UI text):
/// ```text
/// { "code": "cycleDetected", "details": { "tasks": ["…", "…"], "names": ["A", "B"] } }
/// { "code": "projectFinishMissing" }
/// ```
/// The English `Display` text (`to_string()`) is for logs and developers.
///
/// # Example
/// ```
/// use engine::{schedule, ScheduleError};
/// use model::{Dependency, Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut project = Project::new("Loop", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let a = Task::new("A", Duration::from_days(1));
/// let b = Task::new("B", Duration::from_days(1));
/// project.dependencies.push(Dependency::finish_to_start(a.id, b.id));
/// project.dependencies.push(Dependency::finish_to_start(b.id, a.id)); // circle!
/// project.tasks.push(a);
/// project.tasks.push(b);
/// let err = schedule(&project).unwrap_err();
/// assert_eq!(err.to_string(), "circular dependency: A -> B -> A");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
// `tag = "code", content = "details"`: the variant name goes into "code",
// its fields into "details".
#[serde(
    tag = "code",
    content = "details",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScheduleError {
    /// The project calendar ID does not match any calendar.
    #[error("project calendar {0} not found")]
    CalendarNotFound(CalendarId),
    /// A task names a calendar that is not in the project.
    #[error("task {task} uses calendar {calendar}, which does not exist")]
    TaskCalendarNotFound {
        /// The task.
        task: TaskId,
        /// The missing calendar.
        calendar: CalendarId,
    },
    /// The project calendar is unusable.
    ///
    /// `#[from]` lets the `?` operator turn a `CalendarError` into this
    /// variant automatically.
    #[error(transparent)]
    Calendar(#[from] CalendarError),
    /// Two tasks have the same ID.
    #[error("two tasks share the ID {0}")]
    DuplicateTaskId(TaskId),
    /// A task has a negative duration.
    #[error("task \"{name}\" has a negative duration")]
    NegativeDuration {
        /// The task's ID.
        task: TaskId,
        /// The task's name, for the message.
        name: String,
    },
    /// A dependency refers to a task that is not in the project.
    #[error("dependency {dependency} refers to unknown task {task}")]
    UnknownTask {
        /// The broken dependency.
        dependency: DependencyId,
        /// The missing task ID.
        task: TaskId,
    },
    /// Not returned since M1 (all link types are supported); kept so that
    /// existing code matching on it still compiles.
    #[error("dependency {dependency} has type {kind:?}, which is not supported yet")]
    UnsupportedDependencyType {
        /// The dependency.
        dependency: DependencyId,
        /// Its type.
        kind: DependencyType,
    },
    /// The dependencies form a circle.
    ///
    /// `tasks` and `names` list the tasks around the circle in link order,
    /// starting with the one that comes first in the project.
    #[error("circular dependency: {}", cycle_text(names))]
    #[serde(rename = "cycleDetected")]
    Cycle {
        /// IDs of the tasks in the circle.
        tasks: Vec<TaskId>,
        /// Names of the same tasks, in the same order.
        names: Vec<String>,
    },
    /// Scheduling from the finish needs `project.finish`.
    #[error("the project is scheduled from its finish but has no finish date")]
    ProjectFinishMissing,
}

/// Builds the text "A -> B -> C -> A" for a cycle error message: the names
/// joined by arrows, with the first name repeated at the end to show that the
/// chain closes.
fn cycle_text(names: &[String]) -> String {
    let mut parts: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Some(first) = names.first() {
        parts.push(first);
    }
    parts.join(" -> ")
}

/// Calculates the schedule of a project.
///
/// # What it does
/// 1. Checks that no task has a negative duration.
/// 2. Prepares the [`cpm::Context`]: builds the outline and the dependency
///    graph (duplicate IDs, unknown tasks and cycles are found here), then
///    finds and checks the project calendar and the task calendars. The
///    calendars are checked only there, once.
/// 3. Runs the critical path method ([`cpm::run`]): forward and backward
///    passes, ALAP/ASAP moves, summary roll-up, slack, critical flags,
///    explanations.
///
/// # Parameters
/// - `project`: the project to schedule. It is only read, never changed.
///
/// # Returns
/// A [`ScheduleResult`] with one entry per task, in project task order.
///
/// # Errors
/// Any [`ScheduleError`]: missing or invalid calendar, duplicate task IDs,
/// negative durations, links to unknown tasks, a circular dependency, or a
/// missing finish date when scheduling from the finish.
///
/// # Example
/// ```
/// use engine::schedule;
/// use model::{Dependency, Duration, Project, Task};
/// use chrono::NaiveDate;
///
/// // Monday 1 March 2027: "Design" (2 days) then "Permit" (5 days).
/// let mut project = Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let design = Task::new("Design the shed", Duration::from_days(2));
/// let permit = Task::new("Get building permit", Duration::from_days(5));
/// project.dependencies.push(Dependency::finish_to_start(design.id, permit.id));
/// project.tasks.push(design);
/// project.tasks.push(permit);
///
/// let result = schedule(&project).unwrap();
/// // Design: Mon 1 Mar 08:00 - Tue 2 Mar 17:00.
/// // Permit starts at the next working moment, Wed 3 Mar 08:00,
/// // and finishes after 5 working days on Tue 9 Mar 17:00.
/// assert_eq!(result.tasks[1].early_start.to_string(), "2027-03-03 08:00:00");
/// assert_eq!(result.project_finish.to_string(), "2027-03-09 17:00:00");
/// ```
pub fn schedule(project: &Project) -> Result<ScheduleResult, ScheduleError> {
    // Step 1: no negative durations. (The project calendar is NOT looked
    // up here: `cpm::Context::new` does that, after it has checked for
    // dependency loops. Checking it here as well would do the work twice,
    // and a calendar error would hide a loop, which the document must always
    // be able to see.)
    for task in &project.tasks {
        if task.duration.minutes() < 0 {
            return Err(ScheduleError::NegativeDuration {
                task: task.id,
                name: task.name.clone(),
            });
        }
    }

    // Step 2: calendars, outline and graph.
    let context = cpm::Context::new(project)?;

    // Step 3: the critical path method.
    cpm::run(&context)
}
