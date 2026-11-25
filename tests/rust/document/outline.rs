//! # The outline (WBS): `model::Outline` and the outline commands
//!
//! ## What this file checks
//! - `model::Outline::build` turns the flat task list (row order + outline
//!   level) into a tree: parent, position among siblings, outline number
//!   ("1.2"), summary flag; `first_invalid_row` finds rows that break the
//!   rule "at most one level deeper than the row above".
//! - The document's outline commands: `IndentTask`, `OutdentTask`,
//!   `SetOutlineLevels`, `MoveTask`, and removing a summary task (which
//!   removes its subtasks, undone by `RestoreTasks`).
//! - The refusals: `CannotIndent`, `CannotOutdent`, `InvalidOutline`,
//!   `LinkToOwnSummary`.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! `Outline` tests live here (and not in a separate `model` target) because
//! the document is the main user of the outline rules.
//!
//! ## Main ideas
//! The test outline (levels in brackets):
//! ```text
//! row 0  Phase 1 (1)   summary          outline number 1
//! row 1    Design (2)  2 days           1.1
//! row 2    Permit (2)  5 days, after 1  1.2
//! row 3  Build (1)     4 days           2
//! row 4  Paint (1)     2 days           3
//! ```

use crate::commands::refused;
use crate::task_fields::{field, round_trip};
use crate::{ctx, open_ok};
use document::{Command, DocumentError, EntityKind, OutlineLevel};
use ganttora_tests::{add_task, days, link, project_on};
use model::{Dependency, Duration, Outline, Project, Task, TaskId, first_invalid_row};
use serde_json::json;

/// Builds the outline from the module notes.
///
/// # Returns
/// The project and the task IDs in row order.
fn outline_project() -> (Project, Vec<TaskId>) {
    let mut project = project_on("2027-03-01");
    let rows = [
        ("Phase 1", 0, 1),
        ("Design", 2, 2),
        ("Permit", 5, 2),
        ("Build", 4, 1),
        ("Paint", 2, 1),
    ];
    let mut ids = Vec::new();
    for (name, n, level) in rows {
        let id = add_task(&mut project, name, days(n));
        project.tasks.last_mut().expect("just added").outline_level = level;
        ids.push(id);
    }
    link(&mut project, ids[1], ids[2]);
    (project, ids)
}

/// The levels of all rows, for short comparisons.
fn levels(project: &Project) -> Vec<u32> {
    project.tasks.iter().map(|t| t.outline_level).collect()
}

/// The names of all rows in order.
fn names(project: &Project) -> Vec<&str> {
    project.tasks.iter().map(|t| t.name.as_str()).collect()
}

// ------------------------------------------------------------ Outline::build

/// `Outline::build` on the test outline: parents, positions, summary flags,
/// outline numbers and subtree ends exactly as in the module table.
///
/// Why it matters: the grid's "1.2" numbers, the indent arrows and the
/// summary bars all come from this calculation.
#[test]
fn outline_build_derives_the_tree() {
    let (project, _) = outline_project();
    let outline = Outline::build(&project.tasks);
    assert_eq!(outline.len(), 5);
    assert_eq!(
        (0..5).map(|r| outline.parent(r)).collect::<Vec<_>>(),
        vec![None, Some(0), Some(0), None, None]
    );
    assert_eq!(
        (0..5).map(|r| outline.position(r)).collect::<Vec<_>>(),
        vec![0, 0, 1, 1, 2]
    );
    assert_eq!(
        (0..5).map(|r| outline.is_summary(r)).collect::<Vec<_>>(),
        vec![true, false, false, false, false]
    );
    assert_eq!(
        (0..5)
            .map(|r| outline.outline_number(r))
            .collect::<Vec<_>>(),
        vec!["1", "1.1", "1.2", "2", "3"]
    );
    assert_eq!(outline.subtree_end(0), 3);
    assert_eq!(outline.subtree_end(1), 2);
    assert_eq!(outline.ancestors(2), vec![0]);
    assert_eq!(first_invalid_row(&project.tasks), None);
}

/// Three levels deep: "1", "1.1", "1.1.1", then "1.2" and "2".
///
/// Why it matters: numbers must restart at each level and continue after a
/// deeper block ends.
#[test]
fn outline_numbers_restart_per_level() {
    let tasks = vec![
        Task::new("A", Duration::ZERO),
        Task::new("B", Duration::ZERO).at_level(2),
        Task::new("C", days(1)).at_level(3),
        Task::new("D", days(1)).at_level(2),
        Task::new("E", days(1)),
    ];
    let outline = Outline::build(&tasks);
    assert_eq!(
        (0..5)
            .map(|r| outline.outline_number(r))
            .collect::<Vec<_>>(),
        vec!["1", "1.1", "1.1.1", "1.2", "2"]
    );
    assert_eq!(outline.ancestors(2), vec![1, 0]);
    assert_eq!(outline.subtree_end(0), 4);
}

