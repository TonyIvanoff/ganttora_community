//! # Blockers in the `.gtra` file (format 3)
//!
//! ## What this file checks
//! - Blockers survive save → open unchanged: every field, `reported_on`
//!   both `Some` and `None`, and the **order** of each task's list.
//! - A **format-2 file** (written before blockers existed) is upgraded when
//!   opened: `upgraded_from == Some(2)`, tasks have no blockers.
//! - Bad rows written by another tool are reported, not hidden:
//!   a blocker on a summary task → `ValueNotLoaded` (table `blockers`,
//!   column `task_id`); negative values → `ValueNotLoaded`; rows whose task
//!   is missing → `RowsNotLoaded`, and such rows are **kept** when the file
//!   is saved again.
//! - Two blockers with the same ID are refused on save.
//! - Journal records whose values are JSON objects (the `"blocker"` field)
//!   round-trip through the file.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`). Code under
//! test: `backend/crates/gtra/src/read.rs`, `write.rs` and the migration
//! `migrations/0003_blockers.sql`.
//!
//! ## Main ideas
//! Bad rows are made with plain SQL on a saved file (with foreign keys
//! switched off where needed), because the public API never writes them.

use crate::{normalized, open_ok, path_in, save_ok, temp_dir};
use chrono::{TimeZone, Utc};
use ganttora_tests::{date, days, shed};
use gtra::schema::CURRENT_FORMAT_VERSION;
use gtra::{GtraError, JournalFilter, ReadNote};
use model::{BatchId, Blocker, BlockerId, ChangeRecord, Duration, EntityKind, Project};
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

/// The blocker ID made from the number `n` (UUID `0…0n`).
fn bid(n: u128) -> BlockerId {
    BlockerId::from_uuid(Uuid::from_u128(n))
}

/// A blocker with a fixed ID.
///
/// # Parameters
/// - `n`: number for the ID.
/// - `after_work`, `duration`: position and length.
/// - `reason`: free text.
fn blocker(n: u128, after_work: Duration, duration: Duration, reason: &str) -> Blocker {
    Blocker::new(bid(n), after_work, duration, reason)
}

/// The shed with three blockers on "Buy materials" (stored in an order that
/// is *not* sorted by ID or position) and one on "Paint" with a
/// `reported_on` day.
fn shed_with_blockers() -> Project {
    let mut project = shed();
    project.tasks[2].blockers = vec![
        blocker(3, days(2), days(5), "Wood delivery delayed"),
        blocker(1, Duration::ZERO, days(1), "Rain «heavy»"),
        blocker(2, days(2), Duration::from_minutes(90), ""),
    ];
    let mut late_paint = blocker(4, days(1), days(2), "Paint out of stock");
    late_paint.reported_on = Some(date("2027-03-20"));
    project.tasks[6].blockers.push(late_paint);
    project
}

/// Counts the rows of the `blockers` table of a file.
fn blocker_rows(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("open");
    conn.query_row("SELECT COUNT(*) FROM blockers", [], |row| row.get(0))
        .expect("count")
}

/// Blockers round-trip: every field, both `reported_on` cases, and the
/// stored order of each task's list (3, 1, 2 stays 3, 1, 2). The open gives
/// no notes.
///
/// Why it matters: a blocker that is lost on save is a delay the project
/// manager can no longer explain.
#[test]
fn blockers_round_trip_in_order() {
    let project = shed_with_blockers();
    let dir = temp_dir();
    let path = path_in(&dir, "blocked.gtra");
    save_ok(&path, &project);
    let opened = open_ok(&path);
    assert!(opened.notes.is_empty(), "notes: {:?}", opened.notes);
    assert_eq!(opened.format_version, CURRENT_FORMAT_VERSION);
    assert_eq!(normalized(&opened.project), normalized(&project));
    let ids: Vec<BlockerId> = opened.project.tasks[2]
        .blockers
        .iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(ids, vec![bid(3), bid(1), bid(2)]);
    assert_eq!(opened.project.tasks[2].blockers[0].reported_on, None);
    assert_eq!(
        opened.project.tasks[6].blockers[0].reported_on,
        Some(date("2027-03-20"))
    );
    assert_eq!(blocker_rows(&path), 4);
}

