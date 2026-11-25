/**
 * shell/AppShell.tsx: the window layout: title row on top, toolbars and
 * the view in the middle, status bar at the bottom.
 *
 * What it is for:
 *   Every screen of Ganttora lives inside this frame. It holds:
 *   - the title row: app name, file name ("•" = unsaved changes), project
 *     name, finish date and the "Toolbars" button;
 *   - the error, file and settings banners;
 *   - `ToolbarFrame`: the dockable toolbars around the view area (task
 *     grid, splitter, Gantt chart, "Why this date?" panel);
 *   - the status bar (task counts, core version, FPS in development);
 *   - all dialogs.
 *   It also connects the native menu (`useNativeMenu`), listens for the
 *   window-wide shortcuts (`lib/shortcuts.ts`), and starts the close-window
 *   guard and the autosave timer. The grid's own shortcuts are in
 *   `features/grid/gridShortcuts.ts`.
 *
 * Where it fits:
 *   Rendered by `App.tsx`. What the user can *do* is defined once, in
 *   `features/toolbars/actions.ts`.
 *
 * Main ideas:
 *   - The layout is a vertical *flexbox* (`flex flex-col`): the rows take
 *     their natural height, the frame takes all the rest (`flex-1`).
 *     `min-h-0` lets the view shrink so its own scrollbars work.
 *   - Every text comes from the translation file (`t("…")`).
 */

import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { FileBanners } from "@frontend/features/files/FileBanners";
import { useAutosave } from "@frontend/features/files/useAutosave";
import { useCloseGuard } from "@frontend/features/files/useCloseGuard";
import { useDocumentTitle } from "@frontend/features/files/useDocumentTitle";
import { useFileActions } from "@frontend/features/files/useFileActions";
import { CalendarDialog } from "@frontend/features/calendars/CalendarDialog";
import { ExplainPanel } from "@frontend/features/explain/ExplainPanel";
import { useExplainStore } from "@frontend/features/explain/explainStore";
import { AboutDialog } from "@frontend/features/about/AboutDialog";
import { LegendPanel } from "@frontend/features/gantt/LegendDialog";
import { CustomizeDialog } from "@frontend/features/toolbars/CustomizeDialog";
import { ToolbarFrame, ToolbarsMenuButton } from "@frontend/features/toolbars/ToolbarFrame";
import { useRunAction } from "@frontend/features/toolbars/useActions";
import { useNativeMenu } from "@frontend/features/toolbars/useNativeMenu";
import { FpsCounter } from "@frontend/features/gantt/FpsCounter";
import { GanttView } from "@frontend/features/gantt/GanttView";
import { useRememberZoom } from "@frontend/features/gantt/useRememberZoom";
import { SettingsDialog } from "@frontend/features/settings/SettingsDialog";
import { SettingsNoticeBanner } from "@frontend/features/settings/SettingsNoticeBanner";
import { GridSplitter } from "@frontend/features/grid/GridSplitter";
import { TaskGrid } from "@frontend/features/grid/TaskGrid";
import { BlockerDialog } from "@frontend/features/task-info/BlockerDialog";
import { RiskDialog } from "@frontend/features/task-info/RiskDialog";
import { TaskInfoDialog } from "@frontend/features/task-info/TaskInfoDialog";
import { hasDesktopCore } from "@frontend/ipc/client";
import { errorText } from "@frontend/lib/errors";
import { matchShortcut } from "@frontend/lib/shortcuts";
import { isModalOpen } from "./dialogStore";
import { ProjectInfo } from "./ProjectInfo";
// The same picture as the Dock and taskbar icon.
import appIcon from "../../backend/src-tauri/icons/app-icon.svg";
import { SettingsButton } from "./SettingsButton";
import { Tooltips } from "@frontend/components/ui/Tooltips";
import { UiZoom } from "./UiZoom";
import { ThemeSwitch } from "./ThemeSwitch";
import { useAppStore } from "@frontend/state/store";

/**
 * The application frame.
 *
 * @returns The full-window layout.
 */
