//! # Undo and redo: two stacks of commands
//!
//! ## What this file is for
//! Keeps the **undo stack** and the **redo stack** of the open project
//! ("Two stacks: undo and redo"). Each entry is one
//! applied command together with its inverse.
//!
//! ## Where it fits
//! Only the `Document` (in `lib.rs`) uses this file. The `Document` executes
//! commands; this file just remembers them in the right order.
//!
//! ## Main ideas
//! A **stack** is a pile: the last item put on top is the first one taken
//! off. The rules, with the shed (task 2 at 5d, both stacks empty):
//!
//! | You do | Undo stack | Redo stack | Task 2 |
//! |---|---|---|---|
//! | Set task 2 to 6d | `5d→6d` | empty | 6d |
//! | Undo | empty | `5d→6d` | 5d |
//! | Redo | `5d→6d` | empty | 6d |
//! | Undo, then set Paint to 3d | `2d→3d` | **emptied** | 5d |
//!
//! A new edit empties the redo stack: the undone "future" was made for a
//! project that no longer exists.
//!
//! The stacks are **unlimited** (unlimited undo). They live only in
//! memory and are gone when the project is closed, as in MS Project.
//!
//! ## Documentation

use crate::command::Command;

/// One applied command and the command that reverses it.
///
/// # Fields
/// - `command`: what the user did (executed again by redo).
/// - `inverse`: what undoes it (executed by undo).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct UndoEntry {
    /// The command as the user applied it.
    pub command: Command,
    /// The command that reverses it.
    pub inverse: Command,
}

/// The undo stack and the redo stack together.
///
/// # Beginner note: `Vec` as a stack
/// A `Vec` (a growable list) works as a stack when we only use its end:
/// `push` puts an item on top, `pop` takes the top item off and returns it
/// as an `Option` (`None` when the list is empty).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct UndoStack {
    /// Applied commands; the last one is the most recent.
    undo: Vec<UndoEntry>,
    /// Undone commands, waiting for redo; the last one was undone most recently.
    redo: Vec<UndoEntry>,
}

impl UndoStack {
    /// Records a **new** edit: pushes it on the undo stack and empties the
    /// redo stack.
    pub fn push_new(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
        self.redo.clear();
    }

    /// Takes the most recent applied command off the undo stack.
    /// Returns `None` when there is nothing to undo.
    pub fn pop_undo(&mut self) -> Option<UndoEntry> {
        self.undo.pop()
    }

    /// Puts an entry back on the undo stack **without** touching the redo
    /// stack. Used by redo, and to put an entry back when undo fails.
    pub fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
    }

    /// Takes the most recently undone command off the redo stack.
    /// Returns `None` when there is nothing to redo.
    pub fn pop_redo(&mut self) -> Option<UndoEntry> {
        self.redo.pop()
    }

    /// Puts an undone entry on the redo stack.
    pub fn push_redo(&mut self, entry: UndoEntry) {
        self.redo.push(entry);
    }

    /// `true` when there is at least one command to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// `true` when there is at least one command to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
