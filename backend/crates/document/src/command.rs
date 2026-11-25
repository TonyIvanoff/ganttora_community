//! # Edit commands: every change to the project is a command
//!
//! ## What this file is for
//! Decision D16 says: **every edit is a command**. The user
//! interface never changes the project directly. It sends a small message,
//! such as "set the duration of task X to 6 days", and the document carries
//! it out. This file defines those messages ([`Command`]) and what each one
//! does to a [`Project`].
//!
//! ## Where it fits
//! ```text
//!   UI ──IPC──► shell ──► Document::apply(command) ──► command.execute(project)
//!                                                      │
//!                                     returns: the inverse command (for undo)
//!                                              + the field changes (for the journal)
//! ```
//! `lib.rs` (the `Document`) decides *when* a command runs; this file decides
//! *what* it does.
//!
//! ## Main ideas
//! - **Validate, then change.** Each command first checks that it makes
//!   sense (the task exists, the duration is not negative, …) and returns a
//!   [`DocumentError`] if not.
//! - **Every command has an inverse.** The inverse is another command that
//!   undoes it. Worked examples with the garden shed:
//!
//!   | Command | Its inverse |
//!   |---|---|
//!   | `SetTaskDuration` task 2 → 6d (was 5d) | `SetTaskDuration` task 2 → 5d |
//!   | `RemoveTask` task 3 (row 3, links 1→3 and 3→5) | `RestoreTask` at row 3, with both links at their old places |
//!   | `AddDependency` 1→3 | `RemoveDependency` of that link |
//!   | `IndentTask` task 2 (level 1 → 2) | `SetOutlineLevels` task 2 → 1 |
//!   | `MoveTask` task 7 from row 6 to row 2 | `MoveTask` task 7 back to row 6 |
//!   | `AddBlocker` "late wood" on task 3 | `RemoveBlocker` of that blocker |
//!   | `RemoveBlocker` (2nd blocker of task 3) | `RestoreBlocker` at position 1 |
//!
//!   Undo simply executes the stored inverse. Redo executes the stored
//!   original command again. (The inverse of an inverse is not always the
//!   same command: undoing an `AddTask` gives a `RemoveTask`, whose inverse
//!   is a `RestoreTask`. That is why redo keeps the original command.)
//! - **Field changes.** Executing a command also lists, field by field, what
//!   changed (see `journal.rs`). Field names are the model's field names in
//!   snake_case (`duration_minutes`, `outline_level`, `lag_minutes`…).
//!
//! ## Documentation

use crate::DocumentError;
use crate::journal::{EntityKind, FieldChange};
use chrono::{NaiveDate, NaiveDateTime};
use engine::ScheduleResult;
use model::{
    Blocker, BlockerId, Calendar, CalendarId, Constraint, Dependency, DependencyId, DependencyType,
    Duration, Outline, Project, Risk, RiskId, ScheduleFrom, Task, TaskId, TaskMode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A dependency together with its position in `project.dependencies`.
///
/// Used by the "restore" commands, so that undoing a delete puts each link
/// back at exactly the place it came from. That keeps the project
/// *identical* after undo, not just similar.
///
/// # Fields
/// - `index`: the position in the dependency list (0 = first).
/// - `dependency`: the link itself, with its original UUID.
///
/// # Example
/// ```
/// use document::PlacedDependency;
/// use model::{Dependency, TaskId};
/// let link = Dependency::finish_to_start(TaskId::new(), TaskId::new());
/// let placed = PlacedDependency { index: 0, dependency: link };
/// assert_eq!(placed.index, 0);
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedDependency {
    /// Position in `project.dependencies` (0-based).
    pub index: usize,
    /// The link.
    pub dependency: Dependency,
}

/// One task's new outline level, used by [`Command::SetOutlineLevels`].
///
/// # Fields
/// - `id`: the task.
/// - `level`: its new outline level (1 = top level).
///
/// # Example
/// ```
/// use document::OutlineLevel;
/// use model::TaskId;
/// let change = OutlineLevel { id: TaskId::new(), level: 2 };
/// assert_eq!(change.level, 2);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineLevel {
    /// The task.
    pub id: TaskId,
    /// Its new outline level.
    pub level: u32,
}

