//! Brink's level HUD: a small glass pill with an icon, a bar and the percentage, shown at the
//! bottom of the screen the pointer is on while an edge is being slid. The page (ui/hud.html) is a
//! <liquid-glass> scene: a glass track and thumb over a band of Brink's ribbons. The window is clear.

use super::levels::Hud;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const HUD: &str = "hud";
const W: f64 = 264.0;
const H: f64 = 54.0;
/// How long it stays after the last change.
const LINGER_MS: u64 = 1300;

pub fn setup(ns: *mut c_void) {
    if ns.is_null() {
        return;
    }
    super::glass::apply(ns);
    unsafe {
        let w = ns as *mut AnyObject;
        let _: () = msg_send![w, setIgnoresMouseEvents: true];
        let _: () = msg_send![w, setLevel: 25isize];
        // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
        let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
        let _: () = msg_send![w, setCollectionBehavior: behavior];
        // The page draws the glass; the window adds a soft shadow under the pill.
        let _: () = msg_send![w, setHasShadow: Bool::YES];
        let _: () = msg_send![w, setAlphaValue: 0.0f64];
    }
}

fn now_ms() -> u64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

static HIDE_AT: AtomicU64 = AtomicU64::new(0);
static HIDER: AtomicBool = AtomicBool::new(false);

pub fn show(app: &AppHandle, h: Hud) {
    let _ = app.emit_to(HUD, "brink://hud", h);
    HIDE_AT.store(now_ms() + LINGER_MS, Ordering::Relaxed);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || place(&handle));

    // One thread waits out the last change, however many there are.
    if !HIDER.swap(true, Ordering::AcqRel) {
        let app = app.clone();
        std::thread::spawn(move || {
            loop {
                let left = HIDE_AT.load(Ordering::Relaxed).saturating_sub(now_ms());
                if left == 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(left));
            }
            HIDER.store(false, Ordering::Release);
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if HIDE_AT.load(Ordering::Relaxed) > now_ms() {
                    return; // a new change came in meanwhile
                }
                if let Some(Ok(ns)) = handle.get_webview_window(HUD).map(|w| w.ns_window()) {
                    unsafe {
                        let anim: *mut AnyObject = msg_send![ns as *mut AnyObject, animator];
                        let _: () = msg_send![anim, setAlphaValue: 0.0f64];
                    }
                }
            });
        });
    }
}

/// Bottom centre of the screen the pointer is on, above the Dock.
fn place(app: &AppHandle) {
    let Some(Ok(ns)) = app.get_webview_window(HUD).map(|w| w.ns_window()) else { return };
    unsafe {
        let w = ns as *mut AnyObject;
        let Some(ev) = AnyClass::get(c"NSEvent") else { return };
        let Some(scr) = AnyClass::get(c"NSScreen") else { return };
        let mouse: NSPoint = msg_send![ev, mouseLocation];
        let screens: *mut AnyObject = msg_send![scr, screens];
        let n: usize = msg_send![screens, count];
        let mut visible: Option<NSRect> = None;
        for i in 0..n {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            let f: NSRect = msg_send![s, frame];
            if mouse.x >= f.origin.x
                && mouse.x <= f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y <= f.origin.y + f.size.height
            {
                visible = Some(msg_send![s, visibleFrame]);
                break;
            }
        }
        let Some(v) = visible else {
            log::info!("brink: hud found no screen under the pointer ({:.0}, {:.0}) among {n}", mouse.x, mouse.y);
            return;
        };
        // Which screen it lands on, once per move, so a log shows it on an external display too.
        static LAST: parking_lot::Mutex<(i64, i64)> = parking_lot::Mutex::new((i64::MIN, 0));
        let at = (v.origin.x as i64, v.origin.y as i64);
        if *LAST.lock() != at {
            *LAST.lock() = at;
            log::info!("brink: hud on screen at ({:.0}, {:.0}) {:.0}x{:.0} of {n}", v.origin.x, v.origin.y, v.size.width, v.size.height);
        }
        let r = NSRect::new(NSPoint::new((v.origin.x + (v.size.width - W) / 2.0).round(), v.origin.y + 44.0), NSSize::new(W, H));
        let _: () = msg_send![w, setFrame: r, display: false];
        let _: () = msg_send![w, setAlphaValue: 1.0f64];
        let _: () = msg_send![w, orderFrontRegardless];
    }
}
