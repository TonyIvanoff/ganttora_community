//! Schema versions and the migration runner for `.gtra` files.
//!
//! # What this file is for
//! The **schema** is the list of tables and columns in a `.gtra` file. It
//! changes as Ganttora grows. Each change is a numbered **migration**: an SQL
//! file in `backend/crates/gtra/migrations/`. This module knows all
//! migrations, finds out which version a file has, and runs the missing ones.
//!
//! # Where it fits
//! Called by [`crate::create_empty`] (new file: run every migration) and by
//! [`crate::open_db`] (existing file: check the version, maybe upgrade).
//!
//! # Main ideas
//! - **Format version** = the number of migrations applied to a file.
//!   A new file made by this app has [`CURRENT_FORMAT_VERSION`].
//! - Where the number is stored: in SQLite's built-in header field
//!   `PRAGMA user_version` (see "Why `user_version`" below), with a copy in
//!   the `meta` table under `format_version` for people reading the file.
//! - Every `.gtra` file is also stamped with [`GTRA_APPLICATION_ID`]
//!   (`PRAGMA application_id`), so Ganttora can tell its own files apart from
//!   any other SQLite database that happens to be renamed to `.gtra`.
//!
//! ```text
//!   file version  vs  app's CURRENT_FORMAT_VERSION   what happens
//!   ------------      ---------------------------   ------------------------------
//!   0                 (any)                          not a .gtra file -> error
//!   older (1 < 3)     3                              run 0002, 0003 in one transaction
//!   same  (3 = 3)     3                              open normally
//!   newer (4 > 3)     3                              open READ-ONLY
//! ```
//!
//! # Why `user_version` and not only a row in `meta`
//! - It lives in the file's 100-byte header, so it can be read before we
//!   know anything about the tables, even from a file with damaged tables.
//! - It is changed in the same transaction as the migration itself: if the
//!   app crashes halfway, both the tables and the number roll back together.
//! - It is a plain integer, so comparing versions cannot go wrong.
//!
//! The `meta.format_version` row is a convenience copy that the runner keeps
//! equal to `user_version`; the program trusts `user_version`.
//!

use rusqlite::{Connection, TransactionBehavior};

use crate::GtraError;

/// Marks a SQLite database as a `.gtra` file. Stored in the file header with
/// `PRAGMA application_id`.
///
/// # Why this number
/// It is the four ASCII letters `G`, `t`, `r`, `a` (hex `47 74 72 61`) read as
/// one 32-bit number. Tools like the Unix `file` command can recognise a file
/// type by this field.
pub const GTRA_APPLICATION_ID: i32 = 0x4774_7261;

/// One schema change: a version number and the SQL that performs it.
///
/// # Fields
/// - `version`: the format version a file has *after* this migration
///   (`0001_init.sql` → 1).
/// - `name`: the file name without `.sql`, used in error messages.
/// - `sql`: the SQL text, embedded into the program at compile time.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Format version after this migration has run (1, 2, 3, …).
    pub version: u32,
    /// File name without extension, e.g. `"0001_init"`.
    pub name: &'static str,
    /// The SQL statements of the migration.
    pub sql: &'static str,
}

/// Every migration, in order. **Append only**: never edit or remove one
/// that has been released (files & interop rules).
///
/// # Beginner note
/// `include_str!("…")` is a **macro** that reads a text file while the
/// program is being compiled and puts its content into the program as a
/// string. The SQL therefore ships inside the app; no separate files are
/// needed on the user's computer.
pub const MIGRATIONS: &[Migration] = &[
    // M0: every table of the first format.
    Migration {
        version: 1,
        name: "0001_init",
        sql: include_str!("../migrations/0001_init.sql"),
    },
    // M1: calendar exceptions next to holidays (kind + position), and
    // is_milestone becomes the user's own flag.
    Migration {
        version: 2,
        name: "0002_m1_fields",
        sql: include_str!("../migrations/0002_m1_fields.sql"),
    },
    // Blockers: the new table `blockers`, one row per
    // unplanned delay on a task.
    Migration {
        version: 3,
        name: "0003_blockers",
        sql: include_str!("../migrations/0003_blockers.sql"),
    },
    // Percent complete and project rows.
    Migration {
        version: 4,
        name: "0004_progress_projects",
        sql: include_str!("../migrations/0004_progress_projects.sql"),
    },
    // Risks: what could go wrong with a task.
    Migration {
        version: 5,
        name: "0005_risk",
        sql: include_str!("../migrations/0005_risk.sql"),
    },
    // Several risks per task, with priority, response and consequence.
    Migration {
        version: 6,
        name: "0006_risks",
        sql: include_str!("../migrations/0006_risks.sql"),
    },
    Migration {
        version: 7,
        name: "0007_plan",
        sql: include_str!("../migrations/0007_plan.sql"),
    },
];

