//! Crate `gtra`: Ganttora's own file format, `.gtra`.
//!
//! # What this crate is for
//! A `.gtra` file is a **SQLite database**: one ordinary file that holds many
//! tables (tasks, dependencies, calendars, the change journal, …). This crate
//! creates such files, opens them, upgrades older ones and saves them safely.
//! The table layout is in `migrations/0001_init.sql`, changed by the later
//! numbered migrations (`0002_m1_fields.sql`, `0003_blockers.sql`, …).
//!
//! # Where it fits in the project
//! ```text
//!   backend/src-tauri  (File > Open / Save: ipc/file.rs)
//!         │  calls open() / save() with document.project()
//!         ▼
//!   backend/crates/gtra  ◄── you are here
//!     lib.rs      open(), save()            the simple API (one call each)
//!                 create_empty(), open_db(), GtraFile, GtraError
//!     read.rs     tables → model::Project   (GtraFile::read_project)
//!     write.rs    model::Project → tables   (GtraFile::write_project)
//!     schema.rs   format versions, migration runner
//!     compat.rs   newer file → read-only, with a reason code
//!     session.rs  ProjectSession            a project kept open while edited:
//!                                           save, save as, autosave, close
//!     journal.rs  append / read change records (model::ChangeRecord)
//!     recovery.rs autosave copies in the app data folder
//!     atomic.rs   "write a temp file, then rename"; local working copies
//!     storage.rs  Storage trait, LocalFileStorage
//!         │
//!         ▼
//!   SQLite (inside the app, via the rusqlite library)
//! ```
//!
//! # The two calls most code needs
//! ```text
//!   gtra::open(path)                         gtra::save(path, &project, &options)
//!   ─────────────────                        ────────────────────────────────────
//!   1. copy the file to a local              1. check the project
//!      working copy                          2. copy the existing file (if any) to a
//!   2. open it (upgrade the COPY if             local working copy, or create a new one
//!      it is older; read-only if newer)      3. write the project into the copy
//!   3. read the project                      4. VACUUM INTO a temp file next to the
//!   4. delete the working copy                  target, then rename it into place
//!   → OpenedProject                          5. delete the working copy
//! ```
//! The user's file is never opened by SQLite and never changed in place.
//!
//! # The app's calls: a `ProjectSession`
//! `open` and `save` forget the file in between. The desktop app instead
//! keeps a [`ProjectSession`]: the working copy stays open (in a folder the
//! app chooses) for as long as the project window is open. Data the model
//! cannot hold yet (resources, project notes, the journal, …) stays in that working
//! copy, so **Save As never drops it**:
//!
//! ```text
//!   ProjectSession::open(user file, working dir)
//!        │   working copy = copy of the user's file (upgraded if older)
//!        ▼
//!   ┌─────────── working copy (stays open) ───────────┐
//!   │ save(project)      model on top → VACUUM INTO → │──► user's file
//!   │ save_as(path, p)   model on top → VACUUM INTO → │──► new file (all data kept)
//!   │ autosave(p, store) model on top → VACUUM INTO → │──► <app data>/recovery/<id>.gtra
//!   │ append_journal(…)  change records               │
//!   └──────────────────────────────────────────────────┘
//!        │  close(Some(&store)): delete working copy + recovery copy
//!        ▼
//! ```
//!
//! # Main ideas to know first
//! - **Every item has a UUID** as its permanent id, never a row number.
//! - **Format version**: every file records which version of the table layout
//!   it uses. Older files are upgraded by running *migrations*; files from a
//!   newer Ganttora are opened **read-only** ([`schema`]).
//! - **Atomic save**: a save writes a complete new file next to the old one
//!   and then renames it into place ([`atomic`]), so a crash can never leave a
//!   half-written file.
//! - **Work on a local copy**: the file the user opened is copied to a local
//!   working file first ([`storage::Storage::fetch_to`]); [`open_db`] is then
//!   used on that working copy, and [`GtraFile::save_to`] writes it back.
//!
//! # Quick example
//! ```
//! use chrono::NaiveDate;
//! use model::{Duration, Project, Task};
//!
//! let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
//! std::fs::create_dir_all(&dir).unwrap();
//! let path = dir.join("shed.gtra");
//!
//! let mut project = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
//! project.tasks.push(Task::new("Design the shed", Duration::from_days(2)));
//!
//! gtra::save(&path, &project, &gtra::SaveOptions::now("Sam Lee")).unwrap();
//! let opened = gtra::open(&path).unwrap();
//! assert_eq!(opened.project, project);
//! assert_eq!(opened.access, gtra::Access::ReadWrite);
//! assert_eq!(opened.info.author.as_deref(), Some("Sam Lee"));
//! # std::fs::remove_dir_all(&dir).unwrap();
//! ```
//!

