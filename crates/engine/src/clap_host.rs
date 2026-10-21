//! CLAP plugin hosting, behind the same interface as VST3 (see `plugin`).
//!
//! CLAP is strict about threads: the plugin is created, activated, asked about its
//! parameters and state, and shows its editor on the main thread; it processes on the
//! audio thread. Parameter values are plain (in each parameter's own range); Pulse
//! works with normalized ones, converted here. Notes go as CLAP note events (or MIDI
//! if the plugin prefers it); controllers, pitch bend and pressure as MIDI.

use crate::engine::Controller;
use crate::plugin::{binary, PluginInfo, PluginParam, MAX_BLOCK};
use crate::synth::HELD;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::ext::thread_check::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;
use rtrb::{Consumer, Producer, RingBuffer};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_EVENTS: usize = 512;
const MAX_PENDING: usize = 256;

// ----- Libraries ------------------------------------------------------------------------

/// A loaded `.clap` and its factory; never unloaded, like VST3 modules.
struct Library {
    factory: *const clap_plugin_factory,
    _library: libloading::Library,
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

fn libraries() -> &'static Mutex<HashMap<PathBuf, Arc<Library>>> {
    static LIBRARIES: OnceLock<Mutex<HashMap<PathBuf, Arc<Library>>>> = OnceLock::new();
    LIBRARIES.get_or_init(Default::default)
}

fn library(bundle: &Path) -> Result<Arc<Library>, String> {
    let key = bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf());
    if let Some(library) = libraries().lock().unwrap().get(&key) {
        return Ok(library.clone());
    }
    let path = binary(&key);
    let library = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("No se pudo cargar {}: {e}", path.display()))?;
    let entry = unsafe { library.get::<*const clap_plugin_entry>(b"clap_entry") }.map_err(|_| format!("{} no es un plugin CLAP", key.display()))?;
    let entry = unsafe { &**entry };
    let bundle_path = CString::new(key.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    if !unsafe { entry.init.map_or(false, |init| init(bundle_path.as_ptr())) } {
        return Err(format!("{} no se inicializó", key.display()));
    }
    let factory = unsafe { entry.get_factory.map_or(std::ptr::null(), |get| get(CLAP_PLUGIN_FACTORY_ID.as_ptr())) } as *const clap_plugin_factory;
    if factory.is_null() {
        return Err(format!("{} no tiene fábrica de plugins", key.display()));
    }
    let library = Arc::new(Library { factory, _library: library });
    libraries().lock().unwrap().insert(key, library.clone());
    Ok(library)
}

fn text(ptr: *const c_char) -> String {
    if ptr.is_null() { String::new() } else { unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned() }
}

fn chars(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The plugins in a `.clap` (loads it).
pub(crate) fn scan(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    let library = library(bundle)?;
    let factory = unsafe { &*library.factory };
    let count = unsafe { factory.get_plugin_count.map_or(0, |f| f(factory)) };
    let mut plugins = Vec::new();
    for i in 0..count {
        let desc = unsafe { factory.get_plugin_descriptor.map_or(std::ptr::null(), |f| f(factory, i)) };
        if desc.is_null() {
            continue;
        }
        let desc = unsafe { &*desc };
        let mut features = Vec::new();
        let mut feature = desc.features;
        while !feature.is_null() && !unsafe { *feature }.is_null() {
            features.push(text(unsafe { *feature }));
            feature = unsafe { feature.add(1) };
        }
        plugins.push(PluginInfo {
            path: bundle.to_path_buf(),
            cid: text(desc.id),
            name: text(desc.name),
            vendor: text(desc.vendor),
            instrument: features.iter().any(|f| f == "instrument"),
            category: features.join("|"),
        });
    }
    Ok(plugins)
}

// ----- The host side ---------------------------------------------------------------------

thread_local! {
    /// Set on the thread that runs a CLAP processor.
    static AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
}

static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

fn is_main() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_main_np() -> i32;
        }
        unsafe { pthread_main_np() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        MAIN_THREAD.get().is_some_and(|id| *id == std::thread::current().id())
    }
}

/// What the plugin can ask of the host, per instance.
struct HostState {
    /// The editor's window takes the size the plugin asks for.
    resize: RefCell<Option<Box<dyn Fn(i32, i32)>>>,
    /// The plugin asked for `on_main_thread`.
    callback: AtomicBool,
    /// The plugin changed its state or a parameter by itself (its editor): the
    /// project has unsaved changes. Set from any thread.
    edited: AtomicBool,
}

