//! # Calendar tests: holidays, weekends and odd start moments
//!
//! ## What this file checks
//! How the project calendar changes the schedule: a holiday moves dates, a
//! project that "starts" on a weekend really starts on Monday, and a
//! different work week (Saturdays included) gives different dates.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//! Calendars are explained in the project documentation.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, date, days, dt, link, project_on, row_id, shed};
use model::WorkWeek;

/// the project documentation: if Friday 5 March 2027 is a holiday, the permit (task 2)
/// finishes on Wednesday 10 March instead of Tuesday 9 March. "Buy materials"
/// (task 3) loses its Friday and finishes on Monday 8 March, and tasks 4 to 8
/// all move one working day later, so the shed finishes Wed 24 March.
///
/// Why it matters: one holiday typed into the calendar must move every task
/// it touches. This is the book's own example of what scheduling software
/// does for you.
#[test]
fn holiday_on_fri_5_mar_moves_the_permit_to_wed_10_mar() {
    let mut project = shed();
    project.calendars[0].holidays.insert(date("2027-03-05"));
    let result = schedule_ok(&project);

    let finish = |row| dates_of(&result, row_id(&project, row)).early_finish;
    assert_eq!(finish(2), dt("2027-03-10 17:00"));
    assert_eq!(finish(3), dt("2027-03-08 17:00"));
    assert_eq!(finish(4), dt("2027-03-12 17:00"));
    assert_eq!(finish(5), dt("2027-03-16 17:00"));
    assert_eq!(finish(6), dt("2027-03-22 17:00"));
    assert_eq!(finish(7), dt("2027-03-24 17:00"));
    assert_eq!(finish(8), dt("2027-03-24 17:00"));
    assert_eq!(result.project_finish, dt("2027-03-24 17:00"));
    // Task 3 still has 4 days of slack (Tue 9 to Fri 12 March).
    assert_eq!(dates_of(&result, row_id(&project, 3)).total_slack, days(4));
}

/// A holiday that falls on a Saturday changes nothing: Saturday was not a
/// working day anyway.
///
/// Why it matters: a public holiday list often contains weekend dates. They
/// must not shift the schedule by accident.
#[test]
fn holiday_on_a_weekend_changes_nothing() {
    let plain = schedule_ok(&shed());
    let mut project = shed();
    project.calendars[0].holidays.insert(date("2027-03-06"));
    let with_holiday = schedule_ok(&project);
    // Compare only the dates; the task IDs differ between the two shed copies.
    let finishes = |r: &engine::ScheduleResult| -> Vec<_> {
        r.tasks
            .iter()
            .map(|t| (t.early_start, t.early_finish))
            .collect()
    };
    assert_eq!(finishes(&plain), finishes(&with_holiday));
}

/// A project whose start date is Saturday 6 March 2027 really starts on the
/// next working moment: Monday 8 March 08:00.
///
/// Why it matters: users often type any date as the start. Work can only
/// happen in working time, so the engine must move the start forward.
#[test]
fn saturday_project_start_moves_to_monday_eight() {
    let mut project = project_on("2027-03-06");
    let id = add_task(&mut project, "Dig", days(1));
    let result = schedule_ok(&project);

    assert_eq!(result.project_start, dt("2027-03-08 08:00"));
    assert_eq!(dates_of(&result, id).early_start, dt("2027-03-08 08:00"));
    assert_eq!(dates_of(&result, id).early_finish, dt("2027-03-08 17:00"));
}

/// A project that starts on a holiday Monday begins on Tuesday 08:00.
///
/// Why it matters: holidays and weekends are the same thing for the engine:
/// non-working days. Both must be skipped at the very start too.
#[test]
fn project_start_on_a_holiday_moves_to_next_working_day() {
    let mut project = project_on("2027-03-01");
    project.calendars[0].holidays.insert(date("2027-03-01"));
    let id = add_task(&mut project, "Dig", days(1));
    let result = schedule_ok(&project);

    assert_eq!(result.project_start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, id).early_start, dt("2027-03-02 08:00"));
}

/// A project start during the lunch break (12:30) moves to 13:00, and a
/// 1-day task then finishes at 12:00 the next day.
///
/// Why it matters: start moments are not always at the start of the day.
/// The engine must handle part-days correctly.
#[test]
fn project_start_during_lunch_moves_to_one_pm() {
    let mut project = project_on("2027-03-01");
    project.start = dt("2027-03-01 12:30");
    let id = add_task(&mut project, "Dig", days(1));
    let result = schedule_ok(&project);

    assert_eq!(result.project_start, dt("2027-03-01 13:00"));
    assert_eq!(dates_of(&result, id).early_finish, dt("2027-03-02 12:00"));
}

/// With a six-day work week (Saturday is a working day too), a 5-day task
/// starting on Thursday 4 March finishes on Tuesday 9 March: Thu, Fri, Sat,
/// Mon, Tue.
///
/// Why it matters: not every team works Monday to Friday. The engine must
/// read the work week from the calendar instead of assuming it.
#[test]
fn six_day_work_week_counts_saturday() {
    let mut project = project_on("2027-03-04");
    let mut week = WorkWeek::standard();
    week.saturday = week.monday.clone();
    project.calendars[0].week = week;
    let id = add_task(&mut project, "Five days", days(5));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, id).early_finish, dt("2027-03-09 17:00"));
}

/// A holiday inside a successor's duration makes it one day longer on the
/// calendar, and the following link starts after the holiday.
///
/// Why it matters: holidays must be skipped both inside a task and between
/// linked tasks.
#[test]
fn holiday_inside_a_linked_task_pushes_its_successor() {
    let mut project = project_on("2027-03-01");
    project.calendars[0].holidays.insert(date("2027-03-03"));
    let a = add_task(&mut project, "A", days(3));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    let result = schedule_ok(&project);

    // A: Mon, Tue, (Wed holiday), Thu.
    assert_eq!(dates_of(&result, a).early_finish, dt("2027-03-04 17:00"));
    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-05 08:00"));
}
