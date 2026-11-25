//! # Summary task tests: roll-up and links into and out of summaries
//!
//! ## What this file checks
//! A **summary task** is a row with indented rows (subtasks) below it. It has
//! no dates of its own: it starts with its earliest subtask and finishes
//! with its latest one (the "roll-up"). Links can point **into** a summary
//! (then every subtask waits) or come **out of** it (then the successor
//! waits for the whole block). A link between a summary and one of its own
//! subtasks is a circle and is refused.
//!
//! ## Where it fits
//! Part of the `engine` test target (`tests/rust/engine/main.rs`). Book
//! section 2.1 explains the outline (WBS).
//!
//! ## Main ideas
//! The test outline (levels in brackets):
//! ```text
//! row 0  Phase 1 (1)        summary
//! row 1    Design (2)       2 days
//! row 2    Permit (2)       5 days, after Design
//! row 3  Build (1)          4 days
//! ```
//! Standard calendar, project start Monday 1 March 2027.

use crate::{dates_of, schedule_ok};
use engine::{Driver, ScheduleError, schedule};
use ganttora_tests::{add_task, days, dt, link, project_on};
use model::{Constraint, Duration, Project, TaskId};

/// Builds the outline in the module notes.
///
/// # Returns
/// The project and the IDs (phase, design, permit, build).
fn phase_project() -> (Project, TaskId, TaskId, TaskId, TaskId) {
    let mut project = project_on("2027-03-01");
    let phase = add_task(&mut project, "Phase 1", Duration::ZERO);
    let design = add_task(&mut project, "Design", days(2));
    let permit = add_task(&mut project, "Permit", days(5));
    let build = add_task(&mut project, "Build", days(4));
    project.tasks[1].outline_level = 2;
    project.tasks[2].outline_level = 2;
    link(&mut project, design, permit);
    (project, phase, design, permit, build)
}

/// Phase 1 rolls up: start = Design's start Mon 1 08:00, finish = Permit's
/// finish Tue 9 17:00 (Design Mon–Tue, Permit Wed 3 – Tue 9). Its duration
/// is the working time in between: 7 days. It is marked as a summary, and
/// "Why this date?" says "rolled up from its subtasks".
///
/// Why it matters: the summary bar in the Gantt chart must span exactly its
/// subtasks.
#[test]
fn summary_rolls_up_its_subtasks() {
    let (project, phase, design, permit, _) = phase_project();
    let result = schedule_ok(&project);
    let summary = dates_of(&result, phase);
    assert!(summary.is_summary);
    assert!(!dates_of(&result, design).is_summary);
    assert_eq!(summary.start, dt("2027-03-01 08:00"));
    assert_eq!(summary.finish, dt("2027-03-09 17:00"));
    assert_eq!(summary.finish, dates_of(&result, permit).finish);
    assert_eq!(summary.duration, days(7));
    assert_eq!(summary.explanation.driver, Driver::Subtasks);
}

/// A link out of the summary: Phase 1 → Build (FS). Build waits for the
/// whole phase: it starts the next working moment after Tue 9 17:00, Wed 10
/// 08:00, and finishes Mon 15 17:00.
///
/// Why it matters: "start building when phase 1 is done" is a common way to
/// plan.
#[test]
fn link_out_of_a_summary_waits_for_the_whole_block() {
    let (mut project, phase, _, _, build) = phase_project();
    link(&mut project, phase, build);
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, build).start, dt("2027-03-10 08:00"));
    assert_eq!(dates_of(&result, build).finish, dt("2027-03-15 17:00"));
}

