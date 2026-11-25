//! # The dependency graph: who waits for whom, and in what order to calculate
//!
//! ## What this file is for
//! Turns the project's dependencies **and its outline** into a **graph**:
//! for every node, the arrows coming in and going out. Then finds a
//! **topological order**: an order in which every node comes after all the
//! nodes it waits for. The forward pass walks this order from front to back,
//! the backward pass from back to front. Finally, it detects **circular
//! dependencies** (cycles) and reports them by task name.
//!
//! ## Where it fits
//! First step of the critical path method, called by [`super::run`].
//!
//! ## Main ideas
//! - A **graph** is a set of points (*nodes*) connected by arrows (*edges*).
//!   The network diagram in the project documentation is a picture of such a graph.
//! - **Nodes.** Every task is one node; its node number is its row number.
//!   A **summary task** gets a *second* node, its **gate**, numbered after
//!   all the tasks. The two nodes of a summary task do two different jobs:
//!
//!   ```text
//!   links INTO the summary ──► gate ──► each subtask ──► summary node ──► links OUT of the summary
//!                              "not before…"            "roll up: earliest start,
//!                               passed down to           latest finish of the subtasks"
//!                               every subtask
//!   ```
//!
//!   With a single node, the summary would have to be calculated both
//!   *before* its subtasks (to hold them back) and *after* them (to roll
//!   their dates up): impossible in one pass. Two nodes solve it.
//! - **Edges** are either *links* (a dependency, with its type and lag) or
//!   *structure* arrows (gate → subtask, subtask → summary node).
//! - **Topological order** with **Kahn's algorithm**: repeatedly take a node
//!   none of whose predecessors is still waiting, put it next in the order,
//!   and "remove" its outgoing arrows. If several nodes are ready at the
//!   same time, we take the lowest node number (the task that comes first in
//!   the project). That tie-break makes the whole result deterministic.
//! - If some nodes are never ready, they wait for each other in a circle: a
//!   **cycle**. We find one such circle and report its task names. A link
//!   between a summary task and one of its own subtasks is a circle too
//!   (the summary waits for the subtask, which waits for the summary), so it
//!   is refused, as in MS Project.
//!
//! ## Worked example: a small outline
//! ```text
//! rows: 0 Phase 1 (summary)  1 Design  2 Permit     3 Build
//! links: Design -> Permit (FS), Phase 1 -> Build (FS)
//! nodes: 0..3 = the tasks, 4 = gate of Phase 1
//!
//! edges:  1 -> 2 (link)      0 -> 3 (link, out of the summary node)
//!         4 -> 1, 4 -> 2     (gate passes limits down)
//!         1 -> 0, 2 -> 0     (subtasks roll up into the summary node)
//! order:  4, 1, 2, 0, 3      (gate, Design, Permit, Phase 1, Build)
//! ```
//!
//! ## Documentation

use crate::ScheduleError;
use model::{DependencyId, DependencyType, Duration, Outline, Project, TaskId};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

/// What an arrow of the graph stands for.
///
/// # Variants
/// - `Link`: a dependency between two tasks, with everything needed to
///   calculate its limit.
/// - `Structure`: an outline arrow (gate → subtask, or subtask → summary
///   node). It carries no lag; the passes know what to do from the kinds of
///   nodes at its two ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// A dependency.
    Link {
        /// ID of the dependency (for "Why this date?").
        dependency: DependencyId,
        /// FS, SS, FF or SF.
        kind: DependencyType,
        /// The lag in working time (used when `lag_percent` is `None`).
        lag: Duration,
        /// The lag as a percentage of the predecessor's duration.
        lag_percent: Option<i32>,
    },
    /// An outline arrow.
    Structure,
}

/// One arrow of the graph, seen from one of its two nodes.
///
/// In `TaskGraph::predecessors[n]` the `node` field is the node the arrow
/// comes *from*; in `TaskGraph::successors[n]` it is the node it goes *to*.
///
/// # Fields
/// - `node`: the node at the other end of the arrow.
/// - `kind`: what the arrow stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Node number at the other end of the arrow.
    pub node: usize,
    /// Link or outline arrow.
    pub kind: EdgeKind,
}

