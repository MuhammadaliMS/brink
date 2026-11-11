use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Haptic {
    Off,
    Light,
    Medium,
    Strong,
}

/// Where along the edge a slide is allowed to start (fraction of height, from the bottom).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActiveArea {
    Anywhere,
    Threequarters,
    Half,
    Quarter,
}

impl ActiveArea {
    pub fn max_start_y(self) -> f32 {
        match self {
            ActiveArea::Anywhere => 1.0,
            ActiveArea::Threequarters => 0.75,
            ActiveArea::Half => 0.5,
            ActiveArea::Quarter => 0.25,
        }
    }
}

/// What the panel's glass refracts.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    Ribbons,
    Wallpaper,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

/// What a double-tap in a trackpad corner does.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CornerAction {
    Off,
    Mute,
    MuteMic,
    PlayPause,
    NextTrack,
    MissionControl,
    AppWindows,
    ShowDesktop,
    Spotlight,
    Screenshot,
    LockScreen,
    SleepDisplay,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Corners {
    pub top_left: CornerAction,
    pub top_right: CornerAction,
    pub bottom_left: CornerAction,
    pub bottom_right: CornerAction,
}

impl Default for Corners {
    /// Top-right mutes, as Quick Mute always has; the rest wait to be picked.
    fn default() -> Self {
        Self { top_left: CornerAction::Off, top_right: CornerAction::Mute, bottom_left: CornerAction::Off, bottom_right: CornerAction::Off }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub haptic: Haptic,
    /// Double-tap a corner for its action (`corners`). Named for the first one, Quick Mute.
    pub quick_mute: bool,
    pub corners: Corners,
    pub media_strip: bool,
    /// Slide the top edge left/right for the keyboard backlight.
    pub top_strip: bool,
    pub reverse_swipe: bool,
    pub tap_to_play: bool,
    /// Slide the bottom strip to jump back/forward in what's playing; off swipes to skip tracks.
    pub bottom_seek: bool,
    /// 0.5x .. 3.0x — how far a finger travels per step.
    pub sensitivity: f32,
    /// Edge width as a fraction of trackpad width (0.03 .. 0.15).
    pub edge_width: f32,
    pub swap_sides: bool,
    pub active_area: ActiveArea,
    pub require_modifier: bool,
    pub freeze_cursor: bool,
    pub fine_control: bool,
    pub precise_sliding: bool,
    /// Brink sets the levels itself and shows its own HUD with the percentage, instead of
    /// pressing the keys (macOS's HUD has no number).
    pub level_hud: bool,
    /// Brightness for external monitors over DDC/CI (Apple silicon), on monitors that answer it.
    /// Saved under a new name: the old beta switch was off for everyone, and this default is on.
    #[serde(rename = "monitorBrightness")]
    pub external_monitors: bool,
    pub appearance: Appearance,
    pub backdrop: Backdrop,
    pub open_at_login: bool,
    pub magic_mouse: bool,
    /// Two-finger swipes on the top strip of a window.
    pub window_gestures: bool,
    /// Two-finger swipes on an app in the Dock.
    pub dock_gestures: bool,
    /// Dragging the edge between two windows Brink placed side by side resizes both.
    pub drag_dividers: bool,
    /// Control-Option shortcuts for the same window spots.
    pub window_keys: bool,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            haptic: Haptic::Medium,
            quick_mute: true,
            corners: Corners::default(),
            media_strip: true,
            top_strip: true,
            reverse_swipe: false,
            tap_to_play: false,
            bottom_seek: true,
            sensitivity: 2.0,
            edge_width: 0.12,
            swap_sides: false,
            active_area: ActiveArea::Anywhere,
            require_modifier: false,
            freeze_cursor: true,
            fine_control: true,
            precise_sliding: false,
            level_hud: true,
            // Only used on monitors that answer a DDC/CI read; others get the key.
            external_monitors: true,
            appearance: Appearance::Dark,
            backdrop: Backdrop::Ribbons,
            open_at_login: false,
            magic_mouse: false,
            window_gestures: true,
            dock_gestures: true,
            drag_dividers: true,
            // Off by default: Control-Option keys mean something in some apps (Emacs-style
            // editing in a terminal, VoiceOver).
            window_keys: false,
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/in.zerowing.brink/settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(|s| s.clamped())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }

    pub fn clamped(mut self) -> Self {
        self.sensitivity = self.sensitivity.clamp(0.5, 3.0);
        self.edge_width = self.edge_width.clamp(0.03, 0.15);
        self
    }
}
