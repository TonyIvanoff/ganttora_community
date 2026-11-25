//! # The JSON exchanged with the user interface
//!
//! ## What this file checks
//! The UI (TypeScript) and the core (Rust) talk by sending JSON text through
//! Tauri's IPC. Both sides must agree on every name and shape, character by
//! character. These tests pin down:
//! - the exact JSON of each `Command` the UI sends, e.g.
//!   `{"type":"setTaskDuration","id":"…","duration":{"minutes":2880}}`;
//! - that every command survives a trip to JSON and back unchanged;
//! - the field names of the `ScheduleSnapshot` the UI receives (camelCase),
//!   and that dates and slack become `null` when the schedule fails;
//! - the field names of change records and of the `Outcome`.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! TypeScript side of the same shapes is `frontend/ipc/types.ts`; if a test
//! here fails after a rename in Rust, that file must change too.
//!
//! ## Main ideas
//! - "camelCase" means `earlyStart`, not `early_start`. Rust uses snake_case
//!   names; serde renames them for JavaScript.
//! - We compare `serde_json::Value`s (parsed JSON), not raw text, so spacing
//!   and key order do not matter, but every name and value does.

use crate::{break_schedule, ctx, missing_calendar, open_ok};
use document::{Command, Document, OutlineLevel, PlacedDependency};
use ganttora_tests::{days, dt, row_id, shed};
use model::{
    Calendar, CalendarId, Constraint, Dependency, DependencyType, Duration, ScheduleFrom, Task,
    TaskId, TaskMode, WorkWeek,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The set of keys of a JSON object, sorted, for comparing field names.
///
/// # Parameters
/// - `value`: a JSON value that must be an object.
///
/// # Returns
/// The key names. Fails the test if `value` is not an object.
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {value}"))
        .keys()
        .cloned()
        .collect()
}

