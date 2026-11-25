//! # Calendar arithmetic: adding and subtracting *working* time
//!
//! ## What this file is for
//! A schedule counts working time only. "2 days" starting on a Friday does
//! not end on Sunday; it ends on Monday evening. This file contains the
//! functions that do that arithmetic on a [`Calendar`]:
//!
//! | Function | Question it answers |
//! |---|---|
//! | [`add_working_time`] | "If I start at X and work for D, when do I finish?" |
//! | [`subtract_working_time`] | "If I must finish at X and need D, when must I start?" |
//! | [`snap_forward`] | "What is the first moment at or after X when work can happen?" |
//! | [`snap_backward`] | "What is the last moment at or before X when work was happening?" |
//! | [`working_minutes_between`] | "How much working time lies between X and Y?" |
//! | [`touches_working_time`] | "Is X inside a working block or exactly on its edge?" |
//! | [`validate_calendar`] | "Is this calendar usable at all?" |
//!
//! ## Where it fits
//! Part of the `engine` crate. The forward pass (`cpm::forward`), backward
//! pass (`cpm::backward`) and slack calculation (`cpm::slack`) call these
//! functions every time they turn a duration into a date.
//!
//! ## Main ideas
//! - We work in whole **minutes**. A working block such as 08:00-12:00 is
//!   the minutes 480 to 720 of that day. Seconds are ignored.
//! - A moment is a *point* in time, like a tick on a ruler. A working block
//!   08:00-12:00 contains the moments from 08:00 up to 12:00. Work done
//!   "during" the block happens *between* those ticks.
//! - **MS Project convention for finishes**: a task that uses up the last
//!   working minute of Tuesday finishes at **Tuesday 17:00**, not Wednesday
//!   08:00. [`add_working_time`] therefore stops exactly at the end of the
//!   last minute it uses.
//! - **Which working times apply on a date**: an exception covering the
//!   date wins, then the holiday list, then the work week (see
//!   `model::calendar`).
//! - **MS Project convention for starts**: a task that may start at
//!   Tuesday 17:00 really starts at **Wednesday 08:00**, the next moment work
//!   can happen. That move is [`snap_forward`].
//!
//! ## Worked example (Standard calendar: Mon-Fri 08:00-12:00, 13:00-17:00)
//! ```text
//! add 4 hours to Mon 10:00:
//!   10:00-12:00 uses 2 h (2 h left), lunch is skipped,
//!   13:00-15:00 uses 2 h (0 h left)            -> Mon 15:00
//!
//! add 2 days (960 min) to Fri 08:00:
//!   Fri 08:00-17:00 uses 480 min (480 left), Sat and Sun are skipped,
//!   Mon 08:00-17:00 uses 480 min (0 left)      -> Mon 17:00
//!
//! snap_forward(Tue 17:00)                     -> Wed 08:00
//! snap_backward(Wed 08:00)                    -> Tue 17:00
//! ```
//!
//! ## Beginner note: `Result` and the `?` operator
//! Most functions here return `Result<NaiveDateTime, CalendarError>`: either
//! `Ok(date)` or `Err(problem)`. Inside a function that itself returns a
//! `Result`, writing `something()?` means "if this failed, stop here and pass
//! the error up to my caller; otherwise give me the value". See book section
//! 1.7, "Errors and debugging".
//!
//! ## Documentation

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Weekday};
use model::{Calendar, CalendarId, Duration, WorkingTime};
use serde::{Deserialize, Serialize};

/// The number of minutes in a whole day (24 x 60). A working block may end
/// at this value, meaning "midnight at the end of the day".
const MINUTES_PER_DAY_CLOCK: u32 = 1440;

