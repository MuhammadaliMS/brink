//! Window gestures on macOS, through the Accessibility API (which Brink already has).
//!
//! Finds what is under the pointer when two fingers land (the top strip of a window, or an app
//! in the Dock), previews where the window will go, and moves it there: fill the screen, a
//! half, a quarter, the next display, back, or minimized. Brink remembers where it moved a
//! window from, so swiping down puts it back, and which windows it put side by side, so
//! dragging the edge between them resizes both. Control-Option shortcuts do the same to the
//! focused window.

use super::ffi::*;
use super::output::Hook;
use crate::windows::{Action, Dir, Zone};
use crate::Shared;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};

type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn near(&self, o: &Rect) -> bool {
        (self.x - o.x).abs() < 6.0 && (self.y - o.y).abs() < 6.0 && (self.w - o.w).abs() < 6.0 && (self.h - o.h).abs() < 6.0
    }
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    /// A part of this rect given as fractions (x, y, w, h), on whole points.
    fn part(&self, f: [f64; 4]) -> Rect {
        let x0 = (self.w * f[0]).round();
        let x1 = (self.w * (f[0] + f[2])).round();
        let y0 = (self.h * f[1]).round();
        let y1 = (self.h * (f[1] + f[3])).round();
        Rect { x: self.x + x0, y: self.y + y0, w: x1 - x0, h: y1 - y0 }
    }
}

extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(app: AXUIElementRef, x: f32, y: f32, out: *mut AXUIElementRef) -> i32;
    fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementPerformAction(el: AXUIElementRef, action: CFStringRef) -> i32;
    fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, secs: f32) -> i32;
    fn AXValueCreate(ty: u32, value: *const c_void) -> AXValueRef;
    fn AXValueGetValue(v: AXValueRef, ty: u32, out: *mut c_void) -> bool;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> bool;
    fn CFBooleanGetValue(b: CFTypeRef) -> bool;
    fn CFStringGetCString(s: CFStringRef, buf: *mut i8, size: isize, enc: u32) -> bool;
    static kCFBooleanFalse: CFTypeRef;
}

const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;
/// Title bars plus unified toolbars (Safari, Finder, Xcode) are at most about this tall.
const TITLE_BAR: f64 = 52.0;
/// Every window's top this tall is title bar (the traffic lights sit in it), whatever the app.
const PLAIN_TITLE_BAR: f64 = 28.0;
/// How close to the edge between two windows a drag has to start.
const DIVIDER_REACH: f64 = 8.0;

/// A retained AXUIElement (a window or an app).
struct Win(AXUIElementRef);
unsafe impl Send for Win {}
impl Drop for Win {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}
impl Clone for Win {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0) };
        Win(self.0)
    }
}
impl Win {
    fn same(&self, o: &Win) -> bool {
        unsafe { CFEqual(self.0, o.0) }
    }
    fn pid(&self) -> i32 {
        let mut pid = 0;
        unsafe { AXUIElementGetPid(self.0, &mut pid) };
        pid
    }
}

enum Target {
    Window(Win),
    /// An app in the Dock, by its bundle path.
    Dock(String),
}

/// What was under the pointer when the gesture began.
static TARGET: Mutex<Option<Target>> = Mutex::new(None);
/// Windows Brink moved: (window, where it was, where Brink put it).
static MOVED: Mutex<Vec<(Win, Rect, Rect)>> = Mutex::new(Vec::new());

fn attr(el: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    let key = cfstr(name);
    let mut out: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(el, key, &mut out) };
    unsafe { CFRelease(key) };
    (err == 0 && !out.is_null()).then_some(out)
}

