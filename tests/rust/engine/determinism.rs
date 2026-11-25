//! # Determinism tests: same input, same output
//!
//! ## What this file checks
//! The engine is *deterministic*: the same project always
//! gives exactly the same schedule. We check two forms of this rule:
//! 1. Scheduling the same project twice gives identical results.
//! 2. Listing the same tasks and links in a different order does not change
//!    any task's dates, slack or critical flag.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).
//!
//! ## Why it matters
//! Golden-file tests (M2) compare our results with MS Project. That only
//! works if our answer never changes by chance. Users also expect a file to
//! show the same dates every time it is opened.

use crate::{dates_of, schedule_ok};
use ganttora_tests::generate::{SplitMix64, layered_project};
use ganttora_tests::shed;
use model::Project;

/// Scheduling the shed twice gives exactly the same result, value for value.
///
/// Why it matters: the simplest form of determinism. Any hidden randomness
/// (for example the iteration order of a `HashMap`) would break it sooner or
/// later.
#[test]
fn scheduling_twice_gives_identical_results() {
    let project = shed();
    assert_eq!(schedule_ok(&project), schedule_ok(&project));
}

/// Same as above for a generated project with 1,000 tasks and many links,
/// where hidden randomness has far more chances to show.
///
/// Why it matters: small examples can be deterministic by luck.
#[test]
fn scheduling_a_large_project_twice_gives_identical_results() {
    let project = layered_project(50, 20, 42);
    assert_eq!(schedule_ok(&project), schedule_ok(&project));
}

/// Reversing the order of the shed's tasks and links changes nothing: each
/// task (found by its ID) keeps all its dates, slack and critical flag.
///
/// Why it matters: the order of rows is a display choice. Moving a row up
/// or down in the grid must never change a date.
#[test]
fn reversed_input_order_gives_the_same_dates() {
    let project = shed();
    let mut reversed = project.clone();
    reversed.tasks.reverse();
    reversed.dependencies.reverse();
    assert_same_dates(&project, &reversed);
}

/// Shuffling the tasks and links of a generated 1,000-task project (with a
/// fixed, repeatable shuffle) changes no task's dates.
///
/// Why it matters: as above, on a project big enough to have many ties
/// (tasks ready at the same moment), where order effects would show.
#[test]
fn shuffled_input_order_gives_the_same_dates() {
    let project = layered_project(50, 20, 7);
    let mut shuffled = project.clone();
    let mut rng = SplitMix64::new(99);
    shuffle(&mut shuffled.tasks, &mut rng);
    shuffle(&mut shuffled.dependencies, &mut rng);
    assert_same_dates(&project, &shuffled);
}

/// Checks that every task of `a` has exactly the same calculated values in
/// the schedule of `b` (tasks are matched by ID, not by position).
fn assert_same_dates(a: &Project, b: &Project) {
    let result_a = schedule_ok(a);
    let result_b = schedule_ok(b);
    assert_eq!(result_a.project_start, result_b.project_start);
    assert_eq!(result_a.project_finish, result_b.project_finish);
    for task in &result_a.tasks {
        assert_eq!(task, dates_of(&result_b, task.task_id));
    }
}

/// Shuffles a list in a repeatable way (the Fisher–Yates method: walk from
/// the end, swap each item with a random earlier-or-same one).
fn shuffle<T>(items: &mut [T], rng: &mut SplitMix64) {
    for i in (1..items.len()).rev() {
        let j = rng.range(0, i as u64) as usize;
        items.swap(i, j);
    }
}
