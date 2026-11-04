# First contributions

Five scoped tasks that are ready to be picked up. Each one was found by
reading the code (file and line references are from 2026-11-04, commit
`5347594`). **They are proposals and not yet GitHub issues.** The maintainer
publishes them as issues (see [GITHUB_COMMUNITY_SETUP.md](GITHUB_COMMUNITY_SETUP.md#publishing-the-first-issues)).
Once they exist, comment on the issue you want so two people don't do the
same work.

| # | Task | Area | Difficulty | Good first issue |
|---|---|---|---|---|
| 1 | [English versions of the install and platform docs](#1-english-versions-of-the-install-and-platform-docs) | Documentation | Easy | ✅ |
| 2 | [Readable dim text: fix `TEXT_DIM` contrast](#2-readable-dim-text-fix-text_dim-contrast) | UX / accessibility | Easy | ✅ |
| 3 | [Windows: audio to MIDI with any number of threads](#3-windows-audio-to-midi-with-any-number-of-threads) | Tests / quality | Medium | |
| 4 | [`cargo check` on Linux](#4-cargo-check-on-linux) | Platform | Medium | |
| 5 | [`pulse --help` and errors for unknown flags](#5-pulse---help-and-errors-for-unknown-flags) | App / CLI | Easy | ✅ |

Set-up for all of them: [CONTRIBUTING.md](../CONTRIBUTING.md).

---

## 1. English versions of the install and platform docs

**Labels:** `documentation`, `good first issue`, `help wanted`

**Problem.** The README is in English, but the documents it sends people to
are only in Spanish: [`INSTALL_MACOS.md`](INSTALL_MACOS.md) (44 lines),
[`INSTALL_WINDOWS.md`](INSTALL_WINDOWS.md) (111 lines) and
[`PLATFORM_SUPPORT.md`](PLATFORM_SUPPORT.md) (37 lines). An English-speaking
user who hits a Gatekeeper or SmartScreen problem finds no help in their
language beyond the summary in [`DOWNLOADS.md`](DOWNLOADS.md).

**Goal.** English versions next to the Spanish ones, with links between them.

**Scope.** Only these three files. `docs/guia.md` (3,600 lines, shown inside
the app) is out of scope.

**Files.** New: `docs/INSTALL_MACOS.en.md`, `docs/INSTALL_WINDOWS.en.md`,
`docs/PLATFORM_SUPPORT.en.md`. Edit: the three Spanish files (a language link
at the top), `README.md` and `docs/DOWNLOADS.md` (point to the English ones).

**Steps.**
1. Translate each file. Keep the menu names as they appear in the app (in
   Spanish, e.g. **Ajustes → Audio**), with the English meaning in brackets
   the first time.
2. Add `**English** · [Español](INSTALL_MACOS.md)` (and the reverse) at the
   top of each pair.
3. Update the links in `README.md` and `docs/DOWNLOADS.md`.

**Acceptance criteria.**
- The English files say the same as the Spanish ones: no steps added or
  removed.
- Every relative link in the new files resolves (check them in GitHub's
  preview of your branch).
- macOS and Windows statuses are copied exactly. Don't upgrade "Implementado"
  to "works".

**Tests.** None (documentation). Review the rendered Markdown on your fork.

**Knowledge.** Reading Spanish, writing technical English, Markdown.

---

## 2. Readable dim text: fix `TEXT_DIM` contrast

**Labels:** `ui`, `accessibility`, `good first issue`

**Problem.** Secondary text uses `theme::TEXT_DIM = #8c8c8c`
(`crates/app/src/theme.rs:15`), often at a small size (10.5 pt, e.g.
`crates/app/src/about.rs:44`, `crates/app/src/cpu_meter.rs:170`). The WCAG
2.x contrast ratios of that color against the theme's backgrounds are:

| Background | Color | Ratio | WCAG AA (4.5:1) |
|---|---|---|---|
| `BG` | `#181818` | 5.28 | passes |
| `PANEL` | `#262626` | 4.50 | borderline |
| `SURFACE` | `#303030` | 3.92 | **fails** |
| `CONTROL` | `#3d3d3d` | 3.23 | **fails** |

**Goal.** Small secondary text that is readable on every surface it is drawn
on, without losing the visual hierarchy against `TEXT`.

**Scope.** The color constant(s) in `theme.rs` and a test. No layout changes.

**Files.** `crates/app/src/theme.rs` (and its tests).

**Steps.**
1. Add a small `fn contrast(a: Color32, b: Color32) -> f32` in a test module
   (WCAG relative luminance).
2. Write a test asserting `TEXT_DIM` reaches at least 4.5:1 against `BG`,
   `PANEL` and `SURFACE`. It fails today.
3. Pick a lighter grey that passes (around `#a0a0a0`) and stays clearly dimmer
   than `TEXT`. If dim text is also drawn on `CONTROL`, say where in the PR,
   and either include it or propose a separate constant.
4. Regenerate the README screenshots
   (`cargo test --release -p app readme_shots -- --ignored`) and attach
   before/after in the PR.

**Acceptance criteria.** The new test passes. `cargo test --release -p app`
passes. The screenshots show the change.

**Tests.** The new contrast test, plus the existing `-p app` suite.

**Knowledge.** Basic Rust. WCAG contrast is easy to learn on the way.

---

## 3. Windows: audio to MIDI with any number of threads

**Labels:** `windows`, `bug`, `help wanted`

**Problem.** One test fails on every run of the Windows CI
(`.github/workflows/windows.yml`), and passes on macOS:
`transcribe::tests::more_threads_give_the_same_notes`
(`crates/stems/src/transcribe.rs`, the `assert!(a == b)` at line 329).

The test runs the Basic Pitch model on 2 threads and on the default count
(`threads()`: the cores minus one, between 1 and 4, so 3 on CI's 4-core
runner). It then requires the two sets of activations to be **bit-identical**.
On macOS (Apple Silicon) they are. On Windows x64, ONNX Runtime's CPU
kernels may split and sum the work differently per thread count, so the
floats can differ in the last bits. The failure message is just
`assertion failed: a == b`, so nobody knows yet by how much they differ, or
whether the **notes** (which are what the user gets) differ too.

Failing runs, for example
<https://github.com/davidfdzmorilla/pulse/actions/runs/41794967564> (2026-11-04).

**Goal.** Know how far apart the two results are on Windows, and make the
test check what matters: that audio to MIDI gives the same notes whatever
the thread count.

**Scope.** This test, and only if needed the thread setup in
`Transcriber::load_on`. Don't weaken the test to make it pass. The notes
must stay identical. If they differ on Windows, that's a real bug to report,
not something to hide.

**Files.** `crates/stems/src/transcribe.rs`.

**Steps.**
1. Replace `assert!(a == b)` with a check that reports the largest absolute
   difference between the two activations, and how many values differ.
2. Push a branch and open a draft PR so the Windows CI runs. Read the numbers
   in its log.
3. If only the activations differ (by a tiny amount) and the notes are
   identical: compare activations within a stated tolerance, keep
   `assert_eq!` on the notes, and explain the tolerance in a comment.
4. If the notes differ too: don't loosen anything. Report it in the issue,
   with the numbers, so the fix can be chosen. One option is fixing the
   thread count for transcription.
5. Run it on macOS: `cargo test --release -p stems more_threads_give_the_same_notes`.

**Acceptance criteria.** The test passes on both CI jobs. Notes are still
compared exactly. Any tolerance on activations is justified by numbers from
the Windows log.

**Tests.** The test itself on both CI jobs, plus `cargo test --release -p stems`.

**Knowledge.** Rust, basic floating point. ONNX Runtime experience helps.
No Windows machine needed: CI is the Windows test.

**Status of every test that has failed on Windows CI.** This is the reference
table; other documents link here. Evidence: Windows CI runs 41627764299,
41654073501, 41697364751, 41789816848 and 41794967564 (2026-11-03/04, on
`main` or branches identical to it), Windows CI run 41808950553 (this branch at
`00d22e3`, 2026-11-04) and macOS CI run 41805367312 (2 attempts).

| Test | On Windows CI (`main`) | Cause | State on `funcion/preparar-lanzamiento` |
|---|---|---|---|
| `factory_sounds_tests::a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there` | Fails on every run | The test expected `…\Bombos/Bombo house.wav`; Pulse returns `…\Bombos\Bombo house.wav` (CI log). A test bug | **Fixed. Passes on Windows CI** (run [41808950553](https://github.com/davidfdzmorilla/pulse/actions/runs/41808950553), 2026-11-04) and on macOS |
| `lyrics::tests::lyrics_are_saved_and_opened_with_the_project` | Fails on every run | Same: a `/` inside one `Path::join` in the expected value | **Fixed. Passes on Windows CI** (run [41808950553](https://github.com/davidfdzmorilla/pulse/actions/runs/41808950553), 2026-11-04) and on macOS |
| `transcribe::tests::more_threads_give_the_same_notes` | Fails on every run | Requires bit-identical ONNX activations with 2 and 3 threads. The size of the difference hasn't been measured | **Pending**: this task (#3) |
| `transcribe::tests::finds_the_notes_basic_pitch_finds` | Fails on some runs. Also fails **on every run of GitHub's macOS (M1) runner** (2 of 2) | Two onset frames meant to tie; the tie was broken by one-ulp rounding, so a note started 1–2 frames late. Reproduced locally with 1 thread | **Fixed** in `inferred_onsets`, and the new test `one_thread_finds_the_same_notes` reproduces it. Passes on macOS (1–4 threads). **Not yet run on the M1 runner or Windows** |
| `engine::tests::with_threads_a_sidechain_waits_for_its_source` | Fails on some runs ("the other threads did nothing") | Not established. Likely the test needs worker threads to win a race | **Pending**. Not reproduced on macOS |
| `the_audio_threads_do_not_allocate_with_workers` (`crates/engine/tests/stress.rs:669`) | Fails on some runs ("the workers allocate") | **Unknown**. It is a real-time-safety check | **Pending, needs the maintainer**. Not reproduced on macOS |

Summary: on `main`, 3 tests fail on every Windows run and 3 more on some runs.
This branch fixes 3 of the 6: the 2 path tests (passing on Windows CI) and
`finds_the_notes_basic_pitch_finds` (checked on macOS only so far). It leaves 1
reproducible failure (`more_threads_give_the_same_notes`) and 2 intermittent
ones pending.

To check the two path fixes on macOS:

```bash
cargo test --release -p app factory_sounds_tests
cargo test --release -p app lyrics
```

---

## 4. `cargo check` on Linux

**Labels:** `enhancement`, `help wanted`

**Problem.** Linux is not supported ([PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md)),
but much of the code already has Linux branches:
- XDG folders in `crates/paths`;
- plugin editor stubs in `crates/app/src/editor.rs` for
  `not(any(macos, windows))`;
- VST3 module entry in `crates/engine/src/plugin.rs`;
- CLAP in `crates/engine/src/clap_host.rs`;
- `crates/app/src/open_files.rs`.

Other places treat "not Windows" as "macOS":
- `crates/app/src/updates.rs` (`.app` bundle names, `/usr/bin/codesign`, the
  `system` module);
- `crates/app/src/theme.rs` (loads `/System/Library/Fonts/...` in the
  non-Windows branch).

Nobody knows whether it compiles today. A first step toward Linux is making
it compile, without claiming it works.

**Goal.**
`cargo check --release --target x86_64-unknown-linux-gnu --workspace --all-targets`
passes on Linux, and CI keeps it that way.

**Scope.** Compilation only: no packaging, no plugin editor implementation,
and no claim of Linux support. On Linux the updater should be disabled with
a clear message rather than ported.

**Files.** `crates/app/src/updates.rs`, `crates/app/src/theme.rs`, possibly
`crates/app/src/logs.rs` and `crates/app/src/main.rs`, and a new job in a
workflow (e.g. `.github/workflows/linux-check.yml` on `ubuntu-24.04`).

**Steps.**
1. On Linux (or in CI), install ALSA and X11/Wayland development packages
   (`libasound2-dev`, `libxkbcommon-dev`, `libwayland-dev`…). Run the
   `cargo check` above and list the errors in the issue.
2. Add `#[cfg(target_os = "linux")]` (or `not(any(macos, windows))`) arms:
   the updater reports "updates are not available on this system", and fonts
   fall back to egui's defaults when the macOS paths are missing.
3. Add the CI job (check only, no tests at first).
4. Update the Linux column in `PLATFORM_SUPPORT.md` only where it's now
   true: "compiles (cargo check)".

**Acceptance criteria.** The CI job is green. macOS and Windows behaviour is
unchanged (their CI jobs pass). No file claims Linux is supported.

**Tests.** The new CI job, plus the existing macOS/Windows jobs.

**Knowledge.** Rust `cfg` attributes, Linux system packages, GitHub Actions.

---

## 5. `pulse --help` and errors for unknown flags

**Labels:** `enhancement`, `good first issue`

**Problem.** The `pulse` binary has several command-line modes
(`crates/app/src/main.rs:180-230`): `--version`, `--render <out.wav>
[project.pulse]`, `--scan-plugin <bundle>` (internal), `--check-menus`
(macOS) and `--check-editors`. There is no `--help`, and an unknown flag
such as `pulse --rendr out.wav` silently opens the GUI. On Windows it does so
after attaching to the console (`main.rs:188`). Someone scripting renders
gets no error.

**Goal.** `pulse --help` (and `-h`) prints the usage. An unknown argument
starting with `--` prints an error and the usage to stderr and exits with
code 2.

**Scope.** `main.rs` only. Keep existing flags exactly as they are.
**Arguments that aren't flags must still open the GUI**: on Windows, a
double-clicked `.pulse` file arrives as an argument (`open_files.rs:35-46`).

**Files.** `crates/app/src/main.rs`.

**Steps.**
1. Write a `fn usage() -> &'static str` in the style of the existing message
   (`"Uso: pulse --render <salida.wav> [proyecto.pulse]"`, line 200). The CLI
   texts are Spanish like the rest of the app.
2. Handle `--help`/`-h` right after `--version` (before `logs::install()`).
3. After the known flags, if `args[1]` starts with `--`, print the error
   and the usage and exit with code 2. Don't list `--scan-plugin` in the help
   (it's internal), but keep accepting it.
4. Add a test that runs the built binary (`env!("CARGO_BIN_EXE_pulse")`,
   like `crates/app/tests/scan_plugin.rs`) with `--help` and with
   `--no-such-flag` and checks the exit codes.

**Acceptance criteria.**
- `pulse --help` exits with code 0 and prints the usage.
- `pulse --nope` exits with code 2.
- `pulse song.pulse` still opens the app.
- `cargo test --release -p app` passes.

**Tests.** The new integration test.

**Knowledge.** Basic Rust and `std::process`.
