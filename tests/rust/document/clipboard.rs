//! # Copy and paste of task rows (`document::clipboard`)
//!
//! ## What this file checks
//! - **Copy** (`copy_tasks`): rows come out in project order as
//!   tab-separated text (TSV), with an optional header line in English or in
//!   the UI language, Excel-style quoting, `\r\n` line ends, and the
//!   predecessors written as row numbers *inside the copied text*.
//! - **Paste** (`paste_tasks`): reads TSV from Excel or from Ganttora, with
//!   or without a header line, ignores unknown columns, understands quoted
//!   cells, durations with units (`30m`, `4h`, `3d`, `2w`, `1mo`, `1,5d`) and
//!   the predecessor forms `3`, `3SS`, `3FS+2d`, `3FF-50%`, `3+1d`, lists
//!   with `,` or `;`. Every `PasteWarning` code is produced by the input
//!   that should produce it. Outline levels are fitted to the place of the
//!   paste. The result is one `Command::Batch`, so one Undo removes the
//!   whole paste.
//! - **Round trip**: copy, then paste into an empty project, gives the same
//!   names, durations, levels and links.
//! - **Determinism**: with a counter instead of random UUIDs, the same text
//!   gives exactly the same plan every time.
//!
//! ## Where it fits
//! Part of the `document` test target (`tests/rust/document/main.rs`). The
//! Tauri shell calls these functions for Ctrl+C / Ctrl+V in the grid
//! (`backend/src-tauri/src/ipc/clipboard.rs`).
//!
//! ## Main ideas
//! - In the text below, `\t` is a tab and `\r\n` a line break, the way Excel
//!   writes them.
//! - `counter()` replaces `Uuid::new_v4` so every ID is predictable: tasks
//!   get 1, 2, 3 …, then links continue the count.

use crate::{ctx, open_ok};
use document::clipboard::{
    COLUMN_ORDER, ClipboardColumn, ColumnLabels, PastePlan, PasteWarning, copy_tasks,
    format_duration, parse_duration, paste_tasks,
};
use document::{Command, Document};
use ganttora_tests::{add_task, days, project_on, row_id, shed};
use model::{Dependency, DependencyType, Duration, Project, Task, TaskId};
use serde_json::json;
use uuid::Uuid;

/// A stand-in for `Uuid::new_v4`: returns 1, 2, 3 … as UUIDs.
///
/// # Returns
/// A closure. Each call gives the next number.
///
/// # Beginner note
/// `move` makes the closure own its counter `n`, so the counter lives as
/// long as the closure and keeps counting between calls.
fn counter() -> impl FnMut() -> Uuid {
    let mut n: u128 = 0;
    move || {
        n += 1;
        Uuid::from_u128(n)
    }
}

/// Pastes `text` into `project` at `at` with English headings only.
fn plan(project: &Project, text: &str, at: usize) -> PastePlan {
    paste_tasks(project, text, at, None, None, counter())
}

/// The commands inside a plan's batch.
fn commands(plan: &PastePlan) -> &[Command] {
    match &plan.command {
        Command::Batch { commands } => commands,
        other => panic!("a paste must be a batch, got {other:?}"),
    }
}

/// The tasks the plan adds, in order.
fn added_tasks(plan: &PastePlan) -> Vec<Task> {
    commands(plan)
        .iter()
        .filter_map(|c| match c {
            Command::AddTask { task, .. } => Some(task.clone()),
            _ => None,
        })
        .collect()
}

/// The links the plan adds, in order.
fn added_links(plan: &PastePlan) -> Vec<Dependency> {
    commands(plan)
        .iter()
        .filter_map(|c| match c {
            Command::AddDependency { dependency } => Some(dependency.clone()),
            _ => None,
        })
        .collect()
}

/// The 1-based row of `id` among the plan's new tasks.
fn row_of(plan: &PastePlan, id: TaskId) -> usize {
    plan.task_ids
        .iter()
        .position(|t| *t == id)
        .map(|i| i + 1)
        .unwrap_or_else(|| panic!("{id} is not a pasted task"))
}

/// One link as (predecessor row, successor row, type, lag minutes, percent),
/// rows counted inside the plan, for easy comparison.
fn link_shape(
    plan: &PastePlan,
    d: &Dependency,
) -> (usize, usize, DependencyType, i64, Option<i32>) {
    (
        row_of(plan, d.predecessor),
        row_of(plan, d.successor),
        d.kind,
        d.lag.minutes(),
        d.lag_percent,
    )
}