/// Everything that can go wrong in calendar arithmetic.
///
/// # Variants
/// - `NoWorkingTime`: the calendar has no working minute in its whole week,
///   so no task could ever be scheduled on it.
/// - `InvalidWorkingTime`: one weekday's blocks are broken (a block ends
///   before it starts, ends after midnight, or blocks overlap or are not in
///   time order).
/// - `InvalidException`: an exception ends before it starts, or its working
///   times are broken in the same ways as a weekday's.
/// - `DateOutOfRange`: the calculation ran past the last date the computer
///   can represent (the year 262 143). Only absurd durations cause this.
///
/// # JSON (what the UI receives)
/// A stable error code plus details, never an English sentence, so the UI
/// can show the message in the user's language:
/// ```text
/// { "code": "noWorkingTime", "details": { "calendar": "…" } }
/// ```
///
/// # Example
/// ```
/// use engine::calendar_math::{validate_calendar, CalendarError};
/// use model::{Calendar, WorkWeek};
/// let empty_week = WorkWeek { monday: vec![], tuesday: vec![], wednesday: vec![],
///     thursday: vec![], friday: vec![], saturday: vec![], sunday: vec![] };
/// let cal = Calendar::new("Never", empty_week);
/// assert_eq!(validate_calendar(&cal), Err(CalendarError::NoWorkingTime { calendar: cal.id }));
/// ```
///
/// # Beginner note: `thiserror`
/// `#[derive(thiserror::Error)]` and the `#[error("...")]` lines generate the
/// human-readable message for each variant, so we do not write it by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
// `tag = "code", content = "details"`: the variant name goes into "code",
// its fields into "details" (serde calls this "adjacently tagged").
#[serde(
    tag = "code",
    content = "details",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CalendarError {
    /// The calendar has no working time on any weekday.
    #[error("calendar {calendar} has no working time in its week")]
    NoWorkingTime {
        /// The calendar with the problem.
        calendar: CalendarId,
    },
    /// The working blocks of one weekday are invalid.
    #[error("calendar {calendar} has invalid working times on {weekday}")]
    InvalidWorkingTime {
        /// The calendar with the problem.
        calendar: CalendarId,
        /// The weekday whose blocks are broken.
        weekday: Weekday,
    },
    /// An exception of the calendar is invalid.
    #[error("calendar {calendar} has an invalid exception starting {start}")]
    InvalidException {
        /// The calendar with the problem.
        calendar: CalendarId,
        /// The first date of the broken exception.
        start: NaiveDate,
    },
    /// A date calculation went beyond the representable date range.
    #[error("date calculation is out of the supported range")]
    DateOutOfRange,
}

/// All seven weekdays, Monday first. Used to check every day of a work week.
const ALL_WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// Checks that a calendar can be used for scheduling.
///
/// # What it does
/// For each weekday, checks that every block satisfies
/// `start < end <= 1440` and that the blocks are in time order without
/// overlapping (08:00-12:00 then 13:00-17:00 is fine; 08:00-12:00 then
/// 11:00-15:00 is not). Then checks that at least one weekday has working
/// time.
///
/// # Parameters
/// - `calendar`: the calendar to check.
///
/// # Returns
/// `Ok(())` if the calendar is usable. (`()` is the "empty value": there is
/// nothing to return except "it worked".)
///
/// Exceptions are checked too: each must have `start <= finish` and valid
/// working blocks.
///
/// # Errors
/// - [`CalendarError::InvalidWorkingTime`] for the first broken weekday.
/// - [`CalendarError::InvalidException`] for the first broken exception.
/// - [`CalendarError::NoWorkingTime`] if the week has no working minute.
///
/// # Example
/// ```
/// use engine::calendar_math::validate_calendar;
/// use model::Calendar;
/// assert!(validate_calendar(&Calendar::standard()).is_ok());
/// ```
pub fn validate_calendar(calendar: &Calendar) -> Result<(), CalendarError> {
    for weekday in ALL_WEEKDAYS {
        if !blocks_are_valid(calendar.week.day(weekday)) {
            return Err(CalendarError::InvalidWorkingTime {
                calendar: calendar.id,
                weekday,
            });
        }
    }
    for exception in &calendar.exceptions {
        if exception.start > exception.finish || !blocks_are_valid(&exception.working_times) {
            return Err(CalendarError::InvalidException {
                calendar: calendar.id,
                start: exception.start,
            });
        }
    }
    ensure_working_time(calendar)
}

