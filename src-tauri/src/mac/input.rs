//! Raw trackpad contacts from MultitouchSupport, fed into the gesture engine.

use super::ffi::*;
use super::output;
use crate::engine::{Contact, Effect};
use crate::settings::{Haptic, Settings};
use crate::Shared;
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

struct Device {
    ptr: MTDeviceRef,
    id: u64,
    mouse: bool,
}
unsafe impl Send for Device {}

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());

// Magic Mouse family ids; everything else MultitouchSupport reports is a trackpad.
const MOUSE_FAMILIES: [i32; 2] = [112, 113];

/// Seconds on a monotonic clock shared by frames and ticks.
pub fn now() -> f64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The Magic Mouse is a small surface: wide edges, no corner or bottom strip.
fn mouse_settings(s: &Settings) -> Settings {
    Settings { edge_width: s.edge_width.max(0.2), quick_mute: false, media_strip: false, top_strip: false, ..s.clone() }
}

extern "C" fn on_frame(dev: MTDeviceRef, touches: *const MTTouch, n: i32, _ts: f64, _frame: i32) -> i32 {
    let Some(shared) = SHARED.get() else { return 0 };
    let (id, mouse) = DEVICES.lock().iter().find(|d| d.ptr == dev).map(|d| (d.id, d.mouse)).unwrap_or((0, false));

    let mut settings = shared.settings.read().clone();
    if mouse {
        if !settings.magic_mouse {
            return 0;
        }
        settings = mouse_settings(&settings);
    }

    // States 3 (make touch) and 4 (touching) are fingers actually on the glass.
    let slice = if touches.is_null() || n <= 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(touches, n as usize) } };
    let contacts: Vec<Contact> = slice
        .iter()
        .filter(|t| t.state == 3 || t.state == 4)
        .map(|t| Contact { id: t.identifier, x: t.normalized.pos.x, y: t.normalized.pos.y })
        .collect();

    let mods = super::system::modifiers();
    let engine = if mouse { &shared.mouse_engine } else { &shared.engine };
    let effects = engine.lock().frame(&contacts, now(), mods, &settings);
    if !effects.is_empty() {
        if !mouse {
            output::set_active_device(id);
        }
        apply(shared, effects);
    }
    if !mouse {
        let outs = shared.windows.lock().frame(&contacts, now(), &settings, super::window::probe);
        if !outs.is_empty() {
            output::set_active_device(id);
            window_outs(shared, outs);
        }
    }
    0
}

fn window_outs(shared: &Arc<Shared>, outs: Vec<crate::windows::Out>) {
    use crate::windows::Out;
    for o in outs {
        match o {
            Out::Hold(on) => output::hold_scrolling(on),
            Out::Release => output::release_scrolling(),
            Out::Preview(dir) => {
                // A light tick each time the preview shows or changes, none when it hides.
                if dir.is_some() {
                    output::haptic(Haptic::Light.min(shared.settings.read().haptic));
                }
                super::window::preview(shared.clone(), dir);
            }
            Out::Act(action) => {
                output::haptic(shared.settings.read().haptic);
                super::window::perform(shared.clone(), action);
            }
        }
    }
}

pub fn apply(shared: &Shared, effects: Vec<Effect>) {
    let mut changed = false;
    for e in effects {
        match e {
            Effect::Key { key, quarter } => {
                use crate::engine::Key::*;
                let level = matches!(key, VolumeUp | VolumeDown | BrightnessUp | BrightnessDown | KeyboardUp | KeyboardDown);
                if !level {
                    log::info!("brink: key {key:?}");
                }
                output::hide_pointer();
                let (direct, monitors) = {
                    let s = shared.settings.read();
                    (s.level_hud, s.external_monitors)
                };
                // Set it directly and show Brink's HUD; anything that can't be set that way
                // gets the key, and macOS's HUD.
                use super::levels::Outcome;
                match if level { super::levels::step(key, quarter, direct, monitors) } else { Outcome::Key } {
                    Outcome::Shown(h) => shared.show_hud(h),
                    Outcome::Later => {} // BetterDisplay reports the level; the HUD shows then
                    Outcome::Key => {
                        // A monitor app may handle this key: show its OSD broadcast if one follows.
                        if level {
                            super::betterdisplay::note_key();
                        }
                        output::post_key(key, quarter)
                    }
                }
                changed = true;
            }
            Effect::Haptic(h) => output::haptic(h),
            Effect::HapticDouble => {
                output::haptic(Haptic::Strong);
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(70));
                    output::haptic(Haptic::Strong);
                });
            }
            Effect::Freeze(on) => output::freeze(on),
            Effect::SuppressClicks => output::suppress_clicks_for(0.35),
            Effect::Edge(c) => shared.notify_edge(c),
            Effect::Action(a) => super::actions::run(a),
            Effect::CornerFired(k) => shared.notify_corner(k),
            Effect::ScrubStart => super::media::scrub_start(),
            Effect::MediaTap { key, held } => {
                // A real click while that finger was down (a thumb pressing the trackpad's
                // bottom edge): it was a click, not a tap on the player buttons.
                if output::clicked_within(held + 0.05) {
                    log::info!("brink: bottom tap was a click, ignored");
                } else {
                    log::info!("brink: bottom tap {key:?}");
                    super::media::button(key);
                }
            }
            Effect::Scrub { delta, fine } => {
                output::hide_pointer();
                super::media::scrub(delta, fine, shared.settings.read().level_hud);
            }
        }
    }
    if changed {
        shared.notify_levels();
    }
}

