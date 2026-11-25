//! # Calendar and project-calendar commands, and "undo everything"
//!
//! ## What this file checks
//! - `AddCalendar`, `RemoveCalendar`, `RestoreCalendar`, `UpdateCalendar`
//!   and `SetProjectCalendar`: apply, undo, redo, change records, errors
//!   (`CalendarNotFound`, `CalendarIdExists`, `CalendarInUse`,
//!   `IndexOutOfRange`, a calendar update that would break the schedule).
//! - A long mixed sequence of M1 commands, then undo until nothing is left:
//!   the project must be **identical** to the one we started with, and redo
//!   until nothing is left must give the final project again.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`).
//!
//! ## Main ideas
//! The garden shed: 8 tasks, finish Tue 23 Mar 2027 17:00,
//! one calendar ("Standard", the project calendar).

use crate::commands::refused;
use crate::task_fields::{field, round_trip};
use crate::{ctx, open_ok};
use document::{Command, DocumentError, EntityKind, OutlineLevel};
use ganttora_tests::{date, days, dt, row_id, shed};
use model::{
    Calendar, CalendarException, CalendarId, Constraint, Dependency, DependencyType, Task,
    TaskMode, WorkWeek,
};
use serde_json::json;
use uuid::Uuid;

/// A spare calendar with a fixed ID, not yet in the project.
fn spare() -> Calendar {
    Calendar::with_id(
        CalendarId::from_uuid(Uuid::from_u128(0xCA1)),
        "Night shift",
        WorkWeek::standard(),
    )
}

/// Adding a calendar: five records (index, name, week, holidays,
/// exceptions) on the calendar entity, all with old value null; it goes at
/// the end of the list.
///
/// Why it matters: the calendar editor's "New calendar" button.
#[test]
fn add_calendar() {
    let calendar = spare();
    let id = calendar.id;
    let (_, after, records) = round_trip(shed(), Command::AddCalendar { calendar });
    assert_eq!(after.calendars.len(), 2);
    assert_eq!(after.calendars[1].id, id);
    assert_eq!(records.len(), 5);
    assert!(records.iter().all(|r| r.entity == EntityKind::Calendar));
    assert_eq!(field(&records, "index").new_value, json!(1));
    assert_eq!(field(&records, "name").new_value, json!("Night shift"));
}

/// Adding a calendar whose ID is already used gives `CalendarIdExists`.
///
/// Why it matters: IDs are unique.
#[test]
fn add_calendar_with_used_id_is_refused() {
    let mut project = shed();
    project.calendars.push(spare());
    assert_eq!(
        refused(project, Command::AddCalendar { calendar: spare() }),
        DocumentError::CalendarIdExists(spare().id)
    );
}

/// Removing an unused calendar: five records with new value null; undo (a
/// `RestoreCalendar`) puts it back at the same position.
///
/// Why it matters: deleting a calendar must be undoable exactly.
#[test]
fn remove_unused_calendar() {
    let mut project = shed();
    project.calendars.insert(0, spare());
    let (_, after, records) = round_trip(project, Command::RemoveCalendar { id: spare().id });
    assert_eq!(after.calendars.len(), 1);
    assert_eq!(records.len(), 5);
    assert_eq!(field(&records, "index").old_value, json!(0));
}

/// A calendar that is the project calendar, or a task's calendar, cannot be
/// removed: `CalendarInUse`. An unknown calendar gives `CalendarNotFound`.
///
/// Why it matters: removing it would leave tasks pointing at nothing.
#[test]
fn calendar_in_use_cannot_be_removed() {
    let project = shed();
    let standard = project.calendar_id;
    assert_eq!(
        refused(project, Command::RemoveCalendar { id: standard }),
        DocumentError::CalendarInUse(standard)
    );

    let mut project = shed();
    project.calendars.push(spare());
    project.tasks[2].calendar_id = Some(spare().id);
    assert_eq!(
        refused(project, Command::RemoveCalendar { id: spare().id }),
        DocumentError::CalendarInUse(spare().id)
    );

    let ghost = CalendarId::from_uuid(Uuid::from_u128(0xBAD));
    assert_eq!(
        refused(shed(), Command::RemoveCalendar { id: ghost }),
        DocumentError::CalendarNotFound(ghost)
    );
}

/// `RestoreCalendar` past the end of the list gives `IndexOutOfRange`.
///
/// Why it matters: restore commands are validated like any other command.
#[test]
fn restore_calendar_out_of_range_is_refused() {
    assert_eq!(
        refused(
            shed(),
            Command::RestoreCalendar {
                index: 5,
                calendar: spare()
            }
        ),
        DocumentError::IndexOutOfRange { index: 5, len: 1 }
    );
}

/// Updating the project calendar with a holiday on Wed 3 Mar: one record
/// (`holidays` [] → ["2027-03-03"]); the shed now finishes one working day
/// later, Wed 24 Mar 17:00.
///
/// Why it matters: the calendar editor's "Save" is this command; only the
/// fields that really changed are journaled.
#[test]
fn update_calendar_adds_a_holiday() {
    let project = shed();
    let mut changed = project.calendars[0].clone();
    changed.holidays.insert(date("2027-03-03"));
    let (document, _, records) = round_trip(project, Command::UpdateCalendar { calendar: changed });
    assert_eq!(records.len(), 1);
    assert_eq!(field(&records, "holidays").old_value, json!([]));
    assert_eq!(field(&records, "holidays").new_value, json!(["2027-03-03"]));
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-24 17:00")
    );
}

