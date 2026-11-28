//! Decoded audio files shared between the UI (waveforms) and the audio thread (playback).

use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Frames summarised by each waveform peak.
pub const PEAK_BLOCK: usize = 256;

/// The band-limited kernel of [`Sample::read_clean`]: a sinc under a Kaiser
/// window, [`SINC_HALF`] zero crossings to each side. Tabulated in rows: row
/// `p` holds it at `p / SINC_STEPS` past each whole crossing (0, 1, 2…), so the
/// taps of one read are one row (and the next, for the straight line between).
const SINC_HALF: usize = 24;
const SINC_STEPS: usize = 256;
/// The most the kernel widens, for files read faster than their own speed.
const SINC_WIDEN: f64 = 4.0;
static SINC: std::sync::LazyLock<Vec<f32>> = std::sync::LazyLock::new(|| {
    // Zeroth-order modified Bessel function, for the Kaiser window (beta 8:
    // side lobes some 80 dB down).
    let bessel = |x: f64| {
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)) * (x / (2.0 * k));
            sum += term;
            k += 1.0;
        }
        sum
    };
    let beta = 8.0;
    let mut rows = Vec::with_capacity((SINC_STEPS + 1) * SINC_HALF);
    for row in 0..=SINC_STEPS {
        for tap in 0..SINC_HALF {
            let x = tap as f64 + row as f64 / SINC_STEPS as f64;
            let across = x / SINC_HALF as f64;
            let sinc = if x < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
            rows.push(if across >= 1.0 { 0.0 } else { (sinc * bessel(beta * (1.0 - across * across).sqrt()) / bessel(beta)) as f32 });
        }
    }
    rows
});

/// The kernel `x` zero crossings from its middle (0 from [`SINC_HALF`] on).
fn sinc_at(table: &[f32], x: f32) -> f32 {
    let tap = x as usize;
    if tap >= SINC_HALF {
        return 0.0;
    }
    let along = (x - tap as f32) * SINC_STEPS as f32;
    let row = (along as usize).min(SINC_STEPS - 1);
    let (a, b) = (table[row * SINC_HALF + tap], table[(row + 1) * SINC_HALF + tap]);
    a + (b - a) * (along - row as f32)
}

/// Builds the kernel's table, so the audio thread only ever reads it.
pub fn warm_up() {
    std::sync::LazyLock::force(&SINC);
}

/// A decoded, stereo audio file at its original sample rate.
#[derive(Debug, Default)]
pub struct Sample {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: f32,
    /// (min, max) over each block of [`PEAK_BLOCK`] frames, for drawing waveforms.
    pub peaks: Vec<(f32, f32)>,
    /// Where its hits start (seconds, in order), for the Beats warp mode. Found
    /// by the app before a clip in that mode plays it, once (the audio thread
    /// only reads it; until then the clip plays in Stretch).
    pub onsets: std::sync::OnceLock<Vec<f64>>,
    /// Where each cycle of its sound starts (frames, in order; see [`cycles`]),
    /// for the Stretch warp mode to line its grains up. Found off the audio
    /// thread before a clip in that mode plays it, once; until then its grains
    /// play where the tempo puts them.
    pub cycles: std::sync::OnceLock<Vec<f64>>,
}

/// Looking for a sound's cycles: the rate it is looked at, the shortest and
/// longest cycle (seconds), how alike two stretches must be to be cycles of one
/// sound, and the cycle where nothing repeats.
const CYCLE_RATE: f64 = 4000.0;
const CYCLE_RANGE: (f64, f64) = (1.0 / 660.0, 1.0 / 40.0);
const CYCLE_ALIKE: f32 = 0.5;
const CYCLE_REST: f64 = 0.004;
/// How far on the next cycle is measured, at least (seconds).
const CYCLE_STRIDE: f64 = 0.008;

