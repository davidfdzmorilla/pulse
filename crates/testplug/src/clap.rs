//! The same two test plugins in the CLAP format. Their parameters have plain ranges
//! (Octave −1..1 stepped, Bend ±2 semitones, Gain 0..2×), so the host's conversion to
//! and from normalized values is tested too. The synth takes notes as CLAP note
//! events or MIDI, and maps the modulation wheel to Level and pitch bend to Bend.

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Synth,
    Gain,
}

/// (id, name, min, max, default, stepped).
fn params(kind: Kind) -> &'static [(u32, &'static str, f64, f64, f64, bool)] {
    match kind {
        Kind::Synth => &[(0, "Level", 0.0, 1.0, 0.5, false), (1, "Octave", -1.0, 1.0, 0.0, true), (2, "Bend", -2.0, 2.0, 0.0, false)],
        Kind::Gain => &[(0, "Gain", 0.0, 2.0, 1.0, false)],
    }
}

struct Plugin {
    kind: Kind,
    values: Vec<f64>,
    sample_rate: f64,
    voices: Vec<(i16, f32, f64)>,
}

impl Plugin {
    fn handle(&mut self, event: *const clap_event_header) {
        let header = unsafe { &*event };
        if header.space_id != CLAP_CORE_EVENT_SPACE_ID {
            return;
        }
        match header.type_ {
            CLAP_EVENT_NOTE_ON => {
                let note = unsafe { &*(event as *const clap_event_note) };
                self.voices.push((note.key, note.velocity as f32, 0.0));
            }
            CLAP_EVENT_NOTE_OFF => {
                let note = unsafe { &*(event as *const clap_event_note) };
                self.voices.retain(|v| v.0 != note.key);
            }
            CLAP_EVENT_PARAM_VALUE => {
                let p = unsafe { &*(event as *const clap_event_param_value) };
                if let Some(v) = self.values.get_mut(p.param_id as usize) {
                    *v = p.value;
                }
            }
            CLAP_EVENT_MIDI => {
                let midi = unsafe { &*(event as *const clap_event_midi) };
                let [status, a, b] = midi.data;
                match status & 0xF0 {
                    0x90 if b > 0 => self.voices.push((a as i16, b as f32 / 127.0, 0.0)),
                    0x80 | 0x90 => self.voices.retain(|v| v.0 != a as i16),
                    0xB0 if a == 1 && self.kind == Kind::Synth => self.values[0] = b as f64 / 127.0,
                    0xE0 if self.kind == Kind::Synth => {
                        let bend = ((b as i32) << 7 | a as i32) - 8192;
                        self.values[2] = bend as f64 / 8192.0 * 2.0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

unsafe fn data<'a>(plugin: *const clap_plugin) -> &'a mut Plugin {
    unsafe { &mut *((*plugin).plugin_data as *mut Plugin) }
}

unsafe extern "C" fn init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    unsafe {
        drop(Box::from_raw((*plugin).plugin_data as *mut Plugin));
        drop(Box::from_raw(plugin as *mut clap_plugin));
    }
}

unsafe extern "C" fn activate(plugin: *const clap_plugin, sample_rate: f64, _min: u32, _max: u32) -> bool {
    unsafe { data(plugin) }.sample_rate = sample_rate;
    true
}

unsafe extern "C" fn deactivate(_plugin: *const clap_plugin) {}

unsafe extern "C" fn start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C" fn reset(plugin: *const clap_plugin) {
    unsafe { data(plugin) }.voices.clear();
}

unsafe extern "C" fn process(plugin: *const clap_plugin, process: *const clap_process) -> clap_process_status {
    let p = unsafe { data(plugin) };
    let process = unsafe { &*process };
    let events = unsafe { &*process.in_events };
    let count = unsafe { (events.size.unwrap())(events) };
    for i in 0..count {
        p.handle(unsafe { (events.get.unwrap())(events, i) });
    }
    let n = process.frames_count as usize;
    if process.audio_outputs_count == 0 {
        return CLAP_PROCESS_CONTINUE;
    }
    let out: &clap_audio_buffer = unsafe { &*process.audio_outputs };
    let outs = unsafe { std::slice::from_raw_parts(out.data32, out.channel_count as usize) };
    match p.kind {
        Kind::Gain => {
            if process.audio_inputs_count == 0 {
                return CLAP_PROCESS_CONTINUE;
            }
            let input: &clap_audio_buffer = unsafe { &*process.audio_inputs };
            let ins = unsafe { std::slice::from_raw_parts(input.data32, input.channel_count as usize) };
            let gain = p.values[0] as f32;
            for (i, o) in ins.iter().zip(outs) {
                let (i, o) = unsafe { (std::slice::from_raw_parts(*i, n), std::slice::from_raw_parts_mut(*o, n)) };
                for (o, i) in o.iter_mut().zip(i) {
                    *o = i * gain;
                }
            }
        }
        Kind::Synth => {
            let level = p.values[0] as f32;
            let shift = p.values[1].round() + p.values[2] / 12.0;
            let mut mix = vec![0f32; n];
            for (key, velocity, phase) in p.voices.iter_mut() {
                let freq = 440.0 * 2f64.powf((*key as f64 - 69.0) / 12.0 + shift);
                for s in mix.iter_mut() {
                    *s += (*phase * std::f64::consts::TAU).sin() as f32 * *velocity * level;
                    *phase = (*phase + freq / p.sample_rate).fract();
                }
            }
            for o in outs {
                unsafe { std::slice::from_raw_parts_mut(*o, n) }.copy_from_slice(&mix);
            }
        }
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn on_main_thread(_plugin: *const clap_plugin) {}

// ----- Extensions -------------------------------------------------------------------

unsafe extern "C" fn params_count(plugin: *const clap_plugin) -> u32 {
    params(unsafe { data(plugin) }.kind).len() as u32
}

fn copy(src: &str, dst: &mut [c_char]) {
    for (d, s) in dst.iter_mut().zip(src.bytes().chain(std::iter::once(0))) {
        *d = s as c_char;
    }
}

unsafe extern "C" fn params_get_info(plugin: *const clap_plugin, index: u32, info: *mut clap_param_info) -> bool {
    let Some(&(id, name, min, max, default, stepped)) = params(unsafe { data(plugin) }.kind).get(index as usize) else { return false };
    let info = unsafe { &mut *info };
    info.id = id;
    info.flags = CLAP_PARAM_IS_AUTOMATABLE | if stepped { CLAP_PARAM_IS_STEPPED } else { 0 };
    info.cookie = ptr::null_mut();
    copy(name, &mut info.name);
    copy("", &mut info.module);
    info.min_value = min;
    info.max_value = max;
    info.default_value = default;
    true
}

unsafe extern "C" fn params_get_value(plugin: *const clap_plugin, id: clap_id, value: *mut f64) -> bool {
    match unsafe { data(plugin) }.values.get(id as usize) {
        Some(v) => {
            unsafe { *value = *v };
            true
        }
        None => false,
    }
}

unsafe extern "C" fn params_value_to_text(plugin: *const clap_plugin, id: clap_id, value: f64, buffer: *mut c_char, capacity: u32) -> bool {
    let text = match (unsafe { data(plugin) }.kind, id) {
        (Kind::Synth, 1) => format!("{:+}", value.round()),
        (Kind::Synth, 2) => format!("{value:+.1} st"),
        (Kind::Gain, 0) => format!("{value:.2}x"),
        _ => format!("{:.0} %", value * 100.0),
    };
    let dst = unsafe { std::slice::from_raw_parts_mut(buffer, capacity as usize) };
    copy(&text, dst);
    true
}

unsafe extern "C" fn params_text_to_value(_plugin: *const clap_plugin, _id: clap_id, _text: *const c_char, _value: *mut f64) -> bool {
    false
}

unsafe extern "C" fn params_flush(plugin: *const clap_plugin, in_: *const clap_input_events, _out: *const clap_output_events) {
    let p = unsafe { data(plugin) };
    let events = unsafe { &*in_ };
    for i in 0..unsafe { (events.size.unwrap())(events) } {
        p.handle(unsafe { (events.get.unwrap())(events, i) });
    }
}

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_get_info),
    get_value: Some(params_get_value),
    value_to_text: Some(params_value_to_text),
    text_to_value: Some(params_text_to_value),
    flush: Some(params_flush),
};

unsafe extern "C" fn state_save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let p = unsafe { data(plugin) };
    let bytes: Vec<u8> = p.values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let stream = unsafe { &*stream };
    unsafe { (stream.write.unwrap())(stream, bytes.as_ptr() as *const c_void, bytes.len() as u64) == bytes.len() as i64 }
}

unsafe extern "C" fn state_load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    let p = unsafe { data(plugin) };
    let stream = unsafe { &*stream };
    for v in p.values.iter_mut() {
        let mut bytes = [0u8; 8];
        if unsafe { (stream.read.unwrap())(stream, bytes.as_mut_ptr() as *mut c_void, 8) } != 8 {
            return false;
        }
        *v = f64::from_le_bytes(bytes);
    }
    true
}

static STATE: clap_plugin_state = clap_plugin_state { save: Some(state_save), load: Some(state_load) };

unsafe extern "C" fn audio_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    match (unsafe { data(plugin) }.kind, is_input) {
        (Kind::Synth, true) => 0,
        _ => 1,
    }
}

unsafe extern "C" fn audio_ports_get(plugin: *const clap_plugin, index: u32, is_input: bool, info: *mut clap_audio_port_info) -> bool {
    if index >= unsafe { audio_ports_count(plugin, is_input) } {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = if is_input { 0 } else { 1 };
    copy(if is_input { "In" } else { "Out" }, &mut info.name);
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = clap_sys::id::CLAP_INVALID_ID;
    true
}

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(audio_ports_count), get: Some(audio_ports_get) };

unsafe extern "C" fn note_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    (is_input && unsafe { data(plugin) }.kind == Kind::Synth) as u32
}

