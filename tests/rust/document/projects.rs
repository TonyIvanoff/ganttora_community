//! Tests for project rows and percent complete.
//!
//! What they check: every task stays inside a project, projects stay at the
//! top level, and groups average the percent of their tasks.

use chrono::{DateTime, NaiveDate, Utc};
use document::{BatchId, ChangeContext, Command, Document, DocumentError};
use model::{Duration, Project, Task, TaskId};

/// Two projects: A with a 1-day and a 3-day task, B with one task.
fn two_projects() -> Document {
    let mut p = Project::new("A", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    p.tasks.push(Task::new("a1", Duration::from_days(1)));
    p.tasks.push(Task::new("a2", Duration::from_days(3)));
    p.ensure_project_row();
    p.tasks.push(Task::project_row("B"));
    p.tasks
        .push(Task::new("b1", Duration::from_days(2)).at_level(2));
    Document::new(p)
}

/// The ID of a row.
fn id(doc: &Document, row: usize) -> TaskId {
    doc.project().tasks[row].id
}

/// Applies a command.
fn apply(doc: &mut Document, command: Command) -> Result<(), DocumentError> {
    let context = ChangeContext {
        author: "Sam".into(),
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
        batch_id: BatchId::new(),
    };
    doc.apply(command, &context).map(|_| ())
}

#[test]
fn groups_average_their_tasks_by_duration() {
    let mut doc = two_projects();
    let a1 = id(&doc, 1);
    apply(
        &mut doc,
        Command::SetTaskPercentComplete {
            id: a1,
            percent: 100,
        },
    )
    .unwrap();
    let rows = doc.snapshot().tasks;
    assert_eq!(rows[1].percent_complete, 100);
    assert_eq!(rows[0].percent_complete, 25); // 1 of 4 days
    assert_eq!(rows[3].percent_complete, 0);
}

#[test]
fn percent_is_refused_on_projects_and_above_100() {
    let mut doc = two_projects();
    for (row, percent) in [(0, 50), (1, 101)] {
        let id = id(&doc, row);
        let error = apply(&mut doc, Command::SetTaskPercentComplete { id, percent }).unwrap_err();
        assert_eq!(error, DocumentError::InvalidPercent(id));
    }
}

#[test]
fn tasks_cannot_leave_and_projects_cannot_enter() {
    let mut doc = two_projects();
    let (a1, b) = (id(&doc, 1), id(&doc, 3));
    for command in [
        Command::OutdentTask { id: a1 },
        Command::IndentTask { id: b },
        // Between the tasks of project A.
        Command::MoveTask {
            id: b,
            to_index: 2,
            level: None,
        },
    ] {
        let error = apply(&mut doc, command).unwrap_err();
        assert!(
            matches!(error, DocumentError::ProjectOutline { .. }),
            "{error}"
        );
    }
}

#[test]
fn a_task_dropped_above_a_project_joins_the_one_before() {
    let mut doc = two_projects();
    let b1 = id(&doc, 4);
    apply(
        &mut doc,
        Command::MoveTask {
            id: b1,
            to_index: 3,
            level: None,
        },
    )
    .unwrap();
    assert_eq!(doc.project().tasks[3].id, b1);
    assert_eq!(doc.project().tasks[3].outline_level, 2);
}

#[test]
fn projects_can_change_places() {
    let mut doc = two_projects();
    let b = id(&doc, 3);
    apply(
        &mut doc,
        Command::MoveTask {
            id: b,
            to_index: 0,
            level: None,
        },
    )
    .unwrap();
    assert_eq!(doc.project().tasks[0].name, "B");
    assert_eq!(doc.project().tasks[2].name, "A");
}

#[test]
fn deleting_a_project_and_undoing_brings_everything_back() {
    let mut doc = two_projects();
    let before = doc.project().clone();
    let a = id(&doc, 0);
    apply(&mut doc, Command::RemoveTask { id: a }).unwrap();
    assert_eq!(doc.project().tasks.len(), 2);
    let context = ChangeContext {
        author: "Sam".into(),
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
        batch_id: BatchId::new(),
    };
    doc.undo(&context).unwrap();
    assert_eq!(doc.project(), &before);
}

#[test]
fn an_empty_project_is_not_a_milestone() {
    let mut p = Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    p.ensure_project_row();
    let row = Document::new(p).snapshot().tasks.remove(0);
    assert!(row.is_project && !row.is_milestone && !row.is_summary);
}

#[test]
fn lowering_the_percent_clears_the_actual_finish_as_one_step() {
    let mut doc = two_projects();
    let a1 = id(&doc, 1);
    let day = NaiveDate::from_ymd_opt(2027, 3, 2)
        .unwrap()
        .and_hms_opt(17, 0, 0);
    let percent = |percent| Command::SetTaskPercentComplete { id: a1, percent };
    apply(&mut doc, percent(100)).unwrap();
    apply(
        &mut doc,
        Command::SetTaskActualFinish {
            id: a1,
            actual_finish: day,
        },
    )
    .unwrap();
    assert_eq!(doc.snapshot().tasks[1].actual_finish, day);
    let finish = doc.snapshot().tasks[1].finish;
    apply(&mut doc, percent(50)).unwrap();
    assert_eq!(doc.snapshot().tasks[1].actual_finish, None);
    assert_eq!(doc.snapshot().tasks[1].finish, finish); // never moves dates
    let context = ChangeContext {
        author: "Sam".into(),
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
        batch_id: BatchId::new(),
    };
    doc.undo(&context).unwrap();
    let row = &doc.snapshot().tasks[1];
    assert_eq!((row.actual_finish, row.percent_complete), (day, 100));
}

#[test]
fn the_planned_finish_stays_when_the_schedule_moves() {
    let mut doc = two_projects();
    let a1 = id(&doc, 1);
    let planned = doc.snapshot().tasks[1].finish;
    apply(
        &mut doc,
        Command::SetTaskPlannedFinish {
            id: a1,
            planned_finish: planned,
        },
    )
    .unwrap();
    apply(
        &mut doc,
        Command::SetTaskDuration {
            id: a1,
            duration: Duration::from_days(4),
        },
    )
    .unwrap();
    let row = &doc.snapshot().tasks[1];
    assert_eq!(row.planned_finish, planned);
    assert!(row.finish > planned);
}
