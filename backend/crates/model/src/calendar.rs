//! # Calendars: when work can happen
//!
//! ## What this file is for
//! Defines [`Calendar`]: which days of the week are worked, which hours of
//! each day are worked, and which dates are holidays. The engine needs this
//! to turn "2 working days" into real dates.
//!
//! ## Where it fits
//! Part of the `model` crate. A [`Project`](crate::Project) holds a list of
//! calendars and names one of them as the project calendar. The engine's
//! `calendar_math` module does the actual date arithmetic.
//!
//! ## Main ideas
//! - A **work week** ([`WorkWeek`]) lists, for each weekday, the working time
//!   ranges of that day. A day with no ranges is a day off (Saturday and
//!   Sunday in the standard calendar).
//! - A **working time range** ([`WorkingTime`]) is "from 08:00 to 12:00".
//!   Times are stored as *minutes since midnight*: 08:00 is 480, 12:00 is
//!   720, and the very end of the day is 1440 ("24:00"). Using minutes lets
//!   us write a range that runs until midnight, which a normal clock time
//!   cannot express.
//! - **Holidays** are single dates that are not worked, even if their
//!   weekday normally is.
//! - **Exceptions** ([`CalendarException`]) are date ranges with their own
//!   working times: a company shutdown (no working times), or a Saturday that
//!   is worked 08:00-12:00. An exception wins over both the work week and the
//!   holiday list, so it is the most specific rule (MS Project calls these
//!   "exceptions" too).
//!
//! ```text
//! which rule decides a date?   1. an exception covering it   (most specific)
//!                              2. the holiday list
//!                              3. the work week              (most general)
//! ```
//!
//! ## Worked example: the Standard calendar
//! ```text
//!            08:00    12:00  13:00    17:00
//! Mon - Fri    |========|      |========|     = 4 h + 4 h = 8 working hours
//! Sat, Sun     (no working time)
//! ```
//! This is MS Project's "Standard" calendar, and Ganttora's default.
//!
//! ## Documentation

use crate::ids::CalendarId;
use chrono::{NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One continuous block of working time within a day, such as 08:00-12:00.
///
/// # Fields
/// - `start_minute`: when the block starts, in minutes since midnight
///   (08:00 = 480).
/// - `end_minute`: when the block ends, in minutes since midnight
///   (12:00 = 720; 1440 means midnight at the end of the day).
///
/// A valid block has `start_minute < end_minute <= 1440`. This file does not
/// check that (it holds data only); the engine checks it before scheduling.
///
/// # Example
/// ```
/// use model::WorkingTime;
/// let morning = WorkingTime::hours(8, 12);
/// assert_eq!(morning.start_minute, 480);
/// assert_eq!(morning.end_minute, 720);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct WorkingTime {
    /// Start of the block, in minutes since midnight.
    pub start_minute: u32,
    /// End of the block, in minutes since midnight (at most 1440).
    pub end_minute: u32,
}

impl WorkingTime {
    /// Creates a block from minutes since midnight.
    ///
    /// # Parameters
    /// - `start_minute`: start, e.g. 480 for 08:00.
    /// - `end_minute`: end, e.g. 1020 for 17:00.
    ///
    /// # Returns
    /// The block. No validation is done here.
    ///
    /// # Example
    /// ```
    /// use model::WorkingTime;
    /// let half_past_eight_to_noon = WorkingTime::new(510, 720);
    /// assert_eq!(half_past_eight_to_noon.end_minute - half_past_eight_to_noon.start_minute, 210);
    /// ```
    ///
    /// # Beginner note: `u32`
    /// `u32` is an *unsigned* 32-bit integer: a whole number that cannot be
    /// negative. A time of day is never negative, so this type fits.
    pub const fn new(start_minute: u32, end_minute: u32) -> Self {
        WorkingTime {
            start_minute,
            end_minute,
        }
    }