/// The newest format version this app can read and write: the version of
/// the last migration.
///
/// # Example
/// ```
/// assert_eq!(gtra::schema::CURRENT_FORMAT_VERSION, 7);
/// ```
pub const CURRENT_FORMAT_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].version;

/// What a file's version means for this app. Returned by [`check_version`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionStatus {
    /// The file has exactly [`CURRENT_FORMAT_VERSION`]: open normally.
    Current,
    /// The file is older; the migrations after `from` must run first.
    NeedsUpgrade {
        /// The file's format version.
        from: u32,
    },
    /// The file was made by a newer Ganttora: open read-only.
    Newer {
        /// The file's format version.
        found: u32,
    },
}

/// Compares a file's format version with what this app supports.
///
/// # Parameters
/// - `file_version`: the value of `PRAGMA user_version` in the file; must be
///   at least 1 (0 means "not a `.gtra` file", checked by the caller).
///
/// # Returns
/// A [`VersionStatus`] telling the caller what to do.
///
/// # Example
/// ```
/// use gtra::schema::{check_version, VersionStatus, CURRENT_FORMAT_VERSION};
/// assert_eq!(check_version(CURRENT_FORMAT_VERSION), VersionStatus::Current);
/// assert_eq!(
///     check_version(CURRENT_FORMAT_VERSION + 1),
///     VersionStatus::Newer { found: CURRENT_FORMAT_VERSION + 1 }
/// );
/// ```
pub fn check_version(file_version: u32) -> VersionStatus {
    // `match` with guards (`if …`) checks the cases from top to bottom.
    match file_version {
        v if v == CURRENT_FORMAT_VERSION => VersionStatus::Current,
        v if v < CURRENT_FORMAT_VERSION => VersionStatus::NeedsUpgrade { from: v },
        v => VersionStatus::Newer { found: v },
    }
}

/// Reads the format version (`PRAGMA user_version`) of an open database.
///
/// # Parameters
/// - `conn`: an open SQLite connection.
///
/// # Returns
/// The version; 0 for a database that has never been migrated.
///
/// # Errors
/// [`GtraError::Sqlite`] if SQLite cannot read the header (not a database).
/// A negative stored value (never written by Ganttora) is returned as 0.
pub fn read_format_version(conn: &Connection) -> Result<u32, GtraError> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // `u32::try_from` refuses negative numbers; such a file was not made by us.
    Ok(u32::try_from(v).unwrap_or(0))
}

/// Reads the application id (`PRAGMA application_id`) of an open database.
///
/// # Returns
/// The id; [`GTRA_APPLICATION_ID`] for a `.gtra` file, 0 for a plain SQLite
/// database.
///
/// # Errors
/// [`GtraError::Sqlite`] if SQLite cannot read the header.
pub fn read_application_id(conn: &Connection) -> Result<i32, GtraError> {
    Ok(conn.pragma_query_value(None, "application_id", |row| row.get(0))?)
}

