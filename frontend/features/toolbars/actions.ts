/**
 * features/toolbars/actions.ts: the one list of everything the user can do.
 *
 * What it is for:
 *   "Save" appears in the menu bar, on a toolbar and as a shortcut. Its
 *   text, icon, shortcut and "is it possible right now?" rule are written
 *   here once, so all places always agree.
 *
 * Where it fits:
 *   Read by the toolbars (`Toolbar.tsx`, `CustomizeDialog.tsx`), the native
 *   menu (`menuModel.ts`) and the row context menu (`grid/TaskGrid.tsx`).
 *   `useActions.ts` adds the live state and carries an action out. This
 *   file is plain data and pure functions, so tests can check it directly.
 *
 * Main ideas:
 *   - An *action* is described by an {@link ActionDefinition}.
 *   - `enabled` and `pressed` are pure functions of an {@link ActionState},
 *     a small picture of the app ("is there a project? can we undo?").
 *   - Accelerators (shortcuts owned by the menu) are used on macOS only;
 *     see `menuModel.ts` for why.
 */

import type { FileActions } from "@frontend/features/files/useFileActions";
import { stepZoomPercent, type ZoomLevelId } from "@frontend/features/gantt/renderer/timeScale";
import type { TaskActions } from "@frontend/features/grid/useTaskActions";

/** What the rules below need to know about the app. */
export interface ActionState {
  /** `true` in the desktop app, `false` in the browser preview. */
  hasCore: boolean;
  /** The 10,000-task demo is shown (nothing can be edited). */
  isDemoData: boolean;
  /** A project is open. */
  hasProject: boolean;
  /** A task is selected. */
  hasTask: boolean;
  /** The project has at least one group (summary task). */
  hasGroups: boolean;
  /** Rust says Undo is possible. */
  canUndo: boolean;
  /** Rust says Redo is possible. */
  canRedo: boolean;
  /** Current timescale zoom. */
  zoom: ZoomLevelId;
  /** Current zoom in percent. */
  zoomPercent: number;
  /** Two or more tasks are selected. */
  hasSeveralTasks: boolean;
  /** The selected row is a project. */
  onProject: boolean;
  /** The selected tasks can become sub-tasks of the row above. */
  canBecomeSubtasks: boolean;
  /** The selected sub-tasks can move one level up. */
  canBecomeTasks: boolean;
  /** The selected tasks are linked one after another already. */
  selectionLinked: boolean;
  /** The "Why this date?" panel is open. */
  explainOpen: boolean;
  /** The colour legend panel is open. */
  legendOpen: boolean;
}

/** What `run` may use to do its work. */
export interface ActionContext {
  /** New, Open, Save… */
  files: FileActions;
  /** Indent, delete, paste… */
  tasks: TaskActions;
  /** Undo in Rust. */
  undo: () => Promise<void>;
  /** Redo in Rust. */
  redo: () => Promise<void>;
  /** Changes the timescale zoom. */
  setZoom: (zoom: ZoomLevelId) => void;
  /** Changes the zoom percentage. */
  setZoomPercent: (percent: number) => void;
  /** Reads the state at this very moment. */
  state: () => ActionState;
  /** Scrolls the chart to the project start. */
  goToStart: () => void;
  /** Shows today in the chart. */
  goToToday: () => void;
  /** Collapses (`true`) or expands (`false`) every group. */
  collapseAll: (collapsed: boolean) => void;
  /** Shows or hides "Why this date?". */
  toggleExplain: () => void;
  /** Shows or hides the colour legend. */
  toggleLegend: () => void;
  /** Shows or hides the 10,000-task demo. */
  toggleDemo: () => void;
  /** Opens a dialog. */
  openDialog: (dialog: "settings" | "calendars" | "about" | "customize") => void;
  /** Puts all toolbars back to how they were after installing. */
  resetToolbars: () => void;
}

/**
 * Where an action belongs. Decides how it behaves while a text field has
 * the focus: "clipboard" and "history" then belong to the field.
 */
export type ActionKind = "file" | "history" | "clipboard" | "task" | "view" | "app";

/** A translation key, or a function for a text that depends on the state. */
export type ActionKey = string | ((s: ActionState) => string);

