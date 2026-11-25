/**
 * main.tsx: the starting point of the user interface.
 *
 * What it is for:
 *   The first TypeScript file the page runs (index.html loads it). It loads
 *   the styles and translations, then tells React to draw `<App />` inside
 *   the `<div id="root">` of index.html.
 *
 * Where it fits:
 *   index.html → main.tsx → App.tsx → shell/AppShell.tsx → features…
 */

// Importing a CSS file makes Vite add it to the page.
import "./styles/globals.css";
// Importing the i18n module runs its set-up code (translations ready
// before the first render).
import "./i18n";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

const rootElement = document.getElementById("root");
if (rootElement === null) {
  // Cannot happen with our index.html; a clear message helps if it ever does.
  throw new Error('index.html has no element with id="root"');
}

/**
 * Beginner note: `StrictMode` is a development helper. It runs some
 * component code twice on purpose, to reveal bugs such as effects that do
 * not clean up after themselves. It has no effect in release builds.
 */
createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
