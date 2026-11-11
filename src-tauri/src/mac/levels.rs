//! Sets volume, display brightness and the keyboard light directly, instead of pressing the
//! media keys. macOS then shows no HUD of its own, and Brink shows one with the exact
//! percentage (no macOS version puts a number in its HUD). Anything that can't be set this way
//! returns `None`, and the caller falls back to pressing the key.

use super::ffi::*;
use crate::engine::Key;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use serde::Serialize;
use std::ffi::c_void;
use std::sync::OnceLock;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Volume,
    Brightness,
    Keyboard,
    /// An external monitor, over DDC/CI.
    Monitor,
    /// Scrubbing what's playing: `level` is the playhead's place 0..1 and `secs` its time, or
    /// `level` is negative and `secs` the seconds jumped (length unknown).
    Seek,
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Hud {
    pub kind: Kind,
    pub level: f32,
    pub muted: bool,
    /// Seconds, for the seek bar (see `Kind::Seek`).
    pub secs: f32,
}

/// One step up or down on the same grid the keys use: 16 steps, or 64 for quarter steps.
pub fn next(cur: f32, up: bool, quarter: bool) -> f32 {
    let n = if quarter { 64.0 } else { 16.0 };
    let at = (cur.clamp(0.0, 1.0) * n).round();
    ((if up { at + 1.0 } else { at - 1.0 }) / n).clamp(0.0, 1.0)
}

/// `direct`: set levels directly (Brink's HUD). `monitors`: DDC/CI for external monitors.
/// With `direct` off only an external monitor is handled here (the keys can't reach it).
/// What became of one step.
pub enum Outcome {
    /// Brink set it: show this.
    Shown(Hud),
    /// Sent to BetterDisplay; the HUD follows when it reports the new level.
    Later,
    /// Not Brink's to set: press the key.
    Key,
}

pub fn step(key: Key, quarter: bool, direct: bool, monitors: bool) -> Outcome {
    let out = match key {
        Key::VolumeUp | Key::VolumeDown => volume(key == Key::VolumeUp, quarter, direct),
        Key::KeyboardUp | Key::KeyboardDown if direct => keyboard(key == Key::KeyboardUp, quarter).map_or(Outcome::Key, Outcome::Shown),
        Key::BrightnessUp | Key::BrightnessDown => brightness(key == Key::BrightnessUp, quarter, direct, monitors),
        _ => Outcome::Key,
    };
    // Which way each change went, so a log from someone's setup shows what happened.
    let route = match &out {
        Outcome::Shown(h) => format!("set directly, {:?} {:.0}%", h.kind, h.level * 100.0),
        Outcome::Later => "sent to BetterDisplay".into(),
        Outcome::Key => "pressed the key".into(),
    };
    log_route(key, &route);
    out
}

/// Logs a route the first time it is taken and whenever it changes, not on every step.
fn log_route(key: Key, route: &str) {
    static LAST: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());
    let what = route.split(',').next().unwrap_or(route);
    let line = format!("{key:?}: {what}");
    let mut last = LAST.lock();
    if *last != line {
        log::info!("brink: level {key:?} {route}");
        *last = line;
    }
}

// ---------- volume ----------

extern "C" {
    fn AudioHardwareServiceSetPropertyData(
        obj: u32,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        obj: u32,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

const SCOPE_OUTPUT: u32 = fourcc(b"outp");

fn volume(up: bool, quarter: bool, direct: bool) -> Outcome {
    let Some(dev) = super::system::default_output() else { return Outcome::Key };
    // A monitor's speakers: BetterDisplay keeps that volume (it answers the keyboard's volume keys
    // for them too). Setting the Mac's own volume for the device as well made two levels.
    if display_audio(dev) && super::betterdisplay::running() {
        super::betterdisplay::step_volume(device_name(dev).as_deref(), up, quarter);
        return Outcome::Later;
    }
    if !direct {
        return Outcome::Key;
    }
    set_volume(dev, up, quarter).map_or(Outcome::Key, Outcome::Shown)
}

/// The output is a display's own audio (DisplayPort or HDMI).
fn display_audio(dev: u32) -> bool {
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"tran"), scope: fourcc(b"glob"), element: 0 };
    let mut t: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(dev, &addr, 0, std::ptr::null(), &mut size, &mut t as *mut u32 as *mut c_void) };
    r == 0 && (t == fourcc(b"dprt") || t == fourcc(b"hdmi"))
}

