//! External monitor brightness over DDC/CI, on Apple silicon (IOAVService I2C, the same route
//! BetterDisplay, MonitorControl and m1ddc take). Reads happen once per monitor; writes go to a
//! worker thread that only ever sends the latest level, since each one takes ~20 ms.

use super::ffi::{cfstr, CFRelease, CFStringRef, CFTypeRef};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Service = *mut c_void;
type IoObject = u32;

struct Api {
    create: unsafe extern "C" fn(*const c_void, IoObject) -> Service,
    read: unsafe extern "C" fn(Service, u32, u32, *mut u8, u32) -> i32,
    write: unsafe extern "C" fn(Service, u32, u32, *const u8, u32) -> i32,
}

extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn IORegistryGetRootEntry(main_port: u32) -> IoObject;
    fn IORegistryEntryCreateIterator(entry: IoObject, plane: *const c_char, options: u32, iter: *mut IoObject) -> i32;
    fn IOIteratorNext(iter: IoObject) -> IoObject;
    fn IOObjectRelease(obj: IoObject) -> i32;
    fn IOObjectGetClass(obj: IoObject, name: *mut c_char) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: IoObject, key: CFStringRef, alloc: *const c_void, options: u32) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetValue(d: CFTypeRef, key: *const c_void) -> CFTypeRef;
    fn CFNumberGetValue(n: CFTypeRef, kind: isize, out: *mut c_void) -> bool;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, enc: u32) -> bool;
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsBuiltin(d: u32) -> u32;
    fn CGDisplayVendorNumber(d: u32) -> u32;
    fn CGDisplayModelNumber(d: u32) -> u32;
    fn CGDisplaySerialNumber(d: u32) -> u32;
    fn CGGetDisplayTransferByTable(d: u32, capacity: u32, r: *mut f32, g: *mut f32, b: *mut f32, count: *mut u32) -> i32;
    fn CGSetDisplayTransferByTable(d: u32, size: u32, r: *const f32, g: *const f32, b: *const f32) -> i32;
    fn CGDisplayRestoreColorSyncSettings();
}

fn api() -> Option<&'static Api> {
    static A: OnceLock<Option<Api>> = OnceLock::new();
    A.get_or_init(|| unsafe {
        let lib = dlopen(c"/System/Library/Frameworks/IOKit.framework/IOKit".as_ptr(), 2);
        if lib.is_null() {
            return None;
        }
        let f = |n: &CStr| {
            let p = dlsym(lib, n.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(Api {
            create: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*const c_void, IoObject) -> Service>(f(
                c"IOAVServiceCreateWithService",
            )?),
            read: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Service, u32, u32, *mut u8, u32) -> i32>(f(
                c"IOAVServiceReadI2C",
            )?),
            write: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Service, u32, u32, *const u8, u32) -> i32>(f(
                c"IOAVServiceWriteI2C",
            )?),
        })
    })
    .as_ref()
}

/// What the registry says about the display a DCP AV service drives.
#[derive(Default, Clone, Debug)]
struct Attrs {
    vendor: u32,
    product: u32,
    serial: u32,
}

struct Monitor {
    attrs: Option<Attrs>,
    service: Service,
}

#[derive(Default)]
struct State {
    /// Online displays (id, vendor, model, serial) the monitor list was built for. Not just ids:
    /// macOS can give a newly plugged-in monitor the id the last one had.
    online: Vec<(u32, u32, u32, u32)>,
    monitors: Vec<Monitor>,
    /// Display id -> index in `monitors`.
    map: HashMap<u32, usize>,
    /// Display id -> (current, max) as last read or set, and when.
    levels: HashMap<u32, (u16, u16, Instant)>,
    /// Displays whose monitor didn't answer a DDC read, and when: left to the key for a while.
    deaf: HashMap<u32, Instant>,
    /// Software dimming below the monitor's own 0: display id -> (factor, its original gamma).
    dim: HashMap<u32, (f32, Gamma)>,
}

/// A display's gamma table as ColorSync set it, to scale from.
#[derive(Clone)]
struct Gamma {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
}