/// Checks one day's working blocks: each satisfies `start < end <= 1440`,
/// and they are in time order without overlapping.
fn blocks_are_valid(blocks: &[WorkingTime]) -> bool {
    // `previous_end` remembers where the last block ended, so we can check
    // that the next block starts at or after it.
    let mut previous_end = 0;
    for block in blocks {
        let broken = block.start_minute >= block.end_minute
            || block.end_minute > MINUTES_PER_DAY_CLOCK
            || block.start_minute < previous_end;
        if broken {
            return false;
        }
        previous_end = block.end_minute;
    }
    true
}

/// Returns an error if the calendar's week has no working block at all.
///
/// # Why this exists
/// Every "walk forward until we find working time" loop in this file would
/// run forever on such a calendar. Holidays cannot cause that, because there
/// are only finitely many of them: after the last holiday, the normal week
/// pattern always comes back.
fn ensure_working_time(calendar: &Calendar) -> Result<(), CalendarError> {
    // `.iter().any(...)` asks "is there at least one element for which this
    // is true?". The `|day| ...` part is a *closure*: a small unnamed
    // function written in place.
    let has_work = ALL_WEEKDAYS.iter().any(|day| {
        calendar
            .week
            .day(*day)
            .iter()
            .any(|block| block.start_minute < block.end_minute)
    });
    if has_work {
        Ok(())
    } else {
        Err(CalendarError::NoWorkingTime {
            calendar: calendar.id,
        })
    }
}

/// Returns the working blocks of one specific date.
///
/// # The rule (most specific first)
/// 1. The first exception whose range covers the date: its working times
///    (empty for a non-working exception).
/// 2. A holiday: no working time.
/// 3. Otherwise the blocks of its weekday.
///
/// # Parameters
/// - `calendar`: the calendar.
/// - `date`: the date.
///
/// # Returns
/// A borrowed slice of blocks (possibly empty).
fn blocks_on(calendar: &Calendar, date: NaiveDate) -> &[WorkingTime] {
    // Exceptions are few (a handful per year), so a simple scan is fast.
    // `find` stops at the first match, which makes "first one wins" true.
    if let Some(exception) = calendar.exceptions.iter().find(|e| e.covers(date)) {
        return &exception.working_times;
    }
    if calendar.holidays.contains(&date) {
        &[]
    } else {
        calendar.week.day(date.weekday())
    }
}

/// Returns `true` if `date` has at least one working block.
///
/// # The rule
/// The same order as everywhere in this file, most specific first:
/// 1. If an **exception** covers the date, the exception decides: a
///    working exception (e.g. "Saturday 6 Mar 2027, 08:00-12:00") makes
///    even a weekend day a working day; a non-working exception (e.g.
///    "company trip, 10-12 Mar") makes a weekday a day off.
/// 2. Otherwise a **holiday** is a day off.
/// 3. Otherwise the **work week** decides (Mon-Fri working on the standard
///    calendar).
///
/// # Parameters
/// - `calendar`: the calendar.
/// - `date`: the date to check.
///
/// # Returns
/// `true` for a working day, `false` for a day off.
///
/// # Example
/// ```
/// use engine::calendar_math::is_working_day;
/// use model::Calendar;
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let monday = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap();
/// let saturday = NaiveDate::from_ymd_opt(2027, 3, 6).unwrap();
/// assert!(is_working_day(&cal, monday));
/// assert!(!is_working_day(&cal, saturday));
/// ```
pub fn is_working_day(calendar: &Calendar, date: NaiveDate) -> bool {
    !blocks_on(calendar, date).is_empty()
}

/// The minute of the day of a moment: 08:00 -> 480. Seconds are dropped.
fn minute_of_day(moment: NaiveDateTime) -> u32 {
    moment.hour() * 60 + moment.minute()
}

