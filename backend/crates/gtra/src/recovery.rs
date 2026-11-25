//! Autosave and crash-recovery copies.
//!
//! # What this file is for
//! If Ganttora crashes or the computer loses power, the user must not lose
//! more than a few minutes of work (§5). While a project is
//! open, the app regularly writes a complete **recovery copy** of it into a
//! folder in the app's data area. After a clean close the copy is deleted.
//! If the app finds copies at the next start, something went wrong, and it
//! offers to restore them.
//!
//! # Where it fits
//! ```text
//!   src-tauri (timer every N minutes, from Settings → Files)
//!        │  session.autosave(&project, &options, &store)
//!        ▼
//!   lib.rs: ProjectSession ── writes the model into its working copy
//!        │
//!        ▼
//!   recovery.rs: RecoveryStore::write   ◄── you are here
//!        │  VACUUM INTO a temp file, stamp it, rename (atomic.rs)
//!        ▼
//!   <app data>/recovery/<session id>.gtra
//! ```
//!
//! This crate never asks the operating system where the app data folder is.
//! The Tauri shell finds it (`src-tauri/src/paths.rs`) and passes the path to
//! [`RecoveryStore::new`]. That keeps this crate easy to test.
//!
//! # The recovery folder
//! ```text
//!   <app data>/recovery/
//!   ├── 5f0c…-….gtra          one copy per open project ("session"), named
//!   ├── 9a71…-….gtra          after the session's UUID; overwritten by each
//!   │                         autosave of that session
//!   └── .9a71….gtra.<uuid>.tmp  a copy being written right now (ignored)
//! ```
//!
//! Each copy is an ordinary `.gtra` database. Three extra rows in its `meta`
//! table say where it came from. They exist only in recovery copies and are
//! removed again when a copy is restored:
//!
//! | `meta` key            | value                                           |
//! |-----------------------|-------------------------------------------------|
//! | `recovery_session_id` | the session's UUID (same as the file name)      |
//! | `recovery_source`     | path of the user's `.gtra` file; absent for a project never saved |
//! | `recovery_saved_at`   | when the copy was written, `2027-02-15T10:42:07Z` |
//!
//! Keeping this information *inside* the copy (not in a second file next to
//! it) means one atomic rename writes everything: there is never a copy
//! without its description, or a description without its copy.
//!

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use uuid::Uuid;

use crate::atomic::atomic_write;
use crate::schema::{self, GTRA_APPLICATION_ID};
use crate::{GtraError, GtraFile, set_meta};

/// `meta` key holding the session UUID in a recovery copy.
pub const META_SESSION_ID: &str = "recovery_session_id";
/// `meta` key holding the path of the user's file in a recovery copy.
pub const META_SOURCE: &str = "recovery_source";
/// `meta` key holding the time a recovery copy was written.
pub const META_SAVED_AT: &str = "recovery_saved_at";

/// The file extension of recovery copies (the same as normal files).
const EXTENSION: &str = "gtra";

/// A folder that holds recovery copies.
///
/// # Example
/// ```
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// let store = gtra::RecoveryStore::new(dir.join("recovery")).unwrap();
/// assert!(store.list().unwrap().is_empty());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryStore {
    /// The folder, e.g. `<app data>/recovery`.
    dir: PathBuf,
}

/// What is known about one recovery copy found by [`RecoveryStore::list`].
///
/// A copy that cannot be read (damaged, or not a `.gtra` file) is still
/// listed, with `readable: false`, so the app can offer to delete it instead
/// of hiding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCopy {
    /// The session UUID (taken from the file name).
    pub session_id: Uuid,
    /// Full path of the copy.
    pub path: PathBuf,
    /// The user's file it belongs to; `None` for a project that was never
    /// saved, or when the copy is unreadable.
    pub source: Option<PathBuf>,
    /// When the copy was written (`2027-02-15T10:42:07Z`), if known.
    pub saved_at: Option<String>,
    /// The project's name, if readable.
    pub project_name: Option<String>,
    /// The copy's format version (0 if unreadable). A copy newer than this
    /// app can only be restored read-only.
    pub format_version: u32,
    /// `false` if the copy could not be opened and read.
    pub readable: bool,
}

