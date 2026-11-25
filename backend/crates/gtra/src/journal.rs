//! Storing and reading the change journal inside a `.gtra` file.
//!
//! # What this file is for
//! Every edit in Ganttora produces **change records**: "Sam changed the
//! duration of task X from 5 days to 6 days at 10:42". They power "who
//! changed what" and, later, version history (§5). This module
//! appends such records to the `journal` table and reads them back.
//!
//! # Where it fits
//! ```text
//!   document crate (makes a model::ChangeRecord for each changed field)
//!        │  (the Tauri shell passes them on, through ProjectSession)
//!        ▼
//!   journal.rs: GtraFile::append_journal / read_journal   ◄── you are here
//!        │
//!        ▼
//!   table `journal` (created by migrations/0001_init.sql)
//! ```
//!
//! # The table, one row per changed field
//! ```text
//!   journal
//!   ┌────┬──────────────────────┬─────────┬────────┬───────────┬──────────────────┬───────────┬───────────┬──────────┐
//!   │ id │ timestamp            │ author  │ entity │ entity_id │ field            │ old_value │ new_value │ batch_id │
//!   ├────┼──────────────────────┼─────────┼────────┼───────────┼──────────────────┼───────────┼───────────┼──────────┤
//!   │ 57 │ 2027-02-15T10:42:07Z │ Sam Lee │ task   │ 0b7d…     │ duration_minutes │ 2400      │ 2880      │ 9f3a…    │
//!   │ 58 │ 2027-02-15T10:45:00Z │ Sam Lee │ task   │ 77c1…     │ name             │ NULL      │ "Paint"   │ 1e20…    │
//!   │ 59 │ 2027-02-15T10:45:00Z │ Sam Lee │ task   │ 77c1…     │ duration_minutes │ NULL      │ 960       │ 1e20…    │
//!   └────┴──────────────────────┴─────────┴────────┴───────────┴──────────────────┴───────────┴───────────┴──────────┘
//!     id: numbered by SQLite, never reused          old NULL = created, new NULL = deleted
//!     a created (or deleted) item has one row per field (rows 58, 59, … above:
//!     the new task "Paint"), all with the same batch_id
//!     batch_id: shared by all records of one user action (one undo step)
//! ```
//!
//! # The record type: `model::ChangeRecord`
//! The records themselves are [`model::ChangeRecord`]s, the type the
//! `document` crate creates. The table's `id` column (1, 2, 3, …) is not part
//! of a `ChangeRecord`: the file gives the number when it stores a record.
//! [`GtraFile::append_journal`] returns those numbers, and
//! [`GtraFile::read_journal`] returns each record together with its number
//! as a [`JournalEntry`].
//!
//! # How a `ChangeRecord` becomes a row
//! ```text
//!   ChangeRecord field          journal column   stored as
//!   ─────────────────────────   ──────────────   ──────────────────────────────────────
//!   timestamp (DateTime<Utc>)   timestamp        '2027-02-15T10:42:07Z' (whole seconds)
//!   author                      author           text
//!   entity (EntityKind)         entity           'project' | 'task' | 'dependency' | 'calendar'
//!   entity_id (Uuid)            entity_id        UUID text
//!   field                       field            text, e.g. 'duration_minutes'
//!   old_value (JSON value)      old_value        JSON text, e.g. '2400'; JSON null → SQL NULL
//!   new_value (JSON value)      new_value        JSON text; JSON null → SQL NULL
//!   batch_id (BatchId)          batch_id         UUID text
//! ```
//! A JSON `null` is stored as SQL `NULL` and read back as `null`. This
//! matches the table's rule in `0001_init.sql` ("old_value NULL = created,
//! new_value NULL = deleted"). Sub-second parts of the timestamp are not
//! stored: the file keeps whole seconds.
//!
//! # Not here yet
//! Compaction (turning old records into snapshots) comes with
//! version history (`versions.rs`, M4).
//!

