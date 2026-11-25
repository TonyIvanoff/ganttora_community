//! ipc/file.rs: the IPC commands for new, open, save, "save as" and autosave.
//!
//! # What it is for
//! These commands connect the open project (in [`AppState`]) with `.gtra`
//! files on disk, through a `gtra::ProjectSession`. They answer with the
//! same [`ProjectResponse`] as the project commands; its `file` part
//! ([`FileStatus`]) tells the UI the file name, whether the file is
//! read-only (and why), whether it was upgraded from an older format and
//! whether there are unsaved changes.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `file_new`      | replaces the project with a new, empty one (no file yet) |
//! | `file_open`     | opens a `.gtra` file and makes it the open project |
//! | `file_save`     | saves to the path the project came from / was last saved to |
//! | `file_save_as`  | saves to a new path, which becomes the project's path |
//! | `file_autosave` | writes a recovery copy if there are unsaved changes |
//!
//! # Where it fits
//! ```text
//!   UI: File buttons ─► system dialog (plugin-dialog, in the UI) ─► path
//!       └─invoke("file_open", { path })─► file_open ─► ProjectSession::open ─► Document
//!       └─invoke("file_save_as", { path })─► file_save_as ─► ProjectSession::save_as
//!   UI: autosave timer (features/files/useAutosave.ts)
//!       └─invoke("file_autosave")─► ProjectSession::autosave ─► <app data>/recovery
//! ```
//!
//! # Main ideas
//! - **The session and its working copy.** Opening a file copies it into
//!   `<app data>/working` and keeps that *working copy* open while the
//!   project is open. Saving writes the model into the working copy and
//!   then copies the **whole** database to the user's file, atomically
//!   (temporary file + rename). So data this version cannot show yet
//!   (resources, the journal…) is never lost by a save.
//! - **Who picks the path?** The UI. It shows the operating system's
//!   "Open" / "Save As" window with the Tauri dialog plugin
//!   (`frontend/features/files/dialogs.ts`) and sends us only the chosen
//!   path. The commands below then simply take a path, so tests can call
//!   them without any window or dialog.
//! - **The dirty flag.** "Dirty" means "changed since it was last opened or
//!   saved". The project commands set it after every successful edit, undo
//!   and redo (`ipc/project.rs`); a successful save clears it.
//! - **The change journal.** The document collects one change record per
//!   changed field (who, when, old and new value). On every save and
//!   autosave the records collected since the last one are appended to the
//!   working copy's journal ([`flush_journal`]); the next save carries them
//!   into the user's file.
//! - **Read-only files.** A file from a newer Ganttora opens read-only.
//!   `file_save` then refuses (`gtra.read_only`). `file_save_as` writes a
//!   brand-new file with only what this version understands (the UI warns
//!   first) and continues with that file, read-write.
//! - **Autosave never touches the user's file.** It writes a recovery copy
//!   to `<app data>/recovery`; a clean close deletes it. After a crash the
//!   next start offers it (`ipc/recovery.rs`).
//! - **Notes after opening.** `gtra` reports things in the file that did not
//!   fit the model ([`gtra::ReadNote`]). We pass them on as plain data
//!   ([`ReadNoteDto`]); the UI turns each kind into a translated message.
//!   They are sent once, in the answer of the command that read the file:
//!   `file_open`, or `recovery_restore` (`ipc/recovery.rs`) when a recovery
//!   copy is restored. All other answers carry an empty list.
//! - **Thin wrappers, testable core** (as in `ipc/project.rs`): each
//!   `#[tauri::command]` only finds the window label and calls a plain
//!   function (`new_project`, `open_file`, `save`, `save_as`, `autosave`).
//!

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDateTime};
use document::Document;
use gtra::{Access, ProjectSession, ReadNote, ReadOnlyReason, SaveOptions};
use model::Project;
use serde::Serialize;
use tauri::{State, Window};

use crate::ipc::IpcError;
use crate::ipc::project::{ProjectResponse, author_name};
use crate::settings::SettingsState;
use crate::state::{AppState, FileState, OpenDocument};

/// Name of a new project when the UI does not send one. The UI normally
/// sends the translated text of the key `file.untitledName`; this English
/// fallback is only used if that text is missing or empty.
pub const DEFAULT_PROJECT_NAME: &str = "Untitled";

/// The file extension of Ganttora project files, without the dot.
pub const GTRA_EXTENSION: &str = "gtra";

