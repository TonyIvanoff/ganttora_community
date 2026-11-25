//! `ProjectSession`: a project kept open in the app while it is edited.
//!
//! # What this file is for
//! [`crate::open`] and [`crate::save`] are one-shot calls: they copy the
//! file, work on the copy and delete it again. The desktop app needs more:
//! the project stays open for as long as its window is open, and Save,
//! Save As, autosave and the change journal all work on the same local
//! **working copy**. That is what [`ProjectSession`] does.
//!
//! # Where it fits
//! ```text
//!   backend/src-tauri (ipc/file.rs)      one session per open project
//!        │  open / create / recover, save, save_as, autosave, close
//!        ▼
//!   session.rs: ProjectSession   ◄── you are here
//!        │  uses lib.rs (open_db, create_empty, GtraFile, check_replaceable),
//!        │  journal.rs (append / read), recovery.rs (autosave copies)
//!        ▼
//!   working copy: <working dir>/ganttora-working-<uuid>.gtra
//! ```
//!
//! # Main ideas
//! - The **working copy** holds the whole database, including data the
//!   model cannot hold yet (resources, project notes, …) and the change
//!   journal. Every save writes the model on top of it and then copies the
//!   *whole* database, so nothing is lost, not even on Save As.
//! - The user's file is only ever replaced atomically (temp file + rename,
//!   see `atomic.rs`), never edited in place.
//!
//! This code lived in `lib.rs` until M1; it was moved here so that `lib.rs`
//! stays short. `lib.rs` re-exports it, so callers still write
//! `gtra::ProjectSession`.
//!

use std::fs;
use std::path::{Path, PathBuf};

use model::{ChangeRecord, Project};
use uuid::Uuid;

use crate::atomic::WorkingCopy;
use crate::journal::{JournalEntry, JournalFilter, JournalPage};
use crate::storage::{LocalFileStorage, Storage};
// Items from `lib.rs`. Private functions of the crate root (such as
// `describe_opened`) can be used here because `session` is a module *inside*
// the crate: Rust lets a module see the private items of its parents.
use crate::{
    Compatibility, GtraError, GtraFile, OpenedProject, RecoveryCopy, RecoveryStore, SaveOptions,
    check_replaceable, compat, create_empty, describe_opened, open_db, recovery,
    report_original_path, set_meta,
};

/// A project that is open in the app: its working copy stays open between
/// saves.
///
/// # What it is for
/// [`crate::open`] and [`crate::save`] are one-shot calls: they copy, work and delete the
/// working copy each time. A window with a project open needs more:
/// - **Save As must keep everything.** The file may contain data the model
///   cannot hold yet (resources, project notes, the change journal, …). The session
///   keeps the whole database in its working copy, so a save to *any* path
///   writes all of it, not only what the model knows.
/// - **Autosave** writes recovery copies ([`ProjectSession::autosave`]).
/// - **The journal** is collected in the working copy
///   ([`ProjectSession::append_journal`]) and saved with the project.
///
/// # Life cycle
/// ```text
///   open(path) / create(project) / recover(copy)
///        │
///        ▼
///   ┌────────────────────────── session ──────────────────────────┐
///   │ working copy: <working dir>/ganttora-working-<uuid>.gtra    │
///   │   update_working · append_journal · save · save_as · autosave│
///   └──────────────────────────────────────────────────────────────┘
///        │ close(Some(&recovery_store))
///        ▼
///   working copy deleted, recovery copy deleted
/// ```
///
/// If the app crashes, the working copy stays behind, and so does the last
/// recovery copy; [`RecoveryStore::list`] finds it at the next start.
///
/// # Who calls it
/// The Tauri shell (`backend/src-tauri/src/ipc/file.rs`) keeps one session
/// per open project, next to the `document::Document`, and passes
/// `document.project()` to `save`, `save_as` and `autosave`.
///
/// # Example
/// ```
/// use chrono::NaiveDate;
/// use model::{Duration, Project, Task};
///
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// let working_dir = dir.join("working");
/// std::fs::create_dir_all(&dir).unwrap();
///
/// // File > New, then edit, then Save As.
/// let mut project = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let options = gtra::SaveOptions::now("Sam Lee");
/// let mut session = gtra::ProjectSession::create(&project, &options, &working_dir).unwrap();
/// project.tasks.push(Task::new("Design the shed", Duration::from_days(2)));
/// session.save_as(dir.join("shed.gtra"), &project, &options).unwrap();
/// session.close(None).unwrap();
///
/// // Open it again in a new session.
/// let (session, opened) = gtra::ProjectSession::open(dir.join("shed.gtra"), &working_dir).unwrap();
/// assert_eq!(opened.project, project);
/// session.close(None).unwrap();
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
#[derive(Debug)]
pub struct ProjectSession {
    // Field order matters: Rust drops (cleans up) a struct's fields from top
    // to bottom. The database must be closed before its working-copy file is
    // deleted, so `file` comes before `working`.
    /// The open working copy.
    file: GtraFile,
    /// The working-copy file; deleted when the session ends.
    working: WorkingCopy,
    /// The user's file; `None` until a never-saved project is saved.
    target: Option<PathBuf>,
    /// This session's id; names its recovery copy.
    id: Uuid,
}

