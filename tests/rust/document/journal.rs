//! # Change records: the raw material of the change journal
//!
//! ## What this file checks
//! Every successful command, undo and redo produces *change records*: one per
//! changed field, saying which item changed, which field, the old and new
//! value, and who/when/which action. These tests check the count, the fields
//! and the values for each kind of command, and that the context (author,
//! timestamp, batch) is copied into every record.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! records are later written to the `journal` table of the .gtra file
//! ("change journal").
//!
//! ## Main ideas
//! - Durations and lags are recorded in **minutes** (a plain number), so the
//!   journal does not depend on how many hours a "day" has.
//! - Created items have `null` as the old value; deleted items have `null`
//!   as the new value.

use crate::{ctx, ctx_with, open_ok, utc};
use document::{ChangeRecord, Command, EntityKind};
use ganttora_tests::{days, dt, row_id, shed};
use model::{Dependency, Task};
use serde_json::{Value, json};
use uuid::Uuid;

/// Finds the record for one field, or fails the test.
///
/// # Parameters
/// - `records`: the records to search.
/// - `entity_id`: the UUID of the changed item.
/// - `field`: the field name, e.g. `"duration"`.
///
/// # Returns
/// The matching record.
fn record<'a>(records: &'a [ChangeRecord], entity_id: Uuid, field: &str) -> &'a ChangeRecord {
    records
        .iter()
        .find(|r| r.entity_id == entity_id && r.field == field)
        .unwrap_or_else(|| panic!("no record for field {field:?} of {entity_id}"))
}

/// Changing task 2 from 5 to 6 days gives exactly one record: entity Task,
/// field "duration", old 2400 minutes (5 x 480), new 2880 minutes (6 x 480),
/// with the author, timestamp and batch of the context.
///
/// Why it matters: this is the line the journal will show as "Anna changed
/// the duration of 'Get building permit' from 5 days to 6 days on ...".
#[test]
fn set_duration_record() {
    let project = shed();
    let id = row_id(&project, 2);
    let mut document = open_ok(project);
    let context = ctx_with("Anna", "2027-02-15 10:42", 7);

    let outcome = document
        .apply(
            Command::SetTaskDuration {
                id,
                duration: days(6),
            },
            &context,
        )
        .expect("apply");

    assert_eq!(outcome.changes.len(), 1);
    let change = &outcome.changes[0];
    assert_eq!(change.entity, EntityKind::Task);
    assert_eq!(change.entity_id, id.as_uuid());
    assert_eq!(change.field, "duration_minutes");
    assert_eq!(change.old_value, json!(2400));
    assert_eq!(change.new_value, json!(2880));
    assert_eq!(change.author, "Anna");
    assert_eq!(change.timestamp, utc("2027-02-15 10:42"));
    assert_eq!(change.batch_id, context.batch_id);
}

/// Undo produces the *reverse* record (old 2880, new 2400), stamped with the
/// undo's own context, not the original one.
///
/// Why it matters: the journal is a true history: "10:42 Anna: 5 -> 6 days",
/// then "10:43 Anna: 6 -> 5 days (undo)". It never rewrites the past.
#[test]
fn undo_and_redo_records_are_reversed_and_restamped() {
    let project = shed();
    let id = row_id(&project, 2);
    let mut document = open_ok(project);
    document
        .apply(
            Command::SetTaskDuration {
                id,
                duration: days(6),
            },
            &ctx_with("Anna", "2027-02-15 10:42", 1),
        )
        .expect("apply");

    let undo_ctx = ctx_with("Anna", "2027-02-15 10:43", 2);
    let undone = document.undo(&undo_ctx).expect("undo");
    assert_eq!(undone.changes.len(), 1);
    assert_eq!(undone.changes[0].old_value, json!(2880));
    assert_eq!(undone.changes[0].new_value, json!(2400));
    assert_eq!(undone.changes[0].timestamp, undo_ctx.timestamp);
    assert_eq!(undone.changes[0].batch_id, undo_ctx.batch_id);

    let redo_ctx = ctx_with("Ben", "2027-02-15 10:44", 3);
    let redone = document.redo(&redo_ctx).expect("redo");
    assert_eq!(redone.changes[0].old_value, json!(2400));
    assert_eq!(redone.changes[0].new_value, json!(2880));
    assert_eq!(redone.changes[0].author, "Ben");
}

