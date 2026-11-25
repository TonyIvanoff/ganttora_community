/**
 * renderer/theme.ts: reads the Gantt colours from the CSS theme.
 *
 * What it is for:
 *   A canvas is a bitmap: we paint it with JavaScript, so CSS classes do not
 *   apply to what is drawn on it. To keep light and dark themes in one place
 *   (`frontend/styles/themes.css`), the renderer reads the CSS variables
 *   (`--gantt-bar`, …) and paints with those colours.
 *
 * Where it fits:
 *   `controller.ts` calls `readGanttTheme` at start and again whenever the
 *   theme may have changed (operating system switched to dark mode, or the
 *   `data-theme` attribute changed).
 */

/** All colours and the font the renderer paints with. */
export interface GanttTheme {
  /** Chart background. */
  background: string;
  /** Grid lines: vertical (days or weeks) and the thin line between rows. */
  gridLine: string;
  /** Shading of non-working days (weekends). */
  nonWorking: string;
  /** Header background. */
  headerBackground: string;
  /** Header text. */
  headerText: string;
  /** Lines between header cells. */
  headerLine: string;
  /** Alpha of the projects' background colours, 0–1. */
  projectTint: string;
  /** Line between two projects. */
  projectSeparator: string;
  /** Bar of a project row. */
  barProject: string;
  /** Outline and finished part of a task. */
  bar: string;
  /** Bar of a sub-task (cyan). */
  barSubtask: string;
  /** Border around a bar with a low, medium or high risk. */
  riskLow: string;
  /** See {@link GanttTheme.riskLow}. */
  riskMedium: string;
  /** See {@link GanttTheme.riskLow}. */
  riskHigh: string;
  /** Manually scheduled task bar. */
  barManual: string;
  /** Fill of a blocked piece of a bar. */
  barBlocked: string;
  /** Stripes and outline of a blocked piece (so it differs without colour vision). */
  barBlockedLine: string;
  /** Deadline marker (met). */
  deadline: string;
  /** Deadline marker when the task finishes after its deadline. */
  deadlineMissed: string;
  /** Arrow of a task that is late to start. */
  lateStart: string;
  /** Constraint pin. */
  constraint: string;
  /** Drag preview: ghost bar outline and rubber-band link line. */
  dragPreview: string;
  /** Normal dependency link. */
  link: string;
  /** Today's column, see-through. */
  today: string;
  /** Today's mark in the header. */
  todayHeader: string;
  /** Text on today's mark. */
  todayHeaderText: string;
  /** Background band of the selected row. */
  selection: string;
  /** Outline around the selected bar. */
  selectionOutline: string;
  /** CSS font family for all canvas text (same as the rest of the UI). */
  fontFamily: string;
}

/**
 * Colours used when a CSS variable is missing (e.g. in a test environment
 * without stylesheets). They match the light theme.
 */
const FALLBACK: GanttTheme = {
  background: "#ffffff",
  gridLine: "#e3e6ea",
  nonWorking: "#f3f4f6",
  headerBackground: "#f6f7f9",
  headerText: "#3b4350",
  headerLine: "#d9dde3",
  projectTint: "0.1",
  projectSeparator: "#64748b",
  barProject: "#334155",
  bar: "#2563eb",
  barSubtask: "#06b6d4",
  riskLow: "#fbbf24",
  riskMedium: "#ea580c",
  riskHigh: "#9a3412",
  barManual: "rgba(21, 128, 61, 0.8)",
  barBlocked: "#ef4444",
  barBlockedLine: "#7f1d1d",
  deadline: "#15803d",
  deadlineMissed: "#dc2626",
  lateStart: "#f97316",
  constraint: "#7c3aed",
  dragPreview: "#1d4ed8",
  link: "#6b7280",
  today: "rgba(245, 158, 11, 0.25)",
  todayHeader: "#f59e0b",
  todayHeaderText: "#111827",
  selection: "#dbeafe",
  selectionOutline: "#1d4ed8",
  fontFamily: "system-ui, sans-serif",
};

/**
 * Reads the current Gantt theme from CSS.
 *
 * @param element - Any element inside the app; its computed style is read
 *   (variables are inherited from `<html>`).
 * @returns The colours and font. Missing variables use the light defaults.
 *
 * @example
 * const theme = readGanttTheme(canvas);
 * ctx.fillStyle = theme.bar;
 *
 * Beginner note: `getComputedStyle` asks the browser for the *final* style
 * of an element, after all stylesheets and media queries (like dark mode)
 * have been applied.
 */
export function readGanttTheme(element: Element): GanttTheme {
  const style = getComputedStyle(element);
  /** Reads one variable, or returns the fallback if it is empty. */
  const read = (name: string, fallback: string): string => {
    const value = style.getPropertyValue(name).trim();
    return value === "" ? fallback : value;
  };
  return {
    background: read("--gantt-bg", FALLBACK.background),
    gridLine: read("--gantt-grid-line", FALLBACK.gridLine),
    nonWorking: read("--gantt-nonworking", FALLBACK.nonWorking),
    headerBackground: read("--gantt-header-bg", FALLBACK.headerBackground),
    headerText: read("--gantt-header-fg", FALLBACK.headerText),
    headerLine: read("--gantt-header-line", FALLBACK.headerLine),
    projectTint: read("--project-tint", FALLBACK.projectTint),
    projectSeparator: read("--project-separator", FALLBACK.projectSeparator),
    barProject: read("--gantt-bar-project", FALLBACK.barProject),
    bar: read("--gantt-bar", FALLBACK.bar),
    barSubtask: read("--gantt-bar-subtask", FALLBACK.barSubtask),
    riskLow: read("--app-risk-low", FALLBACK.riskLow),
    riskMedium: read("--app-risk", FALLBACK.riskMedium),
    riskHigh: read("--app-risk-high", FALLBACK.riskHigh),
    barManual: read("--gantt-bar-manual", FALLBACK.barManual),
    barBlocked: read("--gantt-bar-blocked", FALLBACK.barBlocked),
    barBlockedLine: read("--gantt-bar-blocked-line", FALLBACK.barBlockedLine),
    deadline: read("--gantt-deadline", FALLBACK.deadline),
    deadlineMissed: read("--gantt-deadline-missed", FALLBACK.deadlineMissed),
    lateStart: read("--gantt-late-start", FALLBACK.lateStart),
    constraint: read("--gantt-constraint", FALLBACK.constraint),
    dragPreview: read("--gantt-drag-preview", FALLBACK.dragPreview),
    link: read("--gantt-link", FALLBACK.link),
    today: read("--gantt-today", FALLBACK.today),
    todayHeader: read("--gantt-today-header", FALLBACK.todayHeader),
    todayHeaderText: read("--gantt-today-header-fg", FALLBACK.todayHeaderText),
    selection: read("--gantt-selection", FALLBACK.selection),
    selectionOutline: read("--gantt-selection-outline", FALLBACK.selectionOutline),
    fontFamily: style.fontFamily || FALLBACK.fontFamily,
  };
}
