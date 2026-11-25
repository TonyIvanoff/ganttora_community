//! # Blocker commands, their journal records and the snapshot fields (D23)
//!
//! ## What this file checks
//! - The five blocker commands: `AddBlocker` (append), `AddBlockerAt`
//!   (position from a date, resolved from the current schedule; no schedule
//!   → before any work), `EditBlocker`, `RemoveBlocker`, `RestoreBlocker`.
//!   Undo gives back exactly the project from before; redo exactly the one
//!   from after.
//! - The refusals: `BlockerNotFound`, `BlockerIdExists`, `BlockerOnSummary`
//!   (also for indent / move / add / paste that would turn a task with
//!   blockers into a summary task) and `NegativeDuration`.
//! - The journal: one record with field `"blocker"` and JSON object values;
//!   "created"/"deleted" task records contain `"blockers"` only when the
//!   task has some.
//! - The snapshot `TaskRow`: `blockers`, `segments` and `blockedMinutes`
//!   are left out of the JSON when the task has no blockers, and carry the
//!   right values when it has.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! code under test is `backend/crates/document/src/command.rs` (commands),
//! `lib.rs` (the summary rule) and `reschedule.rs` (the snapshot).
//!
//! ## Main ideas
//! The garden shed again. "Buy materials" (row 3) runs
//! Wed 3 – Fri 5 Mar 2027 and has 4 days of slack; a 5-day blocker after 2
//! days of work moves it to Fri 12 Mar and the shed's finish from Tue 23 to
//! Wed 24 Mar.

use crate::commands::refused;
use crate::{break_schedule, ctx, open_ok};
use document::clipboard::paste_tasks;
use document::{Command, DocumentError, EntityKind, OutlineLevel};
use ganttora_tests::{date, days, dt, row_id, shed};
use model::{Blocker, BlockerId, Duration, Project, Task, TaskId};
use serde_json::{Value, json};
use uuid::Uuid;

/// The blocker ID made from the number `n` (UUID `0…0n`), so tests can
/// write the expected JSON by hand.
fn bid(n: u128) -> BlockerId {
    BlockerId::from_uuid(Uuid::from_u128(n))
}

/// A blocker with a fixed ID and reason "Late wood".
///
/// # Parameters
/// - `n`: number for the ID (see [`bid`]).
/// - `after_work`: work done before the blocker starts.
/// - `duration`: how long the task is blocked.
fn blocker(n: u128, after_work: Duration, duration: Duration) -> Blocker {
    Blocker::new(bid(n), after_work, duration, "Late wood")
}

/// The journal JSON of a blocker, as the document writes it (snake_case
/// keys, minutes as numbers).
fn blocker_json(b: &Blocker) -> Value {
    json!({
        "id": b.id.to_string(),
        "after_work_minutes": b.after_work.minutes(),
        "duration_minutes": b.duration.minutes(),
        "reason": b.reason,
        "reported_on": b.reported_on.map(|d| d.to_string()),
    })
}

/// The shed with blockers already on "Buy materials" (row 3).
///
/// # Returns
/// The project and the ID of "Buy materials".
fn shed_with(blockers: Vec<Blocker>) -> (Project, TaskId) {
    let mut project = shed();
    project.tasks[2].blockers = blockers;
    let buy = row_id(&project, 3);
    (project, buy)
}

// ------------------------------------------------------------ AddBlocker

