//! Reading a project from a `.gtra` database: SQLite rows → `model::Project`.
//!
//! # What this file is for
//! The partner of `write.rs`. It reads the tables of `migrations/0001_init.sql`
//! (with the columns added by `0002_m1_fields.sql` and the `blockers` table
//! of `0003_blockers.sql`) and builds the
//! [`model::Project`] the rest of Ganttora works with. It also
//! makes a list of [`ReadNote`]s: everything in the file that the model
//! cannot hold yet, or that had to be filled in. Nothing is dropped silently.
//!
//! # Where it fits
//! [`crate::open`] (in `lib.rs`) copies the user's file to a local working
//! copy, opens it, calls [`GtraFile::read_project`] and returns an
//! [`OpenedProject`] to the app.
//!
//! # How the tables become a project
//! ```text
//!   .gtra tables                                      model::Project
//!   ───────────────────────────────────────────       ──────────────────────────
//!   project (exactly one row)                    ──►  id, name, start, calendar_id,
//!                                                     schedule_from, finish
//!   project_settings 'minutes_per_day'           ──►  minutes_per_day (480 if absent)
//!
//!   calendars + calendar_week_days               ──►  calendars, sorted by name, then id
//!     + calendar_working_times                          week: 7 weekdays of blocks
//!     + calendar_exceptions kind 'holiday'              holidays: every date of every range
//!     + calendar_exceptions kind 'exception'            exceptions: by position, then
//!       + calendar_exception_times                        start date, then id
//!     (a weekday a calendar does not define
//!      is taken from its base calendar)
//!
//!   tasks (a tree: parent_id + position)         ──►  tasks in row order,
//!     walked depth-first, siblings by position          outline_level = depth (1 = top)
//!     mode, manual_start, is_milestone,                 mode, manual_start, milestone,
//!     constraint_type + constraint_date,                constraint (one enum value),
//!     deadline, calendar_id, notes, hyperlink           deadline, calendar_id, notes, hyperlink
//!
//!   blockers (task_id + position)                ──►  Task::blockers of that task, by
//!     after_work_minutes, duration_minutes,             position (ties: by id)
//!     reason, reported_on '2027-03-04' / NULL           after_work, duration, reason, reported_on
//!
//!   dependencies                                 ──►  dependencies, sorted by
//!     type 'FS'… , lag_minutes,                         (predecessor row, successor row)
//!     lag_percent (0.5 → 50)                            lag, lag_percent
//! ```
//!
//! # The order of lists
//! The file stores an order for tasks (`position` in the tree), for
//! calendar exceptions (`position`) and for each task's blockers
//! (`position`). Dependencies and calendars have no
//! order column, so they are returned in a fixed, predictable order (see the
//! diagram). A project whose lists are already in that order, such as the
//! garden shed, reads back exactly equal to the project that was saved.
//!
//! # Rebuilding the outline levels
//! The tree walk below gives every task its depth as `outline_level`. Depths
//! from a tree always form a valid outline, so [`model::Outline::build`] on
//! the result finds exactly the same parents and positions that are stored
//! in the file: reading and writing agree.
//!
//! # Depth-first walk
//! "Depth-first" means: take a task, then all of its subtasks (and theirs)
//! before moving on to its next sibling. That is exactly the order of the
//! rows in the grid:
//!
//! ```text
//!   tree                        rows (depth-first)
//!   ├── Design (pos 1)          1  Design         level 1
//!   ├── Build  (pos 2)          2  Build          level 1
//!   │   ├── Floor (pos 1)       3    Floor        level 2
//!   │   └── Walls (pos 2)       4    Walls        level 2
//!   └── Done   (pos 3)          5  Done           level 1
//! ```
//!

use std::collections::{BTreeMap, BTreeSet};

use chrono::{NaiveDate, NaiveDateTime};
use model::{
    Blocker, BlockerId, Calendar, CalendarException, CalendarId, Constraint,
    DEFAULT_MINUTES_PER_DAY, Dependency, DependencyId, DependencyType, Duration, Outline, Project,
    ProjectId, Risk, RiskId, RiskPriority, RiskResponse, ScheduleFrom, Task, TaskId, TaskMode,
    WorkWeek, WorkingTime,
};
use rusqlite::Connection;
use uuid::Uuid;

use crate::write::{WEEKDAYS, date_range, parse_date};
use crate::{Access, DATE_TIME_FORMAT, GtraError, GtraFile};

