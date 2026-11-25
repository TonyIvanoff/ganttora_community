//! # Calendar arithmetic tests (`engine::calendar_math`)
//!
//! ## What this file checks
//! The small functions every schedule is built on: "add 2 working hours to
//! Monday 16:00" (answer: Tuesday 09:00), "subtract", "count the working
//! minutes between two moments", "move to the next working moment". The
//! tests cover the lunch break, nights, weekends, holidays and the error
//! cases (a calendar with no working time, dates beyond the year 262,000).
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). The
//! functions are public in `engine::calendar_math`.
//!
//! ## Main ideas
//! All examples use the standard calendar: Monday to Friday, 08:00–12:00 and
//! 13:00–17:00. March 2027: Mon 1, Fri 5, Sat 6, Sun 7, Mon 8.
//! At the end there are *round-trip* checks: instead of one example, they
//! try many start moments and durations and check a rule that must always
//! hold, such as "counting the time we just added gives back that time".

use chrono::{NaiveDate, NaiveTime, Weekday};
use engine::calendar_math::{
    CalendarError, add_working_time, is_working_day, snap_backward, snap_forward,
    subtract_working_time, touches_working_time, validate_calendar, working_minutes_between,
};
use ganttora_tests::{date, days, dt, hours};
use model::{Calendar, Duration, WorkWeek, WorkingTime};

/// The standard calendar, as every new project gets it.
fn standard() -> Calendar {
    Calendar::standard()
}

/// The standard calendar with Friday 5 March 2027 as a holiday.
fn with_friday_holiday() -> Calendar {
    let mut calendar = Calendar::standard();
    calendar.holidays.insert(date("2027-03-05"));
    calendar
}

/// A calendar where nobody ever works.
fn no_work() -> Calendar {
    let empty = WorkWeek {
        monday: vec![],
        tuesday: vec![],
        wednesday: vec![],
        thursday: vec![],
        friday: vec![],
        saturday: vec![],
        sunday: vec![],
    };
    Calendar::new("Never", empty)
}

// ---------------------------------------------------------------- adding

/// Adding 8 hours to Monday 08:00 ends at Monday 17:00: one full day,
/// lunch hour not counted.
///
/// Why it matters: this is "1 day = 8 working hours" at its simplest.
#[test]
fn add_one_day_from_morning() {
    let end = add_working_time(&standard(), dt("2027-03-01 08:00"), hours(8)).unwrap();
    assert_eq!(end, dt("2027-03-01 17:00"));
}

/// Monday 11:00 + 2 hours = Monday 14:00 (11–12, then 13–14).
///
/// Why it matters: the lunch break must be skipped in the middle of the day.
#[test]
fn add_across_lunch() {
    let end = add_working_time(&standard(), dt("2027-03-01 11:00"), hours(2)).unwrap();
    assert_eq!(end, dt("2027-03-01 14:00"));
}

/// Monday 16:00 + 2 hours = Tuesday 09:00 (16–17, then 08–09).
///
/// Why it matters: the night must be skipped.
#[test]
fn add_across_night() {
    let end = add_working_time(&standard(), dt("2027-03-01 16:00"), hours(2)).unwrap();
    assert_eq!(end, dt("2027-03-02 09:00"));
}

/// Friday 16:00 + 2 hours = Monday 09:00.
///
/// Why it matters: the weekend must be skipped.
#[test]
fn add_across_weekend() {
    let end = add_working_time(&standard(), dt("2027-03-05 16:00"), hours(2)).unwrap();
    assert_eq!(end, dt("2027-03-08 09:00"));
}

/// Thursday 16:00 + 2 hours, with Friday a holiday = Monday 09:00.
///
/// Why it matters: holidays are skipped like weekends.
#[test]
fn add_across_holiday_and_weekend() {
    let end = add_working_time(&with_friday_holiday(), dt("2027-03-04 16:00"), hours(2)).unwrap();
    assert_eq!(end, dt("2027-03-08 09:00"));
}

