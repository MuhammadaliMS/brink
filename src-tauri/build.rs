fn main() {
    // Public frameworks only. MultitouchSupport and DisplayServices are private, so they
    // are opened with dlopen at runtime (see mac/ffi.rs) instead of being linked here.
    #[cfg(target_os = "macos")]
    {
        for fw in ["ApplicationServices", "CoreGraphics", "CoreFoundation", "CoreAudio", "AudioToolbox", "AppKit", "IOKit"] {
            println!("cargo:rustc-link-lib=framework={fw}");
        }
    }
    #[cfg(target_os = "macos")]
    build_now_playing_helper();
    tauri_build::build()
}

/// helper/libbrinknp.dylib, for both architectures (the universal build lipo's only the main
/// binary, and bundles this one file as a framework). See helper/nowplaying.m.
#[cfg(target_os = "macos")]
fn build_now_playing_helper() {
    let src = "helper/nowplaying.m";
    let out = "helper/libbrinknp.dylib";
    println!("cargo:rerun-if-changed={src}");
    let fresh = std::fs::metadata(out)
        .and_then(|o| o.modified())
        .ok()
        .zip(std::fs::metadata(src).and_then(|s| s.modified()).ok())
        .is_some_and(|(o, s)| o >= s);
    if fresh {
        return;
    }
    let ok = std::process::Command::new("clang")
        .args([
            "-dynamiclib",
            "-fobjc-arc",
            "-O2",
            "-arch",
            "arm64",
            "-arch",
            "x86_64",
            "-mmacosx-version-min=13.0",
            "-framework",
            "Foundation",
            src,
            "-o",
            out,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "building {out} failed");
}
