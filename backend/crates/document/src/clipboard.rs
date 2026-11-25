//! # Clipboard: copy and paste task rows as tab-separated text
//!
//! ## What this file is for
//! Lets the user copy task rows out of Ganttora (into Excel, an e-mail, a
//! text file) and paste rows in (from Excel or from Ganttora itself). The
//! clipboard carries plain **tab-separated text** (TSV): one line per task,
//! one cell per column, cells separated by a tab character. This is the
//! format Excel itself puts on the clipboard, so both directions work with
//! Excel without any extra step.
//!
//! ## Where it fits
//! ```text
//!  Copy:   UI selection ──► shell ──► copy_tasks(project, schedule, ids) ──► text ──► OS clipboard
//!  Paste:  OS clipboard ──► text ──► shell ──► paste_tasks(project, text, row, …)
//!                                                   │
//!                                                   ▼
//!                                     PastePlan { command: Batch[AddTask…, AddDependency…],
//!                                                 warnings }
//!                                                   │
//!                                     shell ──► Document::apply(command)   (ONE undo step)
//! ```
//! This file does no I/O: it never touches the operating system's
//! clipboard. The Tauri shell reads and writes the clipboard and calls the
//! two functions here. A paste becomes an ordinary [`Command::Batch`], so
//! it goes through the normal front door ([`crate::Document::apply`]),
//! is checked like any other edit, and one Undo removes the whole paste.
//!
//! ## The columns
//! | Column | Copied as | On paste |
//! |---|---|---|
//! | Name | the task name | the new task's name |
//! | Duration | e.g. `3d`, `4h`, `30m` (summary tasks: rolled up) | parsed (see [`parse_duration`]); empty or missing = 1 day |
//! | Start | scheduled start, `2027-03-01 08:00` | **ignored** (the engine calculates it) |
//! | Finish | scheduled finish, `2027-03-02 17:00` | **ignored** (the engine calculates it) |
//! | Predecessors | e.g. `1`, `2SS+1d`, `3FF-50%` | links between the pasted rows |
//! | Outline level | `1`, `2`, `3`… | the indent, fitted to the place of the paste |
//!
//! **Predecessors are row numbers inside the clipboard text**, not row
//! numbers in the project: `1` means "the first data row of this text". So
//! a copied block keeps its links when it is pasted somewhere else. Links to
//! tasks outside the copied rows are left out when copying.
//!
//! ## Worked example: the garden shed
//! Copying the first three shed tasks gives (→ stands for a tab):
//! ```text
//! Name→Duration→Start→Finish→Predecessors→Outline level
//! Design the shed→2d→2027-03-01 08:00→2027-03-02 17:00→→1
//! Get building permit→5d→2027-03-03 08:00→2027-03-09 17:00→1→1
//! Buy materials→3d→2027-03-03 08:00→2027-03-05 17:00→1→1
//! ```
//! (The header line is there only when the caller asks for one.) Pasting
//! this text at the end of an empty project creates three tasks (2d, 5d,
//! 3d) and two links (1→2, 1→3): one Undo removes all five items.
//!
//! ## Deliberate simplifications (to check against MS Project)
//! - Pasted Start and Finish are ignored. MS Project, when a start date is
//!   pasted into an automatically scheduled task, adds a "Start No Earlier
//!   Than" constraint instead.
//! - Durations are copied in whole days, hours or minutes (`12h`, not
//!   `1.5 days`), so the text never depends on the decimal separator of a
//!   language.
//! - Elapsed durations (`3ed`) are not supported yet.
//! - **Blockers are not copied**. A blocker is the history of
//!   one particular task ("the wood for *this* order was late"), not part
//!   of what the task is, so a pasted copy starts without blockers. The
//!   copied Duration is the planned work (e.g. `3d`), while the copied
//!   Finish is the scheduled finish, which includes any blocked time.
//!
//! ## Documentation

