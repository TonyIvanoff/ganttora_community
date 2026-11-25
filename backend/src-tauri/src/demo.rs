//! demo.rs: builds the sample projects: "Garden shed" and "Garage".
//!
//! # What it is for
//! The app can open `.gtra` files (see `ipc/file.rs`), but no sample files
//! ship with it yet. To show a *real* schedule calculated by the engine at
//! start-up, without asking the user for a file, the shell needs a project
//! from somewhere. This file builds the running example of the book: the
//! garden shed, and [`sample_projects`]: a richer shed and
//! a garage that together show every feature (sub-tasks, risks, blockers…).
//!
//! # Why it lives in the shell
//! The same construction exists as a stand-alone program in
//! `backend/crates/engine/examples/shed.rs` and
//! `backend/crates/document/examples/shed_edit.rs`. Examples are separate
//! programs: other code cannot `use` them. So the shell keeps its own small
//! copy here. This is *not* scheduling logic: it only lists task names,
//! durations and links, exactly what a user would type. The engine (called
//! by the document) calculates all dates.
//!
//! When sample projects ship as real `.gtra` files in `assets/samples/`
//! (M1/M4), the demo command will open such a file and this file can go.
//!
//! # Where it fits
//! Called by the IPC command `project_new_demo` in `ipc/project.rs`.
//!

use chrono::{NaiveDate, NaiveDateTime, TimeDelta};
use model::{
    Blocker, BlockerId, Calendar, CalendarException, Constraint, Dependency, DependencyType,
    Duration, Project, Risk, RiskId, RiskPriority, RiskResponse, Task, TaskMode, WorkWeek,
    WorkingTime,
};

/// The tasks of the garden shed: (name, duration in working days), in row
/// order. Row 1 in the book is index 0 here.
///
/// The names are sample *data*, not user-interface text, so they are not in
/// `locales/en.json`. (Translated sample projects are a later topic.)
const SHED_TASKS: [(&str, i64); 8] = [
    ("Design the shed", 2),
    ("Get building permit", 5),
    ("Buy materials", 3),
    ("Prepare the ground", 2),
    ("Build the floor", 2),
    ("Build walls and roof", 4),
    ("Paint", 2),
    ("Shed finished", 0),
];

/// The links of the garden shed as (predecessor row, successor row), with
/// the book's row numbers (starting at 1). All are Finish-to-Start.
const SHED_LINKS: [(usize, usize); 8] = [
    (1, 2),
    (1, 3),
    (2, 4),
    (3, 5),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 8),
];

/// Builds the garden-shed project of the project documentation.
///
/// # What it does
/// Creates a project that starts on Tuesday 17 November 2026, adds the 8 tasks
/// and the 8 Finish-to-Start links:
///
/// | Row | Task | Duration | Predecessors |
/// |---|---|---|---|
/// | 1 | Design the shed | 2d | – |
/// | 2 | Get building permit | 5d | 1 |
/// | 3 | Buy materials | 3d | 1 |
/// | 4 | Prepare the ground | 2d | 2 |
/// | 5 | Build the floor | 2d | 3, 4 |
/// | 6 | Build walls and roof | 4d | 5 |
/// | 7 | Paint | 2d | 6 |
/// | 8 | Shed finished | 0d | 7 |
///
/// # Returns
/// A new [`Project`]. Every call gives new random UUIDs for the project,
/// the tasks and the links (IDs are never row numbers).
///
/// # Errors
/// None. The start date is a fixed valid date, so the `expect` below can
/// never fire.
///
/// # Example
/// The engine schedules it to finish on Wed 9 Dec 2026 at 17:00:
/// ```
/// let project = ganttora_lib::demo::garden_shed();
/// assert_eq!(project.tasks.len(), 8);
/// let result = engine::schedule(&project).unwrap();
/// assert_eq!(result.project_finish.to_string(), "2026-12-09 17:00:00");
/// ```
///
/// # Beginner note
/// `for (name, days) in SHED_TASKS` takes each pair out of the array and
/// gives its two parts names. This is called *destructuring*.
pub fn garden_shed() -> Project {
    let start = NaiveDate::from_ymd_opt(2026, 11, 17).expect("17 November 2026 is a valid date");
    let mut project = Project::new("Garden shed", start);

    for (name, days) in SHED_TASKS {
        project
            .tasks
            .push(Task::new(name, Duration::from_days(days)));
    }

    for (pred, succ) in SHED_LINKS {
        // Book row numbers start at 1, list positions at 0, hence `- 1`.
        let pred_id = project.tasks[pred - 1].id;
        let succ_id = project.tasks[succ - 1].id;
        project
            .dependencies
            .push(Dependency::finish_to_start(pred_id, succ_id));
    }
    project
}

