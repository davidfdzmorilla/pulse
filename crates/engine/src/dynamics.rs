//! Bus and multiband dynamics: the «Compresor de bus» (a feed-forward VCA
//! glue compressor, SSL bus style) and the «Multibanda (OTT)» (three bands of
//! upward and downward compression). Constructors allocate; `process` never
//! does, and every setting moves per sample, so the sound doesn't depend on
//! how the blocks are cut.

use crate::effects::Key;
use crate::model::{db_to_gain, GlueParams, MultibandParams, MULTIBAND_BANDS};
use std::f32::consts::{PI, SQRT_2};

/// A setting that glides to where it is set (one pole, per sample): no
/// zipper noise, the same whatever the block size.
#[derive(Clone, Copy)]
struct Smooth {
    value: f32,
    coef: f32,
}

impl Smooth {
    /// Settles in about `ms` (time constant).
    fn new(value: f32, ms: f32, sr: f32) -> Self {
        Self { value, coef: (-1.0 / (ms * 0.001 * sr)).exp() }
    }

    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        if self.value != target {
            self.value = target + self.coef * (self.value - target);
            if (self.value - target).abs() <= 1e-6 * target.abs().max(1.0) {
                self.value = target;
            }
        }
        self.value
    }
}

/// Topology-preserving state variable filter (Butterworth at k = √2).
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy)]
struct SvfCoefs {
    a1: f32,
    a2: f32,
    a3: f32,
}

const K: f32 = SQRT_2;

impl SvfCoefs {
    fn new(cutoff: f32, sr: f32) -> Self {
        let g = (PI * cutoff.clamp(10.0, sr * 0.45) / sr).tan();
        let a1 = 1.0 / (1.0 + g * (g + K));
        Self { a1, a2: g * a1, a3: g * g * a1 }
    }
}

impl Svf {
    /// (low, band, high).
    #[inline]
    fn tick(&mut self, x: f32, c: &SvfCoefs) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - K * v1 - v2)
    }
}

/// `amount` over a threshold through a soft knee `width` dB wide: 0 well
/// under it, the overshoot well over it, a parabola between.
#[inline]
fn knee(over: f32, width: f32) -> f32 {
    if width < 0.01 {
        over.max(0.0)
    } else if 2.0 * over <= -width {
        0.0
    } else if 2.0 * over < width {
        (over + 0.5 * width).powi(2) / (2.0 * width)
    } else {
        over
    }
}

/// dB to gain, without `db_to_gain`'s cut to silence under −70 dB.
#[inline]
fn gain(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

#[inline]
fn coef(seconds: f32, sr: f32) -> f32 {
    (-1.0 / (seconds.max(1e-6) * sr)).exp()
}

// ----- Bus compressor ------------------------------------------------------------

/// Where the soft clipper starts bending, and the most it lets out (−0.5 dBFS).
const CLIP_KNEE: f32 = 0.7;
const CLIP_CEILING: f32 = 0.944;

#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        x
    } else {
        let span = CLIP_CEILING - CLIP_KNEE;
        (CLIP_KNEE + span * ((a - CLIP_KNEE) / span).tanh()).copysign(x)
    }
}

/// The auto release: a fast stage for short hits, a slow one that only
/// charges up over long stretches of compression (seconds).
const AUTO_FAST_S: f32 = 0.1;
const AUTO_SLOW_S: f32 = 1.2;
const AUTO_CHARGE_S: f32 = 0.5;

pub struct Glue {
    pub(crate) params: GlueParams,
    pub(crate) key: Key,
    sr: f32,
    hpf: [Svf; 2],
    hpf_coefs: SvfCoefs,
    hpf_hz: f32,
    /// Gain reduction (dB, positive): the attack/release stage and the slow
    /// one of the auto release.
    fast: f32,
    slow: f32,
    threshold: Smooth,
    knee: Smooth,
    makeup: Smooth,
    mix: Smooth,
    /// The most it reduced in the last block (dB), for the meter.
    meter: f32,
}