// `pub mod` makes each file below a public "module" (a named part of the
// crate) that other crates can use as `gtra::atomic`, `gtra::schema`, …
pub mod atomic;
pub mod compat;
pub mod journal;
pub mod read;
pub mod recovery;
pub mod schema;
pub mod session;
pub mod storage;
pub mod write;

// Re-exports: the types used with `open`, `save` and `ProjectSession`,
// available directly as `gtra::OpenedProject`, `gtra::SaveOptions`, …
pub use compat::{Compatibility, ReadOnlyReason};
pub use journal::{JournalEntry, JournalFilter, JournalPage};
pub use read::{FileInfo, OpenedProject, ReadNote};
pub use recovery::{RecoveryCopy, RecoveryStore};
pub use session::ProjectSession;
pub use write::SaveOptions;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use model::Project;
use rusqlite::{Connection, ErrorCode, OpenFlags};
use uuid::Uuid;

use crate::atomic::WorkingCopy;
use crate::schema::{CURRENT_FORMAT_VERSION, GTRA_APPLICATION_ID, VersionStatus};
use crate::storage::{LocalFileStorage, Storage};

/// How a schedule date-time is written in the file: project-local wall-clock
/// time without a time zone, e.g. `2027-03-01T08:00:00` (see the value formats
/// in `0001_init.sql`). `%Y` is the year, `%m` the month, `%d` the day,
/// `%H:%M:%S` the time.
const DATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// How a date is written in the file, e.g. `2027-03-01`.
const DATE_FORMAT: &str = "%Y-%m-%d";

/// The version of this crate (and of the Ganttora workspace), e.g. `"0.1.0"`.
/// Written to `meta.app_version` whenever a file is created or saved.
///
/// `env!("CARGO_PKG_VERSION")` is filled in by Cargo at compile time from
/// `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything that can go wrong in this crate.
///
/// # Beginner note
/// An **enum** is a type whose value is exactly one of several variants, like
/// "red, green or blue". Here each variant is one kind of error, and some
/// carry extra details (a path, a version number). `#[derive(thiserror::Error)]`
/// and the `#[error("…")]` lines generate the human-readable message for each
/// variant. Functions return `Result<T, GtraError>`: either a success value
/// `T` or one of these errors. See the glossary entries "enum" and "Result".
#[derive(Debug, thiserror::Error)]
pub enum GtraError {
    /// The operating system reported a problem with a file or folder
    /// (not found, no permission, disk full, …).
    #[error("file error at {path}: {source}")]
    Io {
        /// The file or folder concerned.
        path: PathBuf,
        /// The original error from the operating system.
        source: io::Error,
    },

    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// The file is not a `.gtra` file: not a SQLite database at all, or a
    /// SQLite database made by another program.
    #[error("{0} is not a Ganttora (.gtra) file")]
    NotAGtraFile(PathBuf),

    /// [`create_empty`] was asked to create a file that already exists.
    #[error("{0} already exists")]
    FileExists(PathBuf),

    /// A path cannot be used, e.g. it has no file name.
    #[error("invalid file path: {0}")]
    InvalidPath(PathBuf),

    /// The file uses a newer format than this app can write. (Opening such
    /// a file is allowed, read-only; this error comes from trying to
    /// upgrade or change it.)
    #[error(
        "the file uses format version {found}; this version of Ganttora supports up to {supported}"
    )]
    NewerFormat {
        /// The file's format version.
        found: u32,
        /// [`schema::CURRENT_FORMAT_VERSION`] of this app.
        supported: u32,
    },

    /// An attempt to change a file that was opened read-only.
    #[error("the file was opened read-only (it was made with a newer version of Ganttora)")]
    ReadOnly,

    /// A migration failed while upgrading a file. The file is unchanged.
    #[error("upgrading the file failed in migration {name}: {source}")]
    Migration {
        /// Name of the migration, e.g. `"0002_task_colour"`.
        name: &'static str,
        /// The SQLite error.
        source: rusqlite::Error,
    },

    /// The file is a `.gtra` file, but its content breaks a rule that the
    /// database cannot check by itself (for example two `project` rows, an
    /// unreadable date, or tasks whose parents form a loop). Comes from
    /// reading ([`GtraFile::read_project`]).
    #[error("the file contains invalid data: {0}")]
    InvalidData(String),

    /// The project to be saved breaks a rule of the file format (for example
    /// a dependency to a task that does not exist). Nothing was written.
    /// Comes from [`write::validate`]. This normally means a bug in the
    /// calling code, not a user mistake.
    #[error("the project cannot be saved: {0}")]
    InvalidProject(String),

    /// A change record handed to [`GtraFile::append_journal`] breaks a rule.
    /// Today the only check is that its `field` is not empty (the types of
    /// `ChangeRecord` already guarantee a valid timestamp, entity kind and
    /// UUIDs). Nothing was written. Like
    /// `InvalidProject`, this normally means a bug in the calling code.
    #[error("the change record cannot be stored: {0}")]
    InvalidRecord(String),

    /// [`ProjectSession::save`] was called for a project that has never been
    /// saved, so there is no file to save to. The app should ask for a place
    /// and call [`ProjectSession::save_as`] instead.
    #[error("the project has not been saved to a file yet; use Save As")]
    NoSavePath,
}