/// The dependency graph of a project plus its calculation order.
///
/// # Fields
/// - `task_count`: the number of tasks; nodes `0..task_count` are tasks.
/// - `gate_of`: for each task row, the node number of its gate (`Some` only
///   for summary tasks).
/// - `summary_of_gate`: for each gate (node `task_count + k`), the row of
///   its summary task.
/// - `predecessors` / `successors`: the arrows into and out of each node.
/// - `order`: all node numbers in topological order.
///
/// # Example
/// ```
/// use engine::cpm::graph::TaskGraph;
/// use model::{Dependency, Duration, Outline, Project, Task};
/// use chrono::NaiveDate;
/// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
/// let a = Task::new("A", Duration::from_days(1));
/// let b = Task::new("B", Duration::from_days(1));
/// // B is listed first in the project but waits for A.
/// p.dependencies.push(Dependency::finish_to_start(a.id, b.id));
/// p.tasks.push(b);
/// p.tasks.push(a);
/// let graph = TaskGraph::build(&p, &Outline::build(&p.tasks)).unwrap();
/// assert_eq!(graph.order, vec![1, 0]); // A (row 1) must come before B (row 0)
/// ```
///
/// # Beginner note: `usize`
/// `usize` is the integer type Rust uses for positions in a list (indexes).
/// `vec[3]` needs a `usize`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskGraph {
    /// Number of task nodes (= number of tasks).
    pub task_count: usize,
    /// Gate node of each task row (summary tasks only).
    pub gate_of: Vec<Option<usize>>,
    /// Summary row of each gate (index = gate node - task_count).
    pub summary_of_gate: Vec<usize>,
    /// Incoming arrows of each node.
    pub predecessors: Vec<Vec<Edge>>,
    /// Outgoing arrows of each node.
    pub successors: Vec<Vec<Edge>>,
    /// Node numbers in topological order.
    pub order: Vec<usize>,
}

