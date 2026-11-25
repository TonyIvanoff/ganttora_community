//! settings.rs: the user's app settings, stored in `settings.json`.
//!
//! # What it is for
//! Settings are choices that belong to the *user*, not to one project: the
//! language, the theme, the name written into the change journal, default
//! working hours, how often to autosave and so on. They are
//! kept in one small JSON file, `settings.json`, in the operating system's
//! config folder (see `paths.rs`):
//!
//! ```text
//! macOS    ~/Library/Application Support/app.ganttora.desktop/settings.json
//! Windows  %APPDATA%\app.ganttora.desktop\settings.json
//! Linux    ~/.config/app.ganttora.desktop/settings.json
//! ```
//!
//! # Where it fits
//! - `lib.rs` calls [`SettingsState::load`] once at start-up and gives the
//!   result to Tauri with `app.manage(...)`.
//! - The IPC commands in `ipc/settings.rs` read and change the settings for
//!   the UI (`frontend/features/settings/`).
//!
//! # Main ideas
//! - **Defaults live here.** [`Settings::default`] is the single place that
//!   says what a fresh install looks like (language `"en"`, theme
//!   "system"…). A missing file simply means "all defaults".
//! - **Never fail to start because of settings.** A damaged file (not JSON,
//!   cut off in the middle, edited by hand) must not stop the app. We then
//!   use the defaults, *move the bad file aside* as a backup (so nothing the
//!   user wrote is lost) and tell the UI with a [`SettingsNotice`].
//! - **One bad value does not throw away the rest.** Every field is read
//!   with [`lenient`]: if one value has the wrong type (for example
//!   `"theme": 42`), only that field falls back to its default.
//! - **Unknown values are cleaned up** by [`Settings::sanitize`] (numbers out
//!   of range, a currency code that is not three letters…).
//! - **Atomic save.** We write the whole file to a temporary file next to
//!   it and then *rename* it over the old one ([`save_to`]). A rename is
//!   all-or-nothing, so a crash or power cut during saving leaves either the
//!   old file or the new one, never half a file.
//! - JSON field names are camelCase and enum values are camelCase strings
//!   (`"fixedUnits"`), the usual style in TypeScript.
//!

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};

/// The version of the settings file layout written by this app.
///
/// Stored as `"version"` in the file. If a later app changes the meaning of
/// a field, it raises this number and can convert older files.
pub const SETTINGS_VERSION: u32 = 1;

/// The language used when none is set (the default is English).
pub const DEFAULT_LANGUAGE: &str = "en";

/// `dateFormat` value meaning "follow the operating system's regional format".
pub const DATE_FORMAT_SYSTEM: &str = "system";

/// `dateFormat` value meaning "always write dates as ISO 8601 (2027-03-24)".
pub const DATE_FORMAT_ISO: &str = "iso";

/// The other date patterns ("mmm" = the month's name).
///
/// Must match `DATE_PATTERNS` in `frontend/i18n/format.ts`.
///
/// # Example
/// ```
/// use ganttora_lib::settings::{DATE_PATTERNS, Settings};
/// let mut settings = Settings::default();
/// for pattern in DATE_PATTERNS {
///     settings.general.date_format = pattern.to_string();
///     settings = settings.sanitize();
///     assert_eq!(settings.general.date_format, pattern);
/// }
/// settings.general.date_format = "banana split".to_string();
/// settings.general.ui_zoom_percent = 900;
/// let settings = settings.sanitize();
/// assert_eq!(settings.general.date_format, "system");
/// assert_eq!(settings.general.ui_zoom_percent, 150); // the largest UI zoom
/// ```
pub const DATE_PATTERNS: [&str; 10] = [
    "yyyy mmm dd",
    "yyyy/mm/dd",
    "yyyy.mm.dd",
    "dd.mm.yyyy",
    "dd/mm/yyyy",
    "dd-mm-yyyy",
    "dd mmm yyyy",
    "mm/dd/yyyy",
    "mmm/dd/yyyy",
    "mmm dd, yyyy",
];

/// Default currency (ISO 4217 code). The garden-shed sample is priced in euros.
pub const DEFAULT_CURRENCY: &str = "EUR";

/// Default calendar for new projects ("Standard": Mon–Fri, 8 h a day).
pub const DEFAULT_CALENDAR: &str = "standard";

/// Narrowest a task-grid column may be, in CSS pixels.
///
/// Must match `MIN_COLUMN_WIDTH` in `frontend/features/grid/columnWidths.ts`.
pub const MIN_COLUMN_WIDTH: u32 = 24;

/// Widest a task-grid column may be, in CSS pixels.
///
/// Must match `MAX_COLUMN_WIDTH` in `frontend/features/grid/columnWidths.ts`.
pub const MAX_COLUMN_WIDTH: u32 = 1200;

/// Most column widths kept in the file. A real grid has a few dozen columns
/// at most; the limit only stops a hand-edited file from growing without end.
pub const MAX_COLUMN_WIDTH_ENTRIES: usize = 200;

/// Most toolbars kept in the file (must match `frontend/features/toolbars/layout.ts`).
pub const MAX_TOOLBARS: usize = 30;

/// Most entries on one toolbar (must match `layout.ts`).
pub const MAX_TOOLBAR_ITEMS: usize = 60;

/// Longest name of a custom toolbar, in characters (must match `layout.ts`).
pub const MAX_TOOLBAR_NAME: usize = 40;
/// Most rows of toolbars at the top and at the bottom edge.
pub const MAX_TOOLBAR_ROWS: u8 = 3;

