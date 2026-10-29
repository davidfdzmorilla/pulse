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
    /// Frames per buffer, if fixed.
    pub buffer: Option<u32>,
    /// Builds Convolution Reverbs' new responses off this thread.
    reverbs: crate::convolution::KernelBuilder,
}

impl EngineHandle {
    /// Sends a command to the audio thread. Returns false if the queue is full.
    pub fn send(&mut self, command: Command) -> bool {
        // A Convolution Reverb's new settings may need a new response: built
        // on another thread, it reaches the engine after this command.
        let rebuild = match &command {
            Command::UpdateDevice { target, index, device } => match device.effect {
                crate::model::Effect::Convolution(p) => Some((*target, *index, p)),
                _ => None,
            },
            _ => None,
        };
        let sent = self.commands.push(command).is_ok();
        if let (true, Some((target, index, p))) = (sent, rebuild) {
            self.reverbs.request(target, index, &p, self.sample_rate);
        }
        sent
    }

    /// Convolution Reverb responses still being built for the engine.
    pub fn building_reverbs(&self) -> usize {
        self.reverbs.pending()
    }

    /// Notes captured by recording since the last call.
    pub fn take_recorded(&mut self) -> Vec<RecordedNote> {
        std::iter::from_fn(|| self.recorded.pop().ok()).collect()
    }

    /// Frees whatever the audio thread has replaced (clips, devices, chains),
    /// and says how many. Call regularly from the UI thread.
    pub fn collect_garbage(&mut self) -> usize {
        std::iter::from_fn(|| self.garbage.pop().ok()).count()
    }
}

/// Keeps the output stream alive; audio stops when this is dropped.
pub struct AudioOutput {
    _stream: cpal::Stream,
}

/// Which audio devices to use and how; `None` is the system's default.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioSettings {
    /// Output and input devices, by name.
    pub output: Option<String>,
    pub input: Option<String>,
    pub sample_rate: Option<u32>,
    /// Frames per buffer: lower is less latency, higher is safer.
    pub buffer: Option<u32>,
}

/// An audio device and what it can do.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    /// Common sample rates it supports.
    pub rates: Vec<u32>,
    /// Buffer sizes it accepts (empty if it does not say).
    pub buffers: Vec<u32>,
}

const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const BUFFERS: [u32; 7] = [32, 64, 128, 256, 512, 1024, 2048];

fn device_name(device: &cpal::Device) -> String {
    device.description().map(|d| d.name().to_string()).or_else(|_| device.id().map(|id| id.to_string())).unwrap_or_else(|_| "?".into())
}

fn describe(device: &cpal::Device, output: bool) -> DeviceInfo {
    let ranges: Vec<cpal::SupportedStreamConfigRange> = if output {
        device.supported_output_configs().map(|c| c.collect()).unwrap_or_default()
    } else {
        device.supported_input_configs().map(|c| c.collect()).unwrap_or_default()
    };
    let rates = RATES.into_iter().filter(|&r| ranges.iter().any(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(&r))).collect();
    let buffers = BUFFERS
        .into_iter()
        .filter(|&b| ranges.iter().any(|c| matches!(*c.buffer_size(), cpal::SupportedBufferSize::Range { min, max } if (min..=max).contains(&b))))
        .collect();
    DeviceInfo { name: device_name(device), rates, buffers }
}

/// The audio outputs on this system.
pub fn output_devices() -> Vec<DeviceInfo> {
    cpal::default_host().output_devices().map(|d| d.map(|d| describe(&d, true)).collect()).unwrap_or_default()
}

/// The audio inputs on this system.
pub fn input_devices() -> Vec<DeviceInfo> {
    cpal::default_host().input_devices().map(|d| d.map(|d| describe(&d, false)).collect()).unwrap_or_default()
}

/// The device called `name` (the default one if there is none by that name).
pub(crate) fn find_device(name: Option<&str>, output: bool) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let named = name.and_then(|name| {
        let mut devices = if output { host.output_devices().ok()?.collect::<Vec<_>>() } else { host.input_devices().ok()?.collect::<Vec<_>>() };
        let index = devices.iter().position(|d| device_name(d) == name)?;
        Some(devices.swap_remove(index))
    });
    named.or_else(|| if output { host.default_output_device() } else { host.default_input_device() })
}

