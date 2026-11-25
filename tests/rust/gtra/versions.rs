//! # Format versions, wrong files, empty files and migrations
//!
//! ## What this file checks
//! - A file made by a *newer* Ganttora (higher format version) opens
//!   read-only, and saving over it is refused, so an old app never damages a
//!   newer file.
//! - Files that are not `.gtra` files (text, an empty file, someone else's
//!   SQLite database, a missing file) give clear errors.
//! - `create_empty` makes a valid new file that opens with sensible defaults.
//! - Migrations take an empty (version 0) database up to the current version
//!   and are safe to run twice.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).
//!
//! ## Main ideas
//! - SQLite has two small numbers in every database header: `user_version`
//!   (we use it as the format version) and `application_id` (ours says
//!   "Gtra"). The tests change them directly with `rusqlite` to fake files
//!   that the public API would never write on purpose.
//! - A *migration* is a script that upgrades a file from one format version
//!   to the next.

use crate::{files_in, normalized, open_ok, path_in, save_ok, temp_dir};
use ganttora_tests::shed;
use gtra::schema::{
    CURRENT_FORMAT_VERSION, GTRA_APPLICATION_ID, MIGRATIONS, VersionStatus, check_version,
};
use gtra::storage::LocalFileStorage;
use gtra::{Access, GtraError, ReadNote};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Sets the format version (`user_version`) of a database file.
fn set_user_version(path: &Path, version: u32) {
    let conn = Connection::open(path).expect("open as SQLite");
    conn.pragma_update(None, "user_version", version)
        .expect("set user_version");
}

/// A shed file whose format version is one higher than this app supports
/// opens read-only. `open` reports the file's real version and still reads
/// the project.
///
/// Why it matters: a colleague with a newer Ganttora sends a file. We can
/// show it, but must not save it, because we might drop data we do not
/// understand.
#[test]
fn newer_format_opens_read_only() {
    let dir = temp_dir();
    let path = path_in(&dir, "newer.gtra");
    let project = shed();
    save_ok(&path, &project);
    let newer = CURRENT_FORMAT_VERSION + 1;
    set_user_version(&path, newer);

    let opened = open_ok(&path);
    assert_eq!(opened.access, Access::ReadOnly);
    assert_eq!(opened.format_version, newer);
    assert_eq!(opened.upgraded_from, None);
    assert_eq!(normalized(&opened.project), normalized(&project));
}

/// Saving over a newer-format file is refused with `NewerFormat`, and the
/// file's bytes stay exactly the same.
///
/// Why it matters: this is the protection itself. The error message tells
/// the user to update Ganttora.
#[test]
fn saving_over_newer_format_is_refused() {
    let dir = temp_dir();
    let path = path_in(&dir, "newer.gtra");
    let project = shed();
    save_ok(&path, &project);
    let newer = CURRENT_FORMAT_VERSION + 1;
    set_user_version(&path, newer);
    let bytes_before = fs::read(&path).expect("read");

    let result = gtra::save(&path, &project, &crate::options());
    match result {
        Err(GtraError::NewerFormat { found, supported }) => {
            assert_eq!(found, newer);
            assert_eq!(supported, CURRENT_FORMAT_VERSION);
        }
        other => panic!("expected NewerFormat, got {other:?}"),
    }
    assert_eq!(fs::read(&path).expect("read"), bytes_before);
    assert_eq!(files_in(dir.path()), vec!["newer.gtra".to_string()]);
}

