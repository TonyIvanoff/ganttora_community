//! # Crate `document`: the open project session
//!
//! ## What this crate is for
//! While a project is open, the `document` holds it in memory and is the
//! **only** place where it can change. It owns:
//! - the project itself (`model::Project`, what the user typed);
//! - the latest schedule calculated by the engine (or the engine's error);
//! - the undo stack and the redo stack;
//! - the change records waiting to be written to the file's journal.
//!
//! ## Where it fits
//! ```text
//!   UI ──IPC──► shell ──► Document ──► engine::schedule()
//!                            │
//!                            ├─ command.rs    what each edit does (+ its inverse)
//!                            ├─ clipboard.rs  copy/paste rows as tab-separated text
//!                            ├─ undo.rs       the undo and redo stacks
//!                            ├─ journal.rs    change records (who, what, when)
//!                            └─ reschedule.rs rerun the engine; snapshot for the UI
//! ```
//!
//! ## Main ideas
//! - **Every edit is a command** (decision D16). [`Document::apply`] is the
//!   front door for all edits.
//! - **All or nothing.** A command runs on a *copy* of the project. Only if
//!   the command is valid *and* the project can still be scheduled does the
//!   copy replace the original. Otherwise the document is unchanged, as if
//!   the command had never been sent. This is called an **atomic** change.
//! - **Deterministic.** The document reads no clock, makes no random IDs and
//!   does no I/O. Author, time and batch ID come in from the caller in a
//!   [`ChangeContext`]; new tasks and links arrive with their UUIDs set.
//!
//! ## Worked example (book Chapter 3)
//! The garden shed finishes on Tue 23 Mar 2027 (17 working days). Setting
//! task 2 "Get building permit" from 5d to 6d moves the finish to Wed 24 Mar
//! (18 working days). Undo brings back 23 Mar; redo 24 Mar again. Run it:
//! `cargo run -p document --example shed_edit`.
//!
//! - **The outline stays valid.** A command may not leave a row two levels
//!   deeper than the row above it (see `model::outline`); such a command is
//!   refused with [`DocumentError::InvalidOutline`].
//! - **No files.** The document is pure memory. The Tauri shell connects it
//!   to the `.gtra` file layer: it opens a file into a `Project`, and on save
//!   writes the project and [`Document::take_pending_changes`] (the journal).
//!
//! ## What is there and what comes later
//! M0: the first nine commands, unlimited undo/redo, in-memory change
//! records, full reschedule after every command, the UI snapshot.
//! M1: commands for every task field, the outline (indent, outdent, move),
//! link editing, constraints, calendars, schedule-from-finish; error codes
//! for the UI; batches (several commands as one undo step, see
//! [`Document::apply_batch`]); copy and paste of rows as text
//! ([`clipboard`]); blockers, unplanned delays on a task (the project documentation,
//! `AddBlocker`, `AddBlockerAt`, `EditBlocker`, `RemoveBlocker`).
//!
//! ## Documentation

pub mod clipboard;
mod command;
mod journal;
mod reschedule;
mod undo;

// `pub use` makes these types available as `document::Command` and so on,
// so users of the crate do not need to know which file each one lives in.
pub use command::{Command, OutlineLevel, PlacedDependency};
pub use journal::{BatchId, ChangeContext, ChangeRecord, EntityKind, FieldChange};
pub use reschedule::{BlockerRow, CalendarRow, LinkRow, ScheduleSnapshot, TaskRow};

use engine::{ScheduleError, ScheduleResult};
use model::{
    BlockerId, CalendarId, DependencyId, Outline, Project, Risk, RiskId, TaskId, first_invalid_row,
};
use serde::{Deserialize, Serialize};
use undo::{UndoEntry, UndoStack};