unsafe extern "C" fn note_ports_get(plugin: *const clap_plugin, index: u32, is_input: bool, info: *mut clap_note_port_info) -> bool {
    if index >= unsafe { note_ports_count(plugin, is_input) } {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
    copy("Notes", &mut info.name);
    true
}

static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports { count: Some(note_ports_count), get: Some(note_ports_get) };

unsafe extern "C" fn get_extension(_plugin: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &AUDIO_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTE_PORTS as *const _ as *const c_void
    } else {
        ptr::null()
    }
}

// ----- Factory and entry --------------------------------------------------------------

struct Descriptor(clap_plugin_descriptor);
unsafe impl Sync for Descriptor {}

struct Features([*const c_char; 3]);
unsafe impl Sync for Features {}

static SYNTH_FEATURES: Features = Features([c"instrument".as_ptr(), c"synthesizer".as_ptr(), ptr::null()]);
static GAIN_FEATURES: Features = Features([c"audio-effect".as_ptr(), c"utility".as_ptr(), ptr::null()]);

const fn descriptor(id: &'static CStr, name: &'static CStr, features: &'static Features) -> Descriptor {
    Descriptor(clap_plugin_descriptor {
        clap_version: CLAP_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        vendor: c"Pulse".as_ptr(),
        url: c"".as_ptr(),
        manual_url: c"".as_ptr(),
        support_url: c"".as_ptr(),
        version: c"1.0.0".as_ptr(),
        description: c"".as_ptr(),
        features: features.0.as_ptr(),
    })
}

