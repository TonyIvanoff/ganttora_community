//! # M1 task-field and link commands: apply, undo, redo, change records
//!
//! ## What this file checks
//! The commands added in M1 that change one task or one link:
//! `SetTaskNotes`, `SetTaskHyperlink`, `SetTaskMode`, `SetTaskManualStart`,
//! `SetTaskMilestone`, `SetTaskConstraint`, `SetTaskDeadline`,
//! `SetTaskCalendar`, `EditDependency`, and the project commands
//! `SetScheduleFrom` and `RenameProject`. For each one:
//! - applying it changes the project and gives the expected change records
//!   (field names are the model's snake_case names, values are JSON);
//! - undo gives back the identical project, with the same records but old
//!   and new values swapped; redo gives the edited project again;
//! - the ways it can be wrong are refused without leaving a trace.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`).
//!
//! ## Main ideas
//! The garden shed: 8 tasks, finish Tue 23 Mar 2027 17:00.

use crate::commands::refused;
use crate::{ctx, open_ok};
use document::{ChangeRecord, Command, Document, DocumentError, EntityKind};
use ganttora_tests::{days, dt, row_id, shed};
use model::{
    Calendar, CalendarId, Constraint, DependencyType, Project, ScheduleFrom, TaskMode, WorkWeek,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// Applies a command, checks undo and redo, and returns what happened.
///
/// # Parameters
/// - `project`: the starting project.
/// - `command`: the edit.
///
/// # Returns
/// The document after redo, the project right after the first apply, and
/// the change records of the first apply.
///
/// # Errors / edge cases
/// Fails the test if the command is refused, if undo does not restore the
/// starting project exactly, if the undo records are not the apply records
/// with old and new swapped, or if redo does not repeat the change.
pub(crate) fn round_trip(
    project: Project,
    command: Command,
) -> (Document, Project, Vec<ChangeRecord>) {
    let before = project.clone();
    let mut document = open_ok(project);
    let applied = document
        .apply(command, &ctx())
        .unwrap_or_else(|error| panic!("command refused: {error}"));
    assert!(!applied.changes.is_empty(), "a real change has records");
    let after = document.project().clone();
    assert_ne!(after, before, "the command should change the project");

    let undone = document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before, "undo restores the project");
    assert_eq!(
        swapped_set(&applied.changes),
        plain_set(&undone.changes),
        "undo records = apply records with old/new swapped"
    );

    let redone = document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &after, "redo repeats the change");
    assert_eq!(plain_set(&applied.changes), plain_set(&redone.changes));
    (document, after, applied.changes)
}

/// The records as sorted text lines "entity id field old new", so two lists
/// can be compared without caring about their order.
fn plain_set(records: &[ChangeRecord]) -> Vec<String> {
    let mut lines: Vec<String> = records
        .iter()
        .map(|r| {
            format!(
                "{:?} {} {} {} {}",
                r.entity, r.entity_id, r.field, r.old_value, r.new_value
            )
        })
        .collect();
    lines.sort();
    lines
}

/// Like [`plain_set`], but with old and new values swapped.
fn swapped_set(records: &[ChangeRecord]) -> Vec<String> {
    let mut lines: Vec<String> = records
        .iter()
        .map(|r| {
            format!(
                "{:?} {} {} {} {}",
                r.entity, r.entity_id, r.field, r.new_value, r.old_value
            )
        })
        .collect();
    lines.sort();
    lines
}

/// Finds the one record for `field`, failing the test if it is missing.
pub(crate) fn field<'a>(records: &'a [ChangeRecord], name: &str) -> &'a ChangeRecord {
    records
        .iter()
        .find(|r| r.field == name)
        .unwrap_or_else(|| panic!("no record for field {name}"))
}

