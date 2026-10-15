#!/usr/bin/env python3
"""Reference results from PyTorch Demucs, to check Pulse's Rust separation.

Usage: python make_reference.py out_dir song.wav
Reads out_dir/reference.npz (from export_htdemucs.py) and writes its arrays as
raw little-endian float32 files (segment_*.f32), then separates the first 20 s
of song.wav the way Demucs does by default without shifts (7.8 s segments,
25 % overlap) and writes song_mix.f32 (2 × n, 44.1 kHz) and song_stems.f32
(4 sources × 2 channels × n).
"""
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from demucs.apply import apply_model
from demucs.audio import convert_audio
from demucs.pretrained import get_model

out = Path(sys.argv[1])
ref = np.load(out / "reference.npz")
for name in ["mix", "mag", "spec", "wave", "stems"]:
    ref[name].astype("<f4").tofile(out / f"segment_{name}.f32")
    print(name, ref[name].shape)

model = get_model("htdemucs")
data, sr = sf.read(sys.argv[2], dtype="float32", always_2d=True)
wav = torch.from_numpy(data.T.copy())
wav = convert_audio(wav, sr, 44100, 2)[:, : 44100 * 20]
r = wav.mean(0)
mean, std = r.mean(), r.std() + 1e-8
with torch.no_grad():
    stems = apply_model(model, ((wav - mean) / std)[None], shifts=0, split=True, overlap=0.25)[0]
stems = stems * std + mean
wav.numpy().astype("<f4").tofile(out / "song_mix.f32")
stems.numpy().astype("<f4").tofile(out / "song_stems.f32")
print("song", tuple(wav.shape), tuple(stems.shape))
