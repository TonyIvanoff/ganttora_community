//! Writing a project into a `.gtra` database: `model::Project` → SQLite rows.
//!
//! # What this file is for
//! The rest of Ganttora works with a [`model::Project`] in memory. This file
//! turns that value into rows of the tables defined in
//! `migrations/0001_init.sql`, `migrations/0002_m1_fields.sql` and
//! `migrations/0003_blockers.sql`. Its
//! partner, `read.rs`, does the opposite.
//!
//! # Where it fits
//! [`crate::save`] (in `lib.rs`) and [`crate::ProjectSession`] (in
//! `session.rs`) prepare a local working copy of the file, call
//! [`GtraFile::write_project`] on it, and then copy the result over the
//! user's file atomically.
//!
//! # How each part of the model is stored
//! ```text
//!   model::Project                       .gtra tables
//!   ─────────────────────────────        ───────────────────────────────────────────
//!   id, name                        ──►  project.id, project.name
//!   start (NaiveDateTime)           ──►  project.start_date  '2027-03-01T00:00:00'
//!   schedule_from (Start/Finish)    ──►  project.schedule_from 'start' | 'finish'
//!   finish (Option)                 ──►  project.finish_date (NULL = none)
//!   calendar_id                     ──►  project.calendar_id
//!   minutes_per_day                 ──►  project_settings ('minutes_per_day', '480')
//!
//!   tasks: Vec<Task> (row order)    ──►  tasks: one row each
//!     id, name                      ──►    id, name
//!     duration (minutes)            ──►    duration_minutes
//!     position in the Vec        ┐  ──►    parent_id + position (see "The outline")
//!     outline_level              ┘
//!     mode (Auto/Manual)            ──►    mode 'auto' | 'manual'
//!     manual_start (Option)         ──►    manual_start
//!     milestone (bool)              ──►    is_milestone 0 | 1
//!     constraint (enum)             ──►    constraint_type 'asap' … 'mfo'
//!                                          + constraint_date (NULL for asap/alap)
//!     deadline (Option)             ──►    deadline
//!     calendar_id (Option)          ──►    calendar_id (NULL = project calendar)
//!     notes, hyperlink              ──►    notes, hyperlink
//!     blockers: Vec<Blocker>        ──►  blockers: one row each (format 3)
//!       id                          ──►    id
//!       (the task it belongs to)    ──►    task_id
//!       position in the Vec         ──►    position 1, 2, 3 …
//!       after_work, duration        ──►    after_work_minutes, duration_minutes
//!       reason                      ──►    reason
//!       reported_on (Option)        ──►    reported_on '2027-03-04' (NULL = none)
//!
//!   dependencies: Vec<Dependency>   ──►  dependencies: one row each
//!     predecessor, successor        ──►    predecessor_id, successor_id
//!     kind                          ──►    type 'FS' | 'SS' | 'FF' | 'SF'
//!     lag (minutes)                 ──►    lag_minutes          ┐ one or the other,
//!     lag_percent (Option<i32>)     ──►    lag_percent (50 % → 0.5) ┘ never both
//!
//!   calendars: Vec<Calendar>        ──►  calendars (base_calendar_id = NULL)
//!     week (7 weekdays)             ──►    calendar_week_days: 7 rows, is_working
//!                                          calendar_working_times: one row per block
//!     holidays (dates)              ──►    calendar_exceptions, kind 'holiday',
//!                                          non-working, one day each (new rows)
//!     exceptions (Vec, in order)    ──►    calendar_exceptions, kind 'exception',
//!                                          position 1, 2, 3 … ; its working times in
//!                                          calendar_exception_times
//!
//!   (file information)              ──►  meta: project_id, modified, author, app_version
//! ```
//!
//! # The outline: from `outline_level` to a tree
//! In the model a task only has a place in the list and an `outline_level`
//! (1 = top level, 2 = indented once, …). [`model::Outline::build`] works out
//! from that who is whose summary task. The file stores the result as a
//! **tree**: every task points to its summary task (`parent_id`) and has a
//! `position` among its siblings (1, 2, 3, …):
//!
//! ```text
//!   row  outline_level  name                     parent_id   position
//!   1    1              Design the shed          NULL        1
//!   2    1              Build                    NULL        2
//!   3    2                Floor                  <Build>     1
//!   4    2                Walls                  <Build>     2
//!   5    1              Shed finished            NULL        3
//! ```
//!
//! `read.rs` walks the tree in the same order and gives each task its depth
//! as `outline_level`, so the round trip is exact. A level that jumps by more
//! than one (level 3 right after level 1) has no place in a tree and is
//! refused with [`GtraError::InvalidProject`] ([`model::first_invalid_row`]).
//!
//! # Updating rows instead of rebuilding the file
//! The file may hold data the model does not know yet: resources, the
//! project's notes, the change journal, … A save must not throw that away.
//! So `write_project` does not empty the tables; it **updates** them:
//! - rows whose id is in the project are inserted or updated ("upsert"),
//!   changing only the columns listed in the diagram above;
//! - rows whose id is no longer in the project are deleted;
//! - every other column keeps its value (or its default for a new row).
//!
//! One exception to "rows not in the project are deleted": a `blockers` row
//! whose `task_id` points to a task that is not in the file (possible only
//! if another tool wrote it with foreign keys switched off) is left alone.
//! `read.rs` did not load it and reported it, and a save must not quietly
//! delete what it never showed the user.
//!
//! Columns that are therefore **not round-tripped yet** (kept when present,
//! default for new rows, reported by `read.rs` as notes): `project.status_date`
//! and `project.notes`; the task columns `duration_unit`, `task_type`,
//! `effort_driven` and `manual_finish`; `dependencies.lag_unit` (only
//! switched between `'%'` and `'d'`); and the tables `resources`,
//! `assignments`, `journal` (written by `journal.rs`) and `versions`.
//!
//! Everything happens inside **one transaction**, so the database ends up
//! either fully updated or unchanged.
//!

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc, Weekday};
use model::{
    Blocker, Calendar, CalendarException, Constraint, DependencyType, Outline, Project,
    RiskPriority, RiskResponse, ScheduleFrom, Task, TaskMode, WorkingTime,
};
use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::{APP_VERSION, DATE_FORMAT, DATE_TIME_FORMAT, GtraError, GtraFile, set_meta};

