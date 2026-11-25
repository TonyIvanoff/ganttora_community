//! # Blockers: unplanned delays that make a task longer
//!
//! ## What this file checks
//! A **blocker** says "after X of the task's work, the task was stuck for
//! Y". The engine must:
//! - make the task longer by the blocked time (its *span* = work + blocked),
//!   in **both** passes, so early/late dates, slack and the critical path
//!   all include the blocked time;
//! - cut the bar into **segments** (work, blocked, work …) in time order;
//! - clamp `after_work` to `0..=duration`, sort several blockers by
//!   `after_work` (ties by blocker ID), skip zero-length blockers, count
//!   negative values as zero and ignore blockers on summary tasks;
//! - work for milestones, manual tasks, ALAP tasks, constraints and
//!   projects scheduled from the finish;
//! - tell "Why this date?" how much blocked time pushed the finish
//!   (`Explanation::blocked`);
//! - keep a **percentage lag** a share of the *planned* work, not of the
//!   work plus blocked time.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). The
//! engine code is `backend/crates/engine/src/blockers.rs`; the data type is
//! `backend/crates/model/src/blocker.rs`.
//!
//! ## Main ideas
//! Standard calendar: Monday to Friday, 08:00–12:00 and 13:00–17:00, so
//! one day = 480 working minutes. Mon 1 Mar 2027 is a Monday. The garden
//! shed finishes Tue 23 Mar 2027 17:00; "Buy materials"
//! (row 3) has 4 days of slack there, "Get building permit" (row 2) none.

use crate::{dates_of, schedule_ok};
use engine::{Segment, SegmentKind};
use ganttora_tests::{add_task, days, dt, link, project_on, row_id, shed};
use model::{
    Blocker, BlockerId, Constraint, Dependency, DependencyType, Duration, ScheduleFrom, TaskMode,
};
use uuid::Uuid;

/// A blocker with a fixed, readable ID (`n` becomes the UUID `0…0n`).
///
/// # Parameters
/// - `n`: a small number for the ID, so tests can compare IDs and order.
/// - `after_work`: how much work is done before the blocker starts.
/// - `duration`: how long the task is blocked.
///
/// # Returns
/// The blocker, with the reason "test" and no "reported on" day.
fn blocker(n: u128, after_work: Duration, duration: Duration) -> Blocker {
    Blocker::new(
        BlockerId::from_uuid(Uuid::from_u128(n)),
        after_work,
        duration,
        "test",
    )
}

/// The blocker ID made from the number `n` (see [`blocker`]).
fn bid(n: u128) -> BlockerId {
    BlockerId::from_uuid(Uuid::from_u128(n))
}

/// A short, readable description of segments: `(start, finish, kind)`
/// where kind is `"work"` or `"blocked"`. Comparing these lists gives a
/// clear message when a test fails.
fn pieces(segments: &[Segment]) -> Vec<(String, String, &'static str)> {
    segments
        .iter()
        .map(|s| {
            let kind = match s.kind {
                SegmentKind::Work => "work",
                SegmentKind::Blocked { .. } => "blocked",
            };
            (
                s.start.format("%a %d %H:%M").to_string(),
                s.finish.format("%a %d %H:%M").to_string(),
                kind,
            )
        })
        .collect()
}

