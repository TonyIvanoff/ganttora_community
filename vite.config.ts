/**
 * vite.config.ts: settings for Vite, the tool that builds and serves the UI.
 *
 * Where it fits:
 * - `pnpm dev` starts Vite's development server. It serves the React app from
 *   `frontend/` at http://localhost:1420 and reloads the page instantly when a
 *   file changes. `pnpm tauri dev` starts this server and opens the desktop
 *   window pointed at it (see `backend/src-tauri/tauri.conf.json`, `devUrl`).
 * - `pnpm build` turns `frontend/` into plain HTML, CSS and JavaScript files in
 *   `dist/`. Tauri packs `dist/` into the installer (`frontendDist`).
 * - `pnpm test` runs Vitest, which reuses this same file, so tests understand
 *   the same aliases and plugins as the app.
 *
 * Main ideas:
 * - `root` is `frontend/`, because that is where `index.html` lives.
 * - Aliases `@frontend` and `@locales` must match `tsconfig.json` "paths".
 */

import { resolve } from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

/**
 * Absolute path of the repository root (the folder that holds this file).
 *
 * Beginner note: `import.meta.dirname` is the folder of the current file.
 * We build every other path from it, so the config works no matter which
 * folder you start `pnpm` from.
 */
const repoRoot = import.meta.dirname;

/**
 * The development server port. Tauri's `devUrl` uses the same number.
 * 1420 is the port the official Tauri templates use.
 */
const DEV_PORT = 1420;

/**
 * The exported Vite (and Vitest) configuration.
 *
 * Beginner note: `defineConfig` does nothing at run time; it only tells the
 * editor the type of the object, so it can autocomplete option names and
 * warn about typos.
 */
export default defineConfig({
  // Where index.html lives. All other relative paths in this file start here.
  root: resolve(repoRoot, "frontend"),

  // Plugins extend Vite: React (JSX + fast refresh) and Tailwind CSS v4.
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@frontend": resolve(repoRoot, "frontend"),
      "@locales": resolve(repoRoot, "locales"),
    },
  },

  // Keep Rust's compiler messages visible: by default Vite clears the screen.
  clearScreen: false,

  server: {
    port: DEV_PORT,
    // Fail instead of silently choosing another port: Tauri would then open
    // the window at the wrong address.
    strictPort: true,
    // The dev server must be allowed to read `locales/`, which is outside
    // `root`.
    fs: { allow: [repoRoot] },
    watch: {
      // Rust files are rebuilt by Tauri, not Vite; watching them would waste
      // effort and could trigger useless page reloads.
      ignored: ["**/backend/**", "**/target/**"],
    },
  },

  // Environment variables starting with these prefixes are readable in the
  // UI via `import.meta.env`. TAURI_ENV_* are set by the Tauri CLI.
  envPrefix: ["VITE_", "TAURI_ENV_"],

  build: {
    // Output goes to `<repo>/dist`, which tauri.conf.json `frontendDist` names.
    outDir: resolve(repoRoot, "dist"),
    emptyOutDir: true,
    // Every WebView Tauri 2 supports understands ES2022 JavaScript.
    target: "es2022",
    // Source maps help debugging; only make them for debug builds.
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },

  // Vitest (unit tests). Tests live in `tests/frontend/` (decision D17),
  // not next to the source code.
  test: {
    root: repoRoot,
    include: ["tests/{frontend,scripts}/**/*.{test,spec}.{ts,tsx}"],
    environment: "node",
  },
});
