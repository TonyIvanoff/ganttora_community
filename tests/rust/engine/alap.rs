//! # "As Late As Possible" (ALAP) and schedule-from-finish tests
//!
//! ## What this file checks
//! - An **ALAP** task is placed as late as it can go without moving any
//!   other task: just before its successor needs it, or at the project
//!   finish. Its *early* dates stay the earliest possible ones.
//! - A project **scheduled from its finish** fixes the finish date and
//!   calculates backward: every task first gets its *late* dates; then ASAP
//!   tasks are moved as early as their predecessors allow.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). Book
//! section 2.3 explains constraints; scheduling from the finish is the project documentation
//! M1.
//!
//! ## Main ideas
//! Standard calendar. The garden shed finishes Tue 23 Mar
//! 2027 17:00; "Buy materials" (task 3) has 4 days of slack there.

use crate::{dates_of, schedule_ok};
use engine::{Driver, ScheduleError, TaskEnd, schedule};
use ganttora_tests::{add_task, days, dt, link, project_on, row_id, shed};
use model::{Constraint, DependencyType, ScheduleFrom};

/// "Buy materials" (3 days) set to ALAP in the shed: its successor "Build
/// the floor" starts Fri 12 Mar 08:00, so it must finish by Thu 11 17:00 and
/// starts 3 working days earlier, Tue 9 08:00. Its early start stays Wed 3
/// 08:00. "Why this date?": the successor link (task 5, FS) fixed the
/// finish.
///
/// Why it matters: "buy as late as possible" keeps materials from waiting in
/// the rain, without delaying anything.
#[test]
fn alap_task_moves_up_to_its_successor() {
    let mut project = shed();
    project.tasks[2].constraint = Constraint::AsLateAsPossible;
    let buy = row_id(&project, 3);
    let floor = row_id(&project, 5);
    let result = schedule_ok(&project);
    let buy_dates = dates_of(&result, buy);
    assert_eq!(buy_dates.start, dt("2027-03-09 08:00"));
    assert_eq!(buy_dates.finish, dt("2027-03-11 17:00"));
    assert_eq!(buy_dates.early_start, dt("2027-03-03 08:00"));
    assert_eq!(result.project_finish, dt("2027-03-23 17:00"));
    assert!(matches!(
        buy_dates.explanation.driver,
        Driver::Successor { task, kind: DependencyType::FinishToStart, .. } if task == floor
    ));
    assert_eq!(buy_dates.explanation.end, TaskEnd::Finish);
}

/// An ALAP task with no successor moves to the project finish: A (5 days,
/// Mon 1 – Fri 5) sets the finish; B (2 days, ALAP) runs Thu 4 08:00 – Fri
/// 5 17:00. Driver: `ProjectFinish`.
///
/// Why it matters: without successors, "as late as possible" means "end
/// with the project".
#[test]
fn alap_task_without_successor_ends_with_the_project() {
    let mut project = project_on("2027-03-01");
    add_task(&mut project, "A", days(5));
    let b = add_task(&mut project, "B", days(2));
    project.tasks[1].constraint = Constraint::AsLateAsPossible;
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-05 17:00"));
    assert_eq!(
        dates_of(&result, b).explanation.driver,
        Driver::ProjectFinish
    );
}

/// An ALAP task never moves the project finish or its successors: the shed
/// with "Buy materials" ALAP has exactly the same dates for every other
/// task as the plain shed.
///
/// Why it matters: ALAP must only use slack, never create delay.
#[test]
fn alap_does_not_move_other_tasks() {
    let plain = schedule_ok(&shed());
    let mut project = shed();
    project.tasks[2].constraint = Constraint::AsLateAsPossible;
    let alap = schedule_ok(&project);
    for (row, (a, b)) in plain.tasks.iter().zip(&alap.tasks).enumerate() {
        if row != 2 {
            assert_eq!((a.start, a.finish), (b.start, b.finish), "row {row}");
        }
    }
}

/// Scheduling from the finish: finish Fri 5 Mar 17:00; A (2d) → B (1d).
/// Backward: B Fri 5 08:00–17:00; A must finish before Fri 08:00 → Thu 4
/// 17:00, so A runs Wed 3 08:00 – Thu 4 17:00. The project start is the
/// earliest task start, Wed 3 08:00. Both tasks are critical.
///
/// Why it matters: "the shed must be ready on Friday; when do we start?" is
/// what scheduling from the finish answers.
#[test]
fn schedule_from_finish_works_backward() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = Some(dt("2027-03-05 17:00"));
    let result = schedule_ok(&project);
    assert_eq!(result.project_finish, dt("2027-03-05 17:00"));
    assert_eq!(result.project_start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, b).start, dt("2027-03-05 08:00"));
    assert_eq!(dates_of(&result, a).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-04 17:00"));
    assert!(dates_of(&result, a).is_critical && dates_of(&result, b).is_critical);
    assert_eq!(
        dates_of(&result, b).explanation.driver,
        Driver::ProjectFinish
    );
    assert!(matches!(
        dates_of(&result, a).explanation.driver,
        Driver::Successor { task, .. } if task == b
    ));
}

/// A finish given outside working time (Sat 6 Mar 00:00) is moved back to
/// the last working moment, Fri 5 17:00.
///
/// Why it matters: users type "Saturday" meaning "by the weekend".
#[test]
fn schedule_from_finish_snaps_the_finish_back() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = Some(dt("2027-03-06 00:00"));
    let result = schedule_ok(&project);
    assert_eq!(result.project_finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-05 17:00"));
}

/// Scheduling from the finish, an independent ASAP task is moved as early
/// as possible: finish Fri 5 17:00, A (2d) → B (1d) as above, plus C (1d)
/// without links. Backward, C would end Fri 17:00; as an ASAP task it moves
/// to the project start, Wed 3 08:00 – 17:00. Driver: `ProjectStart`.
///
/// Why it matters: this is the mirror image of ALAP (documented engine
/// rule; to compare with MS Project later).
#[test]
fn schedule_from_finish_moves_asap_tasks_early() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    let c = add_task(&mut project, "C", days(1));
    link(&mut project, a, b);
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = Some(dt("2027-03-05 17:00"));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, c).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, c).finish, dt("2027-03-03 17:00"));
    assert_eq!(
        dates_of(&result, c).explanation.driver,
        Driver::ProjectStart
    );
}

/// Scheduling from the finish, a deadline earlier than the finish decides
/// the task's late dates: finish Fri 5 17:00, A (2d) → B (1d), deadline on B
/// Thu 4 17:00. B runs Thu 4 08:00–17:00 (driver `Deadline`), A runs Tue 2
/// 08:00 – Wed 3 17:00.
///
/// Why it matters: in a backward schedule the deadline is a real limit; it
/// is the only place `Driver::Deadline` appears.
#[test]
fn schedule_from_finish_honours_a_deadline() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.tasks[1].deadline = Some(dt("2027-03-04 17:00"));
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = Some(dt("2027-03-05 17:00"));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, a).start, dt("2027-03-02 08:00"));
    assert_eq!(
        dates_of(&result, b).explanation.driver,
        Driver::Deadline {
            date: dt("2027-03-04 17:00")
        }
    );
}

/// Scheduling from the finish needs a finish date; without one the engine
/// returns `ProjectFinishMissing` (also checked in `errors.rs`), even for
/// an empty project.
///
/// Why it matters: the error must not depend on whether there are tasks.
#[test]
fn schedule_from_finish_without_date_fails_even_when_empty() {
    let mut project = project_on("2027-03-01");
    project.schedule_from = ScheduleFrom::Finish;
    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::ProjectFinishMissing
    );
}
