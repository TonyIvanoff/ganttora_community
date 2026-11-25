//! # `ProjectSession`, recovery copies, read-only files, the journal, codes
//!
//! ## What this file checks
//! - **`ProjectSession`**: the way the desktop app keeps a project open.
//!   `create` (File > New), `open`, `save`, `save_as` (keeps data the model
//!   does not know, e.g. resources rows), `save_as_model_only`, `autosave`,
//!   `recover`, `close`.
//! - **`RecoveryStore`**: `list` (newest first, damaged copies listed as
//!   `readable: false`, other files ignored) and `discard`.
//! - **Newer files**: a file from a newer Ganttora opens read-only with
//!   reason code `gtra.newer_format`; every save is refused except
//!   `save_as_model_only`.
//! - **Refusals**: Save / Save As never overwrite a newer `.gtra` file or a
//!   file that is not a `.gtra` file.
//! - **Journal**: append, read back, filter by item, batch, page.
//! - **Error codes**: `GtraError::code()` for every variant.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).
//!
//! ## Main ideas
//! Every test works in its own temporary folder: the user's files at the
//! top, `working/` for working copies, `recovery/` for recovery copies.

use crate::{open_ok, options, path_in, save_ok, saved_at, temp_dir};
use chrono::{TimeZone, Utc};
use ganttora_tests::{days, shed};
use gtra::schema::CURRENT_FORMAT_VERSION;
use gtra::{
    Compatibility, GtraError, JournalFilter, ProjectSession, ReadOnlyReason, RecoveryStore,
    SaveOptions,
};
use model::{BatchId, ChangeRecord, EntityKind, Task};
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;
use uuid::Uuid;

/// The working-copy folder of a test.
fn working(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("working")
}

/// The number of rows in the `resources` table of a file (a table the M1
/// model does not read or write).
fn resource_rows(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("open");
    conn.query_row("SELECT COUNT(*) FROM resources", [], |row| row.get(0))
        .expect("count")
}

/// Save options with a chosen time, so recovery copies get known times.
fn options_at(when: &str) -> SaveOptions {
    SaveOptions::new(saved_at(when), "Tester")
}

/// A change record with fixed values.
///
/// # Parameters
/// - `entity_id`: the item the record is about.
/// - `batch`: a small number turned into the batch UUID.
/// - `field`: the field name.
fn record(entity_id: Uuid, batch: u128, field: &str) -> ChangeRecord {
    ChangeRecord {
        timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
        author: "Anna".into(),
        batch_id: BatchId::from_uuid(Uuid::from_u128(batch)),
        entity: EntityKind::Task,
        entity_id,
        field: field.into(),
        old_value: json!(2400),
        new_value: json!(2880),
    }
}

/// Makes the file at `path` look like it came from a newer Ganttora
/// (one format version above ours, `CURRENT_FORMAT_VERSION + 1`).
///
/// Using the constant instead of a fixed number keeps this helper correct
/// when a new migration raises our own version (as blockers did, 2 → 3).
fn make_newer(path: &Path) {
    let conn = Connection::open(path).expect("open");
    conn.pragma_update(None, "user_version", CURRENT_FORMAT_VERSION + 1)
        .expect("pragma");
}

// ---------------------------------------------------------------- sessions

/// File > New, then Save: a new session has no target, so `save` gives
/// `NoSavePath` (code `gtra.no_save_path`); `save_as` writes the file and
/// sets the target; a later `save` goes to the same file.
///
/// Why it matters: the app must ask "where?" the first time and never
/// afterwards.
#[test]
fn create_then_save_as_then_save() {
    let dir = temp_dir();
    let mut project = shed();
    let mut session = ProjectSession::create(&project, &options(), working(&dir)).expect("create");
    assert_eq!(session.target(), None);
    let error = session.save(&project, &options()).unwrap_err();
    assert!(matches!(error, GtraError::NoSavePath));
    assert_eq!(error.code(), "gtra.no_save_path");

    let path = path_in(&dir, "shed.gtra");
    session
        .save_as(&path, &project, &options())
        .expect("save as");
    assert_eq!(session.target(), Some(path.as_path()));
    assert_eq!(open_ok(&path).project, project);

    project.tasks[0].name = "Plan the shed".into();
    session.save(&project, &options()).expect("save");
    assert_eq!(open_ok(&path).project, project);
    session.close(None).expect("close");
}

