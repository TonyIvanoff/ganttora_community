/**
 * features/toolbars/menuModel.ts: describes the native menu bar as data.
 *
 * What it is for:
 *   Decides which menus exist, what they contain, and each entry's text,
 *   shortcut and state, in the user's language. Rust turns this
 *   description into the real menu (`backend/src-tauri/src/menu.rs`).
 *
 * Where it fits:
 *   Called by `useNativeMenu.ts`. A pure function: easy to test.
 *
 * Main ideas:
 *   - **macOS** menus own shortcuts (*accelerators*). To keep normal text
 *     editing working, the Edit menu holds the system's own Undo, Cut, Copy,
 *     Paste while the user is in a text field (`textMode`), and the task
 *     versions otherwise. Keys that text fields need (Delete, Alt+Shift+
 *     arrows…) never become accelerators.
 *   - **Windows and Linux:** the page already handles every shortcut, and an
 *     accelerator there could take Ctrl+C from text fields. So the
 *     shortcut is only *shown*: text after a tab character (`\t`) is drawn
 *     on the right.
 *   - Quit has the ID `quit`; Rust handles it, so the unsaved-changes
 *     question is always asked.
 */

import type { MenuNode, MenuPredefined, MenuStateUpdate, ToolbarLayout } from "@frontend/ipc/types";
import {
  actionOf,
  isActionEnabled,
  isActionPressed,
  type ActionId,
  type ActionState,
} from "./actions";
import { SEPARATOR } from "./layout";
import { actionTexts, toolbarName, type AnyT } from "./useActions";

/** Start of the ID of a "show this toolbar" entry. */
export const TOOLBAR_PREFIX = "toolbar:";

/** ID of Quit / Exit. Must equal `QUIT_ID` in `menu.rs`. */
export const QUIT_ID = "quit";

/** What {@link buildMenu} needs. */
export interface MenuInput {
  /** macOS rules (see the file header). */
  mac: boolean;
  /** The translation function. */
  t: AnyT;
  /** What is possible now. */
  state: ActionState;
  /** All toolbars, for View → Toolbars. */
  toolbars: readonly ToolbarLayout[];
  /** A text field has the focus. */
  textMode: boolean;
  /** A dialog is open: entries that act on the project are greyed out. */
  modal: boolean;
}

/** The standard menus' contents (without the platform's own entries). */
export const MENU_ACTIONS = {
  file: ["fileNew", "fileOpen", "fileOpenSample", SEPARATOR, "fileSave", "fileSaveAs"],
  history: ["undo", "redo"],
  clipboard: ["cut", "copy", "paste"],
  task: [
    "addTask",
    "addSubtask",
    "addMilestone",
    "linkTasks",
    SEPARATOR,
    "indent",
    "outdent",
    "moveUp",
    "moveDown",
    SEPARATOR,
    "addBlocker",
    "addRisk",
    "markOnTrack",
    "taskInfo",
  ],
  view: [
    "zoomDays",
    "zoomWeeks",
    "zoomMonths",
    SEPARATOR,
    "zoomIn",
    "zoomOut",
    "zoomReset",
    "goToToday",
    "goToStart",
    SEPARATOR,
    "expandAll",
    "collapseAll",
    SEPARATOR,
    "explain",
    "legend",
  ],
  project: ["addProject", SEPARATOR, "setPlan", "calendars"],
} as const satisfies Record<string, readonly (ActionId | typeof SEPARATOR)[]>;

/**
 * Doubles every "&": menus on Windows read a single one as "underline the
 * next letter".
 */
const plain = (text: string) => text.replaceAll("&", "&&");

/**
 * Builds the whole menu.
 *
 * @param input - See {@link MenuInput}.
 * @returns The top-level menus, left to right.
 *
 * @example
 * buildMenu({ mac: true, t, state, toolbars, textMode: false, modal: false });
 */
