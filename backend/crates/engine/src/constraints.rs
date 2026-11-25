//! # Constraints and deadlines: date rules on single tasks
//!
//! ## What this file is for
//! Places one task inside its limits, taking its **constraint** (ASAP, ALAP,
//! SNET, SNLT, FNET, FNLT, MSO, MFO) and **deadline** into account, and moves
//! ALAP tasks (or, when scheduling from the finish, ASAP tasks) after the
//! main passes.
//!
//! ## Where it fits
//! The forward pass calls [`place_forward`], the backward pass
//! [`place_backward`], for every ordinary automatic task. [`super::cpm::run`]
//! then calls [`schedule_alap`] or [`schedule_asap`].
//!
//! ## The rules (MS Project semantics, "tasks always honor their constraint dates")
//! | Constraint | Forward pass (early dates) | Backward pass (late dates) |
//! |---|---|---|
//! | ASAP, ALAP | no effect | no effect |
//! | SNET d | start ≥ d | no effect¹ |
//! | FNET d | finish ≥ d | no effect¹ |
//! | SNLT d | start ≤ d, **wins over links**² | start ≤ d |
//! | FNLT d | finish ≤ d, **wins over links**² | finish ≤ d |
//! | MSO d | start = d (wins over links) | start = d |
//! | MFO d | finish = d (wins over links) | finish = d |
//! | deadline d | no effect | finish ≤ d |
//!
//! ¹ When scheduling from the **finish**, the backward pass decides the
//!   dates and these are honoured there instead (start ≥ d wins over links).
//! ² Only in the pass that decides the dates; in the other pass they are
//!   ignored. (This mirrors ¹.)
//!
//! **Negative slack** appears naturally when rules conflict: a predecessor
//! that finishes Wed 17:00 and an SNLT of Mon 08:00 on its successor give
//! the successor a start of Mon (the constraint wins), while the backward
//! pass gives the predecessor a late finish before Mon, earlier than its
//! early finish: its total slack is negative. MS Project shows the same.
//!
//! **Blockers** make a task longer: every rule here uses the
//! task's *span* (work plus blocked time, `blockers::span`) instead of its
//! bare duration. An MFO of Fri 12 Mar 17:00 on a 3-day task with a 5-day
//! blocker therefore gives a start 8 working days earlier, Wed 3 Mar 08:00.
//!
//! ## Worked example: SNET
//! ```text
//! "Build the floor": predecessor finishes Thu 11 Mar 2027 17:00
//!   links:  start >= Thu 11 Mar 17:00
//!   SNET:   start >= Mon 15 Mar 08:00   <- later, wins
//!   start = Mon 15 Mar 08:00, finish (2d) = Tue 16 Mar 17:00
//! ```
//!
//! ## Documentation

use crate::calendar_math::{
    CalendarError, add_working_time, snap_backward, snap_forward, subtract_working_time,
    touches_working_time,
};
use crate::cpm::graph::EdgeKind;
use crate::cpm::{Context, TaskDates, backward, forward};
use crate::explain::{Bound, Driver, Explanation, Limits, TaskEnd};
use crate::summary::scheduled_roll_up;
use chrono::{NaiveDateTime, NaiveTime};
use model::{Calendar, Constraint, Duration, Task, TaskMode};

/// Where a task was placed, and why.
///
/// # Fields
/// - `dates`: the start and finish.
/// - `explanation`: the "Why this date?" answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Start and finish.
    pub dates: TaskDates,
    /// Why.
    pub explanation: Explanation,
}

/// A candidate position for one end of a task: the moment that end would
/// be at, plus the reason and which end the reason was about.
#[derive(Clone, Copy)]
struct Candidate {
    /// Where the end being decided would be (the start in the forward pass,
    /// the finish in the backward pass).
    at: NaiveDateTime,
    /// The limit that asked for it.
    why: Bound,
    /// Which end of the task `why` was about.
    end: TaskEnd,
}

impl Candidate {
    /// Keeps the later candidate (lower limits).
    fn keep_later(&mut self, other: Candidate) {
        let tie_won = other.why.driver.wins_tie_over(&self.why.driver);
        if other.at > self.at || (other.at == self.at && tie_won) {
            *self = other;
        }
    }

