//! Everything Brink does to the system: media keys, haptics, pointer freeze, click guard.
#![allow(non_upper_case_globals)]

use super::ffi::*;
use crate::engine::Key;
use crate::settings::Haptic;
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------- media keys ----------

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}
unsafe impl Encode for NSPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
struct OpaqueCGEvent([u8; 0]);
unsafe impl RefEncode for OpaqueCGEvent {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

// <IOKit/hidsystem/ev_keymap.h>
const NX_KEYTYPE_SOUND_UP: isize = 0;
const NX_KEYTYPE_SOUND_DOWN: isize = 1;
const NX_KEYTYPE_BRIGHTNESS_UP: isize = 2;
const NX_KEYTYPE_BRIGHTNESS_DOWN: isize = 3;
const NX_KEYTYPE_MUTE: isize = 7;
const NX_KEYTYPE_PLAY: isize = 16;
const NX_KEYTYPE_NEXT: isize = 17;
const NX_KEYTYPE_PREVIOUS: isize = 18;
const NX_KEYTYPE_ILLUMINATION_UP: isize = 21;
const NX_KEYTYPE_ILLUMINATION_DOWN: isize = 22;

const NS_EVENT_TYPE_SYSTEM_DEFINED: usize = 14;
const NS_SHIFT: usize = 1 << 17;
const NS_OPTION: usize = 1 << 19;

/// Posts the same system-defined event the keyboard's media keys send, so macOS shows
/// its own volume / brightness HUD. Shift-Option gives the native quarter step.
pub fn post_key(key: Key, quarter: bool) {
    let code = match key {
        Key::VolumeUp => NX_KEYTYPE_SOUND_UP,
        Key::VolumeDown => NX_KEYTYPE_SOUND_DOWN,
        Key::BrightnessUp => NX_KEYTYPE_BRIGHTNESS_UP,
        Key::BrightnessDown => NX_KEYTYPE_BRIGHTNESS_DOWN,
        Key::Mute => NX_KEYTYPE_MUTE,
        Key::PlayPause => NX_KEYTYPE_PLAY,
        Key::Next => NX_KEYTYPE_NEXT,
        Key::Previous => NX_KEYTYPE_PREVIOUS,
        Key::KeyboardUp => NX_KEYTYPE_ILLUMINATION_UP,
        Key::KeyboardDown => NX_KEYTYPE_ILLUMINATION_DOWN,
    };
    let flags = if quarter { NS_SHIFT | NS_OPTION } else { 0 };
    for down in [true, false] {
        post_system_defined(code, down, flags);
    }
}

fn post_system_defined(code: isize, down: bool, flags: usize) {
    let state: isize = if down { 0xa } else { 0xb };
    let data1 = (code << 16) | (state << 8);
    // Modifier flags ride both in the NSEvent and (for down events) the 0xa00 marker.
    let mod_flags = flags | if down { 0xa00 } else { 0xb00 };
    autoreleasepool(|_| unsafe {
        let ev: *mut AnyObject = msg_send![
            class!(NSEvent),
            otherEventWithType: NS_EVENT_TYPE_SYSTEM_DEFINED,
            location: NSPoint { x: 0.0, y: 0.0 },
            modifierFlags: mod_flags,
            timestamp: 0.0f64,
            windowNumber: 0isize,
            context: std::ptr::null_mut::<AnyObject>(),
            subtype: 8i16,
            data1: data1,
            data2: -1isize
        ];
        if ev.is_null() {
            return;
        }
        let cg: *mut OpaqueCGEvent = msg_send![ev, CGEvent];
        if !cg.is_null() {
            CGEventPost(kCGHIDEventTap, cg as CGEventRef);
        }
    });
}

// ---------- haptics ----------

struct Actuator(CFTypeRef);
unsafe impl Send for Actuator {}

static ACTUATORS: Mutex<Option<HashMap<u64, Actuator>>> = Mutex::new(None);
static LAST_DEVICE: AtomicU64 = AtomicU64::new(0);

pub fn set_active_device(id: u64) {
    LAST_DEVICE.store(id, Ordering::Relaxed);
}

/// Fires the Force Touch actuator of the trackpad the finger is on.
pub fn haptic(strength: Haptic) {
    let id = match strength {
        Haptic::Off => return,
        Haptic::Light => 3,
        Haptic::Medium => 4,
        Haptic::Strong => 6,
    };
    let Some(mt) = multitouch() else { return };
    let (Some(create), Some(open), Some(actuate)) = (mt.actuator_create, mt.actuator_open, mt.actuator_actuate) else {
        return;
    };
    let device = LAST_DEVICE.load(Ordering::Relaxed);
    if device == 0 {
        return;
    }
    let mut guard = ACTUATORS.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    let act = map.entry(device).or_insert_with(|| unsafe {
        let a = create(device);
        if !a.is_null() {
            open(a);
        }
        Actuator(a)
    });
    if !act.0.is_null() {
        unsafe {
            actuate(act.0, id, 0, 0.0, 2.0);
        }
    }
}

pub fn drop_actuators() {
    if let Some(map) = ACTUATORS.lock().take() {
        for (_, a) in map {
            if !a.0.is_null() {
                unsafe { CFRelease(a.0) };
            }
        }
    }
}

// ---------- pointer freeze + click guard ----------

static FROZEN: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static FREEZE_AT: Mutex<CGPoint> = Mutex::new(CGPoint { x: 0.0, y: 0.0 });
/// Unix time in ms before which clicks are swallowed.
static SUPPRESS_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static DROPPED_DOWN: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// When the last real (not suppressed) mouse-down came in.
static LAST_DOWN_MS: AtomicU64 = AtomicU64::new(0);

/// Whether a mouse button went down in the last `secs` seconds.
pub fn clicked_within(secs: f64) -> bool {
    now_ms().saturating_sub(LAST_DOWN_MS.load(Ordering::Relaxed)) <= (secs * 1000.0) as u64
}

pub fn suppress_clicks_for(secs: f64) {
    let until = now_ms() + (secs * 1000.0) as u64;
    SUPPRESS_UNTIL_MS.fetch_max(until, Ordering::Relaxed);
}

fn allow_background_cursor_control() {
    let sk = skylight();
    if let (Some(conn), Some(set)) = (sk.default_connection, sk.set_connection_property) {
        unsafe {
            let cid = conn();
            let key = cfstr("SetsCursorInBackground");
            set(cid, cid, key, kCFBooleanTrue);
            CFRelease(key);
        }
    }
}

pub fn freeze(on: bool) {
    if FROZEN.swap(on, Ordering::SeqCst) == on {
        return;
    }
    unsafe {
        if on {
            let ev = CGEventCreate(std::ptr::null());
            if !ev.is_null() {
                *FREEZE_AT.lock() = CGEventGetLocation(ev);
                CFRelease(ev as CFTypeRef);
            }
            CGAssociateMouseAndMouseCursorPosition(0);
        } else {
            CGAssociateMouseAndMouseCursorPosition(1);
            show_pointer();
        }
    }
}

/// Hide the pointer once a slide is really happening (not on every edge touch).
pub fn hide_pointer() {
    if FROZEN.load(Ordering::SeqCst) && !HIDDEN.swap(true, Ordering::SeqCst) {
        unsafe { CGDisplayHideCursor(0) };
    }
}

fn show_pointer() {
    if HIDDEN.swap(false, Ordering::SeqCst) {
        unsafe { CGDisplayShowCursor(0) };
    }
}

static TAP: AtomicU64 = AtomicU64::new(0);

/// While a window gesture is under way, scrolling and pinch/swipe events are held back so the
/// window doesn't also scroll. Momentum scrolling keeps coming after the fingers lift, so the
/// hold lasts a little longer than the gesture.
static HOLD: AtomicBool = AtomicBool::new(false);
static HOLD_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
const SCROLL_AND_GESTURES: [u32; 8] = [22, 18, 19, 20, 29, 30, 31, 32];

/// `on`: fingers landed in a gesture zone. Off: they lifted; the momentum scrolling that
/// follows a lift is still held back for a moment, so it doesn't land on whatever is now under
/// the pointer.
pub fn hold_scrolling(on: bool) {
    if on {
        HOLD.store(true, Ordering::Relaxed);
    } else if HOLD.swap(false, Ordering::Relaxed) {
        HOLD_UNTIL_MS.store(now_ms() + 1500, Ordering::Relaxed);
    }
}

/// Not a window gesture after all, fingers still down: let scrolling through at once.
pub fn release_scrolling() {
    HOLD.store(false, Ordering::Relaxed);
    HOLD_UNTIL_MS.store(0, Ordering::Relaxed);
}

/// kCGScrollWheelEventMomentumPhase: non-zero for scrolling that coasts after a lift.
const MOMENTUM_PHASE: u32 = 123;
const SCROLL_WHEEL: u32 = 22;

fn held(ty: u32, event: CGEventRef) -> bool {
    if HOLD.load(Ordering::Relaxed) {
        return true;
    }
    ty == SCROLL_WHEEL
        && now_ms() < HOLD_UNTIL_MS.load(Ordering::Relaxed)
        && unsafe { CGEventGetIntegerValueField(event, MOMENTUM_PHASE) } != 0
}

/// What the event tap hands to the rest of Brink. Hooks run on the tap's thread and must
/// return quickly.
pub enum Hook {
    /// A Control-Option key; true when Brink used it (it is then swallowed).
    Key(u16),
    LeftDown(CGPoint),
    LeftUp,
}
static HOOK: std::sync::OnceLock<Box<dyn Fn(Hook) -> bool + Send + Sync>> = std::sync::OnceLock::new();
/// Keys whose key-down Brink swallowed; their key-up goes too.
static SWALLOWED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

pub fn set_hook(f: impl Fn(Hook) -> bool + Send + Sync + 'static) {
    let _ = HOOK.set(Box::new(f));
}

fn hook(h: Hook) -> bool {
    HOOK.get().is_some_and(|f| f(h))
}

extern "C" fn tap_callback(_proxy: CGEventTapProxy, ty: u32, event: CGEventRef, _user: *mut c_void) -> CGEventRef {
    match ty {
        kCGEventTapDisabledByTimeout | kCGEventTapDisabledByUserInput => {
            let tap = TAP.load(Ordering::Relaxed) as CFMachPortRef;
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            event
        }
        kCGEventMouseMoved | kCGEventLeftMouseDragged => {
            if FROZEN.load(Ordering::Relaxed) {
                unsafe { CGWarpMouseCursorPosition(*FREEZE_AT.lock()) };
                std::ptr::null_mut()
            } else {
                event
            }
        }
        kCGEventLeftMouseDown | kCGEventRightMouseDown => {
            if now_ms() < SUPPRESS_UNTIL_MS.load(Ordering::Relaxed) {
                DROPPED_DOWN.store(true, Ordering::Relaxed);
                std::ptr::null_mut()
            } else {
                LAST_DOWN_MS.store(now_ms(), Ordering::Relaxed);
                if ty == kCGEventLeftMouseDown {
                    hook(Hook::LeftDown(unsafe { CGEventGetLocation(event) }));
                }
                event
            }
        }
        kCGEventLeftMouseUp | kCGEventRightMouseUp => {
            if ty == kCGEventLeftMouseUp {
                hook(Hook::LeftUp);
            }
            if DROPPED_DOWN.swap(false, Ordering::Relaxed) {
                std::ptr::null_mut()
            } else {
                event
            }
        }
        kCGEventKeyDown | kCGEventKeyUp => unsafe {
            let code = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
            if ty == kCGEventKeyUp {
                let mut sw = SWALLOWED.lock();
                return match sw.iter().position(|c| *c == code) {
                    Some(i) => {
                        sw.remove(i);
                        std::ptr::null_mut()
                    }
                    None => event,
                };
            }
            // Held keys repeat: swallow the repeats of a used key without acting again, even if
            // the modifiers were let go first.
            let repeat = CGEventGetIntegerValueField(event, kCGKeyboardEventAutorepeat) != 0;
            if repeat {
                return if SWALLOWED.lock().contains(&code) { std::ptr::null_mut() } else { event };
            }
            // A fresh press: a key-up Brink was waiting for never came (Secure Input); forget it.
            SWALLOWED.lock().retain(|c| *c != code);
            let flags = CGEventGetFlags(event);
            let wanted = kCGEventFlagMaskControl | kCGEventFlagMaskAlternate;
            if flags & wanted != wanted || flags & (kCGEventFlagMaskCommand | kCGEventFlagMaskShift) != 0 {
                return event;
            }
            if hook(Hook::Key(code as u16)) {
                SWALLOWED.lock().push(code);
                std::ptr::null_mut()
            } else {
                event
            }
        },
        t if SCROLL_AND_GESTURES.contains(&t) && held(t, event) => std::ptr::null_mut(),
        _ => event,
    }
}

/// Installs the session event tap on its own run-loop thread. Needs Accessibility;
/// returns false (and can be retried) when it is not granted yet.
pub fn install_event_tap() -> bool {
    if TAP.load(Ordering::Relaxed) != 0 {
        return true;
    }
    allow_background_cursor_control();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("brink-event-tap".into())
        .spawn(move || unsafe {
            let mask: u64 = [
                kCGEventLeftMouseDown,
                kCGEventLeftMouseUp,
                kCGEventRightMouseDown,
                kCGEventRightMouseUp,
                kCGEventMouseMoved,
                kCGEventLeftMouseDragged,
                kCGEventKeyDown,
                kCGEventKeyUp,
            ]
            .iter()
            .chain(SCROLL_AND_GESTURES.iter())
            .fold(0, |m, t| m | (1u64 << t));
            let tap = CGEventTapCreate(
                kCGSessionEventTap,
                kCGHeadInsertEventTap,
                kCGEventTapOptionDefault,
                mask,
                tap_callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                let _ = tx.send(false);
                return;
            }
            TAP.store(tap as u64, Ordering::Relaxed);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            let _ = tx.send(true);
            CFRunLoopRun();
        })
        .ok();
    rx.recv().unwrap_or(false)
}