impl GtraError {
    /// Wraps an operating-system error together with the path it concerns.
    ///
    /// # Parameters
    /// - `path`: the file or folder the error is about.
    /// - `source`: the error returned by a `std::fs` function.
    ///
    /// # Returns
    /// A [`GtraError::Io`].
    ///
    /// # Example
    /// ```
    /// let err = std::fs::read("/no/such/file").unwrap_err();
    /// let err = gtra::GtraError::io("/no/such/file", err);
    /// assert!(err.to_string().contains("/no/such/file"));
    /// ```
    pub fn io(path: impl AsRef<Path>, source: io::Error) -> Self {
        GtraError::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A stable, language-neutral code for this error.
    ///
    /// # Why
    /// The English message (from `#[error("…")]`) is for logs and developers.
    /// The user sees a translated message: the UI looks it up under
    /// `error.` + code in the locale files (e.g. `error.gtra.newer_format`).
    /// Codes never change once released; messages may.
    ///
    /// # Returns
    /// A code such as `"gtra.not_a_gtra_file"`. Every variant has its own.
    ///
    /// # Example
    /// ```
    /// let err = gtra::GtraError::NewerFormat { found: 3, supported: 1 };
    /// assert_eq!(err.code(), "gtra.newer_format");
    /// ```
    pub fn code(&self) -> &'static str {
        // A `match` must list every variant, so the compiler reminds us to
        // add a code whenever a new error variant is added.
        match self {
            GtraError::Io { source, .. } => match source.kind() {
                io::ErrorKind::NotFound => "gtra.file_not_found",
                io::ErrorKind::PermissionDenied => "gtra.permission_denied",
                _ => "gtra.io",
            },
            GtraError::Sqlite(_) => "gtra.database",
            GtraError::NotAGtraFile(_) => "gtra.not_a_gtra_file",
            GtraError::FileExists(_) => "gtra.file_exists",
            GtraError::InvalidPath(_) => "gtra.invalid_path",
            GtraError::NewerFormat { .. } => "gtra.newer_format",
            GtraError::ReadOnly => "gtra.read_only",
            GtraError::Migration { .. } => "gtra.migration_failed",
            GtraError::InvalidData(_) => "gtra.invalid_data",
            GtraError::InvalidProject(_) => "gtra.invalid_project",
            GtraError::InvalidRecord(_) => "gtra.invalid_record",
            GtraError::NoSavePath => "gtra.no_save_path",
        }
    }
}

/// How a [`GtraFile`] was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Normal: the file can be read, changed and saved.
    ReadWrite,
    /// The file comes from a newer Ganttora. It can be read but not changed,
    /// because this app could damage data it does not understand
    /// The UI shows a notice.
    ReadOnly,
}

/// An open `.gtra` database.
///
/// # What it holds
/// The SQLite connection, the path, and how the file was opened. The methods
/// that read and write the whole project live in `read.rs`
/// ([`GtraFile::read_project`]) and `write.rs` ([`GtraFile::write_project`]).
///
/// Most code does not need a `GtraFile` at all: [`open`] and [`save`] use one
/// internally, and the desktop app uses a [`ProjectSession`], which keeps one
/// open as its working copy.
///
/// # Beginner note
/// A **struct** groups related values under one name, like a form with
/// fields. The fields here are private (no `pub`): other code uses the
/// methods below instead, so the struct can make sure its rules are kept
/// (for example, "a read-only file is never written").
#[derive(Debug)]
pub struct GtraFile {
    /// The open SQLite connection.
    conn: Connection,
    /// Path of the database file (normally the local working copy).
    path: PathBuf,
    /// Read-write or read-only.
    access: Access,
    /// The file's format version as it is now (after any upgrade).
    format_version: u32,
    /// The version before an upgrade, if [`open_db`] upgraded the file.
    upgraded_from: Option<u32>,
}

