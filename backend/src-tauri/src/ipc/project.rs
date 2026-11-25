//! ipc/project.rs: the IPC commands for working with the open project.
//!
//! # What it is for
//! The user interface calls these commands to open the demo project, read
//! the current schedule, send an edit, and undo or redo. Every command
//! answers with the same shape, a [`ProjectResponse`]: the new schedule
//! snapshot plus whether Undo and Redo are possible now, plus the file
//! status. The UI simply replaces what it shows with that answer.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `project_new_demo` | opens the sample projects (garden shed, garage) |
//! | `project_snapshot` | returns the current schedule, changes nothing |
//! | `project_apply`    | applies one edit command (e.g. set a duration) |
//! | `project_undo`     | undoes the last edit |
//! | `project_redo`     | redoes the last undone edit |
//!
//! # Where it fits
//! ```text
//!   UI (frontend/ipc/client.ts) ──invoke──► these commands ──► AppState (state.rs)
//!                                                              └─► Document (document crate)
//!                                                                  └─► engine::schedule
//! ```
//! The shell is a thin layer (decision D16): it finds the right document,
//! adds the *who / when / which action* information ([`ChangeContext`]),
//! hands the work to the document and passes the answer back. It contains
//! no scheduling logic.
//!
//! # Main ideas
//! - **The shell reads the clock, the core does not.** The `document` crate
//!   is deterministic (same input, same output), so it never asks the
//!   operating system for the time or makes random IDs. The shell does that
//!   here, in [`change_context`], and passes the result in.
//! - **The author** of every change is the display name from Settings →
//!   User, or the operating system's user name when that is empty
//!   ([`author_name`]). It is stored in the change journal.
//! - **Thin wrappers, testable core.** Each `#[tauri::command]` function
//!   only finds the window label and calls a plain function (`new_demo`,
//!   `apply`, …) that takes the [`AppState`] directly. The examples in the
//!   doc comments below (doc tests, run by `cargo test`) call those plain
//!   functions without starting a window. The test crate in `tests/`
//!   (`ganttora-tests`) does not depend on this shell crate, so it cannot
//!   call them; it tests the `document` and `gtra` crates underneath.
//! - **Errors with codes.** A refused edit fails with an [`IpcError`] built
//!   from the document's own error (`DocumentError` serializes as
//!   `{code, details}`); the UI shows the translated message.
//!

use chrono::{Local, Utc};
use document::{BatchId, ChangeContext, Command, Document, ScheduleSnapshot};
use model::Calendar;
use serde::Serialize;
use tauri::{State, Window};

use crate::demo;
use crate::ipc::IpcError;
use crate::ipc::file::{FileStatus, open_untitled};
use crate::settings::SettingsState;
use crate::state::{AppState, OpenDocument};

/// What every project command (and every file command, `ipc/file.rs`)
/// sends back to the UI.
///
/// # JSON shape (as the UI receives it, shortened)
/// ```json
/// {
///   "snapshot": { "projectId": "…", "projectName": "Garden shed",
///                 "minutesPerDay": 480, "scheduleFrom": "start",
///                 "projectStart": "2026-11-17T08:00:00",
///                 "projectFinish": "2026-12-09T17:00:00",
///                 "error": null, "tasks": [ … ], "calendars": [ … ] },
///   "canUndo": false,
///   "canRedo": false,
///   "file": { "path": null, "fileName": null, "readOnly": false,
///             "dirty": false, "notes": [], "upgradedFrom": null,
///             "readOnlyReason": null },
///   "calendars": [ { "id": "…", "name": "Standard", "week": { … },
///                    "holidays": [], "exceptions": [] } ]
/// }
/// ```
/// Must match the TypeScript type `ProjectResponse` in
/// `frontend/ipc/types.ts` (hand-written until the types are generated).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    /// The schedule to draw: tasks with their calculated dates.
    pub snapshot: ScheduleSnapshot,
    /// `true` if the Undo button should be enabled.
    pub can_undo: bool,
    /// `true` if the Redo button should be enabled.
    pub can_redo: bool,
    /// The file behind the project: name, read-only, unsaved changes.
    pub file: FileStatus,
    /// Every calendar of the project with its full working times (the
    /// snapshot only lists IDs and names). The calendar editor
    /// (`frontend/features/calendars/`) edits them, and the Gantt chart uses
    /// the project calendar to shade non-working days and to snap drags to
    /// working days. A few calendars are small, so sending them with every
    /// answer keeps the UI in step after undo and redo for free.
    pub calendars: Vec<Calendar>,
}