    /// Creates a block from whole hours, e.g. `hours(13, 17)` for 13:00-17:00.
    ///
    /// # Parameters
    /// - `start_hour`: start hour, 0 to 24.
    /// - `end_hour`: end hour, 0 to 24 (24 = midnight at the end of the day).
    ///
    /// # Returns
    /// The block, with the hours converted to minutes (x 60).
    ///
    /// # Example
    /// ```
    /// use model::WorkingTime;
    /// assert_eq!(WorkingTime::hours(13, 17), WorkingTime::new(780, 1020));
    /// ```
    pub const fn hours(start_hour: u32, end_hour: u32) -> Self {
        WorkingTime::new(start_hour * 60, end_hour * 60)
    }
}

/// The normal weekly working pattern: the working time blocks of each
/// weekday. An empty list means that weekday is not worked.
///
/// # Fields
/// One field per weekday, `monday` to `sunday`. Each holds the blocks of that
/// day, which should be sorted by start time and must not overlap.
///
/// # Example
/// ```
/// use model::WorkWeek;
/// use chrono::Weekday;
/// let week = WorkWeek::standard();
/// assert_eq!(week.day(Weekday::Mon).len(), 2); // morning and afternoon
/// assert!(week.day(Weekday::Sun).is_empty());  // Sunday is free
/// ```
///
/// # Beginner note: `Vec`
/// `Vec<WorkingTime>` is a *vector*: a list that can grow or shrink, here a
/// list of working time blocks.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct WorkWeek {
    /// Working blocks on Mondays.
    pub monday: Vec<WorkingTime>,
    /// Working blocks on Tuesdays.
    pub tuesday: Vec<WorkingTime>,
    /// Working blocks on Wednesdays.
    pub wednesday: Vec<WorkingTime>,
    /// Working blocks on Thursdays.
    pub thursday: Vec<WorkingTime>,
    /// Working blocks on Fridays.
    pub friday: Vec<WorkingTime>,
    /// Working blocks on Saturdays.
    pub saturday: Vec<WorkingTime>,
    /// Working blocks on Sundays.
    pub sunday: Vec<WorkingTime>,
}

impl WorkWeek {
    /// The MS Project "Standard" week: Monday to Friday, 08:00-12:00 and
    /// 13:00-17:00; Saturday and Sunday off.
    ///
    /// # Returns
    /// A new `WorkWeek` with that pattern (8 working hours per weekday,
    /// 40 per week).
    ///
    /// # Example
    /// ```
    /// use model::{WorkWeek, WorkingTime};
    /// use chrono::Weekday;
    /// let week = WorkWeek::standard();
    /// assert_eq!(week.day(Weekday::Fri), &[WorkingTime::hours(8, 12), WorkingTime::hours(13, 17)]);
    /// ```
    pub fn standard() -> Self {
        // `vec![a, b]` is a macro that builds a Vec holding a and b.
        let workday = vec![WorkingTime::hours(8, 12), WorkingTime::hours(13, 17)];
        WorkWeek {
            // `.clone()` makes a copy: each weekday gets its own list.
            monday: workday.clone(),
            tuesday: workday.clone(),
            wednesday: workday.clone(),
            thursday: workday.clone(),
            // The last use can take `workday` itself; no copy needed.
            friday: workday,
            saturday: Vec::new(),
            sunday: Vec::new(),
        }
    }

    /// Returns the working blocks of one weekday.
    ///
    /// # Parameters
    /// - `weekday`: which day of the week, e.g. `Weekday::Mon`.
    ///
    /// # Returns
    /// A borrowed slice (a read-only view) of that day's blocks; empty if the
    /// day is not worked.
    ///
    /// # Example
    /// ```
    /// use model::WorkWeek;
    /// use chrono::Weekday;
    /// assert!(WorkWeek::standard().day(Weekday::Sat).is_empty());
    /// ```
    ///
    /// # Beginner note: `match`
    /// `match` compares a value against several patterns and runs the branch
    /// that fits. The compiler checks that all seven weekdays are handled.
    pub fn day(&self, weekday: Weekday) -> &[WorkingTime] {
        match weekday {
            Weekday::Mon => &self.monday,
            Weekday::Tue => &self.tuesday,
            Weekday::Wed => &self.wednesday,
            Weekday::Thu => &self.thursday,
            Weekday::Fri => &self.friday,
            Weekday::Sat => &self.saturday,
            Weekday::Sun => &self.sunday,
        }
    }
}