/// `AddBlocker` on "Buy materials" stores the blocker, reschedules (the
/// shed now finishes Wed 24 Mar) and writes one journal record: entity
/// Task, field `"blocker"`, old `null`, new = the blocker as an object.
/// Undo gives back exactly the original project; redo the changed one.
///
/// Why it matters: this is the "Add blocker" dialog; the journal must say
/// what was added, and Undo must leave no trace.
#[test]
fn add_blocker_round_trip_and_record() {
    let (project, buy) = shed_with(Vec::new());
    let before = project.clone();
    let mut document = open_ok(project);
    let late = blocker(1, days(2), days(5));
    let outcome = document
        .apply(
            Command::AddBlocker {
                task: buy,
                blocker: late.clone(),
            },
            &ctx(),
        )
        .expect("add");
    assert_eq!(document.project().tasks[2].blockers, vec![late.clone()]);
    let schedule = document.schedule().expect("schedules");
    assert_eq!(schedule.project_finish, dt("2027-03-24 17:00"));

    assert_eq!(outcome.changes.len(), 1);
    let record = &outcome.changes[0];
    assert_eq!(record.entity, EntityKind::Task);
    assert_eq!(record.entity_id, buy.as_uuid());
    assert_eq!(record.field, "blocker");
    assert_eq!(record.old_value, Value::Null);
    assert_eq!(
        record.new_value,
        json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "after_work_minutes": 960,
            "duration_minutes": 2400,
            "reason": "Late wood",
            "reported_on": null,
        })
    );

    let after = document.project().clone();
    let undone = document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    assert_eq!(undone.changes[0].old_value, record.new_value);
    assert_eq!(undone.changes[0].new_value, Value::Null);
    assert_eq!(
        document.schedule().expect("schedules").project_finish,
        dt("2027-03-23 17:00")
    );
    document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &after);
}

/// A second `AddBlocker` is appended at the end of the task's list, and a
/// `reported_on` day is written into the record as `"YYYY-MM-DD"`.
///
/// Why it matters: the Blockers tab lists blockers in the order they were
/// added.
#[test]
fn add_blocker_appends_at_the_end() {
    let first = blocker(1, days(2), days(5));
    let (project, buy) = shed_with(vec![first.clone()]);
    let mut document = open_ok(project);
    let mut rain = blocker(2, Duration::ZERO, days(1));
    rain.reported_on = Some(date("2027-03-02"));
    let outcome = document
        .apply(
            Command::AddBlocker {
                task: buy,
                blocker: rain.clone(),
            },
            &ctx(),
        )
        .expect("add");
    assert_eq!(document.project().tasks[2].blockers, vec![first, rain]);
    assert_eq!(
        outcome.changes[0].new_value["reported_on"],
        json!("2027-03-02")
    );
}

// ---------------------------------------------------------- AddBlockerAt

/// Applies `AddBlockerAt` on "Buy materials" at `at` and returns the stored
/// `after_work`.
///
/// # Parameters
/// - `project`: the starting project (must contain the shed's row 3).
/// - `at`: the date the user picked, `"YYYY-MM-DD HH:MM"`.
///
/// # Returns
/// The `after_work` of the new blocker (ID 9), as resolved by the document.
fn resolved_after_work(project: Project, at: &str) -> Duration {
    let buy = row_id(&project, 3);
    let mut document = open_ok(project);
    document
        .apply(
            Command::AddBlockerAt {
                task: buy,
                id: bid(9),
                at: dt(at),
                duration: days(1),
                reason: "Rain".into(),
                reported_on: None,
            },
            &ctx(),
        )
        .expect("add at");
    let task = &document.project().tasks[2];
    task.blockers
        .iter()
        .find(|b| b.id == bid(9))
        .expect("the new blocker")
        .after_work
}

/// `AddBlockerAt` turns a date into "work done before it", read from the
/// current schedule. "Buy materials" starts Wed 3 08:00: a blocker from
/// Fri 5 08:00 comes after 2 days (960 min); from Thu 4 12:00 after 1.5
/// days; before the start → 0; after the finish → the whole 3 days.
///
/// Why it matters: users think in dates ("blocked from Friday"); the
/// document stores the position as work, so it moves with the task.
#[test]
fn add_blocker_at_resolves_the_date_into_work() {
    let plain = || shed_with(Vec::new()).0;
    assert_eq!(resolved_after_work(plain(), "2027-03-05 08:00"), days(2));
    assert_eq!(
        resolved_after_work(plain(), "2027-03-04 12:00"),
        Duration::from_minutes(720)
    );
    assert_eq!(
        resolved_after_work(plain(), "2027-03-01 08:00"),
        Duration::ZERO
    );
    assert_eq!(resolved_after_work(plain(), "2027-03-20 08:00"), days(3));
}