impl GtraFile {
    /// The path of the open database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The SQLite connection, for the reading and writing code of this crate
    /// (and, for now, for tests and tools).
    ///
    /// # Returns
    /// A borrowed connection. Use [`GtraFile::is_read_only`] before writing.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// How the file was opened.
    pub fn access(&self) -> Access {
        self.access
    }

    /// `true` if the file was opened read-only (it is from a newer Ganttora).
    pub fn is_read_only(&self) -> bool {
        self.access == Access::ReadOnly
    }

    /// The file's format version (after any upgrade done by [`open_db`]).
    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    /// If [`open_db`] upgraded the file, the version it had before.
    ///
    /// # Returns
    /// `Some(old_version)` after an upgrade, `None` otherwise. The UI can use
    /// this to tell the user "this file was upgraded; older versions of
    /// Ganttora will open it read-only after you save".
    pub fn upgraded_from(&self) -> Option<u32> {
        self.upgraded_from
    }

    /// Reads one value from the `meta` table.
    ///
    /// # Parameters
    /// - `key`: e.g. `"author"`, `"project_id"`, `"created"`.
    ///
    /// # Returns
    /// `Ok(Some(value))` if the key exists, `Ok(None)` if not.
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if the query fails.
    ///
    /// # Example
    /// See the crate-level example above.
    pub fn meta(&self, key: &str) -> Result<Option<String>, GtraError> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        // `?` after `rows.next()` passes on a database error; `Some(row)`
        // means a row was found.
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Saves a complete copy of this database to `storage`, atomically.
    ///
    /// # What it does
    /// 1. Refuses if the file is read-only.
    /// 2. Updates `meta.modified` (now, UTC) and `meta.app_version`.
    /// 3. Asks the storage for a temporary path and writes a clean copy of
    ///    the database there with SQLite's `VACUUM INTO` (which also removes
    ///    unused space, so saved files stay small).
    /// 4. The storage renames the copy into place ([`atomic::atomic_write`]).
    ///
    /// # Parameters
    /// - `storage`: where to save, e.g. a [`LocalFileStorage`] for the user's
    ///   `.gtra` file.
    ///
    /// # Errors
    /// [`GtraError::ReadOnly`] for a read-only file; otherwise database or
    /// file errors. On any error the stored file is unchanged.
    ///
    /// # Example
    /// ```
    /// use gtra::storage::LocalFileStorage;
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// std::fs::create_dir_all(&dir).unwrap();
    ///
    /// let working = gtra::create_empty(dir.join("working.gtra")).unwrap();
    /// working.save_to(&LocalFileStorage::new(dir.join("shed.gtra"))).unwrap();
    ///
    /// let saved = gtra::open_db(dir.join("shed.gtra")).unwrap();
    /// assert_eq!(saved.meta("project_id").unwrap(), working.meta("project_id").unwrap());
    /// // Close before deleting: Windows refuses to delete an open file.
    /// saved.close().unwrap();
    /// working.close().unwrap();
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn save_to(&self, storage: &dyn Storage) -> Result<(), GtraError> {
        if self.is_read_only() {
            return Err(GtraError::ReadOnly);
        }
        set_meta(&self.conn, "modified", &now_utc())?;
        set_meta(&self.conn, "app_version", APP_VERSION)?;
        self.copy_to(storage)
    }

    /// Writes a complete copy of this database to `storage`, atomically,
    /// without changing anything in `meta` first. Shared by
    /// [`GtraFile::save_to`] and [`save`] (which has already written `meta`
    /// with the caller's timestamp).
    ///
    /// # Errors
    /// [`GtraError::ReadOnly`] for a read-only file; otherwise database or
    /// file errors. On any error the stored file is unchanged.
    fn copy_to(&self, storage: &dyn Storage) -> Result<(), GtraError> {
        if self.is_read_only() {
            return Err(GtraError::ReadOnly);
        }
        storage.save_with(&mut |tmp: &Path| {
            // VACUUM INTO needs the target path as text. A path that is not
            // valid Unicode cannot be passed to SQLite safely.
            let tmp_str = tmp
                .to_str()
                .ok_or_else(|| GtraError::InvalidPath(tmp.to_path_buf()))?;
            self.conn.execute("VACUUM INTO ?1", [tmp_str])?;
            Ok(())
        })
    }

    /// Closes the database and reports any error that happens while closing.
    ///
    /// Dropping a `GtraFile` also closes it, but silently ignores errors;
    /// call this when you want to know.
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if SQLite cannot close cleanly.
    pub fn close(self) -> Result<(), GtraError> {
        // `close` gives back the connection together with the error on
        // failure; we only need the error.
        self.conn.close().map_err(|(_conn, e)| GtraError::Sqlite(e))
    }
}