/// A date range with its own working times, replacing the work week on
/// those dates.
///
/// # Fields
/// - `name`: what the exception is, e.g. "Summer shutdown".
/// - `start`: first date of the range.
/// - `finish`: last date of the range (inclusive; equal to `start` for one
///   day).
/// - `working_times`: the working blocks of each day in the range. Empty
///   means the whole range is non-working.
///
/// # Worked example
/// ```text
/// "Summer shutdown"  Mon 2 Aug - Fri 13 Aug 2027, no working times
///    -> ten weekdays off; a task needing 3 days that starts Fri 30 Jul
///       finishes Tue 17 Aug instead of Tue 3 Aug.
/// "Saturday push"    Sat 6 Mar 2027, 08:00-12:00
///    -> four extra working hours on that Saturday only.
/// ```
///
/// # Example
/// ```
/// use model::{CalendarException, WorkingTime};
/// use chrono::NaiveDate;
/// let saturday = NaiveDate::from_ymd_opt(2027, 3, 6).unwrap();
/// let push = CalendarException::working("Saturday push", saturday, saturday,
///     vec![WorkingTime::hours(8, 12)]);
/// assert_eq!(push.working_times.len(), 1);
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarException {
    /// Display name, e.g. "Summer shutdown".
    pub name: String,
    /// First date of the range.
    pub start: NaiveDate,
    /// Last date of the range (inclusive).
    pub finish: NaiveDate,
    /// Working blocks on each day of the range; empty = non-working.
    pub working_times: Vec<WorkingTime>,
}

impl CalendarException {
    /// Creates a non-working exception (a shutdown, a bridge day…).
    ///
    /// # Parameters
    /// - `name`: display name.
    /// - `start`, `finish`: first and last date (inclusive).
    ///
    /// # Example
    /// ```
    /// use model::CalendarException;
    /// use chrono::NaiveDate;
    /// let d = NaiveDate::from_ymd_opt(2027, 8, 2).unwrap();
    /// assert!(CalendarException::non_working("Shutdown", d, d).working_times.is_empty());
    /// ```
    pub fn non_working(name: impl Into<String>, start: NaiveDate, finish: NaiveDate) -> Self {
        CalendarException {
            name: name.into(),
            start,
            finish,
            working_times: Vec::new(),
        }
    }

    /// Creates an exception with its own working times.
    ///
    /// # Parameters
    /// - `name`: display name.
    /// - `start`, `finish`: first and last date (inclusive).
    /// - `working_times`: the blocks worked on each of those days, sorted
    ///   and not overlapping.
    ///
    /// # Example
    /// See [`CalendarException`].
    pub fn working(
        name: impl Into<String>,
        start: NaiveDate,
        finish: NaiveDate,
        working_times: Vec<WorkingTime>,
    ) -> Self {
        CalendarException {
            name: name.into(),
            start,
            finish,
            working_times,
        }
    }

    /// `true` when `date` lies inside this exception's range.
    ///
    /// # Example
    /// ```
    /// use model::CalendarException;
    /// use chrono::NaiveDate;
    /// let d = |day| NaiveDate::from_ymd_opt(2027, 8, day).unwrap();
    /// let shutdown = CalendarException::non_working("Shutdown", d(2), d(13));
    /// assert!(shutdown.covers(d(5)));
    /// assert!(!shutdown.covers(d(14)));
    /// ```
    pub fn covers(&self, date: NaiveDate) -> bool {
        self.start <= date && date <= self.finish
    }
}