use chrono::{DateTime, NaiveDateTime, Utc};
use model::{BatchId, ChangeRecord, EntityKind};
use rusqlite::types::Value;
use uuid::Uuid;

use crate::{GtraError, GtraFile};

/// The timestamp format of the `journal.timestamp` column: ISO 8601 in UTC,
/// whole seconds, e.g. `2027-02-15T10:42:07Z` (see `0001_init.sql`).
const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// One stored change record: the record plus the number the file gave it.
///
/// # Fields
/// - `id`: the record's number in the `journal` table (1, 2, 3, …). Numbers
///   only grow and are never reused, so "record 57" always means the same
///   record. Use it with [`JournalFilter::after_id`] to read page by page.
/// - `record`: the change itself.
///
/// # Example
/// ```
/// use chrono::{TimeZone, Utc};
/// use model::{BatchId, ChangeRecord, EntityKind};
/// use serde_json::json;
/// let entry = gtra::JournalEntry {
///     id: 57,
///     record: ChangeRecord {
///         timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
///         author: "Sam Lee".into(),
///         batch_id: BatchId::new(),
///         entity: EntityKind::Task,
///         entity_id: uuid::Uuid::new_v4(),
///         field: "duration_minutes".into(),
///         old_value: json!(2400),
///         new_value: json!(2880),
///     },
/// };
/// assert_eq!(entry.record.new_value, json!(2880));
/// ```
///
/// # Beginner note
/// Why not put `id` inside `ChangeRecord`? A record that an edit has just
/// made has no number yet; only the file can give one. Keeping the two apart
/// means no record ever carries a made-up or missing number.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    /// The record's number in the file (the `journal.id` column).
    pub id: i64,
    /// The change record.
    pub record: ChangeRecord,
}

/// Which records [`GtraFile::read_journal`] should return. Every field is
/// optional; an empty filter (`JournalFilter::default()`) returns everything.
///
/// # Example
/// ```
/// // The 100 records after record 57 that belong to one task.
/// let filter = gtra::JournalFilter {
///     entity_id: Some(uuid::Uuid::new_v4()),
///     after_id: Some(57),
///     limit: Some(100),
///     ..Default::default()
/// };
/// assert!(filter.batch_id.is_none());
/// ```
///
/// # Beginner note
/// `..Default::default()` fills in all the fields not written out with
/// their default values (`None` here). `#[derive(Default)]` below creates
/// that default automatically.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalFilter {
    /// Only records about this item.
    pub entity_id: Option<Uuid>,
    /// Only records of this user action.
    pub batch_id: Option<BatchId>,
    /// Only records with a larger `id` (for reading page by page).
    pub after_id: Option<i64>,
    /// At most this many records.
    pub limit: Option<u32>,
}

/// What [`GtraFile::read_journal_report`] found: the records it could read,
/// and how many it had to skip.
///
/// # Fields
/// - `entries`: the readable records, oldest first.
/// - `skipped_unknown`: how many rows were left out because their entity
///   kind is unknown to this version of Ganttora (for example `'resource'`,
///   written by a newer version). Such rows stay in the file untouched; this
///   version simply cannot show them.
/// - `last_id`: the `id` of the last row the query looked at, readable or
///   skipped (`None` if there were no rows). When reading page by page, pass
///   it as the next [`JournalFilter::after_id`]: a page whose rows were all
///   skipped is empty, but it is not the end of the journal.
///
/// # Example
/// ```
/// let page = gtra::JournalPage { entries: Vec::new(), skipped_unknown: 3, last_id: Some(60) };
/// // Nothing readable on this page, but continue after row 60.
/// assert!(page.entries.is_empty());
/// assert_eq!(page.last_id, Some(60));
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JournalPage {
    /// The records that could be read, oldest first.
    pub entries: Vec<JournalEntry>,
    /// Rows skipped because of an unknown entity kind.
    pub skipped_unknown: usize,
    /// The `id` of the last row looked at (readable or skipped).
    pub last_id: Option<i64>,
}

