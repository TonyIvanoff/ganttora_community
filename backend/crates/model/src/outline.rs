//! # The outline (WBS): parents, children and summary tasks
//!
//! ## What this file is for
//! Tasks are stored in one flat list, in row order, each with an
//! `outline_level` (see [`crate::Task`]). This file *derives* the tree
//! structure from that list: who is the parent of a task, its position among
//! its siblings, which tasks are summary tasks, where each task's block of
//! subtasks ends, and the outline number ("1.2.3") of each row.
//!
//! ## Where it fits
//! Part of the `model` crate. The engine uses it to roll summary dates up;
//! the document uses it to check indent, outdent and move commands, and to
//! fill the snapshot the UI shows; the `.gtra` file layer can use it to
//! write `parent_id` and `position`.
//!
//! ## Main ideas
//! - **One source of truth.** Only the row order and the levels are stored.
//!   Parent and position are calculated, so they can never disagree with the
//!   rows. MS Project keeps its outline the same way.
//! - **The rule of a valid outline:** the first row has level 1, and every
//!   row is at most one level deeper than the row above it. (Indenting a row
//!   by two steps at once would leave a level with no parent.)
//! - The parent of a row is the nearest row **above** it with a **smaller**
//!   level. A row is a **summary task** when the next row is deeper.
//!
//! ## Worked example
//! ```text
//! row  level  name          parent  position  summary  outline number  subtree ends before row
//! 0    1      Preparation   -       0         yes      1               3
//! 1    2        Design      0       0         no       1.1             2
//! 2    2        Permit      0       1         no       1.2             3
//! 3    1      Build         -       1         yes      2               6
//! 4    2        Floor       3       0         no       2.1             5
//! 5    2        Walls       3       1         no       2.2             6
//! ```
//!
//! ## Documentation

use crate::task::Task;

/// The outline structure of a task list, calculated once by
/// [`Outline::build`] and then queried by row number.
///
/// # Fields (all private; use the methods)
/// One entry per row, in row order: the parent row, the position among
/// siblings, the summary flag, the end of the subtree and the effective
/// level.
///
/// # Example
/// ```
/// use model::{Duration, Outline, Task};
/// let tasks = vec![
///     Task::new("Preparation", Duration::ZERO),
///     Task::new("Design", Duration::from_days(2)).at_level(2),
///     Task::new("Permit", Duration::from_days(5)).at_level(2),
///     Task::new("Build", Duration::from_days(4)),
/// ];
/// let outline = Outline::build(&tasks);
/// assert!(outline.is_summary(0));
/// assert_eq!(outline.parent(2), Some(0));
/// assert_eq!(outline.position(2), 1);
/// assert_eq!(outline.outline_number(2), "1.2");
/// assert_eq!(outline.subtree_end(0), 3);
/// ```
///
/// # Beginner note: private fields
/// The fields have no `pub`, so other crates cannot change them by mistake.
/// They can only *ask* questions through the methods below. This is called
/// **encapsulation**.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    /// Parent row of each row (`None` for top-level rows).
    parent: Vec<Option<usize>>,
    /// Position of each row among the children of its parent (0 = first).
    position: Vec<usize>,
    /// `true` when the row has at least one child.
    summary: Vec<bool>,
    /// For each row, the first row *after* its subtree.
    subtree_end: Vec<usize>,
    /// The level actually used for each row (see [`Outline::level`]).
    level: Vec<u32>,
}

impl Outline {
    /// Calculates the outline of a task list.
    ///
    /// # What it does
    /// Walks the rows once from top to bottom with a **stack** of "open"
    /// ancestors: the rows above whose subtree is still going on. For each
    /// row, ancestors at the same or a deeper level are closed (their subtree
    /// ends here), and the ancestor left on top is the parent.
    ///
    /// ```text
    /// row 3 "Build" (level 1) arrives, stack = [Preparation(1), Permit(2)]
    ///   close Permit   -> its subtree ends before row 3
    ///   close Preparation -> its subtree ends before row 3
    ///   stack empty -> "Build" has no parent; push it: stack = [Build(1)]
    /// ```
    ///
    /// # Parameters
    /// - `tasks`: the tasks in row order.
    ///
    /// # Returns
    /// The [`Outline`].
    ///
    /// # Errors / edge cases
    /// Never fails. An invalid list (a first row deeper than 1, or a row two
    /// levels deeper than the row above) is read forgivingly: such a row is
    /// treated as one level deeper than the row above, and a level of 0 as 1.
    /// [`first_invalid_row`] tells whether a list is valid.
    ///
    /// # Example
    /// See [`Outline`].
    pub fn build(tasks: &[Task]) -> Outline {
        let count = tasks.len();
        let mut parent = vec![None; count];
        let mut position = vec![0; count];
        let mut summary = vec![false; count];
        let mut subtree_end = vec![count; count];
        let mut level = vec![1; count];
        // How many children each row has seen so far, to number the next one.
        // `top_level_children` does the same for rows without a parent.
        let mut child_count = vec![0usize; count];
        let mut top_level_children = 0usize;
        // The stack of open ancestors (row numbers).
        let mut stack: Vec<usize> = Vec::new();

        for (row, task) in tasks.iter().enumerate() {
            // The effective level: at least 1, at most one deeper than the
            // row above.
            let deepest_allowed = stack.last().map_or(1, |&above| level[above] + 1);
            let this_level = task.outline_level.clamp(1, deepest_allowed);
            level[row] = this_level;

            // Close every open ancestor at this level or deeper.
            while let Some(&open) = stack.last() {
                if level[open] >= this_level {
                    subtree_end[open] = row;
                    stack.pop();
                } else {
                    break;
                }
            }

            // Whatever is left on top is the parent.
            match stack.last() {
                Some(&p) => {
                    parent[row] = Some(p);
                    summary[p] = true;
                    position[row] = child_count[p];
                    child_count[p] += 1;
                }
                None => {
                    position[row] = top_level_children;
                    top_level_children += 1;
                }
            }
            stack.push(row);
        }
        // Rows still open at the end have subtrees running to the last row;
        // `subtree_end` already holds `count` for them.

        Outline {
            parent,
            position,
            summary,
            subtree_end,
            level,
        }
    }