/// Builds the sample file: the projects "Garden shed" and "Garage".
///
/// # What it does
/// Together the two projects show every feature. The dates are chosen so
/// that on **25 November 2026** (a Wednesday) some work is done, some is late and the rest
/// is still to come.
///
/// | Feature | Where |
/// |---|---|
/// | Project risk, note, hyperlink | both project rows |
/// | Sub-tasks, sub-sub-tasks | "Preparation", "Walls and roof" |
/// | All four link types | finish-start: most; start-start: "Cable trenches"; finish-finish: "Clean the site"; start-finish: "Guard the site" |
/// | Lag, lead, percent lag | "Build the walls", "Order the wood", "Cable trenches" |
/// | All eight constraints | see [`add_shed`] and [`add_garage`] |
/// | Manual task | "Order the garage door" |
/// | Milestones: reached, missed, to come, with a duration | "Design approved", "Neighbours agree", "Shed finished", "Opening party" |
/// | Deadline kept / missed | "Shed finished", "Garage finished" / "Clean up" |
/// | Blockers: before, inside and after the work, two on one task | "Prepare the ground", "Dig the foundation", "Fit the windows" |
/// | Risks of every priority, approved and mitigated | many |
/// | Late to start, behind, overdue | "Order the wood", "Get building permit", "Get the permit" |
/// | Own calendar, holiday, working Saturday | "Let the concrete harden", "Paint"; 27 and 28 November |
///
/// # Returns
/// A new [`Project`] (one file) with two project rows.
///
/// # Example
/// ```
/// let file = ganttora_lib::demo::sample_projects();
/// assert_eq!(file.tasks.iter().filter(|t| t.is_project).count(), 2);
/// assert!(engine::schedule(&file).is_ok());
/// ```
pub fn sample_projects() -> Project {
    let mut project = Project::new("Garden shed", sample_time(11, 17, 8).date());
    add_calendars(&mut project);
    add_shed(&mut project);
    add_garage(&mut project);
    add_plan(&mut project);
    project
}

/// Saves a plan made before the blockers were known, and the actual finish
/// of the work that is done: on time, late and early.
///
/// # Parameters
/// - `project`: the complete sample file.
fn add_plan(project: &mut Project) {
    let mut before = project.clone();
    for task in &mut before.tasks {
        task.blockers.clear();
    }
    let Ok(plan) = engine::schedule(&before) else {
        return;
    };
    for (task, dates) in project.tasks.iter_mut().zip(&plan.tasks) {
        task.planned_finish = Some(dates.finish);
        task.actual_finish = match task.name.as_str() {
            "Measure the garden" => Some(sample_time(11, 17, 17)),
            "Design the shed" | "Design approved" => Some(sample_time(11, 20, 17)),
            "Draw the plans" => Some(sample_time(11, 17, 17)),
            _ => None,
        };
    }
}

