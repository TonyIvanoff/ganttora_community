//! # Batches: several edits as one undo step, and "one kind of lag"
//!
//! ## What this file checks
//! - **`Command::Batch` / `Document::apply_batch`**: a list of commands that
//!   runs as *one* user action. One Undo takes back the whole list, one Redo
//!   repeats it. If any command in the list is refused, the whole batch is
//!   refused and the project is exactly as before. The outline is checked
//!   only after the last command, so "add a task, then indent it" works
//!   inside one batch. An empty batch changes nothing (no undo step). Every
//!   change record of a batch carries the same `BatchId`. The JSON shape
//!   `{"type":"batch","commands":[…]}` survives a round trip.
//! - **One kind of lag**: a link has *either* a time lag ("+2d") *or* a
//!   percentage lag ("+50%"). When the UI sends both, the document keeps
//!   the percentage and stores the time lag as 0.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! task information dialog, the grid's predecessor cell and paste all send
//! batches (see `frontend/state/store.ts`, `applyCommands`).
//!
//! ## Main ideas
//! - "All or nothing" is checked with `==` on the whole `Project` before and
//!   after a refused batch, plus "no undo step, no change records".
//! - Tests use fixed IDs (`Uuid::from_u128`) so failures are repeatable.

use crate::{ctx, ctx_with, open_ok};
use document::{Command, Document, DocumentError};
use ganttora_tests::{days, row_id, shed};
use model::{BatchId, Dependency, DependencyId, DependencyType, Duration, Task, TaskId};
use serde_json::{Value, json};
use uuid::Uuid;

/// A task ID made from a small number, so tests are repeatable.
fn fixed_task(n: u128) -> TaskId {
    TaskId::from_uuid(Uuid::from_u128(n))
}

/// A link ID made from a small number.
fn fixed_link(n: u128) -> DependencyId {
    DependencyId::from_uuid(Uuid::from_u128(n))
}

// ------------------------------------------------------------- one step

/// A batch of a rename and a duration change gives two change records but
/// only ONE undo step: after one Undo both edits are gone and Undo is no
/// longer possible; one Redo brings both back.
///
/// Why it matters: the task information dialog sends all its fields as one
/// batch when the user presses OK. The user thinks of it as one action, so
/// Ctrl+Z must take back all of it, not just the last field.
#[test]
fn batch_is_one_undo_and_one_redo_step() {
    let project = shed();
    let permit = row_id(&project, 2);
    let before = project.clone();
    let mut document = open_ok(project);

    let outcome = document
        .apply_batch(
            vec![
                Command::RenameTask {
                    id: permit,
                    name: "Permit".into(),
                },
                Command::SetTaskDuration {
                    id: permit,
                    duration: days(6),
                },
            ],
            &ctx(),
        )
        .expect("the batch should be accepted");
    assert_eq!(outcome.changes.len(), 2, "one record per changed field");
    assert!(outcome.can_undo);
    let after = document.project().clone();
    assert_eq!(after.tasks[1].name, "Permit");
    assert_eq!(after.tasks[1].duration, days(6));

    document.undo(&ctx()).expect("undo");
    assert_eq!(
        document.project(),
        &before,
        "one undo reverses the whole batch"
    );
    assert!(!document.can_undo(), "the batch was a single undo step");

    document.redo(&ctx()).expect("redo");
    assert_eq!(
        document.project(),
        &after,
        "one redo repeats the whole batch"
    );
    assert!(!document.can_redo());
}

/// `apply(Command::Batch { … })` and `apply_batch(…)` do the same thing.
///
/// Why it matters: the UI sends `{"type":"batch",…}` through the normal
/// `apply` door; Rust code (paste) uses `apply_batch`. Both must agree.
#[test]
fn apply_with_batch_command_equals_apply_batch() {
    let project = shed();
    let permit = row_id(&project, 2);
    let commands = vec![
        Command::RenameTask {
            id: permit,
            name: "Permit".into(),
        },
        Command::SetTaskDuration {
            id: permit,
            duration: days(7),
        },
    ];

    let mut first = open_ok(project.clone());
    let mut second = open_ok(project);
    let a = first.apply_batch(commands.clone(), &ctx()).expect("batch");
    let b = second
        .apply(Command::Batch { commands }, &ctx())
        .expect("batch");
    assert_eq!(a, b);
    assert_eq!(first.project(), second.project());
}