/// Builds an expected piece list in the same form as [`pieces`].
fn expect(list: &[(&str, &str, &'static str)]) -> Vec<(String, String, &'static str)> {
    list.iter()
        .map(|(a, b, k)| (a.to_string(), b.to_string(), *k))
        .collect()
}

/// The worked example from the engine's notes: "Buy materials" (3 days)
/// gets a 5-day blocker after 2 days of work. Its bar becomes work Wed 3 –
/// Thu 4, blocked Fri 5 – Thu 11, work Fri 12; it finishes Fri 12 17:00.
/// It had 4 days of slack, so the shed moves by one working day: Tue 23 →
/// Wed 24 Mar. The task is now critical; its planned duration is still 3
/// days; "Why this date?" reports 5 days of blocked time.
///
/// Why it matters: this is the user story behind blockers ("the wood is a
/// week late"); every number here is shown to the user.
#[test]
fn shed_buy_materials_with_a_late_delivery() {
    let mut project = shed();
    assert_eq!(schedule_ok(&project).project_finish, dt("2027-03-23 17:00"));
    project.tasks[2].blockers.push(blocker(1, days(2), days(5)));
    let buy = row_id(&project, 3);
    let floor = row_id(&project, 5);
    let result = schedule_ok(&project);
    let buy_dates = dates_of(&result, buy);

    assert_eq!(buy_dates.start, dt("2027-03-03 08:00"));
    assert_eq!(buy_dates.finish, dt("2027-03-12 17:00"));
    assert_eq!(buy_dates.duration, days(3), "planned work is unchanged");
    assert_eq!(buy_dates.blocked, days(5));
    assert_eq!(buy_dates.explanation.blocked, days(5));
    assert!(buy_dates.is_critical, "its 4 days of slack are used up");
    assert_eq!(buy_dates.total_slack, Duration::ZERO);
    assert_eq!(
        pieces(&buy_dates.segments),
        expect(&[
            ("Wed 03 08:00", "Thu 04 17:00", "work"),
            ("Fri 05 08:00", "Thu 11 17:00", "blocked"),
            ("Fri 12 08:00", "Fri 12 17:00", "work"),
        ])
    );
    assert_eq!(
        buy_dates.segments[1].kind,
        SegmentKind::Blocked { blocker: bid(1) }
    );
    assert_eq!(dates_of(&result, floor).start, dt("2027-03-15 08:00"));
    assert_eq!(result.project_finish, dt("2027-03-24 17:00"));
}

/// Tasks without blockers have no segments and zero blocked time, also
/// in a project where another task has a blocker.
///
/// Why it matters: the UI draws pieces only when there are some; an empty
/// list keeps every old project exactly as it was.
#[test]
fn tasks_without_blockers_have_no_segments() {
    let mut project = shed();
    project.tasks[2].blockers.push(blocker(1, days(2), days(5)));
    let result = schedule_ok(&project);
    for row in [1, 2, 4, 5, 6, 7, 8] {
        let dates = dates_of(&result, row_id(&project, row));
        assert!(dates.segments.is_empty(), "row {row}");
        assert_eq!(dates.blocked, Duration::ZERO, "row {row}");
        assert_eq!(dates.explanation.blocked, Duration::ZERO, "row {row}");
    }
}

/// A blocker on a **critical** task moves the project finish by its full
/// length: "Get building permit" (no slack) blocked 3 days after 1 day of
/// work → the shed finishes Fri 26 Mar instead of Tue 23 Mar.
///
/// Why it matters: on the critical path, every blocked day is a day of
/// project delay. This is what the user needs to see first.
#[test]
fn blocker_on_a_critical_task_moves_the_finish_by_its_full_length() {
    let mut project = shed();
    project.tasks[1].blockers.push(blocker(1, days(1), days(3)));
    let result = schedule_ok(&project);
    let permit = dates_of(&result, row_id(&project, 2));
    assert_eq!(permit.start, dt("2027-03-03 08:00"));
    assert_eq!(permit.finish, dt("2027-03-12 17:00"));
    assert!(permit.is_critical);
    assert_eq!(result.project_finish, dt("2027-03-26 17:00"));
}

/// `after_work` larger than the duration is clamped to the duration: the
/// blocker comes after all the work and just extends the end. A 3-day task
/// with a 2-day blocker "after 10 days" gives work Mon 1 – Wed 3, blocked
/// Thu 4 – Fri 5, exactly like "after 3 days".
///
/// Why it matters: a user may record a delay after the planned work was
/// already "used up"; it must still count, at the end.
#[test]
fn after_work_past_the_duration_extends_the_end() {
    for after in [days(3), days(10)] {
        let mut project = project_on("2027-03-01");
        let a = add_task(&mut project, "A", days(3));
        project.tasks[0].blockers.push(blocker(1, after, days(2)));
        let result = schedule_ok(&project);
        let dates = dates_of(&result, a);
        assert_eq!(dates.finish, dt("2027-03-05 17:00"), "after {after:?}");
        assert_eq!(
            pieces(&dates.segments),
            expect(&[
                ("Mon 01 08:00", "Wed 03 17:00", "work"),
                ("Thu 04 08:00", "Fri 05 17:00", "blocked"),
            ]),
            "after {after:?}"
        );
    }
}

/// `after_work` of zero (and a negative one, which counts as zero) puts the
/// blocked time **before** any work: blocked Mon 1 – Tue 2, work Wed 3 –
/// Fri 5. The task still starts Mon 1 08:00 (its bar begins with the
/// blocked piece).
///
/// Why it matters: "we could not even start because the site was flooded"
/// is a common delay; a bad negative value must not crash or shorten.
#[test]
fn after_work_zero_or_negative_blocks_before_the_work() {
    for after in [Duration::ZERO, days(-1)] {
        let mut project = project_on("2027-03-01");
        let a = add_task(&mut project, "A", days(3));
        project.tasks[0].blockers.push(blocker(1, after, days(2)));
        let result = schedule_ok(&project);
        let dates = dates_of(&result, a);
        assert_eq!(dates.start, dt("2027-03-01 08:00"));
        assert_eq!(dates.finish, dt("2027-03-05 17:00"));
        assert_eq!(
            pieces(&dates.segments),
            expect(&[
                ("Mon 01 08:00", "Tue 02 17:00", "blocked"),
                ("Wed 03 08:00", "Fri 05 17:00", "work"),
            ]),
            "after {after:?}"
        );
    }
}

/// Several blockers are applied in order of `after_work`, whatever order
/// they are stored in: stored [after 2d (ID 2), after 1d (ID 1)] on a 3-day
/// task gives work Mon, blocked Tue (ID 1), work Wed, blocked Thu (ID 2),
/// work Fri.
///
/// Why it matters: the list order in the file is an accident; the bar must
/// follow the work.
#[test]
fn several_blockers_are_sorted_by_after_work() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    project.tasks[0].blockers = vec![blocker(2, days(2), days(1)), blocker(1, days(1), days(1))];
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert_eq!(dates.blocked, days(2));
    assert_eq!(dates.finish, dt("2027-03-05 17:00"));
    assert_eq!(
        pieces(&dates.segments),
        expect(&[
            ("Mon 01 08:00", "Mon 01 17:00", "work"),
            ("Tue 02 08:00", "Tue 02 17:00", "blocked"),
            ("Wed 03 08:00", "Wed 03 17:00", "work"),
            ("Thu 04 08:00", "Thu 04 17:00", "blocked"),
            ("Fri 05 08:00", "Fri 05 17:00", "work"),
        ])
    );
    assert_eq!(
        dates.segments[1].kind,
        SegmentKind::Blocked { blocker: bid(1) }
    );
    assert_eq!(
        dates.segments[3].kind,
        SegmentKind::Blocked { blocker: bid(2) }
    );
}

/// Two blockers at the same `after_work` are ordered by ID, so storing them
/// in either order gives exactly the same result (no work piece between
/// them, because no work happens in between).
///
/// Why it matters: the engine is deterministic: the same
/// project must always give the same bar.
#[test]
fn blockers_at_the_same_position_are_ordered_by_id() {
    let build = |reverse: bool| {
        let mut project = project_on("2027-03-01");
        add_task(&mut project, "A", days(2));
        let mut list = vec![blocker(1, days(1), days(1)), blocker(2, days(1), days(1))];
        if reverse {
            list.reverse();
        }
        project.tasks[0].blockers = list;
        project
    };
    let first = build(false);
    let second = build(true);
    let one = schedule_ok(&first);
    let two = schedule_ok(&second);
    let a = dates_of(&one, first.tasks[0].id);
    let b = dates_of(&two, second.tasks[0].id);
    assert_eq!(a.segments, b.segments);
    let kinds: Vec<SegmentKind> = a.segments.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SegmentKind::Work,
            SegmentKind::Blocked { blocker: bid(1) },
            SegmentKind::Blocked { blocker: bid(2) },
            SegmentKind::Work,
        ]
    );
    assert_eq!(a.finish, dt("2027-03-04 17:00"));
}

