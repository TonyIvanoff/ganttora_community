//! # Every edit command: apply, undo, redo, and its errors
//!
//! ## What this file checks
//! Each of the nine `Command` variants is applied to a real project. Then we
//! undo it (the project must be exactly as before) and redo it (the project
//! must be exactly as after the first apply). For each command we also try
//! the ways it can be wrong (unknown task, bad position, duplicate ID ...)
//! and check the error and that nothing changed.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`).
//!
//! ## Main ideas
//! - "Exactly as before" is checked with `==` on the whole `Project`. Rust can
//!   compare two projects field by field because `Project` derives
//!   `PartialEq`. This also checks the *order* of tasks and links.
//! - A refused command must leave no trace: same project, no undo step, no
//!   change records.

use crate::{ctx, open_ok};
use document::{Command, Document, DocumentError, PlacedDependency};
use ganttora_tests::{days, dt, row_id, shed};
use model::{Dependency, DependencyId, DependencyType, Duration, Project, Task, TaskId};

/// Applies a command, then undoes and redoes it, checking the project each
/// time.
///
/// # Parameters
/// - `project`: the starting project.
/// - `command`: the edit to test.
///
/// # Returns
/// The document after the final redo, and a copy of the project as it was
/// right after the first apply, so the caller can check what the command did.
///
/// # Errors / edge cases
/// Fails the test if the command is refused, if undo does not give back the
/// starting project, or if redo does not give back the edited project.
pub(crate) fn apply_undo_redo(project: Project, command: Command) -> (Document, Project) {
    let before = project.clone();
    let mut document = open_ok(project);

    let outcome = document
        .apply(command, &ctx())
        .unwrap_or_else(|error| panic!("command refused: {error}"));
    assert!(outcome.can_undo, "a real change must be undoable");
    assert!(!outcome.can_redo, "a fresh change leaves nothing to redo");
    let after = document.project().clone();
    assert_ne!(after, before, "the command should have changed the project");

    document.undo(&ctx()).expect("undo should work");
    assert_eq!(document.project(), &before, "undo must restore the project");
    assert!(document.can_redo());

    document.redo(&ctx()).expect("redo should work");
    assert_eq!(document.project(), &after, "redo must repeat the change");
    assert!(document.can_undo());
    assert!(!document.can_redo());

    (document, after)
}

/// Applies a command that must be refused and checks that nothing changed.
///
/// # Parameters
/// - `project`: the starting project.
/// - `command`: the invalid edit.
///
/// # Returns
/// The error the document returned, for the caller to compare.
pub(crate) fn refused(project: Project, command: Command) -> DocumentError {
    let before = project.clone();
    let mut document = Document::new(project);
    let error = match document.apply(command, &ctx()) {
        Ok(_) => panic!("the command should have been refused"),
        Err(error) => error,
    };
    assert_eq!(
        document.project(),
        &before,
        "a refused command changes nothing"
    );
    assert!(!document.can_undo(), "a refused command adds no undo step");
    assert!(
        document.pending_changes().is_empty(),
        "and no change records"
    );
    error
}

/// A task that is not in any project, for "unknown task" errors.
fn unknown_task() -> TaskId {
    TaskId::new()
}

// ---------------------------------------------------------------- AddTask

/// Adding a task at position 2 puts it exactly there; undo removes it and
/// redo adds it back.
///
/// Why it matters: "Insert task" in the grid must place the new row where the
/// user clicked, and Ctrl+Z must take it away again.
#[test]
fn add_task_inserts_at_index_and_undoes() {
    let task = Task::new("Order the door", days(1));
    let command = Command::AddTask {
        index: 2,
        task: task.clone(),
    };
    let (_, after) = apply_undo_redo(shed(), command);
    assert_eq!(after.tasks.len(), 9);
    assert_eq!(after.tasks[2], task);
}

/// A task can be added at the very end (index = number of tasks) and into an
/// empty project.
///
/// Why it matters: index == length is the edge between "valid" and "out of
/// range"; off-by-one mistakes hide exactly there.
#[test]
fn add_task_at_end_and_into_empty_project() {
    let task = Task::new("Last", days(1));
    let (_, after) = apply_undo_redo(
        shed(),
        Command::AddTask {
            index: 8,
            task: task.clone(),
        },
    );
    assert_eq!(after.tasks.last(), Some(&task));

    let empty = ganttora_tests::project_on("2027-03-01");
    let (_, after) = apply_undo_redo(
        empty,
        Command::AddTask {
            index: 0,
            task: task.clone(),
        },
    );
    assert_eq!(after.tasks, vec![task]);
}