/// A notice for the UI: a stable code plus the values for its message.
///
/// # JSON shape
/// ```json
/// { "code": "gtra.newer_format",
///   "params": { "found": "3", "supported": "2", "writtenBy": "0.4.0" } }
/// ```
/// The UI shows `error.<code>` from `locales/en.json`, filling `{found}`,
/// `{supported}`… with `params`. Must match `NoticeDto` in
/// `frontend/ipc/types.ts`.
///
/// # Beginner note
/// A `BTreeMap` is a lookup table (like `HashMap`) that keeps its keys
/// sorted, so the JSON always lists them in the same order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeDto {
    /// The stable code, e.g. `"gtra.newer_format"`.
    pub code: String,
    /// Values for the message's placeholders, all as text.
    pub params: BTreeMap<String, String>,
}

impl From<&ReadOnlyReason> for NoticeDto {
    /// Describes why a file is read-only, using `gtra`'s own code and
    /// parameters.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::ipc::file::NoticeDto;
    /// let reason = gtra::ReadOnlyReason::NewerFormat { found: 3, supported: 2, written_by: None };
    /// let notice = NoticeDto::from(&reason);
    /// assert_eq!(notice.code, "gtra.newer_format");
    /// assert_eq!(notice.params["found"], "3");
    /// ```
    fn from(reason: &ReadOnlyReason) -> Self {
        NoticeDto {
            code: reason.code().to_string(),
            params: reason
                .params()
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        }
    }
}

/// The `file` part of every [`ProjectResponse`]: facts about the file
/// behind the open project.
///
/// # JSON shape (as the UI receives it)
/// ```json
/// {
///   "path": "/Users/sam/Projects/shed.gtra",
///   "fileName": "shed.gtra",
///   "readOnly": false,
///   "dirty": true,
///   "notes": [],
///   "upgradedFrom": null,
///   "readOnlyReason": null
/// }
/// ```
/// Must match the TypeScript type `FileStatus` in `frontend/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    /// Full path of the file, or `null` for a project never saved.
    pub path: Option<String>,
    /// Just the file name (last part of the path), e.g. `"shed.gtra"`, or
    /// `null` for a project never saved. The UI shows it in the title.
    pub file_name: Option<String>,
    /// `true` if the file must not be saved over (from a newer Ganttora).
    pub read_only: bool,
    /// `true` if there are unsaved changes.
    pub dirty: bool,
    /// Things in the file that did not fit the model. Filled only in the
    /// answers of `file_open` and `recovery_restore`; empty otherwise.
    pub notes: Vec<ReadNoteDto>,
    /// The older format version the file was upgraded from, or `null`.
    pub upgraded_from: Option<u32>,
    /// Why the file is read-only, or `null`.
    pub read_only_reason: Option<NoticeDto>,
}

impl FileStatus {
    /// Describes a [`FileState`] for the UI (with no notes).
    ///
    /// # Parameters
    /// - `file`: the shell's file facts (borrowed).
    ///
    /// # Returns
    /// A new [`FileStatus`] with an empty `notes` list.
    ///
    /// # Example
    /// ```
    /// use std::path::PathBuf;
    /// use ganttora_lib::{ipc::file::FileStatus, state::FileState};
    /// let file = FileState {
    ///     path: Some(PathBuf::from("/tmp/shed.gtra")),
    ///     dirty: true,
    ///     upgraded_from: Some(1),
    ///     ..FileState::default()
    /// };
    /// let status = FileStatus::from_state(&file);
    /// assert_eq!(status.file_name.as_deref(), Some("shed.gtra"));
    /// assert!(status.dirty);
    /// assert_eq!(status.upgraded_from, Some(1));
    /// ```
    ///
    /// # Beginner note
    /// A path is not always valid text (on Linux a file name may contain
    /// bytes that are not UTF-8). `to_string_lossy()` turns it into text and
    /// replaces any such bytes with "�", which is fine for showing it.
    pub fn from_state(file: &FileState) -> Self {
        FileStatus {
            path: file.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            file_name: file
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|name| name.to_string_lossy().into_owned()),
            read_only: file.read_only,
            dirty: file.dirty,
            notes: Vec::new(),
            upgraded_from: file.upgraded_from,
            read_only_reason: file.read_only_reason.as_ref().map(NoticeDto::from),
        }
    }
}