export function buildMenu(input: MenuInput): MenuNode[] {
  const { mac, t, state, toolbars, textMode, modal } = input;
  const line: MenuNode = { kind: "separator" };

  /** One action's entry. */
  function item(id: ActionId): MenuNode {
    const action = actionOf(id);
    const texts = actionTexts(id, t, mac, state);
    const label = plain(texts.label);
    const forText = textMode && (action.kind === "history" || action.kind === "clipboard");
    const pressed = isActionPressed(id, state);
    return {
      kind: "item",
      id,
      label: !mac && texts.shortcut !== undefined ? `${label}\t${texts.shortcut}` : label,
      enabled: !modal && !forText && isActionEnabled(id, state),
      ...(mac && action.accelerator !== undefined ? { accelerator: action.accelerator } : {}),
      ...(pressed === undefined ? {} : { checked: pressed }),
    };
  }
  /** Several actions, with lines. */
  const list = (ids: readonly (ActionId | typeof SEPARATOR)[]): MenuNode[] =>
    ids.map((id) => (id === SEPARATOR ? line : item(id)));
  /** A menu. */
  const menu = (key: string, items: MenuNode[]): MenuNode => ({
    kind: "submenu",
    label: plain(t(`menu.${key}`)),
    items,
  });
  /** An entry the operating system provides. */
  const system = (name: MenuPredefined): MenuNode => ({
    kind: "predefined",
    name,
    label: plain(t(`menu.${name}`)),
  });
  /** Quit, handled by Rust. Works even behind a dialog, like the close button. */
  const quit: MenuNode = {
    kind: "item",
    id: QUIT_ID,
    label: t(mac ? "menu.quit" : "menu.exit"),
    enabled: true,
    ...(mac ? { accelerator: "CmdOrCtrl+Q" } : {}),
  };

  const appMenu: MenuNode[] = mac
    ? [
        {
          kind: "submenu",
          label: t("app.name"),
          items: [
            item("about"),
            line,
            item("openSettings"),
            line,
            system("services"),
            line,
            system("hide"),
            system("hideOthers"),
            system("showAll"),
            line,
            quit,
          ],
        },
      ]
    : [];

  const edit: MenuNode[] =
    mac && textMode
      ? [
          system("undo"),
          system("redo"),
          line,
          system("cut"),
          system("copy"),
          system("paste"),
          system("selectAll"),
        ]
      : [...list(MENU_ACTIONS.history), line, ...list(MENU_ACTIONS.clipboard), item("deleteTasks")];

  const toolbarMenu = menu("toolbars", [
    ...toolbars.map((bar): MenuNode => ({
      kind: "item",
      id: `${TOOLBAR_PREFIX}${bar.id}`,
      label: plain(toolbarName(bar, t)),
      enabled: !modal,
      checked: bar.visible,
    })),
    line,
    item("customizeToolbars"),
    item("resetToolbars"),
  ]);

  return [
    ...appMenu,
    menu("file", [
      ...list(MENU_ACTIONS.file),
      ...(mac ? [] : [line, item("openSettings"), line, quit]),
    ]),
    menu("edit", edit),
    menu("task", list(MENU_ACTIONS.task)),
    menu("view", [
      ...list(MENU_ACTIONS.view),
      line,
      toolbarMenu,
      ...(mac ? [line, system("fullscreen")] : []),
    ]),
    menu("project", list(MENU_ACTIONS.project)),
    ...(mac
      ? [menu("window", [system("minimize"), system("zoomWindow"), line, system("closeWindow")])]
      : []),
    menu("help", [item("about"), line, item("demo")]),
  ];
}

/**
 * The changeable part of a menu: enabled and ticked, per entry.
 *
 * @param nodes - A menu from {@link buildMenu}.
 * @returns One update per entry, also inside submenus.
 */
export function menuStates(nodes: readonly MenuNode[]): MenuStateUpdate[] {
  return nodes.flatMap((node) => {
    if (node.kind === "submenu") return menuStates(node.items);
    if (node.kind !== "item") return [];
    return [{ id: node.id, enabled: node.enabled, checked: node.checked ?? null }];
  });
}

/**
 * The fixed part of a menu, as one text to compare: if it is unchanged, the
 * menu need not be rebuilt and {@link menuStates} is enough.
 *
 * @param nodes - A menu from {@link buildMenu}.
 */
export function menuStructure(nodes: readonly MenuNode[]): string {
  // The second parameter of `stringify` may replace values: here it blanks
  // the two that change often.
  return JSON.stringify(nodes, (key, value: unknown) =>
    key === "enabled" || key === "checked" ? 0 : value,
  );
}