impl RecoveryStore {
    /// Uses `dir` as the recovery folder, creating it (and its parents) if
    /// needed.
    ///
    /// # Parameters
    /// - `dir`: a folder on a local disk, normally `<app data>/recovery`.
    ///
    /// # Errors
    /// [`GtraError::Io`] if the folder cannot be created.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, GtraError> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| GtraError::io(&dir, e))?;
        Ok(Self { dir })
    }

    /// The recovery folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the copy of session `session_id` lives (it may not exist).
    ///
    /// # Example
    /// ```
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// let store = gtra::RecoveryStore::new(&dir).unwrap();
    /// let id = uuid::Uuid::new_v4();
    /// assert_eq!(store.path_for(id), dir.join(format!("{id}.gtra")));
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn path_for(&self, session_id: Uuid) -> PathBuf {
        self.dir.join(format!("{session_id}.{EXTENSION}"))
    }

    /// Writes a recovery copy of `file`, atomically, replacing the session's
    /// previous copy.
    ///
    /// # What it does
    /// 1. Writes a clean copy of the database to a temporary file in the
    ///    recovery folder (`VACUUM INTO`).
    /// 2. Opens that temporary file and adds the three `recovery_*` rows to
    ///    its `meta` table.
    /// 3. Renames it to `<session id>.gtra` ([`atomic_write`]).
    ///
    /// Normally called through [`crate::ProjectSession::autosave`], which
    /// first writes the latest model into the working copy.
    ///
    /// # Parameters
    /// - `file`: the open database to copy (normally a session's working copy).
    /// - `session_id`: which session the copy belongs to.
    /// - `source`: the user's file, or `None` for a project never saved.
    /// - `saved_at`: the time to record, `2027-02-15T10:42:07Z`.
    ///
    /// # Returns
    /// The [`RecoveryCopy`] just written.
    ///
    /// # Errors
    /// Database or file errors. On any error the previous copy is unchanged.
    pub fn write(
        &self,
        file: &GtraFile,
        session_id: Uuid,
        source: Option<&Path>,
        saved_at: &str,
    ) -> Result<RecoveryCopy, GtraError> {
        let target = self.path_for(session_id);
        // A path that is not valid Unicode cannot be stored as text; the copy
        // is then treated like a never-saved project (the user picks a
        // place with Save As when restoring).
        let source_text = source.and_then(Path::to_str);

        atomic_write(&target, |tmp| {
            let tmp_str = tmp
                .to_str()
                .ok_or_else(|| GtraError::InvalidPath(tmp.to_path_buf()))?;
            file.conn.execute("VACUUM INTO ?1", [tmp_str])?;

            let conn = Connection::open_with_flags(tmp, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            set_meta(&conn, META_SESSION_ID, &session_id.to_string())?;
            match source_text {
                Some(text) => set_meta(&conn, META_SOURCE, text)?,
                // The working copy may carry an old value (a restored copy
                // that was "Saved As" elsewhere); make sure none is left.
                None => {
                    conn.execute("DELETE FROM meta WHERE key = ?1", [META_SOURCE])?;
                }
            }
            set_meta(&conn, META_SAVED_AT, saved_at)?;
            // Close before the rename: on Windows an open file cannot be renamed.
            conn.close().map_err(|(_conn, e)| GtraError::Sqlite(e))
        })?;

        Ok(describe(session_id, target))
    }

    /// Lists every recovery copy in the folder, newest first.
    ///
    /// # What it does
    /// Looks at every file named `<uuid>.gtra`, opens it read-only and reads
    /// the `recovery_*` rows and the project name. Other files (such as a
    /// temporary file left by a crash during an autosave) are ignored.
    ///
    /// # Returns
    /// The copies, sorted by `saved_at` (newest first); unreadable copies
    /// come last.
    ///
    /// > **Note:** a copy that belongs to a project open *right now* in this
    /// > app is listed too. The caller should leave out the session ids it
    /// > has open itself.
    ///
    /// # Errors
    /// [`GtraError::Io`] if the folder cannot be read. Problems with single
    /// copies do not fail the list; they show up as `readable: false`.
    pub fn list(&self) -> Result<Vec<RecoveryCopy>, GtraError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            // A missing folder simply means "no copies".
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(GtraError::io(&self.dir, e)),
        };

        let mut copies = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| GtraError::io(&self.dir, e))?;
            let path = entry.path();
            if let Some(session_id) = session_id_from_name(&path) {
                copies.push(describe(session_id, path));
            }
        }
        // `sort_by` with a closure that compares two copies. Readable copies
        // first; among those, the later `saved_at` first (ISO timestamps sort
        // correctly as text). `.reverse()` turns "smallest first" around.
        copies.sort_by(|a, b| {
            (a.readable, &a.saved_at)
                .cmp(&(b.readable, &b.saved_at))
                .reverse()
        });
        Ok(copies)
    }

    /// Deletes the recovery copy of a session. Call this after a clean close
    /// (the project was saved, or the user chose "Don't save"), or when the
    /// user declines to restore a copy.
    ///
    /// # Parameters
    /// - `session_id`: the session whose copy to delete.
    ///
    /// # Returns
    /// `Ok(())`, also when there was no copy.
    ///
    /// # Errors
    /// [`GtraError::Io`] if the copy exists but cannot be deleted.
    pub fn discard(&self, session_id: Uuid) -> Result<(), GtraError> {
        let path = self.path_for(session_id);
        remove_if_exists(&path)?;
        // SQLite's rollback file, which a crash while writing may leave.
        let mut journal = path.into_os_string();
        journal.push("-journal");
        remove_if_exists(Path::new(&journal))
    }
}

