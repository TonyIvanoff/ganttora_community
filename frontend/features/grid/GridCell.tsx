/**
 * features/grid/GridCell.tsx: what one grid cell shows, and the small
 * editor that replaces it while the user types.
 *
 * What it is for:
 *   `TaskGrid.tsx` handles rows, scrolling, selection and keys. For the
 *   special cells it uses the components here: the Name cell (indent,
 *   collapse button, bold summaries), the Indicators cell (symbols), and
 *   while editing the {@link CellEditor}. Plain cell text comes from
 *   `gridFormat.ts`.
 *
 * Where it fits:
 *   Only used by `TaskGrid.tsx`. The editor reports the typed text back;
 *   `cellEdit.ts` turns it into commands.
 *
 * Main ideas:
 *   - **Three kinds of editors:** a text field (name, duration,
 *     predecessors), a date field with a calendar (start, finish) and a list
 *     (mode). See {@link editorKind}.
 *   - **Keys inside the editor:** Enter saves and moves down (Shift+Enter:
 *     up), Tab saves and moves right (Shift+Tab: left), Escape cancels, and
 *     in a text field ↑ / ↓ save and move like Enter. Leaving the cell with
 *     the mouse also saves.
 *   - The keys the editor handles are stopped (`stopPropagation`), so the
 *     grid's own shortcuts (Delete = delete task!) never fire while typing.
 */

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import { DateField } from "@frontend/components/ui/DateField";
import { formatNumber, formatScheduleDate } from "@frontend/i18n/format";
import {
  CONSTRAINT_KEYS,
  percentChoices,
  type EditMove,
  type EditorKind,
  type EditorOption,
} from "./gridFormat";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { projectBlockers } from "@frontend/features/task-info/blockers";
import {
  higherPriority,
  highestPriority,
  missedStartPriority,
  missedStarts,
  taskRisks,
} from "@frontend/features/task-info/risks";
import { progressProblem } from "./gridFormat";
import type { Calendar, Risk, RiskPriority, TaskRow } from "@frontend/ipc/types";
import type { GridRow } from "./gridRows";

/** Pixels of indent per outline level in the Name column. */
const INDENT_PX = 16;

/** Props of {@link CellEditor}. */
export interface CellEditorProps {
  /** Which editor to show. */
  kind: EditorKind;
  /** The choices of a "list" editor. */
  options?: readonly EditorOption[];
  /** The text to start with. */
  initialText: string;
  /** `true` to select all text at the start (Enter/F2); `false` to put the caret at the end (typing). */
  selectAll: boolean;
  /** Accessible name of the field (the column title). */
  label: string;
  /** A translated error message to show under the cell, or `null`. */
  error: string | null;
  /**
   * Called to save. Returns `false` if the text was not accepted (the
   * editor then stays open).
   */
  onCommit: (text: string, move: EditMove) => boolean;
  /** Called to cancel (Escape). */
  onCancel: () => void;
}

/**
 * The editor shown inside a cell while typing.
 *
 * @param props - See {@link CellEditorProps}.
 * @returns A text field, date field or list, filling the cell.
 *
 * Beginner note: `useRef` here holds a flag ("already saved or
 * cancelled") that must not cause a redraw when it changes, unlike
 * `useState`.
 */