/// The levels of the plan's new tasks.
fn levels(plan: &PastePlan) -> Vec<u32> {
    added_tasks(plan).iter().map(|t| t.outline_level).collect()
}

/// An empty project starting Mon 1 Mar 2027.
fn empty() -> Project {
    project_on("2027-03-01")
}

// =================================================================== copy

/// Copying the first three shed tasks gives exactly the text documented in
/// the module: no header, `\r\n` line ends, predecessors as text rows.
///
/// Why it matters: this is what Excel receives. A stray space or a missing
/// tab would shift every column in the spreadsheet.
#[test]
fn copy_three_shed_tasks_exact_text() {
    let project = shed();
    let schedule = engine::schedule(&project).expect("schedules");
    let ids = [
        row_id(&project, 1),
        row_id(&project, 2),
        row_id(&project, 3),
    ];
    let text = copy_tasks(&project, Some(&schedule), &ids, None);
    assert_eq!(
        text,
        "Design the shed\t2d\t2027-03-01 08:00\t2027-03-02 17:00\t\t1\r\n\
         Get building permit\t5d\t2027-03-03 08:00\t2027-03-09 17:00\t1\t1\r\n\
         Buy materials\t3d\t2027-03-03 08:00\t2027-03-05 17:00\t1\t1\r\n"
    );
}

/// The header line uses the labels it is given: English, or the UI
/// language's labels.
///
/// Why it matters: a German user pasting into German Excel expects German
/// column titles; the text comes from the locale files, never from Rust.
#[test]
fn copy_header_english_and_custom_labels() {
    let project = shed();
    let ids = [row_id(&project, 1)];
    let english = copy_tasks(&project, None, &ids, Some(&ColumnLabels::english()));
    assert!(english.starts_with("Name\tDuration\tStart\tFinish\tPredecessors\tOutline level\r\n"));

    let german = german_labels();
    let text = copy_tasks(&project, None, &ids, Some(&german));
    assert!(text.starts_with("Vorgangsname\tDauer\tAnfang\tEnde\tVorgänger\tGliederungsebene\r\n"));
}

/// German column labels, as the UI would send them.
fn german_labels() -> ColumnLabels {
    ColumnLabels {
        name: "Vorgangsname".into(),
        duration: "Dauer".into(),
        start: "Anfang".into(),
        finish: "Ende".into(),
        predecessors: "Vorgänger".into(),
        outline_level: "Gliederungsebene".into(),
    }
}

/// Rows come out in project order whatever order the IDs come in; unknown
/// IDs are ignored; links to rows that are not copied are left out; without
/// a schedule, Start and Finish are empty.
///
/// Why it matters: the user's selection order (click order) must not
/// scramble the copied block, and a predecessor number must always point
/// at a row that is actually in the text.
#[test]
fn copy_uses_project_order_and_drops_outside_links() {
    let project = shed();
    // Rows 5 and 4 (in that order) plus an unknown ID.
    let ids = [row_id(&project, 5), TaskId::new(), row_id(&project, 4)];
    let text = copy_tasks(&project, None, &ids, None);
    // Row 4 "Prepare the ground" waits for row 2 (not copied): no predecessor.
    // Row 5 "Build the floor" waits for rows 3 (not copied) and 4 (= text row 1).
    assert_eq!(
        text,
        "Prepare the ground\t2d\t\t\t\t1\r\nBuild the floor\t2d\t\t\t1\t1\r\n"
    );
}

/// An empty selection gives only the header line, or nothing at all.
///
/// Why it matters: Ctrl+C with nothing selected must not crash or put
/// garbage on the clipboard.
#[test]
fn copy_empty_selection() {
    let project = shed();
    assert_eq!(copy_tasks(&project, None, &[], None), "");
    assert_eq!(
        copy_tasks(&project, None, &[], Some(&ColumnLabels::english())),
        "Name\tDuration\tStart\tFinish\tPredecessors\tOutline level\r\n"
    );
}

