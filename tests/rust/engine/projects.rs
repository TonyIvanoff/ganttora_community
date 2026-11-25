//! Tests for several projects in one file: each has its own
//! critical path and may have its own start date.

use chrono::{NaiveDate, NaiveDateTime};
use engine::schedule;
use model::{Constraint, Dependency, Duration, Project, Task};

/// A moment in March 2027 (the 1st is a Monday).
fn at(day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2027, 3, day)
        .unwrap()
        .and_hms_opt(hour, 0, 0)
        .unwrap()
}

/// House: 5-day and 2-day task. Garage: one 3-day task.
fn house_and_garage() -> Project {
    let mut p = Project::new("House", at(1, 0).date());
    p.tasks.push(Task::project_row("House"));
    for (name, days) in [("Walls", 5), ("Order roof", 2)] {
        p.tasks
            .push(Task::new(name, Duration::from_days(days)).at_level(2));
    }
    p.tasks.push(Task::project_row("Garage"));
    p.tasks
        .push(Task::new("Slab", Duration::from_days(3)).at_level(2));
    p
}

/// Gives the garage its own start.
fn start_garage(p: &mut Project, date: NaiveDateTime) {
    p.tasks[3].constraint = Constraint::StartNoEarlierThan { date };
}

#[test]
fn each_project_has_its_own_critical_path() {
    let result = schedule(&house_and_garage()).unwrap();
    let critical: Vec<bool> = result.tasks.iter().map(|t| t.is_critical).collect();
    assert_eq!(critical, [true, true, false, true, true]);
    assert_eq!(result.tasks[2].total_slack, Duration::from_days(3));
    assert_eq!(result.tasks[2].free_slack, Duration::from_days(3));
    // The file still finishes with its last project.
    assert_eq!(result.project_finish, at(5, 17));
}

#[test]
fn a_link_into_another_project_limits_the_task() {
    let mut p = house_and_garage();
    p.dependencies
        .push(Dependency::finish_to_start(p.tasks[2].id, p.tasks[4].id));
    let result = schedule(&p).unwrap();
    // The garage needs the roof order: it cannot slip any more.
    assert!(result.tasks[2].is_critical);
    assert_eq!(result.tasks[4].start, at(3, 8));
}

#[test]
fn a_project_can_start_later() {
    let mut p = house_and_garage();
    start_garage(&mut p, at(6, 8)); // Saturday: moves to Monday
    let result = schedule(&p).unwrap();
    assert_eq!(result.tasks[4].start, at(8, 8));
    assert_eq!(result.tasks[1].start, at(1, 8)); // the house is not moved
    assert!(result.tasks[4].is_critical);
}

#[test]
fn a_project_can_start_before_the_file() {
    let mut p = house_and_garage();
    p.start = at(8, 8);
    start_garage(&mut p, at(1, 8));
    let result = schedule(&p).unwrap();
    assert_eq!(result.tasks[4].start, at(1, 8));
    assert_eq!(result.tasks[1].start, at(8, 8));
    assert_eq!(result.project_start, at(1, 8));
}

#[test]
fn an_empty_project_stands_at_its_start() {
    let mut p = house_and_garage();
    p.tasks.pop();
    start_garage(&mut p, at(10, 8));
    let result = schedule(&p).unwrap();
    assert_eq!(result.tasks[3].start, at(10, 8));
}

#[test]
fn as_late_as_possible_stops_at_its_own_project() {
    let mut p = house_and_garage();
    p.tasks
        .push(Task::new("Door", Duration::from_days(1)).at_level(2));
    p.tasks[5].constraint = Constraint::AsLateAsPossible;
    let result = schedule(&p).unwrap();
    assert_eq!(result.tasks[5].finish, at(3, 17)); // the garage ends Wed, the house Fri
}