/// One edit to the project.
///
/// # Variants
/// **Tasks**
/// | Variant | What it does |
/// |---|---|
/// | `AddTask` | Inserts a new task at row `index` (0 = first row) |
/// | `RemoveTask` | Deletes a task, **its subtasks** (if it is a summary task) **and every link to or from them** |
/// | `RestoreTask` | Puts back one deleted task with its links (the inverse of `RemoveTask` for a task without subtasks) |
/// | `RestoreTasks` | Puts back a deleted summary task with its subtasks and links |
/// | `RenameTask` | Changes a task's name |
/// | `SetTaskDuration` | Changes a task's duration |
/// | `SetTaskNotes` / `SetTaskHyperlink` | Changes its notes / hyperlink |
/// | `SetTaskMode` | Switches between automatic and manual scheduling |
/// | `SetTaskManualStart` | Moves a manually scheduled task |
/// | `SetTaskMilestone` | Marks or unmarks a task as a milestone |
/// | `SetTaskConstraint` | Sets the constraint (ASAP, SNET, MSO…) |
/// | `SetTaskDeadline` | Sets or clears the deadline |
/// | `SetTaskCalendar` | Sets or clears the task calendar |
/// | `SetTaskPercentComplete` | Sets how much of a task is done (0–100 %); below 100 % it also clears the actual finish |
/// | `SetTaskPlannedFinish` | Sets or clears the planned finish |
/// | `SetTaskActualFinish` | Sets or clears when the work really finished |
///
/// **Outline**
/// | Variant | What it does |
/// |---|---|
/// | `IndentTask` | Indents a task and its subtasks one level (it becomes a subtask of the row above) |
/// | `OutdentTask` | Outdents a task and its subtasks one level |
/// | `SetOutlineLevels` | Sets the levels of several tasks at once (the inverse of indent/outdent) |
/// | `MoveTask` | Moves a task with its subtasks to another row |
///
/// **Links**
/// | Variant | What it does |
/// |---|---|
/// | `AddDependency` | Adds a link at the end of the link list |
/// | `RemoveDependency` | Deletes one link |
/// | `RestoreDependency` | Puts back a deleted link at its old position |
/// | `EditDependency` | Changes a link's type and lag |
///
/// **Blockers** (unplanned delays on a task)
/// | Variant | What it does |
/// |---|---|
/// | `AddBlocker` | Adds a blocker (with its `after_work` position) at the end of the task's list |
/// | `AddBlockerAt` | Adds a blocker that starts at a **date**; the document turns the date into `after_work` from the current schedule and records a plain `AddBlocker` |
/// | `EditBlocker` | Replaces a blocker (found by its ID) with a new version |
/// | `RemoveBlocker` | Deletes one blocker |
/// | `RestoreBlocker` | Puts back a deleted blocker at its old position (the inverse of `RemoveBlocker`) |
///
/// Blockers are refused on summary tasks ([`DocumentError::BlockerOnSummary`]).
///
/// **Risks** (what could go wrong; also allowed on projects and groups)
/// | Variant | What it does |
/// |---|---|
/// | `AddRisk` | Adds a risk at the end of the task's list |
/// | `EditRisk` | Replaces a risk (found by its ID) with a new version |
/// | `RemoveRisk` | Deletes one risk |
/// | `RestoreRisk` | Puts back a deleted risk at its old position |
///
/// Incomplete risks are refused ([`DocumentError::RiskIncomplete`]).
///
/// **Several edits as one step**
/// | Variant | What it does |
/// |---|---|
/// | `Batch` | Runs a list of commands one after the other, as **one** undo step |
///
/// **Project and calendars**
/// | Variant | What it does |
/// |---|---|
/// | `SetProjectStart` | Moves the project start |
/// | `SetScheduleFrom` | Schedules from the project start or from a fixed finish |
/// | `RenameProject` | Changes the project name |
/// | `SetProjectCalendar` | Chooses the project calendar |
/// | `AddCalendar` / `RemoveCalendar` / `RestoreCalendar` | Adds, deletes, puts back a calendar |
/// | `UpdateCalendar` | Replaces a calendar's working times, holidays and exceptions |
///
/// # One kind of lag at a time
/// A link has either a time lag (`lag`) or a percentage lag
/// (`lag_percent`), never both: the `.gtra` file stores only one. When a
/// link arrives with `lag_percent` set (in `AddDependency`,
/// `EditDependency`, or a restore command), its `lag` is set to zero before
/// it is stored. Example: "FS, lag 2d, lag_percent 50%" is stored as
/// "FS +50%" (lag 0).
///
/// The "restore" variants exist mainly for undo, but they are ordinary
/// commands: they are validated like any other.
///
/// # IDs come from the caller
/// A new task, link or calendar arrives **with its UUID already set**. The
/// document never invents IDs, because random numbers would make it
/// non-deterministic.
///
/// # JSON shape (what the UI sends)
/// The variant name goes into a `"type"` field, and all names are camelCase:
/// ```text
/// { "type": "setTaskDuration", "id": "0b7d…", "duration": { "minutes": 2880 } }
/// { "type": "setTaskConstraint", "id": "0b7d…",
///   "constraint": { "type": "startNoEarlierThan", "date": "2027-03-15T08:00:00" } }
/// { "type": "indentTask", "id": "a91e…" }
/// { "type": "moveTask", "id": "a91e…", "toIndex": 2, "level": null }
/// { "type": "batch", "commands": [ { "type": "renameTask", … }, { "type": "setTaskDuration", … } ] }
/// { "type": "addBlockerAt", "task": "0b7d…", "id": "5e2a…", "at": "2027-03-05T08:00:00",
///   "duration": { "minutes": 2400 }, "reason": "Wood delivery delayed", "reportedOn": "2027-03-04" }
/// { "type": "removeBlocker", "task": "0b7d…", "id": "5e2a…" }
/// ```
///
/// # Example
/// ```
/// use document::Command;
/// use model::{Duration, TaskId};
/// let id = TaskId::new();
/// let command = Command::SetTaskDuration { id, duration: Duration::from_days(6) };
/// let text = serde_json::to_string(&command).unwrap();
/// assert!(text.starts_with(r#"{"type":"setTaskDuration","#));
/// ```
///
/// # Beginner note: an `enum` with data
/// In Rust, each variant of an `enum` can carry its own fields. A `Command`
/// is *one* of these shapes, never two at once, and `match` makes sure the
/// code handles every shape.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// `tag = "type"`: the variant name is stored in a "type" field.
// `rename_all`: variant names in camelCase ("setTaskDuration").
// `rename_all_fields`: field names inside variants in camelCase too.
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    /// Insert `task` at row `index` (0 = before the first task;
    /// `index == number of tasks` = at the end). Its `outline_level` must
    /// fit the rows around it.
    AddTask {
        /// Where to insert (0-based row).
        index: usize,
        /// The new task, with its UUID already set by the caller.
        task: Task,
    },
    /// Delete the task with this ID, its subtasks, and every link that
    /// touches any of them.
    RemoveTask {
        /// The task to delete.
        id: TaskId,
    },
    /// Put one deleted task back at `index`, together with its links.
    RestoreTask {
        /// Where to insert the task (0-based row).
        index: usize,
        /// The task, with its original UUID.
        task: Task,
        /// Its links, each with its old position, sorted by position.
        dependencies: Vec<PlacedDependency>,
    },
    /// Put a deleted block of tasks (a summary task and its subtasks) back
    /// at `index`, together with their links.
    RestoreTasks {
        /// Where to insert the first task (0-based row).
        index: usize,
        /// The tasks in row order, with their original UUIDs and levels.
        tasks: Vec<Task>,
        /// Their links, each with its old position, sorted by position.
        dependencies: Vec<PlacedDependency>,
    },
    /// Give a task a new name.
    RenameTask {
        /// The task to rename.
        id: TaskId,
        /// The new name.
        name: String,
    },
    /// Give a task a new duration.
    SetTaskDuration {
        /// The task to change.
        id: TaskId,
        /// The new duration (0 = milestone; negative is refused).
        duration: Duration,
    },
    /// Replace a task's notes.
    SetTaskNotes {
        /// The task.
        id: TaskId,
        /// The new notes (may be empty).
        notes: String,
    },
    /// Replace a task's hyperlink.
    SetTaskHyperlink {
        /// The task.
        id: TaskId,
        /// The new address (empty = none).
        hyperlink: String,
    },
    /// Switch a task between automatic and manual scheduling.
    ///
    /// When switching to manual without a `manual_start`, the document fills
    /// in the task's current start, so the task stays where it is (as in MS
    /// Project).
    SetTaskMode {
        /// The task.
        id: TaskId,
        /// The new mode.
        mode: TaskMode,
        /// The manual start to store (`None` = keep the current start when
        /// switching to manual; clears it when switching to automatic).
        manual_start: Option<NaiveDateTime>,
    },
    /// Move a manually scheduled task (or clear its start with `None`).
    SetTaskManualStart {
        /// The task.
        id: TaskId,
        /// The new start.
        start: Option<NaiveDateTime>,
    },
    /// Mark or unmark a task as a milestone.
    SetTaskMilestone {
        /// The task.
        id: TaskId,
        /// `true` = show as a milestone.
        milestone: bool,
    },
    /// Set a task's constraint.
    SetTaskConstraint {
        /// The task.
        id: TaskId,
        /// The new constraint (with its date, if it has one).
        constraint: Constraint,
    },
    /// Set or clear a task's deadline.
    SetTaskDeadline {
        /// The task.
        id: TaskId,
        /// The new deadline (`None` = no deadline).
        deadline: Option<NaiveDateTime>,
    },
    /// Set or clear a task's calendar.
    SetTaskCalendar {
        /// The task.
        id: TaskId,
        /// The calendar (`None` = use the project calendar).
        calendar_id: Option<CalendarId>,
    },
    /// Set how much of a task is done. Refused on summary tasks and project
    /// rows (theirs is calculated) and above 100. Below 100 the actual finish
    /// is cleared too: work that is not done has not finished.
    SetTaskPercentComplete {
        /// The task.
        id: TaskId,
        /// Percent done, 0 to 100.
        percent: u8,
    },
    /// Set or clear a task's planned finish ("Set plan" sends one per task).
    SetTaskPlannedFinish {
        /// The task.
        id: TaskId,
        /// The planned finish (`None` = no plan).
        planned_finish: Option<NaiveDateTime>,
    },
    /// Set or clear when a task's work really finished.
    SetTaskActualFinish {
        /// The task.
        id: TaskId,
        /// The actual finish (`None` = not known or not finished).
        actual_finish: Option<NaiveDateTime>,
    },
    /// Indent a task (and its subtasks) by one level.
    IndentTask {
        /// The task.
        id: TaskId,
    },
    /// Outdent a task (and its subtasks) by one level.
    OutdentTask {
        /// The task.
        id: TaskId,
    },
    /// Set the outline levels of several tasks at once.
    SetOutlineLevels {
        /// The new levels.
        levels: Vec<OutlineLevel>,
    },
    /// Move a task, together with its subtasks, so that it ends up at row
    /// `to_index`.
    MoveTask {
        /// The task to move.
        id: TaskId,
        /// Its row after the move (counted in the list *without* the moved
        /// block, so 0 = first row, and the largest value = last row).
        to_index: usize,
        /// Its outline level after the move (its subtasks keep their depth
        /// below it). `None` = the level of the row it is dropped above
        /// (or, at the end, its old level if that fits).
        level: Option<u32>,
    },
    /// Add a link at the end of the link list.
    AddDependency {
        /// The new link, with its UUID already set by the caller.
        dependency: Dependency,
    },
    /// Delete one link.
    RemoveDependency {
        /// The link to delete.
        id: DependencyId,
    },
    /// Put a deleted link back at its old position.
    RestoreDependency {
        /// The link and where it goes.
        placed: PlacedDependency,
    },
    /// Change a link's type and lag (its two tasks stay the same).
    EditDependency {
        /// The link.
        id: DependencyId,
        /// The new type.
        kind: DependencyType,
        /// The new time lag (used when `lag_percent` is `None`).
        lag: Duration,
        /// The new percentage lag, or `None` for a time lag.
        lag_percent: Option<i32>,
    },
    /// Move the project start.
    SetProjectStart {
        /// The new start moment. The engine moves it forward to the next
        /// working time, e.g. Mon 1 Mar 2027 00:00 → 08:00.
        start: NaiveDateTime,
    },
    /// Choose whether the project is scheduled from its start or from a
    /// fixed finish.
    SetScheduleFrom {
        /// Start or finish.
        schedule_from: ScheduleFrom,
        /// The fixed finish (needed when scheduling from the finish).
        finish: Option<NaiveDateTime>,
    },
    /// Rename the project.
    RenameProject {
        /// The new name.
        name: String,
    },
    /// Choose the project calendar.
    SetProjectCalendar {
        /// One of the project's calendars.
        calendar_id: CalendarId,
    },
    /// Add a calendar at the end of the calendar list.
    AddCalendar {
        /// The new calendar, with its UUID set by the caller.
        calendar: Calendar,
    },
    /// Delete a calendar that no task and not the project uses.
    RemoveCalendar {
        /// The calendar.
        id: CalendarId,
    },
    /// Put a deleted calendar back at its old position.
    RestoreCalendar {
        /// Position in the calendar list.
        index: usize,
        /// The calendar.
        calendar: Calendar,
    },
    /// Replace a calendar (found by its ID) with a new version: name, work
    /// week, holidays and exceptions.
    UpdateCalendar {
        /// The new version; its `id` says which calendar it replaces.
        calendar: Calendar,
    },
    /// Record a risk at the end of the task's list. Refused when its ID is
    /// already used (`RiskIdExists`) or it is incomplete (`RiskIncomplete`).
    AddRisk {
        /// The task at risk.
        task: TaskId,
        /// The risk, with its UUID already set by the caller.
        risk: Risk,
    },
    /// Replace a risk with a new version (found by `risk.id`).
    EditRisk {
        /// The task the risk belongs to.
        task: TaskId,
        /// The new version of the risk.
        risk: Risk,
    },
    /// Delete one risk of a task.
    RemoveRisk {
        /// The task the risk belongs to.
        task: TaskId,
        /// The risk to delete.
        id: RiskId,
    },
    /// Put a deleted risk back at its old position (the inverse of
    /// `RemoveRisk`).
    RestoreRisk {
        /// The task the risk belongs to.
        task: TaskId,
        /// Its position in `task.risks` (0-based).
        index: usize,
        /// The risk, with its original UUID.
        risk: Risk,
    },
    /// Record a blocker on a task: an unplanned delay after `after_work`
    /// of the task's work. It is added at the end of the
    /// task's blocker list.
    ///
    /// Refused when the task is a summary task (`BlockerOnSummary`), when
    /// the blocker's ID is already used on any task (`BlockerIdExists`), or
    /// when `after_work` or `duration` is negative (`NegativeDuration`).
    AddBlocker {
        /// The task that is blocked.
        task: TaskId,
        /// The blocker, with its UUID already set by the caller.
        blocker: Blocker,
    },
    /// Record a blocker that starts at a **moment** instead of "after X of
    /// work". This is what the UI sends ("blocked from Fri 5 Mar, for 5
    /// days").
    ///
    /// The document turns `at` into `after_work` using the **current**
    /// schedule: the working minutes of the task's own work scheduled
    /// before `at`, clamped to `0..=duration` (a moment before the task
    /// gives 0, a moment at or after its finish gives the whole duration,
    /// so the blocker extends the end). It then records a plain
    /// [`Command::AddBlocker`], so undo, redo and the journal see the
    /// resolved value, exactly like `SetTaskMode` resolving the manual
    /// start. Without a schedule (it failed), `after_work` is 0.
    ///
    /// Worked example: "Buy materials" (3d) is scheduled Wed 3 Mar 08:00 –
    /// Fri 5 Mar 17:00. `at` = Fri 5 Mar 08:00 → 2 days of work lie before
    /// it → `after_work` = 960 minutes.
    AddBlockerAt {
        /// The task that is blocked.
        task: TaskId,
        /// The new blocker's UUID, set by the caller.
        id: BlockerId,
        /// The moment the blocker starts.
        at: NaiveDateTime,
        /// How long the task is blocked (working time, task calendar).
        duration: Duration,
        /// Why.
        reason: String,
        /// The day the delay was reported, if given.
        reported_on: Option<NaiveDate>,
    },
    /// Replace a blocker with a new version (found by `blocker.id`); it
    /// keeps its place in the task's list.
    EditBlocker {
        /// The task the blocker belongs to.
        task: TaskId,
        /// The new version of the blocker.
        blocker: Blocker,
    },
    /// Delete one blocker of a task.
    RemoveBlocker {
        /// The task the blocker belongs to.
        task: TaskId,
        /// The blocker to delete.
        id: BlockerId,
    },
    /// Put a deleted blocker back at its old position in the task's list
    /// (the inverse of `RemoveBlocker`, so undo gives an identical project).
    RestoreBlocker {
        /// The task the blocker belongs to.
        task: TaskId,
        /// Its position in `task.blockers` (0-based).
        index: usize,
        /// The blocker, with its original UUID.
        blocker: Blocker,
    },
    /// Run several commands one after the other, as **one** undo step.
    ///
    /// Used when one user action makes several edits, e.g. the task
    /// information dialog (name + duration + constraint, confirmed with OK)
    /// or a paste of several rows. Undo reverses the whole batch at once.
    ///
    /// - **All or nothing:** if any command in the list fails, the whole
    ///   batch is refused and the project is unchanged.
    /// - **Checked once, at the end:** the outline rule and the schedule are
    ///   checked after the last command, so a step in the middle may break
    ///   the outline for a moment (e.g. "add a task, then indent it").
    /// - **Order matters:** each command sees the project as the commands
    ///   before it left it.
    /// - An empty list changes nothing (no undo step).
    ///
    /// Worked example: the dialog renames task 2 to "Permit" and sets 6d.
    /// The batch `[RenameTask, SetTaskDuration]` gives two change records
    /// and one undo step; its inverse is
    /// `Batch [SetTaskDuration 5d, RenameTask "Get building permit"]`,
    /// the inverses **in reverse order** (the last change is undone first,
    /// like taking off socks and shoes: shoes first).
    Batch {
        /// The commands, in the order they run.
        commands: Vec<Command>,
    },
}