/// Reads what can be read about one copy; never fails.
fn describe(session_id: Uuid, path: PathBuf) -> RecoveryCopy {
    // `match` on the result of the helper: on any error the copy is listed
    // as unreadable instead of failing the whole list.
    match read_copy_info(&path) {
        Ok(info) => RecoveryCopy {
            session_id,
            path,
            source: info.source,
            saved_at: info.saved_at,
            project_name: info.project_name,
            format_version: info.format_version,
            readable: true,
        },
        Err(_) => RecoveryCopy {
            session_id,
            path,
            source: None,
            saved_at: None,
            project_name: None,
            format_version: 0,
            readable: false,
        },
    }
}

/// What [`read_copy_info`] finds inside a readable copy.
struct CopyInfo {
    /// `meta.recovery_source` as a path.
    source: Option<PathBuf>,
    /// `meta.recovery_saved_at`.
    saved_at: Option<String>,
    /// `project.name`.
    project_name: Option<String>,
    /// `PRAGMA user_version`.
    format_version: u32,
}

/// Opens a copy read-only and reads its description.
///
/// # Errors
/// Any database error, or [`GtraError::NotAGtraFile`] for a foreign file.
fn read_copy_info(path: &Path) -> Result<CopyInfo, GtraError> {
    // The recovery folder is on a local disk and belongs to this app, so
    // reading it directly (read-only) is safe; no working copy is needed.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let app_id = schema::read_application_id(&conn)?;
    let version = schema::read_format_version(&conn)?;
    if app_id != GTRA_APPLICATION_ID || version == 0 {
        return Err(GtraError::NotAGtraFile(path.to_path_buf()));
    }
    let meta = |key: &str| -> Result<Option<String>, GtraError> {
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    };
    Ok(CopyInfo {
        source: meta(META_SOURCE)?.map(PathBuf::from),
        saved_at: meta(META_SAVED_AT)?,
        // `.ok()` turns an error (e.g. no project row) into `None`: a copy
        // without a name is still worth restoring.
        project_name: conn
            .query_row("SELECT name FROM project LIMIT 1", [], |row| row.get(0))
            .ok(),
        format_version: version,
    })
}

/// Returns the session UUID if `path` is named `<uuid>.gtra`.
fn session_id_from_name(path: &Path) -> Option<Uuid> {
    // `?` inside a function returning `Option` returns `None` early when a
    // step has no value.
    if path.extension()?.to_str()? != EXTENSION {
        return None;
    }
    Uuid::parse_str(path.file_stem()?.to_str()?).ok()
}

/// Deletes a file; a missing file is not an error.
fn remove_if_exists(path: &Path) -> Result<(), GtraError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GtraError::io(path, e)),
    }
}
