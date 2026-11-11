//! Window gestures, independent of macOS. Two fingers of one hand (the same touch as
//! scrolling) land while the pointer is on the top strip of a window, or on an app in the Dock.
//!
//! Top of a window: move the fingers up to fill the screen, left or right for a half,
//! diagonally for a quarter, down to put it back (or minimize). A preview shows where the
//! window will go while the fingers move; lifting places it, sliding back to the middle
//! cancels.
//!
//! Dock icon: swipe up to show the app's windows, down to hide them, left or right for its
//! next window. Anywhere else two fingers scroll as usual.

use crate::engine::Contact;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Title,
    Dock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Swipe(Dir),
    /// A spot on the screen as fractions of its usable area: x, y, w, h (keyboard).
    Frac([f64; 4]),
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// Two fingers landed in a gesture zone (true): hold back scrolling and pinching so the
    /// window doesn't also scroll. False once the fingers lift (momentum scrolling is still
    /// held back for a moment).
    Hold(bool),
    /// Not a window gesture after all, fingers still down: let scrolling through right away.
    Release,
    /// Show where the window would go for this direction, or hide the preview.
    Preview(Option<Dir>),
    Act(Action),
}

/// The trackpad is about 1.6 times wider than tall; distances are measured in its height.
const ASPECT: f32 = 1.6;
/// Finger travel that shows the preview, and below which it hides again (a little less, so
/// it doesn't flicker at the boundary).
const SHOW: f32 = 0.07;
const HIDE: f32 = 0.05;
/// Dock swipes have no preview: they fire once the fingers travel this far.
const DOCK_SWIPE: f32 = 0.12;
/// Fingers resting in a zone without moving: let scrolling go after this long.
const IDLE_SECS: f64 = 1.0;
/// No trackpad frames for this long mid-gesture (trackpad gone, Mac asleep): let go.
const LOST_SECS: f64 = 0.5;
/// Both fingers must move the same way: each at least this share of the other's travel...
const TOGETHER: f32 = 0.5;
/// ...and within about 45° of each other. A thumb resting while a finger drags is not a swipe.
const SAME_WAY: f32 = 0.7;
/// A swipe is diagonal when its shorter side is at least this share of its longer side
/// (tan 30°), so near-straight swipes stay straight.
const DIAGONAL: f32 = 0.58;

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    /// Fingers are down but this is not a window gesture; wait for them to lift.
    Ignored,
    Armed {
        zone: Zone,
        t0: f64,
        ids: [i32; 2],
        start: [(f32, f32); 2],
        dir: Option<Dir>,
    },
    /// Fired; hold scrolling until the fingers lift.
    Done,
}

pub struct WindowGestures {
    state: State,
    fingers: usize,
    last_frame: f64,
}

impl Default for WindowGestures {
    fn default() -> Self {
        Self::new()
    }
}

fn pos(c: &Contact) -> (f32, f32) {
    (c.x * ASPECT, c.y)
}

/// How far the two fingers moved together (their average), or None when they didn't move as
/// one: a resting thumb plus a moving finger, or fingers going different ways.
fn moved_together(a: &Contact, b: &Contact, start: [(f32, f32); 2]) -> Option<(f32, f32)> {
    let (pa, pb) = (pos(a), pos(b));
    let va = (pa.0 - start[0].0, pa.1 - start[0].1);
    let vb = (pb.0 - start[1].0, pb.1 - start[1].1);
    let (la, lb) = ((va.0 * va.0 + va.1 * va.1).sqrt(), (vb.0 * vb.0 + vb.1 * vb.1).sqrt());
    let avg = ((va.0 + vb.0) / 2.0, (va.1 + vb.1) / 2.0);
    if la.max(lb) < 0.02 {
        return Some(avg);
    }
    let same_way = (va.0 * vb.0 + va.1 * vb.1) / (la * lb).max(1e-6);
    (la.min(lb) >= la.max(lb) * TOGETHER && same_way >= SAME_WAY).then_some(avg)
}

