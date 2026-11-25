//! # The forward pass: the earliest each task can start and finish
//!
//! ## What this file is for
//! Calculates every task's **early start (ES)** and **early finish (EF)** by
//! walking through the graph from the first node to the last, in
//! topological order (predecessors first).
//!
//! ## Where it fits
//! A step of the critical path method, called by [`super::run`]. When
//! scheduling from the project start, its dates *are* the task dates; when
//! scheduling from the finish, it is the second pass and gives the early
//! dates used for slack.
//!
//! ## The rules
//! For each task, in topological order:
//!
//! 1. **Lower limits.** Collect every "not before" limit (see
//!    [`link_limit`]):
//!
//!    | Link | Limit on the successor | Example (predecessor Mon 08:00 – Tue 17:00, lag 0) |
//!    |---|---|---|
//!    | FS | start ≥ predecessor finish + lag | start ≥ Tue 17:00 → Wed 08:00 |
//!    | SS | start ≥ predecessor start + lag | start ≥ Mon 08:00 |
//!    | FF | finish ≥ predecessor finish + lag | finish ≥ Tue 17:00 |
//!    | SF | finish ≥ predecessor start + lag | finish ≥ Mon 08:00 |
//!
//!    plus the project start, plus whatever the task's summary tasks pass
//!    down (their own links and constraints). Lags are counted on the
//!    project calendar.
//! 2. **Finish limits become start limits**: a task of 1 day that must finish
//!    at or after Tue 17:00 must start at or after Tue 08:00 (its duration
//!    counted backward on its own calendar).
//! 3. **The latest limit wins**, then the task's **constraint** is applied
//!    (see `constraints.rs`).
//! 4. **Early start**: the winning moment moved forward to working time on
//!    the task's calendar. A task that may start Tuesday 17:00 starts
//!    Wednesday 08:00 (MS Project convention). *Milestones* stay where they
//!    are if the moment touches working time.
//! 5. **Early finish** = early start + duration, in working time.
//!
//! A **summary task** is not placed this way: it gets the earliest start
//! and latest finish of its subtasks (its "roll-up"). Its **gate** collects
//! the limits of links into the summary and passes them down to every
//! subtask.
//!
//! ## Worked example: the shed project, Standard calendar, start Mon 1 Mar 2027
//! ```text
//! task                    raw start          ES               EF
//! 1 Design (2d)           Mon 1 Mar 08:00    Mon 1 Mar 08:00  Tue 2 Mar 17:00
//! 2 Permit (5d)  after 1  Tue 2 Mar 17:00    Wed 3 Mar 08:00  Tue 9 Mar 17:00
//! 3 Buy (3d)     after 1  Tue 2 Mar 17:00    Wed 3 Mar 08:00  Fri 5 Mar 17:00
//! 4 Ground (2d)  after 2  Tue 9 Mar 17:00    Wed 10 Mar 08:00 Thu 11 Mar 17:00
//! 5 Floor (2d)   after 3, 4: latest of Fri 5 Mar 17:00 and Thu 11 Mar 17:00
//!                         Thu 11 Mar 17:00   Fri 12 Mar 08:00 Mon 15 Mar 17:00
//! 6 Walls (4d)   after 5  Mon 15 Mar 17:00   Tue 16 Mar 08:00 Fri 19 Mar 17:00
//! 7 Paint (2d)   after 6  Fri 19 Mar 17:00   Mon 22 Mar 08:00 Tue 23 Mar 17:00
//! 8 Finished (0d) after 7 Tue 23 Mar 17:00   Tue 23 Mar 17:00 Tue 23 Mar 17:00
//! ```
//! These are the dates of the table at the end of the project documentation.
//!
//! ## Documentation

use super::graph::EdgeKind;
use super::{Context, PassResult, TaskDates, lag_of};
use crate::calendar_math::{CalendarError, add_working_time, working_minutes_between};
use crate::constraints::place_forward;
use crate::explain::{Bound, Driver, Limits};
use crate::manual::place_manual;
use crate::summary::{roll_up, summary_explanation};
use chrono::NaiveDateTime;
use model::{Calendar, DependencyType, Duration, TaskId, TaskMode};