/// Everything that can make the document refuse an edit, undo or redo.
///
/// When any of these is returned, **nothing has changed**: the project, the
/// schedule, both stacks and the pending change records are exactly as
/// before.
///
/// # JSON (what the UI receives)
/// A stable error code plus details, never an English sentence; the UI
/// translates the code (the English `Display` text is for logs):
/// ```text
/// { "code": "taskNotFound", "details": "0b7d…" }
/// { "code": "cannotIndent", "details": "0b7d…" }
/// { "code": "rejected", "details": { "code": "cycleDetected", "details": { … } } }
/// ```
///
/// # Example
/// ```
/// use chrono::{NaiveDate, Utc};
/// use document::{BatchId, ChangeContext, Command, Document, DocumentError};
/// use model::{Project, TaskId};
/// let project = Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let mut doc = Document::new(project);
/// let context = ChangeContext {
///     author: "Sam Lee".into(),
///     timestamp: chrono::DateTime::<Utc>::UNIX_EPOCH,
///     batch_id: BatchId::new(),
/// };
/// let missing = TaskId::new();
/// let result = doc.apply(Command::RenameTask { id: missing, name: "X".into() }, &context);
/// assert_eq!(result.unwrap_err(), DocumentError::TaskNotFound(missing));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
// `tag = "code", content = "details"`: the variant name goes into "code",
// its fields into "details".
#[serde(
    tag = "code",
    content = "details",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DocumentError {
    /// No task has this ID.
    #[error("task {0} not found")]
    TaskNotFound(TaskId),
    /// No link has this ID.
    #[error("dependency {0} not found")]
    DependencyNotFound(DependencyId),
    /// A new task uses an ID that another task already has.
    #[error("a task with ID {0} already exists")]
    TaskIdExists(TaskId),
    /// A new link uses an ID that another link already has.
    #[error("a dependency with ID {0} already exists")]
    DependencyIdExists(DependencyId),
    /// A row position is past the end of the list.
    #[error("position {index} is out of range (the list has {len} items)")]
    IndexOutOfRange {
        /// The position asked for.
        index: usize,
        /// How many items the list has.
        len: usize,
    },
    /// A duration below zero was given for this task.
    #[error("task {0} cannot have a negative duration")]
    NegativeDuration(TaskId),
    /// A link from a task to itself.
    #[error("task {0} cannot depend on itself")]
    SelfLink(TaskId),
    /// A second link between the same two tasks, in the same direction.
    #[error("task {successor} already depends on task {predecessor}")]
    LinkExists {
        /// The predecessor of both links.
        predecessor: TaskId,
        /// The successor of both links.
        successor: TaskId,
    },
    /// `RestoreTask` was given a link that does not touch the restored task.
    #[error("dependency {dependency} does not touch task {task}")]
    LinkNotOnTask {
        /// The link.
        dependency: DependencyId,
        /// The task being restored.
        task: TaskId,
    },
    /// A link between a summary task and one of its own subtasks (MS
    /// Project refuses these: the summary task would wait for itself).
    #[error("task {subtask} is a subtask of {summary}; they cannot be linked")]
    LinkToOwnSummary {
        /// The summary task.
        summary: TaskId,
        /// The subtask inside it.
        subtask: TaskId,
    },
    /// The task cannot be indented (it is the first row, or the row above is
    /// less indented).
    #[error("task {0} cannot be indented")]
    CannotIndent(TaskId),
    /// The task cannot be outdented (it is already at the top level).
    #[error("task {0} cannot be outdented")]
    CannotOutdent(TaskId),
    /// The percent complete is above 100, or the task is a summary task or
    /// a project row, whose percent is calculated from its subtasks.
    #[error("the percent complete of task {0} cannot be set like this")]
    InvalidPercent(TaskId),
    /// After the command, a task would be outside every project, or a
    /// project would be inside another row.
    #[error("row {row}: tasks belong inside a project, and projects at the top level")]
    ProjectOutline {
        /// The first row that breaks the rule.
        row: usize,
    },
    /// After the command, this row would be more than one level deeper
    /// than the row above it (or the first row would not be at level 1).
    #[error("the outline would be invalid at row {row}")]
    InvalidOutline {
        /// The first row that breaks the outline rule.
        row: usize,
    },
    /// No calendar has this ID.
    #[error("calendar {0} not found")]
    CalendarNotFound(CalendarId),
    /// A new calendar uses an ID that another calendar already has.
    #[error("a calendar with ID {0} already exists")]
    CalendarIdExists(CalendarId),
    /// The calendar is the project calendar or a task's calendar.
    #[error("calendar {0} is still in use")]
    CalendarInUse(CalendarId),
    /// No blocker with this ID on the given task.
    #[error("blocker {0} not found")]
    BlockerNotFound(BlockerId),
    /// A new blocker uses an ID that another blocker (on any task) already has.
    #[error("a blocker with ID {0} already exists")]
    BlockerIdExists(BlockerId),
    /// The task is (or would become) a summary task and has blockers.
    /// Summary tasks cannot have blockers: their dates are rolled up from
    /// their subtasks, so a blocker on them would be hidden. Returned for a
    /// blocker added to a summary task, and for an indent, move or paste
    /// that would turn a task with blockers into a summary task.
    #[error("task {0} is a summary task and cannot have blockers")]
    BlockerOnSummary(TaskId),
    /// No risk with this ID on the given task.
    #[error("risk {0} not found")]
    RiskNotFound(RiskId),
    /// A new risk uses an ID that another risk (on any task) already has.
    #[error("a risk with ID {0} already exists")]
    RiskIdExists(RiskId),
    /// A risk lacks its description, its mitigation note (unless approved)
    /// or its consequence (unless there is none).
    #[error("risk {0} is incomplete")]
    RiskIncomplete(RiskId),
    /// The command was valid on its own, but the project could not be
    /// scheduled afterwards (most often: a circular dependency).
    #[error("change refused: {0}")]
    Rejected(ScheduleError),
    /// Undo was asked for, but the undo stack is empty.
    #[error("nothing to undo")]
    NothingToUndo,
    /// Redo was asked for, but the redo stack is empty.
    #[error("nothing to redo")]
    NothingToRedo,
}