/// Largest coordinate of a floating toolbar, in CSS pixels. Only stops
/// absurd values; the UI keeps toolbars inside the window.
pub const MAX_TOOLBAR_POSITION: f64 = 10_000.0;

/// The task grid's columns and their default widths in CSS pixels.
///
/// The column names are the grid's column IDs (`GridColumnId` in
/// `frontend/features/grid/cellEdit.ts`). Must match
/// `DEFAULT_COLUMN_WIDTHS` in `frontend/features/grid/columnWidths.ts`.
pub const DEFAULT_COLUMN_WIDTHS: [(&str, u32); 16] = [
    ("rowNumber", 44),
    ("indicators", 164),
    ("outline", 56),
    ("name", 240),
    ("duration", 84),
    ("complete", 80),
    ("start", 136),
    ("finish", 136),
    ("plannedFinish", 136),
    ("actualFinish", 136),
    ("constraintType", 176),
    ("constraintDate", 136),
    ("deadline", 136),
    ("calendar", 136),
    ("predecessors", 120),
    ("mode", 84),
];

// ---------------------------------------------------------------------------
// The settings types
// ---------------------------------------------------------------------------

/// All app settings: the content of `settings.json`.
///
/// # JSON shape (with the default values)
/// ```json
/// {
///   "version": 1,
///   "user": { "displayName": "", "initials": "", "email": "" },
///   "general": { "language": "en", "theme": "system", "dateFormat": "system",
///                "firstDayOfWeek": "system", "currency": "EUR",
///                "uiZoomPercent": 100 },
///   "scheduling": { "newTaskMode": "auto", "defaultTaskType": "fixedUnits",
///                   "effortDriven": false, "hoursPerDay": 8.0, "hoursPerWeek": 40.0,
///                   "defaultCalendar": "standard" },
///   "files": { "defaultSaveFolder": null, "autosaveMinutes": 5,
///              "historyRetentionDays": 90, "recentFilesMax": 10 },
///   "updates": { "autoUpdate": true, "crashReports": false },
///   "grid": { "columnWidths": { "rowNumber": 44, "indicators": 164, "outline": 56,
///             "name": 240, "duration": 84, "start": 136, "finish": 136,
///             "plannedFinish": 136, "actualFinish": 136, "constraintType": 176,
///             "constraintDate": 136, "deadline": 136, "calendar": 136,
///             "complete": 80, "predecessors": 120, "mode": 84 } },
///   "toolbars": { "layout": [] },
///   "view": { "timescale": "days", "zoomPercent": 100 }
/// }
/// ```
/// Must match the TypeScript type `Settings` in `frontend/ipc/types.ts`.
///
/// # Beginner note
/// `#[serde(default)]` on the struct means: if a field is missing in the
/// JSON, take it from `Settings::default()`. That makes old or hand-written
/// files with fewer fields load without errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Layout version of the file, see [`SETTINGS_VERSION`].
    #[serde(deserialize_with = "lenient")]
    pub version: u32,
    /// Who the user is (used in the change journal).
    #[serde(deserialize_with = "lenient")]
    pub user: UserSettings,
    /// Language, theme and regional formats.
    #[serde(deserialize_with = "lenient")]
    pub general: GeneralSettings,
    /// Defaults for new tasks and projects.
    #[serde(deserialize_with = "lenient")]
    pub scheduling: SchedulingSettings,
    /// Saving, autosave, version history and recent files.
    #[serde(deserialize_with = "lenient")]
    pub files: FileSettings,
    /// Automatic updates and crash reports.
    #[serde(deserialize_with = "lenient")]
    pub updates: UpdateSettings,
    /// The task grid's layout (column widths). Not shown in the Settings
    /// dialog: it changes when the user drags a column edge.
    #[serde(deserialize_with = "lenient")]
    pub grid: GridSettings,
    /// The toolbars and where they sit. Not shown in the Settings dialog:
    /// it changes when the user drags or customizes a toolbar.
    #[serde(deserialize_with = "lenient")]
    pub toolbars: ToolbarSettings,
    /// The Gantt chart's zoom. Not shown in the Settings dialog: it changes
    /// when the user zooms.
    #[serde(deserialize_with = "lenient")]
    pub view: ViewSettings,
}

/// The "User" group: who is working with the app.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserSettings {
    /// Full name, e.g. "Sam Lee". Empty means "use the computer's user name".
    #[serde(deserialize_with = "lenient")]
    pub display_name: String,
    /// Short initials, e.g. "SL" (at most 4 characters).
    #[serde(deserialize_with = "lenient")]
    pub initials: String,
    /// Optional e-mail address. Never sent anywhere; only stored.
    #[serde(deserialize_with = "lenient")]
    pub email: String,
}

/// Light, dark, or follow the operating system.
///
/// # Beginner note
/// An `enum` is a type that is exactly one of a fixed list of values.
/// `rename_all = "camelCase"` writes them as `"light"`, `"dark"`, `"system"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Always the light theme.
    Light,
    /// Always the dark theme.
    Dark,
    /// Follow the operating system's light/dark setting (the default).
    #[default]
    System,
}

/// Which day a week starts on in calendars and the Gantt timescale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstDayOfWeek {
    /// Use the operating system's regional setting (the default).
    #[default]
    System,
    /// Weeks start on Monday (most of Europe, ISO 8601).
    Monday,
    /// Weeks start on Sunday (USA, Canada, Japan…).
    Sunday,
    /// Weeks start on Saturday (parts of the Middle East).
    Saturday,
}