impl ProjectSession {
    /// Opens the user's file in a new session (File > Open).
    ///
    /// # What it does
    /// 1. Copies the file into a new working copy inside `working_dir`
    ///    (reading the user's file exactly once).
    /// 2. Opens the copy with [`open_db`]: an older format is upgraded **in
    ///    the copy only** (`upgraded_from` tells the UI); a newer format is
    ///    opened read-only (`read_only_reason` tells the UI why).
    /// 3. Reads the project.
    ///
    /// # Parameters
    /// - `path`: the user's `.gtra` file.
    /// - `working_dir`: a folder on a local disk for the working copy,
    ///   normally `<app data>/working`. Created if missing.
    ///
    /// # Returns
    /// The session and an [`OpenedProject`] (project, notes, access, versions).
    ///
    /// # Errors
    /// The same as [`crate::open`]. On error no working copy is left behind.
    pub fn open(
        path: impl AsRef<Path>,
        working_dir: impl AsRef<Path>,
    ) -> Result<(Self, OpenedProject), GtraError> {
        let path = path.as_ref();
        let working = new_working_copy(working_dir.as_ref())?;
        LocalFileStorage::new(path).fetch_to(working.path())?;
        let file = open_db(working.path()).map_err(|e| report_original_path(e, path))?;
        let opened = describe_opened(&file)?;
        let session = Self {
            file,
            working,
            target: Some(path.to_path_buf()),
            id: Uuid::new_v4(),
        };
        Ok((session, opened))
    }

    /// Starts a session for a new project that has no file yet (File > New).
    ///
    /// # Parameters
    /// - `project`: the new project (e.g. from `document::Document::new`).
    /// - `options`: time and author, for `meta.created` / `meta.modified`.
    /// - `working_dir`: as in [`ProjectSession::open`].
    ///
    /// # Returns
    /// The session; [`ProjectSession::target`] is `None` until the first
    /// [`ProjectSession::save_as`].
    ///
    /// # Errors
    /// [`GtraError::InvalidProject`] if the project breaks a rule of the
    /// format; file and database errors otherwise.
    pub fn create(
        project: &Project,
        options: &SaveOptions,
        working_dir: impl AsRef<Path>,
    ) -> Result<Self, GtraError> {
        let working = new_working_copy(working_dir.as_ref())?;
        let mut file = create_empty(working.path())?;
        file.write_project(project, options)?;
        set_meta(&file.conn, "created", &options.timestamp())?;
        Ok(Self {
            file,
            working,
            target: None,
            id: Uuid::new_v4(),
        })
    }

