/**
 * features/toolbars/useActions.ts: brings the action registry to life.
 *
 * What it is for:
 *   - {@link useActionState}: the live {@link ActionState} (redraws buttons);
 *   - {@link useRunAction}: one function that carries any action out, with
 *     the checks every caller needs (dialog open? possible now? same key
 *     press twice?);
 *   - {@link actionTexts}: label, short text, tooltip and shortcut text.
 *
 * Where it fits:
 *   Used by `AppShell.tsx` (shortcuts, menu), the toolbars and the grid's
 *   context menu. The definitions are in `actions.ts`.
 */

import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";
import { useCalendarDialogStore } from "@frontend/features/calendars/calendarStore";
import { useExplainStore } from "@frontend/features/explain/explainStore";
import { useFileActions } from "@frontend/features/files/useFileActions";
import { useTaskActions } from "@frontend/features/grid/useTaskActions";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { hasDesktopCore } from "@frontend/ipc/client";
import type { ToolbarLayout } from "@frontend/ipc/types";
import { isTextEntry } from "@frontend/lib/shortcuts";
import { isModalOpen, useDialogStore } from "@frontend/shell/dialogStore";
import { useAppStore, type AppState } from "@frontend/state/store";
import { claimAction, type ActionSource } from "./actionClaim";
import {
  actionOf,
  isActionEnabled,
  type ActionContext,
  type ActionId,
  type ActionState,
  type ActionKey,
} from "./actions";
import { isPredefined } from "./layout";
import {
  canBecomeSubtasks,
  canBecomeTasks,
  selectionIsLinked,
} from "@frontend/features/grid/outline";
import { useToolbarStore } from "./toolbarStore";

/** `true` on macOS (⌘ instead of Ctrl in shortcut texts). */
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

/** Picks the parts of the app store that actions depend on. */
function pick(s: AppState): Omit<ActionState, "explainOpen" | "legendOpen" | "hasCore"> {
  return {
    isDemoData: s.isDemoData,
    hasProject: s.snapshot !== null,
    hasTask: s.selectedTaskId !== null,
    hasGroups: s.snapshot?.tasks.some((task) => task.isSummary) ?? false,
    canUndo: s.canUndo,
    canRedo: s.canRedo,
    zoom: s.zoom,
    zoomPercent: s.zoomPercent,
    hasSeveralTasks: s.selectedTaskIds.length > 1,
    onProject:
      s.snapshot?.tasks.some((task) => task.id === s.selectedTaskId && task.isProject === true) ??
      false,
    canBecomeSubtasks: canBecomeSubtasks(s.snapshot?.tasks ?? [], s.selectedTaskIds),
    canBecomeTasks: canBecomeTasks(s.snapshot?.tasks ?? [], s.selectedTaskIds),
    selectionLinked: selectionIsLinked(s.snapshot?.tasks ?? [], s.selectedTaskIds),
  };
}

/**
 * The state right now, read from the stores (not from a drawing).
 *
 * @returns A fresh {@link ActionState}.
 */
export function readActionState(): ActionState {
  return {
    ...pick(useAppStore.getState()),
    explainOpen: useExplainStore.getState().open,
    legendOpen: useDialogStore.getState().legendOpen,
    hasCore: hasDesktopCore(),
  };
}

/**
 * The live state; the component redraws when it changes.
 *
 * Beginner note: `useShallow` compares the picked values one by one, so a
 * new object with the same values causes no redraw.
 */
export function useActionState(): ActionState {
  const picked = useAppStore(useShallow(pick));
  const explainOpen = useExplainStore((s) => s.open);
  const legendOpen = useDialogStore((s) => s.legendOpen);
  return useMemo(
    () => ({ ...picked, explainOpen, legendOpen, hasCore: hasDesktopCore() }),
    [picked, explainOpen, legendOpen],
  );
}

/**
 * A toolbar's name on screen.
 *
 * @param bar - The toolbar.
 * @param t - The translation function.
 * @returns The translated name of a predefined one, or the user's own name.
 */
export function toolbarName(bar: ToolbarLayout, t: AnyT): string {
  return isPredefined(bar.id) ? t(`toolbars.name.${bar.id}`) : bar.name;
}

/** A translation function that accepts any key (registry keys are plain text). */
export type AnyT = (key: string, values?: Record<string, unknown>) => string;

/** All texts of one action, translated. */
export interface ActionTexts {
  /** Full text, for menus. */
  label: string;
  /** Short text, for toolbar buttons. */
  short: string;
  /** Tooltip. */
  title: string;
  /** Shortcut text, if there is one. */
  shortcut: string | undefined;
}

