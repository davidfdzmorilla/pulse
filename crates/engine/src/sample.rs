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
        }
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
