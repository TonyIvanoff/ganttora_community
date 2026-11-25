//! # Example: save the garden shed to a `.gtra` file, open it again, schedule it
//!
//! ## What this file is for
//! A small, complete program that shows the whole life of a project file:
//!
//! ```text
//!   build the shed      gtra::save          gtra::open           engine::schedule
//!   (model::Project) ──► shed.gtra on disk ──► model::Project ──► finish date
//!                       (a SQLite database)    (equal to the
//!                                               one we saved)
//! ```
//!
//! It also checks that the project read back is *exactly equal* to the one
//! that was saved: nothing was lost or changed on the way through the file.
//!
//! ## How to run it
//! From the repository root, in a terminal:
//!
//! ```text
//! cargo run -p gtra --example save_open
//! ```
//!
//! ## Expected result
//! The file is saved and opened without notes, the projects are equal, and
//! the engine finds the same finish as the project documentation: Tue 23 Mar 2027 17:00.
//!
//! ## Documentation

use std::process::ExitCode;

use chrono::{NaiveDate, TimeZone, Utc};
use model::{Dependency, Duration, Project, Task};

/// Builds the shed project of the project documentation (the same as the engine's
/// `examples/shed.rs`): 8 tasks, 8 Finish-to-Start links, starting on
/// Monday 1 March 2027.
///
/// # Returns
/// The project.
fn shed_project() -> Project {
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
        let pred_id = project.tasks[pred - 1].id;
        let succ_id = project.tasks[succ - 1].id;
        project
            .dependencies
            .push(Dependency::finish_to_start(pred_id, succ_id));
    }
    project
}

/// Saves, opens and schedules the shed.
///
/// # Returns
/// `ExitCode::SUCCESS` if everything worked, `ExitCode::FAILURE` otherwise.
///
/// # Beginner note: `ExitCode`
/// A program tells the terminal whether it succeeded with an *exit code*:
/// 0 means success, anything else means failure. Returning `ExitCode` from
/// `main` lets us choose it. Scripts and CI use it to notice failures.
fn main() -> ExitCode {
    // `run` does the work; `main` only turns an error into a message.
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The steps of the example. Every error is turned into a `String` message
/// with `map_err(|e| e.to_string())` so that one `?` works for both the
/// `gtra` and the `engine` errors.
///
/// # Errors
/// A message describing the first step that failed.
fn run() -> Result<(), String> {
    let project = shed_project();

    // 1. A fresh folder in the computer's temporary folder.
    let folder = std::env::temp_dir().join(format!("ganttora-example-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join("Garden shed.gtra");

    // 2. Save. The time and author are chosen by us (the app would use the
    //    clock and Settings -> User), so every run writes the same meta data.
    let saved_at = Utc
        .with_ymd_and_hms(2027, 2, 15, 10, 42, 7)
        .single()
        .ok_or("invalid save time")?;
    let options = gtra::SaveOptions::new(saved_at, "Sam Lee");
    gtra::save(&path, &project, &options).map_err(|e| e.to_string())?;
    println!("Saved:    {}", path.display());

    // 3. Open it again.
    let opened = gtra::open(&path).map_err(|e| e.to_string())?;
    println!(
        "Opened:   format version {}, {:?}, saved by {} at {}",
        opened.format_version,
        opened.access,
        opened.info.author.as_deref().unwrap_or("?"),
        opened.info.modified.as_deref().unwrap_or("?"),
    );
    println!("Notes:    {}", opened.notes.len());

    // 4. The round trip must not change anything. `==` compares every field
    //    of every task, link and calendar (the `PartialEq` derive in `model`).
    if opened.project != project {
        return Err("the project read back differs from the one saved".to_string());
    }
    println!(
        "Equal:    yes ({} tasks, {} links)",
        opened.project.tasks.len(),
        opened.project.dependencies.len()
    );

    // 5. Schedule the project we read from the file.
    let result = engine::schedule(&opened.project).map_err(|e| e.to_string())?;
    println!(
        "Finish:   {}",
        result.project_finish.format("%a %d %b %Y %H:%M")
    );

    // 6. Tidy up the temporary folder.
    std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(())
}