fn string(el: AXUIElementRef, name: &str) -> Option<String> {
    let v = attr(el, name)?;
    let mut buf = [0i8; 128];
    let ok = unsafe { CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, kCFStringEncodingUTF8) };
    unsafe { CFRelease(v) };
    ok.then(|| unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

fn set(el: AXUIElementRef, name: &str, value: CFTypeRef) -> bool {
    let key = cfstr(name);
    let err = unsafe { AXUIElementSetAttributeValue(el, key, value) };
    unsafe { CFRelease(key) };
    err == 0
}

fn set_flag(el: AXUIElementRef, name: &str, on: bool) -> bool {
    set(el, name, unsafe {
        if on {
            kCFBooleanTrue
        } else {
            kCFBooleanFalse
        }
    })
}

fn perform_ax(el: AXUIElementRef, name: &str) -> bool {
    let action = cfstr(name);
    let err = unsafe { AXUIElementPerformAction(el, action) };
    unsafe { CFRelease(action) };
    err == 0
}

fn frame(w: &Win) -> Option<Rect> {
    let mut p = CGPoint::default();
    let mut s = CGSize::default();
    let pv = attr(w.0, "AXPosition")?;
    let sv = attr(w.0, "AXSize");
    unsafe {
        AXValueGetValue(pv, AX_POINT, &mut p as *mut _ as *mut c_void);
        CFRelease(pv);
        let sv = sv?;
        AXValueGetValue(sv, AX_SIZE, &mut s as *mut _ as *mut c_void);
        CFRelease(sv);
    }
    Some(Rect { x: p.x, y: p.y, w: s.width, h: s.height })
}

fn set_frame(w: &Win, r: Rect) {
    unsafe {
        let p = AXValueCreate(AX_POINT, &CGPoint { x: r.x, y: r.y } as *const _ as *const c_void);
        let s = AXValueCreate(AX_SIZE, &CGSize { width: r.w, height: r.h } as *const _ as *const c_void);
        // Move first so the new size fits on screen, then size, then move again in case the
        // app clamped the size around its old origin.
        set(w.0, "AXPosition", p);
        set(w.0, "AXSize", s);
        set(w.0, "AXPosition", p);
        CFRelease(p);
        CFRelease(s);
    }
}

fn flag(w: &Win, name: &str) -> bool {
    attr(w.0, name)
        .map(|v| unsafe {
            let on = CFBooleanGetValue(v);
            CFRelease(v);
            on
        })
        .unwrap_or(false)
}

fn pointer() -> CGPoint {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return CGPoint::default();
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        p
    }
}

fn own_pid() -> i32 {
    std::process::id() as i32
}

/// The window an element belongs to, if it is a normal window of another app.
fn standard_window(el: AXUIElementRef) -> Option<Win> {
    let window = if string(el, "AXRole").as_deref() == Some("AXWindow") {
        unsafe { CFRetain(el) };
        el
    } else {
        attr(el, "AXWindow")?
    };
    let win = Win(window);
    (win.pid() != own_pid() && string(win.0, "AXSubrole").as_deref() == Some("AXStandardWindow")).then_some(win)
}

unsafe fn ns_string(s: *mut AnyObject) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let c: *const i8 = msg_send![s, UTF8String];
    (!c.is_null()).then(|| std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned())
}

/// A Dock item's app bundle path.
fn dock_app(el: AXUIElementRef) -> Option<String> {
    if string(el, "AXSubrole").as_deref() != Some("AXApplicationDockItem") {
        return None;
    }
    let url = attr(el, "AXURL")?;
    let path = unsafe {
        let p: *mut AnyObject = msg_send![url as *mut AnyObject, path];
        let s = ns_string(p);
        CFRelease(url);
        s
    };
    path.filter(|p| !p.is_empty())
}

/// What gesture zone the pointer is over: a window's title bar or an app in the Dock.
/// Remembers what it found as the target.
pub fn probe() -> Option<Zone> {
    new_gesture();
    // A held click (thumb on the trackpad) is a drag, not a gesture.
    if unsafe { CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, 0) } {
        return None;
    }
    objc2::rc::autoreleasepool(|_| probe_at(pointer()))
}

/// Brink's Accessibility calls give up on an app that doesn't answer within this long, so a
/// hung app under the pointer can't stall the trackpad. (Set on the system-wide element, it
/// applies to every call.)
const AX_TIMEOUT: f32 = 0.25;