/** Everything known about one action. */
export interface ActionDefinition {
  /** Full text, for menus. */
  labelKey: ActionKey;
  /** Short text under or beside a toolbar icon. */
  shortKey: ActionKey;
  /** Tooltip; may use `{shortcut}` and `{modifier}`. Default: the label. */
  titleKey?: ActionKey;
  /** Extra value for the texts (`{count}`). */
  count?: number | undefined;
  /** Extra values that change, e.g. the zoom (`{percent}`). */
  values?: (s: ActionState) => Record<string, unknown>;
  /** Translation keys of the shortcut text. */
  shortcut?: { mac: string; other: string };
  /** The same keys for screen readers (`aria-keyshortcuts`). */
  aria?: string;
  /** The same keys for the macOS menu (Tauri's notation). */
  accelerator?: string;
  /** See {@link ActionKind}. */
  kind: ActionKind;
  /** Opens a dialog (tells screen readers so). */
  opensDialog?: boolean;
  /** Whether it can be used now. Default: always. */
  enabled?: (s: ActionState) => boolean;
  /** For on/off actions: whether it is on. */
  pressed?: (s: ActionState) => boolean;
  /** Does it. */
  run: (c: ActionContext) => void | Promise<void>;
}

/** Number of generated demo tasks, shown in the demo action's text. */
const DEMO_COUNT = 10_000;

/** A project that can be edited is shown. */
const editable = (s: ActionState) => s.hasProject && !s.isDemoData;
/** … and a task is selected. */
const onTask = (s: ActionState) => editable(s) && s.hasTask;
/** There is a Rust side to open and save files. */
const desktop = (s: ActionState) => s.hasCore;

/** Builds the two shortcut keys from their common start. */
const keys = (name: string) => ({ mac: `shortcuts.${name}Mac`, other: `shortcuts.${name}Other` });
/** A shortcut written the same on every system. */
const sameKeys = (name: string) => ({ mac: `shortcuts.${name}`, other: `shortcuts.${name}` });
/** The key of an action's short text. */
const short = (name: string) => `actions.short.${name}`;

/**
 * All actions by ID.
 *
 * Beginner note: `satisfies` checks that every entry is a complete
 * {@link ActionDefinition}, but keeps the exact IDs, so `ActionId` below
 * can list them.
 */
