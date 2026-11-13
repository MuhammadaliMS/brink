//! Scrubbing what's playing from the bottom strip, through MediaRemote (the private framework
//! behind the Now Playing widget).
//!
//! Since macOS 15.4 an app outside Apple can't read what's playing, so Brink asks a helper:
//! /usr/bin/perl (an Apple platform binary, allowed to read it) loading Brink's
//! libbrinknp.dylib (helper/nowplaying.m). Setting the position needs no such trick:
//! MRMediaRemoteSetElapsedTime works from Brink itself, and Chrome, Safari, Music, Spotify...
//! honour it. (They don't all honour "skip ±N seconds": YouTube in Chrome ignores it.)
//!
//! A slide begins by reading where the playhead is and how long the item is; from then on the
//! finger moves the playhead: the trackpad's width spans the whole item, at most ten minutes
//! (a quarter of that with Shift-Option). The HUD shows a seek bar with the time.

use super::levels::{Hud, Kind};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;

extern "C" {
    fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}

type SendCommand = unsafe extern "C" fn(u32, *const c_void) -> bool;
type SetElapsed = unsafe extern "C" fn(f64);

struct Remote {
    send: SendCommand,
    set_elapsed: SetElapsed,
}

fn remote() -> Option<&'static Remote> {
    static R: OnceLock<Option<Remote>> = OnceLock::new();
    R.get_or_init(|| unsafe {
        let lib = dlopen(c"/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote".as_ptr(), 2);
        if lib.is_null() {
            log::warn!("brink: MediaRemote unavailable, no scrubbing");
            return None;
        }
        let f = |n: &std::ffi::CStr| {
            let p = dlsym(lib, n.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(Remote {
            send: std::mem::transmute::<*mut c_void, SendCommand>(f(c"MRMediaRemoteSendCommand")?),
            set_elapsed: std::mem::transmute::<*mut c_void, SetElapsed>(f(c"MRMediaRemoteSetElapsedTime")?),
        })
    })
    .as_ref()
}

// ---------- reading what's playing ----------

struct Helper {
    _child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

static HELPER: Mutex<Option<Helper>> = Mutex::new(None);

fn dylib() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Brink.app/Contents/MacOS/brink -> Contents/Frameworks; `cargo run` uses the source copy.
    let bundled = exe.parent()?.parent()?.join("Frameworks/libbrinknp.dylib");
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("helper/libbrinknp.dylib");
    [bundled, dev].into_iter().find(|p| p.exists())
}

fn spawn() -> Option<Helper> {
    let lib = dylib()?;
    let script = r#"use DynaLoader; my $l = DynaLoader::dl_load_file($ARGV[0], 0) or die DynaLoader::dl_error(); my $s = DynaLoader::dl_find_symbol($l, "brink_np_loop") or die "no symbol"; DynaLoader::dl_install_xsub("main::np", $s); np();"#;
    let mut child = Command::new("/usr/bin/perl")
        .args(["-e", script])
        .arg(lib)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| log::warn!("brink: now-playing helper didn't start: {e}"))
        .ok()?;
    let stdin = child.stdin.take()?;
    let out = BufReader::new(child.stdout.take()?);
    Some(Helper { _child: child, stdin, out })
}

/// Sends one line to the helper (it doesn't answer "set"/"cmd"). False if it couldn't.
fn tell(line: &str) -> bool {
    let mut guard = HELPER.lock();
    for _ in 0..2 {
        if guard.is_none() {
            *guard = spawn();
        }
        let Some(h) = guard.as_mut() else { return false };
        if h.stdin.write_all(line.as_bytes()).and_then(|_| h.stdin.write_all(b"\n")).and_then(|_| h.stdin.flush()).is_ok() {
            return true;
        }
        *guard = None;
    }
    false
}

/// Moves the playhead of what's playing to `secs`.
fn set_position(secs: f64) {
    if !tell(&format!("set {secs:.3}")) {
        if let Some(r) = remote() {
            unsafe { (r.set_elapsed)(secs) };
        }
    }
}