/// Notes: one record `notes` "" → "Ask the neighbours".
///
/// Why it matters: free text must be stored and journaled like any field.
#[test]
fn set_task_notes() {
    let project = shed();
    let id = row_id(&project, 1);
    let (_, after, records) = round_trip(
        project,
        Command::SetTaskNotes {
            id,
            notes: "Ask the neighbours".into(),
        },
    );
    assert_eq!(after.tasks[0].notes, "Ask the neighbours");
    assert_eq!(records.len(), 1);
    assert_eq!(field(&records, "notes").old_value, json!(""));
    assert_eq!(
        field(&records, "notes").new_value,
        json!("Ask the neighbours")
    );
    assert_eq!(records[0].entity, EntityKind::Task);
}

/// Hyperlink: one record `hyperlink`.
///
/// Why it matters: same as notes, a plain text field.
#[test]
fn set_task_hyperlink() {
    let project = shed();
    let id = row_id(&project, 1);
    let (_, after, records) = round_trip(
        project,
        Command::SetTaskHyperlink {
            id,
            hyperlink: "https://example.com/plan.pdf".into(),
        },
    );
    assert_eq!(after.tasks[0].hyperlink, "https://example.com/plan.pdf");
    assert_eq!(
        field(&records, "hyperlink").new_value,
        json!("https://example.com/plan.pdf")
    );
}

/// Risks: add, edit and remove, each undone exactly, each one record `risk`;
/// incomplete risks are refused.
///
/// Why it matters: a risk without mitigation or consequence is not a plan.
#[test]
fn risks_are_added_edited_and_removed() {
    let project = shed();
    let task = row_id(&project, 1);
    let mut risk = model::Risk::new(model::RiskId::new(), "Wood may be late");
    risk.mitigation = "Order earlier".into();
    let refused = Document::new(project.clone())
        .apply(
            Command::AddRisk {
                task,
                risk: risk.clone(),
            },
            &ctx(),
        )
        .unwrap_err();
    assert_eq!(refused, DocumentError::RiskIncomplete(risk.id));

    risk.consequence = "The wall is later".into();
    let (_, with_risk, records) = round_trip(
        project,
        Command::AddRisk {
            task,
            risk: risk.clone(),
        },
    );
    assert_eq!(with_risk.tasks[0].risks, [risk.clone()]);
    assert_eq!(field(&records, "risk").old_value, json!(null));

    let mut approved = risk.clone();
    approved.response = model::RiskResponse::Approved;
    let (_, edited, records) = round_trip(
        with_risk.clone(),
        Command::EditRisk {
            task,
            risk: approved.clone(),
        },
    );
    assert_eq!(edited.tasks[0].risks, [approved]);
    assert_eq!(field(&records, "risk").new_value["response"], "approved");

    // A risk from an older file may be incomplete; completing it can be undone.
    let mut old_file = with_risk.clone();
    old_file.tasks[0].risks[0].consequence = String::new();
    round_trip(
        old_file,
        Command::EditRisk {
            task,
            risk: risk.clone(),
        },
    );

    let (_, removed, _) = round_trip(with_risk, Command::RemoveRisk { task, id: risk.id });
    assert!(removed.tasks[0].risks.is_empty());
}

/// Switching "Buy materials" (starts Wed 3 Mar 08:00) to manual without
/// giving a start: the document fills in the current start, so the task
/// stays where it is. Two records: `mode` "auto" → "manual" and
/// `manual_start` null → "2027-03-03T08:00:00". Redo uses the filled-in
/// start too.
///
/// Why it matters: in MS Project, switching to manual never makes a task
/// jump.
#[test]
fn set_task_mode_to_manual_keeps_the_current_start() {
    let project = shed();
    let id = row_id(&project, 3);
    let (document, after, records) = round_trip(
        project,
        Command::SetTaskMode {
            id,
            mode: TaskMode::Manual,
            manual_start: None,
        },
    );
    assert_eq!(after.tasks[2].mode, TaskMode::Manual);
    assert_eq!(after.tasks[2].manual_start, Some(dt("2027-03-03 08:00")));
    assert_eq!(records.len(), 2);
    assert_eq!(field(&records, "mode").old_value, json!("auto"));
    assert_eq!(field(&records, "mode").new_value, json!("manual"));
    assert_eq!(
        field(&records, "manual_start").new_value,
        json!("2027-03-03T08:00:00")
    );
    let schedule = document.schedule().expect("schedules");
    assert_eq!(
        schedule.task(id).expect("task").start,
        dt("2027-03-03 08:00")
    );
}