/// Adds a holiday, a working Saturday and two more calendars.
///
/// # Parameters
/// - `project`: the sample file; its Standard calendar is at index 0, the
///   new ones are "Six-day week" (1) and "Around the clock" (2).
fn add_calendars(project: &mut Project) {
    let holiday = sample_time(11, 27, 8).date();
    let saturday = sample_time(11, 28, 8).date();
    let standard = &mut project.calendars[0];
    standard.holidays.insert(holiday);
    standard.exceptions.push(CalendarException::working(
        "Extra Saturday",
        saturday,
        saturday,
        vec![WorkingTime::hours(8, 12)],
    ));

    let mut six_days = WorkWeek::standard();
    six_days.saturday = six_days.friday.clone();
    project
        .calendars
        .push(Calendar::new("Six-day week", six_days));

    let all_day = vec![WorkingTime::hours(0, 24)];
    let around_the_clock = WorkWeek {
        monday: all_day.clone(),
        tuesday: all_day.clone(),
        wednesday: all_day.clone(),
        thursday: all_day.clone(),
        friday: all_day.clone(),
        saturday: all_day.clone(),
        sunday: all_day,
    };
    project
        .calendars
        .push(Calendar::new("Around the clock", around_the_clock));
}

/// A sample task.
///
/// # Parameters
/// - `level`: 2 = task, 3 = sub-task, 4 = one deeper (1 is the project).
/// - `name`, `days`: its name and working days (0 = milestone).
/// - `percent`: how much is done.
fn task(level: u32, name: &str, days: i64, percent: u8) -> Task {
    let mut task = Task::new(name, Duration::from_days(days)).at_level(level);
    task.percent_complete = percent;
    task
}

/// Adds the project "Garden shed".
///
/// Constraints shown: start no earlier than, finish no earlier than, as late
/// as possible, start no later than.
///
/// # Parameters
/// - `project`: the sample file, with its calendars.
fn add_shed(project: &mut Project) {
    let mut shed = Task::project_row("Garden shed");
    shed.notes = "The shed stands behind the house, next to the old oak.".to_string();
    shed.hyperlink = "https://example.com/garden-shed".to_string();
    shed.risks.push(risk(
        "A wet winter stops the outdoor work",
        RiskPriority::High,
        Some("Plan the roof first, rent a tent"),
        Some("The shed is finished in spring"),
    ));

    let preparation = task(2, "Preparation", 1, 0);
    let measure = task(3, "Measure the garden", 1, 100);
    let mut design = task(3, "Design the shed", 2, 100);
    design.hyperlink = "https://example.com/drawings".to_string();
    let approved = task(3, "Design approved", 0, 100);
    // Never ticked off: an orange milestone.
    let neighbours = task(3, "Neighbours agree", 0, 0);
    // Behind on 25 November.
    let mut permit = task(3, "Get building permit", 5, 10);
    permit.notes = "Reference number 2026-114. Call on Mondays only.".to_string();
    permit.risks.push(risk(
        "The office asks for another drawing",
        RiskPriority::Medium,
        Some("Prepare the side view now"),
        Some("The permit comes a week later"),
    ));
    // Late to start on 25 November.
    let order = task(3, "Order the wood", 2, 0);

    let groundwork = task(2, "Groundwork", 1, 0);
    let mut ground = task(3, "Prepare the ground", 2, 0);
    let mut excavator = Blocker::new(
        BlockerId::new(),
        Duration::ZERO,
        Duration::from_days(1),
        "The excavator is still at another site",
    );
    excavator.reported_on = Some(sample_time(11, 24, 8).date());
    ground.blockers.push(excavator);
    let mut trenches = task(3, "Cable trenches", 1, 0);
    trenches.risks.push(risk(
        "An old pipe is in the way",
        RiskPriority::Low,
        None,
        None,
    ));
    let pour = task(3, "Pour the floor", 1, 0);
    let mut harden = task(3, "Let the concrete harden", 6, 0);
    harden.calendar_id = Some(project.calendars[2].id);
    harden.notes = "Concrete also hardens at night and at weekends.".to_string();

    let building = task(2, "Building", 1, 0);
    let shell = task(3, "Walls and roof", 1, 0);
    let mut walls = task(4, "Put up the walls", 3, 0);
    walls.constraint = Constraint::StartNoEarlierThan {
        date: sample_time(12, 1, 8),
    };
    let mut roof = task(4, "Put on the roof", 2, 0);
    roof.risks.push(risk(
        "Strong wind",
        RiskPriority::High,
        Some("Secure the roof with ropes"),
        Some("The roof must be built again"),
    ));
    let mut paint = task(3, "Paint", 3, 0);
    paint.calendar_id = Some(project.calendars[1].id);
    paint.constraint = Constraint::FinishNoEarlierThan {
        date: sample_time(12, 11, 17),
    };
    let mut shelves = task(3, "Order the shelves", 1, 0);
    shelves.constraint = Constraint::AsLateAsPossible;
    let mut tidy = task(3, "Clean the site", 1, 0);
    tidy.constraint = Constraint::StartNoLaterThan {
        date: sample_time(12, 18, 8),
    };
    let mut finished = task(2, "Shed finished", 0, 0);
    finished.deadline = Some(sample_time(12, 22, 17));

    let fs = Dependency::finish_to_start;
    project.dependencies.extend([
        fs(measure.id, design.id),
        fs(design.id, approved.id),
        fs(approved.id, neighbours.id),
        fs(approved.id, permit.id),
        // A lead: one day before the design is ready.
        fs(design.id, order.id).with_lag(Duration::from_days(-1)),
        fs(permit.id, ground.id),
        // Starts when half of the ground is prepared.
        Dependency::new(ground.id, trenches.id, DependencyType::StartToStart).with_lag_percent(50),
        fs(ground.id, pour.id),
        fs(trenches.id, pour.id),
        fs(order.id, walls.id),
        fs(pour.id, harden.id),
        fs(harden.id, walls.id),
        fs(walls.id, roof.id).with_lag(Duration::from_days(-1)),
        fs(roof.id, paint.id),
        fs(shelves.id, finished.id),
        Dependency::new(paint.id, tidy.id, DependencyType::FinishToFinish),
        fs(paint.id, finished.id),
        fs(tidy.id, finished.id),
    ]);
    project.tasks.extend([
        shed,
        preparation,
        measure,
        design,
        approved,
        neighbours,
        permit,
        order,
        groundwork,
        ground,
        trenches,
        pour,
        harden,
        building,
        shell,
        walls,
        roof,
        paint,
        shelves,
        tidy,
        finished,
    ]);
}

