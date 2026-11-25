//! # Critical tasks and the critical path
//!
//! ## What this file is for
//! Decides which tasks are **critical**: tasks that cannot slip at all
//! without delaying the project finish. The chain of critical tasks is the
//! **critical path**.
//!
//! ## Where it fits
//! Last step of the critical path method, called by [`super::run`] for each
//! task, and by [`crate::ScheduleResult::critical_tasks`].
//!
//! ## The rule
//! A task is critical when its **total slack is zero or less**. This is MS
//! Project's default ("Tasks are critical if slack is less than or equal to
//! 0 days"). A setting to raise that threshold may come later.
//!
//! ## Worked example: the shed project
//! ```text
//! total slack:  1:0  2:0  3:4d  4:0  5:0  6:0  7:0  8:0
//! critical:     1, 2, 4, 5, 6, 7, 8      (3 has 4 days of slack)
//! critical path: 1 -> 2 -> 4 -> 5 -> 6 -> 7 -> 8, 17 working days
//! ```
//!
//! ## Documentation

use crate::TaskSchedule;
use model::{Duration, TaskId};

/// Decides whether a task with the given total slack is critical.
///
/// # Parameters
/// - `total_slack`: the task's total slack.
///
/// # Returns
/// `true` if the slack is zero or negative.
///
/// # Example
/// ```
/// use engine::cpm::critical::is_critical;
/// use model::Duration;
/// assert!(is_critical(Duration::ZERO));
/// assert!(!is_critical(Duration::from_days(4)));
/// ```
pub fn is_critical(total_slack: Duration) -> bool {
    total_slack.minutes() <= 0
}

/// Lists the IDs of the critical tasks, keeping the order of `tasks`.
///
/// # Parameters
/// - `tasks`: calculated task dates (usually `ScheduleResult::tasks`).
///
/// # Returns
/// The IDs of the tasks whose `is_critical` flag is set.
///
/// # Example
/// ```
/// use engine::{schedule, cpm::critical::critical_tasks};
/// use model::{Duration, Project, Task};
/// use chrono::NaiveDate;
/// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// p.tasks.push(Task::new("Long", Duration::from_days(3)));
/// p.tasks.push(Task::new("Short", Duration::from_days(1)));
/// let result = schedule(&p).unwrap();
/// // Only the longer task decides the finish date.
/// assert_eq!(critical_tasks(&result.tasks), vec![p.tasks[0].id]);
/// ```
pub fn critical_tasks(tasks: &[TaskSchedule]) -> Vec<TaskId> {
    // `.filter` keeps only the critical ones, `.map` turns each into its ID.
    tasks
        .iter()
        .filter(|t| t.is_critical)
        .map(|t| t.task_id)
        .collect()
}