/// Adding from a non-working moment starts counting at the next working
/// moment: Saturday 10:00 + 1 hour = Monday 09:00.
///
/// Why it matters: the start of a count is not always in working time.
#[test]
fn add_from_a_weekend_moment() {
    let end = add_working_time(&standard(), dt("2027-03-06 10:00"), hours(1)).unwrap();
    assert_eq!(end, dt("2027-03-08 09:00"));
}

/// Adding 5 days to Monday 08:00 ends Friday 17:00; 10 days end the
/// following Friday 17:00.
///
/// Why it matters: whole-day durations across several weeks.
#[test]
fn add_whole_weeks() {
    let calendar = standard();
    let start = dt("2027-03-01 08:00");
    assert_eq!(
        add_working_time(&calendar, start, days(5)).unwrap(),
        dt("2027-03-05 17:00")
    );
    assert_eq!(
        add_working_time(&calendar, start, days(10)).unwrap(),
        dt("2027-03-12 17:00")
    );
}

/// Adding zero returns the start unchanged, even when the start is not
/// working time (Saturday 10:00).
///
/// Why it matters: milestones have zero duration; their moment must not be
/// moved by the arithmetic.
#[test]
fn add_zero_returns_the_start_unchanged() {
    let saturday = dt("2027-03-06 10:00");
    assert_eq!(
        add_working_time(&standard(), saturday, Duration::ZERO).unwrap(),
        saturday
    );
}

/// Adding a negative duration is the same as subtracting it.
///
/// Why it matters: leads are negative lags; the forward pass simply "adds"
/// them.
#[test]
fn add_negative_is_subtract() {
    let calendar = standard();
    let from = dt("2027-03-08 09:00");
    assert_eq!(
        add_working_time(&calendar, from, hours(-2)).unwrap(),
        subtract_working_time(&calendar, from, hours(2)).unwrap()
    );
}

// ---------------------------------------------------------------- subtracting

/// Monday 14:00 − 2 hours = Monday 11:00 (back over lunch).
///
/// Why it matters: the backward pass counts back in time and must skip
/// lunch the same way.
#[test]
fn subtract_across_lunch() {
    let start = subtract_working_time(&standard(), dt("2027-03-01 14:00"), hours(2)).unwrap();
    assert_eq!(start, dt("2027-03-01 11:00"));
}

/// Tuesday 09:00 − 2 hours = Monday 16:00 (back over the night).
///
/// Why it matters: nights are skipped going backwards too.
#[test]
fn subtract_across_night() {
    let start = subtract_working_time(&standard(), dt("2027-03-02 09:00"), hours(2)).unwrap();
    assert_eq!(start, dt("2027-03-01 16:00"));
}

/// Monday 09:00 − 2 hours = Friday 16:00; with Friday a holiday, Thursday
/// 16:00.
///
/// Why it matters: weekends and holidays are skipped going backwards.
#[test]
fn subtract_across_weekend_and_holiday() {
    let from = dt("2027-03-08 09:00");
    assert_eq!(
        subtract_working_time(&standard(), from, hours(2)).unwrap(),
        dt("2027-03-05 16:00")
    );
    assert_eq!(
        subtract_working_time(&with_friday_holiday(), from, hours(2)).unwrap(),
        dt("2027-03-04 16:00")
    );
}

/// Monday 17:00 − 1 day = Monday 08:00.
///
/// Why it matters: "late start = late finish − duration" in the backward
/// pass, on whole days.
#[test]
fn subtract_one_day_from_evening() {
    let start = subtract_working_time(&standard(), dt("2027-03-01 17:00"), days(1)).unwrap();
    assert_eq!(start, dt("2027-03-01 08:00"));
}

