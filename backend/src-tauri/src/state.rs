//! state.rs: data the desktop shell keeps while the app runs.
//!
//! # What it is for
//! Holds the **open documents**. A document (`document::Document`) is one
//! open project together with its schedule and its undo/redo history.
//! Next to each document the shell keeps:
//! - facts about its *file* ([`FileState`]): where it is saved, whether it
//!   may be saved over, whether it has unsaved changes, and the notices to
//!   show (upgraded from an older format, read-only and why);
//! - its **session** (`gtra::ProjectSession`): the open *working copy* of
//!   the `.gtra` database in the app's `working/` folder. Saving, Save As
//!   and autosave go through it, so data the model cannot hold yet is kept.
//!
//! The three together form an [`OpenDocument`].
//!
//! the project documentation plans one document per window; each document is stored under
//! a *key*, the window's label (Tauri gives every window a name such as
//! `"main"`). M1 still has only the main window, so there is at most one
//! entry, but the structure already allows more windows later.
//!
//! [`AppState`] also knows the two app folders ([`SessionFolders`]):
//! `<app data>/working` for working copies and `<app data>/recovery` for
//! autosaved recovery copies.
//!
//! # Where it fits
//! `lib.rs` creates one `AppState` at start-up (in `setup`, once the app
//! data folder is known) and hands it to Tauri with `.manage(...)`. Every
//! IPC command can then receive it as a parameter:
//! `fn cmd(state: tauri::State<'_, AppState>)`. The commands in
//! `ipc/project.rs`, `ipc/file.rs`, `ipc/recovery.rs` and `ipc/app.rs` use
//! it.
//!
//! # Main ideas
//! - Commands may run at the same time on different threads, so the shared
//!   map of documents is protected by a **`Mutex`** (short for "mutual
//!   exclusion"): a lock that lets only one thread at a time look inside.
//!   Think of a single key to a room: whoever holds the key may enter, the
//!   others wait outside until the key is handed back.
//! - The lock is held only for the short moment a command needs the
//!   document, and it is released automatically when that moment ends.
//! - **Clean close.** When a document is replaced or closed on purpose
//!   (the user saved, or chose "Don't save"), its session is closed
//!   *cleanly*: the working copy and the recovery copy are deleted. Only a
//!   crash leaves a recovery copy behind, which the next start offers to
//!   restore (`ipc/recovery.rs`).
//! - **Why the file facts live here and not in the `document` crate:** the
//!   document is about the *project* (tasks, links, undo). Where the project
//!   is stored is a desktop-app concern, so the shell keeps it.
//!

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use document::Document;
use gtra::{ProjectSession, ReadOnlyReason, RecoveryStore};

/// Name of the folder for working copies when the app data folder is
/// unknown (inside the system's temporary folder). Not UI text.
pub const FALLBACK_WORKING_DIR: &str = "ganttora-working";

/// Why the shell could not reach a document.
///
/// # Example
/// ```
/// use ganttora_lib::state::{AppState, StateError};
/// let state = AppState::default();
/// let result = state.with_document("main", |doc| doc.can_undo());
/// assert_eq!(result, Err(StateError::NoDocument("main".to_string())));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// No document is open for this key (window label).
    NoDocument(String),
    /// A previous command crashed while it held the lock, so the data may
    /// be half-changed. Rust then marks the lock as *poisoned* and refuses
    /// further access, instead of letting us use broken data.
    Poisoned,
}

/// Turns a [`StateError`] into a readable English sentence (for logs; the
/// UI shows the translated text for the code, see `ipc::IpcError`).
///
/// # Beginner note
/// `impl fmt::Display for StateError` teaches Rust how to print the type
/// with `{}` and gives it `.to_string()`. `Display` is a *trait*: a named
/// set of abilities a type can promise to have (book chapter on Rust).
impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::NoDocument(key) => write!(f, "no project is open in window \"{key}\""),
            StateError::Poisoned => write!(f, "the open project is in an unknown state"),
        }
    }
}

