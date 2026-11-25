//! # The critical path method (CPM): runs the steps in order
//!
//! ## What this file is for
//! This module is the conductor of the scheduling calculation. It prepares a
//! [`Context`] (calendars, outline, graph), calls each step of the critical
//! path method in the right order and assembles the final
//! [`ScheduleResult`].
//!
//! ## Where it fits
//! Called by [`crate::schedule`] after the project has been checked. Each
//! step lives in its own file:
//!
//! ```text
//!  graph.rs        order the nodes so predecessors come first; find cycles
//!     │
//!  forward.rs      early start / early finish     (left to right)
//!     │
//!  backward.rs     late start / late finish       (right to left)
//!     │
//!  ../constraints.rs  ALAP tasks (or ASAP tasks when scheduling from the finish)
//!  ../summary.rs      summary tasks: roll the scheduled dates up
//!     │
//!  slack.rs        total slack and free slack
//!     │
//!  critical.rs     which tasks are critical
//! ```
//!
//! ## Two directions
//! | | Schedule from **start** (default) | Schedule from **finish** |
//! |---|---|---|
//! | fixed date | project start | project finish |
//! | first pass | forward: early dates (these are the task dates) | backward: late dates (these are the task dates) |
//! | second pass | backward from the latest early finish: late dates | forward from the earliest late start: early dates |
//! | moved afterwards | ALAP tasks, as late as their successors allow | ASAP tasks, as early as their predecessors allow |
//!
//! ## Main ideas
//! - Inside the CPM code, tasks are referred to by their **row** (position
//!   in `project.tasks`) and graph nodes by their **node number**. Both
//!   exist only during one calculation; outside the engine we always use
//!   [`TaskId`](model::TaskId).
//! - All lists are `Vec`s in row order, so the result is the same every time
//!   (determinism).
//!
//! ## Documentation

pub mod backward;
pub mod critical;
pub mod forward;
pub mod graph;
pub mod slack;

use crate::calendar_math;
use crate::explain::Explanation;
use crate::{ScheduleError, ScheduleResult, TaskSchedule, blockers, constraints, summary};
use chrono::NaiveDateTime;
use graph::{EdgeKind, TaskGraph};
use model::{Calendar, Constraint, Duration, Outline, Project, ScheduleFrom};
use std::collections::BTreeMap;

/// The start and finish of one task, as produced by a pass. The forward pass
/// fills it with *early* dates, the backward pass with *late* dates.
///
/// # Fields
/// - `start`: the start moment.
/// - `finish`: the finish moment.
///
/// # Example
/// ```
/// use engine::cpm::TaskDates;
/// use chrono::NaiveDate;
/// let day = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap();
/// let dates = TaskDates {
///     start: day.and_hms_opt(8, 0, 0).unwrap(),
///     finish: day.and_hms_opt(17, 0, 0).unwrap(),
/// };
/// assert!(dates.start < dates.finish);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskDates {
    /// Start moment.
    pub start: NaiveDateTime,
    /// Finish moment.
    pub finish: NaiveDateTime,
}

/// What one pass (forward or backward) produced.
///
/// # Fields
/// - `dates`: one [`TaskDates`] per task row. For a summary task, the
///   roll-up of its subtasks.
/// - `explanations`: why each task got its dates in this pass.
/// - `durations`: the working duration of each row: the task's own duration,
///   or, for a summary task, the working time between its rolled-up start
///   and finish (on the project calendar). Percentage lags use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassResult {
    /// Dates per task row.
    pub dates: Vec<TaskDates>,
    /// Explanation per task row.
    pub explanations: Vec<Explanation>,
    /// Working duration per task row.
    pub durations: Vec<Duration>,
}

/// Everything the passes need to know about a project, prepared once.
///
/// # Fields
/// - `project`: the project being scheduled.
/// - `project_calendar`: the project calendar (lags are counted on it, and
///   summary durations are measured on it).
/// - `calendars`: the calendar of each task row (its task calendar, or the
///   project calendar).
/// - `outline`: the outline (parents, summary tasks).
/// - `graph`: the dependency graph and calculation order.
/// - `project_rows`: the project row each row belongs to.
///
/// # Example
/// ```
/// use engine::cpm::Context;
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Dig", Duration::from_days(1)));
/// let context = Context::new(&project).unwrap();
/// assert_eq!(context.calendars.len(), 1);
/// ```
///
/// # Beginner note: lifetimes (`'a`)
/// `Context<'a>` *borrows* the project and its calendars instead of copying
/// them. The `'a` (a "lifetime") tells the compiler: a `Context` must not
/// outlive the project it borrows from.
#[derive(Clone, Debug)]
pub struct Context<'a> {
    /// The project being scheduled.
    pub project: &'a Project,
    /// The project calendar.
    pub project_calendar: &'a Calendar,
    /// The calendar of each task row.
    pub calendars: Vec<&'a Calendar>,
    /// The outline of the tasks.
    pub outline: Outline,
    /// The dependency graph.
    pub graph: TaskGraph,
    /// For each row, the row of the project it belongs to (a project row
    /// belongs to itself); `None` in a file without project rows.
    pub project_rows: Vec<Option<usize>>,
}