/// With a blocker already there (work Wed 3 – Thu 4, blocked Fri 5 – Thu
/// 11, work Fri 12), only *work* pieces count: a date inside the blocked
/// piece (Wed 10) gives 2 days; Fri 12 12:00 gives 2.5 days (1200 min).
///
/// Why it matters: blocked time is not work; counting it would put the
/// second blocker in the wrong place.
#[test]
fn add_blocker_at_counts_only_work_pieces() {
    let existing = || shed_with(vec![blocker(1, days(2), days(5))]).0;
    assert_eq!(resolved_after_work(existing(), "2027-03-10 10:00"), days(2));
    assert_eq!(
        resolved_after_work(existing(), "2027-03-12 12:00"),
        Duration::from_minutes(1200)
    );
}

/// The resolved command is the one stored for redo: after undo, redo puts
/// back the blocker with exactly the same `after_work`, and the project
/// equals the one after the first apply.
///
/// Why it matters: redo must repeat what happened, not recompute it from a
/// schedule that may have changed meanwhile.
#[test]
fn add_blocker_at_redo_repeats_the_resolved_position() {
    let (project, buy) = shed_with(Vec::new());
    let before = project.clone();
    let mut document = open_ok(project);
    document
        .apply(
            Command::AddBlockerAt {
                task: buy,
                id: bid(9),
                at: dt("2027-03-05 08:00"),
                duration: days(5),
                reason: "Late wood".into(),
                reported_on: Some(date("2027-03-04")),
            },
            &ctx(),
        )
        .expect("add at");
    let after = document.project().clone();
    assert_eq!(after.tasks[2].blockers[0].after_work, days(2));
    assert_eq!(
        after.tasks[2].blockers[0].reported_on,
        Some(date("2027-03-04"))
    );
    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &after);
}

/// When the project has no schedule (it cannot be calculated right now),
/// `AddBlockerAt` puts the blocker before any work (`after_work` = 0).
///
/// Why it matters: without dates there is nothing to measure against; the
/// command must still work instead of failing.
#[test]
fn add_blocker_at_without_a_schedule_starts_at_zero() {
    let (mut project, buy) = shed_with(Vec::new());
    break_schedule(&mut project, 8);
    let mut document = document::Document::new(project);
    assert!(document.schedule().is_err());
    document
        .apply(
            Command::AddBlockerAt {
                task: buy,
                id: bid(9),
                at: dt("2027-03-05 08:00"),
                duration: days(1),
                reason: String::new(),
                reported_on: None,
            },
            &ctx(),
        )
        .expect("add at");
    assert_eq!(
        document.project().tasks[2].blockers[0].after_work,
        Duration::ZERO
    );
}

// ------------------------------------------------- EditBlocker / Remove

/// `EditBlocker` replaces the blocker with the same ID (5 days → 2 days):
/// one `"blocker"` record with the old and the new object; undo and redo
/// give the exact projects.
///
/// Why it matters: "the delivery is now only 2 days late" is an edit, not
/// a new blocker.
#[test]
fn edit_blocker_round_trip_and_record() {
    let old = blocker(1, days(2), days(5));
    let (project, buy) = shed_with(vec![old.clone()]);
    let before = project.clone();
    let mut document = open_ok(project);
    let mut new = old.clone();
    new.duration = days(2);
    new.reason = "Shorter delay".into();
    let outcome = document
        .apply(
            Command::EditBlocker {
                task: buy,
                blocker: new.clone(),
            },
            &ctx(),
        )
        .expect("edit");
    assert_eq!(document.project().tasks[2].blockers, vec![new.clone()]);
    assert_eq!(outcome.changes.len(), 1);
    assert_eq!(outcome.changes[0].field, "blocker");
    assert_eq!(outcome.changes[0].old_value, blocker_json(&old));
    assert_eq!(outcome.changes[0].new_value, blocker_json(&new));
    let after = document.project().clone();
    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &after);
}