/// The seven weekdays in ISO order (Monday = 1 … Sunday = 7), the order used
/// by the `weekday` column of `calendar_week_days`.
pub(crate) const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// Information about a save that does not come from the project itself:
/// *when* it is saved and *by whom*. Written to the `meta` table.
///
/// # Why a parameter and not "now"
/// The caller decides, so the app can pass the real time and the user name
/// from Settings → User, while tests and examples can pass fixed values and
/// get exactly the same file every time.
///
/// # Example
/// ```
/// use chrono::{TimeZone, Utc};
/// let options = gtra::SaveOptions::new(
///     Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
///     "Sam Lee",
/// );
/// assert_eq!(options.timestamp(), "2027-02-15T10:42:07Z");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOptions {
    /// The moment of the save, in UTC. Becomes `meta.modified` (and
    /// `meta.created` when the file is new).
    pub saved_at: DateTime<Utc>,
    /// Display name of the person saving (Settings → User); may be empty.
    /// Becomes `meta.author`.
    pub author: String,
    /// Version of the scheduling engine that computed the dates being saved,
    /// e.g. `"0.1.0"`. Becomes `meta.engine_version`. `None` removes the
    /// entry (the version is then unknown), so a stale value from an earlier
    /// save is never left behind.
    ///
    /// # Why the caller passes it
    /// `gtra` must not depend on the `engine` crate (it never schedules,
    /// the project documentation), so it cannot look the version up itself. The Tauri shell,
    /// which uses both crates, fills it in with
    /// [`SaveOptions::with_engine_version`].
    pub engine_version: Option<String>,
}

impl SaveOptions {
    /// Creates save options with a given time and author.
    ///
    /// # Parameters
    /// - `saved_at`: the moment of the save (UTC).
    /// - `author`: the display name; anything that can become a `String`.
    ///
    /// # Returns
    /// The new `SaveOptions`.
    pub fn new(saved_at: DateTime<Utc>, author: impl Into<String>) -> Self {
        Self {
            saved_at,
            author: author.into(),
            engine_version: None,
        }
    }

    /// Returns these options with `engine_version` set.
    ///
    /// # Parameters
    /// - `version`: the engine's version, e.g. `"0.1.0"`.
    ///
    /// # Returns
    /// The changed options. This is a "builder" method: it takes `self` and
    /// gives it back, so calls can be chained.
    ///
    /// # Example
    /// ```
    /// let options = gtra::SaveOptions::now("Sam Lee").with_engine_version("0.1.0");
    /// assert_eq!(options.engine_version.as_deref(), Some("0.1.0"));
    /// ```
    pub fn with_engine_version(mut self, version: impl Into<String>) -> Self {
        self.engine_version = Some(version.into());
        self
    }

    /// Creates save options for "now", as told by the computer's clock.
    ///
    /// # Parameters
    /// - `author`: the display name.
    ///
    /// # Returns
    /// `SaveOptions` with the current time, rounded down to whole seconds
    /// (the file stores seconds only).
    ///
    /// # Example
    /// ```
    /// let options = gtra::SaveOptions::now("Sam Lee");
    /// assert_eq!(options.author, "Sam Lee");
    /// ```
    pub fn now(author: impl Into<String>) -> Self {
        Self::new(DateTime::<Utc>::from(SystemTime::now()), author)
    }

    /// The save time as the file stores it: ISO 8601 in UTC, whole seconds,
    /// e.g. `"2027-02-15T10:42:07Z"`.
    pub fn timestamp(&self) -> String {
        self.saved_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }
}