/// Where each cycle of a sound starts, in frames from 0: each one as long as
/// the lag at which the sound there most resembles itself (the shortest such
/// lag: a period of its lowest note, mostly). Where nothing repeats (noise,
/// silence), cycles of [`CYCLE_REST`].
fn cycles(left: &[f32], right: &[f32], rate: f32) -> Vec<f64> {
    let step = ((rate as f64 / CYCLE_RATE).round() as usize).max(1);
    let low: Vec<f32> = left.chunks(step).zip(right.chunks(step)).map(|(l, r)| (l.iter().sum::<f32>() + r.iter().sum::<f32>()) / (2 * l.len()) as f32).collect();
    let rate = rate as f64 / step as f64;
    let (min, max) = (((CYCLE_RANGE.0 * rate).floor() as usize).max(2), (CYCLE_RANGE.1 * rate).ceil() as usize);
    let rest = CYCLE_REST * rate;
    let mut alike = vec![0f32; max + 2];
    let (mut marks, mut at) = (vec![0.0], 0.0f64);
    let mut cycle = rest;
    while at < low.len() as f64 {
        let a = at.round() as usize;
        cycle = rest;
        if a + 2 * max + 2 <= low.len() {
            let base = &low[a..a + max];
            let energy: f32 = base.iter().map(|x| x * x).sum();
            let mut other: f32 = low[a + min - 1..a + min - 1 + max].iter().map(|x| x * x).sum();
            for lag in min - 1..=max + 1 {
                let dot: f32 = base.iter().zip(&low[a + lag..a + lag + max]).map(|(x, y)| x * y).sum();
                alike[lag] = if energy * other > 1e-12 { dot / (energy * other).sqrt() } else { 0.0 };
                other += low[a + lag + max].powi(2) - low[a + lag].powi(2);
            }
            let best = alike[min..=max].iter().copied().fold(0f32, f32::max);
            // The shortest lag that is a peak nearly as good as the best one.
            let peak = (min..=max).find(|&l| alike[l] >= CYCLE_ALIKE && alike[l] >= 0.9 * best && alike[l] >= alike[l - 1] && alike[l] >= alike[l + 1]);
            // Between two lags, where the peak of the curve through them is.
            let exact = |l: usize| {
                let (before, here, after) = (alike[l - 1] as f64, alike[l] as f64, alike[l + 1] as f64);
                let bend = before - 2.0 * here + after;
                l as f64 + if bend < -1e-9 { (0.5 * (before - after) / bend).clamp(-0.5, 0.5) } else { 0.0 }
            };
            if let Some(l) = peak {
                cycle = exact(l);
                // Several cycles on it repeats too: measured over them, finer.
                for count in 2..=max / l {
                    let far = (cycle * count as f64).round() as usize;
                    if far > max || alike[far] < 0.9 * best || alike[far] < alike[far - 1] || alike[far] < alike[far + 1] {
                        break;
                    }
                    cycle = exact(far) / count as f64;
                }
            }
        }
        // Short cycles, a few at a time: they change no faster than long ones.
        for _ in 0..((CYCLE_STRIDE * rate / cycle) as usize).max(1) {
            at += cycle;
            marks.push(at * step as f64);
        }
    }
    marks.push((at + cycle) * step as f64);
    marks
}

/// How many cycles into a file `frame` is, by its cycle starts (at least two),
/// and the frame at a count of cycles (one undoes the other).
pub(crate) fn cycle_at(marks: &[f64], frame: f64) -> f64 {
    let i = marks.partition_point(|m| *m <= frame).clamp(1, marks.len() - 1);
    (i - 1) as f64 + (frame - marks[i - 1]) / (marks[i] - marks[i - 1])
}

pub(crate) fn cycle_frame(marks: &[f64], cycle: f64) -> f64 {
    let i = (cycle.floor().max(0.0) as usize + 1).clamp(1, marks.len() - 1);
    marks[i - 1] + (cycle - (i - 1) as f64) * (marks[i] - marks[i - 1])
}