impl Gamma {
    fn of(d: u32) -> Option<Gamma> {
        let (mut r, mut g, mut b) = (vec![0f32; 256], vec![0f32; 256], vec![0f32; 256]);
        let mut n = 0u32;
        if unsafe { CGGetDisplayTransferByTable(d, 256, r.as_mut_ptr(), g.as_mut_ptr(), b.as_mut_ptr(), &mut n) } != 0 || n == 0 {
            return None;
        }
        for t in [&mut r, &mut g, &mut b] {
            t.truncate(n as usize);
        }
        Some(Gamma { r, g, b })
    }

    /// Shows the display at `factor` of its brightness. macOS puts the table back if Brink quits.
    fn apply(&self, d: u32, factor: f32) {
        let scale = |t: &[f32]| t.iter().map(|v| v * factor).collect::<Vec<f32>>();
        let (r, g, b) = (scale(&self.r), scale(&self.g), scale(&self.b));
        unsafe { CGSetDisplayTransferByTable(d, r.len() as u32, r.as_ptr(), g.as_ptr(), b.as_ptr()) };
    }
}

/// The bottom of the slide (on the key grid) dims in software once the monitor is at its own 0,
/// which is only its dimmest backlight. At the very bottom the picture keeps this much light.
const SOFT: f32 = 0.25;
const FLOOR: f32 = 0.12;

/// Slide level 0..1 for a monitor at `hw` of its backlight with software `factor` on top.
fn combined(hw: f32, factor: f32) -> f32 {
    if hw <= 0.0 && factor < 1.0 {
        SOFT * ((factor - FLOOR) / (1.0 - FLOOR)).clamp(0.0, 1.0)
    } else {
        SOFT + (1.0 - SOFT) * hw
    }
}

/// The other way: (backlight 0..1, software factor) for a slide level.
fn split(level: f32) -> (f32, f32) {
    if level >= SOFT {
        ((level - SOFT) / (1.0 - SOFT), 1.0)
    } else {
        (0.0, FLOOR + (1.0 - FLOOR) * level / SOFT)
    }
}
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Set when a write fails: the service may belong to a monitor that's gone, so scan again.
static RESCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Job(Service, u16);
unsafe impl Send for Job {}

fn writer() -> &'static Mutex<Sender<Job>> {
    static W: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("brink-ddc".into())
            .spawn(move || {
                while let Ok(mut job) = rx.recv() {
                    // A fast slide queues many levels: only the newest matters.
                    while let Ok(newer) = rx.try_recv() {
                        job = newer;
                    }
                    if !write_vcp(job.0, 0x10, job.1) {
                        RESCAN.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    })
}

/// After this long without a step, read the monitor again: its own buttons may have moved it.
const STALE: Duration = Duration::from_secs(3);

/// One brightness step on the monitor showing display `d`. Returns the new level 0..1, or None
/// when Brink can't drive that monitor (no DDC/CI answer), so the caller presses the key.
pub fn step(d: u32, up: bool, quarter: bool) -> Option<f32> {
    let mut guard = STATE.lock();
    let st = guard.get_or_insert_with(State::default);
    refresh(st);
    if st.deaf.get(&d).is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
        return None;
    }
    let service = st.monitors.get(*st.map.get(&d)?)?.service;
    let cached = st.levels.get(&d).copied();
    let (cur, max) = match cached {
        Some((c, m, at)) if at.elapsed() < STALE => (c, m),
        _ => match read_vcp(service, 0x10) {
            Some(v) => v,
            // A slow answer mid-session: keep going from where we were.
            None if cached.is_some() => cached.map(|(c, m, _)| (c, m))?,
            // Never answered: this monitor doesn't speak DDC/CI (or has it switched off).
            None => {
                log::info!("brink: ddc no answer from display {d}, using the key");
                st.deaf.insert(d, Instant::now());
                return None;
            }
        },
    };
    let factor = st.dim.get(&d).map_or(1.0, |v| v.0);
    let level = super::levels::next(combined(cur as f32 / max as f32, factor), up, quarter);
    let (hw, soft) = split(level);
    if soft < 1.0 {
        if let std::collections::hash_map::Entry::Vacant(e) = st.dim.entry(d) {
            if let Some(g) = Gamma::of(d) {
                e.insert((1.0, g));
            }
        }
        if let Some(v) = st.dim.get_mut(&d) {
            v.0 = soft;
            v.1.apply(d, soft);
        }
    } else if let Some((_, g)) = st.dim.remove(&d) {
        g.apply(d, 1.0);
    }
    let value = (hw * max as f32).round() as u16;
    st.levels.insert(d, (value, max, Instant::now()));
    drop(guard);
    writer().lock().send(Job(service, value)).ok()?;
    Some(level)
}

