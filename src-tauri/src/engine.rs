//! Gesture recognition, independent of macOS. Fed one frame of trackpad contacts at a
//! time (normalized 0..1, origin bottom-left) and returns the effects to perform.
//! `tick` must also be called a few times a second: the trackpad stops sending frames
//! once every finger is gone, and some decisions (a lost finger, a stuck slide) are
//! made on time alone.

use crate::settings::{CornerAction, Haptic, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    /// The top strip, slid left/right.
    Top,
    /// The bottom strip, slid left/right to seek in what's playing.
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    Brightness,
    Volume,
    Keyboard,
    Media,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    VolumeUp,
    VolumeDown,
    BrightnessUp,
    BrightnessDown,
    Mute,
    PlayPause,
    Next,
    Previous,
    KeyboardUp,
    KeyboardDown,
}

/// A trackpad corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Post a media key. `quarter` asks for the Shift-Option quarter step.
    Key {
        key: Key,
        quarter: bool,
    },
    Haptic(Haptic),
    /// Two strong pulses: mute, felt without looking.
    HapticDouble,
    /// Stop (true) or release (false) the pointer.
    Freeze(bool),
    /// Swallow clicks for a moment (tap-to-click from a gesture tap).
    SuppressClicks,
    /// A finger is on (Some) or left (None) a control edge; the f32 is 0..1 along it.
    Edge(Option<(Control, f32)>),
    /// A corner action that isn't a media key (Mission Control, lock screen...).
    Action(CornerAction),
    /// A corner's action just ran (the panel flashes it).
    CornerFired(Corner),
    /// A bottom-strip slide began: scrub what's playing from where it is now.
    ScrubStart,
    /// The scrubbing finger moved this far (fraction of the trackpad's width; positive is
    /// forward). `fine`: the Shift-Option quarter speed.
    Scrub {
        delta: f32,
        fine: bool,
    },
    /// A tap on the bottom strip: previous (left third), play/pause (middle), next (right).
    /// `held`: seconds the finger was down, so a physical click in that time can void it.
    MediaTap {
        key: Key,
        held: f64,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub id: i32,
    pub x: f32,
    pub y: f32,
}

/// Modifier keys held during the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub fn_or_option: bool,
    pub shift_option: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone {
    Side(Side),
    Bottom,
    Corner(Corner),
}

#[derive(Clone, Copy, Debug)]
struct Slide {
    side: Side,
    id: i32,
    /// Last position along the slide axis (y for the side edges, x for the top strip).
    last: f32,
    acc: f32,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    /// Finger down in a zone; deciding whether it is a slide, a swipe or a tap.
    /// `corner`: the finger also landed in a corner with an action (a corner overlaps the
    /// side edges, so it can start a slide too).
    Candidate {
        zone: Zone,
        id: i32,
        x0: f32,
        y0: f32,
        t0: f64,
        frozen: bool,
        corner: Option<Corner>,
    },
    Sliding(Slide),
    /// The sliding finger vanished. Trackpads drop a finger for a frame or two mid-slide,
    /// so wait briefly for it to come back before ending the slide.
    Lost {
        slide: Slide,
        since: f64,
    },
    /// Gesture done (or not ours) until every finger lifts.
    Spent {
        frozen: bool,
    },
}

/// Vertical travel for one full (1/16) step at 1x sensitivity, as a fraction of height.
const BASE_STEP: f32 = 0.06;
const SLIDE_START: f32 = 0.012;
const SWIPE_DISTANCE: f32 = 0.16;
const TAP_MAX_MOVE: f32 = 0.025;
const TAP_MAX_TIME: f64 = 0.28;
const DOUBLE_TAP_GAP: f64 = 0.40;
/// Corners: this far in from the left/right sides, and from the top/bottom.
const CORNER_X: f32 = 0.12;
const CORNER_Y: f32 = 0.20;
const LOST_GRACE: f64 = 0.18;
const REACQUIRE: f32 = 0.12;
/// No frames for this long while a gesture is open: the device went away. Let go.
const WATCHDOG: f64 = 0.8;

