//! # Constraint and deadline tests
//!
//! ## What this file checks
//! A *constraint* is a date rule on one task: "start no earlier than 15
//! March" (SNET), "must finish on Friday" (MFO) and so on. A *deadline* is a
//! target finish that does not move the task but shows **negative slack**
//! when it is missed. The rules follow MS Project's default "tasks always
//! honour their constraint dates":
//!
//! | Constraint | Effect on the task dates |
//! |---|---|
//! | ASAP / ALAP | as early / as late as possible (ALAP: see `alap.rs`) |
//! | SNET / FNET | start / finish not before the date |
//! | SNLT / FNLT | start / finish not after the date, even against links |
//! | MSO / MFO | start / finish exactly on the date |
//!
//! When a constraint wins over a link, the predecessor gets negative slack.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). Book
//! section 2.3 explains constraints and deadlines.
//!
//! ## Main ideas
//! Standard calendar, project start Monday 1 March 2027. Dates worked out by
//! hand in each test's comment.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, days, dt, link, project_on};
use model::{Constraint, Duration};

/// SNET Wed 3 Mar 08:00 on a lone 2-day task: it starts Wed 3 08:00 instead
/// of Mon 1, and finishes Thu 4 17:00.
///
/// Why it matters: "the crane is only available from Wednesday".
#[test]
fn start_no_earlier_than_delays_the_start() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].constraint = Constraint::StartNoEarlierThan {
        date: dt("2027-03-03 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-04 17:00"));
}

/// SNET on a Saturday (Sat 6 Mar 10:00): the start moves to the next working
/// moment, Mon 8 Mar 08:00.
///
/// Why it matters: a constraint date outside working time must still give a
/// working start.
#[test]
fn start_no_earlier_than_on_weekend_moves_to_monday() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.tasks[0].constraint = Constraint::StartNoEarlierThan {
        date: dt("2027-03-06 10:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-08 08:00"));
}

/// SNET earlier than what the links allow has no effect: the predecessor
/// finishes Wed 3 17:00 (3 days), so B starts Thu 4 08:00 even with SNET
/// Mon 1.
///
/// Why it matters: "no earlier than" is a lower limit, not a fixed date.
#[test]
fn start_no_earlier_than_loses_against_a_later_link() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.tasks[1].constraint = Constraint::StartNoEarlierThan {
        date: dt("2027-03-01 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-04 08:00"));
}

/// FNET Fri 5 Mar 17:00 on a 2-day task: its finish may not be earlier, so
/// it starts 2 working days before: Thu 4 08:00.
///
/// Why it matters: a finish rule is turned into a start by counting the
/// duration backward.
#[test]
fn finish_no_earlier_than_delays_the_task() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].constraint = Constraint::FinishNoEarlierThan {
        date: dt("2027-03-05 17:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-05 17:00"));
}

/// MSO Wed 3 08:00 on B, whose predecessor A (5 days) finishes Fri 5 17:00.
/// The constraint wins: B starts Wed 3. Backward: A must finish before B's
/// start, Wed 3 08:00 → Tue 2 17:00, so A's late start is 5 working days
/// earlier, Wed 24 Feb 08:00. A's total slack = Tue 2 17:00 − Fri 5 17:00 =
/// −3 days: A is already 3 days late.
///
/// Why it matters: "must start on" beats links, and the conflict is shown as
/// negative slack, like in MS Project.
#[test]
fn must_start_on_wins_over_links_with_negative_slack() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(5));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.tasks[1].constraint = Constraint::MustStartOn {
        date: dt("2027-03-03 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, a).late_finish, dt("2027-03-02 17:00"));
    assert_eq!(dates_of(&result, a).late_start, dt("2027-02-24 08:00"));
    assert_eq!(dates_of(&result, a).total_slack, days(-3));
    assert!(
        dates_of(&result, a).is_critical,
        "negative slack is critical"
    );
}

/// MSO later than the links: B must start Mon 8 Mar 08:00 although A
/// finishes Tue 2 17:00. B starts Mon 8.
///
/// Why it matters: "must" means exactly that date, also when it is later.
#[test]
fn must_start_on_also_delays() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.tasks[1].constraint = Constraint::MustStartOn {
        date: dt("2027-03-08 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-08 08:00"));
}

/// MFO Fri 5 17:00 on a 2-day task: finish exactly Fri 17:00, start Thu 4
/// 08:00.
///
/// Why it matters: the finish-side twin of MSO.
#[test]
fn must_finish_on_fixes_the_finish() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].constraint = Constraint::MustFinishOn {
        date: dt("2027-03-05 17:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-05 17:00"));
}

