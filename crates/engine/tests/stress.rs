//! Load, soak and real-time-safety tests of the engine, through the same
//! `Engine::process` the audio callback calls.
//!
//! Quick ones run with `cargo test --release -p engine --test stress`; the long
//! ones are ignored:
//!   cargo test --release -p engine --test stress -- --ignored --nocapture bench_big_project
//!   cargo test --release -p engine --test stress -- --ignored --nocapture bench_tracks
//!   PULSE_SOAK_MINUTES=60 cargo test --release -p engine --test stress -- --ignored --nocapture soak_long_session

use engine::model::{
    AudioClip, AutomationLane, AutomationPoint, Device, Effect, InstrumentKind, MidiClip, Note, Param, PluginId, WarpMode,

};
use engine::wavetable::{builtin, Wavetable};
use engine::{build_chain, detached, ChainSlot, Command, Engine, EngineHandle, PlacedClip, PlayClip, Sample, Target, MAX_TRACKS};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ----- A global allocator that counts what the "audio thread" allocates ----------------

struct Counting;

/// Bytes live on the heap (whole process), to see memory growth in the soak.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// What the engine's worker threads allocate or free (they are marked as they
/// start), and how many were marked.
static WORKER_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static MARKED: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Per thread, so tests running in parallel do not count each other's.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static WORKER: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static FREES: Cell<usize> = const { Cell::new(0) };
}

fn armed() -> bool {
    if WORKER.try_with(Cell::get).unwrap_or(false) {
        WORKER_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
    ARMED.try_with(Cell::get).unwrap_or(false)
}

fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
    let _ = counter.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if armed() {
            bump(&ALLOCS);
        }
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if armed() {
            bump(&FREES);
        }
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if armed() {
            bump(&ALLOCS);
        }
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Runs `f` counting this thread's allocations and frees: (allocs, frees).
fn counting<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
    let (a0, f0) = (ALLOCS.with(Cell::get), FREES.with(Cell::get));
    ARMED.with(|a| a.set(true));
    let r = f();
    ARMED.with(|a| a.set(false));
    (r, ALLOCS.with(Cell::get) - a0, FREES.with(Cell::get) - f0)
}

// ----- A big project, built from engine commands ---------------------------------------

const SR: f32 = 48_000.0;

/// Feeds a command, letting the engine take what is queued every 1000 commands so the
/// queue (4096) never fills (as `OfflineRenderer::send` does).
fn send(engine: &mut Engine, handle: &mut EngineHandle, command: Command) {
    thread_local! {
        static SENT: Cell<usize> = const { Cell::new(0) };
    }
    let sent = SENT.with(|s| {
        s.set(s.get() + 1);
        s.get()
    });
    if sent % 1000 == 0 {
        engine.process(&mut [], 2);
        handle.collect_garbage();
    }
    assert!(handle.send(command), "command queue full");
}

fn sine(seconds: f32, freq: f32) -> Arc<Sample> {
    let n = (seconds * SR) as usize;
    let left: Vec<f32> = (0..n).map(|i| (i as f32 * freq * std::f32::consts::TAU / SR).sin() * 0.4 * (-(i as f32) / SR * 3.0).exp()).collect();
    Arc::new(Sample::from_channels(left.clone(), left, SR))
}

fn loop_sample() -> Arc<Sample> {
    // 2 s of a noisy loop at 120 BPM (one bar).
    let n = (2.0 * SR) as usize;
    let mut seed = 0x1234_5678u32;
    let left: Vec<f32> = (0..n)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let beat = (i % (SR as usize / 2)) as f32 / SR;
            noise * 0.3 * (-beat * 20.0).exp() + (i as f32 * 110.0 * std::f32::consts::TAU / SR).sin() * 0.2
        })
        .collect();
    Arc::new(Sample::from_channels(left.clone(), left, SR))
}

fn audio_clip(warp: WarpMode, length: f64) -> AudioClip {
    AudioClip { hard_start: false, fades: Default::default(),
        envelopes: Vec::new(),
        name: "loop".into(),
        color: [0, 0, 0],
        sample: "loop.wav".into(),
        length,
        offset: 0.0,
        warp,
        original_bpm: 120.0,
        warp_markers: Vec::new(),
        takes: Vec::new(),
        key: None,
        gain_db: -6.0,
        transpose: 0.0, muted: false,
    }
}

/// A 4-bar clip: chords on every eighth plus a 16th-note line (a dense, realistic load).
fn busy_clip(root: u8, drums: bool) -> MidiClip {
    let mut clip = MidiClip::new("busy", 16.0, [0, 0, 0]);
    if drums {
        for step in 0..64 {
            let start = step as f64 * 0.25;
            for (pitch, every) in [(36u8, 4), (38, 8), (42, 1), (46, 2), (39, 16)] {
                if step % every == 0 {
                    clip.notes.push(Note { start, length: 0.2, pitch, velocity: 0.8, chance: 1.0 });
                }
            }
        }
    } else {
        for step in 0..32 {
            let start = step as f64 * 0.5;
            for interval in [0u8, 4, 7, 11] {
                clip.notes.push(Note { start, length: 0.45, pitch: root + interval, velocity: 0.7, chance: 1.0 });
            }
        }
        for step in 0..64 {
            clip.notes.push(Note { start: step as f64 * 0.25, length: 0.2, pitch: root + 12 + (step % 7) as u8, velocity: 0.6, chance: 1.0 });
        }
    }
    clip
}

fn lane(param: Param, points: &[(f64, f32)]) -> AutomationLane {
    let mut lane = AutomationLane::new(param);
    lane.points = points.iter().map(|&(beat, value)| AutomationPoint { beat, value, curve: 0.0 }).collect();
    lane
}

/// The effects on track `t` (4–8 of them).
fn track_devices(t: usize) -> Vec<Device> {
    use engine::model::*;
    let mut devices = vec![
        Device::new(Effect::Eq3(Eq3Params::default())),
        Device::new(Effect::Compressor(CompressorParams::default())),
        Device::new(Effect::Saturator(SaturatorParams { drive_db: 12.0, ..Default::default() })),
        Device::new(Effect::AutoFilter(AutoFilterParams::default())),
    ];
    if t % 3 == 0 {
        devices.push(Device::new(Effect::Chorus(ChorusParams::default())));
        devices.push(Device::new(Effect::Phaser(PhaserParams::default())));
        devices.push(Device::new(Effect::Delay(DelayParams::default())));
    }
    if t % 4 == 0 {
        devices.push(Device::new(Effect::Reverb(ReverbParams::default())));
    }
    if t % 6 == 1 {
        devices.push(Device::new(Effect::Glue(GlueParams { sidechain_hpf_hz: 100.0, ..Default::default() })));
        devices.push(Device::new(Effect::Multiband(MultibandParams::default())));
    }
    if t % 5 == 0 {
        devices.push(Device::new(Effect::Bitcrusher(BitcrusherParams::default())));
        devices.push(Device::new(Effect::Limiter(LimiterParams::default())));
    }
    devices
}

