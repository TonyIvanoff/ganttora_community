/**
 * ipc/types.ts: TypeScript shapes of the JSON that Rust sends and receives.
 *
 * ┌──────────────────────────────────────────────────────────────────────┐
 * │ HAND-WRITTEN. MUST MATCH THE RUST TYPES FIELD BY FIELD.              │
 * │ This file will be *generated* from the Rust types (ts-rs, Lead       │
 * │ decision M1) by scripts/gen-ipc-types.sh; after that it must never   │
 * │ be edited by hand. The generator needs `#[derive(TS)]`  │
 * │ in the core crates, which is not done yet. Until then: when a Rust   │
 * │ type below changes, change it here in the same commit.               │
 * └──────────────────────────────────────────────────────────────────────┘
 *
 * What it is for:
 *   Rust turns its structs into JSON with serde; this file tells TypeScript
 *   what that JSON looks like, so a typo such as `snapshot.projectFinsh` is
 *   a compile error instead of a silent `undefined` at run time.
 *
 * Where each type comes from (Rust source → TypeScript type):
 *   - backend/crates/document/src/reschedule.rs  `ScheduleSnapshot`, `TaskRow`, `LinkRow`, `CalendarRow`
 *   - backend/crates/document/src/command.rs     `Command`, `PlacedDependency`, `OutlineLevel`
 *   - backend/crates/document/src/lib.rs         `DocumentError` (as `CodedError`)
 *   - backend/crates/engine/src/lib.rs           `ScheduleError` (as `CodedError`)
 *   - backend/crates/engine/src/explain.rs       `Explanation`, `Driver`, `TaskEnd`
 *   - backend/crates/engine/src/blockers.rs      `Segment`, `SegmentKind`
 *   - backend/crates/model/src/blocker.rs        `Blocker`
 *   - backend/crates/document/src/reschedule.rs  `BlockerRow`
 *   - backend/crates/model/src/*.rs              `Task`, `Dependency`, `Duration`, `Constraint`, `Calendar`…
 *   - backend/src-tauri/src/ipc/project.rs       `ProjectResponse`
 *   - backend/src-tauri/src/ipc/calendar.rs      (plain numbers and dates; no new types)
 *   - backend/src-tauri/src/ipc/file.rs          `FileStatus`, `ReadNoteDto`, `NoticeDto`
 *   - backend/src-tauri/src/ipc/recovery.rs      `RecoveryCopyDto`
 *   - backend/crates/document/src/clipboard.rs   `ColumnLabels`, `PasteWarning`
 *   - backend/src-tauri/src/ipc/clipboard.rs     `PasteResponse`
 *   - backend/src-tauri/src/ipc/mod.rs           `IpcError`
 *   - backend/src-tauri/src/settings.rs          `Settings` and its groups, `SettingsNotice`
 *   - backend/src-tauri/src/ipc/settings.rs      `SettingsResponse`
 *
 * Main ideas:
 *   - Field names and enum values are camelCase (serde `rename_all = "camelCase"`),
 *     e.g. `"finishToStart"`, `"asSoonAsPossible"`.
 *   - Rust `Option<T>` becomes `T | null` (serde writes `null` for `None`).
 *   - Dates are chrono `NaiveDateTime`: ISO 8601 text *without* a time zone,
 *     e.g. "2027-03-24T17:00:00". See `features/gantt/renderer/dates.ts`.
 *   - UUIDs are text, e.g. "3f2b8c1e-9a4d-4e2f-8b1c-0d9e7f6a5b4c".
 *   - Errors from the core are *adjacently tagged*: `{ code, details }`.
 */

/** A UUID written as text. Every entity ID in Ganttora is one (never a row number). */
export type Uuid = string;

/** A date-time without time zone, ISO 8601, e.g. "2027-03-01T08:00:00". */
export type IsoDateTime = string;

/** A date without time, ISO 8601, e.g. "2027-12-25". */
export type IsoDate = string;

/**
 * An amount of *working* time. Rust: `model::Duration`.
 *
 * @example
 * const sixDays: Duration = { minutes: 6 * 480 }; // 480 = 8 h × 60 min
 */
export interface Duration {
  /** Working minutes. Negative only for leads on links. */
  minutes: number;
}

/** Auto- or manually scheduled. Rust: `model::TaskMode` (also used by the settings). */
export type TaskMode = "auto" | "manual";

/**
 * A date rule on a task. Rust: `model::Constraint`, serialized with
 * `tag = "type"`; the six dated kinds carry a `date`.
 *
 * @example
 * const snet: Constraint = { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" };
 */
export type Constraint =
  | { type: "asSoonAsPossible" }
  | { type: "asLateAsPossible" }
  | { type: "startNoEarlierThan"; date: IsoDateTime }
  | { type: "startNoLaterThan"; date: IsoDateTime }
  | { type: "finishNoEarlierThan"; date: IsoDateTime }
  | { type: "finishNoLaterThan"; date: IsoDateTime }
  | { type: "mustStartOn"; date: IsoDateTime }
  | { type: "mustFinishOn"; date: IsoDateTime };

