//! Follow actions: what a session clip (or a scene) does after it has played
//! for a while, as in Live. Two actions, A and B, with a chance between them;
//! the one chosen picks the next clip of its group (the clips next to it on
//! its track, with no empty slot between) or stops the track.
//!
//! Everything here is plain functions on plain data, so the engine decides on
//! the audio thread without allocating and the tests can check each rule.

use crate::meter::Meter;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// What happens when the time is up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FollowKind {
    /// Nothing: it keeps playing (and decides again after another stretch).
    #[default]
    None,
    Stop,
    /// The same clip from its start.
    Again,
    Previous,
    Next,
    First,
    Last,
    /// Any clip of the group, this one included.
    Any,
    /// Any other clip of the group (with none: nothing).
    Other,
}

impl FollowKind {
    pub const ALL: [FollowKind; 9] = [Self::None, Self::Stop, Self::Again, Self::Previous, Self::Next, Self::First, Self::Last, Self::Any, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Nada",
            Self::Stop => "Parar",
            Self::Again => "El mismo",
            Self::Previous => "Anterior",
            Self::Next => "Siguiente",
            Self::First => "Primero",
            Self::Last => "Último",
            Self::Any => "Cualquiera",
            Self::Other => "Otro al azar",
        }
    }
}

/// What the time is counted in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FollowUnit {
    /// Times through the clip (for a scene: its longest clip).
    #[default]
    Loops,
    /// Bars of the song's signature where it started.
    Bars,
}

/// A clip's or a scene's follow action.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Follow {
    pub a: FollowKind,
    pub b: FollowKind,
    /// The chance of A (0..1); B gets the rest.
    pub chance_a: f32,
    /// How long it plays before deciding: `count` loops or bars.
    pub count: u16,
    pub unit: FollowUnit,
}

impl Default for Follow {
    fn default() -> Self {
        Follow { a: FollowKind::None, b: FollowKind::None, chance_a: 1.0, count: 1, unit: FollowUnit::Loops }
    }
}

impl Follow {
    /// Whether it ever does anything.
    pub fn is_active(&self) -> bool {
        self.count > 0 && ((self.a != FollowKind::None && self.chance_a > 0.0) || (self.b != FollowKind::None && self.chance_a < 1.0))
    }

    /// How many beats it plays before deciding, if it started at `launch` and
    /// loops every `loop_length` beats (0: never).
    pub fn duration(&self, loop_length: f64, meter: &Meter, launch: f64) -> f64 {
        let count = self.count as f64;
        match self.unit {
            FollowUnit::Loops => count * loop_length.max(0.0),
            FollowUnit::Bars => count * meter.bar_length_at(launch.max(0.0)),
        }
    }

    /// A or B, by the chance, with the next number of `rng`.
    pub fn choose(&self, rng: &mut u64) -> FollowKind {
        if random(rng) < self.chance_a.clamp(0.0, 1.0) as f64 {
            self.a
        } else {
            self.b
        }
    }
}

/// The result of an action: a slot (or scene) to launch, or a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowTarget {
    Launch(usize),
    Stop,
}

/// The group of `current`: the filled slots around it with no empty one between.
pub fn group(current: usize, slots: usize, filled: impl Fn(usize) -> bool) -> Range<usize> {
    let mut start = current;
    while start > 0 && filled(start - 1) {
        start -= 1;
    }
    let mut end = current + 1;
    while end < slots && filled(end) {
        end += 1;
    }
    start..end
}

/// Where `kind` goes from `current` within `group` (Next and Previous go round).
pub fn resolve(kind: FollowKind, current: usize, group: Range<usize>, rng: &mut u64) -> Option<FollowTarget> {
    let len = group.len();
    if len == 0 || !group.contains(&current) {
        return match kind {
            FollowKind::Stop => Some(FollowTarget::Stop),
            _ => None,
        };
    }
    let at = current - group.start;
    let pick = |n: usize, rng: &mut u64| ((random(rng) * n as f64) as usize).min(n - 1);
    let index = match kind {
        FollowKind::None => return None,
        FollowKind::Stop => return Some(FollowTarget::Stop),
        FollowKind::Again => at,
        FollowKind::Previous => (at + len - 1) % len,
        FollowKind::Next => (at + 1) % len,
        FollowKind::First => 0,
        FollowKind::Last => len - 1,
        FollowKind::Any => pick(len, rng),
        FollowKind::Other if len < 2 => return None,
        // One of the others: a draw among len - 1, skipping this one.
        FollowKind::Other => {
            let other = pick(len - 1, rng);
            if other >= at {
                other + 1
            } else {
                other
            }
        }
    };
    Some(FollowTarget::Launch(group.start + index))
}

/// When it next decides, from `launch`, every `duration` beats, at `from` or
/// after (the first time, `duration` after `launch`).
pub fn next_decision(launch: f64, duration: f64, from: f64) -> Option<f64> {
    if !(duration > 1e-9) || !duration.is_finite() {
        return None;
    }
    let k = ((from - launch) / duration - 1e-9).ceil().max(1.0);
    Some(launch + k * duration)
}