/// Moving a manual task: `SetTaskManualStart` to Mon 8 Mar 08:00 moves
/// "Buy materials" there (3 days → Wed 10 17:00), one record
/// `manual_start`.
///
/// Why it matters: dragging a manual bar in the Gantt chart is this command.
#[test]
fn set_task_manual_start_moves_a_manual_task() {
    let mut project = shed();
    project.tasks[2].mode = TaskMode::Manual;
    project.tasks[2].manual_start = Some(dt("2027-03-03 08:00"));
    let id = row_id(&project, 3);
    let (document, _, records) = round_trip(
        project,
        Command::SetTaskManualStart {
            id,
            start: Some(dt("2027-03-08 08:00")),
        },
    );
    assert_eq!(records.len(), 1);
    assert_eq!(
        field(&records, "manual_start").old_value,
        json!("2027-03-03T08:00:00")
    );
    let task = document
        .schedule()
        .expect("schedules")
        .task(id)
        .cloned()
        .expect("task");
    assert_eq!(task.finish, dt("2027-03-10 17:00"));
}

/// Milestone flag on "Paint": record `milestone` false → true, and the
/// snapshot shows `is_milestone`.
///
/// Why it matters: a task with a duration can still be drawn as a diamond.
#[test]
fn set_task_milestone() {
    let project = shed();
    let id = row_id(&project, 7);
    let (document, _, records) = round_trip(
        project,
        Command::SetTaskMilestone {
            id,
            milestone: true,
        },
    );
    assert_eq!(field(&records, "milestone").old_value, json!(false));
    assert_eq!(field(&records, "milestone").new_value, json!(true));
    assert!(document.snapshot().tasks[6].is_milestone);
}

/// SNET Mon 15 Mar 08:00 on "Build the floor" (normally Fri 12 Mar): it
/// moves to Mon 15 – Tue 16; walls Wed 17 – Mon 22; paint Tue 23 – Wed 24;
/// the project finishes Wed 24 Mar 17:00. One record `constraint` with the
/// constraint as JSON.
///
/// Why it matters: constraint edits change dates downstream and are
/// journaled in the same shape the UI sends.
#[test]
fn set_task_constraint() {
    let project = shed();
    let id = row_id(&project, 5);
    let (document, _, records) = round_trip(
        project,
        Command::SetTaskConstraint {
            id,
            constraint: Constraint::StartNoEarlierThan {
                date: dt("2027-03-15 08:00"),
            },
        },
    );
    assert_eq!(
        field(&records, "constraint").old_value,
        json!({"type": "asSoonAsPossible"})
    );
    assert_eq!(
        field(&records, "constraint").new_value,
        json!({"type": "startNoEarlierThan", "date": "2027-03-15T08:00:00"})
    );
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-24 17:00")
    );
}

/// Deadline Mon 22 Mar 17:00 on "Paint" (which finishes Tue 23): the
/// snapshot marks the missed deadline; record `deadline` null → date.
///
/// Why it matters: the UI shows a warning for missed deadlines.
#[test]
fn set_task_deadline() {
    let project = shed();
    let id = row_id(&project, 7);
    let (document, _, records) = round_trip(
        project,
        Command::SetTaskDeadline {
            id,
            deadline: Some(dt("2027-03-22 17:00")),
        },
    );
    assert_eq!(field(&records, "deadline").old_value, Value::Null);
    assert_eq!(
        field(&records, "deadline").new_value,
        json!("2027-03-22T17:00:00")
    );
    let snapshot = document.snapshot();
    assert!(snapshot.tasks[6].missed_deadline);
    assert_eq!(snapshot.tasks[6].total_slack_minutes, Some(-480));
}