impl TaskGraph {
    /// Builds the graph of a project and computes its topological order.
    ///
    /// # What it does
    /// 1. Maps every task ID to its row, rejecting duplicate IDs.
    /// 2. Gives every summary task a gate node.
    /// 3. Turns every dependency into a link arrow, rejecting links to
    ///    unknown tasks. A link *into* a summary task ends at its gate; a
    ///    link *out of* a summary task starts at its summary node.
    /// 4. Adds the outline arrows (gate → subtask, subtask → summary node).
    ///    A subtask that is itself a summary is entered through *its* gate.
    /// 5. Runs Kahn's algorithm with "lowest node first" as tie-break.
    /// 6. If some nodes remain, finds a cycle among them and reports it.
    ///
    /// # Order of errors (deterministic)
    /// DuplicateTaskId, then UnknownTask (first bad link in list order),
    /// then Cycle.
    ///
    /// # Parameters
    /// - `project`: the project whose tasks and dependencies are used.
    /// - `outline`: the outline of `project.tasks` ([`Outline::build`]).
    ///
    /// # Returns
    /// The finished [`TaskGraph`].
    ///
    /// # Errors
    /// - [`ScheduleError::DuplicateTaskId`] if two tasks share an ID.
    /// - [`ScheduleError::UnknownTask`] if a link names a missing task.
    /// - [`ScheduleError::Cycle`] if the links (with the outline) form a
    ///   circle. A task linked to itself is a circle of one.
    ///
    /// # Example
    /// ```
    /// use engine::{cpm::graph::TaskGraph, ScheduleError};
    /// use model::{Dependency, Duration, Outline, Project, Task};
    /// use chrono::NaiveDate;
    /// let mut p = Project::new("Demo", NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    /// let a = Task::new("A", Duration::from_days(1));
    /// p.dependencies.push(Dependency::finish_to_start(a.id, a.id)); // A waits for A
    /// p.tasks.push(a);
    /// let outline = Outline::build(&p.tasks);
    /// assert!(matches!(TaskGraph::build(&p, &outline), Err(ScheduleError::Cycle { .. })));
    /// ```
    pub fn build(project: &Project, outline: &Outline) -> Result<TaskGraph, ScheduleError> {
        let task_count = project.tasks.len();

        // 1. TaskId -> row. A BTreeMap (sorted map) rather than a HashMap,
        //    so that nothing depends on a random iteration order.
        let mut row_of: BTreeMap<TaskId, usize> = BTreeMap::new();
        for (row, task) in project.tasks.iter().enumerate() {
            // `insert` returns the old value if the key was already there.
            if row_of.insert(task.id, row).is_some() {
                return Err(ScheduleError::DuplicateTaskId(task.id));
            }
        }

        // 2. Gate nodes, numbered after the tasks, in row order.
        let mut gate_of: Vec<Option<usize>> = vec![None; task_count];
        let mut summary_of_gate: Vec<usize> = Vec::new();
        for (row, gate) in gate_of.iter_mut().enumerate() {
            if outline.is_summary(row) {
                *gate = Some(task_count + summary_of_gate.len());
                summary_of_gate.push(row);
            }
        }
        let node_count = task_count + summary_of_gate.len();

        // `vec![Vec::new(); n]` makes `n` empty lists.
        let mut predecessors: Vec<Vec<Edge>> = vec![Vec::new(); node_count];
        let mut successors: Vec<Vec<Edge>> = vec![Vec::new(); node_count];
        // A small closure (an unnamed function written in place) that adds
        // one arrow to both lists.
        let mut connect = |from: usize, to: usize, kind: EdgeKind| {
            successors[from].push(Edge { node: to, kind });
            predecessors[to].push(Edge { node: from, kind });
        };

        // 3. Link arrows, in the order of `project.dependencies`.
        for link in &project.dependencies {
            let lookup = |id: TaskId| {
                row_of.get(&id).copied().ok_or(ScheduleError::UnknownTask {
                    dependency: link.id,
                    task: id,
                })
            };
            let pred = lookup(link.predecessor)?;
            let succ = lookup(link.successor)?;
            // Into a summary task = into its gate. `unwrap_or` keeps the
            // task's own node when it has no gate (it is not a summary).
            let target = gate_of[succ].unwrap_or(succ);
            connect(
                pred,
                target,
                EdgeKind::Link {
                    dependency: link.id,
                    kind: link.kind,
                    lag: link.lag,
                    lag_percent: link.lag_percent,
                },
            );
        }

        // 4. Outline arrows: gate -> subtask (entering a summary subtask
        //    through its gate), subtask -> summary node.
        for (k, &summary) in summary_of_gate.iter().enumerate() {
            let gate = task_count + k;
            let block = summary + 1..outline.subtree_end(summary);
            // `zip` pairs each row number of the block with its gate entry.
            for (child, child_gate) in block.clone().zip(&gate_of[block]) {
                if outline.parent(child) == Some(summary) {
                    let entry = child_gate.unwrap_or(child);
                    connect(gate, entry, EdgeKind::Structure);
                    connect(child, summary, EdgeKind::Structure);
                }
            }
        }

        // 5. Kahn's algorithm. `waiting[n]` = how many incoming arrows of
        //    node n have not been "used" yet.
        let mut waiting: Vec<usize> = predecessors.iter().map(Vec::len).collect();
        // The "ready" set is a BinaryHeap: a collection that always hands out
        // its largest element first. Wrapping node numbers in `Reverse` flips
        // that, so it hands out the *smallest* number first: our
        // deterministic tie-break.
        let mut ready: BinaryHeap<Reverse<usize>> = (0..node_count)
            .filter(|&n| waiting[n] == 0)
            .map(Reverse)
            .collect();
        let mut order: Vec<usize> = Vec::with_capacity(node_count);
        while let Some(Reverse(node)) = ready.pop() {
            order.push(node);
            for edge in &successors[node] {
                waiting[edge.node] -= 1;
                if waiting[edge.node] == 0 {
                    ready.push(Reverse(edge.node));
                }
            }
        }

        let graph = TaskGraph {
            task_count,
            gate_of,
            summary_of_gate,
            predecessors,
            successors,
            order,
        };

        // 6. Nodes that never became ready are stuck in (or behind) a cycle.
        if graph.order.len() < node_count {
            let rows = graph.cycle_rows(&waiting);
            return Err(ScheduleError::Cycle {
                tasks: rows.iter().map(|&r| project.tasks[r].id).collect(),
                names: rows
                    .iter()
                    .map(|&r| project.tasks[r].name.clone())
                    .collect(),
            });
        }
        Ok(graph)
    }

