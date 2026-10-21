//! VST3 plugin hosting.
//!
//! A plugin's module (its `.vst3` bundle) is loaded once and kept for the life of
//! the process. Each instance is split in two halves that share the plugin's COM
//! objects:
//!
//! - [`PluginProcessor`] runs on the audio thread: it turns notes into VST3
//!   events, hands parameter changes to the plugin and calls `process` on
//!   preallocated buffers, never allocating;
//! - [`PluginHandle`] stays on the UI thread: it lists and sets parameters through
//!   the edit controller and saves and restores the plugin's state.
//!
//! Instances are built on the UI thread ([`instantiate`]), and must also be dropped
//! there (the engine returns replaced instruments and effects as garbage).

#![allow(non_snake_case)]

use crate::engine::Controller;
use crate::synth::HELD;
use rtrb::{Consumer, Producer, RingBuffer};
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use vst3::{Class, ComPtr, ComWrapper, Interface, Steinberg::Vst::*, Steinberg::*};

/// Longest block handed to a plugin; longer ones are split.
pub const MAX_BLOCK: usize = 1024;
const MAX_EVENTS: usize = 512;
const MAX_CHANGES: usize = 64;
const MAX_POINTS: usize = 16;
/// Notes waiting for their automatic note-off.
const MAX_PENDING: usize = 256;
/// MIDI controller numbers in VST3: CC 0..127, then aftertouch (128) and pitch bend (129).
const MIDI_CONTROLLERS: usize = 130;

/// A plugin found in a folder: one of the audio classes of a bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub path: PathBuf,
    /// The class id, as 32 hex digits.
    pub cid: String,
    pub name: String,
    pub vendor: String,
    /// VST3 subcategories, e.g. "Instrument|Synth" or "Fx|Reverb".
    pub category: String,
    pub instrument: bool,
}

/// The folders where VST3 and CLAP plugins are installed on this system.
pub fn default_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        for format in ["VST3", "CLAP"] {
            if let Some(home) = &home {
                folders.push(home.join("Library/Audio/Plug-Ins").join(format));
            }
            folders.push(PathBuf::from("/Library/Audio/Plug-Ins").join(format));
        }
    } else if cfg!(target_os = "windows") {
        // The system-wide folders (in Common Files) and the per-user ones, as the
        // VST3 and CLAP specifications name them.
        let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let common = env("CommonProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
        let local = env("LOCALAPPDATA").map(|l| l.join("Programs").join("Common"));
        for format in ["VST3", "CLAP"] {
            folders.push(common.join(format));
            if let Some(local) = &local {
                folders.push(local.join(format));
            }
        }
    } else {
        for format in ["vst3", "clap"] {
            if let Some(home) = &home {
                folders.push(home.join(format!(".{format}")));
            }
            folders.push(PathBuf::from("/usr/lib").join(format));
            folders.push(PathBuf::from("/usr/local/lib").join(format));
        }
    }
    folders
}

/// Whether `path` is a CLAP plugin (otherwise VST3).
pub fn is_clap(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("clap"))
}

/// Every `.vst3` and `.clap` bundle under `folder`, up to `MAX_DEPTH` folders deep.
pub fn bundles(folder: &Path) -> Vec<PathBuf> {
    bundles_under(folder, 0)
}

/// Folders inside folders this deep at most: a link to a folder above would
/// otherwise go round for ever.
const MAX_DEPTH: usize = 8;

fn bundles_under(folder: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if depth > MAX_DEPTH {
        return found;
    }
    let Ok(entries) = std::fs::read_dir(folder) else { return found };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("vst3") || e.eq_ignore_ascii_case("clap")) {
            found.push(path);
        } else if path.is_dir() {
            found.extend(bundles_under(&path, depth + 1));
        }
    }
    found
}

/// The plugins in a VST3 bundle (loads it).
fn vst3_scan(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    let module = module(bundle)?;
    let factory = &module.factory;
    let mut vendor = String::new();
    unsafe {
        let mut info: PFactoryInfo = std::mem::zeroed();
        if factory.getFactoryInfo(&mut info) == kResultOk {
            vendor = c_string(&info.vendor);
        }
    }
    let factory2 = factory.cast::<IPluginFactory2>();
    let mut plugins = Vec::new();
    for i in 0..unsafe { factory.countClasses() } {
        let mut info: PClassInfo = unsafe { std::mem::zeroed() };
        if unsafe { factory.getClassInfo(i, &mut info) } != kResultOk || c_string(&info.category) != "Audio Module Class" {
            continue;
        }
        let (mut category, mut class_vendor) = (String::new(), vendor.clone());
        if let Some(f2) = &factory2 {
            let mut info2: PClassInfo2 = unsafe { std::mem::zeroed() };
            if unsafe { f2.getClassInfo2(i, &mut info2) } == kResultOk {
                category = c_string(&info2.subCategories);
                let v = c_string(&info2.vendor);
                if !v.is_empty() {
                    class_vendor = v;
                }
            }
        }
        plugins.push(PluginInfo {
            path: bundle.to_path_buf(),
            cid: hex(&info.cid),
            name: c_string(&info.name),
            vendor: class_vendor,
            instrument: category.split('|').any(|c| c == "Instrument"),
            category,
        });
    }
    Ok(plugins)
}

fn c_string(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn w_string(chars: &[TChar]) -> String {
    let units: Vec<u16> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u16).collect();
    String::from_utf16_lossy(&units)
}

fn hex(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn parse_cid(text: &str) -> Option<TUID> {
    let mut cid: TUID = [0; 16];
    if text.len() != 32 {
        return None;
    }
    for (i, byte) in cid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()? as c_char;
    }
    Some(cid)
}

// ----- Modules ------------------------------------------------------------------------

/// A loaded bundle and its factory. Never unloaded: plugins often misbehave when
/// their library goes away and comes back.
struct Module {
    factory: ComPtr<IPluginFactory>,
    _library: libloading::Library,
}

fn modules() -> &'static Mutex<HashMap<PathBuf, Arc<Module>>> {
    static MODULES: OnceLock<Mutex<HashMap<PathBuf, Arc<Module>>>> = OnceLock::new();
    MODULES.get_or_init(Default::default)
}

/// The binary inside a bundle (or the path itself if it is a plain library).
pub(crate) fn binary(bundle: &Path) -> PathBuf {
    if bundle.is_file() {
        return bundle.to_path_buf();
    }
    let stem = bundle.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
    if cfg!(target_os = "macos") {
        bundle.join("Contents/MacOS").join(stem)
    } else if cfg!(target_os = "windows") {
        bundle.join("Contents/x86_64-win").join(bundle.file_name().unwrap_or_default())
    } else {
        let mut name = stem;
        name.push(".so");
        bundle.join("Contents/x86_64-linux").join(name)
    }
}

#[cfg(target_os = "macos")]
mod cf {
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFURLCreateFromFileSystemRepresentation(allocator: *const c_void, buffer: *const u8, length: isize, is_directory: u8) -> *const c_void;
        pub fn CFBundleCreate(allocator: *const c_void, url: *const c_void) -> *mut c_void;
        pub fn CFRelease(object: *const c_void);
    }
}

fn module(bundle: &Path) -> Result<Arc<Module>, String> {
    let key = bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf());
    if let Some(module) = modules().lock().unwrap().get(&key) {
        return Ok(module.clone());
    }
    let path = binary(&key);
    let library = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("No se pudo cargar {}: {e}", path.display()))?;
    unsafe {
        #[cfg(target_os = "macos")]
        if let Ok(entry) = library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"bundleEntry") {
            use std::os::unix::ffi::OsStrExt;
            let bytes = key.as_os_str().as_bytes();
            let url = cf::CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 1);
            // The bundle stays alive with the module, for the plugin to find its resources.
            let bundle_ref = if url.is_null() { std::ptr::null_mut() } else { cf::CFBundleCreate(std::ptr::null(), url) };
            if !url.is_null() {
                cf::CFRelease(url);
            }
            if !entry(bundle_ref) {
                return Err(format!("{} no se inicializó", key.display()));
            }
        }
        #[cfg(target_os = "linux")]
        if let Ok(entry) = library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"ModuleEntry") {
            entry(std::ptr::null_mut());
        }
        #[cfg(target_os = "windows")]
        if let Ok(entry) = library.get::<unsafe extern "system" fn() -> bool>(b"InitDll") {
            entry();
        }
        let get = library.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory").map_err(|_| format!("{} no es un plugin VST3", key.display()))?;
        let factory = ComPtr::from_raw(get()).ok_or_else(|| format!("{} no tiene fábrica de plugins", key.display()))?;
        let module = Arc::new(Module { factory, _library: library });
        modules().lock().unwrap().insert(key, module.clone());
        Ok(module)
    }
}

// ----- Host objects -------------------------------------------------------------------

/// Parameter changes, from the UI or the plugin's own editor, on their way to the processor.
type ParamQueue = Arc<Mutex<Producer<(u32, f64)>>>;

struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let name = unsafe { &mut *name };
        for (d, s) in name.iter_mut().zip("Pulse".encode_utf16().chain(std::iter::once(0))) {
            *d = s as TChar;
        }
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        let (cid, iid) = unsafe { ((*cid).map(|c| c as u8), (*iid).map(|c| c as u8)) };
        let instance = if cid == IMessage::IID && iid == IMessage::IID {
            ComWrapper::new(Message::default()).to_com_ptr::<IMessage>().map(|p| p.into_raw() as *mut c_void)
        } else if cid == IAttributeList::IID && iid == IAttributeList::IID {
            ComWrapper::new(Attributes::default()).to_com_ptr::<IAttributeList>().map(|p| p.into_raw() as *mut c_void)
        } else {
            None
        };
        match instance {
            Some(ptr) => {
                unsafe { *obj = ptr };
                kResultOk
            }
            None => {
                unsafe { *obj = std::ptr::null_mut() };
                kResultFalse
            }
        }
    }
}

/// Messages between a plugin's processor and controller, created on request.
#[derive(Default)]
struct Message {
    id: Mutex<Option<std::ffi::CString>>,
    attributes: OnceLock<ComPtr<IAttributeList>>,
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().unwrap().as_ref().map_or(std::ptr::null(), |id| id.as_ptr())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.lock().unwrap() = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        let list = self.attributes.get_or_init(|| ComWrapper::new(Attributes::default()).to_com_ptr::<IAttributeList>().expect("attribute list"));
        list.as_ptr()
    }
}

#[derive(Clone)]
enum Attribute {
    Int(i64),
    Float(f64),
    String(Vec<TChar>),
    Binary(Vec<u8>),
}

#[derive(Default)]
struct Attributes {
    values: Mutex<HashMap<Vec<u8>, Attribute>>,
}

impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}

fn attr_key(id: IAttrID) -> Vec<u8> {
    if id.is_null() { Vec::new() } else { unsafe { CStr::from_ptr(id) }.to_bytes().to_vec() }
}

impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: IAttrID, value: int64) -> tresult {
        self.values.lock().unwrap().insert(attr_key(id), Attribute::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: IAttrID, value: *mut int64) -> tresult {
        match self.values.lock().unwrap().get(&attr_key(id)) {
            Some(Attribute::Int(v)) => {
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: IAttrID, value: f64) -> tresult {
        self.values.lock().unwrap().insert(attr_key(id), Attribute::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: IAttrID, value: *mut f64) -> tresult {
        match self.values.lock().unwrap().get(&attr_key(id)) {
            Some(Attribute::Float(v)) => {
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: IAttrID, string: *const TChar) -> tresult {
        let mut text = Vec::new();
        if !string.is_null() {
            let mut i = 0;
            while unsafe { *string.add(i) } != 0 {
                text.push(unsafe { *string.add(i) });
                i += 1;
            }
        }
        self.values.lock().unwrap().insert(attr_key(id), Attribute::String(text));
        kResultOk
    }

    unsafe fn getString(&self, id: IAttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        match self.values.lock().unwrap().get(&attr_key(id)) {
            Some(Attribute::String(text)) => {
                let room = size_in_bytes as usize / std::mem::size_of::<TChar>();
                if room == 0 {
                    return kResultFalse;
                }
                let n = text.len().min(room - 1);
                unsafe {
                    std::ptr::copy_nonoverlapping(text.as_ptr(), string, n);
                    *string.add(n) = 0;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setBinary(&self, id: IAttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let bytes = if data.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }.to_vec() };
        self.values.lock().unwrap().insert(attr_key(id), Attribute::Binary(bytes));
        kResultOk
    }

    unsafe fn getBinary(&self, id: IAttrID, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        // The data must outlive the call, so it is read in place.
        let values = self.values.lock().unwrap();
        match values.get(&attr_key(id)) {
            Some(Attribute::Binary(bytes)) => {
                unsafe {
                    *data = bytes.as_ptr() as *const c_void;
                    *size_in_bytes = bytes.len() as uint32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// Receives edits made in the plugin's own editor.
struct ComponentHandler {
    queue: ParamQueue,
    /// Set by edits in the plugin's editor: the project has unsaved changes.
    edited: Arc<AtomicBool>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        self.edited.store(true, Ordering::Relaxed);
        let _ = self.queue.lock().unwrap().push((id, value));
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        // Often a preset loaded in the plugin's own browser.
        self.edited.store(true, Ordering::Relaxed);
        kResultOk
    }
}

/// A growable byte stream for states.
#[derive(Default)]
struct Stream {
    data: Mutex<(Vec<u8>, usize)>,
}

impl Class for Stream {
    type Interfaces = (IBStream,);
}

impl Stream {
    fn with(bytes: &[u8]) -> ComWrapper<Stream> {
        ComWrapper::new(Stream { data: Mutex::new((bytes.to_vec(), 0)) })
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.lock().unwrap().0.clone()
    }
}

impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, count: int32, read: *mut int32) -> tresult {
        let mut guard = self.data.lock().unwrap();
        let (data, pos) = &mut *guard;
        let n = (count.max(0) as usize).min(data.len().saturating_sub(*pos));
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer as *mut u8, n) };
        *pos += n;
        if !read.is_null() {
            unsafe { *read = n as int32 };
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, count: int32, written: *mut int32) -> tresult {
        let mut guard = self.data.lock().unwrap();
        let (data, pos) = &mut *guard;
        let n = count.max(0) as usize;
        let bytes = unsafe { std::slice::from_raw_parts(buffer as *const u8, n) };
        if *pos + n > data.len() {
            data.resize(*pos + n, 0);
        }
        data[*pos..*pos + n].copy_from_slice(bytes);
        *pos += n;
        if !written.is_null() {
            unsafe { *written = n as int32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let mut guard = self.data.lock().unwrap();
        let (data, pos) = &mut *guard;
        let base = match mode as u32 {
            m if m == IBStream_::IStreamSeekMode_::kIBSeekSet as u32 => 0,
            m if m == IBStream_::IStreamSeekMode_::kIBSeekCur as u32 => *pos as i64,
            _ => data.len() as i64,
        };
        *pos = (base + offset).clamp(0, data.len() as i64) as usize;
        if !result.is_null() {
            unsafe { *result = *pos as int64 };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            unsafe { *pos = self.data.lock().unwrap().1 as int64 };
        }
        kResultOk
    }
}

/// Note events for one block. Only the audio thread touches it.
struct EventList {
    events: UnsafeCell<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl EventList {
    fn new() -> Self {
        Self { events: UnsafeCell::new(Vec::with_capacity(MAX_EVENTS)) }
    }

    #[allow(clippy::mut_from_ref)]
    fn list(&self) -> &mut Vec<Event> {
        unsafe { &mut *self.events.get() }
    }
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.list().len() as int32
    }

    unsafe fn getEvent(&self, index: int32, event: *mut Event) -> tresult {
        match self.list().get(index as usize) {
            Some(e) => {
                unsafe { *event = *e };
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, event: *mut Event) -> tresult {
        let list = self.list();
        if list.len() < MAX_EVENTS {
            list.push(unsafe { *event });
            kResultOk
        } else {
            kResultFalse
        }
    }
}

/// One parameter's changes in a block.
struct ValueQueue {
    state: UnsafeCell<(ParamID, usize, [(int32, f64); MAX_POINTS])>,
}

impl Class for ValueQueue {
    type Interfaces = (IParamValueQueue,);
}

impl ValueQueue {
    #[allow(clippy::mut_from_ref)]
    fn state(&self) -> &mut (ParamID, usize, [(int32, f64); MAX_POINTS]) {
        unsafe { &mut *self.state.get() }
    }
}

impl IParamValueQueueTrait for ValueQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.state().0
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.state().1 as int32
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        let (_, count, points) = self.state();
        if index < 0 || index as usize >= *count {
            return kInvalidArgument;
        }
        unsafe {
            *offset = points[index as usize].0;
            *value = points[index as usize].1;
        }
        kResultOk
    }

    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let (_, count, points) = self.state();
        if *count >= MAX_POINTS {
            return kResultFalse;
        }
        points[*count] = (offset, value);
        if !index.is_null() {
            unsafe { *index = *count as int32 };
        }
        *count += 1;
        kResultOk
    }
}

/// The parameter changes for one block, in preallocated queues.
struct ParamChanges {
    queues: Vec<(ComWrapper<ValueQueue>, ComPtr<IParamValueQueue>)>,
    used: UnsafeCell<usize>,
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParamChanges {
    fn new() -> Self {
        let queues = (0..MAX_CHANGES)
            .map(|_| {
                let queue = ComWrapper::new(ValueQueue { state: UnsafeCell::new((0, 0, [(0, 0.0); MAX_POINTS])) });
                let ptr = queue.to_com_ptr::<IParamValueQueue>().expect("value queue");
                (queue, ptr)
            })
            .collect();
        Self { queues, used: UnsafeCell::new(0) }
    }

    #[allow(clippy::mut_from_ref)]
    fn used(&self) -> &mut usize {
        unsafe { &mut *self.used.get() }
    }

    fn clear(&self) {
        *self.used() = 0;
    }

    /// Sets parameter `id` to `value` from the start of the block (the last write wins).
    fn set(&self, id: ParamID, value: f64) {
        let used = *self.used();
        if let Some((queue, _)) = self.queues[..used].iter().find(|(q, _)| q.state().0 == id) {
            let state = queue.state();
            state.1 = 1;
            state.2[0] = (0, value);
        } else if used < MAX_CHANGES {
            *self.queues[used].0.state() = (id, 1, [(0, value); MAX_POINTS]);
            *self.used() = used + 1;
        }
    }
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        *self.used() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index >= 0 && (index as usize) < *self.used() { self.queues[index as usize].1.as_ptr() } else { std::ptr::null_mut() }
    }

    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        let used = *self.used();
        if used >= MAX_CHANGES || id.is_null() {
            return std::ptr::null_mut();
        }
        *self.queues[used].0.state() = (unsafe { *id }, 0, [(0, 0.0); MAX_POINTS]);
        *self.used() = used + 1;
        if !index.is_null() {
            unsafe { *index = used as int32 };
        }
        self.queues[used].1.as_ptr()
    }
}

// ----- Instances -----------------------------------------------------------------------

/// The plugin objects both halves of an instance share. Dropping it (on the UI thread)
/// shuts the plugin down.
struct Instance {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    /// Controller created apart from the component (terminated separately).
    separate: bool,
    connected: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    _module: Arc<Module>,
    _host: ComWrapper<HostApplication>,
    _handler: Option<ComWrapper<ComponentHandler>>,
    /// The parameter queue's reading end once no processor is running the plugin
    /// (it was dropped, e.g. with no audio device): then the UI hands changes over.
    idle: Mutex<Option<Consumer<(u32, f64)>>>,
    /// The plugin changed by itself (see `ComponentHandler::edited`) or was set.
    edited: Arc<AtomicBool>,
}

// The COM objects are only used from one thread at a time: the processor from the
// audio thread, the rest from the UI thread, as VST3 hosts do.
unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            self.processor.setProcessing(0);
            self.component.setActive(0);
            if let Some((a, b)) = &self.connected {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(controller) = &self.controller {
                controller.setComponentHandler(std::ptr::null_mut());
                if self.separate {
                    controller.terminate();
                }
            }
            self.component.terminate();
        }
    }
}

/// A parameter as the plugin describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginParam {
    pub id: u32,
    pub name: String,
    pub units: String,
    /// 0 for continuous, otherwise the number of steps.
    pub steps: i32,
    pub default: f64,
    pub automatable: bool,
}

/// The UI-thread half of a VST3 instance.
struct Vst3Handle {
    instance: Arc<Instance>,
    queue: ParamQueue,
}

impl Vst3Handle {
    /// Shows `value` for parameter `id` in the plugin's own editor, without sending it
    /// to the processor (automation reaches the processor from the engine).
    pub fn show(&self, id: u32, value: f64) {
        if let Some(controller) = &self.instance.controller {
            unsafe { controller.setParamNormalized(id, value.clamp(0.0, 1.0)) };
        }
    }

    fn read_params(&self) -> Vec<PluginParam> {
        let Some(controller) = &self.instance.controller else { return Vec::new() };
        let mut params = Vec::new();
        for i in 0..unsafe { controller.getParameterCount() } {
            let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
            if unsafe { controller.getParameterInfo(i, &mut info) } != kResultOk {
                continue;
            }
            let flags = info.flags as u32;
            // Read-only and hidden parameters are not for the host to set.
            if flags & (ParameterInfo_::ParameterFlags_::kIsReadOnly as u32 | ParameterInfo_::ParameterFlags_::kIsHidden as u32) != 0 {
                continue;
            }
            params.push(PluginParam {
                id: info.id,
                name: w_string(&info.title),
                units: w_string(&info.units),
                steps: info.stepCount,
                default: info.defaultNormalizedValue,
                automatable: flags & ParameterInfo_::ParameterFlags_::kCanAutomate as u32 != 0,
            });
        }
        params
    }

    /// The normalized (0..1) value of parameter `id`.
    pub fn value(&self, id: u32) -> f64 {
        self.instance.controller.as_ref().map_or(0.0, |c| unsafe { c.getParamNormalized(id) })
    }

    /// How the plugin shows `value` of parameter `id`, e.g. "-6.0 dB".
    pub fn display(&self, id: u32, value: f64) -> String {
        let Some(controller) = &self.instance.controller else { return format!("{value:.2}") };
        let mut text: String128 = [0; 128];
        if unsafe { controller.getParamStringByValue(id, value, &mut text) } == kResultOk { w_string(&text) } else { format!("{value:.2}") }
    }

    /// Sets parameter `id` (normalized) in the controller and, at the next block, the processor.
    pub fn set(&self, id: u32, value: f64) {
        self.instance.edited.store(true, Ordering::Relaxed);
        let value = value.clamp(0.0, 1.0);
        if let Some(controller) = &self.instance.controller {
            unsafe { controller.setParamNormalized(id, value) };
        }
        let _ = self.queue.lock().unwrap().push((id, value));
    }

    /// Gives queued parameter changes to the plugin when no processor is running it, with
    /// a call of 0 frames (a VST3 "parameter flush").
    fn flush(&self) {
        let mut idle = self.instance.idle.lock().unwrap();
        let Some(consumer) = idle.as_mut() else { return };
        if consumer.is_empty() {
            return;
        }
        let changes = ComWrapper::new(ParamChanges::new());
        while let Ok((id, value)) = consumer.pop() {
            changes.set(id, value);
        }
        let Some(changes_ptr) = changes.to_com_ptr::<IParameterChanges>() else { return };
        let mut data: ProcessData = unsafe { std::mem::zeroed() };
        data.processMode = ProcessModes_::kRealtime as int32;
        data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as int32;
        data.inputParameterChanges = changes_ptr.as_ptr();
        unsafe { self.instance.processor.process(&mut data) };
    }

    /// Whether the plugin changed since the last call.
    fn take_edited(&self) -> bool {
        self.instance.edited.swap(false, Ordering::Relaxed)
    }

    /// The plugin's whole state (component and controller), to restore with [`instantiate`].
    pub fn state(&self) -> Vec<u8> {
        self.flush();
        let component = ComWrapper::new(Stream::default());
        if let Some(stream) = component.to_com_ptr::<IBStream>() {
            unsafe { self.instance.component.getState(stream.as_ptr()) };
        }
        let controller = ComWrapper::new(Stream::default());
        if let (Some(c), Some(stream)) = (&self.instance.controller, controller.to_com_ptr::<IBStream>()) {
            unsafe { c.getState(stream.as_ptr()) };
        }
        let (a, b) = (component.bytes(), controller.bytes());
        let mut state = Vec::with_capacity(12 + a.len() + b.len());
        state.extend_from_slice(STATE_MAGIC);
        state.extend_from_slice(&(a.len() as u32).to_le_bytes());
        state.extend_from_slice(&a);
        state.extend_from_slice(&(b.len() as u32).to_le_bytes());
        state.extend_from_slice(&b);
        state
    }

    fn editor(&self) -> Option<Vst3View> {
        let controller = self.instance.controller.as_ref()?;
        let view = unsafe { ComPtr::from_raw(controller.createView(ViewType::kEditor)) }?;
        let frame = ComWrapper::new(PlugFrame { resize: std::cell::RefCell::new(None) });
        Some(Vst3View { view, frame, attached: false, _instance: self.instance.clone() })
    }
}

/// The platform's kind of native view, as VST3 names it.
pub const PLATFORM_VIEW: &str = if cfg!(target_os = "macos") {
    "NSView"
} else if cfg!(target_os = "windows") {
    "HWND"
} else {
    "X11EmbedWindowID"
};

/// Lets a plugin's editor ask for another size.
struct PlugFrame {
    resize: std::cell::RefCell<Option<Box<dyn Fn(i32, i32)>>>,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        if view.is_null() || size.is_null() {
            return kInvalidArgument;
        }
        let rect = unsafe { *size };
        if let Some(resize) = self.resize.borrow().as_ref() {
            resize(rect.right - rect.left, rect.bottom - rect.top);
        }
        // The window has the new size: tell the view.
        unsafe { ((*(*view).vtbl).onSize)(view, size) };
        kResultTrue
    }
}

/// A VST3 editor. Dropping it takes it out of its window.
struct Vst3View {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    attached: bool,
    _instance: Arc<Instance>,
}

impl Vst3View {
    /// Whether it can show itself in a native view of `platform` (see [`PLATFORM_VIEW`]).
    pub fn supports(&self, platform: &str) -> bool {
        let Ok(name) = std::ffi::CString::new(platform) else { return false };
        unsafe { self.view.isPlatformTypeSupported(name.as_ptr()) == kResultTrue }
    }

    /// Its size in points.
    pub fn size(&self) -> (i32, i32) {
        let mut rect = ViewRect { left: 0, top: 0, right: 0, bottom: 0 };
        if unsafe { self.view.getSize(&mut rect) } == kResultOk { (rect.right - rect.left, rect.bottom - rect.top) } else { (0, 0) }
    }

    pub fn can_resize(&self) -> bool {
        unsafe { self.view.canResize() == kResultTrue }
    }

    /// Tells the editor how many pixels make a point on screen. False if it doesn't listen.
    pub fn set_scale(&self, scale: f64) -> bool {
        let Some(support) = self.view.cast::<IPlugViewContentScaleSupport>() else { return false };
        unsafe { support.setContentScaleFactor(scale as f32) == kResultOk }
    }

    /// Called with the size the editor asks for; the window must take it.
    pub fn on_resize(&self, resize: impl Fn(i32, i32) + 'static) {
        *self.frame.resize.borrow_mut() = Some(Box::new(resize));
    }

    /// Puts the editor in `parent`, a native view of [`PLATFORM_VIEW`].
    ///
    /// # Safety
    /// `parent` must be a live view of that kind, and outlive this editor.
    pub unsafe fn attach(&mut self, parent: *mut c_void) -> bool {
        let Ok(platform) = std::ffi::CString::new(PLATFORM_VIEW) else { return false };
        if let Some(frame) = self.frame.to_com_ptr::<IPlugFrame>() {
            unsafe { self.view.setFrame(frame.as_ptr()) };
        }
        self.attached = unsafe { self.view.attached(parent, platform.as_ptr()) } == kResultOk;
        self.attached
    }

    /// The window was resized by the user to `width` × `height`: the editor follows,
    /// or answers with the nearest size it accepts.
    pub fn set_size(&self, width: i32, height: i32) -> (i32, i32) {
        let mut rect = ViewRect { left: 0, top: 0, right: width, bottom: height };
        unsafe {
            self.view.checkSizeConstraint(&mut rect);
            self.view.onSize(&mut rect);
        }
        (rect.right - rect.left, rect.bottom - rect.top)
    }
}

impl Drop for Vst3View {
    fn drop(&mut self) {
        unsafe {
            if self.attached {
                self.view.removed();
            }
            self.view.setFrame(std::ptr::null_mut());
        }
    }
}

const STATE_MAGIC: &[u8; 4] = b"PLS1";

/// Splits a state from [`PluginHandle::state`] into its component and controller parts.
fn split_state(state: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = state.strip_prefix(STATE_MAGIC)?;
    let take = |bytes: &[u8]| -> Option<(usize, usize)> {
        let len = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        (bytes.len() >= 4 + len).then_some((4, 4 + len))
    };
    let (a0, a1) = take(rest)?;
    let (component, rest) = (&rest[a0..a1], &rest[a1..]);
    let (b0, b1) = take(rest)?;
    Some((component, &rest[b0..b1]))
}

/// The audio-thread half of a VST3 instance.
struct Vst3Processor {
    instance: Arc<Instance>,
    params: Option<Consumer<(u32, f64)>>,
    events: ComWrapper<EventList>,
    events_ptr: ComPtr<IEventList>,
    changes: ComWrapper<ParamChanges>,
    changes_ptr: ComPtr<IParameterChanges>,
    /// Channel buffers of every bus, inputs then outputs.
    buffers: Vec<Vec<f32>>,
    /// The channel pointers the buses point into.
    _pointers: Vec<*mut f32>,
    inputs: Vec<AudioBusBuffers>,
    outputs: Vec<AudioBusBuffers>,
    /// (first buffer, channels) of each input and output bus.
    input_layout: Vec<(usize, usize)>,
    output_layout: Vec<(usize, usize)>,
    context: ProcessContext,
    pending: Vec<(u8, u32)>,
    held: [bool; 128],
    /// The parameter each MIDI controller drives (CC 0..127, aftertouch, pitch bend),
    /// as the plugin's MIDI mapping says.
    midi_map: Box<[Option<ParamID>; MIDI_CONTROLLERS]>,
    /// Controller values waiting for the next block.
    controls: Vec<(ParamID, f64)>,
    sample_rate: f32,
}

unsafe impl Send for Vst3Processor {}

impl Drop for Vst3Processor {
    fn drop(&mut self) {
        *self.instance.idle.lock().unwrap() = self.params.take();
    }
}

impl Vst3Processor {
    pub fn set_tempo(&mut self, tempo: f64) {
        self.context.tempo = tempo;
    }

    /// Where the song is at this block's start, and whether it plays.
    pub fn set_transport(&mut self, tempo: f64, beat: f64, playing: bool) {
        use ProcessContext_::StatesAndFlags_::{kBarPositionValid, kPlaying};
        self.context.tempo = tempo;
        self.context.projectTimeMusic = beat;
        self.context.barPositionMusic = (beat / 4.0).floor() * 4.0;
        self.context.projectTimeSamples = (beat * 60.0 / tempo.max(1.0) * self.sample_rate as f64).round() as i64;
        self.context.state |= kBarPositionValid as u32;
        if playing {
            self.context.state |= kPlaying as u32;
        } else {
            self.context.state &= !(kPlaying as u32);
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        self.note(true, pitch, velocity, 0);
        if gate != HELD && self.pending.len() < MAX_PENDING {
            self.pending.push((pitch, gate));
        }
    }

    /// Sets parameter `id` (normalized) from the next block on: automation.
    pub fn set_normalized(&mut self, id: u32, value: f64) {
        let value = value.clamp(0.0, 1.0);
        if let Some(slot) = self.controls.iter_mut().find(|(p, _)| *p == id) {
            slot.1 = value;
        } else if self.controls.len() < self.controls.capacity() {
            self.controls.push((id, value));
        }
    }

    /// A MIDI controller moved: CC, pitch bend (-1..1) or pressure (0..1).
    pub fn control(&mut self, controller: Controller, pitch: u8, value: f32) {
        let (number, normalized) = match controller {
            Controller::Cc(n) => (n.min(127) as usize, value as f64),
            Controller::Pressure => (ControllerNumbers_::kAfterTouch as usize, value as f64),
            Controller::PitchBend => (ControllerNumbers_::kPitchBend as usize, (value as f64 + 1.0) * 0.5),
            Controller::PolyPressure => {
                let mut event: Event = unsafe { std::mem::zeroed() };
                event.r#type = Event_::EventTypes_::kPolyPressureEvent as u16;
                event.__field0.polyPressure = PolyPressureEvent { channel: 0, pitch: pitch.min(127) as i16, pressure: value.clamp(0.0, 1.0), noteId: -1 };
                let list = self.events.list();
                if list.len() < MAX_EVENTS {
                    list.push(event);
                }
                return;
            }
        };
        let Some(id) = self.midi_map[number] else { return };
        self.set_normalized(id, normalized);
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

    fn note(&mut self, on: bool, pitch: u8, velocity: f32, offset: i32) {
        let pitch = pitch.min(127);
        if !on && !self.held[pitch as usize] {
            return;
        }
        self.held[pitch as usize] = on;
        let mut event: Event = unsafe { std::mem::zeroed() };
        event.sampleOffset = offset;
        if on {
            event.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
            event.__field0.noteOn = NoteOnEvent { channel: 0, pitch: pitch as i16, tuning: 0.0, velocity: velocity.clamp(0.0, 1.0), length: 0, noteId: -1 };
        } else {
            event.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
            event.__field0.noteOff = NoteOffEvent { channel: 0, pitch: pitch as i16, velocity: 0.0, noteId: -1, tuning: 0.0 };
        }
        let list = self.events.list();
        if list.len() < MAX_EVENTS {
            list.push(event);
        }
    }

    /// Adds an instrument's output to `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(MAX_BLOCK);
            self.run(n);
            if let Some(&(first, channels)) = self.output_layout.first() {
                let (l, r) = (first, first + if channels > 1 { 1 } else { 0 });
                for i in 0..n {
                    left[offset + i] += self.buffers[l][i];
                    right[offset + i] += self.buffers[r][i];
                }
            }
            offset += n;
        }
    }

    /// Runs an effect over `left`/`right` in place.
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

    /// One call to the plugin's `process` for `n` frames.
    fn run(&mut self, n: usize) {
        self.changes.clear();
        if let Some(params) = &mut self.params {
            while let Ok((id, value)) = params.pop() {
                self.changes.set(id, value);
            }
        }
        for (id, value) in self.controls.drain(..) {
            self.changes.set(id, value);
        }
        // Notes whose length ends in this block.
        let mut k = 0;
        while k < self.pending.len() {
            let (pitch, left) = self.pending[k];
            if (left as usize) <= n {
                self.pending.swap_remove(k);
                self.note(false, pitch, 0.0, left.saturating_sub(1) as i32);
            } else {
                self.pending[k].1 = left - n as u32;
                k += 1;
            }
        }
        for &(first, channels) in &self.output_layout {
            for b in &mut self.buffers[first..first + channels] {
                b[..n].fill(0.0);
            }
        }
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: self.inputs.len() as int32,
            numOutputs: self.outputs.len() as int32,
            inputs: if self.inputs.is_empty() { std::ptr::null_mut() } else { self.inputs.as_mut_ptr() },
            outputs: if self.outputs.is_empty() { std::ptr::null_mut() } else { self.outputs.as_mut_ptr() },
            inputParameterChanges: self.changes_ptr.as_ptr(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: self.events_ptr.as_ptr(),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut self.context,
        };
        unsafe { self.instance.processor.process(&mut data) };
        self.events.list().clear();
        // Where the song is comes from the host each block (`set_transport`).
        self.context.continousTimeSamples += n as i64;
    }
}

/// Loads VST3 plugin `cid` from `bundle` (see [`instantiate`]): its processor and handle,
/// and whether it is an instrument.
fn vst3_instantiate(bundle: &Path, cid: &str, sample_rate: f32, state: Option<&[u8]>) -> Result<(Vst3Processor, Vst3Handle, bool), String> {
    let module = module(bundle)?;
    let class = parse_cid(cid).ok_or_else(|| format!("Identificador de plugin no válido: {cid}"))?;
    let name = vst3_scan(bundle).ok().and_then(|list| list.into_iter().find(|p| p.cid == cid)).map_or_else(|| "Plugin".to_string(), |p| p.name);
    let host = ComWrapper::new(HostApplication);
    let host_ptr = host.to_com_ptr::<FUnknown>().ok_or("host")?;
    unsafe {
        let mut obj = std::ptr::null_mut();
        if module.factory.createInstance(class.as_ptr(), IComponent::IID.as_ptr() as FIDString, &mut obj) != kResultOk || obj.is_null() {
            return Err(format!("{name}: no se pudo crear el componente"));
        }
        let component = ComPtr::<IComponent>::from_raw(obj as *mut IComponent).ok_or("component")?;
        if component.initialize(host_ptr.as_ptr()) != kResultOk {
            return Err(format!("{name}: no se pudo inicializar"));
        }
        let processor = component.cast::<IAudioProcessor>().ok_or_else(|| format!("{name}: no procesa audio"))?;
        // The controller: the component itself, or a class of its own.
        let (controller, separate) = match component.cast::<IEditController>() {
            Some(c) => (Some(c), false),
            None => {
                let mut controller_cid: TUID = [0; 16];
                let mut controller = None;
                if component.getControllerClassId(&mut controller_cid) == kResultOk && controller_cid != [0; 16] {
                    let mut obj = std::ptr::null_mut();
                    if module.factory.createInstance(controller_cid.as_ptr(), IEditController::IID.as_ptr() as FIDString, &mut obj) == kResultOk && !obj.is_null() {
                        let c = ComPtr::<IEditController>::from_raw(obj as *mut IEditController).ok_or("controller")?;
                        if c.initialize(host_ptr.as_ptr()) == kResultOk {
                            controller = Some(c);
                        }
                    }
                }
                (controller, true)
            }
        };
        let connected = match (&controller, separate) {
            (Some(c), true) => match (component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                (Some(a), Some(b)) => {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                    Some((a, b))
                }
                _ => None,
            },
            _ => None,
        };
        // Which parameter each MIDI controller drives, asked once here (the controller
        // belongs to the UI thread).
        let mut midi_map = Box::new([None; MIDI_CONTROLLERS]);
        if let Some(mapping) = controller.as_ref().and_then(|c| c.cast::<IMidiMapping>()) {
            for (number, slot) in midi_map.iter_mut().enumerate() {
                let mut id: ParamID = 0;
                if mapping.getMidiControllerAssignment(0, 0, number as CtrlNumber, &mut id) == kResultTrue {
                    *slot = Some(id);
                }
            }
        }
        let (producer, consumer) = RingBuffer::new(1024);
        let queue: ParamQueue = Arc::new(Mutex::new(producer));
        let edited = Arc::new(AtomicBool::new(false));
        let handler = controller.as_ref().map(|c| {
            let handler = ComWrapper::new(ComponentHandler { queue: queue.clone(), edited: edited.clone() });
            if let Some(ptr) = handler.to_com_ptr::<IComponentHandler>() {
                c.setComponentHandler(ptr.as_ptr());
            }
            handler
        });

        // Buses: the main audio input and output in stereo, the first event input; the
        // rest stay off but still get buffers.
        let media = |m: MediaTypes| m as MediaType;
        let (audio, event) = (media(MediaTypes_::kAudio), media(MediaTypes_::kEvent));
        let (input, output) = (BusDirections_::kInput as BusDirection, BusDirections_::kOutput as BusDirection);
        let n_in = component.getBusCount(audio, input).max(0) as usize;
        let n_out = component.getBusCount(audio, output).max(0) as usize;
        let n_events = component.getBusCount(event, input).max(0) as usize;
        for i in 0..n_in {
            component.activateBus(audio, input, i as i32, (i == 0) as TBool);
        }
        for i in 0..n_out {
            component.activateBus(audio, output, i as i32, (i == 0) as TBool);
        }
        for i in 0..n_events {
            component.activateBus(event, input, i as i32, (i == 0) as TBool);
        }
        let arrangement = |dir: BusDirection, i: usize| {
            let mut arr: SpeakerArrangement = 0;
            if processor.getBusArrangement(dir, i as i32, &mut arr) != kResultOk {
                arr = SpeakerArr::kStereo;
            }
            arr
        };
        let mut ins: Vec<SpeakerArrangement> = (0..n_in).map(|i| arrangement(input, i)).collect();
        let mut outs: Vec<SpeakerArrangement> = (0..n_out).map(|i| arrangement(output, i)).collect();
        let (mut want_in, mut want_out) = (ins.clone(), outs.clone());
        if let Some(first) = want_in.first_mut() {
            *first = SpeakerArr::kStereo;
        }
        if let Some(first) = want_out.first_mut() {
            *first = SpeakerArr::kStereo;
        }
        if processor.setBusArrangements(want_in.as_mut_ptr(), n_in as i32, want_out.as_mut_ptr(), n_out as i32) == kResultTrue {
            (ins, outs) = (want_in, want_out);
        } else {
            ins = (0..n_in).map(|i| arrangement(input, i)).collect();
            outs = (0..n_out).map(|i| arrangement(output, i)).collect();
        }
        let channels = |arr: SpeakerArrangement| (arr as u64).count_ones() as usize;
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: MAX_BLOCK as int32,
            sampleRate: sample_rate as f64,
        };
        if processor.setupProcessing(&mut setup) != kResultOk {
            return Err(format!("{name}: no acepta {sample_rate} Hz"));
        }

        if let Some((a, b)) = state.and_then(split_state) {
            let stream = Stream::with(a);
            if let Some(ptr) = stream.to_com_ptr::<IBStream>() {
                component.setState(ptr.as_ptr());
            }
            if let Some(c) = &controller {
                let stream = Stream::with(a);
                if let Some(ptr) = stream.to_com_ptr::<IBStream>() {
                    c.setComponentState(ptr.as_ptr());
                }
                let stream = Stream::with(b);
                if let Some(ptr) = stream.to_com_ptr::<IBStream>() {
                    c.setState(ptr.as_ptr());
                }
            }
        } else if let Some(c) = &controller {
            // A fresh instance: the controller learns the processor's defaults.
            let stream = ComWrapper::new(Stream::default());
            if let Some(ptr) = stream.to_com_ptr::<IBStream>() {
                if component.getState(ptr.as_ptr()) == kResultOk {
                    let copy = Stream::with(&stream.bytes());
                    if let Some(ptr) = copy.to_com_ptr::<IBStream>() {
                        c.setComponentState(ptr.as_ptr());
                    }
                }
            }
        }
        component.setActive(1);
        processor.setProcessing(1);

        // Buffers for every bus.
        let mut buffers = Vec::new();
        let mut layout = |arrs: &[SpeakerArrangement]| -> Vec<(usize, usize)> {
            arrs.iter()
                .map(|&arr| {
                    let first = buffers.len();
                    let count = channels(arr).max(1);
                    buffers.extend((0..count).map(|_| vec![0f32; MAX_BLOCK]));
                    (first, count)
                })
                .collect()
        };
        let input_layout = layout(&ins);
        let output_layout = layout(&outs);
        let mut pointers: Vec<*mut f32> = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let bus = |pointers: &mut Vec<*mut f32>, &(first, count): &(usize, usize)| AudioBusBuffers {
            numChannels: count as int32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 { channelBuffers32: pointers.as_mut_ptr().add(first) },
        };
        let inputs = input_layout.iter().map(|l| bus(&mut pointers, l)).collect();
        let outputs = output_layout.iter().map(|l| bus(&mut pointers, l)).collect();

        let mut context: ProcessContext = std::mem::zeroed();
        context.sampleRate = sample_rate as f64;
        context.tempo = 120.0;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        context.state = (ProcessContext_::StatesAndFlags_::kTempoValid | ProcessContext_::StatesAndFlags_::kTimeSigValid | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid) as u32;

        let events = ComWrapper::new(EventList::new());
        let events_ptr = events.to_com_ptr::<IEventList>().ok_or("events")?;
        let changes = ComWrapper::new(ParamChanges::new());
        let changes_ptr = changes.to_com_ptr::<IParameterChanges>().ok_or("changes")?;
        let instance = Arc::new(Instance { component, processor, controller, separate, connected, _module: module, _host: host, _handler: handler, idle: Mutex::new(None), edited });
        let processor = Vst3Processor {
            instance: instance.clone(),
            params: Some(consumer),
            events,
            events_ptr,
            changes,
            changes_ptr,
            buffers,
            _pointers: pointers,
            inputs,
            outputs,
            input_layout,
            output_layout,
            context,
            pending: Vec::with_capacity(MAX_PENDING),
            held: [false; 128],
            midi_map,
            controls: Vec::with_capacity(MAX_CHANGES),
            sample_rate,
        };
        Ok((processor, Vst3Handle { instance, queue }, n_events > 0))
    }
}


// ----- The common interface: VST3 or CLAP ------------------------------------------------

/// The plugins in a `.vst3` or `.clap` bundle (loads it).
pub fn scan(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    if is_clap(bundle) { crate::clap_host::scan(bundle) } else { vst3_scan(bundle) }
}

enum Processor {
    Vst3(Vst3Processor),
    Clap(crate::clap_host::ClapProcessor),
}

/// The audio-thread half of a plugin instance: an instrument or an effect.
pub struct PluginProcessor {
    inner: Processor,
    pub instrument: bool,
}

macro_rules! each {
    ($inner:expr, $p:ident => $call:expr) => {
        match $inner {
            Processor::Vst3($p) => $call,
            Processor::Clap($p) => $call,
        }
    };
}

impl PluginProcessor {
    pub fn set_tempo(&mut self, tempo: f64) {
        each!(&mut self.inner, p => p.set_tempo(tempo))
    }

    /// The tempo, where the song is at the block's start and whether it plays:
    /// what tempo-synced plugins (delays, LFOs, sequencers) follow.
    pub fn set_transport(&mut self, tempo: f64, beat: f64, playing: bool) {
        each!(&mut self.inner, p => p.set_transport(tempo, beat, playing))
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        each!(&mut self.inner, p => p.note_on(pitch, velocity, gate))
    }

    pub fn note_off(&mut self, pitch: u8) {
        each!(&mut self.inner, p => p.note_off(pitch))
    }

    pub fn all_notes_off(&mut self) {
        each!(&mut self.inner, p => p.all_notes_off())
    }

    /// A MIDI controller moved: CC, pitch bend (-1..1) or pressure (0..1).
    pub fn control(&mut self, controller: Controller, pitch: u8, value: f32) {
        each!(&mut self.inner, p => p.control(controller, pitch, value))
    }

    /// Sets parameter `id` (normalized) from the next block on: automation.
    pub fn set_normalized(&mut self, id: u32, value: f64) {
        each!(&mut self.inner, p => p.set_normalized(id, value))
    }

    /// Adds an instrument's output to `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        each!(&mut self.inner, p => p.render(left, right))
    }

    /// Runs an effect over `left`/`right` in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        each!(&mut self.inner, p => p.process(left, right))
    }

    /// Whether controller `number` (CC 0..127, then aftertouch and pitch bend) reaches
    /// the plugin.
    pub fn takes_controller(&self, number: usize) -> bool {
        match &self.inner {
            Processor::Vst3(p) => p.midi_map.get(number).is_some_and(Option::is_some),
            Processor::Clap(p) => p.takes_midi(),
        }
    }
}

enum Handle {
    Vst3(Vst3Handle),
    Clap(crate::clap_host::ClapHandle),
}

/// The UI-thread half of a plugin instance.
pub struct PluginHandle {
    inner: Handle,
    pub name: String,
    /// The parameters, read once (plugins can have thousands), and their index by id.
    params: OnceLock<(Vec<PluginParam>, HashMap<u32, usize>)>,
}

macro_rules! handle {
    ($inner:expr, $h:ident => $call:expr) => {
        match $inner {
            Handle::Vst3($h) => $call,
            Handle::Clap($h) => $call,
        }
    };
}

impl PluginHandle {
    /// The parameters the host may set, in the plugin's order.
    pub fn params(&self) -> &[PluginParam] {
        &self.cached().0
    }

    /// Parameter `id`, if the host may set it.
    pub fn param(&self, id: u32) -> Option<&PluginParam> {
        let (params, index) = self.cached();
        index.get(&id).map(|&i| &params[i])
    }

    fn cached(&self) -> &(Vec<PluginParam>, HashMap<u32, usize>) {
        self.params.get_or_init(|| {
            let params = handle!(&self.inner, h => h.read_params());
            let index = params.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
            (params, index)
        })
    }

    /// The normalized (0..1) value of parameter `id`.
    pub fn value(&self, id: u32) -> f64 {
        handle!(&self.inner, h => h.value(id))
    }

    /// How the plugin shows `value` (normalized) of parameter `id`, e.g. "-6.0 dB".
    pub fn display(&self, id: u32, value: f64) -> String {
        handle!(&self.inner, h => h.display(id, value))
    }

    /// Sets parameter `id` (normalized); the processor gets it at its next block.
    pub fn set(&self, id: u32, value: f64) {
        handle!(&self.inner, h => h.set(id, value))
    }

    /// Shows `value` for parameter `id` in the plugin's own editor, without sending it
    /// to the processor (automation reaches the processor from the engine).
    pub fn show(&self, id: u32, value: f64) {
        handle!(&self.inner, h => h.show(id, value))
    }

    /// The plugin's whole state, to restore with [`instantiate`].
    pub fn state(&self) -> Vec<u8> {
        handle!(&self.inner, h => h.state())
    }

    /// Whether the plugin changed since the last call: its editor moved a
    /// parameter, it loaded a preset, or the host set one. Changes like these
    /// live inside the plugin, so they are the project's unsaved changes.
    pub fn take_edited(&self) -> bool {
        handle!(&self.inner, h => h.take_edited())
    }

    /// Once a frame, on the UI thread: lets the plugin do what it asked to do there.
    pub fn idle(&self) {
        if let Handle::Clap(h) = &self.inner {
            h.idle();
        }
    }

    /// The plugin's own editor, if it has one. The host puts it in a window of its
    /// own (see [`PluginView::attach`]). UI thread only.
    pub fn editor(&self) -> Option<PluginView> {
        Some(match &self.inner {
            Handle::Vst3(h) => PluginView(View::Vst3(h.editor()?)),
            Handle::Clap(h) => PluginView(View::Clap(h.editor()?)),
        })
    }
}

enum View {
    Vst3(Vst3View),
    Clap(crate::clap_host::ClapView),
}

/// A plugin's editor. Dropping it takes it out of its window.
pub struct PluginView(View);

macro_rules! view {
    ($inner:expr, $v:ident => $call:expr) => {
        match $inner {
            View::Vst3($v) => $call,
            View::Clap($v) => $call,
        }
    };
}

impl PluginView {
    /// Whether it can show itself in a native view of `platform` (see [`PLATFORM_VIEW`]).
    pub fn supports(&self, platform: &str) -> bool {
        view!(&self.0, v => v.supports(platform))
    }

    /// Its size in points.
    pub fn size(&self) -> (i32, i32) {
        view!(&self.0, v => v.size())
    }

    pub fn can_resize(&self) -> bool {
        view!(&self.0, v => v.can_resize())
    }

    /// How many pixels make a point on the screen the editor is on (Windows, where
    /// editors work in pixels and the host says the scale). False if it doesn't listen.
    pub fn set_scale(&self, scale: f64) -> bool {
        view!(&self.0, v => v.set_scale(scale))
    }

    /// Called with the size the editor asks for; the window must take it.
    pub fn on_resize(&self, resize: impl Fn(i32, i32) + 'static) {
        view!(&self.0, v => v.on_resize(resize))
    }

    /// Puts the editor in `parent`, a native view of [`PLATFORM_VIEW`].
    ///
    /// # Safety
    /// `parent` must be a live view of that kind, and outlive this editor.
    pub unsafe fn attach(&mut self, parent: *mut c_void) -> bool {
        view!(&mut self.0, v => unsafe { v.attach(parent) })
    }

    /// The window was resized by the user to `width` × `height`: the editor follows,
    /// or answers with the nearest size it accepts.
    pub fn set_size(&self, width: i32, height: i32) -> (i32, i32) {
        view!(&self.0, v => v.set_size(width, height))
    }
}

/// Loads plugin `cid` (a VST3 class id or a CLAP plugin id) from `bundle` and starts it
/// at `sample_rate`, restoring `state` if given (from [`PluginHandle::state`]). Call on
/// the UI thread.
pub fn instantiate(bundle: &Path, cid: &str, sample_rate: f32, state: Option<&[u8]>) -> Result<(PluginProcessor, PluginHandle), String> {
    let name = scan(bundle).ok().and_then(|list| list.into_iter().find(|p| p.cid == cid)).map_or_else(|| "Plugin".to_string(), |p| p.name);
    let (processor, handle, instrument) = if is_clap(bundle) {
        let (p, h, instrument) = crate::clap_host::instantiate(bundle, cid, sample_rate, state)?;
        (Processor::Clap(p), Handle::Clap(h), instrument)
    } else {
        let (p, h, instrument) = vst3_instantiate(bundle, cid, sample_rate, state)?;
        (Processor::Vst3(p), Handle::Vst3(h), instrument)
    };
    Ok((PluginProcessor { inner: processor, instrument }, PluginHandle { inner: handle, name, params: OnceLock::new() }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A folder holding a link to the folder above it: the search ends.
    #[cfg(unix)]
    #[test]
    fn searching_ends_with_a_link_going_round() {
        let root = std::env::temp_dir().join(format!("pulse-plugin-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/Synth.vst3")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("a/vuelta")).unwrap();
        let found = bundles(&root);
        assert!(found.contains(&root.join("a/Synth.vst3")));
        assert!(found.len() <= MAX_DEPTH + 1, "{}", found.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Builds the test plugin (crates/testplug) and wraps it in a `.vst3` bundle.
    pub(crate) fn test_bundle() -> PathBuf {
        static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
        BUNDLE
            .get_or_init(|| {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let target = root.join("target/testplug");
                let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
                let status = std::process::Command::new(cargo)
                    .args(["build", "--release", "-p", "pulse-testplug", "--target-dir"])
                    .arg(&target)
                    .current_dir(&root)
                    .status()
                    .expect("cargo build");
                assert!(status.success(), "the test plugin builds");
                let library = target.join("release").join(if cfg!(target_os = "macos") { "libpulse_testplug.dylib" } else if cfg!(target_os = "windows") { "pulse_testplug.dll" } else { "libpulse_testplug.so" });
                let bundle = target.join("PulseTest.vst3");
                let binary = binary(&bundle);
                std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
                std::fs::copy(&library, &binary).unwrap();
                bundle
            })
            .clone()
    }

    fn synth() -> PluginInfo {
        scan(&test_bundle()).unwrap().into_iter().find(|p| p.instrument).unwrap()
    }

    /// The same test plugin as a `.clap` bundle.
    pub(crate) fn clap_bundle() -> PathBuf {
        static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
        BUNDLE
            .get_or_init(|| {
                let vst3 = test_bundle();
                let bundle = vst3.with_file_name("PulseTest.clap");
                let binary = binary(&bundle);
                std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
                std::fs::copy(super::binary(&vst3), &binary).unwrap();
                bundle
            })
            .clone()
    }

    #[test]
    fn clap_plugins_scan_play_and_keep_their_state() {
        let found = scan(&clap_bundle()).unwrap();
        let names: Vec<(&str, &str, bool, &str)> = found.iter().map(|p| (p.name.as_str(), p.cid.as_str(), p.instrument, p.category.as_str())).collect();
        assert_eq!(names, [("Pulse Test Synth", "org.pulse.test-synth", true, "instrument|synthesizer"), ("Pulse Test Gain", "org.pulse.test-gain", false, "audio-effect|utility")]);
        let synth = &found[0];
        let (mut p, handle) = instantiate(&synth.path, &synth.cid, 48_000.0, None).unwrap();
        assert!(p.instrument);
        assert_eq!(handle.name, "Pulse Test Synth");
        // Plain ranges, seen as normalized values.
        let params: Vec<(&str, i32, f64)> = handle.params().iter().map(|p| (p.name.as_str(), p.steps, p.default)).collect();
        assert_eq!(params, [("Level", 0, 0.5), ("Octave", 2, 0.5), ("Bend", 0, 0.5)]);
        assert_eq!(handle.display(1, 1.0), "+1");
        assert_eq!(handle.display(2, 0.75), "+1.0 st");
        // A note with its length.
        p.note_on(69, 1.0, 24_000);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        let mut at = 0;
        for size in [100, 2000, 333, 21567, 24_000] {
            p.render(&mut l[at..at + size], &mut r[at..at + size]);
            at += size;
        }
        assert!((frequency(&l[..24_000], 48_000.0) - 440.0).abs() < 2.0, "{}", frequency(&l[..24_000], 48_000.0));
        assert!((rms(&l[1000..24_000]) - 0.5 / 2f32.sqrt()).abs() < 0.01);
        assert_eq!(rms(&l[24_000..]), 0.0, "the note ends after its length");
        // Parameters: level a quarter, an octave up (stepped), then saved and restored.
        handle.set(0, 0.25);
        handle.set(1, 0.9);
        p.note_on(57, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 440.0).abs() < 2.0, "an octave up: {}", frequency(&l, 48_000.0));
        assert!((rms(&l[1000..]) - 0.25 / 2f32.sqrt()).abs() < 0.01);
        assert_eq!((handle.value(0), handle.value(1)), (0.25, 1.0), "the stepped octave snapped to +1");
        // Pitch bend and the wheel, as MIDI.
        assert!(p.takes_controller(1) && p.takes_controller(129));
        p.control(Controller::PitchBend, 0, 1.0);
        p.control(Controller::Cc(1), 0, 1.0);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 493.9).abs() < 2.0, "bent to B4: {}", frequency(&l, 48_000.0));
        assert!((rms(&l[1000..]) - 1.0 / 2f32.sqrt()).abs() < 0.01, "full level from the wheel");
        p.all_notes_off();
        let state = handle.state();
        drop((p, handle));
        let (_, handle) = instantiate(&synth.path, &synth.cid, 48_000.0, Some(&state)).unwrap();
        assert_eq!((handle.value(0), handle.value(1)), (1.0, 1.0));
        // MIDI pitch bend tops out at 8191/8192.
        assert!((handle.value(2) - 1.0).abs() < 1e-3, "{}", handle.value(2));
    }

    #[test]
    fn clap_effects_and_idle_parameter_changes() {
        let gain = scan(&clap_bundle()).unwrap().into_iter().find(|p| !p.instrument).unwrap();
        let (mut p, handle) = instantiate(&gain.path, &gain.cid, 44_100.0, None).unwrap();
        let mut l: Vec<f32> = (0..3000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = l.clone();
        let original = l.clone();
        p.process(&mut l, &mut r);
        assert_eq!(l, original, "1× at the default (0.5 of 0..2)");
        handle.set(0, 0.25);
        p.set_normalized(0, 0.25);
        p.process(&mut l, &mut r);
        assert_eq!(l, original.iter().map(|x| x * 0.5).collect::<Vec<_>>());
        assert_eq!(handle.display(0, 0.25), "0.50x");
        // Without a processor (no audio running), changes reach the plugin by a flush.
        drop(p);
        handle.set(0, 1.0);
        assert_eq!(handle.value(0), 1.0);
        assert_eq!(handle.state().len(), 8);
        assert!(handle.editor().is_none(), "the test plugins have no editor");
    }

    fn frequency(signal: &[f32], sr: f32) -> f32 {
        signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * sr / signal.len() as f32
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn scanning_finds_the_instrument_and_the_effect() {
        let found = scan(&test_bundle()).unwrap();
        let names: Vec<(&str, bool, &str)> = found.iter().map(|p| (p.name.as_str(), p.instrument, p.category.as_str())).collect();
        assert_eq!(names, [("Pulse Test Synth", true, "Instrument|Synth"), ("Pulse Test Gain", false, "Fx|Dynamics")]);
        assert_eq!(found[0].vendor, "Pulse");
        let folder = test_bundle().parent().unwrap().to_path_buf();
        assert!(bundles(&folder).contains(&test_bundle()));
    }

    #[test]
    fn an_instrument_plays_notes_with_their_lengths() {
        let info = synth();
        let (mut p, handle) = instantiate(&info.path, &info.cid, 48_000.0, None).unwrap();
        assert!(p.instrument);
        assert_eq!(handle.name, "Pulse Test Synth");
        p.note_on(69, 1.0, 24_000);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        // In odd-sized pieces, as the engine renders between events.
        let mut at = 0;
        for size in [100, 2000, 333, 21567, 24_000] {
            p.render(&mut l[at..at + size], &mut r[at..at + size]);
            at += size;
        }
        assert!((frequency(&l[..24_000], 48_000.0) - 440.0).abs() < 2.0, "{}", frequency(&l[..24_000], 48_000.0));
        assert!((rms(&l[1000..24_000]) - 0.5 / 2f32.sqrt()).abs() < 0.01, "level 50 %: {}", rms(&l[1000..24_000]));
        assert_eq!(rms(&l[24_000..]), 0.0, "the note ends after its length");
        assert_eq!(l, r);
    }

    #[test]
    fn parameters_reach_the_processor_and_the_state_round_trips() {
        let info = synth();
        let (mut p, handle) = instantiate(&info.path, &info.cid, 48_000.0, None).unwrap();
        let params = handle.params();
        assert_eq!(params.iter().map(|p| (p.name.as_str(), p.steps)).collect::<Vec<_>>(), [("Level", 0), ("Octave", 2), ("Bend", 0)]);
        assert_eq!(handle.value(0), 0.5);
        handle.set(0, 0.25);
        handle.set(1, 1.0);
        assert_eq!(handle.display(1, 1.0), "+1");
        p.note_on(57, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 440.0).abs() < 2.0, "an octave up: {}", frequency(&l, 48_000.0));
        assert!((rms(&l[1000..]) - 0.25 / 2f32.sqrt()).abs() < 0.01);
        p.all_notes_off();
        let state = handle.state();
        drop((p, handle));

        // A new instance from the state plays the same.
        let (mut p, handle) = instantiate(&info.path, &info.cid, 48_000.0, Some(&state)).unwrap();
        assert_eq!((handle.value(0), handle.value(1)), (0.25, 1.0), "the controller has the state too");
        p.note_on(57, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 440.0).abs() < 2.0);
        assert!((rms(&l[1000..]) - 0.25 / 2f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn pitch_bend_and_controllers_reach_the_parameters_they_are_mapped_to() {
        let info = synth();
        let (mut p, _handle) = instantiate(&info.path, &info.cid, 48_000.0, None).unwrap();
        p.note_on(69, 1.0, HELD);
        // Pitch bend all the way up: a whole tone; the wheel at a quarter: level 25 %.
        p.control(Controller::PitchBend, 0, 1.0);
        p.control(Controller::Cc(1), 0, 0.25);
        // Unmapped controllers are ignored.
        p.control(Controller::Cc(74), 0, 1.0);
        p.control(Controller::Pressure, 0, 1.0);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 493.9).abs() < 2.0, "bent to B4: {}", frequency(&l, 48_000.0));
        assert!((rms(&l[1000..]) - 0.25 / 2f32.sqrt()).abs() < 0.01, "{}", rms(&l[1000..]));
        // Back to the middle.
        p.control(Controller::PitchBend, 0, 0.0);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        p.render(&mut l, &mut r);
        assert!((frequency(&l, 48_000.0) - 440.0).abs() < 2.0, "{}", frequency(&l, 48_000.0));
    }

    #[test]
    fn an_effect_processes_in_place() {
        let info = scan(&test_bundle()).unwrap().into_iter().find(|p| !p.instrument).unwrap();
        let (mut p, handle) = instantiate(&info.path, &info.cid, 44_100.0, None).unwrap();
        assert!(!p.instrument);
        let mut l: Vec<f32> = (0..3000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = l.clone();
        let original = l.clone();
        p.process(&mut l, &mut r);
        assert_eq!(l, original, "gain 1× at the default");
        handle.set(0, 0.25);
        p.process(&mut l, &mut r);
        let expected: Vec<f32> = original.iter().map(|x| x * 0.5).collect();
        assert_eq!(l, expected);
        assert_eq!(handle.display(0, 0.25), "0.50x");
    }

    /// Plugins are told where the song is and whether it plays, each block,
    /// as the host says: stopped, they don't run on by themselves.
    #[test]
    fn plugins_get_the_transport() {
        use ProcessContext_::StatesAndFlags_::kPlaying;
        let info = scan(&test_bundle()).unwrap().into_iter().find(|p| !p.instrument).unwrap();
        let (mut p, _handle) = instantiate(&info.path, &info.cid, 48_000.0, None).unwrap();
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let context = |p: &PluginProcessor| match &p.inner {
            Processor::Vst3(v) => v.context,
            Processor::Clap(_) => unreachable!("the test plugin is VST3"),
        };
        p.set_transport(140.0, 17.5, true);
        p.process(&mut l, &mut r);
        let c = context(&p);
        assert_eq!((c.tempo, c.projectTimeMusic, c.barPositionMusic), (140.0, 17.5, 16.0));
        assert!(c.state & kPlaying as u32 != 0);
        p.set_transport(140.0, 4.0, false);
        p.process(&mut l, &mut r);
        p.process(&mut l, &mut r);
        let c = context(&p);
        assert_eq!(c.projectTimeMusic, 4.0, "stopped, it stays where the host says");
        assert!(c.state & kPlaying as u32 == 0);
    }

    /// Every plugin installed on this computer: loads, plays a chord (or processes a
    /// chord from the test synth), saves and restores its state. Writes the audio to
    /// `$PULSE_OUT` if set.
    #[test]
    #[ignore = "diagnostic: runs the plugins installed on this computer"]
    fn installed_plugins() {
        let sr = 48_000.0;
        let mut out = Vec::new();
        for bundle in default_folders().iter().flat_map(|f| bundles(f)) {
            let list = match scan(&bundle) {
                Ok(list) => list,
                Err(e) => {
                    println!("{}: {e}", bundle.display());
                    continue;
                }
            };
            for info in list {
                let start = std::time::Instant::now();
                let (mut p, handle) = match instantiate(&info.path, &info.cid, sr, None) {
                    Ok(x) => x,
                    Err(e) => {
                        println!("{}: {e}", info.name);
                        continue;
                    }
                };
                let loaded = start.elapsed();
                let (mut l, mut r) = (vec![0f32; 96_000], vec![0f32; 96_000]);
                if p.instrument {
                    for pitch in [60, 64, 67] {
                        p.note_on(pitch, 0.8, 48_000);
                    }
                    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                        p.render(a, b);
                    }
                } else {
                    let synth = scan(&test_bundle()).unwrap().into_iter().find(|p| p.instrument).unwrap();
                    let (mut source, _) = instantiate(&synth.path, &synth.cid, sr, None).unwrap();
                    for pitch in [60, 64, 67] {
                        source.note_on(pitch, 0.3, 48_000);
                    }
                    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                        source.render(a, b);
                        p.process(a, b);
                    }
                }
                let start = std::time::Instant::now();
                for (a, b) in l.chunks(512).zip(r.chunks(512)) {
                    let (mut a, mut b) = (a.to_vec(), b.to_vec());
                    if p.instrument {
                        p.render(&mut a, &mut b);
                    } else {
                        p.process(&mut a, &mut b);
                    }
                }
                let cpu = start.elapsed().as_secs_f64() / 2.0 * 100.0;
                let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
                let params = handle.params();
                let state = handle.state();
                let restored = instantiate(&info.path, &info.cid, sr, Some(&state)).map(|(_, h)| h.state().len());
                println!(
                    "{} [{}] ({}, {}): cargado en {:.0} ms, {} parámetros, pico {:.1} dBFS, {:.1} % de un núcleo, estado {} bytes, restaurado {:?}",
                    info.name,
                    if is_clap(&info.path) { "CLAP" } else { "VST3" },
                    info.vendor,
                    info.category,
                    loaded.as_secs_f64() * 1000.0,
                    params.len(),
                    20.0 * peak.max(1e-9).log10(),
                    cpu,
                    state.len(),
                    restored
                );
                assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{}", info.name);
                out.extend(l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]));
            }
        }
        if let Ok(path) = std::env::var("PULSE_OUT") {
            let mut bytes = Vec::new();
            let data = out.len() * 4;
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + data as u32).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&3u16.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&48_000u32.to_le_bytes());
            bytes.extend_from_slice(&(48_000u32 * 8).to_le_bytes());
            bytes.extend_from_slice(&8u16.to_le_bytes());
            bytes.extend_from_slice(&32u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&(data as u32).to_le_bytes());
            for s in out {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn plugins_without_an_editor_say_so() {
        let info = synth();
        let (_, handle) = instantiate(&info.path, &info.cid, 48_000.0, None).unwrap();
        assert!(handle.editor().is_none());
    }

    /// Pitch bend and the modulation wheel on the instruments installed on this computer.
    #[test]
    #[ignore = "diagnostic: runs the plugins installed on this computer"]
    fn installed_plugins_follow_controllers() {
        let sr = 48_000.0;
        for info in default_folders().iter().flat_map(|f| bundles(f)).filter_map(|b| scan(&b).ok()).flatten().filter(|p| p.instrument) {
            let (mut p, _handle) = instantiate(&info.path, &info.cid, sr, None).unwrap();
            let mapped: Vec<usize> = (0..MIDI_CONTROLLERS).filter(|&n| p.takes_controller(n)).collect();
            let pitch = |p: &mut PluginProcessor, bend: f32| {
                p.control(Controller::PitchBend, 0, bend);
                p.note_on(69, 0.8, HELD);
                let (mut l, mut r) = (vec![0f32; 48_000], vec![0f32; 48_000]);
                for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                    p.render(a, b);
                }
                p.note_off(69);
                let (mut tail_l, mut tail_r) = (vec![0f32; 48_000], vec![0f32; 48_000]);
                p.render(&mut tail_l, &mut tail_r);
                frequency(&l[12_000..], sr)
            };
            let (flat, bent) = (pitch(&mut p, 0.0), pitch(&mut p, 1.0));
            println!(
                "{} [{}]: {} controladores (rueda CC1: {}, pitch bend: {}, aftertouch: {}); La4 {flat:.1} Hz, con el bend arriba {bent:.1} Hz",
                info.name,
                if is_clap(&info.path) { "CLAP" } else { "VST3" },
                mapped.len(),
                p.takes_controller(1),
                p.takes_controller(ControllerNumbers_::kPitchBend as usize),
                p.takes_controller(ControllerNumbers_::kAfterTouch as usize)
            );
        }
    }

    #[test]
    fn missing_plugins_fail_cleanly() {
        assert!(scan(Path::new("/nowhere/Nothing.vst3")).is_err());
        let info = synth();
        assert!(instantiate(&info.path, "00000000000000000000000000000000", 48_000.0, None).is_err());
        assert!(split_state(b"junk").is_none());
    }
}
