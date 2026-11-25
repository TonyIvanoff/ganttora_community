//! # Durations: how long something takes, in working minutes
//!
//! ## What this file is for
//! Defines [`Duration`], the amount of *working* time a task takes or a lag
//! adds. It is used for task durations, lags on dependencies and slack.
//!
//! ## Where it fits
//! Part of the `model` crate. The engine turns a `Duration` into real dates
//! with a calendar (see `engine::calendar_math`).
//!
//! ## Main ideas
//! - A duration counts **working time only**. "2 days" means 2 working days,
//!   not 48 hours on the wall clock ("Calendars").
//! - We store everything as a whole number of **minutes**. One unit for
//!   everything avoids rounding errors: 1 day = 480 minutes (8 hours x 60) on
//!   the default calendar, 1 hour = 60 minutes.
//! - "How many minutes is one day" is a project setting, like MS Project's
//!   "Hours per day". The default is [`DEFAULT_MINUTES_PER_DAY`] = 480.
//! - A duration may be **negative**. Tasks never have negative durations, but
//!   a lag can: a negative lag is a *lead* (an overlap).
//!
//! ## Worked example
//! ```text
//! 2 days    = 2 x 480 = 960 minutes
//! 4 hours   = 4 x 60  = 240 minutes
//! lead 1d   = -480 minutes (a lag of minus one day)
//! ```
//!
//! ## Documentation

use serde::{Deserialize, Serialize};

/// The number of working minutes in one working day when nothing else is
/// set: 8 hours x 60 minutes = 480. This matches MS Project's default
/// "Hours per day = 8".
///
/// # Beginner note: constants
/// `const` defines a value that never changes and is known when the program is
/// compiled. By convention constant names are written in CAPITAL letters.
pub const DEFAULT_MINUTES_PER_DAY: i64 = 480;

/// An amount of working time, stored as a whole number of minutes.
///
/// # Fields
/// - `minutes`: the number of working minutes. Negative values are allowed
///   (for leads).
///
/// # Example
/// ```
/// use model::Duration;
/// let d = Duration::from_days(2);
/// assert_eq!(d.minutes(), 960);
/// ```
///
/// # Beginner note: `i64`
/// `i64` is a signed 64-bit integer: a whole number that can be negative and
/// that is large enough for any project (about 17 million million years of
/// minutes).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct Duration {
    /// The amount of working time in minutes. Negative means a lead.
    minutes: i64,
}

impl Duration {
    /// A duration of zero minutes. Milestones have this duration.
    pub const ZERO: Duration = Duration { minutes: 0 };

    /// Creates a duration from a number of working minutes.
    ///
    /// # Parameters
    /// - `minutes`: working minutes; may be negative (a lead).
    ///
    /// # Returns
    /// The duration.
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert_eq!(Duration::from_minutes(90).minutes(), 90);
    /// ```
    pub const fn from_minutes(minutes: i64) -> Self {
        Duration { minutes }
    }

    /// Creates a duration from whole working hours (1 hour = 60 minutes).
    ///
    /// # Parameters
    /// - `hours`: working hours; may be negative.
    ///
    /// # Returns
    /// The duration, `hours x 60` minutes.
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert_eq!(Duration::from_hours(4).minutes(), 240);
    /// ```
    pub const fn from_hours(hours: i64) -> Self {
        Duration {
            minutes: hours * 60,
        }
    }

    /// Creates a duration from whole working days, using the default day
    /// length of 480 minutes ([`DEFAULT_MINUTES_PER_DAY`]).
    ///
    /// # Parameters
    /// - `days`: working days; may be negative.
    ///
    /// # Returns
    /// The duration, `days x 480` minutes.
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert_eq!(Duration::from_days(5).minutes(), 2400); // 5 x 480
    /// ```
    pub const fn from_days(days: i64) -> Self {
        Self::from_days_with(days, DEFAULT_MINUTES_PER_DAY)
    }

    /// Creates a duration from whole working days with a custom day length,
    /// for projects where "1 day" is not 8 hours (see
    /// [`Project::minutes_per_day`](crate::Project::minutes_per_day)).
    ///
    /// # Parameters
    /// - `days`: working days; may be negative.
    /// - `minutes_per_day`: how many working minutes make one day, e.g. 450
    ///   for a 7.5-hour day.
    ///
    /// # Returns
    /// The duration, `days x minutes_per_day` minutes.
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert_eq!(Duration::from_days_with(2, 450).minutes(), 900);
    /// ```
    pub const fn from_days_with(days: i64, minutes_per_day: i64) -> Self {
        Duration {
            minutes: days * minutes_per_day,
        }
    }

    /// Returns the number of working minutes.
    ///
    /// # Returns
    /// The stored minutes (negative for a lead).
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert_eq!(Duration::from_hours(1).minutes(), 60);
    /// ```
    pub const fn minutes(&self) -> i64 {
        self.minutes
    }

    /// Returns `true` when the duration is exactly zero (a milestone).
    ///
    /// # Example
    /// ```
    /// use model::Duration;
    /// assert!(Duration::ZERO.is_zero());
    /// assert!(!Duration::from_days(1).is_zero());
    /// ```
    pub const fn is_zero(&self) -> bool {
        self.minutes == 0
    }
}
