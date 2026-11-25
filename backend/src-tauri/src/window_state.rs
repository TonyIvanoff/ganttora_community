//! window_state.rs: remembers the window's position and size.
//!
//! # What it is for
//! Ganttora opens where it was closed: same monitor, same place, same size.
//!
//! # Where it fits
//! `lib.rs` calls [`restore`] at start-up, before the window is shown, then
//! [`watch`] to follow the window, and [`WindowMemory::flush`] at the end.
//! The result is kept in `window.json`, next to `settings.json`. It is not
//! part of the settings, because the UI saves those as a whole.
//!
//! # Main ideas
//! - **Units.** Monitors can have different pixel densities. macOS measures
//!   its desktop in *points* (logical pixels), Windows and Linux in real
//!   pixels. We store the system's own unit; a position in the other unit
//!   would be wrong as soon as two monitors differ.
//! - **A monitor can be gone.** A position that is on no monitor is not used.
//! - **Saving.** The file is written soon after a move, not only at the end,
//!   so the position also survives a crash.
//!

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Runtime, WebviewWindow, WindowEvent};

/// How long to wait after a move before writing the file.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How much of the window's top edge must be on a monitor, so that the user
/// can still grab the title bar.
const GRAB_SIZE: f64 = 100.0;

/// A rectangle on the desktop, in the system's own unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge (negative on a monitor to the left of the main one).
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// What `window.json` holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    /// Position of the window's outside and size of its inside.
    pub rect: Rect,
    /// Filled the monitor (Windows and Linux).
    #[serde(default)]
    pub maximized: bool,
}

/// Fits a saved rectangle to the monitors that exist now.
///
/// # Parameters
/// - `saved`: the saved rectangle.
/// - `monitors`: the monitors.
///
/// # Returns
/// The rectangle, not larger than its monitor; `None` if its top edge is on
/// no monitor, or if the numbers are unusable.
///
/// # Example
/// ```
/// use ganttora_lib::window_state::{Rect, fit};
/// let rect = |x: f64, y: f64, width: f64, height: f64| Rect { x, y, width, height };
/// let small = rect(0.0, 0.0, 1512.0, 982.0);
/// let big = rect(1512.0, -200.0, 3440.0, 1440.0);
/// let saved = rect(1600.0, -100.0, 3000.0, 1200.0);
/// // Both monitors there: exactly where it was.
/// assert_eq!(fit(saved, &[small, big]), Some(saved));
/// // The big monitor is gone: not used.
/// assert_eq!(fit(saved, &[small]), None);
/// // Larger than its monitor: made smaller.
/// let wide = fit(rect(10.0, 10.0, 5000.0, 800.0), &[small]).unwrap();
/// assert_eq!(wide.width, 1512.0);
/// // Nonsense from a damaged file.
/// assert_eq!(fit(rect(0.0, 0.0, -5.0, 10.0), &[small]), None);
/// ```
pub fn fit(saved: Rect, monitors: &[Rect]) -> Option<Rect> {
    if saved.width < 1.0 || saved.height < 1.0 {
        return None;
    }
    // How many units of the top edge are on a monitor.
    let visible = |m: &Rect| {
        let left = saved.x.max(m.x);
        let right = (saved.x + saved.width).min(m.x + m.width);
        right - left
    };
    let monitor = monitors
        .iter()
        // 1 unit of tolerance: positions are rounded.
        .filter(|m| saved.y >= m.y - 1.0 && saved.y < m.y + m.height - GRAB_SIZE / 2.0)
        .filter(|m| visible(m) >= GRAB_SIZE)
        // `total_cmp` orders decimal numbers: the one with most of the window.
        .max_by(|a, b| visible(a).total_cmp(&visible(b)))?;
    Some(Rect {
        width: saved.width.min(monitor.width),
        height: saved.height.min(monitor.height),
        ..saved
    })
}

/// Reads `window.json`.
///
/// # Returns
/// The state; `None` if there is no file or it is unusable (the window then
/// opens in the middle of the main monitor).
pub fn load(path: &Path) -> Option<WindowState> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Writes `window.json`: first a temporary file, then a rename, so it is
/// never half-written.
///
/// # Errors
/// The folder or file cannot be written.
///
/// # Example
/// ```
/// use ganttora_lib::window_state::{Rect, WindowState, load, save};
/// let dir = std::env::temp_dir().join(format!("ganttora-doc-{}", uuid::Uuid::new_v4()));
/// let path = dir.join("window.json");
/// let state = WindowState {
///     rect: Rect { x: -5.5, y: 0.0, width: 900.0, height: 700.0 },
///     maximized: true,
/// };
/// assert_eq!(load(&path), None);
/// save(&path, &state).unwrap();
/// assert_eq!(load(&path), Some(state));
/// std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn save(path: &Path, state: &WindowState) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let json = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(&temp, path)
}

/// Turns real pixels into the system's desktop unit.
///
/// # Parameters
/// - `pixels`: real pixels.
/// - `scale`: real pixels per point of that window or monitor (Retina: 2).
fn unit(pixels: f64, scale: f64) -> f64 {
    if cfg!(target_os = "macos") {
        pixels / scale
    } else {
        pixels
    }
}

