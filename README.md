# Brink

Turn the edges of your MacBook trackpad into controls. Slide the right edge for volume, the left
for brightness, the top for the keyboard light, and use the bottom edge as a media player.
Everything else on the trackpad works as before.

**[Download](https://github.com/MuhammadaliMS/brink-releases/releases/latest/download/Brink.dmg)** ·
[Website](https://zerowing.in/brink) · macOS 13 or later, Apple silicon and Intel

```bash
brew install --cask muhammadalims/tap/brink
```

## What it does

| Where | What |
|---|---|
| Right edge | Volume. Brink shows its own HUD with the percentage. |
| Left edge | Brightness of the screen under the pointer, including external monitors over DDC/CI, and darker than the monitor's own zero. Works alongside BetterDisplay and MonitorControl. |
| Top edge | Keyboard backlight. |
| Bottom edge | Drag to scrub what's playing (YouTube, Spotify, Music...) with a seek bar. Tap the left, middle or right third for previous, play/pause, next. |
| Corners | Double-tap a corner for one of eleven actions: mute, mic mute, Mission Control, lock screen, screenshot... |
| Windows | Two-finger swipes on the top of a window to fill, halve or quarter it, with a live preview. Optional. |

Hold Shift-Option while sliding for quarter steps. Settings live in the menu bar panel.

## Building from source

You need macOS 13+, the Xcode command line tools, [Rust](https://rustup.rs) and Node.js with
[pnpm](https://pnpm.io).

```bash
pnpm install
pnpm dev            # run Brink from source
pnpm test           # the Rust tests
pnpm build:local    # an unsigned Brink.app in src-tauri/target/release/bundle/macos
```

Brink needs Accessibility access to read the trackpad and post keys. macOS asks on first launch.
A build from source is a different app to macOS than the released one, so it asks again; if
gestures don't respond, remove Brink from System Settings → Privacy & Security → Accessibility
and add it back.

## How it's put together

Brink is a [Tauri 2](https://tauri.app) app: Rust underneath, the panel and the HUD in plain
HTML and JavaScript.

| Path | What's there |
|---|---|
| `src-tauri/src/engine.rs` | The gesture engine: trackpad frames in, effects out. Pure Rust with no macOS calls, so it's fully unit-tested. Start here. |
| `src-tauri/src/windows.rs` | The window-gesture recognizer, also pure. |
| `src-tauri/src/settings.rs` | Every setting and its default. |
| `src-tauri/src/mac/` | The macOS side: raw touches (MultitouchSupport), keys and haptics, levels, DDC/CI, media, the HUD, window moves. |
| `src-tauri/helper/nowplaying.m` | A small helper that reads and controls Now Playing (macOS only lets Apple processes read it, so it runs inside `/usr/bin/perl`). |
| `ui/` | The menu bar panel (`index.html`, `app.js`) and the level HUD (`hud.html`, `hud.js`). |
| `site/` | The website at zerowing.in/brink. |
| `scripts/` | Release, icon and download-count scripts (maintainer tools). |

Some macOS frameworks Brink uses are private (MultitouchSupport, DisplayServices,
CoreBrightness, MediaRemote). They're opened at runtime with `dlopen`, so a missing symbol on a
future macOS turns a feature off instead of crashing the app.

## Contributing

Bug reports, ideas and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).
Security issues: [SECURITY.md](SECURITY.md).

## License

[GPL-3.0](LICENSE). You can use, study, change and share Brink; anything you distribute that's
built on it must be open source under the same license.

The glass in the panel and HUD is [liquid-glass](https://github.com/MuhammadaliMS/liquid-glass),
vendored in `ui/vendor` under the [MIT license](ui/vendor/LICENSE-liquid-glass).
