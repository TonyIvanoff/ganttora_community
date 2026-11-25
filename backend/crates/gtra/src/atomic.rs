//! Atomic saving: write a new file completely, then swap it in with one rename.
//!
//! # What this file is for
//! Saving must never leave a half-written `.gtra` file behind, not even if the
//! computer loses power in the middle. This module
//! provides [`atomic_write`], which every save in Ganttora goes through.
//!
//! # Where it fits
//! Called by [`crate::storage::LocalFileStorage`] when a project is saved, and
//! by [`crate::create_empty`] when a new file is created. Later also by the
//! recovery copies (`recovery.rs`, M1) and templates (`templates.rs`, M3).
//! [`WorkingCopy`], at the end of this file, is the local scratch file used
//! by [`crate::open`] and [`crate::save`].
//!
//! # The main idea
//! Never write into the real file. Write a temporary file next to it, make
//! sure it is really on the disk, then rename it over the real file:
//!
//! ```text
//!   folder/                          folder/                      folder/
//!   ├── shed.gtra   (old)     1.     ├── shed.gtra  (old)    3.   └── shed.gtra  (new)
//!                            ───►    └── .shed.gtra.<uuid>.tmp  ───►
//!                                        (new, being written)
//!                                    2. fsync: force it onto the disk
//! ```
//!
//! A rename inside one folder is "atomic": the operating system does it in one
//! step. Anyone looking at `shed.gtra` sees either the complete old file or the
//! complete new one, never a mix. If anything fails before step 3, the old file
//! is untouched and the temporary file is deleted.
//!
//! The temporary file must be in the **same folder** as the target: a rename
//! between two different disks is not atomic (it becomes copy + delete).
//!

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use crate::GtraError;

/// How many times a failed rename is tried on Windows before giving up.
///
/// # Why
/// On Windows, another program (a virus scanner, a cloud-sync program such as
/// OneDrive, the search indexer) may have the target file open for a moment.
/// Windows then refuses the rename with "access denied". Waiting briefly and
/// trying again almost always works. On macOS and Linux a rename does not
/// fail for this reason, so it is tried once.
const WINDOWS_RENAME_ATTEMPTS: u32 = 6;

/// Wait before the first retry; it doubles after every attempt
/// (50, 100, 200, 400, 800 ms: about 1.5 seconds in total).
const WINDOWS_RENAME_FIRST_WAIT: Duration = Duration::from_millis(50);