/// Open, edit, save: the user's file gets the change; the working copy is
/// inside the working folder and is deleted on close.
///
/// Why it matters: the everyday cycle of the app.
#[test]
fn open_edit_save_close() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    assert!(!session.is_read_only());
    assert_eq!(
        session.compatibility().expect("compat"),
        Compatibility::Current
    );
    let working_path = session.working_path().to_path_buf();
    assert!(working_path.starts_with(working(&dir)));
    assert!(working_path.exists());

    let mut project = opened.project;
    project.tasks.push(Task::new("Clean up", days(1)));
    session.save(&project, &options()).expect("save");
    session.close(None).expect("close");
    assert!(!working_path.exists(), "the working copy is deleted");
    assert_eq!(open_ok(&path).project, project);
}

/// Save As keeps data the model does not know: a row in the `resources`
/// table (resources come in a later milestone) and the journal both reach
/// the new file.
///
/// Why it matters: an older app must never silently drop data from a file
/// just because it cannot show it (Lead decision for M1).
#[test]
fn save_as_keeps_unknown_table_data() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    {
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "INSERT INTO resources (id, name) VALUES ('00000000-0000-0000-0000-00000000f001', 'Anna')",
            [],
        )
        .expect("insert resource");
    }
    assert_eq!(resource_rows(&path), 1);

    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    session
        .append_journal(&[record(Uuid::from_u128(5), 1, "name")])
        .expect("journal");
    let copy = path_in(&dir, "copy.gtra");
    session
        .save_as(&copy, &opened.project, &options())
        .expect("save as");
    session.close(None).expect("close");

    assert_eq!(resource_rows(&copy), 1, "the resource row is kept");
    assert_eq!(open_ok(&copy).project, opened.project);
    let (session, _) = ProjectSession::open(&copy, working(&dir)).expect("reopen");
    assert_eq!(
        session
            .read_journal(&JournalFilter::default())
            .expect("read")
            .len(),
        1
    );
    session.close(None).expect("close");
}

/// `save_as_model_only` writes only what the model holds: the resource row
/// is *not* in the new file, the session now targets the new file.
///
/// Why it matters: it is the escape hatch for read-only files (see below),
/// and it must really be a fresh file.
#[test]
fn save_as_model_only_leaves_unknown_data_out() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    Connection::open(&path)
        .expect("open")
        .execute(
            "INSERT INTO resources (id, name) VALUES ('00000000-0000-0000-0000-00000000f001', 'Anna')",
            [],
        )
        .expect("insert");
    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    let fresh = path_in(&dir, "fresh.gtra");
    session
        .save_as_model_only(&fresh, &opened.project, &options())
        .expect("model only");
    assert_eq!(session.target(), Some(fresh.as_path()));
    session.close(None).expect("close");
    assert_eq!(resource_rows(&fresh), 0);
    assert_eq!(open_ok(&fresh).project, opened.project);
}

/// Autosave writes `<recovery>/<session id>.gtra` and never touches the
/// user's file; `list` shows the copy with its source, time and project
/// name; `close(Some(store))` deletes it.
///
/// Why it matters: after a crash the user gets their work back, but autosave
/// must never save over their file behind their back.
#[test]
fn autosave_writes_a_recovery_copy_and_close_discards_it() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    let before = std::fs::read(&path).expect("read");
    let store = RecoveryStore::new(dir.path().join("recovery")).expect("store");

    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    let mut project = opened.project;
    project.name = "Edited shed".into();
    let copy = session
        .autosave(&project, &options_at("2027-02-15 11:00"), &store)
        .expect("autosave");
    assert_eq!(copy.path, store.path_for(session.id()));
    assert!(copy.readable);
    assert_eq!(copy.source.as_deref(), Some(path.as_path()));
    assert_eq!(copy.saved_at.as_deref(), Some("2027-02-15T11:00:00Z"));
    assert_eq!(copy.project_name.as_deref(), Some("Edited shed"));
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "user file untouched"
    );
    assert_eq!(store.list().expect("list"), vec![copy.clone()]);

    session.close(Some(&store)).expect("close");
    assert!(!copy.path.exists());
    assert!(store.list().expect("list").is_empty());
}