/// Finds the first incomplete risk that was not exactly so before.
///
/// # Parameters
/// - `before` / `after`: the project before and after a command.
///
/// # Returns
/// The risk's ID, or `None` if all is well.
fn first_new_incomplete_risk(before: &Project, after: &Project) -> Option<RiskId> {
    let mut incomplete = after
        .tasks
        .iter()
        .flat_map(|t| &t.risks)
        .filter(|r| !r.is_complete())
        .peekable();
    // Normally there are none, and the old project need not be searched.
    incomplete.peek()?;
    let old: Vec<&Risk> = before.tasks.iter().flat_map(|t| &t.risks).collect();
    incomplete.find(|r| !old.contains(r)).map(|r| r.id)
}

/// Finds the first row that breaks the project rule: in a file with project
/// rows, the top-level rows are exactly the projects.
///
/// # Returns
/// The row, or `None` if all is well (also for a file without project rows).
fn first_outside_project(project: &Project) -> Option<usize> {
    if !project.tasks.iter().any(|t| t.is_project) {
        return None;
    }
    project
        .tasks
        .iter()
        .position(|t| (t.outline_level == 1) != t.is_project)
}

/// Finds the first summary task (in row order) that has blockers.
///
/// # Returns
/// Its ID, or `None` when no summary task has blockers (the normal case).
///
/// # Example
/// ```text
/// rows: 1 Buy materials (level 1, one blocker)
///       2 Order wood    (level 2)      <- makes row 1 a summary task
/// summary_with_blockers -> Some(<Buy materials>)
/// ```
fn summary_with_blockers(project: &Project) -> Option<TaskId> {
    // Cheap test first: most projects have no blockers at all, and then the
    // outline does not need to be built.
    if project.tasks.iter().all(|t| t.blockers.is_empty()) {
        return None;
    }
    let outline = Outline::build(&project.tasks);
    project
        .tasks
        .iter()
        .enumerate()
        .find(|(row, task)| !task.blockers.is_empty() && outline.is_summary(*row))
        .map(|(_, task)| task.id)
}