/// Something in the file that did not fit the model exactly. The app shows
/// these to the user after opening (nothing is lost silently).
///
/// # Variants
/// See each variant below. The UI turns each one into a translated message
/// (the text is not made here, because all UI text lives in `locales/`).
///
/// # Example
/// ```
/// let note = gtra::ReadNote::RowsNotLoaded { table: "resources", count: 3 };
/// assert!(matches!(note, gtra::ReadNote::RowsNotLoaded { count: 3, .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadNote {
    /// The project had no start date; the day the file was created (at
    /// 00:00) was used instead. A file made by [`crate::create_empty`] has
    /// no start date yet.
    StartDateMissing {
        /// The start that was used.
        used: NaiveDateTime,
    },
    /// The project had no project calendar. The first calendar (sorted by
    /// name, then id) was used, or a new Standard calendar was added if there was none.
    CalendarMissing {
        /// The calendar that is now the project calendar.
        used: CalendarId,
    },
    /// A task had no duration ("not entered yet"); 0 was used, which makes
    /// it a milestone (a task with zero duration is shown as a milestone).
    DurationMissing {
        /// The task concerned.
        task: TaskId,
    },
    /// A dependency's lag was a percentage of the predecessor's duration and
    /// was converted to minutes.
    ///
    /// **No longer produced** since format version 2: the model now keeps
    /// percentage lags as they are (`Dependency::lag_percent`). The variant
    /// stays only so that code matching on it (the Tauri shell's
    /// `ReadNoteDto`) keeps compiling; it will be removed once that code is
    /// updated.
    PercentLagConverted {
        /// The dependency concerned.
        dependency: DependencyId,
        /// The lag in working minutes that was used.
        minutes: i64,
    },
    /// A calendar was based on another calendar. The model has complete
    /// calendars only, so the inherited weekdays, holidays and exceptions
    /// were copied into it; after a save it no longer follows its base
    /// calendar.
    BaseCalendarFlattened {
        /// The calendar concerned.
        calendar: CalendarId,
        /// Id of its base calendar, as stored in the file.
        base: String,
    },
    /// A stored value the model cannot hold yet, such as a task's task type
    /// or the project's notes. It was not loaded, but it **stays in the
    /// file** when the project is saved (as long as its row still exists).
    ///
    /// Also used for `dependencies.lag_percent` when the stored fraction is
    /// not a whole percentage (e.g. 0.333, written by another tool): the
    /// nearest whole percentage was loaded, and the next save stores that.
    ///
    /// Also used for `blockers` rows (format 3) that break a rule the app
    /// itself never breaks. These blockers **are loaded** and saved back
    /// unchanged; the note only tells the user that something looks wrong:
    /// - `column: "task_id"`, `value` = the task's id: the blocker is on a
    ///   summary task. The engine ignores blockers on summary tasks, so it
    ///   has no effect on the schedule.
    /// - `column: "after_work_minutes"` or `"duration_minutes"`, `value` =
    ///   the stored number: the value is negative. The engine treats it
    ///   as 0.
    ///
    /// In both cases `table` is `"blockers"` and `id` is the blocker's id.
    ValueNotLoaded {
        /// The table, e.g. `"tasks"`.
        table: &'static str,
        /// Id of the row (a UUID, or the key for key/value tables).
        id: String,
        /// The column, e.g. `"constraint_type"`.
        column: &'static str,
        /// The stored value, as text.
        value: String,
    },
    /// A whole table the model does not have yet (resources, assignments).
    /// Its rows stay in the file when the project is saved.
    ///
    /// Also used with `table: "blockers"` for blocker rows whose task is not
    /// in the file (only possible if another tool wrote them with foreign
    /// keys switched off). They were not loaded, and they stay in the file.
    RowsNotLoaded {
        /// The table, e.g. `"resources"`.
        table: &'static str,
        /// How many rows it has.
        count: u64,
    },
}

/// Information about the file itself, from its `meta` table. Each value is
/// `None` if the file does not have it.
///
/// Timestamps are kept as the text stored in the file, e.g.
/// `"2027-02-15T10:42:07Z"` (a moment in UTC).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileInfo {
    /// Display name of the person who last saved the file.
    pub author: Option<String>,
    /// When the file was created.
    pub created: Option<String>,
    /// When the file was last saved.
    pub modified: Option<String>,
    /// Version of Ganttora that last saved the file, e.g. `"0.1.0"`.
    pub app_version: Option<String>,
    /// Version of the scheduling engine that computed the saved dates
    /// (`meta.engine_version`), e.g. `"0.1.0"`.
    pub engine_version: Option<String>,
}

/// Everything [`crate::open`] hands back: the project plus what the app
/// should know about the file.
///
/// # Fields
/// - `project`: the project, ready for the engine and the UI.
/// - `notes`: what did not fit the model (show these to the user).
/// - `access`: [`Access::ReadOnly`] if the file comes from a newer Ganttora;
///   the app must then not save over it (offer "Save As" instead).
/// - `format_version`: the file's format version.
/// - `upgraded_from`: `Some(old)` if the file has an older format; it will
///   be saved in the current format.
/// - `read_only_reason`: why the file is read-only (`None` if writable).
/// - `info`: author, timestamps and versions from `meta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedProject {
    /// The project read from the file.
    pub project: Project,
    /// Things that did not fit the model exactly.
    pub notes: Vec<ReadNote>,
    /// Read-write, or read-only for a file from a newer Ganttora.
    pub access: Access,
    /// The file's format version as stored on disk.
    pub format_version: u32,
    /// The older format version, if the file needs an upgrade when saved.
    pub upgraded_from: Option<u32>,
    /// Why the file is read-only, as a language-neutral code plus details
    /// ([`crate::ReadOnlyReason`]); `None` for a writable file.
    pub read_only_reason: Option<crate::ReadOnlyReason>,
    /// Author, timestamps and versions from `meta`.
    pub info: FileInfo,
}