impl Glue {
    pub(crate) fn new(params: GlueParams, sr: f32) -> Self {
        Self {
            params,
            key: Key::new(),
            sr,
            hpf: Default::default(),
            hpf_coefs: SvfCoefs::new(params.sidechain_hpf_hz, sr),
            hpf_hz: params.sidechain_hpf_hz,
            fast: 0.0,
            slow: 0.0,
            threshold: Smooth::new(params.threshold_db, 20.0, sr),
            knee: Smooth::new(params.knee_db, 20.0, sr),
            makeup: Smooth::new(db_to_gain(params.makeup_db), 20.0, sr),
            mix: Smooth::new(params.mix, 20.0, sr),
            meter: 0.0,
        }
    }

    pub(crate) fn reset(&mut self) {
        let p = self.params;
        self.hpf = Default::default();
        self.fast = 0.0;
        self.slow = 0.0;
        self.threshold.value = p.threshold_db;
        self.knee.value = p.knee_db;
        self.makeup.value = db_to_gain(p.makeup_db);
        self.mix.value = p.mix;
        self.meter = 0.0;
    }

    /// Gain reduction (dB, positive) the static curve asks for at `level_db`.
    pub(crate) fn curve(level_db: f32, threshold_db: f32, ratio: f32, knee_db: f32) -> f32 {
        (1.0 - 1.0 / ratio) * knee(level_db - threshold_db, knee_db)
    }

    /// The most it reduced in the last block, dB.
    pub(crate) fn reduction(&self) -> f32 {
        self.meter
    }

    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let sr = self.sr;
        let ratio = p.ratio.value();
        let attack = coef(p.attack_ms * 0.001, sr);
        let (release, slow_release, charge) = if p.auto_release {
            (coef(AUTO_FAST_S, sr), coef(AUTO_SLOW_S, sr), coef(AUTO_CHARGE_S, sr))
        } else {
            (coef(p.release_s, sr), 0.0, 0.0)
        };
        if p.sidechain_hpf_hz != self.hpf_hz {
            self.hpf_hz = p.sidechain_hpf_hz;
            self.hpf_coefs = SvfCoefs::new(p.sidechain_hpf_hz, sr);
        }
        let filtered = p.sidechain_hpf_hz > GlueParams::HPF_OFF_HZ;
        let makeup_target = db_to_gain(p.makeup_db);
        let keyed = self.key.take(l.len());
        let mut most = 0f32;
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let (x, y) = (*a, *b);
            // Stereo-linked peak, of the sidechain's sound if it has one,
            // without its lows if the filter is on.
            let peak = match (keyed, filtered) {
                (true, false) => self.key.level[i],
                (true, true) => self.hpf[0].tick(self.key.mono[i], &self.hpf_coefs).2.abs(),
                (false, false) => x.abs().max(y.abs()),
                (false, true) => {
                    let hl = self.hpf[0].tick(x, &self.hpf_coefs).2;
                    let hr = self.hpf[1].tick(y, &self.hpf_coefs).2;
                    hl.abs().max(hr.abs())
                }
            };
            let level_db = 20.0 * peak.max(1e-6).log10();
            let threshold = self.threshold.next(p.threshold_db);
            let knee_db = self.knee.next(p.knee_db);
            let target = Self::curve(level_db, threshold, ratio, knee_db);
            let c = if target > self.fast { attack } else { release };
            // Tiny reductions would decay into denormals.
            self.fast = (target + c * (self.fast - target)).max(0.0);
            if self.fast < 1e-6 {
                self.fast = 0.0;
            }
            let reduction = if p.auto_release {
                let c = if target > self.slow { charge } else { slow_release };
                self.slow = target + c * (self.slow - target);
                if self.slow < 1e-6 {
                    self.slow = 0.0;
                }
                self.fast.max(self.slow)
            } else {
                self.fast
            };
            most = most.max(reduction);
            let g = gain(-reduction) * self.makeup.next(makeup_target);
            let wet = self.mix.next(p.mix);
            let dry = 1.0 - wet;
            let (mut u, mut v) = (x * (g * wet + dry), y * (g * wet + dry));
            if p.soft_clip {
                u = soft_clip(u);
                v = soft_clip(v);
            }
            *a = u;
            *b = v;
        }
        self.meter = most;
    }
}

// ----- Multiband (OTT) -----------------------------------------------------------

