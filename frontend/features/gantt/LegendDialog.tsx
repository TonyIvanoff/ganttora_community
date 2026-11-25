/**
 * features/gantt/LegendDialog.tsx: the colour legend of the Gantt chart.
 *
 * What it is for:
 *   Explains what the colours and shapes mean: task, sub-task, blocker,
 *   risks, manual task, milestone, deadline.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx`; shown and hidden
 *   with the Legend toolbar or View → Colour legend, beside the work. The
 *   samples use the same theme colours (`--gantt-*` in `styles/themes.css`)
 *   as the chart, so they always match, in light and dark.
 */

import { useRef, useState, type CSSProperties, type PointerEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDialogStore } from "@frontend/shell/dialogStore";

/** A sample bar. */
function Bar({ style }: { style: CSSProperties }) {
  return <span className="block h-3.5 w-14 rounded-sm" style={style} />;
}

/** A sample bar with the thick border of a risk. */
function RiskBar({ color }: { color: string }) {
  return <Bar style={{ border: "2px solid var(--gantt-bar)", outline: `4px solid ${color}` }} />;
}

/** A sample marker: a small diamond (milestone) or arrow (deadline). */
function Mark({ path, color }: { path: string; color: string }) {
  return (
    <svg width="20" height="20" viewBox="0 0 18 18">
      <path d={path} fill={color} />
    </svg>
  );
}

/** A diamond. */
const DIAMOND = "M9 2l7 7-7 7-7-7z";
/** An arrow pointing down, like the deadline marker in the chart. */
const ARROW = "M7 2h4v7h4l-6 7-6-7h4z";

/** An arrow pointing right, like the "late to start" marker in the chart. */
const RIGHT_ARROW = "M17 9l-8-6v3H1v6h8v3z";

/** A sample finish arrow: right = late, left = early; filled = finished. */
function FinishMark({ color, late, done }: { color: string; late?: boolean; done?: boolean }) {
  const path = late
    ? "M19 9l-7-5v3H1v4h11v3z" // points right
    : "M1 9l7-5v3h11v4H8v3z"; // points left
  return (
    <svg width="20" height="20" viewBox="0 0 20 18">
      <path d={path} fill={done ? color : "none"} stroke={color} />
    </svg>
  );
}

/** The legend's lines: a translation key and its sample. */
const ROWS: readonly { key: Parameters<typeof legendKey>[0]; sample: ReactNode }[] = [
  { key: "project", sample: <Bar style={{ border: "2px solid var(--gantt-bar-project)" }} /> },
  { key: "task", sample: <Bar style={{ border: "2px solid var(--gantt-bar)" }} /> },
  { key: "subtask", sample: <Bar style={{ border: "2px solid var(--gantt-bar-subtask)" }} /> },
  {
    key: "complete",
    sample: (
      <Bar
        style={{
          background: "linear-gradient(90deg, var(--gantt-bar) 50%, transparent 50%)",
          border: "2px solid var(--gantt-bar)",
        }}
      />
    ),
  },
  {
    key: "blocker",
    sample: (
      <Bar
        style={{
          background:
            "repeating-linear-gradient(135deg, var(--gantt-bar-blocked) 0 4px, var(--gantt-bar-blocked-line) 4px 5px)",
          border: "1px solid var(--gantt-bar-blocked-line)",
        }}
      />
    ),
  },
  { key: "riskLow", sample: <RiskBar color="var(--app-risk-low)" /> },
  { key: "riskMedium", sample: <RiskBar color="var(--app-risk)" /> },
  { key: "riskHigh", sample: <RiskBar color="var(--app-risk-high)" /> },
  { key: "manual", sample: <Bar style={{ border: "2px solid var(--gantt-bar-manual)" }} /> },
  { key: "milestone", sample: <Mark path={DIAMOND} color="var(--gantt-bar)" /> },
  { key: "milestoneMissed", sample: <Mark path={DIAMOND} color="var(--gantt-late-start)" /> },
  { key: "today", sample: <Bar style={{ background: "var(--gantt-today-header)" }} /> },
  { key: "deadline", sample: <Mark path={ARROW} color="var(--gantt-deadline)" /> },
  { key: "deadlineMissed", sample: <Mark path={ARROW} color="var(--gantt-deadline-missed)" /> },
  { key: "lateStart", sample: <Mark path={RIGHT_ARROW} color="var(--gantt-late-start)" /> },
  { key: "finishedLate", sample: <FinishMark color="var(--gantt-late-start)" late done /> },
  { key: "willFinishLate", sample: <FinishMark color="var(--gantt-late-start)" late /> },
  { key: "finishedEarly", sample: <FinishMark color="var(--gantt-deadline)" done /> },
  { key: "willFinishEarly", sample: <FinishMark color="var(--gantt-deadline)" /> },
];

