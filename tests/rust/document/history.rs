//! # Undo/redo rules
//!
//! ## What this file checks
//! The rules of the history, independent of which command is used:
//! - a change that would create a circular dependency is refused as a whole
//!   ("atomic refusal"), and the project stays exactly as it was;
//! - a command that changes nothing adds no undo step;
//! - a new command after undo clears the redo list;
//! - removing a task with links and undoing gives back the identical project;
//! - several undos and redos in a row walk the history correctly;
//! - undo/redo with an empty history give clear errors.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`).
//!
//! ## Main ideas
//! Undo/redo is built from two stacks (piles): every applied command goes on
//! the undo pile; undo moves it to the redo pile; a brand-new command throws
//! the redo pile away, like in every editor.

use crate::{break_schedule, ctx, open_ok};
use document::{Command, Document, DocumentError};
use engine::ScheduleError;
use ganttora_tests::{days, row_id, shed};
use model::Dependency;

/// Linking the last shed task back to the first one would make a loop
/// (1 -> ... -> 8 -> 1). The document refuses it with `Rejected(Cycle)`,
/// and the project, the schedule and the history stay unchanged.
///
/// Why it matters: a loop has no possible schedule. If the document applied
/// the link and *then* noticed, the user's project would be broken. "Atomic"
/// means: all or nothing.
#[test]
fn cycle_is_refused_and_nothing_changes() {
    let project = shed();
    let before = project.clone();
    let mut document = open_ok(project);
    let finish_before = document.schedule().expect("schedules").project_finish;

    let back_link = Dependency::finish_to_start(row_id(&before, 8), row_id(&before, 1));
    let result = document.apply(
        Command::AddDependency {
            dependency: back_link,
        },
        &ctx(),
    );

    match result {
        Err(DocumentError::Rejected(ScheduleError::Cycle { tasks, .. })) => {
            assert!(!tasks.is_empty(), "the error names the tasks in the loop");
        }
        other => panic!("expected Rejected(Cycle), got {other:?}"),
    }
    assert_eq!(document.project(), &before);
    assert_eq!(
        document.schedule().expect("still schedules").project_finish,
        finish_before
    );
    assert!(!document.can_undo());
    assert!(document.pending_changes().is_empty());
}

/// A cycle is refused even when the schedule was already broken before the
/// edit (here by a task that names a calendar which does not exist).
///
/// Why it matters: other errors are tolerated while the project is already
/// broken (so the user can fix it step by step), but a loop is never let in.
///
/// This guards a fixed bug: the engine used to report another problem before
/// looking for loops, so a looping link was accepted. Now the engine checks
/// for loops even when another error exists (see `engine/src/cpm/graph.rs`).
///
/// The same bug came back once in M1 (task calendars were checked before
/// the loop check) and was fixed again: `cpm::Context::new` now builds the
/// graph first (see `engine/src/cpm/mod.rs`).
#[test]
fn cycle_is_refused_even_when_schedule_was_broken() {
    let mut project = shed();
    let one = row_id(&project, 1);
    let eight = row_id(&project, 8);
    break_schedule(&mut project, 3);
    let before = project.clone();
    let mut document = Document::new(project);
    assert!(
        document.schedule().is_err(),
        "the missing task calendar breaks the schedule"
    );

    let result = document.apply(
        Command::AddDependency {
            dependency: Dependency::finish_to_start(eight, one),
        },
        &ctx(),
    );
    assert!(
        matches!(
            result,
            Err(DocumentError::Rejected(ScheduleError::Cycle { .. }))
        ),
        "got {result:?}"
    );
    assert_eq!(document.project(), &before);
}

/// While the schedule is already broken, a harmless edit (a rename) is
/// accepted, and the schedule stays broken.
///
/// Why it matters: the user must be able to keep working on (and fixing) a
/// project that does not schedule, instead of being locked out.
#[test]
fn edits_are_allowed_while_schedule_is_broken() {
    let mut project = shed();
    let three = row_id(&project, 3);
    break_schedule(&mut project, 3);
    let mut document = Document::new(project);
    assert!(document.schedule().is_err());

    let outcome = document
        .apply(
            Command::RenameTask {
                id: three,
                name: "Buy timber".into(),
            },
            &ctx(),
        )
        .expect("a rename is fine even if the schedule is broken");
    assert!(outcome.can_undo);
    assert!(document.schedule().is_err());
}