    /// Keeps the earlier candidate (upper limits).
    fn keep_earlier(&mut self, other: Candidate) {
        let tie_won = other.why.driver.wins_tie_over(&self.why.driver);
        if other.at < self.at || (other.at == self.at && tie_won) {
            *self = other;
        }
    }
}

/// A constraint as a [`Bound`] with the right driver.
fn constraint_bound(constraint: Constraint, date: NaiveDateTime) -> Bound {
    Bound::new(date, Driver::Constraint { constraint })
}

/// The finish to aim at for a lower limit on the finish.
///
/// # What it does
/// Usually the limit itself. But a limit can lie where this task's calendar
/// rests, e.g. on Saturday (a predecessor with another calendar, or a
/// "finish no earlier than" date). Counting back from there would end on
/// Friday, *before* the limit. So it moves on to the end of the next working
/// day: Monday 17:00. A milestone moves to the next working moment.
///
/// A limit on the edge of working time stays: finishing Friday 17:00 counts
/// as Monday 08:00, as everywhere in the engine.
///
/// # Parameters
/// - `calendar`: the task's calendar.
/// - `limit`: the lower limit on the finish.
/// - `duration`: the length to place.
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
fn finish_goal(
    calendar: &Calendar,
    limit: NaiveDateTime,
    duration: Duration,
) -> Result<NaiveDateTime, CalendarError> {
    if touches_working_time(calendar, limit) {
        return Ok(limit);
    }
    let next = snap_forward(calendar, limit)?;
    if duration.is_zero() {
        return Ok(next);
    }
    // The last working moment before the next midnight.
    let day = next.date();
    let midnight = day.succ_opt().unwrap_or(day).and_time(NaiveTime::MIN);
    snap_backward(calendar, midnight)
}

/// The start to aim at for an upper limit on the start (a link's, or "start
/// no later than"): the mirror image of [`finish_goal`].
///
/// # What it does
/// A task that must start by Saturday, its day off, must start by Friday
/// 08:00: the start of its last working day before.
///
/// # Parameters
/// - `calendar`: the task's calendar.
/// - `limit`: the upper limit on the start.
/// - `duration`: the length to place; a milestone (zero) moves back to the
///   last working moment.
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
fn start_goal(
    calendar: &Calendar,
    limit: NaiveDateTime,
    duration: Duration,
) -> Result<NaiveDateTime, CalendarError> {
    if touches_working_time(calendar, limit) {
        return Ok(limit);
    }
    let day = snap_backward(calendar, limit)?;
    if duration.is_zero() {
        return Ok(day);
    }
    // Ending at midnight belongs to the day before.
    let day = if day.time() == NaiveTime::MIN {
        day.date().pred_opt().unwrap_or(day.date())
    } else {
        day.date()
    };
    snap_forward(calendar, day.and_time(NaiveTime::MIN))
}

