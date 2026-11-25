//! # Save, then open: the project must come back unchanged
//!
//! ## What this file checks
//! For several kinds of projects (the garden shed, an outline with levels,
//! holidays and several calendars, all link types and leads, unusual
//! settings, an empty project), saving to a `.gtra` file and opening it again
//! gives a project equal to the original (`==`), with links and calendars
//! compared as sets. It also checks the extra information `open` returns
//! (access, format version, notes, author and dates) and that invalid
//! projects are refused before anything is written.
//!
//! ## Where it fits
//! Part of the `gtra` test target (`tests/rust/gtra/main.rs`).

use crate::{files_in, normalized, open_ok, path_in, save_ok, temp_dir};
use ganttora_tests::{add_task, date, days, hours, link, project_on, row_id, shed};
use gtra::schema::CURRENT_FORMAT_VERSION;
use gtra::{Access, GtraError};
use model::{Calendar, Dependency, DependencyType, Duration, Project, Task, WorkWeek, WorkingTime};

/// Saves and opens a project and checks it came back equal.
///
/// # Parameters
/// - `project`: the project to round-trip.
///
/// # Returns
/// The opened project, for further checks.
fn round_trip(project: &Project) -> gtra::OpenedProject {
    let dir = temp_dir();
    let path = path_in(&dir, "plan.gtra");
    save_ok(&path, project);
    let opened = open_ok(&path);
    assert_eq!(normalized(&opened.project), normalized(project));
    opened
}

/// The garden shed survives a save and an open unchanged, and `open` reports
/// a normal, writable, current-version file with no notes.
///
/// Why it matters: this is the most basic promise of a file format: what you
/// save is what you get back.
#[test]
fn shed_round_trip() {
    let project = shed();
    let opened = round_trip(&project);
    assert_eq!(opened.access, Access::ReadWrite);
    assert_eq!(opened.format_version, CURRENT_FORMAT_VERSION);
    assert_eq!(opened.upgraded_from, None);
    assert!(opened.notes.is_empty(), "notes: {:?}", opened.notes);
}

/// For the shed, even the link order comes back the same (the reader sorts
/// links by row), so plain `==` holds without normalizing.
///
/// Why it matters: a stable order means a saved-and-reopened project gives a
/// byte-identical snapshot to the UI.
#[test]
fn shed_round_trip_exact_order() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    let project = shed();
    save_ok(&path, &project);
    assert_eq!(open_ok(&path).project, project);
}

/// The file remembers who saved it and when.
///
/// Why it matters: "last saved by Anna on 15 Feb" is shown in the file
/// properties and used for conflict warnings on shared folders.
#[test]
fn file_info_after_first_save() {
    let dir = temp_dir();
    let path = path_in(&dir, "shed.gtra");
    save_ok(&path, &shed());
    let info = open_ok(&path).info;
    assert_eq!(info.author.as_deref(), Some("Tester"));
    assert_eq!(info.created.as_deref(), Some("2027-02-15T10:00:00Z"));
    assert_eq!(info.modified.as_deref(), Some("2027-02-15T10:00:00Z"));
    assert_eq!(info.app_version.as_deref(), Some(gtra::APP_VERSION));
}

/// Outline levels 1, 2, 3, 2, 1, 2 (a small work breakdown) come back
/// exactly. The file stores a parent and a position for each task; the reader
/// must rebuild the same rows and levels.
///
/// Why it matters: if the outline came back wrong, summary tasks would
/// suddenly contain the wrong subtasks.
#[test]
fn outline_levels_round_trip() {
    let mut project = project_on("2027-03-01");
    for (name, level) in [
        ("Phase A", 1),
        ("A.1", 2),
        ("A.1.a", 3),
        ("A.2", 2),
        ("Phase B", 1),
        ("B.1", 2),
    ] {
        let mut task = Task::new(name, days(1));
        task.outline_level = level;
        project.tasks.push(task);
    }
    let opened = round_trip(&project);
    let levels: Vec<u32> = opened
        .project
        .tasks
        .iter()
        .map(|t| t.outline_level)
        .collect();
    assert_eq!(levels, vec![1, 2, 3, 2, 1, 2]);
}