/// Recover: after a "crash" (the session is closed without discarding), the
/// copy is listed; `recover` restores the autosaved project, keeps the
/// session id, targets the original file, and removes the `recovery_*` meta
/// rows from the working copy so they never reach the user's file.
///
/// Why it matters: this is the whole point of autosave.
#[test]
fn recover_restores_the_autosaved_project() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    let store = RecoveryStore::new(dir.path().join("recovery")).expect("store");
    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    let mut project = opened.project;
    project.tasks.push(Task::new("Unsaved work", days(2)));
    session
        .autosave(&project, &options(), &store)
        .expect("autosave");
    let id = session.id();
    session.close(None).expect("close keeps the copy");

    let copies = store.list().expect("list");
    assert_eq!(copies.len(), 1);
    let (recovered, opened) = ProjectSession::recover(&copies[0], working(&dir)).expect("recover");
    assert_eq!(opened.project, project);
    assert_eq!(recovered.id(), id);
    assert_eq!(recovered.target(), Some(path.as_path()));
    for key in [
        gtra::recovery::META_SESSION_ID,
        gtra::recovery::META_SOURCE,
        gtra::recovery::META_SAVED_AT,
    ] {
        assert_eq!(recovered.file().meta(key).expect("meta"), None, "{key}");
    }
    recovered.close(Some(&store)).expect("close");
    assert!(store.list().expect("list").is_empty());
}

/// A never-saved project's recovery copy has no source; after recovering it,
/// `save` asks for Save As (`NoSavePath`).
///
/// Why it matters: "Untitled" projects can be recovered too.
#[test]
fn recovered_unsaved_project_needs_save_as() {
    let dir = temp_dir();
    let store = RecoveryStore::new(dir.path().join("recovery")).expect("store");
    let project = shed();
    let mut session = ProjectSession::create(&project, &options(), working(&dir)).expect("create");
    let copy = session
        .autosave(&project, &options(), &store)
        .expect("autosave");
    assert_eq!(copy.source, None);
    session.close(None).expect("close");
    let (mut recovered, _) = ProjectSession::recover(&copy, working(&dir)).expect("recover");
    assert!(matches!(
        recovered.save(&project, &options()),
        Err(GtraError::NoSavePath)
    ));
    recovered.close(Some(&store)).expect("close");
}

// ---------------------------------------------------------- recovery store

/// `list` sorts newest first; a damaged copy (junk bytes named `<uuid>.gtra`)
/// is listed last with `readable: false`; files with other names are
/// ignored; recovering the damaged copy gives `NotAGtraFile`; `discard`
/// deletes a copy (and discarding a missing one is fine).
///
/// Why it matters: the "Recover unsaved projects?" dialog lists these in
/// this order and must not crash on a half-written file.
#[test]
fn recovery_list_order_damaged_copies_and_discard() {
    let dir = temp_dir();
    let store = RecoveryStore::new(dir.path().join("recovery")).expect("store");
    let mut ids = Vec::new();
    for (name, when) in [
        ("Older", "2027-02-15 09:00"),
        ("Newest", "2027-02-15 12:00"),
        ("Middle", "2027-02-15 10:30"),
    ] {
        let mut project = shed();
        project.name = name.into();
        let mut session =
            ProjectSession::create(&project, &options(), working(&dir)).expect("create");
        session
            .autosave(&project, &options_at(when), &store)
            .expect("autosave");
        ids.push(session.id());
        session.close(None).expect("close");
    }
    let damaged = Uuid::from_u128(0xDEAD);
    std::fs::write(store.path_for(damaged), b"not a database at all").expect("junk");
    std::fs::write(store.dir().join("notes.txt"), b"ignored").expect("other file");
    std::fs::write(store.dir().join("not-a-uuid.gtra"), b"ignored").expect("other file");

    let copies = store.list().expect("list");
    let names: Vec<Option<&str>> = copies.iter().map(|c| c.project_name.as_deref()).collect();
    assert_eq!(
        names,
        vec![Some("Newest"), Some("Middle"), Some("Older"), None]
    );
    let last = &copies[3];
    assert_eq!(last.session_id, damaged);
    assert!(!last.readable);
    assert_eq!(last.format_version, 0);
    assert!(matches!(
        ProjectSession::recover(last, working(&dir)),
        Err(GtraError::NotAGtraFile(_))
    ));

    store.discard(damaged).expect("discard");
    store.discard(ids[0]).expect("discard");
    store
        .discard(Uuid::from_u128(0xFFFF))
        .expect("missing is fine");
    assert_eq!(store.list().expect("list").len(), 2);
}

// ----------------------------------------------------------- newer files

