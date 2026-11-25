//! # Basic engine tests: the smallest projects
//!
//! ## What this file checks
//! How the engine behaves with the simplest possible inputs: no tasks at
//! all, one task, a few tasks without links. These are the "edge cases"
//! every larger schedule is built from. If they are wrong, nothing else can
//! be right.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, days, dt, hours, project_on};
use model::{Duration, TaskId};

/// An empty project (no tasks) is not an error. The engine returns an empty
/// list, and the project "finishes" the moment it starts.
///
/// Why it matters: a user creating a new file starts with zero tasks. The app
/// must show an empty schedule, not crash.
#[test]
fn empty_project_has_no_tasks_and_finishes_when_it_starts() {
    let project = project_on("2027-03-01");
    let result = schedule_ok(&project);

    assert!(result.tasks.is_empty());
    assert!(result.critical_tasks().is_empty());
    // Project::new sets the start to midnight; the engine moves it to the
    // first working moment, Monday 08:00.
    assert_eq!(result.project_start, dt("2027-03-01 08:00"));
    assert_eq!(result.project_finish, result.project_start);
}

/// A single 1-day task starts at 08:00 and finishes at 17:00 the same day.
/// One day = 8 working hours: 08:00–12:00 and 13:00–17:00.
///
/// Why it matters: this is the most basic "duration counts working time"
/// rule. The lunch hour is not counted.
#[test]
fn one_day_task_runs_from_eight_to_five() {
    let mut project = project_on("2027-03-01");
    let id = add_task(&mut project, "Dig", days(1));
    let result = schedule_ok(&project);
    let dig = dates_of(&result, id);

    assert_eq!(dig.early_start, dt("2027-03-01 08:00"));
    assert_eq!(dig.early_finish, dt("2027-03-01 17:00"));
    assert_eq!(result.project_finish, dt("2027-03-01 17:00"));
}

/// The only task in a project is always critical, and its late dates equal
/// its early dates (it has no slack).
///
/// Why it matters: the task that ends last decides the project finish, so it
/// can never slip without delaying the project.
#[test]
fn single_task_is_critical_with_zero_slack() {
    let mut project = project_on("2027-03-01");
    let id = add_task(&mut project, "Dig", days(3));
    let result = schedule_ok(&project);
    let dig = dates_of(&result, id);

    assert!(dig.is_critical);
    assert_eq!(dig.total_slack, Duration::ZERO);
    assert_eq!(dig.free_slack, Duration::ZERO);
    assert_eq!(dig.late_start, dig.early_start);
    assert_eq!(dig.late_finish, dig.early_finish);
    assert_eq!(result.critical_tasks(), vec![id]);
}

/// A 5-day task started on Monday finishes on Friday, and a 6-day task
/// jumps over the weekend and finishes on Monday.
///
/// Why it matters: weekends are not working time, so durations must skip
/// them. A beginner mistake is to count calendar days instead.
#[test]
fn durations_skip_the_weekend() {
    let mut project = project_on("2027-03-01");
    let five = add_task(&mut project, "Five days", days(5));
    let six = add_task(&mut project, "Six days", days(6));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, five).early_finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, six).early_finish, dt("2027-03-08 17:00"));
}

/// Tasks without links all start at the project start. The longest one is
/// critical; the shorter ones get slack equal to the difference.
///
/// Why it matters: with no dependencies, nothing makes a task wait. Slack
/// then tells the user how much each shorter task may slip.
#[test]
fn independent_tasks_all_start_at_project_start() {
    let mut project = project_on("2027-03-01");
    let long = add_task(&mut project, "Long", days(4));
    let short = add_task(&mut project, "Short", days(1));
    let result = schedule_ok(&project);

    let long_dates = dates_of(&result, long);
    let short_dates = dates_of(&result, short);
    assert_eq!(long_dates.early_start, dt("2027-03-01 08:00"));
    assert_eq!(short_dates.early_start, dt("2027-03-01 08:00"));
    assert!(long_dates.is_critical);
    assert!(!short_dates.is_critical);
    // 4 days - 1 day = 3 days of slack, for both kinds of slack (no successor).
    assert_eq!(short_dates.total_slack, days(3));
    assert_eq!(short_dates.free_slack, days(3));
    assert_eq!(result.project_finish, dt("2027-03-04 17:00"));
}

/// A task shorter than a day (3 hours) started at 08:00 finishes at 11:00,
/// and a 5-hour task finishes at 14:00 because the lunch hour is skipped.
///
/// Why it matters: durations are stored in minutes, not whole days. Hours
/// must be placed correctly around the lunch break.
#[test]
fn hour_durations_skip_lunch() {
    let mut project = project_on("2027-03-01");
    let three = add_task(&mut project, "Three hours", hours(3));
    let five = add_task(&mut project, "Five hours", hours(5));
    let result = schedule_ok(&project);

    assert_eq!(
        dates_of(&result, three).early_finish,
        dt("2027-03-01 11:00")
    );
    assert_eq!(dates_of(&result, five).early_finish, dt("2027-03-01 14:00"));
}

/// `result.tasks` lists the tasks in the same order as `project.tasks`, and
/// `result.task(id)` returns `None` for an ID that is not in the project.
///
/// Why it matters: the UI shows the results row by row next to the task
/// list, so the order must match. Looking up a missing task must not crash.
#[test]
fn results_follow_project_order_and_unknown_ids_give_none() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(2));
    let c = add_task(&mut project, "C", days(3));
    let result = schedule_ok(&project);

    let ids: Vec<TaskId> = result.tasks.iter().map(|t| t.task_id).collect();
    assert_eq!(ids, vec![a, b, c]);
    assert!(result.task(TaskId::new()).is_none());
}

/// Scheduling never changes the project it is given.
///
/// Why it matters: `schedule` takes `&Project` (a read-only borrow). The
/// user's data must stay exactly as typed; only the result holds new dates.
#[test]
fn schedule_does_not_change_the_project() {
    let project = ganttora_tests::shed();
    let before = project.clone();
    let _ = schedule_ok(&project);
    assert_eq!(project, before);
}
