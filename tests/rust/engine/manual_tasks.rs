//! # Manually scheduled task tests
//!
//! ## What this file checks
//! A **manually scheduled** task stays exactly where the user put it: links
//! and constraints do not move it, and its start is not even moved to
//! working time. It still pushes its successors, and it still gets late
//! dates and slack.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). Book
//! section 2.3 explains automatic and manual scheduling.
//!
//! ## Main ideas
//! Standard calendar, project start Monday 1 March 2027.

use crate::{dates_of, schedule_ok};
use engine::{Driver, TaskEnd};
use ganttora_tests::{add_task, days, dt, link, project_on};
use model::TaskMode;

/// A manual task typed to start on Saturday 6 Mar 10:00 keeps that start;
/// its 1 working day is counted on the calendar: the weekend is skipped and
/// Monday 8 Mar 08:00–17:00 is the working day → finish Mon 8 17:00.
/// "Why this date?" says `ManualStart`.
///
/// Why it matters: the user's typed date is respected even on a weekend
/// (MS Project allows that too).
#[test]
fn manual_task_keeps_a_weekend_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "Inspection", days(1));
    project.tasks[0].mode = TaskMode::Manual;
    project.tasks[0].manual_start = Some(dt("2027-03-06 10:00"));
    let result = schedule_ok(&project);
    let a = dates_of(&result, a);
    assert_eq!(a.start, dt("2027-03-06 10:00"));
    assert_eq!(a.finish, dt("2027-03-08 17:00"));
    assert_eq!(a.explanation.driver, Driver::ManualStart);
    assert_eq!(a.explanation.end, TaskEnd::Start);
}

/// A manual task ignores its predecessor: P (3 days) finishes Wed 3 17:00,
/// but the manual successor M was typed for Mon 1 08:00 and stays there.
///
/// Why it matters: "manual" means the engine never moves the task.
#[test]
fn manual_task_ignores_its_predecessor() {
    let mut project = project_on("2027-03-01");
    let p = add_task(&mut project, "P", days(3));
    let m = add_task(&mut project, "M", days(1));
    link(&mut project, p, m);
    project.tasks[1].mode = TaskMode::Manual;
    project.tasks[1].manual_start = Some(dt("2027-03-01 08:00"));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, m).start, dt("2027-03-01 08:00"));
    assert_eq!(dates_of(&result, m).finish, dt("2027-03-01 17:00"));
}

/// A manual task still pushes its automatic successor: M typed for Wed 3
/// 08:00, 2 days → Thu 4 17:00; the successor S starts Fri 5 08:00.
///
/// Why it matters: a manual date is still a real date for the tasks that
/// depend on it.
#[test]
fn manual_task_pushes_its_successor() {
    let mut project = project_on("2027-03-01");
    let m = add_task(&mut project, "M", days(2));
    let s = add_task(&mut project, "S", days(1));
    link(&mut project, m, s);
    project.tasks[0].mode = TaskMode::Manual;
    project.tasks[0].manual_start = Some(dt("2027-03-03 08:00"));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, m).finish, dt("2027-03-04 17:00"));
    assert_eq!(dates_of(&result, s).start, dt("2027-03-05 08:00"));
}

/// A manual task without a typed start begins at the project start (Mon 1
/// 08:00); "Why this date?" then says `ProjectStart`.
///
/// Why it matters: a task switched to manual before it got a date must still
/// appear somewhere sensible (engine-dev note: to check against MS Project).
#[test]
fn manual_task_without_start_uses_project_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.tasks[0].mode = TaskMode::Manual;
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-01 08:00"));
    assert_eq!(
        dates_of(&result, a).explanation.driver,
        Driver::ProjectStart
    );
}

/// A manual task ignores its constraint: MSO Fri 5 Mar on a manual task
/// typed for Mon 1 leaves it on Mon 1.
///
/// Why it matters: constraints are for automatic scheduling only.
#[test]
fn manual_task_ignores_its_constraint() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.tasks[0].mode = TaskMode::Manual;
    project.tasks[0].manual_start = Some(dt("2027-03-01 08:00"));
    project.tasks[0].constraint = model::Constraint::MustStartOn {
        date: dt("2027-03-05 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-01 08:00"));
}
