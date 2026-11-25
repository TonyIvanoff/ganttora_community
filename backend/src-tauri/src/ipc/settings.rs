//! ipc/settings.rs: the IPC commands that read and change the app settings.
//!
//! # What it is for
//! The Settings dialog (`frontend/features/settings/`) needs to read the
//! current settings and store the user's changes. These commands connect it
//! with [`SettingsState`] in `settings.rs`, which owns the file.
//!
//! | Command (name used in `invoke`) | What it does |
//! |---|---|
//! | `settings_get`   | returns the current settings and a start-up notice, if any |
//! | `settings_set`   | cleans, saves and returns new settings |
//! | `settings_reset` | saves and returns the default settings |
//!
//! # Where it fits
//! ```text
//!   UI Settings dialog ─invoke("settings_set", { settings })─► settings_set
//!       ─► SettingsState::update ─► settings::save_to (temp file + rename)
//! ```
//!
//! # Main ideas
//! - The UI always sends the **whole** settings object, and gets back the
//!   cleaned version that was stored. It then shows exactly what Rust
//!   stored, never its own guess.
//! - Errors are sent as an [`IpcError`] with a stable **code**
//!   (`settingsSaveFailed`, …). The UI turns the code into a translated
//!   message (`error.<code>` in `locales/en.json`); the English `detail`
//!   is only for logs.
//!

use serde::Serialize;
use tauri::State;

use crate::ipc::IpcError;
use crate::settings::{Settings, SettingsError, SettingsNotice, SettingsState};

/// Error code: the settings file could not be written.
pub const ERR_SETTINGS_SAVE_FAILED: &str = "settingsSaveFailed";
/// Error code: there is no config folder, so settings last only until the app closes.
pub const ERR_SETTINGS_NOT_STORED: &str = "settingsNotStored";
/// Error code: the shell's settings state is unusable (a crash while it was locked).
pub const ERR_SETTINGS_UNAVAILABLE: &str = "settingsUnavailable";

/// The answer of `settings_get`.
///
/// # JSON shape
/// ```json
/// { "settings": { "version": 1, "general": { "language": "en", … }, … },
///   "notice": null }
/// ```
/// Must match the TypeScript type `SettingsResponse` in `frontend/ipc/types.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    /// The current settings.
    pub settings: Settings,
    /// A problem found when the file was loaded at start-up, or `null`.
    pub notice: Option<SettingsNotice>,
}

/// Turns a [`SettingsError`] into the coded error the UI understands.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::settings::to_ipc_error, settings::SettingsError};
/// assert_eq!(to_ipc_error(SettingsError::NoConfigFolder).code, "settingsNotStored");
/// ```
pub fn to_ipc_error(error: SettingsError) -> IpcError {
    match error {
        SettingsError::NoConfigFolder => IpcError::new(ERR_SETTINGS_NOT_STORED, ""),
        SettingsError::Write(detail) => IpcError::new(ERR_SETTINGS_SAVE_FAILED, detail),
        SettingsError::Poisoned => IpcError::new(ERR_SETTINGS_UNAVAILABLE, ""),
    }
}

/// Returns the current settings and the start-up notice.
///
/// # Errors
/// An [`IpcError`] with code `settingsUnavailable` if the state is broken.
///
/// # Example (from the UI, TypeScript)
/// ```ts
/// const { settings, notice } = await invoke<SettingsResponse>("settings_get");
/// ```
#[tauri::command]
pub fn settings_get(state: State<'_, SettingsState>) -> Result<SettingsResponse, IpcError> {
    let loaded = state.current().map_err(to_ipc_error)?;
    Ok(SettingsResponse {
        settings: loaded.settings,
        notice: loaded.notice,
    })
}

/// Cleans, stores and returns new settings.
///
/// # Parameters
/// - `settings`: the whole settings object from the UI. Unknown or invalid
///   values are replaced by defaults (see `Settings::sanitize`).
///
/// # Returns
/// The settings as stored.
///
/// # Errors
/// An [`IpcError`] with code `settingsSaveFailed`, `settingsNotStored` or
/// `settingsUnavailable`.
///
/// # Example (from the UI, TypeScript)
/// ```ts
/// const stored = await invoke<Settings>("settings_set", { settings });
/// ```
///
/// # Beginner note
/// Tauri reads the `settings` argument from the JSON the UI sent, using the
/// same `Deserialize` code that reads `settings.json`, so the same lenient
/// rules apply.
#[tauri::command]
pub fn settings_set(
    state: State<'_, SettingsState>,
    settings: Settings,
) -> Result<Settings, IpcError> {
    state.update(settings).map_err(to_ipc_error)
}

/// Stores and returns the default settings ("Reset to defaults").
///
/// # Errors
/// Same as [`settings_set`].
///
/// # Example (from the UI, TypeScript)
/// ```ts
/// const defaults = await invoke<Settings>("settings_reset");
/// ```
#[tauri::command]
pub fn settings_reset(state: State<'_, SettingsState>) -> Result<Settings, IpcError> {
    state.update(Settings::default()).map_err(to_ipc_error)
}