/**
 * An unplanned delay recorded on a task. Rust: `model::Blocker`.
 *
 * The position is stored as "after this much of the task's own work", not
 * as a date, so the blocker moves with the task when a predecessor slips.
 * The UI usually sends `addBlockerAt` with a date instead and lets the core
 * work out `afterWork`.
 *
 * @example
 * const lateWood: Blocker = {
 *   id: "6f1c…", afterWork: { minutes: 960 }, duration: { minutes: 2400 },
 *   reason: "Wood delivery delayed", reportedOn: "2027-03-04",
 * };
 */
export interface Blocker {
  /** Unique ID of the blocker (a UUID). */
  id: Uuid;
  /** Working minutes of the task's own work done before the blocker starts. */
  afterWork: Duration;
  /** How long the task is blocked (working time on the task's calendar). */
  duration: Duration;
  /** Why the task is blocked (free text). */
  reason: string;
  /** The day the delay was reported, or `null`. */
  reportedOn: IsoDate | null;
}

/** How important a risk is. Rust: `model::RiskPriority`. */
export type RiskPriority = "low" | "medium" | "high";

/** Approved (known, nothing to do) or mitigated. Rust: `model::RiskResponse`. */
export type RiskResponse = "approved" | "mitigate";

/** Something that could go wrong with a task. Rust: `model::Risk`. */
export interface Risk {
  /** Unique ID of the risk (a UUID). */
  id: Uuid;
  /** What could go wrong. */
  description: string;
  /** How important it is. */
  priority: RiskPriority;
  /** What is done about it. */
  response: RiskResponse;
  /** How it is mitigated; empty for an approved risk. */
  mitigation: string;
  /** The user ticked "no consequence". */
  noConsequence: boolean;
  /** What happens if it goes wrong; empty when there is no consequence. */
  consequence: string;
}

/** Schedule from the project start or from its finish. Rust: `model::ScheduleFrom`. */
export type ScheduleFrom = "start" | "finish";

/**
 * A task as the user typed it. Rust: `model::Task`.
 *
 * The fields marked optional have `#[serde(default)]` in Rust: when they are
 * left out, Rust uses its default (auto mode, no constraint, no notes…).
 */
export interface Task {
  /** Unique ID. */
  id: Uuid;
  /** Task name. */
  name: string;
  /** Planned working time. */
  duration: Duration;
  /** Indent level: 1 = top level. */
  outlineLevel: number;
  /** Auto (default) or manually scheduled. */
  mode?: TaskMode;
  /** The typed start of a manually scheduled task. */
  manualStart?: IsoDateTime | null;
  /** Marked as a milestone (also with a duration). */
  milestone?: boolean;
  /** Date rule; default "as soon as possible". */
  constraint?: Constraint;
  /** Deadline, or `null`. */
  deadline?: IsoDateTime | null;
  /** Task calendar ID, or `null` for the project calendar. */
  calendarId?: Uuid | null;
  /** Free-text notes. */
  notes?: string;
  /** A web or file link. */
  hyperlink?: string;
  /** Unplanned delays on the task; left out when there are none. */
  blockers?: Blocker[];
  /** Percent complete, 0–100 (default 0). */
  percentComplete?: number;
  /** A project row: top level, holds tasks (default `false`). */
  isProject?: boolean;
  /** What could go wrong; left out when there are none. */
  risks?: Risk[];
}

/** The four link types. Rust: `model::DependencyType` (camelCase). */
export type DependencyType = "finishToStart" | "startToStart" | "finishToFinish" | "startToFinish";

/** A link between two tasks. Rust: `model::Dependency`. */
export interface Dependency {
  /** Unique ID of the link. */
  id: Uuid;
  /** The task that comes first. */
  predecessor: Uuid;
  /** The task that waits for it. */
  successor: Uuid;
  /** Link type. */
  kind: DependencyType;
  /** Waiting time after the predecessor (negative = lead). */
  lag: Duration;
  /** Lag as a percent of the predecessor's duration, or `null` (optional in JSON). */
  lagPercent?: number | null;
}

/** A link plus its position in the project's link list. Rust: `document::PlacedDependency`. */
export interface PlacedDependency {
  /** Position in the list (0 = first). */
  index: number;
  /** The link. */
  dependency: Dependency;
}

/** One block of working time in a day, in minutes after midnight. Rust: `model::WorkingTime`. */
export interface WorkingTime {
  /** Start, e.g. 480 = 08:00. */
  startMinute: number;
  /** End, e.g. 1020 = 17:00. */
  endMinute: number;
}