/// Adding a task past the end, with an ID that already exists, or with a
/// negative duration is refused with the matching error.
///
/// Why it matters: commands also arrive from the UI as JSON; bad input must
/// be stopped cleanly, never half-applied.
#[test]
fn add_task_errors() {
    let task = Task::new("New", days(1));
    assert_eq!(
        refused(
            shed(),
            Command::AddTask {
                index: 9,
                task: task.clone()
            }
        ),
        DocumentError::IndexOutOfRange { index: 9, len: 8 }
    );

    let project = shed();
    let mut copy = project.tasks[0].clone();
    copy.name = "Copy with the same ID".into();
    let id = copy.id;
    assert_eq!(
        refused(
            project,
            Command::AddTask {
                index: 0,
                task: copy
            }
        ),
        DocumentError::TaskIdExists(id)
    );

    let negative = Task::new("Negative", Duration::from_minutes(-1));
    let id = negative.id;
    assert_eq!(
        refused(
            shed(),
            Command::AddTask {
                index: 0,
                task: negative
            }
        ),
        DocumentError::NegativeDuration(id)
    );
}

// ------------------------------------------------------------- RemoveTask

/// Removing a task without links takes out just that row.
///
/// Why it matters: the simplest delete must not touch anything else.
#[test]
fn remove_task_without_links() {
    let mut project = shed();
    let extra = ganttora_tests::add_task(&mut project, "Loose task", days(1));
    let (_, after) = apply_undo_redo(project, Command::RemoveTask { id: extra });
    assert_eq!(after.tasks.len(), 8);
    assert_eq!(after.dependencies.len(), 8, "no link was touched");
}

/// Removing a task also removes every link that touches it, because a link
/// to a missing task makes no sense.
///
/// Why it matters: otherwise the project would hold "dangling" links and the
/// engine would refuse to schedule it.
#[test]
fn remove_task_removes_its_links() {
    let project = shed();
    let prepare = row_id(&project, 4);
    let (_, after) = apply_undo_redo(project, Command::RemoveTask { id: prepare });
    assert_eq!(after.tasks.len(), 7);
    assert_eq!(after.dependencies.len(), 6, "links 2->4 and 4->5 are gone");
    assert!(
        after
            .dependencies
            .iter()
            .all(|d| d.predecessor != prepare && d.successor != prepare)
    );
}

/// Removing a task that does not exist gives `TaskNotFound`.
///
/// Why it matters: the UI may send an old ID (e.g. after another edit); the
/// document must say so instead of crashing.
#[test]
fn remove_task_unknown() {
    let id = unknown_task();
    assert_eq!(
        refused(shed(), Command::RemoveTask { id }),
        DocumentError::TaskNotFound(id)
    );
}

// ------------------------------------------------------------ RestoreTask

/// `RestoreTask` (the command undo uses after a delete) puts the task and its
/// links back at their old positions; sent on its own it behaves like any
/// other command.
///
/// Why it matters: it is a public command and can arrive as JSON, so it must
/// work, and undo, on its own, not only inside the undo machinery.
#[test]
fn restore_task_puts_task_and_links_back() {
    let original = shed();
    let prepare_index = 3;
    let task = original.tasks[prepare_index].clone();
    let placed: Vec<PlacedDependency> = original
        .dependencies
        .iter()
        .enumerate()
        .filter(|(_, d)| d.predecessor == task.id || d.successor == task.id)
        .map(|(index, d)| PlacedDependency {
            index,
            dependency: d.clone(),
        })
        .collect();

    // The project without task 4 and its two links.
    let mut removed = original.clone();
    removed.tasks.remove(prepare_index);
    removed
        .dependencies
        .retain(|d| d.predecessor != task.id && d.successor != task.id);

    let (_, after) = apply_undo_redo(
        removed,
        Command::RestoreTask {
            index: prepare_index,
            task,
            dependencies: placed,
        },
    );
    assert_eq!(after, original, "restore gives back the identical project");
}

/// `RestoreTask` refuses a link that does not touch the restored task.
///
/// Why it matters: restoring must only bring back the task's own links; a
/// stray link inside the command means the command is corrupt.
#[test]
fn restore_task_refuses_foreign_link() {
    let project = shed();
    let task = Task::new("Restored", days(1));
    let foreign = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4));
    let foreign_id = foreign.id;
    let task_id = task.id;
    assert_eq!(
        refused(
            project,
            Command::RestoreTask {
                index: 0,
                task,
                dependencies: vec![PlacedDependency {
                    index: 0,
                    dependency: foreign
                }],
            }
        ),
        DocumentError::LinkNotOnTask {
            dependency: foreign_id,
            task: task_id
        }
    );
}

// ------------------------------------------------------------- RenameTask