fn instrument_for(t: usize) -> InstrumentKind {
    let catalog = InstrumentKind::catalog();
    // PULSE_BENCH_HEAVY=1: only the heaviest synths (Polychrome and Modular).
    if std::env::var_os("PULSE_BENCH_HEAVY").is_some() {
        return catalog[5 + t % 2];
    }
    catalog[t % catalog.len()]
}

fn is_drums(kind: &InstrumentKind) -> bool {
    matches!(kind, InstrumentKind::DrumMachine(_) | InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_))
}

/// Puts a big project on the engine: `tracks` tracks, each with an instrument, a busy
/// arrangement clip, 4–8 effects and automation; 2 returns with reverb and delay; a
/// master chain; audio clips on every eighth track; a 16-bar loop; and plays it.
fn load_big_project(engine: &mut Engine, handle: &mut EngineHandle, tracks: usize) {
    use engine::model::*;
    let tables: Vec<Arc<Wavetable>> = vec![Arc::new(builtin(0)), Arc::new(builtin(3))];
    let drum_samples: Vec<Arc<Sample>> = (0..16).map(|k| sine(0.4, 60.0 + k as f32 * 30.0)).collect();
    let loop_file = loop_sample();
    send(engine, handle, Command::SetTempo(124.0));
    send(engine, handle, Command::SetLoop { enabled: true, start: 0.0, end: 64.0 });
    send(engine, handle, Command::SetQuantize(0.0));
    for t in 0..tracks {
        let kind = instrument_for(t);
        send(engine, handle, Command::set_instrument(t, kind, SR));
        let samples = match kind {
            InstrumentKind::Sampler(_) => vec![sine(2.0, 220.0)],
            InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_) => drum_samples.clone(),
            _ => Vec::new(),
        };
        send(engine, handle, Command::SetSamples { track: t, samples: Box::new(samples) });
        send(engine, handle, Command::SetWavetables { track: t, tables: Box::new(tables.clone()) });
        send(engine, handle, Command::ReplaceChain { target: Target::Track(t), chain: build_chain(&track_devices(t), SR) });
        let root = 36 + (t % 4) as u8 * 12;
        let mut clips = vec![
            PlacedClip::new(0.0, 32.0, PlayClip::Midi(busy_clip(root, is_drums(&kind)))),
            PlacedClip::new(32.0, 32.0, PlayClip::Midi(busy_clip(root + 5, is_drums(&kind)))),
        ];
        if t % 8 == 7 {
            let warp = if t % 16 == 7 { WarpMode::Stretch } else { WarpMode::Repitch };
            clips.push(PlacedClip::new(0.0, 64.0, PlayClip::Audio(audio_clip(warp, 4.0), loop_file.clone())));
        }
        send(engine, handle, Command::SetArrangement { track: t, clips: Box::new(clips) });
        // Session clips too (a scene to launch in the soak).
        for slot in 0..2 {
            send(engine, handle, Command::SetClip { track: t, slot, clip: Some(Box::new(PlayClip::Midi(busy_clip(root + slot as u8 * 2, is_drums(&kind))))) });
        }
        let lanes = vec![
            lane(Param::Volume, &[(0.0, 0.3), (32.0, 0.6), (64.0, 0.3)]),
            lane(Param::Pan, &[(0.0, -0.5), (16.0, 0.5), (32.0, -0.5), (48.0, 0.5), (64.0, -0.5)]),
            lane(Param::Device { device: 0, param: 0 }, &[(0.0, -6.0), (64.0, 6.0)]),
            lane(Param::Device { device: 3, param: 0 }, &[(0.0, 300.0), (32.0, 8000.0), (64.0, 300.0)]),
            lane(Param::Send(0), &[(0.0, 0.0), (64.0, 0.5)]),
            lane(Param::Instrument(0), &[(0.0, 0.2), (64.0, 0.8)]),
        ];
        send(engine, handle, Command::SetAutomation { target: Target::Track(t), lanes: Box::new(lanes) });
        send(engine, handle, Command::SetSend { track: t, send: 0, amount: 0.3 });
        send(engine, handle, Command::SetSend { track: t, send: 1, amount: 0.2 });
        send(engine, handle, Command::SetGain { target: Target::Track(t), gain: 0.25 });
        // An arpeggiator on every sixth track.
        if t % 6 == 5 {
            let mut devices = [None; MAX_MIDI_EFFECTS];
            devices[0] = Some(MidiDevice::new(MidiEffect::Arpeggiator(Default::default())));
            send(engine, handle, Command::SetMidiEffects { track: t, devices });
        }
    }
    send(engine, handle, Command::ReplaceChain { target: Target::Return(0), chain: build_chain(&[Device::new(Effect::Reverb(ReverbParams { size: 0.9, mix: 1.0, ..Default::default() }))], SR) });
    send(engine, handle, Command::ReplaceChain { target: Target::Return(1), chain: build_chain(&[Device::new(Effect::Delay(DelayParams { mix: 1.0, ..Default::default() }))], SR) });
    let master = [
        Device::new(Effect::Eq3(Eq3Params::default())),
        Device::new(Effect::Compressor(CompressorParams::default())),
        Device::new(Effect::Limiter(LimiterParams::default())),
    ];
    send(engine, handle, Command::ReplaceChain { target: Target::Master, chain: build_chain(&master, SR) });
    send(engine, handle, Command::SetAutomation { target: Target::Master, lanes: Box::new(vec![lane(Param::Tempo, &[(0.0, 120.0), (32.0, 128.0), (64.0, 120.0)])]) });
    send(engine, handle, Command::SetGain { target: Target::Master, gain: 0.5 });
    send(engine, handle, Command::Play);
}