export function CellEditor(props: CellEditorProps) {
  const { kind, options = [], initialText, selectAll, label, error, onCommit, onCancel } = props;
  const [text, setText] = useState(initialText);
  const inputRef = useRef<HTMLInputElement | HTMLSelectElement | null>(null);
  // Set when Enter/Tab/Escape already closed the editor, so the "blur"
  // that follows (focus goes back to the grid) does not save a second time.
  const doneRef = useRef(false);

  // Put the focus into the field when the editor appears.
  useEffect(() => {
    const input = inputRef.current;
    if (input === null) return;
    input.focus();
    if (input instanceof HTMLInputElement && input.type === "text") {
      if (selectAll || kind === "date") input.select();
      else input.setSelectionRange(input.value.length, input.value.length);
    }
    // Only once, when the editor appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * Saves with a move; reopens the field if the text was refused.
   *
   * @param move - Where the active cell goes afterwards.
   * @param value - The text to save (default: what the field shows).
   */
  function save(move: EditMove, value: string = text): void {
    if (doneRef.current) return;
    doneRef.current = true;
    if (!onCommit(value, move)) {
      doneRef.current = false;
      inputRef.current?.focus();
    }
  }

  /**
   * Handles the keys described in the file header.
   *
   * @param event - The key press inside the editor.
   */
  function onKeyDown(event: KeyboardEvent<HTMLInputElement | HTMLSelectElement>): void {
    let handled = true;
    if (event.key === "Enter") save(event.shiftKey ? "up" : "down");
    else if (event.key === "Tab") save(event.shiftKey ? "left" : "right");
    else if (event.key === "Escape") {
      doneRef.current = true;
      onCancel();
    } else if (kind === "percent" && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      // Step through the list; Enter saves.
      const now = Number.parseInt(text, 10) || 0;
      const choices = percentChoices(initialText);
      const next =
        event.key === "ArrowDown"
          ? choices.find((choice) => choice > now)
          : choices.filter((choice) => choice < now).at(-1);
      if (next !== undefined) setText(String(next));
    } else if (kind === "text" && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      save(event.key === "ArrowUp" ? "up" : "down");
    } else handled = false;
    // Every key stays inside the editor: grid shortcuts such as Delete or
    // Insert must not act while the user is typing.
    event.stopPropagation();
    if (handled) event.preventDefault();
  }

  /** Leaving the field with the mouse saves (unless a key already did). */
  function onBlur(): void {
    if (!doneRef.current) save("none");
  }

  const fieldClass =
    "h-full w-full rounded-none border-2 border-accent bg-surface px-1 text-sm text-fg outline-none";

  return (
    <div className="relative h-full w-full">
      {kind === "list" ? (
        <select
          ref={(el) => {
            inputRef.current = el;
          }}
          aria-label={label}
          className={fieldClass}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          onBlur={onBlur}
        >
          {options.map(({ value, label }) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      ) : kind === "percent" ? (
        <>
          <input
            ref={(el) => {
              inputRef.current = el;
            }}
            type="text"
            inputMode="numeric"
            aria-label={label}
            aria-invalid={error !== null}
            className={fieldClass}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
            onBlur={onBlur}
          />
          <PercentList
            anchor={inputRef}
            current={text}
            choices={percentChoices(initialText)}
            onPick={(percent) => save("none", percent)}
          />
        </>
      ) : kind === "date" ? (
        <DateField
          inputRef={(el) => {
            inputRef.current = el;
          }}
          label={label}
          invalid={error !== null}
          className={fieldClass}
          boxClassName="h-full w-full"
          value={text}
          onChange={setText}
          // Picked from the calendar: done.
          onPick={(day) => save("none", day)}
          onKeyDown={onKeyDown}
          onBlur={onBlur}
        />
      ) : (
        <input
          ref={(el) => {
            inputRef.current = el;
          }}
          type="text"
          aria-label={label}
          aria-invalid={error !== null}
          className={fieldClass}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          onBlur={onBlur}
        />
      )}
      {error !== null && (
        <div
          role="alert"
          className="absolute top-full left-0 z-20 mt-0.5 w-max max-w-[22rem] rounded border border-danger-line bg-danger-bg px-2 py-1 text-xs whitespace-normal text-danger-fg shadow"
        >
          {error}
        </div>
      )}
    </div>
  );
}

/** Props of {@link PercentList}. */
interface PercentListProps {
  /** The field the list hangs under. */
  anchor: RefObject<HTMLInputElement | HTMLSelectElement | null>;
  /** What the field shows now. */
  current: string;
  /** The percentages to offer. */
  choices: readonly number[];
  /** Called with the picked percentage. */
  onPick: (percent: string) => void;
}

/**
 * The percentages under the % complete field. The focus stays in the field,
 * so another number can be typed instead.
 *
 * @param props - See {@link PercentListProps}.
 * @returns A popover (the browser draws it above everything).
 */
function PercentList({ anchor, current, choices, onPick }: PercentListProps) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    const box = anchor.current?.getBoundingClientRect();
    if (el === null || box === undefined) return;
    (el as Partial<Pick<HTMLElement, "showPopover">>).showPopover?.();
    const height = el.offsetHeight;
    const top = box.bottom + height <= window.innerHeight ? box.bottom : box.top - height;
    el.style.top = `${Math.max(0, top)}px`;
    el.style.left = `${box.left}px`;
    el.style.minWidth = `${box.width}px`;
  }, [anchor]);
  return (
    <div
      ref={ref}
      popover="manual"
      role="listbox"
      aria-label={t("grid.column.complete")}
      className="fixed z-50 m-0 rounded border border-line bg-surface py-1 text-sm text-fg shadow-xl"
      style={{ inset: "auto" }}
      // The focus stays in the field; the row below feels nothing.
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={(e) => e.stopPropagation()}
    >
      {choices.map((percent) => (
        <div
          key={percent}
          role="option"
          aria-selected={String(percent) === current}
          className={`cursor-pointer px-3 py-0.5 text-right hover:bg-app ${
            String(percent) === current ? "bg-app font-semibold" : ""
          }`}
          onClick={() => onPick(String(percent))}
        >
          {t("grid.percent", { percent })}
        </div>
      ))}
    </div>
  );
}

/** Props of {@link NameCell}. */
export interface NameCellProps {
  /** The row. */
  row: GridRow;
  /** Called when the collapse/expand button is clicked. */
  onToggle: () => void;
}

/**
 * The Name cell: indented by outline level, bold for summaries, with a
 * small button to show or hide the subtasks.
 *
 * @param props - See {@link NameCellProps}.
 * @returns The cell content.
 */
export function NameCell({ row, onToggle }: NameCellProps) {
  const { t } = useTranslation();
  return (
    <span
      className="flex h-full min-w-0 items-center gap-1"
      style={{ paddingLeft: (row.level - 1) * INDENT_PX }}
    >
      {row.isSummary ? (
        <button
          type="button"
          // Not in the Tab order: the keyboard uses Alt+Shift+− / + instead.
          tabIndex={-1}
          aria-label={t(row.collapsed ? "grid.expand" : "grid.collapse", { name: row.name })}
          className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-fg-muted hover:bg-app"
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
          // Two quick clicks are not a double-click on the row.
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <Chevron open={!row.collapsed} />
        </button>
      ) : (
        <span className="w-4 shrink-0" />
      )}
      <span
        className={`truncate ${row.isSummary && row.task?.isProject !== true ? "font-semibold" : ""}`}
      >
        {row.name}
      </span>
    </span>
  );
}

/** The line between a project's own issues and those of its tasks. */
const GROUP_LINE = "────────────────";
/** The texts of a progress problem. */
const PROGRESS_KEYS = {
  behind: "grid.indicator.behind",
  overdue: "grid.indicator.overdue",
  milestone: "grid.indicator.milestoneMissed",
} as const;
/** One issue in a project's list. */
const bullet = (text: string) => `• ${text}`;
/** No calendars known. */
const NO_CALENDARS: readonly Calendar[] = [];

/** Props of {@link IndicatorsCell}. */
export interface IndicatorsCellProps {
  /** The row. */
  row: GridRow;
  /** All rows, for the risks and blockers of a project's tasks. */
  rows?: readonly TaskRow[];
  /** All calendars, to compare progress with the dates. */
  calendars?: readonly Calendar[];
  /** The project calendar, for tasks without their own. */
  projectCalendarId?: string | null;
  /** Today at 00:00 (ms), for "late to start". Default: the user's clock. */
  today?: number;
  /** Locale for dates in the tooltips. */
  dateLocale: string;
  /** Locale for numbers in the tooltips (blocked days). Default: English. */
  numberLocale?: string;
  /** Working minutes in one day, to show blocked time in days. Default 480. */
  minutesPerDay?: number;
}

/**
 * The Indicators cell. Every row shows what it is (project, task or
 * sub-task); tasks also whether they are critical. When the row has them: a triangle
 * for its risks (the shape shows the highest priority), a shield with a tick
 * for approved risks, a shield with a plus for mitigations, a pin for a date constraint, a clock for a
 * missed deadline, a note sign for notes (tooltip: the note itself), a striped "blocked" sign for a
 * task with blockers (tooltip: total blocked days and the
 * reasons). Each symbol has a translated tooltip and accessible name.
 *
 * @param props - See {@link IndicatorsCellProps}.
 * @returns The symbols (possibly none).
 */
export function IndicatorsCell({
  row,
  rows = [],
  calendars = NO_CALENDARS,
  projectCalendarId = null,
  today = localToday(),
  dateLocale,
  numberLocale = "en",
  minutesPerDay = 480,
}: IndicatorsCellProps) {
  const { t } = useTranslation();
  const task = row.task;
  if (task === null) return null;
  // Every row: what it is.
  const kind =
    task.isProject === true
      ? "project"
      : task.isMilestone && !task.isSummary
        ? "milestone"
        : task.outlineLevel > 2
          ? "subtask"
          : "task";
  const progress = progressProblem(task, calendars, projectCalendarId, today);
  const kindIcon = progress?.kind === "milestone" ? "milestoneMissed" : kind;
  const items: { key: string; text: string; icon: IconName }[] = [
    { key: "kind", text: t(`grid.indicator.${kind}`), icon: kindIcon },
  ];
  // A project has no criticality of its own.
  if (task.isProject !== true) {
    const slack = task.totalSlackMinutes;
    items.push({
      key: "critical",
      text: task.isCritical
        ? t("grid.indicator.critical")
        : slack === null
          ? t("grid.indicator.notCritical")
          : t("grid.indicator.notCriticalSpare", {
              days: formatNumber(slack / minutesPerDay, numberLocale),
              count: slack / minutesPerDay,
            }),
      icon: task.isCritical ? "critical" : "notCritical",
    });
  }
  // The rest only when there is something to show. A project shows its own
  // issues first and, below a line, those of its tasks with their names.
  const isProject = task.isProject === true;
  const grouped = (own: readonly string[], ofTasks: readonly string[]) =>
    !isProject
      ? own.join("\n")
      : [
          ...(own.length > 0 ? [t("grid.indicator.groupProject"), ...own.map(bullet)] : []),
          ...(own.length > 0 && ofTasks.length > 0 ? [GROUP_LINE] : []),
          ...(ofTasks.length > 0 ? [t("grid.indicator.groupTasks"), ...ofTasks.map(bullet)] : []),
        ].join("\n");
  const named = (name: string, text: string) => t("grid.indicator.ofTask", { task: name, text });
  const riskText = (risk: Risk) =>
    t(
      risk.noConsequence
        ? "grid.indicator.riskNoConsequence"
        : risk.consequence === ""
          ? "grid.indicator.riskUnfinished"
          : "grid.indicator.risk",
      {
        risk: risk.description,
        priority: t(`risk.priority.${risk.priority}`),
        consequence: risk.consequence,
      },
    );
  const missedText = (priority: RiskPriority) =>
    t("grid.indicator.riskMissedStart", { priority: t(`risk.priority.${priority}`) });

  const own = task.risks ?? [];
  const ofTasks = isProject ? taskRisks(rows, task.id) : [];
  /** One kind of tooltip for the risks that pass a test. */
  const riskLines = (wanted: (risk: Risk) => boolean, text: (risk: Risk) => string) =>
    grouped(
      own.filter(wanted).map(text),
      ofTasks.filter((e) => wanted(e.risk)).map((e) => named(e.taskName, text(e.risk))),
    );
  // Missed starts are risks that are worked out, not written by the user.
  const missed = missedStartPriority(task, today);
  const missedOfTasks = isProject ? missedStarts(rows, task.id, today) : [];
  const highest = [missed, ...missedOfTasks.map((m) => m.priority)].reduce(
    higherPriority,
    highestPriority([...own, ...ofTasks.map((e) => e.risk)]),
  );
  if (highest !== null) {
    items.push({
      key: "risk",
      text: grouped(
        [...own.map(riskText), ...(missed === null ? [] : [missedText(missed)])],
        [
          ...ofTasks.map((e) => named(e.taskName, riskText(e.risk))),
          ...missedOfTasks.map((m) => named(m.task.name, missedText(m.priority))),
        ],
      ),
      icon: `risk-${highest}`,
    });
  }
  const approved = riskLines(
    (risk) => risk.response === "approved",
    (risk) => t("grid.indicator.riskApproved", { risk: risk.description }),
  );
  if (approved !== "") items.push({ key: "approved", text: approved, icon: "riskApproved" });
  const mitigated = riskLines(
    (risk) => risk.response === "mitigate",
    (risk) =>
      risk.mitigation === ""
        ? t("grid.indicator.riskNoMitigation", { risk: risk.description })
        : t("grid.indicator.riskMitigation", {
            risk: risk.description,
            mitigation: risk.mitigation,
          }),
  );
  if (mitigated !== "") items.push({ key: "mitigation", text: mitigated, icon: "riskMitigation" });
  const constraint = task.constraint;
  if (constraint.type !== "asSoonAsPossible") {
    const type = t(CONSTRAINT_KEYS[constraint.type]);
    const date = "date" in constraint ? formatScheduleDate(constraint.date, dateLocale) : null;
    items.push({
      key: "constraint",
      text: date === null ? type : t("grid.indicator.constraintWithDate", { type, date }),
      icon: "pin",
    });
  }
  if (missed !== null) {
    const date = formatScheduleDate(task.start, dateLocale) ?? "";
    items.push({
      key: "lateStart",
      text: t("grid.indicator.lateStart", { date }),
      icon: "lateStart",
    });
  }
  if (progress !== null) {
    const date = formatScheduleDate(task.finish, dateLocale) ?? "";
    items.push({
      key: "progress",
      text: t(PROGRESS_KEYS[progress.kind], {
        actual: progress.actual,
        expected: progress.expected,
        date,
      }),
      icon: "attention",
    });
  }
  if (task.missedDeadline) {
    const date = formatScheduleDate(task.deadline, dateLocale) ?? "";
    items.push({
      key: "deadline",
      text: t("grid.indicator.missedDeadline", { date }),
      icon: "warning",
    });
  }
  if (task.notes.trim() !== "") {
    items.push({
      key: "notes",
      text: t("grid.indicator.notes", { text: task.notes.trim() }),
      icon: "note",
    });
  }
  const blockedText = (minutes: number, reasons: string) =>
    t("grid.indicator.blocked", {
      days: formatNumber(minutes / minutesPerDay, numberLocale),
      count: minutes / minutesPerDay,
      reasons,
    });
  const blockers = task.blockers ?? [];
  // The total from the schedule; if it is missing, the sum of the lengths.
  const minutes = task.blockedMinutes ?? blockers.reduce((n, b) => n + b.durationMinutes, 0);
  const blockedLines = grouped(
    blockers.length === 0 ? [] : [blockedText(minutes, blockers.map((b) => b.reason).join("; "))],
    (isProject ? projectBlockers(rows, task.id) : []).map(({ taskName, blocker }) =>
      named(taskName, blockedText(blocker.durationMinutes, blocker.reason)),
    ),
  );
  if (blockedLines !== "") items.push({ key: "blocked", text: blockedLines, icon: "blocked" });
  return (
    <span className="flex h-full items-center gap-0.5">
      {items.map((item) => (
        <span key={item.key} role="img" aria-label={item.text} title={item.text}>
          <Icon name={item.icon} />
        </span>
      ))}
    </span>
  );
}

/**
 * A small triangle: pointing down (open) or right (closed).
 *
 * @param props - `open`: which way it points.
 * @returns An SVG drawing, hidden from screen readers (the button has the name).
 */
function Chevron({ open }: { open: boolean }) {
  return (
    <svg aria-hidden="true" width="10" height="10" viewBox="0 0 10 10" className="fill-current">
      {open ? <path d="M1 3h8L5 8z" /> : <path d="M3 1v8l5-4z" />}
    </svg>
  );
}

/** The names of the indicator symbols. */
type IconName =
  | "project"
  | "task"
  | "subtask"
  | "milestone"
  | "milestoneMissed"
  | "critical"
  | "notCritical"
  | "risk-low"
  | "risk-medium"
  | "risk-high"
  | "riskApproved"
  | "riskMitigation"
  | "pin"
  | "warning"
  | "lateStart"
  | "attention"
  | "note"
  | "blocked";

/**
 * The indicator symbols, drawn as tiny SVG pictures (no font needed, sharp
 * at any zoom, coloured by the theme through `currentColor` or the theme's
 * CSS variables).
 *
 * @param props - `name`: which symbol.
 * @returns An SVG drawing (decorative; the wrapper has the accessible name).
 */
function Icon({ name }: { name: IconName }) {
  const common = { "aria-hidden": true, width: 14, height: 14, viewBox: "0 0 12 12" } as const;
  // The kinds in the colours of their bars in the chart.
  if (name === "project") {
    return (
      <svg {...common} fill="var(--gantt-bar-project)">
        {/* The letter P on a filled square. "evenodd" cuts its hole out. */}
        <rect x="0.5" y="0.5" width="11" height="11" rx="2" />
        <path
          d="M3.7 2.2h3a2.2 2.2 0 0 1 0 4.6H5.4v3H3.7zM5.4 3.6v1.8h1.2a.9.9 0 0 0 0-1.8z"
          fill="var(--app-surface)"
          fillRule="evenodd"
        />
      </svg>
    );
  }
  if (name === "milestone" || name === "milestoneMissed") {
    const color = name === "milestone" ? "var(--gantt-bar)" : "var(--gantt-late-start)";
    return (
      <svg {...common} fill={color}>
        <path d="M6 0.8L11.2 6 6 11.2 0.8 6z" />
      </svg>
    );
  }
  if (name === "task" || name === "subtask") {
    const color = name === "task" ? "var(--gantt-bar)" : "var(--gantt-bar-subtask)";
    return name === "task" ? (
      <svg {...common} fill={color}>
        <rect x="0.5" y="3" width="11" height="6" rx="1" />
      </svg>
    ) : (
      <svg {...common} fill="none" stroke={color} strokeWidth="1.5">
        <path d="M1.5 1v6h3" />
        <rect x="5" y="4.5" width="6" height="5" rx="1" fill={color} />
      </svg>
    );
  }
  if (name === "critical") {
    // A filled flame-like bolt; not red, because red means blocked.
    return (
      <svg {...common} fill="var(--gantt-bar-critical)">
        <path d="M7 0 2 7h3.2L4.5 12 10 4.8H6.6z" />
      </svg>
    );
  }
  if (name === "notCritical") {
    return (
      <svg {...common} className="fill-none stroke-current text-fg-muted opacity-60">
        <path d="M7 0.8 2.6 6.6h3L5 11 9.4 5.2H6.4z" />
      </svg>
    );
  }
  if (name === "risk-low") {
    // Outline only; medium is filled, high is a darker octagon: the shapes
    // differ, so the priority does not depend on colour alone.
    return (
      <svg {...common} fill="none" stroke="var(--app-risk-low)">
        <path d="M6 1.2 11.2 11H0.8z" />
        <path d="M6 4.6v3M6 8.8v1" strokeWidth="1.2" />
      </svg>
    );
  }
  if (name === "risk-medium") {
    return (
      <svg {...common} fill="var(--app-risk)">
        <path d="M6 0.5 11.8 11.5H0.2z" />
        <path d="M6 4.2v3.6M6 9.2v1" stroke="var(--app-surface)" strokeWidth="1.2" />
      </svg>
    );
  }
  if (name === "risk-high") {
    return (
      <svg {...common} fill="var(--app-risk-high)">
        <path d="M3.5 0h5L12 3.5v5L8.5 12h-5L0 8.5v-5z" />
        <path
          d="M4.2 2.8v3.8M4.2 8.2v1.2M7.8 2.8v3.8M7.8 8.2v1.2"
          stroke="var(--app-surface)"
          strokeWidth="1.4"
        />
      </svg>
    );
  }
  if (name === "riskApproved" || name === "riskMitigation") {
    // A shield: with a tick (accepted) or a plus (something is done).
    const color = name === "riskApproved" ? "var(--app-risk-approved)" : "var(--app-risk)";
    return (
      <svg {...common} fill="none" stroke={color} strokeWidth="1.2">
        <path d="M6 0.7 10.8 2.3V6c0 2.6-2 4.5-4.8 5.4C3.2 10.5 1.2 8.6 1.2 6V2.3z" />
        <path d={name === "riskApproved" ? "M3.6 5.8 5.4 7.6 8.5 4" : "M6 3.6v4.4M3.8 5.8h4.4"} />
      </svg>
    );
  }
  if (name === "lateStart") {
    // The arrow of the chart: the start has passed.
    return (
      <svg {...common} fill="var(--gantt-late-start)">
        <path d="M11.8 6 6.4 1.4v2.4H0.2v4.4h6.2v2.4z" />
      </svg>
    );
  }
  if (name === "attention") {
    // An exclamation mark: the progress needs attention.
    return (
      <svg {...common} fill="var(--gantt-deadline-missed)">
        <circle cx="6" cy="6" r="5.8" />
        <path d="M6 2.6v4M6 8v1.5" stroke="var(--app-surface)" strokeWidth="1.6" />
      </svg>
    );
  }
  if (name === "pin") {
    return (
      <svg {...common} className="fill-current text-accent">
        <circle cx="6" cy="4" r="3" />
        <rect x="5.4" y="6" width="1.2" height="6" />
      </svg>
    );
  }
  if (name === "blocked") {
    // A small striped block, like the blocked pieces in the Gantt chart,
    // in the same theme colours (CSS variables from styles/themes.css).
    return (
      <svg {...common}>
        <rect
          x="0.5"
          y="2.5"
          width="11"
          height="7"
          fill="var(--gantt-bar-blocked)"
          stroke="var(--gantt-bar-blocked-line)"
        />
        <path d="M1 9 6 3M5 9l5-6" stroke="var(--gantt-bar-blocked-line)" />
      </svg>
    );
  }
  if (name === "warning") {
    return (
      // A clock (a triangle would look like the risk sign).
      <svg {...common} className="fill-none stroke-current text-danger-fg" strokeWidth="1.5">
        <circle cx="6" cy="6" r="5" />
        <path d="M6 3v3.2l2.2 1.3" />
      </svg>
    );
  }
  return (
    <svg {...common} className="fill-none stroke-current text-fg-muted">
      <rect x="1.5" y="1.5" width="9" height="9" rx="1" />
      <path d="M3.5 4.5h5M3.5 6.5h5M3.5 8.5h3" />
    </svg>
  );
}