impl ProjectResponse {
    /// Reads the current state of an open document into a response.
    ///
    /// # Parameters
    /// - `open`: the document and its file facts (borrowed, read only).
    ///
    /// # Returns
    /// A new [`ProjectResponse`] (with an empty `file.notes` list).
    ///
    /// # Example
    /// ```
    /// use document::Document;
    /// use ganttora_lib::{ipc::project::ProjectResponse, state::OpenDocument};
    /// let open = OpenDocument::untitled(Document::new(ganttora_lib::demo::garden_shed()));
    /// let response = ProjectResponse::from_open(&open);
    /// assert!(!response.can_undo);
    /// assert_eq!(response.snapshot.tasks.len(), 8);
    /// assert!(!response.file.dirty);
    /// assert_eq!(response.calendars.len(), 1); // the shed has one calendar
    /// ```
    pub fn from_open(open: &OpenDocument) -> Self {
        ProjectResponse {
            snapshot: open.document.snapshot(),
            can_undo: open.document.can_undo(),
            can_redo: open.document.can_redo(),
            file: FileStatus::from_state(&open.file),
            calendars: open.document.project().calendars.clone(),
        }
    }
}

/// Text used as the author when neither the settings nor the operating
/// system give a name. Stored in change records (the journal), not shown as
/// a UI label, so it is not in `locales/en.json`.
pub const UNKNOWN_AUTHOR: &str = "Unknown";

/// The operating system's login name.
///
/// # What it does
/// Reads the environment variable `USER` (macOS, Linux) or `USERNAME`
/// (Windows). If neither is set or both are empty, returns
/// [`UNKNOWN_AUTHOR`].
///
/// # Returns
/// The name, e.g. `"sam"`.
///
/// # Example
/// ```
/// let name = ganttora_lib::ipc::project::os_user_name();
/// assert!(!name.is_empty());
/// ```
///
/// # Beginner note
/// An *environment variable* is a named piece of text the operating system
/// gives every program when it starts (like a note pinned to the door).
/// `std::env::var("USER")` reads one; it returns a `Result`, because the
/// variable may not exist. `.ok()` turns the `Result` into an `Option`, and
/// `.or_else(...)` tries the next variable only if the first gave nothing.
pub fn os_user_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            std::env::var("USERNAME")
                .ok()
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| UNKNOWN_AUTHOR.to_string())
}

/// Picks the author name for change records and file metadata.
///
/// # Parameters
/// - `display_name`: Settings → User → Display name (may be empty).
///
/// # Returns
/// The trimmed display name, or [`os_user_name`] if it is empty.
///
/// # Example
/// ```
/// use ganttora_lib::ipc::project::author_from;
/// assert_eq!(author_from("  Sam Lee "), "Sam Lee");
/// assert!(!author_from("").is_empty()); // falls back to the OS user name
/// ```
pub fn author_from(display_name: &str) -> String {
    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        os_user_name()
    } else {
        trimmed.to_string()
    }
}

/// The author name from the current settings (see [`author_from`]).
///
/// # Parameters
/// - `settings`: the shell's settings state.
///
/// # Returns
/// The author name. If the settings cannot be read (a poisoned lock), the
/// operating system's user name.
pub fn author_name(settings: &SettingsState) -> String {
    match settings.current() {
        Ok(loaded) => author_from(&loaded.settings.user.display_name),
        Err(_) => os_user_name(),
    }
}

