//! # Engine property tests: rules that hold for every project
//!
//! ## What this file is for
//! Normal tests check one hand-made example each. **Property tests** check a
//! rule (a "property") on hundreds of *randomly generated* projects. The
//! library `proptest` makes the random projects; when a rule fails, it
//! shrinks the project to the smallest one that still fails and prints it,
//! so the bug is easy to see.
//!
//! The properties checked here:
//! 1. Every automatically scheduled task finishes at or after its start,
//!    starts at or after the project start, and finishes at or before the
//!    project finish.
//! 2. **Determinism**: scheduling the same project twice gives exactly the
//!    same result, and so does storing the links in reverse order.
//! 3. **Circles are never accepted**: adding a link that closes a loop
//!    always gives a `Cycle` error.
//! 4. Summary tasks span their subtasks: a summary starts no later than,
//!    and finishes no earlier than, each of its subtasks.
//!
//! ## Where it fits
//! Declared as `[[test]] name = "engine_props"` in `tests/rust/Cargo.toml`.
//! Run it with:
//! ```text
//! cargo test -p ganttora-tests --test engine_props
//! ```
//!
//! ## Main ideas
//! - Generated projects use fixed IDs (`Uuid::from_u128`), so a failing case
//!   can be repeated exactly.
//! - Links only go from an earlier row to a later row. Such a graph can
//!   never contain a circle (you cannot walk "down" and come back "up"), so
//!   every generated project must schedule. Property 3 then adds one
//!   "upward" link on purpose.
//! - The `proptest!` macro turns each `fn name(input in strategy)` into a
//!   normal test that runs the body for many generated inputs.
//!
//! ## Documentation
//! - the project documentation

use engine::{ScheduleError, ScheduleResult, schedule};
use ganttora_tests::{days, generate::task_id, project_on};
use model::{
    Constraint, Dependency, DependencyId, DependencyType, Duration, Outline, Project, Task,
    TaskMode,
};
use proptest::prelude::*;
use uuid::Uuid;

/// The description of one random link, before it is turned into a
/// `Dependency`: (from, to) as fractions of the task count, the link type
/// (0–3), the time lag in hours, and an optional percentage.
type LinkSpec = (usize, usize, u8, i64, Option<i32>);

/// The description of one random task: duration in hours (0 = milestone)
/// and a constraint choice (0 = ASAP, 1 = ALAP, 2 = SNET on day `n`,
/// 3 = FNLT on day `n`), plus `n`.
type TaskSpec = (i64, u8, i64);

/// The four link types, indexed 0–3.
const KINDS: [DependencyType; 4] = [
    DependencyType::FinishToStart,
    DependencyType::StartToStart,
    DependencyType::FinishToFinish,
    DependencyType::StartToFinish,
];

/// Builds a project from random task and link descriptions.
///
/// # Parameters
/// - `tasks`: one `TaskSpec` per task (at least one).
/// - `links`: link descriptions; each becomes a link from the lower row to
///   the higher row (pairs with equal rows, or repeated pairs, are skipped).
///
/// # Returns
/// A project starting Mon 1 Mar 2027 on the standard calendar. It has no
/// circles by construction.
fn build(tasks: &[TaskSpec], links: &[LinkSpec]) -> Project {
    let mut project = project_on("2027-03-01");
    for (index, &(hours, constraint, day)) in tasks.iter().enumerate() {
        let mut task = Task::with_id(
            task_id(index),
            format!("T{index}"),
            Duration::from_hours(hours),
        );
        // Constraint dates: day `day` of March 2027 at 08:00.
        let date = ganttora_tests::dt("2027-03-01 08:00") + chrono::Duration::days(day);
        task.constraint = match constraint {
            1 => Constraint::AsLateAsPossible,
            2 => Constraint::StartNoEarlierThan { date },
            3 => Constraint::FinishNoLaterThan { date },
            _ => Constraint::AsSoonAsPossible,
        };
        project.tasks.push(task);
    }
    let count = tasks.len();
    let mut seen = std::collections::BTreeSet::new();
    for (index, &(a, b, kind, lag_hours, percent)) in links.iter().enumerate() {
        let (from, to) = (a % count, b % count);
        // Only "downward" links: from the smaller row to the larger one.
        let (from, to) = if from < to { (from, to) } else { (to, from) };
        if from == to || !seen.insert((from, to)) {
            continue;
        }
        let mut link = Dependency::new(task_id(from), task_id(to), KINDS[usize::from(kind % 4)])
            .with_lag(Duration::from_hours(lag_hours));
        link.lag_percent = percent;
        link.id = DependencyId::from_uuid(Uuid::from_u128(1_000 + index as u128));
        project.dependencies.push(link);
    }
    project
}

/// The strategy (recipe for random values) for one task description.
fn task_spec() -> impl Strategy<Value = TaskSpec> {
    // Durations 0–40 hours (0–5 days); constraint 0–3 (ASAP twice as likely
    // via the range 0..5 mapped below); constraint day 0–20.
    (0i64..=40, 0u8..5, 0i64..20).prop_map(|(h, c, d)| (h, if c == 4 { 0 } else { c }, d))
}