impl<'a> Context<'a> {
    /// Prepares the context: finds and checks the calendars, builds the
    /// outline and the graph.
    ///
    /// # Parameters
    /// - `project`: the project to schedule.
    ///
    /// # Returns
    /// The ready [`Context`].
    ///
    /// # Errors
    /// Checked in this order, so a loop is always reported first:
    /// - Any error of [`TaskGraph::build`] (for example a dependency loop).
    /// - [`ScheduleError::CalendarNotFound`]: no project calendar.
    /// - [`ScheduleError::TaskCalendarNotFound`]: a task names a calendar
    ///   that does not exist.
    /// - [`ScheduleError::Calendar`]: a calendar in use is invalid.
    ///
    /// # Example
    /// See [`Context`].
    pub fn new(project: &'a Project) -> Result<Context<'a>, ScheduleError> {
        // Build the graph FIRST, before any calendar checks. Why: a loop of
        // links is the one error the document must always refuse, even while
        // the schedule is already broken for another reason (for example a
        // missing task calendar). If calendars were checked first, the
        // calendar error would hide the loop and the document would let it in.
        let outline = Outline::build(&project.tasks);
        let graph = TaskGraph::build(project, &outline)?;

        let find = |id| project.calendars.iter().find(|c| c.id == id);
        let project_calendar = find(project.calendar_id)
            .ok_or(ScheduleError::CalendarNotFound(project.calendar_id))?;
        calendar_math::validate_calendar(project_calendar)?;

        let mut calendars = Vec::with_capacity(project.tasks.len());
        for task in &project.tasks {
            let calendar = match task.calendar_id {
                None => project_calendar,
                Some(id) => {
                    let calendar = find(id).ok_or(ScheduleError::TaskCalendarNotFound {
                        task: task.id,
                        calendar: id,
                    })?;
                    // Validate each task calendar (cheap; usually only a few
                    // distinct ones, and the project calendar is skipped).
                    if calendar.id != project_calendar.id {
                        calendar_math::validate_calendar(calendar)?;
                    }
                    calendar
                }
            };
            calendars.push(calendar);
        }

        // A top-level row starts a new group: a project, or (in a file
        // without projects) nothing.
        let mut current = None;
        let project_rows = project
            .tasks
            .iter()
            .enumerate()
            .map(|(row, task)| {
                if outline.parent(row).is_none() {
                    current = task.is_project.then_some(row);
                }
                current
            })
            .collect();

        Ok(Context {
            project,
            project_calendar,
            calendars,
            outline,
            graph,
            project_rows,
        })
    }

    /// The working duration a percentage lag is measured against, before
    /// any pass has run: the task's own duration (summary tasks get theirs
    /// from a pass).
    fn own_durations(&self) -> Vec<Duration> {
        self.project.tasks.iter().map(|t| t.duration).collect()
    }

    /// The start of each row's project: the project row's own start date
    /// (its "start no earlier than"), or the start of the whole file.
    ///
    /// # Parameters
    /// - `file_start`: the file's start, already in working time.
    ///
    /// # Errors
    /// A [`calendar_math::CalendarError`] if a date is out of range.
    ///
    /// # Worked example
    /// ```text
    /// file start Mon 1 Mar 08:00
    /// House  (no own start)        -> Mon 1 Mar 08:00 for all its tasks
    /// Garage (own start Sat 6 Mar) -> Mon 8 Mar 08:00 for all its tasks
    /// ```
    pub fn project_starts(
        &self,
        file_start: NaiveDateTime,
    ) -> Result<Vec<NaiveDateTime>, calendar_math::CalendarError> {
        self.project_rows
            .iter()
            .map(
                |root| match root.map(|r| self.project.tasks[r].constraint) {
                    Some(Constraint::StartNoEarlierThan { date }) => {
                        calendar_math::snap_forward(self.project_calendar, date)
                    }
                    _ => Ok(file_start),
                },
            )
            .collect()
    }

    /// The finish of each row's project: the latest finish in it. This is
    /// what makes a task critical *for its own project*.
    ///
    /// # Parameters
    /// - `dates`: the early dates of all rows.
    pub fn project_finishes(&self, dates: &[TaskDates]) -> Vec<NaiveDateTime> {
        let mut latest: BTreeMap<Option<usize>, NaiveDateTime> = BTreeMap::new();
        for (&root, d) in self.project_rows.iter().zip(dates) {
            let finish = latest.entry(root).or_insert(d.finish);
            *finish = (*finish).max(d.finish);
        }
        self.project_rows.iter().map(|root| latest[root]).collect()
    }

    /// `true` if some link out of a summary task uses a percentage lag. Only
    /// then does scheduling from the finish need an extra sizing pass (see
    /// [`run`]).
    fn has_percent_lag_from_summary(&self) -> bool {
        (0..self.graph.task_count).any(|row| {
            self.outline.is_summary(row)
                && self.graph.successors[row].iter().any(|e| {
                    matches!(
                        e.kind,
                        EdgeKind::Link {
                            lag_percent: Some(_),
                            ..
                        }
                    )
                })
        })
    }
}

/// Runs the whole critical path method on a prepared context.
///
/// # What it does (scheduling from the start)
/// 1. Snaps the project start forward to working time (Mon 00:00 -> 08:00).
/// 2. Forward pass: early dates ([`forward::forward_pass`]).
/// 3. Project finish = the latest early finish (or the start if no tasks);
///    with project rows, each project has its own.
/// 4. Backward pass from those finishes: late dates ([`backward::backward_pass`]).
/// 5. The task dates are the early dates, then ALAP tasks are moved as late
///    as their successors allow ([`constraints::schedule_alap`]) and summary
///    tasks are rolled up again ([`summary::roll_up_scheduled`]).
/// 6. Slack ([`slack`]) and critical flags ([`critical`]).
/// 7. Tasks with blockers: their scheduled bar is cut into work and blocked
///    pieces ([`blockers::segments`]). (The passes already placed every
///    task with its blocked time, see [`blockers::span`].)
///
/// Scheduling from the finish mirrors steps 1–5 (see the module table).
///
/// # Parameters
/// - `context`: the prepared context ([`Context::new`]).
///
/// # Returns
/// The complete [`ScheduleResult`].
///
/// # Errors
/// [`ScheduleError::ProjectFinishMissing`] when scheduling from the finish
/// without a finish date; [`ScheduleError::Calendar`] from calendar
/// arithmetic (in practice only for dates out of range).
///
/// # Example
/// ```
/// use engine::cpm;
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Dig", Duration::from_days(1)));
/// let context = cpm::Context::new(&project).unwrap();
/// let result = cpm::run(&context).unwrap();
/// assert_eq!(result.project_finish.to_string(), "2027-03-01 17:00:00");
/// ```
///
/// Most callers should use [`crate::schedule`] instead, which also checks
/// the tasks first.
pub fn run(context: &Context) -> Result<ScheduleResult, ScheduleError> {
    let project = context.project;
    let calendar = context.project_calendar;

    // `let (a, b, …) = match … { … }` takes several values out of one
    // `match`: here the results that both directions produce.
    let (project_start, project_finish, finishes, early, late, mut scheduled, explanations) =
        match project.schedule_from {
            ScheduleFrom::Start => {
                let file_start = calendar_math::snap_forward(calendar, project.start)?;
                let starts = context.project_starts(file_start)?;
                let early = forward::forward_pass(context, &starts, true)?;
                let start = starts.iter().copied().min().unwrap_or(file_start);
                let finish = latest_finish(&early, start);
                // Each project has its own finish, so its own critical path.
                let finishes = context.project_finishes(&early.dates);
                let late = backward::backward_pass(context, &finishes, false, &early.durations)?;
                let mut scheduled = early.dates.clone();
                let mut explanations = early.explanations.clone();
                constraints::schedule_alap(
                    context,
                    &mut scheduled,
                    &mut explanations,
                    &early.durations,
                    &finishes,
                )?;
                (
                    start,
                    finish,
                    finishes,
                    early,
                    late,
                    scheduled,
                    explanations,
                )
            }
            ScheduleFrom::Finish => {
                let wanted = project.finish.ok_or(ScheduleError::ProjectFinishMissing)?;
                let finish = calendar_math::snap_backward(calendar, wanted)?;
                // All projects of the file aim at the one fixed finish.
                let finishes = vec![finish; project.tasks.len()];
                // The backward pass comes first here, but a percentage lag
                // out of a summary task needs the summary's duration, which
                // only a forward pass knows. In that rare case a quick
                // forward "sizing" pass from the project start provides it.
                let durations = if context.has_percent_lag_from_summary() {
                    let anchor = calendar_math::snap_forward(calendar, project.start)?;
                    forward::forward_pass(context, &vec![anchor; project.tasks.len()], true)?
                        .durations
                } else {
                    context.own_durations()
                };
                let late = backward::backward_pass(context, &finishes, true, &durations)?;
                let start = earliest_start(&late, finish);
                let early =
                    forward::forward_pass(context, &vec![start; project.tasks.len()], false)?;
                let mut scheduled = late.dates.clone();
                let mut explanations = late.explanations.clone();
                constraints::schedule_asap(
                    context,
                    &mut scheduled,
                    &mut explanations,
                    &early.durations,
                    start,
                )?;
                (
                    start,
                    finish,
                    finishes,
                    early,
                    late,
                    scheduled,
                    explanations,
                )
            }
        };

    // Summary tasks: roll the (possibly moved) task dates up again.
    summary::roll_up_scheduled(&context.outline, &mut scheduled);

    // Slack and critical flags, then one TaskSchedule per task.
    let total = slack::total_slack(context, &early.dates, &late.dates);
    let free = slack::free_slack(context, &early, &total, &finishes)?;

    // `collect::<Result<…>>()` below: the closure returns a `Result` for
    // each task (cutting the blocker segments can fail on calendar
    // arithmetic); collecting into a `Result<Vec<_>, _>` stops at the first
    // error, and `?` passes that error on.
    let tasks = project
        .tasks
        .iter()
        .enumerate()
        .map(|(row, task)| -> Result<TaskSchedule, ScheduleError> {
            let is_summary = context.outline.is_summary(row);
            // Blockers: the passes already placed the task
            // with its blocked time (`blockers::span`). Here we only cut the
            // scheduled bar into work and blocked pieces for the UI, and
            // tell "Why this date?" how much blocked time pushed the finish.
            // Summary tasks ignore blockers (their dates are rolled up).
            let (blocked, segments) = if is_summary {
                (Duration::ZERO, Vec::new())
            } else {
                let pieces =
                    blockers::segments(task, context.calendars[row], scheduled[row].start)?;
                (task.blocked_time(), pieces)
            };
            let mut explanation = explanations[row];
            explanation.blocked = blocked;
            let duration = if is_summary {
                calendar_math::working_minutes_between(
                    calendar,
                    scheduled[row].start,
                    scheduled[row].finish,
                )
            } else {
                task.duration
            };
            Ok(TaskSchedule {
                task_id: task.id,
                start: scheduled[row].start,
                finish: scheduled[row].finish,
                duration,
                early_start: early.dates[row].start,
                early_finish: early.dates[row].finish,
                late_start: late.dates[row].start,
                late_finish: late.dates[row].finish,
                total_slack: total[row],
                free_slack: free[row],
                is_critical: critical::is_critical(total[row]),
                is_summary,
                missed_deadline: task.deadline.is_some_and(|d| scheduled[row].finish > d),
                explanation,
                blocked,
                segments,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ScheduleResult {
        project_start,
        project_finish,
        tasks,
    })
}

/// The latest finish of a pass, or `fallback` when there are no tasks.
fn latest_finish(pass: &PassResult, fallback: NaiveDateTime) -> NaiveDateTime {
    // `.max()` returns `None` for an empty list; `.unwrap_or` then uses the
    // fallback.
    pass.dates
        .iter()
        .map(|d| d.finish)
        .max()
        .unwrap_or(fallback)
}

/// The earliest start of a pass, or `fallback` when there are no tasks.
fn earliest_start(pass: &PassResult, fallback: NaiveDateTime) -> NaiveDateTime {
    pass.dates.iter().map(|d| d.start).min().unwrap_or(fallback)
}

/// The lag of a link in working minutes, given the predecessor's duration.
///
/// # The rule
/// - A time lag is used as it is.
/// - A percentage lag is that share of the predecessor's duration, rounded
///   to the nearest minute (halves away from zero). Example: predecessor
///   3 days = 1 440 min, lag 25 % → 360 min = 6 hours.
///
/// # Parameters
/// - `kind`: the edge (only `Link` edges have a lag; others give zero).
/// - `predecessor_duration`: the predecessor's working duration.
///
/// # Returns
/// The lag (negative for a lead).
pub(crate) fn lag_of(kind: &EdgeKind, predecessor_duration: Duration) -> Duration {
    match kind {
        EdgeKind::Link {
            lag_percent: Some(percent),
            ..
        } => {
            let product = predecessor_duration.minutes() * i64::from(*percent);
            // Integer division rounds toward zero; adding half of 100 (with
            // the product's sign) first makes it round to the nearest.
            let half = if product >= 0 { 50 } else { -50 };
            Duration::from_minutes((product + half) / 100)
        }
        EdgeKind::Link { lag, .. } => *lag,
        EdgeKind::Structure => Duration::ZERO,
    }
}