unsafe fn state<'a>(host: *const clap_host) -> &'a HostState {
    unsafe { &*((*host).host_data as *const HostState) }
}

unsafe extern "C" fn host_get_extension(_host: *const clap_host, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_THREAD_CHECK {
        &THREAD_CHECK as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &HOST_GUI as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &HOST_STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_LOG {
        &HOST_LOG as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn host_request_restart(_host: *const clap_host) {}

unsafe extern "C" fn host_request_process(_host: *const clap_host) {}

unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    unsafe { state(host) }.callback.store(true, Ordering::Release);
}

unsafe extern "C" fn is_main_thread(_host: *const clap_host) -> bool {
    is_main()
}

unsafe extern "C" fn is_audio_thread(_host: *const clap_host) -> bool {
    AUDIO_THREAD.with(Cell::get)
}

static THREAD_CHECK: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(is_main_thread), is_audio_thread: Some(is_audio_thread) };

unsafe extern "C" fn gui_resize_hints_changed(_host: *const clap_host) {}

unsafe extern "C" fn gui_request_resize(host: *const clap_host, width: u32, height: u32) -> bool {
    match unsafe { state(host) }.resize.borrow().as_ref() {
        Some(resize) => {
            resize(width as i32, height as i32);
            true
        }
        None => false,
    }
}

unsafe extern "C" fn gui_request_show(_host: *const clap_host) -> bool {
    false
}

unsafe extern "C" fn gui_request_hide(_host: *const clap_host) -> bool {
    false
}

unsafe extern "C" fn gui_closed(_host: *const clap_host, _was_destroyed: bool) {}

static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(gui_resize_hints_changed),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_hide),
    closed: Some(gui_closed),
};

unsafe extern "C" fn params_rescan(_host: *const clap_host, _flags: clap_param_rescan_flags) {}

unsafe extern "C" fn params_clear(_host: *const clap_host, _id: clap_id, _flags: clap_param_clear_flags) {}

unsafe extern "C" fn params_request_flush(_host: *const clap_host) {}

static HOST_PARAMS: clap_host_params = clap_host_params { rescan: Some(params_rescan), clear: Some(params_clear), request_flush: Some(params_request_flush) };

unsafe extern "C" fn state_mark_dirty(host: *const clap_host) {
    unsafe { state(host) }.edited.store(true, Ordering::Relaxed);
}

static HOST_STATE: clap_host_state = clap_host_state { mark_dirty: Some(state_mark_dirty) };

unsafe extern "C" fn log(_host: *const clap_host, severity: clap_log_severity, message: *const c_char) {
    if std::env::var_os("PULSE_CLAP_LOG").is_some() || severity >= CLAP_LOG_ERROR {
        eprintln!("[clap] {}", text(message));
    }
}

static HOST_LOG: clap_host_log = clap_host_log { log: Some(log) };

// ----- Instances -------------------------------------------------------------------------

/// A parameter's plain range, to convert from and to normalized values.
#[derive(Clone, Copy)]
struct Range {
    min: f64,
    max: f64,
    stepped: bool,
}

impl Range {
    fn plain(self, normalized: f64) -> f64 {
        let value = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        if self.stepped { value.round() } else { value }
    }