/// SNLT Tue 2 08:00 on B, whose predecessor A (3 days) finishes Wed 3 17:00.
/// The constraint is honoured: B starts Tue 2 08:00 and (2 days) finishes
/// Wed 3 17:00, which is also the project finish. Backward: B's late start
/// is Tue 08:00, so A must finish by Mon 1 17:00. A's early finish is Wed 3
/// 17:00: total slack = −(Tue + Wed) = −2 days.
///
/// Why it matters: "start no later than" wins over links in the pass that
/// decides the dates (MS Project default), and the conflict shows up as
/// negative slack on the predecessor.
#[test]
fn start_no_later_than_wins_over_links_with_negative_slack() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    let b = add_task(&mut project, "B", days(2));
    link(&mut project, a, b);
    project.tasks[1].constraint = Constraint::StartNoLaterThan {
        date: dt("2027-03-02 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, a).late_finish, dt("2027-03-01 17:00"));
    assert_eq!(dates_of(&result, a).total_slack, days(-2));
}

/// SNLT later than the natural start has no effect on an ASAP task: a lone
/// task with SNLT Fri 5 Mar still starts Mon 1.
///
/// Why it matters: "no later than" is an upper limit only.
#[test]
fn start_no_later_than_without_conflict_changes_nothing() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    project.tasks[0].constraint = Constraint::StartNoLaterThan {
        date: dt("2027-03-05 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).start, dt("2027-03-01 08:00"));
}

/// FNLT Thu 4 17:00 on B (2 days), whose predecessor A (3 days) finishes Wed
/// 3 17:00. The links would give Thu 4 – Fri 5; the constraint pulls B back
/// to finish Thu 17:00, so B runs Wed 3 08:00 – Thu 4 17:00. A must finish
/// before Wed 08:00 → Tue 2 17:00; its early finish is Wed 17:00: slack −1
/// day.
///
/// Why it matters: the finish-side twin of SNLT.
#[test]
fn finish_no_later_than_wins_over_links_with_negative_slack() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    let b = add_task(&mut project, "B", days(2));
    link(&mut project, a, b);
    project.tasks[1].constraint = Constraint::FinishNoLaterThan {
        date: dt("2027-03-04 17:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-03 08:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-04 17:00"));
    assert_eq!(dates_of(&result, a).total_slack, days(-1));
}

/// A missed deadline: a 2-day task with deadline Mon 1 17:00 still runs Mon
/// 1 – Tue 2 17:00 (a deadline never moves the task), but its late finish
/// is the deadline, its total slack is −1 day and `missed_deadline` is set.
///
/// Why it matters: deadlines warn without rescheduling, exactly like MS
/// Project's red deadline indicator.
#[test]
fn missed_deadline_gives_negative_slack_but_does_not_move_the_task() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].deadline = Some(dt("2027-03-01 17:00"));
    let result = schedule_ok(&project);
    let a = dates_of(&result, a);
    assert_eq!(a.start, dt("2027-03-01 08:00"));
    assert_eq!(a.finish, dt("2027-03-02 17:00"));
    assert_eq!(a.late_finish, dt("2027-03-01 17:00"));
    assert_eq!(a.total_slack, days(-1));
    assert!(a.missed_deadline);
    assert!(a.is_critical);
}

/// A deadline that is met changes nothing: deadline Fri 5 17:00 on a task
/// that finishes Tue 2 17:00; slack stays 0 (it is the only task, so the
/// project finish is the earlier limit) and `missed_deadline` is false.
///
/// Why it matters: a deadline only matters when it is earlier than the other
/// limits.
#[test]
fn met_deadline_changes_nothing() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].deadline = Some(dt("2027-03-05 17:00"));
    let result = schedule_ok(&project);
    assert!(!dates_of(&result, a).missed_deadline);
    assert_eq!(dates_of(&result, a).total_slack, Duration::ZERO);
}

/// A deadline on a successor makes its predecessors late too: A (2d) → B
/// (2d), deadline on B Wed 3 17:00. B finishes Thu 4 17:00, one day late.
/// B's late finish = Wed 17:00 → B's late start Tue 2 08:00 → A's late
/// finish Mon 1 17:00: A also has −1 day of slack.
///
/// Why it matters: negative slack spreads backward along the chain, so the
/// user sees every task that has to speed up.
#[test]
fn deadline_spreads_negative_slack_to_predecessors() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(2));
    link(&mut project, a, b);
    project.tasks[1].deadline = Some(dt("2027-03-03 17:00"));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).total_slack, days(-1));
    assert_eq!(dates_of(&result, a).total_slack, days(-1));
    assert!(!dates_of(&result, a).missed_deadline, "A has no deadline");
}