impl GtraFile {
    /// Reads the whole project from this database.
    ///
    /// # What it does
    /// Reads, in this order: the project settings, the calendars, the
    /// `project` row, the tasks (walking the outline tree), each task's
    /// blockers, the dependencies,
    /// and counts the rows of tables the model does not have yet. Works on
    /// read-only files too.
    ///
    /// # Returns
    /// A pair `(project, notes)`. The notes list everything that did not fit
    /// the model (see [`ReadNote`]); it is empty for a file saved from a
    /// [`Project`] by this version of Ganttora.
    ///
    /// # Errors
    /// - [`GtraError::InvalidData`] if the file's content breaks a rule the
    ///   database cannot check by itself: not exactly one `project` row, an
    ///   unreadable UUID or date, tasks whose parents form a loop, a calendar
    ///   based on itself through other calendars.
    /// - [`GtraError::Sqlite`] if a query fails.
    ///
    /// # Example
    /// See [`GtraFile::write_project`] and [`crate::open`].
    ///
    /// # Beginner note
    /// `(Project, Vec<ReadNote>)` is a **tuple**: two values travelling
    /// together without a struct around them. The caller takes them apart
    /// with `let (project, notes) = file.read_project()?;`.
    pub fn read_project(&self) -> Result<(Project, Vec<ReadNote>), GtraError> {
        let conn = &self.conn;
        let mut notes = Vec::new();

        let minutes_per_day = read_minutes_per_day(conn, &mut notes)?;
        let mut calendars = read_calendars(conn, &mut notes)?;
        let row = read_project_row(conn, &mut notes)?;

        // The start date, or the day the file was created.
        let start = match row.start {
            Some(text) => parse_date_time(&text)?,
            None => {
                let used = start_from_created(self.meta("created")?.as_deref())?;
                notes.push(ReadNote::StartDateMissing { used });
                used
            }
        };

        // The project calendar, or a stand-in (see ReadNote::CalendarMissing).
        let calendar_id = match row.calendar_id {
            Some(text) => CalendarId::from_uuid(parse_uuid(&text, "project calendar")?),
            None => {
                let used = match calendars.first() {
                    Some(calendar) => calendar.id,
                    None => {
                        let standard = Calendar::standard();
                        let id = standard.id;
                        calendars.push(standard);
                        id
                    }
                };
                notes.push(ReadNote::CalendarMissing { used });
                used
            }
        };

        let (mut tasks, task_rows) = read_tasks(conn, &mut notes)?;
        read_blockers(conn, &mut tasks, &task_rows, &mut notes)?;
        read_risks(conn, &mut tasks, &task_rows, &mut notes)?;
        let dependencies = read_dependencies(conn, &tasks, &task_rows, &mut notes)?;

        for table in ["resources", "assignments"] {
            let count: u64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            if count > 0 {
                notes.push(ReadNote::RowsNotLoaded { table, count });
            }
        }

        let project = Project {
            id: ProjectId::from_uuid(parse_uuid(&row.id, "project id")?),
            name: row.name,
            start,
            schedule_from: row.schedule_from,
            // `transpose` turns `Option<Result<…>>` into `Result<Option<…>>`,
            // so `?` can stop on a bad date while `None` stays `None`.
            finish: row.finish.as_deref().map(parse_date_time).transpose()?,
            calendar_id,
            minutes_per_day,
            tasks,
            dependencies,
            calendars,
        };
        Ok((project, notes))
    }

    /// Reads the author and timestamps from the `meta` table.
    ///
    /// # Returns
    /// A [`FileInfo`]; missing entries are `None`.
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if a query fails.
    pub fn file_info(&self) -> Result<FileInfo, GtraError> {
        Ok(FileInfo {
            author: self.meta("author")?,
            created: self.meta("created")?,
            modified: self.meta("modified")?,
            app_version: self.meta("app_version")?,
            engine_version: self.meta("engine_version")?,
        })
    }
}

/// Reads `minutes_per_day` from `project_settings` (480 if absent) and adds
/// a note for every other setting, which the model does not have yet.
///
/// # Errors
/// [`GtraError::InvalidData`] if `minutes_per_day` is not a positive whole
/// number; [`GtraError::Sqlite`] if the query fails.
fn read_minutes_per_day(conn: &Connection, notes: &mut Vec<ReadNote>) -> Result<i64, GtraError> {
    let mut minutes_per_day = DEFAULT_MINUTES_PER_DAY;
    let mut stmt = conn.prepare("SELECT key, value FROM project_settings ORDER BY key")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (key, value) in rows {
        if key == "minutes_per_day" {
            // The value is JSON text; a JSON whole number is just digits.
            minutes_per_day = value
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|minutes| *minutes > 0)
                .ok_or_else(|| {
                    GtraError::InvalidData(format!(
                        "minutes_per_day must be a positive whole number, not '{value}'"
                    ))
                })?;
        } else {
            notes.push(ReadNote::ValueNotLoaded {
                table: "project_settings",
                id: key,
                column: "value",
                value,
            });
        }
    }
    Ok(minutes_per_day)
}

/// One row of the `calendars` table, plus everything collected for it.
struct CalendarRow {
    /// Parsed id.
    id: CalendarId,
    /// Display name.
    name: String,
    /// Base calendar id as stored, if any.
    base: Option<String>,
}