export const ACTIONS = {
  fileNew: {
    labelKey: "file.new",
    shortKey: short("fileNew"),
    titleKey: "file.newTitle",
    shortcut: keys("new"),
    aria: "Control+N Meta+N",
    accelerator: "CmdOrCtrl+N",
    kind: "file",
    enabled: desktop,
    run: (c) => c.files.newFile(),
  },
  fileOpen: {
    labelKey: "file.open",
    shortKey: short("fileOpen"),
    titleKey: "file.openTitle",
    shortcut: keys("open"),
    aria: "Control+O Meta+O",
    accelerator: "CmdOrCtrl+O",
    kind: "file",
    enabled: desktop,
    run: (c) => c.files.openFile(),
  },
  fileOpenSample: {
    labelKey: "file.openSample",
    shortKey: short("fileOpenSample"),
    kind: "file",
    enabled: desktop,
    run: (c) => c.files.openSample(),
  },
  fileSave: {
    labelKey: "file.save",
    shortKey: short("fileSave"),
    titleKey: "file.saveTitle",
    shortcut: keys("save"),
    aria: "Control+S Meta+S",
    accelerator: "CmdOrCtrl+S",
    kind: "file",
    enabled: desktop,
    run: (c) => c.files.saveFile(),
  },
  fileSaveAs: {
    labelKey: "file.saveAs",
    shortKey: short("fileSaveAs"),
    titleKey: "file.saveAsTitle",
    shortcut: keys("saveAs"),
    aria: "Control+Shift+S Meta+Shift+S",
    accelerator: "Shift+CmdOrCtrl+S",
    kind: "file",
    enabled: desktop,
    run: (c) => c.files.saveFileAs(),
  },
  openSettings: {
    labelKey: "toolbar.settings",
    shortKey: short("openSettings"),
    titleKey: "toolbar.settingsTitle",
    shortcut: keys("settings"),
    aria: "Control+, Meta+,",
    accelerator: "CmdOrCtrl+,",
    kind: "app",
    opensDialog: true,
    run: (c) => c.openDialog("settings"),
  },
  undo: {
    labelKey: "toolbar.undo",
    shortKey: short("undo"),
    titleKey: "toolbar.undoTitle",
    shortcut: keys("undo"),
    aria: "Control+Z Meta+Z",
    accelerator: "CmdOrCtrl+Z",
    kind: "history",
    enabled: (s) => s.canUndo && !s.isDemoData,
    run: (c) => c.undo(),
  },
  redo: {
    labelKey: "toolbar.redo",
    shortKey: short("redo"),
    titleKey: "toolbar.redoTitle",
    shortcut: keys("redo"),
    aria: "Control+Shift+Z Meta+Shift+Z Control+Y",
    accelerator: "Shift+CmdOrCtrl+Z",
    kind: "history",
    enabled: (s) => s.canRedo && !s.isDemoData,
    run: (c) => c.redo(),
  },
  cut: {
    labelKey: "task.cut",
    shortKey: short("cut"),
    shortcut: keys("cut"),
    accelerator: "CmdOrCtrl+X",
    kind: "clipboard",
    enabled: onTask,
    run: (c) => c.tasks.cutSelected(),
  },
  copy: {
    labelKey: "task.copy",
    shortKey: short("copy"),
    shortcut: keys("copy"),
    accelerator: "CmdOrCtrl+C",
    kind: "clipboard",
    enabled: onTask,
    run: (c) => c.tasks.copySelected(),
  },
  paste: {
    labelKey: "task.paste",
    shortKey: short("paste"),
    shortcut: keys("paste"),
    accelerator: "CmdOrCtrl+V",
    kind: "clipboard",
    enabled: editable,
    run: (c) => c.tasks.paste(),
  },
  deleteTasks: {
    labelKey: "task.delete",
    shortKey: short("deleteTasks"),
    titleKey: "task.deleteTitle",
    shortcut: sameKeys("delete"),
    // No accelerator: a menu key "Delete" would be taken from text fields.
    kind: "clipboard",
    enabled: onTask,
    run: (c) => c.tasks.deleteSelected(),
  },
  addProject: {
    labelKey: "task.addProject",
    shortKey: short("addProject"),
    titleKey: "task.addProjectTitle",
    kind: "task",
    enabled: editable,
    run: (c) => c.tasks.addProject(),
  },
  addTask: {
    labelKey: "task.add",
    shortKey: short("addTask"),
    titleKey: "task.insertTitle",
    shortcut: sameKeys("insert"),
    kind: "task",
    enabled: editable,
    run: (c) => c.tasks.insertTask(),
  },
  addSubtask: {
    labelKey: "task.addSubtask",
    shortKey: short("addSubtask"),
    titleKey: "task.addSubtaskTitle",
    kind: "task",
    enabled: onTask,
    run: (c) => c.tasks.addSubtask(),
  },
  addMilestone: {
    labelKey: "task.addMilestone",
    shortKey: short("addMilestone"),
    titleKey: "task.addMilestoneTitle",
    kind: "task",
    enabled: editable,
    run: (c) => c.tasks.addMilestone(),
  },
  linkTasks: {
    labelKey: (s) => (s.selectionLinked ? "task.unlink" : "task.link"),
    shortKey: (s) => short(s.selectionLinked ? "unlinkTasks" : "linkTasks"),
    titleKey: (s) => (s.selectionLinked ? "task.unlinkTitle" : "task.linkTitle"),
    kind: "task",
    enabled: (s) => editable(s) && s.hasSeveralTasks,
    run: (c) => c.tasks.toggleLinks(),
  },
  indent: {
    labelKey: "task.indent",
    shortKey: short("indent"),
    titleKey: "task.indentTitle",
    shortcut: keys("indent"),
    kind: "task",
    enabled: (s) => editable(s) && s.canBecomeSubtasks,
    run: (c) => c.tasks.indentSelected(),
  },
  outdent: {
    labelKey: "task.outdent",
    shortKey: short("outdent"),
    titleKey: "task.outdentTitle",
    shortcut: keys("outdent"),
    kind: "task",
    enabled: (s) => editable(s) && s.canBecomeTasks,
    run: (c) => c.tasks.outdentSelected(),
  },
  moveUp: {
    labelKey: "task.moveUp",
    shortKey: short("moveUp"),
    shortcut: keys("moveUp"),
    kind: "task",
    enabled: onTask,
    run: (c) => c.tasks.moveActive("up"),
  },
  moveDown: {
    labelKey: "task.moveDown",
    shortKey: short("moveDown"),
    shortcut: keys("moveDown"),
    kind: "task",
    enabled: onTask,
    run: (c) => c.tasks.moveActive("down"),
  },
  addBlocker: {
    labelKey: "blocker.add",
    shortKey: short("addBlocker"),
    titleKey: "blocker.addTitle",
    shortcut: keys("addBlocker"),
    aria: "Control+Shift+B Meta+Shift+B",
    accelerator: "Shift+CmdOrCtrl+B",
    kind: "task",
    opensDialog: true,
    enabled: onTask,
    run: (c) => c.tasks.addBlocker(),
  },
  addRisk: {
    labelKey: "risk.add",
    shortKey: short("addRisk"),
    titleKey: "risk.addTitle",
    kind: "task",
    opensDialog: true,
    enabled: onTask,
    run: (c) => c.tasks.addRisk(),
  },
  markOnTrack: {
    labelKey: "task.markOnTrack",
    shortKey: short("markOnTrack"),
    titleKey: "task.markOnTrackTitle",
    kind: "task",
    enabled: onTask,
    run: (c) => void c.tasks.markOnTrack(),
  },
  setPlan: {
    labelKey: "task.setPlan",
    shortKey: short("setPlan"),
    titleKey: "task.setPlanTitle",
    kind: "task",
    enabled: editable,
    run: (c) => void c.tasks.setPlan(),
  },
  taskInfo: {
    labelKey: (s) => (s.onProject ? "task.projectInfo" : "task.info"),
    shortKey: (s) => short(s.onProject ? "projectInfo" : "taskInfo"),
    titleKey: (s) => (s.onProject ? "task.projectInfoTitle" : "task.infoTitle"),
    shortcut: sameKeys("taskInfo"),
    kind: "task",
    opensDialog: true,
    enabled: onTask,
    run: (c) => c.tasks.openTaskInfo(),
  },
  zoomDays: {
    labelKey: "toolbar.zoomDays",
    shortKey: "toolbar.zoomDays",
    kind: "view",
    pressed: (s) => s.zoom === "days",
    run: (c) => c.setZoom("days"),
  },
  zoomWeeks: {
    labelKey: "toolbar.zoomWeeks",
    shortKey: "toolbar.zoomWeeks",
    kind: "view",
    pressed: (s) => s.zoom === "weeks",
    run: (c) => c.setZoom("weeks"),
  },
  zoomMonths: {
    labelKey: "toolbar.zoomMonths",
    shortKey: "toolbar.zoomMonths",
    kind: "view",
    pressed: (s) => s.zoom === "months",
    run: (c) => c.setZoom("months"),
  },
  zoomIn: {
    labelKey: "toolbar.zoomIn",
    shortKey: "toolbar.zoomIn",
    titleKey: "toolbar.zoomInTitle",
    shortcut: keys("zoomIn"),
    aria: "Control+= Meta+=",
    accelerator: "CmdOrCtrl+=",
    kind: "view",
    enabled: (s) => stepZoomPercent(s.zoomPercent, "in") !== s.zoomPercent,
    run: (c) => c.setZoomPercent(stepZoomPercent(c.state().zoomPercent, "in")),
  },
  zoomOut: {
    labelKey: "toolbar.zoomOut",
    shortKey: "toolbar.zoomOut",
    titleKey: "toolbar.zoomOutTitle",
    shortcut: keys("zoomOut"),
    aria: "Control+- Meta+-",
    accelerator: "CmdOrCtrl+-",
    kind: "view",
    enabled: (s) => stepZoomPercent(s.zoomPercent, "out") !== s.zoomPercent,
    run: (c) => c.setZoomPercent(stepZoomPercent(c.state().zoomPercent, "out")),
  },
  zoomReset: {
    labelKey: "toolbar.zoomReset",
    shortKey: "toolbar.zoomPercent",
    titleKey: "toolbar.zoomResetTitle",
    kind: "view",
    values: (s) => ({ percent: s.zoomPercent }),
    run: (c) => c.setZoomPercent(100),
  },
  goToStart: {
    labelKey: "toolbar.projectStart",
    shortKey: short("goToStart"),
    kind: "view",
    run: (c) => c.goToStart(),
  },
  goToToday: {
    labelKey: "toolbar.goToToday",
    shortKey: short("goToToday"),
    kind: "view",
    run: (c) => c.goToToday(),
  },
  expandAll: {
    labelKey: "actions.expandAll",
    shortKey: short("expandAll"),
    kind: "view",
    enabled: (s) => editable(s) && s.hasGroups,
    run: (c) => c.collapseAll(false),
  },
  collapseAll: {
    labelKey: "actions.collapseAll",
    shortKey: short("collapseAll"),
    kind: "view",
    enabled: (s) => editable(s) && s.hasGroups,
    run: (c) => c.collapseAll(true),
  },
  explain: {
    labelKey: "toolbar.explain",
    shortKey: short("explain"),
    titleKey: "toolbar.explainTitle",
    kind: "view",
    pressed: (s) => s.explainOpen,
    run: (c) => c.toggleExplain(),
  },
  legend: {
    labelKey: "actions.legend",
    shortKey: short("legend"),
    kind: "view",
    pressed: (s) => s.legendOpen,
    run: (c) => c.toggleLegend(),
  },
  customizeToolbars: {
    labelKey: "actions.customizeToolbars",
    shortKey: short("customizeToolbars"),
    kind: "app",
    opensDialog: true,
    run: (c) => c.openDialog("customize"),
  },
  resetToolbars: {
    labelKey: "actions.resetToolbars",
    shortKey: short("resetToolbars"),
    kind: "app",
    run: (c) => c.resetToolbars(),
  },
  calendars: {
    labelKey: "toolbar.calendars",
    shortKey: short("calendars"),
    titleKey: "toolbar.calendarsTitle",
    kind: "app",
    opensDialog: true,
    enabled: (s) => s.hasCore && editable(s),
    run: (c) => c.openDialog("calendars"),
  },
  about: {
    labelKey: "actions.about",
    shortKey: short("about"),
    kind: "app",
    opensDialog: true,
    run: (c) => c.openDialog("about"),
  },
  demo: {
    labelKey: "toolbar.mockToggle",
    shortKey: short("demo"),
    count: DEMO_COUNT,
    kind: "view",
    enabled: desktop,
    pressed: (s) => s.isDemoData,
    run: (c) => c.toggleDemo(),
  },
} as const satisfies Record<string, ActionDefinition>;