/// Saving again after removing and reordering blockers leaves exactly the
/// new list in the file (removed rows are deleted, positions renumbered).
///
/// Why it matters: "edit, Ctrl+S" must not leave ghost blockers behind.
#[test]
fn resave_removes_and_reorders_blockers() {
    let mut project = shed_with_blockers();
    let dir = temp_dir();
    let path = path_in(&dir, "blocked.gtra");
    save_ok(&path, &project);
    project.tasks[2].blockers.remove(0);
    project.tasks[2].blockers.reverse();
    project.tasks[6].blockers.clear();
    save_ok(&path, &project);
    let opened = open_ok(&path);
    assert_eq!(normalized(&opened.project), normalized(&project));
    assert_eq!(blocker_rows(&path), 2);
}

/// Writes a format-2 file (before blockers): saves normally, drops the
/// `blockers` table and stamps version 2.
///
/// # Parameters
/// - `path`: where to write.
/// - `project`: the project (without blockers).
fn write_format_2_file(path: &Path, project: &Project) {
    save_ok(path, project);
    let conn = Connection::open(path).expect("open as SQLite");
    conn.execute_batch(
        "ALTER TABLE tasks DROP COLUMN planned_finish;
         ALTER TABLE tasks DROP COLUMN actual_finish;
         ALTER TABLE tasks DROP COLUMN percent_complete;
         ALTER TABLE tasks DROP COLUMN is_project;
         DROP TABLE risks;
         DROP TABLE blockers;
         UPDATE meta SET value = '2' WHERE key = 'format_version';
         PRAGMA user_version = 2;",
    )
    .expect("downgrade to format 2");
}

/// A format-2 file opens upgraded (`upgraded_from == Some(2)`), writable,
/// with the same project; after a save it has the current format and a
/// `blockers` table that accepts blockers.
///
/// Why it matters: every file saved before blockers existed must still open
/// and keep working.
#[test]
fn format_2_file_is_upgraded() {
    let project = shed();
    let dir = temp_dir();
    let path = path_in(&dir, "v2.gtra");
    write_format_2_file(&path, &project);
    let opened = open_ok(&path);
    assert_eq!(opened.upgraded_from, Some(2));
    assert_eq!(opened.access, gtra::Access::ReadWrite);
    assert_eq!(normalized(&opened.project), normalized(&project));
    assert!(opened.project.tasks.iter().all(|t| t.blockers.is_empty()));

    let mut edited = opened.project;
    edited.tasks[2]
        .blockers
        .push(blocker(1, days(2), days(5), "Late wood"));
    save_ok(&path, &edited);
    let again = open_ok(&path);
    assert_eq!(again.upgraded_from, None);
    assert_eq!(again.format_version, CURRENT_FORMAT_VERSION);
    assert_eq!(again.project.tasks[2].blockers.len(), 1);
}

/// A blocker row on a **summary** task (written by another tool) is loaded
/// but reported: `ValueNotLoaded { table: "blockers", column: "task_id" }`.
///
/// Why it matters: Ganttora never makes such a row; the user must be told
/// that the engine ignores it.
#[test]
fn blocker_on_a_summary_is_reported() {
    let mut project = shed();
    project.tasks[2]
        .blockers
        .push(blocker(1, days(1), days(1), ""));
    let dir = temp_dir();
    let path = path_in(&dir, "summary.gtra");
    save_ok(&path, &project);
    // Make "Buy materials" a summary by giving it a subtask: "Prepare the
    // ground" moves under it in the tree.
    let buy = project.tasks[2].id.to_string();
    let ground = project.tasks[3].id.to_string();
    let conn = Connection::open(&path).expect("open");
    conn.execute(
        "UPDATE tasks SET parent_id = ?1, position = 1 WHERE id = ?2",
        [&buy, &ground],
    )
    .expect("make a subtask");
    drop(conn);
    let opened = open_ok(&path);
    assert!(
        opened.notes.contains(&ReadNote::ValueNotLoaded {
            table: "blockers",
            id: bid(1).to_string(),
            column: "task_id",
            value: buy.clone(),
        }),
        "notes: {:?}",
        opened.notes
    );
}

