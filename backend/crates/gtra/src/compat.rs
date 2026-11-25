//! Compatibility between a `.gtra` file and this version of Ganttora.
//!
//! # What this file is for
//! Every `.gtra` file records its **format version** (see [`crate::schema`]).
//! When a file is opened, three things can happen:
//!
//! ```text
//!   file's format version   what Ganttora does                  Compatibility
//!   ---------------------   ---------------------------------   -------------------------
//!   same as this app        open normally                       Current
//!   older                   upgrade the LOCAL working copy;     Upgraded { from, to }
//!                           the user's file is unchanged until
//!                           the next save
//!   newer                   open READ-ONLY, explain why         ReadOnly(ReadOnlyReason)
//! ```
//!
//! This module turns that situation into a **structured answer** that the
//! user interface can show in the user's language: a fixed code such as
//! `"gtra.newer_format"` plus the numbers to fill into the message. It never
//! produces English sentences itself, because the UI is translated into nine
//! languages and Rust code must not decide the wording.
//!
//! # Where it fits
//! ```text
//!   lib.rs: open() / ProjectSession::open()
//!        │  after open_db() has decided the access mode
//!        ▼
//!   compat.rs: check(&file) ──► Compatibility ──► src-tauri ──► UI banner
//!                                                              "error.gtra.newer_format"
//! ```
//!
//! # Why newer files are read-only
//! A newer Ganttora may have added tables or columns this app does not know.
//! If this app saved over such a file, it could silently lose that data or
//! break rules it does not know about. Reading is safe; writing is not.
//!

use crate::schema::CURRENT_FORMAT_VERSION;
use crate::{Access, GtraError, GtraFile};

/// Why a file was opened read-only. A code plus the details for the message.
///
/// Only one reason exists today; the enum leaves room for more (for example
/// "the file is locked by another program") without changing the callers'
/// basic shape.
///
/// # Beginner note
/// Each variant carries **named fields** (like a small struct). The UI does
/// not need to understand Rust: the Tauri shell sends [`ReadOnlyReason::code`]
/// and [`ReadOnlyReason::params`] as plain JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// The file was saved by a newer Ganttora with a newer format.
    NewerFormat {
        /// The file's format version, e.g. `3`.
        found: u32,
        /// The newest format this app supports
        /// ([`CURRENT_FORMAT_VERSION`]), e.g. `2`.
        supported: u32,
        /// The Ganttora version that last saved the file (`meta.app_version`),
        /// e.g. `"0.4.0"`, so the notice can say "made with Ganttora 0.4.0".
        /// `None` if the file does not say.
        written_by: Option<String>,
    },
}

impl ReadOnlyReason {
    /// The stable, language-neutral code of this reason.
    ///
    /// # Returns
    /// A short text that never changes between versions, e.g.
    /// `"gtra.newer_format"`. The UI looks up its translated message under
    /// `error.` + code (for example `error.gtra.newer_format` in
    /// `locales/en.json`).
    ///
    /// # Example
    /// ```
    /// let reason = gtra::ReadOnlyReason::NewerFormat { found: 3, supported: 1, written_by: None };
    /// assert_eq!(reason.code(), "gtra.newer_format");
    /// ```
    pub fn code(&self) -> &'static str {
        match self {
            ReadOnlyReason::NewerFormat { .. } => "gtra.newer_format",
        }
    }

    /// The values to fill into the translated message, as name/value pairs.
    ///
    /// # Returns
    /// For `NewerFormat`: `found`, `supported` and, if known, `writtenBy`.
    /// The names match ICU placeholders such as `{found}` in the locale file.
    ///
    /// # Example
    /// ```
    /// let reason = gtra::ReadOnlyReason::NewerFormat {
    ///     found: 3, supported: 1, written_by: Some("0.4.0".into()),
    /// };
    /// let params = reason.params();
    /// assert!(params.contains(&("found", "3".to_string())));
    /// assert!(params.contains(&("writtenBy", "0.4.0".to_string())));
    /// ```
    ///
    /// # Beginner note
    /// `Vec<(&'static str, String)>` is a list of **tuples**: each element is
    /// a pair (name, value). `&'static str` is text built into the program.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            ReadOnlyReason::NewerFormat {
                found,
                supported,
                written_by,
            } => {
                let mut params = vec![
                    ("found", found.to_string()),
                    ("supported", supported.to_string()),
                ];
                // `if let Some(v) = …` runs the block only when there is a value.
                if let Some(version) = written_by {
                    params.push(("writtenBy", version.clone()));
                }
                params
            }
        }
    }
}