fn probe_at(p: CGPoint) -> Option<Zone> {
    let el = unsafe {
        let system = AXUIElementCreateSystemWide();
        AXUIElementSetMessagingTimeout(system, AX_TIMEOUT);
        let mut el: AXUIElementRef = std::ptr::null();
        let err = AXUIElementCopyElementAtPosition(system, p.x as f32, p.y as f32, &mut el);
        CFRelease(system);
        if err != 0 || el.is_null() {
            return None;
        }
        Win(el)
    };
    if let Some(path) = dock_app(el.0) {
        *TARGET.lock() = Some(Target::Dock(path));
        return Some(Zone::Dock);
    }
    let win = standard_window(el.0)?;
    let f = frame(&win)?;
    let on_bar = p.x >= f.x && p.x < f.right() && p.y >= f.y && p.y < f.y + TITLE_BAR;
    // Full screen: the top of the window is the app's content.
    if on_bar && flag(&win, "AXFullScreen") {
        return None;
    }
    // Below the bare title bar, scrolling content (a web page, a list, a text field) that
    // reaches up into the top strip keeps its scrolling.
    if on_bar && p.y >= f.y + PLAIN_TITLE_BAR && in_content(el.0) {
        return None;
    }
    if on_bar {
        *TARGET.lock() = Some(Target::Window(win));
        return Some(Zone::Title);
    }
    None
}

/// Is the element part of something that scrolls or takes text, rather than window chrome?
fn in_content(el: AXUIElementRef) -> bool {
    const CONTENT: [&str; 6] = ["AXScrollArea", "AXWebArea", "AXTextArea", "AXTextField", "AXTable", "AXOutline"];
    unsafe { CFRetain(el) };
    let mut cur = Win(el);
    for _ in 0..12 {
        match string(cur.0, "AXRole").as_deref() {
            Some("AXWindow") | None => return false,
            Some(role) if CONTENT.contains(&role) => return true,
            _ => {}
        }
        match attr(cur.0, "AXParent") {
            Some(parent) => cur = Win(parent),
            None => return false,
        }
    }
    false
}

/// The focused window of the frontmost app, for the keyboard shortcuts.
fn focused_window() -> Option<Win> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        AXUIElementSetMessagingTimeout(system, AX_TIMEOUT);
        let app = attr(system, "AXFocusedApplication");
        CFRelease(system);
        let app = Win(app?);
        let w = Win(attr(app.0, "AXFocusedWindow")?);
        (w.pid() != own_pid() && string(w.0, "AXSubrole").as_deref() == Some("AXStandardWindow")).then_some(w)
    }
}

/// Each screen's full frame and usable area (minus menu bar and Dock), in the top-left
/// coordinates the Accessibility API uses. The first is the primary screen. NSScreen lives on
/// the main thread.
fn screens(shared: &Shared) -> Vec<(Rect, Rect)> {
    shared
        .on_main(|| unsafe {
            let Some(cls) = AnyClass::get(c"NSScreen") else { return Vec::new() };
            let list: *mut AnyObject = msg_send![cls, screens];
            let n: usize = msg_send![list, count];
            let mut out = Vec::new();
            let mut primary_h = 0.0;
            for i in 0..n {
                let s: *mut AnyObject = msg_send![list, objectAtIndex: i];
                let f: NSRect = msg_send![s, frame];
                let v: NSRect = msg_send![s, visibleFrame];
                if i == 0 {
                    primary_h = f.size.height;
                }
                let flip = |r: NSRect| Rect { x: r.origin.x, y: primary_h - r.origin.y - r.size.height, w: r.size.width, h: r.size.height };
                out.push((flip(f), flip(v)));
            }
            out
        })
        .unwrap_or_default()
}

/// The screen holding the middle of the window.
fn screen_of(list: &[(Rect, Rect)], f: Rect) -> Option<(Rect, Rect)> {
    let (cx, cy) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
    list.iter().find(|(full, _)| full.contains(cx, cy)).or_else(|| list.first()).copied()
}

/// The screen next to this one on the left or right, by where the displays are arranged.
fn beside(list: &[(Rect, Rect)], full: Rect, left: bool) -> Option<Rect> {
    list.iter()
        .filter(|(o, _)| {
            let touches = if left { (o.right() - full.x).abs() < 2.0 } else { (o.x - full.right()).abs() < 2.0 };
            touches && o.y < full.bottom() && o.bottom() > full.y
        })
        .map(|(_, usable)| *usable)
        .next()
}

/// Where a window goes for an action.
#[derive(Clone, Copy, Debug)]
enum Dest {
    Place(Rect),
    /// Back to where it was before Brink moved it.
    Restore(Rect),
    Minimize,
    LeaveFullscreen,
}

/// What Brink knows about a window when a gesture starts.
struct Ctx {
    w: Win,
    current: Rect,
    fullscreen: bool,
    screens: Vec<(Rect, Rect)>,
    /// Where it was and where Brink put it, if Brink moved it and it is still there.
    placed: Option<(Rect, Rect)>,
}