/// What executing a command produced.
///
/// # Fields
/// - `inverse`: the command that undoes this one.
/// - `changes`: the changed fields, in order, for the journal.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Executed {
    /// The command that undoes the one just executed.
    pub inverse: Command,
    /// What changed, field by field.
    pub changes: Vec<FieldChange>,
}

impl Command {
    /// Fills in values the caller may leave out, using the current schedule.
    ///
    /// # What it does
    /// Two cases today:
    /// - `SetTaskMode` to manual without a `manual_start` gets the task's
    ///   current scheduled start, so the task does not jump (MS Project
    ///   keeps the dates when you switch a task to manual).
    /// - `AddBlockerAt` becomes an `AddBlocker` whose `after_work` is read
    ///   from the current schedule (see [`Command::AddBlockerAt`]).
    ///
    /// The filled-in command is the one stored for redo, so redo gives
    /// exactly the same result.
    ///
    /// # Parameters
    /// - `project`: the current project.
    /// - `schedule`: the current schedule, if it could be calculated.
    ///
    /// Inside a [`Command::Batch`], each command is completed the same way,
    /// using the schedule from *before* the batch (a task added earlier in
    /// the same batch has no schedule yet, so it falls back to the project
    /// start).
    ///
    /// # Returns
    /// The command, completed.
    pub(crate) fn resolve(self, project: &Project, schedule: Option<&ScheduleResult>) -> Command {
        match self {
            Command::Batch { commands } => Command::Batch {
                commands: commands
                    .into_iter()
                    .map(|c| c.resolve(project, schedule))
                    .collect(),
            },
            Command::SetTaskMode {
                id,
                mode: TaskMode::Manual,
                manual_start: None,
            } => {
                let start = schedule
                    .and_then(|s| s.task(id))
                    .map(|t| t.start)
                    .unwrap_or(project.start);
                Command::SetTaskMode {
                    id,
                    mode: TaskMode::Manual,
                    manual_start: Some(start),
                }
            }
            Command::AddBlockerAt {
                task,
                id,
                at,
                duration,
                reason,
                reported_on,
            } => Command::AddBlocker {
                task,
                blocker: Blocker {
                    id,
                    after_work: work_before(project, schedule, task, at),
                    duration,
                    reason,
                    reported_on,
                },
            },
            other => other,
        }
    }