/// Each band's thresholds (dBFS RMS), the ratios, the make-up and times:
/// those of the OTT preset of a famous multiband compressor (low, mid, high).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Curve {
    pub down_threshold: [f32; MULTIBAND_BANDS],
    pub up_threshold: [f32; MULTIBAND_BANDS],
    pub down_ratio: f32,
    pub up_ratio: f32,
    pub makeup_db: [f32; MULTIBAND_BANDS],
    pub attack_ms: [f32; MULTIBAND_BANDS],
    pub release_ms: [f32; MULTIBAND_BANDS],
}

/// Tuned on the engine's drum loops and pads: the loud parts held almost
/// flat (66:1), the quiet ones pulled up (4:1 upward), the make-up bringing
/// a full mix out about as loud as it went in, attacks fast enough to catch
/// hits (the drums lose crest instead of gaining it) and slower releases
/// that let tails come up.
pub(crate) const OTT: Curve = Curve {
    down_threshold: [-30.8, -27.0, -32.5],
    up_threshold: [-40.8, -41.8, -40.8],
    down_ratio: 66.7,
    up_ratio: 4.17,
    makeup_db: [20.3, 15.7, 20.3],
    attack_ms: [2.4, 1.1, 0.7],
    release_ms: [282.0, 282.0, 132.0],
};
/// The knee of both curves, dB.
const OTT_KNEE: f32 = 6.0;
/// The most a band is pulled up, dB, and the level (dBFS) under which it
/// stops pulling up (fading out over 20 dB): silence stays silence.
const MAX_UP_DB: f32 = 18.0;
const UP_FLOOR_DB: f32 = -90.0;

/// Gain change (dB) of band `b` at RMS level `level_db`: downward and upward
/// compression, each at its amount.
#[inline]
pub(crate) fn ott_gain_db(c: &Curve, band: usize, level_db: f32, downward: f32, upward: f32) -> f32 {
    let down = (1.0 / c.down_ratio - 1.0) * knee(level_db - c.down_threshold[band], OTT_KNEE);
    let floor = ((level_db - UP_FLOOR_DB) / 20.0).clamp(0.0, 1.0);
    let up = ((1.0 - 1.0 / c.up_ratio) * knee(c.up_threshold[band] - level_db, OTT_KNEE)).min(MAX_UP_DB) * floor;
    downward * down + upward * up
}

/// The three-band split of one channel: Linkwitz-Riley 24 dB/oct crossovers,
/// with the low band through the high crossover's allpass, so the bands add
/// up to an allpass (flat).
#[derive(Clone, Copy, Default)]
struct Split {
    first: Svf,
    low: Svf,
    high: Svf,
    allpass: Svf,
    second: Svf,
    mid: Svf,
    top: Svf,
}

impl Split {
    #[inline]
    fn tick(&mut self, x: f32, c1: &SvfCoefs, c2: &SvfCoefs) -> [f32; MULTIBAND_BANDS] {
        let (l1, _, h1) = self.first.tick(x, c1);
        let low = self.low.tick(l1, c1).0;
        let rest = self.high.tick(h1, c1).2;
        let low = low - 2.0 * K * self.allpass.tick(low, c2).1;
        let (l2, _, h2) = self.second.tick(rest, c2);
        [low, self.mid.tick(l2, c2).0, self.top.tick(h2, c2).2]
    }
}

pub struct Multiband {
    pub(crate) params: MultibandParams,
    pub(crate) curve: Curve,
    sr: f32,
    split: [Split; 2],
    low_hz: Smooth,
    high_hz: Smooth,
    coefs: (SvfCoefs, SvfCoefs),
    /// The frequencies `coefs` are for.
    at: (f32, f32),
    /// Each band's mean square (its RMS detector).
    power: [f32; MULTIBAND_BANDS],
    depth: Smooth,
    downward: Smooth,
    upward: Smooth,
    input: Smooth,
    output: Smooth,
    mix: Smooth,
    band_in: [Smooth; MULTIBAND_BANDS],
    band_out: [Smooth; MULTIBAND_BANDS],
    /// Each band's compression (dB, up positive) at the end of the last block.
    meter: [f32; MULTIBAND_BANDS],
}

