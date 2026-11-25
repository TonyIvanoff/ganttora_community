//! ipc/mod.rs: the commands the user interface can call.
//!
//! # What it is for
//! IPC means "inter-process communication": the React UI and this Rust code
//! run separately and talk by sending messages. The UI calls
//! `invoke("app_info")` (see `frontend/ipc/client.ts`); Tauri finds the Rust
//! function with that name, runs it, turns its return value into JSON and
//! sends it back.
//!
//! # Where it fits
//! `lib.rs` registers every command listed here with `generate_handler!`.
//! - this file: `app_info` (basic facts about the app) and [`IpcError`],
//!   the error every command sends;
//! - `project.rs`: the project commands (`project_new_demo`,
//!   `project_snapshot`, `project_apply`, `project_undo`, `project_redo`),
//!   which call the `document` crate;
//! - `file.rs`: the file commands (`file_new`, `file_open`, `file_save`,
//!   `file_save_as`, `file_autosave`), which use a `gtra::ProjectSession`;
//! - `recovery.rs`: the crash-recovery commands (`recovery_list`,
//!   `recovery_restore`, `recovery_discard`), which use `gtra::RecoveryStore`;
//! - `app.rs`: closing the project and quitting (`app_close_project`,
//!   `app_quit`), used after the "Save / Don't save / Cancel" question;
//! - `clipboard.rs`: Copy / Paste of tasks (`copy_tasks`, `paste_tasks`),
//!   which use `document::clipboard` and the system clipboard;
//! - `settings.rs`: the settings commands (`settings_get`, `settings_set`,
//!   `settings_reset`), which use `crate::settings`.
//!
//! # Main ideas
//! - A command is a normal Rust function marked with `#[tauri::command]`.
//! - Its return type must be *serializable* (turnable into JSON); that is
//!   what `#[derive(Serialize)]` provides.
//! - Field names are sent in camelCase (`#[serde(rename_all = "camelCase")]`),
//!   the usual style in TypeScript.
//! - **Errors carry a code.** Every command fails with an [`IpcError`]: a
//!   stable code such as `"settingsSaveFailed"`, `"rejected"` or
//!   `"gtra.newer_format"`, an English detail for logs, and sometimes
//!   `details` (values for the message, e.g. the task names of a loop). The
//!   UI shows the translated text for the code (`error.<code>` in
//!   `locales/en.json`, helper `frontend/lib/errors.ts`), never the English
//!   detail. Errors from the core crates become an `IpcError` through the
//!   `From` conversions in this file, so a command can simply write `?`
//!   after a call that fails with a `DocumentError` or a `GtraError`.
//!

// `pub mod project;` declares the sub-module in `ipc/project.rs`
// (and `pub mod file;` the one in `ipc/file.rs`, and so on).
pub mod app;
pub mod calendar;
pub mod clipboard;
pub mod file;
pub mod project;
pub mod recovery;
pub mod settings;

use serde::Serialize;
use serde_json::Value;

use crate::state::StateError;

/// Basic facts about the running app, sent to the UI by [`app_info`].
///
/// Must match the TypeScript interface `AppInfo` in
/// `frontend/ipc/client.ts` (until that file's types are generated).
///
/// # Example (as JSON in the UI)
/// ```json
/// { "name": "Ganttora", "version": "0.1.0", "os": "macos" }
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Display name of the product, "Ganttora". (The crate name in
    /// Cargo.toml is the lowercase "ganttora", so we send a constant instead.)
    pub name: String,
    /// App version from the workspace Cargo.toml, e.g. "0.1.0".
    pub version: String,
    /// Operating system family: "macos", "windows" or "linux".
    pub os: String,
}

/// An error sent to the UI: a stable code, an English detail and, for some
/// codes, the values the message needs.
///
/// # JSON shape
/// ```json
/// { "code": "settingsSaveFailed", "detail": "Permission denied (os error 13)" }
///
/// { "code": "rejected",
///   "detail": "change refused: circular dependency: A -> B -> A",
///   "details": { "code": "cycleDetected",
///                "details": { "tasks": ["…", "…"], "names": ["A", "B"] } } }
/// ```
/// `details` is left out when there is nothing to add. Must match the
/// TypeScript type `IpcError` in `frontend/ipc/types.ts`.
///
/// # Why a code and not a message
/// The UI is translated into nine languages; Rust only knows English. So
/// Rust sends *what* went wrong as a code, and the UI picks the sentence in
/// the user's language. `detail` (the operating system's own words, for
/// example) is written to the log for developers.
///
/// # Nested codes
/// A refused edit (`"rejected"`) carries the engine's own error in
/// `details`, which again has a `code` and `details`. The UI walks down to
/// the innermost code (`frontend/lib/errors.ts`) and shows, for example,
/// `error.cycleDetected`. The shell passes the core's JSON along unchanged;
/// it does not interpret it.
///
/// # Example
/// ```
/// use ganttora_lib::ipc::IpcError;
/// let error = IpcError::new("settingsSaveFailed", "disk full");
/// assert_eq!(serde_json::to_string(&error).unwrap(),
///            r#"{"code":"settingsSaveFailed","detail":"disk full"}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    /// Stable error code; the UI's key is `error.<code>`. Shell and
    /// document codes are camelCase (`"noProjectOpen"`); file-format codes
    /// come from `gtra` and contain a dot (`"gtra.newer_format"`), which
    /// i18next reads as a nested key (`error` → `gtra` → `newer_format`).
    pub code: String,
    /// English explanation for logs; may be empty. Never shown as UI text.
    pub detail: String,
    /// Extra values for the message, or `None` (then left out of the JSON).
    ///
    /// # Beginner note
    /// `serde_json::Value` is "any JSON value": an object, a list, text, a
    /// number… `skip_serializing_if` tells serde to leave the field out
    /// entirely when the function named there (`Option::is_none`) says so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl IpcError {
    /// Makes an [`IpcError`] without `details`.
    ///
    /// # Parameters
    /// - `code`: the error code, e.g. `"settingsSaveFailed"`.
    /// - `detail`: extra English text for logs (anything that turns into a
    ///   `String`: a `&str`, a `String`…).
    ///
    /// # Beginner note
    /// `impl Into<String>` accepts any type that can become a `String`, so
    /// callers can pass `"text"` or an existing `String` without converting.
    pub fn new(code: &str, detail: impl Into<String>) -> Self {
        IpcError {
            code: code.to_string(),
            detail: detail.into(),
            details: None,
        }
    }

    /// Returns the same error with `details` filled in.
    ///
    /// # Parameters
    /// - `details`: the values the translated message needs.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::ipc::IpcError;
    /// let error = IpcError::new("gtra.newer_format", "")
    ///     .with_details(serde_json::json!({ "found": 3, "supported": 1 }));
    /// assert_eq!(error.details.unwrap()["found"], 3);
    /// ```
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Error code: no project is open in the window that called.
pub const ERR_NO_PROJECT_OPEN: &str = "noProjectOpen";
/// Error code: the shell's shared state is broken (a poisoned lock).
pub const ERR_PROJECT_UNAVAILABLE: &str = "projectUnavailable";