    /// Restores a recovery copy found by [`RecoveryStore::list`] into a new
    /// session.
    ///
    /// # What it does
    /// 1. Copies the recovery copy into a new working copy (the recovery copy
    ///    itself stays until the session is closed cleanly, so a second crash
    ///    loses nothing).
    /// 2. Opens it (upgrading it if this app is newer than the copy).
    /// 3. Removes the `recovery_*` rows from the working copy's `meta`, so
    ///    they never reach the user's file.
    /// 4. Keeps the copy's session id, so later autosaves replace the same
    ///    recovery copy instead of adding a second one.
    ///
    /// The restored project has unsaved changes: the UI should mark it dirty.
    /// If `copy.source` is `None`, the project was never saved and
    /// [`ProjectSession::save`] will ask for Save As ([`GtraError::NoSavePath`]).
    ///
    /// # Parameters
    /// - `copy`: an entry from [`RecoveryStore::list`].
    /// - `working_dir`: as in [`ProjectSession::open`].
    ///
    /// # Errors
    /// [`GtraError::NotAGtraFile`] if the copy is unreadable
    /// (`copy.readable == false`); otherwise as [`ProjectSession::open`].
    pub fn recover(
        copy: &RecoveryCopy,
        working_dir: impl AsRef<Path>,
    ) -> Result<(Self, OpenedProject), GtraError> {
        if !copy.readable {
            return Err(GtraError::NotAGtraFile(copy.path.clone()));
        }
        let working = new_working_copy(working_dir.as_ref())?;
        LocalFileStorage::new(&copy.path).fetch_to(working.path())?;
        let file = open_db(working.path()).map_err(|e| report_original_path(e, &copy.path))?;
        if !file.is_read_only() {
            file.conn.execute(
                "DELETE FROM meta WHERE key IN (?1, ?2, ?3)",
                [
                    recovery::META_SESSION_ID,
                    recovery::META_SOURCE,
                    recovery::META_SAVED_AT,
                ],
            )?;
        }
        let opened = describe_opened(&file)?;
        let session = Self {
            file,
            working,
            target: copy.source.clone(),
            id: copy.session_id,
        };
        Ok((session, opened))
    }

    /// This session's id (the name of its recovery copy).
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The user's file, or `None` for a project that was never saved.
    pub fn target(&self) -> Option<&Path> {
        self.target.as_deref()
    }

    /// The path of the working copy (for diagnostics).
    pub fn working_path(&self) -> &Path {
        self.working.path()
    }

    /// The open working copy, for reading (meta, journal, …).
    pub fn file(&self) -> &GtraFile {
        &self.file
    }

    /// `true` if the project cannot be saved over (a file from a newer
    /// Ganttora). Save As is still possible with
    /// [`ProjectSession::save_as_model_only`].
    pub fn is_read_only(&self) -> bool {
        self.file.is_read_only()
    }

    /// If the file had an older format and the working copy was upgraded,
    /// the old version; otherwise `None`.
    pub fn upgraded_from(&self) -> Option<u32> {
        self.file.upgraded_from()
    }

    /// The file's [`Compatibility`] with this app (current, upgraded or
    /// read-only with a reason code).
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if `meta` cannot be read.
    pub fn compatibility(&self) -> Result<Compatibility, GtraError> {
        compat::check(&self.file)
    }

    /// Writes the project into the working copy only. The user's file is not
    /// touched. [`ProjectSession::save`], `save_as` and `autosave` call this
    /// first; it is public for callers that want the working copy current at
    /// other moments.
    ///
    /// # Errors
    /// [`GtraError::ReadOnly`], [`GtraError::InvalidProject`] or database
    /// errors (see [`GtraFile::write_project`]).
    pub fn update_working(
        &mut self,
        project: &Project,
        options: &SaveOptions,
    ) -> Result<(), GtraError> {
        self.file.write_project(project, options)
    }

