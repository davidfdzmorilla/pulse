//! macOS: the output device's audio work group, which the threads that help
//! the audio thread join (see [`crate::workers`]). The system then plans them
//! with the device's own thread, against its deadline, instead of as threads
//! of their own: a busy machine is less likely to hold one of them back when
//! a block is waiting for it.
//!
//! Elsewhere, and if anything here fails, the threads simply don't join one.

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Duration;

type AudioObjectId = u32;

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

const SYSTEM_OBJECT: AudioObjectId = 1;
const SCOPE_GLOBAL: u32 = code(b"glob");
const ELEMENT_MAIN: u32 = 0;
/// `kAudioHardwarePropertyTranslateUIDToDevice`.
const UID_TO_DEVICE: u32 = code(b"uidd");
/// `kAudioDevicePropertyIOThreadOSWorkgroup` (macOS 11 and later).
const IO_WORKGROUP: u32 = code(b"oswg");
/// `kAudioDevicePropertyBufferFrameSize`.
const BUFFER_FRAMES: u32 = code(b"fsiz");
const UTF8: u32 = 0x0800_0100;

/// `os_workgroup_join_token_s` (64-bit).
#[repr(C)]
struct JoinToken {
    sig: u32,
    opaque: [u8; 36],
}

#[repr(C)]
struct TimeConstraint {
    period: u32,
    computation: u32,
    constraint: u32,
    preemptible: i32,
}

#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(object: AudioObjectId, address: *const PropertyAddress, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(alloc: *const c_void, bytes: *const u8, len: isize, encoding: u32, external: u8) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

unsafe extern "C" {
    fn os_workgroup_join(group: *mut c_void, token: *mut JoinToken) -> i32;
    fn os_workgroup_leave(group: *mut c_void, token: *mut JoinToken);
    fn os_release(object: *mut c_void);
    fn mach_thread_self() -> u32;
    fn mach_task_self() -> u32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
    fn thread_policy_set(thread: u32, flavor: u32, policy: *const TimeConstraint, count: u32) -> i32;
}

/// A device's work group (released when the last thread using it is done).
pub struct Workgroup(*mut c_void);

// SAFETY: an os_workgroup is a reference-counted system object, meant to be
// joined from any thread.
unsafe impl Send for Workgroup {}
unsafe impl Sync for Workgroup {}

impl Drop for Workgroup {
    fn drop(&mut self) {
        unsafe { os_release(self.0) };
    }
}

/// The device whose UID (as `cpal` gives it) is `uid`.
fn device(uid: &str) -> Option<AudioObjectId> {
    let name = unsafe { CFStringCreateWithBytes(std::ptr::null(), uid.as_ptr(), uid.len() as isize, UTF8, 0) };
    if name.is_null() {
        return None;
    }
    let address = PropertyAddress { selector: UID_TO_DEVICE, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
    let mut device: AudioObjectId = 0;
    let mut size = std::mem::size_of::<AudioObjectId>() as u32;
    let qualifier = &name as *const *const c_void as *const c_void;
    let status = unsafe { AudioObjectGetPropertyData(SYSTEM_OBJECT, &address, std::mem::size_of::<*const c_void>() as u32, qualifier, &mut size, &mut device as *mut _ as *mut c_void) };
    unsafe { CFRelease(name) };
    (status == 0 && device != 0).then_some(device)
}

impl Workgroup {
    /// The work group of the device whose UID is `uid`, if the system has one.
    pub fn of_device(uid: &str) -> Option<Arc<Self>> {
        let device = device(uid)?;
        let address = PropertyAddress { selector: IO_WORKGROUP, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
        let mut group: *mut c_void = std::ptr::null_mut();
        let mut size = std::mem::size_of::<*mut c_void>() as u32;
        let status = unsafe { AudioObjectGetPropertyData(device, &address, 0, std::ptr::null(), &mut size, &mut group as *mut _ as *mut c_void) };
        (status == 0 && !group.is_null()).then(|| Arc::new(Self(group)))
    }

    /// Joins this thread to the group until the thread ends. False if the
    /// system said no (the device stopped, or it is already in one).
    pub fn join(self: &Arc<Self>) -> bool {
        let mut token = Box::new(JoinToken { sig: 0, opaque: [0; 36] });
        if unsafe { os_workgroup_join(self.0, &mut *token) } != 0 {
            return false;
        }
        MEMBERSHIP.with(|m| *m.borrow_mut() = Some(Membership { group: self.clone(), token }));
        true
    }
}

/// This thread's place in a group: it leaves it as the thread ends.
struct Membership {
    group: Arc<Workgroup>,
    token: Box<JoinToken>,
}

impl Drop for Membership {
    fn drop(&mut self) {
        unsafe { os_workgroup_leave(self.group.0, &mut *self.token) };
    }
}

thread_local! {
    static MEMBERSHIP: std::cell::RefCell<Option<Membership>> = const { std::cell::RefCell::new(None) };
}

/// The device's buffer (frames), as it is now.
pub fn buffer_frames(uid: &str) -> Option<u32> {
    let device = device(uid)?;
    let address = PropertyAddress { selector: BUFFER_FRAMES, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
    let mut frames = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe { AudioObjectGetPropertyData(device, &address, 0, std::ptr::null(), &mut size, &mut frames as *mut _ as *mut c_void) };
    (status == 0 && frames > 0).then_some(frames)
}

/// Makes this thread a real-time one that runs every `period` (the device's
/// buffer), as the system asks of the threads in an audio work group. It may
/// work for up to half of each period; past that, the system plans it as an
/// ordinary thread for a while. False if the system said no.
pub fn make_realtime(period: Duration) -> bool {
    let mut timebase = Timebase { numer: 0, denom: 0 };
    if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.numer == 0 {
        return false;
    }
    let ticks = |d: Duration| (d.as_nanos() * timebase.denom as u128 / timebase.numer as u128).min(u32::MAX as u128) as u32;
    let policy = TimeConstraint { period: ticks(period), computation: ticks(period / 2), constraint: ticks(period), preemptible: 1 };
    const TIME_CONSTRAINT_POLICY: u32 = 2;
    const TIME_CONSTRAINT_POLICY_COUNT: u32 = 4;
    let thread = unsafe { mach_thread_self() };
    let status = unsafe { thread_policy_set(thread, TIME_CONSTRAINT_POLICY, &policy, TIME_CONSTRAINT_POLICY_COUNT) };
    unsafe { mach_port_deallocate(mach_task_self(), thread) };
    status == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait};

    /// The default output's group, joined from a real-time thread and an
    /// ordinary one (needs an audio device: ignored on CI).
    #[test]
    #[ignore = "diagnostic: needs this computer's audio output"]
    fn the_default_output_has_a_group_threads_can_join() {
        let device = cpal::default_host().default_output_device().expect("an output");
        let uid = device.id().expect("its UID").id().to_string();
        let group = Workgroup::of_device(&uid).expect("its work group");
        let frames = buffer_frames(&uid).expect("its buffer");
        println!("{uid}: búfer {frames}");
        let rt = {
            let group = group.clone();
            std::thread::spawn(move || (make_realtime(Duration::from_secs_f64(frames as f64 / 48_000.0)), group.join())).join().unwrap()
        };
        let plain = {
            let group = group.clone();
            std::thread::spawn(move || group.join()).join().unwrap()
        };
        println!("tiempo real: política {} · unido {} · normal: unido {}", rt.0, rt.1, plain);
        assert!(rt.0 && rt.1);
    }
}
