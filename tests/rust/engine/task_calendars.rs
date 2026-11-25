//! # Task calendar and calendar exception tests
//!
//! ## What this file checks
//! - A **task calendar** replaces the project calendar for one task (for
//!   example a crew that works Monday to Thursday only).
//! - A **calendar exception** changes the working times for a range of
//!   dates: a non-working exception (a company holiday week) or a working
//!   exception (a Saturday that is worked).
//! - Which rule applies on a date: **an exception wins, then the holiday
//!   list, then the normal week**. When two exceptions overlap, **the first
//!   one in the list wins** (M1 rule, to be compared with MS Project later).
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). Book
//! section 2.3 explains calendars.
//!
//! ## Main ideas
//! Standard calendar: Mon–Fri 08:00–12:00 and 13:00–17:00 (8 hours). Project
//! start Monday 1 March 2027 unless a test says otherwise.

use crate::{dates_of, schedule_ok};
use ganttora_tests::{add_task, date, days, dt, project_on};
use model::{
    Calendar, CalendarException, Dependency, DependencyType, Duration, Project, Task, WorkWeek,
    WorkingTime,
};

/// The standard 8-hour blocks of one day (08–12, 13–17).
fn standard_day() -> Vec<WorkingTime> {
    vec![WorkingTime::hours(8, 12), WorkingTime::hours(13, 17)]
}

/// A Monday-to-Thursday week (Friday off), standard hours.
fn four_day_week() -> WorkWeek {
    let mut week = WorkWeek::standard();
    week.friday = Vec::new();
    week
}

/// A task with a Mon–Thu calendar, starting Thu 4 Mar, needs 2 days: Thu 4,
/// then (Friday off, weekend) Mon 8 → finish Mon 8 17:00. A task on the
/// project calendar finishes Fri 5 17:00.
///
/// Why it matters: each task must be counted on its own calendar.
#[test]
fn task_calendar_replaces_the_project_calendar() {
    let mut project = project_on("2027-03-04");
    let crew = Calendar::new("Four-day crew", four_day_week());
    let crew_id = crew.id;
    project.calendars.push(crew);
    let a = add_task(&mut project, "Crew task", days(2));
    let b = add_task(&mut project, "Normal task", days(2));
    project.tasks[0].calendar_id = Some(crew_id);
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-08 17:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-05 17:00"));
    // "Why this date?" names the calendar the task was counted on.
    assert_eq!(dates_of(&result, a).explanation.calendar_id, crew_id);
    assert_eq!(
        dates_of(&result, b).explanation.calendar_id,
        project.calendar_id
    );
}

/// A holiday on the task calendar only (Tue 2 Mar): the task calendar's task
/// (2 days) works Mon 1 and Wed 3 → finish Wed 3 17:00; the project-calendar
/// task is not affected (Tue 2 17:00).
///
/// Why it matters: holidays belong to a calendar, not to the whole project.
#[test]
fn holiday_on_task_calendar_only() {
    let mut project = project_on("2027-03-01");
    let mut site = Calendar::new("Site", WorkWeek::standard());
    site.holidays.insert(date("2027-03-02"));
    let site_id = site.id;
    project.calendars.push(site);
    let a = add_task(&mut project, "On site", days(2));
    let b = add_task(&mut project, "In office", days(2));
    project.tasks[0].calendar_id = Some(site_id);
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-03 17:00"));
    assert_eq!(dates_of(&result, b).finish, dt("2027-03-02 17:00"));
}

/// A non-working exception Tue 2 – Thu 4 Mar on the project calendar: a
/// 2-day task works Mon 1, then Fri 5 → finish Fri 5 17:00.
///
/// Why it matters: exceptions cover date *ranges* (a holiday week), not only
/// single days.
#[test]
fn non_working_exception_range_is_skipped() {
    let mut project = project_on("2027-03-01");
    project.calendars[0]
        .exceptions
        .push(CalendarException::non_working(
            "Trade fair",
            date("2027-03-02"),
            date("2027-03-04"),
        ));
    let a = add_task(&mut project, "A", days(2));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-05 17:00"));
}

/// A working exception on Saturday 6 Mar, 08:00–12:00: a 6-day task (2 880
/// minutes) works Mon–Fri (2 400), Saturday morning (240) and the remaining
/// 240 minutes on Mon 8 08:00–12:00 → finish Mon 8 12:00. Without the
/// exception it would finish Mon 8 17:00.
///
/// Why it matters: a working exception adds working time on a day that is
/// normally free.
#[test]
fn working_exception_adds_a_saturday_morning() {
    let mut project = project_on("2027-03-01");
    project.calendars[0]
        .exceptions
        .push(CalendarException::working(
            "Catch-up Saturday",
            date("2027-03-06"),
            date("2027-03-06"),
            vec![WorkingTime::hours(8, 12)],
        ));
    let a = add_task(&mut project, "A", days(6));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-08 12:00"));
}

