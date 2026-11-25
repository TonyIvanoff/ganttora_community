//! # Journal rows this version does not understand
//!
//! ## What this file checks
//! A newer Ganttora may write journal rows about things this version does
//! not know yet, for example `entity = 'resource'`. Such rows must be
//! skipped, not treated as an error:
//! - `read_journal` returns only the rows it understands;
//! - `read_journal_report` also says how many rows were skipped
//!   (`skipped_unknown`) and the id of the last row it looked at
//!   (`last_id`), so the journal view can keep reading page by page.
//! - The skipped row stays in the file, untouched.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).
//!
//! ## Main ideas
//! The public API cannot write a `'resource'` row on purpose (this version
//! has no resources), so the test writes it with plain SQL through
//! `GtraFile::connection()`, exactly as a newer Ganttora would.

use crate::{path_in, temp_dir};
use chrono::{TimeZone, Utc};
use gtra::{GtraFile, JournalFilter};
use model::{BatchId, ChangeRecord, EntityKind};
use serde_json::json;
use uuid::Uuid;

/// A task change record with fixed values.
///
/// # Parameters
/// - `n`: a small number used for the task ID, so records are distinct.
fn task_record(n: u128) -> ChangeRecord {
    ChangeRecord {
        timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
        author: "Anna".into(),
        batch_id: BatchId::from_uuid(Uuid::from_u128(1)),
        entity: EntityKind::Task,
        entity_id: Uuid::from_u128(n),
        field: "duration_minutes".into(),
        old_value: json!(2400),
        new_value: json!(2880),
    }
}

/// Writes one `'resource'` row the way a newer Ganttora would.
fn insert_resource_row(file: &GtraFile) {
    file.connection()
        .execute(
            "INSERT INTO journal (timestamp, author, entity, entity_id, field, old_value, new_value, batch_id)
             VALUES ('2027-02-15T10:43:00Z', 'Anna', 'resource',
                     '00000000-0000-0000-0000-0000000000aa', 'name', NULL, '\"Anna\"',
                     '00000000-0000-0000-0000-000000000002')",
            [],
        )
        .expect("insert a resource row");
}

/// Task row, resource row, task row: `read_journal` gives the two task
/// rows; `read_journal_report` says one row was skipped and that the last
/// row looked at is the third one. The resource row is still in the file.
///
/// Why it matters: opening a file from a newer Ganttora must still show
/// the history this version can show, instead of failing on one row.
#[test]
fn resource_row_is_skipped_and_counted() {
    let dir = temp_dir();
    let mut file = gtra::create_empty(path_in(&dir, "plan.gtra")).expect("create");
    let first = file.append_journal(&[task_record(1)]).expect("append")[0];
    insert_resource_row(&file);
    let third = file.append_journal(&[task_record(3)]).expect("append")[0];

    let entries = file.read_journal(&JournalFilter::default()).expect("read");
    let ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
    assert_eq!(ids, [first, third]);
    assert_eq!(entries[1].record, task_record(3));

    let page = file
        .read_journal_report(&JournalFilter::default())
        .expect("report");
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.skipped_unknown, 1);
    assert_eq!(page.last_id, Some(third));

    let rows: i64 = file
        .connection()
        .query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))
        .expect("count");
    assert_eq!(rows, 3, "the skipped row is kept in the file");
}

/// When the LAST row is the unknown one, `last_id` points at it, even
/// though `entries` ends earlier; a page with only that row is empty but
/// still moves `last_id` forward.
///
/// Why it matters: the journal view reads page by page with
/// `after_id = last_id`. If `last_id` stopped at the last *readable* row,
/// the view would ask for the same unknown row again and again.
#[test]
fn last_id_counts_skipped_rows_for_paging() {
    let dir = temp_dir();
    let mut file = gtra::create_empty(path_in(&dir, "plan.gtra")).expect("create");
    let first = file.append_journal(&[task_record(1)]).expect("append")[0];
    insert_resource_row(&file);
    let resource_id: i64 = file
        .connection()
        .query_row("SELECT MAX(id) FROM journal", [], |row| row.get(0))
        .expect("max id");

    let page = file
        .read_journal_report(&JournalFilter::default())
        .expect("report");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].id, first);
    assert_eq!(page.skipped_unknown, 1);
    assert_eq!(page.last_id, Some(resource_id));

    // Next page, starting after the first row: only the unknown row.
    let next = file
        .read_journal_report(&JournalFilter {
            after_id: Some(first),
            ..JournalFilter::default()
        })
        .expect("report");
    assert!(next.entries.is_empty());
    assert_eq!(next.skipped_unknown, 1);
    assert_eq!(next.last_id, Some(resource_id));

    // And after that: nothing at all.
    let end = file
        .read_journal_report(&JournalFilter {
            after_id: Some(resource_id),
            ..JournalFilter::default()
        })
        .expect("report");
    assert!(end.entries.is_empty());
    assert_eq!(end.skipped_unknown, 0);
    assert_eq!(end.last_id, None);
}