/// What's playing: (seconds in, length in seconds). None when nothing is, or it can't be read.
pub fn now_playing() -> Option<(f64, f64)> {
    let v = ask()?;
    let duration = v.get("duration")?.as_f64()?;
    let elapsed = v.get("elapsed")?.as_f64()?;
    (duration > 0.0).then_some((elapsed.clamp(0.0, duration), duration))
}

/// Whether any app is in Now Playing, playing or paused. None when that can't be read.
fn anything_open() -> Option<bool> {
    let v = ask()?;
    if v.get("unknown").is_some() {
        return None;
    }
    Some(v.as_object().is_some_and(|o| !o.is_empty()))
}

/// Runs `send` (a play/pause, next or previous) only when an app is in Now Playing. With none,
/// macOS hands those to Apple Music and opens it, which a stray tap should never do. The check
/// talks to the helper, so it runs off the trackpad thread.
pub fn when_something_is_open(send: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        if anything_open() == Some(false) {
            log::info!("brink: nothing in Now Playing, media command skipped");
        } else {
            send();
        }
    });
}

/// One "get" to the helper, as JSON.
fn ask() -> Option<serde_json::Value> {
    let mut guard = HELPER.lock();
    for _ in 0..2 {
        if guard.is_none() {
            *guard = spawn();
        }
        let h = guard.as_mut()?;
        let mut line = String::new();
        let ok =
            h.stdin.write_all(b"get\n").and_then(|_| h.stdin.flush()).is_ok() && h.out.read_line(&mut line).map(|n| n > 0).unwrap_or(false);
        if !ok {
            // The helper died (or never ran): start a fresh one and ask once more.
            *guard = None;
            continue;
        }
        return serde_json::from_str(line.trim()).ok();
    }
    None
}

// ---------- scrubbing ----------

enum Msg {
    Start,
    Drag { delta: f32, fine: bool, hud: bool },
}

static APP: OnceLock<AppHandle> = OnceLock::new();