/// The "General" group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    /// UI language code, e.g. `"en"`, `"de"`, `"zh-CN"`.
    #[serde(deserialize_with = "lenient")]
    pub language: String,
    /// Light, dark or system.
    #[serde(deserialize_with = "lenient")]
    pub theme: Theme,
    /// How dates are written: `"system"` (the OS region's pattern, the
    /// default), `"iso"` (2027-03-24) or one of [`DATE_PATTERNS`]. A locale
    /// code such as `"de-DE"`, saved by older versions, is kept: the UI uses
    /// its pattern.
    #[serde(deserialize_with = "lenient")]
    pub date_format: String,
    /// First day of the week.
    #[serde(deserialize_with = "lenient")]
    pub first_day_of_week: FirstDayOfWeek,
    /// Currency for costs, as an ISO 4217 code such as `"EUR"` or `"USD"`.
    #[serde(deserialize_with = "lenient")]
    pub currency: String,
    /// Zoom of the whole UI in percent, 80–150 (must match `lib/uiZoom.ts`).
    #[serde(deserialize_with = "lenient")]
    pub ui_zoom_percent: u32,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings {
            language: DEFAULT_LANGUAGE.to_string(),
            theme: Theme::System,
            date_format: DATE_FORMAT_SYSTEM.to_string(),
            first_day_of_week: FirstDayOfWeek::System,
            currency: DEFAULT_CURRENCY.to_string(),
            ui_zoom_percent: 100,
        }
    }
}

/// Whether new tasks are scheduled by the engine or placed by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskMode {
    /// The engine calculates the dates (the default).
    #[default]
    Auto,
    /// The user types the dates; the engine leaves them alone.
    Manual,
}

/// Which of duration, work and units stays fixed when one of them changes
/// (the same three task types as MS Project).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskType {
    /// Units stay fixed (MS Project's default).
    #[default]
    FixedUnits,
    /// Duration stays fixed.
    FixedDuration,
    /// Work stays fixed.
    FixedWork,
}

/// The "Scheduling defaults" group: used when new tasks or projects are made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SchedulingSettings {
    /// Auto- or manually scheduled new tasks.
    #[serde(deserialize_with = "lenient")]
    pub new_task_mode: TaskMode,
    /// Task type of new tasks.
    #[serde(deserialize_with = "lenient")]
    pub default_task_type: TaskType,
    /// Whether new tasks are effort-driven.
    #[serde(deserialize_with = "lenient")]
    pub effort_driven: bool,
    /// Working hours in one day (0.5–24). "1d" means this many hours.
    #[serde(deserialize_with = "lenient")]
    pub hours_per_day: f64,
    /// Working hours in one week (1–168). "1w" means this many hours.
    #[serde(deserialize_with = "lenient")]
    pub hours_per_week: f64,
    /// Id of the calendar new projects use, e.g. `"standard"`.
    #[serde(deserialize_with = "lenient")]
    pub default_calendar: String,
}

impl Default for SchedulingSettings {
    fn default() -> Self {
        SchedulingSettings {
            new_task_mode: TaskMode::Auto,
            default_task_type: TaskType::FixedUnits,
            effort_driven: false,
            hours_per_day: 8.0,
            hours_per_week: 40.0,
            default_calendar: DEFAULT_CALENDAR.to_string(),
        }
    }
}

/// The "Files" group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileSettings {
    /// Folder the Save As window starts in; `null` = the system's choice.
    #[serde(deserialize_with = "lenient")]
    pub default_save_folder: Option<String>,
    /// Minutes between autosaves of the recovery copy (0 = off, max 120).
    #[serde(deserialize_with = "lenient")]
    pub autosave_minutes: u32,
    /// Days of detailed version history to keep (1–3650).
    #[serde(deserialize_with = "lenient")]
    pub history_retention_days: u32,
    /// How many recent files to list (0–50).
    #[serde(deserialize_with = "lenient")]
    pub recent_files_max: u32,
}

impl Default for FileSettings {
    fn default() -> Self {
        FileSettings {
            default_save_folder: None,
            autosave_minutes: 5,
            history_retention_days: 90,
            recent_files_max: 10,
        }
    }
}

/// The "Updates & privacy" group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Check for and install updates automatically (can be turned off).
    #[serde(deserialize_with = "lenient")]
    pub auto_update: bool,
    /// Send crash reports. Off unless the user opts in (privacy).
    #[serde(deserialize_with = "lenient")]
    pub crash_reports: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            auto_update: true,
            crash_reports: false,
        }
    }
}

/// The "Grid" group: how the task grid is laid out.
///
/// # JSON shape
/// ```json
/// { "columnWidths": { "name": 300, "duration": 84 } }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GridSettings {
    /// Width of each column in CSS pixels, by column ID (e.g. `"name"`).
    /// A column missing here uses its default width in the UI. Read with
    /// [`lenient_column_widths`], so one bad entry drops only that entry.
    ///
    /// # Beginner note
    /// A `BTreeMap` is a dictionary (key → value) that keeps its keys
    /// sorted, so the file always lists the columns in the same order.
    #[serde(deserialize_with = "lenient_column_widths")]
    pub column_widths: BTreeMap<String, u32>,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            column_widths: DEFAULT_COLUMN_WIDTHS
                .iter()
                .map(|(id, width)| ((*id).to_string(), *width))
                .collect(),
        }
    }
}