/** The ID of an action, e.g. `"fileSave"`. */
export type ActionId = keyof typeof ACTIONS;

/** All IDs, in registry order. */
export const ACTION_IDS = Object.keys(ACTIONS) as ActionId[];

/**
 * Tells whether a text (e.g. from a settings file) is an action ID.
 *
 * @param id - Any text.
 * @returns `true` if the registry has it.
 *
 * @example
 * isActionId("fileSave"); // true
 * isActionId("makeTea");  // false
 */
export function isActionId(id: string): id is ActionId {
  // `hasOwn`, not `in`: "toString" is *in* every object.
  return Object.hasOwn(ACTIONS, id);
}

/**
 * One action's definition, with the optional fields visible.
 *
 * @param id - The action.
 * @returns Its entry in {@link ACTIONS}.
 */
export function actionOf(id: ActionId): ActionDefinition {
  return ACTIONS[id];
}

/** Whether `id` can be used in state `s`. */
export function isActionEnabled(id: ActionId, s: ActionState): boolean {
  return actionOf(id).enabled?.(s) ?? true;
}

/** Whether `id` is switched on in state `s`; `undefined` if it is not an on/off action. */
export function isActionPressed(id: ActionId, s: ActionState): boolean | undefined {
  return actionOf(id).pressed?.(s);
}