/// What an applied command, undo or redo produced. Sent to the UI as JSON.
///
/// # Fields
/// - `changes`: the change records of this step (empty if nothing changed).
/// - `can_undo` / `can_redo`: whether the Undo / Redo buttons should be
///   enabled now.
///
/// The new dates are not in here; ask [`Document::snapshot`] for them.
///
/// # JSON shape
/// ```text
/// { "changes": [ { "timestamp": "…", "author": "Sam Lee", … } ],
///   "canUndo": true, "canRedo": false }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The change records made by this step.
    pub changes: Vec<ChangeRecord>,
    /// `true` if Undo is possible now.
    pub can_undo: bool,
    /// `true` if Redo is possible now.
    pub can_redo: bool,
}

/// One open project: the project, its schedule, undo/redo and pending
/// change records.
///
/// # Example
/// ```
/// use chrono::{NaiveDate, TimeZone, Utc};
/// use document::{BatchId, ChangeContext, Command, Document};
/// use model::{Duration, Project, Task};
///
/// let mut project = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let task = Task::new("Design the shed", Duration::from_days(2));
/// let id = task.id;
/// project.tasks.push(task);
/// let mut doc = Document::new(project);
///
/// let context = ChangeContext {
///     author: "Sam Lee".into(),
///     timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
///     batch_id: BatchId::new(),
/// };
/// // 2 days -> 3 days: Mon 1 Mar 08:00 + 3 working days = Wed 3 Mar 17:00.
/// doc.apply(Command::SetTaskDuration { id, duration: Duration::from_days(3) }, &context)
///     .unwrap();
/// let finish = doc.schedule().unwrap().project_finish;
/// assert_eq!(finish.to_string(), "2027-03-03 17:00:00");
///
/// doc.undo(&context).unwrap();
/// assert_eq!(doc.schedule().unwrap().project_finish.to_string(), "2027-03-02 17:00:00");
/// ```
#[derive(Clone, Debug)]
pub struct Document {
    /// What the user typed. Changed only by commands.
    project: Project,
    /// The engine's result for exactly the current `project`.
    schedule: Result<ScheduleResult, ScheduleError>,
    /// Undo and redo stacks.
    history: UndoStack,
    /// Change records not yet written to the file's journal.
    pending_changes: Vec<ChangeRecord>,
}

impl Document {
    /// Opens a project: stores it and schedules it immediately.
    ///
    /// # Parameters
    /// - `project`: the project to open. The document takes ownership.
    ///
    /// # Returns
    /// A document with empty undo/redo stacks and no pending changes. If the
    /// project cannot be scheduled (e.g. a task names a calendar that is
    /// missing), the document still opens; [`Document::schedule`] returns
    /// the error and the snapshot shows it.
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert!(doc.schedule().is_ok());
    /// assert!(!doc.can_undo());
    /// ```
    pub fn new(project: Project) -> Self {
        let schedule = reschedule::reschedule(&project);
        Document {
            project,
            schedule,
            history: UndoStack::default(),
            pending_changes: Vec::new(),
        }
    }

