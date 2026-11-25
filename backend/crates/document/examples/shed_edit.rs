//! # Example: the Chapter 3 edit, with undo and redo
//!
//! ## What this file is for
//! Follows the edit from book Chapter 3 through the `document` crate: open
//! the garden shed, change task 2 "Get building permit" from 5 days to 6
//! days, then undo and redo. After each step it prints the project finish.
//!
//! ## How to run it
//! ```text
//! cargo run -p document --example shed_edit
//! ```
//!
//! ## Expected result
//! ```text
//! Opened:  finish Tue 23 Mar 2027 17:00 (17 working days)
//! Edit:    task 2 duration 2400 -> 2880 minutes, by Sam Lee
//! Applied: finish Wed 24 Mar 2027 17:00 (18 working days)
//! Undo:    finish Tue 23 Mar 2027 17:00 (17 working days)
//! Redo:    finish Wed 24 Mar 2027 17:00 (18 working days)
//! ```
//! (2400 minutes = 5 days × 480 minutes; 2880 = 6 days.)
//!
//! ## Documentation

use chrono::{NaiveDate, TimeZone, Utc};
use document::{BatchId, ChangeContext, Command, Document};
use model::{Dependency, Duration, Project, Task};

/// Builds the shed project of the project documentation (same as the engine's `shed`
/// example): 8 tasks, 8 Finish-to-Start links, start Mon 1 Mar 2027.
fn shed_project() -> Project {
    let start = NaiveDate::from_ymd_opt(2027, 3, 1).expect("valid date");
    let mut project = Project::new("Garden shed", start);
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
    // (predecessor row, successor row), with the book's 1-based row numbers.
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
        let pred_id = project.tasks[pred - 1].id;
        let succ_id = project.tasks[succ - 1].id;
        project
            .dependencies
            .push(Dependency::finish_to_start(pred_id, succ_id));
    }
    project
}

/// Prints the project finish and its length in working days.
///
/// # Parameters
/// - `label`: the step name shown at the start of the line.
/// - `doc`: the open document.
fn print_finish(label: &str, doc: &Document) {
    match doc.schedule() {
        Ok(result) => {
            let calendar = &doc.project().calendars[0];
            let working = engine::calendar_math::working_minutes_between(
                calendar,
                result.project_start,
                result.project_finish,
            );
            println!(
                "{label:<8} finish {} ({} working days)",
                result.project_finish.format("%a %d %b %Y %H:%M"),
                working.minutes() / doc.project().minutes_per_day
            );
        }
        Err(error) => println!("{label:<8} schedule failed: {error}"),
    }
}

/// Opens the shed, edits task 2, undoes and redoes.
fn main() {
    let mut doc = Document::new(shed_project());
    print_finish("Opened:", &doc);

    // The shell would fill this in from Settings → User and the clock.
    // A fresh batch ID per user action.
    let context = |batch: BatchId| ChangeContext {
        author: "Sam Lee".to_string(),
        timestamp: Utc
            .with_ymd_and_hms(2027, 2, 15, 10, 42, 7)
            .single()
            .expect("valid timestamp"),
        batch_id: batch,
    };

    // Task 2 is at row index 1 (rows count from 0 in code).
    let permit = doc.project().tasks[1].id;
    let edit = Command::SetTaskDuration {
        id: permit,
        duration: Duration::from_days(6),
    };
    let outcome = match doc.apply(edit, &context(BatchId::new())) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("Edit refused: {error}");
            return;
        }
    };
    for record in &outcome.changes {
        println!(
            "Edit:    task 2 {} {} -> {} minutes, by {}",
            record.field, record.old_value, record.new_value, record.author
        );
    }
    print_finish("Applied:", &doc);

    if let Err(error) = doc.undo(&context(BatchId::new())) {
        eprintln!("Undo failed: {error}");
        return;
    }
    print_finish("Undo:", &doc);

    if let Err(error) = doc.redo(&context(BatchId::new())) {
        eprintln!("Redo failed: {error}");
        return;
    }
    print_finish("Redo:", &doc);
}
