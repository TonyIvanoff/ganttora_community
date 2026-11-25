//! # Project builders for tests
//!
//! ## What this file is for
//! Small functions that build test projects step by step, plus the garden
//! shed project from the project documentation, which many tests use as their starting
//! point.
//!
//! ## Where it fits
//! Part of the `ganttora_tests` helper library (`tests/rust/common/lib.rs`).
//!
//! ## Main ideas
//! - A test project starts with [`project_on`] (an empty project on a given
//!   day, with the standard calendar: Monday to Friday, 08:00-12:00 and
//!   13:00-17:00).
//! - [`add_task`] and [`link`] add tasks and Finish-to-Start links and give
//!   back their IDs, so the test can refer to them later.
//! - [`shed`] builds the 8-task garden shed exactly as in the project documentation.
//!   [`row_id`] finds a shed task by its row number in the book (1 to 8).

use crate::dates::date;
use model::{Dependency, DependencyId, Duration, Project, Task, TaskId};

/// A duration of `n` working days (8 working hours each).
///
/// # Parameters
/// - `n`: the number of days; may be negative for a lead.
///
/// # Returns
/// A `Duration` of `n × 480` working minutes.
///
/// # Example
/// ```
/// assert_eq!(ganttora_tests::days(2).minutes(), 960);
/// ```
pub fn days(n: i64) -> Duration {
    Duration::from_days(n)
}

/// A duration of `n` working hours.
///
/// # Parameters
/// - `n`: the number of hours; may be negative.
///
/// # Returns
/// A `Duration` of `n × 60` working minutes.
///
/// # Example
/// ```
/// assert_eq!(ganttora_tests::hours(4).minutes(), 240);
/// ```
pub fn hours(n: i64) -> Duration {
    Duration::from_hours(n)
}

/// Creates an empty project that starts on the given day.
///
/// # Parameters
/// - `start`: the start day as text, `YYYY-MM-DD`.
///
/// # Returns
/// A project named "Test project" with no tasks, no links and the standard
/// calendar. The start moment is midnight of that day; the engine moves it
/// to the first working moment (08:00 on a working day).
///
/// # Example
/// ```
/// let project = ganttora_tests::project_on("2027-03-01");
/// assert!(project.tasks.is_empty());
/// ```
pub fn project_on(start: &str) -> Project {
    Project::new("Test project", date(start))
}

/// Adds a task to the end of the project's task list.
///
/// # Parameters
/// - `project`: the project to change. `&mut` means we *borrow it mutably*:
///   we may change it, but it still belongs to the caller.
/// - `name`: the task name (shown in error messages, for example cycles).
/// - `duration`: the task's working time; zero makes it a milestone.
///
/// # Returns
/// The new task's ID, to use in links and when reading results.
///
/// # Example
/// ```
/// use ganttora_tests::{add_task, days, project_on};
/// let mut project = project_on("2027-03-01");
/// let id = add_task(&mut project, "Dig", days(1));
/// assert_eq!(project.tasks[0].id, id);
/// ```
pub fn add_task(project: &mut Project, name: &str, duration: Duration) -> TaskId {
    let task = Task::new(name, duration);
    let id = task.id;
    project.tasks.push(task);
    id
}

/// Adds a Finish-to-Start link with no lag: `successor` may start only after
/// `predecessor` has finished.
///
/// # Parameters
/// - `project`: the project to change.
/// - `predecessor`: the task that must finish first.
/// - `successor`: the task that waits.
///
/// # Returns
/// The new link's ID.
///
/// # Example
/// ```
/// use ganttora_tests::{add_task, days, link, project_on};
/// let mut project = project_on("2027-03-01");
/// let a = add_task(&mut project, "A", days(1));
/// let b = add_task(&mut project, "B", days(1));
/// link(&mut project, a, b);
/// assert_eq!(project.dependencies.len(), 1);
/// ```
pub fn link(project: &mut Project, predecessor: TaskId, successor: TaskId) -> DependencyId {
    link_with_lag(project, predecessor, successor, Duration::ZERO)
}

/// Adds a Finish-to-Start link with a lag (positive: wait; negative: lead,
/// an overlap).
///
/// # Parameters
/// - `project`: the project to change.
/// - `predecessor`, `successor`: as in [`link`].
/// - `lag`: working time between the predecessor's finish and the
///   successor's start. Negative values let the successor start earlier.
///
/// # Returns
/// The new link's ID.
///
/// # Example
/// ```
/// use ganttora_tests::{add_task, days, link_with_lag, project_on};
/// let mut project = project_on("2027-03-01");
/// let a = add_task(&mut project, "A", days(1));
/// let b = add_task(&mut project, "B", days(1));
/// link_with_lag(&mut project, a, b, days(1));
/// assert_eq!(project.dependencies[0].lag, days(1));
/// ```
pub fn link_with_lag(
    project: &mut Project,
    predecessor: TaskId,
    successor: TaskId,
    lag: Duration,
) -> DependencyId {
    let dependency = Dependency::finish_to_start(predecessor, successor).with_lag(lag);
    let id = dependency.id;
    project.dependencies.push(dependency);
    id
}

/// Builds the garden shed project of the project documentation.
///
/// # Returns
/// A project starting on Monday 1 March 2027 with these 8 tasks (in this
/// order, so row `n` is `project.tasks[n - 1]`) and 8 Finish-to-Start links:
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
/// # Example
/// ```
/// let project = ganttora_tests::shed();
/// assert_eq!(project.tasks.len(), 8);
/// assert_eq!(project.dependencies.len(), 8);
/// ```
pub fn shed() -> Project {
    let mut project = Project::new("Garden shed", date("2027-03-01"));
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
    for (name, n) in rows {
        add_task(&mut project, name, days(n));
    }
    // (predecessor row, successor row), as numbered in the book.
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
        let pred_id = row_id(&project, pred);
        let succ_id = row_id(&project, succ);
        link(&mut project, pred_id, succ_id);
    }
    project
}

/// Finds the ID of the task in row `row` (counting from 1, like the book).
///
/// # Parameters
/// - `project`: the project to look in.
/// - `row`: the row number, starting at 1.
///
/// # Returns
/// The ID of `project.tasks[row - 1]`.
///
/// # Errors / edge cases
/// Row 0 or a row past the end stops the test with a message.
///
/// # Example
/// ```
/// let project = ganttora_tests::shed();
/// let permit = ganttora_tests::row_id(&project, 2);
/// assert_eq!(project.tasks[1].id, permit);
/// ```
pub fn row_id(project: &Project, row: usize) -> TaskId {
    // `checked_sub` returns `None` instead of crashing when row is 0.
    row.checked_sub(1)
        .and_then(|index| project.tasks.get(index))
        .map(|task| task.id)
        .unwrap_or_else(|| panic!("row {row} does not exist in this project"))
}
