/**
 * components/ui/DateField.tsx: a text field for a day, with a calendar.
 *
 * What it is for:
 *   The web view's own date field always uses the operating system's
 *   format. This one follows Settings → General → Date format
 *   and First day of the week.
 *
 * Where it fits:
 *   Every place where the user enters a day: the grid's Start and Finish
 *   cells, Task Information, Add blocker, and Calendars.
 *
 * Main ideas:
 *   - The value is always "YYYY-MM-DD" ("" = empty, {@link INVALID_DAY} =
 *     the text is not a date); only the text shown follows the setting.
 *   - The focus stays in the text field; the calendar is for the mouse.
 *     Keys: ↑ / ↓ one day, Page Up / Page Down one month, Alt+↓ opens the
 *     calendar, Escape closes it, Enter formats what was typed.
 *   - The calendar is a *popover*: the browser draws it above everything,
 *     also above dialogs and outside scrolling boxes.
 */

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type Ref,
  type SyntheticEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import {
  addDays,
  addMonths,
  firstWeekdayOf,
  formatDay,
  INVALID_DAY,
  isDay,
  monthGrid,
  parseDay,
} from "@frontend/i18n/dateInput";
import { systemLocale } from "@frontend/i18n/format";

/** Props of {@link DateField}. */
export interface DateFieldProps {
  /** The day: "YYYY-MM-DD", "" or {@link INVALID_DAY}. */
  value: string;
  /** Called with the new value on every change. */
  onChange: (value: string) => void;
  /** Called after a click on a day in the calendar (after `onChange`). */
  onPick?: (value: string) => void;
  /** Id of the text field, for its label. */
  id?: string;
  /** Classes of the text field. */
  className?: string;
  /** Classes of the box around field and button (its width). */
  boxClassName?: string;
  /** Greyed out. */
  disabled?: boolean;
  /** Marks the field as wrong. */
  invalid?: boolean;
  /** Accessible name, if there is no label element. */
  label?: string;
  /** Id of a hint. */
  describedBy?: string;
  /** Access to the text field. */
  inputRef?: Ref<HTMLInputElement>;
  /** Called for keys the field does not use itself. */
  onKeyDown?: (event: KeyboardEvent<HTMLInputElement>) => void;
  /** Called when the field loses the focus. */
  onBlur?: () => void;
}

/** Space between field and calendar, in pixels. */
const GAP_PX = 2;

/** Keeps an event away from the grid row or label around the calendar. */
function stop(event: SyntheticEvent): void {
  event.stopPropagation();
}

/**
 * Today on the user's computer.
 *
 * @returns "YYYY-MM-DD" (the local day, not UTC).
 */
function today(): string {
  const now = new Date();
  return new Date(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()))
    .toISOString()
    .slice(0, 10);
}

/**
 * The date field.
 *
 * @param props - See {@link DateFieldProps}.
 * @returns The text field with its calendar button.
 *
 * @example
 * <DateField value={deadline} onChange={setDeadline} label={t("taskInfo.deadline")} />
 */
