//! # Atomic save: all or nothing, and no leftovers
//!
//! ## What this file checks
//! A save first writes a complete temporary file next to the target, then
//! renames it over the target in one step ("atomic saves").
//! So:
//! - after a successful save the folder holds only the target file (no
//!   `.plan.gtra.<uuid>.tmp`, no `-journal` file);
//! - when writing fails halfway, the old file is untouched and the temporary
//!   file is removed.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).
//!
//! ## Main ideas
//! A real disk failure is hard to cause on purpose. We simulate failures in
//! three ways: (1) `gtra::atomic::atomic_write` with a "fill" step that writes
//! half a file and then reports an error; (2) a folder made read-only, so the
//! temporary file cannot be created (Unix only); (3) a target in a folder
//! that does not exist.

use crate::{files_in, normalized, open_ok, path_in, save_ok, temp_dir};
use ganttora_tests::shed;
use gtra::GtraError;
use gtra::atomic::{atomic_write, atomic_write_bytes};
use std::fs;
use std::path::Path;

/// A successful first save and a successful re-save leave exactly one file
/// in the folder.
///
/// Why it matters: leftover temporary files would pile up in the user's
/// project folder (and in shared/synced folders, confuse other people).
#[test]
fn successful_saves_leave_no_temp_files() {
    let dir = temp_dir();
    let path = path_in(&dir, "plan.gtra");
    let mut project = shed();
    save_ok(&path, &project);
    assert_eq!(files_in(dir.path()), vec!["plan.gtra".to_string()]);

    project.tasks[0].name = "Changed".into();
    save_ok(&path, &project);
    assert_eq!(files_in(dir.path()), vec!["plan.gtra".to_string()]);
}

/// When the fill step writes half a file and then fails, the target keeps
/// its old bytes, the error is passed on, and the temporary file is gone.
///
/// Why it matters: this is the core promise of an atomic save. A crash or a
/// full disk during saving must never leave a half-written project.
#[test]
fn failing_write_keeps_the_old_file() {
    let dir = temp_dir();
    let target = path_in(&dir, "notes.txt");
    fs::write(&target, b"old content").expect("prepare");

    let result = atomic_write(&target, |tmp: &Path| {
        fs::write(tmp, b"half of the new con").map_err(|e| GtraError::io(tmp, e))?;
        Err(GtraError::InvalidData("simulated failure".into()))
    });

    assert!(
        matches!(result, Err(GtraError::InvalidData(ref m)) if m == "simulated failure"),
        "got {result:?}"
    );
    assert_eq!(fs::read(&target).expect("read"), b"old content");
    assert_eq!(files_in(dir.path()), vec!["notes.txt".to_string()]);
}

/// The same failure on a real `.gtra` file: the project still opens and is
/// the old version.
///
/// Why it matters: checks the promise on the actual file type, not only on
/// a text file.
#[test]
fn failing_write_keeps_the_old_gtra_file_openable() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);

    let result = atomic_write(&path, |tmp: &Path| {
        fs::write(tmp, b"SQLite format 3\0 garbage").map_err(|e| GtraError::io(tmp, e))?;
        Err(GtraError::InvalidData("disk full (simulated)".into()))
    });
    assert!(result.is_err());
    assert_eq!(normalized(&open_ok(&path).project), normalized(&project));
    assert_eq!(files_in(dir.path()), vec!["shed.gtra".to_string()]);
}

/// `atomic_write_bytes` replaces a file's content completely, and creates
/// the file when it does not exist yet.
///
/// Why it matters: the happy path of the same mechanism, used for small
/// files (settings, recovery data).
#[test]
fn atomic_write_bytes_creates_and_replaces() {
    let dir = temp_dir();
    let target = path_in(&dir, "data.bin");
    atomic_write_bytes(&target, b"first").expect("create");
    assert_eq!(fs::read(&target).expect("read"), b"first");
    atomic_write_bytes(&target, b"second, longer").expect("replace");
    assert_eq!(fs::read(&target).expect("read"), b"second, longer");
    assert_eq!(files_in(dir.path()), vec!["data.bin".to_string()]);
}

/// Saving into a folder that does not exist fails with an error and creates
/// nothing.
///
/// Why it matters: e.g. a USB stick that was unplugged; the user must get an
/// error, not a crash.
///
/// Note: today the error is a *database* error ("unable to open database:
/// .../.plan.gtra.<uuid>.tmp"), because SQLite is the first to try to create
/// the temporary file. The test accepts any error; the wording is reported
/// to files-interop-dev as a possible improvement.
#[test]
fn saving_into_missing_folder_fails_cleanly() {
    let dir = temp_dir();
    let path = dir.path().join("no-such-folder").join("plan.gtra");
    let result = gtra::save(&path, &shed(), &crate::options());
    assert!(result.is_err(), "saving into a missing folder must fail");
    assert!(files_in(dir.path()).is_empty());
}

/// A path with no file name (a folder path such as `..`) is refused.
///
/// Why it matters: the temporary name is built from the file name; without
/// one, saving must stop before touching anything.
#[test]
fn path_without_file_name_is_refused() {
    let result = atomic_write_bytes(Path::new(".."), b"x");
    assert!(
        matches!(result, Err(GtraError::InvalidPath(_))),
        "got {result:?}"
    );
}

/// In a read-only folder the save fails, the existing file is unchanged and
/// still opens, and no temporary file is left. (Unix only: Windows folder
/// permissions work differently.)
///
/// Why it matters: simulates "no permission" or "disk is write-protected"
/// in the middle of real work. The user's last good save must survive.
#[cfg(unix)]
#[test]
fn read_only_folder_keeps_the_old_file() {
    // Imported here, not at the top of the file: on Windows this test does not
    // exist, so the imports would be "unused" there and fail the lint.
    use ganttora_tests::{days, row_id};
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);
    let bytes_before = fs::read(&path).expect("read");

    let mut edited = project.clone();
    let permit = row_id(&edited, 2);
    if let Some(task) = edited.tasks.iter_mut().find(|t| t.id == permit) {
        task.duration = days(6);
    }

    // r-x for everyone: files inside can be read, but none can be created.
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).expect("chmod");
    // The super-user (root) ignores permissions; then this test cannot
    // simulate the failure, so it stops here instead of failing wrongly.
    let probe = dir.path().join("probe");
    let can_still_write = fs::write(&probe, b"x").is_ok();
    let result = if can_still_write {
        let _ = fs::remove_file(&probe);
        None
    } else {
        Some(gtra::save(&path, &edited, &crate::options()))
    };
    // Always give write permission back, so TempDir can delete the folder.
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).expect("chmod back");

    let Some(result) = result else {
        eprintln!("running as root: read-only folder cannot be simulated, test skipped");
        return;
    };
    assert!(
        result.is_err(),
        "the save should fail in a read-only folder"
    );
    assert_eq!(fs::read(&path).expect("read"), bytes_before);
    assert_eq!(normalized(&open_ok(&path).project), normalized(&project));
    assert_eq!(files_in(dir.path()), vec!["shed.gtra".to_string()]);
}