/// Two overlapping exceptions on Saturday 6 Mar: the first one in the list
/// decides. Working first → Saturday morning works (finish Mon 8 12:00 as
/// above). Non-working first → Saturday is free (finish Mon 8 17:00).
///
/// Why it matters: the rule "first wins" must be applied the same way every
/// time; otherwise the same file could give two different schedules.
#[test]
fn overlapping_exceptions_first_one_wins() {
    let working = CalendarException::working(
        "Catch-up Saturday",
        date("2027-03-06"),
        date("2027-03-06"),
        vec![WorkingTime::hours(8, 12)],
    );
    let free = CalendarException::non_working("Weekend", date("2027-03-06"), date("2027-03-07"));

    let mut project = project_on("2027-03-01");
    project.calendars[0].exceptions = vec![working.clone(), free.clone()];
    let a = add_task(&mut project, "A", days(6));
    assert_eq!(
        dates_of(&schedule_ok(&project), a).finish,
        dt("2027-03-08 12:00")
    );

    project.calendars[0].exceptions = vec![free, working];
    assert_eq!(
        dates_of(&schedule_ok(&project), a).finish,
        dt("2027-03-08 17:00")
    );
}

/// An exception wins over the holiday list: Tue 2 Mar is a holiday *and* has
/// a working exception with normal hours. The exception wins, so a 2-day
/// task finishes Tue 2 17:00 as if there were no holiday.
///
/// Why it matters: this is the documented order (exception, then holiday,
/// then week); "work on the holiday after all" must be possible.
#[test]
fn exception_wins_over_holiday() {
    let mut project = project_on("2027-03-01");
    project.calendars[0].holidays.insert(date("2027-03-02"));
    project.calendars[0]
        .exceptions
        .push(CalendarException::working(
            "Holiday worked",
            date("2027-03-02"),
            date("2027-03-02"),
            standard_day(),
        ));
    let a = add_task(&mut project, "A", days(2));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-02 17:00"));
}

/// A working exception with shorter hours (Wed 3 Mar 08:00–12:00 only): a
/// 3-day task (1 440 min) works Mon (480), Tue (480), Wed morning (240) and
/// the last 240 minutes on Thu 4 08:00–12:00 → finish Thu 4 12:00.
///
/// Why it matters: an exception can also *shorten* a working day.
#[test]
fn working_exception_can_shorten_a_day() {
    let mut project = project_on("2027-03-01");
    project.calendars[0]
        .exceptions
        .push(CalendarException::working(
            "Half day",
            date("2027-03-03"),
            date("2027-03-03"),
            vec![WorkingTime::hours(8, 12)],
        ));
    let a = add_task(&mut project, "A", days(3));
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, a).finish, dt("2027-03-04 12:00"));
}

/// A six-day "Paint" ends on Saturday 13 March 2027; "Clean" (Mon–Fri) is
/// linked to it. Returns Clean's and Paint's dates.
fn after_a_saturday(
    kind: DependencyType,
    paint_days: i64,
    clean_days: i64,
) -> (engine::TaskSchedule, engine::TaskSchedule) {
    let mut p = Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 8).unwrap());
    let mut week = model::WorkWeek::standard();
    week.saturday = week.friday.clone();
    let six = Calendar::new("Six-day week", week);
    let mut paint = Task::new("Paint", Duration::from_days(paint_days));
    paint.calendar_id = Some(six.id);
    p.calendars.push(six);
    let clean = Task::new("Clean", Duration::from_days(clean_days));
    p.dependencies
        .push(Dependency::new(paint.id, clean.id, kind));
    let (clean_id, paint_id) = (clean.id, paint.id);
    p.tasks.extend([paint, clean]);
    let result = engine::schedule(&p).unwrap();
    (
        result.task(clean_id).unwrap().clone(),
        result.task(paint_id).unwrap().clone(),
    )
}

/// A finish-to-finish successor never ends before its predecessor, also
/// when that ends on the successor's day off; nobody gets negative slack.
#[test]
fn finish_to_finish_over_a_day_off_moves_on_to_monday() {
    for (days, start) in [(1, "2027-03-15 08:00:00"), (3, "2027-03-11 08:00:00")] {
        let (clean, paint) = after_a_saturday(DependencyType::FinishToFinish, 6, days);
        assert_eq!(paint.finish.to_string(), "2027-03-13 17:00:00");
        assert_eq!(clean.start.to_string(), start);
        assert_eq!(clean.finish.to_string(), "2027-03-15 17:00:00");
        assert_eq!(clean.total_slack, Duration::ZERO);
        assert!(paint.total_slack >= Duration::ZERO);
    }
}