export function AppShell() {
  const { t } = useTranslation();
  /** The element around the Gantt chart (the grid syncs its scrolling with it). */
  const ganttContainerRef = useRef<HTMLDivElement>(null);
  // Projects hold tasks; they are not tasks themselves.
  const taskCount = useAppStore((s) => s.tasks.filter((task) => task.isProject !== true).length);
  const isDemoData = useAppStore((s) => s.isDemoData);
  const appInfo = useAppStore((s) => s.appInfo);
  const snapshot = useAppStore((s) => s.snapshot);
  const commandError = useAppStore((s) => s.commandError);
  const dismissError = useAppStore((s) => s.dismissError);
  const hasCore = hasDesktopCore();
  const explainOpen = useExplainStore((s) => s.open);

  const { resolveUnsavedChanges } = useFileActions();
  // Closing the window with unsaved changes asks "Save / Don't save / Cancel".
  useCloseGuard(resolveUnsavedChanges);
  // Every few minutes (Settings → Files), a recovery copy of unsaved work.
  useAutosave();
  // Days/Weeks/Months and the zoom percentage survive a restart.
  useRememberZoom();
  // The file name with "•" for unsaved changes; also sets the window title.
  // Keeps the window title (file name, "•" = unsaved changes) up to date.
  useDocumentTitle();
  // Every action (menu bar, toolbars, shortcuts) goes through `run`.
  const run = useRunAction();
  useNativeMenu(run);

  // Keyboard shortcuts (Undo, Redo, New, Open, Save, Save As…), anywhere in
  // the window. `window.addEventListener` hears every key press; the
  // clean-up function removes the listener again when AppShell disappears
  // (or before the effect runs again with a new `run`).
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      const action = matchShortcut(event);
      if (action === null) return;
      // While a dialog is open, project shortcuts must not act on the
      // project behind it (Ctrl+S there should not save the file).
      if (isModalOpen()) return;
      // Stop the web view's own behaviour (its Undo, "Save page as…",
      // "Open new window"…) from also running.
      event.preventDefault();
      // Holding a key down repeats the key press; opening a file window
      // again and again would be unpleasant, so repeats are ignored for
      // everything but Undo and Redo.
      if (event.repeat && action !== "undo" && action !== "redo") return;
      // A shortcut's action name is also its ID in the action registry.
      run(action, "key");
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [run]);

  // Count critical tasks (projects are not tasks). This selector runs whenever the store changes,
  // which is rare (a click, a new schedule), and walking 10,000 booleans
  // takes far less than a millisecond, so no caching is needed.
  const criticalCount = useAppStore((s) =>
    s.tasks.reduce((n, task) => n + (task.isCritical && task.isProject !== true ? 1 : 0), 0),
  );

  return (
    <div className="flex h-full flex-col bg-app text-fg">
      {/* ---- Title row: app, project, "Toolbars" ---- */}
      <header className="flex flex-wrap items-center gap-3 border-b border-line bg-surface px-3 py-1">
        <span className="flex items-center gap-1.5 font-semibold whitespace-nowrap">
          {/* Decoration: the name next to it says the same. */}
          <img src={appIcon} alt="" width="20" height="20" />
          {t("app.name")}
        </span>
        {/* The clicked row's project. The file name is in the window title. */}
        {snapshot !== null && !isDemoData && (
          <span className="flex flex-wrap gap-x-3 text-sm whitespace-nowrap">
            <ProjectInfo />
          </span>
        )}
        <span className="ml-auto flex items-center gap-2">
          <UiZoom />
          <span aria-hidden="true" className="h-5 border-l border-line" />
          <ThemeSwitch />
          <SettingsButton />
          <ToolbarsMenuButton />
        </span>
      </header>

      {/* ---- Error banner ----
          role="alert" makes screen readers announce it as soon as it appears. */}
      {snapshot?.error != null && !isDemoData && (
        <div
          role="alert"
          className="border-b border-danger-line bg-danger-bg px-3 py-2 text-sm text-danger-fg"
        >
          {t("project.scheduleError", { message: errorText(snapshot.error, t) })}
        </div>
      )}
      {commandError !== null && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 border-b border-danger-line bg-danger-bg px-3 py-2 text-sm text-danger-fg"
        >
          <span>
            {t("project.commandError", {
              // Already translated text, or a raw IpcError to translate now.
              message:
                "text" in commandError ? commandError.text : errorText(commandError.error, t),
            })}
          </span>
          <button
            type="button"
            className="ml-auto rounded border border-danger-line px-2 py-0.5 whitespace-nowrap"
            onClick={dismissError}
          >
            {t("project.dismissError")}
          </button>
        </div>
      )}

      {/* ---- File banners: read-only file, notes after opening ---- */}
      <FileBanners />
      {/* ---- Settings file could not be used at start-up ---- */}
      <SettingsNoticeBanner />

      {/* ---- Toolbars on the edges, the view in the middle ---- */}
      <ToolbarFrame run={run}>
        {/* ---- View area: takes all remaining height ----
          Task grid | splitter | Gantt chart, rows side by side. The grid
          finds the chart's scroller inside `ganttContainerRef` to keep both
          scrolled to the same rows. */}
        <main className="flex min-h-0 flex-1">
          <TaskGrid ganttContainerRef={ganttContainerRef} />
          <GridSplitter />
          <div ref={ganttContainerRef} className="min-w-0 flex-1">
            <GanttView />
          </div>
          {explainOpen && <ExplainPanel />}
        </main>
      </ToolbarFrame>

      {/* ---- Status bar ----
          Not role="status": that would make screen readers announce every
          change, including the FPS counter twice a second. */}
      <footer
        aria-label={t("status.label")}
        className="flex flex-wrap items-center gap-x-4 border-t border-line bg-surface px-3 py-1 text-xs text-fg-muted"
      >
        <span>{t("status.taskCount", { count: taskCount })}</span>
        <span>{t("status.criticalCount", { count: criticalCount })}</span>
        {isDemoData && <span>{t("gantt.demoData")}</span>}
        <span className="ml-auto">
          {appInfo !== null
            ? t("status.core", { version: appInfo.version })
            : hasCore
              ? null
              : t("status.coreUnavailable")}
        </span>
        {/* import.meta.env.DEV is true only in development builds; the
            build tool removes this whole element from release builds. */}
        {import.meta.env.DEV && <FpsCounter />}
      </footer>

      {/* The Settings dialog: invisible until opened. */}
      <SettingsDialog />
      {/* The calendar editor: invisible until opened. */}
      <CalendarDialog />
      {/* Task Information (double-click the ID cell, Shift+F2): invisible until opened. */}
      <TaskInfoDialog />
      {/* Add blocker (toolbar, grid menu, Ctrl/⌘+Shift+B): invisible until opened. */}
      <BlockerDialog />
      <RiskDialog />
      {/* Colour legend, About, Customize toolbars (shell/dialogStore.ts). */}
      <LegendPanel />
      <AboutDialog />
      <CustomizeDialog />
      <Tooltips />
    </div>
  );
}