/// Renaming changes only the name; undo brings the old name back.
///
/// Why it matters: the most common edit in the grid.
#[test]
fn rename_task() {
    let project = shed();
    let id = row_id(&project, 7);
    let (_, after) = apply_undo_redo(
        project,
        Command::RenameTask {
            id,
            name: "Paint (two coats)".into(),
        },
    );
    assert_eq!(after.tasks[6].name, "Paint (two coats)");
}

/// Renaming an unknown task gives `TaskNotFound`.
///
/// Why it matters: same as for delete, a stale ID must give a clear error.
#[test]
fn rename_task_unknown() {
    let id = unknown_task();
    assert_eq!(
        refused(
            shed(),
            Command::RenameTask {
                id,
                name: "x".into()
            }
        ),
        DocumentError::TaskNotFound(id)
    );
}

// -------------------------------------------------------- SetTaskDuration

/// Changing a duration changes the task and reschedules the project.
///
/// Why it matters: this is how the user moves a plan; the dates must follow.
#[test]
fn set_task_duration() {
    let project = shed();
    let id = row_id(&project, 6);
    let (document, after) = apply_undo_redo(
        project,
        Command::SetTaskDuration {
            id,
            duration: days(5),
        },
    );
    assert_eq!(after.tasks[5].duration, days(5));
    let schedule = document.schedule().expect("still schedules");
    assert_eq!(schedule.project_finish, dt("2027-03-24 17:00"));
}

/// A negative duration and an unknown task are refused.
///
/// Why it matters: a duration below zero has no meaning; the engine would
/// refuse it later anyway, but the document must say why straight away.
#[test]
fn set_task_duration_errors() {
    let project = shed();
    let id = row_id(&project, 2);
    assert_eq!(
        refused(
            project,
            Command::SetTaskDuration {
                id,
                duration: Duration::from_minutes(-480)
            }
        ),
        DocumentError::NegativeDuration(id)
    );
    let unknown = unknown_task();
    assert_eq!(
        refused(
            shed(),
            Command::SetTaskDuration {
                id: unknown,
                duration: days(1)
            }
        ),
        DocumentError::TaskNotFound(unknown)
    );
}

/// A duration of zero is allowed: the task becomes a milestone.
///
/// Why it matters: zero is the boundary of the "negative" check; it must be
/// accepted, not refused.
#[test]
fn set_task_duration_zero_is_allowed() {
    let project = shed();
    let id = row_id(&project, 7);
    let (_, after) = apply_undo_redo(
        project,
        Command::SetTaskDuration {
            id,
            duration: Duration::ZERO,
        },
    );
    assert!(after.tasks[6].duration.is_zero());
}

// ---------------------------------------------------------- AddDependency

/// A new link is appended to the end of the link list.
///
/// Why it matters: the link list order is part of the project; undo/redo
/// must keep it stable.
#[test]
fn add_dependency() {
    let project = shed();
    let link = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4));
    let (_, after) = apply_undo_redo(
        project,
        Command::AddDependency {
            dependency: link.clone(),
        },
    );
    assert_eq!(after.dependencies.len(), 9);
    assert_eq!(after.dependencies.last(), Some(&link));
}

/// Every invalid link is refused with its own error: self-link, a second
/// link between the same two tasks, a reused link ID, an unknown task.
///
/// Why it matters: these are the rules of the `dependencies` table too
/// (CHECK and UNIQUE in the .gtra file); catching them here gives the user a
/// clear message instead of a database error at save time.
#[test]
fn add_dependency_errors() {
    let project = shed();
    let one = row_id(&project, 1);
    let two = row_id(&project, 2);

    assert_eq!(
        refused(
            project.clone(),
            Command::AddDependency {
                dependency: Dependency::finish_to_start(one, one)
            }
        ),
        DocumentError::SelfLink(one)
    );

    assert_eq!(
        refused(
            project.clone(),
            Command::AddDependency {
                dependency: Dependency::finish_to_start(one, two)
            }
        ),
        DocumentError::LinkExists {
            predecessor: one,
            successor: two
        }
    );

    let mut reused = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4));
    reused.id = project.dependencies[0].id;
    let reused_id = reused.id;
    assert_eq!(
        refused(
            project.clone(),
            Command::AddDependency { dependency: reused }
        ),
        DocumentError::DependencyIdExists(reused_id)
    );

    let unknown = unknown_task();
    assert_eq!(
        refused(
            project,
            Command::AddDependency {
                dependency: Dependency::finish_to_start(one, unknown)
            }
        ),
        DocumentError::TaskNotFound(unknown)
    );
}

// ------------------------------------------------------- RemoveDependency