/// Opens the `.gtra` file at `path` and reads the project from it.
///
/// # What it does
/// 1. Copies the file to a local working copy ([`WorkingCopy`]), reading the
///    user's file exactly once. The user's file is never opened by SQLite and
///    never changed, not even when it has an older format.
/// 2. Opens the copy with [`open_db`]: an older format is upgraded (in the
///    copy only); a newer format is opened read-only.
/// 3. Reads the project ([`GtraFile::read_project`]) and the file
///    information ([`GtraFile::file_info`]).
/// 4. Closes and deletes the working copy.
///
/// # Parameters
/// - `path`: the `.gtra` file to open, e.g. `/Users/sam/Projects/shed.gtra`.
///
/// # Returns
/// An [`OpenedProject`]: the project, the notes about anything that did not
/// fit the model, and whether the file is read-only. A file from a **newer**
/// Ganttora comes back with `access == Access::ReadOnly`: show it, but do not
/// save over it ([`save`] refuses with [`GtraError::NewerFormat`]).
///
/// # Errors
/// - [`GtraError::Io`] if the file does not exist or cannot be read.
/// - [`GtraError::NotAGtraFile`] if it is not a `.gtra` file.
/// - [`GtraError::InvalidData`] if its content is damaged in a way SQLite
///   cannot detect (see [`GtraFile::read_project`]).
/// - [`GtraError::Migration`] / [`GtraError::Sqlite`] for database errors.
///
/// # Example
/// See the crate-level example at the top of this page, and
/// `examples/save_open.rs`.
pub fn open(path: impl AsRef<Path>) -> Result<OpenedProject, GtraError> {
    let path = path.as_ref();
    // Created first so that it is dropped (and deleted) last, after the
    // database inside it has been closed.
    let working = WorkingCopy::in_temp_dir();
    LocalFileStorage::new(path).fetch_to(working.path())?;
    let file = open_db(working.path()).map_err(|e| report_original_path(e, path))?;

    let opened = describe_opened(&file)?;
    file.close()?;
    Ok(opened)
}

/// Reads the project and everything the app should know about an open file.
/// Shared by [`open`] and the [`ProjectSession`] constructors.
///
/// # Errors
/// See [`GtraFile::read_project`].
fn describe_opened(file: &GtraFile) -> Result<OpenedProject, GtraError> {
    let (project, notes) = file.read_project()?;
    Ok(OpenedProject {
        project,
        notes,
        access: file.access(),
        // For an upgraded copy, the version *on disk* is the old one.
        format_version: file.upgraded_from().unwrap_or(file.format_version()),
        upgraded_from: file.upgraded_from(),
        read_only_reason: compat::read_only_reason(file)?,
        info: file.file_info()?,
    })
}

/// Saves `project` to the `.gtra` file at `path`, atomically.
///
/// # What it does
/// 1. Checks the project ([`write::validate`]); nothing is touched if it is
///    invalid.
/// 2. Prepares a local working copy ([`WorkingCopy`]):
///    - if `path` holds a `.gtra` file of the **same project** (same
///      `project_id`), it is copied, so the data the model does not know yet
///      (resources, project notes, the change journal, …) is kept;
///    - otherwise (new file, or "Save As" over another project) a new, empty
///      database is created.
/// 3. Writes the project into the working copy ([`GtraFile::write_project`]),
///    with `options` for `meta.modified` and `meta.author` (and
///    `meta.created` for a new file).
/// 4. Writes a clean copy next to `path` and renames it into place
///    ([`atomic::atomic_write`]). Until that rename, `path` is unchanged.
/// 5. Deletes the working copy.
///
/// # Parameters
/// - `path`: where to save. The folder must exist; the file may or may not.
/// - `project`: the project to save.
/// - `options`: when and by whom it is saved ([`SaveOptions`]).
///
/// # Errors
/// - [`GtraError::InvalidProject`] if the project breaks a rule of the format.
/// - [`GtraError::NewerFormat`] if `path` holds a file from a newer Ganttora
///   (it is never overwritten).
/// - [`GtraError::NotAGtraFile`] if `path` holds some other kind of file.
/// - [`GtraError::Io`] / [`GtraError::Sqlite`] for file and database errors.
///
/// In every error case the file at `path` is unchanged.
///
/// # Example
/// ```
/// use chrono::{NaiveDate, TimeZone, Utc};
/// use model::Project;
///
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let path = dir.join("shed.gtra");
/// let project = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
///
/// let saved_at = Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap();
/// gtra::save(&path, &project, &gtra::SaveOptions::new(saved_at, "Sam Lee")).unwrap();
///
/// let opened = gtra::open(&path).unwrap();
/// assert_eq!(opened.info.modified.as_deref(), Some("2027-02-15T10:42:07Z"));
/// assert_eq!(opened.info.created.as_deref(), Some("2027-02-15T10:42:07Z"));
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn save(
    path: impl AsRef<Path>,
    project: &Project,
    options: &SaveOptions,
) -> Result<(), GtraError> {
    let path = path.as_ref();
    write::validate(project)?;

    let storage = LocalFileStorage::new(path);
    // Created before the database so that it is deleted after it is closed.
    let working = WorkingCopy::in_temp_dir();
    let (mut file, is_new) = prepare_working_file(&storage, working.path(), project)?;

    file.write_project(project, options)?;
    if is_new {
        set_meta(&file.conn, "created", &options.timestamp())?;
    }
    file.copy_to(&storage)?;
    file.close()
}