impl GtraFile {
    /// Adds change records to the end of the journal, all in one transaction.
    ///
    /// # What it does
    /// 1. Refuses if the file is read-only.
    /// 2. Checks every record (see Errors); nothing is written if one is bad.
    /// 3. Inserts them in the given order (see the table in the file header)
    ///    and returns the number SQLite gave each one.
    ///
    /// This changes only this database (normally the local working copy).
    /// The records reach the user's file with the next save.
    ///
    /// # Parameters
    /// - `records`: the records to add, as made by the `document` crate.
    ///
    /// # Returns
    /// The new ids, in the same order as `records` (always increasing).
    ///
    /// # Errors
    /// - [`GtraError::ReadOnly`] for a read-only file.
    /// - [`GtraError::InvalidRecord`] if a record's `field` is empty.
    /// - [`GtraError::Sqlite`] if an insert fails (then nothing is added).
    ///
    /// # Example
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use model::{BatchId, ChangeRecord, EntityKind};
    /// use serde_json::json;
    ///
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// std::fs::create_dir_all(&dir).unwrap();
    /// let mut file = gtra::create_empty(dir.join("shed.gtra")).unwrap();
    ///
    /// let record = ChangeRecord {
    ///     timestamp: Utc.with_ymd_and_hms(2027, 2, 15, 10, 42, 7).unwrap(),
    ///     author: "Sam Lee".into(),
    ///     batch_id: BatchId::new(),
    ///     entity: EntityKind::Task,
    ///     entity_id: uuid::Uuid::new_v4(),
    ///     field: "duration_minutes".into(),
    ///     old_value: json!(2400),
    ///     new_value: json!(2880),
    /// };
    /// let ids = file.append_journal(&[record.clone()]).unwrap();
    /// let back = file.read_journal(&gtra::JournalFilter::default()).unwrap();
    /// assert_eq!(back[0].id, ids[0]);
    /// assert_eq!(back[0].record, record);
    /// # drop(file);
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn append_journal(&mut self, records: &[ChangeRecord]) -> Result<Vec<i64>, GtraError> {
        if self.is_read_only() {
            return Err(GtraError::ReadOnly);
        }
        // Check everything first, so a bad record in the middle cannot leave
        // half of the batch written (the transaction would roll back anyway,
        // but a clear error before touching the database is friendlier).
        for record in records {
            validate_record(record)?;
        }

        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(records.len());
        {
            // A prepared statement is parsed once and run many times, which is
            // faster than building the SQL text for every record.
            let mut insert = tx.prepare(
                "INSERT INTO journal
                     (timestamp, author, entity, entity_id, field, old_value, new_value, batch_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for r in records {
                insert.execute(rusqlite::params![
                    r.timestamp.format(TIMESTAMP_FORMAT).to_string(),
                    r.author,
                    entity_code(r.entity),
                    r.entity_id.to_string(),
                    r.field,
                    json_to_column(&r.old_value),
                    json_to_column(&r.new_value),
                    r.batch_id.to_string(),
                ])?;
                ids.push(tx.last_insert_rowid());
            }
            // The inner `{ … }` block ends here, so `insert` (which borrows
            // `tx`) is dropped before `tx.commit()` takes `tx` by value.
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Reads change records, oldest first.
    ///
    /// Rows about a kind of item this version does not know (for example
    /// `'resource'`, written by a newer Ganttora) are skipped silently. Use
    /// [`GtraFile::read_journal_report`] to learn how many were skipped.
    ///
    /// # Parameters
    /// - `filter`: which records to return ([`JournalFilter`]).
    ///
    /// # Returns
    /// The matching records ordered by `id` (the order they were added), each
    /// as a [`JournalEntry`]. Works on read-only files too.
    ///
    /// # Errors
    /// - [`GtraError::InvalidData`] if a stored UUID, timestamp or JSON value
    ///   cannot be read (a damaged row).
    /// - [`GtraError::Sqlite`] if the query fails.
    ///
    /// # Example
    /// See [`GtraFile::append_journal`].
    pub fn read_journal(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>, GtraError> {
        Ok(self.read_journal_report(filter)?.entries)
    }

    /// Reads change records like [`GtraFile::read_journal`], and also says
    /// how many rows were skipped because of an unknown entity kind.
    ///
    /// # Why skip instead of fail?
    /// A newer Ganttora may record changes to things this version does not
    /// have yet (resources, for example). Refusing the whole journal because
    /// of them would hide all the records this version *can* show. The
    /// skipped rows are never deleted or changed.
    ///
    /// # Parameters
    /// - `filter`: which records to return ([`JournalFilter`]). Note that
    ///   `limit` counts rows looked at, skipped ones included.
    ///
    /// # Returns
    /// A [`JournalPage`]: the readable records, the number skipped, and the
    /// last row id (for reading the next page).
    ///
    /// # Errors
    /// As [`GtraFile::read_journal`].
    ///
    /// # Example
    /// ```
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// std::fs::create_dir_all(&dir).unwrap();
    /// let file = gtra::create_empty(dir.join("shed.gtra")).unwrap();
    /// // A row written by a newer Ganttora, about a resource.
    /// file.connection().execute(
    ///     "INSERT INTO journal (timestamp, author, entity, entity_id, field, old_value, new_value, batch_id)
    ///      VALUES ('2027-02-15T10:42:07Z', 'Sam Lee', 'resource',
    ///              '00000000-0000-0000-0000-000000000001', 'name', NULL, '\"Anna\"',
    ///              '00000000-0000-0000-0000-000000000002')",
    ///     [],
    /// ).unwrap();
    /// let page = file.read_journal_report(&gtra::JournalFilter::default()).unwrap();
    /// assert!(page.entries.is_empty());
    /// assert_eq!(page.skipped_unknown, 1);
    /// # drop(file);
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn read_journal_report(&self, filter: &JournalFilter) -> Result<JournalPage, GtraError> {
        // Build the WHERE part from the filter. The values travel separately
        // as `?` parameters, never pasted into the SQL text, so a strange
        // value can never change the meaning of the query ("SQL injection").
        let mut conditions: Vec<&str> = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        if let Some(id) = filter.entity_id {
            conditions.push("entity_id = ?");
            params.push(Value::Text(id.to_string()));
        }
        if let Some(id) = filter.batch_id {
            conditions.push("batch_id = ?");
            params.push(Value::Text(id.to_string()));
        }
        if let Some(after) = filter.after_id {
            conditions.push("id > ?");
            params.push(Value::Integer(after));
        }

        let mut sql = String::from(
            "SELECT id, timestamp, author, entity, entity_id, field, old_value, new_value, batch_id
             FROM journal",
        );
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY id");
        if let Some(limit) = filter.limit {
            sql.push_str(" LIMIT ?");
            params.push(Value::Integer(i64::from(limit)));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        let mut page = JournalPage::default();
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            page.last_id = Some(id);
            let entity: String = row.get(3)?;
            // `let … else`: if the kind is unknown, count the row and go on
            // with the next one (`continue`) instead of failing.
            let Some(entity) = parse_entity(&entity) else {
                page.skipped_unknown += 1;
                continue;
            };
            let timestamp: String = row.get(1)?;
            let entity_id: String = row.get(4)?;
            let old_value: Option<String> = row.get(6)?;
            let new_value: Option<String> = row.get(7)?;
            let batch_id: String = row.get(8)?;
            page.entries.push(JournalEntry {
                id,
                record: ChangeRecord {
                    timestamp: parse_timestamp(&timestamp)?,
                    author: row.get(2)?,
                    batch_id: BatchId::from_uuid(parse_journal_uuid(&batch_id)?),
                    entity,
                    entity_id: parse_journal_uuid(&entity_id)?,
                    field: row.get(5)?,
                    old_value: column_to_json(old_value.as_deref())?,
                    new_value: column_to_json(new_value.as_deref())?,
                },
            });
        }
        Ok(page)
    }

    /// How many records the journal holds.
    ///
    /// # Errors
    /// [`GtraError::Sqlite`] if the query fails.
    ///
    /// # Example
    /// ```
    /// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
    /// std::fs::create_dir_all(&dir).unwrap();
    /// let file = gtra::create_empty(dir.join("shed.gtra")).unwrap();
    /// assert_eq!(file.journal_len().unwrap(), 0);
    /// # drop(file);
    /// # std::fs::remove_dir_all(&dir).unwrap();
    /// ```
    pub fn journal_len(&self) -> Result<u64, GtraError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))?)
    }
}

/// Checks one record before it is stored.
///
/// The types of `ChangeRecord` already guarantee a valid timestamp, entity
/// kind and UUIDs, so only the free-text `field` is left to check.
///
/// # Errors
/// [`GtraError::InvalidRecord`] naming the problem.
fn validate_record(record: &ChangeRecord) -> Result<(), GtraError> {
    if record.field.is_empty() {
        return Err(GtraError::InvalidRecord("field is empty".into()));
    }
    Ok(())
}

/// The text stored in `journal.entity` for each kind of item.
///
/// These are the same words serde uses for [`EntityKind`] in JSON, so the
/// journal reads the same everywhere.
///
/// # Example
/// `EntityKind::Dependency` → `"dependency"`.
fn entity_code(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Project => "project",
        EntityKind::Task => "task",
        EntityKind::Dependency => "dependency",
        EntityKind::Calendar => "calendar",
    }
}