    /// The total number of nodes (tasks plus gates).
    pub fn node_count(&self) -> usize {
        self.predecessors.len()
    }

    /// The task row a node belongs to: the node itself for a task node, the
    /// summary task's row for a gate.
    ///
    /// # Example
    /// With 4 tasks and a gate for row 0 (node 4): `row_of(4) == 0`,
    /// `row_of(2) == 2`.
    pub fn row_of(&self, node: usize) -> usize {
        if node < self.task_count {
            node
        } else {
            self.summary_of_gate[node - self.task_count]
        }
    }

    /// `true` when `node` is a gate node.
    pub fn is_gate(&self, node: usize) -> bool {
        node >= self.task_count
    }

    /// Finds one circle among the stuck nodes and turns it into task rows.
    ///
    /// The gate and the summary node of one summary task both stand for the
    /// same task, so when they follow each other in the circle, the task is
    /// named only once.
    fn cycle_rows(&self, waiting: &[usize]) -> Vec<usize> {
        let nodes = find_cycle(&self.predecessors, waiting);
        let mut rows: Vec<usize> = Vec::new();
        for node in nodes {
            let row = self.row_of(node);
            if rows.last() != Some(&row) {
                rows.push(row);
            }
        }
        // The circle closes: the last and first may be the same task too.
        if rows.len() > 1 && rows.first() == rows.last() {
            rows.pop();
        }
        // Rotate so the lowest row comes first (deterministic message).
        if let Some(min_pos) = rows
            .iter()
            .enumerate()
            .min_by_key(|&(_, &row)| row)
            .map(|(pos, _)| pos)
        {
            rows.rotate_left(min_pos);
        }
        rows
    }
}

/// Finds one circle of nodes among the nodes that Kahn's algorithm could not
/// place.
///
/// # How it works
/// Every stuck node still has at least one stuck predecessor (otherwise it
/// would have become ready). So if we start at a stuck node and keep stepping
/// to a stuck predecessor, we must eventually visit a node a second time.
/// The steps between the two visits form a circle.
///
/// ```text
/// stuck: B, C, D      links: B->C, C->D, D->B
/// start at B (lowest number); step to stuck predecessors:
///   B <- D <- C <- B   B is visited again: the circle is B, D, C (walked backwards)
/// reversed into link order: C -> D -> B (-> C)
/// ```
///
/// # Parameters
/// - `predecessors`: incoming arrows of each node.
/// - `waiting`: Kahn's leftover counters; `> 0` means "stuck".
///
/// # Returns
/// Node numbers around the circle in link order.
fn find_cycle(predecessors: &[Vec<Edge>], waiting: &[usize]) -> Vec<usize> {
    let stuck = |n: usize| waiting[n] > 0;
    // The first stuck node is our starting point. There is always one,
    // because this is only called when nodes are left over.
    let Some(start) = (0..waiting.len()).find(|&n| stuck(n)) else {
        return Vec::new();
    };

    // Walk backwards, remembering the position at which each node was seen.
    let mut path: Vec<usize> = Vec::new();
    let mut seen_at: BTreeMap<usize, usize> = BTreeMap::new();
    let mut current = start;
    loop {
        if let Some(&position) = seen_at.get(&current) {
            // Found the circle: the part of the path from the first visit on.
            let mut cycle: Vec<usize> = path[position..].to_vec();
            // We walked against the arrows; reverse to follow them.
            cycle.reverse();
            return cycle;
        }
        seen_at.insert(current, path.len());
        path.push(current);
        // Step to the first stuck predecessor (in arrow order).
        match predecessors[current].iter().find(|e| stuck(e.node)) {
            Some(edge) => current = edge.node,
            None => return path, // cannot happen; return what we have
        }
    }
}