    /// Applies one edit command.
    ///
    /// # What it does
    /// 1. Copies the project and executes the command on the copy
    ///    (validation happens here, including the outline rule).
    /// 2. If nothing actually changed (e.g. renaming to the same name), stops:
    ///    no undo step, no records.
    /// 3. Reschedules the copy. If the schedule worked before and fails now,
    ///    or now has a circle, the command is refused.
    /// 4. Otherwise the copy becomes the project, the command goes on the
    ///    undo stack, the redo stack is emptied, and the change records are
    ///    added to the pending list.
    ///
    /// # Parameters
    /// - `command`: the edit.
    /// - `context`: author, timestamp and batch ID for the change records.
    ///
    /// # Returns
    /// An [`Outcome`] with this step's change records.
    ///
    /// # Errors
    /// Any [`DocumentError`] except `NothingToUndo`/`NothingToRedo`. On
    /// error the document is unchanged.
    ///
    /// # Example
    /// A circle is refused, and the project stays as it was:
    /// ```
    /// use chrono::{NaiveDate, Utc};
    /// use document::{BatchId, ChangeContext, Command, Document, DocumentError};
    /// use model::{Dependency, Duration, Project, Task};
    /// let mut project = Project::new("Loop", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// let a = Task::new("A", Duration::from_days(1));
    /// let b = Task::new("B", Duration::from_days(1));
    /// project.dependencies.push(Dependency::finish_to_start(a.id, b.id));
    /// let back = Dependency::finish_to_start(b.id, a.id); // would close a circle
    /// project.tasks.push(a);
    /// project.tasks.push(b);
    /// let mut doc = Document::new(project);
    /// let before = doc.project().clone();
    /// let context = ChangeContext {
    ///     author: "Sam Lee".into(),
    ///     timestamp: chrono::DateTime::<Utc>::UNIX_EPOCH,
    ///     batch_id: BatchId::new(),
    /// };
    /// let err = doc.apply(Command::AddDependency { dependency: back }, &context).unwrap_err();
    /// assert!(matches!(err, DocumentError::Rejected(_)));
    /// assert_eq!(doc.project(), &before);
    /// assert!(doc.pending_changes().is_empty());
    /// ```
    pub fn apply(
        &mut self,
        command: Command,
        context: &ChangeContext,
    ) -> Result<Outcome, DocumentError> {
        // Step 0: fill in values the caller may leave out (e.g. the current
        // start when a task is switched to manual scheduling).
        let command = command.resolve(&self.project, self.schedule.as_ref().ok());

        // Step 1: work on a copy. If anything below fails, we simply drop
        // the copy and the real project was never touched. (Copying a whole
        // project is simple and obviously correct; if it ever becomes slow
        // for huge projects, it can be optimised later.)
        let mut copy = self.project.clone();
        let executed = command.execute(&mut copy)?;

        // The outline must stay valid, if it was valid before. (A project
        // opened with a broken outline can still be edited and repaired.)
        if first_invalid_row(&self.project.tasks).is_none()
            && let Some(row) = first_invalid_row(&copy.tasks)
        {
            return Err(DocumentError::InvalidOutline { row });
        }

        // Every task stays inside a project, if it was so
        // before.
        if first_outside_project(&self.project).is_none()
            && let Some(row) = first_outside_project(&copy)
        {
            return Err(DocumentError::ProjectOutline { row });
        }

        // Project rows cannot have blockers either, even while empty.
        if let Some(task) = copy
            .tasks
            .iter()
            .find(|t| t.is_project && !t.blockers.is_empty())
        {
            return Err(DocumentError::BlockerOnSummary(task.id));
        }

        // Summary tasks cannot have blockers: a blocker added
        // to a summary task, or an indent / move / paste that turns a task
        // with blockers into a summary task, is refused. Same idea as the
        // outline rule: checked only if the project was fine before, so a
        // file opened in such a state can still be edited and repaired.
        if summary_with_blockers(&self.project).is_none()
            && let Some(task) = summary_with_blockers(&copy)
        {
            return Err(DocumentError::BlockerOnSummary(task));
        }

        // Risks must be filled in completely. A risk that is unchanged is
        // not checked: one upgraded from an older file may be incomplete.
        // Undo does not come through here, so it can bring such a risk back.
        if let Some(risk) = first_new_incomplete_risk(&self.project, &copy) {
            return Err(DocumentError::RiskIncomplete(risk));
        }

        // Step 2: a command that changed nothing is not worth an undo step.
        if executed.changes.is_empty() {
            return Ok(self.outcome(Vec::new()));
        }

        // Step 3: reschedule and check.
        let after = reschedule::reschedule(&copy);
        if let Some(error) = reschedule::rejection(self.schedule.is_ok(), &after) {
            return Err(DocumentError::Rejected(error));
        }

        // Step 4: commit. From here on nothing can fail.
        self.project = copy;
        self.schedule = after;
        self.history.push_new(UndoEntry {
            command,
            inverse: executed.inverse,
        });
        Ok(self.record(executed.changes, context))
    }