    /// Number of rows in the outline.
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    /// `true` when the outline has no rows.
    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// The parent row of `row`, or `None` for a top-level row.
    ///
    /// # Panics
    /// If `row` is not a valid row number (as with `vec[row]`).
    pub fn parent(&self, row: usize) -> Option<usize> {
        self.parent[row]
    }

    /// The position of `row` among its siblings (0 = first child).
    pub fn position(&self, row: usize) -> usize {
        self.position[row]
    }

    /// `true` when `row` has subtasks (it is a summary task).
    pub fn is_summary(&self, row: usize) -> bool {
        self.summary[row]
    }

    /// The first row after the subtree of `row`. The subtree is the rows
    /// `row..subtree_end(row)`: the row itself plus all its descendants.
    /// For a task without children this is `row + 1`.
    pub fn subtree_end(&self, row: usize) -> usize {
        self.subtree_end[row]
    }

    /// The level used for `row`: its `outline_level`, corrected for invalid
    /// lists as described in [`Outline::build`].
    pub fn level(&self, row: usize) -> u32 {
        self.level[row]
    }

    /// The ancestors of `row`, nearest first (parent, grandparent, …).
    ///
    /// # Example
    /// ```
    /// use model::{Duration, Outline, Task};
    /// let tasks = vec![
    ///     Task::new("Phase", Duration::ZERO),
    ///     Task::new("Step", Duration::ZERO).at_level(2),
    ///     Task::new("Detail", Duration::from_days(1)).at_level(3),
    /// ];
    /// assert_eq!(Outline::build(&tasks).ancestors(2), vec![1, 0]);
    /// ```
    pub fn ancestors(&self, row: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut current = self.parent[row];
        // `while let` repeats as long as the pattern matches (here: while
        // there is a parent).
        while let Some(p) = current {
            result.push(p);
            current = self.parent[p];
        }
        result
    }

    /// The outline number of `row`, e.g. `"1.2"` for the second child of
    /// the first top-level task. MS Project shows this as "Outline Number"
    /// and uses it as the default WBS code.
    ///
    /// # Example
    /// See [`Outline`].
    pub fn outline_number(&self, row: usize) -> String {
        // Collect position + 1 of the row and each ancestor, then reverse so
        // the top level comes first.
        let mut parts: Vec<String> = vec![(self.position[row] + 1).to_string()];
        for ancestor in self.ancestors(row) {
            parts.push((self.position[ancestor] + 1).to_string());
        }
        parts.reverse();
        parts.join(".")
    }
}

/// Finds the first row that breaks the outline rule, if any.
///
/// # The rule
/// The first row has level 1; every row has a level of at least 1 and at
/// most one more than the row above.
///
/// # Parameters
/// - `tasks`: the tasks in row order.
///
/// # Returns
/// `None` if the outline is valid, otherwise `Some(row)` of the first bad
/// row.
///
/// # Example
/// ```
/// use model::{first_invalid_row, Duration, Task};
/// let ok = vec![Task::new("A", Duration::ZERO), Task::new("B", Duration::ZERO).at_level(2)];
/// assert_eq!(first_invalid_row(&ok), None);
/// let jump = vec![Task::new("A", Duration::ZERO), Task::new("B", Duration::ZERO).at_level(3)];
/// assert_eq!(first_invalid_row(&jump), Some(1));
/// ```
pub fn first_invalid_row(tasks: &[Task]) -> Option<usize> {
    let mut previous_level = 0;
    for (row, task) in tasks.iter().enumerate() {
        if task.outline_level < 1 || task.outline_level > previous_level + 1 {
            return Some(row);
        }
        previous_level = task.outline_level;
    }
    None
}
