//! Chords: naming the one that sounds (from how much each pitch class plays,
//! and the bass), the chords of a key and what usually follows each, and
//! voicing them so that one leads smoothly into the next.

use crate::key::Key;

const NAMES: [&str; 12] = ["Do", "Do#", "Re", "Mib", "Mi", "Fa", "Fa#", "Sol", "Lab", "La", "Sib", "Si"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Seventh,
    Major7,
    Minor7,
    HalfDiminished,
}

impl Quality {
    const ALL: [Quality; 10] = [
        Quality::Major,
        Quality::Minor,
        Quality::Seventh,
        Quality::Minor7,
        Quality::Major7,
        Quality::Diminished,
        Quality::HalfDiminished,
        Quality::Sus4,
        Quality::Sus2,
        Quality::Augmented,
    ];

    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Quality::Major => &[0, 4, 7],
            Quality::Minor => &[0, 3, 7],
            Quality::Diminished => &[0, 3, 6],
            Quality::Augmented => &[0, 4, 8],
            Quality::Sus2 => &[0, 2, 7],
            Quality::Sus4 => &[0, 5, 7],
            Quality::Seventh => &[0, 4, 7, 10],
            Quality::Major7 => &[0, 4, 7, 11],
            Quality::Minor7 => &[0, 3, 7, 10],
            Quality::HalfDiminished => &[0, 3, 6, 10],
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Quality::Major => "",
            Quality::Minor => "m",
            Quality::Diminished => "°",
            Quality::Augmented => "+",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Seventh => "7",
            Quality::Major7 => "maj7",
            Quality::Minor7 => "m7",
            Quality::HalfDiminished => "ø",
        }
    }

    /// How much less likely than a plain major or minor triad, to break ties
    /// between chords that fit the notes as well.
    fn rarity(self) -> f64 {
        match self {
            Quality::Major | Quality::Minor => 0.0,
            Quality::Seventh | Quality::Minor7 | Quality::Major7 => 0.1,
            Quality::Diminished | Quality::HalfDiminished => 0.06,
            Quality::Sus4 | Quality::Sus2 => 0.07,
            Quality::Augmented => 0.09,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    /// Pitch class of the root (0 = C).
    pub root: u8,
    pub quality: Quality,
    /// Pitch class of the bass, when it is not the root (an inversion).
    pub bass: Option<u8>,
}

impl Chord {
    pub fn new(root: u8, quality: Quality) -> Self {
        Self { root: root % 12, quality, bass: None }
    }

    /// The chord's pitch classes, root first.
    pub fn classes(&self) -> Vec<u8> {
        self.quality.intervals().iter().map(|i| (self.root + i) % 12).collect()
    }

    /// «Lam», «Sol7», «Do/Mi».
    pub fn name(&self) -> String {
        let name = format!("{}{}", NAMES[self.root as usize], self.quality.suffix());
        match self.bass {
            Some(b) => format!("{name}/{}", NAMES[b as usize]),
            None => name,
        }
    }

    /// The chord's degree in `key` as a roman numeral («vi», «V7», «ii°»), if its
    /// root is in the scale.
    pub fn numeral(&self, key: Key) -> Option<String> {
        let degree = (0..7).find(|&d| key.degree(d) == self.root)?;
        const ROMAN: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
        let minor_third = self.quality.intervals().contains(&3);
        let numeral = if minor_third { ROMAN[degree].to_lowercase() } else { ROMAN[degree].to_string() };
        let suffix = match self.quality {
            Quality::Major | Quality::Minor => "",
            Quality::Minor7 => "7",
            q => q.suffix(),
        };
        Some(format!("{numeral}{suffix}"))
    }

    /// The chord that sounds: `weights` is how much each pitch class plays (C
    /// first), `bass` the lowest one, if known; `key` makes its own chords a
    /// little likelier (and those off it less). None when too little sounds to
    /// tell.
    pub fn detect(weights: &[f64; 12], bass: Option<u8>, key: Option<Key>) -> Option<Chord> {
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        // What sounds above the floor every note has in a mix (drums, reverb):
        // the median of the twelve.
        let mut sorted = *weights;
        sorted.sort_by(|a, b| a.total_cmp(b));
        let floor = (sorted[5] + sorted[6]) / 2.0;
        let lifted = weights.map(|x| (x - floor).max(0.0));
        let total: f64 = lifted.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let w = lifted.map(|x| x / total);
        // A chord needs at least two notes that are really there.
        if w.iter().filter(|&&x| x > 0.1).count() < 2 {
            return None;
        }
        let scale = key.map(|k| k.scale());
        let mut best: Option<(Chord, f64)> = None;
        for root in 0..12u8 {
            for quality in Quality::ALL {
                let chord = Chord::new(root, quality);
                let tones = chord.classes();
                // The root must sound; a seventh, about as much as the rest (in
                // audio the third's overtones alone make a faint major seventh).
                let weakest = tones[..3].iter().map(|&t| w[t as usize]).fold(f64::INFINITY, f64::min);
                let tetrad_weak = tones.len() == 4 && (w[tones[3] as usize] < (0.7 * weakest).max(0.08) || weakest < 0.06);
                if w[root as usize] < 0.05 || tetrad_weak {
                    continue;
                }
                let inside: f64 = tones.iter().map(|&t| w[t as usize]).sum();
                let mut score = inside - (1.0 - inside) - quality.rarity();
                if bass == Some(root) {
                    score += 0.08;
                } else if bass.is_some_and(|b| tones.contains(&b)) {
                    score += 0.02;
                }
                // In a known key its chords are likelier, and the rest need
                // more to show for each note off the scale.
                if let Some(scale) = scale {
                    let off = tones.iter().filter(|&&t| !scale[t as usize]).count();
                    score += if off == 0 { 0.04 } else { -0.05 * off as f64 };
                }
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((chord, score));
                }
            }
        }
        let (mut chord, score) = best?;
        if score < 0.2 {
            return None;
        }
        chord.bass = bass.filter(|&b| b != chord.root && chord.classes().contains(&b));
        Some(chord)
    }
}