/// Writes a file atomically: the target is replaced by a complete new file,
/// or left unchanged if anything goes wrong.
///
/// # What it does
/// 1. Chooses a temporary path in the same folder as `target`, such as
///    `.shed.gtra.4f1c…-….tmp` (the dot at the start hides it on macOS/Linux).
/// 2. Calls `fill` with that path. `fill` must create the file and write the
///    complete new content into it. (For a `.gtra` file this is SQLite's
///    `VACUUM INTO`, which writes a clean copy of the database.)
/// 3. Copies the old file's permissions to the new file (macOS/Linux), so
///    saving does not change who may read the file.
/// 4. Calls `fsync` on the new file: waits until the operating system has
///    really written it to the disk, not just kept it in memory.
/// 5. Renames the temporary file over `target` (retrying on Windows, see
///    the private constant `WINDOWS_RENAME_ATTEMPTS` in this file).
/// 6. On macOS/Linux, calls `fsync` on the folder too, so the rename itself
///    survives a power cut.
///
/// # Parameters
/// - `target`: the file to create or replace, e.g. `/Users/sam/shed.gtra`.
///   Its folder must already exist.
/// - `fill`: a function (closure) that receives the temporary path and writes
///   the new content there. The temporary file does not exist yet when `fill`
///   is called.
///
/// # Returns
/// `Ok(())` when `target` now holds the new content.
///
/// # Errors
/// - [`GtraError::InvalidPath`] if `target` has no file name (e.g. `/`).
/// - Whatever error `fill` returns.
/// - [`GtraError::Io`] if the new file cannot be synced or renamed (disk full,
///   no permission, file locked by another program for too long…).
///
/// In every error case the old `target` is unchanged and the temporary file
/// is removed.
///
/// # Example
/// ```
/// use std::fs;
/// use gtra::atomic::atomic_write;
///
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// fs::create_dir_all(&dir).unwrap();
/// let target = dir.join("hello.txt");
///
/// atomic_write(&target, |tmp| {
///     fs::write(tmp, "first version").map_err(|e| gtra::GtraError::io(tmp, e))
/// }).unwrap();
/// atomic_write(&target, |tmp| {
///     fs::write(tmp, "second version").map_err(|e| gtra::GtraError::io(tmp, e))
/// }).unwrap();
///
/// assert_eq!(fs::read_to_string(&target).unwrap(), "second version");
/// # fs::remove_dir_all(&dir).unwrap();
/// ```
///
/// # Beginner note
/// `F: FnOnce(&Path) -> Result<(), GtraError>` is a **generic** parameter: the
/// caller can pass any function or closure that takes a path and returns a
/// `Result`. "FnOnce" means we promise to call it at most once. A **closure**
/// is a small unnamed function written inline, like `|tmp| fs::write(tmp, …)`.
pub fn atomic_write<F>(target: &Path, fill: F) -> Result<(), GtraError>
where
    F: FnOnce(&Path) -> Result<(), GtraError>,
{
    let temp_path = temp_path_for(target)?;

    // The guard deletes the temporary file automatically if we leave this
    // function early because of an error (see `TempFileGuard` below).
    let guard = TempFileGuard::new(temp_path);

    // Step 2: let the caller write the new content.
    fill(guard.path())?;

    // Step 3: keep the permissions of the file we are replacing.
    copy_permissions_if_exists(target, guard.path())?;

    // Step 4: force the new file's bytes onto the disk.
    sync_file(guard.path())?;

    // Step 5: swap it in. After this line the temporary file no longer exists
    // under its old name, so the guard must not try to delete it.
    rename_with_retry(guard.path(), target)?;
    guard.disarm();

    // Step 6: make the rename itself durable.
    sync_parent_dir(target)?;
    Ok(())
}

/// Writes `bytes` to `target` atomically. A shortcut for [`atomic_write`].
///
/// # Parameters
/// - `target`: the file to create or replace.
/// - `bytes`: the complete new content.
///
/// # Returns / Errors
/// Same as [`atomic_write`].
///
/// # Example
/// ```
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let target = dir.join("note.txt");
/// gtra::atomic::atomic_write_bytes(&target, b"hello").unwrap();
/// assert_eq!(std::fs::read(&target).unwrap(), b"hello");
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn atomic_write_bytes(target: &Path, bytes: &[u8]) -> Result<(), GtraError> {
    atomic_write(target, |tmp| {
        fs::write(tmp, bytes).map_err(|e| GtraError::io(tmp, e))
    })
}

/// Builds the temporary file name for `target`: `.<name>.<random uuid>.tmp`
/// in the same folder.
///
/// # Why a random UUID
/// If two saves ever ran at the same time (or an old temporary file was left
/// behind by a crash), each still gets its own unique name.
///
/// # Errors
/// [`GtraError::InvalidPath`] if `target` has no file name.
fn temp_path_for(target: &Path) -> Result<PathBuf, GtraError> {
    let file_name = target
        .file_name()
        .ok_or_else(|| GtraError::InvalidPath(target.to_path_buf()))?;
    // `parent()` of "shed.gtra" is "" (the current folder). Joining onto ""
    // gives a relative path in the current folder, which is what we want.
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let temp_name = format!(".{}.{}.tmp", file_name.to_string_lossy(), Uuid::new_v4());
    Ok(parent.join(temp_name))
}