/// Where a toolbar sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolbarDock {
    /// Along the top edge (the default).
    #[default]
    Top,
    /// Along the bottom edge.
    Bottom,
    /// Along the left edge, buttons below each other.
    Left,
    /// Along the right edge, buttons below each other.
    Right,
    /// In its own little window inside the main window.
    Float,
}

/// One toolbar of the layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolbarLayout {
    /// `"main"`, `"task"`, `"view"`, `"legend"` (predefined) or a custom toolbar's ID.
    #[serde(deserialize_with = "lenient")]
    pub id: String,
    /// Name of a custom toolbar; empty for a predefined one.
    #[serde(deserialize_with = "lenient")]
    pub name: String,
    /// Action IDs and `"separator"`, in order. Entries that are not text
    /// are dropped; IDs the UI does not know are ignored by the UI.
    #[serde(deserialize_with = "lenient_items")]
    pub items: Vec<String>,
    /// Edge or floating.
    #[serde(deserialize_with = "lenient")]
    pub dock: ToolbarDock,
    /// Row at the top or bottom edge (0 = uppermost), below [`MAX_TOOLBAR_ROWS`].
    #[serde(deserialize_with = "lenient")]
    pub row: u8,
    /// Left edge while floating, in CSS pixels.
    #[serde(deserialize_with = "lenient")]
    pub x: f64,
    /// Top edge while floating, in CSS pixels.
    #[serde(deserialize_with = "lenient")]
    pub y: f64,
    /// Shown (the default, also when the value is unusable) or hidden.
    #[serde(deserialize_with = "lenient_true")]
    pub visible: bool,
}

impl Default for ToolbarLayout {
    fn default() -> Self {
        ToolbarLayout {
            id: String::new(),
            name: String::new(),
            items: Vec::new(),
            dock: ToolbarDock::Top,
            row: 0,
            x: 0.0,
            y: 0.0,
            visible: true,
        }
    }
}

/// The "Toolbars" group.
///
/// # JSON shape
/// ```json
/// { "layout": [ { "id": "main", "name": "", "items": ["fileSave", "separator", "undo"],
///                 "dock": "top", "x": 0.0, "y": 0.0, "visible": true } ] }
/// ```
/// An empty layout (the default) means "the predefined toolbars". Which
/// buttons they hold is the UI's business (`features/toolbars/layout.ts`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolbarSettings {
    /// The toolbars in display order. Read with [`lenient_toolbars`], so one
    /// bad entry drops only that entry.
    #[serde(deserialize_with = "lenient_toolbars")]
    pub layout: Vec<ToolbarLayout>,
}

/// The units of the Gantt chart's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Timescale {
    /// Days under months (the default).
    #[default]
    Days,
    /// Weeks under months.
    Weeks,
    /// Months under years.
    Months,
}

/// The "View" group: how the Gantt chart is zoomed.
///
/// # Example
/// ```
/// use ganttora_lib::settings::{Settings, Timescale};
/// let s: Settings = serde_json::from_str(
///     r#"{ "view": { "timescale": "weeks", "zoomPercent": 9999 } }"#,
/// ).unwrap();
/// let s = s.sanitize();
/// assert_eq!(s.view.timescale, Timescale::Weeks);
/// assert_eq!(s.view.zoom_percent, 400); // the largest zoom
///
/// let s: Settings = serde_json::from_str(r#"{ "view": { "zoomPercent": "big" } }"#).unwrap();
/// assert_eq!(s.sanitize().view.zoom_percent, 100); // unusable → default
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewSettings {
    /// Header units.
    #[serde(deserialize_with = "lenient")]
    pub timescale: Timescale,
    /// Zoom in percent, 10–400 (must match `renderer/timeScale.ts`).
    #[serde(deserialize_with = "lenient")]
    pub zoom_percent: u32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            timescale: Timescale::default(),
            zoom_percent: 100,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            user: UserSettings::default(),
            general: GeneralSettings::default(),
            scheduling: SchedulingSettings::default(),
            files: FileSettings::default(),
            updates: UpdateSettings::default(),
            grid: GridSettings::default(),
            toolbars: ToolbarSettings::default(),
            view: ViewSettings::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Reading values leniently and cleaning them up
// ---------------------------------------------------------------------------

/// Reads one field, and uses the type's default if the value is unusable.
///
/// # What it does
/// First reads the value as *any* JSON (`serde_json::Value`), which always
/// works for valid JSON. Then tries to turn it into the wanted type `T`. If
/// that fails (wrong type, unknown enum name), the field gets
/// `T::default()` instead of making the whole file fail.
///
/// # Parameters
/// - `deserializer`: provided by serde; it holds the field's JSON value.
///
/// # Returns
/// The value, or `T::default()` if it could not be read.
///
/// # Errors
/// Only if the input is not JSON at all (cannot happen for a field inside
/// a JSON object that was already parsed).
///
/// # Example
/// ```
/// use ganttora_lib::settings::{Settings, Theme};
/// let s: Settings = serde_json::from_str(r#"{ "general": { "theme": 42, "language": "de" } }"#).unwrap();
/// assert_eq!(s.general.theme, Theme::System); // 42 is not a theme → default
/// assert_eq!(s.general.language, "de");        // the good value is kept
/// ```
///
/// # Beginner note
/// `<'de, D, T>` are *generic parameters*: placeholders filled in by the
/// compiler for each field (`T` = `String`, `Theme`, `u32`…). `'de` is a
/// *lifetime*: it says how long borrowed input text lives. The `where`
/// clause lists what `T` must be able to do: be read from JSON and have a
/// default.
pub fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Reads the column-width map entry by entry, keeping the usable ones.
///
/// # What it does
/// The field must be a JSON object such as `{ "name": 300 }`. Each entry
/// whose value is a non-negative number is kept (a fraction is rounded);
/// an entry with any other value (`"wide"`, `null`, `-5`) is left out. If
/// the field is not an object at all, the default widths are used.
/// Out-of-range widths are fixed later by [`Settings::sanitize`].
///
/// # Parameters
/// - `deserializer`: provided by serde; it holds the field's JSON value.
///
/// # Returns
/// The widths that could be read.
///
/// # Errors
/// Only if the input is not JSON at all (cannot happen inside a parsed file).
///
/// # Example
/// ```
/// use ganttora_lib::settings::Settings;
/// let s: Settings = serde_json::from_str(
///     r#"{ "grid": { "columnWidths": { "name": 300, "start": "wide" } } }"#,
/// ).unwrap();
/// assert_eq!(s.grid.column_widths.get("name"), Some(&300));
/// assert_eq!(s.grid.column_widths.get("start"), None); // bad value dropped
/// ```
pub fn lenient_column_widths<'de, D>(deserializer: D) -> Result<BTreeMap<String, u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(object) = value.as_object() else {
        return Ok(GridSettings::default().column_widths);
    };
    Ok(object
        .iter()
        .filter_map(|(id, width)| {
            // `as_f64` accepts whole numbers and fractions alike.
            let width = width.as_f64().filter(|w| w.is_finite() && *w >= 0.0)?;
            // `as u32` saturates: anything above u32::MAX becomes u32::MAX,
            // which sanitize then brings down to MAX_COLUMN_WIDTH.
            Some((id.clone(), width.round() as u32))
        })
        .collect())
}

