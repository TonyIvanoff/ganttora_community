//! # Generators for big, repeatable test projects
//!
//! ## What this file is for
//! Performance tests need large projects (10,000 tasks). Typing them by hand
//! is impossible, so these functions *generate* them. The generated project
//! is always exactly the same for the same inputs, down to every task ID:
//! that is what "deterministic" means. A benchmark is only useful if every
//! run measures the same work.
//!
//! ## Where it fits
//! Part of the `ganttora_tests` helper library. Used by the benchmark
//! `tests/perf/rust/schedule_10k.rs` and by a size test in
//! `tests/rust/engine/`.
//!
//! ## Main ideas
//! - **Pseudo-random numbers.** We need "random-looking" durations and links,
//!   but the same ones every time. A *pseudo-random number generator* (PRNG)
//!   does that: a small formula that, from a starting number (the *seed*),
//!   produces a long sequence of numbers that look random but are fully
//!   predictable. We use "SplitMix64", a well-known 10-line PRNG, written out
//!   here so we need no extra library.
//! - **Fixed IDs.** Task `i` gets the UUID `i + 1`, link `j` gets the UUID
//!   `2^64 + j`. Real projects use random UUIDs; fixed ones make the generated
//!   project identical on every run.
//! - **Two shapes** of project:
//!   - [`layered_project`]: a realistic big project. Tasks are arranged in
//!     *layers* (think: phases); each task waits for one or two tasks in the
//!     layer before it. 200 layers × 50 tasks = 10,000 tasks, about 2–3 years
//!     long.
//!   - [`chain_project`]: a stress test. Every task waits for the one before
//!     it, so 10,000 tasks form one long chain lasting a century or more.
//!     That shows how the engine copes with very long date ranges.

use chrono::NaiveDate;
use model::{
    Calendar, CalendarId, Dependency, DependencyId, Duration, Project, ProjectId, Task, TaskId,
};
use uuid::Uuid;

/// A tiny pseudo-random number generator (SplitMix64).
///
/// # What it does
/// Each call to [`SplitMix64::next_u64`] returns the next number of a fixed,
/// random-looking sequence. The same seed always gives the same sequence.
///
/// # Example
/// ```
/// use ganttora_tests::generate::SplitMix64;
/// let mut a = SplitMix64::new(7);
/// let mut b = SplitMix64::new(7);
/// assert_eq!(a.next_u64(), b.next_u64()); // same seed, same numbers
/// ```
pub struct SplitMix64 {
    /// The generator's internal state; changes on every call.
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from a seed.
    ///
    /// # Parameters
    /// - `seed`: any number; it picks which sequence we get.
    ///
    /// # Returns
    /// A new generator.
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    /// Returns the next pseudo-random 64-bit number.
    ///
    /// # Beginner note
    /// `wrapping_add` and `wrapping_mul` let the number "wrap around" (like a
    /// car's odometer going from 999999 back to 0) instead of stopping the
    /// program on overflow. `^` is "exclusive or" and `>>` shifts the bits
    /// right; together they mix the bits so the output looks random.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Returns a number from `low` to `high`, both included.
    ///
    /// # Parameters
    /// - `low`, `high`: the range; `low` must not be larger than `high`.
    ///
    /// # Returns
    /// A pseudo-random number in `low..=high`.
    pub fn range(&mut self, low: u64, high: u64) -> u64 {
        // `%` is the remainder of a division: it folds any number into the
        // range 0..span. (Slightly uneven for huge spans, fine for tests.)
        let span = high - low + 1;
        low + self.next_u64() % span
    }
}

/// Makes the fixed ID of generated task number `index` (counting from 0).
///
/// # Example
/// ```
/// use ganttora_tests::generate::task_id;
/// assert_eq!(task_id(0), task_id(0));
/// assert_ne!(task_id(0), task_id(1));
/// ```
pub fn task_id(index: usize) -> TaskId {
    TaskId::from_uuid(Uuid::from_u128(index as u128 + 1))
}

/// Makes the fixed ID of generated link number `index`.
fn dependency_id(index: usize) -> DependencyId {
    DependencyId::from_uuid(Uuid::from_u128((1u128 << 64) + index as u128))
}

/// Creates an empty project with fixed IDs, starting on Monday 4 Jan 2027,
/// with the standard calendar.
fn empty_fixed_project(name: &str) -> Project {
    let start = NaiveDate::from_ymd_opt(2027, 1, 4).unwrap_or_default();
    let mut project = Project::new(name, start);
    project.id = ProjectId::from_uuid(Uuid::from_u128(u128::MAX));
    let calendar_id = CalendarId::from_uuid(Uuid::from_u128(u128::MAX - 1));
    let mut calendar = Calendar::standard();
    calendar.id = calendar_id;
    project.calendars = vec![calendar];
    project.calendar_id = calendar_id;
    project
}