/// Builds the moment "`minute` minutes after midnight on `date`". A minute of
/// 1440 gives midnight of the *next* day, which is how "24:00" is written.
fn at_minute(date: NaiveDate, minute: u32) -> NaiveDateTime {
    date.and_time(NaiveTime::MIN) + TimeDelta::minutes(i64::from(minute))
}

/// The day after `date`, or an error at the end of the supported range.
fn next_day(date: NaiveDate) -> Result<NaiveDate, CalendarError> {
    // `.ok_or(...)` turns an `Option` (a value or nothing) into a `Result`.
    date.succ_opt().ok_or(CalendarError::DateOutOfRange)
}

/// The day before `date`, or an error at the start of the supported range.
fn previous_day(date: NaiveDate) -> Result<NaiveDate, CalendarError> {
    date.pred_opt().ok_or(CalendarError::DateOutOfRange)
}

/// Moves a moment forward to the first moment at which work can be done.
///
/// # What it does
/// If `moment` is inside a working block (from its start up to, but not
/// including, its end), it is returned unchanged. Otherwise the start of the
/// next working block is returned. This is how a task's start is placed: a
/// task that "may start at 17:00" really starts at 08:00 the next working day.
///
/// # Worked examples (Standard calendar)
/// ```text
/// Mon 10:00 -> Mon 10:00   (already inside 08:00-12:00)
/// Mon 12:00 -> Mon 13:00   (12:00 is the end of the morning block: lunch)
/// Fri 17:00 -> Mon 08:00   (end of Friday; the weekend is skipped)
/// Mon 00:00 -> Mon 08:00   (project start at midnight)
/// ```
///
/// # Parameters
/// - `calendar`: the working-time calendar.
/// - `moment`: the moment to move.
///
/// # Returns
/// The first working moment at or after `moment`.
///
/// # Errors
/// [`CalendarError::NoWorkingTime`] if the calendar has no working time;
/// [`CalendarError::DateOutOfRange`] at the end of the date range.
///
/// # Example
/// ```
/// use engine::calendar_math::snap_forward;
/// use model::Calendar;
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let fri_17 = NaiveDate::from_ymd_opt(2027, 3, 5).unwrap().and_hms_opt(17, 0, 0).unwrap();
/// let mon_08 = NaiveDate::from_ymd_opt(2027, 3, 8).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// assert_eq!(snap_forward(&cal, fri_17).unwrap(), mon_08);
/// ```
pub fn snap_forward(
    calendar: &Calendar,
    moment: NaiveDateTime,
) -> Result<NaiveDateTime, CalendarError> {
    ensure_working_time(calendar)?;
    let mut date = moment.date();
    let mut minute = minute_of_day(moment);
    // `loop` repeats forever until a `return` leaves the function. It ends
    // because ensure_working_time() proved that some weekday has work.
    loop {
        for block in blocks_on(calendar, date) {
            if minute < block.end_minute {
                // Either we are inside this block (keep our minute) or before
                // it (jump to its start). `max` picks the later of the two.
                return Ok(at_minute(date, minute.max(block.start_minute)));
            }
        }
        // Nothing left today: try the next day from midnight.
        date = next_day(date)?;
        minute = 0;
    }
}