/// Link types and lags are written the MS Project way: `2SS`, `2FS+2d`,
/// `2FF-50%`, `2SF`, `2FS+90m`, and a lead as `2FS-1d`.
///
/// Why it matters: the same text is read back by paste, by Excel users and
/// by MS Project; it must use the well-known short forms.
#[test]
fn copy_writes_link_codes_and_lags() {
    let mut project = empty();
    let a = add_task(&mut project, "A", days(2));
    let mut successors = Vec::new();
    let links = [
        Dependency::new(a, TaskId::new(), DependencyType::StartToStart),
        Dependency::finish_to_start(a, TaskId::new()).with_lag(days(2)),
        Dependency::new(a, TaskId::new(), DependencyType::FinishToFinish).with_lag_percent(-50),
        Dependency::new(a, TaskId::new(), DependencyType::StartToFinish),
        Dependency::finish_to_start(a, TaskId::new()).with_lag(Duration::from_minutes(90)),
        Dependency::finish_to_start(a, TaskId::new()).with_lag(days(-1)),
    ];
    for (i, mut link) in links.into_iter().enumerate() {
        let id = add_task(&mut project, &format!("S{i}"), days(1));
        link.successor = id;
        successors.push(id);
        project.dependencies.push(link);
    }
    let mut ids = vec![a];
    ids.extend(&successors);
    let text = copy_tasks(&project, None, &ids, None);
    let predecessor_cells: Vec<&str> = text
        .lines()
        .map(|line| line.split('\t').nth(4).unwrap_or(""))
        .collect();
    assert_eq!(
        predecessor_cells,
        ["", "1SS", "1FS+2d", "1FF-50%", "1SF", "1FS+90m", "1FS-1d"]
    );
}

/// A summary task is copied with its rolled-up duration (as shown on
/// screen), not its own stored duration.
///
/// Why it matters: a pasted copy of a phase should show the same length
/// the user saw in the grid.
#[test]
fn copy_summary_uses_rolled_up_duration() {
    let mut project = empty();
    let phase = add_task(&mut project, "Phase", days(1));
    let a = add_task(&mut project, "A", days(2));
    let b = add_task(&mut project, "B", days(3));
    project.tasks[1].outline_level = 2;
    project.tasks[2].outline_level = 2;
    project.dependencies.push(Dependency::finish_to_start(a, b));
    let schedule = engine::schedule(&project).expect("schedules");
    let text = copy_tasks(&project, Some(&schedule), &[phase], None);
    assert!(text.starts_with("Phase\t5d\t"), "got {text:?}");
}

/// `format_duration` writes whole days, else whole hours, else minutes.
///
/// Why it matters: the copied text never contains a decimal number, so it
/// reads the same in every language (no "1.5" vs "1,5" problem).
#[test]
fn format_duration_units() {
    assert_eq!(format_duration(Duration::ZERO, 480), "0d");
    assert_eq!(format_duration(days(3), 480), "3d");
    assert_eq!(format_duration(Duration::from_hours(12), 480), "12h");
    assert_eq!(format_duration(Duration::from_minutes(90), 480), "90m");
    // A 7-hour day: 420 minutes are one day.
    assert_eq!(format_duration(Duration::from_minutes(840), 420), "2d");
}

// ============================================================ round trip