    fn normalized(self, plain: f64) -> f64 {
        if self.max > self.min { ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// The plugin and what both halves share. Dropped (on the UI thread) last.
struct Instance {
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    gui: *const clap_plugin_gui,
    ranges: HashMap<u32, Range>,
    /// Whether the plugin started processing (on the audio thread).
    processing: AtomicBool,
    /// The parameter queue's reading end once no processor runs the plugin.
    idle: Mutex<Option<Consumer<(u32, f64)>>>,
    host_state: Box<HostState>,
    _host: Box<clap_host>,
    _library: Arc<Library>,
}

unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

impl Instance {
    fn plugin(&self) -> &clap_plugin {
        unsafe { &*self.plugin }
    }

    fn range(&self, id: u32) -> Range {
        self.ranges.get(&id).copied().unwrap_or(Range { min: 0.0, max: 1.0, stepped: false })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let plugin = self.plugin();
        unsafe {
            if self.processing.load(Ordering::Acquire) {
                if let Some(stop) = plugin.stop_processing {
                    stop(self.plugin);
                }
            }
            if let Some(deactivate) = plugin.deactivate {
                deactivate(self.plugin);
            }
            if let Some(destroy) = plugin.destroy {
                destroy(self.plugin);
            }
        }
    }
}

/// One event of any kind the host sends, in a slot of fixed size.
#[repr(C)]
#[derive(Clone, Copy)]
union EventSlot {
    header: clap_event_header,
    note: clap_event_note,
    param: clap_event_param_value,
    midi: clap_event_midi,
}

unsafe extern "C" fn events_size(list: *const clap_input_events) -> u32 {
    unsafe { (*((*list).ctx as *const Vec<EventSlot>)).len() as u32 }
}

unsafe extern "C" fn events_get(list: *const clap_input_events, index: u32) -> *const clap_event_header {
    let events = unsafe { &*((*list).ctx as *const Vec<EventSlot>) };
    events.get(index as usize).map_or(std::ptr::null(), |e| unsafe { &e.header } as *const _)
}

/// The plugin's own changes (its editor's) come out as parameter events: they
/// mark it edited. `ctx` is its [`HostState::edited`] (or null).
unsafe extern "C" fn out_events_push(list: *const clap_output_events, event: *const clap_event_header) -> bool {
    let edited = unsafe { (*list).ctx as *const AtomicBool };
    if !edited.is_null() && !event.is_null() && unsafe { (*event).type_ } == CLAP_EVENT_PARAM_VALUE {
        unsafe { &*edited }.store(true, Ordering::Relaxed);
    }
    true
}

fn header<T>(kind: u16, time: u32) -> clap_event_header {
    clap_event_header { size: std::mem::size_of::<T>() as u32, time, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: kind, flags: 0 }
}

/// The audio-thread half of a CLAP instance.
pub(crate) struct ClapProcessor {
    instance: Arc<Instance>,
    params: Option<Consumer<(u32, f64)>>,
    events: Vec<EventSlot>,
    buffers: Vec<Vec<f32>>,
    _pointers: Vec<*mut f32>,
    inputs: Vec<clap_audio_buffer>,
    outputs: Vec<clap_audio_buffer>,
    input_layout: Vec<(usize, usize)>,
    output_layout: Vec<(usize, usize)>,
    transport: clap_event_transport,
    steady: i64,
    pending: Vec<(u8, u32)>,
    held: [bool; 128],
    /// Notes as CLAP events (otherwise MIDI); whether MIDI controllers reach it.
    clap_notes: bool,
    midi: bool,
}

unsafe impl Send for ClapProcessor {}

impl Drop for ClapProcessor {
    fn drop(&mut self) {
        *self.instance.idle.lock().unwrap() = self.params.take();
    }
}

impl ClapProcessor {
    pub fn set_tempo(&mut self, tempo: f64) {
        self.transport.tempo = tempo;
    }

    /// Where the song is at this block's start, and whether it plays.
    pub fn set_transport(&mut self, tempo: f64, beat: f64, playing: bool) {
        let t = &mut self.transport;
        t.tempo = tempo;
        t.song_pos_beats = (beat * CLAP_BEATTIME_FACTOR as f64).round() as i64;
        t.song_pos_seconds = (beat * 60.0 / tempo.max(1.0) * clap_sys::fixedpoint::CLAP_SECTIME_FACTOR as f64).round() as i64;
        let bar = (beat / 4.0).floor();
        t.bar_start = (bar * 4.0 * CLAP_BEATTIME_FACTOR as f64).round() as i64;
        t.bar_number = bar as i32;
        t.flags |= CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
        if playing {
            t.flags |= CLAP_TRANSPORT_IS_PLAYING;
        } else {
            t.flags &= !CLAP_TRANSPORT_IS_PLAYING;
        }
    }

    fn push(&mut self, slot: EventSlot) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(slot);
        }
    }

    fn note(&mut self, on: bool, pitch: u8, velocity: f32, time: u32) {
        let pitch = pitch.min(127);
        if !on && !self.held[pitch as usize] {
            return;
        }
        self.held[pitch as usize] = on;
        if self.clap_notes {
            let kind = if on { CLAP_EVENT_NOTE_ON } else { CLAP_EVENT_NOTE_OFF };
            self.push(EventSlot { note: clap_event_note { header: header::<clap_event_note>(kind, time), note_id: -1, port_index: 0, channel: 0, key: pitch as i16, velocity: velocity as f64 } });
        } else {
            let data = if on { [0x90, pitch, ((velocity * 127.0).round() as u8).clamp(1, 127)] } else { [0x80, pitch, 0] };
            self.midi_event(data, time);
        }
    }

    fn midi_event(&mut self, data: [u8; 3], time: u32) {
        self.push(EventSlot { midi: clap_event_midi { header: header::<clap_event_midi>(CLAP_EVENT_MIDI, time), port_index: 0, data } });
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        self.note(true, pitch, velocity, 0);
        if gate != HELD && self.pending.len() < MAX_PENDING {
            self.pending.push((pitch, gate));
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        self.pending.retain(|&(p, _)| p != pitch);
        self.note(false, pitch, 0.0, 0);
    }

    pub fn all_notes_off(&mut self) {
        self.pending.clear();
        for pitch in 0..128u8 {
            if self.held[pitch as usize] {
                self.note(false, pitch, 0.0, 0);
            }
        }
    }

    /// Controllers, pitch bend (-1..1) and pressure (0..1), as MIDI.
    pub fn control(&mut self, controller: Controller, pitch: u8, value: f32) {
        if !self.midi {
            return;
        }
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 127.0).round() as u8;
        let data = match controller {
            Controller::Cc(n) => [0xB0, n & 127, byte(value)],
            Controller::Pressure => [0xD0, byte(value), 0],
            Controller::PolyPressure => [0xA0, pitch & 127, byte(value)],
            Controller::PitchBend => {
                let bend = ((value.clamp(-1.0, 1.0) * 8192.0) as i32 + 8192).clamp(0, 16383);
                [0xE0, (bend & 127) as u8, (bend >> 7) as u8]
            }
        };
        self.midi_event(data, 0);
    }

    /// Whether MIDI controllers reach the plugin.
    pub fn takes_midi(&self) -> bool {
        self.midi
    }

    /// Sets parameter `id` (normalized) from the next block on.
    pub fn set_normalized(&mut self, id: u32, value: f64) {
        let plain = self.instance.range(id).plain(value);
        self.push(EventSlot {
            param: clap_event_param_value {
                header: header::<clap_event_param_value>(CLAP_EVENT_PARAM_VALUE, 0),
                param_id: id,
                cookie: std::ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value: plain,
            },
        });
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(MAX_BLOCK);
            self.run(n);
            if let Some(&(first, channels)) = self.output_layout.first() {
                let r = first + if channels > 1 { 1 } else { 0 };
                for i in 0..n {
                    left[offset + i] += self.buffers[first][i];
                    right[offset + i] += self.buffers[r][i];
                }
            }
            offset += n;
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(MAX_BLOCK);
            if let Some(&(first, channels)) = self.input_layout.first() {
                self.buffers[first][..n].copy_from_slice(&left[offset..offset + n]);
                if channels > 1 {
                    self.buffers[first + 1][..n].copy_from_slice(&right[offset..offset + n]);
                }
            }
            self.run(n);
            if let Some(&(first, channels)) = self.output_layout.first() {
                let r = first + if channels > 1 { 1 } else { 0 };
                left[offset..offset + n].copy_from_slice(&self.buffers[first][..n]);
                right[offset..offset + n].copy_from_slice(&self.buffers[r][..n]);
            }
            offset += n;
        }
    }