fn context(shared: &Shared, w: Win) -> Option<Ctx> {
    unsafe { AXUIElementSetMessagingTimeout(w.0, 1.0) };
    let current = frame(&w)?;
    let fullscreen = flag(&w, "AXFullScreen");
    let screens = screens(shared);
    let placed = MOVED.lock().iter().find(|(m, _, p)| m.same(&w) && current.near(p)).map(|(_, from, p)| (*from, *p));
    Some(Ctx { w, current, fullscreen, screens, placed })
}

/// Decides where the window goes, and what to call it in the preview.
fn plan(c: &Ctx, action: Action) -> Option<(Dest, &'static str)> {
    if c.fullscreen {
        // A full-screen window can't move; swiping down leaves full screen.
        return (action == Action::Swipe(Dir::Down)).then_some((Dest::LeaveFullscreen, "Leave full screen"));
    }
    let (full, area) = screen_of(&c.screens, c.current)?;
    let back = c.placed.map(|(from, _)| (Dest::Restore(from), "Back"));
    let half = |left: bool| area.part(if left { [0.0, 0.0, 0.5, 1.0] } else { [0.5, 0.0, 0.5, 1.0] });
    let quarter = |x: f64, y: f64| area.part([x, y, 0.5, 0.5]);
    let (to, label) = match action {
        Action::Swipe(Dir::Down) => return Some(back.unwrap_or((Dest::Minimize, "Minimize"))),
        Action::Swipe(Dir::Up) => (area, "Fill screen"),
        Action::Swipe(d @ (Dir::Left | Dir::Right)) => {
            let left = d == Dir::Left;
            // Already on that half: on to the display next to it, if there is one.
            match beside(&c.screens, full, left).filter(|_| c.current.near(&half(left))) {
                Some(next) => (next.part(if left { [0.5, 0.0, 0.5, 1.0] } else { [0.0, 0.0, 0.5, 1.0] }), "Next display"),
                None => (half(left), if left { "Left half" } else { "Right half" }),
            }
        }
        Action::Swipe(Dir::UpLeft) => (quarter(0.0, 0.0), "Top left"),
        Action::Swipe(Dir::UpRight) => (quarter(0.5, 0.0), "Top right"),
        Action::Swipe(Dir::DownLeft) => (quarter(0.0, 0.5), "Bottom left"),
        Action::Swipe(Dir::DownRight) => (quarter(0.5, 0.5), "Bottom right"),
        Action::Frac(f) => (area.part(f), ""),
        Action::Center => {
            let (w, h) = (c.current.w.min(area.w), c.current.h.min(area.h));
            (Rect { x: area.x + ((area.w - w) / 2.0).round(), y: area.y + ((area.h - h) / 2.0).round(), w, h }, "")
        }
    };
    // The same move again on a window Brink put there takes it back.
    if c.current.near(&to) {
        if let Some(b) = back {
            return Some(b);
        }
    }
    Some((Dest::Place(to), label))
}

fn commit(c: &Ctx, dest: Dest) {
    match dest {
        Dest::Place(to) => {
            set_frame(&c.w, to);
            let from = c.placed.map(|(from, _)| from).unwrap_or(c.current);
            let mut moved = MOVED.lock();
            moved.retain(|(m, _, _)| !m.same(&c.w));
            moved.push((c.w.clone(), from, to));
            if moved.len() > 32 {
                moved.remove(0);
            }
        }
        Dest::Restore(from) => {
            set_frame(&c.w, from);
            MOVED.lock().retain(|(m, _, _)| !m.same(&c.w));
        }
        Dest::Minimize => {
            set_flag(c.w.0, "AXMinimized", true);
        }
        Dest::LeaveFullscreen => {
            set_flag(c.w.0, "AXFullScreen", false);
        }
    }
}

fn act_on_window(shared: &Shared, w: Win, action: Action) {
    let Some(c) = context(shared, w) else { return };
    let Some((dest, _)) = plan(&c, action) else { return };
    log::info!("brink: window {action:?} -> {dest:?}");
    commit(&c, dest);
}

fn app_element(pid: i32) -> Win {
    let el = Win(unsafe { AXUIElementCreateApplication(pid) });
    unsafe { AXUIElementSetMessagingTimeout(el.0, 1.0) };
    el
}