/// Reads a yes/no value; anything unusable counts as `true`.
///
/// Used for a toolbar's `visible`: when in doubt, show it.
///
/// # Errors
/// Only if the input is not JSON at all.
fn lenient_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(serde_json::Value::deserialize(deserializer)?
        .as_bool()
        .unwrap_or(true))
}

/// Reads a list of texts, leaving out entries that are not text.
///
/// # Errors
/// Only if the input is not JSON at all.
fn lenient_items<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect())
}

/// Reads the toolbar layout entry by entry, keeping the usable ones.
///
/// # What it does
/// Not a list at all → empty (the UI then uses the predefined toolbars).
/// Each entry must be a JSON object; its fields are read leniently.
///
/// # Errors
/// Only if the input is not JSON at all.
///
/// # Example
/// ```
/// use ganttora_lib::settings::{Settings, ToolbarDock};
/// let s: Settings = serde_json::from_str(r#"{ "toolbars": { "layout": [
///     42,
///     { "id": "main", "dock": "left", "items": ["fileSave", 7], "visible": "maybe" }
/// ] } }"#).unwrap();
/// assert_eq!(s.toolbars.layout.len(), 1); // 42 is not a toolbar
/// let main = &s.toolbars.layout[0];
/// assert_eq!(main.dock, ToolbarDock::Left);
/// assert_eq!(main.items, ["fileSave"]); // 7 is not an action ID
/// assert!(main.visible); // unusable → shown
///
/// let s: Settings = serde_json::from_str(r#"{ "toolbars": "none" }"#).unwrap();
/// assert!(s.toolbars.layout.is_empty());
/// ```
pub fn lenient_toolbars<'de, D>(deserializer: D) -> Result<Vec<ToolbarLayout>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let entries = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(entries
        .iter()
        .filter(|entry| entry.is_object())
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect())
}

/// Cleans a toolbar layout: see [`Settings::sanitize`].
///
/// # Example
/// ```
/// use ganttora_lib::settings::{sanitize_toolbars, ToolbarLayout, MAX_TOOLBAR_NAME};
/// let bar = |id: &str| ToolbarLayout { id: id.into(), ..Default::default() };
/// let mut wild = bar("mine");
/// wild.name = " My bar ".into();
/// wild.x = -5.0;
/// wild.row = 9;
/// wild.items = vec!["fileSave".into(), "not an id!".into()];
/// let clean = sanitize_toolbars(vec![bar("main"), bar("main"), bar("no id!"), wild]);
/// assert_eq!(clean.len(), 2); // the repeat and the bad ID are gone
/// assert_eq!((clean[1].name.as_str(), clean[1].x), ("My bar", 0.0));
/// assert_eq!(clean[1].row, 2); // at most three rows
/// assert_eq!(clean[1].items, ["fileSave"]);
/// assert_eq!(MAX_TOOLBAR_NAME, 40);
/// ```
pub fn sanitize_toolbars(layout: Vec<ToolbarLayout>) -> Vec<ToolbarLayout> {
    // A *set* remembers which IDs we have seen; `insert` answers `false`
    // for one that is already in it.
    let mut seen = std::collections::BTreeSet::new();
    layout
        .into_iter()
        .filter(|bar| is_column_id(&bar.id) && seen.insert(bar.id.clone()))
        .take(MAX_TOOLBARS)
        .map(|mut bar| {
            bar.name = bar.name.trim().chars().take(MAX_TOOLBAR_NAME).collect();
            bar.items.retain(|item| is_column_id(item));
            bar.items.truncate(MAX_TOOLBAR_ITEMS);
            bar.row = bar.row.min(MAX_TOOLBAR_ROWS - 1);
            bar.x = clamp_f64(bar.x, 0.0, MAX_TOOLBAR_POSITION, 0.0);
            bar.y = clamp_f64(bar.y, 0.0, MAX_TOOLBAR_POSITION, 0.0);
            bar
        })
        .collect()
}

