# Pulse by platform

**English** · [Español](PLATFORM_SUPPORT.md)

- **Verified**: tested on that system.
- **Implemented**: the code exists and builds for that system, but has not been
  run on it.
- **CI**: checked in GitHub Actions (`windows-2022`, without a sound card, MIDI
  or third-party plugins), not on a real machine.
- **Partial**: with a limitation (see the note).
- **Not supported**: there is no implementation.

Linux appears only where the existing code accounts for it. It is not compiled,
tested or supported.

Windows details: [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md).

| Feature | macOS (Apple Silicon) | Windows 10/11 x64 | Linux |
|---|---|---|---|
| Native build | Verified | CI (MSVC; also links from macOS with `cargo xwin`) | Not supported |
| Automated tests | Verified (`cargo test --release`) | CI: all pass on `main` since 2026-11-05 (see below) | Not supported |
| Startup and window | Verified | CI on 2026-11-05: a script opens the window, drives it and saves a project, on a machine without a GPU (does not test real graphics cards) | Not supported |
| Data paths | Verified | Implemented (rules tested on macOS) | Implemented (XDG) |
| Open and save projects | Verified | Implemented | — |
| Projects across systems | Verified (Windows paths opened on macOS) | Implemented | — |
| Audio (output, input, buffer) | Verified (CoreAudio) | Partial: shared WASAPI, no ASIO | Not supported |
| Audio recording | Verified | Implemented | Not supported |
| MIDI input | Verified (CoreMIDI) | Implemented (WinMM) | Not supported |
| Built-in instruments and effects | Verified | Implemented (shared code) | — |
| Offline render (export, `--render`) | Verified | CI on 2026-11-05 (`pulse.exe --render`) | — |
| VST3 plugins: loading, sound, state | Verified | Implemented | Not supported |
| CLAP plugins: loading, sound, state | Verified | Implemented | Not supported |
| Plugin editors | Verified (NSView) | Implemented (HWND, display scaling) | Not supported |
| AI: drums, Basic Pitch, tempo | Verified (CPU) | Implemented (CPU; DirectML unused) | — |
| Stem separation (model download) | Verified | Implemented | — |
| Keyboard shortcuts | Verified (menu bar) | Implemented (Ctrl; tested with simulated input) | — |
| Open `.pulse` by double-click | Verified | Partial: CI checks the installer association; double-click is untested | — |
| Crash log | Verified | Implemented | Implemented |
| Package | Verified (Pulse.app, `bundle.sh`) | CI on 2026-11-05 (zip + Inno Setup: installs and uninstalls; `SHA256SUMS.txt`; not attached to releases yet) | Not supported |
| Updates | Verified (code signing) | Implemented (SHA-256 and Pulse Ed25519 signature; untested) | Not supported |
| Code signing | Partial (local certificate) | Not supported (no certificate) | — |

## Note on Windows CI

Since 2026-11-05 all Windows work passes on `main` (run
[42045040404](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040404),
commit `518120e`): build, tests, packaging, checksums, rendering, window
startup and installer. The macOS work is the same (run
[42045040332](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040332)).
None of this has been tested on a real Windows system.

## Windows CI tests

Between 2026-10-29 and 2026-11-05, Windows CI was red. These are the tests that
failed, why they failed and how they were fixed. None was disabled or changed
to a warning.

| Test | Failure | Cause | Fix |
|---|---|---|---|
| `a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there` | Always | Test failure: it expected `…\\Bombos/Bombo house.wav` (a `/` inside a `Path::join`) | Expected path is joined part by part (#209) |
| `lyrics_are_saved_and_opened_with_the_project` | Always | Same test failure | Same (#209) |
| `finds_the_notes_basic_pitch_finds` | Sometimes (and always on the CI M1 Mac) | Two note-start frames had to tie; a one-ulp rounding difference, varying by CPU and threads, broke the tie and the note started 1–2 frames late | `inferred_onsets` keeps the tie; new `one_thread_finds_the_same_notes` test |
| `more_threads_give_the_same_notes` | Always | It required ONNX Runtime activations to be bit-for-bit identical with 2 and 3 threads; on Windows x64 the last bits differ | Notes are identical in time and pitch, amplitude uses a 1e-4 tolerance and activations are below 3e-4; macOS still requires exact equality |
| Window startup from a script | Always since 2026-11-04 | Stack overflow (`0xC00000FD`): the welcome screen needs 1.25–1.5 MB while the Windows main thread has 1 MB | 8 MB stack when linking (`.cargo/config.toml`) |
| `with_threads_a_sidechain_waits_for_its_source` | Sometimes | Helper threads did not start in time (see below) | Threads get high priority as soon as they are created (#211); the test retries for up to one minute when threads have not participated and checks sound on every retry |
| `the_audio_threads_do_not_allocate_with_workers` | Sometimes | Helper threads started late and their first run fell inside the measurement; it did not reproduce when threads started on time | Same (#211); passes with no allocations in audio threads |

### Windows helper threads

A new Windows thread starts at normal priority and was promoted to audio priority
only when it ran its first instruction. When all tests run together, dozens of
threads from other tests occupy the CI's four processors at maximum priority,
and Windows always services them first: in diagnostics, none of the three
threads started in 81 of 82 repetitions. Now the thread that creates them raises
their priority immediately after creating them (`crates/engine/src/workers.rs`).
Pulse's cpal audio thread already runs at maximum priority.

One slowdown remains: the engine stress tests take about 27 minutes in Windows
CI; by themselves, the allocation test takes 34 seconds, and all of them
together on macOS take 82 seconds. They pass; see
[FIRST_CONTRIBUTIONS.md](FIRST_CONTRIBUTIONS.md#3-windows-ci-make-the-engine-stress-tests-fast-again).
