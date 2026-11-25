//! lib.rs: builds and starts the Tauri application.
//!
//! # What it is for
//! Puts the desktop app together: the plugins (single instance, file
//! dialogs), the user settings (loaded once at start-up, see
//! `settings.rs`), the shared state with the app's `working/` and
//! `recovery/` folders (`state.rs`), the IPC commands the UI may call, and
//! the guard that stops the app from quitting with unsaved changes. Then it
//! opens the window described in `tauri.conf.json` and runs until the app
//! ends.
//!
//! # Where it fits
//! `main.rs` calls [`run`]. The window shows the React UI from `frontend/`;
//! the UI calls the commands registered here (see `ipc/mod.rs`).
//!
//! # Main ideas
//! - Tauri uses the *builder pattern*: you start with `tauri::Builder::default()`
//!   and add one piece at a time with method calls (`.plugin(...)`,
//!   `.invoke_handler(...)`), then `.build(...)` and `.run(...)` at the end.
//! - **The run loop.** `.run(|app, event| …)` receives every app-wide event
//!   (`RunEvent`). We watch `ExitRequested` (Quit in our menu, the last window closed,
//!   the system asking the app to end) and hand it to
//!   `ipc::app::on_exit_requested`, which may hold the exit back to ask
//!   about unsaved changes.
//! - The shell is thin: no scheduling logic here (decision D16).
//!

// `mod` declares a module: a file that belongs to this crate.
// `pub` makes it visible outside the crate (needed so tests can reach it later).
pub mod demo;
pub mod ipc;
#[cfg(target_os = "macos")]
mod macos_quit;
pub mod menu;
pub mod paths;
pub mod settings;
pub mod state;
pub mod window_state;

use std::sync::Arc;

use tauri::{AppHandle, Manager, RunEvent, Runtime};

/// Builds the Tauri app and runs it until it ends.
///
/// # Errors
/// Panics with a message if the app cannot start, for example when the
/// system web view is missing. There is nothing sensible to continue with
/// in that case, so stopping with a clear message is the right behaviour.
///
/// # Example
/// ```no_run
/// ganttora_lib::run();
/// ```
pub fn run() {
    let app = tauri::Builder::default()
        // A *plugin* adds a ready-made feature to Tauri.
        // The single-instance plugin must be registered first: when the user
        // starts Ganttora while it is already running, the new copy ends at
        // once and this closure runs in the *first* copy, which brings its
        // window to the front.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window(ipc::app::MAIN_WINDOW) {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // The dialog plugin gives the UI the system's "Open" and "Save As"
        // windows (used by frontend/features/files/dialogs.ts). What the UI
        // may do with it is limited by capabilities/default.json.
        .plugin(tauri_plugin_dialog::init())
        // The clipboard plugin lets our own Rust commands read and write the
        // system clipboard (Copy / Paste of tasks, ipc/clipboard.rs).
        .plugin(tauri_plugin_clipboard_manager::init())
        // `setup` runs once, after Tauri has started but before the window
        // shows the UI. Only now do we have an `AppHandle`, which knows the
        // operating system's config and data folders.
        .setup(|app| {
            let handle = app.handle();
            // `.ok()` turns "no config folder" (an error) into `None`; the
            // settings then live in memory only and the UI is told.
            let path = paths::settings_file(handle).ok();
            // Time stamp for the name of a backup, should the file be damaged.
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
            app.manage(settings::SettingsState::load(path, &stamp));
            app.manage(state::AppState::new(session_folders(handle)));
            // The menu bar's entries; the UI fills them in (menu.rs).
            app.manage(menu::MenuState::default());
            // Quit from the Dock icon must ask about unsaved changes too.
            #[cfg(target_os = "macos")]
            macos_quit::install(handle);
            show_window(handle);
            Ok(())
        })
        // Runs when the user chooses an entry of the menu bar.
        .on_menu_event(|app, event| menu::on_menu_event(app, &event))
        // Registers the commands the UI may call with `invoke(...)`.
        // `generate_handler!` is a *macro* (the `!` marks it): code that
        // writes code. Here it writes the glue that turns a command name
        // sent from JavaScript into a call of the matching Rust function.
        // Every command must be listed here, or the UI gets an
        // "unknown command" error when it calls it.
        .invoke_handler(tauri::generate_handler![
            ipc::app_info,
            ipc::project::project_new_demo,
            ipc::project::project_snapshot,
            ipc::project::project_apply,
            ipc::project::project_undo,
            ipc::project::project_redo,
            ipc::clipboard::copy_tasks,
            ipc::clipboard::paste_tasks,
            ipc::calendar::calendar_working_minutes,
            ipc::calendar::calendar_add_working_time,
            ipc::file::file_new,
            ipc::file::file_open,
            ipc::file::file_save,
            ipc::file::file_save_as,
            ipc::file::file_autosave,
            ipc::recovery::recovery_list,
            ipc::recovery::recovery_restore,
            ipc::recovery::recovery_discard,
            ipc::app::app_close_project,
            ipc::app::app_quit,
            ipc::settings::settings_get,
            ipc::settings::settings_set,
            ipc::settings::settings_reset,
            menu::menu_set,
            menu::menu_update,
        ])
        // `generate_context!` reads tauri.conf.json at compile time.
        .build(tauri::generate_context!())
        .expect("error while building the Ganttora application");

    // The run loop: Tauri calls this closure for every app-wide event.
    app.run(|app, event| {
        match event {
            RunEvent::ExitRequested { api, .. } => {
                ipc::app::on_exit_requested(app, || api.prevent_exit());
            }
            // The app really ends: write a position not written yet.
            RunEvent::Exit => app.state::<Arc<window_state::WindowMemory>>().flush(),
            _ => {}
        }
    });
}

/// Shows the main window where it was last time.
///
/// # What it does
/// The window is created hidden (`tauri.conf.json`), so that it does not
/// jump. It is put in its place (`window_state.rs`), shown, and followed.
fn show_window<R: Runtime>(app: &AppHandle<R>) {
    let path = paths::window_file(app).ok();
    let saved = path.as_deref().and_then(window_state::load);
    let memory = Arc::new(window_state::WindowMemory::new(path, saved));
    app.manage(Arc::clone(&memory));
    let Some(window) = app.get_webview_window(ipc::app::MAIN_WINDOW) else {
        return;
    };
    window_state::restore(&window, saved);
    if let Err(error) = window.show() {
        eprintln!("ganttora: the window could not be shown: {error}");
    }
    window_state::watch(&window, memory);
}

/// Finds (and prepares) the app's `working/` and `recovery/` folders.
///
/// # What it does
/// - `working/`: deletes working copies a crashed run left behind
///   (`paths::clear_stale_working_copies`); `gtra` creates the folder when
///   it first needs it.
/// - `recovery/`: creates it. If that fails, autosave and recovery are
///   switched off (the app still works; the error is logged).
///
/// # Returns
/// The [`state::SessionFolders`] for [`state::AppState::new`].
fn session_folders<R: Runtime>(app: &AppHandle<R>) -> state::SessionFolders {
    let working = paths::working_dir(app).ok();
    if let Some(dir) = &working {
        paths::clear_stale_working_copies(dir);
    }
    let recovery = match paths::recovery_dir(app) {
        Ok(dir) => gtra::RecoveryStore::new(dir)
            .map_err(|error| eprintln!("ganttora: recovery is switched off: {error}"))
            .ok(),
        Err(error) => {
            eprintln!("ganttora: no app data folder, recovery is switched off: {error}");
            None
        }
    };
    state::SessionFolders { working, recovery }
}
