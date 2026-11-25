//! # Rescheduling after each command, and the schedule snapshot for the UI
//!
//! ## What this file is for
//! Two jobs that both sit between the engine and the outside world:
//! 1. **Rescheduling**: after every command the document reruns the engine
//!    on the whole project, and decides whether the new result is
//!    acceptable ([`rejection`]).
//! 2. **The snapshot**: [`ScheduleSnapshot`] combines what the user typed
//!    (task names, durations, links) with what the engine calculated
//!    (dates, slack, critical flag) into one list, in row order, ready to be
//!    sent to the UI as JSON and drawn in the grid and the Gantt chart.
//!
//! ## Why the snapshot lives here, in `document`
//! - `model` holds only what the user typed; it knows nothing of dates.
//! - `engine` holds only calculated dates, keyed by task ID; it knows
//!   nothing of the UI.
//! - The UI needs **both together**. The `document` is the one crate that
//!   owns both the project and the latest result, so it is the natural place
//!   to join them. The shell (`backend/src-tauri/`) then just passes the
//!   snapshot on, without any scheduling knowledge (the project documentation,
//!   dependency rule).
//!
//! ## Main ideas
//! - **Full reschedule**: the document always recalculates the whole
//!   project. For the shed (8 tasks) that takes microseconds and for 10,000
//!   tasks about 10 ms; smarter partial rescheduling can come later if large
//!   projects need it.
//! - **A command must not break a working schedule.** If the engine fails
//!   after a command (for example because a new link closes a circle), the
//!   command is refused and the project stays as it was.
//!
//! ## Documentation

use chrono::{NaiveDate, NaiveDateTime};
use engine::{Explanation, ScheduleError, ScheduleResult, Segment, SegmentKind};
use model::{
    Blocker, BlockerId, CalendarId, Constraint, Dependency, DependencyId, DependencyType, Outline,
    Project, ProjectId, Risk, ScheduleFrom, TaskId, TaskMode,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Runs the engine on the project.
///
/// # Parameters
/// - `project`: the project to schedule (only read).
///
/// # Returns
/// The engine's result: the calculated dates, or the reason it failed.
///
/// # Example
/// Internal: the `Document` calls it after every command.
pub(crate) fn reschedule(project: &Project) -> Result<ScheduleResult, ScheduleError> {
    engine::schedule(project)
}

/// Decides whether a new schedule result means "refuse the command".
///
/// # The rule
/// | Schedule before | Schedule after | Decision |
/// |---|---|---|
/// | worked | works | accept |
/// | worked | fails (any error) | **refuse**: the command broke it |
/// | failed | fails with a circle | **refuse**: a circle is always the edit's fault |
/// | failed | fails otherwise / works | accept: the user may be repairing it |
///
/// "Schedule before failed" can only happen when a project was opened in a
/// broken state (for example an imported file whose task names a calendar
/// that is missing). Refusing every edit then would trap the user, so
/// edits are allowed, except ones that add a circle.
///
/// Example: a project opened with a task calendar that does not exist
/// (schedule fails); the user adds a link 8 -> 1 that closes a loop
/// 1 -> ... -> 8 -> 1. The engine checks for circles before scheduling
/// and says `Cycle`, so the third row of the table applies and the link is
/// refused.
///
/// MS Project behaves the same way for circles: it refuses a link that
/// would create a circular dependency.
///
/// # Parameters
/// - `before_worked`: did the schedule work before the command?
/// - `after`: the engine's result after the command.
///
/// # Returns
/// `Some(error)` when the command must be refused, `None` when it may stay.
pub(crate) fn rejection(
    before_worked: bool,
    after: &Result<ScheduleResult, ScheduleError>,
) -> Option<ScheduleError> {
    match after {
        Ok(_) => None,
        Err(error @ ScheduleError::Cycle { .. }) => Some(error.clone()),
        Err(error) if before_worked => Some(error.clone()),
        Err(_) => None,
    }
}

/// One link into a task, as the grid's "Predecessors" column and the link
/// dialog need it.
///
/// # Fields
/// - `id`: the link's UUID.
/// - `predecessor`: the task it comes from.
/// - `kind`: FS, SS, FF or SF (`"finishToStart"`…).
/// - `lag_minutes`: the time lag in working minutes (negative = lead).
/// - `lag_percent`: the percentage lag, if the lag is a percentage.
///
/// # Example
/// "2FS+1d" in the grid is
/// `{ "id": "…", "predecessor": "<task 2>", "kind": "finishToStart", "lagMinutes": 480, "lagPercent": null }`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRow {
    /// The link's UUID.
    pub id: DependencyId,
    /// The predecessor task.
    pub predecessor: TaskId,
    /// The link type.
    pub kind: DependencyType,
    /// Time lag in working minutes.
    pub lag_minutes: i64,
    /// Percentage lag, if any.
    pub lag_percent: Option<i32>,
}