/// Adds a Finish-to-Start link with the next fixed ID.
fn push_link(project: &mut Project, predecessor: usize, successor: usize) {
    let mut dependency = Dependency::finish_to_start(task_id(predecessor), task_id(successor));
    dependency.id = dependency_id(project.dependencies.len());
    project.dependencies.push(dependency);
}

/// Generates a realistic large project made of layers.
///
/// # What it does
/// Creates `layers × width` tasks. Task `w` of layer `k` gets a duration of
/// 1 to 3 days (about 1 in 20 tasks is a milestone, 0 days). Every task in
/// layers 1 and later waits (Finish-to-Start) for one or two random tasks of
/// the layer before it. Layer 0 has no predecessors.
///
/// # Parameters
/// - `layers`: how many layers (phases).
/// - `width`: how many tasks per layer.
/// - `seed`: picks the pseudo-random sequence; same seed, same project.
///
/// # Returns
/// The project, starting on Monday 4 January 2027.
///
/// # Example
/// ```
/// use ganttora_tests::generate::layered_project;
/// let p = layered_project(4, 3, 1);
/// assert_eq!(p.tasks.len(), 12);
/// assert_eq!(p, layered_project(4, 3, 1)); // fully repeatable
/// ```
pub fn layered_project(layers: usize, width: usize, seed: u64) -> Project {
    let mut rng = SplitMix64::new(seed);
    let mut project = empty_fixed_project("Generated layered project");
    for k in 0..layers {
        for w in 0..width {
            let index = k * width + w;
            let days = if rng.range(1, 20) == 1 {
                0
            } else {
                rng.range(1, 3)
            };
            let mut task = Task::new(
                format!("Task {}", index + 1),
                Duration::from_days(days as i64),
            );
            task.id = task_id(index);
            project.tasks.push(task);
            if k > 0 {
                // One or two predecessors from the previous layer. If both
                // picks are the same task, we add only one link.
                let first = (k - 1) * width + rng.range(0, width as u64 - 1) as usize;
                push_link(&mut project, first, index);
                if rng.range(0, 1) == 1 {
                    let second = (k - 1) * width + rng.range(0, width as u64 - 1) as usize;
                    if second != first {
                        push_link(&mut project, second, index);
                    }
                }
            }
        }
    }
    project
}

/// Generates a "lots of slack" project: one very long task plus many short
/// tasks with no links at all.
///
/// # What it does
/// Task 1 lasts `long_days` working days. Tasks 2 to `count` last 1 day each
/// and have no links, so each of them has almost `long_days` of slack. The
/// engine counts slack by walking the calendar day by day, so this shape
/// makes it walk `count × long_days` days: the worst case for the M0 engine.
///
/// # Parameters
/// - `count`: total number of tasks (including the long one).
/// - `long_days`: duration of the long task in working days.
///
/// # Returns
/// The project, starting on Monday 4 January 2027.
///
/// # Example
/// ```
/// use ganttora_tests::generate::wide_slack_project;
/// let p = wide_slack_project(10, 260);
/// assert_eq!(p.tasks.len(), 10);
/// assert!(p.dependencies.is_empty());
/// ```
pub fn wide_slack_project(count: usize, long_days: i64) -> Project {
    let mut project = empty_fixed_project("Generated wide-slack project");
    for index in 0..count {
        let days = if index == 0 { long_days } else { 1 };
        let mut task = Task::new(format!("Task {}", index + 1), Duration::from_days(days));
        task.id = task_id(index);
        project.tasks.push(task);
    }
    project
}

/// Generates one long chain of tasks: task 1 → task 2 → … → task `count`.
///
/// # What it does
/// Every task gets 1 to 10 working days and waits for the task before it.
/// With 10,000 tasks the project lasts about 55,000 working days (over 200
/// years). Real projects are never like this; it is a *stress test* for the
/// calendar arithmetic, which walks through the calendar day by day.
///
/// # Parameters
/// - `count`: number of tasks.
/// - `seed`: picks the pseudo-random durations.
///
/// # Returns
/// The project, starting on Monday 4 January 2027.
///
/// # Example
/// ```
/// use ganttora_tests::generate::chain_project;
/// let p = chain_project(5, 1);
/// assert_eq!(p.dependencies.len(), 4);
/// ```
pub fn chain_project(count: usize, seed: u64) -> Project {
    let mut rng = SplitMix64::new(seed);
    let mut project = empty_fixed_project("Generated chain project");
    for index in 0..count {
        let days = rng.range(1, 10) as i64;
        let mut task = Task::new(format!("Task {}", index + 1), Duration::from_days(days));
        task.id = task_id(index);
        project.tasks.push(task);
        if index > 0 {
            push_link(&mut project, index - 1, index);
        }
    }
    project
}