pub struct Engine {
    state: State,
    /// The panel has been told a finger is on an edge and needs a matching None.
    edge_shown: bool,
    last_frame: f64,
    /// The last single tap in a corner, waiting for a second one.
    last_corner_tap: Option<(Corner, f64)>,
    last_bottom_tap: f64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self { state: State::Idle, edge_shown: false, last_frame: 0.0, last_corner_tap: None, last_bottom_tap: -10.0 }
    }

    pub fn control_for(side: Side, s: &Settings) -> Control {
        match (side, s.swap_sides) {
            (Side::Top, _) => Control::Keyboard,
            (Side::Bottom, _) => Control::Media,
            (Side::Right, false) | (Side::Left, true) => Control::Volume,
            _ => Control::Brightness,
        }
    }

    fn axis(side: Side, c: &Contact) -> f32 {
        if matches!(side, Side::Top | Side::Bottom) {
            c.x
        } else {
            c.y
        }
    }

    fn show(&mut self, out: &mut Vec<Effect>, control: Control, at: f32) {
        self.edge_shown = true;
        out.push(Effect::Edge(Some((control, at.clamp(0.0, 1.0)))));
    }

    /// Reset (device list changed, Brink switched off).
    pub fn reset(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        self.finish(&mut out);
        self.state = State::Idle;
        out
    }

    /// Time-based decisions; call every ~50 ms.
    pub fn tick(&mut self, now: f64, s: &Settings) -> Vec<Effect> {
        let mut out = Vec::new();
        match self.state {
            State::Lost { since, .. } if now - since > LOST_GRACE => self.lift(now, s, &mut out),
            State::Idle => {}
            _ if now - self.last_frame > WATCHDOG => {
                self.finish(&mut out);
                self.state = State::Idle;
            }
            _ => {}
        }
        out
    }

    pub fn frame(&mut self, contacts: &[Contact], now: f64, mods: Mods, s: &Settings) -> Vec<Effect> {
        let mut out = Vec::new();
        self.last_frame = now;
        if !s.enabled {
            if !matches!(self.state, State::Idle) {
                self.finish(&mut out);
                self.state = State::Idle;
            }
            return out;
        }

        // A slide follows its own finger and ignores the rest (a resting thumb, a palm).
        if let State::Sliding(sl) | State::Lost { slide: sl, .. } = self.state {
            let own = contacts.iter().find(|c| c.id == sl.id).or_else(|| {
                contacts.iter().find(|c| Self::near_edge(c, sl.side, s) && (Self::axis(sl.side, c) - sl.last).abs() < REACQUIRE)
            });
            match own {
                Some(c) => {
                    let c = *c;
                    self.state = State::Sliding(Slide { id: c.id, ..sl });
                    self.slide(c, mods, s, &mut out);
                }
                None if matches!(self.state, State::Sliding(_)) => {
                    self.state = State::Lost { slide: sl, since: now };
                }
                None => {
                    if contacts.is_empty() {
                        // still waiting; tick() decides
                    } else {
                        // A different finger, somewhere else: the slide is over.
                        self.lift(now, s, &mut out);
                        if contacts.len() == 1 {
                            self.touch_down(contacts[0], now, mods, s, &mut out);
                        } else {
                            self.state = State::Spent { frozen: false };
                        }
                    }
                }
            }
            return out;
        }

        match contacts.len() {
            0 => {
                self.lift(now, s, &mut out);
                return out;
            }
            1 => {}
            _ => {
                // Two or more fingers before a slide began: scrolling, pinching. Not ours.
                if !matches!(self.state, State::Spent { .. }) {
                    if self.frozen() {
                        out.push(Effect::Freeze(false));
                    }
                    self.state = State::Spent { frozen: false };
                }
                return out;
            }
        }
        let c = contacts[0];

        match self.state {
            State::Idle => self.touch_down(c, now, mods, s, &mut out),
            State::Candidate { zone, id, x0, y0, frozen, .. } => {
                if id != c.id {
                    self.finish(&mut out);
                    self.state = State::Idle;
                    self.touch_down(c, now, mods, s, &mut out);
                    return out;
                }
                let dx = c.x - x0;
                let dy = c.y - y0;
                match zone {
                    Zone::Side(side) => {
                        // Along the edge is the slide; across it means the user is just moving the pointer.
                        let (along, across) = if side == Side::Top { (dx, dy) } else { (dy, dx) };
                        let leaving = across.abs() > s.edge_width + 0.05 && across.abs() > along.abs() * 1.5;
                        if leaving {
                            // The user is just moving the pointer from the edge.
                            if frozen {
                                out.push(Effect::Freeze(false));
                            }
                            self.state = State::Spent { frozen: false };
                        } else if along.abs() > SLIDE_START {
                            let last = if side == Side::Top { x0 } else { y0 };
                            self.state = State::Sliding(Slide { side, id, last, acc: 0.0 });
                            self.slide(c, mods, s, &mut out);
                        }
                    }
                    Zone::Bottom => {
                        self.show(&mut out, Control::Media, c.x);
                        if dy.abs() > 0.10 && dy.abs() > dx.abs() {
                            if frozen {
                                out.push(Effect::Freeze(false));
                            }
                            self.state = State::Spent { frozen: false };
                        } else if s.bottom_seek && dx.abs() > SLIDE_START {
                            // Slide to scrub, like the other edges slide a level.
                            out.push(Effect::ScrubStart);
                            self.state = State::Sliding(Slide { side: Side::Bottom, id, last: x0, acc: 0.0 });
                            self.slide(c, mods, s, &mut out);
                        } else if !s.bottom_seek && dx.abs() >= SWIPE_DISTANCE {
                            let forward = (dx > 0.0) != s.reverse_swipe;
                            out.push(Effect::Key { key: if forward { Key::Next } else { Key::Previous }, quarter: false });
                            Self::tick_haptic(s.haptic, &mut out);
                            self.state = State::Spent { frozen };
                        }
                    }
                    Zone::Corner(_) => {
                        if dx.abs().max(dy.abs()) > TAP_MAX_MOVE * 2.0 {
                            self.state = State::Spent { frozen };
                        }
                    }
                }
            }
            State::Spent { .. } | State::Sliding(_) | State::Lost { .. } => {}
        }
        out
    }

    fn near_edge(c: &Contact, side: Side, s: &Settings) -> bool {
        let reach = s.edge_width + REACQUIRE;
        match side {
            Side::Left => c.x < reach,
            Side::Right => c.x > 1.0 - reach,
            Side::Top => c.y > 1.0 - Self::strip(s) - REACQUIRE,
            Side::Bottom => c.y < Self::strip(s) + REACQUIRE,
        }
    }

    /// Height of the top and bottom strips.
    fn strip(s: &Settings) -> f32 {
        // Capped: a wide side edge must not turn the top/bottom into a fifth of the trackpad.
        (s.edge_width * 0.9).clamp(0.06, 0.1)
    }

    fn frozen(&self) -> bool {
        match self.state {
            State::Candidate { frozen, .. } | State::Spent { frozen } => frozen,
            State::Sliding(_) | State::Lost { .. } => true,
            State::Idle => false,
        }
    }

    pub fn corner_action(k: Corner, s: &Settings) -> CornerAction {
        if !s.quick_mute {
            return CornerAction::Off;
        }
        match k {
            Corner::TopLeft => s.corners.top_left,
            Corner::TopRight => s.corners.top_right,
            Corner::BottomLeft => s.corners.bottom_left,
            Corner::BottomRight => s.corners.bottom_right,
        }
    }

    /// The corner the finger is in, if that corner has an action.
    fn corner_at(c: &Contact, s: &Settings) -> Option<Corner> {
        let left = c.x < CORNER_X;
        let right = c.x > 1.0 - CORNER_X;
        let bottom = c.y < CORNER_Y;
        let top = c.y > 1.0 - CORNER_Y;
        let k = match (left, right, top, bottom) {
            (true, _, true, _) => Corner::TopLeft,
            (_, true, true, _) => Corner::TopRight,
            (true, _, _, true) => Corner::BottomLeft,
            (_, true, _, true) => Corner::BottomRight,
            _ => return None,
        };
        (Self::corner_action(k, s) != CornerAction::Off).then_some(k)
    }

    fn zone_at(c: Contact, s: &Settings) -> Option<Zone> {
        let w = s.edge_width;
        if let Some(k) = Self::corner_at(&c, s) {
            return Some(Zone::Corner(k));
        }
        let side = if c.x < w {
            Some(Side::Left)
        } else if c.x > 1.0 - w {
            Some(Side::Right)
        } else {
            None
        };
        if let Some(side) = side {
            if c.y <= s.active_area.max_start_y() + 0.001 {
                return Some(Zone::Side(side));
            }
            return None;
        }
        if s.media_strip && c.y < Self::strip(s) {
            return Some(Zone::Bottom);
        }
        if s.top_strip && c.y > 1.0 - Self::strip(s) {
            return Some(Zone::Side(Side::Top));
        }
        None
    }

    fn touch_down(&mut self, c: Contact, now: f64, mods: Mods, s: &Settings, out: &mut Vec<Effect>) {
        if s.require_modifier && !mods.fn_or_option {
            self.state = State::Spent { frozen: false };
            return;
        }
        let Some(zone) = Self::zone_at(c, s) else {
            self.state = State::Spent { frozen: false };
            return;
        };
        // A corner is also the end of a side edge, so a slide can start there too.
        let (zone, corner) = match zone {
            Zone::Corner(k) => {
                let side = if c.x < s.edge_width {
                    Some(Side::Left)
                } else if c.x > 1.0 - s.edge_width {
                    Some(Side::Right)
                } else {
                    None
                };
                match side {
                    Some(side) if s.active_area.max_start_y() >= c.y => (Zone::Side(side), Some(k)),
                    _ => (Zone::Corner(k), Some(k)),
                }
            }
            z => (z, None),
        };
        let freeze = s.freeze_cursor && !matches!(zone, Zone::Corner(_));
        if freeze {
            out.push(Effect::Freeze(true));
        }
        self.state = State::Candidate { zone, id: c.id, x0: c.x, y0: c.y, t0: now, frozen: freeze, corner };
    }

    fn slide(&mut self, c: Contact, mods: Mods, s: &Settings, out: &mut Vec<Effect>) {
        let State::Sliding(Slide { side, id, last, mut acc }) = self.state else { return };
        let quarter = s.precise_sliding || (s.fine_control && mods.shift_option);
        let mut step = BASE_STEP / s.sensitivity;
        if quarter {
            step /= 4.0;
        }
        let pos = Self::axis(side, &c);
        acc += pos - last;
        let control = Self::control_for(side, s);
        if side == Side::Bottom {
            // The playhead follows the finger; right is forward (left with Reverse direction).
            // A haptic tick per level-sized step of travel, so the drag feels like the edges.
            let delta = pos - last;
            if delta != 0.0 {
                out.push(Effect::Scrub { delta: if s.reverse_swipe { -delta } else { delta }, fine: quarter });
            }
            while acc.abs() >= step {
                acc -= acc.signum() * step;
                Self::tick_haptic(s.haptic, out);
            }
            self.show(out, control, pos);
            self.state = State::Sliding(Slide { side, id, last: pos, acc });
            return;
        }
        let (up, down) = match control {
            Control::Volume => (Key::VolumeUp, Key::VolumeDown),
            Control::Keyboard => (Key::KeyboardUp, Key::KeyboardDown),
            _ => (Key::BrightnessUp, Key::BrightnessDown),
        };
        while acc >= step {
            acc -= step;
            out.push(Effect::Key { key: up, quarter });
            Self::tick_haptic(s.haptic, out);
        }
        while acc <= -step {
            acc += step;
            out.push(Effect::Key { key: down, quarter });
            Self::tick_haptic(s.haptic, out);
        }
        self.show(out, control, pos);
        self.state = State::Sliding(Slide { side, id, last: pos, acc });
    }

    fn tick_haptic(h: Haptic, out: &mut Vec<Effect>) {
        if h != Haptic::Off {
            out.push(Effect::Haptic(h));
        }
    }

    fn lift(&mut self, now: f64, s: &Settings, out: &mut Vec<Effect>) {
        if let State::Candidate { zone, t0, corner, .. } = self.state {
            let quick = now - t0 <= TAP_MAX_TIME;
            if let (true, Some(k)) = (quick, corner) {
                out.push(Effect::SuppressClicks);
                match self.last_corner_tap {
                    Some((last, t)) if last == k && now - t <= DOUBLE_TAP_GAP => {
                        let action = Self::corner_action(k, s);
                        out.push(match action {
                            CornerAction::Mute => Effect::Key { key: Key::Mute, quarter: false },
                            CornerAction::PlayPause => Effect::Key { key: Key::PlayPause, quarter: false },
                            CornerAction::NextTrack => Effect::Key { key: Key::Next, quarter: false },
                            a => Effect::Action(a),
                        });
                        out.push(Effect::CornerFired(k));
                        if s.haptic != Haptic::Off {
                            out.push(Effect::HapticDouble);
                        }
                        self.last_corner_tap = None;
                    }
                    _ => self.last_corner_tap = Some((k, now)),
                }
            } else if quick && zone == Zone::Bottom && s.bottom_seek {
                // Like a player's buttons: previous | play/pause | next.
                if let State::Candidate { x0, .. } = self.state {
                    let key = if x0 < 1.0 / 3.0 {
                        Key::Previous
                    } else if x0 > 2.0 / 3.0 {
                        Key::Next
                    } else {
                        Key::PlayPause
                    };
                    let key = match key {
                        Key::Previous if s.reverse_swipe => Key::Next,
                        Key::Next if s.reverse_swipe => Key::Previous,
                        k => k,
                    };
                    out.push(Effect::MediaTap { key, held: now - t0 });
                    out.push(Effect::SuppressClicks);
                    Self::tick_haptic(s.haptic, out);
                }
            } else if quick && zone == Zone::Bottom && s.tap_to_play {
                if now - self.last_bottom_tap <= DOUBLE_TAP_GAP {
                    out.push(Effect::Key { key: Key::PlayPause, quarter: false });
                    Self::tick_haptic(s.haptic, out);
                    out.push(Effect::SuppressClicks);
                    self.last_bottom_tap = -10.0;
                } else {
                    self.last_bottom_tap = now;
                }
            }
        }
        if matches!(self.state, State::Sliding(_) | State::Lost { .. }) {
            out.push(Effect::SuppressClicks);
        }
        self.finish(out);
        self.state = State::Idle;
    }

    fn finish(&mut self, out: &mut Vec<Effect>) {
        if std::mem::take(&mut self.edge_shown) {
            out.push(Effect::Edge(None));
        }
        if self.frozen() {
            out.push(Effect::Freeze(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ActiveArea;

    fn one(x: f32, y: f32) -> Vec<Contact> {
        vec![Contact { id: 1, x, y }]
    }

    fn keys(e: &[Effect]) -> Vec<Key> {
        e.iter().filter_map(|e| if let Effect::Key { key, .. } = e { Some(*key) } else { None }).collect()
    }

    /// Slide one finger along x from y0 to y1 in 0.01 steps, 10 ms apart.
    fn slide(e: &mut Engine, s: &Settings, x: f32, y0: f32, y1: f32) -> Vec<Effect> {
        let mut all = Vec::new();
        let n = ((y1 - y0).abs() / 0.01) as usize;
        for i in 0..=n {
            let y = y0 + (y1 - y0).signum() * i as f32 * 0.01;
            all.extend(e.frame(&one(x, y), i as f64 * 0.01, Mods::default(), s));
        }
        all
    }

    #[test]
    fn right_edge_slide_up_raises_volume() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = slide(&mut e, &s, 0.98, 0.2, 0.6);
        all.extend(e.frame(&[], 1.0, Mods::default(), &s));
        all.extend(e.tick(1.5, &s));
        let k = keys(&all);
        assert!(k.len() >= 8, "{k:?}");
        assert!(k.iter().all(|k| *k == Key::VolumeUp));
        assert_eq!(all.first(), Some(&Effect::Freeze(true)));
        assert!(all.contains(&Effect::Freeze(false)));
        assert!(all.contains(&Effect::Edge(None)));
    }

    #[test]
    fn swap_sides_puts_volume_on_left() {
        let s = Settings { swap_sides: true, ..Default::default() };
        let mut e = Engine::new();
        let k = keys(&slide(&mut e, &s, 0.01, 0.6, 0.3));
        assert!(!k.is_empty() && k.iter().all(|k| *k == Key::VolumeDown));
    }

    #[test]
    fn horizontal_move_from_edge_is_not_a_slide() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = Vec::new();
        for i in 0..30 {
            all.extend(e.frame(&one(0.02 + i as f32 * 0.02, 0.5), i as f64 * 0.01, Mods::default(), &s));
        }
        assert!(keys(&all).is_empty());
        assert_eq!(all.last(), Some(&Effect::Freeze(false)));
    }

    #[test]
    fn double_tap_corner_mutes_with_double_buzz() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = Vec::new();
        for t in [0.0, 0.3] {
            all.extend(e.frame(&one(0.95, 0.95), t, Mods::default(), &s));
            all.extend(e.frame(&one(0.95, 0.95), t + 0.05, Mods::default(), &s));
            all.extend(e.frame(&[], t + 0.08, Mods::default(), &s));
        }
        assert_eq!(keys(&all), vec![Key::Mute]);
        assert!(all.contains(&Effect::HapticDouble));
    }

    fn double_tap(e: &mut Engine, s: &Settings, x: f32, y: f32) -> Vec<Effect> {
        let mut all = Vec::new();
        for t in [0.0, 0.3] {
            all.extend(e.frame(&one(x, y), t, Mods::default(), s));
            all.extend(e.frame(&one(x, y), t + 0.05, Mods::default(), s));
            all.extend(e.frame(&[], t + 0.08, Mods::default(), s));
        }
        all
    }

    #[test]
    fn every_corner_has_its_own_action() {
        use crate::settings::Corners;
        let s = Settings {
            corners: Corners {
                top_left: CornerAction::MissionControl,
                top_right: CornerAction::Mute,
                bottom_left: CornerAction::LockScreen,
                bottom_right: CornerAction::PlayPause,
            },
            ..Default::default()
        };
        let mut e = Engine::new();
        assert!(double_tap(&mut e, &s, 0.04, 0.95).contains(&Effect::Action(CornerAction::MissionControl)));
        assert!(double_tap(&mut e, &s, 0.04, 0.05).contains(&Effect::Action(CornerAction::LockScreen)));
        assert_eq!(keys(&double_tap(&mut e, &s, 0.96, 0.05)), vec![Key::PlayPause]);
        assert_eq!(keys(&double_tap(&mut e, &s, 0.96, 0.96)), vec![Key::Mute]);
    }

    #[test]
    fn taps_in_two_different_corners_are_not_a_double_tap() {
        use crate::settings::Corners;
        let s = Settings { corners: Corners { top_left: CornerAction::Mute, ..Corners::default() }, ..Default::default() };
        let mut e = Engine::new();
        let mut all = Vec::new();
        all.extend(e.frame(&one(0.04, 0.95), 0.0, Mods::default(), &s));
        all.extend(e.frame(&[], 0.05, Mods::default(), &s));
        all.extend(e.frame(&one(0.96, 0.95), 0.2, Mods::default(), &s));
        all.extend(e.frame(&[], 0.25, Mods::default(), &s));
        assert!(keys(&all).is_empty());
    }

    #[test]
    fn corners_without_an_action_are_left_alone() {
        let s = Settings::default();
        let mut e = Engine::new();
        // Bottom-left is off by default: a double-tap there is just a (brightness edge) tap.
        let all = double_tap(&mut e, &s, 0.05, 0.15);
        assert!(!all.iter().any(|x| matches!(x, Effect::Action(_) | Effect::SuppressClicks)));
    }

    #[test]
    fn a_slide_can_start_in_a_corner() {
        use crate::settings::Corners;
        let s = Settings { corners: Corners { bottom_left: CornerAction::LockScreen, ..Corners::default() }, ..Default::default() };
        let mut e = Engine::new();
        let k = keys(&slide(&mut e, &s, 0.03, 0.05, 0.5));
        assert!(k.len() >= 5 && k.iter().all(|k| *k == Key::BrightnessUp), "{k:?}");
    }

    #[test]
    fn top_strip_slides_keyboard_backlight() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = Vec::new();
        for i in 0..30 {
            all.extend(e.frame(&one(0.3 + i as f32 * 0.01, 0.97), i as f64 * 0.01, Mods::default(), &s));
        }
        let k = keys(&all);
        assert!(k.len() >= 3 && k.iter().all(|k| *k == Key::KeyboardUp), "{k:?}");
    }

    /// (scrub starts, total scrub travel).
    fn scrubs(effects: &[Effect]) -> (usize, f32) {
        let starts = effects.iter().filter(|e| matches!(e, Effect::ScrubStart)).count();
        let travel = effects.iter().map(|e| if let Effect::Scrub { delta, .. } = e { *delta } else { 0.0 }).sum();
        (starts, travel)
    }

    #[test]
    fn bottom_slide_seeks() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = Vec::new();
        // Right along the bottom, then back left past the start.
        for i in 0..20 {
            all.extend(e.frame(&one(0.3 + i as f32 * 0.01, 0.02), i as f64 * 0.01, Mods::default(), &s));
        }
        let (starts, fwd) = scrubs(&all);
        assert_eq!(starts, 1);
        assert!((fwd - 0.19).abs() < 1e-4, "{fwd}");
        assert!(keys(&all).is_empty(), "no track skip while scrubbing");
        let mut back = Vec::new();
        for i in 0..40 {
            back.extend(e.frame(&one(0.49 - i as f32 * 0.01, 0.02), 0.2 + i as f64 * 0.01, Mods::default(), &s));
        }
        let (starts, back) = scrubs(&back);
        assert_eq!(starts, 0, "the same slide goes on");
        assert!((back + 0.39).abs() < 1e-4, "{back}");
    }

    #[test]
    fn bottom_taps_are_player_buttons() {
        let s = Settings::default();
        let tap = |x: f32| {
            let mut e = Engine::new();
            let mut all = e.frame(&one(x, 0.03), 0.0, Mods::default(), &s);
            all.extend(e.frame(&one(x, 0.03), 0.08, Mods::default(), &s));
            all.extend(e.frame(&[], 0.12, Mods::default(), &s));
            all.iter().filter_map(|x| if let Effect::MediaTap { key, .. } = x { Some(*key) } else { None }).collect::<Vec<_>>()
        };
        assert_eq!(tap(0.15), vec![Key::Previous]);
        assert_eq!(tap(0.5), vec![Key::PlayPause]);
        assert_eq!(tap(0.85), vec![Key::Next]);
        // Higher up the trackpad is just a tap.
        let mut e = Engine::new();
        let mut all = e.frame(&one(0.5, 0.5), 0.0, Mods::default(), &s);
        all.extend(e.frame(&[], 0.1, Mods::default(), &s));
        assert!(!all.iter().any(|x| matches!(x, Effect::MediaTap { .. })));
    }

    #[test]
    fn bottom_swipe_skips_track() {
        let s = Settings { bottom_seek: false, ..Default::default() };
        let mut e = Engine::new();
        let mut all = Vec::new();
        for i in 0..25 {
            all.extend(e.frame(&one(0.3 + i as f32 * 0.01, 0.02), i as f64 * 0.01, Mods::default(), &s));
        }
        assert_eq!(keys(&all), vec![Key::Next]);
        let s = Settings { reverse_swipe: true, bottom_seek: false, ..Default::default() };
        let mut e = Engine::new();
        let mut all = Vec::new();
        for i in 0..25 {
            all.extend(e.frame(&one(0.3 + i as f32 * 0.01, 0.02), i as f64 * 0.01, Mods::default(), &s));
        }
        assert_eq!(keys(&all), vec![Key::Previous]);
    }

    #[test]
    fn active_area_blocks_high_starts() {
        let s = Settings { active_area: ActiveArea::Half, ..Default::default() };
        let mut e = Engine::new();
        assert!(keys(&slide(&mut e, &s, 0.98, 0.7, 0.5)).is_empty());
        let mut e = Engine::new();
        assert!(!keys(&slide(&mut e, &s, 0.98, 0.4, 0.2)).is_empty());
    }

    #[test]
    fn precise_sliding_uses_quarter_steps() {
        let s = Settings { precise_sliding: true, ..Default::default() };
        let mut e = Engine::new();
        let all = slide(&mut e, &s, 0.98, 0.2, 0.4);
        assert!(all.iter().any(|e| matches!(e, Effect::Key { quarter: true, .. })));
        assert!(keys(&all).len() >= 8);
    }

    #[test]
    fn fine_control_needs_shift_option() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mods = Mods { shift_option: true, ..Default::default() };
        let mut all = Vec::new();
        for i in 0..20 {
            all.extend(e.frame(&one(0.98, 0.2 + i as f32 * 0.01), i as f64 * 0.01, mods, &s));
        }
        assert!(all.iter().any(|e| matches!(e, Effect::Key { quarter: true, .. })));
    }

    #[test]
    fn require_modifier() {
        let s = Settings { require_modifier: true, ..Default::default() };
        let mut e = Engine::new();
        assert!(keys(&slide(&mut e, &s, 0.98, 0.2, 0.6)).is_empty());
    }

    #[test]
    fn lost_finger_mid_slide_keeps_going() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = slide(&mut e, &s, 0.98, 0.2, 0.4);
        // Finger drops out for two frames, then comes back with a new id.
        all.extend(e.frame(&[], 0.30, Mods::default(), &s));
        all.extend(e.tick(0.33, &s));
        all.extend(e.frame(&[], 0.34, Mods::default(), &s));
        assert!(!all.contains(&Effect::Edge(None)), "slide ended early");
        let before = keys(&all).len();
        for i in 0..20 {
            all.extend(e.frame(&[Contact { id: 7, x: 0.97, y: 0.41 + i as f32 * 0.01 }], 0.36 + i as f64 * 0.01, Mods::default(), &s));
        }
        assert!(keys(&all).len() > before + 3);
        assert!(!all.contains(&Effect::Edge(None)));
        // Gone for good: the grace period ends the slide and releases the pointer.
        all.extend(e.frame(&[], 0.7, Mods::default(), &s));
        all.extend(e.tick(1.0, &s));
        assert!(all.contains(&Effect::Edge(None)));
        assert_eq!(all.last(), Some(&Effect::Freeze(false)));
    }

    #[test]
    fn resting_thumb_does_not_end_slide() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = slide(&mut e, &s, 0.98, 0.2, 0.3);
        let before = keys(&all).len();
        for i in 0..20 {
            let y = 0.31 + i as f32 * 0.01;
            let cs = [Contact { id: 1, x: 0.98, y }, Contact { id: 9, x: 0.4, y: 0.05 }];
            all.extend(e.frame(&cs, 0.2 + i as f64 * 0.01, Mods::default(), &s));
        }
        assert!(keys(&all).len() > before + 3);
        assert!(!all.contains(&Effect::Edge(None)));
    }

    #[test]
    fn watchdog_releases_pointer_when_frames_stop() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = slide(&mut e, &s, 0.98, 0.2, 0.3);
        all.extend(e.tick(5.0, &s));
        assert_eq!(all.last(), Some(&Effect::Freeze(false)));
    }

    #[test]
    fn two_finger_scroll_near_edge_is_ignored() {
        let s = Settings::default();
        let mut e = Engine::new();
        let mut all = Vec::new();
        all.extend(e.frame(&one(0.98, 0.3), 0.0, Mods::default(), &s));
        for i in 0..20 {
            let y = 0.3 + i as f32 * 0.01;
            let cs = [Contact { id: 1, x: 0.98, y }, Contact { id: 2, x: 0.9, y }];
            all.extend(e.frame(&cs, 0.01 + i as f64 * 0.01, Mods::default(), &s));
        }
        assert!(keys(&all).is_empty());
        assert_eq!(all.last(), Some(&Effect::Freeze(false)));
    }
}