impl LinkRow {
    /// Builds the row for one dependency.
    fn from_dependency(d: &Dependency) -> Self {
        LinkRow {
            id: d.id,
            predecessor: d.predecessor,
            kind: d.kind,
            lag_minutes: d.lag.minutes(),
            lag_percent: d.lag_percent,
        }
    }
}

/// One blocker of a task, as the UI needs it: what the user
/// typed plus where the engine placed the blocked time.
///
/// # Fields
/// - `id`: the blocker's UUID.
/// - `after_work_minutes`: work done before the blocker starts, in working
///   minutes (as stored).
/// - `duration_minutes`: the blocked time, in working minutes.
/// - `reason`: why the task is blocked.
/// - `reported_on`: the day the delay was reported, if given.
/// - `start` / `finish`: the calculated dates of the blocked piece of the
///   bar; `None` when the schedule failed (or the blocker has zero length).
///
/// # Example
/// "Buy materials" with a 5-day blocker after 2 days of work:
/// ```text
/// { "id": "5e2a…", "afterWorkMinutes": 960, "durationMinutes": 2400,
///   "reason": "Wood delivery delayed", "reportedOn": "2027-03-04",
///   "start": "2027-03-05T08:00:00", "finish": "2027-03-11T17:00:00" }
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockerRow {
    /// The blocker's UUID.
    pub id: BlockerId,
    /// Work done before the blocker starts, in working minutes.
    pub after_work_minutes: i64,
    /// Blocked time in working minutes.
    pub duration_minutes: i64,
    /// Why the task is blocked.
    pub reason: String,
    /// The day the delay was reported, if given.
    pub reported_on: Option<NaiveDate>,
    /// Calculated start of the blocked piece.
    pub start: Option<NaiveDateTime>,
    /// Calculated finish of the blocked piece.
    pub finish: Option<NaiveDateTime>,
}

impl BlockerRow {
    /// Builds the row for one blocker, finding its blocked piece among the
    /// task's calculated segments (empty when the schedule failed).
    fn new(blocker: &Blocker, segments: &[Segment]) -> Self {
        let piece = segments.iter().find(|s| {
            s.kind
                == SegmentKind::Blocked {
                    blocker: blocker.id,
                }
        });
        BlockerRow {
            id: blocker.id,
            after_work_minutes: blocker.after_work.minutes(),
            duration_minutes: blocker.duration.minutes(),
            reason: blocker.reason.clone(),
            reported_on: blocker.reported_on,
            start: piece.map(|p| p.start),
            finish: piece.map(|p| p.finish),
        }
    }
}

/// `true` for zero. Used by serde to leave `blockedMinutes: 0` out of the
/// JSON (see [`TaskRow`]).
fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// A calendar's ID and name, for the calendar pickers of the UI.
///
/// # Fields
/// - `id`: the calendar's UUID.
/// - `name`: its name, e.g. "Standard".
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarRow {
    /// The calendar's UUID.
    pub id: CalendarId,
    /// Its display name.
    pub name: String,
}

