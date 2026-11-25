//! # Link type tests: FS, SS, FF, SF with time lags and percentage lags
//!
//! ## What this file checks
//! A link (dependency) connects one *end* of a predecessor to one *end* of a
//! successor. Since M1 all four kinds are scheduled:
//!
//! | Link | Rule for the successor |
//! |---|---|
//! | FS (finish-to-start) | start ≥ predecessor finish + lag |
//! | SS (start-to-start) | start ≥ predecessor start + lag |
//! | FF (finish-to-finish) | finish ≥ predecessor finish + lag |
//! | SF (start-to-finish) | finish ≥ predecessor start + lag |
//!
//! A lag can be working time ("+1 day") or a percentage of the
//! predecessor's duration ("+50 %"). A negative lag is a *lead* (overlap).
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//!
//! ## Main ideas
//! Every test uses the standard calendar (Mon–Fri, 08:00–12:00 and
//! 13:00–17:00, 480 working minutes a day) and a project that starts on
//! Monday 1 March 2027. The expected dates are worked out by hand in the
//! comments. the project documentation explains link types and lags.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, days, dt, project_on};
use model::{Dependency, DependencyType, Duration, Project, TaskId};

/// Builds the two-task test project used by most tests here: "A" (with the
/// given duration) and "B" (1 day), linked A → B with the given kind.
///
/// # Parameters
/// - `a_duration`: the duration of the predecessor A.
/// - `link`: a function that receives (A, B) and returns the link to add.
///
/// # Returns
/// The project and the IDs of A and B.
fn two_tasks(
    a_duration: Duration,
    link: impl Fn(TaskId, TaskId) -> Dependency,
) -> (Project, TaskId, TaskId) {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", a_duration);
    let b = add_task(&mut project, "B", days(1));
    project.dependencies.push(link(a, b));
    (project, a, b)
}

/// FS without lag: A (2 days) runs Mon 1 08:00 – Tue 2 17:00; B may start at
/// Tue 17:00, which is moved to the next working moment, Wed 3 08:00.
///
/// Why it matters: the most common link, the baseline for the others.
#[test]
fn finish_to_start_waits_for_the_finish() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToStart)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-03 17:00"));
}

/// SS with a 1-day lag: A starts Mon 1 08:00; + 480 working minutes = Mon
/// 17:00; moved to working time = Tue 2 08:00. B (1 day) runs Tue 08:00 –
/// 17:00, although A is still running then (overlap is the point of SS).
///
/// Why it matters: SS lets work overlap ("start painting one day after the
/// walls start").
#[test]
fn start_to_start_with_one_day_lag() {
    let (project, a, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::StartToStart).with_lag(days(1))
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-01 08:00"));
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-02 17:00"));
}

/// SS without lag: B starts together with A, Mon 1 08:00.
///
/// Why it matters: the simplest SS case; it must not be treated like FS.
#[test]
fn start_to_start_without_lag_starts_together() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::StartToStart)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-01 08:00"));
}

/// FF without lag: A finishes Tue 2 17:00, so B must finish at or after Tue
/// 17:00. B takes 1 day, so it starts 1 working day earlier: Tue 08:00.
///
/// Why it matters: FF limits the *finish*; the engine must turn that into a
/// start by counting the duration backward.
#[test]
fn finish_to_finish_ends_together() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToFinish)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-02 17:00"));
}

/// SF with a 2-day lag: A starts Mon 1 08:00; + 960 working minutes = Tue 2
/// 17:00. B must finish at or after Tue 17:00; 1 day earlier is Tue 08:00.
///
/// Why it matters: SF is rare (shift hand-over: the old shift ends when the
/// new one starts) but must follow the same rules.
#[test]
fn start_to_finish_with_two_day_lag() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::StartToFinish).with_lag(days(2))
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-02 17:00"));
}

/// SF without lag: B must finish at or after Mon 1 08:00, which every task
/// does anyway. The project start (Mon 08:00) is the later limit, so B
/// starts Mon 08:00.
///
/// Why it matters: a finish limit that would put the start *before* the
/// project start must not move the task into the past.
#[test]
fn start_to_finish_never_starts_before_the_project() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::StartToFinish)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-01 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-01 17:00"));
}

