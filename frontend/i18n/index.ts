/**
 * i18n/index.ts: sets up translations (i18next) for the whole user interface.
 *
 * What it is for:
 *   No text the user sees is written directly in the components. Components
 *   ask for a *key*, e.g. `t("toolbar.zoomDays")`, and this module looks up
 *   the text for the current language in `locales/<language>.json`.
 *   It also switches the language while the app runs ({@link setLanguage}),
 *   with no restart.
 *
 * Where it fits:
 *   Imported once by `frontend/main.tsx`, before React starts. Components
 *   then use the `useTranslation()` hook from react-i18next to get `t`.
 *   `features/settings/useApplySettings.ts` calls {@link setLanguage}
 *   whenever the language in the settings changes.
 *
 * Main ideas:
 *   - "i18n" is short for "internationalization": i, then 18 letters, then n.
 *   - Messages use the ICU message format, e.g.
 *     `{count, plural, one {# task} other {# tasks}}`. ICU knows the plural
 *     rules of every language (Lithuanian, Polish and Russian have three or
 *     four plural forms; English has two).
 *   - **English is always bundled** (it is the source and the fallback: a
 *     key missing in German shows the English text, never a blank label).
 *   - **Other languages are loaded on demand.** Vite's `import.meta.glob`
 *     finds every `locales/*.json` at build time and turns each into a
 *     separate small file that is fetched only when that language is
 *     chosen. A new language file is picked up automatically, with no code
 *     change. (In M1 only `en.json` exists.)
 */

import i18next from "i18next";
import ICU from "i18next-icu";
import { initReactI18next } from "react-i18next";
import en from "@locales/en.json";

/**
 * The language used when a key is missing in the current language, and the
 * one that is always bundled.
 */
export const FALLBACK_LANGUAGE = "en";

/**
 * Every language Ganttora plans to ship (decision D13), with
 * its name written *in that language* ("endonym"), as the language list in
 * Settings shows it.
 *
 * These names are deliberately not in `locales/en.json`: "Deutsch" is
 * "Deutsch" in every language, so translating it would be wrong.
 */
export const SUPPORTED_LANGUAGES = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "fr", name: "Français" },
  { code: "es", name: "Español" },
  { code: "pt", name: "Português" },
  { code: "lt", name: "Lietuvių" },
  { code: "pl", name: "Polski" },
  { code: "ru", name: "Русский" },
  { code: "zh-CN", name: "中文（简体）" },
] as const;

/**
 * Teaches TypeScript the shape of our English file, so `t("toolbar.zoomDays")`
 * is checked: a misspelled key becomes a compile error instead of a missing
 * label at run time.
 *
 * Beginner note: `declare module "i18next"` does not create code. It *adds*
 * information to the types of the i18next library ("module augmentation").
 */
declare module "i18next" {
  interface CustomTypeOptions {
    /** The namespace used when none is given. We use one: "translation". */
    defaultNS: "translation";
    /** The structure of every key, taken from en.json. */
    resources: { translation: typeof en };
  }
}

/** The shape of one translation file once loaded: nested objects of strings. */
type TranslationFile = Record<string, unknown>;

/**
 * Loaders for the non-English language files, found by Vite at build time.
 *
 * `import.meta.glob(pattern)` gives an object: file path → a function that
 * loads that file. The `!` pattern leaves out `en.json`, which is bundled
 * above. `import: "default"` gives us the JSON object itself.
 *
 * Beginner note: this is a *build-time* feature. Vite replaces the call
 * with a list of `import()` calls, one per matching file. Nothing scans the
 * disk while the app runs.
 */
const otherLanguageLoaders = import.meta.glob<TranslationFile>(
  ["@locales/*.json", "!@locales/en.json"],
  { import: "default" },
);

/**
 * Language code → loader, e.g. `"de"` → loads `locales/de.json`.
 * The code is the file name without ".json".
 */
const loaderByLanguage = new Map<string, () => Promise<TranslationFile>>(
  Object.entries(otherLanguageLoaders).flatMap(([path, load]) => {
    const match = /([^/\\]+)\.json$/.exec(path);
    return match?.[1] !== undefined ? [[match[1], load] as const] : [];
  }),
);

/**
 * Lists the languages that have a translation file in this build.
 *
 * @returns Language codes, English first, e.g. `["en"]` in M1 or
 *   `["en", "de", "ru"]` once those files exist.
 *
 * @example
 * availableLanguages().includes("de"); // false until locales/de.json exists
 */
export function availableLanguages(): string[] {
  return [FALLBACK_LANGUAGE, ...loaderByLanguage.keys()];
}

/**
 * Starts i18next. The promise it returns is ignored on purpose: with the
 * English resources bundled in memory, initialisation finishes immediately
 * (no files to download), so the first render already has all texts.
 *
 * Beginner note: `.use(...)` adds plugins. ICU adds the plural/number syntax;
 * `initReactI18next` connects i18next to React, so components re-render when
 * the language changes.
 */
void i18next
  .use(ICU)
  .use(initReactI18next)
  .init({
    resources: { en: { translation: en } },
    lng: FALLBACK_LANGUAGE,
    fallbackLng: FALLBACK_LANGUAGE,
    interpolation: {
      // React already escapes text it puts on screen, so i18next must not
      // escape it a second time (that would show "&amp;" instead of "&").
      escapeValue: false,
    },
  });

/**
 * Switches the user interface to another language, immediately.
 *
 * @param code - Language code from the settings, e.g. "de" or "zh-CN".
 * @returns A promise of the language actually used: `code` if its file
 *   exists, otherwise "en" (the fallback).
 *
 * Edge cases:
 *   - A language without a file (or a broken file that fails to load)
 *     falls back to English instead of failing, so the UI always has text.
 *   - Each file is loaded only once; switching back is instant.
 *
 * @example
 * await setLanguage("de"); // every `t(...)` now returns German text
 *
 * Beginner note: after `changeLanguage`, react-i18next re-renders every
 * component that called `useTranslation()`, so all labels change at once.
 */
export async function setLanguage(code: string): Promise<string> {
  let language = code;
  if (language !== FALLBACK_LANGUAGE && !i18next.hasResourceBundle(language, "translation")) {
    const load = loaderByLanguage.get(language);
    try {
      if (load === undefined) throw new Error(`no translation file for "${language}"`);
      i18next.addResourceBundle(language, "translation", await load());
    } catch (error: unknown) {
      // Not fatal: English is always there.
      console.warn("language not available, using English", error);
      language = FALLBACK_LANGUAGE;
    }
  }
  if (i18next.language !== language) await i18next.changeLanguage(language);
  // The `lang` attribute tells the browser (and screen readers) which
  // language the page is in; it also picks the right fonts for Chinese.
  if (typeof document !== "undefined") document.documentElement.lang = language;
  return language;
}

/** The configured i18next instance, for code outside React components. */
export default i18next;
