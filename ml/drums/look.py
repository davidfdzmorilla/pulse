"""Prints what the model plays after a beat, as a grid, to check it by eye.

    uv run python look.py runs/drums/model.pt --heat 0.8
"""

import argparse

import numpy as np
import torch

from train import CONTEXT, PHRASE, VOICES, DrumModel

NAMES = ["kick", "snare", "clap", "hat", "open", "rim", "perc", "tom"]

# Four bars to start from, each voice a sixteen-step line (. g x X).
SEEDS = {
    "house": {"kick": "X...X...X...X...", "open": "..x...x...x...x.", "clap": "....X.......X...", "hat": "x.x.x.x.x.x.x.x."},
    "techno": {"kick": "X...X...X...X...", "open": "..x...x...x...x.", "hat": ".g.g.g.g.g.g.g.g", "clap": "....x.......x..."},
    "trap": {"kick": "X......X..X.....", "snare": "........X.......", "hat": "x.x.x.x.x.x.x.x."},
    "rock": {"kick": "X.......X.X.....", "snare": "....X.......X...", "hat": "x.x.x.x.x.x.x.x."},
}


def seed_steps(lines, bars=4):
    steps = np.zeros((16 * bars, VOICES), np.int64)
    for name, line in lines.items():
        for i, c in enumerate(line * bars):
            steps[i, NAMES.index(name)] = ".gxX".index(c)
    return torch.from_numpy(steps)


def play_on(model, seed, steps, heat, rng):
    song = seed.clone()
    offset = (PHRASE - len(seed) % PHRASE) % PHRASE
    for _ in range(steps):
        # As Pulse does: the latest bars that fit, from a downbeat (as it trained).
        start = max(0, -(-(len(song) - CONTEXT) // 16) * 16)
        view = torch.arange(start, len(song))
        phase = (view + offset + 1) % PHRASE
        with torch.no_grad():
            logits = model(song[view][None], phase[None])[0, -1]
        probs = torch.softmax(logits / max(heat, 1e-3), dim=-1)
        step = torch.multinomial(probs, 1, generator=rng)[:, 0]
        song = torch.cat([song, step[None]])
    return song[len(seed):]


def show(steps):
    for bar in range(len(steps) // 16):
        chunk = steps[bar * 16 : (bar + 1) * 16]
        lines = [f"{n:>5} " + "".join(".gxX"[int(l)] for l in chunk[:, v]) for v, n in enumerate(NAMES) if chunk[:, v].any()]
        print(f"bar {bar + 1}")
        print("\n".join(lines))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("--heat", type=float, default=0.8)
    parser.add_argument("--bars", type=int, default=8)
    parser.add_argument("--style", default="house")
    args = parser.parse_args()
    model = DrumModel()
    model.load_state_dict(torch.load(args.model, map_location="cpu", weights_only=True))
    model.eval()
    rng = torch.Generator().manual_seed(1)
    show(play_on(model, seed_steps(SEEDS[args.style]), args.bars * 16, args.heat, rng))