fn worker() -> &'static Mutex<Sender<Msg>> {
    static W: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = channel::<Msg>();
        std::thread::Builder::new()
            .name("brink-scrub".into())
            .spawn(move || {
                let mut scrub: Option<Scrub> = None;
                loop {
                    match rx.recv_timeout(Duration::from_millis(60)) {
                        Ok(Msg::Start) => scrub = None,
                        Ok(Msg::Drag { delta, fine, hud }) => {
                            let s = scrub.get_or_insert_with(Scrub::begin);
                            s.drag(delta, fine);
                            // Catch up on what queued while reading: only the sum matters.
                            while let Ok(m) = rx.try_recv() {
                                match m {
                                    Msg::Drag { delta, fine, .. } => s.drag(delta, fine),
                                    Msg::Start => {}
                                }
                            }
                            s.apply(false);
                            if hud {
                                if let Some(app) = APP.get() {
                                    super::hud::show(app, s.hud());
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if let Some(s) = scrub.as_mut() {
                                s.apply(true);
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    })
}

/// One slide's scrub.
struct Scrub {
    /// Where the playhead was and how long the item is, if it could be read.
    known: Option<(f64, f64)>,
    /// Seconds moved so far this slide.
    moved: f64,
    /// Seconds moved as last sent, and when.
    sent: f64,
    sent_at: Instant,
}

/// Seconds per full trackpad width when the length isn't known (seeking by skips).
const BLIND_SPAN: f64 = 120.0;

impl Scrub {
    fn begin() -> Scrub {
        let known = now_playing();
        log::info!("brink: scrub from {known:?}");
        Scrub { known, moved: 0.0, sent: 0.0, sent_at: Instant::now() - Duration::from_secs(1) }
    }

    fn drag(&mut self, delta: f32, fine: bool) {
        let span = match self.known {
            Some((_, dur)) => dur.clamp(60.0, 600.0).min(dur),
            None => BLIND_SPAN,
        };
        let span = if fine { span / 4.0 } else { span };
        self.moved += delta as f64 * span;
        if let Some((at, dur)) = self.known {
            // Stop at the ends: dragging back from past the end moves at once.
            self.moved = self.moved.clamp(-at, dur - at);
        }
    }

    /// Sends the new position, at most every 60 ms while dragging; `settle` sends what's left.
    fn apply(&mut self, settle: bool) {
        if (self.moved - self.sent).abs() < 0.25 || (!settle && self.sent_at.elapsed() < Duration::from_millis(60)) {
            return;
        }
        match self.known {
            Some((at, _)) => set_position(at + self.moved),
            None => {
                // Length unknown: skip by the difference, whole seconds (some apps ignore this).
                let step = (self.moved - self.sent).round();
                if step == 0.0 {
                    return;
                }
                skip(step as f32);
                self.sent += step;
                self.sent_at = Instant::now();
                return;
            }
        }
        self.sent = self.moved;
        self.sent_at = Instant::now();
    }

    fn hud(&self) -> Hud {
        match self.known {
            Some((at, dur)) => {
                let now = at + self.moved;
                Hud { kind: Kind::Seek, level: (now / dur) as f32, muted: false, secs: now as f32 }
            }
            None => Hud { kind: Kind::Seek, level: -1.0, muted: false, secs: self.moved as f32 },
        }
    }
}

const TOGGLE_PLAY_PAUSE: u32 = 2;
const NEXT: u32 = 4;
const PREVIOUS: u32 = 5;

/// A plain Now Playing command (next, previous...), as Control Center's buttons send.
fn command(c: u32) -> bool {
    tell(&format!("cmd {c}")) || remote().is_some_and(|r| unsafe { (r.send)(c, std::ptr::null()) })
}

/// Skips `secs` seconds (negative: back) with the Now Playing skip command.
fn skip(secs: f32) -> bool {
    use objc2::rc::Retained;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    let Some(r) = remote() else { return false };
    let key = NSString::from_str("kMRMediaRemoteOptionSkipInterval");
    let value = NSNumber::new_f64(secs.abs() as f64);
    let options: Retained<NSDictionary<NSString, NSNumber>> = NSDictionary::from_retained_objects(&[&*key], &[value]);
    let command = if secs >= 0.0 { 17 } else { 18 };
    unsafe { (r.send)(command, Retained::as_ptr(&options) as *const c_void) }
}

/// Starts the helper early, so the first slide doesn't wait for perl to load.
pub fn start(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| {
        let mut h = HELPER.lock();
        if h.is_none() {
            *h = spawn();
        }
    });
}

/// The bottom strip's player buttons, sent as Control Center's buttons send them.
pub fn button(key: crate::engine::Key) {
    use crate::engine::Key;
    let c = match key {
        Key::Previous => PREVIOUS,
        Key::Next => NEXT,
        _ => TOGGLE_PLAY_PAUSE,
    };
    when_something_is_open(move || {
        command(c);
    });
}

pub fn scrub_start() {
    worker().lock().send(Msg::Start).ok();
}

pub fn scrub(delta: f32, fine: bool, hud: bool) {
    worker().lock().send(Msg::Drag { delta, fine, hud }).ok();
}

#[cfg(test)]
mod tests {
    /// Needs something playing: `cargo test --lib reads_what_is_playing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_what_is_playing() {
        let t = std::time::Instant::now();
        println!("now playing {:?} in {:?}", super::now_playing(), t.elapsed());
        let t = std::time::Instant::now();
        println!("again {:?} in {:?}", super::now_playing(), t.elapsed());
    }
}

#[cfg(test)]
mod scrub_tests {
    /// Moves what's playing ahead and back: `cargo test --lib scrubs_what_is_playing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn scrubs_what_is_playing() {
        let before = super::now_playing();
        println!("before {before:?}");
        super::scrub_start();
        for _ in 0..10 {
            super::scrub(0.01, false, false);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        println!("after +0.1 width {:?}", super::now_playing());
        for _ in 0..10 {
            super::scrub(-0.01, false, false);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        println!("back {:?}", super::now_playing());
    }
}