/// The big project with every kind of routing the threads have to respect:
/// nested groups, sidechains (one listening to a track processed after it, two
/// listening to each other, one on a return), a track playing another's sound
/// (one processed after it), a drum split and a muted track. Compensation is on
/// (the limiters look ahead).
fn load_routed_project(engine: &mut Engine, handle: &mut EngineHandle) {
    use engine::model::*;
    load_big_project(engine, handle, 48);
    let mut parents = [None; MAX_TRACKS];
    for t in 1..16 {
        parents[t] = Some(if t < 8 || t == 8 { 0 } else { 8 });
    }
    send(engine, handle, Command::SetGroups(parents));
    // Ids are the track's number plus one.
    send(engine, handle, Command::SetTrackIds(std::array::from_fn(|t| t as u32 + 1)));
    let ducker = |source: usize| Device::new(Effect::Ducker(DuckerParams { depth: 0.8, release: 0.3, sidechain: Some(source as u32 + 1), ..Default::default() }));
    for (track, source) in [(21, 20), (5, 30), (22, 23), (23, 22)] {
        send(engine, handle, Command::InsertDevice { target: Target::Track(track), index: 0, slot: ChainSlot::new(&ducker(source), SR) });
    }
    let keyed = Device::new(Effect::Compressor(CompressorParams { threshold_db: -30.0, ratio: 8.0, sidechain: Some(3), ..Default::default() }));
    send(engine, handle, Command::InsertDevice { target: Target::Return(0), index: 0, slot: ChainSlot::new(&keyed, SR) });
    let drums = InstrumentKind::DrumMachine(Default::default());
    send(engine, handle, Command::set_instrument(34, drums, SR));
    send(engine, handle, Command::SetArrangement { track: 34, clips: Box::new(vec![PlacedClip::new(0.0, 64.0, PlayClip::Midi(busy_clip(36, true)))]) });
    // 33 plays 32's sound, 2 plays 40's, 35 takes two drum sounds of 34.
    let mut from = [0u32; MAX_TRACKS];
    from[33] = 33;
    from[2] = 41;
    from[35] = 35;
    let mut sounds = [0u16; MAX_TRACKS];
    sounds[35] = 0b101;
    send(engine, handle, Command::SetAudioFrom(from));
    send(engine, handle, Command::SetSoundsFrom(sounds));
    send(engine, handle, Command::SetMute { target: Target::Track(12), mute: true });
}

/// `seconds` of the routed project on `threads` threads, in callbacks of `sizes` in turn.
fn render_routed(threads: usize, sizes: &[usize], seconds: f64) -> Vec<f32> {
    render_routed_helped(threads, sizes, seconds).0
}

/// [`render_routed`], and how many tasks the other threads ran.
fn render_routed_helped(threads: usize, sizes: &[usize], seconds: f64) -> (Vec<f32>, u64) {
    let (mut engine, mut handle) = detached(SR);
    engine.set_threads(threads);
    assert_eq!(engine.threads(), threads);
    load_routed_project(&mut engine, &mut handle);
    let total = (seconds * SR as f64) as usize;
    let mut out = vec![0f32; total * 2];
    let (mut at, mut k) = (0, 0);
    while at < total {
        let n = sizes[k % sizes.len()].min(total - at);
        k += 1;
        engine.process(&mut out[at * 2..(at + n) * 2], 2);
        handle.collect_garbage();
        at += n;
    }
    (out, engine.helped())
}

fn same_samples(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn finite(out: &[f32]) -> bool {
    out.iter().all(|s| s.is_finite())
}

// ----- Load ------------------------------------------------------------------------------

/// Time per callback vs. the real-time budget for a big project at several buffer sizes.
#[test]
#[ignore = "benchmark: run with --ignored --nocapture"]
fn bench_big_project() {
    let tracks: usize = std::env::var("PULSE_BENCH_TRACKS").ok().and_then(|v| v.parse().ok()).unwrap_or(48);
    let seconds: f64 = std::env::var("PULSE_BENCH_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0);
    println!("{tracks} pistas, {seconds} s de audio por tamaño, 48 kHz");
    println!("{:>6} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8}", "búfer", "presup.", "p50", "p99", "p99.9", "máx", "carga50");
    for frames in [32usize, 64, 128, 256, 512, 1024] {
        let (mut engine, mut handle) = detached(SR);
        load_big_project(&mut engine, &mut handle, tracks);
        let mut out = vec![0f32; frames * 2];
        // Warm up 2 s (voices start, caches).
        for _ in 0..(2.0 * SR as f64 / frames as f64) as usize {
            engine.process(&mut out, 2);
        }
        handle.collect_garbage();
        let calls = (seconds * SR as f64 / frames as f64) as usize;
        let mut times = Vec::with_capacity(calls);
        let mut ok = true;
        for _ in 0..calls {
            let start = Instant::now();
            engine.process(&mut out, 2);
            times.push(start.elapsed().as_secs_f64() * 1e6);
            ok &= finite(&out);
        }
        assert!(ok, "non-finite output");
        times.sort_by(f64::total_cmp);
        let budget = frames as f64 / SR as f64 * 1e6;
        println!(
            "{frames:>6} {budget:>8.0}µ {:>8.0}µ {:>8.0}µ {:>8.0}µ {:>8.0}µ {:>7.0}%",
            percentile(&times, 0.5),
            percentile(&times, 0.99),
            percentile(&times, 0.999),
            times[times.len() - 1],
            percentile(&times, 0.5) / budget * 100.0
        );
    }
}

/// Time per 128-frame callback with 8, 32 and 64 loaded tracks (instruments,
/// 4–8 effects each, sends to two returns, automation, a master chain), on 1
/// to 8 threads (PULSE_BENCH_THREADS=1,4 for others). PULSE_BENCH_PACED=1 waits
/// out each callback's period, as a sound card does (workers then sleep
/// between callbacks and have to wake up).
#[test]
#[ignore = "benchmark: run with --ignored --nocapture bench_tracks"]
fn bench_tracks() {
    let frames = 128;
    let seconds: f64 = std::env::var("PULSE_BENCH_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let threads: Vec<usize> = std::env::var("PULSE_BENCH_THREADS").ok().map_or(vec![1, 2, 4, 8], |v| v.split(',').filter_map(|t| t.trim().parse().ok()).collect());
    let paced = std::env::var_os("PULSE_BENCH_PACED").is_some();
    let budget = frames as f64 / SR as f64 * 1e6;
    println!("búfer {frames} ({budget:.0} µs), {seconds} s de audio, 48 kHz{}", if paced { ", a ritmo real" } else { "" });
    println!("{:>6} {:>6} {:>9} {:>9} {:>9} {:>8}", "pistas", "hilos", "media", "p50", "p99", "carga");
    for tracks in [8usize, 32, 64] {
        for &threads in &threads {
            let (mut engine, mut handle) = detached(SR);
            engine.set_threads(threads);
            load_big_project(&mut engine, &mut handle, tracks);
            let mut out = vec![0f32; frames * 2];
            for _ in 0..(2.0 * SR as f64 / frames as f64) as usize {
                engine.process(&mut out, 2);
            }
            handle.collect_garbage();
            let calls = (seconds * SR as f64 / frames as f64) as usize;
            let mut times = Vec::with_capacity(calls);
            for _ in 0..calls {
                let start = Instant::now();
                engine.process(&mut out, 2);
                times.push(start.elapsed().as_secs_f64() * 1e6);
                while paced && start.elapsed().as_secs_f64() * 1e6 < budget {
                    std::hint::spin_loop();
                }
            }
            assert!(finite(&out));
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            times.sort_by(f64::total_cmp);
            println!("{tracks:>6} {threads:>6} {mean:>8.0}µ {:>8.0}µ {:>8.0}µ {:>7.0}%", percentile(&times, 0.5), percentile(&times, 0.99), mean / budget * 100.0);
        }
    }
}

/// What each kind of instrument and effect costs, alone, at 256 frames.
#[test]
#[ignore = "benchmark: run with --ignored --nocapture"]
fn bench_components() {
    use engine::model::*;
    let frames = 256;
    let seconds = 5.0;
    let run = |setup: &dyn Fn(&mut Engine, &mut EngineHandle)| -> f64 {
        let (mut engine, mut handle) = detached(SR);
        setup(&mut engine, &mut handle);
        send(&mut engine, &mut handle, Command::Play);
        let mut out = vec![0f32; frames * 2];
        for _ in 0..200 {
            engine.process(&mut out, 2);
        }
        let calls = (seconds * SR as f64 / frames as f64) as usize;
        let start = Instant::now();
        for _ in 0..calls {
            engine.process(&mut out, 2);
        }
        start.elapsed().as_secs_f64() / seconds * 100.0
    };
    let empty = run(&|_, _| {});
    println!("motor vacío (64 pistas sin nada): {empty:.2} % de un núcleo");
    let tables: Vec<Arc<Wavetable>> = vec![Arc::new(builtin(0)), Arc::new(builtin(3))];
    for kind in InstrumentKind::catalog() {
        let tables = tables.clone();
        let cost = run(&move |e, h| {
            send(e, h, Command::set_instrument(0, kind, SR));
            let samples = match kind {
                InstrumentKind::Sampler(_) => vec![sine(2.0, 220.0)],
                InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_) => (0..16).map(|k| sine(0.4, 60.0 + k as f32 * 30.0)).collect(),
                _ => Vec::new(),
            };
            send(e, h, Command::SetSamples { track: 0, samples: Box::new(samples) });
            send(e, h, Command::SetWavetables { track: 0, tables: Box::new(tables.clone()) });
            send(e, h, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(48, is_drums(&kind))))]) });
        });
        println!("{:>12}: {:>6.2} % (menos el vacío: {:.2} %)", kind.label(), cost, cost - empty);
    }
    for effect in Effect::catalog() {
        let cost = run(&move |e, h| {
            send(e, h, Command::set_instrument(0, InstrumentKind::Synth(Default::default()), SR));
            send(e, h, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(48, false)))]) });
            send(e, h, Command::ReplaceChain { target: Target::Track(0), chain: build_chain(&[Device::new(effect)], SR) });
        });
        println!("{:>12}: {:>6.2} %", effect.label(), cost);
    }
    let stretch = run(&|e, h| {
        send(e, h, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Audio(audio_clip(WarpMode::Stretch, 4.0), loop_sample()))]) });
    });
    println!("clip de audio Stretch: {stretch:.2} %");
}