/// The two tracked fingers, if both are still down (in any order), plus whether another
/// finger joined. One of them lifting first is normal at the end of a gesture.
fn pair(contacts: &[Contact], ids: [i32; 2]) -> (Option<(&Contact, &Contact)>, bool) {
    let stranger = contacts.iter().any(|c| !ids.contains(&c.id));
    let a = contacts.iter().find(|c| c.id == ids[0]);
    let b = contacts.iter().find(|c| c.id == ids[1]);
    (a.zip(b), stranger)
}

fn direction(dx: f32, dy: f32, diagonals: bool) -> Dir {
    let (ax, ay) = (dx.abs(), dy.abs());
    if diagonals && ax.min(ay) >= ax.max(ay) * DIAGONAL {
        return match (dx > 0.0, dy > 0.0) {
            (false, true) => Dir::UpLeft,
            (true, true) => Dir::UpRight,
            (false, false) => Dir::DownLeft,
            (true, false) => Dir::DownRight,
        };
    }
    if ay >= ax {
        if dy > 0.0 {
            Dir::Up
        } else {
            Dir::Down
        }
    } else if dx > 0.0 {
        Dir::Right
    } else {
        Dir::Left
    }
}

impl WindowGestures {
    pub fn new() -> Self {
        Self { state: State::Idle, fingers: 0, last_frame: 0.0 }
    }

    fn zone_on(s: &Settings, z: Zone) -> bool {
        match z {
            Zone::Title => s.window_gestures,
            Zone::Dock => s.dock_gestures,
        }
    }

    /// One frame of contacts. `probe` is asked once, when two fingers land: which gesture zone
    /// the pointer is over, if any.
    pub fn frame(&mut self, contacts: &[Contact], now: f64, s: &Settings, probe: impl FnOnce() -> Option<Zone>) -> Vec<Out> {
        let mut out = Vec::new();
        let n = contacts.len();
        let landed = n == 2 && self.fingers < 2;
        self.fingers = n;
        self.last_frame = now;

        if !s.enabled || !(s.window_gestures || s.dock_gestures) {
            self.cancel(&mut out, n == 0);
            self.state = if n == 0 { State::Idle } else { State::Ignored };
            return out;
        }

        match self.state {
            State::Idle => {
                if landed {
                    let (a, b) = (&contacts[0], &contacts[1]);
                    // Two fingers in an edge zone are an edge slide, not a window gesture.
                    let edge = s.edge_width;
                    let in_edge = |c: &Contact| c.x < edge || c.x > 1.0 - edge;
                    let zone = if in_edge(a) || in_edge(b) { None } else { probe().filter(|z| Self::zone_on(s, *z)) };
                    if let Some(zone) = zone {
                        self.state = State::Armed { zone, t0: now, ids: [a.id, b.id], start: [pos(a), pos(b)], dir: None };
                        out.push(Out::Hold(true));
                    } else {
                        self.state = State::Ignored;
                    }
                } else if n > 2 {
                    self.state = State::Ignored;
                }
                // One finger may land a moment before the second: stay idle.
            }
            State::Ignored => {
                if n == 0 {
                    self.state = State::Idle;
                }
            }
            State::Done => {
                if n == 0 {
                    self.state = State::Idle;
                    out.push(Out::Hold(false));
                }
            }
            State::Armed { zone, t0, ids, start, dir } => {
                let (both, stranger) = pair(contacts, ids);
                if stranger {
                    // A third finger: this is something else.
                    self.cancel(&mut out, false);
                    self.state = State::Ignored;
                    return out;
                }
                let Some((a, b)) = both else {
                    // Lifting: place the window where the preview showed.
                    if let Some(d) = dir {
                        out.push(Out::Preview(None));
                        out.push(Out::Act(Action::Swipe(d)));
                    }
                    self.state = State::Done;
                    if n == 0 {
                        self.state = State::Idle;
                        out.push(Out::Hold(false));
                    }
                    return out;
                };
                let together = moved_together(a, b, start);
                let (dx, dy) = together.unwrap_or((0.0, 0.0));
                let travel = (dx * dx + dy * dy).sqrt();
                match zone {
                    Zone::Dock => {
                        if together.is_some() && travel >= DOCK_SWIPE {
                            self.state = State::Done;
                            out.push(Out::Act(Action::Swipe(direction(dx, dy, false))));
                        } else if now - t0 > IDLE_SECS {
                            self.cancel(&mut out, false);
                            self.state = State::Ignored;
                        }
                    }
                    Zone::Title => {
                        let next = if travel >= SHOW || (dir.is_some() && travel >= HIDE) { Some(direction(dx, dy, true)) } else { None };
                        if next.is_none() && dir.is_none() && now - t0 > IDLE_SECS {
                            self.cancel(&mut out, false);
                            self.state = State::Ignored;
                            return out;
                        }
                        if next != dir {
                            out.push(Out::Preview(next));
                            self.state = State::Armed { zone, t0, ids, start, dir: next };
                        }
                    }
                }
            }
        }
        out
    }