/// Rebuilds the monitor list when displays come or go.
fn refresh(st: &mut State) {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    if unsafe { CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut n) } != 0 {
        return;
    }
    let online: Vec<(u32, u32, u32, u32)> = ids[..n as usize]
        .iter()
        .map(|&d| unsafe { (d, CGDisplayVendorNumber(d), CGDisplayModelNumber(d), CGDisplaySerialNumber(d)) })
        .collect();
    let rescan = RESCAN.swap(false, std::sync::atomic::Ordering::Relaxed);
    if online == st.online && !st.monitors.is_empty() && !rescan {
        return;
    }
    for m in st.monitors.drain(..) {
        unsafe { CFRelease(m.service as CFTypeRef) };
    }
    st.map.clear();
    st.levels.clear();
    st.deaf.clear();
    // A display came or went: any software dimming belongs to a layout that's gone.
    if !st.dim.is_empty() {
        st.dim.clear();
        unsafe { CGDisplayRestoreColorSyncSettings() };
    }
    st.online = online.clone();
    st.monitors = scan();

    let externals: Vec<(u32, u32, u32, u32)> = online.iter().copied().filter(|&(d, ..)| unsafe { CGDisplayIsBuiltin(d) } == 0).collect();
    for &(d, vendor, product, serial) in &externals {
        let hit = st.monitors.iter().position(|m| {
            m.attrs.as_ref().is_some_and(|a| {
                a.product == product && (a.vendor == 0 || a.vendor == vendor) && (a.serial == 0 || serial == 0 || a.serial == serial)
            })
        });
        if let Some(i) = hit {
            st.map.insert(d, i);
        }
    }
    // One monitor and one external display: they're each other, whatever the registry says.
    if st.map.is_empty() && st.monitors.len() == 1 && externals.len() == 1 {
        st.map.insert(externals[0].0, 0);
    }
    log::info!("brink: ddc monitors {} mapped {:?}", st.monitors.len(), st.map.keys().collect::<Vec<_>>());
}

