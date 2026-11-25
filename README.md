# Ganttora

Ganttora is a desktop project planner with Gantt charts, for Windows, macOS and Linux. It is in early development.

## Run it from source

You need [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 22 or newer, [pnpm](https://pnpm.io) and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system.

```bash
pnpm install --frozen-lockfile
pnpm tauri dev
```

The first start compiles everything and takes a few minutes.

## Run the checks

```bash
cargo test --workspace
pnpm typecheck
pnpm lint
pnpm test
```

## Where things are

| Folder | Contents |
|---|---|
| `backend/crates/` | Rust core: scheduling engine, documents, file format |
| `backend/src-tauri/` | Desktop app shell (Tauri) |
| `frontend/` | User interface (React, TypeScript) |
| `tests/` | All tests |
| `locales/` | Texts of the user interface |

Projects are saved as `.gtra` files. Ganttora sends no usage data.

## Licence

Copyright (C) 2026 TonyIvanoff

Ganttora is free software under the [GNU General Public License, version 3](LICENSE).