/** The working times of each weekday. Rust: `model::WorkWeek`. */
export interface WorkWeek {
  monday: WorkingTime[];
  tuesday: WorkingTime[];
  wednesday: WorkingTime[];
  thursday: WorkingTime[];
  friday: WorkingTime[];
  saturday: WorkingTime[];
  sunday: WorkingTime[];
}

/** A date range with its own working times (none = days off). Rust: `model::CalendarException`. */
export interface CalendarException {
  /** Name, e.g. "Company holiday". */
  name: string;
  /** First day. */
  start: IsoDate;
  /** Last day. */
  finish: IsoDate;
  /** Working times on those days; empty = not working. */
  workingTimes: WorkingTime[];
}

/** A working-time calendar. Rust: `model::Calendar`. */
export interface Calendar {
  /** Unique ID. */
  id: Uuid;
  /** Name, e.g. "Standard". */
  name: string;
  /** The normal week. */
  week: WorkWeek;
  /** Single days off. */
  holidays: IsoDate[];
  /** Ranges with their own working times. */
  exceptions?: CalendarException[];
}

/** One task's new outline level. Rust: `document::OutlineLevel`. */
export interface OutlineLevel {
  /** The task. */
  id: Uuid;
  /** Its new level (1 = top). */
  level: number;
}

/**
 * One edit command (decision D16: every edit is a command). Rust:
 * `document::Command`, serialized with `tag = "type"`: the variant name
 * goes into a `"type"` field.
 *
 * Beginner note: this is a *discriminated union*. The value is exactly one
 * of the shapes below, and the `type` field says which one. After
 * `if (cmd.type === "setTaskDuration")`, TypeScript knows `cmd.duration`
 * exists.
 *
 * @example
 * const cmd: Command = { type: "setTaskDuration", id: taskId, duration: { minutes: 2880 } };
 */
export type Command =
  | { type: "addTask"; index: number; task: Task }
  | { type: "removeTask"; id: Uuid }
  | { type: "restoreTask"; index: number; task: Task; dependencies: PlacedDependency[] }
  | { type: "restoreTasks"; index: number; tasks: Task[]; dependencies: PlacedDependency[] }
  | { type: "renameTask"; id: Uuid; name: string }
  | { type: "setTaskDuration"; id: Uuid; duration: Duration }
  | { type: "setTaskPercentComplete"; id: Uuid; percent: number }
  | { type: "setTaskPlannedFinish"; id: Uuid; plannedFinish: IsoDateTime | null }
  | { type: "setTaskActualFinish"; id: Uuid; actualFinish: IsoDateTime | null }
  | { type: "setTaskNotes"; id: Uuid; notes: string }
  | { type: "setTaskHyperlink"; id: Uuid; hyperlink: string }
  | { type: "setTaskMode"; id: Uuid; mode: TaskMode; manualStart: IsoDateTime | null }
  | { type: "setTaskManualStart"; id: Uuid; start: IsoDateTime | null }
  | { type: "setTaskMilestone"; id: Uuid; milestone: boolean }
  | { type: "setTaskConstraint"; id: Uuid; constraint: Constraint }
  | { type: "setTaskDeadline"; id: Uuid; deadline: IsoDateTime | null }
  | { type: "setTaskCalendar"; id: Uuid; calendarId: Uuid | null }
  | { type: "indentTask"; id: Uuid }
  | { type: "outdentTask"; id: Uuid }
  | { type: "setOutlineLevels"; levels: OutlineLevel[] }
  | { type: "moveTask"; id: Uuid; toIndex: number; level: number | null }
  | { type: "addDependency"; dependency: Dependency }
  | { type: "removeDependency"; id: Uuid }
  | { type: "restoreDependency"; placed: PlacedDependency }
  | {
      type: "editDependency";
      id: Uuid;
      kind: DependencyType;
      lag: Duration;
      lagPercent: number | null;
    }
  | { type: "setProjectStart"; start: IsoDateTime }
  | { type: "setScheduleFrom"; scheduleFrom: ScheduleFrom; finish: IsoDateTime | null }
  | { type: "renameProject"; name: string }
  | { type: "setProjectCalendar"; calendarId: Uuid }
  | { type: "addCalendar"; calendar: Calendar }
  | { type: "removeCalendar"; id: Uuid }
  | { type: "restoreCalendar"; index: number; calendar: Calendar }
  | { type: "updateCalendar"; calendar: Calendar }
  /** Adds a risk at the end of the task's list. */
  | { type: "addRisk"; task: Uuid; risk: Risk }
  /** Replaces a risk (found by `risk.id`) with a new version. */
  | { type: "editRisk"; task: Uuid; risk: Risk }
  /** Deletes one risk of a task. */
  | { type: "removeRisk"; task: Uuid; id: Uuid }
  /** Puts a deleted risk back at its old position (the undo of `removeRisk`). */
  | { type: "restoreRisk"; task: Uuid; index: number; risk: Risk }
  /** Adds a blocker with a known `afterWork` position (used by redo and tests). */
  | { type: "addBlocker"; task: Uuid; blocker: Blocker }
  /**
   * Adds a blocker that starts at a moment (what the UI sends). The core
   * turns `at` into `afterWork` using the current schedule: a moment before
   * the task blocks it before any work, a moment at or after its finish
   * extends its end.
   */
  | {
      type: "addBlockerAt";
      task: Uuid;
      id: Uuid;
      at: IsoDateTime;
      duration: Duration;
      reason: string;
      reportedOn: IsoDate | null;
    }
  /** Replaces a blocker (found by `blocker.id`) with a new version. */
  | { type: "editBlocker"; task: Uuid; blocker: Blocker }
  /** Deletes one blocker of a task. */
  | { type: "removeBlocker"; task: Uuid; id: Uuid }
  /** Puts a deleted blocker back at its old position (the undo of `removeBlocker`). */
  | { type: "restoreBlocker"; task: Uuid; index: number; blocker: Blocker }
  /**
   * Several commands as one all-or-nothing change and one undo step
   * (e.g. the Task Information dialog's OK). If one is refused, none is
   * applied.
   */
  | { type: "batch"; commands: Command[] };