/// Walks the IOService plane: each framebuffer's DisplayAttributes come just before the DCP AV
/// service that drives the same port.
fn scan() -> Vec<Monitor> {
    let Some(api) = api() else { return Vec::new() };
    let mut out = Vec::new();
    unsafe {
        let root = IORegistryGetRootEntry(0);
        let mut iter: IoObject = 0;
        if IORegistryEntryCreateIterator(root, c"IOService".as_ptr(), 1, &mut iter) != 0 {
            return out;
        }
        let mut last: Option<Attrs> = None;
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            let mut name = [0 as c_char; 128];
            IOObjectGetClass(entry, name.as_mut_ptr());
            let class = CStr::from_ptr(name.as_ptr()).to_string_lossy();
            if class == "AppleCLCD2" || class == "IOMobileFramebufferShim" {
                if let Some(a) = attrs(entry) {
                    last = Some(a);
                }
            } else if class == "DCPAVServiceProxy" && string_prop(entry, "Location").as_deref() == Some("External") {
                let service = (api.create)(std::ptr::null(), entry);
                if !service.is_null() {
                    out.push(Monitor { attrs: last.take(), service });
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);
    }
    out
}

unsafe fn prop(entry: IoObject, key: &str) -> CFTypeRef {
    let k = cfstr(key);
    let v = IORegistryEntryCreateCFProperty(entry, k, std::ptr::null(), 0);
    CFRelease(k);
    v
}

unsafe fn string_of(v: CFTypeRef) -> Option<String> {
    if v.is_null() || CFGetTypeID(v) != CFStringGetTypeID() {
        return None;
    }
    let mut buf = [0 as c_char; 64];
    CFStringGetCString(v, buf.as_mut_ptr(), 64, 0x0800_0100).then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
}

unsafe fn number_of(v: CFTypeRef) -> u32 {
    let mut n: i64 = 0;
    if !v.is_null() && CFGetTypeID(v) == CFNumberGetTypeID() {
        CFNumberGetValue(v, 4 /* kCFNumberSInt64Type */, &mut n as *mut i64 as *mut c_void);
    }
    n as u32
}

unsafe fn string_prop(entry: IoObject, key: &str) -> Option<String> {
    let v = prop(entry, key);
    let s = string_of(v);
    if !v.is_null() {
        CFRelease(v);
    }
    s
}

unsafe fn attrs(entry: IoObject) -> Option<Attrs> {
    let da = prop(entry, "DisplayAttributes");
    if da.is_null() {
        return None;
    }
    let get = |d: CFTypeRef, k: &str| {
        let key = cfstr(k);
        let v = CFDictionaryGetValue(d, key);
        CFRelease(key);
        v
    };
    let mut out = None;
    if CFGetTypeID(da) == CFDictionaryGetTypeID() {
        let pa = get(da, "ProductAttributes");
        if !pa.is_null() && CFGetTypeID(pa) == CFDictionaryGetTypeID() {
            // "GSM" -> the EISA number CGDisplayVendorNumber reports.
            let vendor = string_of(get(pa, "ManufacturerID"))
                .filter(|s| s.len() == 3)
                .map(|s| s.bytes().fold(0u32, |acc, c| (acc << 5) | (c.saturating_sub(64) as u32 & 31)))
                .unwrap_or(0);
            out = Some(Attrs { vendor, product: number_of(get(pa, "ProductID")), serial: number_of(get(pa, "SerialNumber")) });
        }
    }
    CFRelease(da);
    out
}

// ---------- the DDC/CI wire format ----------

const ADDR: u32 = 0x37;
const SUB: u32 = 0x51;

fn checksum(p: &[u8]) -> u8 {
    p.iter().fold(0x6E ^ SUB as u8, |a, b| a ^ b)
}

/// Get VCP feature: (current, max).
fn read_vcp(service: Service, code: u8) -> Option<(u16, u16)> {
    let api = api()?;
    let mut p = [0x82u8, 0x01, code, 0];
    p[3] = checksum(&p[..3]);
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(10));
        if unsafe { (api.write)(service, ADDR, SUB, p.as_ptr(), p.len() as u32) } != 0 {
            continue;
        }
        std::thread::sleep(Duration::from_millis(50));
        let mut r = [0u8; 12];
        if unsafe { (api.read)(service, ADDR, SUB, r.as_mut_ptr(), r.len() as u32) } != 0 {
            continue;
        }
        // 0x02: VCP reply, 0x00: supported, then the code we asked for.
        if r[2] == 0x02 && r[3] == 0x00 && r[4] == code {
            let max = u16::from_be_bytes([r[6], r[7]]);
            let cur = u16::from_be_bytes([r[8], r[9]]);
            if max > 0 {
                return Some((cur.min(max), max));
            }
        }
    }
    None
}