/// A blocker of zero length changes nothing and makes no piece. With only
/// zero-length blockers the task has no segments at all.
///
/// Why it matters: an empty piece would draw a zero-width stripe and could
/// confuse hit-testing in the Gantt chart.
#[test]
fn zero_length_blockers_are_skipped() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0]
        .blockers
        .push(blocker(1, days(1), Duration::ZERO));
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert!(dates.segments.is_empty());
    assert_eq!(dates.blocked, Duration::ZERO);
    assert_eq!(dates.finish, dt("2027-03-02 17:00"));

    // Together with a real blocker, the zero-length one is simply absent.
    project.tasks[0].blockers.push(blocker(2, days(1), days(1)));
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert_eq!(dates.segments.len(), 3);
    assert!(
        dates
            .segments
            .iter()
            .all(|s| s.kind != SegmentKind::Blocked { blocker: bid(1) })
    );
}

/// A negative blocker duration (which the document never lets in, but a
/// damaged file might contain) counts as zero: no extra time, no pieces.
///
/// Why it matters: a negative delay must never make a task *shorter* than
/// its planned work.
#[test]
fn negative_blocker_duration_counts_as_zero() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    project.tasks[0]
        .blockers
        .push(blocker(1, days(1), days(-2)));
    assert_eq!(project.tasks[0].blocked_time(), Duration::ZERO);
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert_eq!(dates.finish, dt("2027-03-03 17:00"));
    assert_eq!(dates.blocked, Duration::ZERO);
    assert!(dates.segments.is_empty());
}