/// Adds the project "Garage".
///
/// Constraints shown: must start on, must finish on, finish no later than,
/// start no earlier than.
///
/// # Parameters
/// - `project`: the sample file.
fn add_garage(project: &mut Project) {
    let mut garage = Task::project_row("Garage");
    garage.notes = "One car, two bicycles, electric door.".to_string();
    garage.risks.push(risk(
        "The budget is not enough",
        RiskPriority::Medium,
        Some("Compare three offers"),
        Some("The garage is built without electrics"),
    ));
    garage
        .risks
        .push(risk("The colour changes", RiskPriority::Low, None, None));

    let planning = task(2, "Planning", 1, 0);
    let plans = task(3, "Draw the plans", 2, 100);
    // Overdue on 25 November.
    let mut permit = task(3, "Get the permit", 3, 75);
    permit.risks.push(risk(
        "The office answers late",
        RiskPriority::Low,
        None,
        None,
    ));
    let mut order = task(2, "Order the garage door", 5, 90);
    order.mode = TaskMode::Manual;
    order.manual_start = Some(sample_time(11, 19, 8));
    order.risks.push(risk(
        "The door is delivered late",
        RiskPriority::High,
        Some("Order from a second supplier"),
        Some("The garage stays open for two weeks"),
    ));
    let mut meeting = task(2, "Meet the bricklayer", 1, 0);
    meeting.constraint = Constraint::MustStartOn {
        date: sample_time(11, 25, 8),
    };
    let foundation = task(2, "Foundation", 1, 0);
    let mut dig = task(3, "Dig the foundation", 2, 25);
    let mut rain = Blocker::new(
        BlockerId::new(),
        Duration::from_days(1),
        Duration::from_days(1),
        "Heavy rain",
    );
    rain.reported_on = Some(sample_time(11, 25, 8).date());
    dig.blockers.push(rain);
    dig.risks.push(risk(
        "Rock under the soil",
        RiskPriority::Medium,
        Some("Rent a bigger excavator"),
        Some("Two more days of digging"),
    ));
    let concrete = task(3, "Pour the concrete", 1, 0);
    let mut walls = task(2, "Build the walls", 3, 0);
    walls.risks.push(risk(
        "Bricks get more expensive",
        RiskPriority::Low,
        None,
        Some("Higher costs"),
    ));
    walls.risks.push(risk(
        "The bricklayer is ill",
        RiskPriority::Medium,
        Some("Ask a second bricklayer"),
        Some("The walls start later"),
    ));
    let mut windows = task(2, "Fit the windows", 2, 0);
    // One blocker inside the work and one after it.
    for (after_days, reason) in [
        (1, "Wrong glass delivered"),
        (2, "Waiting for the inspection"),
    ] {
        windows.blockers.push(Blocker::new(
            BlockerId::new(),
            Duration::from_days(after_days),
            Duration::from_days(1),
            reason,
        ));
    }
    let mut electrics = task(2, "Install the electrics", 2, 0);
    electrics.notes = "Ask for two sockets next to the door.".to_string();
    electrics.hyperlink = "https://example.com/electrics".to_string();
    electrics.constraint = Constraint::FinishNoLaterThan {
        date: sample_time(12, 11, 17),
    };
    let mut door = task(2, "Fit the garage door", 1, 0);
    door.constraint = Constraint::StartNoEarlierThan {
        date: sample_time(12, 14, 8),
    };
    // Lasts until the door is closed for the first time.
    let guard = task(2, "Guard the site", 2, 0);
    let mut clean = task(2, "Clean up", 1, 0);
    clean.deadline = Some(sample_time(12, 11, 17));
    let mut party = task(2, "Opening party", 1, 0);
    party.milestone = true;
    party.constraint = Constraint::MustFinishOn {
        date: sample_time(12, 17, 17),
    };
    let mut finished = task(2, "Garage finished", 0, 0);
    finished.deadline = Some(sample_time(12, 18, 17));

    let fs = Dependency::finish_to_start;
    project.dependencies.extend([
        fs(plans.id, permit.id),
        fs(permit.id, dig.id),
        fs(dig.id, concrete.id),
        fs(concrete.id, walls.id).with_lag(Duration::from_days(1)),
        fs(meeting.id, walls.id),
        fs(walls.id, windows.id),
        Dependency::new(walls.id, electrics.id, DependencyType::StartToStart)
            .with_lag(Duration::from_days(1)),
        fs(windows.id, door.id),
        fs(electrics.id, door.id),
        fs(order.id, door.id),
        Dependency::new(door.id, guard.id, DependencyType::StartToFinish),
        Dependency::new(door.id, clean.id, DependencyType::FinishToFinish),
        fs(clean.id, party.id),
        fs(door.id, finished.id),
        fs(clean.id, finished.id),
        fs(party.id, finished.id),
    ]);
    project.tasks.extend([
        garage, planning, plans, permit, order, meeting, foundation, dig, concrete, walls, windows,
        electrics, door, guard, clean, party, finished,
    ]);
}

