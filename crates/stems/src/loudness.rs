//! Loudness and the measures a mix is judged by: integrated loudness and
//! loudness range (ITU-R BS.1770-4 / EBU R128, in LUFS and LU), true peak
//! (4× oversampled, in dBTP), the stereo correlation, the balance over octave
//! bands, and how much of the time a track plays in the low end.

use rustfft::num_complex::Complex32;

/// A biquad filter (direct form I).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    fn run(&self, input: &[f32]) -> Vec<f64> {
        self.run_f64(&input.iter().map(|&x| x as f64).collect::<Vec<_>>())
    }

    fn run_f64(&self, input: &[f64]) -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        input
            .iter()
            .map(|&x| {
                let y = self.b[0] * x + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect()
    }
}

/// The K-weighting of BS.1770 at `rate`: a high shelf (+4 dB above ~1.7 kHz,
/// the head) and a high pass (~38 Hz), worked out for any rate as libebur128
/// does (at 48 kHz, the standard's own coefficients).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    let shelf = {
        let (f0, gain, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let vh = 10f64.powf(gain / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        }
    };
    let high_pass = {
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] }
    };
    [shelf, high_pass]
}

/// The K-weighted power of both channels, summed, block by block: blocks of
/// `block` seconds every `hop` seconds.
fn block_powers(left: &[f32], right: &[f32], rate: f32, block: f64, hop: f64) -> Vec<f64> {
    let [shelf, high_pass] = k_weighting(rate as f64);
    let weigh = |x: &[f32]| -> Vec<f64> {
        high_pass.run_f64(&shelf.run(x))
    };
    let (l, r) = (weigh(left), weigh(right));
    // Running sums of the squares, for each block's mean in one step.
    let mut sum = vec![0.0; l.len() + 1];
    for i in 0..l.len() {
        sum[i + 1] = sum[i] + l[i] * l[i] + r.get(i).map_or(0.0, |x| x * x);
    }
    let (size, step) = ((block * rate as f64) as usize, ((hop * rate as f64) as usize).max(1));
    if l.len() < size || size == 0 {
        return Vec::new();
    }
    (0..=(l.len() - size) / step).map(|j| (sum[j * step + size] - sum[j * step]) / size as f64).collect()
}

fn lufs(power: f64) -> f64 {
    -0.691 + 10.0 * power.max(1e-20).log10()
}

/// Integrated loudness in LUFS (gated: below -70 LUFS and 10 LU under the
/// rest don't count), or None for silence or under 0.4 s.
pub fn integrated(left: &[f32], right: &[f32], rate: f32) -> Option<f64> {
    let blocks = block_powers(left, right, rate, 0.4, 0.1);
    let loud: Vec<f64> = blocks.into_iter().filter(|&p| lufs(p) > -70.0).collect();
    if loud.is_empty() {
        return None;
    }
    let relative = lufs(loud.iter().sum::<f64>() / loud.len() as f64) - 10.0;
    let gated: Vec<f64> = loud.into_iter().filter(|&p| lufs(p) > relative).collect();
    Some(lufs(gated.iter().sum::<f64>() / gated.len().max(1) as f64))
}

/// The short-term loudness (3 s windows, every 0.1 s), in LUFS.
fn short_term(left: &[f32], right: &[f32], rate: f32) -> Vec<f64> {
    block_powers(left, right, rate, 3.0, 0.1).into_iter().map(lufs).collect()
}

/// The loudest 3 seconds, in LUFS.
pub fn short_term_max(left: &[f32], right: &[f32], rate: f32) -> Option<f64> {
    short_term(left, right, rate).into_iter().filter(|l| l.is_finite()).reduce(f64::max)
}

/// Loudness range (EBU Tech 3342), in LU: how far apart the quiet and loud
/// parts are (the 10th to the 95th percentile of the short-term loudness).
pub fn range(left: &[f32], right: &[f32], rate: f32) -> Option<f64> {
    let loud: Vec<f64> = short_term(left, right, rate).into_iter().filter(|&l| l > -70.0).collect();
    if loud.is_empty() {
        return None;
    }
    let mean = loud.iter().map(|&l| 10f64.powf((l + 0.691) / 10.0)).sum::<f64>() / loud.len() as f64;
    let gate = lufs(mean) - 20.0;
    let mut kept: Vec<f64> = loud.into_iter().filter(|&l| l > gate).collect();
    kept.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f64| kept[((p * (kept.len() - 1) as f64).round() as usize).min(kept.len() - 1)];
    Some(at(0.95) - at(0.10))
}

