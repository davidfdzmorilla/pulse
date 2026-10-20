//! Buffers audio input on the UI thread and cuts recorded takes out of it.

use engine::{start_input_with, AudioInput, InputHandle, Marker, SharedState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Input kept while nothing is being recorded, so a take can start slightly in the past.
const IDLE_SECONDS: usize = 4;

/// How a recording that was going on when Pulse closed is named (see [`Safety`]).
pub const UNFINISHED: &str = "Grabación sin terminar";

/// How often the safety copy is brought up to date on the disk.
const SAFETY_FLUSH: Duration = Duration::from_secs(1);

/// While recording, the input also goes straight to a WAV file whose header is
/// brought up to date every second: if Pulse closes in the middle of a take,
/// what was played is there. Removed when the recording ends (its takes are
/// written as usual).
struct Safety {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    path: PathBuf,
    flushed: Instant,
}

pub struct InputCapture {
    /// The stream (none for an input fed by hand).
    _input: Option<AudioInput>,
    handle: InputHandle,
    /// Interleaved samples; `buffer[0]` is input frame `first_frame`.
    buffer: Vec<f32>,
    first_frame: u64,
    markers: Vec<Marker>,
    pub level: f32,
    safety: Option<Safety>,
}

impl InputCapture {
    pub fn open(state: Arc<SharedState>, settings: &engine::AudioSettings) -> Result<Self, String> {
        let (input, handle) = start_input_with(state, settings)?;
        Ok(Self::with_handle(Some(input), handle))
    }

    pub fn with_handle(input: Option<AudioInput>, handle: InputHandle) -> Self {
        Self {
            _input: input,
            handle,
            buffer: Vec::new(),
            first_frame: 0,
            markers: Vec::new(),
            level: 0.0,
            safety: None,
        }
    }

    pub fn device_name(&self) -> &str {
        &self.handle.device_name
    }

    pub fn sample_rate(&self) -> f32 {
        self.handle.sample_rate
    }

    /// Seconds between a sound reaching the input and Pulse getting it.
    pub fn latency(&self) -> f64 {
        self.handle.latency()
    }

    /// The input for the engine to monitor (once).
    pub fn take_monitor(&mut self) -> Option<engine::MonitorInput> {
        self.handle.monitor.take()
    }

    /// Moves new input into the buffer. Unless `keep_all`, only the last few seconds are kept.
    pub fn pump(&mut self, keep_all: bool) {
        while let Ok(marker) = self.handle.markers.pop() {
            self.markers.push(marker);
        }
        let available = self.handle.samples.slots();
        if let Ok(chunk) = self.handle.samples.read_chunk(available) {
            let (a, b) = chunk.as_slices();
            self.buffer.extend_from_slice(a);
            self.buffer.extend_from_slice(b);
            Self::keep_safely(&mut self.safety, &[a, b]);
            chunk.commit_all();
        }
        self.level = self.handle.take_peak().max(self.level * 0.86);

        if !keep_all {
            let channels = self.handle.channels.max(1);
            let keep = IDLE_SECONDS * self.handle.sample_rate as usize;
            let frames = self.buffer.len() / channels;
            if frames > keep {
                let drop = frames - keep;
                self.buffer.drain(..drop * channels);
                self.first_frame += drop as u64;
            }
            let oldest = self.first_frame;
            self.markers.retain(|m| m.frame >= oldest);
        }
    }

    /// Starts copying the input to a file in `dir` (see [`Safety`]), with the
    /// seconds already buffered (a take can start slightly in the past).
    pub fn start_safety(&mut self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = crate::library::unique(&dir.join(format!("{UNFINISHED} {stamp}.wav")));
        let spec = hound::WavSpec { channels: self.handle.channels.max(1) as u16, sample_rate: self.handle.sample_rate as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let writer = hound::WavWriter::create(&path, spec).map_err(|e| format!("No se pudo crear {}: {e}", path.display()))?;
        self.safety = Some(Safety { writer, path, flushed: Instant::now() });
        let buffered = std::mem::take(&mut self.buffer);
        Self::keep_safely(&mut self.safety, &[&buffered]);
        self.buffer = buffered;
        Ok(())
    }

    /// The recording ended: its takes are written, the copy goes.
    pub fn finish_safety(&mut self) {
        if let Some(safety) = self.safety.take() {
            drop(safety.writer);
            let _ = std::fs::remove_file(&safety.path);
        }
    }

    /// The safety copy being written, if recording.
    pub fn safety_path(&self) -> Option<&Path> {
        self.safety.as_ref().map(|s| s.path.as_path())
    }

    /// Adds input to the safety copy, if recording (see [`Safety`]).
    fn keep_safely(slot: &mut Option<Safety>, parts: &[&[f32]]) {
        let Some(safety) = slot else { return };
        let mut written = parts.iter().flat_map(|p| p.iter()).try_for_each(|&s| safety.writer.write_sample(s));
        if written.is_ok() && safety.flushed.elapsed() >= SAFETY_FLUSH {
            written = safety.writer.flush();
            safety.flushed = Instant::now();
        }
        if let Err(e) = written {
            crate::logs::note(&format!("La copia de seguridad de la grabación ({}) se detuvo: {e}", safety.path.display()));
            *slot = None;
        }
    }

    /// Input frame captured when the transport was at `beat`, from input frame
    /// `after` on (a loop plays the same beats again on each pass).
    pub fn frame_at_beat_after(&self, beat: f64, after: u64) -> Option<u64> {
        frame_at_beat(&self.markers, beat, after)
    }

    /// The input frame after the newest one buffered.
    pub fn end_frame(&self) -> u64 {
        self.first_frame + (self.buffer.len() / self.handle.channels.max(1)) as u64
    }

    /// The newest `frames` of input, both channels mixed; None until there are as many.
    pub fn latest(&self, frames: usize) -> Option<Vec<f32>> {
        let channels = self.handle.channels.max(1);
        let have = self.buffer.len() / channels;
        if have < frames {
            return None;
        }
        let from = (have - frames) * channels;
        Some(self.buffer[from..].chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect())
    }

    /// Stereo copy of `frames` input frames from `start`; missing input is silence.
    pub fn extract(&self, start: u64, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let channels = self.handle.channels.max(1);
        let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
        for i in 0..frames {
            let Some(frame) = (start + i as u64).checked_sub(self.first_frame) else { continue };
            let at = frame as usize * channels;
            let Some(&l) = self.buffer.get(at) else { break };
            left[i] = l;
            right[i] = if channels > 1 { self.buffer.get(at + 1).copied().unwrap_or(l) } else { l };
        }
        (left, right)
    }
}

/// Input frame that was captured when the transport was at `beat`, interpolated
/// between the markers around it (or extrapolated back from the first ones, for
/// takes that start exactly when the transport does).
pub fn frame_at_beat(markers: &[Marker], beat: f64, after: u64) -> Option<u64> {
    let mut first = None;
    for pair in markers.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if !(a.playing && b.playing && b.beat > a.beat && b.frame > a.frame && a.frame >= after) {
            continue;
        }
        first.get_or_insert((a, b));
        if beat >= a.beat && beat <= b.beat {
            let t = (beat - a.beat) / (b.beat - a.beat);
            return Some(a.frame + (t * (b.frame - a.frame) as f64).round() as u64);
        }
    }
    let (a, b) = first?;
    if beat < a.beat {
        let frames_per_beat = (b.frame - a.frame) as f64 / (b.beat - a.beat);
        let back = ((a.beat - beat) * frames_per_beat).round() as u64;
        return Some(a.frame.saturating_sub(back));
    }
    None
}

pub fn write_wav(path: &Path, left: &[f32], right: &[f32], sample_rate: f32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("No se pudo crear {}: {e}", path.display()))?;
    for (&l, &r) in left.iter().zip(right) {
        writer.write_sample(l).map_err(|e| e.to_string())?;
        writer.write_sample(r).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(frame: u64, beat: f64) -> Marker {
        Marker { frame, beat, playing: true }
    }

    #[test]
    fn beats_map_to_input_frames() {
        // 512-frame callbacks at 48 kHz and 120 BPM: 0.02133 beats each.
        let markers: Vec<Marker> = (0..100).map(|i| marker(1000 + i * 512, 2.0 + i as f64 * 512.0 / 24_000.0)).collect();
        assert_eq!(frame_at_beat(&markers, 2.0, 0), Some(1000));
        // Half a beat later is 12 000 frames later.
        assert_eq!(frame_at_beat(&markers, 2.5, 0), Some(13_000));
        // Before the transport started: extrapolated backwards.
        assert_eq!(frame_at_beat(&markers, 1.99, 0), Some(760));
        // Beyond what has been captured.
        assert_eq!(frame_at_beat(&markers, 100.0, 0), None);
    }

    #[test]
    fn stopped_transport_markers_are_ignored() {
        let mut markers = vec![Marker { frame: 0, beat: 0.0, playing: false }, Marker { frame: 512, beat: 0.0, playing: false }];
        markers.extend([marker(1024, 0.0), marker(1536, 512.0 / 24_000.0)]);
        assert_eq!(frame_at_beat(&markers, 0.0, 0), Some(1024));
    }

    #[test]
    fn written_takes_load_back() {
        let dir = std::env::temp_dir().join(format!("pulse-take-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("take.wav");
        let left: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let right: Vec<f32> = left.iter().map(|s| -s).collect();
        write_wav(&path, &left, &right, 48_000.0).unwrap();
        let sample = engine::Sample::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(sample.frames(), 4800);
        assert!((sample.left[100] - left[100]).abs() < 1e-6);
        assert!((sample.right[100] - right[100]).abs() < 1e-6);
    }
}