/// The lower-level API refuses too: a newer file opened with `open_db` is
/// read-only, and both `write_project` and `save_to` return `ReadOnly`.
///
/// Why it matters: every way of writing must respect read-only, not only
/// the convenient `gtra::save`.
#[test]
fn read_only_file_refuses_every_write() {
    let dir = temp_dir();
    let path = path_in(&dir, "newer.gtra");
    let project = shed();
    save_ok(&path, &project);
    set_user_version(&path, CURRENT_FORMAT_VERSION + 5);

    let mut file = gtra::open_db(&path).expect("open_db");
    assert!(file.is_read_only());
    assert_eq!(file.access(), Access::ReadOnly);
    assert_eq!(file.format_version(), CURRENT_FORMAT_VERSION + 5);

    let write = file.write_project(&project, &crate::options());
    assert!(matches!(write, Err(GtraError::ReadOnly)), "got {write:?}");

    let copy = path_in(&dir, "copy.gtra");
    let save = file.save_to(&LocalFileStorage::new(&copy));
    assert!(matches!(save, Err(GtraError::ReadOnly)), "got {save:?}");
    assert!(!copy.exists());
}

/// `check_version` sorts versions into "current", "needs upgrade" and
/// "newer".
///
/// Why it matters: this small decision drives read-only mode and migrations.
#[test]
fn check_version_classifies() {
    assert_eq!(
        check_version(CURRENT_FORMAT_VERSION),
        VersionStatus::Current
    );
    assert_eq!(check_version(0), VersionStatus::NeedsUpgrade { from: 0 });
    assert_eq!(
        check_version(CURRENT_FORMAT_VERSION + 1),
        VersionStatus::Newer {
            found: CURRENT_FORMAT_VERSION + 1
        }
    );
}

/// Opening files that are not `.gtra` files gives `NotAGtraFile` with the
/// path the user chose (not the internal working copy): a text file, an
/// empty file, a SQLite database from another program, and one that has our
/// version number but not our application ID.
///
/// Why it matters: a user may pick the wrong file in the Open dialog. The
/// message must name *their* file, and nothing must crash.
#[test]
fn not_a_gtra_file() {
    let dir = temp_dir();

    let text = path_in(&dir, "readme.gtra");
    fs::write(
        &text,
        "This is not a database, just some text.\n".repeat(50),
    )
    .expect("write");

    let empty = path_in(&dir, "empty.gtra");
    fs::write(&empty, b"").expect("write");

    let other_db = path_in(&dir, "other.gtra");
    {
        let conn = Connection::open(&other_db).expect("create");
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .expect("table");
    }

    let wrong_id = path_in(&dir, "wrong-id.gtra");
    {
        let conn = Connection::open(&wrong_id).expect("create");
        conn.execute_batch("CREATE TABLE t (x);").expect("table");
        conn.pragma_update(None, "user_version", 1)
            .expect("version");
        conn.pragma_update(None, "application_id", 12345)
            .expect("app id");
    }

    for path in [&text, &empty, &other_db, &wrong_id] {
        match gtra::open(path) {
            Err(GtraError::NotAGtraFile(reported)) => {
                assert_eq!(&reported, path, "the error must name the user's file");
            }
            other => panic!("{}: expected NotAGtraFile, got {other:?}", path.display()),
        }
    }
}

/// Saving over a file that is not a `.gtra` file is refused and the file is
/// left alone.
///
/// Why it matters: "Save" must never overwrite someone's unrelated file
/// just because it has the .gtra extension.
#[test]
fn saving_over_a_non_gtra_file_is_refused() {
    let dir = temp_dir();
    let path = path_in(&dir, "readme.gtra");
    let text = "Important notes, not a project.\n".repeat(50);
    fs::write(&path, &text).expect("write");

    let result = gtra::save(&path, &shed(), &crate::options());
    assert!(
        matches!(result, Err(GtraError::NotAGtraFile(_))),
        "got {result:?}"
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), text);
}