/// Step 2 of [`save`]: makes the working copy to write into.
///
/// # Returns
/// The open working copy, and `true` if it is a new, empty database (so
/// `meta.created` must be set), `false` if it is a copy of the existing file.
///
/// # Errors
/// [`GtraError::NewerFormat`] if the existing file is from a newer Ganttora;
/// [`GtraError::NotAGtraFile`] if it is not a `.gtra` file; file and
/// database errors otherwise.
fn prepare_working_file(
    storage: &LocalFileStorage,
    working: &Path,
    project: &Project,
) -> Result<(GtraFile, bool), GtraError> {
    if storage.exists()? {
        storage.fetch_to(working)?;
        let existing = open_db(working).map_err(|e| report_original_path(e, storage.path()))?;
        if existing.is_read_only() {
            return Err(GtraError::NewerFormat {
                found: existing.format_version(),
                supported: CURRENT_FORMAT_VERSION,
            });
        }
        if existing.meta("project_id")? == Some(project.id.to_string()) {
            return Ok((existing, false));
        }
        // Another project: start from an empty file instead of keeping its
        // journal, resources and other data.
        existing.close()?;
        fs::remove_file(working).map_err(|e| GtraError::io(working, e))?;
    }
    Ok((create_empty(working)?, true))
}

/// [`open_db`] runs on the working copy, so a "not a .gtra file" error would
/// name the temporary path. This puts the user's path back into it.
fn report_original_path(error: GtraError, original: &Path) -> GtraError {
    match error {
        GtraError::NotAGtraFile(_) => GtraError::NotAGtraFile(original.to_path_buf()),
        other => other,
    }
}

/// The first 16 bytes of every SQLite database file.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Checks, before a save replaces it, that the file at `path` may be
/// replaced: it does not exist, or it is a `.gtra` file this app can write.
///
/// # How
/// Reads only the first 100 bytes, SQLite's **file header**, without letting
/// SQLite open the user's file (it may be in a synced folder):
///
/// ```text
///   offset  size  meaning                         we check
///   ------  ----  ------------------------------  -----------------------------
///        0    16  "SQLite format 3\0"             is it a SQLite database?
///       60     4  user_version (big-endian)       format version: 0 = not ours,
///                                                 > CURRENT = newer Ganttora
///       68     4  application_id (big-endian)     = GTRA_APPLICATION_ID ("Gtra")
/// ```
///
/// # Errors
/// - [`GtraError::NotAGtraFile`] for any other kind of file (never replaced:
///   the user may have picked the wrong file by mistake).
/// - [`GtraError::NewerFormat`] for a file from a newer Ganttora (never
///   overwritten).
/// - [`GtraError::Io`] if the file exists but cannot be read.
///
/// # Beginner note
/// "Big-endian" means the most important byte comes first, the way we write
/// numbers on paper. `u32::from_be_bytes` turns four such bytes into a number.
fn check_replaceable(path: &Path) -> Result<(), GtraError> {
    use std::io::Read;

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        // Nothing there yet: a new file may be created.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(GtraError::io(path, e)),
    };
    let mut header = [0u8; 100];
    if let Err(e) = file.read_exact(&mut header) {
        // Shorter than 100 bytes: cannot be a database made by Ganttora.
        return Err(if e.kind() == io::ErrorKind::UnexpectedEof {
            GtraError::NotAGtraFile(path.to_path_buf())
        } else {
            GtraError::io(path, e)
        });
    }
    // `header[60..64]` is a slice of 4 bytes; `try_into` turns it into the
    // fixed-size array `[u8; 4]` that `from_be_bytes` needs. It cannot fail
    // here (the slice always has 4 bytes), hence the `unwrap_or` fallback.
    let user_version = u32::from_be_bytes(header[60..64].try_into().unwrap_or([0; 4]));
    let application_id = i32::from_be_bytes(header[68..72].try_into().unwrap_or([0; 4]));
    if &header[..16] != SQLITE_MAGIC || application_id != GTRA_APPLICATION_ID || user_version == 0 {
        return Err(GtraError::NotAGtraFile(path.to_path_buf()));
    }
    if user_version > CURRENT_FORMAT_VERSION {
        return Err(GtraError::NewerFormat {
            found: user_version,
            supported: CURRENT_FORMAT_VERSION,
        });
    }
    Ok(())
}