/// A link into the summary: "Prepare" (3 days, Mon 1 – Wed 3) → Phase 1
/// (FS). Every subtask waits: Design starts Thu 4 08:00 and finishes Fri 5
/// 17:00; Permit follows Mon 8 – Fri 12. The phase rolls up to Thu 4 – Fri
/// 12. Design's "Why this date?" names Prepare and says the limit came
/// through Phase 1 (`via_summary`).
///
/// Why it matters: a link to a phase holds back everything inside it, and
/// the explanation must tell the user where the limit came from.
#[test]
fn link_into_a_summary_holds_back_every_subtask() {
    let (mut project, phase, design, permit, _) = phase_project();
    let prepare = model::Task::new("Prepare", days(3));
    let prepare_id = prepare.id;
    project.tasks.insert(0, prepare);
    let into = link(&mut project, prepare_id, phase);
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, design).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, design).finish, dt("2027-03-05 17:00"));
    assert_eq!(dates_of(&result, permit).start, dt("2027-03-08 08:00"));
    assert_eq!(dates_of(&result, permit).finish, dt("2027-03-12 17:00"));
    assert_eq!(dates_of(&result, phase).start, dt("2027-03-04 08:00"));
    assert_eq!(dates_of(&result, phase).finish, dt("2027-03-12 17:00"));

    let why = dates_of(&result, design).explanation;
    assert_eq!(
        why.driver,
        Driver::Predecessor {
            dependency: into,
            task: prepare_id,
            kind: model::DependencyType::FinishToStart
        }
    );
    assert_eq!(why.via_summary, Some(phase));
}

/// A link from a summary to its own subtask (Phase 1 → Design) is a circle:
/// the phase would wait for Design, which waits for the phase. The engine
/// refuses it with a `Cycle` error. The reverse direction (Design → Phase 1)
/// is refused too.
///
/// Why it matters: MS Project refuses these links; scheduling them would
/// loop forever or give nonsense.
#[test]
fn link_to_own_summary_is_a_cycle() {
    let (mut project, phase, design, _, _) = phase_project();
    link(&mut project, phase, design);
    assert!(
        matches!(schedule(&project), Err(ScheduleError::Cycle { .. })),
        "summary -> own subtask"
    );

    let (mut project, phase, design, _, _) = phase_project();
    link(&mut project, design, phase);
    assert!(
        matches!(schedule(&project), Err(ScheduleError::Cycle { .. })),
        "own subtask -> summary"
    );
}

/// Nested summaries: Phase (1) > Step (2) > Detail (3, 2 days). Both
/// summaries roll up to Detail's dates, Mon 1 08:00 – Tue 2 17:00.
///
/// Why it matters: roll-up must work through several levels.
#[test]
fn nested_summaries_roll_up_through_every_level() {
    let mut project = project_on("2027-03-01");
    let phase = add_task(&mut project, "Phase", Duration::ZERO);
    let step = add_task(&mut project, "Step", Duration::ZERO);
    let detail = add_task(&mut project, "Detail", days(2));
    project.tasks[1].outline_level = 2;
    project.tasks[2].outline_level = 3;
    let result = schedule_ok(&project);
    for id in [phase, step] {
        assert!(dates_of(&result, id).is_summary);
        assert_eq!(dates_of(&result, id).start, dt("2027-03-01 08:00"));
        assert_eq!(dates_of(&result, id).finish, dt("2027-03-02 17:00"));
    }
    assert!(!dates_of(&result, detail).is_summary);
}

/// A constraint on a summary task (SNET Mon 8 Mar 08:00) is passed down: its
/// subtasks cannot start before it. Design starts Mon 8; the explanation
/// says "constraint, via Phase 1".
///
/// Why it matters: "phase 2 starts no earlier than 1 July" is one of the few
/// summary constraints M1 supports (SNET/FNET down, SNLT/FNLT/deadline up).
#[test]
fn summary_start_no_earlier_than_holds_back_subtasks() {
    let (mut project, phase, design, _, _) = phase_project();
    project.tasks[0].constraint = Constraint::StartNoEarlierThan {
        date: dt("2027-03-08 08:00"),
    };
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, design).start, dt("2027-03-08 08:00"));
    let why = dates_of(&result, design).explanation;
    assert!(matches!(why.driver, Driver::Constraint { .. }), "{why:?}");
    assert_eq!(why.via_summary, Some(phase));
}

/// A summary task's own duration field is ignored: giving Phase 1 a duration
/// of 30 days changes nothing; it still rolls up to 7 days.
///
/// Why it matters: only subtasks decide a summary's dates (as in MS Project
/// for automatically scheduled summaries).
#[test]
fn summary_own_duration_is_ignored() {
    let (mut project, phase, _, _, _) = phase_project();
    project.tasks[0].duration = days(30);
    let result = schedule_ok(&project);
    assert_eq!(dates_of(&result, phase).duration, days(7));
    assert_eq!(dates_of(&result, phase).finish, dt("2027-03-09 17:00"));
}