impl GtraFile {
    /// Writes `project` into this database, replacing the project data it
    /// held before, in one transaction.
    ///
    /// # What it does
    /// 1. Checks the project ([`validate`]); nothing is written if it is
    ///    invalid.
    /// 2. In one transaction: writes the calendars (with holidays and
    ///    exceptions), the `project` row and the `minutes_per_day` setting,
    ///    the tasks (as a tree, see the file header), their blockers, the
    ///    dependencies, removes rows that are no longer in the project, and updates `meta`
    ///    (`project_id`, `modified`, `author`, `app_version`,
    ///    `engine_version`).
    ///
    /// This changes only this database (normally a local working copy). Use
    /// [`crate::save`] to save to the user's file.
    ///
    /// # Parameters
    /// - `project`: the project to write.
    /// - `options`: the save time and author for `meta`.
    ///
    /// # Errors
    /// - [`GtraError::ReadOnly`] if the file was opened read-only.
    /// - [`GtraError::InvalidProject`] if the project breaks a rule (see
    ///   [`validate`]).
    /// - [`GtraError::Sqlite`] if a database statement fails. In every error
    ///   case the database is left unchanged.
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use model::{Duration, Project, Task};
    ///
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// std::fs::create_dir_all(&dir).unwrap();
    /// let mut file = gtra::create_empty(dir.join("work.gtra")).unwrap();
    ///
    /// let mut project = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// project.tasks.push(Task::new("Design the shed", Duration::from_days(2)));
    /// file.write_project(&project, &gtra::SaveOptions::now("Sam Lee")).unwrap();
    ///
    /// let (read_back, _notes) = file.read_project().unwrap();
    /// assert_eq!(read_back, project);
    /// # drop(file);
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    ///
    /// # Beginner note
    /// `&mut self` means this method needs to *change* the `GtraFile` (a
    /// transaction needs exclusive use of the connection). The caller must
    /// therefore hold it in a `let mut` variable.
    pub fn write_project(
        &mut self,
        project: &Project,
        options: &SaveOptions,
    ) -> Result<(), GtraError> {
        if self.is_read_only() {
            return Err(GtraError::ReadOnly);
        }
        validate(project)?;

        // Everything below happens inside one transaction. If any `?` returns
        // early, `tx` is dropped without `commit()` and SQLite undoes all of it.
        let tx = self.conn.transaction()?;
        // The order matters because of the foreign keys: a calendar must
        // exist before the project row or a task points to it, and a summary
        // task before its subtasks.
        write_calendars(&tx, &project.calendars)?;
        write_project_row(&tx, project)?;
        delete_stale_dependencies(&tx, project)?;
        write_tasks(&tx, &project.tasks)?;
        // After the tasks: every blocker row points to its task.
        write_blockers(&tx, &project.tasks)?;
        write_risks(&tx, &project.tasks)?;
        write_dependencies(&tx, project)?;
        delete_stale_calendars(&tx, &project.calendars)?;
        write_meta(&tx, project, options)?;
        tx.commit()?;
        Ok(())
    }
}

/// Checks the rules a project must follow before it can be written.
///
/// # What it checks
/// - `minutes_per_day` is greater than zero;
/// - ids are unique within tasks, within dependencies and within calendars
///   (two tasks with the same id would silently overwrite each other);
/// - the project calendar is one of `calendars`, and so is every task
///   calendar;
/// - no task has a negative duration;
/// - the outline levels form a valid outline (the first task is at level 1,
///   and a level is at most one deeper than the level above it);
/// - every dependency links two different tasks of this project, and no two
///   dependencies link the same pair (the file allows one link per pair,
///   like MS Project);
/// - every working-time block (weekdays and exceptions) satisfies
///   `start < end <= 1440`;
/// - every calendar exception starts on or before the day it finishes;
/// - blocker ids are unique across the whole project (they are the primary
///   key of the `blockers` table, shared by all tasks).
///
/// Not checked on purpose: negative blocker values and blockers on summary
/// tasks. The document crate never creates them; if they came from a file,
/// they are written back unchanged instead of making the save fail
/// (`read.rs` already told the user about them).
///
/// # Parameters
/// - `project`: the project to check.
///
/// # Returns
/// `Ok(())` if all rules hold.
///
/// # Errors
/// [`GtraError::InvalidProject`] with a message naming the first problem.
///
/// # Example
/// ```
/// use chrono::NaiveDate;
/// use model::{Duration, Project, Task};
/// let mut project = Project::new("Shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// project.tasks.push(Task::new("Oops", Duration::from_minutes(-5)));
/// assert!(gtra::write::validate(&project).is_err());
/// ```
pub fn validate(project: &Project) -> Result<(), GtraError> {
    // A small helper closure that builds the error, to keep the checks short.
    let invalid = |message: String| Err(GtraError::InvalidProject(message));

    if project.minutes_per_day <= 0 {
        return invalid(format!(
            "minutes per day must be positive, not {}",
            project.minutes_per_day
        ));
    }

    // `BTreeSet::insert` returns `false` if the value was already there.
    let mut calendar_ids = BTreeSet::new();
    for calendar in &project.calendars {
        if !calendar_ids.insert(calendar.id) {
            return invalid(format!("calendar id {} is used twice", calendar.id));
        }
        for weekday in WEEKDAYS {
            if let Some(block) = first_bad_block(calendar.week.day(weekday)) {
                return invalid(format!(
                    "calendar '{}' has an invalid working time on {weekday}: {}-{} minutes",
                    calendar.name, block.start_minute, block.end_minute
                ));
            }
        }
        for exception in &calendar.exceptions {
            if exception.start > exception.finish {
                return invalid(format!(
                    "calendar '{}': exception '{}' finishes before it starts",
                    calendar.name, exception.name
                ));
            }
            if let Some(block) = first_bad_block(&exception.working_times) {
                return invalid(format!(
                    "calendar '{}': exception '{}' has an invalid working time: {}-{} minutes",
                    calendar.name, exception.name, block.start_minute, block.end_minute
                ));
            }
        }
    }
    if !calendar_ids.contains(&project.calendar_id) {
        return invalid(format!(
            "the project calendar {} is not in the project's calendars",
            project.calendar_id
        ));
    }

    let mut task_ids = BTreeSet::new();
    for task in &project.tasks {
        if !task_ids.insert(task.id) {
            return invalid(format!("task id {} is used twice", task.id));
        }
        if task.duration.minutes() < 0 {
            return invalid(format!("task '{}' has a negative duration", task.name));
        }
        // A "let chain": `if let … && …` runs the block only when the task
        // has a calendar AND that calendar is missing from the project.
        if let Some(calendar) = task.calendar_id
            && !calendar_ids.contains(&calendar)
        {
            return invalid(format!(
                "task '{}' uses calendar {calendar}, which is not in the project's calendars",
                task.name
            ));
        }
    }
    // Blocker ids: one set for all tasks, because the table is shared.
    let mut blocker_ids = BTreeSet::new();
    for task in &project.tasks {
        for blocker in &task.blockers {
            if !blocker_ids.insert(blocker.id) {
                return invalid(format!("blocker id {} is used twice", blocker.id));
            }
        }
    }
    // The same for risks.
    let mut risk_ids = BTreeSet::new();
    for risk in project.tasks.iter().flat_map(|t| &t.risks) {
        if !risk_ids.insert(risk.id) {
            return invalid(format!("risk id {} is used twice", risk.id));
        }
    }
    // The model's own outline check: the first row whose level does not fit.
    if let Some(row) = model::first_invalid_row(&project.tasks) {
        let task = &project.tasks[row];
        return invalid(format!(
            "task '{}' (row {}) has outline level {}, which does not fit the row above it",
            task.name,
            row + 1,
            task.outline_level
        ));
    }

    let mut dependency_ids = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    for dependency in &project.dependencies {
        if !dependency_ids.insert(dependency.id) {
            return invalid(format!("dependency id {} is used twice", dependency.id));
        }
        if !task_ids.contains(&dependency.predecessor) || !task_ids.contains(&dependency.successor)
        {
            return invalid(format!(
                "dependency {} points to a task that is not in the project",
                dependency.id
            ));
        }
        if dependency.predecessor == dependency.successor {
            return invalid(format!(
                "dependency {} links a task to itself",
                dependency.id
            ));
        }
        if !pairs.insert((dependency.predecessor, dependency.successor)) {
            return invalid(format!(
                "two dependencies link the same tasks (predecessor {}, successor {})",
                dependency.predecessor, dependency.successor
            ));
        }
    }
    Ok(())
}