/// Removing a link takes it out of the list; undo puts it back at the same
/// position (not at the end).
///
/// Why it matters: the comparison with `==` after undo also checks the
/// position, so a link that comes back at the wrong place fails here.
#[test]
fn remove_dependency_from_the_middle() {
    let project = shed();
    let middle = project.dependencies[3].id;
    let (_, after) = apply_undo_redo(project, Command::RemoveDependency { id: middle });
    assert_eq!(after.dependencies.len(), 7);
    assert!(after.dependencies.iter().all(|d| d.id != middle));
}

/// Removing an unknown link gives `DependencyNotFound`.
///
/// Why it matters: a stale link ID must give a clear error.
#[test]
fn remove_dependency_unknown() {
    let id = DependencyId::new();
    assert_eq!(
        refused(shed(), Command::RemoveDependency { id }),
        DocumentError::DependencyNotFound(id)
    );
}

// ------------------------------------------------------ RestoreDependency

/// `RestoreDependency` inserts a link at a chosen position.
///
/// Why it matters: it is what undo uses after a link delete; on its own it
/// must work and be undoable like any command.
#[test]
fn restore_dependency_at_position() {
    let project = shed();
    let link = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4));
    let (_, after) = apply_undo_redo(
        project,
        Command::RestoreDependency {
            placed: PlacedDependency {
                index: 0,
                dependency: link.clone(),
            },
        },
    );
    assert_eq!(after.dependencies[0], link);
}

/// A position past the end of the link list is refused.
///
/// Why it matters: same off-by-one edge as for tasks.
#[test]
fn restore_dependency_out_of_range() {
    let project = shed();
    let link = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4));
    assert_eq!(
        refused(
            project,
            Command::RestoreDependency {
                placed: PlacedDependency {
                    index: 9,
                    dependency: link
                }
            }
        ),
        DocumentError::IndexOutOfRange { index: 9, len: 8 }
    );
}

// -------------------------------------------------------- SetProjectStart

/// Moving the project start one week later moves the whole schedule one
/// week later.
///
/// Why it matters: "the project starts next Monday instead" is a common
/// change; every date must follow.
#[test]
fn set_project_start_moves_everything() {
    let (document, after) = apply_undo_redo(
        shed(),
        Command::SetProjectStart {
            start: dt("2027-03-08 08:00"),
        },
    );
    assert_eq!(after.start, dt("2027-03-08 08:00"));
    let schedule = document.schedule().expect("still schedules");
    assert_eq!(schedule.project_start, dt("2027-03-08 08:00"));
    assert_eq!(schedule.project_finish, dt("2027-03-30 17:00"));
}

// ------------------------------------- edits that would break the schedule

/// Switching to "schedule from the finish" without giving a finish date is
/// refused with `Rejected(ProjectFinishMissing)`: the schedule worked before,
/// so a change that breaks it is not let in.
///
/// Why it matters: the document must never leave the user without a
/// schedule because of one edit, and it must say why (as an error code the
/// UI can translate). In M0 this test used a start-to-start link, which is
/// no longer an error since M1.
#[test]
fn schedule_breaking_edit_is_refused_when_schedule_worked() {
    let error = refused(
        shed(),
        Command::SetScheduleFrom {
            schedule_from: model::ScheduleFrom::Finish,
            finish: None,
        },
    );
    assert_eq!(
        error,
        DocumentError::Rejected(engine::ScheduleError::ProjectFinishMissing)
    );
}

/// Since M1 a start-to-start link is an ordinary edit: it is accepted, can
/// be undone and redone, and the successor now waits only for the
/// predecessor's *start*.
///
/// Why it matters: this replaces the M0 rule "SS links are refused". In the
/// shed, task 4 "Prepare the ground" normally waits for task 2 (Permit,
/// Wed 3 – Tue 9 Mar). An extra SS link 3→4 from "Buy materials" (starts
/// Wed 3 Mar 08:00) asks for start ≥ Wed 3 Mar 08:00, which is earlier, so
/// task 4 keeps its start Wed 10 Mar 08:00.
#[test]
fn start_to_start_link_is_accepted_since_m1() {
    let project = shed();
    let link = Dependency::new(
        row_id(&project, 3),
        row_id(&project, 4),
        DependencyType::StartToStart,
    );
    let four = row_id(&project, 4);
    let (document, after) = apply_undo_redo(project, Command::AddDependency { dependency: link });
    assert_eq!(after.dependencies.len(), 9);
    let schedule = document.schedule().expect("SS links schedule since M1");
    assert_eq!(
        schedule.task(four).expect("task 4").start,
        dt("2027-03-10 08:00")
    );
}