/// `true` if `text` can be a grid column ID: 1–64 ASCII letters, digits,
/// `_` or `-` (e.g. `"name"`, `"predecessors"`, a future `"custom-text1"`).
/// Toolbar and action IDs follow the same rule.
///
/// # Example
/// ```
/// use ganttora_lib::settings::is_column_id;
/// assert!(is_column_id("rowNumber"));
/// assert!(!is_column_id(""));
/// assert!(!is_column_id("name with spaces"));
/// ```
pub fn is_column_id(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `true` if `text` looks like a language/locale code such as `"en"`,
/// `"zh-CN"` or `"de-DE"`: letters and digits in parts of 2–8 characters,
/// separated by `-`, starting with 2–3 lowercase letters.
///
/// # Example
/// ```
/// use ganttora_lib::settings::is_locale_code;
/// assert!(is_locale_code("en"));
/// assert!(is_locale_code("zh-CN"));
/// assert!(!is_locale_code("English"));
/// assert!(!is_locale_code(""));
/// ```
pub fn is_locale_code(text: &str) -> bool {
    let mut parts = text.split('-');
    let first_ok = parts.next().is_some_and(|first| {
        (2..=3).contains(&first.len()) && first.chars().all(|c| c.is_ascii_lowercase())
    });
    first_ok
        && parts.all(|part| {
            (2..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

/// Keeps a number inside `[min, max]`; a value that is not a number (NaN)
/// becomes `fallback`.
fn clamp_f64(value: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if value.is_nan() {
        fallback
    } else {
        value.clamp(min, max)
    }
}

impl Settings {
    /// Returns a copy with every value made valid.
    ///
    /// # What it does
    /// - trims spaces from text fields; initials keep at most 4 characters;
    /// - an invalid language becomes `"en"`; an invalid date format becomes
    ///   `"system"`; a currency that is not 3 letters becomes `"EUR"`
    ///   (lowercase letters are upper-cased);
    /// - numbers are kept inside their allowed ranges (see the field docs);
    /// - an empty default save folder becomes `null`;
    /// - column widths are kept between [`MIN_COLUMN_WIDTH`] and
    ///   [`MAX_COLUMN_WIDTH`]; entries whose name is not a column ID
    ///   ([`is_column_id`]) are dropped, and at most
    ///   [`MAX_COLUMN_WIDTH_ENTRIES`] are kept;
    /// - toolbars with a bad or repeated ID are dropped, names are trimmed,
    ///   positions and counts are limited ([`sanitize_toolbars`]);
    /// - `version` is set to [`SETTINGS_VERSION`].
    ///
    /// # Returns
    /// The cleaned settings. Already valid settings come back unchanged.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::settings::Settings;
    /// let mut s = Settings::default();
    /// s.general.currency = "usd".into();
    /// s.scheduling.hours_per_day = 30.0;
    /// let clean = s.sanitize();
    /// assert_eq!(clean.general.currency, "USD");
    /// assert_eq!(clean.scheduling.hours_per_day, 24.0);
    /// ```
    ///
    /// # Beginner note
    /// `self` (not `&self`) means the method takes the value *by ownership*:
    /// the caller hands it over and gets a new, cleaned value back.
    pub fn sanitize(mut self) -> Self {
        let defaults = Settings::default();
        self.version = SETTINGS_VERSION;

        // --- User ---
        self.user.display_name = self.user.display_name.trim().to_string();
        // `chars().take(4)` counts characters, not bytes, so initials in
        // any script (e.g. "АБ" or "王") are cut correctly.
        self.user.initials = self.user.initials.trim().chars().take(4).collect();
        self.user.email = self.user.email.trim().to_string();

        // --- General ---
        let language = self.general.language.trim();
        self.general.language = if is_locale_code(language) {
            language.to_string()
        } else {
            DEFAULT_LANGUAGE.to_string()
        };
        let date_format = self.general.date_format.trim();
        self.general.date_format = if date_format == DATE_FORMAT_SYSTEM
            || date_format == DATE_FORMAT_ISO
            || DATE_PATTERNS.contains(&date_format)
            || is_locale_code(date_format)
        {
            date_format.to_string()
        } else {
            DATE_FORMAT_SYSTEM.to_string()
        };
        let currency = self.general.currency.trim().to_ascii_uppercase();
        self.general.currency =
            if currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase()) {
                currency
            } else {
                DEFAULT_CURRENCY.to_string()
            };

        // 0 is what an unusable value reads as.
        self.general.ui_zoom_percent = match self.general.ui_zoom_percent {
            0 => 100,
            percent => percent.clamp(80, 150),
        };

        // --- Scheduling ---
        let s = &mut self.scheduling;
        s.hours_per_day = clamp_f64(
            s.hours_per_day,
            0.5,
            24.0,
            defaults.scheduling.hours_per_day,
        );
        s.hours_per_week = clamp_f64(
            s.hours_per_week,
            1.0,
            168.0,
            defaults.scheduling.hours_per_week,
        );
        let calendar = s.default_calendar.trim();
        s.default_calendar = if calendar.is_empty() {
            DEFAULT_CALENDAR.to_string()
        } else {
            calendar.to_string()
        };

        // --- Files ---
        let f = &mut self.files;
        f.default_save_folder = f
            .default_save_folder
            .take()
            .map(|folder| folder.trim().to_string())
            .filter(|folder| !folder.is_empty());
        f.autosave_minutes = f.autosave_minutes.min(120);
        f.history_retention_days = f.history_retention_days.clamp(1, 3650);
        f.recent_files_max = f.recent_files_max.min(50);

        // --- Grid ---
        // `std::mem::take` moves the map out (leaving an empty one) so we can
        // rebuild it without copying every entry first.
        self.grid.column_widths = std::mem::take(&mut self.grid.column_widths)
            .into_iter()
            .filter(|(id, _)| is_column_id(id))
            .take(MAX_COLUMN_WIDTH_ENTRIES)
            .map(|(id, width)| (id, width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)))
            .collect();

        // --- Toolbars ---
        self.toolbars.layout = sanitize_toolbars(std::mem::take(&mut self.toolbars.layout));

        // --- View ---
        // 0 is what an unusable value reads as.
        self.view.zoom_percent = match self.view.zoom_percent {
            0 => 100,
            percent => percent.clamp(10, 400),
        };

        self
    }
}

// ---------------------------------------------------------------------------
// Loading and saving
// ---------------------------------------------------------------------------

/// Something the UI should tell the user about the settings file.
///
/// # JSON shape
/// ```json
/// { "kind": "corrupt", "backupPath": "/…/settings.corrupt-20261124-101500.json" }
/// { "kind": "unreadable" }
/// ```
/// The UI shows the translated text `settings.notice.<kind>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SettingsNotice {
    /// The file was not valid JSON. Defaults are used; the bad file was moved
    /// to `backup_path` (`None` if even that failed).
    Corrupt {
        /// Where the damaged file was kept, if it could be moved.
        backup_path: Option<String>,
    },
    /// The file exists but could not be read (e.g. no permission). Defaults
    /// are used for this session; the file is left untouched.
    Unreadable,
    /// The operating system reports no config folder, so settings cannot be
    /// stored. Changes last until the app closes.
    NoConfigFolder,
}

/// The result of [`load_from`]: the settings plus an optional notice.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The settings to use (defaults if the file was missing or damaged).
    pub settings: Settings,
    /// Set if something went wrong that the user should know about.
    pub notice: Option<SettingsNotice>,
}

/// Builds the name for the backup of a damaged settings file, e.g.
/// `settings.corrupt-20261124-101500.json`, next to the original.
///
/// # Parameters
/// - `path`: the settings file path.
/// - `stamp`: a time stamp text, e.g. `"20261124-101500"`.
///
/// # Example
/// ```
/// use std::path::Path;
/// use ganttora_lib::settings::backup_path_for;
/// let backup = backup_path_for(Path::new("/cfg/settings.json"), "20261124-101500");
/// assert_eq!(backup, Path::new("/cfg/settings.corrupt-20261124-101500.json"));
/// ```
pub fn backup_path_for(path: &Path, stamp: &str) -> PathBuf {
    path.with_file_name(format!("settings.corrupt-{stamp}.json"))
}

/// Reads settings from `path`, never failing.
///
/// # What it does
/// 1. No file → defaults, no notice (a fresh install).
/// 2. The file cannot be read → defaults + [`SettingsNotice::Unreadable`].
/// 3. The text is not a JSON object → the file is renamed to a backup
///    (see [`backup_path_for`]) and defaults are used, with
///    [`SettingsNotice::Corrupt`].
/// 4. Otherwise every known field is read leniently and the result is
///    cleaned with [`Settings::sanitize`].
///
/// # Parameters
/// - `path`: full path of `settings.json`.
/// - `stamp`: time stamp used in the backup file name (the caller reads the
///   clock, so this function stays easy to test).
///
/// # Returns
/// A [`Loaded`] value.
///
/// # Example
/// ```
/// use ganttora_lib::settings::{load_from, Settings};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// let loaded = load_from(&dir.join("settings.json"), "x");
/// assert_eq!(loaded.settings, Settings::default());
/// assert!(loaded.notice.is_none());
///
/// // A damaged file: defaults are used and the file is kept as a backup.
/// use ganttora_lib::settings::SettingsNotice;
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
/// let loaded = load_from(&dir.join("settings.json"), "20261124-101500");
/// assert_eq!(loaded.settings, Settings::default());
/// let backup = dir.join("settings.corrupt-20261124-101500.json");
/// assert_eq!(
///     loaded.notice,
///     Some(SettingsNotice::Corrupt { backup_path: Some(backup.to_string_lossy().into_owned()) })
/// );
/// assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ not json");
/// std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn load_from(path: &Path, stamp: &str) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        // `ErrorKind::NotFound` = there is no such file: a normal first start.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Loaded {
                settings: Settings::default(),
                notice: None,
            };
        }
        Err(_) => {
            return Loaded {
                settings: Settings::default(),
                notice: Some(SettingsNotice::Unreadable),
            };
        }
    };

    // Parse in two steps: "is this a JSON object at all?" (if not, the file
    // is damaged) and then "read our fields from it" (lenient, cannot fail
    // for an object).
    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .filter(|value| value.is_object())
        .and_then(|value| serde_json::from_value::<Settings>(value).ok());

    match parsed {
        Some(settings) => Loaded {
            settings: settings.sanitize(),
            notice: None,
        },
        None => {
            // Keep the damaged file for the user (they may have edited it by
            // hand), out of the way of the next save.
            let backup = backup_path_for(path, stamp);
            let backup_path = fs::rename(path, &backup)
                .ok()
                .map(|()| backup.to_string_lossy().into_owned());
            Loaded {
                settings: Settings::default(),
                notice: Some(SettingsNotice::Corrupt { backup_path }),
            }
        }
    }
}

