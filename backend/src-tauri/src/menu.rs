//! menu.rs: the native menu bar.
//!
//! # What it is for
//! Builds the operating system's menu bar from a description sent by the
//! UI, keeps "greyed out" and "ticked" up to date, and tells the UI which
//! entry the user chose.
//!
//! # Where it fits
//! - The description comes from `frontend/features/toolbars/menuModel.ts`,
//!   already in the user's language. So this file contains no texts.
//! - `lib.rs` registers the commands and calls [`on_menu_event`].
//! - The chosen entry's ID goes to the UI as the event [`MENU_EVENT`]
//!   (`frontend/features/toolbars/useNativeMenu.ts`).
//!
//! # Main ideas
//! - **A tree.** A [`MenuNode`] is an entry, a line, something the system
//!   provides (Copy, Hide…) or a submenu with more nodes inside.
//! - **Quit is ours.** The system's own Quit on macOS ends the app without
//!   asking. Our entry [`QUIT_ID`] uses `app.exit`, which Tauri announces as
//!   `ExitRequested` first, so the unsaved-changes question is asked
//!   (`ipc/app.rs`).
//!

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::{AppHandle, Emitter, Runtime, State, Wry};

use crate::ipc::IpcError;
use crate::ipc::app::MAIN_WINDOW;

/// Event sent to the UI with the chosen entry's ID. Not UI text.
pub const MENU_EVENT: &str = "app://menu";

/// ID of Quit / Exit. Must equal `QUIT_ID` in `menuModel.ts`.
pub const QUIT_ID: &str = "quit";

/// Error code: the menu could not be built or changed.
pub const ERR_MENU: &str = "menuFailed";

/// Entries the operating system provides. Must match `MenuPredefined` in
/// `frontend/ipc/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Predefined {
    /// Undo typing.
    Undo,
    /// Redo typing.
    Redo,
    /// Cut text.
    Cut,
    /// Copy text.
    Copy,
    /// Paste text.
    Paste,
    /// Select all text.
    SelectAll,
    /// The macOS Services menu.
    Services,
    /// Hide the app (macOS).
    Hide,
    /// Hide other apps (macOS).
    HideOthers,
    /// Show all apps (macOS).
    ShowAll,
    /// Minimize the window.
    Minimize,
    /// Zoom (maximize) the window.
    ZoomWindow,
    /// Close the window (asks about unsaved changes like the close button).
    CloseWindow,
    /// Full screen on or off.
    Fullscreen,
}

/// One part of the menu. Must match `MenuNode` in `frontend/ipc/types.ts`.
///
/// # Example
/// ```
/// use ganttora_lib::menu::MenuNode;
/// let json = r#"{ "kind": "submenu", "label": "File", "items": [
///     { "kind": "item", "id": "fileSave", "label": "Save", "enabled": true,
///       "accelerator": "CmdOrCtrl+S" },
///     { "kind": "separator" },
///     { "kind": "predefined", "name": "hideOthers", "label": "Hide others" }
/// ] }"#;
/// let node: MenuNode = serde_json::from_str(json).unwrap();
/// let MenuNode::Submenu { items, .. } = node else { panic!() };
/// assert_eq!(items.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MenuNode {
    /// An entry of ours.
    Item {
        /// Sent back to the UI when chosen.
        id: String,
        /// The text.
        label: String,
        /// `false` = greyed out.
        enabled: bool,
        /// `Some` = an on/off entry with a tick.
        #[serde(default)]
        checked: Option<bool>,
        /// Keyboard shortcut, e.g. `"CmdOrCtrl+S"`.
        #[serde(default)]
        accelerator: Option<String>,
    },
    /// A dividing line.
    Separator,
    /// An entry the system provides.
    Predefined {
        /// Which one.
        name: Predefined,
        /// The text.
        label: String,
    },
    /// A menu, or a menu inside a menu.
    Submenu {
        /// The text.
        label: String,
        /// What is inside.
        items: Vec<MenuNode>,
    },
}

/// New state of one entry. Must match `MenuStateUpdate` in `types.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct MenuStateUpdate {
    /// The entry.
    pub id: String,
    /// `false` = greyed out.
    pub enabled: bool,
    /// The tick, for on/off entries.
    pub checked: Option<bool>,
}

/// A built entry, kept so its state can change later.
enum Entry {
    /// A plain entry.
    Plain(MenuItem<Wry>),
    /// An entry with a tick.
    Check(CheckMenuItem<Wry>),
}

/// The entries of the current menu, by ID.
///
/// # Beginner note
/// The same action may appear twice (About in the app menu and in Help), so
/// each ID has a list of entries.
#[derive(Default)]
pub struct MenuState(Mutex<HashMap<String, Vec<Entry>>>);

/// Turns a Tauri error into the coded error the UI understands.
fn menu_error(error: tauri::Error) -> IpcError {
    IpcError::new(ERR_MENU, error.to_string())
}