/// Copy a block with a summary, subtasks and links of every kind, paste it
/// into an empty project: names, durations of the normal tasks, levels and
/// links (type and lag) come back the same.
///
/// Why it matters: copying rows from one project to another is the main use
/// of the clipboard; nothing the user typed may get lost on the way.
#[test]
fn copy_then_paste_round_trip() {
    let mut source = empty();
    let phase = add_task(&mut source, "Phase \"one\"", days(1));
    add_task(&mut source, "Sub-phase", days(1));
    let dig = add_task(&mut source, "Dig\tholes", days(2));
    let pour = add_task(&mut source, "Pour\nconcrete", Duration::from_hours(12));
    let inspect = add_task(&mut source, "Inspect", Duration::from_minutes(90));
    let done = add_task(&mut source, "Done", Duration::ZERO);
    for (row, level) in [(1, 2), (2, 3), (3, 3), (4, 2)] {
        source.tasks[row].outline_level = level;
    }
    source
        .dependencies
        .push(Dependency::new(dig, pour, DependencyType::StartToStart).with_lag(days(1)));
    source
        .dependencies
        .push(Dependency::new(pour, inspect, DependencyType::FinishToFinish).with_lag_percent(-50));
    source
        .dependencies
        .push(Dependency::finish_to_start(phase, done));
    let schedule = engine::schedule(&source).expect("schedules");
    let ids: Vec<TaskId> = source.tasks.iter().map(|t| t.id).collect();
    let text = copy_tasks(
        &source,
        Some(&schedule),
        &ids,
        Some(&ColumnLabels::english()),
    );

    let target = empty();
    let plan = plan(&target, &text, 0);
    assert!(plan.warnings.is_empty(), "warnings: {:?}", plan.warnings);
    let tasks = added_tasks(&plan);
    let names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Phase \"one\"",
            "Sub-phase",
            "Dig\tholes",
            "Pour\nconcrete",
            "Inspect",
            "Done"
        ]
    );
    assert_eq!(levels(&plan), [1, 2, 3, 3, 2, 1]);
    // Summary durations are rolled up; the normal tasks keep what was typed.
    let durations: Vec<i64> = tasks[2..].iter().map(|t| t.duration.minutes()).collect();
    assert_eq!(durations, [960, 720, 90, 0]);
    let links: Vec<_> = added_links(&plan)
        .iter()
        .map(|d| link_shape(&plan, d))
        .collect();
    assert_eq!(
        links,
        [
            (3, 4, DependencyType::StartToStart, 480, None),
            (4, 5, DependencyType::FinishToFinish, 0, Some(-50)),
            (1, 6, DependencyType::FinishToStart, 0, None),
        ]
    );

    // And the plan really applies.
    let mut document = open_ok(target);
    document.apply(plan.command, &ctx()).expect("paste applies");
    assert_eq!(document.project().tasks.len(), 6);
    assert_eq!(document.project().dependencies.len(), 3);
}

// =============================================================== headers

/// A first line with English headings decides the column order; headings
/// are matched without regard to case, spaces, `_` and `-`.
///
/// Why it matters: Excel sheets come with all kinds of header spellings
/// ("Task Name", "task_name", "DURATION").
#[test]
fn paste_reads_english_headers_in_any_order() {
    let text = "DURATION\tTask Name\toutline_level\r\n3d\tPaint\t1\r\n";
    let plan = plan(&empty(), text, 0);
    let tasks = added_tasks(&plan);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "Paint");
    assert_eq!(tasks[0].duration, days(3));
    assert_eq!(
        ClipboardColumn::from_header("Finish Date"),
        Some(ClipboardColumn::Finish)
    );
    assert_eq!(
        ClipboardColumn::from_header("predecessor"),
        Some(ClipboardColumn::Predecessors)
    );
    assert_eq!(ClipboardColumn::from_header(""), None);
}

/// A header in the UI language is recognised when its labels are passed.
///
/// Why it matters: text copied from a German Ganttora has German headings;
/// pasting it back must still find the columns.
#[test]
fn paste_reads_custom_label_headers() {
    let text = "Dauer\tVorgangsname\tVorgänger\r\n2d\tEntwurf\t\r\n1w\tGenehmigung\t1\r\n";
    let labels = german_labels();
    let plan = paste_tasks(&empty(), text, 0, None, Some(&labels), counter());
    assert!(plan.warnings.is_empty(), "warnings: {:?}", plan.warnings);
    let tasks = added_tasks(&plan);
    assert_eq!(tasks[0].name, "Entwurf");
    assert_eq!(tasks[1].duration, days(5));
    assert_eq!(added_links(&plan).len(), 1);
}

/// Unknown columns (Cost, Resource) are ignored; known ones still work.
///
/// Why it matters: users paste whole Excel sheets with many columns we do
/// not use yet. Those must not stop the paste or end up in the wrong field.
#[test]
fn paste_ignores_unknown_columns() {
    let text = "Cost\tName\tResource\tDuration\n100\tPaint\tAnna\t2d\n";
    let plan = plan(&empty(), text, 0);
    assert!(plan.warnings.is_empty());
    let tasks = added_tasks(&plan);
    assert_eq!(tasks[0].name, "Paint");
    assert_eq!(tasks[0].duration, days(2));
}

