//! A fixed pool of threads that helps the audio thread through each block's
//! tracks and returns (see [`crate::engine::Engine`]'s `render_block` for the
//! graph they follow).
//!
//! Real-time rules: the pool's threads start with the engine, off the audio
//! thread; a block allocates nothing, takes no lock and never makes the audio
//! thread wait for the OS. The audio thread runs tasks itself: a worker that
//! is late waking up only means less help, never a later block. The only wait
//! is for a task a worker has already started, which it runs to the end.
//!
//! A block is a phase with a number (its epoch). Each task is claimed once per
//! phase, by whoever moves its `claimed` from the last epoch to this one, and
//! runs only when the tasks it depends on show this epoch in `done`. A worker
//! that wakes up late (a phase over, or the next one set up meanwhile) cannot
//! claim anything: every task of a finished phase holds that phase's epoch.
//! Everything a worker reads before claiming is atomic, so a stale read is
//! harmless; what it reads after (the job) stays put until the phase ends,
//! which waits for its task.
//!
//! Between blocks, workers spin a little (blocks of one callback come right
//! after each other, and a loaded song leaves short gaps), then sleep. The
//! audio thread wakes one; each worker woken wakes two more, so the audio
//! thread makes at most one system call per block.

use crate::engine::{MAX_RETURNS, MAX_TRACKS};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::Thread;
use std::time::{Duration, Instant};

/// Every track, then every return.
pub(crate) const TASKS: usize = MAX_TRACKS + MAX_RETURNS;
/// The most threads a block is shared among (the audio thread included).
pub const MAX_THREADS: usize = 16;
/// How long an idle worker spins before it sleeps.
const SPIN: Duration = Duration::from_micros(250);

/// A set of tasks, one bit each.
pub(crate) type Mask = u128;

/// What one block's tasks need: built on the audio thread.
pub(crate) struct Plan {
    /// The tasks, in the order one thread would run them (a valid order).
    pub queue: [u8; TASKS],
    /// The tasks each has to wait for (all earlier in `queue`).
    pub deps: [Mask; TASKS],
    /// Tasks only the audio thread runs (hosted plugins).
    pub pinned: Mask,
}

impl Plan {
    pub fn new() -> Self {
        Self { queue: std::array::from_fn(|i| i as u8), deps: [0; TASKS], pinned: 0 }
    }
}

/// Runs task `task` of the job at `job`.
pub(crate) type Run = unsafe fn(*const (), usize);

struct Shared {
    epoch: AtomicU32,
    queue: [AtomicU8; TASKS],
    deps: [[AtomicU64; 2]; TASKS],
    pinned: [AtomicU64; 2],
    claimed: [AtomicU32; TASKS],
    done: [AtomicU32; TASKS],
    finished: AtomicUsize,
    /// Tasks the workers have run, in all.
    helped: AtomicU64,
    job: AtomicPtr<()>,
    run: AtomicUsize,
    sleeping: Box<[AtomicBool]>,
    threads: OnceLock<Box<[Thread]>>,
    quit: AtomicBool,
}

fn split(mask: Mask) -> [u64; 2] {
    [mask as u64, (mask >> 64) as u64]
}

fn load(words: &[AtomicU64; 2]) -> Mask {
    words[0].load(Ordering::Relaxed) as Mask | (words[1].load(Ordering::Relaxed) as Mask) << 64
}

impl Shared {
    fn wake(&self, worker: usize) {
        if worker < self.sleeping.len() && self.sleeping[worker].load(Ordering::SeqCst) {
            if let Some(threads) = self.threads.get() {
                threads[worker].unpark();
            }
        }
    }

    /// Whether everything `task` waits for is done this phase.
    fn ready(&self, task: usize, epoch: u32) -> bool {
        let mut deps = load(&self.deps[task]);
        while deps != 0 {
            let d = deps.trailing_zeros() as usize;
            deps &= deps - 1;
            if self.done[d].load(Ordering::Acquire) != epoch {
                return false;
            }
        }
        true
    }