/** Which end of a task the "Why this date?" reason fixed. Rust: `engine::TaskEnd`. */
export type TaskEnd = "start" | "finish";

/**
 * The winning reason for a task's dates. Rust: `engine::Driver`
 * (`tag = "type"`).
 */
export type Driver =
  | { type: "projectStart" }
  | { type: "projectFinish" }
  | { type: "predecessor"; dependency: Uuid; task: Uuid; kind: DependencyType }
  | { type: "successor"; dependency: Uuid; task: Uuid; kind: DependencyType }
  | { type: "constraint"; constraint: Constraint }
  | { type: "deadline"; date: IsoDateTime }
  | { type: "manualStart" }
  | { type: "subtasks" };

/** The full "Why this date?" answer for one task. Rust: `engine::Explanation`. */
export interface Explanation {
  /** The winning reason. */
  driver: Driver;
  /** Whether the reason fixed the start or the finish. */
  end: TaskEnd;
  /** The moment the reason asked for, before the calendar moved it. */
  drivenDate: IsoDateTime;
  /** `true` if the calendar moved the date to the next working time. */
  movedByCalendar: boolean;
  /** The calendar used for this task. */
  calendarId: Uuid;
  /** The summary task the reason came through, or `null`. */
  viaSummary: Uuid | null;
  /**
   * Blocked time of the task's own blockers that pushed its finish later
   * Zero for tasks without blockers. Optional: snapshots made
   * before blockers existed do not have it.
   */
  blocked?: Duration;
}

/**
 * What one piece of a bar is. Rust: `engine::SegmentKind` (`tag = "type"`).
 * `blocker` names the blocker that causes a blocked piece.
 */
export type SegmentKind = { type: "work" } | { type: "blocked"; blocker: Uuid };

/**
 * One piece of a task's bar, work or blocked. Rust: `engine::Segment`.
 *
 * @example
 * const piece: Segment = { start: "2027-03-05T08:00:00", finish: "2027-03-11T17:00:00",
 *   kind: { type: "blocked", blocker: "6f1c…" } };
 */
export interface Segment {
  /** Start of the piece. */
  start: IsoDateTime;
  /** Finish of the piece. */
  finish: IsoDateTime;
  /** Work or blocked. */
  kind: SegmentKind;
}

/**
 * One blocker of a task as the UI shows it: what the user typed plus where
 * the engine placed the blocked time. Rust: `document::BlockerRow`.
 */
export interface BlockerRow {
  /** The blocker's UUID. */
  id: Uuid;
  /** Work done before the blocker starts, in working minutes. */
  afterWorkMinutes: number;
  /** Blocked time in working minutes. */
  durationMinutes: number;
  /** Why the task is blocked. */
  reason: string;
  /** The day the delay was reported, or `null`. */
  reportedOn: IsoDate | null;
  /** Calculated start of the blocked piece, or `null` (no schedule, or zero length). */
  start: IsoDateTime | null;
  /** Calculated finish of the blocked piece, or `null`. */
  finish: IsoDateTime | null;
}

/** One incoming link of a task, as shown in the grid. Rust: `document::LinkRow`. */
export interface LinkRow {
  /** Link ID. */
  id: Uuid;
  /** The task this link comes from. */
  predecessor: Uuid;
  /** Link type. */
  kind: DependencyType;
  /** Lag in working minutes (negative = lead). */
  lagMinutes: number;
  /** Lag in percent, or `null`. */
  lagPercent: number | null;
}

/** A calendar's ID and name, for pickers. Rust: `document::CalendarRow`. */
export interface CalendarRow {
  /** Calendar ID. */
  id: Uuid;
  /** Calendar name. */
  name: string;
}

