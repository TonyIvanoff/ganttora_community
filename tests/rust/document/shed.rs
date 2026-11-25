//! # The garden shed through the document: 5 days -> 6 days and back
//!
//! ## What this file checks
//! the project documentation changes "Get building permit" (task 2) from 5 to 6 working
//! days and shows that the whole project then finishes one day later, on
//! Wednesday 24 March 2027 instead of Tuesday 23 March. Here the same edit
//! goes through the `Document`, as it does in the app, and is then undone.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! engine tests check the dates directly (`tests/rust/engine/shed.rs`); this
//! file checks that the document reschedules after each command and that the
//! snapshot sent to the UI shows the new dates.

use crate::{ctx, open_ok};
use document::Command;
use ganttora_tests::{days, dt, row_id, shed};

/// 5d -> 6d moves the finish to Wed 24 Mar 2027 17:00, both in the schedule
/// and in the snapshot; undo moves it back to Tue 23 Mar 2027 17:00.
///
/// Why it matters: this is the book's first "edit and watch the plan move"
/// example. If it fails, the app shows the user something different from the
/// book.
#[test]
fn permit_five_to_six_days_and_back() {
    let project = shed();
    let permit = row_id(&project, 2);
    let mut document = open_ok(project);
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-23 17:00")
    );

    document
        .apply(
            Command::SetTaskDuration {
                id: permit,
                duration: days(6),
            },
            &ctx(),
        )
        .expect("5d -> 6d");
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-24 17:00")
    );
    let snapshot = document.snapshot();
    assert_eq!(snapshot.project_finish, Some(dt("2027-03-24 17:00")));
    assert_eq!(snapshot.tasks[1].duration_minutes, 6 * 480);
    // "Buy materials" (row 3) now has 5 days of slack instead of 4, because
    // the critical path got one day longer.
    assert_eq!(snapshot.tasks[2].total_slack_minutes, Some(5 * 480));
    assert!(snapshot.tasks[1].is_critical);

    document.undo(&ctx()).expect("undo");
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-23 17:00")
    );
    let snapshot = document.snapshot();
    assert_eq!(snapshot.project_finish, Some(dt("2027-03-23 17:00")));
    assert_eq!(snapshot.tasks[2].total_slack_minutes, Some(4 * 480));
}