impl Sample {
    pub fn from_channels(left: Vec<f32>, right: Vec<f32>, sample_rate: f32) -> Self {
        warm_up();
        let peaks = left
            .chunks(PEAK_BLOCK)
            .zip(right.chunks(PEAK_BLOCK))
            .map(|(l, r)| {
                l.iter().chain(r).fold((0f32, 0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)))
            })
            .collect();
        Self {
            left,
            right,
            sample_rate,
            peaks,
            onsets: std::sync::OnceLock::new(),
            cycles: std::sync::OnceLock::new(),
        }
    }

    /// Finds its cycles (see [`Sample::cycles`]), once. Not for the audio thread.
    pub fn find_cycles(&self) {
        self.cycles.get_or_init(|| cycles(&self.left, &self.right, self.sample_rate));
    }

    /// Decodes WAV, AIFF, FLAC, MP3, OGG/Vorbis, AAC/ALAC (MP4) and CAF files.
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| format!("Formato no compatible: {e}"))?;
        let track = format.default_track(TrackType::Audio).ok_or("El archivo no contiene audio")?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or("Códec de audio desconocido")?;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Códec no compatible: {e}"))?;
        let track_id = track.id;

        let (mut left, mut right) = (Vec::new(), Vec::new());
        let mut sample_rate = 0;
        let mut interleaved: Vec<f32> = Vec::new();
        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(Error::ResetRequired) => break,
                Err(e) => return Err(format!("Error leyendo el archivo: {e}")),
            };
            if packet.track_id != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
                Err(e) => return Err(format!("Error decodificando: {e}")),
            };
            sample_rate = decoded.spec().rate();
            let channels = decoded.spec().channels().count().max(1);
            interleaved.resize(decoded.samples_interleaved(), 0.0);
            decoded.copy_to_slice_interleaved(&mut interleaved);
            for frame in interleaved.chunks(channels) {
                left.push(frame[0]);
                right.push(*frame.get(1).unwrap_or(&frame[0]));
            }
        }
        if left.is_empty() || sample_rate == 0 {
            return Err("El archivo no contiene audio".into());
        }
        Ok(Self::from_channels(left, right, sample_rate as f32))
    }

    pub fn frames(&self) -> usize {
        self.left.len()
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate > 0.0 {
            self.frames() as f64 / self.sample_rate as f64
        } else {
            0.0
        }
    }

    /// Reads at a fractional frame position with cubic (Hermite) interpolation; silence outside.
    pub fn read(&self, position: f64) -> (f32, f32) {
        if position < 0.0 || !position.is_finite() {
            return (0.0, 0.0);
        }
        let i = position.floor() as usize;
        if i >= self.frames() {
            return (0.0, 0.0);
        }
        let t = (position - position.floor()) as f32;
        let tap = |buf: &[f32], k: isize| -> f32 {
            if k < 0 {
                0.0
            } else {
                buf.get(k as usize).copied().unwrap_or(0.0)
            }
        };
        let hermite = |buf: &[f32]| {
            let k = i as isize;
            let (y0, y1, y2, y3) = (tap(buf, k - 1), tap(buf, k), tap(buf, k + 1), tap(buf, k + 2));
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            ((c3 * t + c2) * t + c1) * t + y1
        };
        (hermite(&self.left), hermite(&self.right))
    }

    /// Reads at a fractional frame position, band-limited: `step` is how many
    /// frames of the file go by per sample read. No images from the file's own
    /// sample rate and no loss at the top whatever the fraction (a file at
    /// another rate, or a clip off the sample grid); read faster than its own
    /// speed, what would fold back is filtered out first. On a whole frame at
    /// its own speed or slower, the frame itself. Silence outside.
    pub fn read_clean(&self, position: f64, step: f64) -> (f32, f32) {
        if position < 0.0 || !position.is_finite() {
            return (0.0, 0.0);
        }
        let frames = self.frames();
        let whole = position.floor();
        if whole >= frames as f64 {
            return (0.0, 0.0);
        }
        let i = whole as isize;
        let frac = position - whole;
        // A hair over its own speed (rounding) is its own speed.
        let cutoff = if step > 1.0001 { (1.0 / step).max(1.0 / SINC_WIDEN) } else { 1.0 };
        if cutoff == 1.0 && frac == 0.0 {
            return (self.left[i as usize], self.right[i as usize]);
        }
        let table = &SINC[..];
        let (mut l, mut r, mut total) = (0f32, 0f32, 0f32);
        let (before, after) = (i + 1 - SINC_HALF as isize, i + SINC_HALF as isize);
        if cutoff == 1.0 && before >= 0 && (after as usize) < frames {
            // At its own speed or slower, away from the file's ends: the taps
            // before the position are one row of the table, the ones after another.
            let mut side = |weights: (&[f32], &[f32]), t: f32, left: &[f32], right: &[f32], back: bool| {
                for tap in 0..SINC_HALF {
                    let weight = weights.0[tap] + (weights.1[tap] - weights.0[tap]) * t;
                    let k = if back { SINC_HALF - 1 - tap } else { tap };
                    total += weight;
                    l += left[k] * weight;
                    r += right[k] * weight;
                }
            };
            let rows = |distance: f64| {
                let along = distance * SINC_STEPS as f64;
                let row = (along as usize).min(SINC_STEPS - 1);
                ((&table[row * SINC_HALF..(row + 1) * SINC_HALF], &table[(row + 1) * SINC_HALF..(row + 2) * SINC_HALF]), (along - row as f64) as f32)
            };
            let (start, middle, end) = (before as usize, i as usize + 1, after as usize + 1);
            let (weights, t) = rows(frac);
            side(weights, t, &self.left[start..middle], &self.right[start..middle], true);
            let (weights, t) = rows(1.0 - frac);
            side(weights, t, &self.left[middle..end], &self.right[middle..end], false);
        } else {
            let reach = (SINC_HALF as f64 / cutoff).ceil() as isize;
            for k in (i - reach + 1)..=(i + reach) {
                let weight = sinc_at(table, ((k as f64 - position).abs() * cutoff) as f32);
                total += weight;
                if k >= 0 && (k as usize) < frames {
                    l += self.left[k as usize] * weight;
                    r += self.right[k as usize] * weight;
                }
            }
        }
        // By the kernel's own sum: steady level comes out as it is.
        if total > 0.0 { (l / total, r / total) } else { (0.0, 0.0) }
    }
}