/// Without a header line, cells are read in `COLUMN_ORDER`
/// (Name, Duration, Start, Finish, Predecessors, Outline level); Start and
/// Finish are ignored.
///
/// Why it matters: Ganttora's own copy (without header) must paste back.
#[test]
fn paste_without_header_uses_column_order() {
    assert_eq!(COLUMN_ORDER[0], ClipboardColumn::Name);
    assert_eq!(COLUMN_ORDER[5], ClipboardColumn::OutlineLevel);
    let text = "Design\t2d\t2030-01-01 08:00\t2030-01-09 17:00\t\t1\n\
                Permit\t5d\t\t\t1\t1\n";
    let plan = plan(&empty(), text, 0);
    assert!(plan.warnings.is_empty());
    let tasks = added_tasks(&plan);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "Design");
    assert_eq!(tasks[1].duration, days(5));
    // The start/finish dates did not become constraints.
    assert!(
        tasks
            .iter()
            .all(|t| t.constraint == model::Constraint::AsSoonAsPossible)
    );
    assert_eq!(added_links(&plan).len(), 1);
}

/// A row with only a name (no other cells) gets a 1-day duration.
///
/// Why it matters: pasting a plain list of names from a text file is the
/// quickest way to start a plan.
#[test]
fn paste_plain_list_of_names() {
    let plan = plan(&empty(), "Design\nPermit\n\nBuild\n", 0);
    let tasks = added_tasks(&plan);
    let names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Design", "Permit", "Build"], "blank line skipped");
    assert!(tasks.iter().all(|t| t.duration == days(1)));
    assert!(plan.warnings.is_empty());
}

// ================================================================ quoting

/// Quoted cells may contain a tab, a line break and doubled quotes `""`;
/// lines may end with `\r\n` or `\n`.
///
/// Why it matters: Excel quotes exactly these cells. If we split inside a
/// quoted cell, one task would turn into two half-tasks.
#[test]
fn paste_quoted_cells_and_crlf() {
    let text = "\"Paint \"\"red\"\" shed\"\t2d\r\n\"Line one\nline two\"\t1d\r\n\"a\tb\"\t3d\n";
    let plan = plan(&empty(), text, 0);
    let names: Vec<String> = added_tasks(&plan).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["Paint \"red\" shed", "Line one\nline two", "a\tb"]);
    let durations: Vec<i64> = added_tasks(&plan)
        .iter()
        .map(|t| t.duration.minutes())
        .collect();
    assert_eq!(durations, [960, 480, 1440]);
}

// ============================================================== durations

/// Every documented duration unit, a decimal comma, the `?` "estimated"
/// mark and a missing unit (= days).
///
/// Why it matters: users type durations the MS Project way; "1,5d" in
/// Germany means one and a half days.
#[test]
fn parse_duration_units() {
    let minutes = |text: &str| parse_duration(text, 480).map(|d| d.minutes());
    assert_eq!(minutes("30m"), Some(30));
    assert_eq!(minutes("4h"), Some(240));
    assert_eq!(minutes("4 hours"), Some(240));
    assert_eq!(minutes("3d"), Some(1440));
    assert_eq!(minutes("3"), Some(1440));
    assert_eq!(minutes("2w"), Some(4800));
    assert_eq!(minutes("1mo"), Some(9600));
    assert_eq!(minutes("1,5d"), Some(720));
    assert_eq!(minutes("1.5 days"), Some(720));
    assert_eq!(minutes("2d?"), Some(960));
    assert_eq!(minutes("-1d"), Some(-480));
    assert_eq!(minutes("3x"), None);
    assert_eq!(minutes("3ed"), None, "elapsed days are not supported yet");
    assert_eq!(minutes("d"), None, "no number");
    assert_eq!(minutes(""), None);
}

/// On paste: `1,5d` gives 720 minutes; `3x` gives a warning
/// `unreadableDuration` with the text and the task gets 1 day; a negative
/// duration also gives the warning; an empty cell gives 1 day silently.
///
/// Why it matters: one bad cell must not stop a 50-row paste, but the user
/// must be told which row was guessed.
#[test]
fn paste_durations_with_warnings() {
    let text = "Name\tDuration\nA\t1,5d\nB\t3x\nC\t-2d\nD\t\n";
    let plan = plan(&empty(), text, 0);
    let durations: Vec<i64> = added_tasks(&plan)
        .iter()
        .map(|t| t.duration.minutes())
        .collect();
    assert_eq!(durations, [720, 480, 480, 480]);
    assert_eq!(
        plan.warnings,
        [
            PasteWarning::UnreadableDuration {
                row: 2,
                text: "3x".into()
            },
            PasteWarning::UnreadableDuration {
                row: 3,
                text: "-2d".into()
            },
        ]
    );
}