/// Reads every calendar, resolving base calendars, sorted by name then id.
///
/// # How inheritance is resolved
/// For each weekday, we follow the chain "this calendar → its base → the
/// base's base …" and use the first calendar that defines that weekday. A
/// weekday nobody defines is a day off. Holidays of the whole chain are
/// combined; exceptions are listed the calendar's own first, then those of
/// its base, and so on (the first exception covering a date wins).
///
/// # Errors
/// [`GtraError::InvalidData`] for an unreadable id or date, or a chain of
/// base calendars that loops; [`GtraError::Sqlite`] if a query fails.
fn read_calendars(
    conn: &Connection,
    notes: &mut Vec<ReadNote>,
) -> Result<Vec<Calendar>, GtraError> {
    // 1. The calendars themselves, keyed by their id text.
    let mut rows: BTreeMap<String, CalendarRow> = BTreeMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, name, base_calendar_id FROM calendars")?;
        let raw: Vec<(String, String, Option<String>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (id_text, name, base) in raw {
            let id = CalendarId::from_uuid(parse_uuid(&id_text, "calendar id")?);
            rows.insert(id_text, CalendarRow { id, name, base });
        }
    }

    // 2. Which weekdays each calendar defines: (calendar, weekday) → working?
    let mut week_days: BTreeMap<(String, u32), bool> = BTreeMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT calendar_id, weekday, is_working FROM calendar_week_days")?;
        let raw: Vec<(String, u32, bool)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (calendar, weekday, working) in raw {
            week_days.insert((calendar, weekday), working);
        }
    }

    // 3. The working-time blocks of each (calendar, weekday), in time order.
    let mut times: BTreeMap<(String, u32), Vec<WorkingTime>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT calendar_id, weekday, start_minute, end_minute
             FROM calendar_working_times ORDER BY calendar_id, weekday, start_minute",
        )?;
        let raw: Vec<(String, u32, u32, u32)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        for (calendar, weekday, start, end) in raw {
            times
                .entry((calendar, weekday))
                .or_default()
                .push(WorkingTime::new(start, end));
        }
    }

    // 4. The working times of working exceptions, keyed by exception id,
    //    in time order.
    let mut exception_times: BTreeMap<String, Vec<WorkingTime>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT exception_id, start_minute, end_minute
             FROM calendar_exception_times ORDER BY exception_id, start_minute",
        )?;
        let raw: Vec<(String, u32, u32)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (exception, start, end) in raw {
            exception_times
                .entry(exception)
                .or_default()
                .push(WorkingTime::new(start, end));
        }
    }

    // 5. The exception rows. `kind` says which list of the model they belong
    //    to (migration 0002): 'holiday' rows become holiday dates, 'exception'
    //    rows become CalendarExceptions, already in list order thanks to
    //    ORDER BY position (equal positions, from upgraded files, by date).
    let mut own_holidays: BTreeMap<String, BTreeSet<NaiveDate>> = BTreeMap::new();
    let mut own_exceptions: BTreeMap<String, Vec<CalendarException>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, calendar_id, name, start_date, end_date, is_working, kind
             FROM calendar_exceptions ORDER BY position, start_date, id",
        )?;
        // A type for one row, to keep the line below readable.
        type Raw = (String, String, String, String, String, bool, String);
        let raw: Vec<Raw> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        for (id, calendar, name, start, end, working, kind) in raw {
            if kind == "holiday" {
                // Write.rs never makes a working holiday; a damaged or
                // foreign file might. Its hours cannot be a holiday, so it
                // is reported instead of being treated as a day off.
                if working {
                    notes.push(ReadNote::ValueNotLoaded {
                        table: "calendar_exceptions",
                        id,
                        column: "is_working",
                        value: format!("1 (holiday {start} to {end})"),
                    });
                    continue;
                }
                own_holidays
                    .entry(calendar)
                    .or_default()
                    .extend(date_range(&start, &end)?);
            } else {
                // A "working" exception keeps its hours; a non-working one
                // has none (any stray hours rows are ignored).
                let working_times = if working {
                    exception_times.remove(&id).unwrap_or_default()
                } else {
                    Vec::new()
                };
                own_exceptions
                    .entry(calendar)
                    .or_default()
                    .push(CalendarException {
                        name,
                        start: parse_date(&start)?,
                        finish: parse_date(&end)?,
                        working_times,
                    });
            }
        }
    }

    // 6. Build each model calendar, following the base-calendar chain.
    let mut calendars = Vec::with_capacity(rows.len());
    for (id_text, row) in &rows {
        let chain = base_chain(id_text, &rows)?;

        // One list of blocks per weekday, Monday first.
        let mut days: [Vec<WorkingTime>; 7] = Default::default();
        for (index, weekday) in WEEKDAYS.iter().enumerate() {
            let iso = weekday.number_from_monday();
            // `find_map` returns the first calendar in the chain that defines
            // this weekday, together with whether it is working.
            let decided = chain.iter().find_map(|calendar| {
                week_days
                    .get(&(calendar.clone(), iso))
                    .map(|working| (calendar, *working))
            });
            if let Some((calendar, true)) = decided {
                days[index] = times
                    .get(&(calendar.clone(), iso))
                    .cloned()
                    .unwrap_or_default();
            }
        }
        // "Destructuring": take the array apart into seven named variables.
        let [
            monday,
            tuesday,
            wednesday,
            thursday,
            friday,
            saturday,
            sunday,
        ] = days;

        // Holidays of the whole chain are combined. Exceptions are listed
        // the calendar's own first, then its base's, and so on: the first
        // exception covering a date wins, so the most specific one wins.
        let mut holidays = BTreeSet::new();
        let mut exceptions = Vec::new();
        for calendar in &chain {
            if let Some(dates) = own_holidays.get(calendar) {
                holidays.extend(dates.iter().copied());
            }
            if let Some(list) = own_exceptions.get(calendar) {
                exceptions.extend(list.iter().cloned());
            }
        }

        if let Some(base) = &row.base {
            notes.push(ReadNote::BaseCalendarFlattened {
                calendar: row.id,
                base: base.clone(),
            });
        }

        calendars.push(Calendar {
            id: row.id,
            name: row.name.clone(),
            week: WorkWeek {
                monday,
                tuesday,
                wednesday,
                thursday,
                friday,
                saturday,
                sunday,
            },
            holidays,
            exceptions,
        });
    }

    // A predictable order: by name, and by id when two names are equal.
    calendars.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(calendars)
}

/// Returns the chain of calendar ids starting at `start`: the calendar, its
/// base, the base's base, …, stopping at a calendar without a base (or one
/// whose base is missing from the file).
///
/// # Errors
/// [`GtraError::InvalidData`] if the chain comes back to a calendar it
/// already visited (a loop).
fn base_chain(start: &str, rows: &BTreeMap<String, CalendarRow>) -> Result<Vec<String>, GtraError> {
    let mut chain = vec![start.to_string()];
    let mut current = start;
    while let Some(base) = rows.get(current).and_then(|row| row.base.as_deref()) {
        if chain.iter().any(|seen| seen == base) {
            return Err(GtraError::InvalidData(format!(
                "calendar {start} is based on itself through other calendars"
            )));
        }
        if !rows.contains_key(base) {
            break;
        }
        chain.push(base.to_string());
        current = base;
    }
    Ok(chain)
}

