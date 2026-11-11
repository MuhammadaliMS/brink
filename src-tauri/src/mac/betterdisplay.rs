//! BetterDisplay integration (betterdisplay.pro/guide/integration/third-party-apps).
//!
//! When BetterDisplay runs and an edge targets something only it controls (an external monitor's
//! brightness, or a monitor's speakers), Brink asks BetterDisplay to make the change through its
//! request channel, then asks for the new level and shows it in the HUD. That keeps one level:
//! BetterDisplay's own, which the keyboard keys also use. (Setting the Mac's volume for the
//! monitor directly gave a second level the keys didn't know about, and pressing keys for
//! BetterDisplay to catch neither reached it reliably nor produced feedback.)
//!
//! Requests and responses are distributed notifications carrying JSON in the object:
//! `pro.betterdisplay.BetterDisplay.request` {"uuid","commands":["set"],"parameters":{...}} and
//! `.response` {"uuid","result","payload"}. Parameter names are the CLI's, without hyphens.
//! BetterDisplay's OSD broadcast (`.osd`, off unless "Dispatch OSD notifications" is on) is also
//! shown when it follows one of Brink's own changes.

use super::ffi::{cfstr, CFStringRef};
use super::levels::{Hud, Kind};
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use tauri::AppHandle;

type Center = *mut c_void;

extern "C" {
    fn CFNotificationCenterGetDistributedCenter() -> Center;
    fn CFNotificationCenterAddObserver(
        center: Center,
        observer: *const c_void,
        callback: extern "C" fn(Center, *mut c_void, CFStringRef, *const c_void, *const c_void),
        name: CFStringRef,
        object: *const c_void,
        suspension: isize,
    );
    fn CFNotificationCenterPostNotification(
        center: Center,
        name: CFStringRef,
        object: *const c_void,
        user_info: *const c_void,
        deliver_immediately: bool,
    );
    fn CFRelease(cf: *const c_void);
    fn CFGetTypeID(cf: *const c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(s: *const c_void) -> isize;
    fn CFStringGetCString(s: *const c_void, buf: *mut c_char, size: isize, enc: u32) -> bool;
}

/// Deliver even while Brink isn't the active app (it never is: it lives in the menu bar).
const DELIVER_IMMEDIATELY: isize = 4;
/// BetterDisplay's answer to one of Brink's key presses arrives well within this.
const WINDOW_MS: u64 = 1200;

static APP: OnceLock<AppHandle> = OnceLock::new();
static LAST_KEY: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Brink just pressed a level key it couldn't handle itself.
pub fn note_key() {
    LAST_KEY.store(now_ms(), Ordering::Relaxed);
}

/// Starts listening. Call once, on the main thread (the observer runs on its run loop).
pub fn start(app: AppHandle) {
    if APP.set(app).is_err() {
        return;
    }
    unsafe {
        let center = CFNotificationCenterGetDistributedCenter();
        if center.is_null() {
            return;
        }
        // The current prefix only: BetterDisplay broadcasts OSD under both, and answers a request
        // under the prefix it came with.
        CFNotificationCenterAddObserver(
            center,
            std::ptr::null(),
            on_osd,
            cfstr("pro.betterdisplay.BetterDisplay.osd"),
            std::ptr::null(),
            DELIVER_IMMEDIATELY,
        );
        CFNotificationCenterAddObserver(
            center,
            std::ptr::null(),
            on_response,
            cfstr("pro.betterdisplay.BetterDisplay.response"),
            std::ptr::null(),
            DELIVER_IMMEDIATELY,
        );
    }
}

// ---------- is it running ----------

/// Whether BetterDisplay is running, checked at most every two seconds.
pub fn running() -> bool {
    static AT: AtomicU64 = AtomicU64::new(0);
    static UP: AtomicBool = AtomicBool::new(false);
    let now = now_ms();
    if now.saturating_sub(AT.load(Ordering::Relaxed)) < 2000 && AT.load(Ordering::Relaxed) != 0 {
        return UP.load(Ordering::Relaxed);
    }
    AT.store(now, Ordering::Relaxed);
    let up = app_running("pro.betterdisplay.BetterDisplay");
    UP.store(up, Ordering::Relaxed);
    up
}

/// Whether an app with this bundle id is running.
pub fn app_running(bundle: &str) -> bool {
    objc2::rc::autoreleasepool(|_| unsafe {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};
        let Some(cls) = AnyClass::get(c"NSRunningApplication") else { return false };
        let id = objc2_foundation::NSString::from_str(bundle);
        let apps: *mut AnyObject = msg_send![cls, runningApplicationsWithBundleIdentifier: &*id];
        !apps.is_null() && {
            let n: usize = msg_send![apps, count];
            n > 0
        }
    })
}