/// The seven chords of `key`, on each degree: triads, or seventh chords.
pub fn diatonic(key: Key, sevenths: bool) -> [Chord; 7] {
    std::array::from_fn(|d| {
        let root = key.degree(d);
        let third = (key.degree(d + 2) + 12 - root) % 12;
        let fifth = (key.degree(d + 4) + 12 - root) % 12;
        let seventh = (key.degree(d + 6) + 12 - root) % 12;
        let quality = match (third, fifth, sevenths.then_some(seventh)) {
            (4, 7, None) => Quality::Major,
            (3, 7, None) => Quality::Minor,
            (3, 6, None) => Quality::Diminished,
            (4, 7, Some(11)) => Quality::Major7,
            (4, 7, Some(_)) => Quality::Seventh,
            (3, 7, Some(_)) => Quality::Minor7,
            (3, 6, Some(_)) => Quality::HalfDiminished,
            _ => Quality::Augmented,
        };
        Chord::new(root, quality)
    })
}

/// The degrees (0 = tonic) that usually come after `degree`, likeliest first.
pub fn next_degrees(key: Key, degree: usize) -> &'static [usize] {
    if key.minor {
        // i → iv, VI, VII, v; ii° → v; III → VI, iv; iv → v, i, VII; v → i, VI;
        // VI → VII, iv, III; VII → III, i.
        match degree {
            0 => &[3, 5, 6, 4],
            1 => &[4, 0],
            2 => &[5, 3],
            3 => &[4, 0, 6],
            4 => &[0, 5],
            5 => &[6, 3, 2],
            _ => &[2, 0],
        }
    } else {
        // I → IV, V, vi, ii; ii → V, vii°; iii → vi, IV; IV → V, I, ii; V → I,
        // vi; vi → IV, ii, V; vii° → I, iii.
        match degree {
            0 => &[3, 4, 5, 1],
            1 => &[4, 6],
            2 => &[5, 3],
            3 => &[4, 0, 1],
            4 => &[0, 5],
            5 => &[3, 1, 4],
            _ => &[0, 2],
        }
    }
}

/// A ready-made progression: its name and its degrees.
pub struct Progression {
    pub name: &'static str,
    pub degrees: &'static [usize],
    /// The fifth degree as a major chord (the harmonic minor's V).
    pub major_five: bool,
}