/**
 * One row of the schedule: what the user typed plus what the engine
 * calculated. Rust: `document::TaskRow`.
 *
 * The date and slack fields are `null` when the project could not be
 * scheduled (see {@link ScheduleSnapshot.error}).
 */
export interface TaskRow {
  /** Task ID. */
  id: Uuid;
  /** Task name. */
  name: string;
  /** Working time in minutes (divide by `minutesPerDay` for days). For a summary: rolled up. */
  durationMinutes: number;
  /** Indent level: 1 = top level. */
  outlineLevel: number;
  /** Outline number as shown in the grid, e.g. "1.2.3". */
  outlineNumber: string;
  /** The summary task this task belongs to, or `null` at the top level. */
  parentId: Uuid | null;
  /** Has subtasks (its dates are rolled up from them). */
  isSummary: boolean;
  /** Drawn as a diamond: marked as milestone, or zero duration. */
  isMilestone: boolean;
  /** Auto or manually scheduled. */
  mode: TaskMode;
  /** The task's date rule. */
  constraint: Constraint;
  /** Deadline, or `null`. */
  deadline: IsoDateTime | null;
  /** Task calendar, or `null` for the project calendar. */
  calendarId: Uuid | null;
  /** Notes text. */
  notes: string;
  /** Hyperlink text. */
  hyperlink: string;
  /** Scheduled start: what the Gantt bar shows. */
  start: IsoDateTime | null;
  /** Scheduled finish: what the Gantt bar shows. */
  finish: IsoDateTime | null;
  /** Earliest start (forward pass). */
  earlyStart: IsoDateTime | null;
  /** Earliest finish (forward pass). */
  earlyFinish: IsoDateTime | null;
  /** Latest start (backward pass). */
  lateStart: IsoDateTime | null;
  /** Latest finish (backward pass). */
  lateFinish: IsoDateTime | null;
  /** Total slack in working minutes. */
  totalSlackMinutes: number | null;
  /** Free slack in working minutes. */
  freeSlackMinutes: number | null;
  /** On the critical path (drawn with a dark outline)? */
  isCritical: boolean;
  /** Finishes after its deadline. */
  missedDeadline: boolean;
  /** IDs of the tasks this task depends on. */
  predecessors: Uuid[];
  /** The incoming links with their type and lag. */
  links: LinkRow[];
  /** "Why this date?", or `null` if not scheduled. */
  explanation: Explanation | null;
  /**
   * The task's blockers. Left out of the JSON when the task
   * has none, hence optional: read it as `row.blockers ?? []`.
   */
  blockers?: BlockerRow[];
  /**
   * The work and blocked pieces of the bar, in time order. Left out when the
   * task has no blockers (then the bar is one piece, `start`..`finish`).
   */
  segments?: Segment[];
  /**
   * Total blocked time in working minutes; left out when zero.
   * `durationMinutes` stays the planned work; `start`..`finish` include the
   * blocked time.
   */
  blockedMinutes?: number;
  /** Percent complete, 0–100; calculated for groups. Left out when 0. */
  percentComplete?: number;
  /** A project row. Left out when `false`. */
  isProject?: boolean;
  /** What could go wrong. Left out when there are none. */
  risks?: Risk[];
  /** The finish saved by "Set plan". Left out when there is none. */
  plannedFinish?: IsoDateTime;
  /** When the work really finished. Left out when not known. */
  actualFinish?: IsoDateTime;
}

/**
 * An error from the core, in serde's *adjacently tagged* form: the variant
 * name in `code`, its fields in `details`. Rust: `document::DocumentError`,
 * `engine::ScheduleError`, `engine::CalendarError`. `details` may itself be
 * a `CodedError` (e.g. `rejected` → `cycleDetected`), a UUID text, an object
 * of values, or missing.
 *
 * @example
 * const e: CodedError = { code: "cycleDetected", details: { tasks: ["…"], names: ["A", "B"] } };
 */
export interface CodedError {
  /** camelCase code, e.g. "cycleDetected". The UI key is `error.<code>`. */
  code: string;
  /** The variant's fields, if any. */
  details?: unknown;
}

/** The whole schedule for the UI, in row order. Rust: `document::ScheduleSnapshot`. */
export interface ScheduleSnapshot {
  /** Project ID. */
  projectId: Uuid;
  /** Project name, e.g. "Garden shed". */
  projectName: string;
  /** Working minutes in one day (480 = 8 hours). Used to show days. */
  minutesPerDay: number;
  /** Scheduled from the start (usual) or from the finish. */
  scheduleFrom: ScheduleFrom;
  /** The typed project finish (for scheduling from the finish), or `null`. */
  finishDate: IsoDateTime | null;
  /** The project calendar's ID. */
  calendarId: Uuid;
  /** All calendars of the project (ID and name). */
  calendars: CalendarRow[];
  /** Project start, or `null` if scheduling failed. */
  projectStart: IsoDateTime | null;
  /** Project finish, or `null` if scheduling failed. */
  projectFinish: IsoDateTime | null;
  /** Why scheduling failed, or `null` if it worked. Shown with `lib/errors.ts`. */
  error: CodedError | null;
  /** All tasks, in row order. */
  tasks: TaskRow[];
}