/// Blockers on a **summary** task are ignored: its dates are still rolled
/// up from its subtasks, with no segments and no blocked time.
///
/// Why it matters: the document refuses blockers on summaries, but a file
/// from elsewhere could contain them; the summary bar must not lie.
#[test]
fn blockers_on_a_summary_task_are_ignored() {
    let mut project = project_on("2027-03-01");
    let phase = add_task(&mut project, "Phase", days(1));
    let child = add_task(&mut project, "Child", days(2));
    project.tasks[1].outline_level = 2;
    let plain = schedule_ok(&project);
    project.tasks[0]
        .blockers
        .push(blocker(1, Duration::ZERO, days(5)));
    let result = schedule_ok(&project);
    let summary = dates_of(&result, phase);
    assert!(summary.is_summary);
    assert_eq!(summary.start, dates_of(&plain, phase).start);
    assert_eq!(summary.finish, dt("2027-03-02 17:00"));
    assert!(summary.segments.is_empty());
    assert_eq!(summary.blocked, Duration::ZERO);
    assert_eq!(summary.explanation.blocked, Duration::ZERO);
    assert_eq!(dates_of(&result, child).finish, dt("2027-03-02 17:00"));
}

/// A milestone (duration 0) with a 1-day blocker: the blocked time comes
/// first, then the milestone point, which lands at the task's finish. After
/// A (Mon 1 – Tue 2) the milestone's blocked piece is Wed 3, its finish Wed
/// 3 17:00, and its successor starts Thu 4.
///
/// Why it matters: "the inspection was postponed by a day" is a blocker on
/// a milestone; the diamond must move to the end of the delay.
#[test]
fn milestone_with_a_blocker_lands_after_the_blocked_time() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let m = add_task(&mut project, "Inspection", Duration::ZERO);
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, m);
    link(&mut project, m, b);
    project.tasks[1].blockers.push(blocker(1, days(3), days(1)));
    let result = schedule_ok(&project);
    let milestone = dates_of(&result, m);
    assert_eq!(milestone.finish, dt("2027-03-03 17:00"));
    assert_eq!(milestone.blocked, days(1));
    assert_eq!(milestone.segments.len(), 1, "one blocked piece, no work");
    assert!(matches!(
        milestone.segments[0].kind,
        SegmentKind::Blocked { .. }
    ));
    assert_eq!(milestone.segments[0].finish, milestone.finish);
    assert_eq!(dates_of(&result, b).start, dt("2027-03-04 08:00"));
}

