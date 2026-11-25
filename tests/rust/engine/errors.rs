//! # Error tests: every way scheduling can fail
//!
//! ## What this file checks
//! `engine::schedule` returns a `Result`: either a schedule or a
//! `ScheduleError` that says what is wrong. These tests build broken
//! projects on purpose (a circle of links, a link to a task that does not
//! exist, an unusable calendar ...) and check that the engine returns the
//! right error, with the right details, instead of crashing or looping
//! forever.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//!
//! ## Beginner note
//! `unwrap_err()` is the opposite of `unwrap()`: it takes the error out of
//! a `Result` and fails the test if there was *no* error. `matches!` checks
//! whether a value has a certain shape.

use engine::calendar_math::CalendarError;
use engine::{ScheduleError, schedule};
use ganttora_tests::{add_task, days, link, project_on, row_id, shed};
use model::{
    CalendarException, CalendarId, Dependency, DependencyType, Duration, ScheduleFrom, Task,
    TaskId, WorkWeek, WorkingTime,
};

/// Three tasks linked in a circle (A → B → C → A) give a `Cycle` error that
/// names all three, in link order, and the message reads
/// "circular dependency: A -> B -> C -> A".
///
/// Why it matters: a circle has no possible schedule (A waits for C, which
/// waits for A). The user must be told *which* tasks form the circle, so
/// they can fix it.
#[test]
fn circle_of_three_tasks_gives_cycle_error_naming_them() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    let c = add_task(&mut project, "C", days(1));
    link(&mut project, a, b);
    link(&mut project, b, c);
    link(&mut project, c, a);

    let error = schedule(&project).unwrap_err();
    assert_eq!(
        error,
        ScheduleError::Cycle {
            tasks: vec![a, b, c],
            names: vec!["A".into(), "B".into(), "C".into()],
        }
    );
    assert_eq!(error.to_string(), "circular dependency: A -> B -> C -> A");
}

/// A task linked to itself is the smallest possible circle: "A -> A".
///
/// Why it matters: a slip of the mouse can link a task to itself. It must be
/// reported like any other circle, not hang the app.
#[test]
fn task_linked_to_itself_is_a_cycle() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    link(&mut project, a, a);

    let error = schedule(&project).unwrap_err();
    assert_eq!(error.to_string(), "circular dependency: A -> A");
}

/// A task that only *waits for* a circle (D after C, with A → B → C → A) is
/// stuck too, but it is not part of the circle, so it is not named.
///
/// Why it matters: the message should point at the links to fix, not at
/// innocent tasks downstream.
#[test]
fn task_behind_a_cycle_is_not_named() {
    let mut project = project_on("2027-03-01");
    // D comes first in the list, so the engine meets it first.
    let d = add_task(&mut project, "D", days(1));
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    let c = add_task(&mut project, "C", days(1));
    link(&mut project, a, b);
    link(&mut project, b, c);
    link(&mut project, c, a);
    link(&mut project, c, d);

    match schedule(&project).unwrap_err() {
        ScheduleError::Cycle { tasks, .. } => {
            assert!(!tasks.contains(&d), "D is not in the circle");
            assert_eq!(tasks.len(), 3);
        }
        other => panic!("expected a Cycle error, got {other:?}"),
    }
}

/// Adding the link 8 → 1 to the shed ("Shed finished" before "Design")
/// makes a circle. The reported tasks start with task 1 and really form a
/// closed loop: each task is a predecessor of the next one, and the last
/// one leads back to the first.
///
/// Why it matters: in a real project the circle can be long and hidden. We
/// check the *meaning* of the answer (a real loop), not one exact path.
#[test]
fn cycle_in_the_shed_is_reported_as_a_real_loop() {
    let mut project = shed();
    let first = row_id(&project, 1);
    let last = row_id(&project, 8);
    link(&mut project, last, first);

    let tasks = match schedule(&project).unwrap_err() {
        ScheduleError::Cycle { tasks, names } => {
            assert_eq!(tasks.len(), names.len());
            tasks
        }
        other => panic!("expected a Cycle error, got {other:?}"),
    };
    assert_eq!(tasks[0], first);
    let is_link = |p: TaskId, s: TaskId| {
        project
            .dependencies
            .iter()
            .any(|d| d.predecessor == p && d.successor == s)
    };
    for pair in tasks.windows(2) {
        assert!(
            is_link(pair[0], pair[1]),
            "consecutive tasks must be linked"
        );
    }
    assert!(
        is_link(tasks[tasks.len() - 1], tasks[0]),
        "the loop must close"
    );
}

