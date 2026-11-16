//! Rendering without an audio device, faster than real time (for export and tests).

use crate::engine::{Command, Engine, Garbage, LiveNote, RecordedNote, SharedState};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;

const BLOCK: usize = 1024;

pub struct OfflineRenderer {
    engine: Engine,
    commands: Producer<Command>,
    garbage: Consumer<Garbage>,
    // Kept so the engine's queues stay connected; offline rendering has no live input.
    _live: Producer<LiveNote>,
    _recorded: Consumer<RecordedNote>,
    pub state: Arc<SharedState>,
    pub sample_rate: f32,
    scratch: Vec<f32>,
}

impl OfflineRenderer {
    pub fn new(sample_rate: f32) -> Self {
        let (commands, command_rx) = RingBuffer::new(8192);
        let (live, live_rx) = RingBuffer::new(16);
        let (recorded_tx, recorded) = RingBuffer::new(16);
        let (garbage_tx, garbage) = RingBuffer::new(2 * 8192);
        let (engine, state) = Engine::new(sample_rate, command_rx, live_rx, recorded_tx, garbage_tx);
        Self {
            engine,
            commands,
            garbage,
            _live: live,
            _recorded: recorded,
            state,
            sample_rate,
            scratch: vec![0.0; BLOCK * 2],
        }
    }

    /// Shares the work among `threads` (see [`Engine::set_threads`]): the
    /// samples are the same on any number, and a heavy song renders sooner.
    pub fn set_threads(&mut self, threads: usize) {
        self.engine.set_threads(threads);
    }

    /// Threads it renders on.
    pub fn threads(&self) -> usize {
        self.engine.threads()
    }

    /// Queues a command; if the queue is full the engine applies what is pending first.
    pub fn send(&mut self, command: Command) {
        let mut command = command;
        loop {
            match self.commands.push(command) {
                Ok(()) => return,
                Err(rtrb::PushError::Full(back)) => {
                    command = back;
                    self.engine.process(&mut [], 2);
                    self.collect_garbage();
                }
            }
        }
    }

    fn collect_garbage(&mut self) {
        while self.garbage.pop().is_ok() {}
    }

    /// How late the song comes out of its effects (samples), with what was
    /// sent so far (see `Engine::compensate`).
    pub fn latency(&mut self) -> usize {
        self.engine.process(&mut [], 2);
        self.collect_garbage();
        self.engine.latency()
    }

    /// Renders and drops that latency: what it renders from here on is in
    /// time with where the transport was. Returns how many frames it dropped.
    pub fn skip_latency(&mut self) -> usize {
        let frames = self.latency();
        let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        self.render(frames, &mut left, &mut right);
        frames
    }

    /// Renders `frames` stereo frames, appending them to `left` and `right`.
    pub fn render(&mut self, frames: usize, left: &mut Vec<f32>, right: &mut Vec<f32>) {
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(BLOCK);
            let out = &mut self.scratch[..n * 2];
            self.engine.process(out, 2);
            for frame in out.chunks(2) {
                left.push(frame[0]);
                right.push(frame[1]);
            }
            done += n;
            self.collect_garbage();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{PlacedClip, PlayClip};
    use crate::model::{InstrumentKind, MidiClip, Note};

    /// A song rendered on four threads is the one rendered on one, sample by sample.
    #[test]
    fn threads_render_the_same_samples() {
        const SR: f32 = 48_000.0;
        let render = |threads: usize| {
            let mut renderer = OfflineRenderer::new(SR);
            renderer.set_threads(threads);
            assert_eq!(renderer.threads(), threads);
            renderer.engine.share_always();
            let catalog = InstrumentKind::catalog();
            for t in 0..12 {
                renderer.send(Command::set_instrument(t, catalog[t % catalog.len()], SR));
                let mut clip = MidiClip::new("c", 4.0, [0, 0, 0]);
                for k in 0..8 {
                    clip.notes.push(Note { start: k as f64 * 0.5, length: 0.4, pitch: 36 + ((t * 5 + k * 3) % 36) as u8, velocity: 0.8, chance: 1.0 });
                }
                renderer.send(Command::SetArrangement { track: t, clips: Box::new(vec![PlacedClip::new(0.0, 8.0, PlayClip::Midi(clip))]) });
                renderer.send(Command::SetSend { track: t, send: 0, amount: 0.3 });
            }
            renderer.send(Command::SetOutputLimit(false));
            renderer.send(Command::Play);
            let (mut left, mut right) = (Vec::new(), Vec::new());
            // Sizes that aren't whole blocks, too.
            for frames in [48_000, 1, 1023, 30_000, 17_000] {
                renderer.render(frames, &mut left, &mut right);
            }
            (left, right, renderer.engine.helped())
        };
        let (left, right, _) = render(1);
        assert!(left.iter().any(|s| s.abs() > 0.01), "it sounds");
        // A busy machine may keep the workers asleep through a whole render:
        // it is made again until they take part.
        let mut tries = 0;
        let (shared_left, shared_right, helped) = loop {
            let made = render(4);
            tries += 1;
            if made.2 > 0 || tries == 20 {
                break made;
            }
        };
        assert!(helped > 0, "the other threads did nothing");
        let same = |a: &[f32], b: &[f32]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        assert!(same(&left, &shared_left) && same(&right, &shared_right), "four threads sound otherwise");
    }
}
