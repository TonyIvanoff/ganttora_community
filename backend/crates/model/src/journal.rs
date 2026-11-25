//! # Change records: one row of the change journal
//!
//! ## What this file is for
//! Defines [`ChangeRecord`], one field-level change ("Sam Lee changed the
//! duration of task 0b7d… from 2400 to 2880 minutes"), plus the small types
//! it needs: [`BatchId`] (which user action it belongs to) and
//! [`EntityKind`] (what kind of item changed).
//!
//! ## Where it fits
//! The `document` crate *creates* these records when commands run; the
//! `gtra` crate *stores* them in the `journal` table of the `.gtra` file
//! They live here in `model`, the crate both depend on, so
//! neither has to depend on the other (dependency rule).
//!
//! ## Main ideas
//! - **Field-level**: one record = one field of one item.
//! - **Field names** are the model's field names in snake_case, the same
//!   names as the `.gtra` columns, e.g. `name`, `duration_minutes`,
//!   `outline_level`, `lag_minutes`. Values are JSON: durations in working
//!   minutes, dates as `"2027-03-01T08:00:00"`.
//!
//!   | entity | entity_id | field | old_value | new_value |
//!   |---|---|---|---|---|
//!   | task | 0b7d… | duration_minutes | 2400 | 2880 |
//!
//! ## Documentation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// The ID of one **batch**: the group of change records made by one single
/// user action (one command, one undo or one redo).
///
/// # What it is
/// A UUID, like every other ID in Ganttora. All records of a
/// batch share it, so the journal can later show "Sam Lee pasted 2
/// durations" as one line, or roll back the whole action at once.
///
/// # Example
/// ```
/// use model::BatchId;
/// use uuid::Uuid;
/// // The shell makes a fresh random ID for each user action...
/// let fresh = BatchId::new();
/// // ...while tests and examples can use a fixed one, for repeatable output.
/// let fixed = BatchId::from_uuid(Uuid::nil());
/// assert_ne!(fresh, fixed);
/// ```
///
/// # Beginner note: why the document never calls `BatchId::new()` itself
/// `new()` makes a *random* UUID. Randomness inside the document would make
/// it non-deterministic: the same edit would give different records each
/// time. So the caller creates the ID and passes it in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
// `transparent`: in JSON a BatchId is just the UUID text, not `{"0": "..."}`.
#[serde(transparent)]
pub struct BatchId(Uuid);

impl BatchId {
    /// Creates a new, random batch ID (UUID version 4).
    ///
    /// # Returns
    /// A batch ID that is, for all practical purposes, unique in the world.
    ///
    /// # Example
    /// ```
    /// use model::BatchId;
    /// assert_ne!(BatchId::new(), BatchId::new());
    /// ```
    pub fn new() -> Self {
        BatchId(Uuid::new_v4())
    }

    /// Wraps an existing UUID, for example one read back from a file.
    ///
    /// # Parameters
    /// - `uuid`: the UUID to wrap.
    ///
    /// # Example
    /// ```
    /// use model::BatchId;
    /// use uuid::Uuid;
    /// assert!(BatchId::from_uuid(Uuid::nil()).as_uuid().is_nil());
    /// ```
    pub fn from_uuid(uuid: Uuid) -> Self {
        BatchId(uuid)
    }

    /// Returns the UUID inside this batch ID.
    ///
    /// # Example
    /// ```
    /// use model::BatchId;
    /// use uuid::Uuid;
    /// assert_eq!(BatchId::from_uuid(Uuid::nil()).as_uuid(), Uuid::nil());
    /// ```
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for BatchId {
    /// Same as [`BatchId::new`]: a fresh random ID.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for BatchId {
    /// Writes the UUID in its usual text form, e.g.
    /// `"00000000-0000-0000-0000-000000000000"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What kind of item a change record is about.
///
/// An **entity** is one kind of item in the project.
/// Resources and assignments are added in M2.
///
/// In JSON the variants are written in lower camelCase: `"project"`,
/// `"task"`, `"dependency"`, `"calendar"`.
///
/// # Example
/// ```
/// use model::EntityKind;
/// assert_eq!(serde_json::to_string(&EntityKind::Task).unwrap(), "\"task\"");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    /// The project itself (for example its start date).
    Project,
    /// One task.
    Task,
    /// One dependency (link) between two tasks.
    Dependency,
    /// One calendar.
    Calendar,
}

/// One finished change record: the in-memory form of one row of the
/// `journal` table.
///
/// The `id` column of the table (1, 2, 3, …) is not here: the file layer
/// numbers the records when it stores them.
///
/// # Fields
/// Same as the journal columns in the project documentation: `timestamp`, `author`,
/// `entity`, `entity_id`, `field`, `old_value`, `new_value`, `batch_id`.
///
/// # JSON shape
/// ```text
/// { "timestamp": "2027-02-15T10:42:07Z", "author": "Sam Lee",
///   "batchId": "…", "entity": "task", "entityId": "…",
///   "field": "duration_minutes", "oldValue": 2400, "newValue": 2880 }
/// ```
///
/// # Example
/// ```
/// use chrono::{TimeZone, Utc};
/// use model::{BatchId, ChangeRecord, EntityKind};
/// use serde_json::json;
/// use uuid::Uuid;
/// let record = ChangeRecord {
///     timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
///     author: "Sam Lee".to_string(),
///     batch_id: BatchId::from_uuid(Uuid::nil()),
///     entity: EntityKind::Task,
///     entity_id: Uuid::nil(),
///     field: "duration_minutes".to_string(),
///     old_value: json!(2400), // 5 days x 480 minutes
///     new_value: json!(2880), // 6 days x 480 minutes
/// };
/// assert_eq!(record.field, "duration_minutes");
/// ```
///
/// In the `document` crate, `FieldChange::stamp` creates these records.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRecord {
    /// When the change was made (UTC).
    pub timestamp: DateTime<Utc>,
    /// Who made it (display name).
    pub author: String,
    /// The user action it belongs to.
    pub batch_id: BatchId,
    /// What kind of item changed.
    pub entity: EntityKind,
    /// Which item changed (its UUID).
    pub entity_id: Uuid,
    /// Which field changed.
    pub field: String,
    /// The value before (`null` = created).
    pub old_value: serde_json::Value,
    /// The value after (`null` = deleted).
    pub new_value: serde_json::Value,
}