/// Writes `settings` to `path` atomically (temporary file + rename).
///
/// # What it does
/// 1. Creates the config folder if it does not exist yet.
/// 2. Writes the JSON to `settings.json.tmp` in the same folder and forces
///    it onto the disk (`sync_all`).
/// 3. Renames the temporary file to `settings.json`, replacing the old one.
///    If anything fails, the temporary file is removed and the old
///    `settings.json` is left as it was.
///
/// # Parameters
/// - `path`: full path of `settings.json`.
/// - `settings`: what to write.
///
/// # Errors
/// An [`io::Error`] if the folder cannot be created or the file cannot be
/// written or renamed (disk full, no permission…).
///
/// # Example
/// ```
/// use ganttora_lib::settings::{load_from, save_to, Settings};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// let path = dir.join("settings.json");
/// let mut s = Settings::default();
/// s.general.language = "de".into();
/// save_to(&path, &s).unwrap();
/// assert_eq!(load_from(&path, "x").settings.general.language, "de");
/// std::fs::remove_dir_all(&dir).unwrap();
/// ```
///
/// # Beginner note
/// Why the temporary file must be in the *same folder*: a rename is only
/// all-or-nothing within one disk. Moving between disks is a copy, which
/// could be interrupted half-way.
pub fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    // "Pretty" JSON (indented) so a curious user can read the file.
    let mut json = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    json.push('\n');

    let temp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    // A small inner function (a *closure*) so that every `?` inside it
    // leads to the same clean-up below.
    let write_and_rename = || -> io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        // Close the file before renaming it (needed on Windows).
        drop(file);
        fs::rename(&temp, path)
    };
    write_and_rename().inspect_err(|_| {
        // Best effort: if removing fails too, there is nothing more to do.
        let _ = fs::remove_file(&temp);
    })
}