/// One [`gtra::ReadNote`] as plain JSON data for the UI.
///
/// `gtra::ReadNote` cannot be turned into JSON itself (the `gtra` crate
/// does not depend on serde), so this mirror type exists. The UI picks the
/// translated message by `kind` (keys `file.note.<kind>` in
/// `locales/en.json`) and fills in the other fields.
///
/// # JSON shape
/// The variant name goes into a `"kind"` field, the other fields sit next
/// to it, all in camelCase:
/// ```json
/// { "kind": "durationMissing", "taskId": "3f2b8c1e-…" }
/// { "kind": "rowsNotLoaded", "table": "resources", "count": 3 }
/// ```
/// Must match the TypeScript type `ReadNoteDto` in `frontend/ipc/types.ts`.
///
/// # Beginner note
/// `#[serde(tag = "kind")]` is serde's *internally tagged* enum format.
/// `rename_all` renames the variants (`DurationMissing` → `durationMissing`)
/// and `rename_all_fields` renames the fields inside them (`task_id` →
/// `taskId`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReadNoteDto {
    /// The project had no start date; `used` was used instead.
    StartDateMissing {
        /// The start that was used, e.g. `"2027-03-01T00:00:00"`.
        used: NaiveDateTime,
    },
    /// The project had no project calendar; `calendar_id` is now used.
    CalendarMissing {
        /// ID of the calendar that became the project calendar.
        calendar_id: String,
    },
    /// A task had no duration; 0 (a milestone) was used.
    DurationMissing {
        /// ID of the task (the UI can look up its name in the snapshot).
        task_id: String,
    },
    /// A calendar based on another one was turned into a complete calendar.
    BaseCalendarFlattened {
        /// ID of the calendar.
        calendar_id: String,
        /// ID of its base calendar, as stored in the file.
        base_id: String,
    },
    /// A stored value the model cannot hold yet; it stays in the file.
    ValueNotLoaded {
        /// The table, e.g. `"tasks"`.
        table: String,
        /// The row's ID.
        row_id: String,
        /// The column, e.g. `"constraint_type"`.
        column: String,
        /// The stored value, as text.
        value: String,
    },
    /// A whole table the model does not have yet; its rows stay in the file.
    RowsNotLoaded {
        /// The table, e.g. `"resources"`.
        table: String,
        /// How many rows it has.
        count: u64,
    },
}

impl ReadNoteDto {
    /// Converts a `gtra` note into its JSON mirror.
    ///
    /// # Returns
    /// `Some(dto)`, or `None` for a note the UI does not show. Today that is
    /// only `PercentLagConverted`, which `gtra` no longer produces (since
    /// format 2 it stores the percent lag itself); the variant still exists
    /// in `gtra`, so it must still be matched here.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::ipc::file::ReadNoteDto;
    /// let note = gtra::ReadNote::RowsNotLoaded { table: "resources", count: 3 };
    /// let json = serde_json::to_value(ReadNoteDto::from_note(&note).unwrap()).unwrap();
    /// assert_eq!(json["kind"], "rowsNotLoaded");
    /// assert_eq!(json["count"], 3);
    /// ```
    ///
    /// # Beginner note
    /// A `match` must handle every variant, so if the `gtra` crate adds a
    /// new kind of note, this code stops compiling until we handle it: the
    /// compiler reminds us instead of a bug report.
    pub fn from_note(note: &ReadNote) -> Option<Self> {
        let dto = match note {
            ReadNote::StartDateMissing { used } => ReadNoteDto::StartDateMissing { used: *used },
            ReadNote::CalendarMissing { used } => ReadNoteDto::CalendarMissing {
                calendar_id: used.to_string(),
            },
            ReadNote::DurationMissing { task } => ReadNoteDto::DurationMissing {
                task_id: task.to_string(),
            },
            // No longer produced by gtra (Lead decision): nothing to show.
            ReadNote::PercentLagConverted { .. } => return None,
            ReadNote::BaseCalendarFlattened { calendar, base } => {
                ReadNoteDto::BaseCalendarFlattened {
                    calendar_id: calendar.to_string(),
                    base_id: base.clone(),
                }
            }
            ReadNote::ValueNotLoaded {
                table,
                id,
                column,
                value,
            } => ReadNoteDto::ValueNotLoaded {
                table: (*table).to_string(),
                row_id: id.clone(),
                column: (*column).to_string(),
                value: value.clone(),
            },
            ReadNote::RowsNotLoaded { table, count } => ReadNoteDto::RowsNotLoaded {
                table: (*table).to_string(),
                count: *count,
            },
        };
        Some(dto)
    }