    /// Runs tasks of phase `epoch` until none is left to claim. `audio`: the
    /// audio thread, which also runs the pinned ones (and those first).
    fn work(&self, epoch: u32, audio: bool) {
        let previous = epoch.wrapping_sub(1);
        let pinned = load(&self.pinned);
        // Positions before it are all claimed: no need to look at them again.
        let mut first = 0;
        loop {
            if !audio && self.epoch.load(Ordering::Relaxed) != epoch {
                return;
            }
            let (mut open, mut pick, mut prefix) = (false, None, true);
            for p in first..TASKS {
                let task = self.queue[p].load(Ordering::Relaxed) as usize;
                if task >= TASKS || self.claimed[task].load(Ordering::Relaxed) != previous {
                    if prefix {
                        first = p + 1;
                    }
                    continue;
                }
                prefix = false;
                open = true;
                let mine = pinned >> task & 1 == 1;
                if (audio || !mine) && self.ready(task, epoch) {
                    if pick.is_none() {
                        pick = Some(task);
                    }
                    // The audio thread takes a pinned one before others can.
                    if !audio || mine {
                        pick = Some(task);
                        break;
                    }
                }
            }
            if !open {
                return;
            }
            let Some(task) = pick else {
                std::hint::spin_loop();
                continue;
            };
            if self.claimed[task].compare_exchange(previous, epoch, Ordering::Acquire, Ordering::Relaxed).is_err() {
                continue;
            }
            let job = self.job.load(Ordering::Acquire) as *const ();
            // SAFETY: stored from a `Run` by `Workers::run` before this phase began.
            let run: Run = unsafe { std::mem::transmute::<usize, Run>(self.run.load(Ordering::Relaxed)) };
            if audio {
                unsafe { run(job, task) };
            } else {
                // A panic must not leave the audio thread waiting for this task.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { run(job, task) }));
                self.helped.fetch_add(1, Ordering::Relaxed);
            }
            self.done[task].store(epoch, Ordering::Release);
            self.finished.fetch_add(1, Ordering::Release);
        }
    }

    /// Waits for a phase after `seen`; None when the pool shuts down.
    fn next(&self, me: usize, seen: u32) -> Option<u32> {
        let mut spins = 0u32;
        let mut idle: Option<Instant> = None;
        loop {
            if self.quit.load(Ordering::Acquire) {
                return None;
            }
            let epoch = self.epoch.load(Ordering::Acquire);
            if epoch != seen {
                return Some(epoch);
            }
            spins = spins.wrapping_add(1);
            if spins < 1 << 10 || idle.get_or_insert_with(Instant::now).elapsed() < SPIN {
                std::hint::spin_loop();
                continue;
            }
            // Asleep, unless a phase began meanwhile (whoever starts one checks
            // `sleeping` after it: one of the two sees the other).
            self.sleeping[me].store(true, Ordering::SeqCst);
            if self.epoch.load(Ordering::SeqCst) == seen && !self.quit.load(Ordering::SeqCst) {
                std::thread::park_timeout(Duration::from_millis(100));
            }
            self.sleeping[me].store(false, Ordering::SeqCst);
        }
    }
}

/// The pool: `threads - 1` workers next to the audio thread.
pub struct Workers {
    shared: Arc<Shared>,
}

/// Something each worker runs as it starts (tests count its allocations; an
/// app can join it to its audio work group).
pub type WorkerInit = Arc<dyn Fn() + Send + Sync>;

