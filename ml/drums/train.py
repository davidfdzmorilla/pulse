"""Trains the drum model: given the steps so far, how the next one sounds.

A small causal transformer over sixteenth steps. Each step is the level of
every voice (see extract.py); the model reads the steps up to t and gives,
for every voice, how likely each level is at step t + 1. The position in an
eight-bar phrase goes in too, so it learns where fills and changes go.

    uv run python train.py data/grids.npz data/groove.npz --out runs/drums
    uv run python train.py --export runs/drums/model.pt --onnx ../../crates/stems/models/drums.onnx
"""

import argparse
import math
import os
import time

import numpy as np
import torch
from torch import nn

VOICES = 16
LEVELS = 4
PHRASE = 128  # steps in eight bars
CONTEXT = 128


class DrumModel(nn.Module):
    def __init__(self, width=256, layers=6, heads=8, dropout=0.1):
        super().__init__()
        self.hits = nn.Embedding(VOICES * LEVELS, width)
        self.phase = nn.Embedding(PHRASE, width)
        self.position = nn.Embedding(CONTEXT, width)
        layer = nn.TransformerEncoderLayer(
            width, heads, width * 4, dropout, batch_first=True, norm_first=True, activation="gelu"
        )
        self.body = nn.TransformerEncoder(layer, layers, enable_nested_tensor=False)
        self.norm = nn.LayerNorm(width)
        self.out = nn.Linear(width, VOICES * LEVELS)
        self.register_buffer("voice_base", torch.arange(VOICES) * LEVELS, persistent=False)

    def forward(self, steps, phase):
        """steps: (B, T, VOICES) levels; phase: (B, T) phrase position of the
        step each output predicts. Returns logits (B, T, VOICES, LEVELS)."""
        b, t, _ = steps.shape
        x = self.hits(steps + self.voice_base).sum(dim=2)
        x = x + self.phase(phase) + self.position(torch.arange(t, device=steps.device))
        mask = torch.triu(torch.full((t, t), float("-inf"), device=steps.device), diagonal=1)
        x = self.body(x, mask=mask)
        return self.out(self.norm(x)).view(b, t, VOICES, LEVELS)


class Windows:
    """Bar-aligned windows of CONTEXT + 1 steps from every part."""

    def __init__(self, files, weights, valid_every=20):
        self.steps, train, valid = [], [], []
        offset = 0
        for path, weight in zip(files, weights):
            data = np.load(path)
            steps, lengths = data["steps"], data["lengths"]
            self.steps.append(steps)
            start = 0
            for i, n in enumerate(lengths):
                bars = np.arange(0, n - CONTEXT - 1, 16)
                shift = phrase_shift(steps[start : start + n])
                target = valid if i % valid_every == 0 else train
                target.append((offset + start + bars, bars - shift, np.full(len(bars), weight, np.float64)))
                start += n
            offset += len(steps)
        self.steps = torch.from_numpy(np.concatenate(self.steps))
        self.train = self._join(train)
        self.valid = self._join(valid)

    @staticmethod
    def _join(chunks):
        starts = np.concatenate([c[0] for c in chunks])
        local = np.concatenate([c[1] for c in chunks])
        weights = np.concatenate([c[2] for c in chunks])
        return starts, local, np.cumsum(weights / weights.sum())

    def batch(self, split, size, rng):
        """Inputs, targets, and where in its part each window starts."""
        starts, local, cdf = self.train if split == "train" else self.valid
        pick = np.minimum(np.searchsorted(cdf, rng.random(size)), len(starts) - 1)
        index = torch.from_numpy(starts[pick])[:, None] + torch.arange(CONTEXT + 1)
        window = self.steps[index].long()
        return window[:, :-1], window[:, 1:], torch.from_numpy(local[pick])


def phrase_shift(part):
    """Steps from the part's start to its first phrase. Songs have intros and
    pickups of any length, so bars counted from the start say little; phrases
    are found where the fills are (snare and toms at the ends of bars) and
    the crashes that follow them, most every eight bars and some every four."""
    bars = part.reshape(-1, 16, VOICES)
    snare, crash, toms = 1, 6, [8, 9, 10]
    fills = (bars[:, 12:, [snare] + toms] > 0).sum(axis=(1, 2)) + 2 * (bars[:, 8:, toms] > 0).sum(axis=(1, 2))
    # A crash on a downbeat marks the bar before as a phrase's end.
    fills[:-1] += 3 * (bars[1:, 0, crash] > 0)
    index = np.arange(len(bars))
    score = [fills[(index - o) % 8 == 7].sum() + 0.5 * fills[(index - o) % 4 == 3].sum() for o in range(8)]
    return int(np.argmax(score)) * 16


def phase_of(starts_in_part, length):
    return (starts_in_part[:, None] + 1 + torch.arange(length)) % PHRASE  # never negative: torch's % follows the divisor