/// Places an automatic task in the **forward** pass: its earliest dates.
///
/// # What it does
/// 1. Starts from the latest lower limit on the start (`limits.start`).
/// 2. Turns a lower limit on the finish into one on the start (finish limit
///    minus duration, on the task's calendar) and keeps the later one. A
///    limit in non-working time is first moved on to the end of the
///    next working day (see `finish_goal`).
/// 3. Applies the constraint (table in the module notes).
/// 4. Moves the start forward to working time (milestones stay on a working
///    edge) and adds the duration plus any blocked time
///    ([`crate::blockers::span`]).
///
/// # Parameters
/// - `task`: the task (duration and constraint are read).
/// - `calendar`: the task's calendar.
/// - `limits`: lower limits; `start` must be `Some` (the project start at
///   least).
/// - `honor_upper`: apply SNLT/FNLT against the links (`true` when this pass
///   decides the dates).
///
/// # Returns
/// The [`Placement`].
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
///
/// # Example
/// ```
/// use engine::constraints::place_forward;
/// use engine::{Bound, Driver, Limits};
/// use model::{Calendar, Constraint, Duration, Task};
/// use chrono::NaiveDate;
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let cal = Calendar::standard();
/// // Links allow Thu 11 Mar 17:00; SNET Mon 15 Mar 08:00 wins.
/// let task = Task::new("Build the floor", Duration::from_days(2))
///     .with_constraint(Constraint::StartNoEarlierThan { date: at(15, 8) });
/// let limits = Limits { start: Some(Bound::new(at(11, 17), Driver::ProjectStart)), finish: None };
/// let placed = place_forward(&task, &cal, &limits, true).unwrap();
/// assert_eq!(placed.dates.start, at(15, 8));
/// assert_eq!(placed.dates.finish, at(16, 17));
/// ```
pub fn place_forward(
    task: &Task,
    calendar: &Calendar,
    limits: &Limits,
    honor_upper: bool,
) -> Result<Placement, CalendarError> {
    // The length to place: the task's work plus its blocked time (see
    // `blockers.rs`). Without blockers this is just `task.duration`.
    let duration = crate::blockers::span(task);
    // Every caller sets `limits.start`. We still avoid `expect` (which
    // would crash the program if it were missing) and use `unwrap_or_else`:
    // if `start` is `None`, the small function below runs instead and gives
    // a safe fallback.
    let first = limits.start.unwrap_or_else(|| {
        // Defensive fallback: a finish limit alone (never happens today).
        limits
            .finish
            .unwrap_or(Bound::new(NaiveDateTime::MIN, Driver::ProjectStart))
    });
    let mut best = Candidate {
        at: first.at,
        why: first,
        end: TaskEnd::Start,
    };
    if let Some(finish) = limits.finish {
        best.keep_later(Candidate {
            at: subtract_working_time(
                calendar,
                finish_goal(calendar, finish.at, duration)?,
                duration,
            )?,
            why: finish,
            end: TaskEnd::Finish,
        });
    }

    let constraint = task.constraint;
    // Two small helpers that build a candidate for a constraint on the
    // start or on the finish.
    let on_start = |date| Candidate {
        at: date,
        why: constraint_bound(constraint, date),
        end: TaskEnd::Start,
    };
    let on_finish = |date| -> Result<Candidate, CalendarError> {
        Ok(Candidate {
            at: subtract_working_time(calendar, date, duration)?,
            why: constraint_bound(constraint, date),
            end: TaskEnd::Finish,
        })
    };
    match constraint {
        Constraint::AsSoonAsPossible | Constraint::AsLateAsPossible => {}
        Constraint::StartNoEarlierThan { date } => best.keep_later(on_start(date)),
        Constraint::FinishNoEarlierThan { date } => {
            best.keep_later(on_finish(finish_goal(calendar, date, duration)?)?)
        }
        Constraint::MustStartOn { date } => best = on_start(date),
        Constraint::MustFinishOn { date } => best = on_finish(date)?,
        Constraint::StartNoLaterThan { date } if honor_upper => {
            best.keep_earlier(on_start(start_goal(calendar, date, duration)?))
        }
        Constraint::FinishNoLaterThan { date } if honor_upper => {
            best.keep_earlier(on_finish(date)?)
        }
        // SNLT / FNLT in a pass that only measures: no effect.
        Constraint::StartNoLaterThan { .. } | Constraint::FinishNoLaterThan { .. } => {}
    }

    // Move to working time; milestones stay on a working edge (Tue 17:00
    // stays Tue 17:00, like MS Project).
    let start = if duration.is_zero() && touches_working_time(calendar, best.at) {
        best.at
    } else {
        snap_forward(calendar, best.at)?
    };
    let finish = add_working_time(calendar, start, duration)?;
    let placed_end = match best.end {
        TaskEnd::Start => start,
        TaskEnd::Finish => finish,
    };
    Ok(Placement {
        dates: TaskDates { start, finish },
        explanation: best.why.explain(best.end, placed_end, calendar.id),
    })
}