    /// Converts a list of notes, leaving out those the UI does not show.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::ipc::file::ReadNoteDto;
    /// assert!(ReadNoteDto::from_notes(&[]).is_empty());
    /// ```
    pub fn from_notes(notes: &[ReadNote]) -> Vec<Self> {
        notes.iter().filter_map(ReadNoteDto::from_note).collect()
    }
}

/// Builds the save options for every write: time, author and the version
/// of the scheduling engine (`engine::ENGINE_VERSION`, stored as
/// `meta.engine_version`, so a file tells which engine calculated it).
///
/// # Example
/// ```
/// let options = ganttora_lib::ipc::file::save_options("Sam");
/// assert_eq!(options.author, "Sam");
/// assert!(options.engine_version.is_some());
/// ```
pub fn save_options(author: &str) -> SaveOptions {
    SaveOptions::now(author).with_engine_version(engine::ENGINE_VERSION)
}

/// Makes sure a path ends in `.gtra`.
///
/// # What it does
/// The system "Save As" window usually adds the extension from its filter,
/// but not on every system (some Linux file pickers do not). If the
/// extension is missing or different, `.gtra` is appended.
///
/// # Parameters
/// - `path`: the path the user chose.
///
/// # Returns
/// The same path if it already ends in `.gtra` (any letter case), else the
/// path with `.gtra` added.
///
/// # Example
/// ```
/// use std::path::Path;
/// use ganttora_lib::ipc::file::with_gtra_extension;
/// assert_eq!(with_gtra_extension(Path::new("/p/shed")), Path::new("/p/shed.gtra"));
/// assert_eq!(with_gtra_extension(Path::new("/p/shed.GTRA")), Path::new("/p/shed.GTRA"));
/// assert_eq!(with_gtra_extension(Path::new("/p/v1.2")), Path::new("/p/v1.2.gtra"));
/// ```
pub fn with_gtra_extension(path: &Path) -> PathBuf {
    let has_extension = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(GTRA_EXTENSION));
    if has_extension {
        path.to_path_buf()
    } else {
        // Append rather than `set_extension`, so "v1.2" becomes "v1.2.gtra"
        // instead of losing the ".2".
        let mut text = path.as_os_str().to_owned();
        text.push(".");
        text.push(GTRA_EXTENSION);
        PathBuf::from(text)
    }
}

/// Moves the change records collected since the last save into the
/// working copy's journal.
///
/// # What it does
/// Appends `document.pending_changes()` (one `ChangeRecord` per changed
/// field: who, when, old and new value) to the session's working copy and, only if that worked, removes
/// them from the document (so a failed write loses nothing and a later
/// save tries again).
///
/// # Errors
/// The `gtra` error if appending fails. Nothing happens for a document
/// without a session or with a read-only session (nothing can be written).
pub fn flush_journal(open: &mut OpenDocument) -> Result<(), IpcError> {
    let Some(session) = open.session.as_mut() else {
        return Ok(());
    };
    if session.is_read_only() || open.document.pending_changes().is_empty() {
        return Ok(());
    }
    session.append_journal(open.document.pending_changes())?;
    // The records are safely in the working copy now.
    open.document.take_pending_changes();
    Ok(())
}

/// Makes sure the document has a session, creating one if it has none.
///
/// # Parameters
/// - `open`: the document.
/// - `working_dir`: where a new working copy goes ([`AppState::working_dir`]).
/// - `options`: time and author for a new working copy.
///
/// # Errors
/// The `gtra` error if a new working copy cannot be made.
fn ensure_session(
    open: &mut OpenDocument,
    working_dir: &Path,
    options: &SaveOptions,
) -> Result<(), IpcError> {
    if open.session.is_none() {
        let session = ProjectSession::create(open.document.project(), options, working_dir)?;
        open.session = Some(session);
    }
    Ok(())
}

/// Text of the `expect` below: [`ensure_session`] has always run first.
const SESSION_MISSING: &str = "ensure_session runs before every use of the session";

// ---------------------------------------------------------------------------
// Plain functions: the real work, callable from tests without a window.
// ---------------------------------------------------------------------------

