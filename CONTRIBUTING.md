# Contributing to Pulse

**English** · [Español](CONTRIBUTING.es.md)

Thanks for wanting to make Pulse better. Pulse is a DAW written from scratch in
Rust, and it welcomes code, bug reports, plugin compatibility reports,
documentation, translations and sound design.

- **New here?** [docs/FIRST_CONTRIBUTIONS.md](docs/FIRST_CONTRIBUTIONS.md)
  lists scoped tasks, some of them right for a first PR.
- **How the code fits together:** [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
- **For big changes, open an issue first**, so we can agree on the idea
  before you invest time in it.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Project philosophy

- **Sound first.** A change that alters how Pulse sounds must show it doesn't
  make it worse. Most of the engine's tests check the sound itself.
- **The audio thread is sacred.** `Engine::process` never allocates, never
  locks and never waits for the OS. A stress test checks this
  (`crates/engine/tests/stress.rs`).
- **Determinism.** The engine is designed so a project renders the same at
  any buffer size and thread count, and an offline render repeats exactly.
  Automated tests cover both. A change that breaks them needs a very good
  reason.
- **Local and offline.** The analysis tools (audio to MIDI, stems, lyrics,
  tempo, chords) run on your machine. No accounts, no telemetry.
- **Honest status.** We don't call something supported until it has been
  tested on that platform. See [docs/PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md).

## Language

The app's interface and its user guide (`docs/guia.md`) are in **Spanish
(Spain)**. There is no localization layer yet, and building one is welcome
(see [docs/FIRST_CONTRIBUTIONS.md](docs/FIRST_CONTRIBUTIONS.md)).

- **Code comments:** English, short and useful.
- **UI text and the guide:** Spanish, informal (*tú*) and direct.
- **Issues and PR descriptions:** English or Spanish, whichever you prefer.
- **PR titles:** they become one line of the in-app release notes, which are
  in Spanish. If you don't write Spanish, write the title in English and the
  maintainer will adapt it when merging.

## Requirements

| What | Version | Notes |
|---|---|---|
| Rust | **1.99.0**, pinned in [`rust-toolchain.toml`](rust-toolchain.toml) | Install [rustup](https://rustup.rs). It fetches the right toolchain, clippy and the Windows target on first use. |
| macOS | 13.4 or later, Apple Silicon | The verified platform. Needs the Xcode Command Line Tools (`xcode-select --install`). |
| Windows | 10 1809+ / 11, x64 | Experimental. Needs Visual Studio 2022 Build Tools with the C++ workload. Full steps: [docs/BUILD_WINDOWS.md](docs/BUILD_WINDOWS.md). |
| Linux | — | Not supported yet (see [FIRST_CONTRIBUTIONS.md](docs/FIRST_CONTRIBUTIONS.md)). |
| Internet | first build | The `ort` crate downloads a prebuilt ONNX Runtime from `cdn.pyke.io` and caches it. |

You don't need CMake, the ASIO SDK, or a separate ONNX Runtime install.

## Clone, build, run

```bash
git clone https://github.com/davidfdzmorilla/pulse.git
cd pulse
git config core.hooksPath .githooks   # blocks accidental pushes to main
cargo run --release                   # builds and opens Pulse
```

Use `--release` for everything. Debug builds are much slower, and the UI tests
fail in debug because of an egui assertion.

Useful command-line modes of the `pulse` binary (`crates/app/src/main.rs`):

```bash
target/release/pulse --version
target/release/pulse --render out.wav                 # renders the example song, no window
target/release/pulse --render out.wav song.pulse      # renders a project
```

## Tests

```bash
cargo test --release                          # whole workspace (~1,200 tests, several minutes)
cargo test --release -p engine                # just the engine
cargo test --release -p engine --test stress  # real-time safety: no allocation on the audio thread
```

Tests need no sound card and open no windows. UI tests draw the interface
offscreen (`PulseApp::headless`, `crate::offscreen::Canvas`). Some tests are
`#[ignore]`d because they are long benchmarks or need large model files; their
doc comments say how to run them.

Before opening a PR, run the tests of every crate you touched. If you touched
the engine, run `-p app` too.

If you have the Windows target, check that Windows still compiles:

```bash
cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets
```

For every pull request, CI builds and tests Pulse natively on Windows
(`.github/workflows/windows.yml`, which also packages and installs it) and on
Apple Silicon (`.github/workflows/macos.yml`, new: it packages only on
`main`). The Windows job is currently red for reasons unrelated to most
changes: see [docs/FIRST_CONTRIBUTIONS.md](docs/FIRST_CONTRIBUTIONS.md#3-windows-audio-to-midi-with-any-number-of-threads).

## Formatting and lints

- **Don't run `cargo fmt` on the repository.** The code uses a wide,
  hand-kept style that rustfmt doesn't reproduce. Running it rewrites
  thousands of lines and buries your change. Match the style of the
  surrounding code by hand.
- **Clippy** is installed with the toolchain: `cargo clippy --release
  --workspace --all-targets`. CI runs it for information only; new warnings
  in code you touch are worth fixing.

## Architecture in one minute

| Crate | What it does |
|---|---|
| `crates/engine` | The real-time audio engine: transport, clip launching, arrangement playback, instruments, effects, mixer, and VST3/CLAP hosting. |
| `crates/app` | The `pulse` binary: the egui interface, projects, editors, recording, export, the updater and the writing/mixing tools. |
| `crates/stems` | Analysis and ML models on ONNX Runtime: audio to MIDI, drums, stems, lyrics, key and chords, loudness. |
| `crates/beats` | Tempo and bar detection (Beat This!). |
| `crates/paths` | Where settings, data, caches and logs live on each OS. |
| `crates/firma` | Ed25519 signatures of the published Windows packages. |
| `crates/testplug` | A minimal VST3/CLAP plugin used by the hosting tests. |

More detail, with the audio flow and threading rules: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Pull request workflow

1. Fork, then create a branch: `funcion/<topic>` for something new,
   `arreglo/<topic>` for a fix. Any name works on your fork.
2. Make your change with as many commits as you like, each signed off
   (`git commit -s`, see below).
3. If you changed something visible or usable in the app, update
   [`docs/guia.md`](docs/guia.md). The app shows it under Ayuda → Guía de
   Pulse, and `guide.rs` tests check its links.
4. If it's user-facing, add a line to [`CHANGELOG.md`](CHANGELOG.md) under
   **Unreleased**.
5. Open a pull request to `main` and fill in the template. Nothing is pushed
   to `main` directly.
6. The maintainer reviews and **squash-merges** it. The PR title becomes a
   line of the release notes that the app shows when it updates.

Releases are made by the maintainer from `main` with `scripts/release.sh` (see
[docs/PUBLICAR.md](docs/PUBLICAR.md)). Contributors never need to run it.

## Reporting bugs

Use the [issue forms](https://github.com/davidfdzmorilla/pulse/issues/new/choose):
**Bug report**, **Feature request**, **Plugin compatibility report** or
**Installation or platform issue**. The most useful things to include are:

- the version (`pulse --version`);
- the shortest steps that reproduce the problem;
- `pulse.log` (`~/Library/Logs/Pulse/` on macOS, `%LOCALAPPDATA%\Pulse\Logs`
  on Windows);
- on a macOS crash, the `pulse-*.ips` report. The maintainer can map it to
  source lines with `scripts/simbolos.py`.

## Licensing of contributions

> This section is an English translation for convenience. The original wording
> is the Spanish one in [CONTRIBUTING.es.md](CONTRIBUTING.es.md#licencia-de-las-contribuciones);
> if the two differ, the Spanish text applies.

Pulse is GPL-3.0-or-later (see [LICENSE](LICENSE)). By sending a contribution
(a pull request, a patch) you declare that:

1. **You have the right to contribute it.** It is your own work, or you have
   the right to publish it under this license, as described in the
   [Developer Certificate of Origin 1.1](https://developercertificate.org/).
   You confirm this by signing off each commit with `git commit -s`, which
   adds the line `Signed-off-by: Your Name <you@example.com>`.
2. **It is published under Pulse's license**, GPL-3.0-or-later.
3. **You also grant the author of Pulse, David Fernández Morilla, a worldwide,
   royalty-free, non-exclusive, perpetual and irrevocable license** to use,
   copy, modify, sublicense and distribute your contribution, including under
   other licenses, commercial ones included. You keep the copyright of what
   you contribute and can use it however you want.

Point 3 makes it possible to offer Pulse under other terms some day (for
example, a commercial license) without having to find every person who
contributed. If you don't agree with it, say so in the pull request before it
is merged. What this means for the project is explained in
[docs/PROJECT_MODEL.md](docs/PROJECT_MODEL.md).

Don't send code, samples, presets, models or images from other products
(commercial or not) unless their license allows it and you say which it is. If
you add a dependency, run `python3 scripts/notices.py` to update
`packaging/THIRD_PARTY_NOTICES.txt`. A test checks that it is up to date.
