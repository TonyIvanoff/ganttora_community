//! # Milestone tests
//!
//! ## What this file checks
//! A milestone is a task with zero duration: a moment, not a piece of work
//! (for example "Shed finished"). Its start and finish are the same moment.
//! These tests check *which* moment: a milestone after a task sits at that
//! task's finish (17:00), not at the next morning.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//! Milestones are introduced in the project documentation.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, days, dt, link, project_on};
use model::Duration;

/// A milestone after a 1-day task sits at Monday 17:00, the moment the task
/// finishes. Start and finish are equal, and it is critical.
///
/// Why it matters: MS Project shows "Work done" on the same day the work
/// ends. Moving it to Tuesday 08:00 would make reports show the wrong day.
#[test]
fn milestone_after_a_task_sits_at_its_finish() {
    let mut project = project_on("2027-03-01");
    let work = add_task(&mut project, "Work", days(1));
    let done = add_task(&mut project, "Work done", Duration::ZERO);
    link(&mut project, work, done);
    let result = schedule_ok(&project);

    let milestone = dates_of(&result, done);
    assert_eq!(milestone.early_start, dt("2027-03-01 17:00"));
    assert_eq!(milestone.early_finish, dt("2027-03-01 17:00"));
    assert!(milestone.is_critical);
    assert_eq!(result.project_finish, dt("2027-03-01 17:00"));
}

/// A milestone after a task that ends Friday 17:00 stays on Friday; it does
/// not jump over the weekend to Monday.
///
/// Why it matters: same rule as above, at the end of a week, where a mistake
/// would move the milestone by three calendar days.
#[test]
fn milestone_after_friday_task_stays_on_friday() {
    let mut project = project_on("2027-03-01");
    let work = add_task(&mut project, "Work", days(5));
    let done = add_task(&mut project, "Work done", Duration::ZERO);
    link(&mut project, work, done);
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, done).early_start, dt("2027-03-05 17:00"));
}

/// A milestone without predecessors sits at the project start, Monday 08:00.
///
/// Why it matters: "Project kick-off" milestones are common. They must
/// appear at the first working moment.
#[test]
fn milestone_without_predecessors_sits_at_project_start() {
    let mut project = project_on("2027-03-01");
    let kickoff = add_task(&mut project, "Kick-off", Duration::ZERO);
    let result = schedule_ok(&project);

    assert_eq!(
        dates_of(&result, kickoff).early_start,
        dt("2027-03-01 08:00")
    );
    assert_eq!(result.project_finish, dt("2027-03-01 08:00"));
}

/// In a chain A (1d) → milestone → B (1d), B starts on Tuesday 08:00 and all
/// three are critical with zero slack.
///
/// Why it matters: a milestone in the middle of a chain must pass the date
/// on correctly and must not create fake slack between 17:00 and 08:00
/// (that night is not working time).
#[test]
fn milestone_in_a_chain_passes_the_date_on() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let m = add_task(&mut project, "Checkpoint", Duration::ZERO);
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, m);
    link(&mut project, m, b);
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-02 08:00"));
    for id in [a, m, b] {
        let task = dates_of(&result, id);
        assert!(task.is_critical);
        assert_eq!(task.total_slack, Duration::ZERO);
    }
}