/// How an open file relates to this version of Ganttora. Returned by [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    /// The file has exactly this app's format. Nothing to tell the user.
    Current,
    /// The file had an older format; the working copy was upgraded. The UI
    /// should show a short notice: after saving, older Ganttora versions
    /// will open the file read-only.
    Upgraded {
        /// The format version the file had on disk.
        from: u32,
        /// The format version of the working copy now
        /// ([`CURRENT_FORMAT_VERSION`]).
        to: u32,
    },
    /// The file can be read but not saved. Shows why.
    ReadOnly(ReadOnlyReason),
}

impl Compatibility {
    /// A stable code for the notice the UI should show, or `None` when there
    /// is nothing to say.
    ///
    /// # Returns
    /// - `Current` → `None`
    /// - `Upgraded` → `Some("gtra.upgraded")`
    /// - `ReadOnly(reason)` → `Some(reason.code())`
    ///
    /// # Example
    /// ```
    /// use gtra::compat::Compatibility;
    /// assert_eq!(Compatibility::Current.notice_code(), None);
    /// assert_eq!(Compatibility::Upgraded { from: 1, to: 2 }.notice_code(), Some("gtra.upgraded"));
    /// ```
    pub fn notice_code(&self) -> Option<&'static str> {
        match self {
            Compatibility::Current => None,
            Compatibility::Upgraded { .. } => Some("gtra.upgraded"),
            Compatibility::ReadOnly(reason) => Some(reason.code()),
        }
    }
}

/// Works out the [`Compatibility`] of an open file.
///
/// # What it does
/// Looks at how [`crate::open_db`] opened the file (read-only or not, and
/// whether it upgraded it). For a read-only file it also reads
/// `meta.app_version`, so the notice can name the Ganttora version that
/// made the file.
///
/// # Parameters
/// - `file`: a file opened with [`crate::open_db`] (normally the working copy).
///
/// # Returns
/// The [`Compatibility`].
///
/// # Errors
/// [`GtraError::Sqlite`] if `meta` cannot be read.
///
/// # Example
/// ```
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let file = gtra::create_empty(dir.join("shed.gtra")).unwrap();
/// assert_eq!(gtra::compat::check(&file).unwrap(), gtra::compat::Compatibility::Current);
/// # drop(file);
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn check(file: &GtraFile) -> Result<Compatibility, GtraError> {
    match file.access() {
        Access::ReadOnly => Ok(Compatibility::ReadOnly(ReadOnlyReason::NewerFormat {
            found: file.format_version(),
            supported: CURRENT_FORMAT_VERSION,
            written_by: file.meta("app_version")?.filter(|v| !v.is_empty()),
        })),
        Access::ReadWrite => Ok(match file.upgraded_from() {
            Some(from) => Compatibility::Upgraded {
                from,
                to: file.format_version(),
            },
            None => Compatibility::Current,
        }),
    }
}

/// The read-only reason of an open file, if it has one. A shortcut for
/// [`check`] when only the read-only case matters.
///
/// # Returns
/// `Some(reason)` for a read-only file, `None` otherwise.
///
/// # Errors
/// [`GtraError::Sqlite`] if `meta` cannot be read.
pub fn read_only_reason(file: &GtraFile) -> Result<Option<ReadOnlyReason>, GtraError> {
    Ok(match check(file)? {
        Compatibility::ReadOnly(reason) => Some(reason),
        _ => None,
    })
}