// ---------------------------------------------------------------------------
// Shared state for the IPC commands
// ---------------------------------------------------------------------------

/// The settings while the app runs, shared by all IPC commands.
///
/// # Beginner note
/// Commands can run on several threads at once, so the settings sit behind
/// a `Mutex` (a lock that lets one thread in at a time; see `state.rs`).
#[derive(Debug)]
pub struct SettingsState {
    /// Where the file lives; `None` if the OS reports no config folder.
    path: Option<PathBuf>,
    /// The current settings and the start-up notice.
    inner: Mutex<Loaded>,
}

/// Why a settings change could not be stored.
#[derive(Debug)]
pub enum SettingsError {
    /// There is no config folder (see [`SettingsNotice::NoConfigFolder`]).
    NoConfigFolder,
    /// Writing the file failed; the text explains why (English, for logs).
    Write(String),
    /// A previous command crashed while holding the lock.
    Poisoned,
}

impl SettingsState {
    /// Loads the settings at start-up.
    ///
    /// # Parameters
    /// - `path`: the settings file, or `None` if the OS has no config folder.
    /// - `stamp`: time stamp for a possible backup file name.
    ///
    /// # Returns
    /// The state, with defaults and a notice if the file was missing a
    /// folder, unreadable or damaged.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::settings::{SettingsState, SettingsNotice};
    /// let state = SettingsState::load(None, "x");
    /// assert_eq!(state.current().unwrap().notice, Some(SettingsNotice::NoConfigFolder));
    /// ```
    pub fn load(path: Option<PathBuf>, stamp: &str) -> Self {
        let loaded = match &path {
            Some(path) => load_from(path, stamp),
            None => Loaded {
                settings: Settings::default(),
                notice: Some(SettingsNotice::NoConfigFolder),
            },
        };
        SettingsState {
            path,
            inner: Mutex::new(loaded),
        }
    }

    /// Returns a copy of the current settings and the start-up notice.
    ///
    /// # Errors
    /// [`SettingsError::Poisoned`] if the lock is broken.
    pub fn current(&self) -> Result<Loaded, SettingsError> {
        self.inner
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| SettingsError::Poisoned)
    }

    /// Cleans, saves and keeps new settings.
    ///
    /// # Parameters
    /// - `settings`: the new settings from the UI.
    ///
    /// # Returns
    /// The cleaned settings as stored (the UI shows exactly these).
    ///
    /// # Errors
    /// - [`SettingsError::NoConfigFolder`]: kept in memory but not stored.
    /// - [`SettingsError::Write`]: the file could not be written; the
    ///   settings in memory are *not* changed, so UI and file stay in step.
    /// - [`SettingsError::Poisoned`]: the lock is broken.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::settings::{Settings, SettingsState};
    /// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
    /// let state = SettingsState::load(Some(dir.join("settings.json")), "x");
    /// let mut s = Settings::default();
    /// s.user.initials = " sl ".into();
    /// let stored = state.update(s).unwrap();
    /// assert_eq!(stored.user.initials, "sl");
    /// std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn update(&self, settings: Settings) -> Result<Settings, SettingsError> {
        let clean = settings.sanitize();
        let mut guard = self.inner.lock().map_err(|_| SettingsError::Poisoned)?;
        match &self.path {
            Some(path) => {
                save_to(path, &clean).map_err(|e| SettingsError::Write(e.to_string()))?;
            }
            None => {
                // Keep the change for this session, but say it was not stored.
                guard.settings = clean;
                return Err(SettingsError::NoConfigFolder);
            }
        }
        guard.settings = clean.clone();
        Ok(clean)
    }
}