/// An empty task list gives an empty outline and no invalid row.
///
/// Why it matters: a new project has no tasks; nothing may crash.
#[test]
fn outline_of_empty_list() {
    let outline = Outline::build(&[]);
    assert!(outline.is_empty());
    assert_eq!(first_invalid_row(&[]), None);
}

/// Invalid outlines: a first row at level 2, a jump from level 1 to 3, and
/// a level 0 are all found by `first_invalid_row`. `Outline::build` reads
/// them forgivingly: the jump is treated as one level deeper, level 0 as 1.
///
/// Why it matters: files from other programs can be broken; the app must
/// still open them (and the document refuses to *create* such outlines).
#[test]
fn invalid_outlines_are_found_and_read_forgivingly() {
    let first_deep = vec![Task::new("A", days(1)).at_level(2)];
    assert_eq!(first_invalid_row(&first_deep), Some(0));
    assert_eq!(Outline::build(&first_deep).level(0), 1);

    let jump = vec![
        Task::new("A", Duration::ZERO),
        Task::new("B", days(1)).at_level(3),
    ];
    assert_eq!(first_invalid_row(&jump), Some(1));
    let outline = Outline::build(&jump);
    assert_eq!(outline.level(1), 2);
    assert_eq!(outline.parent(1), Some(0));

    let zero = vec![Task::new("A", days(1)), Task::new("B", days(1)).at_level(0)];
    assert_eq!(first_invalid_row(&zero), Some(1));
    assert_eq!(Outline::build(&zero).level(1), 1);
}

/// The snapshot carries the outline: outline numbers, parent IDs and
/// summary flags for every row.
///
/// Why it matters: the UI draws the tree from these fields only.
#[test]
fn snapshot_carries_the_outline() {
    let (project, ids) = outline_project();
    let document = open_ok(project);
    let rows = document.snapshot().tasks;
    assert_eq!(rows[2].outline_number, "1.2");
    assert_eq!(rows[2].parent_id, Some(ids[0]));
    assert!(rows[0].is_summary);
    assert_eq!(rows[3].parent_id, None);
}

// ------------------------------------------------------------ indent/outdent

/// Indenting "Build" makes it the third child of Phase 1: one record
/// `outline_level` 1 → 2; undo is `SetOutlineLevels`.
///
/// Why it matters: the Indent button in the toolbar is this command.
#[test]
fn indent_task() {
    let (project, ids) = outline_project();
    let (document, after, records) = round_trip(project, Command::IndentTask { id: ids[3] });
    assert_eq!(levels(&after), vec![1, 2, 2, 2, 1]);
    assert_eq!(records.len(), 1);
    assert_eq!(field(&records, "outline_level").old_value, json!(1));
    assert_eq!(field(&records, "outline_level").new_value, json!(2));
    assert_eq!(document.snapshot().tasks[3].outline_number, "1.3");
}

/// Indenting a summary task moves its whole block: with a task "Survey" in
/// front, indenting Phase 1 gives levels 1, 2, 3, 3 — three records.
///
/// Why it matters: subtasks must stay below their summary task.
#[test]
fn indent_moves_the_whole_subtree() {
    let (mut project, ids) = outline_project();
    project.tasks.insert(0, Task::new("Survey", days(1)));
    let (_, after, records) = round_trip(project, Command::IndentTask { id: ids[0] });
    assert_eq!(levels(&after), vec![1, 2, 3, 3, 1, 1]);
    assert_eq!(records.len(), 3);
}

/// The first row cannot be indented, and neither can a row whose row above
/// is less indented (Design under Phase 1 would jump to level 3).
///
/// Why it matters: indenting must never break the outline rule.
#[test]
fn cannot_indent_first_row_or_past_the_row_above() {
    let (project, ids) = outline_project();
    assert_eq!(
        refused(project.clone(), Command::IndentTask { id: ids[0] }),
        DocumentError::CannotIndent(ids[0])
    );
    assert_eq!(
        refused(project, Command::IndentTask { id: ids[1] }),
        DocumentError::CannotIndent(ids[1])
    );
}

/// Outdenting "Permit" brings it to level 1: it leaves Phase 1 and becomes
/// a top-level task with outline number "2" (Build becomes "3").
///
/// Why it matters: outdent is the opposite of indent and must be undoable.
#[test]
fn outdent_task() {
    let (project, ids) = outline_project();
    let (document, after, records) = round_trip(project, Command::OutdentTask { id: ids[2] });
    assert_eq!(levels(&after), vec![1, 2, 1, 1, 1]);
    assert_eq!(records.len(), 1);
    let rows = document.snapshot().tasks;
    assert_eq!(rows[2].parent_id, None);
    assert_eq!(rows[2].outline_number, "2");
}