/// An outline that jumps a level (first task at level 2, or level 1 -> 3) is
/// refused with `InvalidProject`, and no file is created.
///
/// Why it matters: such an outline cannot be stored as parent/child rows; it
/// must be caught before writing, not produce a broken file.
#[test]
fn invalid_outline_is_refused_before_writing() {
    for levels in [vec![2], vec![1, 3]] {
        let mut project = project_on("2027-03-01");
        for (i, level) in levels.iter().enumerate() {
            let mut task = Task::new(format!("T{i}"), days(1));
            task.outline_level = *level;
            project.tasks.push(task);
        }
        let dir = temp_dir();
        let path = path_in(&dir, "bad.gtra");
        let result = gtra::save(&path, &project, &crate::options());
        assert!(
            matches!(result, Err(GtraError::InvalidProject(_))),
            "levels {levels:?}: got {result:?}"
        );
        assert!(files_in(dir.path()).is_empty(), "nothing may be written");
    }
}

/// Two calendars (a standard one with holidays, including two days in a row
/// and one on a weekend, and a night-shift calendar with Saturday work up to
/// midnight) come back with the same weeks and the same holidays.
///
/// Why it matters: calendars decide every date. A lost holiday would silently
/// move the whole plan by a day.
#[test]
fn holidays_and_multiple_calendars_round_trip() {
    let mut project = shed();
    let standard = &mut project.calendars[0];
    for day in ["2027-03-08", "2027-03-09", "2027-03-13", "2027-12-25"] {
        standard.holidays.insert(date(day));
    }
    let night_shift = WorkingTime::new(22 * 60, 24 * 60);
    let mut week = WorkWeek::standard();
    week.saturday = vec![WorkingTime::hours(8, 12), night_shift];
    let mut second = Calendar::new("Night shift", week);
    second.holidays.insert(date("2027-04-05"));
    project.calendars.push(second);

    let opened = round_trip(&project);
    assert_eq!(opened.project.calendars.len(), 2);
    let night = opened
        .project
        .calendars
        .iter()
        .find(|c| c.name == "Night shift")
        .expect("second calendar");
    assert_eq!(night.week.saturday.last(), Some(&night_shift));
}

/// The project calendar can be any calendar of the list, not only the first.
///
/// Why it matters: the reader sorts calendars by name; the project must still
/// point at the right one by ID.
#[test]
fn project_calendar_other_than_first_round_trip() {
    let mut project = shed();
    let mut other = Calendar::new("AAA first by name", WorkWeek::standard());
    other.week.friday.clear();
    project.calendar_id = other.id;
    project.calendars.push(other);
    let opened = round_trip(&project);
    assert_eq!(opened.project.calendar_id, project.calendar_id);
}

/// All four link types and a lead (negative lag) come back unchanged.
///
/// Why it matters: the engine does not support every type yet (M0), but the
/// file must already store them, so files from later versions or from MS
/// Project are not damaged.
#[test]
fn link_types_and_lags_round_trip() {
    let mut project = project_on("2027-03-01");
    let ids: Vec<_> = (0..5)
        .map(|i| add_task(&mut project, &format!("T{i}"), days(2)))
        .collect();
    let kinds = [
        DependencyType::FinishToStart,
        DependencyType::StartToStart,
        DependencyType::FinishToFinish,
        DependencyType::StartToFinish,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let lag = match i {
            0 => hours(-4),
            1 => days(1),
            _ => Duration::ZERO,
        };
        project
            .dependencies
            .push(Dependency::new(ids[i], ids[i + 1], kind).with_lag(lag));
    }
    round_trip(&project);
}