fn device_name(dev: u32) -> Option<String> {
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"lnam"), scope: fourcc(b"glob"), element: 0 };
    let mut name: CFTypeRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFTypeRef>() as u32;
    let r = unsafe { AudioObjectGetPropertyData(dev, &addr, 0, std::ptr::null(), &mut size, &mut name as *mut CFTypeRef as *mut c_void) };
    if r != 0 || name.is_null() {
        return None;
    }
    let s = unsafe { cf_to_string(name) };
    unsafe { CFRelease(name) };
    s.filter(|n| !n.is_empty())
}

unsafe fn cf_to_string(s: CFTypeRef) -> Option<String> {
    extern "C" {
        fn CFStringGetLength(s: CFTypeRef) -> isize;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut std::ffi::c_char, size: isize, enc: u32) -> bool;
    }
    let size = CFStringGetLength(s) * 4 + 1;
    let mut buf = vec![0 as std::ffi::c_char; size as usize];
    CFStringGetCString(s, buf.as_mut_ptr(), size, kCFStringEncodingUTF8)
        .then(|| std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
}

fn set_volume(dev: u32, up: bool, quarter: bool) -> Option<Hud> {
    let cur = super::system::volume()?;
    let muted = super::system::muted();
    // Like the volume keys: up while muted unmutes and goes on from where it was.
    let new = if up && muted && cur <= 0.0 { next(0.0, true, quarter) } else { next(cur, up, quarter) };
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"vmvc"), scope: SCOPE_OUTPUT, element: 0 };
    let r = unsafe { AudioHardwareServiceSetPropertyData(dev, &addr, 0, std::ptr::null(), 4, &new as *const f32 as *const c_void) };
    if r != 0 {
        return None; // e.g. HDMI or digital output with no volume: the key shows macOS's "can't change"
    }
    let mute = new <= 0.0;
    if mute != muted {
        let addr = AudioObjectPropertyAddress { selector: fourcc(b"mute"), scope: SCOPE_OUTPUT, element: 0 };
        let m: u32 = mute as u32;
        unsafe { AudioObjectSetPropertyData(dev, &addr, 0, std::ptr::null(), 4, &m as *const u32 as *const c_void) };
    }
    Some(Hud { kind: Kind::Volume, level: new, muted: mute, secs: 0.0 })
}

// ---------- displays ----------

extern "C" {
    fn CGGetDisplaysWithPoint(p: CGPoint, max: u32, displays: *mut u32, count: *mut u32) -> i32;
}

/// The display the pointer is on.
pub fn display_under_pointer() -> Option<u32> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        let (mut id, mut n) = (0u32, 0u32);
        (CGGetDisplaysWithPoint(p, 1, &mut id, &mut n) == 0 && n == 1).then_some(id)
    }
}

fn builtin_display() -> Option<u32> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    unsafe {
        if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut n) != 0 {
            return None;
        }
        ids[..n as usize].iter().copied().find(|&d| CGDisplayIsBuiltin(d) != 0)
    }
}

/// The display under the pointer: the built-in one or an Apple display directly, another monitor
/// over DDC/CI (when switched on) or else through the key, for a monitor app to handle.
fn brightness(up: bool, quarter: bool, direct: bool, monitors: bool) -> Outcome {
    match brightness_inner(up, quarter, direct, monitors) {
        Ok(h) => Outcome::Shown(h),
        Err(o) => o,
    }
}

