//! # Crate `model`: the words Ganttora uses to describe a project
//!
//! ## What this crate is for
//! This crate defines the *data* of a project: tasks, dependencies (links),
//! durations, calendars, the project itself, and the change records of the
//! journal, and the blockers (unplanned delays) of a task. It contains **no scheduling
//! logic**. It only says what a project *looks like*, not how its dates are
//! calculated. The calculation lives in the `engine` crate, which reads these
//! types and produces dates.
//!
//! ## Where it fits
//! Almost every other Rust crate in Ganttora depends on `model`:
//!
//! ```text
//!            model  (this crate: data only)
//!          /   |    \
//!     engine  gtra  document ...   (they all read and write model types)
//! ```
//!
//! - `engine` reads a [`Project`] and calculates dates.
//! - `gtra` saves a [`Project`] to a `.gtra` file and loads it again.
//! - `document` keeps a [`Project`] open while the user edits it.
//!
//! ## Main ideas to know first
//! - **Every entity has a UUID**, a random 128-bit number that is unique
//!   worldwide, wrapped in its own type ([`TaskId`], [`DependencyId`]…).
//!   We never identify a task by its row number, because row numbers change
//!   when rows are inserted or moved.
//! - **Durations are counted in working minutes** ([`Duration`]). "1 day" is
//!   480 minutes on the default 8-hour day, like in MS Project.
//! - **Every type can be turned into JSON and back** (serde
//!   `Serialize`/`Deserialize`), so the user interface can receive it.
//!   One convention everywhere: struct fields in camelCase; enums without
//!   data as plain camelCase strings (`"finishToStart"`); enums with data as
//!   objects with a `"type"` field (`{"type": "mustStartOn", "date": …}`).
//! - **The outline** (WBS) is stored as row order + outline level; parents
//!   and summary tasks are derived from it ([`Outline`]).
//!
//! ## Beginner note: modules and `pub use`
//! A Rust crate is split into *modules*, one per file. `mod ids;` below tells
//! the compiler "there is a file `ids.rs`, include it". `pub use ids::TaskId;`
//! then re-exports the type, so other crates can write `model::TaskId`
//! instead of the longer `model::ids::TaskId`.5,
//! "The architecture".
//!
//! ## Documentation

// Each line declares one module (one file in `src/`). `pub mod` makes the
// module visible to other crates as well.
pub mod blocker;
pub mod calendar;
pub mod dependency;
pub mod duration;
pub mod ids;
pub mod journal;
pub mod outline;
pub mod project;
pub mod risk;
pub mod task;

// Re-exports: the most used types, available directly as `model::Name`.
pub use blocker::Blocker;
pub use calendar::{Calendar, CalendarException, WorkWeek, WorkingTime};
pub use dependency::{Dependency, DependencyType};
pub use duration::{DEFAULT_MINUTES_PER_DAY, Duration};
pub use ids::{BlockerId, CalendarId, DependencyId, ProjectId, RiskId, TaskId};
pub use journal::{BatchId, ChangeRecord, EntityKind};
pub use outline::{Outline, first_invalid_row};
pub use project::{Project, ScheduleFrom};
pub use risk::{Risk, RiskPriority, RiskResponse};
pub use task::{Constraint, Task, TaskMode};