/// Returns the first working-time block that the file cannot store: one
/// that does not start before it ends, or ends after midnight (1440).
///
/// # Returns
/// `Some(block)` for the first bad block, `None` if all are fine.
fn first_bad_block(blocks: &[WorkingTime]) -> Option<&WorkingTime> {
    blocks
        .iter()
        .find(|block| block.start_minute >= block.end_minute || block.end_minute > 1440)
}

/// Writes every calendar: its row, its work week, its holidays and its
/// exceptions.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails; [`GtraError::InvalidData`] if
/// a stored date cannot be read.
fn write_calendars(conn: &Connection, calendars: &[Calendar]) -> Result<(), GtraError> {
    for calendar in calendars {
        let id = calendar.id.to_string();
        // Model calendars are always complete (all seven weekdays), so they
        // never inherit from a base calendar: base_calendar_id becomes NULL.
        conn.execute(
            "INSERT INTO calendars (id, name, base_calendar_id) VALUES (?1, ?2, NULL)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, base_calendar_id = NULL",
            params![id, calendar.name],
        )?;

        // The work week is small (7 rows plus a few blocks), so we simply
        // replace it. Deleting the weekday rows also deletes their working
        // times (ON DELETE CASCADE in the schema).
        conn.execute(
            "DELETE FROM calendar_week_days WHERE calendar_id = ?1",
            [&id],
        )?;
        for weekday in WEEKDAYS {
            let blocks = calendar.week.day(weekday);
            let iso_number = weekday.number_from_monday();
            conn.execute(
                "INSERT INTO calendar_week_days (calendar_id, weekday, is_working)
                 VALUES (?1, ?2, ?3)",
                params![id, iso_number, !blocks.is_empty()],
            )?;
            for block in blocks {
                conn.execute(
                    "INSERT INTO calendar_working_times
                         (calendar_id, weekday, start_minute, end_minute)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, iso_number, block.start_minute, block.end_minute],
                )?;
            }
        }

        write_holidays(conn, &id, &calendar.holidays)?;
        write_exceptions(conn, &id, &calendar.exceptions)?;
    }
    Ok(())
}