/// Updating an unknown calendar gives `CalendarNotFound`; an update that
/// makes the project calendar invalid (an exception ending before it
/// starts) is refused with `Rejected(Calendar(InvalidException))`.
///
/// Why it matters: a broken calendar would leave the user without a
/// schedule.
#[test]
fn bad_calendar_updates_are_refused() {
    assert_eq!(
        refused(shed(), Command::UpdateCalendar { calendar: spare() }),
        DocumentError::CalendarNotFound(spare().id)
    );

    let project = shed();
    let mut broken = project.calendars[0].clone();
    broken.exceptions.push(CalendarException::non_working(
        "Backwards",
        date("2027-03-10"),
        date("2027-03-08"),
    ));
    let error = refused(project, Command::UpdateCalendar { calendar: broken });
    assert!(
        matches!(
            error,
            DocumentError::Rejected(engine::ScheduleError::Calendar(
                engine::calendar_math::CalendarError::InvalidException { .. }
            ))
        ),
        "got {error:?}"
    );
}

/// Choosing another project calendar: one record `calendar_id` on the
/// project. With a calendar whose Monday 1 Mar is a holiday, the shed
/// starts Tue 2 Mar 08:00. An unknown calendar gives `CalendarNotFound`.
///
/// Why it matters: switching the project calendar reschedules everything.
#[test]
fn set_project_calendar() {
    let mut project = shed();
    let mut other = spare();
    other.holidays.insert(date("2027-03-01"));
    let other_id = other.id;
    let old = project.calendar_id;
    project.calendars.push(other);
    let (document, _, records) = round_trip(
        project,
        Command::SetProjectCalendar {
            calendar_id: other_id,
        },
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].entity, EntityKind::Project);
    assert_eq!(
        field(&records, "calendar_id").old_value,
        json!(old.to_string())
    );
    assert_eq!(
        document.schedule().expect("schedules").project_start,
        dt("2027-03-02 08:00")
    );

    let ghost = CalendarId::from_uuid(Uuid::from_u128(0xBAD));
    assert_eq!(
        refused(shed(), Command::SetProjectCalendar { calendar_id: ghost }),
        DocumentError::CalendarNotFound(ghost)
    );
}

/// Undo everything: 14 different M1 commands, then undo until
/// `NothingToUndo`. The project is identical to the start (same tasks,
/// links, calendars, order and every field). Then redo until
/// `NothingToRedo` gives exactly the final project.
///
/// Why it matters: users rely on "Ctrl+Z all the way back" to return to the
/// file as it was opened. Any command whose inverse is slightly off would
/// show up here.
#[test]
fn undo_all_returns_the_identical_project() {
    let project = shed();
    let start = project.clone();
    let id = |row| row_id(&project, row);
    let calendar = spare();
    let calendar_id = calendar.id;
    let new_task = Task::new("Order the door", days(1));
    let commands = vec![
        Command::AddCalendar {
            calendar: calendar.clone(),
        },
        Command::AddTask {
            index: 8,
            task: new_task.clone(),
        },
        Command::IndentTask { id: new_task.id },
        Command::SetTaskCalendar {
            id: id(3),
            calendar_id: Some(calendar_id),
        },
        Command::SetTaskConstraint {
            id: id(5),
            constraint: Constraint::StartNoEarlierThan {
                date: dt("2027-03-15 08:00"),
            },
        },
        Command::SetTaskDeadline {
            id: id(7),
            deadline: Some(dt("2027-03-22 17:00")),
        },
        Command::SetTaskMode {
            id: id(4),
            mode: TaskMode::Manual,
            manual_start: None,
        },
        Command::EditDependency {
            id: project.dependencies[0].id,
            kind: DependencyType::StartToStart,
            lag: days(1),
            lag_percent: None,
        },
        Command::AddDependency {
            dependency: Dependency::new(id(3), id(4), DependencyType::FinishToFinish)
                .with_lag_percent(25),
        },
        Command::MoveTask {
            id: id(7),
            to_index: 1,
            level: None,
        },
        Command::SetOutlineLevels {
            levels: vec![OutlineLevel {
                id: new_task.id,
                level: 1,
            }],
        },
        Command::RemoveTask { id: id(1) },
        Command::RenameProject {
            name: "Bigger shed".into(),
        },
        Command::SetScheduleFrom {
            schedule_from: model::ScheduleFrom::Finish,
            finish: Some(dt("2027-04-30 17:00")),
        },
    ];
    let mut document = open_ok(project.clone());
    for command in commands {
        document
            .apply(command.clone(), &ctx())
            .unwrap_or_else(|e| panic!("{command:?} refused: {e}"));
    }
    let end = document.project().clone();
    let mut steps = 0;
    while document.can_undo() {
        document.undo(&ctx()).expect("undo");
        steps += 1;
    }
    assert_eq!(steps, 14, "every command is one undo step");
    assert_eq!(document.project(), &start);
    assert_eq!(
        document.undo(&ctx()).unwrap_err(),
        DocumentError::NothingToUndo
    );
    while document.can_redo() {
        document.redo(&ctx()).expect("redo");
    }
    assert_eq!(document.project(), &end);
    assert_eq!(
        document.redo(&ctx()).unwrap_err(),
        DocumentError::NothingToRedo
    );
}