/// Brings a database up to [`CURRENT_FORMAT_VERSION`] by running every
/// migration it does not have yet.
///
/// # What it does
/// 1. Switches foreign-key checks off (SQLite cannot change this setting
///    inside a transaction, and some future migrations must rebuild tables,
///    which would trip the checks halfway).
/// 2. Starts one transaction. Inside it, runs each missing migration in
///    order, then stamps the application id, `user_version` and the `meta`
///    row `format_version`.
/// 3. Runs `PRAGMA foreign_key_check`: if any row now points to something
///    that does not exist, everything is rolled back.
/// 4. Commits (makes it permanent), and switches foreign keys back on.
///
/// All migrations run in **one** transaction, so the file ends up either
/// fully upgraded or completely unchanged, never in between.
///
/// # Parameters
/// - `conn`: an open, writable connection. Its current version must not be
///   newer than [`CURRENT_FORMAT_VERSION`].
///
/// # Returns
/// The version the database had before (0 for a brand-new database).
///
/// # Errors
/// - [`GtraError::NewerFormat`] if the database is newer than this app.
/// - [`GtraError::Migration`] if a migration's SQL fails, or the foreign-key
///   check finds broken links afterwards.
/// - [`GtraError::Sqlite`] for other database errors.
///
/// # Example
/// ```
/// let mut conn = rusqlite::Connection::open_in_memory().unwrap();
/// let before = gtra::schema::migrate(&mut conn).unwrap();
/// assert_eq!(before, 0);
/// assert_eq!(gtra::schema::read_format_version(&conn).unwrap(), gtra::schema::CURRENT_FORMAT_VERSION);
/// // Running it again does nothing: the database is already current.
/// assert_eq!(gtra::schema::migrate(&mut conn).unwrap(), gtra::schema::CURRENT_FORMAT_VERSION);
/// ```
///
/// # Beginner note
/// A **transaction** groups several database changes into one unit: either
/// all of them happen (`commit`) or none of them (`rollback`). If the
/// transaction object is dropped without `commit()`, for example because a
/// `?` returned an error early, rusqlite rolls it back automatically.
pub fn migrate(conn: &mut Connection) -> Result<u32, GtraError> {
    let before = read_format_version(conn)?;
    if before > CURRENT_FORMAT_VERSION {
        return Err(GtraError::NewerFormat {
            found: before,
            supported: CURRENT_FORMAT_VERSION,
        });
    }
    if before == CURRENT_FORMAT_VERSION {
        return Ok(before);
    }

    conn.pragma_update(None, "foreign_keys", false)?;
    // We run the migrations in a separate function so that foreign keys are
    // switched back on whatever happens inside it.
    let result = run_pending(conn, before);
    conn.pragma_update(None, "foreign_keys", true)?;
    result.map(|()| before)
}

/// Runs the migrations newer than `from` inside one transaction. Helper for
/// [`migrate`]; see there for the steps and errors.
fn run_pending(conn: &mut Connection, from: u32) -> Result<(), GtraError> {
    // IMMEDIATE: take the write lock at the start, so no other connection can
    // change the file while we upgrade it.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        // `execute_batch` runs several SQL statements separated by `;`.
        tx.execute_batch(m.sql)
            .map_err(|source| GtraError::Migration {
                name: m.name,
                source,
            })?;
    }

    tx.pragma_update(None, "application_id", GTRA_APPLICATION_ID)?;
    tx.pragma_update(None, "user_version", CURRENT_FORMAT_VERSION)?;
    // "INSERT … ON CONFLICT … DO UPDATE" (an "upsert") inserts the row, or
    // updates it if the key already exists.
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('format_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [CURRENT_FORMAT_VERSION.to_string()],
    )?;

    check_foreign_keys(&tx).map_err(|source| GtraError::Migration {
        name: MIGRATIONS[MIGRATIONS.len() - 1].name,
        source,
    })?;

    tx.commit()?;
    Ok(())
}

/// Fails if any row points (by foreign key) to a row that does not exist.
///
/// # Returns
/// `Ok(())` if all links are valid.
///
/// # Errors
/// A `rusqlite::Error` describing the first broken link found.
fn check_foreign_keys(conn: &Connection) -> Result<(), rusqlite::Error> {
    // `foreign_key_check` returns one row per broken link:
    // (table, rowid, referenced table, foreign-key index).
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let table: String = row.get(0)?;
        let parent: String = row.get(2)?;
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(format!(
                "broken foreign key: a row in '{table}' points to a missing row in '{parent}'"
            )),
        ));
    }
    Ok(())
}