/// "1 day" follows the project's own day length (a 7-hour day here).
///
/// Why it matters: a project with 7-hour days must not get 8-hour tasks
/// from a paste.
#[test]
fn paste_default_day_follows_minutes_per_day() {
    let mut project = empty();
    project.minutes_per_day = 420;
    let plan = plan(&project, "A\nB\t2d\n", 0);
    let durations: Vec<i64> = added_tasks(&plan)
        .iter()
        .map(|t| t.duration.minutes())
        .collect();
    assert_eq!(durations, [420, 840]);
}

// ============================================================ predecessors

/// The predecessor forms `3`, `3SS`, `3FS+2d`, `3FF-50%`, `3+1d`, and lower
/// case `3sf`, each give the right type and lag.
///
/// Why it matters: these are the forms MS Project shows in its Predecessors
/// column; text copied from MS Project must keep its links.
#[test]
fn paste_predecessor_forms() {
    let text = "Name\tPredecessors\n\
                A\t\n\
                B\t\n\
                C\t\n\
                D\t3\n\
                E\t3SS\n\
                F\t3FS+2d\n\
                G\t3FF-50%\n\
                H\t3+1d\n\
                I\t3sf\n";
    let plan = plan(&empty(), text, 0);
    assert!(plan.warnings.is_empty(), "warnings: {:?}", plan.warnings);
    let links: Vec<_> = added_links(&plan)
        .iter()
        .map(|d| link_shape(&plan, d))
        .collect();
    assert_eq!(
        links,
        [
            (3, 4, DependencyType::FinishToStart, 0, None),
            (3, 5, DependencyType::StartToStart, 0, None),
            (3, 6, DependencyType::FinishToStart, 960, None),
            (3, 7, DependencyType::FinishToFinish, 0, Some(-50)),
            (3, 8, DependencyType::FinishToStart, 480, None),
            (3, 9, DependencyType::StartToFinish, 0, None),
        ]
    );
}

/// A list separated by `,` and a list separated by `;` (where `1,5d` is a
/// decimal lag) both give two links.
///
/// Why it matters: languages with a decimal comma use `;` between list
/// entries; "1;2SS+1,5d" must not be split at the decimal comma.
#[test]
fn paste_predecessor_lists_with_comma_and_semicolon() {
    let text = "Name\tPredecessors\nA\t\nB\t\nC\t1,2SS\nD\t1;2SS+1,5d\n";
    let plan = plan(&empty(), text, 0);
    assert!(plan.warnings.is_empty(), "warnings: {:?}", plan.warnings);
    let links: Vec<_> = added_links(&plan)
        .iter()
        .map(|d| link_shape(&plan, d))
        .collect();
    assert_eq!(
        links,
        [
            (1, 3, DependencyType::FinishToStart, 0, None),
            (2, 3, DependencyType::StartToStart, 0, None),
            (1, 4, DependencyType::FinishToStart, 0, None),
            (2, 4, DependencyType::StartToStart, 720, None),
        ]
    );
}

// ================================================================ warnings

