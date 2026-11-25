//! paths.rs: where Ganttora keeps its own files on each operating system.
//!
//! # What it is for
//! Settings, the recent-files list and crash-recovery copies must live in
//! the folder each operating system reserves for app data:
//!
//! | OS      | Config folder (example)                              |
//! |---------|------------------------------------------------------|
//! | Windows | `C:\Users\Ana\AppData\Roaming\app.ganttora.desktop`  |
//! | macOS   | `/Users/ana/Library/Application Support/app.ganttora.desktop` |
//! | Linux   | `/home/ana/.config/app.ganttora.desktop`             |
//!
//! Tauri already knows these folders; this module gives them names that
//! fit our app, so the rest of the code never builds such paths by hand.
//!
//! # Where it fits
//! `lib.rs` uses [`settings_file`] at start-up to find `settings.json`
//! (read by `settings.rs`), and [`working_dir`] / [`recovery_dir`] for the
//! `.gtra` sessions (`state.rs`).
//!

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

/// Name of the settings file inside the config folder.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Returns the folder for Ganttora's configuration (settings.json).
///
/// # Parameters
/// - `app`: a handle to the running app; Tauri gives one to every command.
///
/// # Returns
/// `Ok(path)` with the folder path. The folder may not exist yet; the caller
/// creates it before writing.
///
/// # Errors
/// Returns an error if the operating system does not report a config folder
/// (very unusual, e.g. a missing home directory).
///
/// # Beginner note
/// `<R: Runtime>` makes the function *generic*: it works with any Tauri
/// runtime type `R` (the real one, or a mock one in tests). `Result<T, E>`
/// is either `Ok(value)` or `Err(error)`; the caller must handle both.
pub fn config_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    app.path().app_config_dir()
}

/// Returns the full path of `settings.json`.
///
/// # Parameters
/// - `app`: a handle to the running app.
///
/// # Returns
/// `Ok(path)`, e.g. `.../app.ganttora.desktop/settings.json`.
///
/// # Errors
/// Same as [`config_dir`].
pub fn settings_file<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    // The `?` means: if `config_dir` returned an error, return that error
    // from this function right away; otherwise continue with the value.
    Ok(config_dir(app)?.join(SETTINGS_FILE_NAME))
}

/// Name of the file with the window's position (`window_state.rs`).
pub const WINDOW_FILE_NAME: &str = "window.json";

/// Returns the full path of `window.json`.
///
/// # Parameters
/// - `app`: a handle to the running app.
///
/// # Errors
/// Same as [`config_dir`].
pub fn window_file<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(config_dir(app)?.join(WINDOW_FILE_NAME))
}

/// Returns the folder for app data that is not settings (recovery copies,
/// caches).
///
/// # Parameters
/// - `app`: a handle to the running app.
///
/// # Returns
/// `Ok(path)` with the folder path (it may not exist yet).
///
/// # Errors
/// Returns an error if the operating system does not report a data folder.
pub fn data_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    app.path().app_data_dir()
}

/// Name of the folder (inside the data folder) for working copies.
pub const WORKING_DIR_NAME: &str = "working";

/// Name of the folder (inside the data folder) for recovery copies.
pub const RECOVERY_DIR_NAME: &str = "recovery";

/// Start of every working-copy file name (set by `gtra`:
/// `ganttora-working-<uuid>.gtra`).
pub const WORKING_COPY_PREFIX: &str = "ganttora-working-";

/// Returns `<data folder>/working`: where open projects keep their working
/// copies (see `gtra::ProjectSession`).
///
/// # Errors
/// Same as [`data_dir`].
pub fn working_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(data_dir(app)?.join(WORKING_DIR_NAME))
}

/// Returns `<data folder>/recovery`: where autosave writes recovery copies.
///
/// # Errors
/// Same as [`data_dir`].
pub fn recovery_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(data_dir(app)?.join(RECOVERY_DIR_NAME))
}

/// Deletes working copies left behind by an earlier run that crashed.
///
/// # What it does
/// A working copy is only needed while its project is open. After a crash
/// it stays behind, but it is useless then: the project's unsaved work is
/// in the *recovery* copy, not here. So at start-up, before any project is
/// opened, every file in `dir` whose name starts with
/// [`WORKING_COPY_PREFIX`] (also SQLite's `-journal` files next to them) is
/// deleted. Other files are left alone.
///
/// Only safe because a single Ganttora runs at a time (the single-instance
/// plugin in `lib.rs`); otherwise this could delete another window's copy.
///
/// # Parameters
/// - `dir`: the working folder ([`working_dir`]).
///
/// # Returns
/// How many files were deleted. A missing folder gives 0.
///
/// # Errors
/// None returned: a file that cannot be deleted is skipped (the next start
/// tries again).
///
/// # Example
/// ```
/// use ganttora_lib::paths::clear_stale_working_copies;
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("ganttora-working-1.gtra"), b"x").unwrap();
/// std::fs::write(dir.join("notes.txt"), b"x").unwrap();
/// assert_eq!(clear_stale_working_copies(&dir), 1);
/// assert!(dir.join("notes.txt").exists());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn clear_stale_working_copies(dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        // `flatten` skips entries the system could not read.
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(WORKING_COPY_PREFIX)
        })
        .filter(|entry| std::fs::remove_file(entry.path()).is_ok())
        .count()
}