/**
 * Something in an opened `.gtra` file that did not fit the model exactly.
 * Rust: `ReadNoteDto` in `backend/src-tauri/src/ipc/file.rs` (serde
 * `tag = "kind"`). Data only: the UI picks the message by `kind` from
 * `locales/en.json` (keys `file.note.<kind>`).
 *
 * @example
 * const note: ReadNoteDto = { kind: "rowsNotLoaded", table: "resources", count: 3 };
 */
export type ReadNoteDto =
  | { kind: "startDateMissing"; used: IsoDateTime }
  | { kind: "calendarMissing"; calendarId: Uuid }
  | { kind: "durationMissing"; taskId: Uuid }
  | { kind: "baseCalendarFlattened"; calendarId: Uuid; baseId: string }
  | { kind: "valueNotLoaded"; table: string; rowId: string; column: string; value: string }
  | { kind: "rowsNotLoaded"; table: string; count: number };

/**
 * A notice with a code and the values for its message. Rust: `NoticeDto`
 * in `backend/src-tauri/src/ipc/file.rs`. Shown as `error.<code>`.
 *
 * @example
 * const n: NoticeDto = { code: "gtra.newer_format", params: { found: "3", supported: "2" } };
 */
export interface NoticeDto {
  /** Stable code, e.g. "gtra.newer_format". */
  code: string;
  /** Values for the placeholders, all as text. */
  params: Record<string, string>;
}

/**
 * Facts about the file behind the open project. Rust: `FileStatus` in
 * `backend/src-tauri/src/ipc/file.rs`.
 */
export interface FileStatus {
  /** Full path, or `null` if the project was never saved. */
  path: string | null;
  /** Just the file name, e.g. "shed.gtra", or `null` if never saved. */
  fileName: string | null;
  /** From a newer Ganttora: "Save" is refused, offer "Save As". */
  readOnly: boolean;
  /** Changed since it was last opened or saved (show "•"). */
  dirty: boolean;
  /** Notes from reading the file. Non-empty only after open or restore. */
  notes: ReadNoteDto[];
  /** The older format version the file was upgraded from, or `null`. */
  upgradedFrom: number | null;
  /** Why the file is read-only, or `null`. */
  readOnlyReason: NoticeDto | null;
}

/**
 * What every project command and file command answers. Rust:
 * `ProjectResponse` in `backend/src-tauri/src/ipc/project.rs`.
 */
export interface ProjectResponse {
  /** The new schedule. */
  snapshot: ScheduleSnapshot;
  /** Enable the Undo button? */
  canUndo: boolean;
  /** Enable the Redo button? */
  canRedo: boolean;
  /** The file behind the project. */
  file: FileStatus;
  /**
   * Every calendar of the project with its full working times (the
   * snapshot only lists IDs and names). Used by the calendar editor and by
   * the Gantt chart (non-working shading, snapping drags to working days).
   */
  calendars: Calendar[];
}

/**
 * A recovery copy left behind by a crash. Rust: `RecoveryCopyDto` in
 * `backend/src-tauri/src/ipc/recovery.rs`.
 */
export interface RecoveryCopyDto {
  /** The id to send to `recovery_restore` / `recovery_discard`. */
  sessionId: string;
  /** The project's file, or `null` if it was never saved. */
  sourcePath: string | null;
  /** Just the file name, or `null`. */
  sourceFileName: string | null;
  /** When the copy was written (UTC, e.g. "2027-02-15T10:42:07Z"), or `null`. */
  savedAt: string | null;
  /** Project name from the copy, or `null` if unreadable. */
  projectName: string | null;
  /** `false` for a damaged copy (only Discard makes sense). */
  readable: boolean;
}

// ---------------------------------------------------------------------------
// Clipboard (M1). Rust: backend/crates/document/src/clipboard.rs and
// backend/src-tauri/src/ipc/clipboard.rs
// ---------------------------------------------------------------------------

/**
 * Column headings for copied and pasted task text, in the UI language.
 * Rust: `document::clipboard::ColumnLabels`.
 *
 * @example
 * const labels: ColumnLabels = { name: "Name", duration: "Duration", start: "Start",
 *   finish: "Finish", predecessors: "Predecessors", outlineLevel: "Outline level" };
 */
