"""Turns MIDI drum parts into sixteenth-note grids for training.

Each song becomes an array of steps x VOICES, every cell a level:
0 silent, 1 ghost, 2 normal, 3 accented. Only songs in 4/4 are kept, and
bar one starts at the first tick, so step 0 is always a downbeat.

    uv run python extract.py data/groove data/lmd_full --out data/grids.npz
"""

import argparse
import os
import sys
from multiprocessing import Pool

import mido
import numpy as np

# The sounds the model knows, in the order Pulse's drum_model.rs expects.
VOICES = ["kick", "snare", "clap", "hat", "open", "rim", "perc", "tom"]
KICK, SNARE, CLAP, HAT, OPEN, RIM, PERC, TOM = range(8)

# General MIDI drums, plus the Roland TD-11 notes the Groove MIDI Dataset uses.
# Rides keep time like a closed hat; crashes are left out, Pulse kits have none.
NOTE = {
    35: KICK, 36: KICK,
    38: SNARE, 40: SNARE,
    39: CLAP,
    37: RIM,
    42: HAT, 44: HAT, 22: HAT, 51: HAT, 59: HAT, 53: HAT,
    46: OPEN, 26: OPEN,
    41: TOM, 43: TOM, 45: TOM, 47: TOM, 48: TOM, 50: TOM, 58: TOM,
    54: PERC, 56: PERC, 60: PERC, 61: PERC, 62: PERC, 63: PERC, 64: PERC,
    65: PERC, 66: PERC, 69: PERC, 70: PERC, 75: PERC, 76: PERC, 77: PERC,
    82: PERC,
}

MAX_STEPS = 16 * 400


def level(velocity):
    if velocity < 50:
        return 1
    if velocity <= 100:
        return 2
    return 3


def grid(path):
    try:
        midi = mido.MidiFile(path, clip=True)
    except Exception:
        return None
    if midi.type == 2 or midi.ticks_per_beat <= 0:
        return None
    per_step = midi.ticks_per_beat / 4
    hits = []
    for track in midi.tracks:
        tick = 0
        for message in track:
            tick += message.time
            if message.type == "time_signature":
                if (message.numerator, message.denominator) != (4, 4):
                    return None
            elif message.type == "note_on" and message.channel == 9 and message.velocity > 0:
                voice = NOTE.get(message.note)
                if voice is not None:
                    hits.append((round(tick / per_step), voice, level(message.velocity)))
    if len(hits) < 64:
        return None
    steps = min(max(h[0] for h in hits) + 1, MAX_STEPS)
    steps = (steps + 15) // 16 * 16
    out = np.zeros((steps, len(VOICES)), np.uint8)
    for step, voice, lvl in hits:
        if step < steps:
            out[step, voice] = max(out[step, voice], lvl)
    # A part that is mostly silence (an intro hit, a single crash) teaches nothing.
    busy_bars = (out.reshape(-1, 16, len(VOICES)).sum(axis=(1, 2)) > 0).sum()
    if busy_bars < 8:
        return None
    return out


def midi_files(roots):
    for root in roots:
        for folder, _, files in os.walk(root):
            for name in files:
                if name.lower().endswith((".mid", ".midi")):
                    yield os.path.join(folder, name)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("roots", nargs="+")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    paths = sorted(midi_files(args.roots))
    print(f"{len(paths)} MIDI files", file=sys.stderr)
    grids, seen = [], set()
    with Pool() as pool:
        for i, g in enumerate(pool.imap(grid, paths, chunksize=64)):
            if g is not None:
                # Lakh holds many copies of the same song; keep one of each part.
                key = hash(g.tobytes())
                if key not in seen:
                    seen.add(key)
                    grids.append(g)
            if i % 10000 == 0:
                print(f"{i}/{len(paths)}: {len(grids)} parts", file=sys.stderr)
    lengths = np.array([len(g) for g in grids], np.int64)
    np.savez_compressed(args.out, steps=np.concatenate(grids), lengths=lengths)
    print(f"{len(grids)} parts, {lengths.sum() // 16} bars", file=sys.stderr)


if __name__ == "__main__":
    main()
