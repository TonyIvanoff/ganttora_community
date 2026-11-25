//! # Change records: "who changed what, and when"
//!
//! ## What this file is for
//! Every command that changes the project produces **change records**: one
//! record per changed field of one item. They are the raw material of the
//! change journal that the `.gtra` file stores (book section
//! 4.4). This file defines [`FieldChange`] (what changed) and
//! [`ChangeContext`] (who, when), and the step that stamps one with the
//! other into a [`ChangeRecord`].
//!
//! The finished record type [`ChangeRecord`] and its helpers [`BatchId`] and
//! [`EntityKind`] live in the `model` crate (since M1), so that the `gtra`
//! crate can store records without depending on `document`. They are
//! re-exported here, so `document::ChangeRecord` still works.
//!
//! ## Where it fits
//! ```text
//!  command.rs                  journal.rs                    lib.rs
//!  executes a command  ──►  FieldChange (what changed)  ──►  pending_changes
//!                           + ChangeContext (who, when)      (drained later
//!                           = ChangeRecord                    by the file layer)
//! ```
//!
//! ## Main ideas
//! - **Field-level**: one record = one field of one item. Changing task 2
//!   "Get building permit" from 5d to 6d gives exactly one record:
//!
//!   | entity | entity_id | field | old_value | new_value |
//!   |---|---|---|---|---|
//!   | task | 0b7d… | duration_minutes | 2400 | 2880 |
//!
//!   (Durations are stored in minutes of working time: 5 days × 480 = 2400.)
//! - **Deterministic**: the document never reads the clock and never makes
//!   random IDs. The caller (the Tauri shell) passes the author, the
//!   timestamp and the batch ID in a [`ChangeContext`]. The same commands
//!   with the same contexts always give the same records.
//! - **Field names** are the model's field names in snake_case, the same as
//!   the `.gtra` column names: `name`, `duration_minutes`, `outline_level`,
//!   `mode`, `manual_start`, `milestone`, `constraint`, `deadline`,
//!   `calendar_id`, `notes`, `hyperlink`; for links `predecessor`,
//!   `successor`, `kind`, `lag_minutes`, `lag_percent`; `index` is the row
//!   (or list position) of a created or deleted item.
//! - **In memory only**: records wait in the document's pending list. The
//!   shell hands them to the file layer (`gtra`), which writes them into the
//!   `journal` table; `document` itself never touches files.
//! - Only **what the user changed** is recorded. New dates calculated by the
//!   engine are not, because the engine recalculates them every time.
//!
//! ## Documentation

use chrono::{DateTime, Utc};
pub use model::{BatchId, ChangeRecord, EntityKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Who makes an edit, when, and as part of which user action.
///
/// The caller fills this in for every [`crate::Document::apply`],
/// [`crate::Document::undo`] and [`crate::Document::redo`] call.
///
/// # Fields
/// - `author`: the display name from Settings → User, e.g.
///   `"Sam Lee"`.
/// - `timestamp`: the moment of the edit, in UTC (Coordinated Universal
///   Time, the world's reference clock), so records made in different time
///   zones sort correctly.
/// - `batch_id`: one fresh [`BatchId`] per user action.
///
/// # Example
/// ```
/// use chrono::{TimeZone, Utc};
/// use document::{BatchId, ChangeContext};
/// let context = ChangeContext {
///     author: "Sam Lee".to_string(),
///     timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
///     batch_id: BatchId::new(),
/// };
/// assert_eq!(context.author, "Sam Lee");
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeContext {
    /// Display name of the person making the edit.
    pub author: String,
    /// When the edit is made (UTC).
    pub timestamp: DateTime<Utc>,
    /// The user action this edit belongs to.
    pub batch_id: BatchId,
}

/// One changed field, before it is stamped with author, time and batch.
///
/// Commands produce these (see `command.rs`); the document then turns them
/// into [`ChangeRecord`]s with [`FieldChange::stamp`].
///
/// # Fields
/// - `entity`, `entity_id`: which item changed.
/// - `field`: which field, in snake_case like the model and the `.gtra`
///   columns (`"name"`, `"duration_minutes"`, `"outline_level"`, …).
/// - `old_value`, `new_value`: the value before and after, as JSON. `null`
///   as the old value means "the item was created"; `null` as the new value
///   means "the item was deleted".
///
/// # Example
/// ```
/// use document::{EntityKind, FieldChange};
/// use serde_json::json;
/// use uuid::Uuid;
/// let change = FieldChange {
///     entity: EntityKind::Task,
///     entity_id: Uuid::nil(),
///     field: "duration_minutes".to_string(),
///     old_value: json!(2400), // 5 days × 480 minutes
///     new_value: json!(2880), // 6 days × 480 minutes
/// };
/// assert_eq!(change.field, "duration_minutes");
/// ```
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    /// What kind of item changed.
    pub entity: EntityKind,
    /// The UUID of the item that changed.
    pub entity_id: Uuid,
    /// The name of the changed field.
    pub field: String,
    /// The value before the change (`null` = the item did not exist).
    pub old_value: serde_json::Value,
    /// The value after the change (`null` = the item no longer exists).
    pub new_value: serde_json::Value,
}

impl FieldChange {
    /// Turns this change into a full [`ChangeRecord`] by adding who made it,
    /// when, and in which batch.
    ///
    /// # Parameters
    /// - `context`: the author, timestamp and batch ID of the user action.
    ///
    /// # Returns
    /// The finished record. `self` is consumed (moved into the record).
    ///
    /// # Example
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use document::{BatchId, ChangeContext, EntityKind, FieldChange};
    /// use serde_json::json;
    /// use uuid::Uuid;
    /// let context = ChangeContext {
    ///     author: "Sam Lee".to_string(),
    ///     timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
    ///     batch_id: BatchId::from_uuid(Uuid::nil()),
    /// };
    /// let change = FieldChange {
    ///     entity: EntityKind::Task,
    ///     entity_id: Uuid::nil(),
    ///     field: "name".to_string(),
    ///     old_value: json!("Paint"),
    ///     new_value: json!("Paint walls"),
    /// };
    /// let record = change.stamp(&context);
    /// assert_eq!(record.author, "Sam Lee");
    /// assert_eq!(record.new_value, json!("Paint walls"));
    /// ```
    ///
    /// # Beginner note: `self` without `&`
    /// The first parameter is `self`, not `&self`. That means the method
    /// takes **ownership** of the change: after calling `stamp`, the old
    /// `FieldChange` is gone and its text and values live on inside the new
    /// record. No copy is needed.4 on ownership.
    pub fn stamp(self, context: &ChangeContext) -> ChangeRecord {
        ChangeRecord {
            timestamp: context.timestamp,
            author: context.author.clone(),
            batch_id: context.batch_id,
            entity: self.entity,
            entity_id: self.entity_id,
            field: self.field,
            old_value: self.old_value,
            new_value: self.new_value,
        }
    }
}