/// Copies the permissions of `from` to `to`, if `from` exists.
///
/// # Why
/// A newly created file gets default permissions. If the user had made
/// `shed.gtra` read-only for others, saving should not quietly undo that.
///
/// # Errors
/// [`GtraError::Io`] if `to` cannot be changed. A missing `from` is not an
/// error (the first save of a new file).
fn copy_permissions_if_exists(from: &Path, to: &Path) -> Result<(), GtraError> {
    match fs::metadata(from) {
        Ok(meta) => fs::set_permissions(to, meta.permissions()).map_err(|e| GtraError::io(to, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GtraError::io(from, e)),
    }
}

/// Opens `path` and waits until all its data is physically on the disk
/// (`fsync`).
///
/// # Errors
/// [`GtraError::Io`] if the file cannot be opened or synced.
fn sync_file(path: &Path) -> Result<(), GtraError> {
    // On Windows, `sync_all` needs write access, so open for writing.
    // `OpenOptions` lets us choose exactly how a file is opened.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| GtraError::io(path, e))?;
    file.sync_all().map_err(|e| GtraError::io(path, e))
}

/// Renames `from` to `to`, replacing `to` if it exists. On Windows, retries
/// a few times if the file is briefly locked by another program.
///
/// # Errors
/// [`GtraError::Io`] with the last error if every attempt failed.
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), GtraError> {
    // `cfg!(windows)` is `true` when compiling for Windows. Using the macro
    // (instead of `#[cfg]`) keeps this code compiled and checked everywhere.
    let attempts = if cfg!(windows) {
        WINDOWS_RENAME_ATTEMPTS
    } else {
        1
    };
    let mut wait = WINDOWS_RENAME_FIRST_WAIT;
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            // "Access denied" on Windows usually means "someone has the file
            // open right now". Anything else (disk full…) will not get better
            // by waiting, so we stop immediately.
            Err(e) if is_file_locked_error(&e) && attempt < attempts => {
                thread::sleep(wait);
                wait *= 2;
                attempt += 1;
            }
            Err(e) => return Err(GtraError::io(to, e)),
        }
    }
}

/// Windows error code 32, `ERROR_SHARING_VIOLATION`: "the file is being used
/// by another process".
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;

/// Windows error code 33, `ERROR_LOCK_VIOLATION`: "another process has locked
/// a portion of the file".
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

/// Returns `true` if `error` means "another program has the file open right
/// now", which is worth waiting for.
///
/// # Parameters
/// - `error`: the error returned by `fs::rename`.
///
/// # Returns
/// `true` for "access denied" and, on Windows, for the sharing / lock
/// violation codes above (Rust does not give those a named `ErrorKind`, so we
/// compare the raw operating-system number).
fn is_file_locked_error(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::PermissionDenied {
        return true;
    }
    cfg!(windows)
        && matches!(
            error.raw_os_error(),
            Some(WINDOWS_ERROR_SHARING_VIOLATION | WINDOWS_ERROR_LOCK_VIOLATION)
        )
}