/// Turns a list of names into a sorted set, for comparing with `keys`.
fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The exact text the UI sends for "set duration to 6 days" turns into
/// `Command::SetTaskDuration` with 2880 minutes, and turns back into the same
/// JSON.
///
/// Why it matters: this is the shape written in `frontend/ipc/types.ts`. If
/// Rust expected anything else (e.g. `"duration": 2880`), every duration edit
/// in the app would fail.
#[test]
fn set_task_duration_json_shape_from_the_ui() {
    let id = TaskId::from_uuid(Uuid::from_u128(0x1234));
    let text = format!(r#"{{"type":"setTaskDuration","id":"{id}","duration":{{"minutes":2880}}}}"#);

    let command: Command = serde_json::from_str(&text).expect("the UI's JSON must parse");
    assert_eq!(
        command,
        Command::SetTaskDuration {
            id,
            duration: days(6)
        }
    );

    let back = serde_json::to_value(&command).expect("serialize");
    assert_eq!(
        back,
        json!({"type": "setTaskDuration", "id": id.to_string(), "duration": {"minutes": 2880}})
    );
}

/// One example of every command variant, with fixed IDs, in the order of
/// the `Command` enum (32 variants since M1).
fn every_command() -> Vec<Command> {
    let project = shed();
    let one = row_id(&project, 1);
    let two = row_id(&project, 2);
    let task = Task::new("New", days(1));
    let link = Dependency::finish_to_start(one, two).with_lag(ganttora_tests::hours(4));
    let placed = PlacedDependency {
        index: 1,
        dependency: link.clone(),
    };
    let calendar = Calendar::with_id(
        CalendarId::from_uuid(Uuid::from_u128(0xCA1)),
        "Night shift",
        WorkWeek::standard(),
    );
    vec![
        Command::AddTask {
            index: 0,
            task: task.clone(),
        },
        Command::RemoveTask { id: one },
        Command::RestoreTask {
            index: 3,
            task: task.clone(),
            dependencies: vec![placed.clone()],
        },
        Command::RestoreTasks {
            index: 3,
            tasks: vec![task.clone(), task.clone().at_level(2)],
            dependencies: vec![placed.clone()],
        },
        Command::RenameTask {
            id: one,
            name: "Plan «the» shed — ✓".into(),
        },
        Command::SetTaskDuration {
            id: two,
            duration: days(6),
        },
        Command::SetTaskNotes {
            id: one,
            notes: "Ask the neighbours".into(),
        },
        Command::SetTaskHyperlink {
            id: one,
            hyperlink: "https://example.com/plan".into(),
        },
        Command::SetTaskMode {
            id: one,
            mode: TaskMode::Manual,
            manual_start: Some(dt("2027-03-02 08:00")),
        },
        Command::SetTaskManualStart {
            id: one,
            start: Some(dt("2027-03-03 08:00")),
        },
        Command::SetTaskMilestone {
            id: one,
            milestone: true,
        },
        Command::SetTaskConstraint {
            id: one,
            constraint: Constraint::StartNoEarlierThan {
                date: dt("2027-03-15 08:00"),
            },
        },
        Command::SetTaskDeadline {
            id: one,
            deadline: Some(dt("2027-03-19 17:00")),
        },
        Command::SetTaskCalendar {
            id: one,
            calendar_id: Some(calendar.id),
        },
        Command::IndentTask { id: two },
        Command::OutdentTask { id: two },
        Command::SetOutlineLevels {
            levels: vec![OutlineLevel { id: two, level: 2 }],
        },
        Command::MoveTask {
            id: two,
            to_index: 5,
            level: None,
        },
        Command::AddDependency {
            dependency: Dependency::new(one, two, DependencyType::StartToFinish),
        },
        Command::RemoveDependency { id: link.id },
        Command::RestoreDependency { placed },
        Command::EditDependency {
            id: link.id,
            kind: DependencyType::StartToStart,
            lag: Duration::ZERO,
            lag_percent: Some(50),
        },
        Command::SetProjectStart {
            start: dt("2027-03-08 08:00"),
        },
        Command::SetScheduleFrom {
            schedule_from: ScheduleFrom::Finish,
            finish: Some(dt("2027-04-30 17:00")),
        },
        Command::RenameProject {
            name: "Bigger shed".into(),
        },
        Command::SetProjectCalendar {
            calendar_id: calendar.id,
        },
        Command::AddCalendar {
            calendar: calendar.clone(),
        },
        Command::RemoveCalendar { id: calendar.id },
        Command::RestoreCalendar {
            index: 1,
            calendar: calendar.clone(),
        },
        Command::UpdateCalendar { calendar },
    ]
}

/// Every command turns into JSON and back into exactly the same command.
///
/// Why it matters: undo entries and (later) the journal may store commands
/// as JSON; a command that changes on the way would undo the wrong thing.
#[test]
fn every_command_round_trips_through_json() {
    for command in every_command() {
        let text = serde_json::to_string(&command).expect("serialize");
        let back: Command = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, command, "JSON was {text}");
    }
}