/// A working-time calendar: a weekly pattern plus holidays and exceptions.
///
/// # Fields
/// - `id`: unique ID of the calendar.
/// - `name`: display name, e.g. "Standard".
/// - `week`: the normal working pattern of each weekday.
/// - `holidays`: dates that are not worked at all, whatever their weekday.
/// - `exceptions`: date ranges with their own working times (they win over
///   `week` and `holidays`). When two exceptions cover the same date, the
///   first one in the list wins.
///
/// # Example
/// ```
/// use model::Calendar;
/// use chrono::NaiveDate;
/// let mut cal = Calendar::standard();
/// // Make Friday 5 March 2027 a holiday ("A holiday changes the dates").
/// cal.holidays.insert(NaiveDate::from_ymd_opt(2027, 3, 5).unwrap());
/// assert_eq!(cal.holidays.len(), 1);
/// ```
///
/// # Beginner note: `BTreeSet`
/// A `BTreeSet` is a collection that holds each value at most once and keeps
/// the values sorted. We use it (and not a `HashSet`) because its order is
/// always the same, which keeps saved files and engine results identical from
/// run to run.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    /// Unique ID of this calendar.
    pub id: CalendarId,
    /// Display name, e.g. "Standard".
    pub name: String,
    /// The normal weekly working pattern.
    pub week: WorkWeek,
    /// Non-working dates (public holidays, company days off).
    pub holidays: BTreeSet<NaiveDate>,
    /// Date ranges with their own working times. Missing in JSON = empty.
    #[serde(default)]
    pub exceptions: Vec<CalendarException>,
}

impl Calendar {
    /// Creates a calendar with the given name and weekly pattern, a new
    /// random ID and no holidays.
    ///
    /// # Parameters
    /// - `name`: display name.
    /// - `week`: the weekly working pattern.
    ///
    /// # Returns
    /// The new calendar.
    ///
    /// # Example
    /// ```
    /// use model::{Calendar, WorkWeek};
    /// let cal = Calendar::new("Night shift", WorkWeek::standard());
    /// assert_eq!(cal.name, "Night shift");
    /// ```
    ///
    /// # Beginner note: `impl Into<String>`
    /// The parameter type `impl Into<String>` accepts anything that can be
    /// turned into a `String`, so callers can pass `"text"` directly instead
    /// of writing `"text".to_string()`.
    pub fn new(name: impl Into<String>, week: WorkWeek) -> Self {
        Calendar::with_id(CalendarId::new(), name, week)
    }

    /// Like [`Calendar::new`], but with a given ID (for files and tests).
    ///
    /// # Parameters
    /// - `id`: the calendar's ID.
    /// - `name`: display name.
    /// - `week`: the weekly working pattern.
    ///
    /// # Returns
    /// The calendar, without holidays or exceptions.
    ///
    /// # Example
    /// ```
    /// use model::{Calendar, CalendarId, WorkWeek};
    /// let id = CalendarId::new();
    /// assert_eq!(Calendar::with_id(id, "Night", WorkWeek::standard()).id, id);
    /// ```
    pub fn with_id(id: CalendarId, name: impl Into<String>, week: WorkWeek) -> Self {
        Calendar {
            id,
            name: name.into(),
            week,
            holidays: BTreeSet::new(),
            exceptions: Vec::new(),
        }
    }

    /// Creates the default "Standard" calendar: Monday to Friday,
    /// 08:00-12:00 and 13:00-17:00, no holidays. Same as MS Project's
    /// Standard calendar.
    ///
    /// # Returns
    /// The new calendar, with a new random ID.
    ///
    /// # Example
    /// ```
    /// use model::Calendar;
    /// let cal = Calendar::standard();
    /// assert_eq!(cal.name, "Standard");
    /// assert!(cal.holidays.is_empty());
    /// ```
    pub fn standard() -> Self {
        Calendar::new("Standard", WorkWeek::standard())
    }
}