/// Places an automatic task in the **backward** pass: its latest dates.
///
/// # What it does
/// The mirror image of [`place_forward`]:
/// 1. Starts from the earliest upper limit on the finish.
/// 2. Turns an upper limit on the start into one on the finish (start limit
///    plus duration) and keeps the earlier one.
/// 3. Applies the **deadline** (finish ≤ deadline), then the constraint.
/// 4. Moves the finish backward to working time and subtracts the duration
///    plus any blocked time ([`crate::blockers::span`]).
///
/// # Parameters
/// - `task`: the task.
/// - `calendar`: the task's calendar.
/// - `limits`: upper limits; `finish` must be `Some` (the project finish at
///   least).
/// - `honor_lower`: apply SNET/FNET against the links (`true` when this
///   pass decides the dates, i.e. scheduling from the finish).
///
/// # Returns
/// The [`Placement`].
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
///
/// # Example
/// ```
/// use engine::constraints::place_backward;
/// use engine::{Bound, Driver, Limits};
/// use model::{Calendar, Duration, Task};
/// use chrono::NaiveDate;
/// let at = |d, h| NaiveDate::from_ymd_opt(2027, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
/// let cal = Calendar::standard();
/// // Project finish Tue 23 Mar 17:00, but the deadline is Fri 19 Mar 17:00.
/// let mut task = Task::new("Paint", Duration::from_days(2));
/// task.deadline = Some(at(19, 17));
/// let limits = Limits { start: None, finish: Some(Bound::new(at(23, 17), Driver::ProjectFinish)) };
/// let placed = place_backward(&task, &cal, &limits, false).unwrap();
/// assert_eq!(placed.dates.finish, at(19, 17));
/// assert_eq!(placed.dates.start, at(18, 8));
/// ```
pub fn place_backward(
    task: &Task,
    calendar: &Calendar,
    limits: &Limits,
    honor_lower: bool,
) -> Result<Placement, CalendarError> {
    // Work plus blocked time, as in `place_forward`: late dates must leave
    // room for the blocked time too, or slack would be too large.
    let duration = crate::blockers::span(task);
    let first = limits.finish.unwrap_or_else(|| {
        limits
            .start
            .unwrap_or(Bound::new(NaiveDateTime::MAX, Driver::ProjectFinish))
    });
    let mut best = Candidate {
        at: first.at,
        why: first,
        end: TaskEnd::Finish,
    };
    if let Some(start) = limits.start {
        best.keep_earlier(Candidate {
            at: add_working_time(
                calendar,
                start_goal(calendar, start.at, duration)?,
                duration,
            )?,
            why: start,
            end: TaskEnd::Start,
        });
    }
    if let Some(deadline) = task.deadline {
        best.keep_earlier(Candidate {
            at: deadline,
            why: Bound::new(deadline, Driver::Deadline { date: deadline }),
            end: TaskEnd::Finish,
        });
    }

    let constraint = task.constraint;
    let on_finish = |date| Candidate {
        at: date,
        why: constraint_bound(constraint, date),
        end: TaskEnd::Finish,
    };
    let on_start = |date| -> Result<Candidate, CalendarError> {
        Ok(Candidate {
            at: add_working_time(calendar, date, duration)?,
            why: constraint_bound(constraint, date),
            end: TaskEnd::Start,
        })
    };
    match constraint {
        Constraint::AsSoonAsPossible | Constraint::AsLateAsPossible => {}
        Constraint::StartNoLaterThan { date } => {
            best.keep_earlier(on_start(start_goal(calendar, date, duration)?)?)
        }
        Constraint::FinishNoLaterThan { date } => best.keep_earlier(on_finish(date)),
        Constraint::MustStartOn { date } => best = on_start(date)?,
        Constraint::MustFinishOn { date } => best = on_finish(date),
        Constraint::StartNoEarlierThan { date } if honor_lower => best.keep_later(on_start(date)?),
        Constraint::FinishNoEarlierThan { date } if honor_lower => {
            best.keep_later(on_finish(finish_goal(calendar, date, duration)?))
        }
        Constraint::StartNoEarlierThan { .. } | Constraint::FinishNoEarlierThan { .. } => {}
    }

    let finish = if duration.is_zero() && touches_working_time(calendar, best.at) {
        best.at
    } else {
        snap_backward(calendar, best.at)?
    };
    let start = subtract_working_time(calendar, finish, duration)?;
    let placed_end = match best.end {
        TaskEnd::Start => start,
        TaskEnd::Finish => finish,
    };
    Ok(Placement {
        dates: TaskDates { start, finish },
        explanation: best.why.explain(best.end, placed_end, calendar.id),
    })
}