/// The sample projects with their dates moved to around `today`.
///
/// # What it does
/// Moves every date by whole weeks, so that the sample's Wednesday, 25
/// November 2026, becomes the Wednesday nearest to `today`. Weekends stay
/// weekends, and some work is always done, late, starting and to come.
///
/// # Parameters
/// - `today`: the day the sample is opened.
///
/// # Returns
/// The file of [`sample_projects`] with moved dates.
///
/// # Example
/// ```
/// use chrono::NaiveDate;
/// use ganttora_lib::demo::sample_projects_for;
/// let start = |y, m, d| {
///     let today = NaiveDate::from_ymd_opt(y, m, d).unwrap();
///     sample_projects_for(today).start.date().to_string()
/// };
/// assert_eq!(start(2026, 11, 25), "2026-11-17");
/// assert_eq!(start(2026, 11, 28), "2026-11-17"); // Saturday: same week
/// assert_eq!(start(2026, 11, 29), "2026-11-24"); // Sunday: the next one is nearer
/// assert_eq!(start(2027, 3, 3), "2027-02-23");
/// // Every date moves: the moved sample has the same schedule, later.
/// let today = NaiveDate::from_ymd_opt(2027, 3, 3).unwrap();
/// let moved = engine::schedule(&sample_projects_for(today)).unwrap();
/// assert_eq!(moved.project_finish.to_string(), "2027-03-25 17:00:00");
/// ```
pub fn sample_projects_for(today: NaiveDate) -> Project {
    let mut project = sample_projects();
    let days = (today - sample_time(11, 25, 8).date()).num_days();
    // `div_euclid` rounds down also for negative numbers; + 3 makes that
    // "to the nearest week".
    shift_dates(&mut project, TimeDelta::weeks((days + 3).div_euclid(7)));
    project
}

