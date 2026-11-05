<p align="center">
  <img src="packaging/icon-1024.png" alt="Pulse logo" width="112">
</p>

<h1 align="center">Pulse</h1>

<p align="center"><strong>An independent DAW for creative music production.</strong></p>

<p align="center">
Pulse is a digital audio workstation written from scratch in Rust, with its own
real-time audio engine, VST3 and CLAP plugin hosting, and creative tools built in:
audio to MIDI, chords and bass lines that follow your song, stem separation and
mix analysis. They all run on your computer, offline and with no accounts.
</p>

<p align="center">
  <a href="https://github.com/davidfdzmorilla/pulse/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/davidfdzmorilla/pulse?label=release"></a>
  <a href="https://github.com/davidfdzmorilla/pulse/actions/workflows/macos.yml"><img alt="macOS CI" src="https://github.com/davidfdzmorilla/pulse/actions/workflows/macos.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/davidfdzmorilla/pulse/actions/workflows/windows.yml"><img alt="Windows CI" src="https://github.com/davidfdzmorilla/pulse/actions/workflows/windows.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="License: GPL-3.0-or-later" src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue"></a>
  <img alt="Platforms: macOS Apple Silicon, Windows x64 experimental" src="https://img.shields.io/badge/platforms-macOS%20(Apple%20Silicon)%20%C2%B7%20Windows%20x64%20(experimental)-555">
</p>

<p align="center">
  <a href="docs/DOWNLOADS.md"><strong>Download</strong></a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#what-you-can-do">Features</a> ·
  <a href="docs/guia.md">Documentation</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="https://github.com/davidfdzmorilla/pulse/discussions">Discussions</a>
</p>

<p align="center"><strong>English</strong> · <a href="README.es.md">Español</a></p>

![The Pulse Arrangement with a full track](docs/capturas/arrangement.png)

