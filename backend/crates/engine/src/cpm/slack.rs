//! # Slack: how far a task can slip
//!
//! ## What this file is for
//! Calculates two kinds of slack (also called *float*) for every task:
//!
//! - **Total slack**: how much working time the task can slip without
//!   delaying the **project finish** (or breaking a constraint or deadline).
//! - **Free slack**: how much working time it can slip without delaying the
//!   early dates of **any successor**.
//!
//! ## Where it fits
//! A step of the critical path method, called by [`super::run`] after both
//! passes. The critical flag ([`super::critical`]) is decided from the total
//! slack.
//!
//! ## The rules
//! - Total slack = the smaller of `LS - ES` and `LF - EF`, in working time on
//!   the task's calendar (summary tasks: the project calendar). This is MS
//!   Project's definition. It is **negative** when the rules conflict (a
//!   deadline or "finish no later than" that cannot be met): the task is
//!   then already late by that much.
//! - Free slack = for each successor link, the working time between the
//!   moment the link allows and the successor's early date, by link type:
//!
//!   | Link | gap |
//!   |---|---|
//!   | FS | successor ES − (EF + lag) |
//!   | SS | successor ES − (ES + lag) |
//!   | FF | successor EF − (EF + lag) |
//!   | SF | successor EF − (ES + lag) |
//!
//!   Take the smallest gap. Links out of the task's summary tasks count too.
//!   A task with no successor measures to the project finish. Free slack is
//!   never negative, and never more than total slack when total slack is
//!   not negative (with negative total slack, free slack is 0).
//! - Slack is **working time**, counted with [`working_minutes_between`]; it
//!   skips weekends and holidays.
//!
//! ## Worked example: "Buy materials" in the shed project
//! ```text
//! ES Wed 3 Mar 08:00, LS Tue 9 Mar 08:00
//! working time between: Wed + Thu + Fri + Mon = 4 x 480 = 1 920 min = 4 days
//!   -> total slack 4 days
//! EF Fri 5 Mar 17:00; its successor "Build the floor" starts Fri 12 Mar 08:00
//! working time between: Mon + Tue + Wed + Thu = 1 920 min = 4 days
//!   -> free slack 4 days
//! ```
//! These match the table in the project documentation.
//!
//! ## Documentation

use super::graph::EdgeKind;
use super::{Context, PassResult, TaskDates, lag_of};
use crate::calendar_math::{CalendarError, add_working_time, working_minutes_between};
use chrono::NaiveDateTime;
use model::{Calendar, DependencyType, Duration};

/// Calculates the total slack of every task.
///
/// # Parameters
/// - `context`: the prepared project (for each row's calendar).
/// - `early`: early dates from the forward pass.
/// - `late`: late dates from the backward pass (same order).
///
/// # Returns
/// One [`Duration`] per task, in row order.
///
/// # Example
/// ```
/// use engine::cpm::{slack::total_slack, Context, TaskDates};
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// p.tasks.push(Task::new("Buy materials", Duration::from_days(3)));
/// let context = Context::new(&p).unwrap();
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// // "Buy materials": early Wed 3 - Fri 5 Mar, late Tue 9 - Thu 11 Mar.
/// let early = [TaskDates { start: at(3, 8), finish: at(5, 17) }];
/// let late = [TaskDates { start: at(9, 8), finish: at(11, 17) }];
/// assert_eq!(total_slack(&context, &early, &late)[0].minutes(), 4 * 480);
/// ```
///
/// # Beginner note: slices
/// `&[TaskDates]` is a *slice*: a borrowed view of a list. It accepts a
/// `Vec`, an array, or part of either, without copying anything.
pub fn total_slack(context: &Context, early: &[TaskDates], late: &[TaskDates]) -> Vec<Duration> {
    // `.zip` walks two lists side by side, giving pairs (early[i], late[i]).
    early
        .iter()
        .zip(late)
        .enumerate()
        .map(|(row, (e, l))| {
            let calendar = row_calendar(context, row);
            let by_start = working_minutes_between(calendar, e.start, l.start);
            let by_finish = working_minutes_between(calendar, e.finish, l.finish);
            by_start.min(by_finish)
        })
        .collect()
}

/// Calculates the free slack of every task.
///
/// # Parameters
/// - `context`: the prepared project (graph, outline, calendars).
/// - `early`: the forward pass (early dates and durations).
/// - `total`: total slack per task (free slack is capped by it).
/// - `project_finishes`: the finish of each row's project, used for tasks without successors.
///
/// # Returns
/// One [`Duration`] per task, in row order; always `>= 0` and `<= total
/// slack` (when total slack is not negative).
///
/// # Errors
/// A [`CalendarError`] if adding a lag fails (in practice only for dates out
/// of range).
///
/// # Example
/// See the `shed` example (`cargo run -p engine --example shed`): "Buy
/// materials" gets 4 days of free slack, every other task 0.
pub fn free_slack(
    context: &Context,
    early: &PassResult,
    total: &[Duration],
    project_finishes: &[NaiveDateTime],
) -> Result<Vec<Duration>, CalendarError> {
    let graph = &context.graph;
    let rows = graph.task_count;
    let mut result = Vec::with_capacity(rows);
    // `total` has one entry per row, so walking it gives every row number.
    for (row, &row_total) in total.iter().enumerate().take(rows) {
        let calendar = row_calendar(context, row);
        let mine = early.dates[row];
        // The task's own links, and (for an ordinary task) those of the
        // summary tasks above it.
        let mut owners = vec![row];
        if !context.outline.is_summary(row) {
            owners.extend(context.outline.ancestors(row));
        }
        let mut smallest: Option<Duration> = None;
        for owner in owners {
            for edge in &graph.successors[owner] {
                let EdgeKind::Link { kind, .. } = edge.kind else {
                    continue;
                };
                let theirs = early.dates[graph.row_of(edge.node)];
                let lag = lag_of(&edge.kind, early.durations[owner]);
                // Which end of this task, and which end of the successor.
                let (from, to) = match kind {
                    DependencyType::FinishToStart => (mine.finish, theirs.start),
                    DependencyType::StartToStart => (mine.start, theirs.start),
                    DependencyType::FinishToFinish => (mine.finish, theirs.finish),
                    DependencyType::StartToFinish => (mine.start, theirs.finish),
                };
                let allowed = add_working_time(context.project_calendar, from, lag)?;
                let gap = working_minutes_between(calendar, allowed, to);
                smallest = Some(smallest.map_or(gap, |s| s.min(gap)));
            }
        }
        // No successor at all: the task may slip up to the project finish.
        let free = match smallest {
            Some(gap) => gap,
            None => working_minutes_between(calendar, mine.finish, project_finishes[row]),
        };
        // Free slack never exceeds total slack, and is never negative.
        result.push(free.min(row_total).max(Duration::ZERO));
    }
    Ok(result)
}

/// The calendar slack is measured on for one row: the task's calendar, or
/// the project calendar for a summary task.
fn row_calendar<'a>(context: &Context<'a>, row: usize) -> &'a Calendar {
    if context.outline.is_summary(row) {
        context.project_calendar
    } else {
        context.calendars[row]
    }
}