/// A task calendar with Wed 3 Mar as a holiday on "Buy materials" (3 days,
/// normally Wed 3 – Fri 5): it now works Thu 4, Fri 5, Mon 8 → finish Mon 8
/// 17:00. Record `calendar_id` null → the calendar's UUID.
///
/// Why it matters: choosing a task calendar is an ordinary undoable edit.
#[test]
fn set_task_calendar() {
    let mut project = shed();
    let mut crew = Calendar::new("Crew", WorkWeek::standard());
    crew.holidays.insert(ganttora_tests::date("2027-03-03"));
    let crew_id = crew.id;
    project.calendars.push(crew);
    let id = row_id(&project, 3);
    let (document, _, records) = round_trip(
        project,
        Command::SetTaskCalendar {
            id,
            calendar_id: Some(crew_id),
        },
    );
    assert_eq!(field(&records, "calendar_id").old_value, Value::Null);
    assert_eq!(
        field(&records, "calendar_id").new_value,
        json!(crew_id.to_string())
    );
    let task = document
        .schedule()
        .expect("schedules")
        .task(id)
        .cloned()
        .expect("task");
    assert_eq!(task.finish, dt("2027-03-08 17:00"));
}

/// A task calendar that does not exist is refused with `CalendarNotFound`.
///
/// Why it matters: a task must never point at a missing calendar.
#[test]
fn set_task_calendar_to_unknown_calendar_is_refused() {
    let project = shed();
    let id = row_id(&project, 3);
    let ghost = CalendarId::from_uuid(Uuid::from_u128(0xBAD));
    assert_eq!(
        refused(
            project,
            Command::SetTaskCalendar {
                id,
                calendar_id: Some(ghost)
            }
        ),
        DocumentError::CalendarNotFound(ghost)
    );
}

/// Every task-field command on an unknown task is refused with
/// `TaskNotFound`.
///
/// Why it matters: a stale ID from the UI (a task deleted meanwhile) must
/// give a clear error, never a crash.
#[test]
fn task_field_commands_on_unknown_task_are_refused() {
    let ghost = model::TaskId::from_uuid(Uuid::from_u128(0xBAD));
    let commands = [
        Command::SetTaskNotes {
            id: ghost,
            notes: "x".into(),
        },
        Command::SetTaskHyperlink {
            id: ghost,
            hyperlink: "x".into(),
        },
        Command::SetTaskMode {
            id: ghost,
            mode: TaskMode::Manual,
            manual_start: None,
        },
        Command::SetTaskManualStart {
            id: ghost,
            start: None,
        },
        Command::SetTaskMilestone {
            id: ghost,
            milestone: true,
        },
        Command::SetTaskConstraint {
            id: ghost,
            constraint: Constraint::AsLateAsPossible,
        },
        Command::SetTaskDeadline {
            id: ghost,
            deadline: None,
        },
        Command::SetTaskCalendar {
            id: ghost,
            calendar_id: None,
        },
        Command::IndentTask { id: ghost },
        Command::OutdentTask { id: ghost },
        Command::MoveTask {
            id: ghost,
            to_index: 0,
            level: None,
        },
    ];
    for command in commands {
        assert_eq!(
            refused(shed(), command.clone()),
            DocumentError::TaskNotFound(ghost),
            "{command:?}"
        );
    }
}