// ----- Real-time safety ------------------------------------------------------------------

/// Nothing the audio callback runs allocates or frees while a big project plays,
/// nor while it takes the commands of a project load or of preset changes.
#[test]
fn the_audio_thread_does_not_allocate() {
    let (mut engine, mut handle) = detached(SR);
    // Loading: the commands are built here (allocates, as the UI does); the engine
    // takes them when it processes. Count only the processing.
    load_big_project(&mut engine, &mut handle, 48);
    let mut out = vec![0f32; 256 * 2];
    let (_, load_allocs, load_frees) = counting(|| engine.process(&mut out, 2));
    handle.collect_garbage();
    // Steady playing, 30 s.
    let (_, allocs, frees) = counting(|| {
        for _ in 0..(30.0 * SR / 256.0) as usize {
            engine.process(&mut out, 2);
        }
    });
    handle.collect_garbage();
    // Changes while playing: presets, instrument swaps, chain edits, clips, scenes, tempo.
    let mut changes = Vec::new();
    for t in 0..48 {
        let mut kind = instrument_for(t);
        kind.set(0, 0.9);
        changes.push(Command::update_instrument(t, kind));
        changes.push(Command::set_instrument(t, instrument_for(t + 1), SR));
        changes.push(Command::UpdateDevice { target: Target::Track(t), index: 0, device: Device::new(Effect::Eq3(engine::model::Eq3Params { low_db: 3.0, ..Default::default() })) });
        changes.push(Command::InsertDevice { target: Target::Track(t), index: 1, slot: ChainSlot::new(&Device::new(Effect::Reverb(Default::default())), SR) });
        changes.push(Command::RemoveDevice { target: Target::Track(t), index: 2 });
        changes.push(Command::SetClip { track: t, slot: 3, clip: Some(Box::new(PlayClip::Midi(busy_clip(50, false)))) });
    }
    changes.push(Command::LaunchScene(1));
    changes.push(Command::SetTempo(140.0));
    let mut change_allocs = (0, 0);
    for command in changes {
        assert!(handle.send(command));
        let (_, a, f) = counting(|| engine.process(&mut out, 2));
        change_allocs.0 += a;
        change_allocs.1 += f;
        handle.collect_garbage();
    }
    let (_, after_allocs, after_frees) = counting(|| {
        for _ in 0..(10.0 * SR / 256.0) as usize {
            engine.process(&mut out, 2);
        }
    });
    println!("carga: {load_allocs}/{load_frees} · reproducción: {allocs}/{frees} · cambios: {change_allocs:?} · después: {after_allocs}/{after_frees}");
    assert_eq!((load_allocs, load_frees), (0, 0), "taking a project load allocates");
    assert_eq!((allocs, frees), (0, 0), "playing allocates");
    assert_eq!(change_allocs, (0, 0), "changes while playing allocate");
    assert_eq!((after_allocs, after_frees), (0, 0), "playing after the changes allocates");
}

/// The same with the tracks shared among threads: neither the audio thread nor
/// the workers allocate or free.
#[test]
fn the_audio_threads_do_not_allocate_with_workers() {
    let (mut engine, mut handle) = detached(SR);
    engine.set_threads_with(4, Some(Arc::new(|| {
        WORKER.with(|w| w.set(true));
        MARKED.fetch_add(1, Ordering::Relaxed);
    })));
    load_routed_project(&mut engine, &mut handle);
    let mut out = vec![0f32; 256 * 2];
    let (_, load_allocs, load_frees) = counting(|| engine.process(&mut out, 2));
    handle.collect_garbage();
    let before = WORKER_ALLOCS.load(Ordering::Relaxed);
    let (_, allocs, frees) = counting(|| {
        for _ in 0..(20.0 * SR / 256.0) as usize {
            engine.process(&mut out, 2);
        }
    });
    let mut change_allocs = (0, 0);
    for t in 0..48 {
        let mut kind = instrument_for(t);
        kind.set(0, 0.9);
        for command in [
            Command::update_instrument(t, kind),
            Command::set_instrument(t, instrument_for(t + 1), SR),
            Command::InsertDevice { target: Target::Track(t), index: 1, slot: ChainSlot::new(&Device::new(Effect::Reverb(Default::default())), SR) },
            Command::RemoveDevice { target: Target::Track(t), index: 2 },
        ] {
            assert!(handle.send(command));
            let (_, a, f) = counting(|| engine.process(&mut out, 2));
            change_allocs.0 += a;
            change_allocs.1 += f;
            handle.collect_garbage();
        }
    }
    let workers = WORKER_ALLOCS.load(Ordering::Relaxed) - before;
    println!("carga: {load_allocs}/{load_frees} · reproducción: {allocs}/{frees} · cambios: {change_allocs:?} · hilos de ayuda: {workers}");
    assert_eq!(engine.threads(), 4);
    assert_eq!(MARKED.load(Ordering::Relaxed), 3, "every worker is watched");
    assert!(engine.helped() > 1000, "the workers did little: {}", engine.helped());
    assert_eq!((load_allocs, load_frees), (0, 0), "taking a project load allocates");
    assert_eq!((allocs, frees), (0, 0), "playing allocates");
    assert_eq!(change_allocs, (0, 0), "changes while playing allocate");
    assert_eq!(workers, 0, "the workers allocate");
}