/// Each `PasteWarning` code comes from the input meant to cause it, and the
/// paste still goes ahead without the bad part:
/// `unreadableOutlineLevel` ("x", "0"), `unreadablePredecessor` ("2XX",
/// "2FS2d"), `predecessorNotPasted` (row 9, row 0), `selfLink`,
/// `duplicateLink`, `linkToOwnSummary`.
///
/// Why it matters: the UI shows these codes as translated notices; each
/// one must really be produced, with the right row numbers.
#[test]
fn paste_produces_every_warning_code() {
    let text = "Name\tPredecessors\tOutline level\n\
                Phase\t\t1\n\
                Child\t1\t2\n\
                Loose\t2XX,2FS2d\tx\n\
                Far\t9,0\t1\n\
                Me\t5\t0\n\
                Twice\t4,4SS\t1\n";
    let plan = plan(&empty(), text, 0);
    assert_eq!(
        plan.warnings,
        [
            PasteWarning::UnreadableOutlineLevel {
                row: 3,
                text: "x".into()
            },
            PasteWarning::UnreadableOutlineLevel {
                row: 5,
                text: "0".into()
            },
            PasteWarning::LinkToOwnSummary {
                row: 2,
                predecessor_row: 1
            },
            PasteWarning::UnreadablePredecessor {
                row: 3,
                text: "2XX".into()
            },
            PasteWarning::UnreadablePredecessor {
                row: 3,
                text: "2FS2d".into()
            },
            PasteWarning::PredecessorNotPasted {
                row: 4,
                predecessor_row: 9
            },
            PasteWarning::PredecessorNotPasted {
                row: 4,
                predecessor_row: 0
            },
            PasteWarning::SelfLink { row: 5 },
            PasteWarning::DuplicateLink {
                row: 6,
                predecessor_row: 4
            },
        ]
    );
    // "Loose" had level "x": it takes the level of the row above (2).
    assert_eq!(levels(&plan), [1, 2, 2, 1, 1, 1]);
    // Only the first "4" of "Twice" became a link.
    let links: Vec<_> = added_links(&plan)
        .iter()
        .map(|d| link_shape(&plan, d))
        .collect();
    assert_eq!(links, [(4, 6, DependencyType::FinishToStart, 0, None)]);
}

/// The warnings travel to the UI as `{ "code": …, "row": …, … }` with
/// camelCase names.
///
/// Why it matters: the frontend picks the translated text by `code`
/// (`paste.*` keys); a renamed field would show an empty notice.
#[test]
fn paste_warning_json_shape() {
    let warning = PasteWarning::UnreadableDuration {
        row: 2,
        text: "3x".into(),
    };
    assert_eq!(
        serde_json::to_value(&warning).unwrap(),
        json!({ "code": "unreadableDuration", "row": 2, "text": "3x" })
    );
    let warning = PasteWarning::PredecessorNotPasted {
        row: 4,
        predecessor_row: 9,
    };
    assert_eq!(
        serde_json::to_value(&warning).unwrap(),
        json!({ "code": "predecessorNotPasted", "row": 4, "predecessorRow": 9 })
    );
    assert_eq!(
        serde_json::to_value(PasteWarning::SelfLink { row: 1 }).unwrap(),
        json!({ "code": "selfLink", "row": 1 })
    );
}

// ========================================================= outline levels

/// The documented example: levels 2, 3, 3, 5 pasted above a level-1 row
/// become 1, 2, 2, 3 (relative depth kept, never more than one level below
/// the row before).
///
/// Why it matters: a block copied from deep inside one project must fit
/// wherever it is pasted, or the paste would be refused as a broken outline.
#[test]
fn paste_fits_levels_above_a_top_level_row() {
    let text = "Name\tOutline level\nA\t2\nB\t3\nC\t3\nD\t5\n";
    let plan = plan(&shed(), text, 2);
    assert_eq!(levels(&plan), [1, 2, 2, 3]);
}

/// Pasted at the end of a project whose last row is at level 2, the first
/// pasted row becomes level 2 too; rows never go above the first one.
///
/// Why it matters: pasting below the last subtask of a phase should add to
/// that phase, as in MS Project.
#[test]
fn paste_at_end_fits_to_the_last_row() {
    let mut project = empty();
    add_task(&mut project, "Phase", days(1));
    add_task(&mut project, "Sub", days(1));
    project.tasks[1].outline_level = 2;
    let text = "Name\tOutline level\nA\t3\nB\t1\nC\t4\n";
    let plan = plan(&project, text, 2);
    // A → 2 (base); B would be 0 → clamped to 2; C → 2 + (4 - 3) = 3.
    assert_eq!(levels(&plan), [2, 2, 3]);

    let mut document = open_ok(project);
    document.apply(plan.command, &ctx()).expect("valid outline");
}

// ======================================================= where it is pasted

