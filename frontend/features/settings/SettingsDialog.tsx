/**
 * features/settings/SettingsDialog.tsx: the Settings window.
 *
 * What it is for:
 *   Lets the user see and change every app setting, in five
 *   groups shown as tabs: User, General, Scheduling defaults, Files, and
 *   Updates & privacy. Changes are made on a *draft* copy; nothing happens
 *   until the user presses Save. Then the whole draft is sent to Rust,
 *   which cleans it, writes `settings.json` and answers with what it
 *   stored. Theme and language then switch at once (`useApplySettings.ts`).
 *
 * Where it fits:
 *   Rendered once by `shell/AppShell.tsx`; opened by the toolbar button
 *   "Settings…" or Ctrl+, / ⌘, (`lib/shortcuts.ts`). State lives in
 *   `settingsStore.ts`.
 *
 * Main ideas:
 *   - **The native `<dialog>` element.** `showModal()` shows it above the
 *     page, keeps keyboard focus inside it, dims the page behind it and
 *     closes it on Escape: accessibility for free, no extra library.
 *   - **Tabs follow the WAI-ARIA pattern:** a `tablist` with `tab` buttons
 *     and `tabpanel`s; Arrow Up/Down, Home and End move between tabs.
 *   - Fields with no effect yet (scheduling defaults, autosave, updates)
 *     are stored already and marked with a short note, so the user is not
 *     surprised that nothing changes yet.
 *   - Layout: labels sit *above* their fields and widths are flexible, so
 *     German labels (~30% longer) and Chinese text fit.
 *   - Every text comes from `locales/en.json` (`settings.*`).
 */

import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { availableLanguages, SUPPORTED_LANGUAGES } from "@frontend/i18n";
import {
  DATE_FORMAT_SYSTEM,
  dateFormatChoice,
  dateStyleOf,
  formatScheduleDate,
  patternOf,
} from "@frontend/i18n/format";
import { hasDesktopCore } from "@frontend/ipc/client";
import type { Settings } from "@frontend/ipc/types";
import { errorMessage } from "@frontend/lib/errors";
import {
  CALENDAR_OPTIONS,
  CURRENCY_OPTIONS,
  DATE_FORMAT_OPTIONS,
  FIRST_DAY_OPTIONS,
  TASK_MODE_OPTIONS,
  TASK_TYPE_OPTIONS,
  THEME_OPTIONS,
} from "./options";
import { useSettingsStore } from "./settingsStore";

/** The tabs, in order, with the translation keys of their labels. */
const GROUPS = [
  { id: "user", labelKey: "settings.groups.user" },
  { id: "general", labelKey: "settings.groups.general" },
  { id: "scheduling", labelKey: "settings.groups.scheduling" },
  { id: "files", labelKey: "settings.groups.files" },
  { id: "updates", labelKey: "settings.groups.updates" },
] as const;

/** One tab id: "user" | "general" | … */
type GroupId = (typeof GROUPS)[number]["id"];

/**
 * The date used to preview each date format: the garden shed's finish
 * a Wednesday, so the weekday is visible too.
 */
const SAMPLE_DATE = "2027-03-24T17:00:00";

/** Shared look of text, number and drop-down fields (Tailwind classes). */
const FIELD_CLASS =
  "w-full min-w-0 rounded border border-line bg-surface px-2 py-1 text-sm text-fg " +
  "focus:outline-2 focus:outline-focus";

/** Shared look of buttons. */
const BUTTON_CLASS =
  "whitespace-nowrap rounded border border-line px-3 py-1 text-sm hover:bg-app " +
  "disabled:opacity-50";

/**
 * A label above a field, with an optional hint below it.
 *
 * @param props.label - The visible label text.
 * @param props.hint - Optional help text under the field.
 * @param props.notYetActive - `true` for a setting that is saved but has
 *   no effect yet in this version; a short note says so under the field.
 * @param props.children - A function that receives the ids to put on the
 *   field (`id` for the label, `describedBy` for the hint and the note).
 * @returns The labelled field.
 *
 * Beginner note: passing a *function* as `children` ("render prop") lets
 * this component create the ids and hand them to whatever field the caller
 * draws, so label and field are always linked for screen readers.
 */