/// Subtracting zero returns the moment unchanged; subtracting a negative
/// duration is the same as adding it.
///
/// Why it matters: same special cases as for adding, mirrored.
#[test]
fn subtract_zero_and_negative() {
    let calendar = standard();
    let sunday = dt("2027-03-07 12:00");
    assert_eq!(
        subtract_working_time(&calendar, sunday, Duration::ZERO).unwrap(),
        sunday
    );
    let from = dt("2027-03-01 16:00");
    assert_eq!(
        subtract_working_time(&calendar, from, hours(-2)).unwrap(),
        add_working_time(&calendar, from, hours(2)).unwrap()
    );
}

// ---------------------------------------------------------------- snapping

/// `snap_forward` moves a moment to the next working moment: Saturday →
/// Monday 08:00, 12:30 → 13:00, 17:00 → next day 08:00. A moment inside
/// working time (10:00) or at a block start (08:00) stays.
///
/// Why it matters: tasks may only *start* in working time.
#[test]
fn snap_forward_moves_to_next_working_moment() {
    let c = standard();
    assert_eq!(
        snap_forward(&c, dt("2027-03-06 10:00")).unwrap(),
        dt("2027-03-08 08:00")
    );
    assert_eq!(
        snap_forward(&c, dt("2027-03-01 12:30")).unwrap(),
        dt("2027-03-01 13:00")
    );
    assert_eq!(
        snap_forward(&c, dt("2027-03-01 17:00")).unwrap(),
        dt("2027-03-02 08:00")
    );
    assert_eq!(
        snap_forward(&c, dt("2027-03-01 10:00")).unwrap(),
        dt("2027-03-01 10:00")
    );
    assert_eq!(
        snap_forward(&c, dt("2027-03-01 08:00")).unwrap(),
        dt("2027-03-01 08:00")
    );
}

/// `snap_backward` moves a moment to the previous working moment: Monday
/// 08:00 → Friday 17:00, 12:30 → 12:00, Sunday → Friday 17:00. Monday 17:00
/// (the end of work) stays.
///
/// Why it matters: tasks may only *finish* in working time; used by the
/// backward pass.
#[test]
fn snap_backward_moves_to_previous_working_moment() {
    let c = standard();
    assert_eq!(
        snap_backward(&c, dt("2027-03-08 08:00")).unwrap(),
        dt("2027-03-05 17:00")
    );
    assert_eq!(
        snap_backward(&c, dt("2027-03-01 12:30")).unwrap(),
        dt("2027-03-01 12:00")
    );
    assert_eq!(
        snap_backward(&c, dt("2027-03-07 15:00")).unwrap(),
        dt("2027-03-05 17:00")
    );
    assert_eq!(
        snap_backward(&c, dt("2027-03-01 17:00")).unwrap(),
        dt("2027-03-01 17:00")
    );
}

/// `touches_working_time` is true inside a block and at its edges (12:00,
/// 17:00), false in the lunch break and on weekends.
///
/// Why it matters: this decides whether a milestone may stay where it is.
#[test]
fn touches_working_time_includes_block_edges() {
    let c = standard();
    assert!(touches_working_time(&c, dt("2027-03-01 10:00")));
    assert!(touches_working_time(&c, dt("2027-03-01 12:00")));
    assert!(touches_working_time(&c, dt("2027-03-01 17:00")));
    assert!(!touches_working_time(&c, dt("2027-03-01 12:30")));
    assert!(!touches_working_time(&c, dt("2027-03-06 10:00")));
}

/// With a 24-hour calendar, midnight is the end ("24:00") of the day before,
/// so it touches working time even when the new day is not worked.
///
/// Why it matters: round-the-clock calendars (factories, servers) exist,
/// and midnight is a tricky edge.
#[test]
fn midnight_touches_a_block_ending_at_24_00() {
    let mut week = WorkWeek::standard();
    week.friday = vec![WorkingTime::new(0, 1440)];
    let calendar = Calendar::new("Night shift", week);
    assert!(validate_calendar(&calendar).is_ok());
    // Saturday 00:00 is the end of Friday's 00:00–24:00 block.
    assert!(touches_working_time(&calendar, dt("2027-03-06 00:00")));
}