/// Adds a **summary task's** own lower limits to the limits its gate passes
/// down: SNET (on the starts of its subtasks) and FNET (on their finishes).
///
/// # Why only these
/// Constraints on summary tasks are rare (MS Project discourages them). M1
/// supports the useful ones: "Phase 2 starts no earlier than 1 July"
/// (SNET), "finishes no earlier than" (FNET), and, in the backward
/// direction ([`summary_upper_limits`]), FNLT, SNLT and the deadline. MSO,
/// MFO and ALAP on a summary task are ignored.
///
/// # Parameters
/// - `summary`: the summary task.
/// - `limits`: the gate's limits, extended in place.
pub fn summary_lower_limits(summary: &Task, limits: &mut Limits) {
    let mark = |date| {
        let mut b = constraint_bound(summary.constraint, date);
        b.via_summary = Some(summary.id);
        b
    };
    match summary.constraint {
        Constraint::StartNoEarlierThan { date } => limits.later_start(mark(date)),
        Constraint::FinishNoEarlierThan { date } => limits.later_finish(mark(date)),
        _ => {}
    }
}

/// Adds a **summary task's** own upper limits to the limits it passes down
/// in the backward pass: SNLT, FNLT and its deadline.
///
/// # Parameters
/// - `summary`: the summary task.
/// - `limits`: the limits, extended in place.
pub fn summary_upper_limits(summary: &Task, limits: &mut Limits) {
    let mark = |mut b: Bound| {
        b.via_summary = Some(summary.id);
        b
    };
    match summary.constraint {
        Constraint::StartNoLaterThan { date } => {
            limits.earlier_start(mark(constraint_bound(summary.constraint, date)))
        }
        Constraint::FinishNoLaterThan { date } => {
            limits.earlier_finish(mark(constraint_bound(summary.constraint, date)))
        }
        _ => {}
    }
    if let Some(date) = summary.deadline {
        limits.earlier_finish(mark(Bound::new(date, Driver::Deadline { date })));
    }
}

/// Moves **ALAP** tasks as late as possible (scheduling from the start).
///
/// # The rule
/// An "As Late As Possible" task is moved as late as it can go **without
/// moving any other task as scheduled**: it must still finish before each
/// successor needs it (using the successor's *current* dates), not after
/// the project finish, and not after its deadline. It never moves earlier
/// than its early dates. Tasks are handled successors-first (reverse
/// topological order), so a chain of ALAP tasks slides together.
///
/// Links out of the task's summary tasks count too: moving a subtask later
/// must not push its summary task past that summary's successors.
///
/// ```text
/// "Buy materials" (3d, ALAP): early Wed 3 - Fri 5 Mar 2027
///   successor "Build the floor" starts Fri 12 Mar 08:00 (FS)
///   -> finish <= Thu 11 Mar 17:00 -> Tue 9 Mar 08:00 - Thu 11 Mar 17:00
/// ```
///
/// # Parameters
/// - `context`: the prepared project.
/// - `scheduled`: the task dates (early dates on entry), changed in place.
/// - `explanations`: changed in place for moved tasks.
/// - `durations`: working durations per row (for percentage lags).
/// - `project_finishes`: the finish of each row's project.
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
pub fn schedule_alap(
    context: &Context,
    scheduled: &mut [TaskDates],
    explanations: &mut [Explanation],
    durations: &[Duration],
    project_finishes: &[NaiveDateTime],
) -> Result<(), CalendarError> {
    let graph = &context.graph;
    let project = context.project;
    for &node in graph.order.iter().rev() {
        if graph.is_gate(node) || context.outline.is_summary(node) {
            continue;
        }
        let row = node;
        let task = &project.tasks[row];
        if task.mode != TaskMode::Auto || task.constraint != Constraint::AsLateAsPossible {
            continue;
        }
        let mut limits = Limits {
            start: None,
            finish: Some(Bound::new(project_finishes[row], Driver::ProjectFinish)),
        };
        // The task itself, then each summary task above it.
        let mut owners = vec![row];
        owners.extend(context.outline.ancestors(row));
        for owner in owners {
            for edge in &graph.successors[owner] {
                if !matches!(edge.kind, EdgeKind::Link { .. }) {
                    continue;
                }
                let q = graph.row_of(edge.node);
                let q_dates = if context.outline.is_summary(q) {
                    scheduled_roll_up(&context.outline, q, scheduled)
                } else {
                    scheduled[q]
                };
                let (on_start, on_finish) = backward::link_limit(
                    context.project_calendar,
                    &edge.kind,
                    project.tasks[q].id,
                    q_dates,
                    durations[owner],
                )?;
                let mark = |mut b: Bound| {
                    if owner != row {
                        b.via_summary = Some(project.tasks[owner].id);
                    }
                    b
                };
                if let Some(b) = on_start {
                    limits.earlier_start(mark(b));
                }
                if let Some(b) = on_finish {
                    limits.earlier_finish(mark(b));
                }
            }
        }
        let placement = place_backward(task, context.calendars[row], &limits, false)?;
        if placement.dates.start > scheduled[row].start {
            scheduled[row] = placement.dates;
            explanations[row] = placement.explanation;
        }
    }
    Ok(())
}