/// `RemoveBlocker` of the middle one of three: record old = object, new =
/// `null`; undo (`RestoreBlocker`) puts it back **at the same position**,
/// so the project is exactly as before.
///
/// Why it matters: undo must not reorder the user's list.
#[test]
fn remove_blocker_and_undo_keeps_the_order() {
    let list = vec![
        blocker(1, days(1), days(1)),
        blocker(2, days(2), days(1)),
        blocker(3, days(3), days(1)),
    ];
    let (project, buy) = shed_with(list.clone());
    let before = project.clone();
    let mut document = open_ok(project);
    let outcome = document
        .apply(
            Command::RemoveBlocker {
                task: buy,
                id: bid(2),
            },
            &ctx(),
        )
        .expect("remove");
    let ids: Vec<BlockerId> = document.project().tasks[2]
        .blockers
        .iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(ids, vec![bid(1), bid(3)]);
    assert_eq!(outcome.changes[0].old_value, blocker_json(&list[1]));
    assert_eq!(outcome.changes[0].new_value, Value::Null);
    let after = document.project().clone();
    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    document.redo(&ctx()).expect("redo");
    assert_eq!(document.project(), &after);
}

/// `RestoreBlocker` sent directly puts a blocker at the given index; an
/// index past the end is refused with `IndexOutOfRange`.
///
/// Why it matters: undo uses this command; a wrong index must fail loudly
/// instead of silently appending.
#[test]
fn restore_blocker_at_index_and_out_of_range() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let mut document = open_ok(project.clone());
    document
        .apply(
            Command::RestoreBlocker {
                task: buy,
                index: 0,
                blocker: blocker(2, Duration::ZERO, days(1)),
            },
            &ctx(),
        )
        .expect("restore");
    assert_eq!(document.project().tasks[2].blockers[0].id, bid(2));

    let error = refused(
        project,
        Command::RestoreBlocker {
            task: buy,
            index: 5,
            blocker: blocker(2, Duration::ZERO, days(1)),
        },
    );
    assert_eq!(error, DocumentError::IndexOutOfRange { index: 5, len: 1 });
}

/// Several blocker commands in one `Batch` are one undo step.
///
/// Why it matters: the Task Information dialog sends all blocker changes
/// together with the other fields; one Undo must take them all back.
#[test]
fn blocker_commands_in_a_batch_are_one_undo_step() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let before = project.clone();
    let mut document = open_ok(project);
    document
        .apply_batch(
            vec![
                Command::RemoveBlocker {
                    task: buy,
                    id: bid(1),
                },
                Command::AddBlocker {
                    task: buy,
                    blocker: blocker(2, days(2), days(5)),
                },
                Command::RenameTask {
                    id: buy,
                    name: "Buy wood".into(),
                },
            ],
            &ctx(),
        )
        .expect("batch");
    assert_eq!(document.project().tasks[2].blockers[0].id, bid(2));
    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before);
    assert!(!document.can_undo());
}

// --------------------------------------------------------------- errors

