//! ipc/clipboard.rs: Copy and Paste of tasks through the system clipboard.
//!
//! # What it is for
//! The user selects rows in the task grid and presses Ctrl/Cmd+C: the tasks
//! go to the system clipboard as tab-separated text, which Excel, Numbers,
//! Google Sheets and MS Project understand. Ctrl/Cmd+V reads such text back
//! and inserts the rows as new tasks.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `copy_tasks`  | writes the given tasks to the clipboard |
//! | `paste_tasks` | reads the clipboard and inserts its rows as tasks (one undo step) |
//!
//! # Where it fits
//! ```text
//!   UI (frontend/ipc/client.ts) ──invoke──► these commands
//!       ├─► document::clipboard::copy_tasks / paste_tasks  (text ↔ tasks, pure)
//!       ├─► Document::apply (the paste plan is one Command::Batch)
//!       └─► tauri-plugin-clipboard-manager                 (the OS clipboard)
//! ```
//! The shell only moves text between the operating system and the
//! `document` crate; turning text into tasks is done there (decision D16).
//!
//! # Main ideas
//! - **Column headings come from the UI** ([`ColumnLabels`]), because Rust
//!   never contains UI text. The UI takes them from
//!   `locales/<lang>.json` (`grid.clipboard.*`).
//! - **One paste = one undo step.** The plan's command is a
//!   `Command::Batch`, applied with one change context (one batch ID in
//!   the journal).
//! - **Warnings, not failures.** A cell that cannot be read (e.g. a
//!   duration "3x") does not stop the paste; it comes back as a warning
//!   with a code, which the UI translates (`paste.*` keys).
//! - **Testable core.** As in `project.rs`, the plain functions
//!   [`copy_text`] and [`paste_text`] do the work without a window or a
//!   clipboard; the `#[tauri::command]` wrappers add only those two.
//!

use document::clipboard::{
    ColumnLabels, PasteWarning, copy_tasks as doc_copy, paste_tasks as doc_paste,
};
use model::TaskId;
use serde::Serialize;
use tauri::{AppHandle, State, Window};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::ipc::IpcError;
use crate::ipc::project::{ProjectResponse, author_name, change_context};
use crate::settings::SettingsState;
use crate::state::AppState;

/// Error code: the system clipboard could not be read or written.
pub const ERR_CLIPBOARD_UNAVAILABLE: &str = "clipboardUnavailable";

/// What `paste_tasks` sends back.
///
/// # JSON shape (shortened)
/// ```json
/// { "response": { "snapshot": { … }, "canUndo": true, … },
///   "taskIds": ["…", "…"],
///   "warnings": [{ "code": "unreadableDuration", "row": 2, "text": "3x" }] }
/// ```
/// Must match `PasteResponse` in `frontend/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteResponse {
    /// The project after the paste (unchanged if nothing was pasted).
    pub response: ProjectResponse,
    /// The IDs of the new tasks, in row order (empty if nothing was pasted).
    pub task_ids: Vec<TaskId>,
    /// Cells that could not be used as they were.
    pub warnings: Vec<PasteWarning>,
}

/// Makes the clipboard text for some tasks of the document under `key`.
///
/// # Parameters
/// - `state`: the shared app state.
/// - `key`: the window label (`"main"`).
/// - `ids`: the tasks to copy (any order; they are written in row order).
/// - `labels`: column headings for the header line, or `None` for no header.
///
/// # Returns
/// Tab-separated text, one line per task.
///
/// # Errors
/// `noProjectOpen` if the window has no project.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::{clipboard, project}, state::AppState};
/// let state = AppState::default();
/// let shed = project::new_demo(&state, "main", "Sam").unwrap();
/// let first = shed.snapshot.tasks[3].id; // after the project, "Preparation" and the first sub-task
/// let text = clipboard::copy_text(&state, "main", &[first], None).unwrap();
/// assert!(text.starts_with("Design the shed\t"));
/// ```
pub fn copy_text(
    state: &AppState,
    key: &str,
    ids: &[TaskId],
    labels: Option<&ColumnLabels>,
) -> Result<String, IpcError> {
    Ok(state.with_open(key, |open| {
        let doc = &open.document;
        doc_copy(doc.project(), doc.schedule().ok(), ids, labels)
    })?)
}