/// Shared among 2, 4 or 8 threads, the routed project sounds exactly as on one,
/// with callbacks of any size: every dependency (groups, sidechains, a track
/// playing another, drum splits, returns, compensation) held.
#[test]
fn threads_play_the_same_samples() {
    let one = render_routed(1, &[256], 6.0);
    assert!(finite(&one) && one.iter().any(|s| s.abs() > 0.01), "the project plays");
    for threads in [2, 4, 8] {
        let (out, helped) = render_routed_helped(threads, &[256], 6.0);
        assert!(helped > 0, "the other threads did nothing");
        assert!(same_samples(&out, &one), "{threads} threads sound otherwise");
    }
    let odd = [1usize, 63, 64, 500, 1024, 4096, 37, 128];
    assert!(same_samples(&render_routed(4, &odd, 4.0), &render_routed(1, &odd, 4.0)), "with callbacks of other sizes");
}

/// A non-finite sample (a corrupt float WAV, a plugin blowing up) gets into a reverb
/// return; does the master ever recover?
#[test]
fn one_nan_never_poisons_the_master() {
    let (mut engine, mut handle) = detached(SR);
    let mut left: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    left[1000] = f32::NAN;
    let file = Arc::new(Sample::from_channels(left.clone(), left, SR));
    // The clip plays once (bar 1), then the track is silent.
    send(&mut engine, &mut handle, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 0.2, PlayClip::Audio(audio_clip(WarpMode::Off, 0.2), file))]) });
    send(&mut engine, &mut handle, Command::SetSend { track: 0, send: 0, amount: 0.5 });
    send(&mut engine, &mut handle, Command::ReplaceChain { target: Target::Return(0), chain: build_chain(&[Device::new(Effect::Reverb(Default::default()))], SR) });
    // A healthy track playing next to it.
    send(&mut engine, &mut handle, Command::set_instrument(1, InstrumentKind::Synth(Default::default()), SR));
    send(&mut engine, &mut handle, Command::SetArrangement { track: 1, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(48, false)))]) });
    send(&mut engine, &mut handle, Command::Play);
    let mut out = vec![0f32; 512 * 2];
    let mut bad_seconds = 0;
    let mut first_bad = None;
    for second in 0..60 {
        let mut bad = false;
        for _ in 0..(SR as usize / 512) {
            engine.process(&mut out, 2);
            bad |= !finite(&out);
        }
        if bad {
            bad_seconds += 1;
            first_bad.get_or_insert(second);
        }
    }
    let meter = handle.state.master.take();
    println!("segundos con NaN en la salida: {bad_seconds}/60 (desde el {first_bad:?}); medidor del máster: {meter}");
    assert_eq!(bad_seconds, 0, "the master stays NaN after one bad sample");
}

/// cpal may deliver any number of frames: odd sizes, 1 frame, more than the engine's
/// 1024 block. Output stays finite, and with no automation it barely depends on how
/// the frames are cut.
#[test]
fn any_callback_size_plays_the_same() {
    let render = |sizes: &mut dyn FnMut() -> usize| -> Vec<f32> {
        let (mut engine, mut handle) = detached(SR);
        for t in 0..4 {
            let kind = [InstrumentKind::Synth(Default::default()), InstrumentKind::DrumMachine(Default::default()), InstrumentKind::Fm(Default::default()), InstrumentKind::SuperSaw(Default::default())][t];
            send(&mut engine, &mut handle, Command::set_instrument(t, kind, SR));
            send(&mut engine, &mut handle, Command::SetArrangement { track: t, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(48, is_drums(&kind))))]) });
            send(&mut engine, &mut handle, Command::ReplaceChain { target: Target::Track(t), chain: build_chain(&[Device::new(Effect::Delay(Default::default())), Device::new(Effect::Reverb(Default::default()))], SR) });
        }
        send(&mut engine, &mut handle, Command::Play);
        let total = 4 * SR as usize;
        let mut out = vec![0f32; total * 2];
        let mut at = 0;
        while at < total {
            let n = sizes().min(total - at);
            engine.process(&mut out[at * 2..(at + n) * 2], 2);
            at += n;
        }
        out
    };
    let fixed = render(&mut || 512);
    let mut seed = 7u32;
    let random = render(&mut || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        [1, 7, 33, 64, 100, 128, 511, 1023, 1024, 1025, 2048, 4096][(seed >> 24) as usize % 12]
    });
    let tiny = render(&mut || 1);
    assert!(finite(&fixed) && finite(&random) && finite(&tiny));
    // Notes land within a sample whatever the callback size (see note_onsets_by_callback_size),
    // so compare loudness per 100 ms rather than sample by sample.
    let rms = |x: &[f32]| -> Vec<f32> { x.chunks(9600).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).collect() };
    let (a, b, c) = (rms(&fixed), rms(&random), rms(&tiny));
    let worst = |x: &[f32], y: &[f32]| x.iter().zip(y).skip(1).map(|(p, q)| (p - q).abs() / p.max(1e-3)).fold(0f32, f32::max);
    println!("diferencia de nivel por 100 ms: 512 vs aleatorio {:.2} %, 512 vs 1 muestra {:.2} %", worst(&a, &b) * 100.0, worst(&a, &c) * 100.0);
    assert!(worst(&a, &b) < 0.05 && worst(&a, &c) < 0.05, "output depends on the callback size");
}

// ----- Soak ---------------------------------------------------------------------------------

