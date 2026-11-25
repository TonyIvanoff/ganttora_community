//! # Dependencies: "this task waits for that task"
//!
//! ## What this file is for
//! Defines [`Dependency`] (also called a *link*) and its four types
//! ([`DependencyType`]): Finish-to-Start, Start-to-Start, Finish-to-Finish
//! and Start-to-Finish.
//!
//! ## Where it fits
//! Part of the `model` crate. A [`Project`](crate::Project) holds a list of
//! dependencies. The engine builds a graph from them (`engine::cpm::graph`)
//! and uses them in the forward and backward passes.
//!
//! ## Main ideas
//! - The **predecessor** comes first; the **successor** waits for it.
//! - The **type** says which ends are connected: Finish-to-Start (FS, the
//!   default and by far the most common), Start-to-Start (SS),
//!   Finish-to-Finish (FF) and Start-to-Finish (SF). The engine supports all
//!   four (since M1).
//! - **Lag** is extra waiting time; a negative lag is a **lead** (overlap).
//!   A lag is either a fixed amount of working time (`lag`) or a percentage
//!   of the predecessor's duration (`lag_percent`), like MS Project's
//!   "2FS+50%".
//!
//! ## Worked example
//! ```text
//! 7FS+2d   -> predecessor task 7, Finish-to-Start, lag +2 days (960 min)
//! 1FS-1d   -> predecessor task 1, Finish-to-Start, lead 1 day (lag -480 min)
//! 2SS+50%  -> predecessor task 2, Start-to-Start, lag = half of task 2's
//!             duration (task 2 lasts 4d, so the lag is 2d = 960 min)
//! ```
//!
//! ## Documentation

use crate::duration::Duration;
use crate::ids::{DependencyId, TaskId};
use serde::{Deserialize, Serialize};

/// The four kinds of dependency. The name says which end of the predecessor
/// is connected to which end of the successor.
///
/// # Variants
/// - `FinishToStart` (FS): the successor starts after the predecessor
///   finishes. The default.
/// - `StartToStart` (SS): the successor starts after the predecessor starts.
/// - `FinishToFinish` (FF): the successor finishes after the predecessor
///   finishes.
/// - `StartToFinish` (SF): the successor finishes after the predecessor
///   starts. Rare.
///
/// # JSON
/// A plain camelCase string: `"finishToStart"`, `"startToStart"`,
/// `"finishToFinish"` or `"startToFinish"`.
///
/// # Example
/// ```
/// use model::DependencyType;
/// assert_eq!(DependencyType::default(), DependencyType::FinishToStart);
/// assert_eq!(serde_json::to_string(&DependencyType::StartToStart).unwrap(), "\"startToStart\"");
/// ```
///
/// # Beginner note: enums
/// An `enum` is a type whose value is exactly one of a fixed list of choices.
/// You wrote this very enum in the project documentation, "Data structures".
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum DependencyType {
    /// Finish-to-Start (FS): successor starts after predecessor finishes.
    #[default]
    FinishToStart,
    /// Start-to-Start (SS): successor starts after predecessor starts.
    StartToStart,
    /// Finish-to-Finish (FF): successor finishes after predecessor finishes.
    FinishToFinish,
    /// Start-to-Finish (SF): successor finishes after predecessor starts.
    StartToFinish,
}

/// A link between two tasks.
///
/// # Fields
/// - `id`: unique ID of the link.
/// - `predecessor`: the task that comes first.
/// - `successor`: the task that waits.
/// - `kind`: which ends are connected (FS, SS, FF, SF).
/// - `lag`: extra working time between the two; negative for a lead. Used
///   when `lag_percent` is `None`.
/// - `lag_percent`: a lag given as a whole percentage of the predecessor's
///   duration (50 = 50 %, -25 = a lead of 25 %). When set, it replaces
///   `lag`. Example: predecessor 4 days, `lag_percent` 50 → lag 2 days.
///
/// # Example
/// ```
/// use model::{Dependency, Duration, Task};
/// let paint = Task::new("Paint", Duration::from_days(2));
/// let move_in = Task::new("Move tools in", Duration::from_days(1));
/// // "Move tools in" waits 2 days after "Paint" finishes (the paint dries).
/// let link = Dependency::finish_to_start(paint.id, move_in.id)
///     .with_lag(Duration::from_days(2));
/// assert_eq!(link.lag.minutes(), 960);
/// ```
///
/// # Beginner note: why `kind` and not `type`
/// `type` is a reserved word in Rust (it is used to give a type a new name),
/// so a field cannot be called `type`. We call it `kind` instead.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// In JSON, field names are written in camelCase (`outline_level` becomes
// `outlineLevel`), the naming style of the TypeScript UI.
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// Unique ID of the link.
    pub id: DependencyId,
    /// The task that comes first.
    pub predecessor: TaskId,
    /// The task that waits for the predecessor.
    pub successor: TaskId,
    /// Which ends of the two tasks are connected.
    pub kind: DependencyType,
    /// Extra working time between the tasks; negative means lead (overlap).
    pub lag: Duration,
    /// Lag as a percentage of the predecessor's duration; replaces `lag`
    /// when set. Missing in JSON = `None` (M0 files have no such field).
    #[serde(default)]
    pub lag_percent: Option<i32>,
}