/// One row of the [`ScheduleSnapshot`]: one task with its calculated dates.
///
/// # Fields
/// What the user typed (name, duration, outline level, mode, constraint,
/// deadline, calendar, notes, hyperlink, links), the outline facts derived
/// from the rows (outline number, parent, summary flag), and what the engine
/// calculated (dates, slack, critical flag, missed deadline, explanation).
///
/// The calculated fields are `None` (JSON `null`) only when the schedule
/// could not be calculated (see [`ScheduleSnapshot::error`]).
///
/// Dates are written as `"YYYY-MM-DDTHH:MM:SS"`, without a time zone,
/// e.g. `"2027-03-01T08:00:00"`. Durations and slack are in minutes of
/// working time; divide by `minutesPerDay` (480) to show days.
///
/// # Blockers
/// Three fields describe a task's blockers. They are **left out of the
/// JSON** when the task has no blockers, so rows of ordinary tasks look
/// exactly as before; the UI treats a missing field as "none":
/// - `blockers`: one [`BlockerRow`] per blocker, in the task's list order.
/// - `segments`: the work and blocked pieces of the bar, in time order
///   (`{ "start", "finish", "kind": { "type": "work" } }` or
///   `"kind": { "type": "blocked", "blocker": "<id>" }`). Draw blocked
///   pieces in the blocker colour; `start`/`finish` of the row span them all.
/// - `blockedMinutes`: the total blocked time. `durationMinutes` stays the
///   planned work (3 days), while the bar is 3 + 5 = 8 days long.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    /// The task's UUID.
    pub id: TaskId,
    /// The task's name.
    pub name: String,
    /// The duration in minutes of working time (5 days = 2400). For a
    /// summary task, the rolled-up duration when the schedule is known.
    pub duration_minutes: i64,
    /// The outline level (1 = top level).
    pub outline_level: u32,
    /// The outline number, e.g. "1.2" (also used as the WBS code).
    pub outline_number: String,
    /// The summary task this task belongs to, if any.
    pub parent_id: Option<TaskId>,
    /// `true` for a summary task (it has subtasks).
    pub is_summary: bool,
    /// `true` when shown as a milestone (zero duration or marked).
    pub is_milestone: bool,
    /// Automatic or manual scheduling.
    pub mode: TaskMode,
    /// The constraint (ASAP by default).
    pub constraint: Constraint,
    /// The deadline, if any.
    pub deadline: Option<NaiveDateTime>,
    /// The task calendar, if any (`None` = project calendar).
    pub calendar_id: Option<CalendarId>,
    /// The notes.
    pub notes: String,
    /// The hyperlink (empty = none).
    pub hyperlink: String,
    /// Scheduled start (what the Gantt bar shows).
    pub start: Option<NaiveDateTime>,
    /// Scheduled finish.
    pub finish: Option<NaiveDateTime>,
    /// Earliest start (forward pass).
    pub early_start: Option<NaiveDateTime>,
    /// Earliest finish (forward pass).
    pub early_finish: Option<NaiveDateTime>,
    /// Latest start that does not delay the project (backward pass).
    pub late_start: Option<NaiveDateTime>,
    /// Latest finish that does not delay the project (backward pass).
    pub late_finish: Option<NaiveDateTime>,
    /// Total slack in minutes (negative when a constraint or deadline
    /// cannot be met).
    pub total_slack_minutes: Option<i64>,
    /// Free slack in minutes.
    pub free_slack_minutes: Option<i64>,
    /// `true` if the task is on the critical path. `false` when unknown.
    pub is_critical: bool,
    /// `true` when the task finishes after its deadline.
    pub missed_deadline: bool,
    /// The tasks this one depends on, in link-list order.
    pub predecessors: Vec<TaskId>,
    /// The links into this task, in link-list order, with type and lag.
    pub links: Vec<LinkRow>,
    /// "Why this date?" (`None` when the schedule failed).
    pub explanation: Option<Explanation>,
    /// The task's blockers (left out of the JSON when there are none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<BlockerRow>,
    /// Work and blocked pieces of the bar (left out when there are none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// Total blocked time in working minutes (left out when zero).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub blocked_minutes: i64,
    /// Percent complete, 0–100; for a summary task the average of its
    /// subtasks, weighted by duration (left out when zero).
    #[serde(default, skip_serializing_if = "is_zero_percent")]
    pub percent_complete: u8,
    /// `true` for a project row (left out when `false`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_project: bool,
    /// What could go wrong (left out when there are none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<Risk>,
    /// The finish saved by "Set plan" (left out when there is none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_finish: Option<NaiveDateTime>,
    /// When the work really finished (left out when not known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_finish: Option<NaiveDateTime>,
}

/// `true` for 0 %: serde then leaves the field out of the JSON.
fn is_zero_percent(percent: &u8) -> bool {
    *percent == 0
}