/// FS with a lead of 1 day (lag −1d): A finishes Tue 2 17:00; minus 480
/// working minutes = Tue 08:00. B starts Tue 2 08:00 (overlapping A's last
/// day).
///
/// Why it matters: a negative lag must count *backward* in working time.
#[test]
fn finish_to_start_with_negative_lag_overlaps() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToStart).with_lag(days(-1))
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
}

/// FS +50 %: A takes 2 days = 960 minutes; 50 % = 480 minutes = 1 day. A
/// finishes Tue 2 17:00; + 1 day = Wed 3 17:00; moved to working time = Thu
/// 4 08:00.
///
/// Why it matters: percentage lags scale with the predecessor ("wait half
/// as long as the concrete took").
#[test]
fn finish_to_start_with_percentage_lag() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToStart).with_lag_percent(50)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-04 08:00"));
}

/// SS +25 % of a 3-day predecessor: 25 % of 1 440 minutes = 360 minutes =
/// 6 hours. A starts Mon 1 08:00; 4 hours until lunch (12:00), then 2 hours
/// after lunch → Mon 15:00. B (1 day = 8 hours) runs Mon 15:00 – Tue 2
/// 15:00 (2 hours on Monday, 6 hours on Tuesday: 08–12 and 13–15).
///
/// Why it matters: a percentage lag can end in the middle of a day, and the
/// lunch break must be skipped.
#[test]
fn start_to_start_with_percentage_lag_ends_mid_day() {
    let (project, _, b) = two_tasks(days(3), |a, b| {
        Dependency::new(a, b, DependencyType::StartToStart).with_lag_percent(25)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-01 15:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-02 15:00"));
}

/// FS −50 % (a percentage lead): 50 % of 2 days = 1 day back from Tue 2
/// 17:00 = Tue 08:00.
///
/// Why it matters: negative percentages are allowed (MS Project: "2FS-50%").
#[test]
fn finish_to_start_with_negative_percentage_lag() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToStart).with_lag_percent(-50)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
}

/// When both a time lag and a percentage are set, the percentage wins (the
/// model says `lag_percent` replaces `lag`). FS with lag 5 days but 0 %: B
/// starts right after A, Wed 3 08:00.
///
/// Why it matters: the two fields must never be added together.
#[test]
fn percentage_lag_replaces_time_lag() {
    let (project, _, b) = two_tasks(days(2), |a, b| {
        Dependency::new(a, b, DependencyType::FinishToStart)
            .with_lag(days(5))
            .with_lag_percent(0)
    });
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-03 08:00"));
}

/// With an SS link, the predecessor's slack is measured against the
/// successor's *start*: A (2d) SS → B (5d). B runs Mon 1 – Fri 5 and decides
/// the project finish (Fri 17:00). A must start no later than B's late
/// start (Mon 08:00) → A has zero total slack, although A itself finishes
/// three days before the project does.
///
/// Why it matters: the backward pass must mirror each link type, otherwise
/// slack and the critical path would be wrong for SS links.
#[test]
fn start_to_start_predecessor_is_critical_through_its_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(5));
    project
        .dependencies
        .push(Dependency::new(a, b, DependencyType::StartToStart));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, a).late_start, dt("2027-03-01 08:00"));
    assert_eq!(dates_of(&result, a).total_slack, Duration::ZERO);
    assert!(dates_of(&result, a).is_critical);
}

/// With an FF link, the predecessor's late finish is the successor's late
/// finish: A (2d) FF → B (1d) and an independent C (5d, Mon–Fri) that sets
/// the project finish to Fri 5 17:00. B may finish as late as Fri 17:00, so
/// A may also finish Fri 17:00: A's total slack is Wed + Thu + Fri = 3 days.
///
/// Why it matters: checks the backward rule "finish ≤ successor finish − lag".
#[test]
fn finish_to_finish_predecessor_slack() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    add_task(&mut project, "C", days(5));
    project
        .dependencies
        .push(Dependency::new(a, b, DependencyType::FinishToFinish));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).late_finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, a).total_slack, days(3));
}