/// Editing link 1 → 2 from FS to SS +1 day: two records, `kind`
/// "finishToStart" → "startToStart" and `lag_minutes` 0 → 480
/// (`lag_percent` stays null, so it has no record). Permit now starts 1 day
/// after Design starts: Tue 2 Mar 08:00.
///
/// Why it matters: the link dialog changes type and lag in one step.
#[test]
fn edit_dependency_type_and_lag() {
    let project = shed();
    let link = project.dependencies[0].id;
    let permit = row_id(&project, 2);
    let (document, after, records) = round_trip(
        project,
        Command::EditDependency {
            id: link,
            kind: DependencyType::StartToStart,
            lag: days(1),
            lag_percent: None,
        },
    );
    assert_eq!(after.dependencies[0].kind, DependencyType::StartToStart);
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.entity == EntityKind::Dependency));
    assert_eq!(field(&records, "kind").old_value, json!("finishToStart"));
    assert_eq!(field(&records, "kind").new_value, json!("startToStart"));
    assert_eq!(field(&records, "lag_minutes").new_value, json!(480));
    let task = document
        .schedule()
        .expect("schedules")
        .task(permit)
        .cloned()
        .expect("task");
    assert_eq!(task.start, dt("2027-03-02 08:00"));
}

/// A percentage lag: `lag_percent` null → 50 is recorded.
///
/// Why it matters: "2FS+50%" is stored as a percentage, not as minutes.
#[test]
fn edit_dependency_percentage_lag() {
    let project = shed();
    let link = project.dependencies[0].id;
    let (_, _, records) = round_trip(
        project,
        Command::EditDependency {
            id: link,
            kind: DependencyType::FinishToStart,
            lag: model::Duration::ZERO,
            lag_percent: Some(50),
        },
    );
    assert_eq!(records.len(), 1);
    assert_eq!(field(&records, "lag_percent").new_value, json!(50));
}

/// Editing a link that does not exist gives `DependencyNotFound`.
///
/// Why it matters: same as for tasks, stale IDs must give a clear error.
#[test]
fn edit_unknown_dependency_is_refused() {
    let ghost = model::DependencyId::from_uuid(Uuid::from_u128(0xBAD));
    assert_eq!(
        refused(
            shed(),
            Command::EditDependency {
                id: ghost,
                kind: DependencyType::FinishToStart,
                lag: model::Duration::ZERO,
                lag_percent: None
            }
        ),
        DocumentError::DependencyNotFound(ghost)
    );
}

/// Scheduling the shed from a fixed finish Fri 26 Mar 17:00: two records on
/// the project (`schedule_from` "start" → "finish", `finish` null → date),
/// and the project now finishes exactly then. The shed needs 17 working
/// days, so it starts 17 working days earlier: Thu 4 Mar 08:00.
///
/// Why it matters: "when must we start to be done by the 26th?" is a
/// one-click change that must be undoable.
#[test]
fn set_schedule_from_finish() {
    let project = shed();
    let (document, _, records) = round_trip(
        project,
        Command::SetScheduleFrom {
            schedule_from: ScheduleFrom::Finish,
            finish: Some(dt("2027-03-26 17:00")),
        },
    );
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.entity == EntityKind::Project));
    assert_eq!(field(&records, "schedule_from").old_value, json!("start"));
    assert_eq!(field(&records, "schedule_from").new_value, json!("finish"));
    assert_eq!(
        field(&records, "finish").new_value,
        json!("2027-03-26T17:00:00")
    );
    let schedule = document.schedule().expect("schedules");
    assert_eq!(schedule.project_finish, dt("2027-03-26 17:00"));
    assert_eq!(schedule.project_start, dt("2027-03-04 08:00"));
}

/// Renaming the project: one record `name` on the project entity.
///
/// Why it matters: the project name is shown in the title bar and printed.
#[test]
fn rename_project() {
    let project = shed();
    let project_id = project.id.as_uuid();
    let (document, _, records) = round_trip(
        project,
        Command::RenameProject {
            name: "Bigger shed".into(),
        },
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].entity, EntityKind::Project);
    assert_eq!(records[0].entity_id, project_id);
    assert_eq!(field(&records, "name").old_value, json!("Garden shed"));
    assert_eq!(document.snapshot().project_name, "Bigger shed");
}