/// Makes the holiday rows (`kind = 'holiday'`) of one calendar match
/// `holidays`.
///
/// # How it works
/// An existing holiday row whose every day is still a holiday is **kept**,
/// with its name and date range (so "Christmas, 24-26 Dec" stays one named
/// row). All others are deleted. Then every holiday not covered yet gets a
/// new one-day row with an empty name. Exception rows (`kind =
/// 'exception'`) are handled by [`write_exceptions`].
///
/// # Parameters
/// - `conn`: the connection (inside the save transaction).
/// - `calendar_id`: the calendar's id as text.
/// - `holidays`: the holiday dates from the model.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails; [`GtraError::InvalidData`] if
/// a stored date cannot be read.
fn write_holidays(
    conn: &Connection,
    calendar_id: &str,
    holidays: &BTreeSet<NaiveDate>,
) -> Result<(), GtraError> {
    let mut covered: BTreeSet<NaiveDate> = BTreeSet::new();
    let existing: Vec<(String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, start_date, end_date FROM calendar_exceptions
             WHERE calendar_id = ?1 AND kind = 'holiday'",
        )?;
        // `query_map` runs the query and turns each row into a value;
        // `collect` gathers them, stopping at the first error.
        stmt.query_map([calendar_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?
    };
    for (id, start, end) in existing {
        let days = date_range(&start, &end)?;
        if days.iter().all(|day| holidays.contains(day)) {
            covered.extend(days);
        } else {
            conn.execute("DELETE FROM calendar_exceptions WHERE id = ?1", [&id])?;
        }
    }
    for day in holidays.difference(&covered) {
        let text = day.format(DATE_FORMAT).to_string();
        conn.execute(
            "INSERT INTO calendar_exceptions
                 (id, calendar_id, name, start_date, end_date, is_working, kind, position)
             VALUES (?1, ?2, '', ?3, ?3, 0, 'holiday', 0)",
            params![Uuid::new_v4().to_string(), calendar_id, text],
        )?;
    }
    Ok(())
}

/// Replaces the exception rows (`kind = 'exception'`) of one calendar with
/// `exceptions`, keeping their order in `position` (1, 2, 3, …).
///
/// # Why replace instead of update
/// A model exception has no id of its own, so there is nothing to match an
/// existing row against. Deleting and inserting again is simple and exact;
/// deleting a row also deletes its working times (ON DELETE CASCADE). The
/// rows get new UUIDs on every save, which is fine because nothing else in
/// the file points to them.
///
/// ```text
///   model exception                         calendar_exceptions row
///   ─────────────────────────────────────   ─────────────────────────────────────────
///   [0] "Working Saturday", 6-6 Mar,        kind 'exception', position 1, is_working 1
///       08:00-12:00                         + calendar_exception_times (480, 720)
///   [1] "Summer shutdown", 2-13 Aug, none   kind 'exception', position 2, is_working 0
/// ```
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_exceptions(
    conn: &Connection,
    calendar_id: &str,
    exceptions: &[CalendarException],
) -> Result<(), GtraError> {
    conn.execute(
        "DELETE FROM calendar_exceptions WHERE calendar_id = ?1 AND kind = 'exception'",
        [calendar_id],
    )?;
    // `enumerate` gives each item with its index 0, 1, 2…; the file counts
    // positions from 1, hence `index + 1`.
    for (index, exception) in exceptions.iter().enumerate() {
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO calendar_exceptions
                 (id, calendar_id, name, start_date, end_date, is_working, kind, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'exception', ?7)",
            params![
                id,
                calendar_id,
                exception.name,
                exception.start.format(DATE_FORMAT).to_string(),
                exception.finish.format(DATE_FORMAT).to_string(),
                // No working times = a non-working range.
                !exception.working_times.is_empty(),
                index + 1,
            ],
        )?;
        for block in &exception.working_times {
            conn.execute(
                "INSERT INTO calendar_exception_times (exception_id, start_minute, end_minute)
                 VALUES (?1, ?2, ?3)",
                params![id, block.start_minute, block.end_minute],
            )?;
        }
    }
    Ok(())
}

/// Lists every date from `start` to `end` (both included), given as stored
/// date text such as `"2027-12-24"`.
///
/// # Errors
/// [`GtraError::InvalidData`] if a date cannot be read.
pub(crate) fn date_range(start: &str, end: &str) -> Result<Vec<NaiveDate>, GtraError> {
    let first = parse_date(start)?;
    let last = parse_date(end)?;
    // `iter_days` counts forward one day at a time; `take_while` stops after
    // the last day.
    Ok(first.iter_days().take_while(|day| *day <= last).collect())
}

/// Reads a stored date such as `"2027-03-01"`.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not a valid date.
pub(crate) fn parse_date(text: &str) -> Result<NaiveDate, GtraError> {
    NaiveDate::parse_from_str(text, DATE_FORMAT)
        .map_err(|_| GtraError::InvalidData(format!("'{text}' is not a valid date")))
}

/// Formats an optional schedule date-time for the file: `None` stays `None`
/// (SQL `NULL`), a value becomes text such as `"2027-03-01T08:00:00"`.
fn date_time_text(value: Option<NaiveDateTime>) -> Option<String> {
    value.map(|moment| moment.format(DATE_TIME_FORMAT).to_string())
}

/// Writes the single `project` row and the `minutes_per_day` setting.
///
/// Any other `project` row (for example the one [`crate::create_empty`]
/// made with a random id) is removed: a file holds exactly one project.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_project_row(conn: &Connection, project: &Project) -> Result<(), GtraError> {
    let id = project.id.to_string();
    conn.execute("DELETE FROM project WHERE id <> ?1", [&id])?;
    conn.execute(
        "INSERT INTO project (id, name, start_date, calendar_id, schedule_from, finish_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             start_date = excluded.start_date,
             calendar_id = excluded.calendar_id,
             schedule_from = excluded.schedule_from,
             finish_date = excluded.finish_date",
        params![
            id,
            project.name,
            project.start.format(DATE_TIME_FORMAT).to_string(),
            project.calendar_id.to_string(),
            schedule_from_code(project.schedule_from),
            date_time_text(project.finish),
        ],
    )?;
    // project_settings values are JSON text; a JSON number is just its digits.
    conn.execute(
        "INSERT INTO project_settings (key, value) VALUES ('minutes_per_day', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [project.minutes_per_day.to_string()],
    )?;
    Ok(())
}