def train(args):
    device = "mps" if torch.backends.mps.is_available() else "cpu"
    files = args.data
    weights = [1.0] * len(files)
    if len(files) > 1:
        # The real drummers are few next to Lakh; let them be heard.
        weights[-1] = args.groove_weight
    data = Windows(files, weights)
    print(f"{len(data.train[0])} train windows, {len(data.valid[0])} valid", flush=True)
    model = DrumModel().to(device)
    print(f"{sum(p.numel() for p in model.parameters()) / 1e6:.1f}M parameters", flush=True)
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=0.05, betas=(0.9, 0.95))
    rng = np.random.default_rng(1)
    os.makedirs(args.out, exist_ok=True)
    loss_fn = nn.CrossEntropyLoss()
    begun = time.time()
    best = math.inf
    for step in range(args.steps + 1):
        lr = args.lr * min(1.0, step / 1000) * 0.5 * (1 + math.cos(math.pi * step / args.steps))
        for group in opt.param_groups:
            group["lr"] = lr
        x, y, phase = sample(data, "train", args.batch, rng, device)
        model.train()
        logits = model(x, phase)
        loss = loss_fn(logits.reshape(-1, LEVELS), y.reshape(-1))
        opt.zero_grad(set_to_none=True)
        loss.backward()
        nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        if step % 500 == 0:
            model.eval()
            with torch.no_grad():
                losses = []
                for _ in range(10):
                    x, y, phase = sample(data, "valid", args.batch, rng, device)
                    logits = model(x, phase)
                    losses.append(loss_fn(logits.reshape(-1, LEVELS), y.reshape(-1)).item())
            valid = sum(losses) / len(losses)
            print(
                f"step {step} train {loss.item():.4f} valid {valid:.4f} lr {lr:.2e} "
                f"{(time.time() - begun) / 60:.1f} min",
                flush=True,
            )
            if valid < best:
                best = valid
                torch.save(model.state_dict(), os.path.join(args.out, "model.pt"))


def sample(data, split, size, rng, device):
    x, y, start = data.batch(split, size, rng)
    phase = phase_of(start, CONTEXT)
    return x.to(device), y.to(device), phase.to(device)


class Exported(nn.Module):
    """The same network written out by hand, so ONNX keeps the length free
    (PyTorch's attention exports with the traced length baked in)."""

    def __init__(self, model):
        super().__init__()
        self.m = model

    def forward(self, steps, phase):
        m = self.m
        x = m.hits(steps + m.voice_base).sum(dim=2)
        position = torch.cumsum(torch.ones_like(phase), dim=1) - 1
        x = x + m.phase(phase) + m.position(position)
        ones = torch.ones_like(phase[0], dtype=torch.float32)
        mask = torch.triu(ones[:, None] * ones[None, :], diagonal=1) * -1e9
        for layer in m.body.layers:
            attention = layer.self_attn
            heads = attention.num_heads
            q, k, v = nn.functional.linear(layer.norm1(x), attention.in_proj_weight, attention.in_proj_bias).chunk(3, dim=-1)
            q, k, v = (t.unflatten(-1, (heads, -1)).transpose(1, 2) for t in (q, k, v))
            scores = q @ k.transpose(-1, -2) / math.sqrt(q.shape[-1]) + mask
            heard = (scores.softmax(dim=-1) @ v).transpose(1, 2).flatten(-2)
            x = x + attention.out_proj(heard)
            x = x + layer.linear2(nn.functional.gelu(layer.linear1(layer.norm2(x))))
        return m.out(m.norm(x)).unflatten(-1, (VOICES, LEVELS))


def export(args):
    model = DrumModel()
    model.load_state_dict(torch.load(args.export, map_location="cpu", weights_only=True))
    model.eval()
    exported = Exported(model).eval()
    with torch.no_grad():
        for t in (1, 37, 128):
            steps = torch.randint(0, LEVELS, (1, t, VOICES))
            phase = torch.randint(0, PHRASE, (1, t))
            gap = (model(steps, phase) - exported(steps, phase)).abs().max().item()
            assert gap < 1e-4, f"the hand-written network differs by {gap}"
    steps = torch.zeros(1, 64, VOICES, dtype=torch.int64)
    phase = torch.arange(64)[None] % PHRASE
    torch.onnx.export(
        exported,
        (steps, phase),
        args.onnx,
        input_names=["steps", "phase"],
        output_names=["logits"],
        dynamic_axes={"steps": {1: "time"}, "phase": {1: "time"}, "logits": {1: "time"}},
        opset_version=17,
    )
    import onnxruntime

    session = onnxruntime.InferenceSession(args.onnx)
    for t in (1, 37, 128):
        steps = torch.randint(0, LEVELS, (1, t, VOICES))
        phase = torch.randint(0, PHRASE, (1, t))
        (logits,) = session.run(None, {"steps": steps.numpy(), "phase": phase.numpy()})
        with torch.no_grad():
            gap = abs(logits - model(steps, phase).numpy()).max()
        assert gap < 1e-3, f"ONNX differs by {gap} at {t} steps"
    print(f"wrote {args.onnx} ({os.path.getsize(args.onnx) / 1e6:.1f} MB)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("data", nargs="*")
    parser.add_argument("--out", default="runs/drums")
    parser.add_argument("--steps", type=int, default=30000)
    parser.add_argument("--batch", type=int, default=128)
    parser.add_argument("--lr", type=float, default=6e-4)
    parser.add_argument("--groove-weight", type=float, default=20.0)
    parser.add_argument("--export")
    parser.add_argument("--onnx")
    args = parser.parse_args()
    if args.export:
        export(args)
    else:
        train(args)