    fn run(&mut self, n: usize) {
        AUDIO_THREAD.with(|a| a.set(true));
        let plugin = self.instance.plugin;
        if !self.instance.processing.load(Ordering::Acquire) {
            let started = unsafe { (*plugin).start_processing.is_none_or(|start| start(plugin)) };
            self.instance.processing.store(started, Ordering::Release);
        }
        if let Some(mut params) = self.params.take() {
            while let Ok((id, value)) = params.pop() {
                self.set_normalized(id, value);
            }
            self.params = Some(params);
        }
        let mut k = 0;
        while k < self.pending.len() {
            let (pitch, left) = self.pending[k];
            if (left as usize) <= n {
                self.pending.swap_remove(k);
                self.note(false, pitch, 0.0, left.saturating_sub(1));
            } else {
                self.pending[k].1 = left - n as u32;
                k += 1;
            }
        }
        // Events must come in time order, keeping the order of those at the same
        // time (an insertion sort: stable, and nothing to allocate here).
        for i in 1..self.events.len() {
            let mut j = i;
            while j > 0 && unsafe { self.events[j - 1].header.time > self.events[j].header.time } {
                self.events.swap(j - 1, j);
                j -= 1;
            }
        }
        for &(first, channels) in &self.output_layout {
            for b in &mut self.buffers[first..first + channels] {
                b[..n].fill(0.0);
            }
        }
        let in_events = clap_input_events { ctx: &mut self.events as *mut Vec<EventSlot> as *mut c_void, size: Some(events_size), get: Some(events_get) };
        let out_events = clap_output_events { ctx: &self.instance.host_state.edited as *const AtomicBool as *mut c_void, try_push: Some(out_events_push) };
        let process = clap_process {
            steady_time: self.steady,
            frames_count: n as u32,
            transport: &self.transport,
            audio_inputs: if self.inputs.is_empty() { std::ptr::null() } else { self.inputs.as_ptr() },
            audio_outputs: if self.outputs.is_empty() { std::ptr::null_mut() } else { self.outputs.as_mut_ptr() },
            audio_inputs_count: self.inputs.len() as u32,
            audio_outputs_count: self.outputs.len() as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        if self.instance.processing.load(Ordering::Acquire) {
            if let Some(process_fn) = unsafe { (*plugin).process } {
                unsafe { process_fn(plugin, &process) };
            }
        }
        self.events.clear();
        self.steady += n as i64;
        // Where the song is comes from the host each block (`set_transport`).
    }
}

/// The UI-thread half of a CLAP instance.
pub(crate) struct ClapHandle {
    instance: Arc<Instance>,
    queue: Mutex<Producer<(u32, f64)>>,
}

impl ClapHandle {
    fn params_ext(&self) -> Option<&clap_plugin_params> {
        unsafe { self.instance.params.as_ref() }
    }