/// A stream configuration for `device` at the settings' rate and buffer (when it
/// supports them), else its default.
pub(crate) fn stream_config(device: &cpal::Device, settings: &AudioSettings, output: bool) -> Result<(SampleFormat, StreamConfig), String> {
    let default = if output { device.default_output_config() } else { device.default_input_config() }.map_err(|e| format!("No se puede leer la configuración de audio: {e}"))?;
    let ranges: Vec<cpal::SupportedStreamConfigRange> = if output {
        device.supported_output_configs().map(|c| c.collect()).unwrap_or_default()
    } else {
        device.supported_input_configs().map(|c| c.collect()).unwrap_or_default()
    };
    let chosen = settings
        .sample_rate
        .and_then(|rate| {
            // The default format and channels at that rate if possible, else any.
            let fits = |c: &&cpal::SupportedStreamConfigRange| (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate);
            let same = ranges.iter().filter(fits).find(|c| c.sample_format() == default.sample_format() && c.channels() == default.channels());
            same.or_else(|| ranges.iter().find(fits)).map(|c| c.with_sample_rate(rate))
        })
        .unwrap_or(default);
    let format = chosen.sample_format();
    let accepts = |b: u32| matches!(*chosen.buffer_size(), cpal::SupportedBufferSize::Range { min, max } if (min..=max).contains(&b));
    let mut config: StreamConfig = chosen.into();
    if let Some(buffer) = settings.buffer.filter(|&b| accepts(b)) {
        config.buffer_size = cpal::BufferSize::Fixed(buffer);
    }
    Ok((format, config))
}

pub fn start() -> Result<(AudioOutput, EngineHandle), String> {
    start_with(&AudioSettings::default())
}

/// Starts the engine on the output the settings choose.
pub fn start_with(settings: &AudioSettings) -> Result<(AudioOutput, EngineHandle), String> {
    let device = find_device(settings.output.as_deref(), true).ok_or("No hay ningún dispositivo de salida de audio")?;
    let device_name = device_name(&device);
    let (format, config) = stream_config(&device, settings, true)?;
    let sample_rate = config.sample_rate as f32;
    let buffer = match config.buffer_size {
        cpal::BufferSize::Fixed(n) => Some(n),
        cpal::BufferSize::Default => None,
    };

    let (mut engine, mut handle) = detached(sample_rate);
    // The tracks are shared among several cores (PULSE_THREADS=1: all on the audio thread).
    let threads = std::env::var("PULSE_THREADS").ok().and_then(|t| t.parse().ok()).unwrap_or_else(crate::workers::default_threads);
    engine.set_threads(threads);
    handle.device_name = device_name;
    handle.buffer = buffer;

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

    Ok((AudioOutput { _stream: stream }, handle))
}

/// An engine and its handle with no audio device: whoever holds the engine calls
/// [`Engine::process`] (tests drive it by hand).
pub fn detached(sample_rate: f32) -> (Engine, EngineHandle) {
    let (command_tx, command_rx) = RingBuffer::new(crate::engine::COMMAND_QUEUE);
    let (garbage_tx, garbage_rx) = RingBuffer::new(crate::engine::GARBAGE_QUEUE);
    let (live_tx, live_rx) = RingBuffer::new(1024);
    let (recorded_tx, recorded_rx) = RingBuffer::new(4096);
    let (mut engine, state) = Engine::new(sample_rate, command_rx, live_rx, recorded_tx, garbage_tx);
    let (kernel_tx, kernel_rx) = RingBuffer::new(crate::convolution::DELIVERIES);
    engine.set_kernel_queue(kernel_rx);
    let handle = EngineHandle {
        reverbs: crate::convolution::KernelBuilder::new(kernel_tx),
        commands: command_tx,
        recorded: recorded_rx,
        live: LiveInput(Arc::new(Mutex::new(live_tx))),
        garbage: garbage_rx,
        state,
        sample_rate,
        device_name: "Sin dispositivo".into(),
        buffer: None,
    };
    (engine, handle)
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
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                // What is rendered now is heard this much later (buffer, device, safety offset).
                let time = info.timestamp();
                engine.shared().set_output_latency(time.playback.duration_since(time.callback).as_secs_f64());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "diagnostic: lists this computer's audio devices"]
    fn list_devices() {
        for d in output_devices() {
            println!("salida: {} · {:?} Hz · búferes {:?}", d.name, d.rates, d.buffers);
        }
        for d in input_devices() {
            println!("entrada: {} · {:?} Hz · búferes {:?}", d.name, d.rates, d.buffers);
        }
        let device = find_device(None, true).unwrap();
        let (_, config) = stream_config(&device, &AudioSettings { sample_rate: Some(44_100), buffer: Some(128), ..Default::default() }, true).unwrap();
        println!("44,1 kHz, 128: {config:?}");
    }
}
