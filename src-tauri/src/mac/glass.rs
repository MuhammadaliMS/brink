//! Makes the panel window fully clear behind the (transparent) webview. The panel draws its own
//! glass in WebGL, so nothing native sits behind it: an NSGlassEffectView there only showed as a
//! dark rim around the panel's rounded corners.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use std::ffi::c_void;

pub fn apply(ns_window: *mut c_void) {
    if ns_window.is_null() {
        return;
    }
    unsafe {
        let window = ns_window as *mut AnyObject;
        let _: () = msg_send![window, setOpaque: Bool::NO];
        let clear: *mut AnyObject = msg_send![AnyClass::get(c"NSColor").unwrap(), clearColor];
        let _: () = msg_send![window, setBackgroundColor: clear];
    }
}