/// On macOS and Linux, `fsync`s the folder that contains `path`, so that the
/// rename is recorded on the disk. Does nothing on Windows, where folders
/// cannot be opened like this (NTFS records renames durably by itself).
///
/// # Errors
/// [`GtraError::Io`] if the folder cannot be opened or synced.
fn sync_parent_dir(path: &Path) -> Result<(), GtraError> {
    if cfg!(windows) {
        return Ok(());
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = File::open(parent).map_err(|e| GtraError::io(parent, e))?;
    dir.sync_all().map_err(|e| GtraError::io(parent, e))
}

/// Deletes a temporary file when it goes out of scope, unless told not to.
///
/// # Why
/// `atomic_write` has many places where an error makes it return early (each
/// `?`). Instead of writing "delete the temporary file" before every one of
/// them, this guard does it automatically.
///
/// # Beginner note
/// Rust calls a value's `drop` method automatically when the value is no
/// longer used (here: when `atomic_write` returns, normally or with an error).
/// Tying clean-up to that moment is a common Rust pattern called **RAII**
/// ("resource acquisition is initialization"). See the glossary.
struct TempFileGuard {
    /// Path of the temporary file.
    path: PathBuf,
    /// `true` while the guard should still delete the file.
    armed: bool,
}

impl TempFileGuard {
    /// Creates an armed guard for `path`.
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// The temporary file's path.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Stops the guard from deleting the file (used after the rename).
    ///
    /// `mut self` takes the guard by value: after `disarm()` it is dropped at
    /// once, with `armed` set to `false`, so nothing is deleted.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    /// Deletes the temporary file if the guard is still armed. Errors are
    /// ignored on purpose: the file may never have been created, and a
    /// failed clean-up must not hide the real error.
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// A private, local scratch copy of a `.gtra` file, deleted automatically
/// when it is no longer needed.
///
/// # What it is for
/// [`crate::open`] and [`crate::save`] never let SQLite work directly on the
/// user's file, which may be in a network or cloud-synced folder.
/// Instead they copy the file to a working copy in the computer's temporary
/// folder, let SQLite work there, and (for a save) write the result back with
/// [`atomic_write`]:
///
/// ```text
///   user's folder (maybe synced)           local temp folder
///   ┌────────────┐   1. copy (fetch_to)    ┌──────────────────────────────┐
///   │ shed.gtra  │ ──────────────────────► │ ganttora-working-<uuid>.gtra │
///   │            │                         │  2. SQLite reads / writes    │
///   │            │ ◄────────────────────── │     here only                │
///   └────────────┘   3. save: VACUUM INTO  └──────────────────────────────┘
///                       a temp file next to   4. deleted automatically
///                       shed.gtra + rename       (Drop)
/// ```
///
/// Creating a `WorkingCopy` only chooses a unique path; nothing is written
/// until someone copies or creates a file there.
///
/// # Example
/// ```
/// use gtra::atomic::WorkingCopy;
/// let path;
/// {
///     let working = WorkingCopy::in_temp_dir();
///     path = working.path().to_path_buf();
///     std::fs::write(&path, b"scratch").unwrap();
///     assert!(path.exists());
/// } // `working` goes out of scope here and deletes the file.
/// assert!(!path.exists());
/// ```
///
/// # Beginner note
/// This uses the same "clean up in `drop`" pattern (RAII) as the private
/// temporary-file guard above. Remember that Rust drops local variables in
/// the **reverse** order of their creation: create the `WorkingCopy` *before*
/// opening a database inside it, so the database is closed first and the
/// file deleted afterwards.
#[derive(Debug)]
pub struct WorkingCopy {
    /// Where the working copy lives (it may not exist yet).
    path: PathBuf,
}

impl WorkingCopy {
    /// Chooses a new, unique working-copy path in the operating system's
    /// temporary folder (for example `/tmp` on Linux or `%TEMP%` on Windows),
    /// which is a local disk.
    ///
    /// # Returns
    /// The `WorkingCopy`; its file does not exist yet.
    pub fn in_temp_dir() -> Self {
        Self::in_dir(&std::env::temp_dir())
    }

    /// Chooses a new, unique working-copy path inside `dir`.
    ///
    /// # Parameters
    /// - `dir`: a folder on a local disk. It must exist before a file is
    ///   written to the working copy.
    ///
    /// # Returns
    /// The `WorkingCopy`, named `ganttora-working-<random uuid>.gtra`.
    ///
    /// # Example
    /// ```
    /// let dir = std::env::temp_dir();
    /// let working = gtra::atomic::WorkingCopy::in_dir(&dir);
    /// assert!(working.path().starts_with(&dir));
    /// ```
    pub fn in_dir(dir: &Path) -> Self {
        let name = format!("ganttora-working-{}.gtra", Uuid::new_v4());
        Self {
            path: dir.join(name),
        }
    }

    /// The working copy's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkingCopy {
    /// Deletes the working copy and the `-journal` file SQLite may have left
    /// next to it after a crash. Errors are ignored on purpose (the files may
    /// never have been created).
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        // `OsString` is the operating system's kind of text, used for paths.
        let mut journal = self.path.clone().into_os_string();
        journal.push("-journal");
        let _ = fs::remove_file(journal);
    }
}