/// The text stored in `project.schedule_from`.
///
/// # Example
/// `ScheduleFrom::Finish` → `"finish"`.
pub(crate) fn schedule_from_code(value: ScheduleFrom) -> &'static str {
    match value {
        ScheduleFrom::Start => "start",
        ScheduleFrom::Finish => "finish",
    }
}

/// The text stored in `tasks.mode`.
///
/// # Example
/// `TaskMode::Manual` → `"manual"`.
pub(crate) fn task_mode_code(mode: TaskMode) -> &'static str {
    match mode {
        TaskMode::Auto => "auto",
        TaskMode::Manual => "manual",
    }
}

/// Splits a constraint into the two columns `constraint_type` and
/// `constraint_date`.
///
/// ```text
///   Constraint                               constraint_type   constraint_date
///   AsSoonAsPossible                         'asap'            NULL
///   AsLateAsPossible                         'alap'            NULL
///   StartNoEarlierThan { 2027-03-08 08:00 }  'snet'            '2027-03-08T08:00:00'
///   StartNoLaterThan   { … }                 'snlt'            …
///   FinishNoEarlierThan { … }                'fnet'            …
///   FinishNoLaterThan  { … }                 'fnlt'            …
///   MustStartOn        { … }                 'mso'             …
///   MustFinishOn       { … }                 'mfo'             …
/// ```
///
/// # Returns
/// The code and the date (`None` for asap / alap). The table's CHECK rule
/// insists on exactly this pairing.
pub(crate) fn constraint_columns(constraint: Constraint) -> (&'static str, Option<NaiveDateTime>) {
    let code = match constraint {
        Constraint::AsSoonAsPossible => "asap",
        Constraint::AsLateAsPossible => "alap",
        Constraint::StartNoEarlierThan { .. } => "snet",
        Constraint::StartNoLaterThan { .. } => "snlt",
        Constraint::FinishNoEarlierThan { .. } => "fnet",
        Constraint::FinishNoLaterThan { .. } => "fnlt",
        Constraint::MustStartOn { .. } => "mso",
        Constraint::MustFinishOn { .. } => "mfo",
    };
    (code, constraint.date())
}

/// Inserts or updates every task, then deletes the tasks that are no longer
/// in the project.
///
/// # Why this order
/// Tasks are written top to bottom, so a summary task always exists before
/// its subtasks point to it. Old tasks are deleted only afterwards: deleting
/// a summary task also deletes its subtasks (ON DELETE CASCADE), so a subtask
/// that moved to another parent must be updated first.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_tasks(conn: &Connection, tasks: &[Task]) -> Result<(), GtraError> {
    // The model's outline: parent row and position for every row. `validate`
    // has already refused level jumps, so the levels are used as given.
    let outline = Outline::build(tasks);
    for (row, task) in tasks.iter().enumerate() {
        // `map` turns `Some(row)` into `Some(id text)`; `None` (top level)
        // stays `None`, which rusqlite writes as SQL NULL.
        let parent = outline.parent(row).map(|p| tasks[p].id.to_string());
        // `Outline` counts positions from 0, the file from 1. `as i64`
        // converts the number type (positions are small).
        let position = outline.position(row) as i64 + 1;
        let (constraint_type, constraint_date) = constraint_columns(task.constraint);
        conn.execute(
            "INSERT INTO tasks (id, parent_id, position, name, duration_minutes, is_milestone,
                                mode, manual_start, constraint_type, constraint_date, deadline,
                                calendar_id, notes, hyperlink, percent_complete, is_project,
                                planned_finish, actual_finish)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18)
             ON CONFLICT(id) DO UPDATE SET
                 parent_id = excluded.parent_id,
                 position = excluded.position,
                 name = excluded.name,
                 duration_minutes = excluded.duration_minutes,
                 is_milestone = excluded.is_milestone,
                 mode = excluded.mode,
                 manual_start = excluded.manual_start,
                 constraint_type = excluded.constraint_type,
                 constraint_date = excluded.constraint_date,
                 deadline = excluded.deadline,
                 calendar_id = excluded.calendar_id,
                 notes = excluded.notes,
                 hyperlink = excluded.hyperlink,
                 percent_complete = excluded.percent_complete,
                 is_project = excluded.is_project,
                 planned_finish = excluded.planned_finish,
                 actual_finish = excluded.actual_finish",
            params![
                task.id.to_string(),
                parent,
                position,
                task.name,
                task.duration.minutes(),
                // The user's own "milestone" flag. A zero-duration task is
                // a milestone anyway (`Task::is_milestone`), so the flag is
                // stored exactly as set, not derived from the duration.
                task.milestone,
                task_mode_code(task.mode),
                date_time_text(task.manual_start),
                constraint_type,
                date_time_text(constraint_date),
                date_time_text(task.deadline),
                task.calendar_id.map(|id| id.to_string()),
                task.notes,
                task.hyperlink,
                task.percent_complete,
                task.is_project,
                date_time_text(task.planned_finish),
                date_time_text(task.actual_finish),
            ],
        )?;
    }

    let keep: BTreeSet<String> = tasks.iter().map(|task| task.id.to_string()).collect();
    delete_rows_not_in(conn, "tasks", &keep)
}