    /// Applies several commands as **one** undo step.
    ///
    /// # What it does
    /// A shortcut for `apply(Command::Batch { commands }, context)`: the
    /// commands run one after the other on a copy of the project; the
    /// outline and the schedule are checked once, after the last one; if
    /// anything fails, nothing changes. One Undo reverses all of them. All
    /// change records share the batch ID of `context`.
    ///
    /// # Parameters
    /// - `commands`: the edits, in the order they should run.
    /// - `context`: author, timestamp and batch ID for the change records.
    ///
    /// # Returns
    /// An [`Outcome`] with the change records of all the commands.
    ///
    /// # Errors
    /// The first [`DocumentError`] of any command, or `Rejected` if the
    /// result cannot be scheduled. On error the document is unchanged.
    ///
    /// # Example
    /// The task information dialog renames a task and changes its duration;
    /// one Undo brings both back:
    /// ```
    /// use chrono::{NaiveDate, Utc};
    /// use document::{BatchId, ChangeContext, Command, Document};
    /// use model::{Duration, Project, Task};
    /// let mut project = Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// let permit = Task::new("Get building permit", Duration::from_days(5));
    /// let id = permit.id;
    /// project.tasks.push(permit);
    /// let mut doc = Document::new(project);
    /// let context = ChangeContext {
    ///     author: "Sam Lee".into(),
    ///     timestamp: chrono::DateTime::<Utc>::UNIX_EPOCH,
    ///     batch_id: BatchId::new(),
    /// };
    /// let outcome = doc.apply_batch(vec![
    ///     Command::RenameTask { id, name: "Permit".into() },
    ///     Command::SetTaskDuration { id, duration: Duration::from_days(6) },
    /// ], &context).unwrap();
    /// assert_eq!(outcome.changes.len(), 2);
    ///
    /// doc.undo(&context).unwrap(); // one step undoes both
    /// assert_eq!(doc.project().tasks[0].name, "Get building permit");
    /// assert_eq!(doc.project().tasks[0].duration, Duration::from_days(5));
    /// assert!(!doc.can_undo());
    /// ```
    pub fn apply_batch(
        &mut self,
        commands: Vec<Command>,
        context: &ChangeContext,
    ) -> Result<Outcome, DocumentError> {
        self.apply(Command::Batch { commands }, context)
    }

    /// Undoes the most recent applied command.
    ///
    /// # What it does
    /// Takes the top entry off the undo stack, executes its inverse,
    /// reschedules, and puts the entry on the redo stack. The reversal
    /// produces change records too (old and new values swapped), so the
    /// journal always describes how the file got to its current state.
    ///
    /// # Parameters
    /// - `context`: author, timestamp and batch ID for the change records.
    ///
    /// # Returns
    /// An [`Outcome`] with the reversal's change records.
    ///
    /// # Errors
    /// [`DocumentError::NothingToUndo`] if the undo stack is empty. Any other
    /// error would mean a bug (an inverse that does not fit); the document
    /// is then left unchanged.
    ///
    /// # Example
    /// See [`Document`] and the `shed_edit` example.
    pub fn undo(&mut self, context: &ChangeContext) -> Result<Outcome, DocumentError> {
        let entry = self
            .history
            .pop_undo()
            .ok_or(DocumentError::NothingToUndo)?;
        match self.step(&entry.inverse) {
            Ok(changes) => {
                self.history.push_redo(entry);
                Ok(self.record(changes, context))
            }
            Err(error) => {
                // Put the entry back so the stacks are unchanged.
                self.history.push_undo(entry);
                Err(error)
            }
        }
    }