/// The big project played for a long time (faster than real time), with preset and
/// instrument changes, scene launches and tempo changes every few seconds. Checks for
/// non-finite output, runaway levels, time per block drifting up, heap growth and
/// allocations on the audio thread.
#[test]
#[ignore = "soak: PULSE_SOAK_MINUTES=60 cargo test --release -p engine --test stress -- --ignored --nocapture soak_long_session"]
fn soak_long_session() {
    let minutes: f64 = std::env::var("PULSE_SOAK_MINUTES").ok().and_then(|v| v.parse().ok()).unwrap_or(30.0);
    let tracks = 32;
    let frames = 256;
    let (mut engine, mut handle) = detached(SR);
    load_big_project(&mut engine, &mut handle, tracks);
    let mut out = vec![0f32; frames * 2];
    let calls_per_second = SR as usize / frames;
    let mut seed = 0x9E37_79B9u32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut minute_times: Vec<f64> = Vec::new();
    let mut minute_peaks: Vec<f32> = Vec::new();
    let mut worst_call = 0f64;
    let mut audio_allocs = 0usize;
    let mut non_finite = 0usize;
    let mut live_start = 0usize;
    let total_seconds = (minutes * 60.0) as usize;
    let wall = Instant::now();
    for second in 0..total_seconds {
        // Every 3 s: a change, as someone working on the song would make.
        if second % 3 == 0 {
            let t = rand() as usize % tracks;
            let command = match rand() % 8 {
                0 => {
                    let mut kind = instrument_for(t);
                    kind.set((rand() % 8) as usize, (rand() % 1000) as f32 / 1000.0 * 2.0);
                    Command::update_instrument(t, kind)
                }
                1 => Command::set_instrument(t, instrument_for(rand() as usize), SR),
                2 => Command::ReplaceChain { target: Target::Track(t), chain: build_chain(&track_devices(rand() as usize), SR) },
                3 => Command::UpdateDevice { target: Target::Track(t), index: 2, device: Device::new(Effect::Saturator(engine::model::SaturatorParams { drive_db: (rand() % 30) as f32, ..Default::default() })) },
                4 => Command::LaunchScene((rand() % 2) as usize),
                5 => Command::BackToArrangement,
                6 => Command::SetTempo(90.0 + (rand() % 80) as f64),
                _ => Command::SetClip { track: t, slot: (rand() % 2) as usize, clip: Some(Box::new(PlayClip::Midi(busy_clip(40 + (rand() % 24) as u8, false)))) },
            };
            assert!(handle.send(command));
        }
        let start = Instant::now();
        let mut peak = 0f32;
        for _ in 0..calls_per_second {
            let call = Instant::now();
            let (_, a, _) = counting(|| engine.process(&mut out, 2));
            worst_call = worst_call.max(call.elapsed().as_secs_f64());
            audio_allocs += a;
            for s in &out {
                if !s.is_finite() {
                    non_finite += 1;
                }
                peak = peak.max(s.abs());
            }
        }
        handle.collect_garbage();
        let elapsed = start.elapsed().as_secs_f64();
        if second == 60 {
            live_start = LIVE.load(Ordering::Relaxed);
        }
        if second % 60 == 0 {
            minute_times.push(0.0);
            minute_peaks.push(0.0);
        }
        *minute_times.last_mut().unwrap() += elapsed;
        let p = minute_peaks.last_mut().unwrap();
        *p = p.max(peak);
    }
    let live_end = LIVE.load(Ordering::Relaxed);
    println!("{minutes} min de audio ({tracks} pistas, búfer {frames}) en {:.0} s de reloj", wall.elapsed().as_secs_f64());
    println!("carga por minuto (% de tiempo real): {:?}", minute_times.iter().map(|t| (t / 60.0 * 1000.0).round() / 10.0).collect::<Vec<_>>());
    println!("pico por minuto: {:?}", minute_peaks.iter().map(|p| (p * 100.0).round() / 100.0).collect::<Vec<_>>());
    println!("peor llamada: {:.0} µs (presupuesto {:.0} µs)", worst_call * 1e6, frames as f64 / SR as f64 * 1e6);
    println!("muestras no finitas: {non_finite}; asignaciones en el hilo de audio: {audio_allocs}");
    println!("heap vivo: minuto 1 {:.1} MB → final {:.1} MB", live_start as f64 / 1e6, live_end as f64 / 1e6);
    assert_eq!(non_finite, 0);
    let first = minute_times[1];
    let last = minute_times[minute_times.len() - 1];
    assert!(last < first * 1.5, "time per block drifts up: {first} → {last}");
}

// ----- Plugins ------------------------------------------------------------------------------

fn test_bundle() -> std::path::PathBuf {
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/testplug");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = std::process::Command::new(cargo)
        .args(["build", "--release", "-p", "pulse-testplug", "--target-dir"])
        .arg(&target)
        .current_dir(&root)
        .status()
        .expect("cargo build");
    assert!(status.success());
    let library = target.join("release").join("libpulse_testplug.dylib");
    let bundle = target.join("PulseTest.vst3");
    let binary = bundle.join("Contents/MacOS/PulseTest");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::copy(&library, &binary).unwrap();
    let clap = target.join("PulseTest.clap");
    let clap_binary = clap.join("Contents/MacOS/PulseTest");
    std::fs::create_dir_all(clap_binary.parent().unwrap()).unwrap();
    std::fs::copy(&library, &clap_binary).unwrap();
    bundle
}

/// 64 plugin instruments and 64 plugin effects (VST3 and CLAP) playing, their state
/// saved and reloaded on the UI thread while playing, every instance replaced over
/// and over; parameters moved from the UI and by automation.
#[test]
#[ignore = "plugins: run with --ignored --nocapture"]
fn many_plugins_and_state_reload_while_playing() {
    // The clips leave short gaps where nothing sounds: count silent blocks without
    // reloading too, to tell those from gaps a reload makes.
    let quiet = plugins_playing(false);
    let reloading = plugins_playing(true);
    println!("bloques en silencio: {quiet} sin recargar, {reloading} recargando");
}