/// Builds the *who / when / which action* information for one user action.
///
/// # What it does
/// Reads the clock (`Utc::now()`) and creates a fresh random [`BatchId`].
/// One user action (one click on Apply, one press of Ctrl+Z) gets one
/// context, so all change records it produces share a batch ID.
///
/// # Parameters
/// - `author`: who made the change (see [`author_name`]).
///
/// # Returns
/// A new [`ChangeContext`].
///
/// # Example
/// ```
/// let context = ganttora_lib::ipc::project::change_context("Sam Lee");
/// assert_eq!(context.author, "Sam Lee");
/// ```
pub fn change_context(author: &str) -> ChangeContext {
    ChangeContext {
        author: author.to_string(),
        timestamp: Utc::now(),
        batch_id: BatchId::new(),
    }
}

// ---------------------------------------------------------------------------
// Plain functions: the real work, callable from tests without a window.
// ---------------------------------------------------------------------------

/// Opens the sample projects (garden shed and garage) under `key`, replacing whatever was
/// open there.
///
/// # Parameters
/// - `state`: the shell's shared state.
/// - `key`: the window label, e.g. `"main"`.
/// - `author`: written into the working copy's metadata.
///
/// # Returns
/// The new project's [`ProjectResponse`] (empty undo/redo history, no
/// file yet, not dirty).
///
/// # Errors
/// An [`IpcError`] if the working copy cannot be created (`gtra.*` codes)
/// or the shared state is unusable.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::project, state::AppState};
/// let state = AppState::default();
/// let response = project::new_demo(&state, "main", "Sam").unwrap();
/// assert_eq!(
///     response.snapshot.project_finish.unwrap().to_string(),
///     "2026-12-17 17:00:00"
/// );
/// assert_eq!(response.file.path, None);
/// ```
pub fn new_demo(state: &AppState, key: &str, author: &str) -> Result<ProjectResponse, IpcError> {
    open_untitled(state, key, Document::new(demo::sample_projects()), author)
}

/// Returns the current state of the document under `key`. Changes nothing.
///
/// # Parameters
/// - `state`: the shell's shared state.
/// - `key`: the window label.
///
/// # Returns
/// The current [`ProjectResponse`].
///
/// # Errors
/// `noProjectOpen` if no document is open under `key`;
/// `projectUnavailable` if the lock is poisoned.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::project, state::AppState};
/// let state = AppState::default();
/// assert_eq!(project::snapshot(&state, "main").unwrap_err().code, "noProjectOpen");
/// project::new_demo(&state, "main", "Sam").unwrap();
/// assert!(project::snapshot(&state, "main").is_ok());
/// ```
pub fn snapshot(state: &AppState, key: &str) -> Result<ProjectResponse, IpcError> {
    Ok(state.with_open(key, |open| ProjectResponse::from_open(open))?)
}