/// The app's normal windows, front to back.
fn app_windows(app_el: &Win) -> Vec<Win> {
    let Some(list) = attr(app_el.0, "AXWindows") else { return Vec::new() };
    let mut out = Vec::new();
    unsafe {
        for i in 0..CFArrayGetCount(list) {
            let el = CFArrayGetValueAtIndex(list, i);
            if el.is_null() {
                continue;
            }
            CFRetain(el);
            let w = Win(el);
            if string(w.0, "AXSubrole").as_deref() == Some("AXStandardWindow") {
                out.push(w);
            }
        }
        CFRelease(list);
    }
    out
}

fn running_pid(path: &str) -> Option<i32> {
    unsafe {
        let ws: *mut AnyObject = msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
        let apps: *mut AnyObject = msg_send![ws, runningApplications];
        let n: usize = msg_send![apps, count];
        for i in 0..n {
            let a: *mut AnyObject = msg_send![apps, objectAtIndex: i];
            let url: *mut AnyObject = msg_send![a, bundleURL];
            if url.is_null() {
                continue;
            }
            if ns_string(msg_send![url, path]).as_deref() == Some(path) {
                return Some(msg_send![a, processIdentifier]);
            }
        }
    }
    None
}

fn launch(path: &str) {
    unsafe {
        let (Some(ws), Some(url_cls)) = (AnyClass::get(c"NSWorkspace"), AnyClass::get(c"NSURL")) else { return };
        let ws: *mut AnyObject = msg_send![ws, sharedWorkspace];
        let p = NSString::from_str(path);
        let url: *mut AnyObject = msg_send![url_cls, fileURLWithPath: &*p];
        let _: Bool = msg_send![ws, openURL: url];
    }
}

/// Dock icon: up shows the app's windows (or opens it), down hides them, left/right brings
/// its next window forward.
fn act_on_dock(path: String, action: Action) {
    log::info!("brink: dock {action:?} {path}");
    let Some(pid) = running_pid(&path) else {
        if action == Action::Swipe(Dir::Up) {
            launch(&path);
        }
        return;
    };
    let app_el = app_element(pid);
    match action {
        Action::Swipe(Dir::Up) => {
            set_flag(app_el.0, "AXHidden", false);
            let windows = app_windows(&app_el);
            for w in &windows {
                if flag(w, "AXMinimized") {
                    set_flag(w.0, "AXMinimized", false);
                }
            }
            // Raise back to front so the window that was in front stays there.
            for w in windows.iter().rev() {
                perform_ax(w.0, "AXRaise");
            }
            set_flag(app_el.0, "AXFrontmost", true);
            if windows.is_empty() {
                // No windows: what clicking the icon does (most apps open a new one).
                launch(&path);
            }
        }
        Action::Swipe(Dir::Down) => {
            set_flag(app_el.0, "AXHidden", true);
        }
        Action::Swipe(dir @ (Dir::Left | Dir::Right)) => {
            set_flag(app_el.0, "AXHidden", false);
            let windows: Vec<Win> = app_windows(&app_el).into_iter().filter(|w| !flag(w, "AXMinimized")).collect();
            // Right raises the back window (so repeats go through all of them); left goes
            // back to the one just behind the front.
            let pick = if dir == Dir::Right { windows.last() } else { windows.get(1).or(windows.first()) };
            if let Some(w) = pick {
                perform_ax(w.0, "AXRaise");
                set_flag(w.0, "AXMain", true);
            }
            set_flag(app_el.0, "AXFrontmost", true);
        }
        _ => {}
    }
}

type Job = Box<dyn FnOnce() + Send>;
static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// Window moves run one at a time, in order, off the trackpad and event-tap threads (they wait
/// on the target app). In order, so a second swipe sees where the first one put the window.
fn queue(job: impl FnOnce() + Send + 'static) {
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("brink-windows".into())
            .spawn(move || {
                for job in rx {
                    objc2::rc::autoreleasepool(|_| job());
                }
            })
            .ok();
        Mutex::new(tx)
    });
    let _ = tx.lock().send(Box::new(job));
}

pub fn perform(shared: Arc<Shared>, action: Action) {
    let Some(target) = TARGET.lock().take() else { return };
    queue(move || match target {
        Target::Window(w) => act_on_window(&shared, w, action),
        Target::Dock(path) => act_on_dock(path, action),
    });
}

// ---------- the preview ----------