pub fn progressions(key: Key) -> &'static [Progression] {
    const MAJOR: [Progression; 6] = [
        Progression { name: "Pop (I–V–vi–IV)", degrees: &[0, 4, 5, 3], major_five: false },
        Progression { name: "Balada (vi–IV–I–V)", degrees: &[5, 3, 0, 4], major_five: false },
        Progression { name: "Años 50 (I–vi–IV–V)", degrees: &[0, 5, 3, 4], major_five: false },
        Progression { name: "Jazz (ii–V–I)", degrees: &[1, 4, 0, 0], major_five: false },
        Progression { name: "Rock (I–IV–I–V)", degrees: &[0, 3, 0, 4], major_five: false },
        Progression { name: "Canon (I–V–vi–iii–IV–I–IV–V)", degrees: &[0, 4, 5, 2, 3, 0, 3, 4], major_five: false },
    ];
    const MINOR: [Progression; 5] = [
        Progression { name: "Épica (i–VI–III–VII)", degrees: &[0, 5, 2, 6], major_five: false },
        Progression { name: "Andaluza (i–VII–VI–V)", degrees: &[0, 6, 5, 4], major_five: true },
        Progression { name: "Menor (i–iv–v–i)", degrees: &[0, 3, 4, 0], major_five: false },
        Progression { name: "Dramática (i–VI–VII–i)", degrees: &[0, 5, 6, 0], major_five: false },
        Progression { name: "Lamento (i–iv–VII–III)", degrees: &[0, 3, 6, 2], major_five: false },
    ];
    if key.minor { &MINOR } else { &MAJOR }
}

/// The chord in `key` on `degree`, a seventh chord or not; with `major_five`,
/// the fifth degree is major (with a minor key's raised seventh).
pub fn on_degree(key: Key, degree: usize, sevenths: bool, major_five: bool) -> Chord {
    let chord = diatonic(key, sevenths)[degree % 7];
    if major_five && degree % 7 == 4 {
        Chord::new(chord.root, if sevenths { Quality::Seventh } else { Quality::Major })
    } else {
        chord
    }
}

/// MIDI notes to play `chord`: its bass between E1 and D#2 (40..52), and the
/// rest close together from about F2 (53), in the inversion nearest to the
/// `previous` chord's upper notes, so the voices move as little as they can.
pub fn voice(chord: &Chord, previous: &[u8]) -> Vec<u8> {
    let bass_class = chord.bass.unwrap_or(chord.root);
    let bass = 40 + (bass_class + 12 - 4) % 12;
    let tones = chord.classes();
    let stack = |inversion: usize| -> Vec<u8> {
        let mut notes = Vec::with_capacity(tones.len());
        for k in 0..tones.len() {
            let class = tones[(inversion + k) % tones.len()];
            let mut p = match notes.last() {
                None => 53 + (class + 12 - 5) % 12,
                Some(&last) => last + 1 + (class + 12 - (last + 1) % 12) % 12,
            };
            if p > 127 {
                p = 127;
            }
            notes.push(p);
        }
        notes
    };
    let candidates: Vec<Vec<u8>> = (0..tones.len()).map(stack).collect();
    let centre = |notes: &[u8]| notes.iter().map(|&n| n as f64).sum::<f64>() / notes.len().max(1) as f64;
    let distance = |notes: &[u8]| -> f64 {
        if previous.is_empty() {
            // Around middle C (C3 = 60) when nothing came before.
            return (centre(notes) - 62.0).abs();
        }
        if previous.len() == notes.len() {
            previous.iter().zip(notes).map(|(&a, &b)| (a as f64 - b as f64).abs()).sum()
        } else {
            (centre(previous) - centre(notes)).abs() * notes.len() as f64
        }
    };
    let upper = candidates.into_iter().min_by(|a, b| distance(a).total_cmp(&distance(b))).unwrap_or_default();
    std::iter::once(bass).chain(upper).collect()
}

/// How much each pitch class sounds over the time, frame by frame, in the whole
/// range and in the bass, to name the chords of a stretch of audio.
pub struct Chromagram {
    /// Seconds between frames, and of the first frame's centre.
    hop: f64,
    first: f64,
    /// Each frame's pitch classes, its bass (the lowest strong peak below 220
    /// Hz) and how loud that is.
    frames: Vec<([f64; 12], Option<(u8, f64)>)>,
}

