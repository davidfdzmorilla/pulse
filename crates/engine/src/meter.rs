//! Time signatures and where they change: bars, counts and bar lines for any meter
//! (4/4, 3/4, 6/8, 7/8…), with changes along the song. Beats are quarter notes.

use serde::{Deserialize, Serialize};

/// A time signature: `beats` notes of 1/`unit` a bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeSig {
    pub beats: u8,
    pub unit: u8,
}

impl Default for TimeSig {
    fn default() -> Self {
        Self::COMMON
    }
}

impl TimeSig {
    pub const COMMON: TimeSig = TimeSig { beats: 4, unit: 4 };

    /// The ones offered in menus, most used first.
    pub const PRESETS: [TimeSig; 11] = [
        TimeSig { beats: 4, unit: 4 },
        TimeSig { beats: 3, unit: 4 },
        TimeSig { beats: 2, unit: 4 },
        TimeSig { beats: 6, unit: 8 },
        TimeSig { beats: 12, unit: 8 },
        TimeSig { beats: 5, unit: 4 },
        TimeSig { beats: 7, unit: 8 },
        TimeSig { beats: 9, unit: 8 },
        TimeSig { beats: 5, unit: 8 },
        TimeSig { beats: 7, unit: 4 },
        TimeSig { beats: 2, unit: 2 },
    ];

    /// A valid signature near `beats`/`unit`: 1..32 beats of a power-of-two unit (1..32).
    pub fn new(beats: u32, unit: u32) -> Self {
        let unit = [1u32, 2, 4, 8, 16, 32].into_iter().min_by_key(|u| u.abs_diff(unit)).unwrap_or(4);
        TimeSig { beats: beats.clamp(1, 32) as u8, unit: unit as u8 }
    }

    /// Length of a bar, in beats (quarter notes): 3 in 3/4, 3 in 6/8, 3.5 in 7/8.
    pub fn bar(self) -> f64 {
        self.beats.max(1) as f64 * 4.0 / self.unit.max(1) as f64
    }

    /// Compound meters (6/8, 9/8, 12/8…) are felt in dotted beats of three.
    pub fn compound(self) -> bool {
        self.unit >= 8 && self.beats > 3 && self.beats % 3 == 0
    }

    /// What the metronome counts, in beats: a dotted quarter in 6/8, an eighth in 7/8.
    pub fn count(self) -> f64 {
        let note = 4.0 / self.unit.max(1) as f64;
        if self.compound() { note * 3.0 } else { note }
    }

    /// Counts in a bar: 4 in 4/4, 2 in 6/8, 7 in 7/8.
    pub fn counts(self) -> u32 {
        (self.bar() / self.count()).round().max(1.0) as u32
    }

    /// The bar's pulses, as felt: (start, length) in beats. Quarters in 4/4 and 3/4,
    /// dotted quarters in 6/8, and in odd eighths groups of two with one of three
    /// (5/8 is 3+2, 7/8 is 2+2+3).
    pub fn pulses(self) -> Vec<(f64, f64)> {
        let note = 4.0 / self.unit.max(1) as f64;
        let groups: Vec<u32> = if self.compound() {
            vec![3; self.beats as usize / 3]
        } else if self.unit >= 8 && self.beats > 3 {
            let n = self.beats as u32;
            if n == 5 {
                vec![3, 2]
            } else if n % 2 == 0 {
                vec![2; n as usize / 2]
            } else {
                let mut g = vec![2; (n as usize - 3) / 2];
                g.push(3);
                g
            }
        } else {
            vec![1; self.beats.max(1) as usize]
        };
        let mut at = 0.0;
        groups
            .into_iter()
            .map(|g| {
                let pulse = (at, g as f64 * note);
                at += g as f64 * note;
                pulse
            })
            .collect()
    }

    pub fn label(self) -> String {
        format!("{}/{}", self.beats, self.unit)
    }

    /// Reads «6/8», «7 / 8»…
    pub fn parse(text: &str) -> Option<TimeSig> {
        let (a, b) = text.split_once('/')?;
        let (beats, unit) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
        (beats >= 1 && unit >= 1).then(|| TimeSig::new(beats, unit))
    }
}

/// Changes a song can have.
pub const MAX_METER_CHANGES: usize = 16;

/// From bar `bar` (counting from 0) on, the song is in `sig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterChange {
    pub bar: u32,
    pub sig: TimeSig,
}