function Field(props: {
  label: string;
  hint?: string;
  notYetActive?: boolean;
  children: (ids: { id: string; describedBy: string | undefined }) => ReactNode;
}) {
  const { t } = useTranslation();
  const id = useId();
  const hintId = `${id}-hint`;
  const noteId = `${id}-note`;
  // `aria-describedby` takes several ids separated by spaces.
  const describedIds = [
    props.hint !== undefined ? hintId : null,
    props.notYetActive === true ? noteId : null,
  ].filter((value) => value !== null);
  const describedBy = describedIds.length > 0 ? describedIds.join(" ") : undefined;
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-sm font-medium">
        {props.label}
      </label>
      {props.children({ id, describedBy })}
      {props.hint !== undefined && (
        <p id={hintId} className="text-xs text-fg-muted">
          {props.hint}
        </p>
      )}
      {props.notYetActive === true && (
        <p id={noteId} className="text-xs text-fg-muted italic">
          {t("settings.fieldNotYetActive")}
        </p>
      )}
    </div>
  );
}

/**
 * A checkbox with its label to the right and an optional hint.
 *
 * @param props.label - The label text.
 * @param props.hint - Optional help text.
 * @param props.notYetActive - `true` for a setting with no effect yet (see
 *   {@link Field}).
 * @param props.checked - Current value.
 * @param props.onChange - Called with the new value.
 * @returns The checkbox row.
 */
function CheckField(props: {
  label: string;
  hint?: string;
  notYetActive?: boolean;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <div className="flex items-start gap-2">
      <input
        id={id}
        type="checkbox"
        className="mt-1"
        checked={props.checked}
        aria-describedby={
          [
            props.hint !== undefined ? `${id}-hint` : null,
            props.notYetActive === true ? `${id}-note` : null,
          ]
            .filter((value) => value !== null)
            .join(" ") || undefined
        }
        onChange={(e) => props.onChange(e.target.checked)}
      />
      <div className="flex flex-col">
        <label htmlFor={id} className="text-sm">
          {props.label}
        </label>
        {props.hint !== undefined && (
          <p id={`${id}-hint`} className="text-xs text-fg-muted">
            {props.hint}
          </p>
        )}
        {props.notYetActive === true && (
          <p id={`${id}-note`} className="text-xs text-fg-muted italic">
            {t("settings.fieldNotYetActive")}
          </p>
        )}
      </div>
    </div>
  );
}

/**
 * A number field that lets the user clear it while typing.
 *
 * A plain controlled `<input type="number">` would jump back to the old
 * value the moment the box is empty. So the typed *text* is kept here, and
 * the number is passed on only when the text is a valid number in range.
 * When the field loses focus, it shows the stored number again (so an
 * invalid or empty entry is undone).
 *
 * @param props.id - Element id (from {@link Field}).
 * @param props.describedBy - Id of the hint, if any.
 * @param props.value - The current number.
 * @param props.min - Smallest allowed value.
 * @param props.max - Largest allowed value.
 * @param props.step - Step of the arrow buttons (e.g. 0.5 or 1).
 * @param props.onChange - Called with each valid new number.
 * @returns The input element.
 */
function NumberField(props: {
  id: string;
  describedBy: string | undefined;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}) {
  // The text being typed, or `null` when the field is not being edited.
  // While `null`, the field simply shows the stored number, so a change
  // from outside (e.g. "Reset to defaults") appears at once.
  const [editing, setEditing] = useState<string | null>(null);
  const text = editing ?? String(props.value);
  return (
    <input
      id={props.id}
      type="number"
      inputMode="decimal"
      className={FIELD_CLASS}
      min={props.min}
      max={props.max}
      step={props.step}
      value={text}
      aria-describedby={props.describedBy}
      onChange={(e) => {
        setEditing(e.target.value);
        const n = e.target.valueAsNumber;
        if (!Number.isNaN(n) && n >= props.min && n <= props.max) props.onChange(n);
      }}
      onBlur={() => setEditing(null)}
    />
  );
}

