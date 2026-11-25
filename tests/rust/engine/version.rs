//! # Engine version and "loops first"
//!
//! ## What this file checks
//! - `engine::ENGINE_VERSION` is the engine crate's own package version
//!   (the `version` in `backend/crates/engine/Cargo.toml`, which takes the
//!   workspace version). Every `.gtra` save stores this text, so a later
//!   Ganttora can tell which engine calculated the stored dates.
//! - A circle of links is reported as `Cycle` (JSON code `cycleDetected`)
//!   even when the project calendar is missing. The engine looks for loops
//!   *before* it checks calendars.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//!
//! ## Main ideas
//! - `include_str!` copies a file's text into the test program when it is
//!   compiled. We use it to read the two `Cargo.toml` files and find the
//!   version written there, without any file reading at run time.

use engine::{ENGINE_VERSION, ScheduleError, schedule};
use ganttora_tests::{add_task, days, link, project_on};
use model::CalendarId;
use serde_json::json;
use uuid::Uuid;

/// The engine's Cargo.toml, as text.
const ENGINE_TOML: &str = include_str!("../../../backend/crates/engine/Cargo.toml");
/// The workspace Cargo.toml, as text (it holds the shared version).
const WORKSPACE_TOML: &str = include_str!("../../../Cargo.toml");

/// Finds `version = "x.y.z"` in a TOML text, or `None`.
///
/// # Parameters
/// - `toml`: the file content.
///
/// # Returns
/// The first version string found at the start of a line.
///
/// # Beginner note
/// This is a tiny hand-written reader, good enough for our two files. A real
/// TOML library would be overkill for one line.
fn version_in(toml: &str) -> Option<String> {
    toml.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("version")?.trim_start();
        let rest = rest.strip_prefix('=')?.trim();
        Some(rest.trim_matches('"').to_string())
    })
}

/// `ENGINE_VERSION` equals the version in the engine's Cargo.toml (or, when
/// that says `version.workspace = true`, the workspace version).
///
/// Why it matters: if the constant were typed by hand it could drift from
/// the real crate version, and files would record the wrong engine.
#[test]
fn engine_version_is_the_engine_package_version() {
    let expected = if ENGINE_TOML.contains("version.workspace = true") {
        version_in(WORKSPACE_TOML).expect("the workspace has a version")
    } else {
        version_in(ENGINE_TOML).expect("the engine has a version")
    };
    assert_eq!(ENGINE_VERSION, expected);
    assert_eq!(ENGINE_VERSION.split('.').count(), 3, "major.minor.patch");
}

/// A two-task circle in a project whose calendar ID points nowhere is still
/// reported as `Cycle`, and its JSON code is `cycleDetected`.
///
/// Why it matters: the document refuses any edit that creates a loop. If
/// the missing calendar were reported first, it would hide the loop and the
/// document would let the loop in, while the schedule is already broken.
#[test]
fn loop_with_missing_project_calendar_reports_cycle() {
    let mut project = project_on("2027-03-01");
    let a = add_task(&mut project, "A", days(1));
    let b = add_task(&mut project, "B", days(1));
    link(&mut project, a, b);
    link(&mut project, b, a);
    project.calendar_id = CalendarId::from_uuid(Uuid::from_u128(0xDEAD));

    let error = schedule(&project).unwrap_err();
    assert!(
        matches!(error, ScheduleError::Cycle { .. }),
        "expected a cycle, got {error:?}"
    );
    let value = serde_json::to_value(&error).expect("to JSON");
    assert_eq!(value["code"], json!("cycleDetected"));
    assert_eq!(value["details"]["names"], json!(["A", "B"]));
}

/// Without a loop, the same missing calendar gives `CalendarNotFound`.
///
/// Why it matters: the control case. It shows the test above really is
/// about the order of the checks, not about calendars being ignored.
#[test]
fn missing_project_calendar_without_loop_reports_calendar() {
    let mut project = project_on("2027-03-01");
    add_task(&mut project, "A", days(1));
    let missing = CalendarId::from_uuid(Uuid::from_u128(0xDEAD));
    project.calendar_id = missing;
    assert_eq!(
        schedule(&project).unwrap_err(),
        ScheduleError::CalendarNotFound(missing)
    );
}