    /// Lets go of the gesture without acting. `lifted`: the fingers are off the trackpad, so
    /// momentum may still follow; otherwise scrolling is let through at once.
    fn cancel(&mut self, out: &mut Vec<Out>, lifted: bool) {
        let release = if lifted { Out::Hold(false) } else { Out::Release };
        match self.state {
            State::Armed { dir, .. } => {
                if dir.is_some() {
                    out.push(Out::Preview(None));
                }
                out.push(release);
            }
            State::Done => out.push(release),
            State::Idle | State::Ignored => {}
        }
    }

    /// Called a few times a second. If trackpad frames stopped mid-gesture (the trackpad went
    /// away, the Mac slept), let go so scrolling isn't held back forever.
    pub fn tick(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        if matches!(self.state, State::Armed { .. } | State::Done) && now - self.last_frame > LOST_SECS {
            self.cancel(&mut out, true);
            self.state = State::Idle;
            self.fingers = 0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Settings {
        Settings::default()
    }

    fn two(ax: f32, ay: f32, bx: f32, by: f32) -> Vec<Contact> {
        vec![Contact { id: 1, x: ax, y: ay }, Contact { id: 2, x: bx, y: by }]
    }

    /// Plays a two-finger move from `from` to `to` over 10 frames, then lifts.
    fn play_in(g: &mut WindowGestures, t: &mut f64, from: (f32, f32, f32, f32), to: (f32, f32, f32, f32), zone: Option<Zone>) -> Vec<Out> {
        let mut out = Vec::new();
        for i in 0..=10 {
            let k = i as f32 / 10.0;
            let l = |a: f32, b: f32| a + (b - a) * k;
            let c = two(l(from.0, to.0), l(from.1, to.1), l(from.2, to.2), l(from.3, to.3));
            out.extend(g.frame(&c, *t, &s(), || zone));
            *t += 0.016;
        }
        out.extend(g.frame(&[], *t, &s(), || zone));
        *t += 0.016;
        out
    }

    fn play(g: &mut WindowGestures, t: &mut f64, from: (f32, f32, f32, f32), to: (f32, f32, f32, f32), title: bool) -> Vec<Out> {
        play_in(g, t, from, to, title.then_some(Zone::Title))
    }

    fn acts(out: &[Out]) -> Vec<Action> {
        out.iter().filter_map(|o| if let Out::Act(a) = o { Some(*a) } else { None }).collect()
    }

    #[test]
    fn swipes_pick_a_direction_with_a_preview() {
        let mut t = 0.0;
        for (to, want) in [
            ((0.42, 0.75, 0.58, 0.75), Dir::Up),
            ((0.42, 0.25, 0.58, 0.25), Dir::Down),
            ((0.24, 0.5, 0.40, 0.5), Dir::Left),
            ((0.60, 0.5, 0.76, 0.5), Dir::Right),
            ((0.32, 0.65, 0.48, 0.65), Dir::UpLeft),
            ((0.52, 0.35, 0.68, 0.35), Dir::DownRight),
        ] {
            let mut g = WindowGestures::new();
            let out = play(&mut g, &mut t, (0.42, 0.5, 0.58, 0.5), to, true);
            assert_eq!(acts(&out), vec![Action::Swipe(want)], "to {to:?}");
            assert!(out.contains(&Out::Preview(Some(want))));
            assert_eq!(out.first(), Some(&Out::Hold(true)));
            assert_eq!(out.last(), Some(&Out::Hold(false)));
            let hides = out.iter().filter(|o| **o == Out::Preview(None)).count();
            assert_eq!(hides, 1, "the preview goes away when the window is placed");
        }
    }

    #[test]
    fn the_preview_follows_the_fingers() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.36, 0.5, 0.52, 0.5), 0.05, &s(), z)); // left
        out.extend(g.frame(&two(0.48, 0.5, 0.64, 0.5), 0.10, &s(), z)); // over to the right
        out.extend(g.frame(&[], 0.15, &s(), z));
        assert_eq!(out.iter().filter(|o| matches!(o, Out::Preview(Some(_)))).count(), 2);
        assert!(out.contains(&Out::Preview(Some(Dir::Left))));
        assert!(out.contains(&Out::Preview(Some(Dir::Right))));
        assert_eq!(acts(&out), vec![Action::Swipe(Dir::Right)]);
    }

    #[test]
    fn sliding_back_to_the_middle_cancels() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.42, 0.62, 0.58, 0.62), 0.05, &s(), z));
        out.extend(g.frame(&two(0.42, 0.51, 0.58, 0.51), 0.10, &s(), z));
        out.extend(g.frame(&[], 0.15, &s(), z));
        assert!(out.contains(&Out::Preview(Some(Dir::Up))));
        assert!(out.contains(&Out::Preview(None)));
        assert!(acts(&out).is_empty());
        assert_eq!(out.last(), Some(&Out::Hold(false)));
    }

    #[test]
    fn only_in_a_zone() {
        let mut g = WindowGestures::new();
        let mut t = 0.0;
        let out = play(&mut g, &mut t, (0.42, 0.5, 0.58, 0.5), (0.42, 0.8, 0.58, 0.8), false);
        assert!(out.is_empty());
    }

    #[test]
    fn fingers_landing_one_after_the_other_still_arm() {
        let mut g = WindowGestures::new();
        let mut out = g.frame(&[Contact { id: 1, x: 0.42, y: 0.5 }], 0.0, &s(), || Some(Zone::Title));
        out.extend(g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.02, &s(), || Some(Zone::Title)));
        assert_eq!(out, vec![Out::Hold(true)]);
    }

    #[test]
    fn one_finger_lifting_first_still_places() {
        let mut g = WindowGestures::new();
        let mut out = Vec::new();
        out.extend(g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), || Some(Zone::Title)));
        out.extend(g.frame(&two(0.42, 0.8, 0.58, 0.8), 0.05, &s(), || Some(Zone::Title)));
        out.extend(g.frame(&[Contact { id: 2, x: 0.58, y: 0.8 }], 0.07, &s(), || None));
        out.extend(g.frame(&[], 0.09, &s(), || None));
        assert_eq!(acts(&out), vec![Action::Swipe(Dir::Up)]);
        assert_eq!(out.last(), Some(&Out::Hold(false)));
    }

    #[test]
    fn resting_fingers_let_scrolling_go() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.42, 0.5, 0.58, 0.5), 1.2, &s(), z));
        assert_eq!(out, vec![Out::Hold(true), Out::Release]);
        // Moving afterwards scrolls as usual.
        out.extend(g.frame(&two(0.42, 0.9, 0.58, 0.9), 1.3, &s(), z));
        out.extend(g.frame(&[], 1.4, &s(), z));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn dock_gestures() {
        let mut t = 0.0;
        let dock = Some(Zone::Dock);
        for (to, want) in [
            ((0.42, 0.75, 0.58, 0.75), Dir::Up),
            ((0.42, 0.25, 0.58, 0.25), Dir::Down),
            ((0.60, 0.5, 0.76, 0.5), Dir::Right),
            // Diagonals pick the stronger side on the Dock.
            ((0.32, 0.62, 0.48, 0.62), Dir::Left),
        ] {
            let mut g = WindowGestures::new();
            let out = play_in(&mut g, &mut t, (0.42, 0.5, 0.58, 0.5), to, dock);
            assert_eq!(acts(&out), vec![Action::Swipe(want)], "to {to:?}");
            assert!(!out.iter().any(|o| matches!(o, Out::Preview(_))));
        }
    }

    #[test]
    fn small_moves_and_edges_do_nothing() {
        let mut g = WindowGestures::new();
        let mut t = 0.0;
        assert!(acts(&play(&mut g, &mut t, (0.42, 0.5, 0.58, 0.5), (0.43, 0.53, 0.59, 0.53), true)).is_empty());
        // A finger in the edge zone belongs to the edge controls.
        let out = play(&mut g, &mut t, (0.05, 0.5, 0.3, 0.5), (0.05, 0.9, 0.3, 0.9), true);
        assert!(out.is_empty());
    }

    #[test]
    fn a_third_finger_cancels() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.42, 0.62, 0.58, 0.62), 0.02, &s(), z));
        let mut three = two(0.42, 0.65, 0.58, 0.65);
        three.push(Contact { id: 3, x: 0.5, y: 0.3 });
        out.extend(g.frame(&three, 0.04, &s(), z));
        out.extend(g.frame(&two(0.42, 0.9, 0.58, 0.9), 0.06, &s(), z));
        out.extend(g.frame(&[], 0.08, &s(), z));
        assert_eq!(out, vec![Out::Hold(true), Out::Preview(Some(Dir::Up)), Out::Preview(None), Out::Release]);
    }

    #[test]
    fn off_in_settings() {
        let mut g = WindowGestures::new();
        let off = Settings { window_gestures: false, dock_gestures: false, ..Settings::default() };
        assert!(g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &off, || Some(Zone::Title)).is_empty());
        // Top of window off, Dock on: the window is ignored.
        let mut g = WindowGestures::new();
        let dock_only = Settings { window_gestures: false, ..Settings::default() };
        assert!(g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &dock_only, || Some(Zone::Title)).is_empty());
    }

    #[test]
    fn switching_off_mid_gesture_lets_go() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.42, 0.62, 0.58, 0.62), 0.02, &s(), z));
        let off = Settings { enabled: false, ..Settings::default() };
        out.extend(g.frame(&two(0.42, 0.7, 0.58, 0.7), 0.04, &off, z));
        assert_eq!(out, vec![Out::Hold(true), Out::Preview(Some(Dir::Up)), Out::Preview(None), Out::Release]);
    }

    #[test]
    fn a_resting_thumb_and_a_dragging_finger_is_not_a_swipe() {
        // The usual way to drag a window: thumb holds the click, index finger drags.
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = Vec::new();
        for i in 0..=10 {
            let k = i as f32 / 10.0;
            let thumb = Contact { id: 1, x: 0.5, y: 0.1 };
            let finger = Contact { id: 2, x: 0.4 + 0.3 * k, y: 0.6 + 0.2 * k };
            out.extend(g.frame(&[thumb, finger], i as f64 * 0.016, &s(), z));
        }
        out.extend(g.frame(&[Contact { id: 1, x: 0.5, y: 0.1 }], 0.2, &s(), z));
        out.extend(g.frame(&[], 0.25, &s(), z));
        assert!(acts(&out).is_empty());
        assert!(!out.iter().any(|o| matches!(o, Out::Preview(Some(_)))));
    }

    #[test]
    fn fingers_going_different_ways_is_not_a_swipe() {
        let mut g = WindowGestures::new();
        let mut t = 0.0;
        // A pinch: the fingers move apart.
        let out = play(&mut g, &mut t, (0.45, 0.5, 0.55, 0.5), (0.30, 0.5, 0.70, 0.5), true);
        assert!(acts(&out).is_empty());
    }

    #[test]
    fn lost_frames_let_go() {
        let mut g = WindowGestures::new();
        let z = || Some(Zone::Title);
        let mut out = g.frame(&two(0.42, 0.5, 0.58, 0.5), 0.0, &s(), z);
        out.extend(g.frame(&two(0.42, 0.62, 0.58, 0.62), 0.02, &s(), z));
        assert!(g.tick(0.3).is_empty());
        out.extend(g.tick(0.6));
        assert_eq!(out, vec![Out::Hold(true), Out::Preview(Some(Dir::Up)), Out::Preview(None), Out::Hold(false)]);
    }
}