/// Opens `document` under `key` as a project that has no file yet, with a
/// fresh session (used by File > New and the sample project).
///
/// # Parameters
/// - `state`, `key`: the shell's state and the window label.
/// - `document`: the new document.
/// - `author`: written into the working copy's metadata.
///
/// # Returns
/// The new project's [`ProjectResponse`].
///
/// # Errors
/// The `gtra` error if the working copy cannot be made; the project open
/// before then stays open.
pub fn open_untitled(
    state: &AppState,
    key: &str,
    document: Document,
    author: &str,
) -> Result<ProjectResponse, IpcError> {
    let session = ProjectSession::create(
        document.project(),
        &save_options(author),
        state.working_dir(),
    )?;
    let open = OpenDocument {
        document,
        file: FileState::default(),
        session: Some(session),
    };
    // Build the answer *before* handing the document to the state, because
    // `state.open` takes ownership of it.
    let response = ProjectResponse::from_open(&open);
    state.open(key, open)?;
    Ok(response)
}

/// Replaces the project under `key` with a new, empty project.
///
/// # Parameters
/// - `state`: the shell's shared state.
/// - `key`: the window label, e.g. `"main"`.
/// - `name`: the project name (the UI sends the translated "Untitled").
///   `None` or blank text gives [`DEFAULT_PROJECT_NAME`].
/// - `author`: as in [`open_untitled`].
///
/// # Returns
/// The new project's [`ProjectResponse`]: no tasks, no file, not dirty.
/// The project starts today (the shell may read the clock; the core may not).
///
/// # Errors
/// As [`open_untitled`].
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::file, state::AppState};
/// let state = AppState::default();
/// let response = file::new_project(&state, "main", Some("Kitchen"), "Sam").unwrap();
/// assert_eq!(response.snapshot.project_name, "Kitchen");
/// assert_eq!(response.snapshot.tasks.len(), 1); // the project row
/// assert!(response.snapshot.tasks[0].is_project);
/// assert_eq!(response.file.path, None);
/// assert!(!response.file.dirty);
///
/// let unnamed = file::new_project(&state, "main", Some("  "), "Sam").unwrap();
/// assert_eq!(unnamed.snapshot.project_name, file::DEFAULT_PROJECT_NAME);
/// ```
pub fn new_project(
    state: &AppState,
    key: &str,
    name: Option<&str>,
    author: &str,
) -> Result<ProjectResponse, IpcError> {
    let name = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(DEFAULT_PROJECT_NAME);
    // "Today" in the user's own time zone (Local, not Utc): someone in
    // Vilnius starting a project at 01:00 must get today, not yesterday.
    let mut project = Project::new(name, Local::now().date_naive());
    // Every project shows itself as the first row.
    project.ensure_project_row();
    open_untitled(state, key, Document::new(project), author)
}

/// Opens the `.gtra` file at `path` as the project under `key`.
///
/// # Parameters
/// - `state`, `key`: as in [`new_project`].
/// - `path`: the file to open.
///
/// # Returns
/// The opened project's [`ProjectResponse`], with `file.path` set, `dirty`
/// false, `readOnly` (and `readOnlyReason`) for a file from a newer
/// Ganttora, `upgradedFrom` for a file from an older one, and the notes from
/// reading in `file.notes`.
///
/// # Errors
/// The `gtra` error (`gtra.file_not_found`, `gtra.not_a_gtra_file`,
/// `gtra.invalid_data`…) if the file cannot be opened. The project open
/// before stays open and unchanged.
///
/// # Example
/// See [`save_as`] for a full round trip. Opening a missing file:
/// ```
/// use ganttora_lib::{ipc::file, state::AppState};
/// let state = AppState::default();
/// let err = file::open_file(&state, "main", std::path::Path::new("/no/such/shed.gtra")).unwrap_err();
/// assert_eq!(err.code, "gtra.file_not_found");
/// ```
pub fn open_file(state: &AppState, key: &str, path: &Path) -> Result<ProjectResponse, IpcError> {
    let (session, mut opened) = ProjectSession::open(path, state.working_dir())?;
    // A file from before project rows existed gets one (saved with the
    // user's next save).
    opened.project.ensure_project_row();
    let open = OpenDocument {
        document: Document::new(opened.project),
        file: FileState {
            path: Some(path.to_path_buf()),
            read_only: opened.access == Access::ReadOnly,
            dirty: false,
            upgraded_from: opened.upgraded_from,
            read_only_reason: opened.read_only_reason,
        },
        session: Some(session),
    };
    let mut response = ProjectResponse::from_open(&open);
    response.file.notes = ReadNoteDto::from_notes(&opened.notes);
    state.open(key, open)?;
    Ok(response)
}

