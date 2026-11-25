//! # Lag and lead tests
//!
//! ## What this file checks
//! A *lag* is waiting time on a link: "start B two days after A finishes"
//! (for example, concrete must dry). A *lead* is a negative lag: "start B one
//! day before A finishes" (an overlap). Lags count **working** time, like
//! durations. These tests check that rule around weekends, nights, lunch
//! breaks and the project start.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//! Lag and lead are explained in the project documentation.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, date, days, dt, hours, link_with_lag, project_on};
use model::Duration;

/// A 1-day lag after a task that finishes on Friday at 17:00: the lag day is
/// Monday, so the successor starts on Tuesday at 08:00.
///
/// Why it matters: lags count working days. Counting calendar days would
/// wrongly let the successor start on Monday (Saturday being the "lag day").
#[test]
fn one_day_lag_after_friday_finish_starts_tuesday() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "Pour concrete", days(5)); // Mon-Fri
    let b = add_task(&mut project, "Build on it", days(1));
    link_with_lag(&mut project, a, b, days(1));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, a).early_finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-09 08:00"));
    assert_eq!(dates_of(&result, b).early_finish, dt("2027-03-09 17:00"));
}

/// A 4-hour lag after a task that finishes Monday 17:00 uses the Tuesday
/// morning (08:00–12:00). The successor starts after lunch, Tuesday 13:00,
/// and a 1-day successor finishes Wednesday 12:00.
///
/// Why it matters: lags can be shorter than a day. The night, and then the
/// lunch break, must both be skipped.
#[test]
fn four_hour_lag_starts_successor_after_lunch() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, hours(4));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-02 13:00"));
    assert_eq!(dates_of(&result, b).early_finish, dt("2027-03-03 12:00"));
}

/// A 2-day lag after a Thursday finish skips the weekend: Friday and Monday
/// are the lag days, so the successor starts on Tuesday.
///
/// Why it matters: a lag longer than one day must also skip whole weekends.
#[test]
fn two_day_lag_skips_the_weekend() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(4)); // Mon-Thu
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, days(2));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-09 08:00"));
}

/// A 1-day lag after a Thursday finish, with Friday a holiday: the lag day is
/// Monday, so the successor starts Tuesday.
///
/// Why it matters: holidays are non-working time for lags as well as for
/// durations.
#[test]
fn lag_skips_a_holiday() {
    let mut project = project_on("2027-03-01");
    project.calendars[0].holidays.insert(date("2027-03-05"));
    let a = add_task(&mut project, "A", days(4)); // Mon-Thu
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, days(1));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-09 08:00"));
}

/// A lead of 1 day (lag −1d): A runs Monday to Wednesday, so B may start one
/// working day before A's finish, on Wednesday 08:00.
///
/// Why it matters: leads let tasks overlap ("start painting the first wall
/// while the last wall is still being built"). The engine must move the
/// successor earlier, counting working time backwards.
#[test]
fn one_day_lead_overlaps_by_one_day() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3)); // Mon-Wed
    let b = add_task(&mut project, "B", days(2));
    link_with_lag(&mut project, a, b, days(-1));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, b).early_finish, dt("2027-03-04 17:00"));
}

/// A lead that crosses a weekend: A runs Mon 1 – Mon 8 March (6 days); a lead
/// of 2 days pulls B back over the weekend to Friday 5 March 08:00.
///
/// Why it matters: counting working time backwards must skip weekends too.
#[test]
fn lead_counts_back_over_the_weekend() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(6));
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, days(-2));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, a).early_finish, dt("2027-03-08 17:00"));
    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-05 08:00"));
}

/// A lead longer than the predecessor itself cannot pull the successor
/// before the project start: B starts at the project start, Monday 08:00.
///
/// Why it matters: no task may start before the project does. Without this
/// rule, a large lead would schedule work in the past.
#[test]
fn lead_never_moves_a_task_before_the_project_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, days(-5));
    let result = schedule_ok(&project);

    assert_eq!(dates_of(&result, b).early_start, dt("2027-03-01 08:00"));
}

/// With a lag on the critical path, both tasks are critical and the lag is
/// not counted as slack: A (1d) +2d lag → B (1d) ends Thursday; free slack of
/// A is zero.
///
/// Why it matters: waiting time on a link is required, not optional. It must
/// not show up as "spare time" that the user could use.
#[test]
fn lag_is_not_slack() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    link_with_lag(&mut project, a, b, days(2));
    let result = schedule_ok(&project);

    let a_dates = dates_of(&result, a);
    assert!(a_dates.is_critical);
    assert_eq!(a_dates.total_slack, Duration::ZERO);
    assert_eq!(a_dates.free_slack, Duration::ZERO);
    assert!(dates_of(&result, b).is_critical);
    assert_eq!(result.project_finish, dt("2027-03-04 17:00"));
}