/// The `project` row, with the columns the model uses kept as text.
struct ProjectRow {
    /// `project.id`.
    id: String,
    /// `project.name`.
    name: String,
    /// `project.start_date`, if set.
    start: Option<String>,
    /// `project.calendar_id`, if set.
    calendar_id: Option<String>,
    /// `project.schedule_from`, already turned into the model's enum.
    schedule_from: ScheduleFrom,
    /// `project.finish_date`, if set (text, parsed by the caller).
    finish: Option<String>,
}

/// Reads the single `project` row, with notes for the columns the model
/// does not have yet (`status_date`, `notes`).
///
/// # Errors
/// [`GtraError::InvalidData`] if there is not exactly one row, or
/// `schedule_from` holds an unknown value; [`GtraError::Sqlite`] if the
/// query fails.
fn read_project_row(conn: &Connection, notes: &mut Vec<ReadNote>) -> Result<ProjectRow, GtraError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, start_date, calendar_id, schedule_from, finish_date, status_date, notes
         FROM project",
    )?;
    // A type for one row, to keep the line below readable.
    type Raw = (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    );
    let mut raw: Vec<Raw> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    if raw.len() != 1 {
        return Err(GtraError::InvalidData(format!(
            "the file must contain exactly one project, but it contains {}",
            raw.len()
        )));
    }
    // `pop` takes the last (here: the only) element out of the list.
    let (id, name, start, calendar_id, schedule_from, finish_date, status_date, project_notes) =
        raw.pop().expect("exactly one row, checked above");

    let mut not_loaded = |column: &'static str, value: String| {
        notes.push(ReadNote::ValueNotLoaded {
            table: "project",
            id: id.clone(),
            column,
            value,
        });
    };
    if let Some(value) = status_date {
        not_loaded("status_date", value);
    }
    if !project_notes.is_empty() {
        not_loaded("notes", project_notes);
    }

    // The table's CHECK rule allows only these two words.
    let schedule_from = match schedule_from.as_str() {
        "start" => ScheduleFrom::Start,
        "finish" => ScheduleFrom::Finish,
        other => {
            return Err(GtraError::InvalidData(format!(
                "unknown project schedule_from '{other}'"
            )));
        }
    };

    Ok(ProjectRow {
        id,
        name,
        start,
        calendar_id,
        schedule_from,
        finish: finish_date,
    })
}

/// One row of the `tasks` table, as read, before its place in the outline
/// is known.
struct TaskRow {
    /// `tasks.id` as text (the key used by `parent_id`).
    id: String,
    /// `tasks.parent_id` as text, `None` at the top level.
    parent: Option<String>,
    /// Every model field read from the row. `outline_level` is filled in
    /// later, by the tree walk.
    task: Task,
    /// `true` if `duration_minutes` was NULL ("not entered yet"); 0 was used.
    duration_missing: bool,
}