/// `is_working_day`: Monday yes, Saturday no, a holiday no.
///
/// Why it matters: the Gantt chart will shade non-working days with it.
#[test]
fn is_working_day_knows_weekends_and_holidays() {
    let c = with_friday_holiday();
    assert!(is_working_day(&c, date("2027-03-01")));
    assert!(!is_working_day(&c, date("2027-03-06")));
    assert!(!is_working_day(&c, date("2027-03-05")));
}

// ---------------------------------------------------------------- counting

/// Counting working time: Mon 08:00→17:00 = 8h; Mon 08:00→Tue 08:00 = 8h;
/// Fri 08:00→Mon 17:00 = 16h; Sat→Sun = 0; with the Friday holiday,
/// Thu 08:00→Mon 17:00 = 16h.
///
/// Why it matters: slack is measured with this function.
#[test]
fn working_minutes_between_examples() {
    let c = standard();
    let count = |a, b| working_minutes_between(&c, dt(a), dt(b));
    assert_eq!(count("2027-03-01 08:00", "2027-03-01 17:00"), hours(8));
    assert_eq!(count("2027-03-01 08:00", "2027-03-02 08:00"), hours(8));
    assert_eq!(count("2027-03-05 08:00", "2027-03-08 17:00"), hours(16));
    assert_eq!(
        count("2027-03-06 00:00", "2027-03-07 23:59"),
        Duration::ZERO
    );
    assert_eq!(
        count("2027-03-01 10:00", "2027-03-01 10:00"),
        Duration::ZERO
    );
    let h = with_friday_holiday();
    assert_eq!(
        working_minutes_between(&h, dt("2027-03-04 08:00"), dt("2027-03-08 17:00")),
        hours(16)
    );
}

/// Counting backwards in time gives the same amount with a minus sign:
/// between(a, b) = −between(b, a), for many pairs of moments.
///
/// Why it matters: negative slack (M1, deadlines) relies on this symmetry.
#[test]
fn working_minutes_between_is_symmetric() {
    let c = with_friday_holiday();
    let moments = sample_moments();
    for &a in &moments {
        for &b in &moments {
            let forward = working_minutes_between(&c, a, b);
            let backward = working_minutes_between(&c, b, a);
            assert_eq!(forward.minutes(), -backward.minutes(), "{a} and {b}");
        }
    }
}

/// Counting is additive: between(a, c) = between(a, b) + between(b, c) for
/// moments a ≤ b ≤ c.
///
/// Why it matters: splitting a period in two must not lose or double-count
/// minutes, or slack values would drift.
#[test]
fn working_minutes_between_is_additive() {
    let c = with_friday_holiday();
    let moments = sample_moments(); // already sorted
    for (i, &a) in moments.iter().enumerate() {
        for (j, &b) in moments.iter().enumerate().skip(i) {
            for &m in moments.iter().skip(j) {
                let whole = working_minutes_between(&c, a, m).minutes();
                let parts = working_minutes_between(&c, a, b).minutes()
                    + working_minutes_between(&c, b, m).minutes();
                assert_eq!(whole, parts, "{a} .. {b} .. {m}");
            }
        }
    }
}

/// Round trip: for many start moments and durations, counting the working
/// time between the start and `add_working_time(start, d)` gives back `d`.
/// The same for `subtract_working_time`.
///
/// Why it matters: adding and counting are two views of the same calendar.
/// If they disagree anywhere, slack and dates would contradict each other.
#[test]
fn adding_then_counting_gives_back_the_duration() {
    let c = with_friday_holiday();
    let durations = [1, 30, 59, 60, 240, 241, 480, 481, 960, 2400, 4800];
    for &start in &sample_moments() {
        for &minutes in &durations {
            let d = Duration::from_minutes(minutes);
            let end = add_working_time(&c, start, d).unwrap();
            assert_eq!(
                working_minutes_between(&c, start, end),
                d,
                "{start} + {minutes}min"
            );
            let begin = subtract_working_time(&c, start, d).unwrap();
            assert_eq!(
                working_minutes_between(&c, begin, start),
                d,
                "{start} - {minutes}min"
            );
        }
    }
}

