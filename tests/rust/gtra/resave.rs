//! # Saving again after edits
//!
//! ## What this file checks
//! After the first save, the app saves into the *existing* file: it updates
//! the rows that changed, adds new ones and deletes removed ones. These tests
//! edit the shed in many ways (through the `Document`, like the app does),
//! save again into the same file and check that the file then holds exactly
//! the edited project, that the creation date is kept and the "modified"
//! date and author are updated.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).

use crate::{files_in, normalized, open_ok, path_in, save_ok, saved_at, temp_dir};
use document::{BatchId, ChangeContext, Command, Document};
use ganttora_tests::{date, days, row_id, shed};
use gtra::SaveOptions;
use model::{Calendar, Dependency, Task, WorkWeek};

/// A fixed edit context (who/when) for the document.
fn ctx() -> ChangeContext {
    ChangeContext {
        author: "Tester".into(),
        timestamp: saved_at("2027-02-15 11:00"),
        batch_id: BatchId::from_uuid(uuid::Uuid::from_u128(1)),
    }
}

/// Edits through the document (rename, new duration, delete a task with
/// links, add a task, add a link, move the start), then saves into the same
/// file: the file now holds exactly the edited project.
///
/// Why it matters: this is the everyday "edit, Ctrl+S, edit, Ctrl+S" cycle.
/// Stale rows left in the file (for example the deleted task's links) would
/// make the next open fail or show ghosts.
#[test]
fn resave_after_document_edits() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);

    let mut document = Document::new(open_ok(&path).project);
    let permit = row_id(&project, 2);
    let prepare = row_id(&project, 4);
    let materials = row_id(&project, 3);
    let paint = row_id(&project, 7);
    let new_task = Task::new("Install shelves", days(1));
    let commands = [
        Command::RenameTask {
            id: permit,
            name: "Permit (council)".into(),
        },
        Command::SetTaskDuration {
            id: permit,
            duration: days(6),
        },
        Command::RemoveTask { id: prepare },
        Command::AddTask {
            index: 6,
            task: new_task.clone(),
        },
        Command::AddDependency {
            dependency: Dependency::finish_to_start(materials, new_task.id),
        },
        Command::AddDependency {
            dependency: Dependency::finish_to_start(new_task.id, paint),
        },
        Command::SetProjectStart {
            start: ganttora_tests::dt("2027-03-08 08:00"),
        },
    ];
    for command in commands {
        document
            .apply(command.clone(), &ctx())
            .unwrap_or_else(|e| panic!("{command:?}: {e}"));
    }
    let edited = document.project().clone();

    gtra::save(
        &path,
        &edited,
        &SaveOptions::new(saved_at("2027-02-16 09:30"), "Anna"),
    )
    .expect("second save");
    let opened = open_ok(&path);
    assert_eq!(normalized(&opened.project), normalized(&edited));
    assert!(opened.notes.is_empty(), "notes: {:?}", opened.notes);

    // The creation date stays; modified and author change.
    assert_eq!(opened.info.created.as_deref(), Some("2027-02-15T10:00:00Z"));
    assert_eq!(
        opened.info.modified.as_deref(),
        Some("2027-02-16T09:30:00Z")
    );
    assert_eq!(opened.info.author.as_deref(), Some("Anna"));
    assert_eq!(files_in(dir.path()), vec!["shed.gtra".to_string()]);
}

/// Undoing everything and saving again gives back the original project.
///
/// Why it matters: rows deleted in one save and re-added in the next (same
/// IDs) must not clash with leftovers in the file.
#[test]
fn resave_after_undo_restores_original() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);

    let mut document = Document::new(project.clone());
    let prepare = row_id(&project, 4);
    document
        .apply(Command::RemoveTask { id: prepare }, &ctx())
        .expect("remove");
    save_ok(&path, document.project());
    assert_eq!(open_ok(&path).project.tasks.len(), 7);

    document.undo(&ctx()).expect("undo");
    save_ok(&path, document.project());
    assert_eq!(normalized(&open_ok(&path).project), normalized(&project));
}

/// Changing the outline (indenting tasks under a new summary task, then
/// outdenting them again) and re-saving stores the new parents correctly.
///
/// Why it matters: the file stores the outline as parent/child links; moving
/// a task to another parent is an update, not an insert.
#[test]
fn resave_after_outline_changes() {
    let dir = temp_dir();
    let path = path_in(&dir, "outline.gtra");
    let mut project = shed();
    save_ok(&path, &project);

    // Rows 2..=4 become children of row 1, row 3 a grandchild.
    project.tasks[1].outline_level = 2;
    project.tasks[2].outline_level = 3;
    project.tasks[3].outline_level = 2;
    save_ok(&path, &project);
    assert_eq!(normalized(&open_ok(&path).project), normalized(&project));

    for task in &mut project.tasks {
        task.outline_level = 1;
    }
    save_ok(&path, &project);
    assert_eq!(normalized(&open_ok(&path).project), normalized(&project));
}

/// Adding, removing and changing holidays and calendars between saves.
///
/// Why it matters: holidays are stored as separate rows; a removed holiday
/// that stayed in the file would block a working day forever.
#[test]
fn resave_after_calendar_changes() {
    let dir = temp_dir();
    let path = path_in(&dir, "calendars.gtra");
    let mut project = shed();
    project.calendars[0].holidays.insert(date("2027-03-08"));
    project.calendars[0].holidays.insert(date("2027-03-09"));
    let extra = Calendar::new("Extra", WorkWeek::standard());
    project.calendars.push(extra);
    save_ok(&path, &project);

    // Remove one holiday, add another, rename the standard calendar, drop
    // the extra calendar, change the work week.
    let standard = &mut project.calendars[0];
    standard.holidays.remove(&date("2027-03-09"));
    standard.holidays.insert(date("2027-03-17"));
    standard.name = "Office".into();
    standard.week.friday.clear();
    project.calendars.truncate(1);
    save_ok(&path, &project);

    let opened = open_ok(&path).project;
    assert_eq!(normalized(&opened), normalized(&project));
    assert_eq!(opened.calendars.len(), 1);
}

/// Saving a *different* project (another project ID) over an existing file
/// replaces the file completely: nothing of the old project remains, and the
/// creation date is the new save's date.
///
/// Why it matters: "Save As" onto an existing file must not mix two projects.
#[test]
fn saving_a_different_project_replaces_the_file() {
    let dir = temp_dir();
    let path = path_in(&dir, "plan.gtra");
    save_ok(&path, &shed());

    let mut other = ganttora_tests::project_on("2027-05-03");
    ganttora_tests::add_task(&mut other, "Only task", days(3));
    gtra::save(
        &path,
        &other,
        &SaveOptions::new(saved_at("2027-03-01 12:00"), "Ben"),
    )
    .expect("save other project");

    let opened = open_ok(&path);
    assert_eq!(normalized(&opened.project), normalized(&other));
    assert_eq!(opened.info.created.as_deref(), Some("2027-03-01T12:00:00Z"));
}