/// Set VCP feature. Sent twice: some monitors miss the first one. False when neither went out.
fn write_vcp(service: Service, code: u8, value: u16) -> bool {
    let Some(api) = api() else { return false };
    let [hi, lo] = value.to_be_bytes();
    let mut p = [0x84u8, 0x03, code, hi, lo, 0];
    p[5] = checksum(&p[..5]);
    let mut sent = false;
    for _ in 0..2 {
        std::thread::sleep(Duration::from_millis(10));
        sent |= unsafe { (api.write)(service, ADDR, SUB, p.as_ptr(), p.len() as u32) } == 0;
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sets 0 for 3 s and reads it back: `cargo test --lib monitor_floor -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn monitor_floor() {
        let mut st = State::default();
        refresh(&mut st);
        for m in &st.monitors {
            let was = read_vcp(m.service, 0x10);
            println!("was {was:?}, write 0 -> {}", write_vcp(m.service, 0x10, 0));
            std::thread::sleep(Duration::from_millis(500));
            println!("reads {:?}", read_vcp(m.service, 0x10));
            std::thread::sleep(Duration::from_millis(2500));
            if let Some((cur, _)) = was {
                write_vcp(m.service, 0x10, cur);
            }
        }
    }

    /// Dims the external display in software for 2 s: `cargo test --lib soft_dim -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn soft_dim() {
        let mut st = State::default();
        refresh(&mut st);
        for &(d, ..) in st.online.iter().filter(|o| unsafe { CGDisplayIsBuiltin(o.0) } == 0) {
            let g = Gamma::of(d).expect("gamma table");
            println!("display {d}: {} entries, top {:?}", g.r.len(), g.r.last());
            g.apply(d, FLOOR);
            let now = Gamma::of(d).unwrap();
            println!("dimmed top {:?}", now.r.last());
            std::thread::sleep(Duration::from_millis(2000));
            g.apply(d, 1.0);
            println!("restored top {:?}", Gamma::of(d).unwrap().r.last());
        }
    }

    /// Talks to the real monitor: `cargo test talks_to_the_monitor -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn talks_to_the_monitor() {
        let mut st = State::default();
        refresh(&mut st);
        println!(
            "monitors {} attrs {:?} map {:?}",
            st.monitors.len(),
            st.monitors.iter().map(|m| m.attrs.clone()).collect::<Vec<_>>(),
            st.map
        );
        let externals: Vec<u32> = st.online.iter().map(|o| o.0).filter(|&d| unsafe { CGDisplayIsBuiltin(d) } == 0).collect();
        for d in externals {
            println!(
                "display {d} vendor {} model {} serial {}",
                unsafe { CGDisplayVendorNumber(d) },
                unsafe { CGDisplayModelNumber(d) },
                unsafe { CGDisplaySerialNumber(d) }
            );
        }
        for m in &st.monitors {
            let b = read_vcp(m.service, 0x10);
            println!("brightness {b:?} contrast {:?} volume {:?}", read_vcp(m.service, 0x12), read_vcp(m.service, 0x62));
            if let Some((cur, max)) = b {
                let low = cur.saturating_sub(max / 4);
                write_vcp(m.service, 0x10, low);
                std::thread::sleep(Duration::from_millis(400));
                println!("after write {low}: {:?}", read_vcp(m.service, 0x10));
                std::thread::sleep(Duration::from_millis(1200));
                write_vcp(m.service, 0x10, cur);
                std::thread::sleep(Duration::from_millis(400));
                println!("restored: {:?}", read_vcp(m.service, 0x10));
            }
        }
    }

    #[test]
    fn the_bottom_of_the_slide_dims_in_software() {
        assert_eq!(split(1.0), (1.0, 1.0));
        assert_eq!(split(SOFT), (0.0, 1.0));
        assert_eq!(split(0.0), (0.0, FLOOR));
        // Round trips on the key grid, so steps never get stuck.
        for i in 0..=16 {
            let l = i as f32 / 16.0;
            let (hw, f) = split(l);
            assert!((combined(hw, f) - l).abs() < 1e-5, "{l}");
        }
        // A monitor read at 0 with no dimming sits at the switch-over point.
        assert_eq!(combined(0.0, 1.0), SOFT);
    }

    #[test]
    fn packets_match_the_ddc_spec() {
        // Get brightness: 6E 51 82 01 10 AC (the documented example).
        assert_eq!(checksum(&[0x82, 0x01, 0x10]), 0xAC);
    }
}