/// A list of test moments over two weeks around the Friday holiday, every
/// 30 minutes from 06:00 to 19:00, plus midnights. Sorted from early to late.
fn sample_moments() -> Vec<chrono::NaiveDateTime> {
    let mut moments = Vec::new();
    let first = date("2027-03-03");
    for day_offset in 0..7 {
        let day = first + chrono::Days::new(day_offset);
        moments.push(day.and_time(NaiveTime::MIN));
        for half_hour in 12..=38 {
            let minute = half_hour * 30;
            moments
                .push(day.and_time(NaiveTime::from_hms_opt(minute / 60, minute % 60, 0).unwrap()));
        }
    }
    moments
}

// ---------------------------------------------------------------- errors

/// `validate_calendar` accepts the standard calendar and a block that ends
/// exactly at 24:00, and rejects: a block whose start equals its end, a
/// block past midnight (end > 1440), and blocks out of order.
///
/// Why it matters: the rest of the arithmetic assumes sorted, non-empty,
/// in-day blocks. Bad calendars must be stopped at the door.
#[test]
fn validate_calendar_accepts_good_and_rejects_bad_blocks() {
    assert!(validate_calendar(&standard()).is_ok());

    let check = |blocks: Vec<WorkingTime>| {
        let mut week = WorkWeek::standard();
        week.wednesday = blocks;
        validate_calendar(&Calendar::new("Test", week))
    };
    assert!(check(vec![WorkingTime::new(1200, 1440)]).is_ok());
    for bad in [
        vec![WorkingTime::hours(9, 9)],
        vec![WorkingTime::new(600, 1500)],
        vec![WorkingTime::hours(13, 17), WorkingTime::hours(8, 12)],
    ] {
        assert!(
            matches!(
                check(bad.clone()),
                Err(CalendarError::InvalidWorkingTime {
                    weekday: Weekday::Wed,
                    ..
                })
            ),
            "{bad:?} should be rejected"
        );
    }
}

/// With a calendar that has no working time, `validate_calendar`,
/// `snap_forward`, `snap_backward` and `add_working_time` return
/// `NoWorkingTime` instead of looping forever; counting gives zero.
///
/// Why it matters: without this check, "find the next working moment" would
/// search forever and freeze the app.
#[test]
fn calendar_without_work_gives_an_error_not_an_endless_loop() {
    let c = no_work();
    let moment = dt("2027-03-01 08:00");
    let expected = CalendarError::NoWorkingTime { calendar: c.id };
    assert_eq!(validate_calendar(&c), Err(expected));
    assert_eq!(snap_forward(&c, moment), Err(expected));
    assert_eq!(snap_backward(&c, moment), Err(expected));
    assert_eq!(add_working_time(&c, moment, hours(1)), Err(expected));
    assert_eq!(subtract_working_time(&c, moment, hours(1)), Err(expected));
    assert_eq!(
        working_minutes_between(&c, moment, dt("2027-03-08 08:00")),
        Duration::ZERO
    );
}

/// Adding time on the very last day the date library can represent
/// (year 262,142) gives `DateOutOfRange`, and subtracting on the very first
/// day does too.
///
/// Why it matters: even absurd input must give a clean error, not a crash.
#[test]
fn dates_beyond_the_supported_range_give_an_error() {
    // Use a 7-day calendar so the last day is surely a working day.
    let mut week = WorkWeek::standard();
    week.saturday = week.monday.clone();
    week.sunday = week.monday.clone();
    let c = Calendar::new("Every day", week);

    let last = NaiveDate::MAX.and_time(NaiveTime::from_hms_opt(16, 0, 0).unwrap());
    assert_eq!(
        add_working_time(&c, last, hours(2)),
        Err(CalendarError::DateOutOfRange)
    );
    let first = NaiveDate::MIN.and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    assert_eq!(
        subtract_working_time(&c, first, hours(2)),
        Err(CalendarError::DateOutOfRange)
    );
}
