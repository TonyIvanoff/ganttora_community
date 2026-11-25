//! # The backward pass: the latest each task can start and finish
//!
//! ## What this file is for
//! Calculates every task's **late finish (LF)** and **late start (LS)**: the
//! latest moments it can finish and start without delaying the project
//! finish (or any successor, constraint or deadline). It walks the graph
//! from the last node to the first (reverse topological order: successors
//! first).
//!
//! ## Where it fits
//! A step of the critical path method, called by [`super::run`]. When
//! scheduling from the project start it is the second pass (late dates for
//! slack); when scheduling from the finish its dates *are* the task dates.
//!
//! ## The rules — the forward rules mirrored
//! For each task, in reverse topological order:
//!
//! 1. **Upper limits.** Collect every "not after" limit ([`link_limit`]):
//!
//!    | Link to a successor | Limit on this task | Example (successor late Wed 08:00 – Thu 17:00, lag 0) |
//!    |---|---|---|
//!    | FS | finish ≤ successor start − lag | finish ≤ Wed 08:00 → Tue 17:00 |
//!    | SS | start ≤ successor start − lag | start ≤ Wed 08:00 |
//!    | FF | finish ≤ successor finish − lag | finish ≤ Thu 17:00 |
//!    | SF | start ≤ successor finish − lag | start ≤ Thu 17:00 |
//!
//!    plus the project finish, the deadline, and whatever the task's
//!    summary tasks pass down.
//! 2. **Start limits become finish limits** (duration counted forward).
//! 3. **The earliest limit wins**, then the constraint is applied.
//! 4. **Late finish**: moved *backward* to working time. A task that must
//!    finish by Wednesday 08:00 must really finish by Tuesday 17:00.
//! 5. **Late start** = late finish − duration, in working time.
//!
//! A summary task's own node collects the limits of links *out of* the
//! summary and passes them down to its subtasks; its gate rolls the
//! subtasks' late dates up.
//!
//! ## Worked example: the shed project (project finish Tue 23 Mar 2027 17:00)
//! ```text
//! task          successors' LS - lag        LF                LS
//! 8 Finished    none: project finish        Tue 23 Mar 17:00  Tue 23 Mar 17:00
//! 7 Paint       8: Tue 23 Mar 17:00         Tue 23 Mar 17:00  Mon 22 Mar 08:00
//! 6 Walls       7: Mon 22 Mar 08:00         Fri 19 Mar 17:00  Tue 16 Mar 08:00
//! 5 Floor       6: Tue 16 Mar 08:00         Mon 15 Mar 17:00  Fri 12 Mar 08:00
//! 4 Ground      5: Fri 12 Mar 08:00         Thu 11 Mar 17:00  Wed 10 Mar 08:00
//! 3 Buy (3d)    5: Fri 12 Mar 08:00         Thu 11 Mar 17:00  Tue 9 Mar 08:00
//! 2 Permit      4: Wed 10 Mar 08:00         Tue 9 Mar 17:00   Wed 3 Mar 08:00
//! 1 Design      2: Wed 3 Mar 08:00, 3: Tue 9 Mar 08:00 -> earliest: Wed 3 Mar 08:00
//!                                           Tue 2 Mar 17:00   Mon 1 Mar 08:00
//! ```
//! "Buy materials" could start as late as Tuesday 9 March: that is its slack.
//!
//! ## Documentation

use super::graph::EdgeKind;
use super::{Context, PassResult, TaskDates, lag_of};
use crate::calendar_math::{CalendarError, subtract_working_time};
use crate::constraints::{place_backward, summary_upper_limits};
use crate::explain::{Bound, Driver, Limits, TaskEnd};
use crate::manual::place_manual;
use crate::summary::{roll_up, summary_explanation};
use chrono::NaiveDateTime;
use model::{Calendar, DependencyType, Duration, TaskId, TaskMode};

