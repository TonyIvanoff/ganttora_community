//! # "Why this date?" tests: the explanation of every task's dates
//!
//! ## What this file checks
//! Every scheduled task carries an `Explanation`: the **driver** (the
//! reason) that decided its date, which end it fixed, the moment the reason
//! asked for (`driven_date`) and whether the calendar then moved that moment
//! (`moved_by_calendar`). These tests check:
//! - one example for each `Driver` variant (ProjectStart, ProjectFinish,
//!   Predecessor, Successor, Constraint, Deadline, ManualStart, Subtasks;
//!   some are in `alap.rs`, `manual_tasks.rs` and `summaries.rs` too);
//! - the **tie-breaking** rules, which make the answer deterministic: a
//!   specific reason beats the generic project start; between two links
//!   asking for the same moment, the link with the smaller UUID wins, no
//!   matter in which order the links are stored.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). The UI
//! shows the explanation in the "Why this date?" panel.
//!
//! ## Main ideas
//! Standard calendar, project start Monday 1 March 2027.

use crate::{dates_of, schedule_ok};
use engine::{Driver, TaskEnd};
use ganttora_tests::{add_task, days, dt, project_on, row_id, shed};
use model::{Constraint, Dependency, DependencyId, DependencyType, TaskMode};
use uuid::Uuid;

/// A lone task: nothing holds it back, so the driver is `ProjectStart`, the
/// driven date is Mon 1 08:00 and the calendar did not move it.
///
/// Why it matters: the simplest answer, "it starts with the project".
#[test]
fn project_start_driver() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let why = dates_of(&schedule_ok(&project), a).explanation;
    assert_eq!(why.driver, Driver::ProjectStart);
    assert_eq!(why.end, TaskEnd::Start);
    assert_eq!(why.driven_date, dt("2027-03-01 08:00"));
    assert!(!why.moved_by_calendar);
}

/// The shed's "Build the floor" (task 5) waits for "Prepare the ground"
/// (task 4), which finishes Thu 11 Mar 17:00 (later than "Buy materials",
/// Fri 5 Mar). The driver is the link 4 → 5 (FS), the driven date Thu 11
/// 17:00, and the calendar moved it to Fri 12 08:00.
///
/// Why it matters: this is the worked example of the explain module; the
/// user sees "waits for Prepare the ground".
#[test]
fn predecessor_driver_in_the_shed() {
    let project = shed();
    let ground = row_id(&project, 4);
    let floor = row_id(&project, 5);
    let link = project
        .dependencies
        .iter()
        .find(|d| d.predecessor == ground && d.successor == floor)
        .expect("link 4 -> 5")
        .id;
    let result = schedule_ok(&project);
    let why = dates_of(&result, floor).explanation;
    assert_eq!(
        why.driver,
        Driver::Predecessor {
            dependency: link,
            task: ground,
            kind: DependencyType::FinishToStart
        }
    );
    assert_eq!(why.driven_date, dt("2027-03-11 17:00"));
    assert!(why.moved_by_calendar);
    assert_eq!(dates_of(&result, floor).start, dt("2027-03-12 08:00"));
    assert_eq!(why.via_summary, None);
}

/// An FF link fixes the *finish*: B (1 day) FF after A (2 days) gets driver
/// Predecessor with `end = Finish` and driven date Tue 2 17:00.
///
/// Why it matters: the panel must say "its finish was decided" for FF/SF.
#[test]
fn finish_side_link_fixes_the_finish() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    project
        .dependencies
        .push(Dependency::new(a, b, DependencyType::FinishToFinish));
    let why = dates_of(&schedule_ok(&project), b).explanation;
    assert!(matches!(why.driver, Driver::Predecessor { task, .. } if task == a));
    assert_eq!(why.end, TaskEnd::Finish);
    assert_eq!(why.driven_date, dt("2027-03-02 17:00"));
}

/// A constraint driver: SNET Wed 3 08:00 gives `Driver::Constraint` carrying
/// the constraint itself.
///
/// Why it matters: the panel can show "Start no earlier than Wed 3 Mar".
#[test]
fn constraint_driver_carries_the_constraint() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let snet = Constraint::StartNoEarlierThan {
        date: dt("2027-03-03 08:00"),
    };
    project.tasks[0].constraint = snet;
    let why = dates_of(&schedule_ok(&project), a).explanation;
    assert_eq!(why.driver, Driver::Constraint { constraint: snet });
    assert!(!why.moved_by_calendar);
}