/// Opening a file that does not exist gives an I/O error ("not found").
///
/// Why it matters: e.g. a recent-files entry pointing to a deleted file.
#[test]
fn missing_file_gives_io_error() {
    let dir = temp_dir();
    let result = gtra::open(path_in(&dir, "gone.gtra"));
    match result {
        Err(GtraError::Io { source, .. }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io(NotFound), got {other:?}"),
    }
}

/// `create_empty` makes a writable, current-version file with our
/// application ID, a project ID and the file name as project name. Opening it
/// gives an empty project; because a new file has no start date and no
/// calendar yet, `open` fills them in and says so in its notes.
///
/// Why it matters: "File > New" starts from this file; it must open cleanly
/// and tell the UI which values were defaults.
#[test]
fn create_empty_makes_a_valid_file() {
    let dir = temp_dir();
    let path = path_in(&dir, "My plan.gtra");

    let file = gtra::create_empty(&path).expect("create_empty");
    assert_eq!(file.access(), Access::ReadWrite);
    assert_eq!(file.format_version(), CURRENT_FORMAT_VERSION);
    assert_eq!(file.upgraded_from(), None);
    let project_id = file
        .meta("project_id")
        .expect("meta")
        .expect("project_id set");
    assert_eq!(
        file.meta("format_version").expect("meta"),
        Some(CURRENT_FORMAT_VERSION.to_string())
    );
    let app_id: i32 = file
        .connection()
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .expect("application_id");
    assert_eq!(app_id, GTRA_APPLICATION_ID);
    file.close().expect("close");
    assert_eq!(files_in(dir.path()), vec!["My plan.gtra".to_string()]);

    let opened = open_ok(&path);
    assert_eq!(opened.access, Access::ReadWrite);
    assert_eq!(opened.project.name, "My plan");
    assert_eq!(opened.project.id.to_string(), project_id);
    assert!(opened.project.tasks.is_empty());
    assert_eq!(opened.project.calendars.len(), 1);
    assert!(
        opened
            .notes
            .iter()
            .any(|n| matches!(n, ReadNote::StartDateMissing { .. })),
        "notes: {:?}",
        opened.notes
    );
    assert!(
        opened
            .notes
            .iter()
            .any(|n| matches!(n, ReadNote::CalendarMissing { used } if *used == opened.project.calendar_id)),
        "notes: {:?}",
        opened.notes
    );
}

/// `create_empty` refuses to overwrite an existing file.
///
/// Why it matters: "New" must never destroy an existing project.
#[test]
fn create_empty_refuses_existing_file() {
    let dir = temp_dir();
    let path = path_in(&dir, "plan.gtra");
    save_ok(&path, &shed());
    let bytes_before = fs::read(&path).expect("read");
    match gtra::create_empty(&path) {
        Err(GtraError::FileExists(reported)) => assert_eq!(reported, path),
        other => panic!("expected FileExists, got {other:?}"),
    }
    assert_eq!(fs::read(&path).expect("read"), bytes_before);
}

/// The project in a file made by `create_empty` can be filled and saved with
/// `gtra::save`, keeping the project ID.
///
/// Why it matters: New, then add tasks, then Save is the normal first
/// session.
#[test]
fn create_empty_then_save_project() {
    let dir = temp_dir();
    let path = path_in(&dir, "new.gtra");
    gtra::create_empty(&path)
        .expect("create")
        .close()
        .expect("close");
    let mut project = open_ok(&path).project;
    ganttora_tests::add_task(&mut project, "First task", ganttora_tests::days(2));
    save_ok(&path, &project);
    let reopened = open_ok(&path);
    assert_eq!(normalized(&reopened.project), normalized(&project));
    assert!(reopened.notes.is_empty(), "notes: {:?}", reopened.notes);
}

/// The list of migrations is in order, numbered 1, 2, 3 ... without gaps,
/// and the current format version is the last one's number.
///
/// Why it matters: a gap or a wrong order would skip or repeat an upgrade
/// step and corrupt old files.
#[test]
fn migration_list_is_consistent() {
    assert!(!MIGRATIONS.is_empty());
    for (index, migration) in MIGRATIONS.iter().enumerate() {
        assert_eq!(migration.version as usize, index + 1, "{}", migration.name);
        assert!(
            !migration.sql.trim().is_empty(),
            "{} is empty",
            migration.name
        );
    }
    assert_eq!(
        MIGRATIONS.last().map(|m| m.version),
        Some(CURRENT_FORMAT_VERSION)
    );
}

/// Every table the format defines, as named in the project documentation / the
/// migration `0001_init.sql`, plus `blockers` (migration `0003_blockers.sql`,
/// D23).
const EXPECTED_TABLES: [&str; 17] = [
    "assignments",
    "blockers",
    "calendar_exception_times",
    "calendar_exceptions",
    "calendar_week_days",
    "calendar_working_times",
    "calendars",
    "dependencies",
    "journal",
    "meta",
    "project",
    "project_settings",
    "resources",
    "risks",
    "tasks",
    "versions",
    // SQLite's own table for AUTOINCREMENT counters (created by `journal`).
    "sqlite_sequence",
];

/// Migrating an empty database (format version 0) runs every migration:
/// afterwards it has the current version, our application ID, the
/// `format_version` meta entry and every table. Running `migrate` again does
/// nothing and reports the current version.
///
/// Why it matters: this is the path every file takes when it is created, and
/// the path old files will take when the format grows. Running twice must be
/// harmless (e.g. opening the same file again).
#[test]
fn migrate_from_version_zero_to_current() {
    let dir = temp_dir();
    let path = path_in(&dir, "blank.db");
    let mut conn = Connection::open(&path).expect("create");

    let before = gtra::schema::migrate(&mut conn).expect("migrate");
    assert_eq!(before, 0, "migrate reports the version it started from");
    assert_eq!(
        gtra::schema::read_format_version(&conn).expect("version"),
        CURRENT_FORMAT_VERSION
    );
    assert_eq!(
        gtra::schema::read_application_id(&conn).expect("app id"),
        GTRA_APPLICATION_ID
    );
    let meta_version: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'format_version'",
            [],
            |row| row.get(0),
        )
        .expect("meta format_version");
    assert_eq!(meta_version, CURRENT_FORMAT_VERSION.to_string());

    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare");
    let mut tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    drop(stmt);
    tables.sort();
    let mut expected: Vec<String> = EXPECTED_TABLES.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(tables, expected);

    // Foreign keys are switched back on after migrating.
    let fk: i64 = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("pragma");
    assert_eq!(fk, 1);

    // Second run: nothing to do.
    let again = gtra::schema::migrate(&mut conn).expect("migrate again");
    assert_eq!(again, CURRENT_FORMAT_VERSION);
}

