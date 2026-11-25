/**
 * features/settings/options.ts: default settings for the browser preview,
 * and the choices the Settings dialog offers.
 *
 * What it is for:
 *   - {@link BROWSER_DEFAULT_SETTINGS}: the real defaults live in Rust
 *     (`Settings::default()` in `backend/src-tauri/src/settings.rs`). In a
 *     plain browser (`pnpm dev`, no Rust side) the UI still needs *some*
 *     settings to start with, so this is a copy of those defaults. The
 *     desktop app never uses it once Rust has answered.
 *   - The lists of choices for drop-down menus (date formats, currencies…),
 *     each with the translation key of its label.
 *
 * Where it fits:
 *   Used by `settingsStore.ts` (initial value) and `SettingsDialog.tsx`
 *   (the drop-downs).
 *
 * Main ideas:
 *   - Labels are translation keys, never English text.
 *   - `as const` keeps each key as an exact string, so `t(option.labelKey)`
 *     is checked against `locales/en.json` at compile time.
 */

import { DEFAULT_COLUMN_WIDTHS } from "@frontend/features/grid/columnWidths";
import { DATE_FORMAT_ISO, DATE_FORMAT_SYSTEM, DATE_PATTERNS } from "@frontend/i18n/format";
import type { FirstDayOfWeek, Settings, TaskMode, TaskType, Theme } from "@frontend/ipc/types";

/**
 * Copy of Rust's `Settings::default()`, used only in the browser preview.
 * When the Rust defaults change, change this too (same commit).
 */
export const BROWSER_DEFAULT_SETTINGS: Settings = {
  version: 1,
  user: { displayName: "", initials: "", email: "" },
  general: {
    language: "en",
    theme: "system",
    dateFormat: DATE_FORMAT_SYSTEM,
    firstDayOfWeek: "system",
    currency: "EUR",
    uiZoomPercent: 100,
  },
  scheduling: {
    newTaskMode: "auto",
    defaultTaskType: "fixedUnits",
    effortDriven: false,
    hoursPerDay: 8,
    hoursPerWeek: 40,
    defaultCalendar: "standard",
  },
  files: {
    defaultSaveFolder: null,
    autosaveMinutes: 5,
    historyRetentionDays: 90,
    recentFilesMax: 10,
  },
  updates: { autoUpdate: true, crashReports: false },
  // A copy, so changing the settings never changes the defaults object.
  grid: { columnWidths: { ...DEFAULT_COLUMN_WIDTHS } },
  // Empty = the predefined toolbars (features/toolbars/layout.ts).
  toolbars: { layout: [] },
  view: { timescale: "days", zoomPercent: 100 },
};

/** Theme choices. */
export const THEME_OPTIONS = [
  { value: "system", labelKey: "settings.general.themeSystem" },
  { value: "light", labelKey: "settings.general.themeLight" },
  { value: "dark", labelKey: "settings.general.themeDark" },
] as const satisfies readonly { value: Theme; labelKey: string }[];

/**
 * Date format choices: patterns without a language. The dialog shows each
 * with a sample date.
 */
export const DATE_FORMAT_OPTIONS = [DATE_FORMAT_SYSTEM, DATE_FORMAT_ISO, ...DATE_PATTERNS] as const;

/** First-day-of-week choices. */
export const FIRST_DAY_OPTIONS = [
  { value: "system", labelKey: "settings.general.firstDaySystem" },
  { value: "monday", labelKey: "settings.general.firstDayMonday" },
  { value: "sunday", labelKey: "settings.general.firstDaySunday" },
  { value: "saturday", labelKey: "settings.general.firstDaySaturday" },
] as const satisfies readonly { value: FirstDayOfWeek; labelKey: string }[];

/**
 * Currency choices (ISO 4217 codes). The dialog shows each with its symbol
 * and its name in the user's language, both from `Intl`, so no currency
 * names need translating by hand.
 */
export const CURRENCY_OPTIONS = [
  "EUR",
  "USD",
  "GBP",
  "CHF",
  "PLN",
  "CZK",
  "SEK",
  "NOK",
  "DKK",
  "RUB",
  "CNY",
  "JPY",
  "BRL",
  "CAD",
  "AUD",
] as const;

/** New-task mode choices. */
export const TASK_MODE_OPTIONS = [
  { value: "auto", labelKey: "settings.scheduling.modeAuto" },
  { value: "manual", labelKey: "settings.scheduling.modeManual" },
] as const satisfies readonly { value: TaskMode; labelKey: string }[];

/** Task type choices. */
export const TASK_TYPE_OPTIONS = [
  { value: "fixedUnits", labelKey: "settings.scheduling.typeFixedUnits" },
  { value: "fixedDuration", labelKey: "settings.scheduling.typeFixedDuration" },
  { value: "fixedWork", labelKey: "settings.scheduling.typeFixedWork" },
] as const satisfies readonly { value: TaskType; labelKey: string }[];

/**
 * Calendar choices for new projects. Only the built-in "Standard" calendar
 * is offered: calendars made in the calendar editor
 * (`features/calendars/CalendarDialog.tsx`) belong to one project file,
 * while this setting is for *new* projects. Offering user calendars here
 * needs app-wide calendar templates, which come later.
 */
export const CALENDAR_OPTIONS = [
  { value: "standard", labelKey: "settings.scheduling.calendarStandard" },
] as const satisfies readonly { value: string; labelKey: string }[];