/// Unknown blocker IDs give `BlockerNotFound` for edit and remove; an
/// unknown task gives `TaskNotFound`. The project stays unchanged.
///
/// Why it matters: the UI may hold an old ID (e.g. after an undo in another
/// window); the error must name the problem.
#[test]
fn unknown_blocker_or_task_is_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    assert_eq!(
        refused(
            project.clone(),
            Command::RemoveBlocker {
                task: buy,
                id: bid(7)
            }
        ),
        DocumentError::BlockerNotFound(bid(7))
    );
    assert_eq!(
        refused(
            project.clone(),
            Command::EditBlocker {
                task: buy,
                blocker: blocker(7, days(1), days(1))
            }
        ),
        DocumentError::BlockerNotFound(bid(7))
    );
    let nowhere = TaskId::from_uuid(Uuid::from_u128(0xBAD));
    assert_eq!(
        refused(
            project,
            Command::AddBlocker {
                task: nowhere,
                blocker: blocker(2, days(1), days(1))
            }
        ),
        DocumentError::TaskNotFound(nowhere)
    );
}

/// A blocker ID is unique in the whole project: adding ID 1 again, to the
/// same task or to another one, gives `BlockerIdExists`.
///
/// Why it matters: undo, the journal and the file all find a blocker by
/// its ID; two with the same ID would be confused.
#[test]
fn duplicate_blocker_id_is_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let permit = row_id(&project, 2);
    for task in [buy, permit] {
        assert_eq!(
            refused(
                project.clone(),
                Command::AddBlocker {
                    task,
                    blocker: blocker(1, days(1), days(1))
                }
            ),
            DocumentError::BlockerIdExists(bid(1))
        );
    }
}

/// Negative `after_work` or `duration` gives `NegativeDuration(task)`, for
/// add and for edit.
///
/// Why it matters: a negative delay would make a task shorter than its
/// planned work; the UI must show "must not be negative".
#[test]
fn negative_values_are_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let bad = [
        blocker(2, days(-1), days(1)),
        blocker(2, days(1), Duration::from_minutes(-1)),
    ];
    for b in bad {
        assert_eq!(
            refused(
                project.clone(),
                Command::AddBlocker {
                    task: buy,
                    blocker: b
                }
            ),
            DocumentError::NegativeDuration(buy)
        );
    }
    assert_eq!(
        refused(
            project,
            Command::EditBlocker {
                task: buy,
                blocker: blocker(1, days(1), days(-3))
            }
        ),
        DocumentError::NegativeDuration(buy)
    );
}

/// A blocker on a summary task is refused with `BlockerOnSummary`.
///
/// Why it matters: summary dates are rolled up from their subtasks; a
/// delay belongs to the subtask that is actually stuck.
#[test]
fn blocker_on_a_summary_task_is_refused() {
    let mut project = shed();
    project.tasks[2].outline_level = 2; // Buy materials under Permit
    let permit = row_id(&project, 2);
    // Permit is now the summary of Buy; drop the link between them.
    let buy = row_id(&project, 3);
    project
        .dependencies
        .retain(|d| !(d.predecessor == permit && d.successor == buy));
    assert_eq!(
        refused(
            project,
            Command::AddBlocker {
                task: permit,
                blocker: blocker(1, days(1), days(1))
            }
        ),
        DocumentError::BlockerOnSummary(permit)
    );
}

/// Indenting "Prepare the ground" (row 4) under "Buy materials", which has
/// a blocker, would make "Buy materials" a summary task: refused with
/// `BlockerOnSummary(buy)`. The same for `SetOutlineLevels`.
///
/// Why it matters: the rule "summaries have no blockers" must hold however
/// a task becomes a summary.
#[test]
fn indent_that_makes_a_blocked_task_a_summary_is_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let ground = row_id(&project, 4);
    assert_eq!(
        refused(project.clone(), Command::IndentTask { id: ground }),
        DocumentError::BlockerOnSummary(buy)
    );
    assert_eq!(
        refused(
            project,
            Command::SetOutlineLevels {
                levels: vec![OutlineLevel {
                    id: ground,
                    level: 2
                }]
            }
        ),
        DocumentError::BlockerOnSummary(buy)
    );
}

