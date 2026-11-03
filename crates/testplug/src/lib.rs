//! A tiny VST3 plugin for testing Pulse's plugin host: "Pulse Test Synth", an
//! instrument that plays a sine per note, and "Pulse Test Gain", an effect. Each has
//! a separate edit controller, parameters (one of them stepped) and a state, so the
//! host's loading, buses, events, parameter changes and state round trips can all
//! be checked without any third-party plugin installed. The same library is also a
//! CLAP plugin with both (see `clap`).
//!
//! Like the plugins it stands for, the Gain has a sidechain input (it adds what
//! comes in there to its output, so a test hears that it arrived) and the Synth
//! has two outputs besides its main one, as a drum plugin does: notes from C1
//! (36) to B1 go to «Aux 1», those below C1 to «Aux 2», the rest to the main one.
//!
//! Started at some sample rates the VST3 plugins misbehave on purpose, for the
//! host's tests (see [`Mode`]): they hang while loading, crash, or are slow.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::{ptr, slice};

use vst3::{uid, Class, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*};

mod clap;

const SYNTH_CID: TUID = uid(0x50554C53, 0x45545354, 0x53594E54, 0x48000001);
const SYNTH_CONTROLLER_CID: TUID = uid(0x50554C53, 0x45545354, 0x53594E54, 0x48000002);
const GAIN_CID: TUID = uid(0x50554C53, 0x45545354, 0x4741494E, 0x00000001);
const GAIN_CONTROLLER_CID: TUID = uid(0x50554C53, 0x45545354, 0x4741494E, 0x00000002);

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    for (d, s) in dst.iter_mut().zip(src.bytes().chain(std::iter::once(0))) {
        *d = s as c_char;
    }
    let full = src.len() >= dst.len();
    if let (true, Some(last)) = (full, dst.last_mut()) {
        *last = 0;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut len = 0;
    for (d, s) in dst.iter_mut().zip(src.encode_utf16()) {
        *d = s as TChar;
        len += 1;
    }
    if len < dst.len() {
        dst[len] = 0;
    }
}

/// What a plugin started at an odd sample rate does instead of behaving.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    /// At 11 025 Hz: loading waits while the file [`hold_file`] exists.
    Hold,
    /// At 8 000 Hz: loading takes the process down (abort).
    CrashLoading,
    /// At 8 100 Hz: the first block takes the process down.
    CrashProcessing,
    /// At 16 000 Hz: each block takes 2 ms more (a heavy plugin).
    Slow,
    /// At 22 050 Hz: the synth's sound comes out [`LATENCY`] samples late, as it says.
    Late,
}

/// How late a synth in [`Mode::Late`] sounds.
const LATENCY: usize = 256;

impl Mode {
    fn at(sample_rate: f64) -> Mode {
        match sample_rate as u32 {
            11_025 => Mode::Hold,
            8_000 => Mode::CrashLoading,
            8_100 => Mode::CrashProcessing,
            16_000 => Mode::Slow,
            22_050 => Mode::Late,
            _ => Mode::Normal,
        }
    }
}

/// Where the host's tests say a plugin in [`Mode::Hold`] waits.
fn hold_file() -> std::path::PathBuf {
    std::env::temp_dir().join("pulse-testplug-hold")
}

/// Output buses of the synth: the main one and two more.
const SYNTH_OUTPUTS: i32 = 3;

/// The output a note plays on (see the module's notes).
fn output_of(pitch: i16) -> usize {
    match pitch {
        p if p < 36 => 2,
        p if p < 48 => 1,
        _ => 0,
    }
}

/// Which plugin a processor or controller is.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Synth,
    Gain,
}

/// Parameters: (id, title, steps, default).
fn params(kind: Kind) -> &'static [(u32, &'static str, i32, f64)] {
    match kind {
        Kind::Synth => &[(0, "Level", 0, 0.5), (1, "Octave", 2, 0.5), (2, "Bend", 0, 0.5)],
        Kind::Gain => &[(0, "Gain", 0, 0.5)],
    }
}

fn read_state(stream: *mut IBStream, values: &mut [f64]) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else { return kInvalidArgument };
    for v in values.iter_mut() {
        let mut bytes = [0u8; 8];
        let mut read = 0;
        unsafe {
            if stream.read(bytes.as_mut_ptr() as *mut c_void, 8, &mut read) != kResultOk || read != 8 {
                return kResultFalse;
            }
        }
        *v = f64::from_le_bytes(bytes);
    }
    kResultOk
}