/// Saves the project under `key` to the path it was opened from or last
/// saved to ("Save").
///
/// # Parameters
/// - `state`, `key`: as in [`new_project`].
/// - `author`: the name stored as the file's last author (see
///   `ipc::project::author_name`).
///
/// # Returns
/// The [`ProjectResponse`] after saving: `file.dirty` is false.
///
/// # Errors
/// - `gtra.no_save_path` if the project was never saved (use [`save_as`]).
/// - `gtra.read_only` if the file is read-only (use [`save_as`]).
/// - Another `gtra.*` code if writing fails; the file on disk is then
///   unchanged (atomic save) and the project stays dirty.
///
/// # Example
/// Saving a project that has no file yet is refused:
/// ```
/// use ganttora_lib::{ipc::{file, project}, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// assert_eq!(file::save(&state, "main", "Sam").unwrap_err().code, "gtra.no_save_path");
/// ```
///
/// Saving over a read-only file is refused, and the file is not touched.
/// (A real read-only file comes from a newer Ganttora; here we mark the
/// state by hand to test the rule.)
/// ```
/// use ganttora_lib::{ipc::{file, project}, state::AppState};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let path = dir.join("shed.gtra");
///
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// file::save_as(&state, "main", &path, "Sam").unwrap();
/// let before = std::fs::read(&path).unwrap();
///
/// state.with_open("main", |open| open.file.read_only = true).unwrap();
/// assert_eq!(file::save(&state, "main", "Sam").unwrap_err().code, "gtra.read_only");
/// assert_eq!(std::fs::read(&path).unwrap(), before); // unchanged
///
/// // "Save As" to another path still works and clears the read-only flag.
/// let copy = file::save_as(&state, "main", &dir.join("copy.gtra"), "Sam").unwrap();
/// assert!(!copy.file.read_only);
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn save(state: &AppState, key: &str, author: &str) -> Result<ProjectResponse, IpcError> {
    let working_dir = state.working_dir();
    state.with_open(key, |open| {
        if open.file.read_only {
            return Err(IpcError::from(gtra::GtraError::ReadOnly));
        }
        let path = open.file.path.clone().ok_or(gtra::GtraError::NoSavePath)?;
        let options = save_options(author);
        ensure_session(open, &working_dir, &options)?;
        // The journal goes into the working copy first, so the save below
        // carries it into the user's file.
        flush_journal(open)?;
        // Beginner note: `session` borrows the `session` field and
        // `open.document` is a different field, so Rust lets us use both
        // at the same time ("disjoint borrows").
        let session = open.session.as_mut().expect(SESSION_MISSING);
        // A session made on the spot (see `ensure_session`) does not know
        // the file yet, so it needs "save as" to the same path once.
        if session.target().is_some() {
            session.save(open.document.project(), &options)?;
        } else {
            session.save_as(&path, open.document.project(), &options)?;
        }
        open.file.dirty = false;
        Ok(ProjectResponse::from_open(open))
    })?
}