/// Inserts or updates every blocker of every task, then deletes the blocker
/// rows that are no longer in the project.
///
/// # What it does
/// - Each blocker becomes one row of `blockers` with its task's id and its
///   place in `Task::blockers` as `position` (1, 2, 3, …), so `read.rs` can
///   rebuild exactly the same list.
/// - An existing row with the same id is updated ("upsert"), including its
///   `task_id`, so a blocker that moved to another task is simply moved.
/// - Rows whose id is no longer in the project are deleted, but only if
///   their task is in the file. Rows of a missing task were never loaded,
///   so they are kept (see "Updating rows" in the file header). Rows of
///   deleted tasks are already gone: `ON DELETE CASCADE` removed them when
///   [`write_tasks`] deleted the task.
///
/// # Parameters
/// - `conn`: the open transaction.
/// - `tasks`: the project's tasks (already written by [`write_tasks`]).
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_blockers(conn: &Connection, tasks: &[Task]) -> Result<(), GtraError> {
    let mut keep: BTreeSet<String> = BTreeSet::new();
    for task in tasks {
        // `enumerate` gives (index, item) pairs: index 0, 1, 2 … → position 1, 2, 3 …
        for (index, blocker) in task.blockers.iter().enumerate() {
            write_blocker(conn, task, index as i64 + 1, blocker)?;
            keep.insert(blocker.id.to_string());
        }
    }

    // Ids of the blocker rows whose task is in the file. `EXISTS (…)` is
    // true when the inner query finds at least one row.
    let existing: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT b.id FROM blockers b
             WHERE EXISTS (SELECT 1 FROM tasks t WHERE t.id = b.task_id)",
        )?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    for id in existing {
        if !keep.contains(&id) {
            conn.execute("DELETE FROM blockers WHERE id = ?1", [&id])?;
        }
    }
    Ok(())
}

/// Inserts or updates the row of one blocker.
///
/// # Parameters
/// - `conn`: the open transaction.
/// - `task`: the task the blocker belongs to.
/// - `position`: its place in `task.blockers`, counted from 1.
/// - `blocker`: the blocker itself.
///
/// # Errors
/// [`GtraError::Sqlite`] if the statement fails.
fn write_blocker(
    conn: &Connection,
    task: &Task,
    position: i64,
    blocker: &Blocker,
) -> Result<(), GtraError> {
    conn.execute(
        "INSERT INTO blockers
             (id, task_id, position, after_work_minutes, duration_minutes, reason, reported_on)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             task_id = excluded.task_id,
             position = excluded.position,
             after_work_minutes = excluded.after_work_minutes,
             duration_minutes = excluded.duration_minutes,
             reason = excluded.reason,
             reported_on = excluded.reported_on",
        params![
            blocker.id.to_string(),
            task.id.to_string(),
            position,
            blocker.after_work.minutes(),
            blocker.duration.minutes(),
            blocker.reason,
            // A date such as '2027-03-04', or NULL when the day is not known.
            blocker
                .reported_on
                .map(|day| day.format(DATE_FORMAT).to_string()),
        ],
    )?;
    Ok(())
}

/// Inserts or updates every risk of every task, then deletes the risk rows
/// that are no longer in the project (rows of unknown tasks are kept, as in
/// [`write_blockers`]).
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_risks(conn: &Connection, tasks: &[Task]) -> Result<(), GtraError> {
    let mut keep: BTreeSet<String> = BTreeSet::new();
    for task in tasks {
        for (index, risk) in task.risks.iter().enumerate() {
            conn.execute(
                "INSERT INTO risks
                     (id, task_id, position, description, priority, response,
                      mitigation, no_consequence, consequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     task_id = excluded.task_id,
                     position = excluded.position,
                     description = excluded.description,
                     priority = excluded.priority,
                     response = excluded.response,
                     mitigation = excluded.mitigation,
                     no_consequence = excluded.no_consequence,
                     consequence = excluded.consequence",
                params![
                    risk.id.to_string(),
                    task.id.to_string(),
                    index as i64 + 1,
                    risk.description,
                    match risk.priority {
                        RiskPriority::Low => "low",
                        RiskPriority::Medium => "medium",
                        RiskPriority::High => "high",
                    },
                    match risk.response {
                        RiskResponse::Approved => "approved",
                        RiskResponse::Mitigate => "mitigate",
                    },
                    risk.mitigation,
                    risk.no_consequence,
                    risk.consequence,
                ],
            )?;
            keep.insert(risk.id.to_string());
        }
    }
    let existing: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT r.id FROM risks r
             WHERE EXISTS (SELECT 1 FROM tasks t WHERE t.id = r.task_id)",
        )?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    for id in existing {
        if !keep.contains(&id) {
            conn.execute("DELETE FROM risks WHERE id = ?1", [&id])?;
        }
    }
    Ok(())
}

/// Deletes the dependency rows that are no longer in the project, or that
/// now connect a different pair of tasks.
///
/// # Why "different pair" too
/// The table allows only one link per (predecessor, successor) pair. If two
/// existing links swapped their tasks, updating them one by one would
/// briefly create a duplicate pair and fail. Deleting changed links first and
/// inserting them again avoids that.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn delete_stale_dependencies(conn: &Connection, project: &Project) -> Result<(), GtraError> {
    let wanted: BTreeMap<String, (String, String)> = project
        .dependencies
        .iter()
        .map(|d| {
            (
                d.id.to_string(),
                (d.predecessor.to_string(), d.successor.to_string()),
            )
        })
        .collect();
    let existing: Vec<(String, String, String)> = {
        let mut stmt = conn.prepare("SELECT id, predecessor_id, successor_id FROM dependencies")?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?
    };
    for (id, predecessor, successor) in existing {
        let keep = wanted.get(&id) == Some(&(predecessor, successor));
        if !keep {
            conn.execute("DELETE FROM dependencies WHERE id = ?1", [&id])?;
        }
    }
    Ok(())
}