impl Dependency {
    /// Creates a link of any type with no lag and a new random ID.
    ///
    /// # Parameters
    /// - `predecessor`: ID of the task that comes first.
    /// - `successor`: ID of the task that waits.
    /// - `kind`: the dependency type.
    ///
    /// # Returns
    /// The new dependency.
    ///
    /// # Example
    /// ```
    /// use model::{Dependency, DependencyType, TaskId};
    /// let link = Dependency::new(TaskId::new(), TaskId::new(), DependencyType::StartToStart);
    /// assert_eq!(link.kind, DependencyType::StartToStart);
    /// ```
    pub fn new(predecessor: TaskId, successor: TaskId, kind: DependencyType) -> Self {
        Dependency {
            id: DependencyId::new(),
            predecessor,
            successor,
            kind,
            lag: Duration::ZERO,
            lag_percent: None,
        }
    }

    /// Creates a Finish-to-Start link with no lag, the most common case.
    ///
    /// # Parameters
    /// - `predecessor`: ID of the task that comes first.
    /// - `successor`: ID of the task that waits.
    ///
    /// # Returns
    /// The new FS dependency.
    ///
    /// # Example
    /// ```
    /// use model::{Dependency, DependencyType, TaskId};
    /// let link = Dependency::finish_to_start(TaskId::new(), TaskId::new());
    /// assert_eq!(link.kind, DependencyType::FinishToStart);
    /// assert!(link.lag.is_zero());
    /// ```
    pub fn finish_to_start(predecessor: TaskId, successor: TaskId) -> Self {
        Dependency::new(predecessor, successor, DependencyType::FinishToStart)
    }

    /// Returns the same link with a different lag. Handy for chaining:
    /// `Dependency::finish_to_start(a, b).with_lag(...)`.
    ///
    /// # Parameters
    /// - `lag`: the new lag; negative for a lead.
    ///
    /// # Returns
    /// The modified dependency.
    ///
    /// # Example
    /// ```
    /// use model::{Dependency, Duration, TaskId};
    /// let lead = Dependency::finish_to_start(TaskId::new(), TaskId::new())
    ///     .with_lag(Duration::from_days(-1)); // 1FS-1d
    /// assert_eq!(lead.lag.minutes(), -480);
    /// ```
    ///
    /// # Beginner note: `mut self`
    /// The method takes the dependency by value (`self`), changes it, and
    /// hands it back. `mut` allows the change.
    pub fn with_lag(mut self, lag: Duration) -> Self {
        self.lag = lag;
        self.lag_percent = None;
        self
    }

    /// Returns the same link with a percentage lag (MS Project's "+50%").
    ///
    /// # Parameters
    /// - `percent`: whole percent of the predecessor's duration; negative
    ///   for a lead.
    ///
    /// # Returns
    /// The modified dependency (`lag` is reset to zero, because
    /// `lag_percent` now decides the lag).
    ///
    /// # Example
    /// ```
    /// use model::{Dependency, DependencyType, TaskId};
    /// let half = Dependency::new(TaskId::new(), TaskId::new(), DependencyType::StartToStart)
    ///     .with_lag_percent(50); // 2SS+50%
    /// assert_eq!(half.lag_percent, Some(50));
    /// ```
    pub fn with_lag_percent(mut self, percent: i32) -> Self {
        self.lag = Duration::ZERO;
        self.lag_percent = Some(percent);
        self
    }

    /// Returns the same link with another dependency type.
    ///
    /// # Example
    /// ```
    /// use model::{Dependency, DependencyType, TaskId};
    /// let ff = Dependency::finish_to_start(TaskId::new(), TaskId::new())
    ///     .with_kind(DependencyType::FinishToFinish);
    /// assert_eq!(ff.kind, DependencyType::FinishToFinish);
    /// ```
    pub fn with_kind(mut self, kind: DependencyType) -> Self {
        self.kind = kind;
        self
    }
}