/// Moves a moment backward to the last moment at which work was being done.
///
/// # What it does
/// The mirror image of [`snap_forward`]. If `moment` is inside a working
/// block (after its start, up to and including its end), it is returned
/// unchanged. Otherwise the end of the previous working block is returned.
/// The backward pass uses this to place a task's late finish: a task that
/// "must finish by Monday 08:00" really must finish by Friday 17:00.
///
/// # Worked examples (Standard calendar)
/// ```text
/// Mon 10:00 -> Mon 10:00   (inside 08:00-12:00)
/// Mon 13:00 -> Mon 12:00   (start of the afternoon block: back to before lunch)
/// Mon 08:00 -> Fri 17:00   (start of Monday; the weekend is skipped)
/// ```
///
/// # Parameters
/// - `calendar`: the working-time calendar.
/// - `moment`: the moment to move.
///
/// # Returns
/// The last working moment at or before `moment`.
///
/// # Errors
/// As for [`snap_forward`].
///
/// # Example
/// ```
/// use engine::calendar_math::snap_backward;
/// use model::Calendar;
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let mon_08 = NaiveDate::from_ymd_opt(2027, 3, 8).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// let fri_17 = NaiveDate::from_ymd_opt(2027, 3, 5).unwrap().and_hms_opt(17, 0, 0).unwrap();
/// assert_eq!(snap_backward(&cal, mon_08).unwrap(), fri_17);
/// ```
pub fn snap_backward(
    calendar: &Calendar,
    moment: NaiveDateTime,
) -> Result<NaiveDateTime, CalendarError> {
    ensure_working_time(calendar)?;
    let mut date = moment.date();
    let mut minute = minute_of_day(moment);
    loop {
        // `.rev()` walks the blocks from last to first, because we are
        // looking backward in time.
        for block in blocks_on(calendar, date).iter().rev() {
            if minute > block.start_minute {
                // Inside this block (keep our minute) or after it (jump back
                // to its end). `min` picks the earlier of the two.
                return Ok(at_minute(date, minute.min(block.end_minute)));
            }
        }
        // Nothing earlier today: try the previous day from its very end.
        date = previous_day(date)?;
        minute = MINUTES_PER_DAY_CLOCK;
    }
}

/// Returns `true` if `moment` lies inside a working block or exactly on one
/// of its edges (its start or its end).
///
/// # Why this exists
/// Milestones (zero-duration tasks) do not need to be moved to the *start*
/// of working time. MS Project shows a milestone that follows a task ending
/// Tuesday 17:00 at **Tuesday 17:00**, not Wednesday 08:00. So the engine
/// keeps a milestone's moment as long as it touches working time, and only
/// snaps it when it falls in the middle of non-working time (e.g. Saturday).
///
/// # Parameters
/// - `calendar`: the calendar.
/// - `moment`: the moment to check.
///
/// # Returns
/// `true` if the moment touches working time.
///
/// # Example
/// ```
/// use engine::calendar_math::touches_working_time;
/// use model::Calendar;
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let day = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap(); // a Monday
/// assert!(touches_working_time(&cal, day.and_hms_opt(17, 0, 0).unwrap()));  // end of day
/// assert!(!touches_working_time(&cal, day.and_hms_opt(20, 0, 0).unwrap())); // evening
/// ```
pub fn touches_working_time(calendar: &Calendar, moment: NaiveDateTime) -> bool {
    let date = moment.date();
    let minute = minute_of_day(moment);
    let on_this_day = blocks_on(calendar, date)
        .iter()
        .any(|b| b.start_minute <= minute && minute <= b.end_minute);
    if on_this_day {
        return true;
    }
    // Midnight can also be the *end* ("24:00") of a block of the day before.
    // `if a && let Some(x) = b` (a "let chain", Rust 2024) runs the block only
    // when `a` is true AND `b` holds a value, which is then named `x`.
    if minute == 0
        && let Some(yesterday) = date.pred_opt()
    {
        return blocks_on(calendar, yesterday)
            .iter()
            .any(|b| b.end_minute == MINUTES_PER_DAY_CLOCK);
    }
    false
}