/// Tie: a constraint asking for exactly the project start (SNET Mon 1 08:00)
/// wins over the generic `ProjectStart`.
///
/// Why it matters: "starts no earlier than Monday" is more useful than
/// "starts with the project" when both are true.
#[test]
fn specific_reason_wins_a_tie_with_project_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.tasks[0].constraint = Constraint::StartNoEarlierThan {
        date: dt("2027-03-01 08:00"),
    };
    let why = dates_of(&schedule_ok(&project), a).explanation;
    assert!(matches!(why.driver, Driver::Constraint { .. }), "{why:?}");
}

/// Tie: an SS link from a task that starts at the project start asks for
/// the same moment as the project start; the link wins.
///
/// Why it matters: same rule for links ("starts with A" beats "starts with
/// the project").
#[test]
fn link_wins_a_tie_with_project_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    project
        .dependencies
        .push(Dependency::new(a, b, DependencyType::StartToStart));
    let why = dates_of(&schedule_ok(&project), b).explanation;
    assert!(matches!(why.driver, Driver::Predecessor { task, .. } if task == a));
}

/// Tie between two links: P1 and P2 (both 2 days) finish at the same moment,
/// Tue 2 17:00, and both link FS to S. The link with the **smaller UUID**
/// wins (here the link from P2, UUID …01), and the answer is the same when
/// the links are stored in the opposite order.
///
/// Why it matters: without a fixed rule the panel could name P1 today and P2
/// after a reload; the engine must be deterministic.
#[test]
fn smaller_link_uuid_wins_a_tie_between_links() {
    let small = DependencyId::from_uuid(Uuid::from_u128(1));
    let large = DependencyId::from_uuid(Uuid::from_u128(2));
    for reversed in [false, true] {
        let mut project = project_on("2027-03-01");
        let p1 = add_task(&mut project, "P1", days(2));
        let p2 = add_task(&mut project, "P2", days(2));
        let s = add_task(&mut project, "S", days(1));
        let mut from_p1 = Dependency::finish_to_start(p1, s);
        from_p1.id = large;
        let mut from_p2 = Dependency::finish_to_start(p2, s);
        from_p2.id = small;
        project.dependencies = if reversed {
            vec![from_p2, from_p1]
        } else {
            vec![from_p1, from_p2]
        };
        let why = dates_of(&schedule_ok(&project), s).explanation;
        assert_eq!(
            why.driver,
            Driver::Predecessor {
                dependency: small,
                task: p2,
                kind: DependencyType::FinishToStart
            },
            "links reversed: {reversed}"
        );
    }
}

/// Each `Driver` variant serializes as `{"type": "<camelCase name>", …}`,
/// the JSON the "Why this date?" panel reads.
///
/// Why it matters: the frontend switches on these exact type names.
#[test]
fn every_driver_variant_has_a_camel_case_json_tag() {
    let link = DependencyId::from_uuid(Uuid::from_u128(1));
    let task = model::TaskId::from_uuid(Uuid::from_u128(2));
    let cases = [
        (Driver::ProjectStart, "projectStart"),
        (Driver::ProjectFinish, "projectFinish"),
        (
            Driver::Predecessor {
                dependency: link,
                task,
                kind: DependencyType::StartToStart,
            },
            "predecessor",
        ),
        (
            Driver::Successor {
                dependency: link,
                task,
                kind: DependencyType::FinishToStart,
            },
            "successor",
        ),
        (
            Driver::Constraint {
                constraint: Constraint::AsLateAsPossible,
            },
            "constraint",
        ),
        (
            Driver::Deadline {
                date: dt("2027-03-05 17:00"),
            },
            "deadline",
        ),
        (Driver::ManualStart, "manualStart"),
        (Driver::Subtasks, "subtasks"),
    ];
    for (driver, tag) in cases {
        let value = serde_json::to_value(driver).expect("serialize");
        assert_eq!(value["type"], serde_json::json!(tag));
    }
    let predecessor = serde_json::to_value(cases[2].0).expect("serialize");
    assert_eq!(predecessor["kind"], serde_json::json!("startToStart"));
    assert_eq!(
        predecessor["dependency"],
        serde_json::json!(link.to_string())
    );
}

/// Every task of a mixed project gets an explanation whose `calendar_id` is
/// a calendar of the project, and manual tasks say `ManualStart`.
///
/// Why it matters: the panel must never show an empty or dangling answer.
#[test]
fn every_task_has_an_explanation_with_a_known_calendar() {
    let mut project = shed();
    project.tasks[6].mode = TaskMode::Manual;
    project.tasks[6].manual_start = Some(dt("2027-03-22 08:00"));
    let result = schedule_ok(&project);
    for schedule in &result.tasks {
        assert!(
            project
                .calendars
                .iter()
                .any(|c| c.id == schedule.explanation.calendar_id)
        );
    }
    assert_eq!(result.tasks[6].explanation.driver, Driver::ManualStart);
}
