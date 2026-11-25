//! ipc/recovery.rs: the IPC commands for crash recovery.
//!
//! # What it is for
//! While a project has unsaved changes, autosave writes a *recovery copy*
//! to `<app data>/recovery` every few minutes (`file_autosave` in
//! `ipc/file.rs`). A clean close deletes it. If Ganttora crashes (or the
//! computer loses power), the copy stays behind. At the next start the UI
//! lists the copies with these commands and asks the user, for each one,
//! **Restore** or **Discard**.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `recovery_list`    | lists the recovery copies left behind |
//! | `recovery_restore` | opens one copy as the window's project (with unsaved changes) |
//! | `recovery_discard` | deletes one copy |
//!
//! # Where it fits
//! ```text
//!   UI start-up (frontend/features/files/recovery.ts)
//!     └─invoke("recovery_list")─► RecoveryStore::list
//!     └─invoke("recovery_restore", { sessionId })─► ProjectSession::recover
//!     └─invoke("recovery_discard", { sessionId })─► RecoveryStore::discard
//! ```
//!
//! # Main ideas
//! - The copies are found by `gtra::RecoveryStore`; the shell only turns
//!   them into JSON ([`RecoveryCopyDto`]).
//! - A restored project is **dirty**: its changes were never saved to the
//!   user's file. Its file path is the file it came from (if it had one),
//!   so "Save" writes there.
//! - Only one Ganttora runs at a time (the single-instance plugin in
//!   `lib.rs`), so every copy in the folder really is left over from a
//!   crash, never a copy another running window is still using.
//!

use gtra::{Access, ProjectSession, RecoveryCopy, RecoveryStore};
use serde::Serialize;
use tauri::{State, Window};
use uuid::Uuid;

use crate::ipc::IpcError;
use crate::ipc::file::ReadNoteDto;
use crate::ipc::project::ProjectResponse;
use crate::state::{AppState, FileState, OpenDocument};

/// Error code: recovery is switched off (the recovery folder could not be
/// created at start-up).
pub const ERR_RECOVERY_UNAVAILABLE: &str = "recoveryUnavailable";
/// Error code: no recovery copy with that session id exists (any more).
pub const ERR_RECOVERY_NOT_FOUND: &str = "recoveryNotFound";

/// One recovery copy, as JSON for the UI.
///
/// # JSON shape
/// ```json
/// { "sessionId": "9b1d…", "sourcePath": "/Users/sam/shed.gtra",
///   "sourceFileName": "shed.gtra", "savedAt": "2027-02-15T10:42:07Z",
///   "projectName": "Garden shed", "readable": true }
/// ```
/// Must match `RecoveryCopyDto` in `frontend/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCopyDto {
    /// The id the UI sends back to restore or discard this copy.
    pub session_id: String,
    /// The user's file the project belonged to, or `null` if it was never
    /// saved.
    pub source_path: Option<String>,
    /// Just the file name of `source_path`, or `null`.
    pub source_file_name: Option<String>,
    /// When the copy was written (UTC, ISO 8601 text), or `null` if unknown.
    pub saved_at: Option<String>,
    /// The project's name from the copy, or `null` if it cannot be read.
    pub project_name: Option<String>,
    /// `false` if the copy is damaged; the UI then only offers Discard.
    pub readable: bool,
}

impl From<&RecoveryCopy> for RecoveryCopyDto {
    fn from(copy: &RecoveryCopy) -> Self {
        RecoveryCopyDto {
            session_id: copy.session_id.to_string(),
            source_path: copy
                .source
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            source_file_name: copy
                .source
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|name| name.to_string_lossy().into_owned()),
            saved_at: copy.saved_at.clone(),
            project_name: copy.project_name.clone(),
            readable: copy.readable,
        }
    }
}

/// Gives the recovery store or the "switched off" error.
fn store(state: &AppState) -> Result<&RecoveryStore, IpcError> {
    state
        .recovery()
        .ok_or_else(|| IpcError::new(ERR_RECOVERY_UNAVAILABLE, ""))
}

/// Reads a session id sent by the UI.
///
/// # Errors
/// `recoveryNotFound` if the text is not a UUID (it cannot name a copy).
fn parse_id(session_id: &str) -> Result<Uuid, IpcError> {
    Uuid::parse_str(session_id).map_err(|e| IpcError::new(ERR_RECOVERY_NOT_FOUND, e.to_string()))
}

// ---------------------------------------------------------------------------
// Plain functions: the real work, callable from tests without a window.
// ---------------------------------------------------------------------------