/// Applies one edit command to the document under `key`.
///
/// # Parameters
/// - `state`: the shell's shared state.
/// - `key`: the window label.
/// - `command`: the edit, e.g. `Command::SetTaskDuration { … }`.
/// - `context`: author, time and batch ID (see [`change_context`]).
///
/// # Returns
/// The [`ProjectResponse`] after the edit.
///
/// # Errors
/// `noProjectOpen`, or the document's own code if it refused the command
/// (e.g. `taskNotFound`, `negativeDuration`, `rejected` with the engine's
/// `cycleDetected` inside). When the document refuses, the project is
/// unchanged.
///
/// # Example
/// The milestone "Garage finished" becomes one day long: the finish moves
/// from Thu 17 Dec to Fri 18 Dec 2026.
/// ```
/// use document::Command;
/// use ganttora_lib::{ipc::project, state::AppState};
/// use model::Duration;
/// let state = AppState::default();
/// let opened = project::new_demo(&state, "main", "Sam").unwrap();
/// let last = opened.snapshot.tasks.last().unwrap().id; // "Garage finished"
/// let command = Command::SetTaskDuration { id: last, duration: Duration::from_days(1) };
/// let after = project::apply(&state, "main", command, &project::change_context("Sam")).unwrap();
/// assert_eq!(after.snapshot.project_finish.unwrap().to_string(), "2026-12-18 17:00:00");
/// assert!(after.can_undo);
/// ```
///
/// The same edit as the UI sends it (JSON), and the JSON that goes back.
/// This checks the contract with `frontend/ipc/types.ts`:
/// ```
/// use document::Command;
/// use ganttora_lib::{ipc::project, state::AppState};
/// let state = AppState::default();
/// let opened = project::new_demo(&state, "main", "Sam").unwrap();
/// let last = opened.snapshot.tasks.last().unwrap().id; // "Garage finished"
/// let json = format!(
///     r#"{{"type":"setTaskDuration","id":"{last}","duration":{{"minutes":480}}}}"#
/// );
/// let command: Command = serde_json::from_str(&json).unwrap();
/// let after = project::apply(&state, "main", command, &project::change_context("Sam")).unwrap();
/// let sent = serde_json::to_value(&after).unwrap();
/// assert_eq!(sent["snapshot"]["projectFinish"], "2026-12-18T17:00:00");
/// assert_eq!(sent["snapshot"]["tasks"][37]["durationMinutes"], 480);
/// assert_eq!(sent["snapshot"]["tasks"][37]["links"][0]["kind"], "finishToStart");
/// assert_eq!(sent["canUndo"], true);
/// assert_eq!(sent["canRedo"], false);
/// assert_eq!(sent["file"]["dirty"], true);
/// ```
pub fn apply(
    state: &AppState,
    key: &str,
    command: Command,
    context: &ChangeContext,
) -> Result<ProjectResponse, IpcError> {
    edit(state, key, |doc| doc.apply(command, context).map(|_| ()))
}

/// Undoes the last edit in the document under `key`.
///
/// # Parameters
/// - `state`, `key`: as in [`apply`].
/// - `context`: author, time and batch ID for the change records of the undo.
///
/// # Returns
/// The [`ProjectResponse`] after the undo.
///
/// # Errors
/// `noProjectOpen`, or `nothingToUndo`.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::project, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// let err = project::undo(&state, "main", &project::change_context("Sam")).unwrap_err();
/// assert_eq!(err.code, "nothingToUndo");
/// ```
pub fn undo(
    state: &AppState,
    key: &str,
    context: &ChangeContext,
) -> Result<ProjectResponse, IpcError> {
    edit(state, key, |doc| doc.undo(context).map(|_| ()))
}

/// Redoes the last undone edit in the document under `key`.
///
/// # Parameters
/// - `state`, `key`: as in [`apply`].
/// - `context`: author, time and batch ID for the change records of the redo.
///
/// # Returns
/// The [`ProjectResponse`] after the redo.
///
/// # Errors
/// `noProjectOpen`, or `nothingToRedo`.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::project, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// let err = project::redo(&state, "main", &project::change_context("Sam")).unwrap_err();
/// assert_eq!(err.code, "nothingToRedo");
/// ```
pub fn redo(
    state: &AppState,
    key: &str,
    context: &ChangeContext,
) -> Result<ProjectResponse, IpcError> {
    edit(state, key, |doc| doc.redo(context).map(|_| ()))
}

/// Shared helper for `apply`, `undo` and `redo`: runs one change on the
/// document and, if it worked, describes the document afterwards.
///
/// # Parameters
/// - `state`, `key`: which document.
/// - `change`: the change to run. Returns `Ok(())` or the document's error.
///
/// # Returns
/// The [`ProjectResponse`] after the change, or the [`IpcError`].
///
/// A successful change also marks the project **dirty** (unsaved changes,
/// see `ipc/file.rs`). A refused change leaves the flag as it was, because
/// the project did not change.
///
/// All steps happen inside one `with_open` call, so they run under one
/// lock: no other command can slip in between the change and the snapshot.
/// The change records the document collects are written to the file's
/// journal on the next save or autosave (`ipc/file.rs`).
fn edit(
    state: &AppState,
    key: &str,
    change: impl FnOnce(&mut Document) -> Result<(), document::DocumentError>,
) -> Result<ProjectResponse, IpcError> {
    // `with_open` returns `Result<Result<…, DocumentError>, StateError>`.
    // The outer `?` turns a StateError into an IpcError (the `From` in
    // ipc/mod.rs) and unwraps it; the inner result is then converted by
    // `map_err(IpcError::from)`.
    state
        .with_open(key, |open| {
            change(&mut open.document)?;
            open.file.dirty = true;
            Ok::<_, document::DocumentError>(ProjectResponse::from_open(open))
        })?
        .map_err(IpcError::from)
}