pub const PREVIEW: &str = "preview";

/// The preview for the gesture under way. `gen` counts gestures, so a context that arrives
/// late for an old gesture is dropped.
struct PreviewState {
    gen: u64,
    loading: bool,
    ctx: Option<Ctx>,
    want: Option<Dir>,
    /// The screen the overlay window currently covers.
    shown_on: Option<Rect>,
}
static PREVIEW_STATE: Mutex<PreviewState> = Mutex::new(PreviewState { gen: 0, loading: false, ctx: None, want: None, shown_on: None });

#[derive(serde::Serialize, Clone)]
struct PreviewView {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &'static str,
    /// "place", or "minimize" when the window will leave the screen.
    kind: &'static str,
}

/// A new gesture: forget the last one's preview.
fn new_gesture() {
    let mut st = PREVIEW_STATE.lock();
    st.gen += 1;
    st.loading = false;
    st.ctx = None;
    st.want = None;
}

/// The overlay floats over everything, never takes clicks, and follows to every Space.
pub fn setup_preview_window(ns: *mut c_void) {
    unsafe {
        let w = ns as *mut AnyObject;
        let _: () = msg_send![w, setIgnoresMouseEvents: true];
        let _: () = msg_send![w, setLevel: 25isize];
        // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
        let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
        let _: () = msg_send![w, setCollectionBehavior: behavior];
        let _: () = msg_send![w, setHasShadow: false];
    }
    super::glass::apply(ns);
}

/// Shows where the target window would go for `dir`, or hides the preview.
pub fn preview(shared: Arc<Shared>, dir: Option<Dir>) {
    let mut st = PREVIEW_STATE.lock();
    st.want = dir;
    if dir.is_none() {
        hide_preview(&shared, &mut st);
        return;
    }
    if st.ctx.is_some() {
        show_preview(&shared, &mut st);
        return;
    }
    if st.loading {
        return;
    }
    let Some(Target::Window(w)) = TARGET.lock().as_ref().map(|t| match t {
        Target::Window(w) => Target::Window(w.clone()),
        Target::Dock(p) => Target::Dock(p.clone()),
    }) else {
        return;
    };
    st.loading = true;
    let gen = st.gen;
    drop(st);
    std::thread::spawn(move || {
        let ctx = objc2::rc::autoreleasepool(|_| context(&shared, w));
        let mut st = PREVIEW_STATE.lock();
        if st.gen != gen {
            return;
        }
        st.loading = false;
        st.ctx = ctx;
        if st.want.is_some() {
            show_preview(&shared, &mut st);
        }
    });
}

fn show_preview(shared: &Shared, st: &mut PreviewState) {
    let (Some(c), Some(dir), Some(app)) = (st.ctx.as_ref(), st.want, shared.app()) else { return };
    let Some((dest, label)) = plan(c, Action::Swipe(dir)) else {
        hide_preview(shared, st);
        return;
    };
    let (shape, kind) = match dest {
        Dest::Place(r) | Dest::Restore(r) => (r, "place"),
        Dest::Minimize => (c.current, "minimize"),
        Dest::LeaveFullscreen => (c.current, "minimize"),
    };
    let Some((full, _)) = screen_of(&c.screens, shape) else { return };
    let view = PreviewView { x: shape.x - full.x, y: shape.y - full.y, w: shape.w, h: shape.h, label, kind };
    let _ = app.emit_to(PREVIEW, "brink://preview", Some(view));
    if st.shown_on != Some(full) {
        st.shown_on = Some(full);
        let primary_h = c.screens.first().map(|(f, _)| f.h).unwrap_or(0.0);
        let main = app.clone();
        let _ = main.run_on_main_thread(move || {
            // The gesture may have ended while this waited for the main thread.
            if PREVIEW_STATE.lock().shown_on != Some(full) {
                return;
            }
            let Some(Ok(ns)) = app.get_webview_window(PREVIEW).map(|w| w.ns_window()) else { return };
            let r = NSRect::new(NSPoint::new(full.x, primary_h - full.y - full.h), NSSize::new(full.w, full.h));
            unsafe {
                let ns = ns as *mut AnyObject;
                let _: () = msg_send![ns, setFrame: r, display: true];
                let _: () = msg_send![ns, orderFrontRegardless];
            }
        });
    }
}

