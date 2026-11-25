//! macos_quit.rs: asks about unsaved changes when macOS itself ends the app.
//!
//! # What it is for
//! Quit in the Dock icon's menu, logging out and shutting down do not go
//! through our menu. macOS asks the app's *delegate* (a helper object)
//! `applicationShouldTerminate:`. The window library we use (tao) does not
//! answer that question, so macOS would end the app at once, without
//! "Save / Don't save / Cancel". This file adds the missing answer.
//!
//! # Where it fits
//! `lib.rs` calls [`install`] once at start-up, on macOS only. The decision
//! itself is made by `ipc::app::on_exit_requested`, as for every other way
//! to quit.
//!
//! # Main ideas
//! - Objective-C (the language macOS is built on) looks functions up by name
//!   while the program runs, so a function can be *added* to a class later.
//! - Quitting from our own menu does not come through here: it ends the run
//!   loop directly.
//! - This needs `unsafe`: the compiler cannot check calls into Objective-C.
//!   Every use says why it is correct.
//!

use std::sync::OnceLock;

use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, class, msg_send, sel};
use tauri::{AppHandle, Wry};

use crate::ipc::app::on_exit_requested;

/// macOS's "do not end the app" (`NSTerminateCancel`).
const TERMINATE_CANCEL: usize = 0;
/// macOS's "end the app now" (`NSTerminateNow`).
const TERMINATE_NOW: usize = 1;

/// The running app, for [`should_terminate`], which macOS calls without it.
///
/// Beginner note: a `OnceLock` is a box that can be filled exactly once.
static APP: OnceLock<AppHandle<Wry>> = OnceLock::new();

/// Answers macOS's question "may the app end?".
///
/// # Parameters
/// The three values Objective-C passes to every such method: the object,
/// the method's name, and the sender. None is needed.
///
/// # Returns
/// [`TERMINATE_CANCEL`] if there are unsaved changes (the window then asks
/// the user), otherwise [`TERMINATE_NOW`].
extern "C-unwind" fn should_terminate(
    _this: &AnyObject,
    _name: Sel,
    _sender: *mut AnyObject,
) -> usize {
    let mut answer = TERMINATE_NOW;
    if let Some(app) = APP.get() {
        on_exit_requested(app, || answer = TERMINATE_CANCEL);
    }
    answer
}

/// Adds [`should_terminate`] to the app's delegate. Call once, at start-up.
///
/// # Parameters
/// - `app`: the running app.
///
/// # Errors / edge cases
/// If it is not on the main thread, there is no delegate, or the delegate
/// already answers the question, nothing is changed and a line is logged:
/// the app works, only this safety net is missing.
pub fn install(app: &AppHandle<Wry>) {
    if MainThreadMarker::new().is_none() || APP.set(app.clone()).is_err() {
        eprintln!("ganttora: quit from the Dock is not guarded (wrong thread or called twice)");
        return;
    }
    // SAFETY: we are on the main thread, where AppKit may be used. These are
    // plain getters that return an object or nil.
    let (ns_app, delegate): (*mut AnyObject, *mut AnyObject) = unsafe {
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        (ns_app, msg_send![ns_app, delegate])
    };
    // SAFETY: the delegate is nil (gives `None`) or a live object, which the
    // window library keeps for as long as the app runs.
    let Some(object) = (unsafe { delegate.as_ref() }) else {
        eprintln!("ganttora: quit from the Dock is not guarded (no delegate)");
        return;
    };
    let delegate_class: *const AnyClass = object.class();
    // SAFETY: "Q@:@" says: returns an unsigned integer, takes the object,
    // the name and one object. That is exactly `should_terminate`, so the
    // pointer may be treated as a general method pointer (`Imp`).
    // `class_addMethod` never replaces an existing method.
    let added = unsafe {
        let method = std::mem::transmute::<
            extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize,
            Imp,
        >(should_terminate);
        class_addMethod(
            delegate_class.cast_mut(),
            sel!(applicationShouldTerminate:),
            method,
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("ganttora: quit from the Dock is not guarded (method exists already)");
        return;
    }
    // AppKit may remember which questions a delegate answers from the moment
    // it was set. Setting the same one again makes it look anew.
    // SAFETY: main thread; the same live object as before.
    unsafe {
        let _: () = msg_send![ns_app, setDelegate: delegate];
    }
}