fn plugins_playing(reload: bool) -> usize {
    use engine::plugin::{instantiate, scan};
    let vst3 = test_bundle();
    let clap = vst3.with_file_name("PulseTest.clap");
    let mut found = scan(&vst3).unwrap();
    found.extend(scan(&clap).unwrap());
    let synths: Vec<_> = found.iter().filter(|p| p.instrument).cloned().collect();
    let gains: Vec<_> = found.iter().filter(|p| !p.instrument).cloned().collect();
    assert_eq!((synths.len(), gains.len()), (2, 2));
    let (mut engine, mut handle) = detached(SR);
    let mut handles = Vec::new();
    for t in 0..MAX_TRACKS {
        let synth = &synths[t % 2];
        let (p, h) = instantiate(&synth.path, &synth.cid, SR, None).unwrap();
        send(&mut engine, &mut handle, Command::set_plugin(t, PluginId(t as u64), p));
        let gain = &gains[t % 2];
        let (g, gh) = instantiate(&gain.path, &gain.cid, SR, None).unwrap();
        let device = Device::new(Effect::Plugin(PluginId(1000 + t as u64)));
        let mut chain = build_chain(&[], SR);
        chain.push(ChainSlot::plugin(&device, g));
        send(&mut engine, &mut handle, Command::ReplaceChain { target: Target::Track(t), chain });
        send(&mut engine, &mut handle, Command::SetArrangement { track: t, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(36 + (t % 4) as u8 * 12, false)))]) });
        send(&mut engine, &mut handle, Command::SetAutomation { target: Target::Track(t), lanes: Box::new(vec![lane(Param::Instrument(0), &[(0.0, 0.1), (16.0, 0.9)]), lane(Param::Device { device: 0, param: 0 }, &[(0.0, 0.2), (16.0, 0.8)])]) });
        send(&mut engine, &mut handle, Command::SetGain { target: Target::Track(t), gain: 0.1 });
        handles.push((h, gh));
    }
    send(&mut engine, &mut handle, Command::SetLoop { enabled: true, start: 0.0, end: 16.0 });
    send(&mut engine, &mut handle, Command::Play);
    let frames = 128;
    let mut out = vec![0f32; frames * 2];
    let mut times = Vec::new();
    let mut audio_allocs = 0;
    let mut silent_blocks = 0;
    let mut ok = true;
    // 60 s of audio; every 0.5 s reload one track's plugins from their saved state.
    let calls = (60.0 * SR as f64 / frames as f64) as usize;
    for call in 0..calls {
        if reload && call % (SR as usize / frames / 2) == 0 {
            let t = (call / (SR as usize / frames / 2)) % MAX_TRACKS;
            let (h, gh) = &handles[t];
            h.set(0, 0.7);
            let (state, gain_state) = (h.state(), gh.state());
            let synth = &synths[t % 2];
            let gain = &gains[t % 2];
            let (p, nh) = instantiate(&synth.path, &synth.cid, SR, Some(&state)).unwrap();
            let (g, ngh) = instantiate(&gain.path, &gain.cid, SR, Some(&gain_state)).unwrap();
            assert!(handle.send(Command::set_plugin(t, PluginId(t as u64), p)));
            let mut chain = build_chain(&[], SR);
            chain.push(ChainSlot::plugin(&Device::new(Effect::Plugin(PluginId(1000 + t as u64))), g));
            assert!(handle.send(Command::ReplaceChain { target: Target::Track(t), chain }));
            handles[t] = (nh, ngh);
        }
        let start = Instant::now();
        let (_, a, _) = counting(|| engine.process(&mut out, 2));
        times.push(start.elapsed().as_secs_f64() * 1e6);
        audio_allocs += a;
        ok &= finite(&out);
        if out.iter().all(|s| *s == 0.0) {
            silent_blocks += 1;
        }
        handle.collect_garbage();
    }
    times.sort_by(f64::total_cmp);
    println!(
        "64 sintes + 64 efectos de plugin{}, búfer {frames} (presupuesto {:.0} µs): p50 {:.0} µs, p99 {:.0} µs, máx {:.0} µs; asignaciones en audio: {audio_allocs}",
        if reload { ", recargando uno cada 0,5 s" } else { "" },
        frames as f64 / SR as f64 * 1e6,
        percentile(&times, 0.5),
        percentile(&times, 0.99),
        times[times.len() - 1]
    );
    assert!(ok);
    silent_blocks
}

/// A CLAP plugin given many events in one block (a dense chord plus automation):
/// the host sorts them with a stable sort, which needs a buffer beyond ~70 events.
#[test]
#[ignore = "plugins: run with --ignored --nocapture"]
fn clap_events_are_sorted_without_allocating() {
    use engine::plugin::{instantiate, scan};
    let clap = test_bundle().with_file_name("PulseTest.clap");
    let synth = scan(&clap).unwrap().into_iter().find(|p| p.instrument).unwrap();
    let (mut p, _h) = instantiate(&synth.path, &synth.cid, SR, None).unwrap();
    let (mut l, mut r) = (vec![0f32; 256], vec![0f32; 256]);
    for events in [10usize, 60, 100, 200] {
        for k in 0..events {
            p.note_on((k % 128) as u8, 0.5, 100);
        }
        let (_, allocs, _) = counting(|| p.render(&mut l, &mut r));
        p.all_notes_off();
        p.render(&mut l, &mut r);
        println!("{events} eventos en un bloque CLAP: {allocs} asignaciones");
        assert_eq!(allocs, 0, "{events} events");
    }
}

/// The browser's preview: starting one while another plays, and one ending by itself,
/// hand the old file back wrapped in a new `Box<Vec<_>>`, built on the audio thread.
#[test]
fn previews_do_not_allocate_on_the_audio_thread() {
    let (mut engine, mut handle) = detached(SR);
    let mut out = vec![0f32; 256 * 2];
    assert!(handle.send(Command::PreviewSample(Some(sine(0.1, 440.0)))));
    let (_, start_allocs, _) = counting(|| engine.process(&mut out, 2));
    assert!(handle.send(Command::PreviewSample(Some(sine(0.1, 220.0)))));
    let (_, replace_allocs, _) = counting(|| engine.process(&mut out, 2));
    let (_, end_allocs, _) = counting(|| {
        for _ in 0..40 {
            engine.process(&mut out, 2);
        }
    });
    println!("preview: empezar {start_allocs}, sustituir {replace_allocs}, terminar sola {end_allocs} asignaciones en el hilo de audio");
    assert_eq!(replace_allocs + end_allocs, 0, "previews allocate on the audio thread");
}

/// Commands with odd values (indices out of range, huge or tiny numbers, empty clips,
/// loops of zero length) never panic the engine, and the output stays finite.
#[test]
fn odd_commands_do_not_panic() {
    use engine::model::*;
    let (mut engine, mut handle) = detached(SR);
    load_big_project(&mut engine, &mut handle, 8);
    let mut seed = 0xDEAD_BEEFu32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    // (Clip lengths are kept >= 0.01 here: tiny ones hang the engine, see
    // a_tiny_clip_length_stalls_the_audio_thread.)
    let odd_f64 = [0.0, -1.0, 1e-12, 1e12, -1e12, 4.0, 0.001];
    let odd_f32 = [0.0f32, -1.0, 1e-30, 1e30, -1e30, 0.5, 100.0];
    let mut out = vec![0f32; 300 * 2];
    let mut non_finite_blocks = 0;
    for i in 0..20_000 {
        let f = odd_f64[rand() as usize % odd_f64.len()];
        let g = odd_f32[rand() as usize % odd_f32.len()];
        let t = [0usize, 7, 63, 64, 1000][rand() as usize % 5];
        let kind = rand() % 16;
        let command = match kind {
            0 => Command::SetTempo(f),
            1 => Command::SetLoop { enabled: true, start: f, end: odd_f64[rand() as usize % odd_f64.len()] },
            2 => Command::SetPosition(f),
            3 => Command::SetQuantize(f),
            4 => Command::LaunchClip { track: t, slot: rand() as usize % 10 },
            // Faders as the UI allows them (absurd gains like 1e30 do overflow to inf/NaN).
            5 => Command::SetGain { target: Target::Track(t), gain: g.clamp(0.0, 2.0) },
            6 => Command::SetPan { target: Target::Return(t % 3), pan: g },
            7 => Command::SetSend { track: t, send: rand() as usize % 3, amount: g },
            8 => Command::RemoveDevice { target: Target::Track(t), index: rand() as usize % 20 },
            9 => Command::SetClip { track: t, slot: rand() as usize % 10, clip: Some(Box::new(PlayClip::Midi(MidiClip::new("empty", if f.abs() < 0.01 { 0.01 } else { f }, [0, 0, 0])))) },
            10 => Command::SetSwing { amount: g, grid: f },
            11 => Command::PreviewNote { track: t, pitch: (rand() % 256) as u8, velocity: g },
            12 => Command::SetAutomation { target: Target::Track(t), lanes: Box::new(vec![lane(Param::Device { device: rand() as usize % 20, param: rand() as usize % 40 }, &[(0.0, g), (f.abs(), -g)]), lane(Param::Instrument(rand() as usize % 300), &[(0.0, g)])]) },
            13 => Command::SetCountIn(f),
            14 => Command::RecordClip { track: t, slot: rand() as usize % 10 },
            _ => if i % 2 == 0 { Command::Play } else { Command::Stop },
        };
        handle.send(command);
        let frames = [1usize, 13, 64, 300][rand() as usize % 4];
        engine.process(&mut out[..frames * 2], 2);
        if !finite(&out[..frames * 2]) {
            if non_finite_blocks == 0 {
                println!("primera salida no finita en la orden {i}: tipo {kind}, f={f:e}, g={g:e}");
            }
            non_finite_blocks += 1;
        }
        handle.collect_garbage();
    }
    println!("bloques con salida no finita tras órdenes extremas: {non_finite_blocks}");
}

