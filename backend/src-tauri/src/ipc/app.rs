//! ipc/app.rs: closing the project and quitting the app safely.
//!
//! # What it is for
//! Unsaved changes must never be lost by accident. Two ways out of the app
//! are guarded:
//! 1. **Closing the window** (close button, Alt+F4, Cmd+W). The UI holds the
//!    close back (`frontend/features/files/useCloseGuard.ts`), asks
//!    "Save / Don't save / Cancel", then calls `app_close_project` and
//!    closes the window.
//! 2. **Quitting the app** (Quit / Exit in our menu, Cmd+Q on macOS; see
//!    `menu.rs`). Tauri reports this as `RunEvent::ExitRequested`. If there
//!    are unsaved changes, [`on_exit_requested`] stops the exit and sends
//!    the event [`QUIT_REQUESTED_EVENT`] to the window. The UI asks the same
//!    question and, unless the user cancels, calls `app_quit`.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `app_close_project` | closes the window's project cleanly (deletes working and recovery copies) |
//! | `app_quit`          | closes every project cleanly and ends the app |
//!
//! # Main ideas
//! - **Clean close = the user decided.** After Save or Don't save, the
//!   recovery copy is no longer needed, so it is deleted. If the app ends
//!   with unsaved changes *without* asking (no window left to ask in), the
//!   recovery copy is kept, so the next start can offer it.
//! - The shell does not show any dialog itself: all texts are translated in
//!   the UI (`locales/en.json`).
//!

use tauri::{AppHandle, Emitter, Manager, Runtime, State, Window};

use crate::ipc::IpcError;
use crate::state::AppState;

/// Name of the event sent to the UI when the user tries to quit while a
/// project has unsaved changes. The UI listens for it in
/// `frontend/features/files/useCloseGuard.ts`. Not UI text.
pub const QUIT_REQUESTED_EVENT: &str = "app://quit-requested";

/// Label of the main window (set in `tauri.conf.json`).
pub const MAIN_WINDOW: &str = "main";

/// IPC: closes the calling window's project cleanly.
///
/// # What it does
/// Removes the project from the shell's state and ends its session: the
/// working copy and the recovery copy are deleted. The UI calls this right
/// before it closes the window, after the user answered the unsaved-changes
/// question with Save (and the save worked) or Don't save.
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke("app_close_project");
/// ```
///
/// # Errors
/// `projectUnavailable` if the shared state is broken. Closing a window
/// with no project is fine (nothing happens).
#[tauri::command]
pub fn app_close_project(window: Window, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.close(window.label())?;
    Ok(())
}

/// IPC: closes every project cleanly and ends the app.
///
/// # What it does
/// Called by the UI after the quit question was answered with Save (and
/// the save worked) or Don't save. Afterwards no project is dirty, so the
/// exit that `app.exit(0)` starts is not held back again by
/// [`on_exit_requested`].
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke("app_quit");
/// ```
///
/// # Beginner note
/// `AppHandle` is a handle to the whole running app (not one window).
#[tauri::command]
pub fn app_quit(app: AppHandle, state: State<'_, AppState>) {
    state.close_all(false);
    app.exit(0);
}

/// Decides what happens when the app is asked to end
/// (`RunEvent::ExitRequested`). Called from the run loop in `lib.rs`.
///
/// # What it does
/// - No unsaved changes: close every project cleanly and let the app end.
/// - Unsaved changes and the main window is still there: keep the app
///   running (`prevent_exit`) and send [`QUIT_REQUESTED_EVENT`] to the
///   window, so the UI can ask "Save / Don't save / Cancel".
/// - Unsaved changes but no window left (it cannot ask anyone): close every
///   project but **keep** the recovery copies, so nothing is lost.
///
/// # Parameters
/// - `app`: the running app.
/// - `prevent_exit`: a function that stops this exit (Tauri's
///   `ExitRequestApi::prevent_exit`).
///
/// # Beginner note
/// `<R: Runtime>` makes the function work with any Tauri *runtime*; read
/// it as "for the app, whatever engine runs it".
/// `prevent_exit: impl FnOnce()` takes a closure, so this function does not
/// need to know Tauri's exact event types; `lib.rs` passes
/// `|| api.prevent_exit()`.
pub fn on_exit_requested<R: Runtime>(app: &AppHandle<R>, prevent_exit: impl FnOnce()) {
    let state = app.state::<AppState>();
    if !state.any_dirty() {
        state.close_all(false);
        return;
    }
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => {
            prevent_exit();
            // Bring the window forward so the user sees the question.
            let _ = window.unminimize();
            let _ = window.set_focus();
            if let Err(error) = window.emit(QUIT_REQUESTED_EVENT, ()) {
                eprintln!("ganttora: could not ask the window about unsaved changes: {error}");
            }
        }
        None => state.close_all(true),
    }
}