*The Arrangement: a full track with its sections, MIDI and audio clips, transitions and filter automation. ([More screenshots](#screenshots))*

> **Status: early public release, in active development.** macOS (Apple
> Silicon) is the supported platform. Windows builds are experimental. **The interface and the
> in-app guide are in Spanish for now.** Help with translation is very welcome.

## Platform status

| | macOS 13.4+ (Apple Silicon) | Windows 10/11 x64 | Linux |
|---|---|---|---|
| Download | [`Pulse-<version>.zip`](https://github.com/davidfdzmorilla/pulse/releases/latest) | Not published yet: [build it](docs/BUILD_WINDOWS.md) or use a CI artifact | — |
| Status | **Supported**: every release passes the full test suite and is tried by hand before publishing | **Experimental**: on CI's Windows it builds, passes the full test suite, packages, renders, opens its window and installs (2026-11-05); not tested with real audio/MIDI hardware or third-party plugins | Not supported |
| Signing | Self-signed, **not notarized** by Apple (one-time Gatekeeper step) | Unsigned (SmartScreen warning); updates are checked with Pulse's own Ed25519 signature | — |

Details per feature: [docs/PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) · Intel Macs are not supported yet.

## Getting started

1. Download `Pulse-<version>.zip` from the
   [latest release](https://github.com/davidfdzmorilla/pulse/releases/latest),
   unzip it and **move `Pulse.app` to Applications before opening it**.
2. The first time, macOS blocks it because it isn't notarized. Open
   **System Settings → Privacy & Security** and click **Open Anyway**.
   Full steps, checksums and uninstalling: [docs/DOWNLOADS.md](docs/DOWNLOADS.md).
3. On first start Pulse asks **¿Qué quieres hacer?** (what do you want to do?).
   You can listen to its example song, start from a genre template (house,
   techno, trap, reggaetón, pop, lo-fi, drum & bass, podcast), hum an idea,
   remix a song or follow one of its step-by-step **lessons**, which tick
   themselves off as you go.
4. Pulse updates itself from the GitHub releases (Pulse → Buscar
   actualizaciones…).

## What you can do

**Write**
- A Session View of clips and scenes launched in time, and an Arrangement with
  takes, comping, transitions, automation and automatic arranging into
  sections (intro, build-up, drop, break, outro).
- A MIDI editor with scales, chords, quantize and groove. Pitch bend, mod
  wheel, aftertouch and any CC are recorded and edited in lanes inside the
  clip.
- A project key, chords by scale degree and **Accompany**: bass lines (root,
  octaves, off-beat, walking, rolling, gallop), arpeggios, pads, stabs and
  **melodies** (hook, singable topline, trance lead, call and response) that
  follow the chords of a clip.
- **Generate beat** in the style you choose (house, techno, trap, reggaeton…),
  written out or played by a drum AI.
- **Hum to song**: hum a melody and Pulse harmonizes it and adds drums, bass
  and chords.

**Voice and audio**
- **Audio to MIDI** for vocals (pYIN) and for instruments or chords (Basic
  Pitch).
- **Tune a vocal note by note**, split a song into **stems** (HTDemucs) and
  follow the tempo of any audio (Beat This!).
- **Transcribe the lyrics** of a vocal with their timing (Whisper), correct
  them by hand and export them as .txt or .lrc.
- **Play with your voice**: sing, hum or beatbox into the microphone and a
  track plays it like a MIDI keyboard.

**Sound**
- Built-in instruments: Pulse Synth, Super Saw, Acid Bass, FM Synth,
  Wavetable, Polychrome, Modular, Pulse Keys (piano, electric pianos and
  organ), Sampler, Kick Designer, Drum Machine and Drum Pads, with hundreds of
  factory presets.
- **Pulse Sounds**: drum kits by genre and one-shots, made by Pulse itself with
  no third-party samples.
- 23 effects, including an 8-band EQ, compressor, **bus compressor**,
  **multiband (OTT)**, limiter, saturator, de-esser, the **Space** convolution
  reverb, delay, chorus, phaser, a **Vocoder** and **Voice: pitch and
  formants**.
- Savable effect chains, and **VST3 and CLAP** plugins with their editors.

**Mix and finish**
- A mixer with groups, returns, sidechain (also in one click from the kick)
  and latency compensation.
- A **mixing and mastering assistant**: it measures the mix, explains what it
  finds and, if you want, balances it, makes room for each track and brings it
  to the loudness of streaming, Apple Music, club, hard techno, radio and TV,
  or that of a reference song.
- Export to WAV, per-track stems and MIDI files; import of Ableton Live sets
  (`.als`).
- Project versions, autosave and recovery after an unexpected quit.

**Engine**
- Real-time audio designed with no locks or memory allocation on the audio
  thread, spread across several cores. The audio engine is designed for
  deterministic rendering across buffer sizes and thread counts. Automated
  tests cover this behavior, including repeatability of offline renders.

Everything is explained in the **guide** (in Spanish),
[`docs/guia.md`](docs/guia.md), which the app shows under Ayuda → Guía de Pulse
(with search). Every new feature updates the guide in the same pull request.

## Screenshots

![The Session view](docs/capturas/session.png)

*The Session view: clips and scenes to jam with, plus the mixer, sends and the Space and Delay returns.*

![The piano roll](docs/capturas/piano-roll.png)

*The piano roll: a melody written by Accompany over the chords, with its expression lane (brightness).*

![Devices](docs/capturas/dispositivos.png)

*Devices: the drum bus with the bus compressor and the Space convolution reverb.*

![The audio editor](docs/capturas/editor-de-audio-con-letra.png)

*The audio editor: the vocal with its transcribed lyrics, word by word, under the waveform.*

![The mix and master assistant](docs/capturas/asistente-de-mezcla.png)

*The mix and master assistant: it measures loudness, peaks and stereo, and suggests fixes you apply in one click.*

The screenshots come from a sample project built by a test, without opening the app. Regenerate them with `cargo test --release -p app readme_shots -- --ignored`.

## Build and run

Requirements, Windows and test details: [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo run --release
```

Projects are saved as `.pulse` files (JSON). `cargo test --release` runs the
tests, which need no sound card and open no windows. To render without the
interface: `target/release/pulse --render out.wav`.

## Code

- `crates/engine`: the real-time audio engine, instruments and effects.
- `crates/app`: the interface (Session, Arrangement, editors, mixer, browser)
  and the writing and mixing tools.
- `crates/stems`: analysis and AI models on ONNX Runtime (audio to MIDI,
  drums, stems, lyrics, chords and key).
- `crates/beats`: the tempo and bars of a piece of audio.
- `crates/paths`: where settings, data, caches and logs go on each system.
- `crates/firma`: the Ed25519 signature of published packages.
- `crates/testplug`: a minimal VST3/CLAP plugin to test plugin hosting.

## How the repository works

- `main` is always ready to release: it only gets finished, tested work,
  through pull requests. Nothing is pushed to `main` directly
  (`.githooks/pre-push` prevents it; enable it in each clone with
  `git config core.hooksPath .githooks`).
- Each task has its own branch: `funcion/<topic>` for something new,
  `arreglo/<topic>` for a fix, with as many commits as needed.
- When it's done, a pull request says what changes and how it was tested. The
  repository owner reviews it and squash-merges it: the pull request title (in
  Spanish) becomes a line of the release notes.
- Releasing: `scripts/release.sh [patch|minor|major]` from an up-to-date `main`
  (see [`docs/PUBLICAR.md`](docs/PUBLICAR.md)). The version comes from the
  latest tag (`v1.2.3`); the script runs the tests, signs Pulse.app, creates
  the tag and publishes it on GitHub, where the app updates from. The Windows
  package is added to that release from Windows with
  `scripts/package-windows.ps1` (see [`docs/BUILD_WINDOWS.md`](docs/BUILD_WINDOWS.md)).
- CI builds and tests Pulse for every pull request, natively on Windows
  (`.github/workflows/windows.yml`, which also packages and installs it) and
  on Apple Silicon (`.github/workflows/macos.yml`, which packages on `main`).
  CI packages are test builds, not releases.

## AI models

They all run on the CPU with ONNX Runtime, offline.

- **Audio to MIDI.** Vocal melodies come from pYIN
  (`crates/stems/src/pitch.rs`, matching `librosa.pyin`). Instruments and
  chords come from Basic Pitch (Spotify, Apache 2.0, see
  `crates/stems/models/BASIC_PITCH_NOTICE`), a 230 KB model built into the
  app. The Rust port is in `crates/stems/src/transcribe.rs`, and its test
  compares the notes with the original's:

      python scripts/transcribe/make_reference.py   # (with `basic-pitch[onnx]`)
      cargo test --release -p stems transcribe

  To compare pYIN with librosa on a file: `PULSE_PITCH_IN=voice.wav
  PULSE_PITCH_OUT=f0.txt cargo test --release -p stems dump_track -- --ignored --nocapture`.

- **Stem separation.** HTDemucs (Meta, MIT). The model (`htdemucs.onnx`,
  174 MB) is not built into the app: it is downloaded once from the
  repository's `modelo-htdemucs-1` release and kept in `Models`, inside the
  data folder (`~/Library/Application Support/Pulse` on macOS,
  `%LOCALAPPDATA%\Pulse` on Windows). To regenerate it, in a Python
  environment with `demucs onnx onnxruntime soundfile` (`demo.wav` is Pulse's
  example song, `target/release/pulse --render demo.wav`):

      python scripts/stems/export_htdemucs.py out/
      python scripts/stems/make_reference.py out/ demo.wav
      PULSE_STEMS_DIR=out/ cargo test --release -p stems -- --include-ignored

  The tests compare the spectrogram, the reconstruction, the network and the
  separation of a song with PyTorch's results. If the file changes, its size
  and SHA-256 must be updated in `crates/app/src/stems_ui.rs` and it must be
  published under another tag.

- **Lyrics.** Quantized Whisper small (OpenAI, MIT, see
  `crates/stems/models/WHISPER_NOTICE`), in `crates/stems/src/whisper.rs`. Its
  251 MB are downloaded the first time it is used, after a prompt, from a
  pinned revision of `onnx-community/whisper-small` on Hugging Face, with its
  SHA-256 checked, and kept in `Models/whisper-small`. The log-mel is compared
  with Hugging Face's (`scripts/lyrics/make_reference.py`).

- **Drums and tempo.** The drum AI (`crates/stems/models/DRUMS_NOTICE`) is
  built into the app. Tempo and bars come from Beat This! (JKU Linz), through
  its `beat-this` port.

## License

Copyright © 2026 David Fernández Morilla.

Pulse is free software: you can redistribute it and/or modify it under the
terms of the [GNU General Public License](LICENSE), version 3 or (at your
option) any later version (`GPL-3.0-or-later`). It comes with no warranty.

What Pulse includes from third parties (Rust crates, ONNX Runtime, the AI
models, the fonts) keeps its own license: they are all in
[`packaging/THIRD_PARTY_NOTICES.txt`](packaging/THIRD_PARTY_NOTICES.txt), which
ships in the macOS and Windows packages and shows in **Acerca de Pulse**
(About Pulse). After changing dependencies, regenerate it with
`python3 scripts/notices.py` (a test checks it is up to date).

To contribute: [CONTRIBUTING.md](CONTRIBUTING.md). How the open-source core
and any future paid services relate: [docs/PROJECT_MODEL.md](docs/PROJECT_MODEL.md).

### Trademarks

Ableton and Live are trademarks of Ableton AG; VST is a trademark of Steinberg
Media Technologies GmbH; CLAP is a free-audio standard; Roland, TR-808 and
TR-909 are trademarks of Roland Corporation. They are named only to describe
compatibility (importing `.als` sets, hosting VST3 and CLAP plugins) or a
style of sound. Pulse is not affiliated with or endorsed by any of them.