// ---------- requests ----------

/// Pending `get` requests: uuid -> what the answer is a level of.
static PENDING: Mutex<Option<HashMap<String, Kind>>> = Mutex::new(None);

fn send(commands: &[&str], parameters: serde_json::Value, answer: Option<Kind>) {
    static N: AtomicU64 = AtomicU64::new(0);
    let uuid = format!("brink-{}-{}", now_ms(), N.fetch_add(1, Ordering::Relaxed));
    if let Some(kind) = answer {
        let mut p = PENDING.lock();
        let map = p.get_or_insert_with(HashMap::new);
        if map.len() > 64 {
            map.clear(); // answers that never came
        }
        map.insert(uuid.clone(), kind);
    }
    let json = serde_json::json!({ "uuid": uuid, "commands": commands, "parameters": parameters }).to_string();
    log::info!("brink: betterdisplay request {json}");
    unsafe {
        let center = CFNotificationCenterGetDistributedCenter();
        if center.is_null() {
            return;
        }
        let name = cfstr("pro.betterdisplay.BetterDisplay.request");
        let object = cfstr(&json);
        CFNotificationCenterPostNotification(center, name, object, std::ptr::null(), true);
        CFRelease(object);
        CFRelease(name);
    }
}

/// One step, as an offset BetterDisplay applies to its own current level: "6.25%" or "-1.5625%".
fn offset(up: bool, quarter: bool) -> String {
    let step = if quarter { 100.0 / 64.0 } else { 100.0 / 16.0 };
    format!("{}{}%", if up { "" } else { "-" }, step)
}

/// Steps the brightness of display `display` (a macOS display ID), then asks for the new level.
pub fn step_brightness(display: u32, up: bool, quarter: bool) {
    note_key();
    let id = display.to_string();
    send(&["set"], serde_json::json!({ "brightness": offset(up, quarter), "offset": null, "displayID": id }), None);
    send(&["get"], serde_json::json!({ "brightness": null, "displayID": id }), Some(Kind::Monitor));
}

/// Steps the volume of the monitor named `device` (the current sound output), then asks for the
/// new level. BetterDisplay pairs audio with a display by name, so this targets the same one.
pub fn step_volume(device: Option<&str>, up: bool, quarter: bool) {
    note_key();
    let mut set = serde_json::json!({ "volume": offset(up, quarter), "offset": null });
    let mut get = serde_json::json!({ "volume": null });
    let target = match device {
        Some(name) => ("nameLike", serde_json::Value::from(name)),
        None => ("displayWithMouse", serde_json::Value::Null),
    };
    set[target.0] = target.1.clone();
    get[target.0] = target.1;
    send(&["set"], set, None);
    send(&["get"], get, Some(Kind::Volume));
}

#[derive(Deserialize, Default)]
struct Response {
    uuid: Option<String>,
    result: Option<bool>,
    payload: Option<String>,
}

extern "C" fn on_response(_: Center, _: *mut c_void, _: CFStringRef, object: *const c_void, _: *const c_void) {
    let Some(json) = (unsafe { cf_string(object) }) else { return };
    let Ok(r) = serde_json::from_str::<Response>(&json) else { return };
    let Some(uuid) = r.uuid.as_deref().filter(|u| u.starts_with("brink-")) else { return };
    log::info!("brink: betterdisplay response {json}");
    let Some(kind) = PENDING.lock().as_mut().and_then(|m| m.remove(uuid)) else { return };
    if r.result == Some(false) {
        return;
    }
    let Some(level) = r.payload.as_deref().and_then(level_of) else { return };
    if let Some(app) = APP.get() {
        super::hud::show(app, Hud { kind, level, muted: false, secs: 0.0 });
    }
}