    /// Saves the project to the user's file (File > Save), atomically.
    ///
    /// # What it does
    /// 1. Checks that the target may be replaced (still a `.gtra` file, not
    ///    from a newer Ganttora; see the private `check_replaceable`).
    /// 2. Writes the project into the working copy.
    /// 3. Writes a clean copy of the **whole** working database next to the
    ///    target and renames it into place.
    ///
    /// # Errors
    /// - [`GtraError::NoSavePath`] if the project was never saved: use
    ///   [`ProjectSession::save_as`].
    /// - [`GtraError::ReadOnly`] for a read-only session.
    /// - [`GtraError::NewerFormat`] / [`GtraError::NotAGtraFile`] if the
    ///   target was replaced meanwhile by a newer or a foreign file.
    /// - File and database errors. The user's file is unchanged on any error.
    pub fn save(&mut self, project: &Project, options: &SaveOptions) -> Result<(), GtraError> {
        let target = self.target.clone().ok_or(GtraError::NoSavePath)?;
        self.save_whole_database(&target, project, options)
    }

    /// Saves the project to a new path (File > Save As), keeping **all**
    /// data in the file, also data the model cannot hold yet.
    ///
    /// # What it does
    /// The result is "the current working database copied to `path`, with
    /// the model written on top": the model is written into the working copy,
    /// then the whole database is written to a temporary file next to `path`
    /// and renamed into place (atomic). Afterwards the session's target is
    /// `path`, so the next [`ProjectSession::save`] goes there. The project
    /// keeps its id (like Save As in MS Project).
    ///
    /// # Parameters
    /// - `path`: the new file; its folder must exist.
    /// - `project`, `options`: as in [`ProjectSession::save`].
    ///
    /// # Errors
    /// - [`GtraError::ReadOnly`] for a read-only session (its database comes
    ///   from a newer Ganttora and cannot be written by this app). Ask the
    ///   user, then use [`ProjectSession::save_as_model_only`].
    /// - [`GtraError::NewerFormat`] / [`GtraError::NotAGtraFile`] if `path`
    ///   holds a file this app must not replace.
    /// - File and database errors. On any error nothing at `path` changes and
    ///   the target stays as it was.
    pub fn save_as(
        &mut self,
        path: impl AsRef<Path>,
        project: &Project,
        options: &SaveOptions,
    ) -> Result<(), GtraError> {
        let path = path.as_ref();
        self.save_whole_database(path, project, options)?;
        self.target = Some(path.to_path_buf());
        Ok(())
    }

    /// Saves only what the model holds into a brand-new file at `path`, and
    /// continues the session with that file.
    ///
    /// # When to use it
    /// Only after warning the user that data will be left out. The main case
    /// is a read-only file from a newer Ganttora: this app cannot write its
    /// database, but the user may want to keep working on what this app
    /// understands. Everything the model cannot hold (and the journal) is
    /// **not** in the new file.
    ///
    /// # Errors
    /// [`GtraError::InvalidProject`], [`GtraError::NewerFormat`] /
    /// [`GtraError::NotAGtraFile`] for a target that must not be replaced,
    /// and file or database errors. On any error the session is unchanged.
    pub fn save_as_model_only(
        &mut self,
        path: impl AsRef<Path>,
        project: &Project,
        options: &SaveOptions,
    ) -> Result<(), GtraError> {
        let path = path.as_ref();
        check_replaceable(path)?;
        let dir = self
            .working
            .path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let working = new_working_copy(&dir)?;
        let mut file = create_empty(working.path())?;
        file.write_project(project, options)?;
        set_meta(&file.conn, "created", &options.timestamp())?;
        file.copy_to(&LocalFileStorage::new(path))?;

        // Swap in the new working copy. `std::mem::replace` puts the new
        // value in and hands back the old one, which we then drop: first the
        // old database (closing it), then its file (deleting it).
        let old_file = std::mem::replace(&mut self.file, file);
        let old_working = std::mem::replace(&mut self.working, working);
        drop(old_file);
        drop(old_working);
        self.target = Some(path.to_path_buf());
        Ok(())
    }

