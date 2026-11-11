//! Corner actions that aren't media keys: microphone mute, Mission Control and friends,
//! Spotlight, screenshot, lock screen, display sleep.

use super::ffi::*;
use crate::settings::CornerAction;
use std::ffi::{c_char, c_void, CString};

extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn AudioObjectSetPropertyData(
        obj: u32,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_LAZY: i32 = 1;

/// A function from a system framework, looked up at run time (private ones may move).
fn lookup(lib: &str, name: &str) -> *mut c_void {
    let (Ok(lib), Ok(name)) = (CString::new(lib), CString::new(name)) else { return std::ptr::null_mut() };
    unsafe {
        let h = dlopen(lib.as_ptr(), RTLD_LAZY);
        if h.is_null() {
            return std::ptr::null_mut();
        }
        dlsym(h, name.as_ptr())
    }
}

pub fn run(a: CornerAction) {
    log::info!("brink: corner {a:?}");
    std::thread::spawn(move || match a {
        CornerAction::MuteMic => toggle_mic(),
        CornerAction::MissionControl => dock("com.apple.expose.awake", "Mission Control"),
        CornerAction::AppWindows => dock("com.apple.expose.front.awake", "Mission Control"),
        CornerAction::ShowDesktop => dock("com.apple.showdesktop.awake", "Mission Control"),
        CornerAction::Spotlight => key_with(49, kCGEventFlagMaskCommand),
        CornerAction::Screenshot => {
            let _ = std::process::Command::new("/usr/bin/open").args(["-b", "com.apple.screenshot.launcher"]).status();
        }
        CornerAction::LockScreen => lock(),
        CornerAction::SleepDisplay => {
            let _ = std::process::Command::new("/usr/bin/pmset").arg("displaysleepnow").status();
        }
        // Media keys go through the engine's own key path.
        CornerAction::Off | CornerAction::Mute | CornerAction::PlayPause | CornerAction::NextTrack => {}
    });
}

/// The Dock runs Mission Control, App Exposé and Show Desktop; it listens for these.
fn dock(notification: &str, fallback_app: &str) {
    type SendFn = unsafe extern "C" fn(CFStringRef, *const c_void);
    let f = lookup("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices", "CoreDockSendNotification");
    if f.is_null() {
        let _ = std::process::Command::new("/usr/bin/open").args(["-a", fallback_app]).status();
        return;
    }
    unsafe {
        let send: SendFn = std::mem::transmute(f);
        let name = cfstr(notification);
        send(name, std::ptr::null());
        CFRelease(name);
    }
}

fn key_with(code: u16, flags: u64) {
    unsafe {
        for down in [true, false] {
            let e = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if e.is_null() {
                return;
            }
            CGEventSetFlags(e, flags);
            CGEventPost(kCGHIDEventTap, e);
            CFRelease(e as CFTypeRef);
        }
    }
}

fn lock() {
    type LockFn = unsafe extern "C" fn() -> i32;
    let f = lookup("/System/Library/PrivateFrameworks/login.framework/Versions/Current/login", "SACLockScreenImmediate");
    if f.is_null() {
        // Sleeping the display locks it when a password is required after sleep.
        let _ = std::process::Command::new("/usr/bin/pmset").arg("displaysleepnow").status();
        return;
    }
    unsafe {
        let lock: LockFn = std::mem::transmute(f);
        lock();
    }
}

// ---------- microphone ----------

const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_INPUT: u32 = fourcc(b"inpt");

/// Input level before Brink muted a mic that has no mute switch of its own.
static SAVED_LEVEL: parking_lot::Mutex<Option<(u32, f32)>> = parking_lot::Mutex::new(None);

fn default_input() -> Option<u32> {
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"dIn "), scope: SCOPE_GLOBAL, element: 0 };
    let mut id: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size, &mut id as *mut u32 as *mut c_void) };
    (r == 0 && id != 0).then_some(id)
}

fn get_u32(dev: u32, addr: &AudioObjectPropertyAddress) -> Option<u32> {
    let mut v: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(dev, addr, 0, std::ptr::null(), &mut size, &mut v as *mut u32 as *mut c_void) };
    (r == 0).then_some(v)
}

fn get_f32(dev: u32, addr: &AudioObjectPropertyAddress) -> Option<f32> {
    let mut v: f32 = 0.0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(dev, addr, 0, std::ptr::null(), &mut size, &mut v as *mut f32 as *mut c_void) };
    (r == 0).then_some(v)
}

fn set_raw<T>(dev: u32, addr: &AudioObjectPropertyAddress, v: &T) -> bool {
    unsafe {
        AudioObjectSetPropertyData(dev, addr, 0, std::ptr::null(), std::mem::size_of::<T>() as u32, v as *const T as *const c_void) == 0
    }
}

/// Mutes the default microphone, or unmutes it. Uses the device's mute switch when it has
/// one; otherwise turns its input level to zero and back.
fn toggle_mic() {
    let Some(dev) = default_input() else { return };
    let mute = AudioObjectPropertyAddress { selector: fourcc(b"mute"), scope: SCOPE_INPUT, element: 0 };
    if unsafe { AudioObjectHasProperty(dev, &mute) } {
        if let Some(on) = get_u32(dev, &mute) {
            if set_raw(dev, &mute, &(if on != 0 { 0u32 } else { 1u32 })) {
                log::info!("brink: microphone {}", if on != 0 { "on" } else { "muted" });
                return;
            }
        }
    }
    // No mute switch: the input volume, on the main element or on each channel.
    let elements: Vec<u32> = [0u32, 1, 2]
        .into_iter()
        .filter(|e| unsafe {
            AudioObjectHasProperty(dev, &AudioObjectPropertyAddress { selector: fourcc(b"volm"), scope: SCOPE_INPUT, element: *e })
        })
        .collect();
    let Some(&first) = elements.first() else { return };
    let addr = |e: u32| AudioObjectPropertyAddress { selector: fourcc(b"volm"), scope: SCOPE_INPUT, element: e };
    let now = get_f32(dev, &addr(first)).unwrap_or(0.0);
    let mut saved = SAVED_LEVEL.lock();
    let level = if now <= 0.001 {
        // Muted (by Brink or anyone): back to where it was, or a sensible level.
        match saved.take() {
            Some((d, l)) if d == dev && l > 0.001 => l,
            _ => 0.75,
        }
    } else {
        *saved = Some((dev, now));
        0.0
    };
    for e in elements {
        set_raw(dev, &addr(e), &level);
    }
    log::info!("brink: microphone {}", if level > 0.0 { "on" } else { "muted" });
}