// -------------------------------------------------------- all or nothing

/// If the LAST command of a batch is refused (unknown task), the earlier
/// commands are not applied either: the project is unchanged, there is no
/// undo step and no change record, and the error is the one of the failing
/// command.
///
/// Why it matters: a half-applied dialog (name changed, duration not) would
/// leave the project in a state the user never asked for.
#[test]
fn one_failing_command_refuses_the_whole_batch() {
    let project = shed();
    let permit = row_id(&project, 2);
    let ghost = fixed_task(0xBAD);
    let before = project.clone();
    let mut document = Document::new(project);

    let error = document
        .apply_batch(
            vec![
                Command::RenameTask {
                    id: permit,
                    name: "Permit".into(),
                },
                Command::SetTaskDuration {
                    id: ghost,
                    duration: days(1),
                },
            ],
            &ctx(),
        )
        .unwrap_err();
    assert_eq!(error, DocumentError::TaskNotFound(ghost));
    assert_eq!(document.project(), &before, "nothing changed");
    assert!(!document.can_undo(), "no undo step");
    assert!(document.pending_changes().is_empty(), "no change records");
}

/// A batch whose commands are each valid, but whose result has a circle of
/// links, is refused as a whole with `Rejected`.
///
/// Why it matters: the schedule is checked once, at the end of the batch.
/// A paste or a dialog must never sneak a circle into the project.
#[test]
fn batch_that_creates_a_circle_is_rejected_as_a_whole() {
    let project = shed();
    let design = row_id(&project, 1);
    let finished = row_id(&project, 8);
    let before = project.clone();
    let mut document = Document::new(project);

    let back = Dependency::finish_to_start(finished, design);
    let error = document
        .apply_batch(
            vec![
                Command::RenameTask {
                    id: design,
                    name: "Design".into(),
                },
                Command::AddDependency { dependency: back },
            ],
            &ctx(),
        )
        .unwrap_err();
    assert!(
        matches!(error, DocumentError::Rejected(_)),
        "expected Rejected, got {error:?}"
    );
    assert_eq!(document.project(), &before);
    assert!(!document.can_undo());
}

// --------------------------------------------- outline checked at the end

/// Inside one batch, a new task can be added at level 1 and then indented
/// under the row above. One Undo removes it again.
///
/// Why it matters: "Insert subtask" is built from two commands. The outline
/// rule is checked only after the last command, so the steps in between may
/// look unusual for a moment.
#[test]
fn add_then_indent_inside_one_batch() {
    let project = shed();
    let before = project.clone();
    let mut document = open_ok(project);
    let new_task = Task::with_id(fixed_task(100), "Sketch the door", days(1));
    let new_id = new_task.id;

    document
        .apply_batch(
            vec![
                Command::AddTask {
                    index: 1,
                    task: new_task,
                },
                Command::IndentTask { id: new_id },
            ],
            &ctx(),
        )
        .expect("add + indent");
    let tasks = &document.project().tasks;
    assert_eq!(tasks[1].id, new_id);
    assert_eq!(
        tasks[1].outline_level, 2,
        "indented under 'Design the shed'"
    );
    assert_eq!(tasks[0].outline_level, 1);

    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
}

/// A batch that adds a task at level 3 directly under a level-1 row breaks
/// the outline at the end and is refused with `InvalidOutline`.
///
/// Why it matters: the end-of-batch check must really happen; otherwise a
/// batch could leave a broken outline that the grid cannot draw.
#[test]
fn batch_that_ends_with_a_broken_outline_is_refused() {
    let project = shed();
    let before = project.clone();
    let mut document = Document::new(project);
    let deep = Task::with_id(fixed_task(101), "Too deep", days(1)).at_level(3);

    let error = document
        .apply_batch(
            vec![Command::AddTask {
                index: 1,
                task: deep,
            }],
            &ctx(),
        )
        .unwrap_err();
    assert!(
        matches!(error, DocumentError::InvalidOutline { .. }),
        "expected InvalidOutline, got {error:?}"
    );
    assert_eq!(document.project(), &before);
}

// ---------------------------------------------------------- empty batch