/// A predecessor that ends on Friday 17:00 changes nothing, and a milestone
/// still goes to the next working moment.
#[test]
fn finish_to_finish_on_a_working_edge_is_unchanged() {
    let (clean, _) = after_a_saturday(DependencyType::FinishToFinish, 5, 1);
    assert_eq!(clean.finish.to_string(), "2027-03-12 17:00:00");
    let (milestone, _) = after_a_saturday(DependencyType::FinishToFinish, 6, 0);
    assert_eq!(milestone.finish.to_string(), "2027-03-15 08:00:00");
}

/// The mirror image: a Mon–Fri task that must start no later than one that
/// starts on Saturday must start by Friday morning.
#[test]
fn a_start_limit_on_a_day_off_moves_back_to_friday() {
    for kind in [DependencyType::StartToStart, DependencyType::StartToFinish] {
        let mut p = Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 8).unwrap());
        let mut week = WorkWeek::standard();
        week.saturday = week.friday.clone();
        // Starts (and, for SF, ends) within the Saturday.
        let six = Calendar::new("Six-day week", week);
        let mut b = Task::new("B", Duration::from_days(1));
        b.calendar_id = Some(six.id);
        p.calendars.push(six);
        b.constraint = model::Constraint::MustStartOn {
            date: dt("2027-03-13 08:00"),
        };
        let a = Task::new("A", Duration::from_days(1));
        let long = Task::new("Long", Duration::from_days(9));
        p.dependencies.push(Dependency::new(a.id, b.id, kind));
        // SF limits A's start to B's finish, Saturday 17:00: also a day off.
        p.tasks.extend([a, b, long]);
        let result = engine::schedule(&p).unwrap();
        assert_eq!(result.tasks[0].late_start, dt("2027-03-12 08:00"));
        assert_eq!(result.tasks[0].total_slack, Duration::from_days(4));
    }
}

/// "Finish no earlier than" a Saturday ends on Monday, from either end of
/// the project; a Friday date stays.
#[test]
fn finish_no_earlier_than_a_day_off_moves_on_to_monday() {
    for (date, days, finish) in [
        ("2027-03-13 17:00", 1, "2027-03-15 17:00"),
        ("2027-03-13 17:00", 3, "2027-03-15 17:00"),
        ("2027-03-13 17:00", 0, "2027-03-15 08:00"),
        ("2027-03-12 17:00", 1, "2027-03-12 17:00"),
    ] {
        let mut p = Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 8).unwrap());
        p.tasks.push(
            Task::new("T", Duration::from_days(days))
                .with_constraint(model::Constraint::FinishNoEarlierThan { date: dt(date) }),
        );
        let result = engine::schedule(&p).unwrap();
        assert_eq!(result.tasks[0].finish, dt(finish));
        assert!(result.tasks[0].total_slack >= Duration::ZERO);
        // Scheduled from a finish before the date, the date still wins.
        p.schedule_from = model::ScheduleFrom::Finish;
        p.finish = Some(dt("2027-03-11 17:00"));
        let result = engine::schedule(&p).unwrap();
        assert_eq!(result.tasks[0].finish, dt(finish));
    }
}

/// "Start no later than" a Saturday starts by Friday, even against a link;
/// "must finish on" a Saturday ends on Friday.
#[test]
fn upper_dates_on_a_day_off_stay_before_it() {
    use model::Constraint::{MustFinishOn, StartNoLaterThan};
    let saturday = dt("2027-03-13 08:00");
    let run = |days, constraint, linked: bool, from_finish: bool| {
        let mut p = Project::new("P", chrono::NaiveDate::from_ymd_opt(2027, 3, 8).unwrap());
        let before = Task::new("Before", Duration::from_days(5)); // ends Fri 12 Mar
        let task = Task::new("T", Duration::from_days(days)).with_constraint(constraint);
        if linked {
            p.dependencies
                .push(Dependency::finish_to_start(before.id, task.id));
        }
        p.tasks.extend([before, task]);
        if from_finish {
            p.schedule_from = model::ScheduleFrom::Finish;
            p.finish = Some(dt("2027-03-31 17:00"));
        }
        engine::schedule(&p).unwrap().tasks[1].clone()
    };
    let snlt = StartNoLaterThan { date: saturday };
    // Pushed by the link to Monday, the date holds it on Friday.
    assert_eq!(run(1, snlt, true, false).start, dt("2027-03-12 08:00"));
    assert_eq!(run(0, snlt, true, false).start, dt("2027-03-12 17:00"));
    // Late dates, and scheduling from the finish.
    assert_eq!(run(1, snlt, false, true).start, dt("2027-03-12 08:00"));
    let long = run(1, snlt, false, false);
    assert!(long.late_start <= dt("2027-03-12 08:00"));
    // A date on a working day is kept.
    let friday = StartNoLaterThan {
        date: dt("2027-03-12 08:00"),
    };
    assert_eq!(run(1, friday, true, false).start, dt("2027-03-12 08:00"));
    let must = MustFinishOn {
        date: dt("2027-03-13 17:00"),
    };
    assert_eq!(run(1, must, false, false).finish, dt("2027-03-12 17:00"));
}