fn write_state(stream: *mut IBStream, values: &[f64]) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else { return kInvalidArgument };
    for v in values {
        let mut bytes = v.to_le_bytes();
        let mut written = 0;
        unsafe {
            if stream.write(bytes.as_mut_ptr() as *mut c_void, 8, &mut written) != kResultOk {
                return kResultFalse;
            }
        }
    }
    kResultOk
}

struct Processor {
    kind: Kind,
    values: RefCell<Vec<f64>>,
    sample_rate: Cell<f64>,
    /// Sounding notes: (pitch, velocity, phase).
    voices: RefCell<Vec<(i16, f32, f64)>>,
    /// Each output's last [`LATENCY`] samples, in [`Mode::Late`].
    delay: RefCell<Vec<std::collections::VecDeque<f32>>>,
}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor);
}

impl Processor {
    fn new(kind: Kind) -> Self {
        Self { kind, values: RefCell::new(params(kind).iter().map(|p| p.3).collect()), sample_rate: Cell::new(44_100.0), voices: RefCell::new(Vec::new()), delay: RefCell::new(Vec::new()) }
    }
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = if self.kind == Kind::Synth { SYNTH_CONTROLLER_CID } else { GAIN_CONTROLLER_CID } };
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        match (media as MediaTypes, dir as BusDirections, self.kind) {
            (MediaTypes_::kAudio, BusDirections_::kOutput, Kind::Synth) => SYNTH_OUTPUTS,
            (MediaTypes_::kAudio, BusDirections_::kOutput, Kind::Gain) => 1,
            // The main input and a sidechain.
            (MediaTypes_::kAudio, BusDirections_::kInput, Kind::Gain) => 2,
            (MediaTypes_::kEvent, BusDirections_::kInput, Kind::Synth) => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(&self, media: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        if index < 0 || index >= unsafe { self.getBusCount(media, dir) } {
            return kInvalidArgument;
        }
        let bus = unsafe { &mut *bus };
        bus.mediaType = media;
        bus.direction = dir;
        bus.channelCount = if media as MediaTypes == MediaTypes_::kAudio { 2 } else { 16 };
        let input = dir as BusDirections == BusDirections_::kInput;
        let name = match (input, index) {
            (true, 0) => "Input",
            (true, _) => "Sidechain",
            (false, 0) => "Output",
            (false, 1) => "Aux 1",
            (false, _) => "Aux 2",
        };
        copy_wstring(name, &mut bus.name);
        bus.busType = if index == 0 { BusTypes_::kMain } else { BusTypes_::kAux } as BusType;
        bus.flags = if index == 0 { BusInfo_::BusFlags_::kDefaultActive as _ } else { 0 };
        kResultOk
    }

    unsafe fn getRoutingInfo(&self, _in_info: *mut RoutingInfo, _out_info: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(&self, _media: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        self.voices.borrow_mut().clear();
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        read_state(state, &mut self.values.borrow_mut())
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        write_state(state, &self.values.borrow())
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(&self, inputs: *mut SpeakerArrangement, num_ins: i32, outputs: *mut SpeakerArrangement, num_outs: i32) -> tresult {
        let (expected_ins, expected_outs) = if self.kind == Kind::Gain { (2, 1) } else { (0, SYNTH_OUTPUTS) };
        if num_ins != expected_ins || num_outs != expected_outs {
            return kResultFalse;
        }
        let stereo = |arrs: *mut SpeakerArrangement, n: i32| (0..n as usize).all(|i| unsafe { *arrs.add(i) } == SpeakerArr::kStereo);
        if !stereo(inputs, num_ins) || !stereo(outputs, num_outs) {
            return kResultFalse;
        }
        kResultTrue
    }

    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        if index < 0 || index >= unsafe { self.getBusCount(MediaTypes_::kAudio as MediaType, dir) } {
            return kInvalidArgument;
        }
        unsafe { *arr = SpeakerArr::kStereo };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size as SymbolicSampleSizes == SymbolicSampleSizes_::kSample32 { kResultOk } else { kNotImplemented }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        if self.kind == Kind::Synth && Mode::at(self.sample_rate.get()) == Mode::Late { LATENCY as u32 } else { 0 }
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let sample_rate = unsafe { (*setup).sampleRate };
        self.sample_rate.set(sample_rate);
        match Mode::at(sample_rate) {
            Mode::CrashLoading => std::process::abort(),
            Mode::Hold => {
                let started = std::time::Instant::now();
                while hold_file().exists() && started.elapsed() < std::time::Duration::from_secs(60) {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            _ => {}
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        match Mode::at(self.sample_rate.get()) {
            Mode::CrashProcessing if data.numSamples > 0 => std::process::abort(),
            Mode::Slow if data.numSamples > 0 => {
                let started = std::time::Instant::now();
                while started.elapsed() < std::time::Duration::from_millis(2) {
                    std::hint::spin_loop();
                }
            }
            _ => {}
        }
        let mut values = self.values.borrow_mut();
        if let Some(changes) = unsafe { ComRef::from_raw(data.inputParameterChanges) } {
            for i in 0..unsafe { changes.getParameterCount() } {
                let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(i)) }) else { continue };
                let id = unsafe { queue.getParameterId() } as usize;
                let (mut offset, mut value) = (0, 0.0);
                let points = unsafe { queue.getPointCount() };
                if points > 0 && unsafe { queue.getPoint(points - 1, &mut offset, &mut value) } == kResultOk && id < values.len() {
                    values[id] = value;
                }
            }
        }
        let mut voices = self.voices.borrow_mut();
        if let Some(events) = unsafe { ComRef::from_raw(data.inputEvents) } {
            for i in 0..unsafe { events.getEventCount() } {
                let mut event: Event = unsafe { std::mem::zeroed() };
                if unsafe { events.getEvent(i, &mut event) } != kResultOk {
                    continue;
                }
                match event.r#type as u32 {
                    t if t == Event_::EventTypes_::kNoteOnEvent as u32 => {
                        let on = unsafe { event.__field0.noteOn };
                        voices.push((on.pitch, on.velocity, 0.0));
                    }
                    t if t == Event_::EventTypes_::kNoteOffEvent as u32 => {
                        let off = unsafe { event.__field0.noteOff };
                        voices.retain(|v| v.0 != off.pitch);
                    }
                    _ => {}
                }
            }
        }
        let n = data.numSamples as usize;
        if data.numOutputs < 1 || n == 0 {
            return kResultOk;
        }
        let channels = |bus: &AudioBusBuffers| unsafe { slice::from_raw_parts(bus.__field0.channelBuffers32, bus.numChannels as usize) };
        let out = unsafe { &*data.outputs };
        let outs = channels(out);
        match self.kind {
            Kind::Gain => {
                if data.numInputs < 1 {
                    return kResultOk;
                }
                let input = unsafe { &*data.inputs };
                let ins = channels(input);
                let gain = (values[0] * 2.0) as f32;
                for (i, o) in ins.iter().zip(outs) {
                    let (i, o) = unsafe { (slice::from_raw_parts(*i, n), slice::from_raw_parts_mut(*o, n)) };
                    for (o, i) in o.iter_mut().zip(i) {
                        *o = i * gain;
                    }
                }
                // What comes in by the sidechain goes out too.
                if data.numInputs > 1 {
                    let side = channels(unsafe { &*data.inputs.add(1) });
                    for (s, o) in side.iter().zip(outs) {
                        let (s, o) = unsafe { (slice::from_raw_parts(*s, n), slice::from_raw_parts_mut(*o, n)) };
                        for (o, s) in o.iter_mut().zip(s) {
                            *o += s;
                        }
                    }
                }
            }
            Kind::Synth => {
                let level = values[0] as f32;
                // Octave: -1, 0 or +1; bend: ±2 semitones.
                let octave = (values[1] * 2.0).round() - 1.0 + (values[2] * 2.0 - 1.0) * 2.0 / 12.0;
                let sr = self.sample_rate.get();
                let mut mix = vec![vec![0f32; n]; SYNTH_OUTPUTS as usize];
                for (pitch, velocity, phase) in voices.iter_mut() {
                    let freq = 440.0 * 2f64.powf((*pitch as f64 - 69.0) / 12.0 + octave);
                    for s in mix[output_of(*pitch)].iter_mut() {
                        *s += (*phase * std::f64::consts::TAU).sin() as f32 * *velocity * level;
                        *phase = (*phase + freq / sr).fract();
                    }
                }
                if Mode::at(sr) == Mode::Late {
                    let mut delay = self.delay.borrow_mut();
                    delay.resize_with(mix.len(), || std::iter::repeat_n(0.0, LATENCY).collect());
                    for (line, mix) in delay.iter_mut().zip(mix.iter_mut()) {
                        for s in mix.iter_mut() {
                            line.push_back(*s);
                            *s = line.pop_front().unwrap_or(0.0);
                        }
                    }
                }
                for (bus, mix) in mix.iter().enumerate().take(data.numOutputs.max(0) as usize) {
                    for o in channels(unsafe { &*data.outputs.add(bus) }) {
                        unsafe { slice::from_raw_parts_mut(*o, n) }.copy_from_slice(mix);
                    }
                }
            }
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

struct Controller {
    kind: Kind,
    values: RefCell<Vec<f64>>,
}

impl Class for Controller {
    type Interfaces = (IEditController, IMidiMapping);
}

/// The synth maps the modulation wheel to its level and pitch bend to its bend.
impl IMidiMappingTrait for Controller {
    unsafe fn getMidiControllerAssignment(&self, bus: i32, _channel: i16, number: CtrlNumber, id: *mut ParamID) -> tresult {
        let param = match (self.kind, bus, number as u32) {
            (Kind::Synth, 0, 1) => 0,
            (Kind::Synth, 0, n) if n == ControllerNumbers_::kPitchBend as u32 => 2,
            _ => return kResultFalse,
        };
        unsafe { *id = param };
        kResultTrue
    }
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        read_state(state, &mut self.values.borrow_mut())
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        params(self.kind).len() as i32
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let Some(&(id, title, steps, default)) = params(self.kind).get(index as usize) else { return kInvalidArgument };
        let info = unsafe { &mut *info };
        info.id = id;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = steps;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, id: u32, value: f64, string: *mut String128) -> tresult {
        let text = match (self.kind, id) {
            (Kind::Synth, 1) => format!("{:+}", (value * 2.0).round() - 1.0),
            (Kind::Gain, 0) => format!("{:.2}x", value * 2.0),
            _ => format!("{:.0} %", value * 100.0),
        };
        copy_wstring(&text, unsafe { &mut *string });
        kResultOk
    }

    unsafe fn getParamValueByString(&self, _id: u32, _string: *mut TChar, _value: *mut f64) -> tresult {
        kNotImplemented
    }

    unsafe fn normalizedParamToPlain(&self, _id: u32, value: f64) -> f64 {
        value
    }

    unsafe fn plainParamToNormalized(&self, _id: u32, value: f64) -> f64 {
        value
    }

    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        self.values.borrow().get(id as usize).copied().unwrap_or(0.0)
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        match self.values.borrow_mut().get_mut(id as usize) {
            Some(v) => {
                *v = value;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        ptr::null_mut()
    }
}

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

/// (cid, category, name, subcategories) of every class.
const CLASSES: [(TUID, &str, &str, &str); 4] = [
    (SYNTH_CID, "Audio Module Class", "Pulse Test Synth", "Instrument|Synth"),
    (SYNTH_CONTROLLER_CID, "Component Controller Class", "Pulse Test Synth", ""),
    (GAIN_CID, "Audio Module Class", "Pulse Test Gain", "Fx|Dynamics"),
    (GAIN_CONTROLLER_CID, "Component Controller Class", "Pulse Test Gain", ""),
];

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        copy_cstring("Pulse", &mut info.vendor);
        copy_cstring("", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some(&(cid, category, name, _)) = CLASSES.get(index as usize) else { return kInvalidArgument };
        let info = unsafe { &mut *info };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(name, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        let instance = match unsafe { *(cid as *const TUID) } {
            SYNTH_CID => ComWrapper::new(Processor::new(Kind::Synth)).to_com_ptr::<FUnknown>(),
            GAIN_CID => ComWrapper::new(Processor::new(Kind::Gain)).to_com_ptr::<FUnknown>(),
            SYNTH_CONTROLLER_CID => ComWrapper::new(Controller { kind: Kind::Synth, values: RefCell::new(params(Kind::Synth).iter().map(|p| p.3).collect()) }).to_com_ptr::<FUnknown>(),
            GAIN_CONTROLLER_CID => ComWrapper::new(Controller { kind: Kind::Gain, values: RefCell::new(params(Kind::Gain).iter().map(|p| p.3).collect()) }).to_com_ptr::<FUnknown>(),
            _ => None,
        };
        match instance {
            Some(instance) => {
                let ptr = instance.as_ptr();
                unsafe { ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj) }
            }
            None => kInvalidArgument,
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some(&(cid, category, name, sub)) = CLASSES.get(index as usize) else { return kInvalidArgument };
        let info = unsafe { &mut *info };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(name, &mut info.name);
        info.classFlags = 0;
        copy_cstring(sub, &mut info.subCategories);
        copy_cstring("Pulse", &mut info.vendor);
        copy_cstring("1.0.0", &mut info.version);
        copy_cstring("VST 3.7", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleExit() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().map_or(ptr::null_mut(), |p| p.into_raw())
}