/// The true peak, in dBTP: the highest the signal goes between its samples
/// too, found by oversampling 4× (a windowed-sinc interpolator, 12 taps a phase).
pub fn true_peak(left: &[f32], right: &[f32]) -> f64 {
    const PHASES: usize = 4;
    const TAPS: usize = 12;
    // The interpolator's coefficients, phase by phase.
    let coefficients: Vec<[f64; TAPS]> = (0..PHASES)
        .map(|phase| {
            std::array::from_fn(|k| {
                let t = (k as f64 - (TAPS / 2) as f64 + 1.0) - phase as f64 / PHASES as f64;
                let sinc = if t.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
                let window = 0.5 + 0.5 * (std::f64::consts::PI * t / (TAPS as f64 / 2.0 + 1.0)).cos();
                sinc * window
            })
        })
        .collect();
    let mut peak = 0f64;
    for channel in [left, right] {
        for &x in channel {
            peak = peak.max(x.abs() as f64);
        }
        for i in TAPS..channel.len() {
            for c in &coefficients[1..] {
                let v: f64 = (0..TAPS).map(|k| c[k] * channel[i - k] as f64).sum();
                peak = peak.max(v.abs());
            }
        }
    }
    20.0 * peak.max(1e-10).log10()
}

/// The highest sample, in dBFS.
pub fn sample_peak(left: &[f32], right: &[f32]) -> f64 {
    let peak = left.iter().chain(right).fold(0f32, |p, s| p.max(s.abs()));
    20.0 * (peak as f64).max(1e-10).log10()
}

/// How alike the two channels are: 1 mono, 0 unrelated, below 0 in opposite
/// phase (what cancels when played in mono).
pub fn correlation(left: &[f32], right: &[f32]) -> f64 {
    let (mut lr, mut ll, mut rr) = (0.0, 0.0, 0.0);
    for (&l, &r) in left.iter().zip(right) {
        let (l, r) = (l as f64, r as f64);
        lr += l * r;
        ll += l * l;
        rr += r * r;
    }
    if ll <= 0.0 || rr <= 0.0 {
        return 1.0;
    }
    lr / (ll * rr).sqrt()
}

/// The centres of the octave bands [`bands`] measures, in Hz.
pub const BAND_HZ: [f64; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// How much each octave band sounds over the whole audio, in dB (relative: the
/// power averaged over time, mid and side together).
pub fn bands(left: &[f32], right: &[f32], rate: f32) -> [f64; 10] {
    const SIZE: usize = 4096;
    let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) * 0.5).collect();
    let mut power = [0f64; 10];
    if mono.len() < SIZE {
        return [f64::NEG_INFINITY; 10];
    }
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()).collect();
    let band_of: Vec<Option<usize>> = (0..SIZE / 2)
        .map(|b| {
            let f = b as f64 * rate as f64 / SIZE as f64;
            BAND_HZ.iter().position(|&c| f >= c / 2f64.sqrt() && f < c * 2f64.sqrt())
        })
        .collect();
    // At most about two thousand frames, spread over the whole audio.
    let frames = (mono.len() - SIZE) / SIZE + 1;
    let step = (frames / 2000).max(1) * SIZE;
    let mut buffer = vec![Complex32::new(0.0, 0.0); SIZE];
    let mut start = 0;
    while start + SIZE <= mono.len() {
        for (i, b) in buffer.iter_mut().enumerate() {
            *b = Complex32::new(mono[start + i] * window[i], 0.0);
        }
        fft.process(&mut buffer);
        for (b, band) in band_of.iter().enumerate() {
            if let Some(band) = band {
                power[*band] += buffer[b].norm_sqr() as f64;
            }
        }
        start += step;
    }
    power.map(|p| 10.0 * p.max(1e-20).log10())
}