/// Calculates the early start and early finish of every task.
///
/// # Parameters
/// - `context`: the prepared project ([`Context::new`]).
/// - `anchors`: for each row the start of its project, already moved to
///   working time ([`Context::project_starts`]). Tasks with no other limit
///   start here.
/// - `honor_upper`: `true` when this pass decides the task dates
///   (scheduling from the start). Then "start/finish no later than"
///   constraints are honoured even against links, like MS Project's default
///   "tasks will always honor their constraint dates". `false` when the
///   pass only measures early dates (scheduling from the finish).
///
/// # Returns
/// A [`PassResult`]: early dates, explanations and durations per row.
///
/// # Errors
/// A [`CalendarError`] if calendar arithmetic fails (in practice only for
/// dates out of range).
///
/// # Example
/// ```
/// use engine::cpm::{forward::forward_pass, Context};
/// use model::{Dependency, Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 5).unwrap()); // a Friday
/// let a = Task::new("A", Duration::from_days(1));
/// let b = Task::new("B", Duration::from_days(1));
/// p.dependencies.push(Dependency::finish_to_start(a.id, b.id));
/// p.tasks.push(a);
/// p.tasks.push(b);
/// let context = Context::new(&p).unwrap();
/// let start = NaiveDate::from_ymd_opt(2027, 3, 5).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// let early = forward_pass(&context, &[start, start], true).unwrap();
/// // A: Fri 08:00-17:00. B starts at the next working moment: Mon 8 Mar 08:00.
/// assert_eq!(early.dates[1].start.to_string(), "2027-03-08 08:00:00");
/// ```
pub fn forward_pass(
    context: &Context,
    anchors: &[NaiveDateTime],
    honor_upper: bool,
) -> Result<PassResult, CalendarError> {
    let project = context.project;
    let graph = &context.graph;
    let rows = graph.task_count;

    // Placeholders, overwritten below. Because we follow the topological
    // order, everything a node waits for is calculated before the node.
    let mut dates: Vec<TaskDates> = anchors
        .iter()
        .map(|&at| TaskDates {
            start: at,
            finish: at,
        })
        .collect();
    let mut explanations: Vec<_> = context
        .calendars
        .iter()
        .zip(anchors)
        .map(|(c, &at)| {
            Bound::new(at, Driver::ProjectStart).explain(crate::TaskEnd::Start, at, c.id)
        })
        .collect();
    let mut durations: Vec<Duration> = project.tasks.iter().map(|t| t.duration).collect();
    // The limits each gate passes down (indexed by gate number).
    let mut gate_limits: Vec<Limits> = vec![Limits::default(); graph.summary_of_gate.len()];

    for &node in &graph.order {
        let row = graph.row_of(node);
        let task = &project.tasks[row];
        let start_bound = Bound::new(anchors[row], Driver::ProjectStart);

        if graph.is_gate(node) {
            // A summary task's gate: collect the limits for its subtasks.
            let mut limits = Limits {
                start: Some(start_bound),
                finish: None,
            };
            for edge in &graph.predecessors[node] {
                match edge.kind {
                    EdgeKind::Link { .. } => {
                        let p = edge.node;
                        let (on_start, on_finish) = link_limit(
                            context.project_calendar,
                            &edge.kind,
                            project.tasks[p].id,
                            dates[p],
                            durations[p],
                        )?;
                        // These limits belong to the summary task: remember
                        // that, so its subtasks can say "via Phase 2".
                        if let Some(b) = on_start {
                            limits.later_start(via(b, task.id));
                        }
                        if let Some(b) = on_finish {
                            limits.later_finish(via(b, task.id));
                        }
                    }
                    // The gate of an enclosing summary task.
                    EdgeKind::Structure => {
                        let parent = gate_limits[edge.node - rows];
                        inherit_later(&mut limits, parent);
                    }
                }
            }
            crate::constraints::summary_lower_limits(task, &mut limits);
            gate_limits[node - rows] = limits;
        } else if context.outline.is_summary(row) {
            // A summary task's own node: roll up its direct subtasks.
            let children = graph.predecessors[node]
                .iter()
                .filter(|e| e.kind == EdgeKind::Structure)
                .map(|e| dates[e.node]);
            if let Some(rolled) = roll_up(children) {
                dates[row] = rolled;
                durations[row] =
                    working_minutes_between(context.project_calendar, rolled.start, rolled.finish);
                explanations[row] = summary_explanation(
                    rolled.start,
                    crate::TaskEnd::Start,
                    context.project_calendar.id,
                );
            }
        } else {
            // An ordinary task: collect its limits, then place it.
            let mut limits = Limits {
                start: Some(start_bound),
                finish: None,
            };
            for edge in &graph.predecessors[node] {
                match edge.kind {
                    EdgeKind::Link { .. } => {
                        let p = edge.node;
                        let (on_start, on_finish) = link_limit(
                            context.project_calendar,
                            &edge.kind,
                            project.tasks[p].id,
                            dates[p],
                            durations[p],
                        )?;
                        if let Some(b) = on_start {
                            limits.later_start(b);
                        }
                        if let Some(b) = on_finish {
                            limits.later_finish(b);
                        }
                    }
                    EdgeKind::Structure => {
                        inherit_later(&mut limits, gate_limits[edge.node - rows]);
                    }
                }
            }
            let calendar = context.calendars[row];
            let placement = match task.mode {
                TaskMode::Manual => place_manual(task, calendar, anchors[row])?,
                TaskMode::Auto => place_forward(task, calendar, &limits, honor_upper)?,
            };
            dates[row] = placement.dates;
            explanations[row] = placement.explanation;
        }
    }

    Ok(PassResult {
        dates,
        explanations,
        durations,
    })
}