fn hide_preview(shared: &Shared, st: &mut PreviewState) {
    let Some(app) = shared.app() else { return };
    let _ = app.emit_to(PREVIEW, "brink://preview", None::<PreviewView>);
    if st.shown_on.take().is_some() {
        let main = app.clone();
        let _ = main.run_on_main_thread(move || {
            if PREVIEW_STATE.lock().shown_on.is_some() {
                return;
            }
            if let Some(Ok(ns)) = app.get_webview_window(PREVIEW).map(|w| w.ns_window()) {
                let _: () = unsafe { msg_send![ns as *mut AnyObject, orderOut: std::ptr::null::<AnyObject>()] };
            }
        });
    }
}

// ---------- dragging the edge between windows ----------

static LEFT_DOWN: AtomicBool = AtomicBool::new(false);
static DIVIDER_TX: OnceLock<mpsc::Sender<CGPoint>> = OnceLock::new();

/// Which side of the divider a window is on.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Before,
    After,
}

struct Divider {
    vertical: bool,
    at: f64,
    windows: Vec<(Win, Side)>,
}

/// The edge between windows Brink placed next to each other, if the press is on one.
fn divider_at(p: CGPoint) -> Option<Divider> {
    let moved: Vec<(Win, Rect)> = MOVED.lock().iter().map(|(w, _, placed)| (w.clone(), *placed)).collect();
    for vertical in [true, false] {
        let (pos, along) = if vertical { (p.x, p.y) } else { (p.y, p.x) };
        let end = |r: &Rect| if vertical { r.right() } else { r.bottom() };
        let start = |r: &Rect| if vertical { r.x } else { r.y };
        let spans = |r: &Rect| if vertical { along >= r.y && along < r.bottom() } else { along >= r.x && along < r.right() };
        // Side by side with the pressed window, not just lined up with it somewhere else.
        let overlaps =
            |r: &Rect, o: &Rect| if vertical { o.y < r.bottom() && o.bottom() > r.y } else { o.x < r.right() && o.right() > r.x };
        for (_, r) in &moved {
            let at = end(r);
            if (pos - at).abs() > DIVIDER_REACH || !spans(r) {
                continue;
            }
            let windows: Vec<(Win, Side)> = moved
                .iter()
                .filter(|(_, o)| overlaps(r, o))
                .filter_map(|(w, o)| {
                    if (end(o) - at).abs() < 2.0 {
                        Some((w.clone(), Side::Before))
                    } else if (start(o) - at).abs() < 2.0 {
                        Some((w.clone(), Side::After))
                    } else {
                        None
                    }
                })
                .collect();
            if windows.iter().any(|(_, s)| *s == Side::After) {
                return Some(Divider { vertical, at, windows });
            }
        }
    }
    None
}

fn edge_of(d: &Divider, r: &Rect, side: Side) -> f64 {
    match (d.vertical, side) {
        (true, Side::Before) => r.right(),
        (true, Side::After) => r.x,
        (false, Side::Before) => r.bottom(),
        (false, Side::After) => r.y,
    }
}