    /// Redoes the most recently undone command.
    ///
    /// # What it does
    /// Takes the top entry off the redo stack, executes the original command
    /// again, reschedules, and puts the entry back on the undo stack.
    ///
    /// # Parameters
    /// - `context`: author, timestamp and batch ID for the change records.
    ///
    /// # Returns
    /// An [`Outcome`] with the change records.
    ///
    /// # Errors
    /// [`DocumentError::NothingToRedo`] if the redo stack is empty.
    ///
    /// # Example
    /// See the `shed_edit` example.
    pub fn redo(&mut self, context: &ChangeContext) -> Result<Outcome, DocumentError> {
        let entry = self
            .history
            .pop_redo()
            .ok_or(DocumentError::NothingToRedo)?;
        match self.step(&entry.command) {
            Ok(changes) => {
                self.history.push_undo(entry);
                Ok(self.record(changes, context))
            }
            Err(error) => {
                self.history.push_redo(entry);
                Err(error)
            }
        }
    }

    /// `true` if there is a command to undo (enables the Undo button).
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert!(!doc.can_undo());
    /// ```
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// `true` if there is a command to redo (enables the Redo button).
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert!(!doc.can_redo());
    /// ```
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The current project (read only).
    ///
    /// # Beginner note
    /// It returns `&Project`, a *borrowed* view. The caller can read the
    /// project but cannot change it; changes go through [`Document::apply`].
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert_eq!(doc.project().name, "Shed");
    /// ```
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The latest schedule: the engine's result, or its error.
    ///
    /// # Returns
    /// `Ok(&ScheduleResult)` or `Err(&ScheduleError)`, both borrowed.
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert!(doc.schedule().unwrap().tasks.is_empty());
    /// ```
    pub fn schedule(&self) -> Result<&ScheduleResult, &ScheduleError> {
        // `as_ref` turns `&Result<A, B>` into `Result<&A, &B>`.
        self.schedule.as_ref()
    }

    /// Builds the [`ScheduleSnapshot`] for the UI: tasks with their dates,
    /// in row order.
    ///
    /// # Example
    /// See [`ScheduleSnapshot`].
    pub fn snapshot(&self) -> ScheduleSnapshot {
        ScheduleSnapshot::build(&self.project, self.schedule())
    }

    /// The change records waiting to be written to the journal (oldest first).
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// assert!(doc.pending_changes().is_empty());
    /// ```
    pub fn pending_changes(&self) -> &[ChangeRecord] {
        &self.pending_changes
    }

    /// Hands over all pending change records and empties the list.
    ///
    /// The Tauri shell calls this when it saves, and passes the records to
    /// the file layer (`gtra`), which writes them into the `journal` table.
    /// (`document` itself never touches files.)
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use document::Document;
    /// use model::Project;
    /// let mut doc = Document::new(Project::new("Empty", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()));
    /// let records = doc.take_pending_changes();
    /// assert!(records.is_empty());
    /// ```
    pub fn take_pending_changes(&mut self) -> Vec<ChangeRecord> {
        // `mem::take` swaps an empty list in and returns the old one.
        std::mem::take(&mut self.pending_changes)
    }

    // -- private helpers ----------------------------------------------------

    /// Executes a command for undo or redo: on a copy, then reschedule and
    /// commit. Unlike `apply`, the result is never refused because of the
    /// schedule: undo and redo return to a state that already existed.
    ///
    /// # Returns
    /// The field changes, or an error (the document is then unchanged).
    fn step(&mut self, command: &Command) -> Result<Vec<FieldChange>, DocumentError> {
        let mut copy = self.project.clone();
        let executed = command.execute(&mut copy)?;
        self.schedule = reschedule::reschedule(&copy);
        self.project = copy;
        Ok(executed.changes)
    }

    /// Stamps field changes with the context, adds them to the pending list
    /// and builds the [`Outcome`].
    fn record(&mut self, changes: Vec<FieldChange>, context: &ChangeContext) -> Outcome {
        let records: Vec<ChangeRecord> = changes.into_iter().map(|c| c.stamp(context)).collect();
        self.pending_changes.extend(records.iter().cloned());
        self.outcome(records)
    }

    /// Builds an [`Outcome`] with the current state of the stacks.
    fn outcome(&self, changes: Vec<ChangeRecord>) -> Outcome {
        Outcome {
            changes,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }
}