    pub fn read_params(&self) -> Vec<PluginParam> {
        let Some(ext) = self.params_ext() else { return Vec::new() };
        let plugin = self.instance.plugin;
        let count = unsafe { ext.count.map_or(0, |f| f(plugin)) };
        let mut params = Vec::new();
        for i in 0..count {
            let mut info: clap_param_info = unsafe { std::mem::zeroed() };
            if !unsafe { ext.get_info.is_some_and(|f| f(plugin, i, &mut info)) } {
                continue;
            }
            if info.flags & (CLAP_PARAM_IS_HIDDEN | CLAP_PARAM_IS_READONLY) != 0 {
                continue;
            }
            let range = Range { min: info.min_value, max: info.max_value, stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0 };
            params.push(PluginParam {
                id: info.id,
                name: chars(&info.name),
                units: String::new(),
                steps: if range.stepped { (range.max - range.min).round() as i32 } else { 0 },
                default: range.normalized(info.default_value),
                automatable: info.flags & CLAP_PARAM_IS_AUTOMATABLE != 0,
            });
        }
        params
    }

    pub fn value(&self, id: u32) -> f64 {
        let Some(ext) = self.params_ext() else { return 0.0 };
        let mut plain = 0.0;
        if unsafe { ext.get_value.is_some_and(|f| f(self.instance.plugin, id, &mut plain)) } { self.instance.range(id).normalized(plain) } else { 0.0 }
    }

    pub fn display(&self, id: u32, value: f64) -> String {
        let plain = self.instance.range(id).plain(value);
        let Some(ext) = self.params_ext() else { return format!("{plain:.2}") };
        let mut buffer = [0 as c_char; 128];
        if unsafe { ext.value_to_text.is_some_and(|f| f(self.instance.plugin, id, plain, buffer.as_mut_ptr(), buffer.len() as u32)) } {
            chars(&buffer)
        } else {
            format!("{plain:.2}")
        }
    }

    pub fn set(&self, id: u32, value: f64) {
        self.instance.host_state.edited.store(true, Ordering::Relaxed);
        let _ = self.queue.lock().unwrap().push((id, value.clamp(0.0, 1.0)));
        self.flush();
    }

    /// Whether the plugin changed since the last call (see [`HostState::edited`]).
    pub fn take_edited(&self) -> bool {
        self.instance.host_state.edited.swap(false, Ordering::Relaxed)
    }

    /// CLAP plugins have no separate controller: their editor follows what they process.
    pub fn show(&self, _id: u32, _value: f64) {}