/// Creates a new, empty `.gtra` file at `path` and opens it.
///
/// # What it does
/// Builds the whole new file in a temporary file next to `path` (all tables
/// from the migrations, the `meta` rows, one `project` row), then renames it
/// into place atomically, then opens it with [`open_db`]. A crash halfway
/// leaves no file at all, never a half-made one.
///
/// The new file contains:
/// - `meta`: `format_version` (written by the migration runner,
///   [`schema::migrate`], not by this function), and from this function
///   `app_version`, `project_id` (new UUID), `created` and `modified` (now,
///   UTC), `author` (empty until Settings → User is wired up);
/// - `project`: one row with the same UUID and the file name without
///   `.gtra` as the project name.
///
/// # Parameters
/// - `path`: where to create the file. Its folder must exist; the file must
///   not.
///
/// # Returns
/// The open, writable [`GtraFile`].
///
/// # Errors
/// - [`GtraError::FileExists`] if something already exists at `path`.
/// - [`GtraError::Io`] / [`GtraError::Sqlite`] if the file cannot be created.
///
/// # Example
/// ```
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let file = gtra::create_empty(dir.join("Garden shed.gtra")).unwrap();
/// let name: String = file
///     .connection()
///     .query_row("SELECT name FROM project", [], |row| row.get(0))
///     .unwrap();
/// assert_eq!(name, "Garden shed");
/// // A second create at the same path is refused.
/// assert!(gtra::create_empty(dir.join("Garden shed.gtra")).is_err());
/// // Close before deleting: Windows refuses to delete an open file.
/// file.close().unwrap();
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn create_empty(path: impl AsRef<Path>) -> Result<GtraFile, GtraError> {
    let path = path.as_ref();
    if path.try_exists().map_err(|e| GtraError::io(path, e))? {
        return Err(GtraError::FileExists(path.to_path_buf()));
    }
    let project_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    LocalFileStorage::new(path).save_with(&mut |tmp: &Path| {
        let mut conn = Connection::open(tmp)?;
        configure_writable(&conn)?;
        schema::migrate(&mut conn)?;
        write_initial_rows(&mut conn, &project_name)?;
        // Close before the rename: on Windows an open file cannot be renamed.
        conn.close().map_err(|(_conn, e)| GtraError::Sqlite(e))
    })?;

    open_db(path)
}