fn brightness_inner(up: bool, quarter: bool, direct: bool, monitors: bool) -> Result<Hud, Outcome> {
    let ds = display();
    let can = |d: u32| ds.can_change.map(|f| unsafe { f(d) }).unwrap_or(false);
    let under = display_under_pointer();
    if let Some(d) = under {
        let builtin = unsafe { CGDisplayIsBuiltin(d) != 0 };
        if !builtin && !can(d) {
            // BetterDisplay first when it runs: it already drives this monitor, and keeps the level
            // its keyboard keys use. Brink's own DDC (beta) only when it doesn't.
            if super::betterdisplay::running() {
                super::betterdisplay::step_brightness(d, up, quarter);
                return Err(Outcome::Later);
            }
            // MonitorControl keeps its own level for the monitor: let it handle the key, or the
            // two drift apart (what BetterDisplay users saw).
            if monitors && !super::betterdisplay::app_running("me.guillaumeb.MonitorControl") {
                #[cfg(target_arch = "aarch64")]
                if let Some(level) = super::ddc::step(d, up, quarter) {
                    return Ok(Hud { kind: Kind::Monitor, level, muted: false, secs: 0.0 });
                }
            }
            // A monitor Brink can't reach: press the key, so a monitor app (MonitorControl)
            // changes the screen you're looking at, not the built-in one.
            return Err(Outcome::Key);
        } else if direct {
            if let Some(h) = set_display(d, up, quarter) {
                return Ok(h);
            }
        }
    }
    if !direct {
        return Err(Outcome::Key);
    }
    builtin_display().and_then(|d| set_display(d, up, quarter)).ok_or(Outcome::Key)
}

fn set_display(d: u32, up: bool, quarter: bool) -> Option<Hud> {
    let ds = display();
    let (get, set) = (ds.get_brightness?, ds.set_brightness?);
    let mut cur = 0f32;
    if unsafe { get(d, &mut cur) } != 0 {
        return None;
    }
    let new = next(cur, up, quarter);
    (unsafe { set(d, new) } == 0).then_some(Hud { kind: Kind::Brightness, level: new, muted: false, secs: 0.0 })
}

// ---------- keyboard light ----------

/// CoreBrightness's client: the same one Control Center's keyboard slider uses. Pressing the
/// illumination keys only moved it one step on some Macs; setting it directly always works.
struct Keyboard {
    client: *mut AnyObject,
    id: u64,
}
unsafe impl Send for Keyboard {}
unsafe impl Sync for Keyboard {}

fn kbd() -> Option<&'static Keyboard> {
    static K: OnceLock<Option<Keyboard>> = OnceLock::new();
    K.get_or_init(|| unsafe {
        let path = std::ffi::CString::new("/System/Library/PrivateFrameworks/CoreBrightness.framework/CoreBrightness").ok()?;
        if dlopen_now(path.as_ptr()).is_null() {
            return None;
        }
        let cls = AnyClass::get(c"KeyboardBrightnessClient")?;
        let client: *mut AnyObject = msg_send![cls, new];
        if client.is_null() {
            return None;
        }
        let ids: *mut AnyObject = msg_send![client, copyKeyboardBacklightIDs];
        if ids.is_null() {
            return None;
        }
        let count: usize = msg_send![ids, count];
        if count == 0 {
            return None;
        }
        let first: *mut AnyObject = msg_send![ids, objectAtIndex: 0usize];
        let id: u64 = msg_send![first, unsignedLongLongValue];
        Some(Keyboard { client, id })
    })
    .as_ref()
}

pub fn keyboard_level() -> Option<f32> {
    let k = kbd()?;
    let v: f32 = unsafe { msg_send![k.client, brightnessForKeyboard: k.id] };
    Some(v.clamp(0.0, 1.0))
}

fn keyboard(up: bool, quarter: bool) -> Option<Hud> {
    let k = kbd()?;
    let new = next(keyboard_level()?, up, quarter);
    let ok: Bool = unsafe { msg_send![k.client, setBrightness: new, forKeyboard: k.id] };
    ok.as_bool().then_some(Hud { kind: Kind::Keyboard, level: new, muted: false, secs: 0.0 })
}

#[cfg(test)]
mod tests {
    use super::next;

    #[test]
    fn steps_land_on_the_key_grid() {
        assert_eq!(next(0.5, true, false), 0.5625);
        assert_eq!(next(0.0, false, false), 0.0);
        assert_eq!(next(1.0, true, false), 1.0);
        // Off the grid (auto brightness): snaps to the nearest step, then moves one.
        assert_eq!(next(0.874, true, false), 0.9375);
        assert_eq!(next(0.5, false, true), 0.484375);
    }
}