/// What the shell knows about the file behind an open document.
///
/// # Example
/// ```
/// use ganttora_lib::state::FileState;
/// let file = FileState::default(); // a new project, never saved
/// assert!(file.path.is_none());
/// assert!(!file.read_only);
/// assert!(!file.dirty);
/// assert!(file.upgraded_from.is_none());
/// ```
///
/// # Beginner note
/// `PathBuf` is Rust's type for a file path that the value owns (like
/// `String` is for text). `Option<PathBuf>` is either `Some(path)` or
/// `None`: a project that was never saved has no path yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileState {
    /// Where the project was opened from or last saved to. `None` for a new
    /// project (or the demo) that has not been saved yet; "Save" then has
    /// nowhere to write, and the UI asks for a path with "Save As".
    pub path: Option<PathBuf>,
    /// `true` if the file came from a newer Ganttora. Saving over it is
    /// refused (it could destroy data this version does not understand,
    /// the project documentation); "Save As" to another path is allowed.
    pub read_only: bool,
    /// `true` if the project changed since it was opened or last saved.
    /// Set by every successful edit, undo and redo, and for a restored
    /// recovery copy; cleared by a save. The UI shows a "•" next to the file
    /// name while it is `true`.
    pub dirty: bool,
    /// If the file had an older format and was upgraded on opening, that
    /// old format version; otherwise `None`. The UI shows a short notice.
    pub upgraded_from: Option<u32>,
    /// Why the file is read-only (from `gtra`), or `None`.
    pub read_only_reason: Option<ReadOnlyReason>,
}

/// One open project: the document, the facts about its file and its
/// `.gtra` session.
///
/// # Example
/// ```
/// use document::Document;
/// use ganttora_lib::state::OpenDocument;
/// let open = OpenDocument::untitled(Document::new(ganttora_lib::demo::garden_shed()));
/// assert!(open.file.path.is_none());
/// assert!(open.session.is_none());
/// ```
#[derive(Debug)]
pub struct OpenDocument {
    /// The project, its schedule and its undo history.
    pub document: Document,
    /// Where it is stored and whether it has unsaved changes.
    pub file: FileState,
    /// The open working copy of the `.gtra` file. `None` only for
    /// documents made without one (in doc examples and tests); the file
    /// commands then create a session when it is first needed.
    pub session: Option<ProjectSession>,
}

impl OpenDocument {
    /// Wraps a document that has no file and no session yet.
    ///
    /// # Parameters
    /// - `document`: the document. The new value takes ownership of it.
    ///
    /// # Returns
    /// An [`OpenDocument`] with no path, not read-only, not dirty, no session.
    ///
    /// # Example
    /// See [`OpenDocument`].
    pub fn untitled(document: Document) -> Self {
        OpenDocument {
            document,
            file: FileState::default(),
            session: None,
        }
    }
}

/// The two app folders the `.gtra` sessions use.
///
/// # Example
/// ```
/// use ganttora_lib::state::SessionFolders;
/// let folders = SessionFolders::default(); // folders unknown
/// assert!(folders.working.is_none() && folders.recovery.is_none());
/// ```
#[derive(Debug, Clone, Default)]
pub struct SessionFolders {
    /// `<app data>/working`: where working copies live. `None` if the app
    /// data folder is unknown; a folder in the system's temporary folder is
    /// used instead (see [`AppState::working_dir`]).
    pub working: Option<PathBuf>,
    /// `<app data>/recovery`: where autosave writes recovery copies. `None`
    /// if the folder could not be created; autosave and recovery are then
    /// switched off.
    pub recovery: Option<RecoveryStore>,
}

/// Everything the shell remembers while running.
///
/// # Example
/// ```
/// use ganttora_lib::state::{AppState, OpenDocument};
/// use document::Document;
/// let state = AppState::default();
/// let doc = Document::new(ganttora_lib::demo::garden_shed());
/// state.open("main", OpenDocument::untitled(doc)).unwrap();
/// let tasks = state.with_document("main", |doc| doc.snapshot().tasks.len()).unwrap();
/// assert_eq!(tasks, 8);
/// ```
///
/// # Beginner note
/// `#[derive(Debug, Default)]` asks the compiler to write two standard
/// abilities for us: `Debug` (print the value for debugging) and `Default`
/// (create an "empty" value with `AppState::default()`: here, a map with no
/// documents and unknown folders).
#[derive(Debug, Default)]
pub struct AppState {
    /// The open documents, keyed by window label (e.g. `"main"`).
    ///
    /// `Mutex<HashMap<…>>` reads from the inside out: a `HashMap` (a lookup
    /// table from key to value) wrapped in a `Mutex` (the lock).
    documents: Mutex<HashMap<String, OpenDocument>>,
    /// The working and recovery folders, fixed at start-up.
    folders: SessionFolders,
}