/// A link whose predecessor is not in the project gives `UnknownTask`,
/// naming the broken link and the missing task.
///
/// Why it matters: after an import or a bad edit a link may point at a
/// deleted task. The engine must say so instead of crashing.
#[test]
fn link_from_unknown_task_gives_unknown_task_error() {
    let mut project = project_on("2027-03-01");
    let b = add_task(&mut project, "B", days(1));
    let ghost = TaskId::new();
    let dependency = link(&mut project, ghost, b);

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::UnknownTask {
            dependency,
            task: ghost
        }
    );
}

/// Same as above for a missing successor.
///
/// Why it matters: both ends of a link must be checked.
#[test]
fn link_to_unknown_task_gives_unknown_task_error() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let ghost = TaskId::new();
    let dependency = link(&mut project, a, ghost);

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::UnknownTask {
            dependency,
            task: ghost
        }
    );
}

/// Since M1 every link type (SS, FF, SF) schedules; the old error
/// `UnsupportedDependencyType` is never returned any more.
///
/// Why it matters: this replaces the M0 test "SS/FF/SF are refused". The
/// detailed dates of each link type are checked in `link_types.rs`; here we
/// only make sure no link type is an error.
#[test]
fn every_link_type_schedules_since_m1() {
    for kind in [
        DependencyType::FinishToStart,
        DependencyType::StartToStart,
        DependencyType::FinishToFinish,
        DependencyType::StartToFinish,
    ] {
        let mut project = project_on("2027-03-01");
        let a = add_task(&mut project, "A", days(1));
        let b = add_task(&mut project, "B", days(1));
        project.dependencies.push(Dependency::new(a, b, kind));
        assert!(schedule(&project).is_ok(), "link type {kind:?}");
    }
}

/// A task that names a calendar which is not in the project gives
/// `TaskCalendarNotFound`, naming the task and the missing calendar.
///
/// Why it matters: a task calendar can be deleted or lost when importing;
/// the user must see which task points at nothing.
#[test]
fn missing_task_calendar_is_reported() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let ghost = CalendarId::from_uuid(uuid::Uuid::from_u128(0xDEAD));
    project.tasks[0].calendar_id = Some(ghost);
    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::TaskCalendarNotFound {
            task: a,
            calendar: ghost
        }
    );
}

/// Scheduling from the finish without a finish date gives
/// `ProjectFinishMissing`.
///
/// Why it matters: "schedule backward from the finish" needs a finish to
/// start from; the engine must say so instead of guessing.
#[test]
fn schedule_from_finish_without_finish_date_is_reported() {
    let mut project = project_on("2027-03-01");
    add_task(&mut project, "A", days(1));
    project.schedule_from = ScheduleFrom::Finish;
    project.finish = None;
    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::ProjectFinishMissing
    );
}

/// An exception that ends before it starts makes the calendar invalid:
/// `Calendar(InvalidException)` with the exception's first date.
///
/// Why it matters: a broken exception would make the calendar arithmetic
/// meaningless; refusing it with a clear code is safer.
#[test]
fn exception_ending_before_it_starts_is_rejected() {
    let mut project = project_on("2027-03-01");
    add_task(&mut project, "A", days(1));
    let calendar_id = project.calendar_id;
    project.calendars[0]
        .exceptions
        .push(CalendarException::non_working(
            "Backwards",
            ganttora_tests::date("2027-03-10"),
            ganttora_tests::date("2027-03-08"),
        ));
    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::Calendar(CalendarError::InvalidException {
            calendar: calendar_id,
            start: ganttora_tests::date("2027-03-10"),
        })
    );
}