/// Opens an existing `.gtra` file.
///
/// # What it does
/// 1. Opens the file **read-only** first and checks it: is it a SQLite
///    database, does it carry the `.gtra` application id, which format
///    version does it have?
/// 2. Format version newer than this app → keeps it open read-only
///    ([`Access::ReadOnly`]). Nothing in the file is touched.
/// 3. Same or older → reopens it read-write, switches on foreign-key checks
///    and runs any missing migrations ([`schema::migrate`]).
///
/// > **Warning:** this works directly on the file at `path`, and an older file
/// > is upgraded in place. Call it on the **local working copy**, not on a
/// > file in a network or synced folder (see [`storage`]).
///
/// # Parameters
/// - `path`: the `.gtra` file to open.
///
/// # Returns
/// The open [`GtraFile`]. Check [`GtraFile::is_read_only`] and
/// [`GtraFile::upgraded_from`] to tell the user what happened.
///
/// # Errors
/// - [`GtraError::Io`] (not found) if the file does not exist.
/// - [`GtraError::NotAGtraFile`] if it is not a `.gtra` file.
/// - [`GtraError::Migration`] if upgrading an older file fails (the file is
///   left unchanged).
/// - [`GtraError::Sqlite`] for other database errors (e.g. a damaged file).
///
/// # Example
/// ```
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let path = dir.join("shed.gtra");
/// gtra::create_empty(&path).unwrap().close().unwrap();
///
/// let file = gtra::open_db(&path).unwrap();
/// assert_eq!(file.access(), gtra::Access::ReadWrite);
///
/// // A text file renamed to .gtra is refused with a clear error.
/// std::fs::write(dir.join("fake.gtra"), "hello").unwrap();
/// assert!(matches!(gtra::open_db(dir.join("fake.gtra")), Err(gtra::GtraError::NotAGtraFile(_))));
/// // Close before deleting: Windows refuses to delete an open file.
/// file.close().unwrap();
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn open_db(path: impl AsRef<Path>) -> Result<GtraFile, GtraError> {
    let path = path.as_ref();
    // SQLite would report a missing file as a vague "unable to open"; we give
    // the operating system's clearer "not found" instead.
    if !path.try_exists().map_err(|e| GtraError::io(path, e))? {
        return Err(GtraError::io(
            path,
            io::Error::from(io::ErrorKind::NotFound),
        ));
    }

    // Step 1: inspect the file without any chance of changing it.
    let ro = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let file_version = inspect(&ro, path)?;

    match schema::check_version(file_version) {
        // Step 2: newer file → stay read-only.
        VersionStatus::Newer { found } => Ok(GtraFile {
            conn: ro,
            path: path.to_path_buf(),
            access: Access::ReadOnly,
            format_version: found,
            upgraded_from: None,
        }),
        // Step 3: current or older → reopen writable, upgrade if needed.
        VersionStatus::Current | VersionStatus::NeedsUpgrade { .. } => {
            drop(ro);
            // Without SQLITE_OPEN_CREATE, SQLite refuses to create a new file.
            let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            configure_writable(&conn)?;
            let before = schema::migrate(&mut conn)?;
            Ok(GtraFile {
                conn,
                path: path.to_path_buf(),
                access: Access::ReadWrite,
                format_version: CURRENT_FORMAT_VERSION,
                upgraded_from: (before < CURRENT_FORMAT_VERSION).then_some(before),
            })
        }
    }
}

/// Checks that `conn` is a `.gtra` database and returns its format version.
///
/// # Errors
/// [`GtraError::NotAGtraFile`] if the file is not a SQLite database, has
/// another program's application id, or has format version 0.
fn inspect(conn: &Connection, path: &Path) -> Result<u32, GtraError> {
    // SQLite only reads the file when the first query runs, so a non-database
    // file is detected here, as the error code "NotADatabase".
    let not_gtra = |e: GtraError| match e {
        GtraError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
            if f.code == ErrorCode::NotADatabase =>
        {
            GtraError::NotAGtraFile(path.to_path_buf())
        }
        other => other,
    };
    let app_id = schema::read_application_id(conn).map_err(not_gtra)?;
    let version = schema::read_format_version(conn).map_err(not_gtra)?;
    if app_id != GTRA_APPLICATION_ID || version == 0 {
        return Err(GtraError::NotAGtraFile(path.to_path_buf()));
    }
    Ok(version)
}

/// Settings applied to every writable connection.
///
/// - `foreign_keys = ON`: SQLite checks foreign keys only when asked, and the
///   setting is per connection, so it must be switched on every time
///   ("Foreign keys").
/// - `journal_mode = DELETE`: SQLite's classic mode, where all data stays in
///   the one `.gtra` file. The faster "WAL" mode keeps extra `-wal` and
///   `-shm` files next to it, which a user could forget to copy, and which
///   behave badly in synced folders.
///
/// # Errors
/// [`GtraError::Sqlite`] if a setting cannot be applied.
fn configure_writable(conn: &Connection) -> Result<(), GtraError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    // `journal_mode` answers with the new mode, so it is a query, not an update.
    let _mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))?;
    Ok(())
}

/// Writes the `meta` rows and the single `project` row of a new file, in one
/// transaction.
///
/// # Errors
/// [`GtraError::Sqlite`] if an insert fails.
fn write_initial_rows(conn: &mut Connection, project_name: &str) -> Result<(), GtraError> {
    let project_id = Uuid::new_v4().to_string();
    let now = now_utc();
    let tx = conn.transaction()?;
    for (key, value) in [
        ("app_version", APP_VERSION),
        ("project_id", project_id.as_str()),
        ("created", now.as_str()),
        ("modified", now.as_str()),
        ("author", ""),
    ] {
        set_meta(&tx, key, value)?;
    }
    tx.execute(
        "INSERT INTO project (id, name) VALUES (?1, ?2)",
        [project_id.as_str(), project_name],
    )?;
    tx.commit()?;
    Ok(())
}

/// Inserts or replaces one `meta` row.
///
/// # Errors
/// [`GtraError::Sqlite`] if the statement fails.
fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), GtraError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

/// The current moment in UTC as ISO 8601 text, e.g. `"2027-02-15T10:42:07Z"`,
/// the timestamp format used throughout the file (see `0001_init.sql`).
fn now_utc() -> String {
    chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