/// Reads the tasks and puts them in row order by walking the outline tree
/// depth-first (see the file header).
///
/// # Returns
/// The tasks in row order, and a map from task id text to row index (0, 1,
/// 2, …), used to sort the dependencies.
///
/// # Errors
/// [`GtraError::InvalidData`] for an unreadable id, date or code, or tasks
/// that cannot be reached from the top level (their parents form a loop or
/// are missing); [`GtraError::Sqlite`] if the query fails.
fn read_tasks(
    conn: &Connection,
    notes: &mut Vec<ReadNote>,
) -> Result<(Vec<Task>, BTreeMap<String, usize>), GtraError> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, duration_minutes,
                mode, is_milestone, task_type, effort_driven, constraint_type, constraint_date,
                deadline, manual_start, manual_finish, calendar_id, notes, hyperlink,
                percent_complete, is_project, planned_finish, actual_finish
         FROM tasks ORDER BY position, id",
    )?;
    let mut rows: Vec<TaskRow> = Vec::new();
    // `query` gives the rows one at a time, so we can look at many columns
    // without building a huge tuple.
    let mut result = stmt.query([])?;
    while let Some(row) = result.next()? {
        let id_text: String = row.get(0)?;
        let id = TaskId::from_uuid(parse_uuid(&id_text, "task id")?);
        let duration: Option<i64> = row.get(3)?;

        // Columns the model does not have yet: a note whenever the value is
        // not the default.
        let mut not_loaded = |column: &'static str, value: String| {
            notes.push(ReadNote::ValueNotLoaded {
                table: "tasks",
                id: id_text.clone(),
                column,
                value,
            });
        };
        let task_type: String = row.get(6)?;
        if task_type != "fixed_units" {
            not_loaded("task_type", task_type);
        }
        let effort_driven: bool = row.get(7)?;
        if effort_driven {
            not_loaded("effort_driven", "1".to_string());
        }
        let manual_finish: Option<String> = row.get(12)?;
        if let Some(value) = manual_finish {
            not_loaded("manual_finish", value);
        }

        // Columns the model has (since M1). Each optional date-time column
        // is NULL or text such as '2027-03-08T08:00:00'.
        let mode: String = row.get(4)?;
        let constraint_type: String = row.get(8)?;
        let constraint_date: Option<String> = row.get(9)?;
        let deadline: Option<String> = row.get(10)?;
        let manual_start: Option<String> = row.get(11)?;
        let calendar: Option<String> = row.get(13)?;
        let planned_finish: Option<String> = row.get(18)?;
        let actual_finish: Option<String> = row.get(19)?;

        let task = Task {
            mode: parse_task_mode(&mode)?,
            manual_start: optional_date_time(manual_start.as_deref())?,
            milestone: row.get(5)?,
            constraint: parse_constraint(&constraint_type, constraint_date.as_deref())?,
            deadline: optional_date_time(deadline.as_deref())?,
            calendar_id: calendar
                .as_deref()
                .map(|text| parse_uuid(text, "task calendar").map(CalendarId::from_uuid))
                .transpose()?,
            notes: row.get(14)?,
            hyperlink: row.get(15)?,
            percent_complete: row.get(16)?,
            is_project: row.get(17)?,
            planned_finish: optional_date_time(planned_finish.as_deref())?,
            actual_finish: optional_date_time(actual_finish.as_deref())?,
            // The rest (id, name, duration, outline_level 1 for now) comes
            // from `Task::with_id`. `..` is "struct update syntax": take every
            // field not written above from this other value.
            ..Task::with_id(
                id,
                row.get::<_, String>(2)?,
                Duration::from_minutes(duration.unwrap_or(0)),
            )
        };
        rows.push(TaskRow {
            id: id_text,
            parent: row.get(1)?,
            task,
            duration_missing: duration.is_none(),
        });
    }

    // Children of each parent, already in position order thanks to ORDER BY.
    // The key `None` holds the top-level tasks.
    let mut children: BTreeMap<Option<&str>, Vec<usize>> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        children
            .entry(row.parent.as_deref())
            .or_default()
            .push(index);
    }

    // Depth-first walk with a "stack": a list where we always take the last
    // item. Children are pushed in reverse, so the first child comes out first.
    let mut order: Vec<(usize, u32)> = Vec::with_capacity(rows.len());
    let mut stack: Vec<(usize, u32)> = children
        .get(&None)
        .map(|top| top.iter().rev().map(|&index| (index, 1)).collect())
        .unwrap_or_default();
    while let Some((index, level)) = stack.pop() {
        order.push((index, level));
        if let Some(kids) = children.get(&Some(rows[index].id.as_str())) {
            stack.extend(kids.iter().rev().map(|&kid| (kid, level + 1)));
        }
    }
    if order.len() != rows.len() {
        return Err(GtraError::InvalidData(format!(
            "{} task(s) cannot be placed in the outline: their parent tasks are missing or form a loop",
            rows.len() - order.len()
        )));
    }

    let mut tasks = Vec::with_capacity(rows.len());
    let mut row_of: BTreeMap<String, usize> = BTreeMap::new();
    for (row_number, (index, level)) in order.into_iter().enumerate() {
        let row = &rows[index];
        if row.duration_missing {
            notes.push(ReadNote::DurationMissing { task: row.task.id });
        }
        row_of.insert(row.id.clone(), row_number);
        // The depth in the tree is the outline level (see the file header,
        // "Rebuilding the outline levels").
        tasks.push(Task {
            outline_level: level,
            ..row.task.clone()
        });
    }
    Ok((tasks, row_of))
}

/// Reads the `blockers` table and puts each blocker into its task's
/// `blockers` list.
///
/// # What it does
/// 1. Reads every row, sorted by task, then `position`, then `id`, so each
///    task's list comes back in the order it was saved (the id only breaks
///    ties, so the order never depends on how SQLite stores the rows).
/// 2. A row whose task is not in the file is **skipped**; all such rows
///    together give one [`ReadNote::RowsNotLoaded`] note. They stay in the
///    file (`write.rs` does not delete them).
/// 3. A row on a summary task, or with a negative length, is **loaded** but
///    gets a [`ReadNote::ValueNotLoaded`] note (see that variant for the
///    exact codes).
///
/// # Parameters
/// - `conn`: the database.
/// - `tasks`: the tasks from [`read_tasks`], in row order; blockers are
///   added to them.
/// - `task_rows`: task id text → row index in `tasks` (from [`read_tasks`]).
/// - `notes`: the list that receives the notes.
///
/// # Errors
/// [`GtraError::InvalidData`] for an unreadable blocker id or date;
/// [`GtraError::Sqlite`] if the query fails.
///
/// # Beginner note
/// `&mut [Task]` is a *mutable borrow* of a slice: this function may change
/// the tasks in place (push blockers into them), but it does not own the
/// list, so it cannot add or remove tasks.
fn read_blockers(
    conn: &Connection,
    tasks: &mut [Task],
    task_rows: &BTreeMap<String, usize>,
    notes: &mut Vec<ReadNote>,
) -> Result<(), GtraError> {
    // Which rows are summary tasks, worked out from the outline levels that
    // `read_tasks` has just rebuilt.
    let outline = Outline::build(tasks);
    let mut stmt = conn.prepare(
        "SELECT id, task_id, after_work_minutes, duration_minutes, reason, reported_on
         FROM blockers ORDER BY task_id, position, id",
    )?;
    let mut result = stmt.query([])?;
    let mut orphans: u64 = 0;
    while let Some(row) = result.next()? {
        let id_text: String = row.get(0)?;
        let task_text: String = row.get(1)?;
        // `let … else`: if the task is not in the map, run the `else` block,
        // which must leave the loop iteration (`continue`).
        let Some(&task_row) = task_rows.get(&task_text) else {
            orphans += 1;
            continue;
        };
        let after_work: i64 = row.get(2)?;
        let duration: i64 = row.get(3)?;
        let reported_on: Option<String> = row.get(5)?;

        let mut warn = |column: &'static str, value: String| {
            notes.push(ReadNote::ValueNotLoaded {
                table: "blockers",
                id: id_text.clone(),
                column,
                value,
            });
        };
        if outline.is_summary(task_row) {
            warn("task_id", task_text.clone());
        }
        if after_work < 0 {
            warn("after_work_minutes", after_work.to_string());
        }
        if duration < 0 {
            warn("duration_minutes", duration.to_string());
        }

        tasks[task_row].blockers.push(Blocker {
            id: BlockerId::from_uuid(parse_uuid(&id_text, "blocker id")?),
            after_work: Duration::from_minutes(after_work),
            duration: Duration::from_minutes(duration),
            reason: row.get(4)?,
            reported_on: reported_on.as_deref().map(parse_date).transpose()?,
        });
    }
    if orphans > 0 {
        notes.push(ReadNote::RowsNotLoaded {
            table: "blockers",
            count: orphans,
        });
    }
    Ok(())
}