export interface ColumnLabels {
  /** Heading of the Name column. */
  name: string;
  /** Heading of the Duration column. */
  duration: string;
  /** Heading of the Start column. */
  start: string;
  /** Heading of the Finish column. */
  finish: string;
  /** Heading of the Predecessors column. */
  predecessors: string;
  /** Heading of the Outline level column. */
  outlineLevel: string;
}

/**
 * A pasted cell that could not be used as it was (the paste still went
 * ahead). Rust: `document::clipboard::PasteWarning` (`tag = "code"`).
 * `row` is the data row in the pasted text, counted from 1. Shown as
 * `paste.<code>`.
 */
export type PasteWarning =
  | { code: "unreadableDuration"; row: number; text: string }
  | { code: "unreadableOutlineLevel"; row: number; text: string }
  | { code: "unreadablePredecessor"; row: number; text: string }
  | { code: "predecessorNotPasted"; row: number; predecessorRow: number }
  | { code: "selfLink"; row: number }
  | { code: "duplicateLink"; row: number; predecessorRow: number }
  | { code: "linkToOwnSummary"; row: number; predecessorRow: number };

/** The answer of `paste_tasks`. Rust: `PasteResponse` in `ipc/clipboard.rs`. */
export interface PasteResponse {
  /** The project after the paste. */
  response: ProjectResponse;
  /** IDs of the new tasks in row order (empty: nothing was pasted). */
  taskIds: Uuid[];
  /** Cells that could not be used as they were. */
  warnings: PasteWarning[];
}

// ---------------------------------------------------------------------------
// Errors with a code (M1)
// ---------------------------------------------------------------------------

/**
 * The error every Rust command rejects with. Rust: `IpcError` in
 * `backend/src-tauri/src/ipc/mod.rs`. The UI shows the translated text
 * `error.<code>` (see `frontend/lib/errors.ts`), never `detail`.
 *
 * Codes: shell (`noProjectOpen`, `recoveryNotFound`…), settings
 * (`settingsSaveFailed`…), document (`taskNotFound`, `rejected`…; then
 * `details` holds the engine's {@link CodedError}) and file format
 * (`gtra.newer_format`…).
 *
 * @example
 * const e: IpcError = { code: "settingsSaveFailed", detail: "disk full" };
 */
export interface IpcError {
  /** Stable code, e.g. "settingsSaveFailed" or "gtra.file_not_found". */
  code: string;
  /** English explanation for logs; may be empty. */
  detail: string;
  /** Values for the message (or a nested {@link CodedError}); often missing. */
  details?: unknown;
}

// ---------------------------------------------------------------------------
// Settings (M1). Rust: backend/src-tauri/src/settings.rs
// ---------------------------------------------------------------------------

/** Light, dark, or follow the operating system. Rust: `Theme`. */
export type Theme = "light" | "dark" | "system";

/** First day of the week. Rust: `FirstDayOfWeek`. */
export type FirstDayOfWeek = "system" | "monday" | "sunday" | "saturday";

/** What stays fixed when duration, work or units change. Rust: `TaskType`. */
export type TaskType = "fixedUnits" | "fixedDuration" | "fixedWork";

/** The "User" group. Rust: `UserSettings`. */
export interface UserSettings {
  /** Full name for the change journal; empty = the computer's user name. */
  displayName: string;
  /** Initials, at most 4 characters. */
  initials: string;
  /** Optional e-mail address (stored only). */
  email: string;
}

/** The "General" group. Rust: `GeneralSettings`. */
export interface GeneralSettings {
  /** UI language code, e.g. "en", "zh-CN". */
  language: string;
  /** Light / dark / system. */
  theme: Theme;
  /** "system" (OS region), "iso" (2027-03-24), a pattern like "dd.mm.yyyy", or (older files) a locale code. */
  dateFormat: string;
  /** First day of the week. */
  firstDayOfWeek: FirstDayOfWeek;
  /** ISO 4217 currency code, e.g. "EUR". */
  currency: string;
  /** Zoom of the whole UI in percent, 80 to 150. */
  uiZoomPercent: number;
}

/** The "Scheduling defaults" group. Rust: `SchedulingSettings`. */
export interface SchedulingSettings {
  /** New tasks are auto- or manually scheduled. */
  newTaskMode: TaskMode;
  /** Task type of new tasks. */
  defaultTaskType: TaskType;
  /** New tasks are effort-driven. */
  effortDriven: boolean;
  /** Working hours per day (0.5–24). */
  hoursPerDay: number;
  /** Working hours per week (1–168). */
  hoursPerWeek: number;
  /** Calendar id for new projects, e.g. "standard". */
  defaultCalendar: string;
}

/** The "Files" group. Rust: `FileSettings`. */
export interface FileSettings {
  /** Folder the Save As window starts in, or `null` for the system's choice. */
  defaultSaveFolder: string | null;
  /** Minutes between autosaves (0 = off, max 120). */
  autosaveMinutes: number;
  /** Days of detailed version history to keep (1–3650). */
  historyRetentionDays: number;
  /** How many recent files to list (0–50). */
  recentFilesMax: number;
}

