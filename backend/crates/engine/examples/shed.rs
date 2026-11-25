//! # Example: scheduling the garden shed from the book
//!
//! ## What this file is for
//! Builds the running example of the project documentation (the garden shed, 8 tasks),
//! schedules it with the engine and prints the result as a table. It is a
//! small, complete program that shows how the `model` and `engine` crates
//! are used together, and a quick way to check that the engine gives the
//! same answer as the calculation by hand in the project documentation.
//!
//! ## How to run it
//! From the repository root, in a terminal:
//!
//! ```text
//! cargo run -p engine --example shed
//! ```
//!
//! `-p engine` picks the crate, `--example shed` picks this file (Cargo looks
//! for examples in the crate's `examples/` folder).
//!
//! ## Expected result
//! Critical path 1 -> 2 -> 4 -> 5 -> 6 -> 7 -> 8, project from Mon 1 Mar 2027
//! 08:00 to Tue 23 Mar 2027 17:00 (17 working days); "Buy materials" has
//! 4 days of total and free slack.
//!
//! ## Documentation

use chrono::{NaiveDate, NaiveDateTime};
use model::{Dependency, Duration, Project, Task};

/// Formats a moment the way the book writes dates: "Mon 01 Mar 17:00".
///
/// # Parameters
/// - `moment`: the moment to format.
///
/// # Returns
/// The formatted text. `%a` is the short weekday name, `%d` the day, `%b`
/// the short month name, `%H:%M` the time.
fn show(moment: NaiveDateTime) -> String {
    moment.format("%a %d %b %H:%M").to_string()
}

/// Builds the shed project of the project documentation.
///
/// # Returns
/// A project starting on Monday 1 March 2027 with 8 tasks and 8
/// Finish-to-Start links:
///
/// | ID | Task | Duration | Predecessors |
/// |---|---|---|---|
/// | 1 | Design the shed | 2d | – |
/// | 2 | Get building permit | 5d | 1 |
/// | 3 | Buy materials | 3d | 1 |
/// | 4 | Prepare the ground | 2d | 2 |
/// | 5 | Build the floor | 2d | 3, 4 |
/// | 6 | Build walls and roof | 4d | 5 |
/// | 7 | Paint | 2d | 6 |
/// | 8 | Shed finished | 0d | 7 |
fn shed_project() -> Project {
    // `.expect(...)` unwraps an `Option`: it gives the date, or stops the
    // program with this message if the date were invalid (it is not).
    let start = NaiveDate::from_ymd_opt(2027, 3, 1).expect("valid date");
    let mut project = Project::new("Garden shed", start);

    // (name, duration in days). The row number in the book is position + 1.
    let rows = [
        ("Design the shed", 2),
        ("Get building permit", 5),
        ("Buy materials", 3),
        ("Prepare the ground", 2),
        ("Build the floor", 2),
        ("Build walls and roof", 4),
        ("Paint", 2),
        ("Shed finished", 0),
    ];
    for (name, days) in rows {
        project
            .tasks
            .push(Task::new(name, Duration::from_days(days)));
    }

    // Links as (predecessor row, successor row), using the book's numbers.
    let links = [
        (1, 2),
        (1, 3),
        (2, 4),
        (3, 5),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 8),
    ];
    for (pred, succ) in links {
        // Row numbers start at 1, list positions at 0, hence the `- 1`.
        let pred_id = project.tasks[pred - 1].id;
        let succ_id = project.tasks[succ - 1].id;
        project
            .dependencies
            .push(Dependency::finish_to_start(pred_id, succ_id));
    }
    project
}

/// Schedules the shed and prints the result.
///
/// # Beginner note: `main`
/// Every Rust program starts in a function called `main`. Here it builds the
/// project, calls the engine and prints one line per task.
fn main() {
    let project = shed_project();

    // `match` handles both outcomes of the `Result`: success or error.
    let result = match engine::schedule(&project) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Scheduling failed: {error}");
            return;
        }
    };

    let minutes_per_day = project.minutes_per_day;
    println!(
        "{:<3} {:<22} {:<16} {:<16} {:<16} {:<16} {:>5} {:>5} {:<4}",
        "ID",
        "Task",
        "Early start",
        "Early finish",
        "Late start",
        "Late finish",
        "TS",
        "FS",
        "Crit"
    );
    for (row, (task, dates)) in project.tasks.iter().zip(&result.tasks).enumerate() {
        // Slack is stored in minutes; divide by 480 to show working days.
        let total_days = dates.total_slack.minutes() as f64 / minutes_per_day as f64;
        let free_days = dates.free_slack.minutes() as f64 / minutes_per_day as f64;
        println!(
            "{:<3} {:<22} {:<16} {:<16} {:<16} {:<16} {:>4}d {:>4}d {:<4}",
            row + 1,
            task.name,
            show(dates.early_start),
            show(dates.early_finish),
            show(dates.late_start),
            show(dates.late_finish),
            total_days,
            free_days,
            if dates.is_critical { "yes" } else { "no" },
        );
    }

    // The critical path, written with the book's row numbers.
    let critical: Vec<String> = result
        .critical_tasks()
        .iter()
        .filter_map(|id| project.tasks.iter().position(|t| t.id == *id))
        .map(|position| (position + 1).to_string())
        .collect();
    let calendar = &project.calendars[0];
    let working = engine::calendar_math::working_minutes_between(
        calendar,
        result.project_start,
        result.project_finish,
    );
    println!();
    println!("Critical path: {}", critical.join(" -> "));
    println!(
        "Project: {} to {} = {} working days",
        show(result.project_start),
        show(result.project_finish),
        working.minutes() / minutes_per_day
    );
}