/// A **manual** task is extended from its typed start: start Mon 8 08:00,
/// 2 days of work, blocked 1 day after 1 day → work Mon 8, blocked Tue 9,
/// work Wed 10, finish Wed 10 17:00.
///
/// Why it matters: manual tasks are placed by the user, but a delay still
/// makes them longer.
#[test]
fn manual_task_with_a_blocker() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    project.tasks[0].mode = TaskMode::Manual;
    project.tasks[0].manual_start = Some(dt("2027-03-08 08:00"));
    project.tasks[0].blockers.push(blocker(1, days(1), days(1)));
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert_eq!(dates.start, dt("2027-03-08 08:00"));
    assert_eq!(dates.finish, dt("2027-03-10 17:00"));
    assert_eq!(
        pieces(&dates.segments),
        expect(&[
            ("Mon 08 08:00", "Mon 08 17:00", "work"),
            ("Tue 09 08:00", "Tue 09 17:00", "blocked"),
            ("Wed 10 08:00", "Wed 10 17:00", "work"),
        ])
    );
}

/// An **ALAP** task leaves room for its blocked time: A (5 days) sets the
/// finish Fri 5; B (1 day, ALAP) blocked 1 day before its work occupies Thu
/// 4 (blocked) and Fri 5 (work).
///
/// Why it matters: "as late as possible" must still finish on time when
/// the delay is counted.
#[test]
fn alap_task_with_a_blocker_starts_earlier() {
    let mut project = project_on("2027-03-01");
    add_task(&mut project, "A", days(5));
    let b = add_task(&mut project, "B", days(1));
    project.tasks[1].constraint = Constraint::AsLateAsPossible;
    project.tasks[1]
        .blockers
        .push(blocker(1, Duration::ZERO, days(1)));
    let result = schedule_ok(&project);
    let dates = dates_of(&result, b);
    assert_eq!(dates.start, dt("2027-03-04 08:00"));
    assert_eq!(dates.finish, dt("2027-03-05 17:00"));
    assert_eq!(
        pieces(&dates.segments),
        expect(&[
            ("Thu 04 08:00", "Thu 04 17:00", "blocked"),
            ("Fri 05 08:00", "Fri 05 17:00", "work"),
        ])
    );
    assert_eq!(result.project_finish, dt("2027-03-05 17:00"));
}

/// A **Must Finish On** constraint on a task with a blocker counts back the
/// whole span: MFO Fri 12 Mar 17:00 on a 3-day task with a 5-day blocker
/// gives the start Wed 3 Mar 08:00 (8 working days earlier).
///
/// Why it matters: constraints and blockers must agree, or the bar would
/// end on the wrong day.
#[test]
fn must_finish_on_counts_back_work_and_blocked_time() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(3));
    project.tasks[0].constraint = Constraint::MustFinishOn {
        date: dt("2027-03-12 17:00"),
    };
    project.tasks[0].blockers.push(blocker(1, days(2), days(5)));
    let result = schedule_ok(&project);
    let dates = dates_of(&result, a);
    assert_eq!(dates.start, dt("2027-03-03 08:00"));
    assert_eq!(dates.finish, dt("2027-03-12 17:00"));
}

