//! # Date helpers for tests
//!
//! ## What this file is for
//! Tests talk about dates all the time ("the permit finishes on Tuesday 9
//! March at 17:00"). Building such a date in Rust takes a long line of code.
//! These two helpers let a test write the date as short text instead:
//!
//! ```text
//!   dt("2027-03-09 17:00")   a moment: date + time of day
//!   date("2027-03-05")       a calendar day without a time
//! ```
//!
//! ## Where it fits
//! Part of the `ganttora_tests` helper library (`tests/rust/common/lib.rs`).

use chrono::{NaiveDate, NaiveDateTime};

/// Turns text like `"2027-03-01 08:00"` into a moment (date and time).
///
/// # Parameters
/// - `text`: the moment written as `YYYY-MM-DD HH:MM` (24-hour clock).
///
/// # Returns
/// A `NaiveDateTime`: a date and time without a time zone ("naive" means it
/// does not know about time zones; schedules do not need them).
///
/// # Errors / edge cases
/// If the text is not in that exact form (for example `"2027-3-1 8:00"` or
/// `"2027-02-30 08:00"`), the test stops with a message naming the bad text.
///
/// # Example
/// ```
/// use ganttora_tests::dt;
/// let monday_morning = dt("2027-03-01 08:00");
/// assert_eq!(monday_morning.to_string(), "2027-03-01 08:00:00");
/// ```
pub fn dt(text: &str) -> NaiveDateTime {
    // `parse_from_str` reads text using a pattern: %Y = year, %m = month,
    // %d = day, %H = hour, %M = minute.
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")
        .unwrap_or_else(|_| panic!("bad test moment {text:?}, expected \"YYYY-MM-DD HH:MM\""))
}

/// Turns text like `"2027-03-05"` into a calendar day.
///
/// # Parameters
/// - `text`: the day written as `YYYY-MM-DD`.
///
/// # Returns
/// A `NaiveDate` (a day without a time of day).
///
/// # Errors / edge cases
/// Bad text stops the test with a message naming it.
///
/// # Example
/// ```
/// use ganttora_tests::date;
/// assert_eq!(date("2027-03-05").to_string(), "2027-03-05");
/// ```
pub fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("bad test date {text:?}, expected \"YYYY-MM-DD\""))
}