/// Moving "Paint" to just below "Buy materials" one level deeper would make
/// "Buy materials" a summary: refused.
///
/// Why it matters: drag-and-drop in the grid is a `MoveTask`.
#[test]
fn move_that_makes_a_blocked_task_a_summary_is_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let paint = row_id(&project, 7);
    assert_eq!(
        refused(
            project,
            Command::MoveTask {
                id: paint,
                to_index: 3,
                level: Some(2),
            }
        ),
        DocumentError::BlockerOnSummary(buy)
    );
}

/// Adding a task one level deeper just below "Buy materials" (what Insert
/// does) is refused, also inside a `Batch` (the shape of a paste). A real
/// paste plan at that place fits the level to the row below (level 1), so
/// it is accepted and "Buy materials" keeps its blocker.
///
/// Why it matters: inserting and pasting rows are easy ways to create
/// subtasks by accident; the rule must catch them, while a normal paste
/// must keep working.
#[test]
fn add_or_paste_that_makes_a_blocked_task_a_summary_is_refused() {
    let (project, buy) = shed_with(vec![blocker(1, days(1), days(1))]);
    let child = || {
        let mut task = Task::new("Order screws", days(1));
        task.outline_level = 2;
        task
    };
    assert_eq!(
        refused(
            project.clone(),
            Command::AddTask {
                index: 3,
                task: child()
            }
        ),
        DocumentError::BlockerOnSummary(buy)
    );
    assert_eq!(
        refused(
            project.clone(),
            Command::Batch {
                commands: vec![Command::AddTask {
                    index: 3,
                    task: child()
                }]
            }
        ),
        DocumentError::BlockerOnSummary(buy)
    );

    // A paste with "Outline level" 2 right below "Buy materials": the first
    // pasted row becomes a sibling of the row it is pasted above (level 1).
    let mut counter = 0u128;
    let plan = paste_tasks(
        &project,
        "Name\tDuration\tOutline level\nOrder screws\t1d\t2\n",
        3,
        None,
        None,
        || {
            counter += 1;
            Uuid::from_u128(0x5000 + counter)
        },
    );
    let mut document = open_ok(project);
    document.apply(plan.command, &ctx()).expect("paste");
    assert_eq!(document.project().tasks[3].outline_level, 1);
    assert_eq!(document.project().tasks[2].blockers.len(), 1);
}

/// A project that *already* has a summary with blockers (e.g. opened from
/// a damaged file) can still be edited: other commands are not refused.
///
/// Why it matters: the user must be able to repair such a file inside
/// Ganttora, not be locked out of every edit.
#[test]
fn existing_summary_with_blockers_does_not_block_other_edits() {
    let mut project = shed();
    project.tasks[2].outline_level = 2;
    let permit = row_id(&project, 2);
    let buy = row_id(&project, 3);
    project
        .dependencies
        .retain(|d| !(d.predecessor == permit && d.successor == buy));
    project.tasks[1].blockers.push(blocker(1, days(1), days(1)));
    let mut document = open_ok(project);
    document
        .apply(
            Command::RenameTask {
                id: row_id(document.project(), 1),
                name: "Design".into(),
            },
            &ctx(),
        )
        .expect("other edits still work");
}

/// The three new error codes serialize as `{"code": "...", "details": id}`
/// with the camelCase names the UI translates.
///
/// Why it matters: the UI looks up `errors.blockerNotFound` etc. by code.
#[test]
fn blocker_error_codes_in_json() {
    let task = TaskId::from_uuid(Uuid::from_u128(3));
    let cases = [
        (DocumentError::BlockerNotFound(bid(1)), "blockerNotFound"),
        (DocumentError::BlockerIdExists(bid(1)), "blockerIdExists"),
        (DocumentError::BlockerOnSummary(task), "blockerOnSummary"),
    ];
    for (error, code) in cases {
        let value = serde_json::to_value(&error).expect("json");
        assert_eq!(value["code"], json!(code));
        assert!(value["details"].is_string(), "{value}");
    }
}

// --------------------------------------------------------------- journal