/**
 * Translates an action's texts.
 *
 * @param id - The action.
 * @param t - The translation function.
 * @param mac - macOS shortcut texts (default: this computer's).
 * @param state - The live state, for texts that change (the zoom).
 *
 * @example
 * actionTexts("fileSave", t, false).shortcut; // "Ctrl+S"
 */
export function actionTexts(
  id: ActionId,
  t: AnyT,
  mac: boolean = IS_MAC,
  state: ActionState = readActionState(),
): ActionTexts {
  const action = actionOf(id);
  const shortcut =
    action.shortcut === undefined
      ? undefined
      : t(mac ? action.shortcut.mac : action.shortcut.other);
  /** The key for this state. */
  const key = (k: ActionKey): string => (typeof k === "string" ? k : k(state));
  const label = t(key(action.labelKey), { count: action.count });
  return {
    label,
    short: t(key(action.shortKey), action.values?.(state)),
    title:
      action.titleKey === undefined
        ? label
        : t(key(action.titleKey), {
            shortcut,
            modifier: t(mac ? "shortcuts.wheelModifierMac" : "shortcuts.wheelModifierOther"),
          }),
    shortcut,
  };
}

/**
 * The translation function for registry keys.
 *
 * `t` normally accepts only keys it can check at compile time. The
 * registry's keys are checked by a test instead (`actions.test.ts`).
 */
export function useAnyT(): AnyT {
  const { t } = useTranslation();
  return t as unknown as AnyT;
}

/**
 * Whether the keyboard focus (or a text selection) wants Cut, Copy, Paste
 * and Undo for *text* right now.
 *
 * @returns `true` in text fields, drop-down lists and while text outside
 *   the task grid is selected.
 */
export function isTextMode(): boolean {
  const focused = document.activeElement;
  if (isTextEntry(focused) || focused instanceof HTMLSelectElement) return true;
  const selection = document.getSelection();
  if (selection === null || selection.isCollapsed) return false;
  const anchor = selection.anchorNode;
  const element = anchor instanceof Element ? anchor : anchor?.parentElement;
  return element?.closest('[role="treegrid"]') == null;
}

/** Carries an action out; see {@link useRunAction}. */
export type RunAction = (id: ActionId, source: ActionSource) => void;

/**
 * Gives the function that carries out any action.
 *
 * It does nothing when a dialog is open, when the action is not possible
 * now, when it is the same key press reported twice, or when a menu entry
 * for tasks is chosen while a text field is being edited.
 *
 * @returns A stable function (good for effects).
 *
 * @example
 * const run = useRunAction();
 * <button onClick={() => run("fileSave", "button")}>…</button>
 */
export function useRunAction(): RunAction {
  const t = useAnyT();
  const files = useFileActions();
  const tasks = useTaskActions();

  const context = useMemo<ActionContext>(
    () => ({
      files,
      tasks,
      undo: () => useAppStore.getState().undo(),
      redo: () => useAppStore.getState().redo(),
      setZoom: (zoom) => useAppStore.getState().setZoom(zoom),
      setZoomPercent: (percent) => useAppStore.getState().setZoomPercent(percent),
      state: readActionState,
      goToStart: () => useAppStore.getState().requestScrollToStart(),
      goToToday: () => useAppStore.getState().requestScrollToToday(),
      collapseAll: (collapsed) => {
        const app = useAppStore.getState();
        const groups = app.snapshot?.tasks.filter((task) => task.isSummary) ?? [];
        app.setCollapsed(
          groups.map((task) => task.id),
          collapsed,
        );
      },
      toggleExplain: () => useExplainStore.getState().toggle(),
      toggleLegend: () => useDialogStore.getState().toggleLegend(),
      toggleDemo: () => {
        const app = useAppStore.getState();
        app.setShowMock(!app.isDemoData, (n) => t("gantt.mockTaskName", { number: n }));
      },
      openDialog: (dialog) => {
        if (dialog === "settings") useSettingsStore.getState().openDialog();
        else if (dialog === "calendars") useCalendarDialogStore.getState().openDialog();
        else useDialogStore.getState().show(dialog);
      },
      resetToolbars: () => useToolbarStore.getState().reset(),
    }),
    [files, tasks, t],
  );

  return useCallback(
    (id, source) => {
      if (isModalOpen()) return;
      const action = actionOf(id);
      if (source === "menu") {
        if (isTextMode() && (action.kind === "history" || action.kind === "clipboard")) return;
      }
      if (!isActionEnabled(id, readActionState())) return;
      if (!claimAction(id, source)) return;
      if (source === "menu" && action.kind !== "history" && action.kind !== "clipboard") {
        // Choosing a menu entry does not move the focus. Leave the field
        // first, so an open cell edit is saved before the action runs.
        if (document.activeElement instanceof HTMLElement && isTextEntry(document.activeElement)) {
          document.activeElement.blur();
        }
      }
      void action.run(context);
    },
    [context],
  );
}