/// The level below 120 Hz, frame by frame (every 0.1 s), in dB: when a track
/// fills the low end.
pub fn low_end(left: &[f32], right: &[f32], rate: f32) -> Vec<f64> {
    // Two one-pole low passes in a row, at 120 Hz.
    let a = 1.0 - (-2.0 * std::f64::consts::PI * 120.0 / rate as f64).exp();
    let (mut y1, mut y2) = (0.0, 0.0);
    let frame = (rate * 0.1) as usize;
    let mut out = Vec::new();
    let mut energy = 0.0;
    for (i, (&l, &r)) in left.iter().zip(right).enumerate() {
        let x = (l + r) as f64 * 0.5;
        y1 += a * (x - y1);
        y2 += a * (y1 - y2);
        energy += y2 * y2;
        if (i + 1) % frame == 0 {
            out.push(10.0 * (energy / frame as f64).max(1e-20).log10());
            energy = 0.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amplitude: f32, seconds: f32, rate: f32, phase: f32) -> Vec<f32> {
        (0..(seconds * rate) as usize).map(|i| amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / rate + phase).sin()).collect()
    }

    #[test]
    fn the_k_weighting_is_the_standards_at_48khz() {
        let [shelf, high_pass] = k_weighting(48_000.0);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-8;
        assert!(close(shelf.b[0], 1.535_124_859_586_97) && close(shelf.b[1], -2.691_696_189_406_38) && close(shelf.b[2], 1.198_392_810_852_85), "{:?}", shelf.b);
        assert!(close(shelf.a[0], -1.690_659_293_182_41) && close(shelf.a[1], 0.732_480_774_215_85), "{:?}", shelf.a);
        assert!(close(high_pass.a[0], -1.990_047_454_833_98) && close(high_pass.a[1], 0.990_072_250_366_21), "{:?}", high_pass.a);
    }

    #[test]
    fn a_sine_at_1khz_measures_as_the_standard_says() {
        // 0 dBFS in both channels is 0 LUFS (each alone, -3 LUFS); at 48 and 44.1 kHz.
        for rate in [48_000.0, 44_100.0] {
            let s = sine(1000.0, 1.0, 5.0, rate, 0.0);
            let both = integrated(&s, &s, rate).unwrap();
            assert!((both - 0.0).abs() < 0.1, "{rate}: {both}");
            let silent = vec![0.0; s.len()];
            let one = integrated(&s, &silent, rate).unwrap();
            assert!((one + 3.01).abs() < 0.1, "{rate}: {one}");
            let quiet = sine(1000.0, 0.1, 5.0, rate, 0.0);
            assert!((integrated(&quiet, &quiet, rate).unwrap() + 20.0).abs() < 0.1);
        }
        assert_eq!(integrated(&[0.0; 48_000], &[0.0; 48_000], 48_000.0), None);
    }

    #[test]
    fn gating_leaves_out_the_silence() {
        // Ten seconds at -20 LUFS with ten of silence: still -20.
        let rate = 48_000.0;
        let mut s = sine(1000.0, 0.1, 10.0, rate, 0.0);
        s.extend(vec![0.0; (rate * 10.0) as usize]);
        assert!((integrated(&s, &s, rate).unwrap() + 20.0).abs() < 0.1);
        // Loud and quiet halves 20 dB apart: a range of about 20 LU.
        let mut parts = sine(1000.0, 0.5, 10.0, rate, 0.0);
        parts.extend(sine(1000.0, 0.05, 10.0, rate, 0.0));
        let lra = range(&parts, &parts, rate).unwrap();
        assert!((lra - 20.0).abs() < 1.0, "{lra}");
        let steady = sine(1000.0, 0.5, 10.0, rate, 0.0);
        assert!(range(&steady, &steady, rate).unwrap() < 0.5);
    }

    #[test]
    fn the_true_peak_sees_between_the_samples() {
        // A quarter of the rate, 45° off: every sample at 0.707, the wave at 1.
        let rate = 48_000.0;
        let s = sine(12_000.0, 1.0, 0.5, rate, std::f32::consts::FRAC_PI_4);
        assert!((sample_peak(&s, &s) + 3.01).abs() < 0.05);
        let tp = true_peak(&s, &s);
        assert!(tp > -0.5 && tp < 0.3, "{tp}");
        // A low tone: the true peak is the sample peak.
        let low = sine(100.0, 0.5, 0.5, rate, 0.0);
        assert!((true_peak(&low, &low) - sample_peak(&low, &low)).abs() < 0.05);
    }

    #[test]
    fn correlation_bands_and_low_end() {
        let rate = 48_000.0;
        let s = sine(440.0, 0.5, 1.0, rate, 0.0);
        let inverted: Vec<f32> = s.iter().map(|x| -x).collect();
        assert!((correlation(&s, &s) - 1.0).abs() < 1e-9);
        assert!((correlation(&s, &inverted) + 1.0).abs() < 1e-9);
        // A 60 Hz tone fills the 63 Hz band and the low end; a 4 kHz one, not.
        let low = sine(60.0, 0.5, 2.0, rate, 0.0);
        let b = bands(&low, &low, rate);
        assert_eq!(b.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).unwrap().0, 1);
        let high = sine(4000.0, 0.5, 2.0, rate, 0.0);
        let (l, h) = (low_end(&low, &low, rate), low_end(&high, &high, rate));
        assert_eq!(l.len(), 20);
        assert!(l[10] > h[10] + 40.0, "{} vs {}", l[10], h[10]);
    }
}