/**
 * The Settings dialog. Renders nothing visible while closed.
 *
 * @returns The `<dialog>` element.
 */
export function SettingsDialog() {
  const { t, i18n } = useTranslation();
  const dialogOpen = useSettingsStore((s) => s.dialogOpen);
  const settings = useSettingsStore((s) => s.settings);
  const save = useSettingsStore((s) => s.save);
  const reset = useSettingsStore((s) => s.reset);
  const closeDialog = useSettingsStore((s) => s.closeDialog);

  const dialogRef = useRef<HTMLDialogElement>(null);
  const tabRefs = useRef<Partial<Record<GroupId, HTMLButtonElement | null>>>({});
  const baseId = useId();
  const [group, setGroup] = useState<GroupId>("general");
  const [draft, setDraft] = useState<Settings>(settings);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Open / close the native dialog when the store says so. Each time it
  // opens, the draft starts as a fresh copy of the current settings.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    if (dialogOpen && !dialog.open) {
      setDraft(useSettingsStore.getState().settings);
      setError(null);
      dialog.showModal();
    } else if (!dialogOpen && dialog.open) {
      dialog.close();
    }
  }, [dialogOpen]);

  /**
   * Changes one group of the draft.
   *
   * Beginner note: `K extends keyof Settings` makes this *generic*: `key`
   * is "user", "general"… and `patch` must contain fields of exactly that
   * group. `{ ...a, ...b }` (spread) copies `a` and overwrites with `b`;
   * React needs a *new* object to notice the change.
   */
  function update<K extends "user" | "general" | "scheduling" | "files" | "updates">(
    key: K,
    patch: Partial<Settings[K]>,
  ): void {
    setDraft((d) => ({ ...d, [key]: { ...d[key], ...patch } }));
  }

  /** Sends the draft to Rust; closes on success, shows the error otherwise. */
  async function onSave(): Promise<void> {
    setBusy(true);
    const failure = await save(draft);
    setBusy(false);
    if (failure === null) closeDialog();
    else setError(errorMessage(failure, t));
  }

  /** Restores the defaults (stored at once) and shows them in the draft. */
  async function onReset(): Promise<void> {
    setBusy(true);
    const failure = await reset();
    setBusy(false);
    setDraft(useSettingsStore.getState().settings);
    setError(failure === null ? null : errorMessage(failure, t));
  }

  /** Lets the user pick the default save folder with the system window. */
  async function onChooseFolder(): Promise<void> {
    try {
      const folder = await openFolderDialog({
        directory: true,
        multiple: false,
        ...(draft.files.defaultSaveFolder !== null
          ? { defaultPath: draft.files.defaultSaveFolder }
          : {}),
      });
      if (folder !== null) update("files", { defaultSaveFolder: folder });
    } catch (e: unknown) {
      setError(errorMessage(e, t));
    }
  }

  /** Arrow keys, Home and End move between tabs (WAI-ARIA tabs pattern). */
  function onTabKeyDown(event: KeyboardEvent<HTMLButtonElement>): void {
    const index = GROUPS.findIndex((g) => g.id === group);
    let next: number;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") next = index + 1;
    else if (event.key === "ArrowUp" || event.key === "ArrowLeft") next = index - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = GROUPS.length - 1;
    else return;
    event.preventDefault();
    // `%` (remainder) wraps around: after the last tab comes the first.
    const target = GROUPS[(next + GROUPS.length) % GROUPS.length];
    if (target === undefined) return;
    setGroup(target.id);
    tabRefs.current[target.id]?.focus();
  }

  const languages = SUPPORTED_LANGUAGES.filter((l) => availableLanguages().includes(l.code));
  // Names of currencies in the UI language, from Intl ("Euro", "US Dollar"…).
  const currencyNames = new Intl.DisplayNames([i18n.language], { type: "currency" });
  const sample = (format: string) =>
    formatScheduleDate(SAMPLE_DATE, dateStyleOf(format, i18n.language)) ?? "";

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={`${baseId}-title`}
      className="m-auto w-[min(48rem,calc(100vw-2rem))] max-h-[calc(100vh-2rem)] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      // Escape, or the window's own close: keep the store in step.
      onClose={closeDialog}
    >
      <form
        method="dialog"
        className="flex max-h-[calc(100vh-2rem)] flex-col"
        onSubmit={(e) => {
          // We save through Rust first; the dialog closes only on success.
          e.preventDefault();
          void onSave();
        }}
      >
        <h2
          id={`${baseId}-title`}
          className="border-b border-line px-4 py-3 text-base font-semibold"
        >
          {t("settings.title")}
        </h2>

        <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
          {/* ---- Tabs ---- */}
          <div
            role="tablist"
            aria-orientation="vertical"
            aria-label={t("settings.groupsLabel")}
            className="flex shrink-0 flex-row flex-wrap gap-1 border-b border-line p-2 sm:flex-col sm:border-r sm:border-b-0"
          >
            {GROUPS.map((g) => (
              <button
                key={g.id}
                ref={(el) => {
                  tabRefs.current[g.id] = el;
                }}
                type="button"
                role="tab"
                id={`${baseId}-tab-${g.id}`}
                aria-selected={group === g.id}
                aria-controls={`${baseId}-panel`}
                // Only the selected tab is in the Tab order; arrows move
                // between tabs (the standard behaviour of tab lists).
                tabIndex={group === g.id ? 0 : -1}
                className="rounded px-3 py-1.5 text-left text-sm whitespace-nowrap hover:bg-app aria-selected:bg-accent aria-selected:text-accent-fg"
                onClick={() => setGroup(g.id)}
                onKeyDown={onTabKeyDown}
              >
                {t(g.labelKey)}
              </button>
            ))}
          </div>

          {/* ---- The selected group ---- */}
          <div
            role="tabpanel"
            id={`${baseId}-panel`}
            aria-labelledby={`${baseId}-tab-${group}`}
            className="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-4"
          >
            {group === "user" && (
              <>
                <p className="text-xs text-fg-muted">{t("settings.user.intro")}</p>
                <Field
                  label={t("settings.user.displayName")}
                  hint={t("settings.user.displayNameHint")}
                >
                  {({ id, describedBy }) => (
                    <input
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.user.displayName}
                      autoComplete="name"
                      onChange={(e) => update("user", { displayName: e.target.value })}
                    />
                  )}
                </Field>
                <Field label={t("settings.user.initials")} hint={t("settings.user.initialsHint")}>
                  {({ id, describedBy }) => (
                    <input
                      id={id}
                      aria-describedby={describedBy}
                      className={`${FIELD_CLASS} max-w-32`}
                      value={draft.user.initials}
                      maxLength={4}
                      onChange={(e) => update("user", { initials: e.target.value })}
                    />
                  )}
                </Field>
                <Field label={t("settings.user.email")} hint={t("settings.user.emailHint")}>
                  {({ id, describedBy }) => (
                    <input
                      id={id}
                      type="email"
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.user.email}
                      autoComplete="email"
                      onChange={(e) => update("user", { email: e.target.value })}
                    />
                  )}
                </Field>
              </>
            )}

            {group === "general" && (
              <>
                <Field
                  label={t("settings.general.language")}
                  hint={t("settings.general.languageHint")}
                >
                  {({ id, describedBy }) => (
                    <select
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.general.language}
                      onChange={(e) => update("general", { language: e.target.value })}
                    >
                      {languages.map((l) => (
                        // `lang` lets the screen reader pronounce each name
                        // in its own language.
                        <option key={l.code} value={l.code} lang={l.code}>
                          {l.name}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field label={t("settings.general.theme")}>
                  {({ id }) => (
                    <select
                      id={id}
                      className={FIELD_CLASS}
                      value={draft.general.theme}
                      onChange={(e) =>
                        update("general", {
                          theme: e.target.value as Settings["general"]["theme"],
                        })
                      }
                    >
                      {THEME_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.labelKey)}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field
                  label={t("settings.general.dateFormat")}
                  hint={t("settings.general.dateFormatHint")}
                >
                  {({ id, describedBy }) => (
                    <select
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={dateFormatChoice(draft.general.dateFormat)}
                      onChange={(e) => update("general", { dateFormat: e.target.value })}
                    >
                      {DATE_FORMAT_OPTIONS.map((format) => (
                        <option key={format} value={format}>
                          {format === DATE_FORMAT_SYSTEM
                            ? t("settings.general.dateFormatSystem", { sample: sample(format) })
                            : t("settings.general.dateFormatPattern", {
                                sample: sample(format),
                                pattern: patternOf(format),
                              })}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field label={t("settings.general.firstDayOfWeek")}>
                  {({ id }) => (
                    <select
                      id={id}
                      className={FIELD_CLASS}
                      value={draft.general.firstDayOfWeek}
                      onChange={(e) =>
                        update("general", {
                          firstDayOfWeek: e.target.value as Settings["general"]["firstDayOfWeek"],
                        })
                      }
                    >
                      {FIRST_DAY_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.labelKey)}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field label={t("settings.general.currency")}>
                  {({ id }) => (
                    <select
                      id={id}
                      className={FIELD_CLASS}
                      value={draft.general.currency}
                      onChange={(e) => update("general", { currency: e.target.value })}
                    >
                      {/* A currency stored by hand that is not in our list
                          is still shown, so it is not silently replaced. */}
                      {!(CURRENCY_OPTIONS as readonly string[]).includes(
                        draft.general.currency,
                      ) && <option value={draft.general.currency}>{draft.general.currency}</option>}
                      {CURRENCY_OPTIONS.map((code) => (
                        <option key={code} value={code}>
                          {t("settings.general.currencyOption", {
                            code,
                            name: currencyNames.of(code) ?? code,
                          })}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
              </>
            )}

            {group === "scheduling" && (
              <>
                <Field label={t("settings.scheduling.newTaskMode")}>
                  {({ id, describedBy }) => (
                    <select
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.scheduling.newTaskMode}
                      onChange={(e) =>
                        update("scheduling", {
                          newTaskMode: e.target.value as Settings["scheduling"]["newTaskMode"],
                        })
                      }
                    >
                      {TASK_MODE_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.labelKey)}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field label={t("settings.scheduling.defaultTaskType")} notYetActive>
                  {({ id, describedBy }) => (
                    <select
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.scheduling.defaultTaskType}
                      onChange={(e) =>
                        update("scheduling", {
                          defaultTaskType: e.target
                            .value as Settings["scheduling"]["defaultTaskType"],
                        })
                      }
                    >
                      {TASK_TYPE_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.labelKey)}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <CheckField
                  label={t("settings.scheduling.effortDriven")}
                  notYetActive
                  hint={t("settings.scheduling.effortDrivenHint")}
                  checked={draft.scheduling.effortDriven}
                  onChange={(effortDriven) => update("scheduling", { effortDriven })}
                />
                <Field label={t("settings.scheduling.hoursPerDay")} notYetActive>
                  {({ id, describedBy }) => (
                    <NumberField
                      id={id}
                      describedBy={describedBy}
                      value={draft.scheduling.hoursPerDay}
                      min={0.5}
                      max={24}
                      step={0.5}
                      onChange={(hoursPerDay) => update("scheduling", { hoursPerDay })}
                    />
                  )}
                </Field>
                <Field label={t("settings.scheduling.hoursPerWeek")} notYetActive>
                  {({ id, describedBy }) => (
                    <NumberField
                      id={id}
                      describedBy={describedBy}
                      value={draft.scheduling.hoursPerWeek}
                      min={1}
                      max={168}
                      step={0.5}
                      onChange={(hoursPerWeek) => update("scheduling", { hoursPerWeek })}
                    />
                  )}
                </Field>
                <Field label={t("settings.scheduling.defaultCalendar")} notYetActive>
                  {({ id, describedBy }) => (
                    <select
                      id={id}
                      aria-describedby={describedBy}
                      className={FIELD_CLASS}
                      value={draft.scheduling.defaultCalendar}
                      onChange={(e) => update("scheduling", { defaultCalendar: e.target.value })}
                    >
                      {CALENDAR_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.labelKey)}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
              </>
            )}

            {group === "files" && (
              <>
                <Field
                  label={t("settings.files.defaultSaveFolder")}
                  notYetActive
                  hint={t("settings.files.defaultSaveFolderHint")}
                >
                  {({ id, describedBy }) => (
                    <div className="flex flex-wrap gap-2">
                      <input
                        id={id}
                        aria-describedby={describedBy}
                        className={`${FIELD_CLASS} flex-1`}
                        value={draft.files.defaultSaveFolder ?? ""}
                        placeholder={t("settings.files.systemChoice")}
                        onChange={(e) =>
                          update("files", {
                            defaultSaveFolder: e.target.value === "" ? null : e.target.value,
                          })
                        }
                      />
                      <button
                        type="button"
                        className={BUTTON_CLASS}
                        disabled={!hasDesktopCore()}
                        onClick={() => void onChooseFolder()}
                      >
                        {t("settings.files.chooseFolder")}
                      </button>
                    </div>
                  )}
                </Field>
                <Field
                  label={t("settings.files.autosaveMinutes")}
                  hint={t("settings.files.autosaveHint")}
                >
                  {({ id, describedBy }) => (
                    <NumberField
                      id={id}
                      describedBy={describedBy}
                      value={draft.files.autosaveMinutes}
                      min={0}
                      max={120}
                      step={1}
                      onChange={(autosaveMinutes) => update("files", { autosaveMinutes })}
                    />
                  )}
                </Field>
                <Field
                  label={t("settings.files.historyRetentionDays")}
                  notYetActive
                  hint={t("settings.files.historyHint")}
                >
                  {({ id, describedBy }) => (
                    <NumberField
                      id={id}
                      describedBy={describedBy}
                      value={draft.files.historyRetentionDays}
                      min={1}
                      max={3650}
                      step={1}
                      onChange={(historyRetentionDays) => update("files", { historyRetentionDays })}
                    />
                  )}
                </Field>
                <Field label={t("settings.files.recentFilesMax")} notYetActive>
                  {({ id, describedBy }) => (
                    <NumberField
                      id={id}
                      describedBy={describedBy}
                      value={draft.files.recentFilesMax}
                      min={0}
                      max={50}
                      step={1}
                      onChange={(recentFilesMax) => update("files", { recentFilesMax })}
                    />
                  )}
                </Field>
              </>
            )}

            {group === "updates" && (
              <>
                <p className="text-xs text-fg-muted">{t("settings.notYetActive")}</p>
                <CheckField
                  label={t("settings.updates.autoUpdate")}
                  hint={t("settings.updates.autoUpdateHint")}
                  checked={draft.updates.autoUpdate}
                  onChange={(autoUpdate) => update("updates", { autoUpdate })}
                />
                <CheckField
                  label={t("settings.updates.crashReports")}
                  hint={t("settings.updates.crashReportsHint")}
                  checked={draft.updates.crashReports}
                  onChange={(crashReports) => update("updates", { crashReports })}
                />
              </>
            )}
          </div>
        </div>

        {/* ---- Error and buttons ---- */}
        {error !== null && (
          <p
            role="alert"
            className="border-t border-danger-line bg-danger-bg px-4 py-2 text-sm text-danger-fg"
          >
            {error}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2 border-t border-line px-4 py-3">
          <button
            type="button"
            className={BUTTON_CLASS}
            disabled={busy}
            onClick={() => void onReset()}
          >
            {t("settings.reset")}
          </button>
          <span className="flex-1" />
          <button type="button" className={BUTTON_CLASS} onClick={closeDialog}>
            {t("settings.cancel")}
          </button>
          <button
            type="submit"
            disabled={busy}
            className={`${BUTTON_CLASS} border-accent bg-accent text-accent-fg hover:bg-accent`}
          >
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
