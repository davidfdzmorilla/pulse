"""Reference for Pulse's audio-to-MIDI (crates/stems/src/transcribe.rs).

Writes a test melody (a few piano-like notes and a chord) to
crates/stems/tests/data/melody.wav and what Basic Pitch (Spotify, Apache 2.0)
finds in it to melody_notes.json, with its default settings, so the Rust port
can be compared with the original.

    python scripts/transcribe/make_reference.py
(in an environment with `basic-pitch[onnx]`).
"""
import json
import math
import pathlib
import struct
import wave

from basic_pitch.inference import predict

RATE = 44100
OUT = pathlib.Path(__file__).resolve().parents[2] / "crates/stems/tests/data"

# (start s, length s, MIDI notes)
EVENTS = [(0.0, 0.45, [60]), (0.5, 0.45, [64]), (1.0, 0.45, [67]), (1.5, 0.9, [72]), (2.5, 1.2, [57, 60, 64])]


def tone(t, pitch):
    f = 440 * 2 ** ((pitch - 69) / 12)
    return sum(math.sin(2 * math.pi * f * h * t) / h for h in (1, 2, 3, 4))


def main():
    n = int(4.0 * RATE)
    data = [0.0] * n
    for start, length, pitches in EVENTS:
        s = int(start * RATE)
        for i in range(int(length * RATE)):
            t = i / RATE
            env = min(1.0, t / 0.005) * math.exp(-t * 2.0) * min(1.0, (length - t) / 0.02)
            data[s + i] += sum(tone(t, p) for p in pitches) * env * 0.2
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / "melody.wav"
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, x)) * 32767)) for x in data))
    # The ONNX model, as Pulse runs it (on a Mac the default would be CoreML's).
    model = pathlib.Path(__file__).resolve().parents[2] / "crates/stems/models/basic_pitch.onnx"
    _, _, notes = predict(str(path), model)
    notes = sorted([[round(float(s), 4), round(float(e), 4), int(p), round(float(a), 4)] for s, e, p, a, _ in notes])
    (OUT / "melody_notes.json").write_text(json.dumps(notes, indent=1))
    print(len(notes), "notes")
    for n_ in notes:
        print(n_)


if __name__ == "__main__":
    main()