    /// Hands queued changes over when no processor runs the plugin (`params.flush`).
    fn flush(&self) {
        let mut idle = self.instance.idle.lock().unwrap();
        let Some(consumer) = idle.as_mut() else { return };
        let Some(ext) = self.params_ext() else { return };
        let mut events: Vec<EventSlot> = Vec::new();
        while let Ok((id, value)) = consumer.pop() {
            let plain = self.instance.range(id).plain(value);
            events.push(EventSlot {
                param: clap_event_param_value { header: header::<clap_event_param_value>(CLAP_EVENT_PARAM_VALUE, 0), param_id: id, cookie: std::ptr::null_mut(), note_id: -1, port_index: -1, channel: -1, key: -1, value: plain },
            });
        }
        if events.is_empty() {
            return;
        }
        let in_events = clap_input_events { ctx: &mut events as *mut Vec<EventSlot> as *mut c_void, size: Some(events_size), get: Some(events_get) };
        let out_events = clap_output_events { ctx: &self.instance.host_state.edited as *const AtomicBool as *mut c_void, try_push: Some(out_events_push) };
        if let Some(flush) = ext.flush {
            unsafe { flush(self.instance.plugin, &in_events, &out_events) };
        }
    }

    pub fn state(&self) -> Vec<u8> {
        self.flush();
        let Some(ext) = (unsafe { self.instance.state.as_ref() }) else { return Vec::new() };
        let mut bytes: Vec<u8> = Vec::new();
        unsafe extern "C" fn write(stream: *const clap_ostream, buffer: *const c_void, size: u64) -> i64 {
            let bytes = unsafe { &mut *((*stream).ctx as *mut Vec<u8>) };
            bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(buffer as *const u8, size as usize) });
            size as i64
        }
        let stream = clap_ostream { ctx: &mut bytes as *mut Vec<u8> as *mut c_void, write: Some(write) };
        if unsafe { ext.save.is_some_and(|save| save(self.instance.plugin, &stream)) } { bytes } else { Vec::new() }
    }

    /// Runs what the plugin asked to do on the main thread.
    pub fn idle(&self) {
        if self.instance.host_state.callback.swap(false, Ordering::AcqRel) {
            let plugin = self.instance.plugin();
            if let Some(callback) = plugin.on_main_thread {
                unsafe { callback(self.instance.plugin) };
            }
        }
    }

    pub fn editor(&self) -> Option<ClapView> {
        let gui = unsafe { self.instance.gui.as_ref() }?;
        let plugin = self.instance.plugin;
        let api = WINDOW_API.as_ptr();
        // Editors go into a window handle: an NSView or an HWND (X11 ids aren't pointers).
        if !cfg!(any(target_os = "macos", windows)) || !unsafe { gui.is_api_supported.is_some_and(|f| f(plugin, api, false)) } {
            return None;
        }
        if !unsafe { gui.create.is_some_and(|f| f(plugin, api, false)) } {
            return None;
        }
        Some(ClapView { instance: self.instance.clone(), attached: false })
    }
}

/// The system's window API, as CLAP names it (the same kind of view as
/// [`crate::plugin::PLATFORM_VIEW`]).
const WINDOW_API: &std::ffi::CStr = if cfg!(target_os = "macos") {
    CLAP_WINDOW_API_COCOA
} else if cfg!(windows) {
    CLAP_WINDOW_API_WIN32
} else {
    CLAP_WINDOW_API_X11
};

/// A CLAP editor. Dropping it hides and destroys it.
pub(crate) struct ClapView {
    instance: Arc<Instance>,
    attached: bool,
}

impl ClapView {
    fn gui(&self) -> &clap_plugin_gui {
        unsafe { &*self.instance.gui }
    }

    pub fn supports(&self, platform: &str) -> bool {
        platform == crate::plugin::PLATFORM_VIEW
    }

    /// Tells the editor how many pixels make a point on screen (Windows: the
    /// window's DPI over 96). False if it works that out itself.
    pub fn set_scale(&self, scale: f64) -> bool {
        unsafe { self.gui().set_scale.is_some_and(|f| f(self.instance.plugin, scale)) }
    }

    pub fn size(&self) -> (i32, i32) {
        let (mut w, mut h) = (0u32, 0u32);
        if unsafe { self.gui().get_size.is_some_and(|f| f(self.instance.plugin, &mut w, &mut h)) } { (w as i32, h as i32) } else { (0, 0) }
    }

    pub fn can_resize(&self) -> bool {
        unsafe { self.gui().can_resize.is_some_and(|f| f(self.instance.plugin)) }
    }

    pub fn on_resize(&self, resize: impl Fn(i32, i32) + 'static) {
        *self.instance.host_state.resize.borrow_mut() = Some(Box::new(resize));
    }

    /// # Safety
    /// `parent` must be a live NSView (an HWND on Windows) that outlives this editor.
    pub unsafe fn attach(&mut self, parent: *mut c_void) -> bool {
        let window = clap_window { api: WINDOW_API.as_ptr(), specific: clap_window_handle { ptr: parent } };
        let plugin = self.instance.plugin;
        self.attached = unsafe { self.gui().set_parent.is_some_and(|f| f(plugin, &window)) };
        if self.attached {
            unsafe { self.gui().show.map(|show| show(plugin)) };
        }
        self.attached
    }