/// The reverse of [`entity_code`].
///
/// # Returns
/// The kind, or `None` for any other text (for example `'resource'`, which
/// only a newer Ganttora writes). The caller skips such rows.
fn parse_entity(code: &str) -> Option<EntityKind> {
    match code {
        "project" => Some(EntityKind::Project),
        "task" => Some(EntityKind::Task),
        "dependency" => Some(EntityKind::Dependency),
        "calendar" => Some(EntityKind::Calendar),
        _ => None,
    }
}

/// Turns a JSON value into what the `old_value` / `new_value` column stores.
///
/// # Returns
/// `None` (SQL `NULL`) for JSON `null`, otherwise the value as compact JSON
/// text: `2880` → `"2880"`, `"Paint"` → `"\"Paint\""`.
fn json_to_column(value: &serde_json::Value) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(value.to_string())
    }
}

/// The reverse of [`json_to_column`].
///
/// # Errors
/// [`GtraError::InvalidData`] if the stored text is not valid JSON.
fn column_to_json(text: Option<&str>) -> Result<serde_json::Value, GtraError> {
    match text {
        None => Ok(serde_json::Value::Null),
        Some(text) => serde_json::from_str(text).map_err(|_| {
            GtraError::InvalidData(format!("journal: '{text}' is not a valid JSON value"))
        }),
    }
}

/// Reads a stored journal timestamp such as `"2027-02-15T10:42:07Z"`.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not in that form.
fn parse_timestamp(text: &str) -> Result<DateTime<Utc>, GtraError> {
    NaiveDateTime::parse_from_str(text, TIMESTAMP_FORMAT)
        // The text is UTC by definition (the trailing `Z`), so we attach
        // the UTC time zone to the date and time we just read.
        .map(|naive| naive.and_utc())
        .map_err(|_| GtraError::InvalidData(format!("journal: '{text}' is not a valid timestamp")))
}

/// Reads a UUID stored in the journal.
///
/// # Errors
/// [`GtraError::InvalidData`] if the text is not a UUID.
fn parse_journal_uuid(text: &str) -> Result<Uuid, GtraError> {
    Uuid::parse_str(text)
        .map_err(|_| GtraError::InvalidData(format!("journal: '{text}' is not a valid UUID")))
}