/** The "Updates & privacy" group. Rust: `UpdateSettings`. */
export interface UpdateSettings {
  /** Install updates automatically. */
  autoUpdate: boolean;
  /** Send crash reports (opt-in, off by default). */
  crashReports: boolean;
}

/** The "Grid" group: the task grid's layout. Rust: `GridSettings`. */
export interface GridSettings {
  /**
   * Width of each column in CSS pixels, by column ID (e.g. `name: 300`).
   * A column missing here uses its default width (24–1200 when present).
   */
  columnWidths: Record<string, number>;
}

/** Where a toolbar sits: docked to an edge, or floating. Rust: `ToolbarDock`. */
export type ToolbarDock = "top" | "bottom" | "left" | "right" | "float";

/** One toolbar of the layout. Rust: `ToolbarLayout`. */
export interface ToolbarLayout {
  /** "main", "task", "view" (predefined), or a custom toolbar's own ID. */
  id: string;
  /** Name of a custom toolbar; empty for a predefined one (its name is translated). */
  name: string;
  /** Action IDs (`features/toolbars/actions.ts`) and "separator", in order. */
  items: string[];
  /** The edge it is docked to, or "float". */
  dock: ToolbarDock;
  /** Row at the top or bottom edge, 0–2 (0 = uppermost). */
  row: number;
  /** Left edge while floating, in CSS px inside the main frame. */
  x: number;
  /** Top edge while floating, in CSS px inside the main frame. */
  y: number;
  /** Shown or hidden. */
  visible: boolean;
}

/** The "View" group: the Gantt chart's zoom. Rust: `ViewSettings`. */
export interface ViewSettings {
  /** Header units. */
  timescale: "days" | "weeks" | "months";
  /** Zoom in percent (10–400). */
  zoomPercent: number;
}

/** The "Toolbars" group. Rust: `ToolbarSettings`. */
export interface ToolbarSettings {
  /** The toolbars in display order; empty = the predefined ones. */
  layout: ToolbarLayout[];
}

/** All app settings: the content of settings.json. Rust: `Settings`. */
export interface Settings {
  /** Layout version of the file (1). */
  version: number;
  /** User group. */
  user: UserSettings;
  /** General group. */
  general: GeneralSettings;
  /** Scheduling defaults group. */
  scheduling: SchedulingSettings;
  /** Files group. */
  files: FileSettings;
  /** Updates & privacy group. */
  updates: UpdateSettings;
  /** Task grid layout (changed by dragging column edges, not in the dialog). */
  grid: GridSettings;
  /** Toolbars and their places (changed by dragging and in Customize). */
  toolbars: ToolbarSettings;
  /** The Gantt chart's zoom (changed by zooming, not in the dialog). */
  view: ViewSettings;
}

/**
 * A problem found when settings.json was loaded. Rust: `SettingsNotice`.
 * The UI shows `settings.notice.<kind>`.
 */
export type SettingsNotice =
  | { kind: "corrupt"; backupPath: string | null }
  | { kind: "unreadable" }
  | { kind: "noConfigFolder" };

/** The answer of `settings_get`. Rust: `SettingsResponse` in `ipc/settings.rs`. */
export interface SettingsResponse {
  /** The current settings. */
  settings: Settings;
  /** A start-up problem with the file, or `null`. */
  notice: SettingsNotice | null;
}

// ---------------------------------------------------------------------------
// The native menu bar. Rust: backend/src-tauri/src/menu.rs
// ---------------------------------------------------------------------------

/** Menu entries the operating system provides. Rust: `Predefined`. */
export type MenuPredefined =
  | "undo"
  | "redo"
  | "cut"
  | "copy"
  | "paste"
  | "selectAll"
  | "services"
  | "hide"
  | "hideOthers"
  | "showAll"
  | "minimize"
  | "zoomWindow"
  | "closeWindow"
  | "fullscreen";

/** One part of the menu bar. Rust: `MenuNode`. */
export type MenuNode =
  | {
      kind: "item";
      /** Sent back when chosen. */
      id: string;
      /** The text. */
      label: string;
      /** `false` = greyed out. */
      enabled: boolean;
      /** Present = an on/off entry with a tick. */
      checked?: boolean;
      /** Keyboard shortcut, e.g. "CmdOrCtrl+S". */
      accelerator?: string;
    }
  | { kind: "separator" }
  | { kind: "predefined"; name: MenuPredefined; label: string }
  | { kind: "submenu"; label: string; items: MenuNode[] };

/** New state of one menu entry. Rust: `MenuStateUpdate`. */
export interface MenuStateUpdate {
  /** The entry. */
  id: string;
  /** `false` = greyed out. */
  enabled: boolean;
  /** The tick; `null` for a plain entry. */
  checked: boolean | null;
}