    pub fn set_size(&self, width: i32, height: i32) -> (i32, i32) {
        let plugin = self.instance.plugin;
        let (mut w, mut h) = (width.max(1) as u32, height.max(1) as u32);
        unsafe {
            self.gui().adjust_size.map(|adjust| adjust(plugin, &mut w, &mut h));
            self.gui().set_size.map(|set| set(plugin, w, h));
        }
        (w as i32, h as i32)
    }
}

impl Drop for ClapView {
    fn drop(&mut self) {
        let plugin = self.instance.plugin;
        unsafe {
            if self.attached {
                self.gui().hide.map(|hide| hide(plugin));
            }
            self.gui().destroy.map(|destroy| destroy(plugin));
        }
        *self.instance.host_state.resize.borrow_mut() = None;
    }
}

/// Creates CLAP plugin `id` from `bundle` at `sample_rate`, restoring `state` if given:
/// its processor and handle, and whether it is an instrument. Main thread only.
pub(crate) fn instantiate(bundle: &Path, id: &str, sample_rate: f32, state: Option<&[u8]>) -> Result<(ClapProcessor, ClapHandle, bool), String> {
    let _ = MAIN_THREAD.set(std::thread::current().id());
    let library = library(bundle)?;
    let info = scan(bundle)?.into_iter().find(|p| p.cid == id).ok_or_else(|| format!("{} no tiene el plugin {id}", bundle.display()))?;
    let host_state = Box::new(HostState { resize: RefCell::new(None), callback: AtomicBool::new(false), edited: AtomicBool::new(false) });
    let host = Box::new(clap_host {
        clap_version: CLAP_VERSION,
        host_data: &*host_state as *const HostState as *mut c_void,
        name: c"Pulse".as_ptr(),
        vendor: c"Pulse".as_ptr(),
        url: c"".as_ptr(),
        version: c"1.0".as_ptr(),
        get_extension: Some(host_get_extension),
        request_restart: Some(host_request_restart),
        request_process: Some(host_request_process),
        request_callback: Some(host_request_callback),
    });
    let factory = unsafe { &*library.factory };
    let cid = CString::new(id).map_err(|e| e.to_string())?;
    let plugin = unsafe { factory.create_plugin.map_or(std::ptr::null(), |create| create(factory, &*host, cid.as_ptr())) };
    if plugin.is_null() {
        return Err(format!("{}: no se pudo crear", info.name));
    }
    let p = unsafe { &*plugin };
    if !unsafe { p.init.is_some_and(|init| init(plugin)) } {
        unsafe { p.destroy.map(|destroy| destroy(plugin)) };
        return Err(format!("{}: no se pudo inicializar", info.name));
    }
    let extension = |name: &CStr| unsafe { p.get_extension.map_or(std::ptr::null(), |get| get(plugin, name.as_ptr())) };
    let params = extension(CLAP_EXT_PARAMS) as *const clap_plugin_params;
    let state_ext = extension(CLAP_EXT_STATE) as *const clap_plugin_state;
    let gui = extension(CLAP_EXT_GUI) as *const clap_plugin_gui;
    let audio_ports = extension(CLAP_EXT_AUDIO_PORTS) as *const clap_plugin_audio_ports;
    let note_ports = extension(CLAP_EXT_NOTE_PORTS) as *const clap_plugin_note_ports;

    // Parameter ranges, for converting normalized values.
    let mut ranges = HashMap::new();
    if let Some(ext) = unsafe { params.as_ref() } {
        for i in 0..unsafe { ext.count.map_or(0, |f| f(plugin)) } {
            let mut info: clap_param_info = unsafe { std::mem::zeroed() };
            if unsafe { ext.get_info.is_some_and(|f| f(plugin, i, &mut info)) } {
                ranges.insert(info.id, Range { min: info.min_value, max: info.max_value, stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0 });
            }
        }
    }
    if let (Some(bytes), Some(ext)) = (state.filter(|s| !s.is_empty()), unsafe { state_ext.as_ref() }) {
        struct Reader<'a> {
            bytes: &'a [u8],
            at: usize,
        }
        unsafe extern "C" fn read(stream: *const clap_istream, buffer: *mut c_void, size: u64) -> i64 {
            let reader = unsafe { &mut *((*stream).ctx as *mut Reader) };
            let n = (size as usize).min(reader.bytes.len() - reader.at);
            unsafe { std::ptr::copy_nonoverlapping(reader.bytes.as_ptr().add(reader.at), buffer as *mut u8, n) };
            reader.at += n;
            n as i64
        }
        let mut reader = Reader { bytes, at: 0 };
        let stream = clap_istream { ctx: &mut reader as *mut Reader as *mut c_void, read: Some(read) };
        unsafe { ext.load.map(|load| load(plugin, &stream)) };
    }

    // Audio ports: the main ones get Pulse's stereo signal; the rest get buffers too.
    let ports = |input: bool| -> Vec<usize> {
        let Some(ext) = (unsafe { audio_ports.as_ref() }) else { return Vec::new() };
        (0..unsafe { ext.count.map_or(0, |f| f(plugin, input)) })
            .map(|i| {
                let mut info: clap_audio_port_info = unsafe { std::mem::zeroed() };
                if unsafe { ext.get.is_some_and(|f| f(plugin, i, input, &mut info)) } { info.channel_count.max(1) as usize } else { 2 }
            })
            .collect()
    };
    let (ins, outs) = (ports(true), ports(false));
    let (mut clap_notes, mut midi) = (true, false);
    if let Some(ext) = unsafe { note_ports.as_ref() } {
        if unsafe { ext.count.map_or(0, |f| f(plugin, true)) } > 0 {
            let mut info: clap_note_port_info = unsafe { std::mem::zeroed() };
            if unsafe { ext.get.is_some_and(|f| f(plugin, 0, true, &mut info)) } {
                clap_notes = info.supported_dialects & CLAP_NOTE_DIALECT_CLAP != 0;
                midi = info.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0;
            }
        }
    }
    if !unsafe { p.activate.is_some_and(|activate| activate(plugin, sample_rate as f64, 1, MAX_BLOCK as u32)) } {
        unsafe { p.destroy.map(|destroy| destroy(plugin)) };
        return Err(format!("{}: no acepta {sample_rate} Hz", info.name));
    }

    let mut buffers = Vec::new();
    let mut layout = |counts: &[usize]| -> Vec<(usize, usize)> {
        counts
            .iter()
            .map(|&count| {
                let first = buffers.len();
                buffers.extend((0..count).map(|_| vec![0f32; MAX_BLOCK]));
                (first, count)
            })
            .collect()
    };
    let input_layout = layout(&ins);
    let output_layout = layout(&outs);
    let mut pointers: Vec<*mut f32> = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
    let port = |pointers: &mut Vec<*mut f32>, &(first, count): &(usize, usize)| clap_audio_buffer {
        data32: unsafe { pointers.as_mut_ptr().add(first) },
        data64: std::ptr::null_mut(),
        channel_count: count as u32,
        latency: 0,
        constant_mask: 0,
    };
    let inputs = input_layout.iter().map(|l| port(&mut pointers, l)).collect();
    let outputs = output_layout.iter().map(|l| port(&mut pointers, l)).collect();

    let transport = clap_event_transport {
        header: header::<clap_event_transport>(CLAP_EVENT_TRANSPORT, 0),
        flags: CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_HAS_TIME_SIGNATURE,
        song_pos_beats: 0,
        song_pos_seconds: 0,
        tempo: 120.0,
        tempo_inc: 0.0,
        loop_start_beats: 0,
        loop_end_beats: 0,
        loop_start_seconds: 0,
        loop_end_seconds: 0,
        bar_start: 0,
        bar_number: 0,
        tsig_num: 4,
        tsig_denom: 4,
    };
    let (producer, consumer) = RingBuffer::new(1024);
    let instance = Arc::new(Instance {
        plugin,
        params,
        state: state_ext,
        gui,
        ranges,
        processing: AtomicBool::new(false),
        idle: Mutex::new(None),
        host_state,
        _host: host,
        _library: library,
    });
    let processor = ClapProcessor {
        instance: instance.clone(),
        params: Some(consumer),
        events: Vec::with_capacity(MAX_EVENTS),
        buffers,
        _pointers: pointers,
        inputs,
        outputs,
        input_layout,
        output_layout,
        transport,
        steady: 0,
        pending: Vec::with_capacity(MAX_PENDING),
        held: [false; 128],
        clap_notes,
        midi,
    };
    Ok((processor, ClapHandle { instance, queue: Mutex::new(producer) }, info.instrument))
}