// ---------------------------------------------------------------------------
// The Tauri commands: thin wrappers the UI calls with `invoke`.
// ---------------------------------------------------------------------------
//
// Beginner notes for the parameters below:
// - `window: Window` is filled in by Tauri with the window that sent the
//   command. Its label (`"main"`) is the key of that window's document.
// - `state: State<'_, AppState>` is filled in by Tauri with the value given
//   to `.manage(...)` in lib.rs. `'_` is a *lifetime*: it tells Rust the
//   borrowed state lives at least as long as this call (book chapter on Rust).
// - Parameters that are not `Window` or `State` come from the JSON the UI
//   sends: `invoke("project_apply", { command: {...} })` fills `command`.
// - These commands are not `async`, so Tauri runs them on the main thread.
//   That is fine: each one takes microseconds for the shed. Commands that
//   touch the disk (`ipc/file.rs`) are `async`.

/// IPC: opens the sample projects in the calling window, with their dates
/// moved to around today (see [`demo::sample_projects_for`]; [`new_demo`]
/// keeps the fixed dates, so examples always give the same result).
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("project_new_demo");
/// ```
///
/// # Returns
/// The new project's [`ProjectResponse`].
///
/// # Errors
/// An [`IpcError`] (`gtra.*` if the working copy cannot be made).
#[tauri::command]
pub fn project_new_demo(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
) -> Result<ProjectResponse, IpcError> {
    // In the user's own time zone (Local): the sample lies around today.
    let sample = demo::sample_projects_for(Local::now().date_naive());
    open_untitled(
        &state,
        window.label(),
        Document::new(sample),
        &author_name(&settings),
    )
}

/// IPC: returns the current schedule of the calling window's project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("project_snapshot");
/// ```
///
/// # Returns
/// The current [`ProjectResponse`].
///
/// # Errors
/// `noProjectOpen` if no project is open in this window.
#[tauri::command]
pub fn project_snapshot(
    window: Window,
    state: State<'_, AppState>,
) -> Result<ProjectResponse, IpcError> {
    snapshot(&state, window.label())
}

/// IPC: applies one edit command to the calling window's project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("project_apply", {
///   command: { type: "setTaskDuration", id: "…", duration: { minutes: 2880 } },
/// });
/// ```
///
/// # Parameters
/// - `command`: the edit, decoded from JSON by serde (see `document::Command`
///   for every variant and its fields).
///
/// # Returns
/// The [`ProjectResponse`] after the edit.
///
/// # Errors
/// An [`IpcError`] if no project is open or the edit was refused; the
/// project is then unchanged.
#[tauri::command]
pub fn project_apply(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
    command: Command,
) -> Result<ProjectResponse, IpcError> {
    let context = change_context(&author_name(&settings));
    apply(&state, window.label(), command, &context)
}

/// IPC: undoes the last edit in the calling window's project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("project_undo");
/// ```
///
/// # Errors
/// `noProjectOpen`, or `nothingToUndo`.
#[tauri::command]
pub fn project_undo(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
) -> Result<ProjectResponse, IpcError> {
    undo(
        &state,
        window.label(),
        &change_context(&author_name(&settings)),
    )
}

/// IPC: redoes the last undone edit in the calling window's project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("project_redo");
/// ```
///
/// # Errors
/// `noProjectOpen`, or `nothingToRedo`.
#[tauri::command]
pub fn project_redo(
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
) -> Result<ProjectResponse, IpcError> {
    redo(
        &state,
        window.label(),
        &change_context(&author_name(&settings)),
    )
}