/// An empty batch is accepted but changes nothing: no records, no undo step.
///
/// Why it matters: the dialog's OK button with no edits sends nothing worth
/// undoing; an empty Undo step would confuse the user ("Undo did nothing").
#[test]
fn empty_batch_gives_no_undo_step() {
    let mut document = open_ok(shed());
    let outcome = document.apply_batch(Vec::new(), &ctx()).expect("ok");
    assert!(outcome.changes.is_empty());
    assert!(!outcome.can_undo);
    assert!(!document.can_undo());
}

/// A batch whose commands change nothing (rename to the same name) also
/// gives no undo step.
///
/// Why it matters: same reason as the empty batch; "no change" must never
/// fill the undo history.
#[test]
fn batch_of_no_op_commands_gives_no_undo_step() {
    let project = shed();
    let permit = row_id(&project, 2);
    let mut document = open_ok(project);
    let outcome = document
        .apply_batch(
            vec![Command::RenameTask {
                id: permit,
                name: "Get building permit".into(),
            }],
            &ctx(),
        )
        .expect("ok");
    assert!(outcome.changes.is_empty());
    assert!(!document.can_undo());
}

// ------------------------------------------------------------- batch id

/// Every change record of a batch carries the batch ID of the context, and
/// the records of the following Undo carry the Undo's own batch ID.
///
/// Why it matters: the journal groups records by batch ID to show "one user
/// action" as one line ("Anna edited task 2: name, duration").
#[test]
fn all_records_of_a_batch_share_one_batch_id() {
    let project = shed();
    let permit = row_id(&project, 2);
    let paint = row_id(&project, 7);
    let mut document = open_ok(project);
    let context = ctx_with("Anna", "2027-02-15 11:00", 42);

    let outcome = document
        .apply_batch(
            vec![
                Command::RenameTask {
                    id: permit,
                    name: "Permit".into(),
                },
                Command::SetTaskDuration {
                    id: paint,
                    duration: days(3),
                },
                Command::SetTaskNotes {
                    id: paint,
                    notes: "Two coats".into(),
                },
            ],
            &context,
        )
        .expect("batch");
    let wanted = BatchId::from_uuid(Uuid::from_u128(42));
    assert_eq!(outcome.changes.len(), 3);
    assert!(outcome.changes.iter().all(|r| r.batch_id == wanted));
    assert!(outcome.changes.iter().all(|r| r.author == "Anna"));

    let undo = document
        .undo(&ctx_with("Anna", "2027-02-15 11:01", 43))
        .expect("undo");
    let undo_id = BatchId::from_uuid(Uuid::from_u128(43));
    assert_eq!(undo.changes.len(), 3, "undo records each field back");
    assert!(undo.changes.iter().all(|r| r.batch_id == undo_id));
}

// ----------------------------------------------------------------- JSON

/// The JSON the UI sends for a batch, `{"type":"batch","commands":[…]}`,
/// reads into `Command::Batch` and writes back to the same JSON.
///
/// Why it matters: TypeScript writes this JSON by hand
/// (`frontend/ipc/types.ts`). One wrong name and every dialog OK would fail.
#[test]
fn batch_json_round_trip() {
    let id = "00000000-0000-0000-0000-000000000007";
    let text = json!({
        "type": "batch",
        "commands": [
            { "type": "renameTask", "id": id, "name": "Permit" },
            { "type": "setTaskDuration", "id": id, "duration": { "minutes": 2880 } },
            { "type": "batch", "commands": [] }
        ]
    });
    let command: Command = serde_json::from_value(text.clone()).expect("reads");
    let task = fixed_task(7);
    assert_eq!(
        command,
        Command::Batch {
            commands: vec![
                Command::RenameTask {
                    id: task,
                    name: "Permit".into()
                },
                Command::SetTaskDuration {
                    id: task,
                    duration: Duration::from_minutes(2880)
                },
                Command::Batch {
                    commands: Vec::new()
                },
            ]
        }
    );
    let back: Value = serde_json::to_value(&command).expect("writes");
    assert_eq!(back, text);
}

// -------------------------------------------------------- one kind of lag