/// Guesses the tempo of a loop from its length: the whole number of bars that puts
/// it closest to `prefer_bpm`, within 70–180 BPM, of 1, 2, 4, 8… bars (what
/// loops are cut to; some always fits in that range). Short one-shots return `None`.
pub fn guess_loop_bpm(duration: f64, prefer_bpm: f64) -> Option<f64> {
    if duration < 1.0 {
        return None;
    }
    let closest = |bars: &mut dyn Iterator<Item = u32>| {
        bars.map(|bars| bars as f64 * 4.0 * 60.0 / duration)
            .filter(|bpm| (70.0..=180.0).contains(bpm))
            .min_by(|a, b| (a - prefer_bpm).abs().total_cmp(&(b - prefer_bpm).abs()))
    };
    closest(&mut (0..7).map(|k| 1 << k)).or_else(|| closest(&mut (1..=64)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cycles of a sound: a note's are its period (a low one's too, a
    /// chord's the one its notes share, two of a note too high to tell), and
    /// they are counted back and forth without loss. Silence, noise and files too short to tell get even ones.
    #[test]
    fn the_cycles_of_a_sound_are_its_period() {
        let tau = std::f64::consts::TAU;
        for (rate, notes, period) in [(48_000.0, vec![440.0], 1.0 / 440.0), (44_100.0, vec![82.41], 1.0 / 82.41), (48_000.0, vec![220.0, 275.0, 330.0], 1.0 / 55.0), (96_000.0, vec![1_000.0], 2.0 / 1_000.0)] {
            let wave: Vec<f32> = (0..(2.0 * rate) as usize).map(|i| notes.iter().enumerate().map(|(n, f)| (i as f64 / rate * f * tau + n as f64).sin()).sum::<f64>() as f32 * 0.3).collect();
            let marks = cycles(&wave, &wave, rate as f32);
            let inside: Vec<f64> = marks.windows(2).map(|w| (w[1] - w[0]) / rate).collect();
            let steady = &inside[..inside.len() * 9 / 10];
            assert!(steady.iter().all(|c| (c / period - 1.0).abs() < 0.002), "{notes:?}: {:?}", &steady[..4]);
            for frame in [-500.0, 0.0, 1234.5, rate, 3.0 * rate] {
                assert!((cycle_frame(&marks, cycle_at(&marks, frame)) - frame).abs() < 1e-6, "{frame}");
            }
        }
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..48_000).map(|_| { seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (seed >> 8) as f32 / 8_388_608.0 - 1.0 }).collect();
        for (wave, even) in [(vec![0.0; 48_000], true), (noise, false), (vec![0.3; 7], true), (Vec::new(), true)] {
            let marks = cycles(&wave, &wave, 48_000.0);
            assert!(marks.len() >= 2 && marks.windows(2).all(|w| w[1] - w[0] >= 48_000.0 / 660.0 - 13.0 && w[1] - w[0] <= 48_000.0 / 40.0 + 13.0), "{} frames", wave.len());
            if even {
                assert!(marks.windows(2).all(|w| (w[1] - w[0] - 48_000.0 * CYCLE_REST).abs() < 1e-6));
            }
        }
    }

    fn write_wav(path: &Path, rate: u32, channels: u16, frames: &[i16]) {
        let data_len = (frames.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in frames {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn decodes_mono_wav_into_stereo() {
        let dir = std::env::temp_dir().join(format!("pulse-sample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let frames: Vec<i16> = (0..4410).map(|i| ((i as f32 * 0.1).sin() * 16000.0) as i16).collect();
        write_wav(&path, 44100, 1, &frames);
        let sample = Sample::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(sample.sample_rate, 44100.0);
        assert_eq!(sample.frames(), 4410);
        assert_eq!(sample.left, sample.right);
        assert!((sample.duration() - 0.1).abs() < 1e-6);
        assert!((sample.left[10] - frames[10] as f32 / 32768.0).abs() < 1e-4);
        assert_eq!(sample.peaks.len(), 4410usize.div_ceil(PEAK_BLOCK));
    }

    /// Real-world AIFF files that ship with macOS; skipped elsewhere.
    #[test]
    fn decodes_system_aiff_sounds() {
        let path = Path::new("/System/Library/Sounds/Glass.aiff");
        if !path.exists() {
            return;
        }
        let sample = Sample::load(path).unwrap();
        assert!(sample.duration() > 0.5 && sample.duration() < 5.0, "duration {}", sample.duration());
        assert!(sample.left.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn interpolation_hits_the_original_samples() {
        let sample = Sample::from_channels(vec![0.0, 1.0, 0.5, -0.5], vec![0.0; 4], 100.0);
        assert_eq!(sample.read(1.0).0, 1.0);
        assert_eq!(sample.read(2.0).0, 0.5);
        assert_eq!(sample.read(-1.0), (0.0, 0.0));
        assert_eq!(sample.read(10.0), (0.0, 0.0));
    }

    #[test]
    fn guesses_loop_tempo_from_whole_bars() {
        // Two bars at 120 BPM last 4 seconds.
        assert_eq!(guess_loop_bpm(4.0, 120.0), Some(120.0));
        // One bar at 96 BPM lasts 2.5 s; the 1-bar reading is the only one in range.
        assert_eq!(guess_loop_bpm(2.5, 120.0), Some(96.0));
        assert_eq!(guess_loop_bpm(0.3, 120.0), None);
        // 6.4 s: 4 bars at 150, not 3 at 112.5 (closer to 128).
        assert_eq!(guess_loop_bpm(6.4, 128.0), Some(150.0));
    }
}