/// A `get` answer as a level 0..1. The docs don't fix its format, so take "0.62", "62%", "62" or
/// a number inside JSON ({"value":0.62} or [0.62]).
fn level_of(payload: &str) -> Option<f32> {
    let t = payload.trim();
    let num = |s: &str| -> Option<(f64, bool)> {
        let s = s.trim().trim_matches('"');
        let pct = s.ends_with('%');
        s.trim_end_matches('%').trim().parse::<f64>().ok().map(|v| (v, pct))
    };
    let (v, pct) = num(t).or_else(|| {
        let j: serde_json::Value = serde_json::from_str(t).ok()?;
        let n = match &j {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::Array(a) => a.first().and_then(|v| v.as_f64()),
            serde_json::Value::Object(o) => o.get("value").and_then(|v| v.as_f64()),
            _ => None,
        }?;
        Some((n, false))
    })?;
    let v = if pct || v > 1.0 { v / 100.0 } else { v };
    v.is_finite().then_some(v.clamp(0.0, 1.0) as f32)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Osd {
    system_icon_id: Option<i64>,
    control_target: Option<String>,
    value: Option<f64>,
    max_value: Option<f64>,
}

static LAST_SHOWN: AtomicU64 = AtomicU64::new(0);

extern "C" fn on_osd(_: Center, _: *mut c_void, _: CFStringRef, object: *const c_void, _: *const c_void) {
    let now = now_ms();
    let last = LAST_KEY.load(Ordering::Relaxed);
    if last == 0 || now.saturating_sub(last) > WINDOW_MS {
        return;
    }
    // Both prefixes may arrive for one change: show it once.
    if now.saturating_sub(LAST_SHOWN.swap(now, Ordering::Relaxed)) < 15 {
        return;
    }
    let Some(json) = (unsafe { cf_string(object) }) else { return };
    let Some(hud) = parse(&json) else { return };
    if let Some(app) = APP.get() {
        super::hud::show(app, hud);
    }
}

fn parse(json: &str) -> Option<Hud> {
    let o: Osd = serde_json::from_str(json).ok()?;
    let target = o.control_target.as_deref().unwrap_or("");
    let kind = match (target, o.system_icon_id) {
        ("volume" | "mute", _) | (_, Some(3 | 4)) => Kind::Volume,
        (t, _) if t.to_ascii_lowercase().contains("brightness") => Kind::Monitor,
        (_, Some(1)) => Kind::Monitor,
        _ => return None, // contrast, gamma and the rest: not something an edge changes
    };
    let max = o.max_value.filter(|m| *m > 0.0).unwrap_or(1.0);
    let level = (o.value? / max).clamp(0.0, 1.0) as f32;
    let muted = target == "mute" || o.system_icon_id == Some(4);
    Some(Hud { kind, level: if muted { 0.0 } else { level }, muted, secs: 0.0 })
}

unsafe fn cf_string(s: *const c_void) -> Option<String> {
    if s.is_null() || CFGetTypeID(s) != CFStringGetTypeID() {
        return None;
    }
    let size = CFStringGetLength(s) * 4 + 1;
    let mut buf = vec![0 as c_char; size as usize];
    CFStringGetCString(s, buf.as_mut_ptr(), size, 0x0800_0100)
        .then(|| std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_get_answers() {
        assert_eq!(level_of("0.62"), Some(0.62));
        assert_eq!(level_of(" 62% "), Some(0.62));
        assert_eq!(level_of("62"), Some(0.62));
        assert_eq!(level_of("{\"value\":0.25}"), Some(0.25));
        assert_eq!(level_of("[1]"), Some(1.0));
        assert_eq!(level_of("on"), None);
        assert_eq!(offset(true, false), "6.25%");
        assert_eq!(offset(false, true), "-1.5625%");
    }

    #[test]
    fn reads_betterdisplay_osd() {
        let h = parse(r#"{"displayID":2,"controlTarget":"volume","value":30,"maxValue":100}"#).unwrap();
        assert_eq!(h.kind, Kind::Volume);
        assert!((h.level - 0.3).abs() < 1e-6 && !h.muted);
        let h = parse(r#"{"systemIconID":1,"controlTarget":"combinedBrightness","value":0.75}"#).unwrap();
        assert_eq!(h.kind, Kind::Monitor);
        assert!((h.level - 0.75).abs() < 1e-6);
        let h = parse(r#"{"controlTarget":"mute","value":1,"maxValue":1}"#).unwrap();
        assert!(h.muted && h.level == 0.0);
        assert!(parse(r#"{"controlTarget":"hardwareContrast","value":0.5}"#).is_none());
        // A missing or zero maximum must not divide by zero.
        assert!((parse(r#"{"controlTarget":"volume","value":0.4,"maxValue":0}"#).unwrap().level - 0.4).abs() < 1e-6);
    }
}

#[cfg(test)]
mod wire {
    /// Posts a real request (run with --ignored while something listens on the channel).
    #[test]
    #[ignore]
    fn posts_a_request() {
        super::step_brightness(2, true, false);
        super::step_volume(Some("DELL U2723QE"), false, true);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