/// Calculates the late start and late finish of every task.
///
/// # Parameters
/// - `context`: the prepared project ([`Context::new`]).
/// - `anchors`: for each row the finish of its project (already moved to
///   working time). Tasks with no other limit finish here.
/// - `honor_lower`: `true` when this pass decides the task dates
///   (scheduling from the finish). Then "start/finish no earlier than"
///   constraints are honoured even against links, and manually scheduled
///   tasks keep their typed dates. `false` when the pass only measures late
///   dates (scheduling from the start).
/// - `durations`: the working duration of each row, for percentage lags
///   (summary tasks get theirs from a forward pass).
///
/// # Returns
/// A [`PassResult`]: late dates, explanations and durations per row.
///
/// # Errors
/// A [`CalendarError`] if calendar arithmetic fails (in practice only for
/// dates out of range).
///
/// # Example
/// ```
/// use engine::cpm::{backward::backward_pass, Context};
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// p.tasks.push(Task::new("Paint", Duration::from_days(2)));
/// let context = Context::new(&p).unwrap();
/// let finish = NaiveDate::from_ymd_opt(2027, 3, 23).unwrap().and_hms_opt(17, 0, 0).unwrap();
/// let durations = vec![Duration::from_days(2)];
/// let late = backward_pass(&context, &[finish], false, &durations).unwrap();
/// // Ends Tue 23 Mar 17:00 at the latest, so starts Mon 22 Mar 08:00 at the latest.
/// assert_eq!(late.dates[0].start.to_string(), "2027-03-22 08:00:00");
/// ```
pub fn backward_pass(
    context: &Context,
    anchors: &[NaiveDateTime],
    honor_lower: bool,
    durations: &[Duration],
) -> Result<PassResult, CalendarError> {
    let project = context.project;
    let graph = &context.graph;
    let rows = graph.task_count;

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
        .map(|(c, &at)| Bound::new(at, Driver::ProjectFinish).explain(TaskEnd::Finish, at, c.id))
        .collect();
    // The limits each summary task's own node passes down (indexed by row;
    // only summary rows are used).
    let mut summary_limits: Vec<Limits> = vec![Limits::default(); rows];

    // `.iter().rev()` walks the order backwards: successors first.
    for &node in graph.order.iter().rev() {
        let row = graph.row_of(node);
        let task = &project.tasks[row];
        let finish_bound = Bound::new(anchors[row], Driver::ProjectFinish);

        if graph.is_gate(node) {
            // A summary task's gate: roll up the late dates of its direct
            // subtasks (a subtask that is a summary is reached through its
            // gate, and its rolled-up dates are in `dates[its row]`).
            let children = graph.successors[node]
                .iter()
                .filter(|e| e.kind == EdgeKind::Structure)
                .map(|e| dates[graph.row_of(e.node)]);
            if let Some(rolled) = roll_up(children) {
                dates[row] = rolled;
                explanations[row] = summary_explanation(
                    rolled.finish,
                    TaskEnd::Finish,
                    context.project_calendar.id,
                );
            }
        } else {
            // A task node (an ordinary task, or a summary task's own node):
            // collect the "not after" limits from its outgoing arrows.
            let mut limits = Limits {
                start: None,
                finish: Some(finish_bound),
            };
            let is_summary = context.outline.is_summary(row);
            for edge in &graph.successors[node] {
                match edge.kind {
                    EdgeKind::Link { .. } => {
                        let q = graph.row_of(edge.node);
                        let (on_start, on_finish) = link_limit(
                            context.project_calendar,
                            &edge.kind,
                            project.tasks[q].id,
                            dates[q],
                            durations[row],
                        )?;
                        // Limits of a summary task's own links are marked
                        // "via" it before they are passed down.
                        let mark = |b: Bound| if is_summary { via(b, task.id) } else { b };
                        if let Some(b) = on_start {
                            limits.earlier_start(mark(b));
                        }
                        if let Some(b) = on_finish {
                            limits.earlier_finish(mark(b));
                        }
                    }
                    // The arrow to the enclosing summary task's own node.
                    EdgeKind::Structure => {
                        inherit_earlier(&mut limits, summary_limits[edge.node]);
                    }
                }
            }

            if is_summary {
                summary_upper_limits(task, &mut limits);
                summary_limits[row] = limits;
                continue;
            }

            let calendar = context.calendars[row];
            let placement = match task.mode {
                TaskMode::Manual if honor_lower => place_manual(task, calendar, anchors[row])?,
                _ => place_backward(task, calendar, &limits, honor_lower)?,
            };
            dates[row] = placement.dates;
            explanations[row] = placement.explanation;
        }
    }

    Ok(PassResult {
        dates,
        explanations,
        durations: durations.to_vec(),
    })
}

/// The "not after" limit one link puts on its predecessor.
///
/// # What it does
/// Applies the table in the module notes: FS and FF limit the predecessor's
/// **finish**, SS and SF its **start**. The lag is subtracted on the project
/// calendar.
///
/// # Parameters
/// - `project_calendar`: lags are counted on it.
/// - `kind`: the link edge; any other edge gives no limit.
/// - `successor`: the successor's ID (for the explanation).
/// - `successor_dates`: the successor's dates in this pass (or its current
///   task dates, when ALAP tasks are moved).
/// - `predecessor_duration`: the predecessor's working duration (for
///   percentage lags).
///
/// # Returns
/// `(limit on the start, limit on the finish)`: exactly one is `Some`.
///
/// # Errors
/// A [`CalendarError`] from subtracting the lag.
pub fn link_limit(
    project_calendar: &Calendar,
    kind: &EdgeKind,
    successor: TaskId,
    successor_dates: TaskDates,
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
    let driver = Driver::Successor {
        dependency,
        task: successor,
        kind: link_type,
    };
    // Which end of the successor the link arrives at.
    let to = match link_type {
        DependencyType::FinishToStart | DependencyType::StartToStart => successor_dates.start,
        DependencyType::FinishToFinish | DependencyType::StartToFinish => successor_dates.finish,
    };
    let bound = Bound::new(subtract_working_time(project_calendar, to, lag)?, driver);
    // Which end of the predecessor it limits.
    Ok(match link_type {
        DependencyType::FinishToStart | DependencyType::FinishToFinish => (None, Some(bound)),
        DependencyType::StartToStart | DependencyType::StartToFinish => (Some(bound), None),
    })
}

/// Marks a bound as belonging to a summary task (unless it already does).
fn via(mut bound: Bound, summary: TaskId) -> Bound {
    if bound.via_summary.is_none() {
        bound.via_summary = Some(summary);
    }
    bound
}

/// Adds the upper limits a summary task passes down to its subtasks.
fn inherit_earlier(limits: &mut Limits, parent: Limits) {
    if let Some(b) = parent.start {
        limits.earlier_start(b);
    }
    if let Some(b) = parent.finish {
        limits.earlier_finish(b);
    }
}
