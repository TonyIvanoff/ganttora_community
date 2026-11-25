//! build.rs: the Tauri build script.
//!
//! # What it is for
//! Cargo runs a file called `build.rs` *before* compiling the crate itself.
//! Tauri uses it to read `tauri.conf.json` and the `capabilities/` folder,
//! check them, and embed them (plus the app icons) into the program.
//!
//! # Where it fits
//! Runs automatically on `cargo build`, `pnpm tauri dev` and
//! `pnpm tauri build`. You never call it yourself.
//!

/// Entry point of the build script.
///
/// # Errors
/// Panics (stops the build with a message) if `tauri.conf.json` or a
/// capability file is invalid, so mistakes are caught at compile time.
fn main() {
    tauri_build::build()
}