/// The strategy for one link description: lags from −16 to +24 hours, and
/// one link in four uses a percentage from −50 % to +100 %.
fn link_spec() -> impl Strategy<Value = LinkSpec> {
    (
        0usize..100,
        0usize..100,
        0u8..4,
        -16i64..=24,
        prop_oneof![3 => Just(None), 1 => (-50i32..=100).prop_map(Some)],
    )
}

/// Checks property 1 on a result.
fn check_bounds(project: &Project, result: &ScheduleResult) -> Result<(), TestCaseError> {
    for (task, dates) in project.tasks.iter().zip(&result.tasks) {
        if task.mode != TaskMode::Auto {
            continue;
        }
        prop_assert!(
            dates.finish >= dates.start,
            "{}: finish before start",
            task.name
        );
        // An FNLT constraint is honoured even when it is earlier than the
        // project start (MS Project default "tasks always honor their
        // constraint dates"), so such a task may start before the project.
        if !matches!(task.constraint, Constraint::FinishNoLaterThan { .. }) {
            prop_assert!(
                dates.start >= result.project_start,
                "{}: starts before the project",
                task.name
            );
        }
        prop_assert!(
            dates.finish <= result.project_finish,
            "{}: finishes after the project",
            task.name
        );
        prop_assert!(dates.early_finish >= dates.early_start);
    }
    Ok(())
}

proptest! {
    // 256 random projects per property (proptest's default).

    /// Property 1: finish ≥ start for every automatic task, and every task
    /// lies inside the project's start and finish.
    ///
    /// Why it matters: a bar that ends before it begins, or sticks out of
    /// the project, is always a bug, whatever the input.
    #[test]
    fn auto_tasks_finish_after_they_start(
        tasks in prop::collection::vec(task_spec(), 1..20),
        links in prop::collection::vec(link_spec(), 0..30),
    ) {
        let project = build(&tasks, &links);
        let result = schedule(&project).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(result.tasks.len(), project.tasks.len());
        check_bounds(&project, &result)?;
    }

    /// Property 2: the same project always gives the same result, also when
    /// the links are stored in reverse order.
    ///
    /// Why it matters: the engine must be deterministic.
    /// The order of links in a file must not change any date or any
    /// "Why this date?" answer.
    #[test]
    fn scheduling_is_deterministic(
        tasks in prop::collection::vec(task_spec(), 1..15),
        links in prop::collection::vec(link_spec(), 0..25),
    ) {
        let project = build(&tasks, &links);
        let first = schedule(&project);
        let second = schedule(&project);
        prop_assert_eq!(&first, &second);
        let mut reversed = project.clone();
        reversed.dependencies.reverse();
        prop_assert_eq!(&first, &schedule(&reversed));
    }

    /// Property 3: a chain T0 → T1 → … → Tn plus one link from a later task
    /// back to an earlier (or the same) task is always refused with a
    /// `Cycle` error.
    ///
    /// Why it matters: a circular plan has no schedule; letting one through
    /// would hang the engine or give nonsense dates.
    #[test]
    fn cycles_are_never_accepted(
        count in 1usize..12,
        back in (0usize..100, 0usize..100),
        kind in 0u8..4,
    ) {
        let mut project = project_on("2027-03-01");
        for index in 0..count {
            project.tasks.push(Task::with_id(task_id(index), format!("T{index}"), days(1)));
        }
        for index in 1..count {
            project.dependencies.push(Dependency::finish_to_start(task_id(index - 1), task_id(index)));
        }
        let (a, b) = (back.0 % count, back.1 % count);
        let (early, late) = if a <= b { (a, b) } else { (b, a) };
        project.dependencies.push(Dependency::new(
            task_id(late),
            task_id(early),
            KINDS[usize::from(kind)],
        ));
        let is_cycle = matches!(schedule(&project), Err(ScheduleError::Cycle { .. }));
        prop_assert!(is_cycle, "the loop T{} -> T{} was not refused", late, early);
    }

    /// Property 4: with a random valid outline (each row at most one level
    /// deeper than the row above), every summary task starts no later than
    /// and finishes no earlier than each of its direct subtasks.
    ///
    /// Why it matters: the summary bar must always cover its subtasks.
    #[test]
    fn summaries_cover_their_subtasks(
        steps in prop::collection::vec(0u8..3, 1..20),
        hours in prop::collection::vec(0i64..=40, 20),
    ) {
        let mut project = project_on("2027-03-01");
        let mut level = 0u32;
        for (index, step) in steps.iter().enumerate() {
            // step 0 = one level deeper (if allowed), 1 = same, 2 = one up.
            level = match step {
                0 => level + 1,
                1 => level.max(1),
                _ => level.saturating_sub(1).max(1),
            };
            let task = Task::with_id(task_id(index), format!("T{index}"), Duration::from_hours(hours[index]))
                .at_level(level);
            project.tasks.push(task);
        }
        prop_assert_eq!(model::first_invalid_row(&project.tasks), None);
        let outline = Outline::build(&project.tasks);
        let result = schedule(&project).map_err(|e| TestCaseError::fail(e.to_string()))?;
        for row in 0..project.tasks.len() {
            prop_assert_eq!(result.tasks[row].is_summary, outline.is_summary(row));
            if let Some(parent) = outline.parent(row) {
                prop_assert!(result.tasks[parent].start <= result.tasks[row].start);
                prop_assert!(result.tasks[parent].finish >= result.tasks[row].finish);
            }
        }
    }
}