/// The `"type"` tag of every variant is the camelCase name the UI uses, and
/// the field names inside are camelCase too.
///
/// Why it matters: `frontend/ipc/types.ts` lists these exact names.
#[test]
fn command_tags_and_field_names() {
    let expected: [(&str, &[&str]); 30] = [
        ("addTask", &["type", "index", "task"]),
        ("removeTask", &["type", "id"]),
        ("restoreTask", &["type", "index", "task", "dependencies"]),
        ("restoreTasks", &["type", "index", "tasks", "dependencies"]),
        ("renameTask", &["type", "id", "name"]),
        ("setTaskDuration", &["type", "id", "duration"]),
        ("setTaskNotes", &["type", "id", "notes"]),
        ("setTaskHyperlink", &["type", "id", "hyperlink"]),
        ("setTaskMode", &["type", "id", "mode", "manualStart"]),
        ("setTaskManualStart", &["type", "id", "start"]),
        ("setTaskMilestone", &["type", "id", "milestone"]),
        ("setTaskConstraint", &["type", "id", "constraint"]),
        ("setTaskDeadline", &["type", "id", "deadline"]),
        ("setTaskCalendar", &["type", "id", "calendarId"]),
        ("indentTask", &["type", "id"]),
        ("outdentTask", &["type", "id"]),
        ("setOutlineLevels", &["type", "levels"]),
        ("moveTask", &["type", "id", "toIndex", "level"]),
        ("addDependency", &["type", "dependency"]),
        ("removeDependency", &["type", "id"]),
        ("restoreDependency", &["type", "placed"]),
        (
            "editDependency",
            &["type", "id", "kind", "lag", "lagPercent"],
        ),
        ("setProjectStart", &["type", "start"]),
        ("setScheduleFrom", &["type", "scheduleFrom", "finish"]),
        ("renameProject", &["type", "name"]),
        ("setProjectCalendar", &["type", "calendarId"]),
        ("addCalendar", &["type", "calendar"]),
        ("removeCalendar", &["type", "id"]),
        ("restoreCalendar", &["type", "index", "calendar"]),
        ("updateCalendar", &["type", "calendar"]),
    ];
    let commands = every_command();
    assert_eq!(commands.len(), expected.len(), "one example per variant");
    for (command, (tag, fields)) in commands.iter().zip(expected) {
        let value = serde_json::to_value(command).expect("serialize");
        assert_eq!(value["type"], json!(tag));
        assert_eq!(keys(&value), names(fields), "fields of {tag}");
    }

    // Nested shapes: a task, a placed link, a link, a date, a constraint.
    let add = serde_json::to_value(&commands[0]).expect("serialize");
    assert_eq!(
        keys(&add["task"]),
        names(&[
            "id",
            "name",
            "duration",
            "outlineLevel",
            "mode",
            "manualStart",
            "milestone",
            "constraint",
            "deadline",
            "calendarId",
            "notes",
            "hyperlink"
        ])
    );
    assert_eq!(add["task"]["duration"], json!({"minutes": 480}));
    assert_eq!(add["task"]["mode"], json!("auto"));
    assert_eq!(
        add["task"]["constraint"],
        json!({"type": "asSoonAsPossible"})
    );

    let restore = serde_json::to_value(&commands[20]).expect("serialize");
    assert_eq!(keys(&restore["placed"]), names(&["index", "dependency"]));
    let link = &restore["placed"]["dependency"];
    assert_eq!(
        keys(link),
        names(&[
            "id",
            "predecessor",
            "successor",
            "kind",
            "lag",
            "lagPercent"
        ])
    );
    // DependencyType is camelCase in JSON since M1 ("FinishToStart" before).
    assert_eq!(link["kind"], json!("finishToStart"));
    assert_eq!(link["lag"], json!({"minutes": 240}));
    assert_eq!(link["lagPercent"], Value::Null);

    let start = serde_json::to_value(&commands[22]).expect("serialize");
    assert_eq!(start["start"], json!("2027-03-08T08:00:00"));

    let constraint = serde_json::to_value(&commands[11]).expect("serialize");
    assert_eq!(
        constraint["constraint"],
        json!({"type": "startNoEarlierThan", "date": "2027-03-15T08:00:00"})
    );
    let mode = serde_json::to_value(&commands[8]).expect("serialize");
    assert_eq!(mode["mode"], json!("manual"));
    let from = serde_json::to_value(&commands[23]).expect("serialize");
    assert_eq!(from["scheduleFrom"], json!("finish"));
    let edit = serde_json::to_value(&commands[21]).expect("serialize");
    assert_eq!(edit["kind"], json!("startToStart"));
    assert_eq!(edit["lagPercent"], json!(50));
}

