# Contributing to Brink

Thanks for helping. A few things make reviews quick.

## Before you start

- **Bugs:** open an issue with your Mac model, macOS version and what you did. If it's about
  levels, a monitor or media, attach `~/Library/Logs/in.zerowing.brink/brink.log`. It records
  which route each change took.
- **New gestures or features:** open an issue or a discussion first. Brink tries to stay small:
  a few gestures that are easy to remember and don't get in the way of normal trackpad use.
  Something that fires by accident during ordinary clicking or scrolling won't be merged, however
  useful it is.

## Setting up

```bash
pnpm install
pnpm dev
```

Accessibility access is needed for the trackpad. A dev build is a separate app to macOS, so grant
it again. If touches stop arriving after a rebuild, remove the old entry in System Settings →
Privacy & Security → Accessibility and add the new one.

## Checks

Pull requests run these on CI, so run them first:

```bash
cd src-tauri
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Some tests talk to real hardware: a monitor over DDC/CI, or something playing. They're marked
`#[ignore]` and only run when asked, e.g.
`cargo test --lib talks_to_the_monitor -- --ignored --nocapture`.

## Code

- Gesture logic belongs in `engine.rs` or `windows.rs`, which stay free of macOS calls. Add a test
  there for any change in behaviour; the existing tests show how to feed frames.
- The macOS side (`src-tauri/src/mac/`) talks to private frameworks through `dlopen`. Handle a
  missing symbol by turning the feature off, never by panicking.
- New settings go in `settings.rs` with a default. Saved settings files from older versions must
  still load, so new fields must default to something sensible.
- Comments explain why something is done, not what the next line does.
- The panel UI is plain HTML, CSS and JavaScript in `ui/`. It renders in WebKit, so test there:
  some canvas features (like `ctx.filter`) don't exist in WebKit.

## Pull requests

- Keep each one to one change, and describe what you saw before and after.
- Screenshots or a short recording help for anything visible.
- Releases, signing and notarization are done by the maintainer; you don't need a developer
  certificate.

By contributing you agree that your work is licensed under the project's
[GPL-3.0 license](LICENSE), and you agree to follow the [code of conduct](CODE_OF_CONDUCT.md).