/// `migrate` refuses a database with a newer version instead of touching it.
///
/// Why it matters: an upgrade script must never run "backwards" on a file
/// from the future.
#[test]
fn migrate_refuses_newer_database() {
    let dir = temp_dir();
    let path = path_in(&dir, "future.db");
    let mut conn = Connection::open(&path).expect("create");
    conn.pragma_update(None, "user_version", CURRENT_FORMAT_VERSION + 1)
        .expect("set version");
    let result = gtra::schema::migrate(&mut conn);
    assert!(
        matches!(result, Err(GtraError::NewerFormat { found, .. }) if found == CURRENT_FORMAT_VERSION + 1),
        "got {result:?}"
    );
}

/// A database with our application ID but format version 0 is not accepted
/// by `open_db` (version 0 means "never set up").
///
/// Why it matters: a half-created file (e.g. the app crashed while creating
/// it) must not be mistaken for a valid project.
#[test]
fn version_zero_with_our_id_is_not_a_gtra_file() {
    let dir = temp_dir();
    let path = path_in(&dir, "half.gtra");
    {
        let conn = Connection::open(&path).expect("create");
        conn.execute_batch("CREATE TABLE t (x);").expect("table");
        conn.pragma_update(None, "application_id", GTRA_APPLICATION_ID)
            .expect("app id");
    }
    let result = gtra::open_db(&path);
    assert!(
        matches!(result, Err(GtraError::NotAGtraFile(_))),
        "got {result:?}"
    );
}