/// Records collect in `pending_changes` until the app takes them (to write
/// them to the file); `take_pending_changes` hands them over and empties the
/// list.
///
/// Why it matters: if records were lost or handed over twice, the journal in
/// the file would miss changes or show them twice.
#[test]
fn pending_changes_accumulate_and_are_taken_once() {
    let project = shed();
    let two = row_id(&project, 2);
    let mut document = open_ok(project);
    document
        .apply(
            Command::RenameTask {
                id: two,
                name: "Permit".into(),
            },
            &ctx(),
        )
        .expect("rename");
    document.undo(&ctx()).expect("undo");
    assert_eq!(document.pending_changes().len(), 2);

    let taken = document.take_pending_changes();
    assert_eq!(taken.len(), 2);
    assert_eq!(taken[0].new_value, json!("Permit"));
    assert_eq!(taken[1].new_value, json!("Get building permit"));
    assert!(document.pending_changes().is_empty());
    assert!(document.take_pending_changes().is_empty());
}

/// A rename gives one record with field "name" and the old and new text.
///
/// Why it matters: text values are stored as JSON strings in the journal.
#[test]
fn rename_record() {
    let project = shed();
    let id = row_id(&project, 7);
    let mut document = open_ok(project);
    let outcome = document
        .apply(
            Command::RenameTask {
                id,
                name: "Varnish".into(),
            },
            &ctx(),
        )
        .expect("rename");
    assert_eq!(outcome.changes.len(), 1);
    let change = &outcome.changes[0];
    assert_eq!(change.field, "name");
    assert_eq!(change.old_value, json!("Paint"));
    assert_eq!(change.new_value, json!("Varnish"));
}

/// Adding a task gives twelve records, one per stored field (index, name,
/// duration_minutes, outline_level, mode, manual_start, milestone,
/// constraint, deadline, calendar_id, notes, hyperlink), each with `null` as
/// the old value. Field names are the model's snake_case names (M1 decision).
///
/// Why it matters: a "created" entry in the journal must carry every value
/// of the new item, so the journal alone can tell what was added.
#[test]
fn add_task_records() {
    let mut document = open_ok(shed());
    let task = Task::new("Order the door", days(1));
    let id = task.id.as_uuid();
    let outcome = document
        .apply(Command::AddTask { index: 2, task }, &ctx())
        .expect("add");

    assert_eq!(outcome.changes.len(), 12);
    for change in &outcome.changes {
        assert_eq!(change.entity, EntityKind::Task);
        assert_eq!(change.entity_id, id);
        assert_eq!(change.old_value, Value::Null, "field {}", change.field);
    }
    let r = &outcome.changes;
    assert_eq!(record(r, id, "index").new_value, json!(2));
    assert_eq!(record(r, id, "name").new_value, json!("Order the door"));
    assert_eq!(record(r, id, "duration_minutes").new_value, json!(480));
    assert_eq!(record(r, id, "outline_level").new_value, json!(1));
    assert_eq!(record(r, id, "mode").new_value, json!("auto"));
    assert_eq!(record(r, id, "manual_start").new_value, Value::Null);
    assert_eq!(record(r, id, "milestone").new_value, json!(false));
    assert_eq!(
        record(r, id, "constraint").new_value,
        json!({ "type": "asSoonAsPossible" })
    );
    assert_eq!(record(r, id, "deadline").new_value, Value::Null);
    assert_eq!(record(r, id, "calendar_id").new_value, Value::Null);
    assert_eq!(record(r, id, "notes").new_value, json!(""));
    assert_eq!(record(r, id, "hyperlink").new_value, json!(""));
}