/// Saves the project under `key` to a new `path` ("Save As"). From then on
/// that path is the project's file.
///
/// # Parameters
/// - `state`, `key`: as in [`new_project`].
/// - `path`: where to save. `.gtra` is added if missing
///   ([`with_gtra_extension`]). The folder must exist.
/// - `author`: as in [`save`].
///
/// # What it does
/// Normally the whole working database is written to `path` (nothing is
/// lost). For a **read-only** file (from a newer Ganttora) that is not
/// allowed, so a brand-new file with only what this version understands is
/// written instead, and the project continues with it, read-write. The UI
/// warns the user before it calls this for a read-only file.
///
/// # Returns
/// The [`ProjectResponse`] after saving: `file.path` is the new path,
/// `readOnly` and `dirty` are false, no upgrade notice.
///
/// # Errors
/// A `gtra.*` code if writing fails (no permission, folder missing, `path`
/// holds a file from a newer Ganttora, …). Then nothing changes: the
/// project keeps its old path and its dirty flag.
///
/// # Example
/// The M0 proof: open the sample, make its last task one
/// day long, save as `shed.gtra`, start a new project, open `shed.gtra` again:
/// the finish is still Fri 18 Dec 2026. Along the way, the dirty flag is
/// set by an edit, cleared by a save, and set again by undo, and the change
/// records end up in the file's journal.
/// ```
/// use document::Command;
/// use ganttora_lib::{ipc::{file, project}, state::AppState};
/// use model::Duration;
///
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let state = AppState::default();
/// let context = || project::change_context("Sam");
///
/// let opened = project::new_demo(&state, "main", "Sam").unwrap();
/// assert!(!opened.file.dirty);
/// let last = opened.snapshot.tasks.last().unwrap().id; // "Garage finished"
/// let command = Command::SetTaskDuration { id: last, duration: Duration::from_days(1) };
/// let edited = project::apply(&state, "main", command, &context()).unwrap();
/// assert!(edited.file.dirty); // an edit makes it dirty
///
/// // The ".gtra" is added automatically.
/// let saved = file::save_as(&state, "main", &dir.join("shed"), "Sam").unwrap();
/// assert_eq!(saved.file.file_name.as_deref(), Some("shed.gtra"));
/// assert!(!saved.file.dirty); // saving clears it
///
/// let undone = project::undo(&state, "main", &context()).unwrap();
/// assert!(undone.file.dirty); // undo is a change, too
/// let redone = project::redo(&state, "main", &context()).unwrap();
/// assert!(redone.file.dirty);
/// assert!(!file::save(&state, "main", "Sam").unwrap().file.dirty); // plain "Save"
///
/// let fresh = file::new_project(&state, "main", None, "Sam").unwrap();
/// assert_eq!(fresh.snapshot.tasks.len(), 1); // the project row
///
/// let reopened = file::open_file(&state, "main", &dir.join("shed.gtra")).unwrap();
/// assert_eq!(reopened.snapshot.project_name, "Garden shed");
/// assert_eq!(reopened.snapshot.tasks.last().unwrap().duration_minutes, 480);
/// assert_eq!(
///     reopened.snapshot.project_finish.unwrap().to_string(),
///     "2026-12-18 17:00:00" // Fri 18 Dec 2026, end of the working day
/// );
/// assert!(!reopened.file.dirty);
/// assert!(!reopened.file.read_only);
/// assert!(reopened.file.notes.is_empty());
/// assert!(!reopened.can_undo); // a freshly opened file has no history
///
/// // The edit, its undo and its redo are in the file's journal.
/// let journal = state
///     .with_open("main", |open| {
///         open.session.as_ref().unwrap().read_journal(&gtra::JournalFilter::default()).unwrap()
///     })
///     .unwrap();
/// assert_eq!(journal.len(), 3);
/// assert!(journal.iter().all(|e| e.record.author == "Sam" && e.record.field == "duration_minutes"));
/// # state.close("main").unwrap();
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn save_as(
    state: &AppState,
    key: &str,
    path: &Path,
    author: &str,
) -> Result<ProjectResponse, IpcError> {
    let path = with_gtra_extension(path);
    let working_dir = state.working_dir();
    state.with_open(key, |open| {
        let options = save_options(author);
        ensure_session(open, &working_dir, &options)?;
        let read_only = open.session.as_ref().is_some_and(|s| s.is_read_only());
        if read_only {
            // Only what the model holds can be written (see "What it does").
            let session = open.session.as_mut().expect(SESSION_MISSING);
            session.save_as_model_only(&path, open.document.project(), &options)?;
            // The new working copy can be written: keep the journal there;
            // it reaches the file with the next save.
            flush_journal(open)?;
        } else {
            flush_journal(open)?;
            let session = open.session.as_mut().expect(SESSION_MISSING);
            session.save_as(&path, open.document.project(), &options)?;
        }
        open.file = FileState {
            path: Some(path.clone()),
            ..FileState::default()
        };
        Ok(ProjectResponse::from_open(open))
    })?
}

