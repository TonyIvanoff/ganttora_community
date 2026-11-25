//! ipc/calendar.rs: calendar questions the Gantt chart asks while the user drags.
//!
//! # What it is for
//! When the user drags the right edge of a bar, the chart must turn "the
//! bar now ends on Friday" into a *duration in working minutes* before it
//! can send `SetTaskDuration`. And while a bar is dragged, the tooltip shows
//! the finish date the task would get. Both answers depend on the task's
//! calendar (working hours, weekends, holidays). That is calendar maths, and
//! calendar maths belongs to the engine (decision D16: no scheduling logic
//! in the UI). So the UI asks these two read-only commands:
//!
//! | Command (name used in `invoke`) | What it answers |
//! |---|---|
//! | `calendar_working_minutes` | working minutes between two moments |
//! | `calendar_add_working_time` | the moment reached after N working minutes |
//!
//! # Where it fits
//! ```text
//!   Gantt drag (frontend/features/gantt/interactions.ts)
//!     └─► frontend/ipc/client.ts ──invoke──► these commands
//!           └─► engine::calendar_math (working_minutes_between, add_working_time)
//! ```
//! Nothing here changes the project: the answers only help the UI build
//! the one command it sends when the drag ends.
//!
//! # Main ideas
//! - **`calendar_id: None` means the project calendar**, the same rule as a
//!   task's `calendarId: null`.
//! - **Plain functions first.** Each `#[tauri::command]` only finds the
//!   window label and calls a plain function ([`working_minutes`],
//!   [`add_working_time`]) that tests can call without a window.
//!

use chrono::NaiveDateTime;
use engine::calendar_math;
use model::{Calendar, CalendarId, Duration};
use tauri::{State, Window};

use crate::ipc::{IpcError, coded_json_error};
use crate::state::AppState;

/// Error code: the calendar asked for is not in the project.
pub const ERR_CALENDAR_NOT_FOUND: &str = "calendarNotFound";

/// Finds a calendar of the open project and runs `action` with it.
///
/// # Parameters
/// - `state`, `key`: which open document (the window label).
/// - `calendar_id`: the calendar, or `None` for the project calendar.
/// - `action`: what to do with the calendar (borrowed, read only).
///
/// # Returns
/// Whatever `action` returns.
///
/// # Errors
/// `noProjectOpen` / `projectUnavailable` from the shell state;
/// `calendarNotFound` if no calendar has that ID.
///
/// # Beginner note
/// `impl FnOnce(&Calendar) -> R` means "any function that takes a borrowed
/// calendar and returns some type `R`". The caller decides what `R` is.
/// This lets both commands below share the "find the calendar" part.
fn with_calendar<R>(
    state: &AppState,
    key: &str,
    calendar_id: Option<CalendarId>,
    action: impl FnOnce(&Calendar) -> R,
) -> Result<R, IpcError> {
    state.with_open(key, |open| {
        let project = open.document.project();
        // `unwrap_or` picks the project calendar when no ID was given.
        let wanted = calendar_id.unwrap_or(project.calendar_id);
        project
            .calendars
            .iter()
            .find(|calendar| calendar.id == wanted)
            .map(action)
            .ok_or_else(|| IpcError::new(ERR_CALENDAR_NOT_FOUND, wanted.to_string()))
    })?
    // The `?` above turned a state error into an IpcError and unwrapped the
    // outer `Result`; what is left is the inner "found / not found" result.
}

/// Counts the working minutes between two moments on a calendar.
///
/// # Parameters
/// - `state`, `key`: which open document.
/// - `calendar_id`: the calendar, or `None` for the project calendar.
/// - `start`, `finish`: the two moments (finish before start gives a
///   negative number).
///
/// # Returns
/// The working minutes, e.g. 480 for one standard day.
///
/// # Errors
/// As [`with_calendar`].
///
/// # Example
/// Monday 08:00 to Friday 17:00 on the standard calendar is 5 days.
/// ```
/// use ganttora_lib::{ipc::{calendar, project}, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// let mon = "2027-03-01T08:00:00".parse().unwrap();
/// let fri = "2027-03-05T17:00:00".parse().unwrap();
/// assert_eq!(calendar::working_minutes(&state, "main", None, mon, fri).unwrap(), 5 * 480);
/// ```
pub fn working_minutes(
    state: &AppState,
    key: &str,
    calendar_id: Option<CalendarId>,
    start: NaiveDateTime,
    finish: NaiveDateTime,
) -> Result<i64, IpcError> {
    with_calendar(state, key, calendar_id, |calendar| {
        calendar_math::working_minutes_between(calendar, start, finish).minutes()
    })
}