/// Moves every date of a project.
///
/// # Parameters
/// - `project`: the project to change.
/// - `by`: how far; may be negative.
fn shift_dates(project: &mut Project, by: TimeDelta) {
    project.start += by;
    for calendar in &mut project.calendars {
        calendar.holidays = calendar.holidays.iter().map(|day| *day + by).collect();
        for exception in &mut calendar.exceptions {
            exception.start += by;
            exception.finish += by;
        }
    }
    for task in &mut project.tasks {
        task.manual_start = task.manual_start.map(|date| date + by);
        task.deadline = task.deadline.map(|date| date + by);
        task.planned_finish = task.planned_finish.map(|date| date + by);
        task.actual_finish = task.actual_finish.map(|date| date + by);
        for blocker in &mut task.blockers {
            blocker.reported_on = blocker.reported_on.map(|day| day + by);
        }
        match &mut task.constraint {
            Constraint::AsSoonAsPossible | Constraint::AsLateAsPossible => {}
            Constraint::StartNoEarlierThan { date }
            | Constraint::StartNoLaterThan { date }
            | Constraint::FinishNoEarlierThan { date }
            | Constraint::FinishNoLaterThan { date }
            | Constraint::MustStartOn { date }
            | Constraint::MustFinishOn { date } => *date += by,
        }
    }
}

/// A moment in 2026, for the sample dates.
///
/// # Parameters
/// - `month`, `day`: the date; `hour`: the full hour.
///
/// # Returns
/// The date and time.
///
/// # Example
/// `sample_time(11, 19, 8)` is 19 November 2026, 08:00.
fn sample_time(month: u32, day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, month, day)
        .and_then(|date| date.and_hms_opt(hour, 0, 0))
        .expect("the sample dates are valid")
}

/// Builds one complete sample risk.
///
/// # Parameters
/// - `description`, `priority`: what could happen and how bad it is.
/// - `mitigation`: what is done about it; `None` = the risk is approved.
/// - `consequence`: what it leads to; `None` = no consequence.
///
/// # Returns
/// The risk.
///
/// # Example
/// `risk("Late answer", RiskPriority::Low, None, None)` is an approved low
/// risk without a consequence.
fn risk(
    description: &str,
    priority: RiskPriority,
    mitigation: Option<&str>,
    consequence: Option<&str>,
) -> Risk {
    let mut risk = Risk::new(RiskId::new(), description);
    risk.priority = priority;
    risk.response = match mitigation {
        Some(_) => RiskResponse::Mitigate,
        None => RiskResponse::Approved,
    };
    risk.mitigation = mitigation.unwrap_or_default().to_string();
    risk.no_consequence = consequence.is_none();
    risk.consequence = consequence.unwrap_or_default().to_string();
    risk
}