/// Unusual but valid values survive: a 7-hour day (420 minutes), a start
/// time in the afternoon, names with non-English letters, quotes and emoji,
/// an empty task name, a zero-length task (milestone) and a very long task.
///
/// Why it matters: users type anything; a file format must never mangle text
/// or round numbers.
#[test]
fn unusual_values_round_trip() {
    let mut project = Project::new("Šiltnamis «Дача» 温室 \"quoted\" 🌱", date("2027-03-01"));
    project.start = ganttora_tests::dt("2027-03-01 13:30");
    project.minutes_per_day = 420;
    add_task(
        &mut project,
        "Ąžuolas ёлка 木 ' \" ; DROP TABLE tasks; --",
        days(1),
    );
    add_task(&mut project, "", Duration::ZERO);
    add_task(&mut project, "Long", Duration::from_minutes(480 * 2000));
    let a = row_id(&project, 1);
    let b = row_id(&project, 3);
    link(&mut project, a, b);
    round_trip(&project);
}

/// A project with no tasks and no links can be saved and opened.
///
/// Why it matters: "File > New" then "Save" is the first thing a user does.
#[test]
fn empty_project_round_trip() {
    let opened = round_trip(&project_on("2027-03-01"));
    assert!(opened.project.tasks.is_empty());
}

/// Projects the file cannot represent are refused with `InvalidProject`
/// before anything is written: two tasks with the same ID, a link to a task
/// that does not exist, a project calendar missing from the list, a day of
/// zero minutes, two links between the same tasks.
///
/// Why it matters: the database rules would reject these halfway through
/// writing; checking first gives a clear message and keeps the disk clean.
#[test]
fn invalid_projects_are_refused_before_writing() {
    let mut cases: Vec<(&str, Project)> = Vec::new();

    let mut duplicate_task = shed();
    let copy = duplicate_task.tasks[0].clone();
    duplicate_task.tasks.push(copy);
    cases.push(("duplicate task id", duplicate_task));

    let mut dangling = shed();
    let first = row_id(&dangling, 1);
    dangling
        .dependencies
        .push(Dependency::finish_to_start(first, model::TaskId::new()));
    cases.push(("link to missing task", dangling));

    let mut no_calendar = shed();
    no_calendar.calendar_id = model::CalendarId::new();
    cases.push(("missing project calendar", no_calendar));

    let mut zero_day = shed();
    zero_day.minutes_per_day = 0;
    cases.push(("zero minutes per day", zero_day));

    let mut double_link = shed();
    let (one, two) = (row_id(&double_link, 1), row_id(&double_link, 2));
    double_link
        .dependencies
        .push(Dependency::finish_to_start(one, two));
    cases.push(("two links between the same tasks", double_link));

    let mut self_link = shed();
    // Each shed() has fresh random IDs, so take row 1 of *this* project.
    let one = row_id(&self_link, 1);
    self_link
        .dependencies
        .push(Dependency::finish_to_start(one, one));
    cases.push(("self link", self_link));

    let mut negative = shed();
    negative.tasks[0].duration = Duration::from_minutes(-1);
    cases.push(("negative duration", negative));

    let mut bad_hours = shed();
    bad_hours.calendars[0].week.monday = vec![WorkingTime::new(600, 500)];
    cases.push(("working time that ends before it starts", bad_hours));

    for (what, project) in cases {
        let dir = temp_dir();
        let path = path_in(&dir, "bad.gtra");
        let result = gtra::save(&path, &project, &crate::options());
        assert!(
            matches!(result, Err(GtraError::InvalidProject(_))),
            "{what}: got {result:?}"
        );
        assert!(files_in(dir.path()).is_empty(), "{what}: nothing written");
    }
}

/// Saving the same project twice into two files gives the same project when
/// opened.
///
/// Why it matters: determinism; saving must not invent or reorder data.
#[test]
fn saving_twice_gives_the_same_result() {
    let dir = temp_dir();
    let project = shed();
    let a = path_in(&dir, "a.gtra");
    let b = path_in(&dir, "b.gtra");
    save_ok(&a, &project);
    save_ok(&b, &project);
    assert_eq!(open_ok(&a).project, open_ok(&b).project);
}
