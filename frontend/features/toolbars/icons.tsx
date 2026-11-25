/**
 * features/toolbars/icons.tsx: the small drawings on toolbar buttons.
 *
 * What it is for:
 *   One simple 16×16 line drawing per action. They are our own SVG paths, so
 *   no icon package is needed and they follow the text colour in the light
 *   and the dark theme (`currentColor`).
 *
 * Where it fits:
 *   Used by `Toolbar.tsx` and `CustomizeDialog.tsx`.
 *
 * Beginner note: an SVG *path* is a list of pen movements: `M` move to,
 * `L`/`H`/`V` draw a line, `a` draw an arc, `z` back to the start. Small
 * letters are relative to the current point.
 */

import type { ActionId } from "./actions";

/** The path of every action's icon (an action without one fails to compile). */
const PATHS: Record<ActionId, string> = {
  fileNew: "M4 1.5h5l3 3v10H4z M9 1.5v3h3",
  fileOpen: "M1.5 13V3h4l1.5 2h6v2 M1.5 13l2-6h11l-2 6z",
  fileOpenSample: "M2 14V7l6-4.5L14 7v7z M6.5 14v-4h3v4",
  fileSave: "M2.5 2.5h9l2 2v9h-11z M5 2.5v4h5v-4 M4.5 13.5v-4h7v4",
  fileSaveAs: "M2.5 2.5h9l2 2v3 M8 13.5H2.5v-11 M5 2.5v4h5v-4 M11.5 9v5 M9 11.5h5",
  openSettings: "M2 4h7 M12 4h2 M2 8h2 M7 8h7 M2 12h6 M11 12h3 M9 2.5v3 M4.5 6.5v3 M8.5 10.5v3",
  undo: "M6 3L2.5 6.5 6 10 M2.5 6.5H10a3.5 3.5 0 0 1 0 7H7",
  redo: "M10 3l3.5 3.5L10 10 M13.5 6.5H6a3.5 3.5 0 0 0 0 7h3",
  cut: "M4 2l6 8.5 M12 2L6 10.5 M3 12.5a2 2 0 1 0 4 0a2 2 0 1 0-4 0 M9 12.5a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
  copy: "M5.5 5.5h8v9h-8z M10.5 5.5v-4h-8v9h3",
  paste: "M3 3h2 M11 3h2v11.5H3V3 M5.5 1.5h5v3h-5z M5.5 8h5 M5.5 11h5",
  deleteTasks: "M2.5 4h11 M6 4V2h4v2 M4 4l.7 10h6.6L12 4 M6.7 6.5v5 M9.3 6.5v5",
  addTask: "M1.5 4h13v4h-13z M8 10v5 M5.5 12.5h5",
  addSubtask: "M1.5 2h9v3h-9z M3.5 5v5.5H6 M6 9h8v3H6z M12 13.5v2 M11 14.5h2",
  addMilestone: "M6.5 1.5l5 5-5 5-5-5z M12.5 10v5 M10 12.5h5",
  indent: "M2 3h12 M8 6.5h6 M8 9.5h6 M2 13h12 M2 6l3 2-3 2z",
  outdent: "M2 3h12 M8 6.5h6 M8 9.5h6 M2 13h12 M5 6L2 8l3 2z",
  moveUp: "M8 14V3 M3.5 7L8 2.5 12.5 7",
  moveDown: "M8 2v11 M3.5 9L8 13.5 12.5 9",
  addProject: "M3 14V2.5h4a3 3 0 0 1 0 6H3 M12 9.5v5 M9.5 12h5",
  linkTasks: "M1.5 3h5v3h-5z M9.5 10h5v3h-5z M4 6v5.5h4 M6.5 9.5l2 2-2 2",
  addBlocker: "M8 1.5a6.5 6.5 0 1 0 .01 0z M3.5 3.5l9 9",
  addRisk: "M8 2 14.5 13.5h-13z M8 6.5v3.5 M8 11.2v.8",
  markOnTrack: "M1.5 4.5h13v7h-13z M4.5 8l2.5 2.3 4.5-4.6",
  setPlan: "M3.5 1.5v13 M3.5 2.5h9l-2.5 3 2.5 3h-9",
  taskInfo: "M8 1.5a6.5 6.5 0 1 0 .01 0z M8 7v4.5 M8 4.2v.6",
  zoomDays: "M2 3h12v11H2z M2 6h12 M5 1.5v3 M11 1.5v3 M7 8.5h2v3H7z",
  zoomWeeks: "M2 3h12v11H2z M2 6h12 M5 1.5v3 M11 1.5v3 M4 10h8",
  zoomMonths: "M2 3h12v11H2z M2 6h12 M5 1.5v3 M11 1.5v3 M4.5 8.5h7 M4.5 11.5h7",
  zoomIn: "M7 2a5 5 0 1 0 .01 0z M10.5 10.5l4 4 M7 4.7v4.6 M4.7 7h4.6",
  zoomOut: "M7 2a5 5 0 1 0 .01 0z M10.5 10.5l4 4 M4.7 7h4.6",
  zoomReset: "M7 2a5 5 0 1 0 .01 0z M10.5 10.5l4 4",
  goToToday: "M2 3h12v11H2z M2 6.5h12 M5 1.5v3 M11 1.5v3 M8 8v4 M6 10.5l2 2 2-2",
  goToStart: "M3 2.5v11 M13 8H6 M9 4.5L5.5 8 9 11.5",
  expandAll: "M2 2h12v12H2z M8 5v6 M5 8h6",
  collapseAll: "M2 2h12v12H2z M5 8h6",
  explain: "M8 1.5a6.5 6.5 0 1 0 .01 0z M6 6.2a2 2 0 1 1 2.8 1.9c-.6.3-.8.7-.8 1.4 M8 11.4v.6",
  legend: "M2 2.5h3v3H2z M2 10.5h3v3H2z M7.5 4H14 M7.5 12H14 M2 8h12",
  customizeToolbars: "M1.5 3h13v4h-13z M4 3v4 M9.5 14.5l.5-2L14 8.5l1.5 1.5-4 4z",
  resetToolbars: "M3 8a5 5 0 1 0 1.5-3.5 M4.5 1.5v3h3",
  calendars: "M2 3h12v11H2z M2 6.5h12 M5 1.5v3 M11 1.5v3 M5 9h1.5 M9.5 9H11 M5 11.5h1.5",
  about: "M8 1.5a6.5 6.5 0 1 0 .01 0z M8 7v4.5 M8 4.2v.6",
  demo: "M2 13.5h12 M2 3h5v2H2z M5 7h6v2H5z M9 11h5v1H9z",
};

/** Props of {@link ActionIcon}. */
interface ActionIconProps {
  /** Which action. */
  id: ActionId;
}

/**
 * An action's icon. Hidden from screen readers: the button's text names it.
 *
 * @param props - See {@link ActionIconProps}.
 * @returns The drawing.
 */
export function ActionIcon({ id }: ActionIconProps) {
  return (
    <svg
      aria-hidden="true"
      width="16"
      height="16"
      viewBox="0 0 16 16"
      className="shrink-0"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={PATHS[id]} />
    </svg>
  );
}