/// Errors reach the UI as JSON `{ "code": …, "details": … }`, never as an
/// English sentence: a cycle has code `cycleDetected` with the task IDs and
/// names; a missing finish has only a code.
///
/// Why it matters: the UI translates the code into the user's language
/// (no hard-coded UI text). A renamed code would show a raw key.
#[test]
fn errors_serialize_as_code_and_details() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    link(&mut project, b, a);
    let error = schedule(&project).unwrap_err();
    assert_eq!(
        serde_json::to_value(&error).expect("serialize"),
        serde_json::json!({
            "code": "cycleDetected",
            "details": { "tasks": [a.to_string(), b.to_string()], "names": ["A", "B"] }
        })
    );
    assert_eq!(
        serde_json::to_value(ScheduleError::ProjectFinishMissing).expect("serialize"),
        serde_json::json!({ "code": "projectFinishMissing" })
    );
    let calendar = CalendarId::from_uuid(uuid::Uuid::from_u128(7));
    assert_eq!(
        serde_json::to_value(ScheduleError::Calendar(CalendarError::NoWorkingTime {
            calendar
        }))
        .expect("serialize"),
        serde_json::json!({
            "code": "calendar",
            "details": { "code": "noWorkingTime", "details": { "calendar": calendar.to_string() } }
        })
    );
}

/// Two tasks with the same ID give `DuplicateTaskId`.
///
/// Why it matters: IDs must be unique, otherwise a link could
/// not tell which task it means.
#[test]
fn duplicate_task_ids_are_rejected() {
    let mut project = project_on("2027-03-01");
    let task = Task::new("A", days(1));
    let id = task.id;
    project.tasks.push(task.clone());
    project.tasks.push(task);

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::DuplicateTaskId(id)
    );
}

/// A task with a negative duration gives `NegativeDuration` with its name.
///
/// Why it matters: "−2 days" of work makes no sense. The user should see
/// which task has the bad value.
#[test]
fn negative_duration_is_rejected() {
    let mut project = project_on("2027-03-01");
    let id = add_task(&mut project, "Time machine", Duration::from_hours(-2));

    let error = schedule(&project).unwrap_err();
    assert_eq!(
        error,
        ScheduleError::NegativeDuration {
            task: id,
            name: "Time machine".into()
        }
    );
    assert_eq!(
        error.to_string(),
        "task \"Time machine\" has a negative duration"
    );
}

/// A project whose calendar ID matches none of its calendars gives
/// `CalendarNotFound`.
///
/// Why it matters: without a calendar the engine cannot know what working
/// time is. A damaged file must produce a clear error.
#[test]
fn missing_project_calendar_is_reported() {
    let mut project = project_on("2027-03-01");
    let wrong = CalendarId::new();
    project.calendar_id = wrong;

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::CalendarNotFound(wrong)
    );
}

/// A calendar with no working time at all (every day empty) gives
/// `Calendar(NoWorkingTime)` instead of searching forever for a working day.
///
/// Why it matters: an endless loop would freeze the app. The engine must
/// check first and refuse.
#[test]
fn calendar_without_working_time_is_rejected() {
    let mut project = project_on("2027-03-01");
    let calendar_id = project.calendar_id;
    let week = &mut project.calendars[0].week;
    for day in [
        &mut week.monday,
        &mut week.tuesday,
        &mut week.wednesday,
        &mut week.thursday,
        &mut week.friday,
    ] {
        day.clear();
    }
    add_task(&mut project, "A", days(1));

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::Calendar(CalendarError::NoWorkingTime {
            calendar: calendar_id
        })
    );
}

/// Working hours that end before they start (17:00–08:00 on Monday) give
/// `Calendar(InvalidWorkingTime)` naming Monday.
///
/// Why it matters: a typo in the calendar dialog must be caught before it
/// produces nonsense dates.
#[test]
fn calendar_with_backwards_hours_is_rejected() {
    let mut project = project_on("2027-03-01");
    let calendar_id = project.calendar_id;
    project.calendars[0].week.monday = vec![WorkingTime::hours(17, 8)];

    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::Calendar(CalendarError::InvalidWorkingTime {
            calendar: calendar_id,
            weekday: chrono::Weekday::Mon,
        })
    );
}

/// The checks also run when the broken calendar is not the standard one:
/// a fresh `WorkWeek` with overlapping blocks (08–12 and 11–17) is rejected.
///
/// Why it matters: overlapping blocks would count the hour 11–12 twice.
#[test]
fn calendar_with_overlapping_hours_is_rejected() {
    let mut project = project_on("2027-03-01");
    let mut week = WorkWeek::standard();
    week.tuesday = vec![WorkingTime::hours(8, 12), WorkingTime::hours(11, 17)];
    project.calendars[0].week = week;

    assert!(matches!(
        schedule(&project).unwrap_err(),
        ScheduleError::Calendar(CalendarError::InvalidWorkingTime {
            weekday: chrono::Weekday::Tue,
            ..
        })
    ));
}