/// The song's time signatures: the first from bar 0, then each change. Fixed-size,
/// so the audio thread gets it as a plain copy. Saved as a list of changes.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<MeterChange>", into = "Vec<MeterChange>")]
pub struct Meter {
    changes: [MeterChange; MAX_METER_CHANGES],
    count: u8,
}

impl Default for Meter {
    fn default() -> Self {
        Meter::COMMON
    }
}

impl std::fmt::Debug for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.changes().iter().map(|c| (c.bar, c.sig.label()))).finish()
    }
}

impl From<Vec<MeterChange>> for Meter {
    /// In bar order, one a bar (the last given wins), starting at bar 0 (4/4 if
    /// none says), without changes to the signature already playing.
    fn from(mut list: Vec<MeterChange>) -> Self {
        list.reverse();
        list.sort_by_key(|c| c.bar);
        list.dedup_by_key(|c| c.bar);
        let mut meter = Meter::COMMON;
        for change in list {
            let sig = TimeSig::new(change.sig.beats as u32, change.sig.unit as u32);
            if change.bar == 0 {
                meter.changes[0].sig = sig;
            } else if meter.changes[meter.count as usize - 1].sig != sig && (meter.count as usize) < MAX_METER_CHANGES {
                meter.changes[meter.count as usize] = MeterChange { bar: change.bar, sig };
                meter.count += 1;
            }
        }
        // A first signature equal to the second makes that change say nothing.
        if meter.count > 1 && meter.changes[1].sig == meter.changes[0].sig {
            meter.changes.copy_within(2..meter.count as usize, 1);
            meter.count -= 1;
        }
        meter
    }
}

impl From<Meter> for Vec<MeterChange> {
    fn from(meter: Meter) -> Self {
        meter.changes().to_vec()
    }
}

impl Meter {
    /// The whole song in 4/4. (Made without allocating: the audio thread uses it.)
    pub const COMMON: Meter = Meter::new(TimeSig::COMMON);

    /// The whole song in `sig`.
    pub const fn new(sig: TimeSig) -> Self {
        Meter { changes: [MeterChange { bar: 0, sig }; MAX_METER_CHANGES], count: 1 }
    }

    pub fn changes(&self) -> &[MeterChange] {
        &self.changes[..self.count.max(1) as usize]
    }

    /// The whole song in 4/4.
    pub fn is_common(&self) -> bool {
        self.count <= 1 && self.changes[0].sig == TimeSig::COMMON
    }

    /// The signature the song starts in.
    pub fn first(&self) -> TimeSig {
        self.changes[0].sig
    }

    /// The same with `sig` from bar `bar` on (replacing a change there).
    pub fn with_change(&self, bar: u32, sig: TimeSig) -> Meter {
        let mut list: Vec<MeterChange> = self.changes().iter().filter(|c| c.bar != bar).copied().collect();
        list.push(MeterChange { bar, sig });
        Meter::from(list)
    }

    /// The same without the change at bar `bar` (the first one can't go: it becomes 4/4).
    pub fn without_change(&self, bar: u32) -> Meter {
        if bar == 0 {
            return self.with_change(0, TimeSig::COMMON);
        }
        Meter::from(self.changes().iter().filter(|c| c.bar != bar).copied().collect::<Vec<_>>())
    }

