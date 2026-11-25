//! Where a project is kept: the [`Storage`] trait and its local-file version.
//!
//! # What this file is for
//! In v1 every project is a `.gtra` file on a disk. A later
//! edition will also keep projects in a shared team store. To
//! avoid rewriting the open/save code then, the code that opens and saves a
//! project talks to a [`Storage`]: "something that can hand me the project as
//! a local file, and take a new version back". Today there is one kind,
//! [`LocalFileStorage`].
//!
//! # Where it fits
//! ```text
//!   lib.rs (open / save)
//!        │  uses
//!        ▼
//!   trait Storage ───────────┬──────────────────────────┐
//!        fetch_to(local)     │                          │
//!        save_with(fill)     ▼                          ▼
//!                      LocalFileStorage          (later: TeamStorage,
//!                      a .gtra file on disk       later, not in v1)
//!                      uses atomic.rs
//! ```
//!
//! # The main idea: always work on a local copy
//! Ganttora never keeps SQLite working directly on the user's file when it
//! might be in a network or cloud-synced folder. Instead it:
//! 1. copies the stored project to a local working file once ([`Storage::fetch_to`]),
//! 2. edits and autosaves the working file,
//! 3. on Save, writes a fresh copy back atomically ([`Storage::save_with`]).
//!

use std::fs;
use std::path::{Path, PathBuf};

use crate::GtraError;
use crate::atomic::atomic_write;

/// Something that can store a project: today a file, later a team server.
///
/// # What it does
/// Defines the three things the rest of the crate needs from a place where a
/// project is kept. Code that works with a `&dyn Storage` works with every
/// kind of storage.
///
/// # Beginner note
/// A **trait** is a list of abilities ("methods") that a type promises to
/// have, like a job description. Any type that `impl`ements the trait can be
/// used wherever the trait is asked for. `dyn Storage` means "some type that
/// implements `Storage`; which one is decided while the program runs".
/// See the glossary entry "trait".
pub trait Storage {
    /// A human-readable description of where the project is, for messages
    /// and the window title, e.g. `/Users/sam/Projects/shed.gtra`.
    ///
    /// # Returns
    /// The description as a `String`.
    fn location(&self) -> String;

    /// Does a stored project exist at this location yet?
    ///
    /// # Returns
    /// `Ok(true)` if it exists, `Ok(false)` if not.
    ///
    /// # Errors
    /// [`GtraError::Io`] if it cannot be checked (e.g. no permission to look
    /// in the folder).
    fn exists(&self) -> Result<bool, GtraError>;

    /// Copies the stored project into the local file `local`, replacing it.
    ///
    /// # Parameters
    /// - `local`: path of the working copy, normally in the app's data folder.
    ///
    /// # Errors
    /// [`GtraError::Io`] if the project cannot be read or the copy written.
    fn fetch_to(&self, local: &Path) -> Result<(), GtraError>;

    /// Saves a new version of the project, all or nothing.
    ///
    /// # What it does
    /// Gives `fill` the path of a temporary local file that does not exist
    /// yet. `fill` must write the complete project there (for example with
    /// SQLite's `VACUUM INTO`). The storage then puts that file in place in
    /// one step. If `fill` or the storage fails, the stored project is left
    /// unchanged.
    ///
    /// # Parameters
    /// - `fill`: writes the project to the path it is given.
    ///
    /// # Errors
    /// Whatever `fill` returns, or [`GtraError::Io`] from the storage itself.
    ///
    /// # Beginner note
    /// `&mut dyn FnMut(&Path) -> Result<(), GtraError>` is a borrowed
    /// function value. We use `dyn` (not a generic) so that `Storage` can be
    /// used as `&dyn Storage`; traits with generic methods cannot be.
    fn save_with(
        &self,
        fill: &mut dyn FnMut(&Path) -> Result<(), GtraError>,
    ) -> Result<(), GtraError>;
}

/// A project kept as a `.gtra` file on a local, network or synced disk.
///
/// # Example
/// ```
/// use gtra::storage::{LocalFileStorage, Storage};
///
/// let dir = std::env::temp_dir().join(format!("gtra-doc-{}", uuid::Uuid::new_v4()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let storage = LocalFileStorage::new(dir.join("shed.gtra"));
///
/// assert!(!storage.exists().unwrap());
/// storage.save_with(&mut |tmp| {
///     std::fs::write(tmp, b"pretend this is a database").map_err(|e| gtra::GtraError::io(tmp, e))
/// }).unwrap();
/// assert!(storage.exists().unwrap());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFileStorage {
    /// Full path of the `.gtra` file.
    path: PathBuf,
}

impl LocalFileStorage {
    /// Creates a storage for the file at `path`. Nothing is read or written
    /// yet; the file does not need to exist.
    ///
    /// # Parameters
    /// - `path`: where the `.gtra` file is (or will be). Anything that can be
    ///   turned into a `PathBuf` works: a `&str`, a `String`, a `PathBuf`.
    ///
    /// # Returns
    /// The new `LocalFileStorage`.
    ///
    /// # Beginner note
    /// `impl Into<PathBuf>` accepts any type that can be converted into a
    /// `PathBuf`. `.into()` performs the conversion.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path of the `.gtra` file.
    ///
    /// # Returns
    /// A borrowed `&Path`; the storage keeps ownership.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Storage for LocalFileStorage {
    fn location(&self) -> String {
        self.path.display().to_string()
    }

    fn exists(&self) -> Result<bool, GtraError> {
        // `try_exists` (unlike `exists`) reports "could not check" as an
        // error instead of quietly answering "no".
        self.path
            .try_exists()
            .map_err(|e| GtraError::io(&self.path, e))
    }

    fn fetch_to(&self, local: &Path) -> Result<(), GtraError> {
        // Reading the user's file exactly once, in one go, is the safest way
        // to read from a synced folder. The working copy is written
        // atomically too, so a crash never leaves a half-copied working file.
        atomic_write(local, |tmp| {
            fs::copy(&self.path, tmp)
                .map(|_bytes_copied| ())
                .map_err(|e| GtraError::io(&self.path, e))
        })
    }

    fn save_with(
        &self,
        fill: &mut dyn FnMut(&Path) -> Result<(), GtraError>,
    ) -> Result<(), GtraError> {
        atomic_write(&self.path, fill)
    }
}