/// The percent complete to show for a row.
///
/// # What it does
/// An ordinary task shows its own value. A summary task shows the average
/// of the ordinary tasks inside it, weighted by duration, rounded **down**
/// so that 100 % means "everything is done". If they all have zero duration
/// (milestones), each counts the same.
///
/// # Example
/// ```text
/// Build (summary)     30 %   = (4 d × 50 % + 2 d × 0 % + ...)
/// ```
/// With subtasks of 4 days at 50 % and 2 days at 0 %: 200 / 6 = 33 %.
fn shown_percent(project: &Project, outline: &Outline, row: usize) -> u8 {
    if !outline.is_summary(row) {
        // An empty project has nothing to complete.
        let task = &project.tasks[row];
        return if task.is_project {
            0
        } else {
            task.percent_complete.min(100)
        };
    }
    let leaves: Vec<_> = (row + 1..outline.subtree_end(row))
        .filter(|&r| !outline.is_summary(r))
        .map(|r| &project.tasks[r])
        .collect();
    let all_zero = leaves.iter().all(|t| t.duration.minutes() <= 0);
    let weight = |minutes: i64| if all_zero { 1 } else { minutes.max(0) };
    let total: i64 = leaves.iter().map(|t| weight(t.duration.minutes())).sum();
    let done: i64 = leaves
        .iter()
        .map(|t| weight(t.duration.minutes()) * i64::from(t.percent_complete.min(100)))
        .sum();
    u8::try_from(done / total.max(1)).unwrap_or(100)
}

/// Everything the UI needs to draw the grid and the Gantt chart, in one
/// JSON-ready value.
///
/// # JSON shape (shortened)
/// ```text
/// {
///   "projectId": "…", "projectName": "Garden shed",
///   "minutesPerDay": 480, "scheduleFrom": "start", "finishDate": null,
///   "calendarId": "…", "calendars": [ { "id": "…", "name": "Standard" } ],
///   "projectStart": "2027-03-01T08:00:00",
///   "projectFinish": "2027-03-23T17:00:00",
///   "error": null,
///   "tasks": [
///     { "id": "6f1c…", "name": "Design the shed", "durationMinutes": 960,
///       "outlineLevel": 1, "outlineNumber": "1", "parentId": null,
///       "isSummary": false, "isMilestone": false, "mode": "auto",
///       "constraint": { "type": "asSoonAsPossible" }, "deadline": null,
///       "start": "2027-03-01T08:00:00", "finish": "2027-03-02T17:00:00",
///       "totalSlackMinutes": 0, "isCritical": true, "predecessors": [],
///       "links": [], "explanation": { "driver": { "type": "projectStart" }, … },
///       … },
///     …
///   ]
/// }
/// ```
///
/// # Example
/// ```
/// use chrono::NaiveDate;
/// use document::Document;
/// use model::{Duration, Project, Task};
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Design the shed", Duration::from_days(2)));
/// let snapshot = Document::new(project).snapshot();
/// let json = serde_json::to_value(&snapshot).unwrap();
/// assert_eq!(json["tasks"][0]["durationMinutes"], 960);
/// assert_eq!(json["tasks"][0]["earlyFinish"], "2027-03-02T17:00:00");
/// assert_eq!(json["error"], serde_json::Value::Null);
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSnapshot {
    /// The project's UUID.
    pub project_id: ProjectId,
    /// The project's name.
    pub project_name: String,
    /// Working minutes in one day (480 = 8 hours), for showing days.
    pub minutes_per_day: i64,
    /// Whether the project is scheduled from its start or its finish.
    pub schedule_from: ScheduleFrom,
    /// The fixed finish typed by the user (used when scheduling from the
    /// finish), if any.
    pub finish_date: Option<NaiveDateTime>,
    /// The project calendar.
    pub calendar_id: CalendarId,
    /// All calendars (ID and name), for pickers.
    pub calendars: Vec<CalendarRow>,
    /// First working moment of the project (`None` if the schedule failed).
    pub project_start: Option<NaiveDateTime>,
    /// Finish of the whole project (`None` if the schedule failed).
    pub project_finish: Option<NaiveDateTime>,
    /// Why the schedule could not be calculated (an error code with
    /// details, see `engine::ScheduleError`), otherwise `None`.
    pub error: Option<ScheduleError>,
    /// One row per task, in project (row) order.
    pub tasks: Vec<TaskRow>,
}