/// Outdenting "Design" would make "Permit" (still level 2) Design's
/// subtask, but Design → Permit is a link: a summary linked to its own
/// subtask is a circle, so the outdent is refused with `Rejected(Cycle)`.
///
/// Why it matters: an outline edit must not sneak in a forbidden link.
#[test]
fn outdent_that_creates_link_to_own_summary_is_refused() {
    let (project, ids) = outline_project();
    let error = refused(project, Command::OutdentTask { id: ids[1] });
    assert!(
        matches!(
            error,
            DocumentError::Rejected(engine::ScheduleError::Cycle { .. })
        ),
        "got {error:?}"
    );
}

/// A top-level task cannot be outdented: `CannotOutdent`.
///
/// Why it matters: there is no level 0.
#[test]
fn cannot_outdent_a_top_level_task() {
    let (project, ids) = outline_project();
    assert_eq!(
        refused(project, Command::OutdentTask { id: ids[3] }),
        DocumentError::CannotOutdent(ids[3])
    );
}

/// `SetOutlineLevels` changes several levels at once (Build and Paint to 2)
/// and records each change.
///
/// Why it matters: it is the undo of indent/outdent, and can be sent by the
/// UI for multi-row edits.
#[test]
fn set_outline_levels() {
    let (project, ids) = outline_project();
    let (_, after, records) = round_trip(
        project,
        Command::SetOutlineLevels {
            levels: vec![
                OutlineLevel {
                    id: ids[3],
                    level: 2,
                },
                OutlineLevel {
                    id: ids[4],
                    level: 2,
                },
            ],
        },
    );
    assert_eq!(levels(&after), vec![1, 2, 2, 2, 2]);
    assert_eq!(records.len(), 2);
}

/// Levels that would break the outline are refused with `InvalidOutline`
/// naming the first bad row: Permit to level 4 (row 2), or level 0.
///
/// Why it matters: the outline rule is checked after every command.
#[test]
fn invalid_levels_are_refused() {
    let (project, ids) = outline_project();
    assert_eq!(
        refused(
            project.clone(),
            Command::SetOutlineLevels {
                levels: vec![OutlineLevel {
                    id: ids[2],
                    level: 4
                }]
            }
        ),
        DocumentError::InvalidOutline { row: 2 }
    );
    assert!(matches!(
        refused(
            project,
            Command::SetOutlineLevels {
                levels: vec![OutlineLevel {
                    id: ids[2],
                    level: 0
                }]
            }
        ),
        DocumentError::InvalidOutline { .. }
    ));
}

/// Adding a task at level 3 after "Paint" (level 1) is refused with
/// `InvalidOutline { row: 5 }`.
///
/// Why it matters: new rows must fit the rows around them.
#[test]
fn add_task_with_impossible_level_is_refused() {
    let (project, _) = outline_project();
    assert_eq!(
        refused(
            project,
            Command::AddTask {
                index: 5,
                task: Task::new("Deep", days(1)).at_level(3)
            }
        ),
        DocumentError::InvalidOutline { row: 5 }
    );
}

// ------------------------------------------------------------------ moving

/// Moving "Build" to the top (row 0, level taken from the row it is dropped
/// above): rows become Build, Phase 1, Design, Permit, Paint. Record
/// `index` 3 → 0.
///
/// Why it matters: drag-and-drop in the grid is this command.
#[test]
fn move_task_to_the_top() {
    let (project, ids) = outline_project();
    let (_, after, records) = round_trip(
        project,
        Command::MoveTask {
            id: ids[3],
            to_index: 0,
            level: None,
        },
    );
    assert_eq!(
        names(&after),
        vec!["Build", "Phase 1", "Design", "Permit", "Paint"]
    );
    assert_eq!(field(&records, "index").old_value, json!(3));
    assert_eq!(field(&records, "index").new_value, json!(0));
}

/// Moving a summary task moves its block: Phase 1 to the end (index 2 in
/// the list without the block) gives Build, Paint, Phase 1, Design, Permit
/// with the levels kept.
///
/// Why it matters: subtasks travel with their summary.
#[test]
fn move_summary_moves_its_block() {
    let (project, ids) = outline_project();
    let (_, after, _) = round_trip(
        project,
        Command::MoveTask {
            id: ids[0],
            to_index: 2,
            level: None,
        },
    );
    assert_eq!(
        names(&after),
        vec!["Build", "Paint", "Phase 1", "Design", "Permit"]
    );
    assert_eq!(levels(&after), vec![1, 1, 1, 2, 2]);
}