fn device_list() -> Vec<Device> {
    let Some(mt) = multitouch() else { return Vec::new() };
    let mut out = Vec::new();
    unsafe {
        let list = (mt.create_list)();
        if list.is_null() {
            return out;
        }
        for i in 0..CFArrayGetCount(list) {
            let ptr = CFArrayGetValueAtIndex(list, i) as MTDeviceRef;
            if ptr.is_null() {
                continue;
            }
            CFRetain(ptr as CFTypeRef);
            let mut id = 0u64;
            if let Some(f) = mt.get_device_id {
                f(ptr, &mut id);
            }
            let mut family = 0i32;
            if let Some(f) = mt.get_family_id {
                f(ptr, &mut family);
            }
            out.push(Device { ptr, id, mouse: MOUSE_FAMILIES.contains(&family) });
        }
        CFRelease(list);
    }
    out
}

fn attach(new: Vec<Device>) {
    let Some(mt) = multitouch() else { return };
    let mut devices = DEVICES.lock();
    unsafe {
        for d in devices.drain(..) {
            (mt.unregister)(d.ptr, on_frame);
            (mt.stop)(d.ptr);
            CFRelease(d.ptr as CFTypeRef);
        }
        for d in &new {
            (mt.register)(d.ptr, on_frame);
            (mt.start)(d.ptr, 0);
        }
    }
    *devices = new;
    output::drop_actuators();
    log::info!("brink: watching {} multitouch device(s)", devices.len());
}

fn ids(v: &[Device]) -> Vec<u64> {
    let mut ids: Vec<u64> = v.iter().map(|d| d.id).collect();
    ids.sort_unstable();
    ids
}

/// Starts listening and keeps the device list fresh: trackpads come and go (Bluetooth
/// Magic Trackpad, sleep/wake), and a stopped device silently delivers nothing.
pub fn start(shared: Arc<Shared>) -> bool {
    if multitouch().is_none() {
        log::error!("brink: MultitouchSupport unavailable");
        return false;
    }
    let _ = SHARED.set(shared.clone());
    attach(device_list());
    let ticker = shared.clone();
    std::thread::Builder::new()
        .name("brink-tick".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(50));
            let s = ticker.settings.read().clone();
            let t = now();
            let mut effects = ticker.engine.lock().tick(t, &s);
            effects.extend(ticker.mouse_engine.lock().tick(t, &mouse_settings(&s)));
            if !effects.is_empty() {
                apply(&ticker, effects);
            }
            let outs = ticker.windows.lock().tick(t);
            if !outs.is_empty() {
                window_outs(&ticker, outs);
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("brink-devices".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            let fresh = device_list();
            let stale = {
                let cur = DEVICES.lock();
                let not_running =
                    multitouch().and_then(|m| m.is_running).map(|f| cur.iter().any(|d| !unsafe { f(d.ptr) })).unwrap_or(false);
                ids(&cur) != ids(&fresh) || not_running
            };
            if stale {
                let mut effects = shared.engine.lock().reset();
                effects.extend(shared.mouse_engine.lock().reset());
                apply(&shared, effects);
                attach(fresh);
            } else {
                for d in fresh {
                    unsafe { CFRelease(d.ptr as CFTypeRef) };
                }
            }
        })
        .ok();
    true
}