impl Workers {
    /// Starts `threads - 1` workers (at most [`MAX_THREADS`] in all). Call off the audio thread.
    pub fn new(threads: usize, init: Option<WorkerInit>) -> Self {
        let count = threads.clamp(1, MAX_THREADS) - 1;
        let shared = Arc::new(Shared {
            epoch: AtomicU32::new(0),
            queue: std::array::from_fn(|i| AtomicU8::new(i as u8)),
            deps: std::array::from_fn(|_| [AtomicU64::new(0), AtomicU64::new(0)]),
            pinned: [AtomicU64::new(0), AtomicU64::new(0)],
            claimed: std::array::from_fn(|_| AtomicU32::new(0)),
            done: std::array::from_fn(|_| AtomicU32::new(0)),
            finished: AtomicUsize::new(0),
            helped: AtomicU64::new(0),
            job: AtomicPtr::new(std::ptr::null_mut()),
            run: AtomicUsize::new(0),
            sleeping: (0..count).map(|_| AtomicBool::new(false)).collect(),
            threads: OnceLock::new(),
            quit: AtomicBool::new(false),
        });
        let threads: Box<[Thread]> = (0..count)
            .filter_map(|me| {
                let (shared, init) = (shared.clone(), init.clone());
                std::thread::Builder::new()
                    .name(format!("pulse-audio-{}", me + 1))
                    .spawn(move || worker(shared, me, init))
                    .ok()
                    .map(|handle| handle.thread().clone())
            })
            .collect();
        // A worker that failed to start is never woken nor waited for.
        let _ = shared.threads.set(threads);
        Self { shared }
    }

    /// Threads a block is shared among, the audio thread included.
    pub fn threads(&self) -> usize {
        self.shared.threads.get().map_or(0, |t| t.len()) + 1
    }

    /// Tasks the workers have run so far.
    pub fn helped(&self) -> u64 {
        self.shared.helped.load(Ordering::Relaxed)
    }

    /// Runs every task of `plan` with `run(job, task)`, on this thread and the
    /// workers, and returns when all are done.
    ///
    /// # Safety
    /// `run` must be sound to call from any thread for unpinned tasks, at the
    /// same time as for other tasks the plan doesn't order with it, and `job`
    /// must stay valid until this returns.
    pub(crate) unsafe fn run(&self, plan: &Plan, job: *const (), run: Run) {
        let s = &*self.shared;
        let epoch = s.epoch.load(Ordering::Relaxed).wrapping_add(1);
        for (slot, &task) in s.queue.iter().zip(&plan.queue) {
            slot.store(task, Ordering::Relaxed);
        }
        for (slot, &deps) in s.deps.iter().zip(&plan.deps) {
            let [a, b] = split(deps);
            slot[0].store(a, Ordering::Relaxed);
            slot[1].store(b, Ordering::Relaxed);
        }
        let [a, b] = split(plan.pinned);
        s.pinned[0].store(a, Ordering::Relaxed);
        s.pinned[1].store(b, Ordering::Relaxed);
        s.finished.store(0, Ordering::Relaxed);
        s.job.store(job as *mut (), Ordering::Relaxed);
        s.run.store(run as usize, Ordering::Relaxed);
        s.epoch.store(epoch, Ordering::SeqCst);
        s.wake(0);
        s.work(epoch, true);
        // What workers still run.
        while s.finished.load(Ordering::Acquire) < TASKS {
            std::hint::spin_loop();
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        // They stop by themselves (nothing waits for them here).
        self.shared.quit.store(true, Ordering::SeqCst);
        for thread in self.shared.threads.get().into_iter().flat_map(|t| t.iter()) {
            thread.unpark();
        }
    }
}

fn worker(shared: Arc<Shared>, me: usize, init: Option<WorkerInit>) {
    crate::engine::flush_denormals();
    raise_priority();
    if let Some(init) = init {
        init();
    }
    let mut seen = 0;
    while let Some(epoch) = shared.next(me, seen) {
        seen = epoch;
        shared.wake(2 * me + 1);
        shared.wake(2 * me + 2);
        shared.work(epoch, false);
    }
}

/// Asks the system to run this thread as soon as it has work, as it does the
/// audio thread (and on a performance core).
fn raise_priority() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
        }
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    }
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
        }
        const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
    }
}

/// Threads to share blocks among by default: half the cores, up to 6. A
/// worker the system stops halfway through a track holds the block up until
/// it runs again, so the rest are left for the interface and other programs
/// (past 6, blocks barely get shorter).
pub fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, 6)
}