/// The monitors, in the system's desktop unit.
fn monitors<R: Runtime>(window: &WebviewWindow<R>) -> Vec<Rect> {
    let found = window.available_monitors().unwrap_or_default();
    found
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            Rect {
                x: unit(f64::from(m.position().x), scale),
                y: unit(f64::from(m.position().y), scale),
                width: unit(f64::from(m.size().width), scale),
                height: unit(f64::from(m.size().height), scale),
            }
        })
        .collect()
}

/// Where the window is now.
///
/// # Returns
/// `None` while it is minimized or full screen: that place is not its own.
fn current<R: Runtime>(
    window: &WebviewWindow<R>,
    last: Option<WindowState>,
) -> Option<WindowState> {
    if window.is_minimized().ok()? || window.is_fullscreen().ok()? {
        return None;
    }
    let scale = window.scale_factor().ok()?;
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let rect = Rect {
        x: unit(f64::from(position.x), scale),
        y: unit(f64::from(position.y), scale),
        width: unit(f64::from(size.width), scale),
        height: unit(f64::from(size.height), scale),
    };
    // On macOS a window that fills the screen is an ordinary one. Elsewhere
    // "maximized" is a state of its own; the size before it is kept.
    let maximized = !cfg!(target_os = "macos") && window.is_maximized().ok()?;
    let before = last.map(|state| state.rect);
    match (maximized, before) {
        (true, Some(before)) => {
            // The place decides on which monitor it is maximized again.
            let centre = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let monitor = monitors(window).into_iter().find(|m| {
                (m.x..m.x + m.width).contains(&centre.0)
                    && (m.y..m.y + m.height).contains(&centre.1)
            });
            let same = monitor.is_some_and(|m| fit(before, &[m]).is_some());
            let rect = match monitor {
                Some(m) if !same => Rect {
                    x: m.x,
                    y: m.y,
                    ..before
                },
                _ => before,
            };
            Some(WindowState { rect, maximized })
        }
        _ => Some(WindowState { rect, maximized }),
    }
}

/// Puts the window where it was. Does nothing if that is not possible.
///
/// # Parameters
/// - `window`: the main window, still hidden.
/// - `saved`: from [`load`].
pub fn restore<R: Runtime>(window: &WebviewWindow<R>, saved: Option<WindowState>) {
    let Some(saved) = saved else { return };
    let Some(rect) = fit(saved.rect, &monitors(window)) else {
        return;
    };
    // Position first: the size is then measured on the right monitor.
    let done = if cfg!(target_os = "macos") {
        window
            .set_position(tauri::LogicalPosition::new(rect.x, rect.y))
            .and_then(|()| window.set_size(tauri::LogicalSize::new(rect.width, rect.height)))
            // macOS keeps the lower left corner when the size changes.
            .and_then(|()| window.set_position(tauri::LogicalPosition::new(rect.x, rect.y)))
    } else {
        // `as` drops the (always zero) decimals.
        window
            .set_position(tauri::PhysicalPosition::new(rect.x as i32, rect.y as i32))
            .and_then(|()| {
                window.set_size(tauri::PhysicalSize::new(
                    rect.width as u32,
                    rect.height as u32,
                ))
            })
    };
    if let Err(error) = done {
        eprintln!("ganttora: the window's position could not be restored: {error}");
    }
    if saved.maximized {
        let _ = window.maximize();
    }
}

/// The last known position, and the file it is written to.
pub struct WindowMemory {
    /// `window.json`; `None` without a config folder.
    path: Option<PathBuf>,
    /// The last known position.
    last: Mutex<Option<WindowState>>,
    /// Something is not written yet.
    dirty: AtomicBool,
}

impl WindowMemory {
    /// Creates the memory.
    ///
    /// # Parameters
    /// - `path`: where to write, or `None` to remember nothing.
    /// - `last`: from [`load`].
    pub fn new(path: Option<PathBuf>, last: Option<WindowState>) -> Self {
        WindowMemory {
            path,
            last: Mutex::new(last),
            dirty: AtomicBool::new(false),
        }
    }

    /// Writes the position, if it changed since the last time.
    pub fn flush(&self) {
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        // Holding the lock while writing: two writers never mix.
        let Ok(last) = self.last.lock() else { return };
        if let (Some(path), Some(state)) = (&self.path, &*last)
            && let Err(error) = save(path, state)
        {
            eprintln!("ganttora: the window's position was not saved: {error}");
        }
    }
}

/// Follows the window and saves its position shortly after each change.
///
/// # Parameters
/// - `window`: the main window.
/// - `memory`: where to keep it.
///
/// # Beginner note
/// An `Arc` is a value with several owners; here the event handler, the
/// saving thread and `lib.rs`.
pub fn watch<R: Runtime>(window: &WebviewWindow<R>, memory: Arc<WindowMemory>) {
    let watched = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
            return;
        }
        {
            let Ok(mut last) = memory.last.lock() else {
                return;
            };
            match current(&watched, *last) {
                Some(state) if *last != Some(state) => *last = Some(state),
                _ => return,
            }
        }
        // Dragging makes many events; one thread at a time waits and writes.
        if !memory.dirty.swap(true, Ordering::SeqCst) {
            let memory = Arc::clone(&memory);
            thread::spawn(move || {
                thread::sleep(SAVE_DELAY);
                memory.flush();
            });
        }
    });
}