/// Unknown command types, missing fields and wrongly shaped values are
/// refused when parsing.
///
/// Why it matters: a typo in the UI must produce a clear error at the IPC
/// boundary, never a half-filled command.
#[test]
fn malformed_command_json_is_refused() {
    let id = Uuid::from_u128(1);
    let bad = [
        // Unknown type.
        format!(r#"{{"type":"deleteEverything","id":"{id}"}}"#),
        // snake_case tag instead of camelCase.
        format!(r#"{{"type":"set_task_duration","id":"{id}","duration":{{"minutes":1}}}}"#),
        // Missing duration.
        format!(r#"{{"type":"setTaskDuration","id":"{id}"}}"#),
        // Duration as a bare number instead of {{"minutes": …}}.
        format!(r#"{{"type":"setTaskDuration","id":"{id}","duration":2880}}"#),
        // Not a UUID.
        r#"{"type":"removeTask","id":"row-3"}"#.to_string(),
        // No type at all.
        format!(r#"{{"id":"{id}"}}"#),
    ];
    for text in bad {
        assert!(
            serde_json::from_str::<Command>(&text).is_err(),
            "should be refused: {text}"
        );
    }
}

/// The snapshot of a working schedule has exactly the camelCase fields the UI
/// expects, with dates as ISO text and slack as numbers.
///
/// Why it matters: the Gantt chart and the grid read these names; a renamed
/// field would show up as an empty column, not as an error.
#[test]
fn snapshot_field_names_when_schedule_works() {
    let document = open_ok(shed());
    let value = serde_json::to_value(document.snapshot()).expect("serialize");

    assert_eq!(
        keys(&value),
        names(&[
            "projectId",
            "projectName",
            "minutesPerDay",
            "scheduleFrom",
            "finishDate",
            "calendarId",
            "calendars",
            "projectStart",
            "projectFinish",
            "error",
            "tasks"
        ])
    );
    assert_eq!(value["projectName"], json!("Garden shed"));
    assert_eq!(value["minutesPerDay"], json!(480));
    assert_eq!(value["projectStart"], json!("2027-03-01T08:00:00"));
    assert_eq!(value["projectFinish"], json!("2027-03-23T17:00:00"));
    assert_eq!(value["error"], Value::Null);

    let tasks = value["tasks"].as_array().expect("tasks is a list");
    assert_eq!(tasks.len(), 8);
    let row = &tasks[2];
    assert_eq!(
        keys(row),
        names(&[
            "id",
            "name",
            "durationMinutes",
            "outlineLevel",
            "outlineNumber",
            "parentId",
            "isSummary",
            "isMilestone",
            "mode",
            "constraint",
            "deadline",
            "calendarId",
            "notes",
            "hyperlink",
            "start",
            "finish",
            "earlyStart",
            "earlyFinish",
            "lateStart",
            "lateFinish",
            "totalSlackMinutes",
            "freeSlackMinutes",
            "isCritical",
            "missedDeadline",
            "predecessors",
            "links",
            "explanation"
        ])
    );
    assert_eq!(row["name"], json!("Buy materials"));
    assert_eq!(row["durationMinutes"], json!(1440));
    assert_eq!(row["earlyStart"], json!("2027-03-03T08:00:00"));
    assert_eq!(row["lateFinish"], json!("2027-03-11T17:00:00"));
    assert_eq!(row["totalSlackMinutes"], json!(4 * 480));
    assert_eq!(row["isCritical"], json!(false));
    assert_eq!(row["predecessors"].as_array().map(Vec::len), Some(1));
    assert_eq!(row["outlineNumber"], json!("3"));
    assert_eq!(row["parentId"], Value::Null);
    assert_eq!(row["start"], json!("2027-03-03T08:00:00"));
    assert_eq!(value["scheduleFrom"], json!("start"));
    assert_eq!(value["calendars"].as_array().map(Vec::len), Some(1));
    // The link row of "Buy materials" (after task 1, FS, no lag).
    let link = &row["links"][0];
    assert_eq!(
        keys(link),
        names(&["id", "predecessor", "kind", "lagMinutes", "lagPercent"])
    );
    assert_eq!(link["kind"], json!("finishToStart"));
    assert_eq!(link["lagMinutes"], json!(0));
    // "Why this date?": it waits for task 1 (a predecessor link).
    assert_eq!(row["explanation"]["driver"]["type"], json!("predecessor"));
}

/// When the schedule fails, the snapshot still lists every task (name,
/// duration, links), but all dates and slack are `null`, `isCritical` is
/// false, and `error` holds an error code with details. The schedule is
/// broken on purpose with a task calendar that does not exist.
///
/// Why it matters: the UI must keep showing the grid (so the user can fix the
/// problem) and show the error, instead of crashing on missing dates.
#[test]
fn snapshot_nulls_when_schedule_fails() {
    let mut project = shed();
    let three = row_id(&project, 3);
    break_schedule(&mut project, 3);
    project.dependencies.push(Dependency::new(
        three,
        row_id(&project, 4),
        DependencyType::StartToStart,
    ));
    let document = Document::new(project);
    assert!(document.schedule().is_err());

    let value = serde_json::to_value(document.snapshot()).expect("serialize");
    assert_eq!(value["projectStart"], Value::Null);
    assert_eq!(value["projectFinish"], Value::Null);
    // Since M1 the error is a code plus details, not an English sentence;
    // the UI translates the code.
    assert_eq!(
        value["error"],
        json!({
            "code": "taskCalendarNotFound",
            "details": {
                "task": three.to_string(),
                "calendar": missing_calendar().to_string()
            }
        })
    );

    let tasks = value["tasks"].as_array().expect("tasks is a list");
    assert_eq!(tasks.len(), 8);
    for row in tasks {
        for field in [
            "earlyStart",
            "earlyFinish",
            "lateStart",
            "lateFinish",
            "totalSlackMinutes",
            "freeSlackMinutes",
            "start",
            "finish",
            "explanation",
        ] {
            assert_eq!(row[field], Value::Null, "{field} of {}", row["name"]);
        }
        assert_eq!(row["isCritical"], json!(false));
        assert!(row["name"].is_string());
        assert!(row["durationMinutes"].is_number());
    }
    // Links are still listed: row 4 now has two predecessors (2 and 3).
    assert_eq!(tasks[3]["predecessors"].as_array().map(Vec::len), Some(2));
}

/// Change records and the `Outcome` use camelCase names too, and the entity
/// kind is written in lowercase ("task").
///
/// Why it matters: the UI receives the `Outcome` after every edit (to enable
/// the Undo/Redo buttons) and will show change records in the journal view.
#[test]
fn outcome_and_change_record_field_names() {
    let project = shed();
    let id = row_id(&project, 2);
    let mut document = open_ok(project);
    let outcome = document
        .apply(
            Command::SetTaskDuration {
                id,
                duration: days(6),
            },
            &ctx(),
        )
        .expect("apply");

    let value = serde_json::to_value(&outcome).expect("serialize");
    assert_eq!(keys(&value), names(&["changes", "canUndo", "canRedo"]));
    assert_eq!(value["canUndo"], json!(true));
    assert_eq!(value["canRedo"], json!(false));

    let record = &value["changes"][0];
    assert_eq!(
        keys(record),
        names(&[
            "timestamp",
            "author",
            "batchId",
            "entity",
            "entityId",
            "field",
            "oldValue",
            "newValue"
        ])
    );
    assert_eq!(record["entity"], json!("task"));
    assert_eq!(record["entityId"], json!(id.to_string()));
    assert_eq!(record["batchId"], json!(Uuid::from_u128(1).to_string()));
    assert_eq!(record["timestamp"], json!("2027-02-15T10:00:00Z"));
    assert_eq!(record["oldValue"], json!(2400));
    assert_eq!(record["newValue"], json!(2880));
}
