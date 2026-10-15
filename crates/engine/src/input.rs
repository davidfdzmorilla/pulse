//! Audio input capture for recording.
//!
//! The input callback pushes samples into a lock-free ring and, once per
//! callback, a [`Marker`] with the transport position at that moment, so the
//! UI can line recorded audio up with the beat it was played on.

use crate::engine::SharedState;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicU32, Ordering};
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
    pub markers: Consumer<Marker>,
    pub channels: usize,
    pub sample_rate: f32,
    pub device_name: String,
    peak: Arc<AtomicU32>,
}

impl InputHandle {
    /// Input peak since the last call.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
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
    let capture = Capture {
        samples: sample_tx,
        markers: marker_tx,
        state,
        peak: peak.clone(),
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
        InputHandle { samples, markers, channels, sample_rate, device_name, peak },
    ))
}

struct Capture {
    samples: Producer<f32>,
    markers: Producer<Marker>,
    state: Arc<SharedState>,
    peak: Arc<AtomicU32>,
    channels: usize,
    frame: u64,
}

impl Capture {
    fn push<T>(&mut self, data: &[T])
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
        }
        self.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
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
            move |data: &[T], _: &cpal::InputCallbackInfo| capture.push(data),
            |err| eprintln!("audio input error: {err}"),
            None,
        )
        .map_err(|e| format!("No se puede abrir la entrada: {e}"))
}