/// Adding a task **with** blockers gives 13 "created" records: the usual 12
/// plus `"blockers"` (a list of blocker objects). Removing it gives the
/// same 13 as "deleted" records. A task without blockers keeps exactly 12.
///
/// Why it matters: the journal alone must be able to tell what was added
/// or lost, without changing the records of ordinary tasks.
#[test]
fn create_and_delete_records_include_blockers_only_when_present() {
    let mut document = open_ok(shed());
    let mut task = Task::new("Order the door", days(1));
    let late = blocker(1, Duration::ZERO, days(2));
    task.blockers.push(late.clone());
    let id = task.id;
    let added = document
        .apply(Command::AddTask { index: 8, task }, &ctx())
        .expect("add");
    assert_eq!(added.changes.len(), 13);
    let list = added
        .changes
        .iter()
        .find(|c| c.field == "blockers")
        .expect("blockers record");
    assert_eq!(list.old_value, Value::Null);
    assert_eq!(list.new_value, json!([blocker_json(&late)]));

    let removed = document
        .apply(Command::RemoveTask { id }, &ctx())
        .expect("remove");
    let list = removed
        .changes
        .iter()
        .find(|c| c.field == "blockers")
        .expect("blockers record");
    assert_eq!(list.old_value, json!([blocker_json(&late)]));
    assert_eq!(list.new_value, Value::Null);

    let plain = document
        .apply(
            Command::AddTask {
                index: 0,
                task: Task::new("Plain", days(1)),
            },
            &ctx(),
        )
        .expect("add plain");
    assert_eq!(plain.changes.len(), 12);
    assert!(plain.changes.iter().all(|c| c.field != "blockers"));
}

// -------------------------------------------------------------- snapshot

/// In the snapshot JSON a task without blockers has **no** `blockers`,
/// `segments` or `blockedMinutes` key. "Buy materials" with a 5-day
/// blocker has all three: one blocker row (with camelCase keys and the
/// blocked piece's dates Fri 5 08:00 – Thu 11 17:00), three segments and
/// `blockedMinutes` 2400; `durationMinutes` stays 1440 (the planned work).
///
/// Why it matters: the UI draws the striped pieces and the Blockers tab
/// from exactly these fields; old rows must look as before.
#[test]
fn snapshot_blocker_fields() {
    let mut late = blocker(1, days(2), days(5));
    late.reported_on = Some(date("2027-03-04"));
    let (project, _) = shed_with(vec![late]);
    let document = open_ok(project);
    let value = serde_json::to_value(document.snapshot()).expect("json");
    let rows = value["tasks"].as_array().expect("tasks");

    let design = rows[0].as_object().expect("row");
    for key in ["blockers", "segments", "blockedMinutes"] {
        assert!(!design.contains_key(key), "plain row has no {key}");
    }

    let buy = &rows[2];
    assert_eq!(buy["durationMinutes"], json!(1440));
    assert_eq!(buy["blockedMinutes"], json!(2400));
    assert_eq!(buy["finish"], json!("2027-03-12T17:00:00"));
    assert_eq!(
        buy["blockers"],
        json!([{
            "id": "00000000-0000-0000-0000-000000000001",
            "afterWorkMinutes": 960,
            "durationMinutes": 2400,
            "reason": "Late wood",
            "reportedOn": "2027-03-04",
            "start": "2027-03-05T08:00:00",
            "finish": "2027-03-11T17:00:00",
        }])
    );
    assert_eq!(
        buy["segments"],
        json!([
            { "start": "2027-03-03T08:00:00", "finish": "2027-03-04T17:00:00",
              "kind": { "type": "work" } },
            { "start": "2027-03-05T08:00:00", "finish": "2027-03-11T17:00:00",
              "kind": { "type": "blocked",
                        "blocker": "00000000-0000-0000-0000-000000000001" } },
            { "start": "2027-03-12T08:00:00", "finish": "2027-03-12T17:00:00",
              "kind": { "type": "work" } },
        ])
    );
    assert_eq!(buy["explanation"]["blocked"], json!({ "minutes": 2400 }));
}