impl Multiband {
    pub(crate) fn new(params: MultibandParams, sr: f32) -> Self {
        let s = |v: f32| Smooth::new(v, 20.0, sr);
        let p = params;
        Self {
            params,
            curve: OTT,
            sr,
            split: Default::default(),
            low_hz: s(p.low_hz),
            high_hz: s(p.high_hz),
            coefs: (SvfCoefs::new(p.low_hz, sr), SvfCoefs::new(p.high_hz, sr)),
            at: (p.low_hz, p.high_hz),
            power: [0.0; MULTIBAND_BANDS],
            depth: s(p.depth),
            downward: s(p.downward),
            upward: s(p.upward),
            input: s(db_to_gain(p.input_db)),
            output: s(db_to_gain(p.output_db)),
            mix: s(p.mix),
            band_in: p.band_in_db.map(|db| s(db_to_gain(db))),
            band_out: p.band_out_db.map(s),
            meter: [0.0; MULTIBAND_BANDS],
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self { curve: self.curve, ..Self::new(self.params, self.sr) };
    }

    /// Each band's compression in the last block, dB (up positive).
    pub(crate) fn reduction(&self) -> [f32; MULTIBAND_BANDS] {
        self.meter
    }

    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let sr = self.sr;
        let time = p.time.max(0.01);
        // The detector follows the power (an RMS one): half the time
        // constant of the level's.
        let curve = self.curve;
        let attack = curve.attack_ms.map(|ms| coef(ms * 0.0005 * time, sr));
        let release = curve.release_ms.map(|ms| coef(ms * 0.0005 * time, sr));
        let input_target = db_to_gain(p.input_db);
        let output_target = db_to_gain(p.output_db);
        let band_in_target = p.band_in_db.map(db_to_gain);
        // The low crossover stays under the high one.
        let high_target = p.high_hz;
        let low_target = p.low_hz.min(p.high_hz * 0.5);
        let mut comp = [0f32; MULTIBAND_BANDS];
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (f1, f2) = (self.low_hz.next(low_target), self.high_hz.next(high_target));
            if (f1, f2) != self.at {
                self.at = (f1, f2);
                self.coefs = (SvfCoefs::new(f1, sr), SvfCoefs::new(f2, sr));
            }
            let gain_in = self.input.next(input_target);
            let (x, y) = (*a * gain_in, *b * gain_in);
            let bands_l = self.split[0].tick(x, &self.coefs.0, &self.coefs.1);
            let bands_r = self.split[1].tick(y, &self.coefs.0, &self.coefs.1);
            let depth = self.depth.next(p.depth);
            let downward = self.downward.next(p.downward);
            let upward = self.upward.next(p.upward);
            let (mut wet_l, mut wet_r, mut dry_l, mut dry_r) = (0f32, 0f32, 0f32, 0f32);
            for band in 0..MULTIBAND_BANDS {
                let into = self.band_in[band].next(band_in_target[band]);
                let (u, v) = (bands_l[band] * into, bands_r[band] * into);
                let square = 0.5 * (u * u + v * v);
                let c = if square > self.power[band] { attack[band] } else { release[band] };
                self.power[band] = square + c * (self.power[band] - square);
                if self.power[band] < 1e-20 {
                    self.power[band] = 0.0;
                }
                let level_db = 10.0 * (self.power[band] + 1e-20).log10();
                comp[band] = depth * ott_gain_db(&curve, band, level_db, downward, upward);
                let out_db = self.band_out[band].next(p.band_out_db[band]);
                let g = gain(comp[band] + depth * curve.makeup_db[band] + out_db);
                wet_l += u * g;
                wet_r += v * g;
                // The bands as they are add up to the input through an allpass:
                // the dry sound in phase with the wet one.
                dry_l += bands_l[band];
                dry_r += bands_r[band];
            }
            let mix = self.mix.next(p.mix);
            let gain_out = self.output.next(output_target);
            *a = (dry_l + mix * (wet_l - dry_l)) * gain_out;
            *b = (dry_r + mix * (wet_r - dry_r)) * gain_out;
        }
        self.meter = comp;
    }
}

#[cfg(test)]
mod tests;