/// A MIDI clip whose length is tiny but positive (a hand-edited or imported file)
/// makes the sequencer walk one lap per clip length: the time of one callback grows
/// as 1/length, with no limit.
#[test]
fn a_tiny_clip_length_does_not_stall_the_audio_thread() {
    for length in [1e-2, 1e-4, 1e-6, 1e-8] {
        let (mut engine, mut handle) = detached(SR);
        let mut clip = MidiClip::new("tiny", length, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: length / 2.0, pitch: 60, velocity: 0.5, chance: 1.0 });
        send(&mut engine, &mut handle, Command::SetQuantize(0.0));
        send(&mut engine, &mut handle, Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
        send(&mut engine, &mut handle, Command::LaunchClip { track: 0, slot: 0 });
        let mut out = vec![0f32; 256 * 2];
        engine.process(&mut out, 2);
        let start = Instant::now();
        engine.process(&mut out, 2);
        let us = start.elapsed().as_secs_f64() * 1e6;
        println!("clip de {length:e} beats: {us:.0} µs por callback de 256 (presupuesto 5333 µs)");
        assert!(us < 2000.0, "{length:e} beats: {us:.0} µs");
    }
}

/// The app sends `Command::set_instrument` (a freshly built synth) when an effect is
/// added inside the wavetable or modular synth (`needs_rebuild`); does it take effect
/// while the track plays?
#[test]
fn an_effect_added_inside_the_wavetable_synth_is_heard() {
    use engine::model::*;
    let level = |with_fx: bool| -> f32 {
        let (mut engine, mut handle) = detached(SR);
        let base = InstrumentKind::Wavetable(WaveParams::default());
        send(&mut engine, &mut handle, Command::set_instrument(0, base, SR));
        send(&mut engine, &mut handle, Command::SetWavetables { track: 0, tables: Box::new(vec![Arc::new(builtin(0)), Arc::new(builtin(3))]) });
        send(&mut engine, &mut handle, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi(busy_clip(48, false)))]) });
        send(&mut engine, &mut handle, Command::SetOutputLimit(false));
        send(&mut engine, &mut handle, Command::Play);
        let mut out = vec![0f32; 512 * 2];
        for _ in 0..50 {
            engine.process(&mut out, 2);
        }
        if with_fx {
            let mut p = WaveParams::default();
            p.fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -40.0, ..Default::default() })));
            let kind = InstrumentKind::Wavetable(p);
            assert!(base.needs_rebuild(&kind), "the app rebuilds for this");
            assert!(handle.send(Command::set_instrument(0, kind, SR)));
        }
        let mut sum = 0f32;
        for _ in 0..100 {
            engine.process(&mut out, 2);
            sum += out.iter().map(|s| s * s).sum::<f32>();
        }
        sum.sqrt()
    };
    let (plain, with) = (level(false), level(true));
    println!("wavetable sin efecto {plain:.2}, con Utility −40 dB añadido en marcha {with:.2}");
    assert!(with < plain * 0.1, "the added effect is not heard");
}

/// `sync_all` (undo, adding a track…) loads every plugin again from its state and sends
/// `Command::set_plugin` with the same id; the app keeps the new instance's handle. Does
/// the engine play the new instance (the one the UI now controls)?
#[test]
#[ignore = "plugins: run with --ignored --nocapture"]
fn a_reloaded_plugin_instrument_is_the_one_that_plays() {
    use engine::plugin::{instantiate, scan};
    let vst3 = test_bundle();
    for bundle in [vst3.clone(), vst3.with_file_name("PulseTest.clap")] {
        let synth = scan(&bundle).unwrap().into_iter().find(|p| p.instrument).unwrap();
        let (mut engine, mut handle) = detached(SR);
        let (p1, h1) = instantiate(&synth.path, &synth.cid, SR, None).unwrap();
        send(&mut engine, &mut handle, Command::set_plugin(0, PluginId(1), p1));
        send(&mut engine, &mut handle, Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 1e6, PlayClip::Midi({
            let mut c = MidiClip::new("held", 1e6, [0, 0, 0]);
            c.notes.push(Note { start: 0.0, length: 1e5, pitch: 69, velocity: 1.0, chance: 1.0 });
            c
        }))]) });
        send(&mut engine, &mut handle, Command::Play);
        let mut out = vec![0f32; 512 * 2];
        let mut level = |engine: &mut Engine, handle: &mut EngineHandle| {
            let mut peak = 0f32;
            for _ in 0..40 {
                engine.process(&mut out, 2);
                peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
                handle.collect_garbage();
            }
            peak
        };
        let before = level(&mut engine, &mut handle);
        // What sync_all does: state from the running one, a new instance, the same id.
        let state = h1.state();
        let (p2, h2) = instantiate(&synth.path, &synth.cid, SR, Some(&state)).unwrap();
        drop(h1);
        assert!(handle.send(Command::set_plugin(0, PluginId(1), p2)));
        level(&mut engine, &mut handle);
        // The UI turns the level down on the instance it holds now.
        h2.set(0, 0.0);
        let after = level(&mut engine, &mut handle);
        println!("{}: nivel {before:.3} → con el nivel del plugin a 0 en la instancia nueva {after:.3}", bundle.file_name().unwrap().to_string_lossy());
        assert!(after < before * 0.1, "the engine still plays the old instance; the UI controls a silent one");
    }
}