impl AppState {
    /// Makes the state with known app folders (called once in `lib.rs`).
    ///
    /// # Parameters
    /// - `folders`: the working and recovery folders.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::state::{AppState, SessionFolders};
    /// let state = AppState::new(SessionFolders::default());
    /// assert!(state.recovery().is_none());
    /// ```
    pub fn new(folders: SessionFolders) -> Self {
        AppState {
            documents: Mutex::new(HashMap::new()),
            folders,
        }
    }

    /// The folder for working copies.
    ///
    /// # Returns
    /// `<app data>/working`, or `<temp>/ganttora-working` if the app data
    /// folder is unknown. `gtra` creates the folder when it needs it.
    pub fn working_dir(&self) -> PathBuf {
        self.folders
            .working
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join(FALLBACK_WORKING_DIR))
    }

    /// The recovery store, or `None` if recovery is switched off.
    pub fn recovery(&self) -> Option<&RecoveryStore> {
        self.folders.recovery.as_ref()
    }

    /// Stores `open` under `key`. A document that was open there before is
    /// closed *cleanly* (its working copy and recovery copy are deleted).
    ///
    /// # Parameters
    /// - `key`: the window label, e.g. `"main"`.
    /// - `open`: the document, its file facts and its session. `AppState`
    ///   takes ownership.
    ///
    /// # Returns
    /// `Ok(())` when stored.
    ///
    /// # Errors
    /// [`StateError::Poisoned`] if the lock is poisoned (see there).
    ///
    /// # Edge cases
    /// Unsaved changes of the replaced document are dropped without asking.
    /// Asking first is the UI's job: it shows "Save / Don't save / Cancel"
    /// (`frontend/features/files/useFileActions.ts`).
    ///
    /// # Example
    /// See [`AppState`].
    pub fn open(&self, key: &str, open: OpenDocument) -> Result<(), StateError> {
        // `lock()` waits until no other thread holds the lock, then gives us
        // a *guard*. While the guard exists we have the room to ourselves.
        // When `documents` goes out of scope at the end of this function,
        // the guard is dropped and the lock is released automatically.
        let mut documents = self.documents.lock().map_err(|_| StateError::Poisoned)?;
        // `insert` hands back the value that was stored under the key before.
        if let Some(previous) = documents.insert(key.to_string(), open) {
            end_session(previous, self.recovery());
        }
        Ok(())
    }

    /// Removes the document under `key` and closes it cleanly.
    ///
    /// # Returns
    /// `Ok(true)` if a document was open there, `Ok(false)` if not.
    ///
    /// # Errors
    /// [`StateError::Poisoned`] if the lock is poisoned.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::state::{AppState, OpenDocument};
    /// use document::Document;
    /// let state = AppState::default();
    /// state.open("main", OpenDocument::untitled(Document::new(ganttora_lib::demo::garden_shed()))).unwrap();
    /// assert_eq!(state.close("main"), Ok(true));
    /// assert_eq!(state.close("main"), Ok(false));
    /// ```
    pub fn close(&self, key: &str) -> Result<bool, StateError> {
        let mut documents = self.documents.lock().map_err(|_| StateError::Poisoned)?;
        match documents.remove(key) {
            Some(open) => {
                end_session(open, self.recovery());
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Closes every open document (used when the app quits).
    ///
    /// # Parameters
    /// - `keep_recovery`: `false` for a normal quit (recovery copies are
    ///   deleted); `true` if the app ends while a document still has unsaved
    ///   changes nobody was asked about (its recovery copy stays, so the next
    ///   start can offer it).
    ///
    /// # Errors
    /// None: a poisoned lock simply leaves the files for the next start.
    pub fn close_all(&self, keep_recovery: bool) {
        let Ok(mut documents) = self.documents.lock() else {
            return;
        };
        // `drain()` empties the map and hands out every entry once.
        for (_, open) in documents.drain() {
            let store = if keep_recovery && open.file.dirty {
                None
            } else {
                self.recovery()
            };
            end_session(open, store);
        }
    }

    /// `true` if any open document has unsaved changes.
    ///
    /// # Example
    /// ```
    /// let state = ganttora_lib::state::AppState::default();
    /// assert!(!state.any_dirty());
    /// ```
    pub fn any_dirty(&self) -> bool {
        self.documents
            .lock()
            .map(|documents| documents.values().any(|open| open.file.dirty))
            // A poisoned lock: we cannot tell, so assume the worst.
            .unwrap_or(true)
    }

    /// Runs `action` on the document open under `key`, while holding the lock.
    ///
    /// A shortcut for [`AppState::with_open`] when only the document matters,
    /// not its file facts.
    ///
    /// # Parameters
    /// - `key`: the window label, e.g. `"main"`.
    /// - `action`: what to do with the document. It receives a mutable
    ///   reference (`&mut Document`), so it may apply commands, undo or redo.
    ///   (Use [`AppState::with_open`] for changes, so the dirty flag can be set.)
    ///
    /// # Returns
    /// `Ok(value)` with whatever `action` returned.
    ///
    /// # Errors
    /// - [`StateError::NoDocument`] if nothing is open under `key`.
    /// - [`StateError::Poisoned`] if the lock is poisoned.
    ///
    /// # Example
    /// See [`AppState`].
    ///
    /// # Beginner note
    /// `action` is a *closure*: a small function written inline by the
    /// caller, like `|doc| doc.can_undo()`. `impl FnOnce(&mut Document) -> R`
    /// means "any function that can be called once with a document and
    /// returns some type `R`". `<R>` makes `with_document` *generic*: it
    /// works for every result type `R` the caller chooses. Passing a closure
    /// means the caller never touches the lock itself, so it can never
    /// forget to release it.
    pub fn with_document<R>(
        &self,
        key: &str,
        action: impl FnOnce(&mut Document) -> R,
    ) -> Result<R, StateError> {
        self.with_open(key, |open| action(&mut open.document))
    }

    /// Runs `action` on the whole [`OpenDocument`] under `key` (document,
    /// file facts and session), while holding the lock.
    ///
    /// # Parameters
    /// - `key`: the window label, e.g. `"main"`.
    /// - `action`: receives `&mut OpenDocument`, so it may change the
    ///   document, the [`FileState`] and the session together, under one lock.
    ///
    /// # Returns
    /// `Ok(value)` with whatever `action` returned.
    ///
    /// # Errors
    /// - [`StateError::NoDocument`] if nothing is open under `key`.
    /// - [`StateError::Poisoned`] if the lock is poisoned.
    ///
    /// # Example
    /// ```
    /// use ganttora_lib::state::{AppState, OpenDocument};
    /// use document::Document;
    /// let state = AppState::default();
    /// let doc = Document::new(ganttora_lib::demo::garden_shed());
    /// state.open("main", OpenDocument::untitled(doc)).unwrap();
    /// state.with_open("main", |open| open.file.dirty = true).unwrap();
    /// assert!(state.with_open("main", |open| open.file.dirty).unwrap());
    /// ```
    pub fn with_open<R>(
        &self,
        key: &str,
        action: impl FnOnce(&mut OpenDocument) -> R,
    ) -> Result<R, StateError> {
        let mut documents = self.documents.lock().map_err(|_| StateError::Poisoned)?;
        let open = documents
            .get_mut(key)
            .ok_or_else(|| StateError::NoDocument(key.to_string()))?;
        Ok(action(open))
    }
}

/// Ends the session of a document that is going away.
///
/// # Parameters
/// - `open`: the document; it is consumed (dropped at the end).
/// - `recovery`: `Some(store)` for a clean close (the recovery copy is
///   deleted), `None` to keep the recovery copy.
///
/// # Errors
/// None returned: a failure (for example a recovery copy that cannot be
/// deleted) is written to the error log. It is harmless: at worst the next
/// start offers a copy the user can discard.
fn end_session(open: OpenDocument, recovery: Option<&RecoveryStore>) {
    if let Some(session) = open.session
        && let Err(error) = session.close(recovery)
    {
        eprintln!("ganttora: closing the project's working copy failed: {error}");
    }
}