/// Removing "Prepare the ground" (row 4) gives 24 records: 6 for each of its
/// two links (index, predecessor, successor, kind, lag_minutes, lag_percent)
/// and 12 for the task.
/// Every new value is `null`; the old values describe what was deleted.
///
/// Why it matters: a delete must record everything needed to understand (and
/// one day reconstruct) what was lost, including the links that went with it.
#[test]
fn remove_task_records_include_its_links() {
    let project = shed();
    let prepare = row_id(&project, 4);
    let link_in = project.dependencies[2].clone(); // 2 -> 4, at position 2
    let link_out = project.dependencies[4].clone(); // 4 -> 5, at position 4
    assert_eq!(link_in.successor, prepare);
    assert_eq!(link_out.predecessor, prepare);
    let mut document = open_ok(project);

    let outcome = document
        .apply(Command::RemoveTask { id: prepare }, &ctx())
        .expect("remove");
    let r = &outcome.changes;
    assert_eq!(r.len(), 24);
    assert!(r.iter().all(|c| c.new_value == Value::Null));
    assert_eq!(
        r.iter()
            .filter(|c| c.entity == EntityKind::Dependency)
            .count(),
        12
    );

    let id = link_in.id.as_uuid();
    assert_eq!(record(r, id, "index").old_value, json!(2));
    assert_eq!(
        record(r, id, "predecessor").old_value,
        json!(link_in.predecessor.to_string())
    );
    assert_eq!(
        record(r, id, "successor").old_value,
        json!(prepare.to_string())
    );
    assert_eq!(record(r, id, "kind").old_value, json!("finishToStart"));
    assert_eq!(record(r, id, "lag_minutes").old_value, json!(0));
    assert_eq!(record(r, id, "lag_percent").old_value, Value::Null);
    assert_eq!(
        record(r, link_out.id.as_uuid(), "index").old_value,
        json!(4)
    );

    let task = prepare.as_uuid();
    assert_eq!(record(r, task, "index").old_value, json!(3));
    assert_eq!(
        record(r, task, "name").old_value,
        json!("Prepare the ground")
    );
    assert_eq!(record(r, task, "duration_minutes").old_value, json!(960));
}

/// Adding a link with a lag of 2 hours records the lag as 120 minutes.
///
/// Why it matters: lags, like durations, are stored in minutes.
#[test]
fn add_dependency_records_lag_in_minutes() {
    let project = shed();
    let link = Dependency::finish_to_start(row_id(&project, 3), row_id(&project, 4))
        .with_lag(ganttora_tests::hours(2));
    let id = link.id.as_uuid();
    let mut document = open_ok(project);
    let outcome = document
        .apply(Command::AddDependency { dependency: link }, &ctx())
        .expect("add link");
    assert_eq!(outcome.changes.len(), 6);
    assert!(
        outcome
            .changes
            .iter()
            .all(|c| c.entity == EntityKind::Dependency && c.old_value == Value::Null)
    );
    assert_eq!(
        record(&outcome.changes, id, "lag_minutes").new_value,
        json!(120)
    );
    assert_eq!(record(&outcome.changes, id, "index").new_value, json!(8));
}

/// Moving the project start gives one record on the *project* entity, with
/// both dates as ISO text ("2027-03-01T00:00:00" -> "2027-03-08T08:00:00").
///
/// Why it matters: the project itself is an entity in the journal, with its
/// own UUID.
#[test]
fn set_project_start_record() {
    let project = shed();
    let project_id = project.id.as_uuid();
    let mut document = open_ok(project);
    let outcome = document
        .apply(
            Command::SetProjectStart {
                start: dt("2027-03-08 08:00"),
            },
            &ctx(),
        )
        .expect("move start");
    assert_eq!(outcome.changes.len(), 1);
    let change = &outcome.changes[0];
    assert_eq!(change.entity, EntityKind::Project);
    assert_eq!(change.entity_id, project_id);
    assert_eq!(change.field, "start");
    assert_eq!(change.old_value, json!("2027-03-01T00:00:00"));
    assert_eq!(change.new_value, json!("2027-03-08T08:00:00"));
}

/// Every record of one action carries the same author, timestamp and batch.
///
/// Why it matters: the batch ID groups the 24 records of one delete into one
/// line "Anna deleted 'Prepare the ground'" in the journal view.
#[test]
fn all_records_of_one_action_share_the_context() {
    let project = shed();
    let prepare = row_id(&project, 4);
    let mut document = open_ok(project);
    let context = ctx_with("Anna", "2027-02-15 11:00", 42);
    let outcome = document
        .apply(Command::RemoveTask { id: prepare }, &context)
        .expect("remove");
    for change in &outcome.changes {
        assert_eq!(change.author, "Anna");
        assert_eq!(change.timestamp, context.timestamp);
        assert_eq!(change.batch_id, context.batch_id);
    }
}