/// Turns a whole percentage from the model into the fraction stored in
/// `dependencies.lag_percent`: 50 → 0.5, -25 → -0.25.
///
/// # Why a fraction
/// `0001_init.sql` defines the column as a fraction (1.0 = 100 %), like every
/// other percentage in the file. `read.rs` multiplies by 100 again and gets
/// the same whole number back (`f64` is exact enough for that).
pub(crate) fn percent_to_fraction(percent: i32) -> f64 {
    f64::from(percent) / 100.0
}

/// Inserts or updates every dependency.
///
/// # Lags
/// - Without `lag_percent`: `lag_minutes` = the lag, `lag_percent` = NULL;
///   a display unit of `'%'` becomes `'d'` (days).
/// - With `lag_percent`: `lag_percent` = the fraction, `lag_minutes` = 0 and
///   `lag_unit` = `'%'`. The table allows only one kind of lag per link
///   (CHECK rule), and the model ignores `lag` while `lag_percent` is set, so
///   a leftover `lag` is not stored.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_dependencies(conn: &Connection, project: &Project) -> Result<(), GtraError> {
    for dependency in &project.dependencies {
        let (lag_minutes, lag_percent) = match dependency.lag_percent {
            Some(percent) => (0, Some(percent_to_fraction(percent))),
            None => (dependency.lag.minutes(), None),
        };
        conn.execute(
            "INSERT INTO dependencies
                 (id, predecessor_id, successor_id, type, lag_minutes, lag_percent, lag_unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?6 IS NULL THEN 'd' ELSE '%' END)
             ON CONFLICT(id) DO UPDATE SET
                 type = excluded.type,
                 lag_minutes = excluded.lag_minutes,
                 lag_percent = excluded.lag_percent,
                 lag_unit = CASE
                     WHEN excluded.lag_percent IS NOT NULL THEN '%'
                     WHEN lag_unit = '%' THEN 'd'
                     ELSE lag_unit
                 END",
            params![
                dependency.id.to_string(),
                dependency.predecessor.to_string(),
                dependency.successor.to_string(),
                dependency_type_code(dependency.kind),
                lag_minutes,
                lag_percent,
            ],
        )?;
    }
    Ok(())
}

/// The two-letter code stored in `dependencies.type` for each dependency type.
///
/// # Example
/// `DependencyType::FinishToStart` → `"FS"`.
pub(crate) fn dependency_type_code(kind: DependencyType) -> &'static str {
    match kind {
        DependencyType::FinishToStart => "FS",
        DependencyType::StartToStart => "SS",
        DependencyType::FinishToFinish => "FF",
        DependencyType::StartToFinish => "SF",
    }
}

/// Deletes the calendars that are no longer in the project.
///
/// A calendar that other calendars are based on cannot be deleted
/// (`ON DELETE RESTRICT`), so links to the calendars being removed are
/// cleared first. Tasks and resources that used a removed calendar fall back
/// to the project calendar (`ON DELETE SET NULL`).
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn delete_stale_calendars(conn: &Connection, calendars: &[Calendar]) -> Result<(), GtraError> {
    let keep: BTreeSet<String> = calendars.iter().map(|c| c.id.to_string()).collect();
    let stale: Vec<String> = all_ids(conn, "calendars")?
        .into_iter()
        .filter(|id| !keep.contains(id))
        .collect();
    for id in &stale {
        conn.execute(
            "UPDATE calendars SET base_calendar_id = NULL WHERE base_calendar_id = ?1",
            [id],
        )?;
    }
    for id in &stale {
        conn.execute("DELETE FROM calendars WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Writes the `meta` rows that describe this save.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn write_meta(
    conn: &Connection,
    project: &Project,
    options: &SaveOptions,
) -> Result<(), GtraError> {
    set_meta(conn, "project_id", &project.id.to_string())?;
    set_meta(conn, "modified", &options.timestamp())?;
    set_meta(conn, "author", &options.author)?;
    set_meta(conn, "app_version", APP_VERSION)?;
    // An unknown engine version removes the entry instead of keeping a value
    // that belonged to an earlier save (see `SaveOptions::engine_version`).
    match &options.engine_version {
        Some(version) => set_meta(conn, "engine_version", version)?,
        None => {
            conn.execute("DELETE FROM meta WHERE key = 'engine_version'", [])?;
        }
    }
    Ok(())
}

/// Deletes every row of `table` whose `id` is not in `keep`.
///
/// # Parameters
/// - `table`: a table name from this crate (never user input, because table
///   names cannot be passed as `?` parameters and are put into the SQL text).
/// - `keep`: the ids to keep, as text.
///
/// # Errors
/// [`GtraError::Sqlite`] if a statement fails.
fn delete_rows_not_in(
    conn: &Connection,
    table: &'static str,
    keep: &BTreeSet<String>,
) -> Result<(), GtraError> {
    for id in all_ids(conn, table)? {
        if !keep.contains(&id) {
            conn.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [&id])?;
        }
    }
    Ok(())
}

/// Returns the `id` of every row of `table` (a table name from this crate).
///
/// # Errors
/// [`GtraError::Sqlite`] if the query fails.
fn all_ids(conn: &Connection, table: &'static str) -> Result<Vec<String>, GtraError> {
    let mut stmt = conn.prepare(&format!("SELECT id FROM {table}"))?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}