/// Follows a drag on a divider: whichever window macOS is resizing leads, the windows on the
/// other side (and its neighbours on the same side) follow.
fn follow(mut d: Divider) {
    // Only windows still where Brink put them.
    let placed: Vec<(Win, Rect)> = MOVED.lock().iter().map(|(w, _, p)| (w.clone(), *p)).collect();
    d.windows.retain(|(w, _)| {
        let now = frame(w);
        placed.iter().any(|(m, p)| m.same(w) && now.is_some_and(|f| f.near(p)))
    });
    if !d.windows.iter().any(|(_, s)| *s == Side::Before) || !d.windows.iter().any(|(_, s)| *s == Side::After) {
        return;
    }
    log::info!("brink: dragging a divider between {} windows", d.windows.len());
    let edges = |frames: &[Option<Rect>]| -> Vec<Option<f64>> {
        d.windows.iter().zip(frames).map(|((_, side), f)| f.map(|f| edge_of(&d, &f, *side))).collect()
    };
    let first: Vec<Option<f64>> = edges(&d.windows.iter().map(|(w, _)| frame(w)).collect::<Vec<_>>());
    // The window macOS is resizing is the first whose edge moves; it leads for the whole drag
    // and the others follow. A click that doesn't drag moves nothing.
    let mut lead: Option<usize> = None;
    let mut at = d.at;
    let button_down =
        || LEFT_DOWN.load(Ordering::Relaxed) && unsafe { CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, 0) };
    while button_down() {
        std::thread::sleep(Duration::from_millis(15));
        let frames: Vec<Option<Rect>> = d.windows.iter().map(|(w, _)| frame(w)).collect();
        let now = edges(&frames);
        if lead.is_none() {
            lead = now.iter().zip(&first).position(|(n, f)| matches!((n, f), (Some(n), Some(f)) if (n - f).abs() > 1.0));
        }
        let Some(to) = lead.and_then(|i| now[i]) else { continue };
        if (to - at).abs() <= 1.0 {
            continue;
        }
        for (i, ((w, side), f)) in d.windows.iter().zip(&frames).enumerate() {
            let Some(f) = *f else { continue };
            // Apps that size in steps (Terminal) land a few points off; leave them be.
            if Some(i) == lead || (edge_of(&d, &f, *side) - to).abs() <= 3.0 {
                continue;
            }
            let r = match (d.vertical, side) {
                (true, Side::Before) => Rect { w: to - f.x, ..f },
                (true, Side::After) => Rect { x: to, w: f.right() - to, ..f },
                (false, Side::Before) => Rect { h: to - f.y, ..f },
                (false, Side::After) => Rect { y: to, h: f.bottom() - to, ..f },
            };
            set_frame(w, r);
        }
        at = to;
    }
    LEFT_DOWN.store(false, Ordering::Relaxed);
    // Remember the new sizes, so swiping down still puts them back.
    let mut moved = MOVED.lock();
    for (w, _) in &d.windows {
        if let (Some(f), Some(entry)) = (frame(w), moved.iter_mut().find(|(m, _, _)| m.same(w))) {
            entry.2 = f;
        }
    }
}

pub fn start_dividers(shared: Arc<Shared>) {
    let (tx, rx) = mpsc::channel::<CGPoint>();
    if DIVIDER_TX.set(tx).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("brink-dividers".into())
        .spawn(move || {
            for p in rx {
                if !shared.settings.read().drag_dividers || !LEFT_DOWN.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(d) = divider_at(p) {
                    follow(d);
                }
            }
        })
        .ok();
}

// ---------- keyboard ----------

/// Control-Option shortcuts, close to the usual window-manager ones.
fn key_action(code: u16) -> Option<Action> {
    const THIRD: f64 = 1.0 / 3.0;
    Some(match code {
        123 => Action::Swipe(Dir::Left),
        124 => Action::Swipe(Dir::Right),
        126 | 36 => Action::Swipe(Dir::Up),
        125 => Action::Swipe(Dir::Down),
        32 => Action::Swipe(Dir::UpLeft),                   // U
        34 => Action::Swipe(Dir::UpRight),                  // I
        38 => Action::Swipe(Dir::DownLeft),                 // J
        40 => Action::Swipe(Dir::DownRight),                // K
        2 => Action::Frac([0.0, 0.0, THIRD, 1.0]),          // D
        3 => Action::Frac([THIRD, 0.0, THIRD, 1.0]),        // F
        5 => Action::Frac([2.0 * THIRD, 0.0, THIRD, 1.0]),  // G
        14 => Action::Frac([0.0, 0.0, 2.0 * THIRD, 1.0]),   // E
        17 => Action::Frac([THIRD, 0.0, 2.0 * THIRD, 1.0]), // T
        8 => Action::Center,                                // C
        _ => return None,
    })
}

/// Called on the event tap's thread: decide fast, act elsewhere.
pub fn on_event(shared: &Arc<Shared>, h: Hook) -> bool {
    match h {
        Hook::Key(code) => {
            {
                let s = shared.settings.read();
                if !s.enabled || !s.window_keys {
                    return false;
                }
            }
            let Some(action) = key_action(code) else { return false };
            let shared = shared.clone();
            queue(move || {
                if let Some(w) = focused_window() {
                    act_on_window(&shared, w, action);
                }
            });
            true
        }
        Hook::LeftDown(p) => {
            LEFT_DOWN.store(true, Ordering::Relaxed);
            if let Some(tx) = DIVIDER_TX.get() {
                let _ = tx.send(p);
            }
            false
        }
        Hook::LeftUp => {
            LEFT_DOWN.store(false, Ordering::Relaxed);
            false
        }
    }
}