impl Chromagram {
    pub fn new(mono: &[f32], rate: f32) -> Self {
        use rustfft::num_complex::Complex32;
        const SIZE: usize = 8192;
        const HOP: usize = 2048;
        let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(SIZE);
        let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()).collect();
        // The pitch class of each bin, for the whole range and for the bass.
        let class_in = |low: f64, high: f64| -> Vec<Option<usize>> {
            (0..SIZE / 2)
                .map(|b| {
                    let f = b as f64 * rate as f64 / SIZE as f64;
                    (low..high).contains(&f).then(|| ((69.0 + 12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12) as usize)
                })
                .collect()
        };
        let all = class_in(100.0, 2000.0);
        let bin_hz = rate as f64 / SIZE as f64;
        let (low_bin, high_bin) = ((38.0 / bin_hz) as usize, (220.0 / bin_hz) as usize);
        // Each frame on its own, the frames shared out among the cores.
        let mut frames: Vec<([f64; 12], Option<(u8, f64)>)> = vec![([0.0; 12], None); mono.len().div_ceil(HOP)];
        crate::parallel(&mut frames, 64, |first, chunk| {
            let mut buffer = vec![Complex32::new(0.0, 0.0); SIZE];
            let mut scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
            for (n, frame) in chunk.iter_mut().enumerate() {
                let start = (first + n) * HOP;
                for (i, b) in buffer.iter_mut().enumerate() {
                    *b = Complex32::new(mono.get(start + i).copied().unwrap_or(0.0) * window[i], 0.0);
                }
                fft.process_with_scratch(&mut buffer, &mut scratch);
                let mut a = [0f64; 12];
                for b in 0..SIZE / 2 {
                    if let Some(c) = all[b] {
                        a[c] += buffer[b].norm() as f64;
                    }
                }
                let m = |b: usize| buffer[b].norm() as f64;
                let loudest = (low_bin..=high_bin).map(m).fold(0.0, f64::max);
                let bass = (low_bin.max(1)..high_bin).find(|&b| m(b) >= 0.5 * loudest && m(b) >= m(b - 1) && m(b) >= m(b + 1)).filter(|_| loudest > 0.0).map(|b| {
                    let f = b as f64 * bin_hz;
                    ((((69.0 + 12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12)) as u8, m(b))
                });
                *frame = (a, bass);
            }
        });
        // Each class's median over about 0.4 s: what holds (a chord) stays, what
        // only hits (a kick, a snare) goes.
        const SPAN: usize = 4;
        let held: Vec<[f64; 12]> = (0..frames.len())
            .map(|i| {
                let around = &frames[i.saturating_sub(SPAN)..(i + SPAN + 1).min(frames.len())];
                std::array::from_fn(|c| {
                    let mut values: Vec<f64> = around.iter().map(|f| f.0[c]).collect();
                    values.sort_by(|a, b| a.total_cmp(b));
                    values[values.len() / 2]
                })
            })
            .collect();
        for (frame, h) in frames.iter_mut().zip(held) {
            frame.0 = h;
        }
        Self { hop: HOP as f64 / rate as f64, first: SIZE as f64 / 2.0 / rate as f64, frames }
    }

    /// The pitch classes over `from..to` seconds, and the bass's, if it is
    /// the same most of the time. Only the frames wholly inside count (the
    /// chord before rings on into the start), or, in a short stretch, those
    /// centred in it.
    pub fn between(&self, from: f64, to: f64) -> ([f64; 12], Option<u8>) {
        let centre = |i: usize| self.first + i as f64 * self.hop;
        let half = self.first;
        let inside = |i: usize| centre(i) - half >= from - 1e-9 && centre(i) + half <= to + 1e-9;
        let any_inside = (0..self.frames.len()).any(inside);
        let (mut all, mut low) = ([0f64; 12], [0f64; 12]);
        for (i, (a, bass)) in self.frames.iter().enumerate() {
            let counts = if any_inside { inside(i) } else { (from..to).contains(&centre(i)) };
            if counts {
                for c in 0..12 {
                    all[c] += a[c];
                }
                if let Some((c, loud)) = bass {
                    low[*c as usize] += loud;
                }
            }
        }
        let total: f64 = low.iter().sum();
        let (class, most) = low.iter().enumerate().fold((0, 0.0), |m, (c, &x)| if x > m.1 { (c, x) } else { m });
        (all, (total > 0.0 && most / total > 0.4).then_some(class as u8))
    }

    /// The chords of consecutive stretches (seconds), equal ones run together:
    /// for each, where it starts and ends, and the chord (None where none sounds).
    pub fn chords(&self, stretches: &[(f64, f64)], key: Option<Key>) -> Vec<(f64, f64, Option<Chord>)> {
        let mut out: Vec<(f64, f64, Option<Chord>)> = Vec::new();
        for &(from, to) in stretches {
            let (weights, bass) = self.between(from, to);
            let chord = Chord::detect(&weights, bass, key);
            match out.last_mut() {
                Some(last) if last.2 == chord && (last.1 - from).abs() < 1e-6 => last.1 = to,
                _ => out.push((from, to, chord)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(classes: &[u8]) -> [f64; 12] {
        let mut w = [0.0; 12];
        for &c in classes {
            w[c as usize % 12] += 1.0;
        }
        w
    }

    fn name(classes: &[u8], bass: Option<u8>) -> Option<String> {
        Chord::detect(&weights(classes), bass, None).map(|c| c.name())
    }

    #[test]
    fn chords_are_named_from_their_notes() {
        assert_eq!(name(&[0, 4, 7], Some(0)).as_deref(), Some("Do"));
        assert_eq!(name(&[9, 0, 4], Some(9)).as_deref(), Some("Lam"));
        assert_eq!(name(&[7, 11, 2, 5], Some(7)).as_deref(), Some("Sol7"));
        assert_eq!(name(&[9, 0, 4, 7], Some(9)).as_deref(), Some("Lam7"));
        assert_eq!(name(&[0, 4, 7, 11], None).as_deref(), Some("Domaj7"));
        assert_eq!(name(&[11, 2, 5], Some(11)).as_deref(), Some("Si°"));
        assert_eq!(name(&[2, 7, 9], Some(2)).as_deref(), Some("Resus4"));
        // An inversion: C major over E.
        assert_eq!(name(&[0, 4, 7], Some(4)).as_deref(), Some("Do/Mi"));
        // A missing fifth is still the chord.
        assert_eq!(name(&[2, 5], Some(2)).as_deref(), Some("Rem"));
        // One note is no chord.
        assert_eq!(name(&[0, 0, 0], Some(0)), None);
        assert_eq!(Chord::detect(&[0.0; 12], None, None), None);
    }

    #[test]
    fn a_melody_note_over_a_chord_does_not_change_it() {
        // A minor held, with a passing D in the melody.
        let mut w = weights(&[9, 0, 4, 9, 0, 4]);
        w[2] += 0.5;
        assert_eq!(Chord::detect(&w, Some(9), None).map(|c| c.name()).as_deref(), Some("Lam"));
    }

    #[test]
    fn the_keys_chords_and_their_numerals() {
        let c = Key { tonic: 0, minor: false };
        let names: Vec<String> = diatonic(c, false).iter().map(|ch| ch.name()).collect();
        assert_eq!(names, ["Do", "Rem", "Mim", "Fa", "Sol", "Lam", "Si°"]);
        let numerals: Vec<String> = diatonic(c, false).iter().map(|ch| ch.numeral(c).unwrap()).collect();
        assert_eq!(numerals, ["I", "ii", "iii", "IV", "V", "vi", "vii°"]);
        let sevenths: Vec<String> = diatonic(c, true).iter().map(|ch| ch.name()).collect();
        assert_eq!(sevenths, ["Domaj7", "Rem7", "Mim7", "Famaj7", "Sol7", "Lam7", "Siø"]);
        let a = Key { tonic: 9, minor: true };
        let names: Vec<String> = diatonic(a, false).iter().map(|ch| ch.name()).collect();
        assert_eq!(names, ["Lam", "Si°", "Do", "Rem", "Mim", "Fa", "Sol"]);
        assert_eq!(on_degree(a, 4, false, true).name(), "Mi");
        assert_eq!(Chord::new(4, Quality::Major).numeral(a).as_deref(), Some("V"));
        assert_eq!(Chord::new(1, Quality::Major).numeral(c), None);
    }

    #[test]
    fn voices_move_as_little_as_they_can() {
        let c = voice(&Chord::new(0, Quality::Major), &[]);
        assert_eq!(c[0], 48, "the bass: C2");
        let upper = &c[1..];
        assert!(upper.windows(2).all(|w| w[0] < w[1] && w[1] - w[0] <= 5), "close together: {c:?}");
        // C to F: the nearest F major is C F A (C stays, E→F, G→A).
        let f = voice(&Chord::new(5, Quality::Major), upper);
        assert_eq!(f[0], 41);
        let moved: i32 = upper.iter().zip(&f[1..]).map(|(&a, &b)| (a as i32 - b as i32).abs()).sum();
        assert!(moved <= 3, "{upper:?} → {:?}", &f[1..]);
        // A slash chord has its bass: C/E.
        let c_e = voice(&Chord { bass: Some(4), ..Chord::new(0, Quality::Major) }, &[]);
        assert_eq!(c_e[0] % 12, 4);
    }

    /// Prints the chords of `PULSE_CHORD_IN` (a WAV), a second at a time.
    #[test]
    #[ignore]
    fn dump_chords() {
        let Ok(path) = std::env::var("PULSE_CHORD_IN") else { return };
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => reader.samples::<i32>().map(|s| s.unwrap() as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32).collect(),
        };
        let channels = spec.channels as usize;
        let mono: Vec<f32> = samples.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
        let rate = spec.sample_rate as f32;
        let key = Key::detect(&crate::key::chroma(&mono, rate));
        let gram = Chromagram::new(&mono, rate);
        let seconds = mono.len() as f64 / rate as f64;
        let stretches: Vec<(f64, f64)> = (0..seconds.ceil() as usize).map(|i| (i as f64, (i as f64 + 1.0).min(seconds))).collect();
        println!("key {:?}", key.map(|k| k.name()));
        for (a, b, c) in gram.chords(&stretches, key) {
            println!("{a:5.1}–{b:5.1}  {}", c.map_or("—".into(), |c| c.name()));
        }
        for &(a, b) in &stretches {
            let (w, bass) = gram.between(a, b);
            let total: f64 = w.iter().sum::<f64>().max(1e-12);
            let shares: Vec<String> = w.iter().map(|x| format!("{:3.0}", 100.0 * x / total)).collect();
            println!("{a:4.1} bass {bass:?} {}", shares.join(" "));
        }
    }

    /// Am, F, C, G, two seconds each, played with harmonics like a pad and a bass.
    #[test]
    fn chords_come_out_of_audio() {
        let rate = 22_050.0;
        let progression: [&[u8]; 4] = [&[45, 57, 60, 64], &[41, 57, 60, 65], &[48, 55, 60, 64], &[43, 55, 59, 62]];
        let mut audio = Vec::new();
        for chord in progression {
            for i in 0..(rate * 2.0) as usize {
                let t = i as f32 / rate;
                let v: f32 = chord
                    .iter()
                    .map(|&p| {
                        let f = 440.0 * 2f32.powf((p as f32 - 69.0) / 12.0);
                        (1..5).map(|h| (2.0 * std::f32::consts::PI * f * h as f32 * t).sin() / (h * h) as f32).sum::<f32>()
                    })
                    .sum();
                audio.push(v * 0.1);
            }
        }
        let gram = Chromagram::new(&audio, rate);
        let stretches: Vec<(f64, f64)> = (0..8).map(|i| (i as f64, i as f64 + 1.0)).collect();
        let found: Vec<(f64, Option<String>)> = gram.chords(&stretches, None).iter().map(|(a, _, c)| (*a, c.map(|c| c.name()))).collect();
        let expected = [(0.0, "Lam"), (2.0, "Fa"), (4.0, "Do"), (6.0, "Sol")].map(|(t, n)| (t, Some(n.to_string())));
        assert_eq!(found, expected);
        // The frames on every core are the frames one after another, as before.
        let mut noisy = audio.repeat(8);
        let mut seed = 5u32;
        for x in &mut noisy {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *x += 0.05 * ((seed >> 9) as f32 / (1u32 << 23) as f32 - 1.0);
        }
        assert!(Chromagram::new(&noisy, rate).frames == crate::reference::chroma_frames(&noisy, rate));
    }
}