/// Turns a shell state error into an [`IpcError`].
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::IpcError, state::StateError};
/// let error = IpcError::from(StateError::NoDocument("main".into()));
/// assert_eq!(error.code, "noProjectOpen");
/// ```
///
/// # Beginner note
/// `impl From<A> for B` teaches Rust how to turn an `A` into a `B`. The `?`
/// operator uses it automatically: inside a function that returns
/// `Result<_, IpcError>`, `something_that_fails()?` converts the error for
/// us. That keeps every command short.
impl From<StateError> for IpcError {
    fn from(error: StateError) -> Self {
        let code = match &error {
            StateError::NoDocument(_) => ERR_NO_PROJECT_OPEN,
            StateError::Poisoned => ERR_PROJECT_UNAVAILABLE,
        };
        IpcError::new(code, error.to_string())
    }
}

/// Turns an error from the `document` crate (a refused edit, nothing to
/// undo…) into an [`IpcError`].
///
/// # What it does
/// `DocumentError` already serializes as `{ "code": …, "details": … }`
/// (serde's *adjacently tagged* form). We take that JSON as it is: its
/// `code` becomes our code and its `details` our details, so nothing is
/// lost on the way to the UI.
///
/// # Example
/// ```
/// use ganttora_lib::ipc::IpcError;
/// let error = IpcError::from(document::DocumentError::NothingToUndo);
/// assert_eq!(error.code, "nothingToUndo");
/// assert!(error.details.is_none());
/// ```
impl From<document::DocumentError> for IpcError {
    fn from(error: document::DocumentError) -> Self {
        coded_json_error(&error, error.to_string())
    }
}

/// Turns an error from the `gtra` crate (file not found, newer format…)
/// into an [`IpcError`].
///
/// # What it does
/// Uses `GtraError::code()`, e.g. `"gtra.newer_format"`. For a newer
/// format it also sends the two version numbers, so the message can say
/// "format 3; this version supports up to 2".
///
/// # Example
/// ```
/// use ganttora_lib::ipc::IpcError;
/// let error = IpcError::from(gtra::GtraError::NewerFormat { found: 3, supported: 2 });
/// assert_eq!(error.code, "gtra.newer_format");
/// assert_eq!(error.details.unwrap()["supported"], 2);
/// ```
impl From<gtra::GtraError> for IpcError {
    fn from(error: gtra::GtraError) -> Self {
        let ipc = IpcError::new(error.code(), error.to_string());
        match error {
            gtra::GtraError::NewerFormat { found, supported } => {
                ipc.with_details(serde_json::json!({ "found": found, "supported": supported }))
            }
            _ => ipc,
        }
    }
}

/// Builds an [`IpcError`] from any value that serializes as
/// `{ "code": …, "details": … }` (the core's error enums).
///
/// # Parameters
/// - `error`: the error value; only its JSON form is used.
/// - `detail`: the English message for logs.
///
/// # Returns
/// The [`IpcError`]. If the JSON unexpectedly has no text `code`, the code
/// is `"unknown"` (the UI then shows its general message).
///
/// # Beginner note
/// `&impl Serialize` means "a reference to any value that serde can turn
/// into JSON". `serde_json::to_value` builds the JSON as a tree of
/// `Value`s instead of text, so we can pick fields out of it.
pub(crate) fn coded_json_error(error: &impl Serialize, detail: String) -> IpcError {
    let json = serde_json::to_value(error).unwrap_or(Value::Null);
    let code = json
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    IpcError {
        code,
        detail,
        details: json.get("details").cloned(),
    }
}

/// Display name of the product. A proper name, so it is not translated.
const PRODUCT_NAME: &str = "Ganttora";

/// Returns basic information about the app. The M0 demo command.
///
/// # Returns
/// An [`AppInfo`] with the product name, the version and the OS family.
///
/// # Errors
/// None: this command cannot fail.
///
/// # Example (from the UI, TypeScript)
/// ```ts
/// const info = await invoke<AppInfo>("app_info");
/// ```
///
/// # Beginner note
/// `env!("CARGO_PKG_VERSION")` is filled in by the compiler with the version
/// from Cargo.toml, so the version is written in only one place.
/// `std::env::consts::OS` is a constant the standard library sets for the
/// system the program was compiled for.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: PRODUCT_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
    }
}
