//! # Size tests: big generated projects
//!
//! ## What this file checks
//! That the engine handles a large project (10,000 tasks, about 15,000
//! links) correctly: it finishes, every task gets dates, and the general
//! rules of a schedule hold for every task. It does **not** measure speed;
//! that is the job of the benchmark in `tests/perf/rust/` (timings in a
//! normal test would make it fail at random on a slow computer).
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`).

use crate::schedule_ok;
use ganttora_tests::generate::{chain_project, layered_project};
use model::Duration;

/// A generated 10,000-task layered project schedules without error, and for
/// every task: finish ≥ start, late ≥ early, 0 ≤ free slack ≤ total slack,
/// critical exactly when total slack is zero, and no task ends after the
/// project finish. At least one task is critical.
///
/// Why it matters: the M0 goal is a 10k-task project. Rules that
/// hold for 8 tasks must also hold for 10,000.
#[test]
fn ten_thousand_task_project_obeys_the_general_rules() {
    let project = layered_project(200, 50, 2027);
    assert_eq!(project.tasks.len(), 10_000);
    let result = schedule_ok(&project);

    assert_eq!(result.tasks.len(), 10_000);
    for task in &result.tasks {
        assert!(task.early_finish >= task.early_start);
        assert!(task.late_finish >= task.late_start);
        assert!(task.late_start >= task.early_start);
        assert!(task.late_finish >= task.early_finish);
        assert!(task.free_slack >= Duration::ZERO);
        assert!(task.free_slack <= task.total_slack);
        assert_eq!(task.is_critical, task.total_slack <= Duration::ZERO);
        assert!(task.early_finish <= result.project_finish);
    }
    assert!(!result.critical_tasks().is_empty());
}

/// In a single chain every task waits for the one before it, so every task
/// is critical and the project finish is the last task's finish.
///
/// Why it matters: a quick, exact check on a long project (1,000 tasks,
/// about 22 years): nothing may have slack in a chain.
#[test]
fn every_task_in_a_chain_is_critical() {
    let project = chain_project(1_000, 5);
    let result = schedule_ok(&project);
    assert_eq!(result.critical_tasks().len(), 1_000);
    assert_eq!(result.project_finish, result.tasks[999].early_finish);
}