/// Reads the `risks` table (format 6) and puts each risk into its task's
/// `risks` list, in `position` order.
///
/// # Parameters
/// As for [`read_blockers`]. Rows whose task is missing become one note and
/// stay in the file.
///
/// # Errors
/// [`GtraError::InvalidData`] for an unreadable id, priority or response;
/// [`GtraError::Sqlite`] if the query fails.
fn read_risks(
    conn: &Connection,
    tasks: &mut [Task],
    task_rows: &BTreeMap<String, usize>,
    notes: &mut Vec<ReadNote>,
) -> Result<(), GtraError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, description, priority, response, mitigation,
                no_consequence, consequence
         FROM risks ORDER BY task_id, position, id",
    )?;
    let mut result = stmt.query([])?;
    let mut orphans: u64 = 0;
    while let Some(row) = result.next()? {
        let id_text: String = row.get(0)?;
        let task_text: String = row.get(1)?;
        let Some(&task_row) = task_rows.get(&task_text) else {
            orphans += 1;
            continue;
        };
        let priority: String = row.get(3)?;
        let response: String = row.get(4)?;
        tasks[task_row].risks.push(Risk {
            id: RiskId::from_uuid(parse_uuid(&id_text, "risk id")?),
            description: row.get(2)?,
            priority: match priority.as_str() {
                "low" => RiskPriority::Low,
                "medium" => RiskPriority::Medium,
                "high" => RiskPriority::High,
                other => {
                    return Err(GtraError::InvalidData(format!(
                        "unknown risk priority '{other}'"
                    )));
                }
            },
            response: match response.as_str() {
                "approved" => RiskResponse::Approved,
                "mitigate" => RiskResponse::Mitigate,
                other => {
                    return Err(GtraError::InvalidData(format!(
                        "unknown risk response '{other}'"
                    )));
                }
            },
            mitigation: row.get(5)?,
            no_consequence: row.get(6)?,
            consequence: row.get(7)?,
        });
    }
    if orphans > 0 {
        notes.push(ReadNote::RowsNotLoaded {
            table: "risks",
            count: orphans,
        });
    }
    Ok(())
}

/// Turns a stored `tasks.mode` (`'auto'` / `'manual'`) into a [`TaskMode`].
///
/// # Errors
/// [`GtraError::InvalidData`] for any other text.
fn parse_task_mode(code: &str) -> Result<TaskMode, GtraError> {
    match code {
        "auto" => Ok(TaskMode::Auto),
        "manual" => Ok(TaskMode::Manual),
        other => Err(GtraError::InvalidData(format!(
            "unknown task mode '{other}'"
        ))),
    }
}

/// Joins the two columns `constraint_type` and `constraint_date` into one
/// [`Constraint`] (the reverse of `write::constraint_columns`).
///
/// # Parameters
/// - `code`: `'asap'`, `'alap'`, `'snet'`, `'snlt'`, `'fnet'`, `'fnlt'`,
///   `'mso'` or `'mfo'`.
/// - `date`: the stored date-time, required by every code except asap/alap
///   (the table's CHECK rule guarantees that pairing).
///
/// # Errors
/// [`GtraError::InvalidData`] for an unknown code, a missing date, or a date
/// that cannot be read.
///
/// # Beginner note
/// `needs_date` below is a **closure**: a small function written inline and
/// stored in a variable. It can use `code` from the surrounding function,
/// which a normal `fn` could not.
fn parse_constraint(code: &str, date: Option<&str>) -> Result<Constraint, GtraError> {
    // Reads the date for the codes that need one.
    let needs_date = |text: Option<&str>| -> Result<NaiveDateTime, GtraError> {
        match text {
            Some(text) => parse_date_time(text),
            None => Err(GtraError::InvalidData(format!(
                "constraint '{code}' has no date"
            ))),
        }
    };
    Ok(match code {
        "asap" => Constraint::AsSoonAsPossible,
        "alap" => Constraint::AsLateAsPossible,
        "snet" => Constraint::StartNoEarlierThan {
            date: needs_date(date)?,
        },
        "snlt" => Constraint::StartNoLaterThan {
            date: needs_date(date)?,
        },
        "fnet" => Constraint::FinishNoEarlierThan {
            date: needs_date(date)?,
        },
        "fnlt" => Constraint::FinishNoLaterThan {
            date: needs_date(date)?,
        },
        "mso" => Constraint::MustStartOn {
            date: needs_date(date)?,
        },
        "mfo" => Constraint::MustFinishOn {
            date: needs_date(date)?,
        },
        other => {
            return Err(GtraError::InvalidData(format!(
                "unknown constraint type '{other}'"
            )));
        }
    })
}

/// Reads an optional stored date-time: `None` stays `None`, text such as
/// `"2027-03-01T08:00:00"` becomes `Some(date-time)`.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not a valid date-time.
fn optional_date_time(text: Option<&str>) -> Result<Option<NaiveDateTime>, GtraError> {
    text.map(parse_date_time).transpose()
}