/// Negative `after_work_minutes` and `duration_minutes` are reported with
/// `ValueNotLoaded` naming the column and the value.
///
/// Why it matters: the engine treats negative values as zero; the user
/// should know the file said something else.
#[test]
fn negative_blocker_values_are_reported() {
    let mut project = shed();
    project.tasks[2]
        .blockers
        .push(blocker(1, days(1), days(1), ""));
    let dir = temp_dir();
    let path = path_in(&dir, "negative.gtra");
    save_ok(&path, &project);
    let conn = Connection::open(&path).expect("open");
    conn.execute(
        "UPDATE blockers SET after_work_minutes = -60, duration_minutes = -480",
        [],
    )
    .expect("negative values");
    drop(conn);
    let opened = open_ok(&path);
    let id = bid(1).to_string();
    for (column, value) in [("after_work_minutes", "-60"), ("duration_minutes", "-480")] {
        assert!(
            opened.notes.contains(&ReadNote::ValueNotLoaded {
                table: "blockers",
                id: id.clone(),
                column,
                value: value.to_string(),
            }),
            "missing note for {column}: {:?}",
            opened.notes
        );
    }
}

/// A blocker row whose task is not in the file (only possible with foreign
/// keys switched off) is not loaded, reported as `RowsNotLoaded { table:
/// "blockers", count: 1 }`, and **kept** when the project is saved again.
///
/// Why it matters: a save must never quietly delete data it never showed
/// the user.
#[test]
fn orphan_blocker_rows_are_reported_and_kept() {
    let project = shed();
    let dir = temp_dir();
    let path = path_in(&dir, "orphan.gtra");
    save_ok(&path, &project);
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         INSERT INTO blockers (id, task_id, position, after_work_minutes, duration_minutes, reason)
         VALUES ('00000000-0000-0000-0000-0000000000b1',
                 '00000000-0000-0000-0000-0000000000ff', 1, 0, 480, 'orphan');",
    )
    .expect("orphan row");
    drop(conn);
    let opened = open_ok(&path);
    assert!(
        opened.notes.contains(&ReadNote::RowsNotLoaded {
            table: "blockers",
            count: 1
        }),
        "notes: {:?}",
        opened.notes
    );
    assert!(opened.project.tasks.iter().all(|t| t.blockers.is_empty()));
    save_ok(&path, &opened.project);
    assert_eq!(blocker_rows(&path), 1, "the orphan row is still there");
}

/// Two blockers with the same ID (on different tasks) are refused on save
/// with `InvalidProject`, and no file is written.
///
/// Why it matters: the ID is the table's primary key; the second row would
/// silently overwrite the first.
#[test]
fn duplicate_blocker_ids_are_refused_on_save() {
    let mut project = shed();
    project.tasks[1]
        .blockers
        .push(blocker(1, days(1), days(1), "a"));
    project.tasks[2]
        .blockers
        .push(blocker(1, days(1), days(1), "b"));
    let dir = temp_dir();
    let path = path_in(&dir, "dup.gtra");
    let error = gtra::save(&path, &project, &crate::options()).unwrap_err();
    assert!(matches!(error, GtraError::InvalidProject(_)), "{error:?}");
    assert!(!path.exists());
}

/// A journal record with JSON **object** values (field `"blocker"`, as the
/// document writes for blocker edits) is stored and read back unchanged.
///
/// Why it matters: earlier journal values were numbers and strings; the
/// change history of blockers must survive in the file too.
#[test]
fn journal_object_values_round_trip() {
    let dir = temp_dir();
    let path = path_in(&dir, "journal.gtra");
    save_ok(&path, &shed());
    let mut file = gtra::open_db(&path).expect("open db");
    let old = json!({
        "id": bid(1).to_string(), "after_work_minutes": 960,
        "duration_minutes": 2400, "reason": "Late wood", "reported_on": null
    });
    let new = json!({
        "id": bid(1).to_string(), "after_work_minutes": 960,
        "duration_minutes": 960, "reason": "Shorter", "reported_on": "2027-03-04"
    });
    let record = ChangeRecord {
        timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
        author: "Anna".into(),
        batch_id: BatchId::from_uuid(Uuid::from_u128(1)),
        entity: EntityKind::Task,
        entity_id: Uuid::from_u128(3),
        field: "blocker".into(),
        old_value: old,
        new_value: new,
    };
    file.append_journal(std::slice::from_ref(&record))
        .expect("append");
    let entries = file.read_journal(&JournalFilter::default()).expect("read");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].record, record);
}

