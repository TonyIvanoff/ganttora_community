/**
 * eslint.config.js: rules for ESLint, the "spell checker" for our TypeScript.
 *
 * ESLint reads the code without running it and warns about likely bugs
 * (for example a variable that is never used, or a React hook called in the
 * wrong place). `pnpm lint` runs it on `frontend/` and `tests/`.
 *
 * This is a "flat config": an array of config objects. Later objects add to
 * or override earlier ones. Each object can limit itself to some files.
 */

import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  // 1. Folders ESLint must never look at: build output, dependencies,
  //    Rust code, and the generated IPC types (made by a script, not people).
  {
    ignores: [
      "dist/**",
      "target/**",
      "node_modules/**",
      "backend/**",
      "docs/**",
      "website/**",
      "frontend/ipc/types.ts",
      // Temporary copies of the project made by development tools.
      ".claude/**",
    ],
  },

  // 2. Base rules for all JavaScript and TypeScript files.
  js.configs.recommended,
  ...tseslint.configs.recommended,

  // 3. React rules for the UI code.
  {
    files: ["frontend/**/*.{ts,tsx}", "tests/frontend/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      // The UI runs in a browser engine, so `window`, `document`… exist.
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      // Hooks (useState, useEffect…) must follow React's "rules of hooks".
      ...reactHooks.configs.recommended.rules,
      // Fast refresh (instant reload while editing) only works when a file
      // exports React components; warn if it exports other things too.
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },

  // 4. Config files run in Node.js, not in the browser.
  {
    files: ["*.{js,ts}"],
    languageOptions: { globals: globals.node },
  },
);
