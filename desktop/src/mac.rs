//! macOS specifics: load the Chromium framework from the app bundle, install an
//! NSApplication subclass that speaks CEF's event protocol, and add the standard menus
//! (Cmd+Q, and the Edit menu that routes Cmd+C/V/X/A/Z to text fields).
// AppKit methods are safe or unsafe depending on the objc2-app-kit version; wrap them all.
#![allow(unused_unsafe)]
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use cef::library_loader::LibraryLoader;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol, Sel};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem, NSResponder};
use objc2_foundation::{NSObject, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

/// Loads "Chromium Embedded Framework.framework" relative to this executable. Keep the
/// returned loader alive for the whole process.
pub fn load_framework(helper: bool) -> LibraryLoader {
    let exe = std::env::current_exe().expect("current executable path");
    let loader = LibraryLoader::new(&exe, helper);
    assert!(loader.load(), "cannot load the Chromium Embedded Framework");
    loader
}

static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);

define_class!(
    #[unsafe(super(NSApplication, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CardboardApplication"]
    struct CardboardApplication;

    unsafe impl NSObjectProtocol for CardboardApplication {}

    unsafe impl CrAppProtocol for CardboardApplication {
        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling(&self) -> Bool {
            Bool::new(HANDLING_SEND_EVENT.load(Ordering::Relaxed))
        }
    }

    unsafe impl CrAppControlProtocol for CardboardApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling(&self, value: Bool) {
            HANDLING_SEND_EVENT.store(value.as_bool(), Ordering::Relaxed);
        }
    }

    unsafe impl CefAppProtocol for CardboardApplication {}
);

/// Must run before CEF initializes: makes NSApp an instance of our subclass.
pub fn setup_application() {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app: Retained<NSApplication> =
        unsafe { msg_send![CardboardApplication::class(), sharedApplication] };
    let menubar = unsafe { NSMenu::new(mtm) };
    unsafe {
        menubar.addItem(&submenu(
            mtm,
            "Cardboard",
            &[("Quit Cardboard", sel!(terminate:), "q", false)],
        ))
    };
    unsafe {
        menubar.addItem(&submenu(
            mtm,
            "Edit",
            &[
                ("Undo", sel!(undo:), "z", false),
                ("Redo", sel!(redo:), "z", true),
                ("Cut", sel!(cut:), "x", false),
                ("Copy", sel!(copy:), "c", false),
                ("Paste", sel!(paste:), "v", false),
                ("Select All", sel!(selectAll:), "a", false),
            ],
        ))
    };
    unsafe { app.setMainMenu(Some(&menubar)) };
}

fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[(&str, Sel, &str, bool)],
) -> Retained<NSMenuItem> {
    let menu = unsafe { NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title)) };
    for &(label, action, key, shift) in items {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(label),
                Some(action),
                &NSString::from_str(key),
            )
        };
        if shift {
            unsafe {
                item.setKeyEquivalentModifierMask(
                    NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
                )
            };
        }
        unsafe { menu.addItem(&item) };
    }
    let holder = unsafe { NSMenuItem::new(mtm) };
    unsafe { holder.setSubmenu(Some(&menu)) };
    holder
}
