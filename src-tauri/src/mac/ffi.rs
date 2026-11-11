//! Raw bindings. Public frameworks are linked in build.rs; the two private ones
//! (MultitouchSupport for raw trackpad contacts + the haptic actuator, DisplayServices
//! for reading brightness) are resolved with dlopen so a missing symbol degrades
//! gracefully instead of failing to launch.
#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

pub type CFTypeRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;
pub type CGEventRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type MTDeviceRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

// ---------- MultitouchSupport ----------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTPoint {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTVector {
    pub pos: MTPoint,
    pub vel: MTPoint,
}

/// Layout reverse-engineered by the community (see OpenMultitouchSupport); stable since 10.6.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTTouch {
    pub frame: i32,
    pub timestamp: f64,
    pub identifier: i32,
    pub state: i32,
    pub finger_id: i32,
    pub hand_id: i32,
    pub normalized: MTVector,
    pub z_total: f32,
    pub field9: i32,
    pub angle: f32,
    pub major_axis: f32,
    pub minor_axis: f32,
    pub absolute: MTVector,
    pub field14: i32,
    pub field15: i32,
    pub z_density: f32,
}

pub type MTContactCallback = extern "C" fn(MTDeviceRef, *const MTTouch, i32, f64, i32) -> i32;

pub struct Multitouch {
    pub create_list: unsafe extern "C" fn() -> CFArrayRef,
    pub register: unsafe extern "C" fn(MTDeviceRef, MTContactCallback),
    pub unregister: unsafe extern "C" fn(MTDeviceRef, MTContactCallback),
    pub start: unsafe extern "C" fn(MTDeviceRef, i32) -> i32,
    pub stop: unsafe extern "C" fn(MTDeviceRef) -> i32,
    pub is_running: Option<unsafe extern "C" fn(MTDeviceRef) -> bool>,
    pub is_builtin: Option<unsafe extern "C" fn(MTDeviceRef) -> bool>,
    pub get_device_id: Option<unsafe extern "C" fn(MTDeviceRef, *mut u64) -> i32>,
    pub get_family_id: Option<unsafe extern "C" fn(MTDeviceRef, *mut i32) -> i32>,
    pub actuator_create: Option<unsafe extern "C" fn(u64) -> CFTypeRef>,
    pub actuator_open: Option<unsafe extern "C" fn(CFTypeRef) -> i32>,
    pub actuator_actuate: Option<unsafe extern "C" fn(CFTypeRef, i32, u32, f32, f32) -> i32>,
}

unsafe impl Send for Multitouch {}
unsafe impl Sync for Multitouch {}

pub struct Display {
    pub get_brightness: Option<unsafe extern "C" fn(u32, *mut f32) -> i32>,
    pub set_brightness: Option<unsafe extern "C" fn(u32, f32) -> i32>,
    /// True for the built-in display and Apple displays (not for DDC monitors).
    pub can_change: Option<unsafe extern "C" fn(u32) -> bool>,
}

pub struct Skylight {
    pub default_connection: Option<unsafe extern "C" fn() -> i32>,
    pub set_connection_property: Option<unsafe extern "C" fn(i32, i32, CFStringRef, CFTypeRef) -> i32>,
}

extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_NOW: i32 = 0x2;

pub unsafe fn dlopen_now(path: *const c_char) -> *mut c_void {
    dlopen(path, RTLD_NOW)
}

fn open(path: &str) -> *mut c_void {
    let c = CString::new(path).unwrap();
    unsafe { dlopen(c.as_ptr(), RTLD_NOW) }
}

unsafe fn sym<T: Copy>(lib: *mut c_void, name: &str) -> Option<T> {
    if lib.is_null() {
        return None;
    }
    let c = CString::new(name).unwrap();
    let p = dlsym(lib, c.as_ptr());
    if p.is_null() {
        None
    } else {
        Some(std::mem::transmute_copy::<*mut c_void, T>(&p))
    }
}