/// Pasted in the middle (above row 3 of the shed), the new rows land at
/// rows 3 and 4 and the old row 3 moves down. One Undo removes the whole
/// paste, links included.
///
/// Why it matters: "paste here" must put the rows where the user clicked,
/// and Ctrl+Z must take back the whole paste in one step.
#[test]
fn paste_in_the_middle_and_undo() {
    let project = shed();
    let before = project.clone();
    let mut document = open_ok(project);
    let text = "Name\tDuration\tPredecessors\nOrder door\t1d\t\nFit door\t2h\t1\n";
    let plan = paste_tasks(document.project(), text, 2, None, None, counter());
    let first = commands(&plan)[0].clone();
    assert!(matches!(first, Command::AddTask { index: 2, .. }));

    document.apply(plan.command.clone(), &ctx()).expect("paste");
    let tasks = &document.project().tasks;
    assert_eq!(tasks.len(), 10);
    assert_eq!(tasks[2].name, "Order door");
    assert_eq!(tasks[3].name, "Fit door");
    assert_eq!(tasks[4].name, "Buy materials");
    assert_eq!(document.project().dependencies.len(), 9);

    document.undo(&ctx()).expect("undo");
    assert_eq!(document.project(), &before, "one undo removes the paste");
    assert!(!document.can_undo());
}

/// Pasted at the end (index = number of tasks), the rows are appended.
///
/// Why it matters: "paste below the last row" is the most common paste.
#[test]
fn paste_at_the_end() {
    let project = shed();
    let mut document = open_ok(project);
    let plan = paste_tasks(
        document.project(),
        "Clean up\t1d\n",
        8,
        None,
        None,
        counter(),
    );
    document.apply(plan.command, &ctx()).expect("paste");
    let tasks = &document.project().tasks;
    assert_eq!(tasks.len(), 9);
    assert_eq!(tasks[8].name, "Clean up");
    assert_eq!(plan.task_ids, [tasks[8].id]);
}

/// Text with no data rows gives an empty batch; applying it changes nothing
/// and adds no undo step.
///
/// Why it matters: Ctrl+V with an empty clipboard must be harmless.
#[test]
fn paste_of_empty_text_changes_nothing() {
    let project = shed();
    let mut document: Document = open_ok(project);
    for text in ["", "\r\n\r\n", "Name\tDuration\r\n"] {
        let plan = paste_tasks(document.project(), text, 0, None, None, counter());
        assert!(commands(&plan).is_empty(), "text {text:?}");
        assert!(plan.task_ids.is_empty());
        let outcome = document.apply(plan.command, &ctx()).expect("ok");
        assert!(outcome.changes.is_empty());
    }
    assert!(!document.can_undo());
}

// ============================================================ determinism

/// With a counter for new IDs, the same text gives exactly the same plan
/// every time: tasks get IDs 1, 2, 3 in row order, then the links continue
/// with 4, 5.
///
/// Why it matters: the document never invents random IDs.
/// Tests and the journal depend on a paste being repeatable.
#[test]
fn paste_is_deterministic_with_a_counter() {
    let project = shed();
    let text = "Name\tPredecessors\nA\t\nB\t1\nC\t1,2SS\n";
    let first = plan(&project, text, 3);
    let second = plan(&project, text, 3);
    assert_eq!(first, second);
    let task_ids: Vec<Uuid> = first.task_ids.iter().map(|t| t.as_uuid()).collect();
    assert_eq!(
        task_ids,
        [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)]
    );
    let link_ids: Vec<Uuid> = added_links(&first).iter().map(|d| d.id.as_uuid()).collect();
    assert_eq!(
        link_ids,
        [Uuid::from_u128(4), Uuid::from_u128(5), Uuid::from_u128(6)]
    );
    // AddTask commands come first, in row order, then AddDependency.
    let kinds: Vec<&str> = commands(&first)
        .iter()
        .map(|c| match c {
            Command::AddTask { .. } => "task",
            Command::AddDependency { .. } => "link",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["task", "task", "task", "link", "link", "link"]);
}

/// A wished level wins over the row below, but never breaks the outline.
///
/// Why it matters: pasting below the last sub-task must give a sub-task,
/// although the next row is a task.
#[test]
fn paste_with_a_level() {
    let mut project = shed();
    project.tasks[1].outline_level = 2; // row 2 is a sub-task of row 1
    let level = |wished| {
        let plan = paste_tasks(&project, "Door\n", 2, wished, None, counter());
        added_tasks(&plan)[0].outline_level
    };
    assert_eq!(level(None), 1);
    assert_eq!(level(Some(2)), 2);
    assert_eq!(level(Some(9)), 3, "at most one below the row above");
}