/// Lists the recovery copies left behind, newest first.
///
/// # Returns
/// The copies; an empty list if there are none or recovery is switched off.
///
/// # Errors
/// A `gtra.*` code if the recovery folder cannot be read.
///
/// # Example
/// ```
/// let state = ganttora_lib::state::AppState::default(); // recovery off
/// assert!(ganttora_lib::ipc::recovery::list(&state).unwrap().is_empty());
/// ```
pub fn list(state: &AppState) -> Result<Vec<RecoveryCopyDto>, IpcError> {
    let Some(store) = state.recovery() else {
        return Ok(Vec::new());
    };
    let mut copies = store.list()?;
    // Newest first: the UI asks about the most recent work first. A copy
    // without a time sorts last (`None` is smaller than any `Some`).
    copies.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Ok(copies.iter().map(RecoveryCopyDto::from).collect())
}

/// Opens one recovery copy as the project under `key`, replacing whatever
/// was open there.
///
/// # Parameters
/// - `state`, `key`: the shell's state and the window label.
/// - `session_id`: the copy's id from [`list`].
///
/// # Returns
/// The restored project's [`ProjectResponse`]: `dirty` is true, `path` is
/// the file it came from (or `null`), plus any read notes.
///
/// # Errors
/// - `recoveryUnavailable` / `recoveryNotFound`;
/// - `gtra.not_a_gtra_file` if the copy is damaged; other `gtra.*` codes.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::{file, project, recovery}, state::{AppState, SessionFolders}};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// let folders = SessionFolders {
///     working: Some(dir.join("working")),
///     recovery: Some(gtra::RecoveryStore::new(dir.join("recovery")).unwrap()),
/// };
/// // A first "run" writes a recovery copy and then "crashes" (forgets the
/// // state without closing it cleanly).
/// let crashed = AppState::new(folders.clone());
/// project::new_demo(&crashed, "main", "Sam").unwrap();
/// crashed.with_open("main", |open| open.file.dirty = true).unwrap();
/// file::autosave(&crashed, "main", "Sam").unwrap();
/// crashed.close_all(true); // keep_recovery: like a crash
///
/// // The next start finds and restores it.
/// let state = AppState::new(folders);
/// let copies = recovery::list(&state).unwrap();
/// assert_eq!(copies.len(), 1);
/// assert_eq!(copies[0].project_name.as_deref(), Some("Garden shed"));
/// let restored = recovery::restore(&state, "main", &copies[0].session_id).unwrap();
/// assert!(restored.file.dirty);
/// assert_eq!(restored.snapshot.tasks.len(), 38); // all rows of the two sample projects
/// state.close("main").unwrap(); // clean close: the copy is gone
/// assert!(recovery::list(&state).unwrap().is_empty());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn restore(state: &AppState, key: &str, session_id: &str) -> Result<ProjectResponse, IpcError> {
    let id = parse_id(session_id)?;
    let copy = store(state)?
        .list()?
        .into_iter()
        .find(|copy| copy.session_id == id)
        .ok_or_else(|| IpcError::new(ERR_RECOVERY_NOT_FOUND, session_id.to_string()))?;
    let (session, mut opened) = ProjectSession::recover(&copy, state.working_dir())?;
    opened.project.ensure_project_row();
    let open = OpenDocument {
        document: document::Document::new(opened.project),
        file: FileState {
            path: copy.source.clone(),
            read_only: opened.access == Access::ReadOnly,
            // The copy holds changes that never reached the user's file.
            dirty: true,
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

/// Deletes one recovery copy.
///
/// # Returns
/// `Ok(())`, also when the copy was already gone.
///
/// # Errors
/// `recoveryUnavailable`, `recoveryNotFound` (not a valid id), or
/// `gtra.io` / `gtra.permission_denied` if the file cannot be deleted.
pub fn discard(state: &AppState, session_id: &str) -> Result<(), IpcError> {
    let id = parse_id(session_id)?;
    store(state)?.discard(id)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The Tauri commands.
// ---------------------------------------------------------------------------

/// IPC: lists the recovery copies left behind by a crash.
///
/// # Request (from TypeScript)
/// ```ts
/// const copies = await invoke<RecoveryCopyDto[]>("recovery_list");
/// ```
#[tauri::command]
pub async fn recovery_list(state: State<'_, AppState>) -> Result<Vec<RecoveryCopyDto>, IpcError> {
    list(&state)
}

/// IPC: restores one recovery copy as the calling window's project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<ProjectResponse>("recovery_restore", { sessionId });
/// ```
///
/// # Beginner note
/// The Rust parameter is `session_id`; Tauri accepts it from JavaScript as
/// `sessionId` (it converts camelCase argument names automatically).
#[tauri::command]
pub async fn recovery_restore(
    window: Window,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ProjectResponse, IpcError> {
    restore(&state, window.label(), &session_id)
}

/// IPC: deletes one recovery copy.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke("recovery_discard", { sessionId });
/// ```
#[tauri::command]
pub async fn recovery_discard(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), IpcError> {
    discard(&state, &session_id)
}