/// Finds the moment reached after `minutes` of working time from `start`.
///
/// # What it does
/// First moves `start` forward to the next working moment (a task cannot
/// start at night), then adds the working time. With `minutes == 0` the
/// answer is that snapped start (a milestone).
///
/// # Parameters
/// - `state`, `key`: which open document.
/// - `calendar_id`: the calendar, or `None` for the project calendar.
/// - `start`: where to start counting.
/// - `minutes`: working minutes to add (0 or more; negative counts backwards).
///
/// # Returns
/// The finish moment, e.g. Friday 17:00 for Monday 08:00 + 5 days.
///
/// # Errors
/// As [`with_calendar`], plus the engine's calendar errors
/// (`noWorkingTime`, `dateOutOfRange`) with their codes.
///
/// # Example
/// ```
/// use ganttora_lib::{ipc::{calendar, project}, state::AppState};
/// let state = AppState::default();
/// project::new_demo(&state, "main", "Sam").unwrap();
/// // Saturday 00:00 snaps to Monday 08:00; plus one day → Monday 17:00.
/// let sat = "2027-03-06T00:00:00".parse().unwrap();
/// let finish = calendar::add_working_time(&state, "main", None, sat, 480).unwrap();
/// assert_eq!(finish.to_string(), "2027-03-08 17:00:00");
/// ```
pub fn add_working_time(
    state: &AppState,
    key: &str,
    calendar_id: Option<CalendarId>,
    start: NaiveDateTime,
    minutes: i64,
) -> Result<NaiveDateTime, IpcError> {
    with_calendar(state, key, calendar_id, |calendar| {
        let snapped = calendar_math::snap_forward(calendar, start)?;
        calendar_math::add_working_time(calendar, snapped, Duration::from_minutes(minutes))
    })?
    // The engine's `CalendarError` serializes as `{code, details}`, like
    // every core error; `coded_json_error` keeps both for the UI.
    .map_err(|error| coded_json_error(&error, error.to_string()))
}

// ---------------------------------------------------------------------------
// The Tauri commands: thin wrappers the UI calls with `invoke`.
// ---------------------------------------------------------------------------

/// IPC: working minutes between two moments (see [`working_minutes`]).
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<number>("calendar_working_minutes", {
///   calendarId: null, start: "2027-03-01T08:00:00", finish: "2027-03-05T17:00:00",
/// }); // 2400
/// ```
///
/// # Errors
/// `noProjectOpen`, `calendarNotFound`.
#[tauri::command]
pub fn calendar_working_minutes(
    window: Window,
    state: State<'_, AppState>,
    calendar_id: Option<CalendarId>,
    start: NaiveDateTime,
    finish: NaiveDateTime,
) -> Result<i64, IpcError> {
    working_minutes(&state, window.label(), calendar_id, start, finish)
}

/// IPC: the moment after N working minutes (see [`add_working_time`]).
///
/// # Request (from TypeScript)
/// ```ts
/// await invoke<string>("calendar_add_working_time", {
///   calendarId: null, start: "2027-03-01T08:00:00", minutes: 2400,
/// }); // "2027-03-05T17:00:00"
/// ```
///
/// # Errors
/// `noProjectOpen`, `calendarNotFound`, engine calendar errors.
#[tauri::command]
pub fn calendar_add_working_time(
    window: Window,
    state: State<'_, AppState>,
    calendar_id: Option<CalendarId>,
    start: NaiveDateTime,
    minutes: i64,
) -> Result<NaiveDateTime, IpcError> {
    add_working_time(&state, window.label(), calendar_id, start, minutes)
}
