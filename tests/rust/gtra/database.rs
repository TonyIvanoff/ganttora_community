//! # Rules inside the database
//!
//! ## What this file checks
//! The `.gtra` schema has rules that the database itself enforces, as a last
//! line of defence behind the Rust checks: *foreign keys* (a link must point
//! to tasks that exist), *cascades* (deleting a task deletes its links),
//! `CHECK` rules (no link from a task to itself) and `UNIQUE` rules (one link
//! per pair of tasks). These tests check that the connections the `gtra`
//! crate opens really have those rules switched on.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).
//!
//! ## Main ideas
//! In SQLite, foreign keys are **off** by default for every new connection;
//! the app must switch them on each time (`PRAGMA foreign_keys = ON`). If it
//! forgot, broken links could be stored silently.

use crate::{path_in, save_ok, temp_dir};
use ganttora_tests::{row_id, shed};
use rusqlite::Connection;

/// Reads `PRAGMA foreign_keys` of a connection (1 = on).
fn foreign_keys(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("pragma foreign_keys")
}

/// A file made by `create_empty` has foreign keys switched on, and inserting
/// a link between two tasks that do not exist is refused by the database.
///
/// Why it matters: proves the last line of defence is active.
#[test]
fn foreign_keys_are_enforced_on_new_files() {
    let dir = temp_dir();
    let path = path_in(&dir, "new.gtra");
    let file = gtra::create_empty(&path).expect("create");
    let conn = file.connection();
    assert_eq!(foreign_keys(conn), 1);

    let result = conn.execute(
        "INSERT INTO dependencies (id, predecessor_id, successor_id) VALUES ('d1', 'nope1', 'nope2')",
        [],
    );
    assert!(result.is_err(), "a link to missing tasks must be refused");
}

/// The same holds for an existing file opened with `open_db`; and deleting
/// a task deletes its links (ON DELETE CASCADE).
///
/// Why it matters: opened files are where real data lives; the cascade keeps
/// the file consistent if a task row is removed directly.
#[test]
fn foreign_keys_and_cascade_on_opened_files() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);

    let file = gtra::open_db(&path).expect("open_db");
    let conn = file.connection();
    assert_eq!(foreign_keys(conn), 1);

    let bad = conn.execute(
        "UPDATE dependencies SET successor_id = 'missing' WHERE rowid = (SELECT MIN(rowid) FROM dependencies)",
        [],
    );
    assert!(bad.is_err(), "pointing a link at a missing task must fail");

    // Task 4 ("Prepare the ground") has two links: 2->4 and 4->5.
    let prepare = row_id(&project, 4).to_string();
    let count_links = |conn: &Connection| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM dependencies WHERE predecessor_id = ?1 OR successor_id = ?1",
            [&prepare],
            |row| row.get(0),
        )
        .expect("count")
    };
    assert_eq!(count_links(conn), 2);
    conn.execute("DELETE FROM tasks WHERE id = ?1", [&prepare])
        .expect("delete task");
    assert_eq!(count_links(conn), 0, "its links are deleted with it");
}

/// The database refuses a self-link and a second link between the same two
/// tasks, even when written directly with SQL.
///
/// Why it matters: these rules match `DocumentError::SelfLink` and
/// `LinkExists`; the database must agree with the document.
#[test]
fn check_and_unique_rules_on_links() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);
    let file = gtra::open_db(&path).expect("open_db");
    let conn = file.connection();

    let one = row_id(&project, 1).to_string();
    let two = row_id(&project, 2).to_string();
    let self_link = conn.execute(
        "INSERT INTO dependencies (id, predecessor_id, successor_id) VALUES ('x1', ?1, ?1)",
        [&one],
    );
    assert!(self_link.is_err(), "self-link must be refused");

    let duplicate = conn.execute(
        "INSERT INTO dependencies (id, predecessor_id, successor_id) VALUES ('x2', ?1, ?2)",
        [&one, &two],
    );
    assert!(duplicate.is_err(), "second link 1->2 must be refused");
}

/// A negative duration written directly is refused by a CHECK rule.
///
/// Why it matters: the same rule as `DocumentError::NegativeDuration`, kept
/// in the file itself so other tools cannot store nonsense either.
#[test]
fn negative_duration_is_refused_by_the_database() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);
    let file = gtra::open_db(&path).expect("open_db");
    let result = file.connection().execute(
        "UPDATE tasks SET duration_minutes = -1 WHERE id = ?1",
        [row_id(&project, 1).to_string()],
    );
    assert!(result.is_err());
}