/// A file from a newer Ganttora (our format + 1) opens read-only in a
/// session, with `Compatibility::ReadOnly(NewerFormat { found: ours + 1,
/// supported: ours, .. })`,
/// code `gtra.newer_format` and the version that wrote it. Save, Save As
/// and autosave give `ReadOnly`; `save_as_model_only` works and the session
/// is writable afterwards.
///
/// Why it matters: saving over a newer file could drop data this app does
/// not understand; the user can still keep working on a separate copy.
#[test]
fn newer_file_is_read_only_in_a_session() {
    let dir = temp_dir();
    let path = path_in(&dir, "future.gtra");
    save_ok(&path, &shed());
    make_newer(&path);
    let store = RecoveryStore::new(dir.path().join("recovery")).expect("store");

    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    assert!(session.is_read_only());
    let compat = session.compatibility().expect("compat");
    assert_eq!(compat.notice_code(), Some("gtra.newer_format"));
    let Compatibility::ReadOnly(reason) = compat else {
        panic!("expected read-only, got {compat:?}");
    };
    assert_eq!(reason.code(), "gtra.newer_format");
    assert_eq!(
        reason,
        ReadOnlyReason::NewerFormat {
            found: CURRENT_FORMAT_VERSION + 1,
            supported: CURRENT_FORMAT_VERSION,
            written_by: Some(gtra::APP_VERSION.to_string()),
        }
    );
    assert!(
        reason
            .params()
            .contains(&("found", (CURRENT_FORMAT_VERSION + 1).to_string()))
    );
    assert_eq!(opened.read_only_reason, Some(reason));

    let project = opened.project;
    let error = session.save(&project, &options()).unwrap_err();
    assert!(matches!(error, GtraError::ReadOnly));
    assert_eq!(error.code(), "gtra.read_only");
    assert!(matches!(
        session.save_as(path_in(&dir, "copy.gtra"), &project, &options()),
        Err(GtraError::ReadOnly)
    ));
    assert!(matches!(
        session.autosave(&project, &options(), &store),
        Err(GtraError::ReadOnly)
    ));
    assert!(matches!(
        session.append_journal(&[record(Uuid::from_u128(1), 1, "name")]),
        Err(GtraError::ReadOnly)
    ));

    let fresh = path_in(&dir, "fresh.gtra");
    session
        .save_as_model_only(&fresh, &project, &options())
        .expect("model only");
    assert!(!session.is_read_only());
    session.close(None).expect("close");
    assert_eq!(open_ok(&fresh).format_version, CURRENT_FORMAT_VERSION);
}

/// Save As never replaces a newer `.gtra` file (`NewerFormat`, code
/// `gtra.newer_format`) or a file that is not a `.gtra` file
/// (`NotAGtraFile`, code `gtra.not_a_gtra_file`); both files stay
/// byte-for-byte unchanged. `save_as_model_only` refuses them too.
///
/// Why it matters: a slip in the Save dialog must not destroy a colleague's
/// newer file or an unrelated document.
#[test]
fn save_as_refuses_to_overwrite_newer_or_foreign_files() {
    let dir = temp_dir();
    let newer = path_in(&dir, "newer.gtra");
    save_ok(&newer, &shed());
    make_newer(&newer);
    let foreign = path_in(&dir, "letter.gtra");
    std::fs::write(&foreign, b"Dear neighbour, ...").expect("write");
    let newer_bytes = std::fs::read(&newer).expect("read");

    let project = shed();
    let mut session = ProjectSession::create(&project, &options(), working(&dir)).expect("create");
    let error = session.save_as(&newer, &project, &options()).unwrap_err();
    assert!(matches!(error, GtraError::NewerFormat { .. }), "{error:?}");
    assert_eq!(error.code(), "gtra.newer_format");
    let error = session.save_as(&foreign, &project, &options()).unwrap_err();
    assert!(matches!(error, GtraError::NotAGtraFile(_)), "{error:?}");
    assert_eq!(error.code(), "gtra.not_a_gtra_file");
    assert!(
        session
            .save_as_model_only(&foreign, &project, &options())
            .is_err()
    );
    assert_eq!(session.target(), None, "a refused Save As changes nothing");
    session.close(None).expect("close");

    assert_eq!(std::fs::read(&newer).expect("read"), newer_bytes);
    assert_eq!(
        std::fs::read(&foreign).expect("read"),
        b"Dear neighbour, ..."
    );
}

// ------------------------------------------------------------------ journal