/// Adds working time to a moment, skipping non-working time.
///
/// # What it does
/// Starting at `start`, walks forward through the calendar and counts only
/// working minutes until `duration` is used up. It stops at the end of the
/// last minute used, so a task that fills Tuesday finishes at Tuesday 17:00
/// (MS Project convention).
///
/// # Worked example (Standard calendar)
/// ```text
/// start Wed 3 Mar 2027 08:00, duration 5 days = 2 400 minutes
///   Wed  480 used  (1 920 left)
///   Thu  480 used  (1 440 left)
///   Fri  480 used  (  960 left)
///   Sat, Sun: no working time
///   Mon  480 used  (  480 left)
///   Tue  480 used  (    0 left)  -> Tue 9 Mar 2027 17:00
/// ```
/// This is task 2 of the book's shed project (section 2.3).
///
/// # Parameters
/// - `calendar`: defines which hours are working time.
/// - `start`: the moment we start counting from. It does not need to be a
///   working moment; non-working time before the first block is skipped.
/// - `duration`: how much working time to add. A negative duration is
///   handed to [`subtract_working_time`]. Zero returns `start` unchanged.
///
/// # Returns
/// The moment when the work would be finished.
///
/// # Errors
/// [`CalendarError::NoWorkingTime`] if the calendar has no working time;
/// [`CalendarError::DateOutOfRange`] for absurdly long durations.
///
/// # Example
/// ```
/// use engine::calendar_math::add_working_time;
/// use model::{Calendar, Duration};
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let mon_10 = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap().and_hms_opt(10, 0, 0).unwrap();
/// let mon_15 = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap().and_hms_opt(15, 0, 0).unwrap();
/// // 4 hours from 10:00 skip the lunch break and end at 15:00.
/// assert_eq!(add_working_time(&cal, mon_10, Duration::from_hours(4)).unwrap(), mon_15);
/// ```
///
/// # Beginner note
/// The `&` in `&Calendar` means we *borrow* the calendar: we can read it,
/// but we do not take ownership of it, so the caller can keep using it.
pub fn add_working_time(
    calendar: &Calendar,
    start: NaiveDateTime,
    duration: Duration,
) -> Result<NaiveDateTime, CalendarError> {
    if duration.minutes() < 0 {
        return subtract_working_time(calendar, start, Duration::from_minutes(-duration.minutes()));
    }
    if duration.is_zero() {
        return Ok(start);
    }
    ensure_working_time(calendar)?;

    // `remaining` counts down the working minutes still to be placed.
    let mut remaining = duration.minutes();
    let mut date = start.date();
    let mut minute = minute_of_day(start);
    loop {
        for block in blocks_on(calendar, date) {
            // Work in this block can begin at its start, or at our cursor if
            // the cursor is already past the start.
            let from = minute.max(block.start_minute);
            if from >= block.end_minute {
                continue; // this block is already behind us
            }
            let available = i64::from(block.end_minute - from);
            if remaining <= available {
                // The work ends inside this block. `remaining` is at most
                // 1440 here, so converting it to u32 cannot fail.
                let end = from + u32::try_from(remaining).unwrap_or(0);
                return Ok(at_minute(date, end));
            }
            remaining -= available;
        }
        date = next_day(date)?;
        minute = 0;
    }
}

/// Subtracts working time from a moment, skipping non-working time.
///
/// # What it does
/// The mirror image of [`add_working_time`]: starting at `end`, walks
/// *backward* and counts working minutes until `duration` is used up. It
/// stops at the start of the earliest minute used. The backward pass uses it
/// to find a late start from a late finish.
///
/// # Worked example (Standard calendar)
/// ```text
/// end Tue 23 Mar 2027 17:00, duration 2 days = 960 minutes
///   Tue  480 used (480 left)
///   Mon  480 used (  0 left)  -> Mon 22 Mar 2027 08:00
/// ```
/// This is the late start of "Paint" in the shed project (section 2.4).
///
/// # Parameters
/// - `calendar`: the working-time calendar.
/// - `end`: the moment we count back from.
/// - `duration`: how much working time to subtract. Negative is handed to
///   [`add_working_time`]; zero returns `end` unchanged.
///
/// # Returns
/// The moment at which the work would have to start.
///
/// # Errors
/// As for [`add_working_time`].
///
/// # Example
/// ```
/// use engine::calendar_math::subtract_working_time;
/// use model::{Calendar, Duration};
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let tue_17 = NaiveDate::from_ymd_opt(2027, 3, 23).unwrap().and_hms_opt(17, 0, 0).unwrap();
/// let mon_08 = NaiveDate::from_ymd_opt(2027, 3, 22).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// assert_eq!(subtract_working_time(&cal, tue_17, Duration::from_days(2)).unwrap(), mon_08);
/// ```
pub fn subtract_working_time(
    calendar: &Calendar,
    end: NaiveDateTime,
    duration: Duration,
) -> Result<NaiveDateTime, CalendarError> {
    if duration.minutes() < 0 {
        return add_working_time(calendar, end, Duration::from_minutes(-duration.minutes()));
    }
    if duration.is_zero() {
        return Ok(end);
    }
    ensure_working_time(calendar)?;

    let mut remaining = duration.minutes();
    let mut date = end.date();
    let mut minute = minute_of_day(end);
    loop {
        for block in blocks_on(calendar, date).iter().rev() {
            // Work in this block can end at its end, or at our cursor if the
            // cursor is before the block's end.
            let to = minute.min(block.end_minute);
            if to <= block.start_minute {
                continue; // this block is still ahead of us
            }
            let available = i64::from(to - block.start_minute);
            if remaining <= available {
                let start = to - u32::try_from(remaining).unwrap_or(0);
                return Ok(at_minute(date, start));
            }
            remaining -= available;
        }
        date = previous_day(date)?;
        minute = MINUTES_PER_DAY_CLOCK;
    }
}