use crate::command::Command;
use chrono::NaiveDateTime;
use engine::ScheduleResult;
use model::{Dependency, DependencyId, DependencyType, Duration, Project, Task, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// One of the columns this file knows how to copy and paste.
///
/// # Example
/// ```
/// use document::clipboard::ClipboardColumn;
/// assert_eq!(ClipboardColumn::from_header("Task Name"), Some(ClipboardColumn::Name));
/// assert_eq!(ClipboardColumn::from_header("Cost"), None); // unknown columns are ignored
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardColumn {
    /// The task name.
    Name,
    /// The duration, e.g. `3d`.
    Duration,
    /// The scheduled start (copied only).
    Start,
    /// The scheduled finish (copied only).
    Finish,
    /// The predecessors, as row numbers inside the text.
    Predecessors,
    /// The outline level (1 = top level).
    OutlineLevel,
}

/// The columns in the order [`copy_tasks`] writes them. Text pasted
/// **without** a header line is read in this order too.
pub const COLUMN_ORDER: [ClipboardColumn; 6] = [
    ClipboardColumn::Name,
    ClipboardColumn::Duration,
    ClipboardColumn::Start,
    ClipboardColumn::Finish,
    ClipboardColumn::Predecessors,
    ClipboardColumn::OutlineLevel,
];

impl ClipboardColumn {
    /// Recognises an English column heading, as MS Project and most
    /// spreadsheets write it. Case, spaces, `_` and `-` do not matter.
    ///
    /// # Parameters
    /// - `text`: one cell of the first line, e.g. `"Task Name"`.
    ///
    /// # Returns
    /// The column, or `None` for a heading this file does not know (such a
    /// column is ignored on paste).
    ///
    /// # Example
    /// ```
    /// use document::clipboard::ClipboardColumn;
    /// assert_eq!(ClipboardColumn::from_header("outline_level"), Some(ClipboardColumn::OutlineLevel));
    /// assert_eq!(ClipboardColumn::from_header("  DURATION "), Some(ClipboardColumn::Duration));
    /// ```
    pub fn from_header(text: &str) -> Option<ClipboardColumn> {
        match squash(text).as_str() {
            "name" | "taskname" | "task" => Some(ClipboardColumn::Name),
            "duration" => Some(ClipboardColumn::Duration),
            "start" | "startdate" => Some(ClipboardColumn::Start),
            "finish" | "finishdate" | "end" | "enddate" => Some(ClipboardColumn::Finish),
            "predecessors" | "predecessor" => Some(ClipboardColumn::Predecessors),
            "outlinelevel" | "level" => Some(ClipboardColumn::OutlineLevel),
            _ => None,
        }
    }
}

/// The column headings in the user's language, sent by the UI.
///
/// # What it is for
/// UI text is never hard-coded in Rust: the frontend takes
/// these headings from `locales/*.json` and passes them in. [`copy_tasks`]
/// writes them as the header line; [`paste_tasks`] recognises them (as well
/// as the English headings of [`ClipboardColumn::from_header`]).
///
/// # JSON (what the UI sends)
/// ```text
/// { "name": "Vorgangsname", "duration": "Dauer", "start": "Anfang",
///   "finish": "Ende", "predecessors": "Vorgänger", "outlineLevel": "Gliederungsebene" }
/// ```
///
/// # Example
/// ```
/// use document::clipboard::ColumnLabels;
/// let labels = ColumnLabels::english();
/// assert_eq!(labels.outline_level, "Outline level");
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnLabels {
    /// Heading of the Name column.
    pub name: String,
    /// Heading of the Duration column.
    pub duration: String,
    /// Heading of the Start column.
    pub start: String,
    /// Heading of the Finish column.
    pub finish: String,
    /// Heading of the Predecessors column.
    pub predecessors: String,
    /// Heading of the Outline level column.
    pub outline_level: String,
}

impl ColumnLabels {
    /// English headings. Useful for tests and as a fallback; the app passes
    /// the headings of the UI language instead.
    ///
    /// # Example
    /// ```
    /// assert_eq!(document::clipboard::ColumnLabels::english().name, "Name");
    /// ```
    pub fn english() -> ColumnLabels {
        ColumnLabels {
            name: "Name".into(),
            duration: "Duration".into(),
            start: "Start".into(),
            finish: "Finish".into(),
            predecessors: "Predecessors".into(),
            outline_level: "Outline level".into(),
        }
    }

    /// The heading of one column.
    fn get(&self, column: ClipboardColumn) -> &str {
        match column {
            ClipboardColumn::Name => &self.name,
            ClipboardColumn::Duration => &self.duration,
            ClipboardColumn::Start => &self.start,
            ClipboardColumn::Finish => &self.finish,
            ClipboardColumn::Predecessors => &self.predecessors,
            ClipboardColumn::OutlineLevel => &self.outline_level,
        }
    }

    /// The column whose heading is `text` (compared like
    /// [`ClipboardColumn::from_header`]: case and spaces do not matter).
    fn find(&self, text: &str) -> Option<ClipboardColumn> {
        let wanted = squash(text);
        COLUMN_ORDER
            .into_iter()
            .find(|&column| !wanted.is_empty() && squash(self.get(column)) == wanted)
    }
}

/// Something in the pasted text that could not be used as it was. The
/// paste still goes ahead; the UI may show these as a short notice.
///
/// `row` is always the **data row** in the pasted text, counted from 1
/// (the header line and blank lines are not counted).
///
/// # JSON (what the UI receives)
/// A code plus the details, translated in the frontend (`paste.*` keys):
/// ```text
/// { "code": "unreadableDuration", "row": 2, "text": "3x" }
/// { "code": "predecessorNotPasted", "row": 4, "predecessorRow": 9 }
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PasteWarning {
    /// The duration could not be read (or was negative); the task got 1 day.
    UnreadableDuration {
        /// The data row (from 1).
        row: usize,
        /// The cell as it was pasted.
        text: String,
    },
    /// The outline level was not a whole number of 1 or more; the row got
    /// the level of the row above it.
    UnreadableOutlineLevel {
        /// The data row (from 1).
        row: usize,
        /// The cell as it was pasted.
        text: String,
    },
    /// One entry of the Predecessors cell could not be read; it was skipped.
    UnreadablePredecessor {
        /// The data row (from 1).
        row: usize,
        /// The entry as it was pasted, e.g. `"2XX"`.
        text: String,
    },
    /// A predecessor names a row that is not part of the pasted text; the
    /// link was skipped.
    PredecessorNotPasted {
        /// The data row (from 1).
        row: usize,
        /// The row number it named.
        predecessor_row: usize,
    },
    /// A row names itself as its predecessor; skipped.
    SelfLink {
        /// The data row (from 1).
        row: usize,
    },
    /// The same predecessor appears twice in one row; the second was skipped
    /// (only one link per pair of tasks is allowed).
    DuplicateLink {
        /// The data row (from 1).
        row: usize,
        /// The row number named twice.
        predecessor_row: usize,
    },
    /// A link between a summary task and one of its own subtasks; skipped
    /// (the summary task would wait for itself).
    LinkToOwnSummary {
        /// The data row (from 1).
        row: usize,
        /// The row number it named.
        predecessor_row: usize,
    },
}

/// What [`paste_tasks`] prepared: the command to apply, and notes about
/// cells that could not be used.
///
/// # Fields
/// - `command`: a [`Command::Batch`] with one `AddTask` per pasted row (in
///   row order) followed by one `AddDependency` per link. Apply it with
///   [`crate::Document::apply`]: one undo step.
/// - `task_ids`: the IDs of the new tasks, in row order (so the UI can
///   select the pasted rows afterwards).
/// - `warnings`: what could not be used.
///
/// # JSON
/// `{ "command": { "type": "batch", … }, "taskIds": [ … ], "warnings": [ … ] }`
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastePlan {
    /// The batch of commands that performs the paste.
    pub command: Command,
    /// The new tasks' IDs, in row order.
    pub task_ids: Vec<TaskId>,
    /// Cells that could not be used as they were.
    pub warnings: Vec<PasteWarning>,
}

// ---------------------------------------------------------------------------
// Copy.
// ---------------------------------------------------------------------------

/// Turns the selected tasks into tab-separated text for the clipboard.
///
/// # What it does
/// Writes one line per selected task, **in project row order** (whatever
/// order the IDs come in), with the columns of [`COLUMN_ORDER`]. Each line
/// ends with `\r\n`, as Excel's own clipboard text does.
///
/// # Parameters
/// - `project`: the open project.
/// - `schedule`: the current schedule, for the Start and Finish columns
///   (`None` = leave those cells empty, e.g. while the schedule has an
///   error).
/// - `ids`: the selected tasks. Unknown IDs are ignored. Subtasks of a
///   selected summary task are **not** added automatically; the UI decides
///   what is selected.
/// - `header`: headings for a first line, or `None` for no header line.
///
/// # Returns
/// The text. An empty selection gives just the header line (or `""`).
///
/// # Example
/// ```
/// use chrono::NaiveDate;
/// use document::clipboard::copy_tasks;
/// use model::{Dependency, Duration, Project, Task};
/// let mut project = Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let design = Task::new("Design the shed", Duration::from_days(2));
/// let permit = Task::new("Get building permit", Duration::from_days(5));
/// project.dependencies.push(Dependency::finish_to_start(design.id, permit.id));
/// let ids = [design.id, permit.id];
/// project.tasks.extend([design, permit]);
/// let schedule = engine::schedule(&project).unwrap();
///
/// let text = copy_tasks(&project, Some(&schedule), &ids, None);
/// assert_eq!(
///     text,
///     "Design the shed\t2d\t2027-03-01 08:00\t2027-03-02 17:00\t\t1\r\n\
///      Get building permit\t5d\t2027-03-03 08:00\t2027-03-09 17:00\t1\t1\r\n"
/// );
/// ```
pub fn copy_tasks(
    project: &Project,
    schedule: Option<&ScheduleResult>,
    ids: &[TaskId],
    header: Option<&ColumnLabels>,
) -> String {
    // A `HashSet` answers "is this task selected?" quickly, even for 10 000
    // rows. We only ask it yes/no questions and never walk through it, so
    // its unpredictable order cannot make the output change between runs.
    let selected: HashSet<TaskId> = ids.iter().copied().collect();
    let rows: Vec<&Task> = project
        .tasks
        .iter()
        .filter(|t| selected.contains(&t.id))
        .collect();
    // Row number inside the text (1 = first copied task), by task ID.
    let row_of: HashMap<TaskId, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i + 1))
        .collect();

    let mut text = String::new();
    if let Some(labels) = header {
        let cells: Vec<String> = COLUMN_ORDER
            .iter()
            .map(|&c| quote_cell(labels.get(c)))
            .collect();
        push_line(&mut text, &cells);
    }
    for task in &rows {
        let dates = schedule.and_then(|s| s.task(task.id));
        // A summary task shows its rolled-up duration (as on screen); other
        // tasks their own typed duration.
        let duration = match dates {
            Some(d) if d.is_summary => d.duration,
            _ => task.duration,
        };
        let cells = [
            quote_cell(&task.name),
            format_duration(duration, project.minutes_per_day),
            dates.map(|d| format_date(d.start)).unwrap_or_default(),
            dates.map(|d| format_date(d.finish)).unwrap_or_default(),
            format_predecessors(project, task.id, &row_of),
            task.outline_level.to_string(),
        ];
        push_line(&mut text, &cells);
    }
    text
}