/// Scheduling **from the finish**: finish Fri 5 17:00, A (2 days, blocked
/// 1 day after 1 day) → B (1 day). B is Fri 5; A's span is 3 days, so A
/// runs Tue 2 – Thu 4 and the project starts Tue 2 08:00.
///
/// Why it matters: the backward calculation must leave room for the delay
/// too, or the project would start too late to finish on time.
#[test]
fn schedule_from_finish_includes_blocked_time() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = Some(dt("2027-03-05 17:00"));
    project.tasks[0].blockers.push(blocker(1, days(1), days(1)));
    let result = schedule_ok(&project);
    assert_eq!(result.project_start, dt("2027-03-02 08:00"));
    let dates = dates_of(&result, a);
    assert_eq!(dates.start, dt("2027-03-02 08:00"));
    assert_eq!(dates.finish, dt("2027-03-04 17:00"));
    assert_eq!(
        pieces(&dates.segments),
        expect(&[
            ("Tue 02 08:00", "Tue 02 17:00", "work"),
            ("Wed 03 08:00", "Wed 03 17:00", "blocked"),
            ("Thu 04 08:00", "Thu 04 17:00", "work"),
        ])
    );
    assert_eq!(dates_of(&result, b).start, dt("2027-03-05 08:00"));
}

/// The backward pass (late dates, slack) includes blocked time. A (5 days)
/// and B (2 days) both lead to C. Without blockers B has 3 days of slack.
/// A 2-day blocker leaves 1 day (late start Tue 2 08:00); a 3-day blocker
/// leaves none (B becomes critical); a 4-day blocker makes B the driver and
/// gives A 1 day of slack.
///
/// Why it matters: slack is how much a task may slip; if the backward pass
/// forgot the blocked time it would promise slack that does not exist.
#[test]
fn slack_shrinks_by_the_blocked_time() {
    let cases = [
        (Duration::ZERO, days(3), false, Duration::ZERO),
        (days(2), days(1), false, Duration::ZERO),
        (days(3), Duration::ZERO, true, Duration::ZERO),
        (days(4), Duration::ZERO, true, days(1)),
    ];
    for (blocked, b_slack, b_critical, a_slack) in cases {
        let mut project = project_on("2027-03-01");
        let a = add_task(&mut project, "A", days(5));
        let b = add_task(&mut project, "B", days(2));
        let c = add_task(&mut project, "C", days(1));
        link(&mut project, a, c);
        link(&mut project, b, c);
        project.tasks[1].blockers.push(blocker(1, days(1), blocked));
        let result = schedule_ok(&project);
        let b_dates = dates_of(&result, b);
        assert_eq!(b_dates.total_slack, b_slack, "blocked {blocked:?}");
        assert_eq!(b_dates.is_critical, b_critical, "blocked {blocked:?}");
        assert_eq!(
            dates_of(&result, a).total_slack,
            a_slack,
            "blocked {blocked:?}"
        );
        // C always starts right after the longer of the two branches.
        let longest = if blocked > days(3) { b } else { a };
        assert_eq!(
            dates_of(&result, c).explanation.driven_date,
            dates_of(&result, longest).finish,
            "blocked {blocked:?}"
        );
    }
    // The 2-day case in detail: late start Tue 2 08:00 (4 working days of
    // span, 2 work + 2 blocked, before the late finish Fri 5 17:00).
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(5));
    let b = add_task(&mut project, "B", days(2));
    let c = add_task(&mut project, "C", days(1));
    link(&mut project, a, c);
    link(&mut project, b, c);
    project.tasks[1].blockers.push(blocker(1, days(1), days(2)));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, b).late_start, dt("2027-03-02 08:00"));
    assert_eq!(dates_of(&result, b).late_finish, dt("2027-03-05 17:00"));
}

/// A **percentage lag** stays a share of the predecessor's *planned* work:
/// A (4 days) with a 2-day blocker at the end runs Mon 1 – Mon 8; an FS
/// link with 50 % lag waits 2 days (50 % of 4 days, not of 6), so B starts
/// Thu 11 08:00.
///
/// Why it matters: "start when half the work is done" is about the work;
/// a delay must not silently stretch the lag as well.
#[test]
fn percentage_lag_is_a_share_of_planned_work() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(4));
    let b = add_task(&mut project, "B", days(1));
    project
        .dependencies
        .push(Dependency::new(a, b, DependencyType::FinishToStart).with_lag_percent(50));
    project.tasks[0].blockers.push(blocker(1, days(4), days(2)));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-08 17:00"));
    assert_eq!(dates_of(&result, b).start, dt("2027-03-11 08:00"));
}