/// Commands that set a value to what it already is (same name, same
/// duration, same start) return no changes and add no undo step.
///
/// Why it matters: clicking into a cell and leaving it without typing must
/// not fill the undo list with empty steps (pressing Ctrl+Z would then seem
/// to do nothing).
#[test]
fn no_op_commands_add_no_undo_step() {
    let project = shed();
    let id = row_id(&project, 2);
    let name = project.tasks[1].name.clone();
    let start = project.start;
    let before = project.clone();
    let mut document = open_ok(project);

    let no_ops = [
        Command::RenameTask { id, name },
        Command::SetTaskDuration {
            id,
            duration: days(5),
        },
        Command::SetProjectStart { start },
    ];
    for command in no_ops {
        let outcome = document
            .apply(command.clone(), &ctx())
            .unwrap_or_else(|e| panic!("{command:?} failed: {e}"));
        assert!(outcome.changes.is_empty(), "{command:?} changed something");
        assert!(!outcome.can_undo, "{command:?} added an undo step");
    }
    assert_eq!(document.project(), &before);
    assert!(document.pending_changes().is_empty());
}

/// After an undo, applying a *new* command clears the redo list.
///
/// Why it matters: redo would otherwise re-apply an old change on top of a
/// different project, which can make no sense (the edited task might even be
/// gone).
#[test]
fn new_command_clears_redo() {
    let project = shed();
    let two = row_id(&project, 2);
    let three = row_id(&project, 3);
    let mut document = open_ok(project);

    document
        .apply(
            Command::RenameTask {
                id: two,
                name: "A".into(),
            },
            &ctx(),
        )
        .expect("rename");
    document.undo(&ctx()).expect("undo");
    assert!(document.can_redo());

    let outcome = document
        .apply(
            Command::RenameTask {
                id: three,
                name: "B".into(),
            },
            &ctx(),
        )
        .expect("rename");
    assert!(!outcome.can_redo);
    assert!(!document.can_redo());
    assert_eq!(document.redo(&ctx()), Err(DocumentError::NothingToRedo));
}

/// A refused command after an undo does *not* clear the redo list.
///
/// Why it matters: a refusal must leave no trace at all, including in the
/// history.
#[test]
fn refused_command_keeps_redo() {
    let project = shed();
    let two = row_id(&project, 2);
    let mut document = open_ok(project);
    document
        .apply(
            Command::RenameTask {
                id: two,
                name: "A".into(),
            },
            &ctx(),
        )
        .expect("rename");
    document.undo(&ctx()).expect("undo");

    let bad = document.apply(
        Command::RemoveTask {
            id: model::TaskId::new(),
        },
        &ctx(),
    );
    assert!(bad.is_err());
    assert!(document.can_redo(), "redo must survive a refused command");
}

/// Removing "Prepare the ground" (row 4) also removes its two links, which
/// sit in the middle of the link list. Undo gives back the *identical*
/// project: same task position, same links at the same positions.
///
/// Why it matters: if undo put the links back at the end, the project would
/// look the same in the chart but differ in the file, and the change journal
/// would show phantom moves.
#[test]
fn remove_task_with_links_then_undo_is_identical() {
    let project = shed();
    let before = project.clone();
    let prepare = row_id(&project, 4);
    let mut document = open_ok(project);

    document
        .apply(Command::RemoveTask { id: prepare }, &ctx())
        .expect("remove");
    let removed = document.project().clone();
    assert_eq!(removed.dependencies.len(), 6);

    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    assert_eq!(
        document.snapshot(),
        Document::new(before.clone()).snapshot()
    );

    document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &removed);
}

/// Three edits, three undos, three redos: each step lands on the right
/// project.
///
/// Why it matters: the history is a stack; this checks that the order is
/// last-in, first-out and that nothing is lost on the way.
#[test]
fn several_undos_and_redos_walk_the_history() {
    let project = shed();
    let two = row_id(&project, 2);
    let six = row_id(&project, 6);
    let mut states = vec![project.clone()];
    let mut document = open_ok(project);

    let commands = [
        Command::RenameTask {
            id: two,
            name: "Permit".into(),
        },
        Command::SetTaskDuration {
            id: six,
            duration: days(6),
        },
        Command::RemoveTask { id: two },
    ];
    for command in commands {
        document.apply(command, &ctx()).expect("apply");
        states.push(document.project().clone());
    }

    // Walk back: state 3 -> 2 -> 1 -> 0.
    for expected in states.iter().rev().skip(1) {
        document.undo(&ctx()).expect("undo");
        assert_eq!(document.project(), expected);
    }
    assert!(!document.can_undo());
    assert_eq!(document.undo(&ctx()), Err(DocumentError::NothingToUndo));

    // Walk forward again: 0 -> 1 -> 2 -> 3.
    for expected in states.iter().skip(1) {
        document.redo(&ctx()).expect("redo");
        assert_eq!(document.project(), expected);
    }
    assert!(!document.can_redo());
}

/// A fresh document has nothing to undo or redo, and says so with clear
/// errors instead of panicking.
///
/// Why it matters: the Undo button may be pressed at any time (keyboard
/// shortcut), even when it is greyed out.
#[test]
fn empty_history_errors() {
    let mut document = open_ok(shed());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
    assert_eq!(document.undo(&ctx()), Err(DocumentError::NothingToUndo));
    assert_eq!(document.redo(&ctx()), Err(DocumentError::NothingToRedo));
}
