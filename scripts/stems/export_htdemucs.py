#!/usr/bin/env python3
"""Exports Meta's HTDemucs (4 stems: drums, bass, other, vocals) to ONNX for Pulse.

ONNX has no complex numbers, so the STFT and its inverse stay outside the graph:
Pulse computes the spectrogram of each 7.8 s segment and gives it to the model
with the waveform; the model returns each source's spectrogram (complex as
real/imaginary channels) and its time-branch waveform, both already
de-normalized. Each stem is then istft(spec) + wave (see crates/engine/src/stems.rs).

Usage (in a venv with `demucs onnx onnxruntime`):
    python export_htdemucs.py out_dir
Writes out_dir/htdemucs.onnx and out_dir/reference.npz (a test segment with the
PyTorch result, for checking the Rust side).
"""
import sys
from pathlib import Path

import numpy as np
import torch
from demucs.pretrained import get_model

SEGMENT = 343980  # 7.8 s at 44.1 kHz: the length HTDemucs was trained on


class Core(torch.nn.Module):
    """HTDemucs.forward from the normalization to the de-normalized outputs,
    without the STFT, the masking (cac) and the iSTFT."""

    def __init__(self, model):
        super().__init__()
        self.m = model

    def forward(self, mix, mag):
        m = self.m
        x = mag
        B, C, Fq, T = x.shape
        mean = x.mean(dim=(1, 2, 3), keepdim=True)
        std = x.std(dim=(1, 2, 3), keepdim=True)
        x = (x - mean) / (1e-5 + std)
        xt = mix
        meant = xt.mean(dim=(1, 2), keepdim=True)
        stdt = xt.std(dim=(1, 2), keepdim=True)
        xt = (xt - meant) / (1e-5 + stdt)

        saved, saved_t, lengths, lengths_t = [], [], [], []
        for idx, encode in enumerate(m.encoder):
            lengths.append(x.shape[-1])
            inject = None
            if idx < len(m.tencoder):
                lengths_t.append(xt.shape[-1])
                tenc = m.tencoder[idx]
                xt = tenc(xt)
                if not tenc.empty:
                    saved_t.append(xt)
                else:
                    inject = xt
            x = encode(x, inject)
            if idx == 0 and m.freq_emb is not None:
                frs = torch.arange(x.shape[-2], device=x.device)
                emb = m.freq_emb(frs).t()[None, :, :, None].expand_as(x)
                x = x + m.freq_emb_scale * emb
            saved.append(x)
        if m.crosstransformer:
            if m.bottom_channels:
                b, c, f, t = x.shape
                x = x.reshape(b, c, f * t)
                x = m.channel_upsampler(x)
                x = x.reshape(b, -1, f, t)
                xt = m.channel_upsampler_t(xt)
            x, xt = m.crosstransformer(x, xt)
            if m.bottom_channels:
                b, c, f, t = x.shape
                x = x.reshape(b, c, f * t)
                x = m.channel_downsampler(x)
                x = x.reshape(b, -1, f, t)
                xt = m.channel_downsampler_t(xt)
        for idx, decode in enumerate(m.decoder):
            skip = saved.pop(-1)
            x, pre = decode(x, skip, lengths.pop(-1))
            offset = m.depth - len(m.tdecoder)
            if idx >= offset:
                tdec = m.tdecoder[idx - offset]
                length_t = lengths_t.pop(-1)
                if tdec.empty:
                    pre = pre[:, :, 0]
                    xt, _ = tdec(pre, None, length_t)
                else:
                    skip = saved_t.pop(-1)
                    xt, _ = tdec(xt, skip, length_t)
        S = len(m.sources)
        x = x.view(B, S, -1, Fq, T)
        x = x * std[:, None] + mean[:, None]
        xt = xt.view(B, S, -1, SEGMENT)
        xt = xt * stdt[:, None] + meant[:, None]
        return x, xt


def main(out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    model = get_model("htdemucs").models[0]
    model.eval()
    assert model.sources == ["drums", "bass", "other", "vocals"], model.sources
    assert model.samplerate == 44100 and model.audio_channels == 2

    # A test segment: noise shaped a little, so every branch does something.
    torch.manual_seed(0)
    mix = torch.randn(1, 2, SEGMENT) * 0.1
    z = model._spec(mix)
    mag = model._magnitude(z)
    core = Core(model)
    with torch.no_grad():
        spec, wave = core(mix, mag)
        full = model(mix)  # the whole PyTorch forward, for the check
        # The same through the outside steps, as Pulse will do them.
        zout = model._mask(z, spec)
        rebuilt = model._ispec(zout, SEGMENT) + wave
    error = (rebuilt - full).abs().max().item()
    print(f"split vs whole forward: max error {error:.2e}")
    assert error < 1e-4, error

    path = out_dir / "htdemucs.onnx"
    torch.onnx.export(
        core, (mix, mag), str(path), input_names=["mix", "mag"], output_names=["spec", "wave"],
        opset_version=17, dynamo=False,
    )
    import onnxruntime as ort
    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    o_spec, o_wave = session.run(None, {"mix": mix.numpy(), "mag": mag.numpy()})
    print("onnx vs torch:", np.abs(o_spec - spec.numpy()).max(), np.abs(o_wave - wave.numpy()).max())
    np.savez(out_dir / "reference.npz", mix=mix.numpy(), mag=mag.numpy(), spec=spec.numpy(), wave=wave.numpy(), stems=full.numpy())
    print(f"{path}: {path.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
