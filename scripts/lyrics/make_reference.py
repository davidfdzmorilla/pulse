"""Reference for Pulse's lyrics (crates/stems/src/whisper.rs).

Works out the log-mel spectrogram Whisper takes, with Hugging Face's
`WhisperFeatureExtractor` (the same numbers as OpenAI's `log_mel_spectrogram`),
for a test signal both sides make the same way, and writes a few of its frames
to crates/stems/tests/data/whisper_mel.txt, so the Rust port can be compared.

    python scripts/lyrics/make_reference.py
(in an environment with `numpy` and `transformers`; no PyTorch needed).
"""
import math
import pathlib

import numpy as np
from transformers import WhisperFeatureExtractor

OUT = pathlib.Path(__file__).resolve().parents[2] / "crates/stems/tests/data/whisper_mel.txt"
RATE = 16000
# Frames written: the start, the middle of the sound, its end, the silence after.
FRAMES = [0, 1, 37, 100, 199, 200, 201, 250, 2999]


def signal():
    """Two seconds: a 440 Hz tone, a 3 kHz one fading in, and a click."""
    n = 2 * RATE
    x = []
    for i in range(n):
        t = i / RATE
        v = 0.5 * math.sin(2 * math.pi * 440 * t) + 0.25 * (i / n) * math.sin(2 * math.pi * 3000 * t)
        if i == RATE:
            v += 0.9
        x.append(v)
    return np.array(x, dtype=np.float32)


def main():
    extractor = WhisperFeatureExtractor(feature_size=80)
    mel = extractor(signal(), sampling_rate=RATE, return_tensors="np").input_features[0]
    assert mel.shape == (80, 3000), mel.shape
    with open(OUT, "w") as f:
        f.write("# frame, then its 80 log-mel values (Whisper's scale)\n")
        for frame in FRAMES:
            f.write(f"{frame} " + " ".join(f"{v:.6f}" for v in mel[:, frame]) + "\n")
    print(f"{OUT}: {len(FRAMES)} frames")


if __name__ == "__main__":
    main()