    /// Each change with the beat it starts at.
    fn segments(&self) -> impl Iterator<Item = (MeterChange, f64)> + '_ {
        let mut start = 0.0;
        let mut previous: Option<MeterChange> = None;
        self.changes().iter().map(move |&c| {
            if let Some(p) = previous {
                start += (c.bar - p.bar) as f64 * p.sig.bar();
            }
            previous = Some(c);
            (c, start)
        })
    }

    /// The change in force at `beat`, with its starting beat (before the song's start:
    /// the first signature, going back).
    fn segment_at(&self, beat: f64) -> (MeterChange, f64) {
        let mut found = (self.changes[0], 0.0);
        for (change, start) in self.segments() {
            if start <= beat + 1e-9 {
                found = (change, start);
            } else {
                break;
            }
        }
        found
    }

    /// The signature of bar `bar` (bars before 0 have the first).
    pub fn sig_at_bar(&self, bar: i64) -> TimeSig {
        self.changes().iter().rev().find(|c| c.bar as i64 <= bar).map_or(self.changes[0].sig, |c| c.sig)
    }

    /// The signature playing at `beat`.
    pub fn sig_at(&self, beat: f64) -> TimeSig {
        self.segment_at(beat).0.sig
    }

    /// The beat where bar `bar` starts.
    pub fn bar_start(&self, bar: i64) -> f64 {
        let mut found = (self.changes[0], 0.0);
        for (change, start) in self.segments() {
            if change.bar as i64 <= bar {
                found = (change, start);
            }
        }
        let (change, start) = found;
        start + (bar - change.bar as i64) as f64 * change.sig.bar()
    }

    /// The bar `beat` is in, and how far into it (beats).
    pub fn bar_at(&self, beat: f64) -> (i64, f64) {
        let (change, start) = self.segment_at(beat);
        let len = change.sig.bar();
        let into = ((beat - start) / len + 1e-9).floor();
        let bar = change.bar as i64 + into as i64;
        (bar, (beat - self.bar_start(bar)).max(0.0))
    }

    /// Length of the bar at `beat`, in beats.
    pub fn bar_length_at(&self, beat: f64) -> f64 {
        self.sig_at(beat).bar()
    }

    /// The first bar line at or after `beat`.
    pub fn next_bar(&self, beat: f64) -> f64 {
        let (bar, into) = self.bar_at(beat);
        if into < 1e-9 { self.bar_start(bar) } else { self.bar_start(bar + 1) }
    }

    /// The last bar line at or before `beat`.
    pub fn prev_bar(&self, beat: f64) -> f64 {
        self.bar_start(self.bar_at(beat).0)
    }

    /// The bar line nearest to `beat`.
    pub fn round_to_bar(&self, beat: f64) -> f64 {
        let (before, after) = (self.prev_bar(beat), self.next_bar(beat));
        if beat - before <= after - beat { before } else { after }
    }

    /// `bars` bars on from `beat` (whole bars, following the changes; negative goes back).
    pub fn add_bars(&self, beat: f64, bars: i64) -> f64 {
        let (bar, into) = self.bar_at(beat);
        let to = bar + bars;
        let len = self.sig_at_bar(to).bar();
        self.bar_start(to) + into.min(len)
    }

    /// Where `beat` is as people count it: bar, count and sixteenth, all from 1.
    pub fn position(&self, beat: f64) -> (i64, u32, u32) {
        let (bar, into) = self.bar_at(beat);
        let count = self.sig_at_bar(bar).count();
        let n = (into / count + 1e-9).floor();
        let sixteenth = ((into - n * count) / 0.25 + 1e-9).floor();
        (bar + 1, n as u32 + 1, sixteenth as u32 + 1)
    }

    /// The metronome's counts from `from` (included) to `to` (not), with true on
    /// each bar's first.
    pub fn counts(&self, from: f64, to: f64, mut each: impl FnMut(f64, bool)) {
        let (mut bar, into) = self.bar_at(from);
        let mut k = {
            let count = self.sig_at_bar(bar).count();
            (into / count - 1e-9).ceil().max(0.0) as u32
        };
        loop {
            let sig = self.sig_at_bar(bar);
            if k >= sig.counts() {
                bar += 1;
                k = 0;
                continue;
            }
            let at = self.bar_start(bar) + k as f64 * sig.count();
            if at >= to - 1e-9 {
                break;
            }
            if at >= from - 1e-9 {
                each(at, k == 0);
            }
            k += 1;
        }
    }

    /// Each bar line from `from` (included) to `to` (not), with its bar number (from 0).
    pub fn bars(&self, from: f64, to: f64, mut each: impl FnMut(i64, f64)) {
        let mut bar = self.bar_at(self.next_bar(from)).0;
        loop {
            let at = self.bar_start(bar);
            if at >= to - 1e-9 {
                break;
            }
            each(bar, at);
            bar += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(b: u32, u: u32) -> TimeSig {
        TimeSig::new(b, u)
    }

    #[test]
    fn signatures_know_their_bars_and_counts() {
        assert_eq!((sig(4, 4).bar(), sig(4, 4).count(), sig(4, 4).counts()), (4.0, 1.0, 4));
        assert_eq!((sig(3, 4).bar(), sig(3, 4).counts()), (3.0, 3));
        assert_eq!((sig(6, 8).bar(), sig(6, 8).count(), sig(6, 8).counts()), (3.0, 1.5, 2));
        assert_eq!((sig(12, 8).bar(), sig(12, 8).counts()), (6.0, 4));
        assert_eq!((sig(7, 8).bar(), sig(7, 8).count(), sig(7, 8).counts()), (3.5, 0.5, 7));
        assert_eq!((sig(2, 2).bar(), sig(2, 2).count()), (4.0, 2.0));
        assert_eq!(sig(5, 5).unit, 4, "units are powers of two");
        assert_eq!(TimeSig::parse(" 6 / 8"), Some(sig(6, 8)));
        assert_eq!(TimeSig::parse("x/8"), None);
    }

    #[test]
    fn bars_are_felt_in_pulses() {
        assert_eq!(sig(4, 4).pulses(), [(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 1.0)]);
        assert_eq!(sig(3, 4).pulses().len(), 3);
        assert_eq!(sig(6, 8).pulses(), [(0.0, 1.5), (1.5, 1.5)]);
        assert_eq!(sig(7, 8).pulses(), [(0.0, 1.0), (1.0, 1.0), (2.0, 1.5)]);
        assert_eq!(sig(5, 8).pulses(), [(0.0, 1.5), (1.5, 1.0)]);
        assert_eq!(sig(12, 8).pulses().len(), 4);
        for s in TimeSig::PRESETS {
            let total: f64 = s.pulses().iter().map(|p| p.1).sum();
            assert!((total - s.bar()).abs() < 1e-9, "{}", s.label());
        }
    }

    #[test]
    fn a_meter_maps_beats_to_bars_across_changes() {
        // 4/4 for 2 bars, then 3/4 for 2, then 6/8.
        let m = Meter::default().with_change(2, sig(3, 4)).with_change(4, sig(6, 8));
        assert_eq!(m.bar_start(0), 0.0);
        assert_eq!(m.bar_start(2), 8.0);
        assert_eq!(m.bar_start(3), 11.0);
        assert_eq!(m.bar_start(4), 14.0);
        assert_eq!(m.bar_start(6), 20.0);
        assert_eq!(m.bar_at(12.5), (3, 1.5));
        assert_eq!(m.bar_at(14.0), (4, 0.0));
        assert_eq!(m.bar_at(-1.0), (-1, 3.0), "before the start, the first signature goes back");
        assert_eq!(m.sig_at(13.9), sig(3, 4));
        assert_eq!(m.next_bar(8.5), 11.0);
        assert_eq!(m.next_bar(11.0), 11.0);
        assert_eq!(m.prev_bar(13.0), 11.0);
        assert_eq!(m.round_to_bar(12.6), 14.0);
        assert_eq!(m.add_bars(1.0, 3), 12.0);
        assert_eq!(m.position(15.5), (5, 2, 1), "6/8 counts dotted quarters");
        assert_eq!(m.position(9.25), (3, 2, 2));
    }

    #[test]
    fn the_metronome_counts_each_meter() {
        let m = Meter::new(sig(7, 8));
        let mut clicks = Vec::new();
        m.counts(0.0, 3.5, |at, accent| clicks.push((at, accent)));
        assert_eq!(clicks.len(), 7);
        assert_eq!(clicks[0], (0.0, true));
        assert_eq!(clicks[1], (0.5, false));
        let m = Meter::default().with_change(1, sig(6, 8));
        let mut clicks = Vec::new();
        m.counts(2.5, 10.0, |at, accent| clicks.push((at, accent)));
        assert_eq!(clicks, [(3.0, false), (4.0, true), (5.5, false), (7.0, true), (8.5, false)]);
        let mut bars = Vec::new();
        m.bars(0.5, 10.0, |bar, at| bars.push((bar, at)));
        assert_eq!(bars, [(1, 4.0), (2, 7.0)]);
    }

    #[test]
    fn changes_stay_tidy_and_save_as_a_list() {
        let m = Meter::from(vec![MeterChange { bar: 4, sig: sig(3, 4) }, MeterChange { bar: 8, sig: sig(3, 4) }, MeterChange { bar: 2, sig: sig(4, 4) }]);
        assert_eq!(m.changes(), [MeterChange { bar: 0, sig: TimeSig::COMMON }, MeterChange { bar: 4, sig: sig(3, 4) }], "no changes to what already plays");
        assert!(Meter::default().is_common() && !m.is_common());
        assert!(m.without_change(4).is_common());
        assert_eq!(Meter::new(sig(3, 4)).with_change(5, sig(3, 4)).changes().len(), 1);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Meter>(&json).unwrap(), m);
        assert!(json.starts_with('['));
    }
}