/// The "not before" limit one link puts on its successor.
///
/// # What it does
/// Applies the table in the module notes: FS and SS limit the successor's
/// **start**, FF and SF its **finish**. The lag (time, or a percentage of
/// the predecessor's duration) is added on the project calendar.
///
/// # Parameters
/// - `project_calendar`: lags are counted on it.
/// - `kind`: the link edge (`EdgeKind::Link`); any other edge gives no limit.
/// - `predecessor`: the predecessor's ID (for the explanation).
/// - `predecessor_dates`: the predecessor's dates in this pass (or its
///   current task dates, when ALAP/ASAP tasks are moved).
/// - `predecessor_duration`: its working duration (for percentage lags).
///
/// # Returns
/// `(limit on the start, limit on the finish)`: exactly one is `Some`.
///
/// # Worked example
/// ```text
/// predecessor "Pour concrete" Mon 1 Mar 08:00 - Wed 3 Mar 17:00, 3 days
/// SS +50%: lag = 50% of 1440 min = 720 min = 1.5 days
///          start limit = Mon 08:00 + 1.5 days = Tue 2 Mar 13:00
/// ```
///
/// # Errors
/// A [`CalendarError`] from adding the lag.
pub fn link_limit(
    project_calendar: &Calendar,
    kind: &EdgeKind,
    predecessor: TaskId,
    predecessor_dates: TaskDates,
    predecessor_duration: Duration,
) -> Result<(Option<Bound>, Option<Bound>), CalendarError> {
    let EdgeKind::Link {
        dependency,
        kind: link_type,
        ..
    } = *kind
    else {
        return Ok((None, None));
    };
    let lag = lag_of(kind, predecessor_duration);
    let driver = Driver::Predecessor {
        dependency,
        task: predecessor,
        kind: link_type,
    };
    // Which end of the predecessor the link starts from.
    let from = match link_type {
        DependencyType::FinishToStart | DependencyType::FinishToFinish => predecessor_dates.finish,
        DependencyType::StartToStart | DependencyType::StartToFinish => predecessor_dates.start,
    };
    let bound = Bound::new(add_working_time(project_calendar, from, lag)?, driver);
    // Which end of the successor it limits.
    Ok(match link_type {
        DependencyType::FinishToStart | DependencyType::StartToStart => (Some(bound), None),
        DependencyType::FinishToFinish | DependencyType::StartToFinish => (None, Some(bound)),
    })
}

/// Marks a bound as belonging to a summary task (unless it already does).
fn via(mut bound: Bound, summary: TaskId) -> Bound {
    if bound.via_summary.is_none() {
        bound.via_summary = Some(summary);
    }
    bound
}

/// Adds the lower limits a summary task's gate passes down.
fn inherit_later(limits: &mut Limits, parent: Limits) {
    if let Some(b) = parent.start {
        limits.later_start(b);
    }
    if let Some(b) = parent.finish {
        limits.later_finish(b);
    }
}