/** The full translation key of a legend line. */
function legendKey(
  name:
    | "project"
    | "task"
    | "complete"
    | "subtask"
    | "blocker"
    | "riskLow"
    | "riskMedium"
    | "riskHigh"
    | "manual"
    | "milestone"
    | "milestoneMissed"
    | "today"
    | "deadline"
    | "deadlineMissed"
    | "lateStart"
    | "finishedLate"
    | "willFinishLate"
    | "finishedEarly"
    | "willFinishEarly",
) {
  return `legend.${name}` as const;
}

/** The panel's width, so it can be kept inside the window. */
const WIDTH = "17rem";

/**
 * The legend panel: a small window inside the app that stays open while the
 * user works. Its title bar drags it; × or Escape closes it.
 *
 * @returns The panel, or nothing while closed.
 */
export function LegendPanel() {
  const { t } = useTranslation();
  const open = useDialogStore((s) => s.legendOpen);
  const toggle = useDialogStore((s) => s.toggleLegend);
  /** Where it was dragged to; `null` = the bottom right corner. */
  const [corner, setCorner] = useState<{ x: number; y: number } | null>(null);
  /** Pointer minus the panel's corner, while it is dragged. */
  const grab = useRef<{ x: number; y: number } | null>(null);
  if (!open) return null;

  function onPointerDown(event: PointerEvent<HTMLDivElement>): void {
    if (event.button !== 0 || event.currentTarget.parentElement === null) return;
    const r = event.currentTarget.parentElement.getBoundingClientRect();
    // `?.()`: the method is missing in the tests' imitation browser.
    event.currentTarget.setPointerCapture?.(event.pointerId);
    grab.current = { x: event.clientX - r.left, y: event.clientY - r.top };
  }
  function onPointerMove(event: PointerEvent<HTMLDivElement>): void {
    if (grab.current === null) return;
    setCorner({ x: event.clientX - grab.current.x, y: event.clientY - grab.current.y });
  }
  const drop = () => (grab.current = null);

  return (
    <section
      aria-label={t("legend.title")}
      className="fixed z-20 flex max-h-[calc(100vh-4rem)] flex-col rounded border border-line bg-surface text-sm shadow-lg"
      style={{
        width: WIDTH,
        // `clamp` keeps the title bar in the window, also after resizing it.
        ...(corner === null
          ? { right: "2rem", bottom: "4rem" }
          : {
              left: `clamp(0px, ${corner.x}px, calc(100vw - ${WIDTH}))`,
              top: `clamp(0px, ${corner.y}px, calc(100vh - 3rem))`,
            }),
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") toggle();
      }}
    >
      <div
        data-legend-title=""
        className="flex cursor-move touch-none items-center border-b border-line bg-app px-2 select-none"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={drop}
        onPointerCancel={drop}
        onLostPointerCapture={drop}
      >
        <h2 className="py-1 font-semibold">{t("legend.title")}</h2>
        <button
          type="button"
          aria-label={t("legend.close")}
          title={t("legend.close")}
          className="ml-auto rounded px-1.5 hover:bg-surface"
          // Pressing × must not start a drag of the title bar around it.
          onPointerDown={(e) => e.stopPropagation()}
          onClick={toggle}
        >
          <span aria-hidden="true">×</span>
        </button>
      </div>
      <ul className="flex flex-col gap-2 overflow-y-auto p-3">
        {ROWS.map((row) => (
          <li key={row.key} className="flex items-center gap-3">
            <span aria-hidden="true" className="flex w-16 shrink-0 justify-center">
              {row.sample}
            </span>
            <span>{t(legendKey(row.key))}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