impl ScheduleSnapshot {
    /// Joins the project and the engine's result into one snapshot.
    ///
    /// # Parameters
    /// - `project`: what the user typed.
    /// - `schedule`: the engine's latest result for exactly this project.
    ///
    /// # Returns
    /// The snapshot. If `schedule` is an error, the dates are `None` and
    /// `error` holds it.
    ///
    /// # Example
    /// Use [`crate::Document::snapshot`], which calls this.
    pub fn build(project: &Project, schedule: Result<&ScheduleResult, &ScheduleError>) -> Self {
        let outline = Outline::build(&project.tasks);
        // Group the links by successor once, so each row finds its links
        // quickly (instead of scanning every link for every task, which
        // would be slow for 10,000 tasks). A BTreeMap keeps it deterministic.
        let mut links_into: BTreeMap<TaskId, Vec<&Dependency>> = BTreeMap::new();
        for d in &project.dependencies {
            links_into.entry(d.successor).or_default().push(d);
        }

        let tasks = project
            .tasks
            .iter()
            .enumerate()
            .map(|(row, task)| {
                // `result.tasks[i]` belongs to `project.tasks[i]` (engine
                // guarantee), so we can pick the dates by row number.
                let dates = schedule.ok().and_then(|result| result.tasks.get(row));
                let segments: Vec<Segment> = dates.map(|d| d.segments.clone()).unwrap_or_default();
                let links: Vec<LinkRow> = links_into
                    .get(&task.id)
                    .map(|list| list.iter().map(|d| LinkRow::from_dependency(d)).collect())
                    .unwrap_or_default();
                TaskRow {
                    id: task.id,
                    name: task.name.clone(),
                    duration_minutes: dates
                        .map_or(task.duration.minutes(), |d| d.duration.minutes()),
                    outline_level: task.outline_level,
                    outline_number: outline.outline_number(row),
                    parent_id: outline.parent(row).map(|p| project.tasks[p].id),
                    is_summary: outline.is_summary(row),
                    is_milestone: task.is_milestone() && !task.is_project,
                    mode: task.mode,
                    constraint: task.constraint,
                    deadline: task.deadline,
                    calendar_id: task.calendar_id,
                    notes: task.notes.clone(),
                    hyperlink: task.hyperlink.clone(),
                    start: dates.map(|d| d.start),
                    finish: dates.map(|d| d.finish),
                    early_start: dates.map(|d| d.early_start),
                    early_finish: dates.map(|d| d.early_finish),
                    late_start: dates.map(|d| d.late_start),
                    late_finish: dates.map(|d| d.late_finish),
                    total_slack_minutes: dates.map(|d| d.total_slack.minutes()),
                    free_slack_minutes: dates.map(|d| d.free_slack.minutes()),
                    is_critical: dates.is_some_and(|d| d.is_critical),
                    missed_deadline: dates.is_some_and(|d| d.missed_deadline),
                    predecessors: links.iter().map(|l| l.predecessor).collect(),
                    links,
                    explanation: dates.map(|d| d.explanation),
                    blockers: task
                        .blockers
                        .iter()
                        .map(|b| BlockerRow::new(b, &segments))
                        .collect(),
                    blocked_minutes: dates
                        .map_or(task.blocked_time().minutes(), |d| d.blocked.minutes()),
                    segments,
                    percent_complete: shown_percent(project, &outline, row),
                    is_project: task.is_project,
                    risks: task.risks.clone(),
                    planned_finish: task.planned_finish,
                    actual_finish: task.actual_finish,
                }
            })
            .collect();

        ScheduleSnapshot {
            project_id: project.id,
            project_name: project.name.clone(),
            minutes_per_day: project.minutes_per_day,
            schedule_from: project.schedule_from,
            finish_date: project.finish,
            calendar_id: project.calendar_id,
            calendars: project
                .calendars
                .iter()
                .map(|c| CalendarRow {
                    id: c.id,
                    name: c.name.clone(),
                })
                .collect(),
            project_start: schedule.ok().map(|r| r.project_start),
            project_finish: schedule.ok().map(|r| r.project_finish),
            error: schedule.err().cloned(),
            tasks,
        }
    }
}
