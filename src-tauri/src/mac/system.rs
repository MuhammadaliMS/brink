//! Reading the current levels for the menu, and the Accessibility check.

use super::ffi::*;
use std::ffi::c_void;

const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");

pub fn default_output() -> Option<u32> {
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"dOut"), scope: SCOPE_GLOBAL, element: 0 };
    let mut id: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size, &mut id as *mut u32 as *mut c_void) };
    (r == 0 && id != 0).then_some(id)
}

/// Output volume 0..1 (the value the volume HUD shows).
pub fn volume() -> Option<f32> {
    let dev = default_output()?;
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"vmvc"), scope: SCOPE_OUTPUT, element: 0 };
    let mut v: f32 = 0.0;
    let mut size = 4u32;
    let r = unsafe { AudioHardwareServiceGetPropertyData(dev, &addr, 0, std::ptr::null(), &mut size, &mut v as *mut f32 as *mut c_void) };
    (r == 0).then_some(v.clamp(0.0, 1.0))
}

pub fn muted() -> bool {
    let Some(dev) = default_output() else { return false };
    let addr = AudioObjectPropertyAddress { selector: fourcc(b"mute"), scope: SCOPE_OUTPUT, element: 0 };
    if !unsafe { AudioObjectHasProperty(dev, &addr) } {
        return false;
    }
    let mut m: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { AudioObjectGetPropertyData(dev, &addr, 0, std::ptr::null(), &mut size, &mut m as *mut u32 as *mut c_void) };
    r == 0 && m != 0
}

fn builtin_display() -> u32 {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    unsafe {
        if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for &d in &ids[..n as usize] {
                if CGDisplayIsBuiltin(d) != 0 {
                    return d;
                }
            }
        }
        CGMainDisplayID()
    }
}

/// Built-in display brightness 0..1.
pub fn brightness() -> Option<f32> {
    let f = display().get_brightness?;
    let mut b: f32 = 0.0;
    let r = unsafe { f(builtin_display(), &mut b) };
    (r == 0).then_some(b.clamp(0.0, 1.0))
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Shows the system "allow Accessibility" prompt (once per launch, macOS decides).
pub fn request_trust() -> bool {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let ok = AXIsProcessTrustedWithOptions(dict);
        CFRelease(dict);
        ok
    }
}

pub fn modifiers() -> crate::engine::Mods {
    let f = unsafe { CGEventSourceFlagsState(kCGEventSourceStateCombinedSessionState) };
    crate::engine::Mods {
        fn_or_option: f & (kCGEventFlagMaskSecondaryFn | kCGEventFlagMaskAlternate) != 0,
        shift_option: f & kCGEventFlagMaskShift != 0 && f & kCGEventFlagMaskAlternate != 0,
    }
}

/// The current desktop picture as a JPEG the webview can always decode (HEIC and dynamic
/// `.madesktop` wallpapers are converted once with `sips` and cached).
pub fn wallpaper_jpeg() -> Option<Vec<u8>> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::{c_char, CStr};
    use std::path::PathBuf;

    let path = unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
        if ws.is_null() || screen.is_null() {
            return None;
        }
        let url: *mut AnyObject = msg_send![ws, desktopImageURLForScreen: screen];
        if url.is_null() {
            return None;
        }
        let p: *mut AnyObject = msg_send![url, path];
        let s: *const c_char = msg_send![p, UTF8String];
        if s.is_null() {
            return None;
        }
        PathBuf::from(CStr::from_ptr(s).to_string_lossy().into_owned())
    };
    let mut src = std::fs::canonicalize(&path).unwrap_or(path);
    if src.extension().is_some_and(|e| e == "madesktop") {
        let out = std::process::Command::new("plutil").args(["-extract", "thumbnailPath", "raw", "-o", "-"]).arg(&src).output().ok()?;
        src = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let meta = std::fs::metadata(&src).ok()?;
    if !meta.is_file() || meta.len() > 80 << 20 {
        return None;
    }
    let stamp = format!("{}-{}", src.display(), meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs());
    let cache = PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches/in.zerowing.brink");
    let _ = std::fs::create_dir_all(&cache);
    let jpg = cache.join("wallpaper.jpg");
    let key = cache.join("wallpaper.key");
    if std::fs::read_to_string(&key).ok().as_deref() != Some(stamp.as_str()) || !jpg.exists() {
        let ok = std::process::Command::new("sips")
            .args(["-s", "format", "jpeg", "-s", "formatOptions", "85", "-Z", "2560"])
            .arg(&src)
            .arg("--out")
            .arg(&jpg)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return None;
        }
        let _ = std::fs::write(&key, &stamp);
    }
    std::fs::read(jpg).ok()
}