    /// Carries out this command on `project`.
    ///
    /// # What it does
    /// Checks that the command makes sense, changes the project, and
    /// returns the inverse command plus the list of changed fields. It does
    /// **not** reschedule; the `Document` does that afterwards (and also
    /// checks that the outline is still valid).
    ///
    /// # Parameters
    /// - `project`: the project to change. `&mut` means "borrowed with
    ///   permission to change it".
    ///
    /// # Returns
    /// An [`Executed`] with the inverse and the changes. A command that
    /// changes nothing (renaming a task to its current name) returns an
    /// empty change list.
    ///
    /// # Errors
    /// A [`DocumentError`] when the command does not make sense: unknown
    /// task, link or calendar, ID already used, row index too large,
    /// negative duration, a task linked to itself or to its own summary
    /// task, a second link between the same two tasks, an indent or outdent
    /// that is not possible, a calendar still in use…
    ///
    /// **On error the project may be partly changed.** That is why the
    /// `Document` always executes commands on a *copy* of the project and
    /// keeps the copy only when everything succeeded (see `lib.rs`).
    ///
    /// # Example
    /// Internal (`pub(crate)`): used through [`crate::Document::apply`].
    pub(crate) fn execute(&self, project: &mut Project) -> Result<Executed, DocumentError> {
        // `match` picks the branch for this command's variant. Each branch
        // returns a `Result`, which becomes the result of the function.
        match self {
            Command::AddTask { index, task } => add_task(project, *index, task),
            Command::RemoveTask { id } => remove_task(project, *id),
            Command::RestoreTask {
                index,
                task,
                dependencies,
            } => restore_tasks(project, *index, std::slice::from_ref(task), dependencies),
            Command::RestoreTasks {
                index,
                tasks,
                dependencies,
            } => restore_tasks(project, *index, tasks, dependencies),
            Command::RenameTask { id, name } => set_task_field(
                project,
                *id,
                "name",
                name.clone(),
                |t| &mut t.name,
                |id, name| Command::RenameTask { id, name },
            ),
            Command::SetTaskDuration { id, duration } => set_task_duration(project, *id, *duration),
            Command::SetTaskNotes { id, notes } => set_task_field(
                project,
                *id,
                "notes",
                notes.clone(),
                |t| &mut t.notes,
                |id, notes| Command::SetTaskNotes { id, notes },
            ),
            Command::SetTaskHyperlink { id, hyperlink } => set_task_field(
                project,
                *id,
                "hyperlink",
                hyperlink.clone(),
                |t| &mut t.hyperlink,
                |id, hyperlink| Command::SetTaskHyperlink { id, hyperlink },
            ),
            Command::SetTaskMode {
                id,
                mode,
                manual_start,
            } => set_task_mode(project, *id, *mode, *manual_start),
            Command::SetTaskManualStart { id, start } => set_task_field(
                project,
                *id,
                "manual_start",
                *start,
                |t| &mut t.manual_start,
                |id, start| Command::SetTaskManualStart { id, start },
            ),
            Command::SetTaskMilestone { id, milestone } => set_task_field(
                project,
                *id,
                "milestone",
                *milestone,
                |t| &mut t.milestone,
                |id, milestone| Command::SetTaskMilestone { id, milestone },
            ),
            Command::SetTaskConstraint { id, constraint } => set_task_field(
                project,
                *id,
                "constraint",
                *constraint,
                |t| &mut t.constraint,
                |id, constraint| Command::SetTaskConstraint { id, constraint },
            ),
            Command::SetTaskDeadline { id, deadline } => set_task_field(
                project,
                *id,
                "deadline",
                *deadline,
                |t| &mut t.deadline,
                |id, deadline| Command::SetTaskDeadline { id, deadline },
            ),
            Command::SetTaskCalendar { id, calendar_id } => {
                if let Some(calendar) = calendar_id {
                    calendar_index(project, *calendar)?;
                }
                set_task_field(
                    project,
                    *id,
                    "calendar_id",
                    *calendar_id,
                    |t| &mut t.calendar_id,
                    |id, calendar_id| Command::SetTaskCalendar { id, calendar_id },
                )
            }
            Command::SetTaskPercentComplete { id, percent } => {
                let row = task_index(project, *id)?;
                let grouped =
                    project.tasks[row].is_project || Outline::build(&project.tasks).is_summary(row);
                if grouped || *percent > 100 {
                    return Err(DocumentError::InvalidPercent(*id));
                }
                if *percent < 100 && project.tasks[row].actual_finish.is_some() {
                    // Both changes as one step. The second command comes
                    // back here, but then there is nothing left to clear.
                    let clear = Command::SetTaskActualFinish {
                        id: *id,
                        actual_finish: None,
                    };
                    return batch(project, &[clear, self.clone()]);
                }
                set_task_field(
                    project,
                    *id,
                    "percent_complete",
                    *percent,
                    |t| &mut t.percent_complete,
                    |id, percent| Command::SetTaskPercentComplete { id, percent },
                )
            }
            Command::SetTaskPlannedFinish { id, planned_finish } => set_task_field(
                project,
                *id,
                "planned_finish",
                *planned_finish,
                |t| &mut t.planned_finish,
                |id, planned_finish| Command::SetTaskPlannedFinish { id, planned_finish },
            ),
            Command::SetTaskActualFinish { id, actual_finish } => set_task_field(
                project,
                *id,
                "actual_finish",
                *actual_finish,
                |t| &mut t.actual_finish,
                |id, actual_finish| Command::SetTaskActualFinish { id, actual_finish },
            ),
            Command::IndentTask { id } => shift_subtree(project, *id, 1),
            Command::OutdentTask { id } => shift_subtree(project, *id, -1),
            Command::SetOutlineLevels { levels } => set_outline_levels(project, levels),
            Command::MoveTask {
                id,
                to_index,
                level,
            } => move_task(project, *id, *to_index, *level),
            Command::AddDependency { dependency } => {
                // "Add" = "insert at the end of the list".
                let at_end = project.dependencies.len();
                insert_dependency(project, at_end, dependency)
            }
            Command::RemoveDependency { id } => remove_dependency(project, *id),
            Command::RestoreDependency { placed } => {
                insert_dependency(project, placed.index, &placed.dependency)
            }
            Command::EditDependency {
                id,
                kind,
                lag,
                lag_percent,
            } => edit_dependency(project, *id, *kind, *lag, *lag_percent),
            Command::SetProjectStart { start } => set_project_start(project, *start),
            Command::SetScheduleFrom {
                schedule_from,
                finish,
            } => set_schedule_from(project, *schedule_from, *finish),
            Command::RenameProject { name } => rename_project(project, name),
            Command::SetProjectCalendar { calendar_id } => {
                set_project_calendar(project, *calendar_id)
            }
            Command::AddCalendar { calendar } => {
                let at_end = project.calendars.len();
                insert_calendar(project, at_end, calendar)
            }
            Command::RemoveCalendar { id } => remove_calendar(project, *id),
            Command::RestoreCalendar { index, calendar } => {
                insert_calendar(project, *index, calendar)
            }
            Command::UpdateCalendar { calendar } => update_calendar(project, calendar),
            Command::AddRisk { task, risk } => {
                let at_end =
                    task_index(project, *task).map(|row| project.tasks[row].risks.len())?;
                insert_risk(project, *task, at_end, risk)
            }
            Command::EditRisk { task, risk } => edit_risk(project, *task, risk),
            Command::RemoveRisk { task, id } => remove_risk(project, *task, *id),
            Command::RestoreRisk { task, index, risk } => insert_risk(project, *task, *index, risk),
            Command::AddBlocker { task, blocker } => {
                let at_end = task_index(project, *task)
                    .map(|row| project.tasks[row].blockers.len())
                    .unwrap_or(0);
                insert_blocker(project, *task, at_end, blocker)
            }
            // Normally already turned into `AddBlocker` by `resolve`. If it
            // arrives here unresolved, there is no schedule to read the
            // position from, so the blocker starts before any work.
            Command::AddBlockerAt { .. } => self.clone().resolve(project, None).execute(project),
            Command::EditBlocker { task, blocker } => edit_blocker(project, *task, blocker),
            Command::RemoveBlocker { task, id } => remove_blocker(project, *task, *id),
            Command::RestoreBlocker {
                task,
                index,
                blocker,
            } => insert_blocker(project, *task, *index, blocker),
            Command::Batch { commands } => batch(project, commands),
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers: finding things and describing them as field changes.
// ---------------------------------------------------------------------------

/// Finds the row (0-based position) of a task.
///
/// # Errors
/// [`DocumentError::TaskNotFound`] if no task has this ID.
fn task_index(project: &Project, id: TaskId) -> Result<usize, DocumentError> {
    // `position` walks the list and returns `Some(i)` for the first match.
    // `ok_or` turns `None` into our error.
    project
        .tasks
        .iter()
        .position(|t| t.id == id)
        .ok_or(DocumentError::TaskNotFound(id))
}

/// Finds the position of a link in `project.dependencies`.
///
/// # Errors
/// [`DocumentError::DependencyNotFound`] if no link has this ID.
fn dependency_index(project: &Project, id: DependencyId) -> Result<usize, DocumentError> {
    project
        .dependencies
        .iter()
        .position(|d| d.id == id)
        .ok_or(DocumentError::DependencyNotFound(id))
}

/// Finds the position of a calendar in `project.calendars`.
///
/// # Errors
/// [`DocumentError::CalendarNotFound`] if no calendar has this ID.
fn calendar_index(project: &Project, id: CalendarId) -> Result<usize, DocumentError> {
    project
        .calendars
        .iter()
        .position(|c| c.id == id)
        .ok_or(DocumentError::CalendarNotFound(id))
}

/// Turns any value into its JSON form for a change record. Values that
/// cannot be turned into JSON (none of ours) become `null`.
fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The journal fields of a task, as (field name, JSON value) pairs.
///
/// `index` is the task's row (0-based). Durations are in minutes of working
/// time, e.g. 2 days on an 8-hour day = 960.
fn task_fields(task: &Task, index: usize) -> Vec<(&'static str, Value)> {
    let mut fields = vec![
        ("index", json!(index)),
        ("name", json!(task.name)),
        ("duration_minutes", json!(task.duration.minutes())),
        ("outline_level", json!(task.outline_level)),
        ("mode", to_json(&task.mode)),
        ("manual_start", to_json(&task.manual_start)),
        ("milestone", json!(task.milestone)),
        ("constraint", to_json(&task.constraint)),
        ("deadline", to_json(&task.deadline)),
        ("calendar_id", to_json(&task.calendar_id)),
        ("notes", json!(task.notes)),
        ("hyperlink", json!(task.hyperlink)),
    ];
    // Blockers only when there are some, so the records of ordinary tasks
    // stay exactly as they were before blockers existed.
    if !task.blockers.is_empty() {
        let list: Vec<Value> = task.blockers.iter().map(blocker_json).collect();
        fields.push(("blockers", Value::Array(list)));
    }
    // The same for the fields that ordinary new tasks do not use.
    if task.percent_complete != 0 {
        fields.push(("percent_complete", json!(task.percent_complete)));
    }
    if task.planned_finish.is_some() {
        fields.push(("planned_finish", to_json(&task.planned_finish)));
    }
    if task.actual_finish.is_some() {
        fields.push(("actual_finish", to_json(&task.actual_finish)));
    }
    if !task.risks.is_empty() {
        fields.push(("risks", json!(task.risks)));
    }
    if task.is_project {
        fields.push(("is_project", json!(true)));
    }
    fields
}

/// One blocker as a JSON object for the journal, with snake_case keys like
/// every journal field name:
/// ```text
/// { "id": "5e2a…", "after_work_minutes": 960, "duration_minutes": 2400,
///   "reason": "Wood delivery delayed", "reported_on": "2027-03-04" }
/// ```
fn blocker_json(blocker: &Blocker) -> Value {
    json!({
        "id": blocker.id,
        "after_work_minutes": blocker.after_work.minutes(),
        "duration_minutes": blocker.duration.minutes(),
        "reason": blocker.reason,
        "reported_on": blocker.reported_on,
    })
}

/// The journal fields of a link, as (field name, JSON value) pairs.
///
/// `kind` is written as the type's JSON name, e.g. `"finishToStart"`;
/// `lag_minutes` in minutes of working time.
fn dependency_fields(dependency: &Dependency, index: usize) -> Vec<(&'static str, Value)> {
    vec![
        ("index", json!(index)),
        ("predecessor", json!(dependency.predecessor)),
        ("successor", json!(dependency.successor)),
        ("kind", json!(dependency.kind)),
        ("lag_minutes", json!(dependency.lag.minutes())),
        ("lag_percent", json!(dependency.lag_percent)),
    ]
}

/// The journal fields of a calendar, as (field name, JSON value) pairs.
fn calendar_fields(calendar: &Calendar, index: usize) -> Vec<(&'static str, Value)> {
    vec![
        ("index", json!(index)),
        ("name", json!(calendar.name)),
        ("week", to_json(&calendar.week)),
        ("holidays", to_json(&calendar.holidays)),
        ("exceptions", to_json(&calendar.exceptions)),
    ]
}

/// Builds the change records for an item that was **created**: one record
/// per field, old value `null`.
fn created(
    entity: EntityKind,
    id: uuid::Uuid,
    fields: Vec<(&'static str, Value)>,
) -> Vec<FieldChange> {
    fields
        .into_iter()
        .map(|(field, value)| FieldChange {
            entity,
            entity_id: id,
            field: field.to_string(),
            old_value: Value::Null,
            new_value: value,
        })
        .collect()
}

/// Builds the change records for an item that was **deleted**: one record
/// per field, new value `null`.
fn deleted(
    entity: EntityKind,
    id: uuid::Uuid,
    fields: Vec<(&'static str, Value)>,
) -> Vec<FieldChange> {
    fields
        .into_iter()
        .map(|(field, value)| FieldChange {
            entity,
            entity_id: id,
            field: field.to_string(),
            old_value: value,
            new_value: Value::Null,
        })
        .collect()
}

/// Builds the change list for one field going from `old` to `new`, or an
/// empty list when the value did not really change.
fn updated(
    entity: EntityKind,
    id: uuid::Uuid,
    field: &str,
    old: Value,
    new: Value,
) -> Vec<FieldChange> {
    if old == new {
        // Nothing changed, so there is nothing to record.
        return Vec::new();
    }
    vec![FieldChange {
        entity,
        entity_id: id,
        field: field.to_string(),
        old_value: old,
        new_value: new,
    }]
}

/// Checks a block of tasks that is about to be inserted at `index`.
///
/// # Errors
/// `IndexOutOfRange`, `TaskIdExists` (also for two tasks of the block with
/// the same ID) or `NegativeDuration`.
fn check_new_tasks(project: &Project, index: usize, tasks: &[Task]) -> Result<(), DocumentError> {
    let len = project.tasks.len();
    // Inserting at `len` means "at the end", which is allowed; beyond is not.
    if index > len {
        return Err(DocumentError::IndexOutOfRange { index, len });
    }
    for (i, task) in tasks.iter().enumerate() {
        let clash = project.tasks.iter().any(|t| t.id == task.id)
            || tasks[..i].iter().any(|t| t.id == task.id);
        if clash {
            return Err(DocumentError::TaskIdExists(task.id));
        }
        if task.duration.minutes() < 0 {
            return Err(DocumentError::NegativeDuration(task.id));
        }
        if let Some(calendar) = task.calendar_id {
            calendar_index(project, calendar)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tasks.
// ---------------------------------------------------------------------------

/// `AddTask`: inserts `task` at row `index`. Inverse: `RemoveTask`.
fn add_task(project: &mut Project, index: usize, task: &Task) -> Result<Executed, DocumentError> {
    check_new_tasks(project, index, std::slice::from_ref(task))?;
    // `insert` shifts the tasks at `index` and after one row down.
    project.tasks.insert(index, task.clone());
    Ok(Executed {
        inverse: Command::RemoveTask { id: task.id },
        changes: created(
            EntityKind::Task,
            task.id.as_uuid(),
            task_fields(task, index),
        ),
    })
}

/// `RemoveTask`: deletes a task, its subtasks and every link touching them.
/// Inverse: `RestoreTask` (one task) or `RestoreTasks` (a summary task with
/// subtasks), with the tasks and links at their old places.
///
/// Worked example (the shed): removing task 3 "Buy materials" also removes
/// the links 1→3 and 3→5. Task 5 "Build the floor" then depends only on
/// task 4. Undo puts all three items back. Removing a summary task
/// "Preparation" with its two subtasks removes three rows (as in MS
/// Project, where deleting a summary task deletes its subtasks).
fn remove_task(project: &mut Project, id: TaskId) -> Result<Executed, DocumentError> {
    let index = task_index(project, id)?;
    let end = Outline::build(&project.tasks).subtree_end(index);
    let ids: Vec<TaskId> = project.tasks[index..end].iter().map(|t| t.id).collect();

    // Collect the links that touch any removed task, with their positions,
    // in list order (so the smallest position comes first).
    let placed: Vec<PlacedDependency> = project
        .dependencies
        .iter()
        .enumerate()
        .filter(|(_, d)| ids.contains(&d.predecessor) || ids.contains(&d.successor))
        .map(|(i, d)| PlacedDependency {
            index: i,
            dependency: d.clone(),
        })
        .collect();

    // Records: links first (they point at the tasks), then the tasks.
    let mut changes = Vec::new();
    for p in &placed {
        changes.extend(deleted(
            EntityKind::Dependency,
            p.dependency.id.as_uuid(),
            dependency_fields(&p.dependency, p.index),
        ));
    }

    // Remove the links from the back to the front. Why backwards? Removing
    // position 2 would shift position 5 to 4; going from the highest
    // position down, the positions we still have to remove never move.
    for p in placed.iter().rev() {
        project.dependencies.remove(p.index);
    }
    // `drain` removes a range of rows and hands them to us.
    let tasks: Vec<Task> = project.tasks.drain(index..end).collect();
    for (offset, task) in tasks.iter().enumerate() {
        changes.extend(deleted(
            EntityKind::Task,
            task.id.as_uuid(),
            task_fields(task, index + offset),
        ));
    }

    let inverse = if tasks.len() == 1 {
        // `into_iter().next()` takes the only element out of the list.
        let task = tasks
            .into_iter()
            .next()
            .ok_or(DocumentError::TaskNotFound(id))?;
        Command::RestoreTask {
            index,
            task,
            dependencies: placed,
        }
    } else {
        Command::RestoreTasks {
            index,
            tasks,
            dependencies: placed,
        }
    };
    Ok(Executed { inverse, changes })
}

/// `RestoreTask` / `RestoreTasks`: inserts a block of tasks, then their
/// links at their old positions. Inverse: `RemoveTask` of the first task
/// (which removes its subtasks too).
///
/// The links are inserted from the smallest position to the largest, the
/// mirror image of `remove_task`, so each lands exactly where it was.
fn restore_tasks(
    project: &mut Project,
    index: usize,
    tasks: &[Task],
    dependencies: &[PlacedDependency],
) -> Result<Executed, DocumentError> {
    let Some(first) = tasks.first() else {
        return Err(DocumentError::IndexOutOfRange { index, len: 0 });
    };
    check_new_tasks(project, index, tasks)?;
    let mut changes = Vec::new();
    for (offset, task) in tasks.iter().enumerate() {
        project.tasks.insert(index + offset, task.clone());
        changes.extend(created(
            EntityKind::Task,
            task.id.as_uuid(),
            task_fields(task, index + offset),
        ));
    }

    for placed in dependencies {
        // Every restored link must touch a restored task. Otherwise the
        // inverse (`RemoveTask`) would not remove it again, and undo/redo
        // would drift apart.
        let d = &placed.dependency;
        let touches = tasks
            .iter()
            .any(|t| d.predecessor == t.id || d.successor == t.id);
        if !touches {
            return Err(DocumentError::LinkNotOnTask {
                dependency: d.id,
                task: first.id,
            });
        }
        let inserted = insert_dependency(project, placed.index, d)?;
        changes.extend(inserted.changes);
    }

    Ok(Executed {
        inverse: Command::RemoveTask { id: first.id },
        changes,
    })
}

/// Sets one field of a task, records the change and builds the inverse.
/// Used by every simple "set this field" command.
///
/// # Parameters
/// - `project`: the project.
/// - `id`: the task.
/// - `field`: the journal field name, e.g. `"notes"`.
/// - `value`: the new value.
/// - `slot`: a small function that points at the field inside a task.
/// - `inverse`: a small function that builds the undo command from the old
///   value.
///
/// # Beginner note: generics
/// `<T>` makes this one function work for any field type: text, a date, a
/// constraint… The `where` line says what `T` must be able to do: be copied
/// (`Clone`), compared (`PartialEq`) and turned into JSON (`Serialize`).
fn set_task_field<T>(
    project: &mut Project,
    id: TaskId,
    field: &str,
    value: T,
    slot: fn(&mut Task) -> &mut T,
    inverse: fn(TaskId, T) -> Command,
) -> Result<Executed, DocumentError>
where
    T: Clone + PartialEq + Serialize,
{
    let index = task_index(project, id)?;
    // `std::mem::replace` puts the new value in and hands back the old one,
    // in one step.
    let old = std::mem::replace(slot(&mut project.tasks[index]), value.clone());
    Ok(Executed {
        changes: updated(
            EntityKind::Task,
            id.as_uuid(),
            field,
            to_json(&old),
            to_json(&value),
        ),
        inverse: inverse(id, old),
    })
}

/// `SetTaskDuration`: changes the duration. Inverse: the old duration.
///
/// Worked example (Chapter 3): task 2 "Get building permit" goes from 5d
/// (2400 minutes) to 6d (2880 minutes). After rescheduling, the shed
/// finishes on Wed 24 Mar 2027 instead of Tue 23 Mar.
fn set_task_duration(
    project: &mut Project,
    id: TaskId,
    duration: Duration,
) -> Result<Executed, DocumentError> {
    let index = task_index(project, id)?;
    if duration.minutes() < 0 {
        return Err(DocumentError::NegativeDuration(id));
    }
    let old = std::mem::replace(&mut project.tasks[index].duration, duration);
    Ok(Executed {
        changes: updated(
            EntityKind::Task,
            id.as_uuid(),
            "duration_minutes",
            json!(old.minutes()),
            json!(duration.minutes()),
        ),
        inverse: Command::SetTaskDuration { id, duration: old },
    })
}

/// `SetTaskMode`: sets the mode and the manual start together. Inverse:
/// both old values.
fn set_task_mode(
    project: &mut Project,
    id: TaskId,
    mode: TaskMode,
    manual_start: Option<NaiveDateTime>,
) -> Result<Executed, DocumentError> {
    let index = task_index(project, id)?;
    let task = &mut project.tasks[index];
    let old_mode = std::mem::replace(&mut task.mode, mode);
    let old_start = std::mem::replace(&mut task.manual_start, manual_start);
    let mut changes = updated(
        EntityKind::Task,
        id.as_uuid(),
        "mode",
        to_json(&old_mode),
        to_json(&mode),
    );
    changes.extend(updated(
        EntityKind::Task,
        id.as_uuid(),
        "manual_start",
        to_json(&old_start),
        to_json(&manual_start),
    ));
    Ok(Executed {
        changes,
        inverse: Command::SetTaskMode {
            id,
            mode: old_mode,
            manual_start: old_start,
        },
    })
}

// ---------------------------------------------------------------------------
// The outline.
// ---------------------------------------------------------------------------

/// Applies new outline levels to some rows and records the changes.
///
/// # Returns
/// The change records and the inverse (`SetOutlineLevels` with the old
/// levels of the rows that really changed).
fn apply_levels(project: &mut Project, levels: &[(usize, u32)]) -> (Vec<FieldChange>, Command) {
    let mut changes = Vec::new();
    let mut old_levels = Vec::new();
    for &(row, level) in levels {
        let task = &mut project.tasks[row];
        if task.outline_level != level {
            changes.extend(updated(
                EntityKind::Task,
                task.id.as_uuid(),
                "outline_level",
                json!(task.outline_level),
                json!(level),
            ));
            old_levels.push(OutlineLevel {
                id: task.id,
                level: task.outline_level,
            });
            task.outline_level = level;
        }
    }
    (changes, Command::SetOutlineLevels { levels: old_levels })
}

/// `IndentTask` (`delta` = +1) and `OutdentTask` (`delta` = -1): change the
/// level of a task **and its subtasks** by one. Inverse: `SetOutlineLevels`
/// with the old levels.
///
/// # The rules (MS Project behaviour)
/// - **Indent** is possible when the row above is at the same level or
///   deeper: the task becomes a subtask of the nearest task above it at its
///   old level. The first row can never be indented.
/// - **Outdent** is possible when the level is above 1. Tasks below that
///   were siblings of the outdented task now sit one level deeper than it,
///   so they become its subtasks, exactly as in MS Project.
///
/// ```text
/// before            indent "Permit"     outdent "Design" (from the left picture)
/// 1 Preparation     1 Preparation       1 Preparation
/// 2   Design        2   Design          1 Design
/// 2   Permit        3     Permit        2   Permit      <- now a subtask of Design
/// ```
///
/// # Errors
/// `TaskNotFound`, `CannotIndent`, `CannotOutdent`.
fn shift_subtree(project: &mut Project, id: TaskId, delta: i64) -> Result<Executed, DocumentError> {
    let row = task_index(project, id)?;
    let outline = Outline::build(&project.tasks);
    let level = project.tasks[row].outline_level;
    if delta > 0 {
        let fits = row > 0 && project.tasks[row - 1].outline_level >= level;
        if !fits {
            return Err(DocumentError::CannotIndent(id));
        }
    } else if level <= 1 {
        return Err(DocumentError::CannotOutdent(id));
    }
    let levels: Vec<(usize, u32)> = (row..outline.subtree_end(row))
        .map(|r| {
            let old = i64::from(project.tasks[r].outline_level);
            // The new level is at least 1; `u32::try_from` cannot fail for
            // such small numbers, `unwrap_or(1)` is only a safety net.
            (r, u32::try_from((old + delta).max(1)).unwrap_or(1))
        })
        .collect();
    let (changes, inverse) = apply_levels(project, &levels);
    Ok(Executed { inverse, changes })
}

/// `SetOutlineLevels`: sets the given levels. Inverse: the old levels.
///
/// # Errors
/// `TaskNotFound`; `InvalidOutline` for a level of 0.
fn set_outline_levels(
    project: &mut Project,
    levels: &[OutlineLevel],
) -> Result<Executed, DocumentError> {
    let mut rows = Vec::with_capacity(levels.len());
    for change in levels {
        let row = task_index(project, change.id)?;
        if change.level < 1 {
            return Err(DocumentError::InvalidOutline { row });
        }
        rows.push((row, change.level));
    }
    let (changes, inverse) = apply_levels(project, &rows);
    Ok(Executed { inverse, changes })
}

/// `MoveTask`: moves a task and its subtasks to row `to_index`, optionally
/// at another outline level. Inverse: `MoveTask` back to the old row and
/// level.
///
/// # Worked example
/// ```text
/// rows before: 0 Design, 1 Permit, 2 Buy, 3 Ground      move "Buy" to row 1
/// 1. take the block out ("Buy" has no subtasks): Design, Permit, Ground
/// 2. insert it at row 1 of that list:            Design, Buy, Permit, Ground
/// inverse: move "Buy" to row 2 -> the original order again
/// ```
///
/// # Errors
/// `TaskNotFound`; `IndexOutOfRange` when `to_index` is past the end of the
/// list without the block; `InvalidOutline` for a level of 0. A level that
/// does not fit the rows around the new place is refused by the document's
/// outline check.
fn move_task(
    project: &mut Project,
    id: TaskId,
    to_index: usize,
    level: Option<u32>,
) -> Result<Executed, DocumentError> {
    let from = task_index(project, id)?;
    let end = Outline::build(&project.tasks).subtree_end(from);
    let mut block: Vec<Task> = project.tasks.drain(from..end).collect();
    let len = project.tasks.len();
    if to_index > len {
        return Err(DocumentError::IndexOutOfRange {
            index: to_index,
            len,
        });
    }

    // The level of the moved task after the move.
    let old_level = block[0].outline_level;
    let new_level = match level {
        Some(0) => return Err(DocumentError::InvalidOutline { row: to_index }),
        Some(l) => l,
        // Dropped above a row: take that row's level (become its sibling).
        None if to_index < len => project.tasks[to_index].outline_level,
        // Dropped at the end: keep the old level if the row above allows it.
        None => {
            let above = if to_index > 0 {
                project.tasks[to_index - 1].outline_level
            } else {
                0
            };
            old_level.min(above + 1)
        }
    };
    // In a file with project rows, a project stays at the top level and
    // every other task stays inside a project.
    let new_level = if block[0].is_project {
        // Between the tasks of another project it would take them over.
        if project.tasks.get(to_index).is_some_and(|t| !t.is_project) {
            return Err(DocumentError::ProjectOutline { row: to_index });
        }
        1
    } else if project.tasks.iter().any(|t| t.is_project) {
        new_level.max(2)
    } else {
        new_level
    };
    let delta = i64::from(new_level) - i64::from(old_level);

    let mut changes = updated(
        EntityKind::Task,
        id.as_uuid(),
        "index",
        json!(from),
        json!(to_index),
    );
    for task in &mut block {
        let new = u32::try_from((i64::from(task.outline_level) + delta).max(1)).unwrap_or(1);
        changes.extend(updated(
            EntityKind::Task,
            task.id.as_uuid(),
            "outline_level",
            json!(task.outline_level),
            json!(new),
        ));
        task.outline_level = new;
    }
    // `splice` inserts the whole block at `to_index` in one step (it would
    // also replace the range `to_index..to_index`, which is empty).
    project.tasks.splice(to_index..to_index, block);

    Ok(Executed {
        changes,
        inverse: Command::MoveTask {
            id,
            to_index: from,
            level: Some(old_level),
        },
    })
}

// ---------------------------------------------------------------------------
// Links.
// ---------------------------------------------------------------------------

/// Inserts a link at position `index` (used by `AddDependency`,
/// `RestoreDependency` and the restore-task commands). Inverse:
/// `RemoveDependency`.
///
/// # Checks
/// - `index` is at most the number of links;
/// - the link's UUID is not used yet;
/// - both tasks exist, and they are two different tasks;
/// - neither task is a summary task of the other (MS Project refuses such
///   links: the summary task would wait for itself);
/// - there is no other link from the same predecessor to the same
///   successor (MS Project allows only one link per pair of tasks).
///
/// A link that would close a circle (A→B when B→A exists) passes these
/// checks; the engine finds the circle when the `Document` reschedules, and
/// the document then refuses the command.
///
/// A link with a percentage lag is stored with `lag` = 0 (see
/// [`one_kind_of_lag`]).
fn insert_dependency(
    project: &mut Project,
    index: usize,
    dependency: &Dependency,
) -> Result<Executed, DocumentError> {
    let mut dependency = dependency.clone();
    dependency.lag = one_kind_of_lag(dependency.lag, dependency.lag_percent);
    // From here on `dependency` is our own (cleaned) copy; `&dependency`
    // borrows it where the code below needs a reference.
    let dependency = &dependency;
    let len = project.dependencies.len();
    if index > len {
        return Err(DocumentError::IndexOutOfRange { index, len });
    }
    if project.dependencies.iter().any(|d| d.id == dependency.id) {
        return Err(DocumentError::DependencyIdExists(dependency.id));
    }
    // `?` returns early with the error if a task is missing.
    let pred = task_index(project, dependency.predecessor)?;
    let succ = task_index(project, dependency.successor)?;
    if dependency.predecessor == dependency.successor {
        return Err(DocumentError::SelfLink(dependency.predecessor));
    }
    let outline = Outline::build(&project.tasks);
    let (high, low) = if pred < succ {
        (pred, succ)
    } else {
        (succ, pred)
    };
    if low < outline.subtree_end(high) {
        // The lower row lies inside the block of the higher one: the higher
        // row is a summary task above the lower one.
        return Err(DocumentError::LinkToOwnSummary {
            summary: project.tasks[high].id,
            subtask: project.tasks[low].id,
        });
    }
    let duplicate = project
        .dependencies
        .iter()
        .any(|d| d.predecessor == dependency.predecessor && d.successor == dependency.successor);
    if duplicate {
        return Err(DocumentError::LinkExists {
            predecessor: dependency.predecessor,
            successor: dependency.successor,
        });
    }

    project.dependencies.insert(index, dependency.clone());
    Ok(Executed {
        inverse: Command::RemoveDependency { id: dependency.id },
        changes: created(
            EntityKind::Dependency,
            dependency.id.as_uuid(),
            dependency_fields(dependency, index),
        ),
    })
}

/// Makes sure a link has only one kind of lag: returns `lag` unchanged for
/// a time lag, and zero when a percentage lag is set.
///
/// # Why
/// The engine uses `lag_percent` and ignores `lag` when both are set, and
/// the `.gtra` file has room for only one of them. Keeping a stale time lag
/// next to a percentage would be invisible to the user but could come back
/// after a save and reopen.
///
/// # Parameters
/// - `lag`: the time lag that was sent.
/// - `lag_percent`: the percentage lag that was sent (`None` = time lag).
///
/// # Returns
/// The time lag to store.
///
/// # Example
/// `one_kind_of_lag(2 days, Some(50))` → 0 (the link is "+50%");
/// `one_kind_of_lag(2 days, None)` → 2 days (960 minutes).
fn one_kind_of_lag(lag: Duration, lag_percent: Option<i32>) -> Duration {
    if lag_percent.is_some() {
        Duration::ZERO
    } else {
        lag
    }
}

/// `RemoveDependency`: deletes one link.
/// Inverse: `RestoreDependency` at the old position.
fn remove_dependency(project: &mut Project, id: DependencyId) -> Result<Executed, DocumentError> {
    let index = dependency_index(project, id)?;
    let dependency = project.dependencies.remove(index);
    Ok(Executed {
        changes: deleted(
            EntityKind::Dependency,
            id.as_uuid(),
            dependency_fields(&dependency, index),
        ),
        inverse: Command::RestoreDependency {
            placed: PlacedDependency { index, dependency },
        },
    })
}

/// `EditDependency`: changes a link's type and lag. Inverse: the old type
/// and lag.
///
/// Worked example: "Paint" → "Move tools in" changes from FS to FS+2d
/// (the paint dries): `kind` stays `finishToStart`, `lag` 0 → 960 minutes.
///
/// With `lag_percent` set, `lag` is stored as 0 (see [`one_kind_of_lag`]).
fn edit_dependency(
    project: &mut Project,
    id: DependencyId,
    kind: DependencyType,
    lag: Duration,
    lag_percent: Option<i32>,
) -> Result<Executed, DocumentError> {
    let lag = one_kind_of_lag(lag, lag_percent);
    let index = dependency_index(project, id)?;
    let link = &mut project.dependencies[index];
    let old_kind = std::mem::replace(&mut link.kind, kind);
    let old_lag = std::mem::replace(&mut link.lag, lag);
    let old_percent = std::mem::replace(&mut link.lag_percent, lag_percent);
    let uuid = id.as_uuid();
    let mut changes = updated(
        EntityKind::Dependency,
        uuid,
        "kind",
        json!(old_kind),
        json!(kind),
    );
    changes.extend(updated(
        EntityKind::Dependency,
        uuid,
        "lag_minutes",
        json!(old_lag.minutes()),
        json!(lag.minutes()),
    ));
    changes.extend(updated(
        EntityKind::Dependency,
        uuid,
        "lag_percent",
        json!(old_percent),
        json!(lag_percent),
    ));
    Ok(Executed {
        changes,
        inverse: Command::EditDependency {
            id,
            kind: old_kind,
            lag: old_lag,
            lag_percent: old_percent,
        },
    })
}

// ---------------------------------------------------------------------------
// The project and its calendars.
// ---------------------------------------------------------------------------

/// `SetProjectStart`: moves the project start. Inverse: the old start.
///
/// The journal value is the date and time as text, e.g.
/// `"2027-03-01T00:00:00"`.
fn set_project_start(
    project: &mut Project,
    start: NaiveDateTime,
) -> Result<Executed, DocumentError> {
    let old = std::mem::replace(&mut project.start, start);
    Ok(Executed {
        changes: updated(
            EntityKind::Project,
            project.id.as_uuid(),
            "start",
            json!(old),
            json!(start),
        ),
        inverse: Command::SetProjectStart { start: old },
    })
}

/// `SetScheduleFrom`: sets the direction and the fixed finish. Inverse: the
/// old values.
fn set_schedule_from(
    project: &mut Project,
    schedule_from: ScheduleFrom,
    finish: Option<NaiveDateTime>,
) -> Result<Executed, DocumentError> {
    let old_from = std::mem::replace(&mut project.schedule_from, schedule_from);
    let old_finish = std::mem::replace(&mut project.finish, finish);
    let uuid = project.id.as_uuid();
    let mut changes = updated(
        EntityKind::Project,
        uuid,
        "schedule_from",
        to_json(&old_from),
        to_json(&schedule_from),
    );
    changes.extend(updated(
        EntityKind::Project,
        uuid,
        "finish",
        to_json(&old_finish),
        to_json(&finish),
    ));
    Ok(Executed {
        changes,
        inverse: Command::SetScheduleFrom {
            schedule_from: old_from,
            finish: old_finish,
        },
    })
}

/// `RenameProject`: changes the project name. Inverse: the old name.
fn rename_project(project: &mut Project, name: &str) -> Result<Executed, DocumentError> {
    let old = std::mem::replace(&mut project.name, name.to_string());
    Ok(Executed {
        changes: updated(
            EntityKind::Project,
            project.id.as_uuid(),
            "name",
            json!(old),
            json!(name),
        ),
        inverse: Command::RenameProject { name: old },
    })
}

/// `SetProjectCalendar`: chooses the project calendar. Inverse: the old one.
///
/// # Errors
/// `CalendarNotFound`.
fn set_project_calendar(project: &mut Project, id: CalendarId) -> Result<Executed, DocumentError> {
    calendar_index(project, id)?;
    let old = std::mem::replace(&mut project.calendar_id, id);
    Ok(Executed {
        changes: updated(
            EntityKind::Project,
            project.id.as_uuid(),
            "calendar_id",
            json!(old),
            json!(id),
        ),
        inverse: Command::SetProjectCalendar { calendar_id: old },
    })
}

/// `AddCalendar` / `RestoreCalendar`: inserts a calendar at `index`.
/// Inverse: `RemoveCalendar`.
///
/// # Errors
/// `IndexOutOfRange`, `CalendarIdExists`.
fn insert_calendar(
    project: &mut Project,
    index: usize,
    calendar: &Calendar,
) -> Result<Executed, DocumentError> {
    let len = project.calendars.len();
    if index > len {
        return Err(DocumentError::IndexOutOfRange { index, len });
    }
    if project.calendars.iter().any(|c| c.id == calendar.id) {
        return Err(DocumentError::CalendarIdExists(calendar.id));
    }
    project.calendars.insert(index, calendar.clone());
    Ok(Executed {
        inverse: Command::RemoveCalendar { id: calendar.id },
        changes: created(
            EntityKind::Calendar,
            calendar.id.as_uuid(),
            calendar_fields(calendar, index),
        ),
    })
}

/// `RemoveCalendar`: deletes a calendar nobody uses. Inverse:
/// `RestoreCalendar` at the old position.
///
/// # Errors
/// `CalendarNotFound`; `CalendarInUse` when it is the project calendar or
/// some task's calendar.
fn remove_calendar(project: &mut Project, id: CalendarId) -> Result<Executed, DocumentError> {
    let index = calendar_index(project, id)?;
    let in_use =
        project.calendar_id == id || project.tasks.iter().any(|t| t.calendar_id == Some(id));
    if in_use {
        return Err(DocumentError::CalendarInUse(id));
    }
    let calendar = project.calendars.remove(index);
    Ok(Executed {
        changes: deleted(
            EntityKind::Calendar,
            id.as_uuid(),
            calendar_fields(&calendar, index),
        ),
        inverse: Command::RestoreCalendar { index, calendar },
    })
}

/// `UpdateCalendar`: replaces a calendar with a new version. Inverse: the
/// old version.
///
/// # Errors
/// `CalendarNotFound`.
fn update_calendar(project: &mut Project, calendar: &Calendar) -> Result<Executed, DocumentError> {
    let index = calendar_index(project, calendar.id)?;
    let old = std::mem::replace(&mut project.calendars[index], calendar.clone());
    let uuid = calendar.id.as_uuid();
    let mut changes = Vec::new();
    // Compare field by field (skipping `index`, which does not change).
    for ((field, before), (_, after)) in calendar_fields(&old, index)
        .into_iter()
        .zip(calendar_fields(calendar, index))
        .skip(1)
    {
        changes.extend(updated(EntityKind::Calendar, uuid, field, before, after));
    }
    Ok(Executed {
        changes,
        inverse: Command::UpdateCalendar { calendar: old },
    })
}

// ---------------------------------------------------------------------------
// Batches.
// ---------------------------------------------------------------------------

/// `Batch`: runs `commands` in order on the same project. Inverse: a
/// `Batch` of their inverses, in **reverse** order.
///
/// # Why reverse order?
/// Each command's inverse was made for the project as it was right after
/// that command. Undo must therefore walk back the way we came, last
/// command first. Example: `[AddTask "Paint", IndentTask "Paint"]` is
/// undone by `[SetOutlineLevels "Paint" → 1, RemoveTask "Paint"]`. The
/// other order would try to outdent a task that was already removed.
///
/// # Errors
/// The first error of any command in the list. The project may then be
/// partly changed, which is fine: the `Document` works on a copy and throws
/// it away (see [`Command::execute`]).
fn batch(project: &mut Project, commands: &[Command]) -> Result<Executed, DocumentError> {
    let mut changes = Vec::new();
    let mut inverses = Vec::with_capacity(commands.len());
    for command in commands {
        let executed = command.execute(project)?;
        // A command that changed nothing needs no inverse: leaving it out
        // keeps the undo step small.
        if !executed.changes.is_empty() {
            changes.extend(executed.changes);
            inverses.push(executed.inverse);
        }
    }
    // `reverse` turns the list around in place: the last command's inverse
    // now comes first, so it is undone first.
    inverses.reverse();
    Ok(Executed {
        inverse: Command::Batch { commands: inverses },
        changes,
    })
}

// ---------------------------------------------------------------------------
// Risks.
// ---------------------------------------------------------------------------

/// The journal record of a risk change: about the task, field `"risk"`, the
/// whole old and new risk as JSON (or `null`).
fn risk_change(task: TaskId, old: Option<&Risk>, new: Option<&Risk>) -> Vec<FieldChange> {
    updated(
        EntityKind::Task,
        task.as_uuid(),
        "risk",
        old.map_or(Value::Null, to_json),
        new.map_or(Value::Null, to_json),
    )
}

/// Finds a risk on a task: the task's row and the risk's position.
///
/// # Errors
/// `TaskNotFound`, `RiskNotFound`.
fn risk_index(
    project: &Project,
    task: TaskId,
    id: RiskId,
) -> Result<(usize, usize), DocumentError> {
    let row = task_index(project, task)?;
    let index = project.tasks[row]
        .risks
        .iter()
        .position(|r| r.id == id)
        .ok_or(DocumentError::RiskNotFound(id))?;
    Ok((row, index))
}

/// `AddRisk` (at the end) and `RestoreRisk` (at `index`). Inverse:
/// `RemoveRisk`.
///
/// # Errors
/// `TaskNotFound`, `IndexOutOfRange`, `RiskIdExists`.
fn insert_risk(
    project: &mut Project,
    task: TaskId,
    index: usize,
    risk: &Risk,
) -> Result<Executed, DocumentError> {
    let row = task_index(project, task)?;
    let len = project.tasks[row].risks.len();
    if index > len {
        return Err(DocumentError::IndexOutOfRange { index, len });
    }
    // Risk IDs are unique in the whole project, like every other ID.
    let used = project
        .tasks
        .iter()
        .flat_map(|t| &t.risks)
        .any(|r| r.id == risk.id);
    if used {
        return Err(DocumentError::RiskIdExists(risk.id));
    }
    project.tasks[row].risks.insert(index, risk.clone());
    Ok(Executed {
        inverse: Command::RemoveRisk { task, id: risk.id },
        changes: risk_change(task, None, Some(risk)),
    })
}

/// `EditRisk`: replaces a risk in place. Inverse: `EditRisk` with the old
/// version.
///
/// # Errors
/// `TaskNotFound`, `RiskNotFound`.
fn edit_risk(project: &mut Project, task: TaskId, risk: &Risk) -> Result<Executed, DocumentError> {
    let (row, index) = risk_index(project, task, risk.id)?;
    let old = std::mem::replace(&mut project.tasks[row].risks[index], risk.clone());
    Ok(Executed {
        changes: risk_change(task, Some(&old), Some(risk)),
        inverse: Command::EditRisk { task, risk: old },
    })
}

/// `RemoveRisk`. Inverse: `RestoreRisk` at its old position.
///
/// # Errors
/// `TaskNotFound`, `RiskNotFound`.
fn remove_risk(project: &mut Project, task: TaskId, id: RiskId) -> Result<Executed, DocumentError> {
    let (row, index) = risk_index(project, task, id)?;
    let old = project.tasks[row].risks.remove(index);
    Ok(Executed {
        changes: risk_change(task, Some(&old), None),
        inverse: Command::RestoreRisk {
            task,
            index,
            risk: old,
        },
    })
}

// ---------------------------------------------------------------------------
// Blockers.
// ---------------------------------------------------------------------------

/// The journal record of a blocker change.
///
/// # How blocker changes are recorded
/// A blocker belongs to a task, so the record is about the **task**
/// (`entity` = task, `entity_id` = the task's UUID) with the field
/// `"blocker"`. The old and new values are the whole blocker as a JSON
/// object ([`blocker_json`]), or `null`:
///
/// | Change | old_value | new_value |
/// |---|---|---|
/// | added | `null` | `{ "id": …, "after_work_minutes": 960, "duration_minutes": 2400, … }` |
/// | edited | the old object | the new object |
/// | removed | the object | `null` |
///
/// (Why not a separate entity kind "blocker"? The `.gtra` journal table
/// knows only project, task, dependency and calendar today; adding a kind
/// needs a change in the file layer first.)
///
/// # Returns
/// One record, or none when `old` and `new` are equal.
fn blocker_change(task: TaskId, old: Option<&Blocker>, new: Option<&Blocker>) -> Vec<FieldChange> {
    updated(
        EntityKind::Task,
        task.as_uuid(),
        "blocker",
        old.map_or(Value::Null, blocker_json),
        new.map_or(Value::Null, blocker_json),
    )
}

/// Checks the values of a blocker that is about to be stored on `task`.
///
/// # Errors
/// - `NegativeDuration(task)` when `after_work` or `duration` is negative.
/// - `BlockerOnSummary(task)` when the task (at row `row`) is a summary
///   task.
fn check_blocker(
    project: &Project,
    row: usize,
    task: TaskId,
    blocker: &Blocker,
) -> Result<(), DocumentError> {
    if blocker.after_work.minutes() < 0 || blocker.duration.minutes() < 0 {
        return Err(DocumentError::NegativeDuration(task));
    }
    if Outline::build(&project.tasks).is_summary(row) {
        return Err(DocumentError::BlockerOnSummary(task));
    }
    Ok(())
}

/// Finds a blocker on a task: the task's row and the blocker's position in
/// its list.
///
/// # Errors
/// `TaskNotFound`, or `BlockerNotFound` when the task has no blocker with
/// this ID.
fn blocker_index(
    project: &Project,
    task: TaskId,
    id: BlockerId,
) -> Result<(usize, usize), DocumentError> {
    let row = task_index(project, task)?;
    let index = project.tasks[row]
        .blockers
        .iter()
        .position(|b| b.id == id)
        .ok_or(DocumentError::BlockerNotFound(id))?;
    Ok((row, index))
}

/// `AddBlocker` (at the end of the list) and `RestoreBlocker` (at `index`):
/// stores a blocker on a task. Inverse: `RemoveBlocker`.
///
/// # Errors
/// `TaskNotFound`, `IndexOutOfRange`, `BlockerIdExists` (the ID is used by
/// a blocker on any task), `NegativeDuration`, `BlockerOnSummary`.
fn insert_blocker(
    project: &mut Project,
    task: TaskId,
    index: usize,
    blocker: &Blocker,
) -> Result<Executed, DocumentError> {
    let row = task_index(project, task)?;
    let len = project.tasks[row].blockers.len();
    if index > len {
        return Err(DocumentError::IndexOutOfRange { index, len });
    }
    // Blocker IDs are unique in the whole project, like every other ID.
    // `flat_map` walks the blockers of every task as one long list.
    if project
        .tasks
        .iter()
        .flat_map(|t| &t.blockers)
        .any(|b| b.id == blocker.id)
    {
        return Err(DocumentError::BlockerIdExists(blocker.id));
    }
    check_blocker(project, row, task, blocker)?;
    project.tasks[row].blockers.insert(index, blocker.clone());
    Ok(Executed {
        inverse: Command::RemoveBlocker {
            task,
            id: blocker.id,
        },
        changes: blocker_change(task, None, Some(blocker)),
    })
}

/// `EditBlocker`: replaces a blocker (same ID) in place. Inverse:
/// `EditBlocker` with the old version.
///
/// # Errors
/// `TaskNotFound`, `BlockerNotFound`, `NegativeDuration`,
/// `BlockerOnSummary`.
fn edit_blocker(
    project: &mut Project,
    task: TaskId,
    blocker: &Blocker,
) -> Result<Executed, DocumentError> {
    let (row, index) = blocker_index(project, task, blocker.id)?;
    check_blocker(project, row, task, blocker)?;
    let old = std::mem::replace(&mut project.tasks[row].blockers[index], blocker.clone());
    Ok(Executed {
        changes: blocker_change(task, Some(&old), Some(blocker)),
        inverse: Command::EditBlocker { task, blocker: old },
    })
}

/// `RemoveBlocker`: deletes one blocker. Inverse: `RestoreBlocker` at its
/// old position.
///
/// # Errors
/// `TaskNotFound`, `BlockerNotFound`.
fn remove_blocker(
    project: &mut Project,
    task: TaskId,
    id: BlockerId,
) -> Result<Executed, DocumentError> {
    let (row, index) = blocker_index(project, task, id)?;
    let old = project.tasks[row].blockers.remove(index);
    Ok(Executed {
        changes: blocker_change(task, Some(&old), None),
        inverse: Command::RestoreBlocker {
            task,
            index,
            blocker: old,
        },
    })
}

/// How much of a task's own work is scheduled before the moment `at`: the
/// `after_work` of a blocker that starts at `at` (used by `AddBlockerAt`).
///
/// # The rule
/// Add up the working minutes (task calendar) of the task's **work** that
/// lie before `at`, then clamp the sum to `0..=duration`. Blocked pieces
/// of existing blockers do not count: they are not work.
///
/// ```text
/// "Buy materials" 3d, scheduled Wed 3 Mar 2027 08:00 - Fri 5 Mar 17:00
///   at = Fri 5 Mar 08:00  -> Wed + Thu = 960 min (2 days)
///   at = Mon 1 Mar 08:00  -> before the task: 0
///   at = Mon 8 Mar 08:00  -> after the finish: 1440 (the whole 3 days)
/// ```
///
/// # Parameters
/// - `project`: the current project.
/// - `schedule`: the current schedule (`None` if it failed).
/// - `task`: the blocked task.
/// - `at`: the moment the blocker starts.
///
/// # Returns
/// The working time before `at`; zero when the task, its schedule or its
/// calendar cannot be found.
fn work_before(
    project: &Project,
    schedule: Option<&ScheduleResult>,
    task: TaskId,
    at: NaiveDateTime,
) -> Duration {
    let Some(model_task) = project.tasks.iter().find(|t| t.id == task) else {
        return Duration::ZERO;
    };
    let Some(dates) = schedule.and_then(|s| s.task(task)) else {
        return Duration::ZERO;
    };
    let calendar_id = model_task.calendar_id.unwrap_or(project.calendar_id);
    let Some(calendar) = project.calendars.iter().find(|c| c.id == calendar_id) else {
        return Duration::ZERO;
    };
    // The work pieces of the bar: the whole bar when there are no blockers
    // yet, otherwise the `Work` segments the engine cut.
    let work_pieces: Vec<(NaiveDateTime, NaiveDateTime)> = if dates.segments.is_empty() {
        vec![(dates.start, dates.finish)]
    } else {
        dates
            .segments
            .iter()
            .filter(|seg| seg.kind == engine::SegmentKind::Work)
            .map(|seg| (seg.start, seg.finish))
            .collect()
    };
    let minutes: i64 = work_pieces
        .iter()
        .filter(|(start, _)| at > *start)
        .map(|(start, finish)| {
            engine::calendar_math::working_minutes_between(calendar, *start, at.min(*finish))
                .minutes()
        })
        .sum();
    Duration::from_minutes(minutes.clamp(0, model_task.duration.minutes().max(0)))
}