export function DateField(props: DateFieldProps) {
  const { value, onChange, onPick, disabled = false, onKeyDown, onBlur, inputRef } = props;
  const { t } = useTranslation();
  const { dateLocale } = useFormatLocale();
  const boxRef = useRef<HTMLSpanElement>(null);
  // The month shown in the calendar (any of its days), or null = closed.
  const [month, setMonth] = useState<string | null>(null);
  // What the user typed, and the value it stood for. If the value changes
  // from outside (e.g. "Clear"), the typed text is out of date.
  const [typed, setTyped] = useState<{ text: string; value: string } | null>(null);
  const shown = typed !== null && typed.value === value ? typed.text : formatDay(value, dateLocale);

  /** Reads the typed text and reports the value. */
  function type(text: string): void {
    const next = text.trim() === "" ? "" : (parseDay(text, dateLocale) ?? INVALID_DAY);
    setTyped({ text, value: next });
    if (isDay(next) && month !== null) setMonth(next);
    onChange(next);
  }

  /** Sets a day. */
  function set(day: string): void {
    setTyped(null);
    if (month !== null) setMonth(day);
    onChange(day);
  }

  /** Opens the calendar at the field's day. */
  function open(): void {
    if (!disabled) setMonth(isDay(value) ? value : today());
  }

  /** The keys of the file header; all others go to the owner. */
  function handleKey(event: KeyboardEvent<HTMLInputElement>): void {
    const from = isDay(value) ? value : today();
    if (event.key === "Escape" && month !== null) setMonth(null);
    else if (event.key === "ArrowDown" && event.altKey) open();
    else if (event.key === "ArrowUp") set(addDays(from, 1));
    else if (event.key === "ArrowDown") set(addDays(from, -1));
    else if (event.key === "PageUp") set(addMonths(from, 1));
    else if (event.key === "PageDown") set(addMonths(from, -1));
    else if (event.key === "Enter" && onKeyDown === undefined && typed !== null && isDay(value)) {
      // In a dialog the first Enter writes the date in the setting's format
      // ("20261115" → "15.11.2026"); in the grid Enter saves the cell.
      setTyped(null);
      setMonth(null);
    } else {
      onKeyDown?.(event);
      return;
    }
    // Handled here: a dialog must not close, the grid must not move.
    event.preventDefault();
    event.stopPropagation();
  }

  return (
    <span ref={boxRef} className={`relative inline-flex ${props.boxClassName ?? ""}`}>
      <input
        ref={inputRef}
        type="text"
        data-date-field=""
        autoComplete="off"
        id={props.id}
        className={`w-full min-w-0 pr-7 ${props.className ?? ""}`}
        // The setting's pattern, e.g. "2027-12-31" for year-month-day.
        placeholder={formatDay("2027-12-31", dateLocale)}
        value={shown}
        disabled={disabled}
        aria-label={props.label}
        aria-describedby={props.describedBy}
        aria-invalid={props.invalid === true || value === INVALID_DAY}
        onChange={(e) => type(e.target.value)}
        onClick={open}
        onKeyDown={handleKey}
        onBlur={() => {
          setMonth(null);
          // "24.3.27" becomes "24.03.2027"; a wrong text stays to be corrected.
          if (isDay(value)) setTyped(null);
          onBlur?.();
        }}
      />
      <button
        type="button"
        // Not in the Tab order: Alt+↓ opens the calendar from the keyboard.
        tabIndex={-1}
        disabled={disabled}
        aria-label={t("dateField.open")}
        className="absolute inset-y-0 right-0 flex w-6 items-center justify-center text-fg-muted hover:text-fg disabled:opacity-50"
        // Keeps the focus where it is.
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
        onDoubleClick={stop}
        onClick={(e) => {
          e.stopPropagation();
          boxRef.current?.querySelector("input")?.focus();
          if (month === null) open();
          else setMonth(null);
        }}
      >
        <svg
          aria-hidden="true"
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
        >
          <path d="M2 3h12v11H2z M2 6.5h12 M5 1.5v3 M11 1.5v3" />
        </svg>
      </button>
      {month !== null && (
        <MonthPopup
          month={month}
          selected={value}
          anchor={boxRef}
          onMonth={setMonth}
          onClose={() => setMonth(null)}
          onPick={(day) => {
            setTyped(null);
            setMonth(null);
            onChange(day);
            onPick?.(day);
          }}
        />
      )}
    </span>
  );
}

/** Props of {@link MonthPopup}. */
interface MonthPopupProps {
  /** Any day of the month to show. */
  month: string;
  /** The chosen day (or not a day). */
  selected: string;
  /** The box of the field; the calendar opens below it. */
  anchor: { current: HTMLElement | null };
  /** Called to show another month. */
  onMonth: (month: string) => void;
  /** Called to close. */
  onClose: () => void;
  /** Called with the clicked day. */
  onPick: (day: string) => void;
}

/**
 * The calendar of one month.
 *
 * @param props - See {@link MonthPopupProps}.
 * @returns The popover.
 *
 * Beginner note: `useLayoutEffect` runs before the screen is drawn, so the
 * calendar never appears in a wrong place first.
 */