/// The seed the draws start from each time the transport starts: the same
/// choices every time, and as exported.
pub const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// A number in 0..1 from xorshift64* (no allocation, the same on every machine).
pub fn random(state: &mut u64) -> f64 {
    let mut x = if *state == 0 { SEED } else { *state };
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_the_clips_next_to_each_other() {
        // Slots 0 1 2 filled, 3 empty, 4 5 filled.
        let filled = |s: usize| [true, true, true, false, true, true, false, false][s];
        assert_eq!(group(1, 8, filled), 0..3);
        assert_eq!(group(0, 8, filled), 0..3);
        assert_eq!(group(5, 8, filled), 4..6);
        assert_eq!(group(4, 8, filled), 4..6);
    }

    #[test]
    fn each_action_goes_where_it_says() {
        let mut rng = SEED;
        let g = 2..6;
        let go = |kind, current, rng: &mut u64| resolve(kind, current, g.clone(), rng);
        assert_eq!(go(FollowKind::Next, 3, &mut rng), Some(FollowTarget::Launch(4)));
        assert_eq!(go(FollowKind::Next, 5, &mut rng), Some(FollowTarget::Launch(2)), "round to the first");
        assert_eq!(go(FollowKind::Previous, 2, &mut rng), Some(FollowTarget::Launch(5)), "round to the last");
        assert_eq!(go(FollowKind::Previous, 4, &mut rng), Some(FollowTarget::Launch(3)));
        assert_eq!(go(FollowKind::First, 4, &mut rng), Some(FollowTarget::Launch(2)));
        assert_eq!(go(FollowKind::Last, 3, &mut rng), Some(FollowTarget::Launch(5)));
        assert_eq!(go(FollowKind::Again, 3, &mut rng), Some(FollowTarget::Launch(3)));
        assert_eq!(go(FollowKind::Stop, 3, &mut rng), Some(FollowTarget::Stop));
        assert_eq!(go(FollowKind::None, 3, &mut rng), None);
        // Alone in its group: no other to go to.
        assert_eq!(resolve(FollowKind::Other, 7, 7..8, &mut rng), None);
        assert_eq!(resolve(FollowKind::Any, 7, 7..8, &mut rng), Some(FollowTarget::Launch(7)));
    }

    #[test]
    fn other_never_repeats_and_any_reaches_them_all() {
        let mut rng = SEED;
        let mut other = [0; 4];
        let mut any = [0; 4];
        for _ in 0..4000 {
            let Some(FollowTarget::Launch(s)) = resolve(FollowKind::Other, 1, 0..4, &mut rng) else { panic!() };
            other[s] += 1;
            let Some(FollowTarget::Launch(s)) = resolve(FollowKind::Any, 1, 0..4, &mut rng) else { panic!() };
            any[s] += 1;
        }
        assert_eq!(other[1], 0, "never the same one: {other:?}");
        assert!(other.iter().enumerate().filter(|&(i, _)| i != 1).all(|(_, &n)| (1150..1520).contains(&n)), "{other:?}");
        assert!(any.iter().all(|&n| (850..1150).contains(&n)), "{any:?}");
    }

    #[test]
    fn the_chance_shares_a_and_b() {
        let follow = Follow { a: FollowKind::Next, b: FollowKind::Stop, chance_a: 0.75, ..Default::default() };
        let mut rng = SEED;
        let a = (0..10_000).filter(|_| follow.choose(&mut rng) == FollowKind::Next).count();
        assert!((7300..7700).contains(&a), "{a} of 10000");
        // The same seed, the same choices.
        let (mut r1, mut r2) = (SEED, SEED);
        let first: Vec<_> = (0..50).map(|_| follow.choose(&mut r1)).collect();
        let again: Vec<_> = (0..50).map(|_| follow.choose(&mut r2)).collect();
        assert_eq!(first, again);
        // All A, all B.
        assert_eq!(Follow { chance_a: 1.0, ..follow }.choose(&mut rng), FollowKind::Next);
        assert_eq!(Follow { chance_a: 0.0, ..follow }.choose(&mut rng), FollowKind::Stop);
    }

    #[test]
    fn active_only_with_something_to_do() {
        assert!(!Follow::default().is_active());
        assert!(Follow { a: FollowKind::Next, ..Default::default() }.is_active());
        assert!(!Follow { b: FollowKind::Next, chance_a: 1.0, ..Default::default() }.is_active(), "B never chosen");
        assert!(Follow { b: FollowKind::Next, chance_a: 0.5, ..Default::default() }.is_active());
        assert!(!Follow { a: FollowKind::Next, count: 0, ..Default::default() }.is_active());
    }

    #[test]
    fn durations_and_decision_times() {
        use crate::meter::TimeSig;
        let loops = Follow { a: FollowKind::Next, count: 2, ..Default::default() };
        assert_eq!(loops.duration(3.0, &Meter::default(), 0.0), 6.0);
        let bars = Follow { unit: FollowUnit::Bars, ..loops };
        assert_eq!(bars.duration(3.0, &Meter::default(), 0.0), 8.0);
        assert_eq!(bars.duration(3.0, &Meter::new(TimeSig::new(3, 4)), 0.0), 6.0);
        assert_eq!(next_decision(4.0, 8.0, 4.0), Some(12.0), "the first one, a stretch after the launch");
        assert_eq!(next_decision(4.0, 8.0, 12.0), Some(12.0), "on it: now");
        assert_eq!(next_decision(4.0, 8.0, 12.5), Some(20.0), "missed: the next one");
        assert_eq!(next_decision(4.0, 0.0, 5.0), None);
    }
}
