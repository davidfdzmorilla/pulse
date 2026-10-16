//! Audio input capture for recording.
//!
//! The input callback pushes samples into a lock-free ring and, once per
//! callback, a [`Marker`] with the transport position at that moment, so the
//! UI can line recorded audio up with the beat it was played on.

use crate::engine::SharedState;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Seconds of audio the ring can hold if the UI stalls.
const RING_SECONDS: usize = 30;

/// Transport position when the input frame `frame` was captured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub frame: u64,
    pub beat: f64,
    pub playing: bool,
}

/// Keeps the input stream open; capture stops when this is dropped.
pub struct AudioInput {
    _stream: cpal::Stream,
}

pub struct InputHandle {
    pub samples: Consumer<f32>,
    /// The same input for the engine to play back as it comes (monitoring); taken once.
    pub monitor: Option<MonitorInput>,
    pub markers: Consumer<Marker>,
    pub channels: usize,
    pub sample_rate: f32,
    pub device_name: String,
    peak: Arc<AtomicU32>,
    /// Seconds from the sound reaching the input to its callback (f64 bits).
    latency: Arc<AtomicU64>,
}

impl InputHandle {
    /// Input peak since the last call.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    /// Seconds between a sound reaching the input and Pulse getting it.
    pub fn latency(&self) -> f64 {
        f64::from_bits(self.latency.load(Ordering::Relaxed))
    }
}

pub fn start_input(state: Arc<SharedState>) -> Result<(AudioInput, InputHandle), String> {
    start_input_with(state, &crate::audio::AudioSettings::default())
}

/// Opens the input the settings choose (at their sample rate if it supports it).
pub fn start_input_with(state: Arc<SharedState>, settings: &crate::audio::AudioSettings) -> Result<(AudioInput, InputHandle), String> {
    let device = crate::audio::find_device(settings.input.as_deref(), false).ok_or("No hay ningún dispositivo de entrada de audio")?;
    let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "entrada".into());
    let (format, config) = crate::audio::stream_config(&device, &crate::audio::AudioSettings { buffer: None, ..settings.clone() }, false)?;
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f32;

    let (sample_tx, samples) = RingBuffer::new(RING_SECONDS * sample_rate as usize * channels);
    let (marker_tx, markers) = RingBuffer::new(RING_SECONDS * 1000);
    let peak = Arc::new(AtomicU32::new(0));
    let latency = Arc::new(AtomicU64::new(0));
    // A second of input is plenty: the engine keeps only a block or two of it.
    let (monitor_tx, monitor_rx) = RingBuffer::new(sample_rate as usize * channels);
    let capture = Capture {
        samples: sample_tx,
        monitor: monitor_tx,
        markers: marker_tx,
        state,
        peak: peak.clone(),
        latency: latency.clone(),
        channels,
        frame: 0,
    };
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, capture),
        SampleFormat::I16 => build::<i16>(&device, config, capture),
        SampleFormat::I32 => build::<i32>(&device, config, capture),
        SampleFormat::U16 => build::<u16>(&device, config, capture),
        other => return Err(format!("Formato de entrada no compatible: {other}")),
    }?;
    stream.play().map_err(|e| format!("No se puede abrir la entrada: {e}"))?;
    Ok((
        AudioInput { _stream: stream },
        InputHandle {
            samples,
            monitor: Some(MonitorInput::new(monitor_rx, channels, sample_rate)),
            markers,
            channels,
            sample_rate,
            device_name,
            peak,
            latency,
        },
    ))
}

/// Live input for monitoring: interleaved samples as the input callback delivers them.
pub struct MonitorInput {
    pub(crate) samples: Consumer<f32>,
    pub(crate) channels: usize,
    pub sample_rate: f32,
    /// Stereo input frames taken from the ring, not yet played (resampling reads
    /// between them); allocated here, off the audio thread.
    pub(crate) pending: Vec<(f32, f32)>,
    /// Where the next output frame reads, in frames of `pending`.
    pub(crate) position: f64,
}

impl MonitorInput {
    pub(crate) fn new(samples: Consumer<f32>, channels: usize, sample_rate: f32) -> Self {
        Self { samples, channels, sample_rate, pending: Vec::with_capacity(8192), position: 0.0 }
    }
}

/// An input with no device: whoever holds the producers feeds it samples and
/// markers by hand (tests).
pub fn detached_input(sample_rate: f32, channels: usize) -> (Producer<f32>, Producer<Marker>, InputHandle) {
    let (samples_tx, samples) = RingBuffer::new(RING_SECONDS * sample_rate as usize * channels);
    let (markers_tx, markers) = RingBuffer::new(RING_SECONDS * 1000);
    let (_, monitor) = RingBuffer::new(1);
    let handle = InputHandle {
        samples,
        monitor: Some(MonitorInput::new(monitor, channels, sample_rate)),
        markers,
        channels,
        sample_rate,
        device_name: "Sin dispositivo".into(),
        peak: Arc::default(),
        latency: Arc::default(),
    };
    (samples_tx, markers_tx, handle)
}

struct Capture {
    samples: Producer<f32>,
    monitor: Producer<f32>,
    markers: Producer<Marker>,
    state: Arc<SharedState>,
    peak: Arc<AtomicU32>,
    latency: Arc<AtomicU64>,
    channels: usize,
    frame: u64,
}

impl Capture {
    fn push<T>(&mut self, data: &[T], latency: f64)
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let marker = Marker {
            frame: self.frame,
            beat: self.state.position(),
            playing: self.state.playing.load(Ordering::Relaxed),
        };
        let _ = self.markers.push(marker);
        let mut peak = 0f32;
        for &sample in data {
            let value = <f32 as FromSample<T>>::from_sample_(sample);
            peak = peak.max(value.abs());
            // A full ring means the UI stalled for RING_SECONDS; dropping is all we can do.
            let _ = self.samples.push(value);
            // Full when nothing monitors (the engine drains it otherwise).
            let _ = self.monitor.push(value);
        }
        self.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        self.latency.store(latency.to_bits(), Ordering::Relaxed);
        self.frame += (data.len() / self.channels.max(1)) as u64;
    }
}

fn build<T>(device: &cpal::Device, config: StreamConfig, mut capture: Capture) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                let time = info.timestamp();
                capture.push(data, time.callback.duration_since(time.capture).as_secs_f64())
            },
            |err| eprintln!("audio input error: {err}"),
            None,
        )
        .map_err(|e| format!("No se puede abrir la entrada: {e}"))
}
