//! Connects the [`Engine`] to the system audio output through cpal.

use crate::engine::{Command, Engine, Garbage, LiveNote, RecordedNote, SharedState};
use std::sync::Mutex;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;

/// The UI thread's handle to a running engine.
/// Sends live notes straight to the audio thread. Cloneable, so MIDI input
/// callbacks on other threads can share it.
#[derive(Clone)]
pub struct LiveInput(Arc<Mutex<Producer<LiveNote>>>);

impl LiveInput {
    pub fn send(&self, note: LiveNote) {
        if let Ok(mut producer) = self.0.lock() {
            let _ = producer.push(note);
        }
    }
}

pub struct EngineHandle {
    commands: Producer<Command>,
    recorded: Consumer<RecordedNote>,
    pub live: LiveInput,
    garbage: Consumer<Garbage>,
    pub state: Arc<SharedState>,
    pub sample_rate: f32,
    pub device_name: String,
}

impl EngineHandle {
    /// Sends a command to the audio thread. Returns false if the queue is full.
    pub fn send(&mut self, command: Command) -> bool {
        self.commands.push(command).is_ok()
    }

    /// Notes captured by recording since the last call.
    pub fn take_recorded(&mut self) -> Vec<RecordedNote> {
        std::iter::from_fn(|| self.recorded.pop().ok()).collect()
    }

    /// Frees whatever the audio thread has replaced (clips, devices, chains). Call regularly from the UI thread.
    pub fn collect_garbage(&mut self) {
        while self.garbage.pop().is_ok() {}
    }
}

/// Keeps the output stream alive; audio stops when this is dropped.
pub struct AudioOutput {
    _stream: cpal::Stream,
}

pub fn start() -> Result<(AudioOutput, EngineHandle), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found")?;
    let device_name = device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| "default output".into());
    let supported = device
        .default_output_config()
        .map_err(|e| format!("Cannot read output config: {e}"))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let sample_rate = config.sample_rate as f32;

    let (command_tx, command_rx) = RingBuffer::new(4096);
    let (garbage_tx, garbage_rx) = RingBuffer::new(1024);
    let (live_tx, live_rx) = RingBuffer::new(1024);
    let (recorded_tx, recorded_rx) = RingBuffer::new(4096);
    let (engine, state) = Engine::new(sample_rate, command_rx, live_rx, recorded_tx, garbage_tx);

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config.clone(), engine),
        SampleFormat::F64 => build::<f64>(&device, config.clone(), engine),
        SampleFormat::I16 => build::<i16>(&device, config.clone(), engine),
        SampleFormat::I32 => build::<i32>(&device, config.clone(), engine),
        SampleFormat::U16 => build::<u16>(&device, config.clone(), engine),
        other => return Err(format!("Unsupported sample format {other}")),
    }?;
    stream
        .play()
        .map_err(|e| format!("Cannot start audio stream: {e}"))?;

    Ok((
        AudioOutput { _stream: stream },
        EngineHandle {
            commands: command_tx,
            recorded: recorded_rx,
            live: LiveInput(Arc::new(Mutex::new(live_tx))),
            garbage: garbage_rx,
            state,
            sample_rate,
            device_name,
        },
    ))
}

fn build<T>(device: &cpal::Device, config: StreamConfig, mut engine: Engine) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    // Scratch space for formats other than f32, sized up front so the callback never allocates.
    let mut scratch = vec![0f32; 8192 * channels];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for chunk in data.chunks_mut(scratch.len()) {
                    let buffer = &mut scratch[..chunk.len()];
                    engine.process(buffer, channels);
                    for (out, &sample) in chunk.iter_mut().zip(buffer.iter()) {
                        *out = T::from_sample(sample);
                    }
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| format!("Cannot open audio stream: {e}"))
}