/// While the schedule fails, a task's blocker rows are still there (what
/// the user typed) but with `start`/`finish` `null`; `segments` is absent
/// and `blockedMinutes` is the sum of the blockers (2400).
///
/// Why it matters: the Blockers tab must keep working while the user fixes
/// the error that broke the schedule.
#[test]
fn snapshot_blocker_fields_without_a_schedule() {
    let (mut project, _) = shed_with(vec![blocker(1, days(2), days(5))]);
    break_schedule(&mut project, 8);
    let document = document::Document::new(project);
    let value = serde_json::to_value(document.snapshot()).expect("json");
    let buy = &value["tasks"][2];
    assert_eq!(buy["blockers"][0]["start"], Value::Null);
    assert_eq!(buy["blockers"][0]["finish"], Value::Null);
    assert_eq!(buy["blockers"][0]["afterWorkMinutes"], json!(960));
    assert!(buy.get("segments").is_none());
    assert_eq!(buy["blockedMinutes"], json!(2400));
}

// ------------------------------------------------------------ command JSON

/// The exact JSON of the blocker commands the UI sends, and a trip to JSON
/// and back gives the same command.
///
/// Why it matters: `frontend/ipc/types.ts` builds these objects by hand;
/// one wrong name and the command is rejected.
#[test]
fn blocker_command_json() {
    let task = TaskId::from_uuid(Uuid::from_u128(3));
    let task_text = "00000000-0000-0000-0000-000000000003";
    let blocker_text = "00000000-0000-0000-0000-000000000001";
    let at = Command::AddBlockerAt {
        task,
        id: bid(1),
        at: dt("2027-03-05 08:00"),
        duration: days(5),
        reason: "Late wood".into(),
        reported_on: Some(date("2027-03-04")),
    };
    assert_eq!(
        serde_json::to_value(&at).expect("json"),
        json!({
            "type": "addBlockerAt",
            "task": task_text,
            "id": blocker_text,
            "at": "2027-03-05T08:00:00",
            "duration": { "minutes": 2400 },
            "reason": "Late wood",
            "reportedOn": "2027-03-04",
        })
    );
    let add = Command::AddBlocker {
        task,
        blocker: blocker(1, days(2), days(5)),
    };
    assert_eq!(
        serde_json::to_value(&add).expect("json"),
        json!({
            "type": "addBlocker",
            "task": task_text,
            "blocker": {
                "id": blocker_text,
                "afterWork": { "minutes": 960 },
                "duration": { "minutes": 2400 },
                "reason": "Late wood",
                "reportedOn": null,
            },
        })
    );
    let remove = Command::RemoveBlocker { task, id: bid(1) };
    assert_eq!(
        serde_json::to_value(&remove).expect("json"),
        json!({ "type": "removeBlocker", "task": task_text, "id": blocker_text })
    );
    let commands = [
        at,
        add,
        remove,
        Command::EditBlocker {
            task,
            blocker: blocker(1, days(1), days(1)),
        },
        Command::RestoreBlocker {
            task,
            index: 0,
            blocker: blocker(1, days(1), days(1)),
        },
    ];
    for command in commands {
        let text = serde_json::to_string(&command).expect("json");
        let back: Command = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, command, "{text}");
    }
    assert_eq!(
        serde_json::to_value(Command::EditBlocker {
            task,
            blocker: blocker(1, days(1), days(1)),
        })
        .expect("json")["type"],
        json!("editBlocker")
    );
    assert_eq!(
        serde_json::to_value(Command::RestoreBlocker {
            task,
            index: 0,
            blocker: blocker(1, days(1), days(1)),
        })
        .expect("json")["type"],
        json!("restoreBlocker")
    );
}
