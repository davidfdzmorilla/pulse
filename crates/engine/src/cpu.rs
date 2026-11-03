//! How hard the audio thread works: the load (time spent making each block
//! over the time the block lasts), its mean and highest, and the dropouts
//! (blocks made too late, callbacks that came too late). With the detail on,
//! also the time of each track, instrument, device and return: added up in
//! nanoseconds, for the UI to turn into shares of real time. All atomics: the
//! audio thread never waits for the UI nor asks for memory.

use crate::engine::{Target, MAX_CHAIN, MAX_RETURNS, MAX_TRACKS};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// How long the mean load takes to follow a change (seconds).
const SMOOTHING: f64 = 0.3;

pub struct Cpu {
    /// The mean load and the highest since the UI last took it (f32 bits;
    /// positive floats order like their bits).
    load: AtomicU32,
    peak: AtomicU32,
    /// Blocks made in more time than they last, and callbacks that came so
    /// late the device must have run dry: dropouts heard (or about to be).
    late_blocks: AtomicU64,
    late_callbacks: AtomicU64,
    /// Whether to time each track, instrument and device (the UI is showing them).
    detail: AtomicBool,
    tracks: Box<[AtomicU64]>,
    instruments: Box<[AtomicU64]>,
    returns: Box<[AtomicU64]>,
    master: AtomicU64,
    /// Every device of every channel, as the analyzer numbers them.
    devices: Box<[AtomicU64]>,
}

/// What the UI reads of a load: the mean and the highest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub mean: f32,
    pub peak: f32,
}

fn counters(n: usize) -> Box<[AtomicU64]> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

/// The index of `device` of `target` among all devices.
fn device_index(target: Target, device: usize) -> usize {
    let channel = match target {
        Target::Track(t) => t,
        Target::Return(r) => MAX_TRACKS + r,
        Target::Master => MAX_TRACKS + MAX_RETURNS,
    };
    channel * MAX_CHAIN + device.min(MAX_CHAIN - 1)
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            load: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            late_blocks: AtomicU64::new(0),
            late_callbacks: AtomicU64::new(0),
            detail: AtomicBool::new(false),
            tracks: counters(MAX_TRACKS),
            instruments: counters(MAX_TRACKS),
            returns: counters(MAX_RETURNS),
            master: AtomicU64::new(0),
            devices: counters((MAX_TRACKS + MAX_RETURNS + 1) * MAX_CHAIN),
        }
    }
}