/// Moving with an explicit level: "Paint" to row 3 at level 2 makes it the
/// third child of Phase 1; its record lists `outline_level` 1 → 2.
///
/// Why it matters: dropping a row "into" a phase sets its level.
#[test]
fn move_task_with_level() {
    let (project, ids) = outline_project();
    let (_, after, records) = round_trip(
        project,
        Command::MoveTask {
            id: ids[4],
            to_index: 3,
            level: Some(2),
        },
    );
    assert_eq!(
        names(&after),
        vec!["Phase 1", "Design", "Permit", "Paint", "Build"]
    );
    assert_eq!(levels(&after), vec![1, 2, 2, 2, 1]);
    assert_eq!(field(&records, "outline_level").new_value, json!(2));
}

/// Moves that are impossible: past the end (`IndexOutOfRange`) and to a
/// level that jumps (Paint to row 1 at level 3 → `InvalidOutline { row: 1 }`).
///
/// Why it matters: a bad drop must leave the project untouched.
#[test]
fn impossible_moves_are_refused() {
    let (project, ids) = outline_project();
    assert_eq!(
        refused(
            project.clone(),
            Command::MoveTask {
                id: ids[4],
                to_index: 9,
                level: None
            }
        ),
        DocumentError::IndexOutOfRange { index: 9, len: 4 }
    );
    assert_eq!(
        refused(
            project,
            Command::MoveTask {
                id: ids[4],
                to_index: 1,
                level: Some(3)
            }
        ),
        DocumentError::InvalidOutline { row: 1 }
    );
}

/// Moving a task to where it already is changes nothing: no undo step, no
/// records.
///
/// Why it matters: a drag that ends where it started must not fill the undo
/// list.
#[test]
fn move_to_same_place_is_a_no_op() {
    let (project, ids) = outline_project();
    let mut document = open_ok(project);
    let outcome = document
        .apply(
            Command::MoveTask {
                id: ids[3],
                to_index: 3,
                level: None,
            },
            &ctx(),
        )
        .expect("a no-op is not an error");
    assert!(outcome.changes.is_empty());
    assert!(!document.can_undo());
}

// --------------------------------------------------- summary tasks and links

/// A link between a summary task and its own subtask is refused with
/// `LinkToOwnSummary`, in both directions.
///
/// Why it matters: the summary would wait for itself (MS Project refuses it
/// too); the error names both tasks so the UI can explain.
#[test]
fn link_to_own_summary_is_refused() {
    let (project, ids) = outline_project();
    let expected = DocumentError::LinkToOwnSummary {
        summary: ids[0],
        subtask: ids[1],
    };
    assert_eq!(
        refused(
            project.clone(),
            Command::AddDependency {
                dependency: Dependency::finish_to_start(ids[0], ids[1])
            }
        ),
        expected
    );
    assert_eq!(
        refused(
            project,
            Command::AddDependency {
                dependency: Dependency::finish_to_start(ids[1], ids[0])
            }
        ),
        expected
    );
}

/// Indenting a task under its own predecessor would turn their link into a
/// summary-to-subtask link (a circle): Build → Paint exists; indenting Paint
/// under Build is refused with `Rejected(Cycle)`.
///
/// Why it matters: an outline edit must not sneak in a forbidden link.
#[test]
fn indent_that_creates_link_to_own_summary_is_refused() {
    let (mut project, ids) = outline_project();
    link(&mut project, ids[3], ids[4]);
    let error = refused(project, Command::IndentTask { id: ids[4] });
    assert!(
        matches!(
            error,
            DocumentError::Rejected(engine::ScheduleError::Cycle { .. })
        ),
        "got {error:?}"
    );
}

/// Removing Phase 1 removes it, Design and Permit, and the link Design →
/// Permit: 3 × 12 task records + 6 link records = 42. Undo (a
/// `RestoreTasks`) gives back the identical project; redo removes all three
/// again.
///
/// Why it matters: deleting a phase deletes its contents, and one Ctrl+Z
/// must bring everything back exactly.
#[test]
fn removing_a_summary_removes_its_subtasks() {
    let (project, ids) = outline_project();
    let (_, after, records) = round_trip(project, Command::RemoveTask { id: ids[0] });
    assert_eq!(names(&after), vec!["Build", "Paint"]);
    assert!(after.dependencies.is_empty());
    assert_eq!(records.len(), 42);
    assert_eq!(
        records
            .iter()
            .filter(|r| r.entity == EntityKind::Dependency)
            .count(),
        6
    );
}