/// `AddDependency` with lag 2 days AND lag_percent 50 stores lag 0 and 50 %;
/// the "created" change record shows `lag_minutes` 0. Undo removes the link.
///
/// Why it matters: the `.gtra` file has room for only one kind of lag. A
/// hidden 2-day lag next to the 50 % would be invisible on screen and could
/// come back after save and reopen.
#[test]
fn add_dependency_with_both_lags_keeps_only_the_percentage() {
    let project = shed();
    let buy = row_id(&project, 3);
    let paint = row_id(&project, 7);
    let before = project.clone();
    let mut document = open_ok(project);
    let mut link = Dependency::finish_to_start(buy, paint).with_lag(days(2));
    link.lag_percent = Some(50);
    link.id = fixed_link(500);

    let outcome = document
        .apply(Command::AddDependency { dependency: link }, &ctx())
        .expect("add");
    let stored = document
        .project()
        .dependencies
        .iter()
        .find(|d| d.id == fixed_link(500))
        .expect("stored")
        .clone();
    assert_eq!(stored.lag, Duration::ZERO, "time lag dropped");
    assert_eq!(stored.lag_percent, Some(50));
    let lag_record = outcome
        .changes
        .iter()
        .find(|r| r.field == "lag_minutes")
        .expect("a lag_minutes record");
    assert_eq!(lag_record.new_value, json!(0));

    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
}

/// `EditDependency` from "FS+1d" to "lag 2d, lag_percent 50" stores lag 0
/// and 50 %; the record shows `lag_minutes` 480 → 0. Undo brings back the
/// 1-day lag exactly.
///
/// Why it matters: same rule for edits; and Undo must restore the old time
/// lag that the edit cleared.
#[test]
fn edit_dependency_with_both_lags_keeps_only_the_percentage() {
    let mut project = shed();
    project.dependencies[0].lag = days(1);
    let link = project.dependencies[0].id;
    let before = project.clone();
    let mut document = open_ok(project);

    let outcome = document
        .apply(
            Command::EditDependency {
                id: link,
                kind: DependencyType::FinishToStart,
                lag: days(2),
                lag_percent: Some(50),
            },
            &ctx(),
        )
        .expect("edit");
    let stored = &document.project().dependencies[0];
    assert_eq!(stored.lag, Duration::ZERO);
    assert_eq!(stored.lag_percent, Some(50));
    let lag_record = outcome
        .changes
        .iter()
        .find(|r| r.field == "lag_minutes")
        .expect("a lag_minutes record");
    assert_eq!(lag_record.old_value, json!(480));
    assert_eq!(lag_record.new_value, json!(0));

    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before, "the 1-day lag is back");
}

/// Without a percentage, the time lag is kept as sent.
///
/// Why it matters: the "one kind of lag" rule must not throw away ordinary
/// time lags.
#[test]
fn time_lag_without_percentage_is_kept() {
    let project = shed();
    let link = project.dependencies[0].id;
    let mut document = open_ok(project);
    document
        .apply(
            Command::EditDependency {
                id: link,
                kind: DependencyType::FinishToStart,
                lag: days(2),
                lag_percent: None,
            },
            &ctx(),
        )
        .expect("edit");
    assert_eq!(document.project().dependencies[0].lag, days(2));
    assert_eq!(document.project().dependencies[0].lag_percent, None);
}

// ------------------------------------------- loops while already broken

/// While the project calendar is missing (the schedule already fails), a
/// batch that adds a loop is still refused with `Rejected` (cycle), and the
/// project stays unchanged.
///
/// Why it matters: a project opened with a broken calendar can still be
/// edited and repaired, but a loop must never get in, because the engine
/// finds loops before calendar problems.
#[test]
fn loop_is_refused_even_when_the_project_calendar_is_missing() {
    let mut project = shed();
    project.calendar_id = crate::missing_calendar();
    let design = row_id(&project, 1);
    let finished = row_id(&project, 8);
    let before = project.clone();
    let mut document = Document::new(project);
    assert!(document.schedule().is_err(), "already broken");

    let error = document
        .apply_batch(
            vec![Command::AddDependency {
                dependency: Dependency::finish_to_start(finished, design),
            }],
            &ctx(),
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            DocumentError::Rejected(engine::ScheduleError::Cycle { .. })
        ),
        "expected a rejected cycle, got {error:?}"
    );
    assert_eq!(document.project(), &before);
}