/// Writes a recovery copy of the project under `key`, if it has unsaved
/// changes (autosave).
///
/// # Parameters
/// - `state`, `key`: as in [`new_project`].
/// - `author`: as in [`save`].
///
/// # Returns
/// `Ok(true)` if a copy was written; `Ok(false)` if there was nothing to do
/// (no unsaved changes, a read-only file, or recovery is switched off).
/// The user's file is never touched.
///
/// # Errors
/// A `gtra.*` code if the copy cannot be written; the previous copy stays.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::{file, project}, state::{AppState, SessionFolders}};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// let store = gtra::RecoveryStore::new(dir.join("recovery")).unwrap();
/// let state = AppState::new(SessionFolders {
///     working: Some(dir.join("working")),
///     recovery: Some(store.clone()),
/// });
/// project::new_demo(&state, "main", "Sam").unwrap();
/// assert!(!file::autosave(&state, "main", "Sam").unwrap()); // nothing changed
/// state.with_open("main", |open| open.file.dirty = true).unwrap();
/// assert!(file::autosave(&state, "main", "Sam").unwrap());
/// assert_eq!(store.list().unwrap().len(), 1);
/// state.close("main").unwrap(); // a clean close deletes the copy
/// assert!(store.list().unwrap().is_empty());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn autosave(state: &AppState, key: &str, author: &str) -> Result<bool, IpcError> {
    let Some(store) = state.recovery() else {
        return Ok(false);
    };
    let working_dir = state.working_dir();
    state.with_open(key, |open| {
        if !open.file.dirty || open.file.read_only {
            return Ok(false);
        }
        let options = save_options(author);
        ensure_session(open, &working_dir, &options)?;
        flush_journal(open)?;
        let session = open.session.as_mut().expect(SESSION_MISSING);
        session.autosave(open.document.project(), &options, store)?;
        Ok(true)
    })?
}

// ---------------------------------------------------------------------------
// The Tauri commands: thin wrappers the UI calls with `invoke`.
// ---------------------------------------------------------------------------
//
// These commands are `async`: Tauri runs them on a background thread, so
// the window stays responsive while a large file is read or written. The
// `State` parameters therefore need the `'_` lifetime and the commands must
// return a `Result` (a Tauri rule for async commands that borrow state).

/// IPC: replaces the calling window's project with a new, empty one.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("file_new", { name: t("file.untitledName") });
/// ```
///
/// # Parameters
/// - `name`: the project name; optional (`null` or missing gives
///   [`DEFAULT_PROJECT_NAME`]).
///
/// # Errors
/// An [`IpcError`] (`gtra.*` if the working copy cannot be made).
#[tauri::command]
pub async fn file_new(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
    name: Option<String>,
) -> Result<ProjectResponse, IpcError> {
    new_project(
        &state,
        window.label(),
        name.as_deref(),
        &author_name(&settings),
    )
}

/// IPC: opens a `.gtra` file in the calling window.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("file_open", { path: "/Users/sam/shed.gtra" });
/// ```
///
/// # Errors
/// A `gtra.*` [`IpcError`] if the file cannot be opened; the project open
/// before stays open.
#[tauri::command]
pub async fn file_open(
    window: Window,
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<ProjectResponse, IpcError> {
    open_file(&state, window.label(), &path)
}

/// IPC: saves the calling window's project to its current file.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("file_save");
/// ```
///
/// # Errors
/// `gtra.no_save_path` / `gtra.read_only` (the UI shows Save As instead),
/// or another `gtra.*` code if writing fails.
#[tauri::command]
pub async fn file_save(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
) -> Result<ProjectResponse, IpcError> {
    save(&state, window.label(), &author_name(&settings))
}

/// IPC: saves the calling window's project to a new file.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("file_save_as", { path: "/Users/sam/shed.gtra" });
/// ```
///
/// # Errors
/// A `gtra.*` [`IpcError`] if writing fails; nothing changes then.
#[tauri::command]
pub async fn file_save_as(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
    path: PathBuf,
) -> Result<ProjectResponse, IpcError> {
    save_as(&state, window.label(), &path, &author_name(&settings))
}

/// IPC: writes a recovery copy of the calling window's project if it has
/// unsaved changes. Called by the UI's autosave timer.
///
/// # Request (from TypeScript)
/// ```ts
/// const written = await invoke<boolean>("file_autosave");
/// ```
///
/// # Returns
/// `true` if a copy was written, `false` if there was nothing to do.
///
/// # Errors
/// A `gtra.*` [`IpcError`] if the copy cannot be written.
#[tauri::command]
pub async fn file_autosave(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
) -> Result<bool, IpcError> {
    autosave(&state, window.label(), &author_name(&settings))
}