static SYNTH: Descriptor = descriptor(c"org.pulse.test-synth", c"Pulse Test Synth", &SYNTH_FEATURES);
static GAIN: Descriptor = descriptor(c"org.pulse.test-gain", c"Pulse Test Gain", &GAIN_FEATURES);

unsafe extern "C" fn plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    2
}

unsafe extern "C" fn plugin_descriptor(_factory: *const clap_plugin_factory, index: u32) -> *const clap_plugin_descriptor {
    match index {
        0 => &SYNTH.0,
        1 => &GAIN.0,
        _ => ptr::null(),
    }
}

unsafe extern "C" fn create_plugin(_factory: *const clap_plugin_factory, _host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    let id = unsafe { CStr::from_ptr(id) };
    let (kind, desc) = if id == unsafe { CStr::from_ptr(SYNTH.0.id) } {
        (Kind::Synth, &SYNTH.0)
    } else if id == unsafe { CStr::from_ptr(GAIN.0.id) } {
        (Kind::Gain, &GAIN.0)
    } else {
        return ptr::null();
    };
    let data = Box::new(Plugin { kind, values: params(kind).iter().map(|p| p.4).collect(), sample_rate: 44_100.0, voices: Vec::new() });
    let plugin = Box::new(clap_plugin {
        desc,
        plugin_data: Box::into_raw(data) as *mut c_void,
        init: Some(init),
        destroy: Some(destroy),
        activate: Some(activate),
        deactivate: Some(deactivate),
        start_processing: Some(start_processing),
        stop_processing: Some(stop_processing),
        reset: Some(reset),
        process: Some(process),
        get_extension: Some(get_extension),
        on_main_thread: Some(on_main_thread),
    });
    Box::into_raw(plugin)
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(plugin_count),
    get_plugin_descriptor: Some(plugin_descriptor),
    create_plugin: Some(create_plugin),
};

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn get_factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID { &FACTORY as *const _ as *const c_void } else { ptr::null() }
}

#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry { clap_version: CLAP_VERSION, init: Some(entry_init), deinit: Some(entry_deinit), get_factory: Some(get_factory) };