/// The journal: records appended in a session get increasing ids, read back
/// identical, survive Save and reopen, and can be filtered by item, by
/// batch, and page by page (`after_id`, `limit`).
///
/// Why it matters: the journal view ("who changed what, when") reads it in
/// pages and per task.
#[test]
fn journal_append_read_and_filter() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    let (mut session, opened) = ProjectSession::open(&path, working(&dir)).expect("open");
    let task_a = Uuid::from_u128(0xA);
    let task_b = Uuid::from_u128(0xB);
    let records = vec![
        record(task_a, 1, "name"),
        record(task_a, 1, "duration_minutes"),
        record(task_b, 2, "notes"),
        record(task_a, 3, "deadline"),
    ];
    let ids = session.append_journal(&records).expect("append");
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids increase: {ids:?}");
    session.save(&opened.project, &options()).expect("save");
    session.close(None).expect("close");

    let (session, _) = ProjectSession::open(&path, working(&dir)).expect("reopen");
    let all = session
        .read_journal(&JournalFilter::default())
        .expect("all");
    assert_eq!(
        all.iter().map(|e| e.record.clone()).collect::<Vec<_>>(),
        records
    );
    let for_a = session
        .read_journal(&JournalFilter {
            entity_id: Some(task_a),
            ..Default::default()
        })
        .expect("by item");
    assert_eq!(for_a.len(), 3);
    let batch_1 = session
        .read_journal(&JournalFilter {
            batch_id: Some(BatchId::from_uuid(Uuid::from_u128(1))),
            ..Default::default()
        })
        .expect("by batch");
    assert_eq!(batch_1.len(), 2);
    let page = session
        .read_journal(&JournalFilter {
            after_id: Some(all[0].id),
            limit: Some(2),
            ..Default::default()
        })
        .expect("page");
    assert_eq!(
        page.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![all[1].id, all[2].id]
    );
    session.close(None).expect("close");
}

/// A record with an empty field name is refused with `InvalidRecord` (code
/// `gtra.invalid_record`), and nothing of the batch is written.
///
/// Why it matters: all-or-nothing: a half-written journal batch would tell
/// a wrong story.
#[test]
fn invalid_journal_record_writes_nothing() {
    let dir = temp_dir();
    let project = shed();
    let mut session = ProjectSession::create(&project, &options(), working(&dir)).expect("create");
    let good = record(Uuid::from_u128(1), 1, "name");
    let bad = record(Uuid::from_u128(1), 1, "");
    let error = session.append_journal(&[good, bad]).unwrap_err();
    assert!(matches!(error, GtraError::InvalidRecord(_)));
    assert_eq!(error.code(), "gtra.invalid_record");
    assert!(
        session
            .read_journal(&JournalFilter::default())
            .expect("read")
            .is_empty()
    );
    session.close(None).expect("close");
}

// -------------------------------------------------------------- error codes

/// Every `GtraError` variant has its stable code; I/O errors are split into
/// "file not found", "permission denied" and other.
///
/// Why it matters: the UI translates `error.<code>` (e.g.
/// `error.gtra.newer_format` in `locales/en.json`). A changed code would
/// show a raw key to the user.
#[test]
fn every_error_has_its_code() {
    use std::io::{Error, ErrorKind};
    let path = std::path::PathBuf::from("/x.gtra");
    let cases: Vec<(GtraError, &str)> = vec![
        (
            GtraError::io(&path, Error::from(ErrorKind::NotFound)),
            "gtra.file_not_found",
        ),
        (
            GtraError::io(&path, Error::from(ErrorKind::PermissionDenied)),
            "gtra.permission_denied",
        ),
        (GtraError::io(&path, Error::other("disk full")), "gtra.io"),
        (
            GtraError::Sqlite(rusqlite::Error::InvalidQuery),
            "gtra.database",
        ),
        (
            GtraError::NotAGtraFile(path.clone()),
            "gtra.not_a_gtra_file",
        ),
        (GtraError::FileExists(path.clone()), "gtra.file_exists"),
        (GtraError::InvalidPath(path.clone()), "gtra.invalid_path"),
        (
            GtraError::NewerFormat {
                found: 3,
                supported: 2,
            },
            "gtra.newer_format",
        ),
        (GtraError::ReadOnly, "gtra.read_only"),
        (
            GtraError::Migration {
                name: "0002_m1_fields",
                source: rusqlite::Error::InvalidQuery,
            },
            "gtra.migration_failed",
        ),
        (GtraError::InvalidData("x".into()), "gtra.invalid_data"),
        (
            GtraError::InvalidProject("x".into()),
            "gtra.invalid_project",
        ),
        (GtraError::InvalidRecord("x".into()), "gtra.invalid_record"),
        (GtraError::NoSavePath, "gtra.no_save_path"),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error}");
    }
}

/// Opening a file that does not exist gives an I/O error with code
/// `gtra.file_not_found`, and no working copy is left behind.
///
/// Why it matters: "Recent files" can point at deleted files.
#[test]
fn opening_a_missing_file_leaves_no_working_copy() {
    let dir = temp_dir();
    let error = ProjectSession::open(path_in(&dir, "gone.gtra"), working(&dir)).unwrap_err();
    assert_eq!(error.code(), "gtra.file_not_found");
    let left: Vec<_> = std::fs::read_dir(working(&dir))
        .map(|entries| entries.collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");
}