/// Inserts the rows of a clipboard text as new tasks, as one undo step.
///
/// # Parameters
/// - `state`, `key`: which document.
/// - `text`: the clipboard text (tab-separated rows, header optional).
/// - `at_index`: the row the first new task goes to (0 = first row; larger
///   than the number of tasks = at the end).
/// - `level`: the outline level of the first new task, or `None` to take it
///   from the place of the paste.
/// - `labels`: the UI language's column headings, or `None` (English
///   headings are always recognised).
/// - `author`: the journal author.
///
/// # Returns
/// A [`PasteResponse`]. If the text holds no rows, nothing changes and
/// `task_ids` is empty.
///
/// # Errors
/// `noProjectOpen`, or the document's error if the batch is refused (e.g. a
/// loop with existing links); the project is then unchanged.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::{clipboard, project}, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// let pasted = clipboard::paste_text(&state, "main", "Paint\t2d", 1, None, None, "Sam").unwrap();
/// assert_eq!(pasted.task_ids.len(), 1);
/// assert_eq!(pasted.response.snapshot.tasks[1].name, "Paint");
/// ```
pub fn paste_text(
    state: &AppState,
    key: &str,
    text: &str,
    at_index: usize,
    level: Option<u32>,
    labels: Option<&ColumnLabels>,
    author: &str,
) -> Result<PasteResponse, IpcError> {
    let context = change_context(author);
    state
        .with_open(key, |open| {
            let len = open.document.project().tasks.len();
            // A fresh random UUID for every new task and link (the document
            // never invents IDs itself, so it stays deterministic).
            let plan = doc_paste(
                open.document.project(),
                text,
                at_index.min(len),
                level,
                labels,
                uuid::Uuid::new_v4,
            );
            if !plan.task_ids.is_empty() {
                open.document.apply(plan.command, &context)?;
                open.file.dirty = true;
            }
            Ok::<_, document::DocumentError>(PasteResponse {
                response: ProjectResponse::from_open(open),
                task_ids: plan.task_ids,
                warnings: plan.warnings,
            })
        })?
        .map_err(IpcError::from)
}

/// IPC: copies tasks of the calling window's project to the system clipboard.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke("copy_tasks", { ids: ["…"], labels: { name: "Name", … } });
/// ```
///
/// # Errors
/// `noProjectOpen`, or `clipboardUnavailable` if the clipboard cannot be written.
#[tauri::command]
pub fn copy_tasks(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    ids: Vec<TaskId>,
    labels: Option<ColumnLabels>,
) -> Result<(), IpcError> {
    let text = copy_text(&state, window.label(), &ids, labels.as_ref())?;
    app.clipboard()
        .write_text(text)
        .map_err(|e| IpcError::new(ERR_CLIPBOARD_UNAVAILABLE, e.to_string()))
}

/// IPC: pastes the clipboard's rows as new tasks into the calling window's
/// project.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<PasteResponse>("paste_tasks", { atIndex: 3, level: 2, labels });
/// ```
///
/// # Errors
/// `noProjectOpen`, `clipboardUnavailable`, or the document's error.
#[tauri::command]
pub fn paste_tasks(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    settings: State<'_, SettingsState>,
    at_index: usize,
    level: Option<u32>,
    labels: Option<ColumnLabels>,
) -> Result<PasteResponse, IpcError> {
    let text = app
        .clipboard()
        .read_text()
        .map_err(|e| IpcError::new(ERR_CLIPBOARD_UNAVAILABLE, e.to_string()))?;
    paste_text(
        &state,
        window.label(),
        &text,
        at_index,
        level,
        labels.as_ref(),
        &author_name(&settings),
    )
}
