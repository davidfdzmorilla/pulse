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
        let (garbage_tx, garbage) = RingBuffer::new(8192);
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
