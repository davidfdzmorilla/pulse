//! Small DSP building blocks shared by the engine and the app.

/// In-place radix-2 FFT of `re`/`im` (length a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (w_re, w_im) = ((angle * k as f32).cos(), (angle * k as f32).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
            }
        }
        len <<= 1;
    }
}

/// In-place inverse FFT (scaled by 1/n), via the conjugate trick.
pub fn ifft(re: &mut [f32], im: &mut [f32]) {
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft(re, im);
    let scale = 1.0 / re.len() as f32;
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        *r *= scale;
        *i = -*i * scale;
    }
}

/// The true-peak detector's interpolator: taps and phases (4× oversampling, the
/// same filter the loudness meter measures true peaks with).
pub(crate) const TP_TAPS: usize = 12;
pub(crate) const TP_PHASES: usize = 4;
/// How far behind the newest sample the interpolator looks (samples).
pub(crate) const TP_DELAY: usize = TP_TAPS / 2 - 1;

/// The interpolator's coefficients for the points between samples (phases 1 to 3).
pub(crate) fn true_peak_taps() -> [[f32; TP_TAPS]; TP_PHASES - 1] {
    std::array::from_fn(|p| {
        let phase = p + 1;
        std::array::from_fn(|k| {
            let t = (k as f64 - (TP_TAPS / 2) as f64 + 1.0) - phase as f64 / TP_PHASES as f64;
            let sinc = if t.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
            let window = 0.5 + 0.5 * (std::f64::consts::PI * t / (TP_TAPS as f64 / 2.0 + 1.0)).cos();
            (sinc * window) as f32
        })
    })
}

/// A true-peak meter (4× oversampled, as BS.1770 asks): the highest the
/// sound goes between its samples too.
pub struct TruePeakMeter {
    history: [[f32; 2 * TP_TAPS]; 2],
    at: usize,
    taps: [[f32; TP_TAPS]; TP_PHASES - 1],
    peak: f32,
}

impl Default for TruePeakMeter {
    fn default() -> Self {
        Self { history: [[0.0; 2 * TP_TAPS]; 2], at: 0, taps: true_peak_taps(), peak: 0.0 }
    }
}

impl TruePeakMeter {
    pub fn push(&mut self, left: f32, right: f32) {
        self.at = (self.at + 1) % TP_TAPS;
        for (channel, v) in [left, right].into_iter().enumerate() {
            let h = &mut self.history[channel];
            h[self.at] = v;
            h[self.at + TP_TAPS] = v;
            let recent = &h[self.at + 1..=self.at + TP_TAPS];
            let mut peak = v.abs();
            for c in &self.taps {
                let mut s = 0f32;
                for k in 0..TP_TAPS {
                    s += c[k] * recent[TP_TAPS - 1 - k];
                }
                peak = peak.max(s.abs());
            }
            self.peak = self.peak.max(peak);
        }
    }

    /// The highest so far (linear).
    pub fn peak(&self) -> f32 {
        self.peak
    }
}

/// How many 100 ms blocks the short-term loudness spans (3 s).
const SHORT_TERM_BLOCKS: usize = 30;

/// The short-term loudness of what goes by (ITU-R BS.1770 / EBU R128: the
/// K-weighted power of both channels over the last 3 s, in LUFS), worked out
/// a sample at a time without allocating: for the master's meter.
pub struct LoudnessMeter {
    /// The K-weighting (a high shelf, then a high pass): b0, b1, b2, a1, a2.
    stages: [[f64; 5]; 2],
    /// Each channel's state in each stage: x1, x2, y1, y2.
    state: [[[f64; 4]; 2]; 2],
    block: usize,
    filled: usize,
    sum: f64,
    blocks: [f64; SHORT_TERM_BLOCKS],
    at: usize,
}

impl LoudnessMeter {
    pub fn new(rate: f32) -> Self {
        let rate = rate as f64;
        // As libebur128 works them out for any rate (at 48 kHz, the standard's).
        let shelf = {
            let (f0, gain, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
            let k = (std::f64::consts::PI * f0 / rate).tan();
            let vh = 10f64.powf(gain / 20.0);
            let vb = vh.powf(0.499_666_774_154_541_6);
            let a0 = 1.0 + k / q + k * k;
            [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]
        };
        let high_pass = {
            let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
            let k = (std::f64::consts::PI * f0 / rate).tan();
            let a0 = 1.0 + k / q + k * k;
            [1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]
        };
        Self { stages: [shelf, high_pass], state: [[[0.0; 4]; 2]; 2], block: ((rate / 10.0) as usize).max(1), filled: 0, sum: 0.0, blocks: [0.0; SHORT_TERM_BLOCKS], at: 0 }
    }

    /// One frame. Every 100 ms, the loudness of the last 3 s (LUFS; minus
    /// infinity in silence).
    pub fn push(&mut self, left: f32, right: f32) -> Option<f32> {
        self.push_block(left, right)?;
        let power = self.blocks.iter().sum::<f64>() / SHORT_TERM_BLOCKS as f64;
        Some(if power > 1e-12 { (-0.691 + 10.0 * power.log10()) as f32 } else { f32::NEG_INFINITY })
    }

    /// One frame. Every 100 ms, that block's K-weighted power (both channels
    /// summed): what integrated loudness, short-term loudness and loudness
    /// range are worked out from.
    pub fn push_block(&mut self, left: f32, right: f32) -> Option<f64> {
        for (channel, x) in [left, right].into_iter().enumerate() {
            let mut v = x as f64;
            for (stage, c) in self.stages.iter().enumerate() {
                let s = &mut self.state[channel][stage];
                let y = c[0] * v + c[1] * s[0] + c[2] * s[1] - c[3] * s[2] - c[4] * s[3];
                *s = [v, s[0], y, s[2]];
                v = y;
            }
            self.sum += v * v;
        }
        self.filled += 1;
        if self.filled < self.block {
            return None;
        }
        let power = self.sum / self.block as f64;
        self.blocks[self.at] = power;
        self.at = (self.at + 1) % SHORT_TERM_BLOCKS;
        self.filled = 0;
        self.sum = 0.0;
        Some(power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_fft_restores_the_signal() {
        let signal: Vec<f32> = (0..64).map(|i| ((i * 7) % 13) as f32 - 6.0).collect();
        let (mut re, mut im) = (signal.clone(), vec![0.0; 64]);
        fft(&mut re, &mut im);
        ifft(&mut re, &mut im);
        for (a, b) in re.iter().zip(&signal) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    /// The master's meter reads what the loudness meter of the assistant
    /// reads: a 1 kHz sine at -20 dBFS in both channels is -20 LUFS
    /// (BS.1770's own check, give or take its 0.01 dB), at 44.1 and 48 kHz.
    #[test]
    fn short_term_loudness_of_a_sine() {
        for rate in [44_100.0f32, 48_000.0] {
            let mut meter = super::LoudnessMeter::new(rate);
            let amplitude = 10f32.powf(-20.0 / 20.0);
            let mut last = None;
            for i in 0..(4.0 * rate) as usize {
                let x = amplitude * (std::f32::consts::TAU * 997.0 * i as f32 / rate).sin();
                if let Some(l) = meter.push(x, x) {
                    last = Some(l);
                }
            }
            let l = last.unwrap();
            println!("{rate} Hz: {l:.3} LUFS");
            assert!((l - -20.0).abs() < 0.1, "{l}");
        }
        let mut quiet = super::LoudnessMeter::new(48_000.0);
        assert!((0..4800).filter_map(|_| quiet.push(0.0, 0.0)).all(|l| l == f32::NEG_INFINITY));
    }
}