    /// Writes a recovery copy (autosave), atomically.
    ///
    /// # What it does
    /// Writes the project into the working copy, then asks `store` to write a
    /// copy of the whole database to `<recovery dir>/<session id>.gtra`. The
    /// user's file is **not** touched (autosave never saves over the user's
    /// file; that is the user's decision).
    ///
    /// # Parameters
    /// - `project`, `options`: the current project; `options.saved_at` is
    ///   recorded as the copy's time.
    /// - `store`: the recovery folder ([`RecoveryStore`]).
    ///
    /// # Returns
    /// The [`RecoveryCopy`] written.
    ///
    /// # Errors
    /// [`GtraError::ReadOnly`] for a read-only session (nothing can be saved,
    /// so nothing is recovered); otherwise database and file errors. The
    /// previous recovery copy stays intact on error.
    pub fn autosave(
        &mut self,
        project: &Project,
        options: &SaveOptions,
        store: &RecoveryStore,
    ) -> Result<RecoveryCopy, GtraError> {
        self.update_working(project, options)?;
        store.write(
            &self.file,
            self.id,
            self.target.as_deref(),
            &options.timestamp(),
        )
    }

    /// Adds change records to the working copy's journal. They reach the
    /// user's file with the next save. See [`GtraFile::append_journal`].
    ///
    /// # Parameters
    /// - `records`: the change records made by the `document` crate for one
    ///   or more user actions.
    ///
    /// # Returns
    /// The number the file gave each record, in the same order.
    ///
    /// # Errors
    /// As [`GtraFile::append_journal`].
    pub fn append_journal(&mut self, records: &[ChangeRecord]) -> Result<Vec<i64>, GtraError> {
        self.file.append_journal(records)
    }

    /// Reads change records from the working copy. See
    /// [`GtraFile::read_journal`].
    ///
    /// # Returns
    /// The matching records, oldest first, each with its number in the file.
    ///
    /// # Errors
    /// As [`GtraFile::read_journal`].
    pub fn read_journal(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>, GtraError> {
        self.file.read_journal(filter)
    }

    /// Reads change records from the working copy and says how many rows
    /// were skipped because a newer Ganttora wrote them about a kind of item
    /// this version does not know. See [`GtraFile::read_journal_report`].
    ///
    /// # Returns
    /// A [`JournalPage`].
    ///
    /// # Errors
    /// As [`GtraFile::read_journal`].
    pub fn read_journal_report(&self, filter: &JournalFilter) -> Result<JournalPage, GtraError> {
        self.file.read_journal_report(filter)
    }

    /// Ends the session: closes and deletes the working copy, and, if a
    /// recovery store is given, deletes this session's recovery copy.
    ///
    /// # When to pass the store
    /// Pass `Some(&store)` on a **clean** close: the project was saved, or
    /// the user chose "Don't save". Pass `None` to keep the recovery copy
    /// (for example when quitting with unsaved changes the user wants back).
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if the database cannot be closed cleanly (the
    /// working copy is still deleted, the recovery copy is kept);
    /// [`GtraError::Io`] if the recovery copy cannot be deleted.
    pub fn close(self, recovery: Option<&RecoveryStore>) -> Result<(), GtraError> {
        // "Destructuring": take the struct apart into its fields, so each can
        // be closed or dropped in the right order.
        let ProjectSession {
            file, working, id, ..
        } = self;
        file.close()?;
        drop(working);
        if let Some(store) = recovery {
            store.discard(id)?;
        }
        Ok(())
    }

    /// Shared by `save` and `save_as`: model on top, then the whole database
    /// to `path`, atomically.
    fn save_whole_database(
        &mut self,
        path: &Path,
        project: &Project,
        options: &SaveOptions,
    ) -> Result<(), GtraError> {
        if self.file.is_read_only() {
            return Err(GtraError::ReadOnly);
        }
        check_replaceable(path)?;
        self.file.write_project(project, options)?;
        self.file.copy_to(&LocalFileStorage::new(path))
    }
}

/// Makes sure `dir` exists and chooses a new working-copy path inside it.
///
/// # Errors
/// [`GtraError::Io`] if the folder cannot be created.
fn new_working_copy(dir: &Path) -> Result<WorkingCopy, GtraError> {
    fs::create_dir_all(dir).map_err(|e| GtraError::io(dir, e))?;
    Ok(WorkingCopy::in_dir(dir))
}