/// Writes a duration the short way MS Project does: whole days (`3d`) if
/// it is a whole number of days, otherwise whole hours (`12h`), otherwise
/// minutes (`90m`).
///
/// # Parameters
/// - `duration`: the working time; may be negative (a lead in a lag).
/// - `minutes_per_day`: how many minutes make one day (480 by default).
///
/// # Returns
/// The text. Zero is written `0d`.
///
/// # Example
/// ```
/// use document::clipboard::format_duration;
/// use model::Duration;
/// assert_eq!(format_duration(Duration::from_days(3), 480), "3d");
/// assert_eq!(format_duration(Duration::from_hours(12), 480), "12h"); // 1.5 days
/// assert_eq!(format_duration(Duration::from_minutes(90), 480), "90m");
/// assert_eq!(format_duration(Duration::from_days(-1), 480), "-1d");
/// ```
pub fn format_duration(duration: Duration, minutes_per_day: i64) -> String {
    let minutes = duration.minutes();
    // `%` is the remainder of a division: 1440 % 480 = 0 (exactly 3 days),
    // 720 % 480 = 240 (not a whole number of days).
    if minutes_per_day > 0 && minutes % minutes_per_day == 0 {
        format!("{}d", minutes / minutes_per_day)
    } else if minutes % 60 == 0 {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

/// Writes a date the way spreadsheets understand: `2027-03-01 08:00`.
fn format_date(moment: NaiveDateTime) -> String {
    moment.format("%Y-%m-%d %H:%M").to_string()
}

/// Builds the Predecessors cell of one task: its links whose predecessor is
/// also copied, as `row[TYPE][±lag]`, separated by commas.
///
/// Worked examples (row 2 is the predecessor):
/// FS without lag → `2` · FS + 2 days → `2FS+2d` · SS → `2SS` ·
/// FF with a lead of 50 % → `2FF-50%`.
fn format_predecessors(project: &Project, task: TaskId, row_of: &HashMap<TaskId, usize>) -> String {
    let mut parts = Vec::new();
    for link in project.dependencies.iter().filter(|d| d.successor == task) {
        // A link from a task that is not copied is left out: its row number
        // would mean nothing in the pasted text.
        let Some(row) = row_of.get(&link.predecessor) else {
            continue;
        };
        let code = match link.kind {
            DependencyType::FinishToStart => "FS",
            DependencyType::StartToStart => "SS",
            DependencyType::FinishToFinish => "FF",
            DependencyType::StartToFinish => "SF",
        };
        let lag = match link.lag_percent {
            Some(0) | None if link.lag.is_zero() => String::new(),
            Some(p) => format!("{p:+}%"),
            None => {
                let text = format_duration(link.lag, project.minutes_per_day);
                // `format_duration` writes "-2d" for a lead; add "+" for a lag.
                if link.lag.minutes() > 0 {
                    format!("+{text}")
                } else {
                    text
                }
            }
        };
        if link.kind == DependencyType::FinishToStart && lag.is_empty() {
            // The most common link is written as just the row, as in MS Project.
            parts.push(row.to_string());
        } else {
            parts.push(format!("{row}{code}{lag}"));
        }
    }
    parts.join(",")
}

/// Puts a cell in double quotes if it contains a tab, a line break or a
/// double quote (doubling the quotes inside), the way Excel does. Other
/// cells are returned unchanged.
///
/// Example: `Paint "red" shed` → `"Paint ""red"" shed"`.
fn quote_cell(text: &str) -> String {
    if text.contains(['\t', '\n', '\r', '"']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Appends one line: the cells joined by tabs, then `\r\n`.
fn push_line(text: &mut String, cells: &[String]) {
    text.push_str(&cells.join("\t"));
    text.push_str("\r\n");
}

// ---------------------------------------------------------------------------
// Paste.
// ---------------------------------------------------------------------------

/// Prepares a paste: reads tab-separated text and builds the command that
/// inserts the rows as new tasks at row `at_index`.
///
/// # What it does
/// 1. Splits the text into lines and cells (Excel's quoting rules).
/// 2. If the first line contains a known heading (English, or one of
///    `labels`), it is the header line and decides which column is which;
///    unknown columns are ignored. Otherwise the columns are read in
///    [`COLUMN_ORDER`]. Blank lines are skipped. (So a first line that
///    contains a known heading is always taken as the header: a single row
///    named "Duration" would be read as a header, not as a task.)
/// 3. Makes one task per data row: name, duration (1 day if missing or
///    unreadable), outline level fitted to the place of the paste (see
///    below).
/// 4. Makes the links named in the Predecessors column, between pasted rows.
/// 5. Returns everything as one [`Command::Batch`].
///
/// # How the outline levels are fitted
/// The first pasted row gets `level` if it is given (cut to what the row
/// above allows), else the level of the row it is pasted above (so
/// it becomes that row's sibling), or, at the end, the level of the last
/// row. The other rows keep their depth *relative to the first pasted row*,
/// but never go above it and never more than one level below the row
/// before them. Example: rows at levels 2, 3, 3 pasted above a level-1 row
/// become 1, 2, 2. This keeps the outline valid, so the paste is not
/// refused.
///
/// # Parameters
/// - `project`: the open project (read only).
/// - `text`: the clipboard text.
/// - `at_index`: the row the first pasted task goes to (0 = top; the number
///   of tasks = at the end).
/// - `level`: the outline level of the first pasted row, or `None` to take
///   it from the place of the paste.
/// - `labels`: the column headings of the UI language, if any.
/// - `new_id`: called once per new task (in row order), then once per new
///   link, to get a fresh UUID. The shell passes `Uuid::new_v4`; tests pass
///   a counter. The document never makes random IDs itself (determinism).
///
/// # Returns
/// A [`PastePlan`]. Text without data rows gives an empty batch (applying
/// it changes nothing).
///
/// # Errors
/// None here. Applying the plan can still fail like any command, e.g.
/// `IndexOutOfRange` when `at_index` is past the end.
///
/// # Example
/// ```
/// use chrono::{NaiveDate, Utc};
/// use document::clipboard::paste_tasks;
/// use document::{BatchId, ChangeContext, Document};
/// use model::{Duration, Project};
/// let project = Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let mut doc = Document::new(project);
///
/// let text = "Name\tDuration\tPredecessors\nDesign\t2d\t\nPermit\t1w\t1\n";
/// let plan = paste_tasks(doc.project(), text, 0, None, None, uuid::Uuid::new_v4);
/// assert!(plan.warnings.is_empty());
///
/// let context = ChangeContext {
///     author: "Sam Lee".into(),
///     timestamp: chrono::DateTime::<Utc>::UNIX_EPOCH,
///     batch_id: BatchId::new(),
/// };
/// doc.apply(plan.command, &context).unwrap();
/// assert_eq!(doc.project().tasks[1].duration, Duration::from_days(5)); // 1w = 5 days
/// assert_eq!(doc.project().dependencies.len(), 1);
/// doc.undo(&context).unwrap(); // one Undo removes the whole paste
/// assert!(doc.project().tasks.is_empty());
/// ```
pub fn paste_tasks(
    project: &Project,
    text: &str,
    at_index: usize,
    level: Option<u32>,
    labels: Option<&ColumnLabels>,
    mut new_id: impl FnMut() -> Uuid,
) -> PastePlan {
    let mut warnings = Vec::new();

    // Step 1 and 2: lines, header, columns.
    let lines: Vec<Vec<String>> = split_tsv(text)
        .into_iter()
        .filter(|cells| cells.iter().any(|c| !c.trim().is_empty()))
        .collect();
    let (columns, data) = match lines.split_first() {
        Some((first, rest)) => match header_columns(first, labels) {
            Some(columns) => (columns, rest),
            None => (COLUMN_ORDER.iter().copied().map(Some).collect(), &lines[..]),
        },
        None => (Vec::new(), &lines[..]),
    };
    // Which cell index holds each known column (the first one wins if a
    // heading appears twice).
    let index_of = |wanted: ClipboardColumn| columns.iter().position(|&c| c == Some(wanted));
    let name_at = index_of(ClipboardColumn::Name);
    let duration_at = index_of(ClipboardColumn::Duration);
    let predecessors_at = index_of(ClipboardColumn::Predecessors);
    let level_at = index_of(ClipboardColumn::OutlineLevel);
    // `cell(row, at)` = the trimmed cell, or "" when the column is missing.
    let cell = |row: &[String], at: Option<usize>| -> String {
        at.and_then(|i| row.get(i))
            .map(|c| c.trim().to_string())
            .unwrap_or_default()
    };

    // Step 3: the tasks.
    let one_day = Duration::from_minutes(project.minutes_per_day);
    let raw_levels = read_levels(data, |row| cell(row, level_at), &mut warnings);
    let levels = fit_levels(project, at_index, level, &raw_levels);
    let mut tasks = Vec::with_capacity(data.len());
    for (i, row) in data.iter().enumerate() {
        let duration_text = cell(row, duration_at);
        let duration = if duration_text.is_empty() {
            one_day
        } else {
            match parse_duration(&duration_text, project.minutes_per_day) {
                Some(d) if d.minutes() >= 0 => d,
                _ => {
                    warnings.push(PasteWarning::UnreadableDuration {
                        row: i + 1,
                        text: duration_text,
                    });
                    one_day
                }
            }
        };
        let id = TaskId::from_uuid(new_id());
        tasks.push(Task::with_id(id, cell(row, name_at), duration).at_level(levels[i]));
    }

    // Step 4: the links.
    let mut links = Vec::new();
    for (i, row) in data.iter().enumerate() {
        let successor_row = i + 1;
        let mut seen = HashSet::new();
        for entry in split_predecessors(&cell(row, predecessors_at)) {
            let Some(parsed) = parse_predecessor(entry, project.minutes_per_day) else {
                warnings.push(PasteWarning::UnreadablePredecessor {
                    row: successor_row,
                    text: entry.to_string(),
                });
                continue;
            };
            let p = parsed.row;
            if p == 0 || p > tasks.len() {
                warnings.push(PasteWarning::PredecessorNotPasted {
                    row: successor_row,
                    predecessor_row: p,
                });
            } else if p == successor_row {
                warnings.push(PasteWarning::SelfLink { row: successor_row });
            } else if !seen.insert(p) {
                // `insert` returns false when `p` was already in the set.
                warnings.push(PasteWarning::DuplicateLink {
                    row: successor_row,
                    predecessor_row: p,
                });
            } else if is_inside(&levels, p - 1, i) || is_inside(&levels, i, p - 1) {
                warnings.push(PasteWarning::LinkToOwnSummary {
                    row: successor_row,
                    predecessor_row: p,
                });
            } else {
                let mut link = Dependency::new(tasks[p - 1].id, tasks[i].id, parsed.kind);
                link.id = DependencyId::from_uuid(new_id());
                link = match parsed.lag {
                    Lag::Time(lag) => link.with_lag(lag),
                    Lag::Percent(percent) => link.with_lag_percent(percent),
                };
                links.push(link);
            }
        }
    }

    // Step 5: one batch.
    let task_ids = tasks.iter().map(|t| t.id).collect();
    let mut commands: Vec<Command> = tasks
        .into_iter()
        .enumerate()
        .map(|(offset, task)| Command::AddTask {
            index: at_index + offset,
            task,
        })
        .collect();
    commands.extend(
        links
            .into_iter()
            .map(|dependency| Command::AddDependency { dependency }),
    );
    PastePlan {
        command: Command::Batch { commands },
        task_ids,
        warnings,
    }
}

/// Reads a duration typed the MS Project way.
///
/// # What it accepts
/// A number (decimals with `.` or `,`) followed by an optional unit; spaces
/// and a trailing `?` (MS Project's "estimated" mark) are allowed:
///
/// | Unit | Meaning | Example (8-hour day) |
/// |---|---|---|
/// | `m`, `min`, `minutes` | minutes | `30m` = 30 |
/// | `h`, `hr`, `hours` | hours | `4h` = 240 |
/// | `d`, `day`, `days`, or none | days of `minutes_per_day` | `3d` = 1440 |
/// | `w`, `wk`, `weeks` | weeks of 5 days | `2w` = 4800 |
/// | `mo`, `mon`, `months` | months of 20 days | `1mo` = 9600 |
///
/// (5 days a week and 20 days a month are MS Project's defaults, "hours
/// per week 40" and "days per month 20".)
///
/// # Parameters
/// - `text`: the text, e.g. `"3d"`, `"1,5 days"`, `"-1d"` (leads in lags).
/// - `minutes_per_day`: minutes in one day (480 by default).
///
/// # Returns
/// The duration, rounded to whole minutes, or `None` if the text cannot be
/// read (an unknown unit, an elapsed unit such as `3ed`, no number…).
///
/// # Example
/// ```
/// use document::clipboard::parse_duration;
/// assert_eq!(parse_duration("3d", 480).unwrap().minutes(), 1440);
/// assert_eq!(parse_duration("2w", 480).unwrap().minutes(), 4800);
/// assert_eq!(parse_duration("4h", 480).unwrap().minutes(), 240);
/// assert_eq!(parse_duration("1,5 days?", 480).unwrap().minutes(), 720);
/// assert_eq!(parse_duration("5", 480).unwrap().minutes(), 2400); // no unit = days
/// assert!(parse_duration("3x", 480).is_none());
/// ```
pub fn parse_duration(text: &str, minutes_per_day: i64) -> Option<Duration> {
    let text = text.trim().trim_end_matches('?').trim().to_lowercase();
    // Split "1,5 days" into the number "1,5" and the unit "days": the number
    // is everything up to the first character that cannot be part of it.
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+')))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    // `parse::<f64>` reads a decimal number; it wants "1.5", not "1,5".
    let value: f64 = number.replace(',', ".").parse().ok()?;
    let day = minutes_per_day as f64;
    let unit_minutes = match unit.trim() {
        "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "" | "d" | "dy" | "dys" | "day" | "days" => day,
        "w" | "wk" | "wks" | "week" | "weeks" => 5.0 * day,
        "mo" | "mon" | "mons" | "month" | "months" => 20.0 * day,
        _ => return None,
    };
    let minutes = (value * unit_minutes).round();
    // `is_finite` filters out "infinity" and "not a number", which a float
    // can hold but a whole number of minutes cannot.
    minutes
        .is_finite()
        .then(|| Duration::from_minutes(minutes as i64))
}

// -- paste helpers -----------------------------------------------------------

/// A lag read from a Predecessors entry.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lag {
    /// Working time, e.g. `+2d`.
    Time(Duration),
    /// Percent of the predecessor's duration, e.g. `+50%`.
    Percent(i32),
}

/// One entry of a Predecessors cell, e.g. `3SS+1d`, read into its parts.
#[derive(Clone, Copy, PartialEq, Debug)]
struct ParsedPredecessor {
    /// The row number it names (1 = first data row).
    row: usize,
    /// The link type (FS when none is written).
    kind: DependencyType,
    /// The lag (zero when none is written).
    lag: Lag,
}

/// Splits a Predecessors cell into its entries. Entries are separated by
/// `;` if the cell contains one (some languages use `;` as the list
/// separator, because `,` is their decimal comma), otherwise by `,`.
///
/// Example: `"1,3SS+1d"` → `["1", "3SS+1d"]`; `"1;3FS+1,5d"` → `["1", "3FS+1,5d"]`.
fn split_predecessors(cell: &str) -> Vec<&str> {
    let separator = if cell.contains(';') { ';' } else { ',' };
    cell.split(separator)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Reads one Predecessors entry: a row number, an optional type (`FS`,
/// `SS`, `FF`, `SF`) and an optional lag starting with `+` or `-` (a
/// duration, or a whole percentage ending in `%`).
///
/// Worked examples: `3` → row 3, FS, no lag · `3SS` → SS ·
/// `3FS+2d` → FS, lag 960 min · `3FF-50%` → FF, lead of 50 % ·
/// `3+1d` → FS, lag 1 day (the type may be left out).
///
/// Returns `None` if the entry does not follow this pattern.
fn parse_predecessor(entry: &str, minutes_per_day: i64) -> Option<ParsedPredecessor> {
    let digits = entry
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(entry.len());
    let row: usize = entry[..digits].parse().ok()?;
    let rest = entry[digits..].trim();
    // `get(..2)` returns `None` (instead of crashing) when `rest` is shorter
    // than two bytes or the cut would fall inside a character.
    let (kind, rest) = match rest.get(..2).map(str::to_ascii_uppercase).as_deref() {
        Some("FS") => (DependencyType::FinishToStart, &rest[2..]),
        Some("SS") => (DependencyType::StartToStart, &rest[2..]),
        Some("FF") => (DependencyType::FinishToFinish, &rest[2..]),
        Some("SF") => (DependencyType::StartToFinish, &rest[2..]),
        _ => (DependencyType::FinishToStart, rest),
    };
    let rest = rest.trim();
    let lag = if rest.is_empty() {
        Lag::Time(Duration::ZERO)
    } else {
        // A lag must start with a sign: "+2d" or "-1d".
        if !rest.starts_with(['+', '-']) {
            return None;
        }
        match rest.strip_suffix('%') {
            Some(percent) => Lag::Percent(percent.trim().parse().ok()?),
            None => Lag::Time(parse_duration(rest, minutes_per_day)?),
        }
    };
    Some(ParsedPredecessor { row, kind, lag })
}

/// Reads the Outline level cell of every data row. A missing or unreadable
/// cell takes the level of the row before it (the first row: 1); an
/// unreadable one also adds a warning.
fn read_levels(
    data: &[Vec<String>],
    level_cell: impl Fn(&[String]) -> String,
    warnings: &mut Vec<PasteWarning>,
) -> Vec<u32> {
    let mut levels = Vec::with_capacity(data.len());
    let mut previous = 1;
    for (i, row) in data.iter().enumerate() {
        let text = level_cell(row);
        let level = if text.is_empty() {
            previous
        } else {
            match text.parse::<u32>() {
                Ok(level) if level >= 1 => level,
                _ => {
                    warnings.push(PasteWarning::UnreadableOutlineLevel { row: i + 1, text });
                    previous
                }
            }
        };
        levels.push(level);
        previous = level;
    }
    levels
}

/// Fits the pasted levels to the place of the paste (see "How the outline
/// levels are fitted" in [`paste_tasks`]).
///
/// Worked example: raw levels 2, 3, 3, 5 pasted above a level-1 row:
/// base = 1; relative to the first row: 0, +1, +1, +3; so 1, 2, 2, then
/// 4 is cut to 3 (at most one level below the row before it).
fn fit_levels(project: &Project, at_index: usize, level: Option<u32>, raw: &[u32]) -> Vec<u32> {
    let tasks = &project.tasks;
    // A wished level, never deeper than one below the row above.
    let above = at_index.checked_sub(1).and_then(|row| tasks.get(row));
    let deepest = above.map_or(1, |t| t.outline_level + 1);
    let base = match (level, tasks.get(at_index)) {
        (Some(level), _) => level.clamp(1, deepest),
        // Pasted above a row: become its sibling.
        (None, Some(below)) => below.outline_level,
        // At the end: become a sibling of the last row (level 1 if empty).
        (None, None) => tasks.last().map_or(1, |t| t.outline_level),
    }
    // In a file with project rows, tasks are never at the top level.
    .max(if tasks.iter().any(|t| t.is_project) {
        2
    } else {
        1
    });
    let Some(&first) = raw.first() else {
        return Vec::new();
    };
    let mut levels = Vec::with_capacity(raw.len());
    let mut previous = base;
    for (i, &level) in raw.iter().enumerate() {
        let fitted = if i == 0 {
            base
        } else {
            // i64 so that "level - first" may be negative for a moment.
            let wanted = i64::from(base) + i64::from(level) - i64::from(first);
            let highest = i64::from(base);
            let deepest = i64::from(previous) + 1;
            // `clamp(a, b)` keeps the value between a and b.
            u32::try_from(wanted.clamp(highest, deepest)).unwrap_or(base)
        };
        levels.push(fitted);
        previous = fitted;
    }
    levels
}

/// `true` if pasted row `inner` lies inside the block (the subtasks) of
/// pasted row `outer`, judging by the fitted levels: `inner` comes after
/// `outer`, and every row from `outer + 1` to `inner` is deeper than
/// `outer`. (Both are 0-based positions in the pasted rows.)
fn is_inside(levels: &[u32], outer: usize, inner: usize) -> bool {
    outer < inner && levels[outer + 1..=inner].iter().all(|&l| l > levels[outer])
}

/// Finds which column each cell of the first line is, if the first line is
/// a header line.
///
/// # Returns
/// `Some(columns)` (one entry per cell, `None` for an unknown heading) when
/// at least one cell is a known heading; `None` when the line is data.
fn header_columns(
    first: &[String],
    labels: Option<&ColumnLabels>,
) -> Option<Vec<Option<ClipboardColumn>>> {
    let columns: Vec<Option<ClipboardColumn>> = first
        .iter()
        .map(|cell| {
            labels
                .and_then(|l| l.find(cell))
                .or_else(|| ClipboardColumn::from_header(cell))
        })
        .collect();
    columns.iter().any(Option::is_some).then_some(columns)
}

/// Lower-case text without spaces, `_` and `-`, for comparing headings:
/// `"Outline level"` and `"outline_level"` both become `"outlinelevel"`.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Splits tab-separated text into lines of cells, following Excel's rules.
///
/// # The rules
/// - Cells are separated by a tab; lines end with `\n` or `\r\n`.
/// - A cell that starts with `"` is *quoted*: it runs to the next lone `"`
///   and may contain tabs and line breaks; `""` inside it stands for one
///   `"`. Example: `"Paint ""red""` + tab + `2d` → `Paint "red"`, `2d`.
/// - A final line break does not make an extra empty line.
///
/// # Beginner note: a tiny "state machine"
/// The loop reads one character at a time and remembers one fact,
/// `in_quotes`, that changes how the next character is treated. Programs
/// that read text this way are called state machines.
fn split_tsv(text: &str) -> Vec<Vec<String>> {
    let mut lines = Vec::new();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut at_cell_start = true;
    // `peekable` lets us look at the next character without taking it.
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next(); // `""` = one quote character
                    cell.push('"');
                } else {
                    in_quotes = false; // the closing quote
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if at_cell_start => {
                in_quotes = true;
                at_cell_start = false;
            }
            '\t' => {
                cells.push(std::mem::take(&mut cell));
                at_cell_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {} // the \n ends the line
            '\n' | '\r' => {
                cells.push(std::mem::take(&mut cell));
                lines.push(std::mem::take(&mut cells));
                at_cell_start = true;
            }
            _ => {
                cell.push(c);
                at_cell_start = false;
            }
        }
    }
    // The last line, if the text did not end with a line break.
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        lines.push(cells);
    }
    lines
}