impl Cpu {
    /// A block that lasts `lasts` took `took` to make (audio thread).
    pub fn record(&self, took: Duration, lasts: Duration) {
        if lasts.is_zero() {
            return;
        }
        let load = (took.as_secs_f64() / lasts.as_secs_f64()) as f32;
        let mean = f32::from_bits(self.load.load(Ordering::Relaxed));
        let k = (1.0 - (-lasts.as_secs_f64() / SMOOTHING).exp()) as f32;
        self.load.store((mean + (load - mean) * k).max(0.0).to_bits(), Ordering::Relaxed);
        self.peak.fetch_max(load.max(0.0).to_bits(), Ordering::Relaxed);
        if took > lasts {
            self.late_blocks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The device asked for a block `since` after the last one, which lasted
    /// `lasts`: so late (over twice as long, by more than 2 ms) that it ran
    /// dry. Much longer gaps (a computer waking up) don't count.
    pub fn callback(&self, since: Duration, lasts: Duration) {
        if since > lasts * 2 + Duration::from_millis(2) && since < Duration::from_secs(1) {
            self.late_callbacks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The mean load and the highest since the last call (1: all the time a block lasts).
    pub fn take_load(&self) -> Load {
        Load { mean: f32::from_bits(self.load.load(Ordering::Relaxed)), peak: f32::from_bits(self.peak.swap(0, Ordering::Relaxed)) }
    }

    /// Dropouts so far: blocks made late and callbacks that came late.
    pub fn dropouts(&self) -> u64 {
        self.late_blocks.load(Ordering::Relaxed) + self.late_callbacks.load(Ordering::Relaxed)
    }

    /// Of those, the blocks made late (the rest the device's).
    pub fn late_blocks(&self) -> u64 {
        self.late_blocks.load(Ordering::Relaxed)
    }

    /// Times each track, instrument and device from now on (or stops).
    pub fn set_detail(&self, on: bool) {
        self.detail.store(on, Ordering::Relaxed);
    }

    pub fn detail(&self) -> bool {
        self.detail.load(Ordering::Relaxed)
    }

    /// Nanoseconds spent on track `t` (its instrument, effects and fader) while timing.
    pub fn track(&self, t: usize) -> u64 {
        self.tracks.get(t).map_or(0, |a| a.load(Ordering::Relaxed))
    }

    /// Of those, its instrument's.
    pub fn instrument(&self, t: usize) -> u64 {
        self.instruments.get(t).map_or(0, |a| a.load(Ordering::Relaxed))
    }

    /// Nanoseconds spent on return `r` (its sends in and its effects) while timing.
    pub fn send_return(&self, r: usize) -> u64 {
        self.returns.get(r).map_or(0, |a| a.load(Ordering::Relaxed))
    }

    /// Nanoseconds spent on the master's effects while timing.
    pub fn master(&self) -> u64 {
        self.master.load(Ordering::Relaxed)
    }

    /// Nanoseconds spent in device `device` of `target` while timing.
    pub fn device(&self, target: Target, device: usize) -> u64 {
        if device >= MAX_CHAIN {
            return 0;
        }
        self.devices[device_index(target, device)].load(Ordering::Relaxed)
    }

    pub(crate) fn add_track(&self, t: usize, nanos: u64) {
        self.tracks[t].fetch_add(nanos, Ordering::Relaxed);
    }

    pub(crate) fn add_instrument(&self, t: usize, nanos: u64) {
        self.instruments[t].fetch_add(nanos, Ordering::Relaxed);
    }

    pub(crate) fn add_return(&self, r: usize, nanos: u64) {
        self.returns[r].fetch_add(nanos, Ordering::Relaxed);
    }

    pub(crate) fn add_master(&self, nanos: u64) {
        self.master.fetch_add(nanos, Ordering::Relaxed);
    }

    pub(crate) fn add_device(&self, target: Target, device: usize, nanos: u64) {
        if device < MAX_CHAIN {
            self.devices[device_index(target, device)].fetch_add(nanos, Ordering::Relaxed);
        }
    }
}

/// Nanoseconds since `started`, if timing.
pub(crate) fn since(started: Option<std::time::Instant>) -> u64 {
    started.map_or(0, |s| s.elapsed().as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block made in half the time it lasts is half the load; one made in
    /// more is a dropout; the peak is the highest since it was last read.
    #[test]
    fn the_load_follows_and_late_blocks_count() {
        let cpu = Cpu::default();
        let block = Duration::from_micros(1333);
        for _ in 0..2000 {
            cpu.record(block / 2, block);
        }
        let load = cpu.take_load();
        assert!((load.mean - 0.5).abs() < 0.01 && (load.peak - 0.5).abs() < 0.01, "{load:?}");
        assert_eq!(cpu.dropouts(), 0);
        cpu.record(block * 3, block);
        let load = cpu.take_load();
        assert!((load.peak - 3.0).abs() < 0.01, "{load:?}");
        assert!(load.mean < 0.6, "one block moves the mean a little: {load:?}");
        assert_eq!(cpu.dropouts(), 1);
        assert_eq!(cpu.take_load().peak, 0.0, "the peak was taken");
        // The device calling late: twice the block and more; a long sleep doesn't count.
        cpu.callback(block, block);
        cpu.callback(block * 3 + Duration::from_millis(3), block);
        cpu.callback(Duration::from_secs(5), block);
        assert_eq!((cpu.dropouts(), cpu.late_blocks()), (2, 1));
    }
}