/// Moves **ASAP** tasks as early as possible (scheduling from the finish).
///
/// # The rule
/// The mirror image of [`schedule_alap`]. When a project is scheduled from
/// its finish, every task first gets its *late* dates. An "As Soon As
/// Possible" task is then moved as early as it can go without moving any
/// other task as scheduled: not before each predecessor (as scheduled), not
/// before the project start, and never later than its late dates.
/// Predecessors-first (topological order).
///
/// # Parameters
/// - `context`: the prepared project.
/// - `scheduled`: the task dates (late dates on entry), changed in place.
/// - `explanations`: changed in place for moved tasks.
/// - `durations`: working durations per row (for percentage lags).
/// - `project_start`: the calculated project start.
///
/// # Errors
/// A [`CalendarError`] from calendar arithmetic.
pub fn schedule_asap(
    context: &Context,
    scheduled: &mut [TaskDates],
    explanations: &mut [Explanation],
    durations: &[Duration],
    project_start: NaiveDateTime,
) -> Result<(), CalendarError> {
    let graph = &context.graph;
    let project = context.project;
    for &node in &graph.order {
        if graph.is_gate(node) || context.outline.is_summary(node) {
            continue;
        }
        let row = node;
        let task = &project.tasks[row];
        if task.mode != TaskMode::Auto || task.constraint != Constraint::AsSoonAsPossible {
            continue;
        }
        let mut limits = Limits {
            start: Some(Bound::new(project_start, Driver::ProjectStart)),
            finish: None,
        };
        // Links into the task itself, and into each summary task above it
        // (those end at the summary's gate node).
        let mut entries = vec![(row, row)];
        for ancestor in context.outline.ancestors(row) {
            if let Some(gate) = graph.gate_of[ancestor] {
                entries.push((ancestor, gate));
            }
        }
        for (owner, entry) in entries {
            for edge in &graph.predecessors[entry] {
                if !matches!(edge.kind, EdgeKind::Link { .. }) {
                    continue;
                }
                let p = edge.node;
                let p_dates = if context.outline.is_summary(p) {
                    scheduled_roll_up(&context.outline, p, scheduled)
                } else {
                    scheduled[p]
                };
                let (on_start, on_finish) = forward::link_limit(
                    context.project_calendar,
                    &edge.kind,
                    project.tasks[p].id,
                    p_dates,
                    durations[p],
                )?;
                let mark = |mut b: Bound| {
                    if owner != row {
                        b.via_summary = Some(project.tasks[owner].id);
                    }
                    b
                };
                if let Some(b) = on_start {
                    limits.later_start(mark(b));
                }
                if let Some(b) = on_finish {
                    limits.later_finish(mark(b));
                }
            }
        }
        let placement = place_forward(task, context.calendars[row], &limits, false)?;
        if placement.dates.start < scheduled[row].start {
            scheduled[row] = placement.dates;
            explanations[row] = placement.explanation;
        }
    }
    Ok(())
}
