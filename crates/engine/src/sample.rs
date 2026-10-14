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

/// A decoded, stereo audio file at its original sample rate.
#[derive(Debug, Default)]
pub struct Sample {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: f32,
    /// (min, max) over each block of [`PEAK_BLOCK`] frames, for drawing waveforms.
    pub peaks: Vec<(f32, f32)>,
}

impl Sample {
    pub fn from_channels(left: Vec<f32>, right: Vec<f32>, sample_rate: f32) -> Self {
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
}

/// Guesses the tempo of a loop from its length: the whole number of bars that puts
/// it closest to `prefer_bpm`, within 70–180 BPM. Short one-shots return `None`.
pub fn guess_loop_bpm(duration: f64, prefer_bpm: f64) -> Option<f64> {
    if duration < 1.0 {
        return None;
    }
    (1..=64)
        .map(|bars| bars as f64 * 4.0 * 60.0 / duration)
        .filter(|bpm| (70.0..=180.0).contains(bpm))
        .min_by(|a, b| (a - prefer_bpm).abs().total_cmp(&(b - prefer_bpm).abs()))
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
    }
}
