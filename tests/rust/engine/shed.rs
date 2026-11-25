//! # The garden shed: the book's running example as tests
//!
//! ## What this file checks
//! The 8-task garden shed project from the project documentation. Section 2.4 works out
//! its schedule by hand: early and late dates, slack and the critical path.
//! These tests check that the engine gets exactly the same answers.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//!
//! ## Main ideas
//! - the project documentation counts in "working day numbers" (task 2 starts at the
//!   end of working day 2). Here the same results are written as real
//!   moments on the calendar, starting on Monday 1 March 2027, 08:00.
//! - Chapter 3 then changes task 2 from 5 to 6 days and shows what moves.

use crate::{dates_of, schedule_ok};
use engine::calendar_math::working_minutes_between;
use ganttora_tests::{days, dt, row_id, shed};
use model::{Duration, TaskId};

/// One expected row of the shed table in the project documentation.
struct Expected {
    /// Row number in the book (1 to 8).
    row: usize,
    /// Early start, early finish, late start, late finish.
    es: &'static str,
    ef: &'static str,
    ls: &'static str,
    lf: &'static str,
    /// Total and free slack in working days.
    total_days: i64,
    free_days: i64,
    /// Is the task on the critical path?
    critical: bool,
}

/// The full table of the project documentation, as calendar moments.
///
/// `#[rustfmt::skip]` asks the code formatter to leave this table alone, so
/// each task stays on one line and the table reads like the one in the book.
#[rustfmt::skip]
const SHED_TABLE: [Expected; 8] = [
    Expected { row: 1, es: "2027-03-01 08:00", ef: "2027-03-02 17:00", ls: "2027-03-01 08:00", lf: "2027-03-02 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 2, es: "2027-03-03 08:00", ef: "2027-03-09 17:00", ls: "2027-03-03 08:00", lf: "2027-03-09 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 3, es: "2027-03-03 08:00", ef: "2027-03-05 17:00", ls: "2027-03-09 08:00", lf: "2027-03-11 17:00", total_days: 4, free_days: 4, critical: false },
    Expected { row: 4, es: "2027-03-10 08:00", ef: "2027-03-11 17:00", ls: "2027-03-10 08:00", lf: "2027-03-11 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 5, es: "2027-03-12 08:00", ef: "2027-03-15 17:00", ls: "2027-03-12 08:00", lf: "2027-03-15 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 6, es: "2027-03-16 08:00", ef: "2027-03-19 17:00", ls: "2027-03-16 08:00", lf: "2027-03-19 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 7, es: "2027-03-22 08:00", ef: "2027-03-23 17:00", ls: "2027-03-22 08:00", lf: "2027-03-23 17:00", total_days: 0, free_days: 0, critical: true },
    Expected { row: 8, es: "2027-03-23 17:00", ef: "2027-03-23 17:00", ls: "2027-03-23 17:00", lf: "2027-03-23 17:00", total_days: 0, free_days: 0, critical: true },
];

/// Every one of the 8 shed tasks gets exactly the early start, early
/// finish, late start, late finish, total slack, free slack and critical
/// flag worked out by hand in the project documentation.
///
/// Why it matters: this is the book's promise that the engine calculates
/// like a person with pencil and paper, only faster. If any of the 56 values
/// differ, either the engine or the book is wrong.
#[test]
fn shed_every_task_matches_the_book_table() {
    let project = shed();
    let result = schedule_ok(&project);

    for expected in &SHED_TABLE {
        let task = dates_of(&result, row_id(&project, expected.row));
        // The extra text after each assert is printed only if it fails, so
        // we immediately know which row and which column is wrong.
        let row = expected.row;
        assert_eq!(task.early_start, dt(expected.es), "row {row}: early start");
        assert_eq!(
            task.early_finish,
            dt(expected.ef),
            "row {row}: early finish"
        );
        assert_eq!(task.late_start, dt(expected.ls), "row {row}: late start");
        assert_eq!(task.late_finish, dt(expected.lf), "row {row}: late finish");
        assert_eq!(
            task.total_slack,
            days(expected.total_days),
            "row {row}: total slack"
        );
        assert_eq!(
            task.free_slack,
            days(expected.free_days),
            "row {row}: free slack"
        );
        assert_eq!(task.is_critical, expected.critical, "row {row}: critical");
    }
}

/// The critical path of the shed is 1 → 2 → 4 → 5 → 6 → 7 → 8 (path A in
/// the book); only task 3, "Buy materials", is not critical.
///
/// Why it matters: the critical path tells the user which tasks they must
/// watch most closely. It is the headline result of the whole calculation.
#[test]
fn shed_critical_path_is_1_2_4_5_6_7_8() {
    let project = shed();
    let result = schedule_ok(&project);
    let expected: Vec<TaskId> = [1, 2, 4, 5, 6, 7, 8]
        .iter()
        .map(|&row| row_id(&project, row))
        .collect();
    assert_eq!(result.critical_tasks(), expected);
}

/// The shed runs from Monday 1 March 2027 08:00 to Tuesday 23 March 2027
/// 17:00, which is 17 working days.
///
/// Why it matters: "when will it be finished?" is the first question every
/// project owner asks. The 17 days are the length of path A in the book.
#[test]
fn shed_takes_17_working_days_from_mon_1_mar_to_tue_23_mar() {
    let project = shed();
    let result = schedule_ok(&project);

    assert_eq!(result.project_start, dt("2027-03-01 08:00"));
    assert_eq!(result.project_finish, dt("2027-03-23 17:00"));
    let working = working_minutes_between(
        &project.calendars[0],
        result.project_start,
        result.project_finish,
    );
    assert_eq!(working, days(17));
}

/// Chapter 3's example edit: task 2 "Get building permit" grows from 5 to 6
/// days. The permit now finishes Wed 10 Mar, the project finishes Wed 24 Mar
/// 2027 (18 working days), task 3 gains one day of slack (5 days), and the
/// critical path stays the same.
///
/// Why it matters: this is exactly the scenario the project documentation uses to show
/// how an edit flows from the UI to the engine and back. A longer critical
/// task must push every task after it.
#[test]
fn shed_with_six_day_permit_finishes_wed_24_mar() {
    let mut project = shed();
    project.tasks[1].duration = days(6);
    let result = schedule_ok(&project);

    let permit = dates_of(&result, row_id(&project, 2));
    assert_eq!(permit.early_finish, dt("2027-03-10 17:00"));
    assert_eq!(result.project_finish, dt("2027-03-24 17:00"));
    let working = working_minutes_between(
        &project.calendars[0],
        result.project_start,
        result.project_finish,
    );
    assert_eq!(working, days(18));

    let materials = dates_of(&result, row_id(&project, 3));
    assert_eq!(materials.total_slack, days(5));
    assert!(!materials.is_critical);

    let expected: Vec<TaskId> = [1, 2, 4, 5, 6, 7, 8]
        .iter()
        .map(|&row| row_id(&project, row))
        .collect();
    assert_eq!(result.critical_tasks(), expected);
}

/// If "Buy materials" (task 3) takes 8 days instead of 3, path B
/// (1 → 3 → 5 → 6 → 7 → 8, now 18 days) becomes longer than path A
/// (17 days). The critical path switches to path B, and tasks 2 and 4 get
/// one day of slack.
///
/// Why it matters: the critical path is not fixed. It must be recalculated
/// after every edit, and a task that had slack can become critical.
#[test]
fn shed_critical_path_moves_when_materials_take_longer() {
    let mut project = shed();
    project.tasks[2].duration = days(8);
    let result = schedule_ok(&project);

    let expected: Vec<TaskId> = [1, 3, 5, 6, 7, 8]
        .iter()
        .map(|&row| row_id(&project, row))
        .collect();
    assert_eq!(result.critical_tasks(), expected);
    assert_eq!(dates_of(&result, row_id(&project, 2)).total_slack, days(1));
    assert_eq!(dates_of(&result, row_id(&project, 4)).total_slack, days(1));
    assert_eq!(result.project_finish, dt("2027-03-24 17:00"));
}

/// For every shed task: finish is not before start, late dates are not
/// before early dates, free slack is never more than total slack, and a
/// task is critical exactly when its total slack is zero.
///
/// Why it matters: these are rules that must hold for *any* schedule, not
/// only this one. Checking them here catches mistakes the table might miss.
#[test]
fn shed_results_obey_the_general_rules() {
    let project = shed();
    let result = schedule_ok(&project);
    for task in &result.tasks {
        assert!(task.early_finish >= task.early_start);
        assert!(task.late_finish >= task.late_start);
        assert!(task.late_start >= task.early_start);
        assert!(task.late_finish >= task.early_finish);
        assert!(task.free_slack <= task.total_slack);
        assert!(task.free_slack >= Duration::ZERO);
        assert_eq!(task.is_critical, task.total_slack <= Duration::ZERO);
        assert!(task.early_finish <= result.project_finish);
    }
}