/// Builds one entry of ours and remembers it.
///
/// An accelerator the system cannot read is left out: an entry without a
/// shortcut is better than no menu.
fn build_item(
    app: &AppHandle<Wry>,
    node: &MenuNode,
    entries: &mut HashMap<String, Vec<Entry>>,
) -> tauri::Result<Option<Box<dyn IsMenuItem<Wry>>>> {
    let MenuNode::Item {
        id,
        label,
        enabled,
        checked,
        accelerator,
    } = node
    else {
        return Ok(None);
    };
    let keys = accelerator.as_deref();
    let no_keys = None::<&str>;
    let list = entries.entry(id.clone()).or_default();
    Ok(Some(match checked {
        Some(on) => {
            let item = CheckMenuItem::with_id(app, id, label, *enabled, *on, keys)
                .or_else(|_| CheckMenuItem::with_id(app, id, label, *enabled, *on, no_keys))?;
            list.push(Entry::Check(item.clone()));
            Box::new(item)
        }
        None => {
            let item = MenuItem::with_id(app, id, label, *enabled, keys)
                .or_else(|_| MenuItem::with_id(app, id, label, *enabled, no_keys))?;
            list.push(Entry::Plain(item.clone()));
            Box::new(item)
        }
    }))
}

/// Builds a system entry.
fn build_predefined(
    app: &AppHandle<Wry>,
    name: Predefined,
    label: &str,
) -> tauri::Result<PredefinedMenuItem<Wry>> {
    let text = Some(label);
    match name {
        Predefined::Undo => PredefinedMenuItem::undo(app, text),
        Predefined::Redo => PredefinedMenuItem::redo(app, text),
        Predefined::Cut => PredefinedMenuItem::cut(app, text),
        Predefined::Copy => PredefinedMenuItem::copy(app, text),
        Predefined::Paste => PredefinedMenuItem::paste(app, text),
        Predefined::SelectAll => PredefinedMenuItem::select_all(app, text),
        Predefined::Services => PredefinedMenuItem::services(app, text),
        Predefined::Hide => PredefinedMenuItem::hide(app, text),
        Predefined::HideOthers => PredefinedMenuItem::hide_others(app, text),
        Predefined::ShowAll => PredefinedMenuItem::show_all(app, text),
        Predefined::Minimize => PredefinedMenuItem::minimize(app, text),
        Predefined::ZoomWindow => PredefinedMenuItem::maximize(app, text),
        Predefined::CloseWindow => PredefinedMenuItem::close_window(app, text),
        Predefined::Fullscreen => PredefinedMenuItem::fullscreen(app, text),
    }
}

/// Builds one node, with everything inside it.
///
/// # Beginner note
/// The function calls *itself* for a submenu's contents. That is called
/// *recursion* and is the natural way to walk a tree. `Box<dyn IsMenuItem>`
/// means "some kind of menu item, kept behind a pointer", because the kinds
/// have different sizes.
fn build_node(
    app: &AppHandle<Wry>,
    node: &MenuNode,
    entries: &mut HashMap<String, Vec<Entry>>,
) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    Ok(match node {
        MenuNode::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        MenuNode::Predefined { name, label } => Box::new(build_predefined(app, *name, label)?),
        MenuNode::Submenu { label, items } => {
            let submenu = Submenu::new(app, label, true)?;
            for item in items {
                submenu.append(build_node(app, item, entries)?.as_ref())?;
            }
            Box::new(submenu)
        }
        MenuNode::Item { .. } => match build_item(app, node, entries)? {
            Some(item) => item,
            // Cannot happen (it *is* an item); a line is harmless.
            None => Box::new(PredefinedMenuItem::separator(app)?),
        },
    })
}

/// IPC: replaces the whole menu bar.
///
/// # Parameters
/// - `menu`: the top-level menus (only submenus are used), left to right.
///
/// # Errors
/// `menuFailed` if the system refuses; the old menu then stays.
#[tauri::command]
pub fn menu_set(
    app: AppHandle<Wry>,
    state: State<'_, MenuState>,
    menu: Vec<MenuNode>,
) -> Result<(), IpcError> {
    let mut entries = HashMap::new();
    let bar = Menu::new(&app).map_err(menu_error)?;
    for node in &menu {
        if matches!(node, MenuNode::Submenu { .. }) {
            let submenu = build_node(&app, node, &mut entries).map_err(menu_error)?;
            bar.append(submenu.as_ref()).map_err(menu_error)?;
        }
    }
    app.set_menu(bar).map_err(menu_error)?;
    // A lock broken by a crash elsewhere is still usable here: we replace
    // its whole content.
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = entries;
    Ok(())
}

/// IPC: changes "greyed out" and "ticked" of some entries.
///
/// # Parameters
/// - `states`: the changes; unknown IDs are ignored.
///
/// # Errors
/// `menuFailed` if the system refuses.
#[tauri::command]
pub fn menu_update(
    state: State<'_, MenuState>,
    states: Vec<MenuStateUpdate>,
) -> Result<(), IpcError> {
    let entries = state.0.lock().unwrap_or_else(|e| e.into_inner());
    for update in states {
        for entry in entries.get(&update.id).into_iter().flatten() {
            match entry {
                Entry::Plain(item) => item.set_enabled(update.enabled),
                Entry::Check(item) => item
                    .set_enabled(update.enabled)
                    .and_then(|()| item.set_checked(update.checked.unwrap_or(false))),
            }
            .map_err(menu_error)?;
        }
    }
    Ok(())
}

/// Called by Tauri when the user chooses an entry of ours.
///
/// Quit asks the app to end (which asks about unsaved changes, see
/// `ipc::app::on_exit_requested`); every other entry is carried out by the UI.
pub fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    let id = event.id().as_ref();
    if id == QUIT_ID {
        app.exit(0);
    } else if let Err(error) = app.emit_to(MAIN_WINDOW, MENU_EVENT, id) {
        eprintln!("ganttora: could not forward the menu choice: {error}");
    }
}
