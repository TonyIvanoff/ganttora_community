//! main.rs: the entry point of the Ganttora desktop program.
//!
//! # What it is for
//! When you start Ganttora, the operating system runs the `main` function
//! below. It does only one thing: call [`ganttora_lib::run`], which lives in
//! `lib.rs`. Keeping `main.rs` this small is the standard Tauri layout: all
//! real code is in the library, where it can also be tested and reused.
//!

// On Windows, a program is either a "console" program (opens a black
// terminal window) or a "windows" program (no terminal). Release builds
// should not show a terminal, so we pick "windows" when debug assertions
// are off (= release build). The `#![...]` form is an *attribute* that
// applies to the whole file. On macOS and Linux this line has no effect.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Starts the app. Never returns while the app is running.
///
/// # Beginner note
/// Every Rust program starts at a function called `main`.
fn main() {
    ganttora_lib::run();
}