/// Reads the dependencies, sorted by (predecessor row, successor row).
///
/// # Parameters
/// - `tasks`: the tasks in row order (to look up the task ids).
/// - `row_of`: task id text → row index, from [`read_tasks`].
///
/// # Errors
/// [`GtraError::InvalidData`] for an unreadable id or an unknown type code;
/// [`GtraError::Sqlite`] if the query fails.
fn read_dependencies(
    conn: &Connection,
    tasks: &[Task],
    row_of: &BTreeMap<String, usize>,
    notes: &mut Vec<ReadNote>,
) -> Result<Vec<Dependency>, GtraError> {
    let mut stmt = conn.prepare(
        "SELECT id, predecessor_id, successor_id, type, lag_minutes, lag_percent
         FROM dependencies",
    )?;
    type Raw = (String, String, String, String, i64, Option<f64>);
    let raw: Vec<Raw> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    // (predecessor row, successor row, dependency) so we can sort.
    let mut found: Vec<(usize, usize, Dependency)> = Vec::with_capacity(raw.len());
    for (id, predecessor, successor, kind, lag_minutes, lag_percent) in raw {
        // The foreign keys guarantee both tasks exist; `ok_or_else` turns a
        // missing one (a damaged file) into a clear error anyway.
        let missing =
            || GtraError::InvalidData(format!("dependency {id} points to a missing task"));
        let pred_row = *row_of.get(&predecessor).ok_or_else(missing)?;
        let succ_row = *row_of.get(&successor).ok_or_else(missing)?;
        let dependency_id = DependencyId::from_uuid(parse_uuid(&id, "dependency id")?);

        // A percentage lag is kept as a whole percentage (0.5 in the file =
        // 50 in the model); the engine works out the minutes when it
        // schedules. A fraction that is not a whole percentage (e.g. 0.333
        // from another tool) is rounded, with a note.
        let lag_percent = match lag_percent {
            None => None,
            Some(fraction) => {
                let percent = fraction_to_percent(fraction);
                if (fraction * 100.0 - f64::from(percent)).abs() > 1e-6 {
                    notes.push(ReadNote::ValueNotLoaded {
                        table: "dependencies",
                        id: id.clone(),
                        column: "lag_percent",
                        value: fraction.to_string(),
                    });
                }
                Some(percent)
            }
        };

        found.push((
            pred_row,
            succ_row,
            Dependency {
                id: dependency_id,
                predecessor: tasks[pred_row].id,
                successor: tasks[succ_row].id,
                kind: parse_dependency_type(&kind)?,
                lag: Duration::from_minutes(lag_minutes),
                lag_percent,
            },
        ));
    }
    // Sort by the two row numbers; the id breaks any remaining tie.
    found.sort_by(|a, b| (a.0, a.1, a.2.id).cmp(&(b.0, b.1, b.2.id)));
    Ok(found
        .into_iter()
        .map(|(_, _, dependency)| dependency)
        .collect())
}

/// Turns the fraction stored in `dependencies.lag_percent` into the model's
/// whole percentage: 0.5 → 50, -0.25 → -25 (the reverse of
/// `write::percent_to_fraction`).
///
/// # Returns
/// The nearest whole percentage. Values beyond what an `i32` can hold (never
/// written by Ganttora) are capped at its limits.
///
/// # Beginner note
/// `as i32` converts a decimal number to a whole number. Rust's `as` from a
/// float "saturates": a too-large value becomes `i32::MAX` instead of
/// wrapping around to a strange number.
fn fraction_to_percent(fraction: f64) -> i32 {
    (fraction * 100.0).round() as i32
}

/// Turns a stored type code (`'FS'`, `'SS'`, `'FF'`, `'SF'`) into a
/// [`DependencyType`].
///
/// # Errors
/// [`GtraError::InvalidData`] for any other text.
fn parse_dependency_type(code: &str) -> Result<DependencyType, GtraError> {
    match code {
        "FS" => Ok(DependencyType::FinishToStart),
        "SS" => Ok(DependencyType::StartToStart),
        "FF" => Ok(DependencyType::FinishToFinish),
        "SF" => Ok(DependencyType::StartToFinish),
        other => Err(GtraError::InvalidData(format!(
            "unknown dependency type '{other}'"
        ))),
    }
}

/// Reads a UUID stored as text.
///
/// # Parameters
/// - `text`: e.g. `"67e55044-10b1-426f-9247-bb680e5fe0c8"`.
/// - `what`: what the id belongs to, for the error message.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not a UUID.
fn parse_uuid(text: &str, what: &str) -> Result<Uuid, GtraError> {
    Uuid::parse_str(text)
        .map_err(|_| GtraError::InvalidData(format!("{what} '{text}' is not a valid UUID")))
}

/// Reads a stored date-time such as `"2027-03-01T08:00:00"`.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not in that format.
fn parse_date_time(text: &str) -> Result<NaiveDateTime, GtraError> {
    NaiveDateTime::parse_from_str(text, DATE_TIME_FORMAT)
        .map_err(|_| GtraError::InvalidData(format!("'{text}' is not a valid date and time")))
}

/// The stand-in project start for a file without one: 00:00 on the day the
/// file was created (from a timestamp such as `"2027-02-15T10:42:07Z"`).
///
/// # Errors
/// [`GtraError::InvalidData`] if `created` is missing or unreadable (then
/// there is no sensible start to use).
fn start_from_created(created: Option<&str>) -> Result<NaiveDateTime, GtraError> {
    created
        // The first 10 characters of a timestamp are the date.
        .and_then(|text| text.get(..10))
        .and_then(|date| NaiveDate::parse_from_str(date, crate::DATE_FORMAT).ok())
        .map(|date| date.and_time(chrono::NaiveTime::MIN))
        .ok_or_else(|| {
            GtraError::InvalidData(
                "the project has no start date and the file has no creation date".to_string(),
            )
        })
}