/// Percent complete, project rows and risks survive a save and a reopen.
#[test]
fn progress_and_projects_round_trip() {
    let dir = temp_dir();
    let path = path_in(&dir, "p.gtra");
    let mut project =
        model::Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    let mut task = model::Task::new("T", model::Duration::from_days(1));
    task.percent_complete = 40;
    let mut high = model::Risk::new(model::RiskId::new(), "Wood may be late");
    high.priority = model::RiskPriority::High;
    high.mitigation = "Order earlier".into();
    high.consequence = "The wall is later".into();
    let mut accepted = model::Risk::new(model::RiskId::new(), "Rain");
    accepted.priority = model::RiskPriority::Low;
    accepted.response = model::RiskResponse::Approved;
    accepted.no_consequence = true;
    task.risks = vec![high, accepted];
    project.tasks.push(task);
    project.ensure_project_row();
    save_ok(&path, &project);
    assert_eq!(open_ok(&path).project.tasks, project.tasks);

    // A removed risk disappears from the file too.
    project.tasks.last_mut().unwrap().risks.remove(0);
    save_ok(&path, &project);
    assert_eq!(open_ok(&path).project.tasks, project.tasks);
}

/// A format-5 file has one risk text per task. It becomes one risk to be
/// completed; empty texts become nothing.
#[test]
fn format_5_risk_texts_become_risks() {
    let dir = temp_dir();
    let path = path_in(&dir, "old.gtra");
    let mut project =
        model::Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    for name in ["T", "U"] {
        project
            .tasks
            .push(model::Task::new(name, model::Duration::from_days(1)));
    }
    project.ensure_project_row();
    save_ok(&path, &project);
    let conn = Connection::open(&path).expect("open as SQLite");
    conn.execute_batch(
        "ALTER TABLE tasks DROP COLUMN planned_finish;
         ALTER TABLE tasks DROP COLUMN actual_finish;
         DROP TABLE risks;
         ALTER TABLE tasks ADD COLUMN risk TEXT NOT NULL DEFAULT '';
         UPDATE tasks SET risk = 'Wood may be late' WHERE name = 'T';
         UPDATE meta SET value = '5' WHERE key = 'format_version';
         PRAGMA user_version = 5;",
    )
    .expect("downgrade to format 5");
    drop(conn);
    let opened = open_ok(&path).project;
    let risks = |name: &str| {
        let task = opened.tasks.iter().find(|t| t.name == name).unwrap();
        task.risks.clone()
    };
    assert!(risks("U").is_empty());
    let [risk] = &risks("T")[..] else {
        panic!("one risk expected")
    };
    assert_eq!(risk.description, "Wood may be late");
    assert_eq!(risk.priority, model::RiskPriority::Medium);
    assert!(!risk.is_complete());
}

/// The planned and actual finish survive saving; a format-6 file has neither.
#[test]
fn planned_and_actual_finish_round_trip() {
    let dir = temp_dir();
    let path = path_in(&dir, "plan.gtra");
    let mut project = shed();
    project.ensure_project_row();
    let day = |d| date(&format!("2027-03-0{d}")).and_hms_opt(17, 0, 0);
    project.tasks[1].planned_finish = day(2);
    project.tasks[1].actual_finish = day(4);
    save_ok(&path, &project);
    let task = open_ok(&path).project.tasks[1].clone();
    assert_eq!((task.planned_finish, task.actual_finish), (day(2), day(4)));

    let conn = Connection::open(&path).expect("open as SQLite");
    conn.execute_batch(
        "ALTER TABLE tasks DROP COLUMN planned_finish;
         ALTER TABLE tasks DROP COLUMN actual_finish;
         UPDATE meta SET value = '6' WHERE key = 'format_version';
         PRAGMA user_version = 6;",
    )
    .expect("downgrade to format 6");
    drop(conn);
    let task = open_ok(&path).project.tasks[1].clone();
    assert_eq!((task.planned_finish, task.actual_finish), (None, None));
}