function MonthPopup({ month, selected, anchor, onMonth, onClose, onPick }: MonthPopupProps) {
  const { t, i18n } = useTranslation();
  const firstDay = useSettingsStore((s) => s.settings.general.firstDayOfWeek);
  const ref = useRef<HTMLDivElement>(null);
  const days = monthGrid(month, firstWeekdayOf(firstDay, systemLocale()));
  const now = today();

  // Show above everything, below the field (above it if there is no room).
  useLayoutEffect(() => {
    const el = ref.current;
    const box = anchor.current?.getBoundingClientRect();
    if (el === null || box === undefined) return;
    // Not every web view knows popovers; then it is an ordinary fixed box.
    (el as Partial<Pick<HTMLElement, "showPopover">>).showPopover?.();
    const { offsetWidth: width, offsetHeight: height } = el;
    const below = box.bottom + GAP_PX;
    const top = below + height <= window.innerHeight ? below : box.top - GAP_PX - height;
    el.style.top = `${Math.max(0, top)}px`;
    el.style.left = `${Math.max(0, Math.min(box.left, window.innerWidth - width))}px`;
  }, [anchor]);

  // Close when something else is pressed, or the field moves away.
  useEffect(() => {
    function onPress(event: Event): void {
      if (event.target instanceof Node && anchor.current?.contains(event.target) === true) return;
      onClose();
    }
    window.addEventListener("pointerdown", onPress, true);
    window.addEventListener("scroll", onPress, true);
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPress, true);
      window.removeEventListener("scroll", onPress, true);
      window.removeEventListener("resize", onClose);
    };
  }, [anchor, onClose]);

  /** Formats a day in the UI language. */
  const write = (day: string, options: Intl.DateTimeFormatOptions): string =>
    new Intl.DateTimeFormat(i18n.language, { ...options, timeZone: "UTC" }).format(
      Date.parse(`${day}T00:00:00Z`),
    );
  const stepClass = "rounded px-2 py-0.5 hover:bg-app";

  return (
    <div
      ref={ref}
      popover="manual"
      role="dialog"
      aria-label={t("dateField.calendar")}
      className="fixed z-50 m-0 rounded border border-line bg-surface p-2 text-sm font-normal text-fg shadow-xl"
      style={{ inset: "auto" }}
      // The focus stays in the text field; the row below feels nothing.
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={stop}
      onDoubleClick={stop}
    >
      <div className="mb-1 flex items-center justify-between">
        <button
          type="button"
          tabIndex={-1}
          className={stepClass}
          aria-label={t("dateField.previous")}
          onClick={() => onMonth(addMonths(month, -1))}
        >
          ‹
        </button>
        <span className="font-semibold">{write(month, { month: "long", year: "numeric" })}</span>
        <button
          type="button"
          tabIndex={-1}
          className={stepClass}
          aria-label={t("dateField.next")}
          onClick={() => onMonth(addMonths(month, 1))}
        >
          ›
        </button>
      </div>
      <div className="grid grid-cols-7 text-center">
        {days.slice(0, 7).map((day) => (
          <span key={day} className="px-1 text-xs text-fg-muted">
            {write(day, { weekday: "short" })}
          </span>
        ))}
        {days.map((day) => (
          <button
            key={day}
            type="button"
            tabIndex={-1}
            data-day={day}
            aria-label={write(day, { dateStyle: "long" })}
            aria-pressed={day === selected}
            className={`h-7 w-8 rounded hover:bg-app aria-pressed:bg-accent aria-pressed:text-accent-fg ${
              day.slice(0, 7) === month.slice(0, 7) ? "" : "text-fg-muted opacity-60"
            } ${day === now ? "border border-accent font-semibold" : ""}`}
            onClick={() => onPick(day)}
          >
            {Number(day.slice(8))}
          </button>
        ))}
      </div>
      <button
        type="button"
        tabIndex={-1}
        className="mt-1 w-full rounded border border-line py-0.5 hover:bg-app"
        onClick={() => onPick(now)}
      >
        {t("dateField.today")}
      </button>
    </div>
  );
}