/// Counts the working minutes between two moments.
///
/// # What it does
/// Adds up, day by day, how much of each working block lies between `from`
/// and `to`. If `to` is earlier than `from`, the answer is negative (the
/// same amount with a minus sign). Slack is calculated with this function.
///
/// # Worked example (Standard calendar)
/// ```text
/// from Wed 3 Mar 2027 08:00 to Tue 9 Mar 2027 08:00
///   Wed 480 + Thu 480 + Fri 480 + (Sat, Sun 0) + Mon 480 = 1 920 minutes
///   = 4 days of 480 minutes  -> the 4 days of slack of "Buy materials"
/// ```
///
/// # Parameters
/// - `calendar`: the working-time calendar.
/// - `from`: the first moment.
/// - `to`: the second moment.
///
/// # Returns
/// The working time between them (negative if `to` < `from`).
///
/// # Errors / edge cases
/// None. The loop runs once per calendar day between the two moments, so it
/// always ends.
///
/// # Example
/// ```
/// use engine::calendar_math::working_minutes_between;
/// use model::{Calendar, Duration};
/// use chrono::NaiveDate;
/// let cal = Calendar::standard();
/// let fri_17 = NaiveDate::from_ymd_opt(2027, 3, 5).unwrap().and_hms_opt(17, 0, 0).unwrap();
/// let mon_08 = NaiveDate::from_ymd_opt(2027, 3, 8).unwrap().and_hms_opt(8, 0, 0).unwrap();
/// // Over the weekend no working time passes at all.
/// assert_eq!(working_minutes_between(&cal, fri_17, mon_08), Duration::ZERO);
/// ```
pub fn working_minutes_between(
    calendar: &Calendar,
    from: NaiveDateTime,
    to: NaiveDateTime,
) -> Duration {
    if to < from {
        let positive = working_minutes_between(calendar, to, from);
        return Duration::from_minutes(-positive.minutes());
    }
    let mut total: i64 = 0;
    let mut date = from.date();
    let last_date = to.date();
    while date <= last_date {
        // On the first day we count from `from`'s minute, on the last day up
        // to `to`'s minute; on days in between, the whole day.
        let low = if date == from.date() {
            minute_of_day(from)
        } else {
            0
        };
        let high = if date == last_date {
            minute_of_day(to)
        } else {
            MINUTES_PER_DAY_CLOCK
        };
        for block in blocks_on(calendar, date) {
            let start = low.max(block.start_minute);
            let end = high.min(block.end_minute);
            if end > start {
                total += i64::from(end - start);
            }
        }
        match date.succ_opt() {
            Some(next) => date = next,
            None => break, // end of the representable calendar
        }
    }
    Duration::from_minutes(total)
}