pub fn multitouch() -> Option<&'static Multitouch> {
    static MT: OnceLock<Option<Multitouch>> = OnceLock::new();
    MT.get_or_init(|| unsafe {
        let lib = open("/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport");
        Some(Multitouch {
            create_list: sym(lib, "MTDeviceCreateList")?,
            register: sym(lib, "MTRegisterContactFrameCallback")?,
            unregister: sym(lib, "MTUnregisterContactFrameCallback")?,
            start: sym(lib, "MTDeviceStart")?,
            stop: sym(lib, "MTDeviceStop")?,
            is_running: sym(lib, "MTDeviceIsRunning"),
            is_builtin: sym(lib, "MTDeviceIsBuiltIn"),
            get_device_id: sym(lib, "MTDeviceGetDeviceID"),
            get_family_id: sym(lib, "MTDeviceGetFamilyID"),
            actuator_create: sym(lib, "MTActuatorCreateFromDeviceID"),
            actuator_open: sym(lib, "MTActuatorOpen"),
            actuator_actuate: sym(lib, "MTActuatorActuate"),
        })
    })
    .as_ref()
}

pub fn display() -> &'static Display {
    static D: OnceLock<Display> = OnceLock::new();
    D.get_or_init(|| unsafe {
        let lib = open("/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices");
        Display {
            get_brightness: sym(lib, "DisplayServicesGetBrightness"),
            set_brightness: sym(lib, "DisplayServicesSetBrightness"),
            can_change: sym(lib, "DisplayServicesCanChangeBrightness"),
        }
    })
}

pub fn skylight() -> &'static Skylight {
    static S: OnceLock<Skylight> = OnceLock::new();
    S.get_or_init(|| unsafe {
        let mut lib = open("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight");
        if lib.is_null() {
            lib = open("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices");
        }
        Skylight {
            default_connection: sym(lib, "_CGSDefaultConnection").or_else(|| sym(lib, "SLSMainConnectionID")),
            set_connection_property: sym(lib, "CGSSetConnectionProperty").or_else(|| sym(lib, "SLSSetConnectionProperty")),
        }
    })
}

// ---------- CoreFoundation ----------

#[repr(C)]
pub struct CFDictionaryKeyCallBacks {
    _private: [u8; 0],
}

extern "C" {
    pub fn CFArrayGetCount(a: CFArrayRef) -> isize;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, i: isize) -> *const c_void;
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, enc: u32) -> CFStringRef;
    pub fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: isize,
        kcb: *const CFDictionaryKeyCallBacks,
        vcb: *const c_void,
    ) -> CFDictionaryRef;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopRun();
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;

    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    pub static kCFTypeDictionaryValueCallBacks: c_void;
}

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

pub fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), kCFStringEncodingUTF8) }
}

// ---------- CoreGraphics / ApplicationServices ----------

pub type CGEventTapCallBack = extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

extern "C" {
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventSourceFlagsState(state: i32) -> u64;
    pub fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    pub fn CGEventCreate(source: *const c_void) -> CGEventRef;
    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGEventCreateCopy(event: CGEventRef) -> CGEventRef;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    pub fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
    pub fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    pub fn CGDisplayHideCursor(display: u32) -> i32;
    pub fn CGDisplayShowCursor(display: u32) -> i32;
    pub fn CGMainDisplayID() -> u32;
    pub fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    pub fn CGDisplayIsBuiltin(display: u32) -> u32;
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: CGEventTapCallBack,
        user: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);

    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;
}

pub const kCGHIDEventTap: u32 = 0;
pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionDefault: u32 = 0;
pub const kCGEventSourceStateCombinedSessionState: i32 = 0;

pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventLeftMouseUp: u32 = 2;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventRightMouseUp: u32 = 4;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventLeftMouseDragged: u32 = 6;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventKeyUp: u32 = 11;
pub const kCGKeyboardEventAutorepeat: u32 = 8;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGEventSourceUserData: u32 = 42;
pub const kCGEventFlagMaskControl: u64 = 0x0004_0000;
pub const kCGEventFlagMaskCommand: u64 = 0x0010_0000;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;

pub const kCGEventFlagMaskShift: u64 = 0x0002_0000;
pub const kCGEventFlagMaskAlternate: u64 = 0x0008_0000;
pub const kCGEventFlagMaskSecondaryFn: u64 = 0x0080_0000;

// ---------- CoreAudio ----------

#[repr(C)]
pub struct AudioObjectPropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

extern "C" {
    pub fn AudioObjectGetPropertyData(
        obj: u32,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        io_size: *mut u32,
        out: *mut c_void,
    ) -> i32;
    pub fn AudioObjectHasProperty(obj: u32, addr: *const AudioObjectPropertyAddress) -> bool;
    pub fn AudioHardwareServiceGetPropertyData(
        obj: u32,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        io_size: *mut u32,
        out: *mut c_void,
    ) -> i32;
}

pub const fn fourcc(s: &[u8; 4]) -> u32 {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
}
