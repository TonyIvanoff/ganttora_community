//! # The project: everything the engine needs, in one value
//!
//! ## What this file is for
//! Defines [`Project`], the top-level container: project settings (name,
//! start, project calendar, day length) and the lists of tasks, dependencies
//! and calendars.
//!
//! ## Where it fits
//! Part of the `model` crate. The engine's `schedule(&Project)` takes a
//! `Project` and returns the calculated dates. The `.gtra` file stores one
//! `Project`; the `document` crate keeps one open while the user edits it.
//!
//! ## Main ideas
//! - A project is scheduled either **forward from its start** (the default:
//!   you give the start, the engine works out the finish) or **backward from
//!   its finish** (you give the finish, the engine works out when work must
//!   begin). See [`ScheduleFrom`].
//! - The **order of `tasks`** is the order of the rows in the grid. The engine
//!   also uses it to break ties, so results are always the same.
//! - Calculated dates are not stored here; they live in the engine's result.
//!
//! ## Documentation

use crate::calendar::Calendar;
use crate::dependency::Dependency;
use crate::duration::DEFAULT_MINUTES_PER_DAY;
use crate::ids::{CalendarId, ProjectId};
use crate::task::Task;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Which end of the project is fixed: the start or the finish.
///
/// # Variants
/// - `Start`: the user gives the project start; tasks are placed as early as
///   possible and the engine calculates the finish (the default).
/// - `Finish`: the user gives the project finish (`Project::finish`); tasks
///   are placed as late as possible and the engine calculates the start.
///   Use it when the end date is fixed, e.g. "the shed must be ready on
///   Fri 26 Mar 2027".
///
/// # JSON
/// A plain string: `"start"` or `"finish"`.
///
/// # Example
/// ```
/// use model::ScheduleFrom;
/// assert_eq!(ScheduleFrom::default(), ScheduleFrom::Start);
/// assert_eq!(serde_json::to_string(&ScheduleFrom::Finish).unwrap(), "\"finish\"");
/// ```
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleFrom {
    /// Schedule forward from the project start.
    #[default]
    Start,
    /// Schedule backward from the project finish.
    Finish,
}

/// A whole project: settings plus tasks, dependencies and calendars.
///
/// # Fields
/// - `id`: unique ID of the project.
/// - `name`: project name.
/// - `start`: the project start. Tasks without predecessors start here (moved
///   forward to the next working moment if needed, e.g. from midnight to
///   08:00). Stored without a time zone (dates are local). Used
///   when `schedule_from` is `Start`.
/// - `schedule_from`: whether the start or the finish is fixed.
/// - `finish`: the fixed project finish, used when `schedule_from` is
///   `Finish` (the engine reports an error if it is missing then).
/// - `calendar_id`: which entry of `calendars` is the project calendar. Tasks
///   without their own task calendar use it, and lags are counted on it.
/// - `minutes_per_day`: how many working minutes make "1 day" when durations
///   are shown in days. Default 480 (8 hours), as in MS Project.
/// - `tasks`: the tasks, in row order.
/// - `dependencies`: the links between tasks.
/// - `calendars`: all calendars of this project.
///
/// # Example
/// ```
/// use model::{Dependency, Duration, Project, Task};
/// use chrono::NaiveDate;
///
/// let mut project = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let design = Task::new("Design the shed", Duration::from_days(2));
/// let permit = Task::new("Get building permit", Duration::from_days(5));
/// project.dependencies.push(Dependency::finish_to_start(design.id, permit.id));
/// project.tasks.push(design);
/// project.tasks.push(permit);
/// assert_eq!(project.tasks.len(), 2);
/// ```
///
/// # Beginner note: `NaiveDateTime`
/// `NaiveDateTime` comes from the `chrono` library. It is a date plus a time
/// of day, *without* a time zone ("naive"). Schedules are local: "08:00"
/// means 08:00 wherever the project happens.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Unique ID of the project.
    pub id: ProjectId,
    /// Project name.
    pub name: String,
    /// Project start; tasks without predecessors start here.
    pub start: NaiveDateTime,
    /// Whether the project is scheduled from its start or from its finish.
    #[serde(default)]
    pub schedule_from: ScheduleFrom,
    /// Fixed project finish (used when scheduling from the finish).
    #[serde(default)]
    pub finish: Option<NaiveDateTime>,
    /// ID of the project calendar (must be one of `calendars`).
    pub calendar_id: CalendarId,
    /// Working minutes in one "day" of duration (default 480).
    pub minutes_per_day: i64,
    /// Tasks in row order.
    pub tasks: Vec<Task>,
    /// Links between tasks.
    pub dependencies: Vec<Dependency>,
    /// All calendars of the project.
    pub calendars: Vec<Calendar>,
}

impl Project {
    /// Makes sure that every task is inside a project row.
    ///
    /// # What it does
    /// If there is no project row yet (a new project, or a file from before
    /// project rows existed), adds one at the top, named like the project,
    /// and moves every task one level deeper. Otherwise changes nothing.
    ///
    /// # Example
    /// ```
    /// use chrono::NaiveDate;
    /// use model::{Duration, Project, Task};
    /// let mut p = Project::new("House", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// p.tasks.push(Task::new("Walls", Duration::from_days(2)));
    /// p.ensure_project_row();
    /// p.ensure_project_row(); // the second call does nothing
    /// assert_eq!(p.tasks.len(), 2);
    /// assert!(p.tasks[0].is_project);
    /// assert_eq!(p.tasks[1].outline_level, 2);
    /// ```
    pub fn ensure_project_row(&mut self) {
        if self.tasks.iter().any(|t| t.is_project) {
            return;
        }
        for task in &mut self.tasks {
            task.outline_level += 1;
        }
        self.tasks.insert(0, Task::project_row(self.name.clone()));
    }

    /// Creates an empty project that starts at midnight of `start_date`, with
    /// the Standard calendar as its project calendar.
    ///
    /// # Parameters
    /// - `name`: the project name.
    /// - `start_date`: the day the project starts. The time is set to 00:00;
    ///   the engine moves it to the first working moment (08:00 on a working
    ///   day of the Standard calendar).
    ///
    /// # Returns
    /// A project with no tasks or links, one calendar ("Standard") and
    /// `minutes_per_day` = 480.
    ///
    /// # Example
    /// ```
    /// use model::Project;
    /// use chrono::NaiveDate;
    /// let p = Project::new("Garden shed", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// assert_eq!(p.calendars.len(), 1);
    /// assert_eq!(p.calendar_id, p.calendars[0].id);
    /// ```
    pub fn new(name: impl Into<String>, start_date: NaiveDate) -> Self {
        let calendar = Calendar::standard();
        Project {
            id: ProjectId::new(),
            name: name.into(),
            // `NaiveTime::MIN` is 00:00:00, the very start of the day.
            start: start_date.and_time(chrono::NaiveTime::MIN),
            schedule_from: ScheduleFrom::Start,
            finish: None,
            calendar_id: calendar.id,
            minutes_per_day: DEFAULT_MINUTES_PER_DAY,
            tasks: Vec::new(),
            dependencies: Vec::new(),
            calendars: vec![calendar],
        }
    }
}
