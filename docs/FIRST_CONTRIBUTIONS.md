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
| 3 | [Make the Windows test suite pass: path separator bugs](#3-make-the-windows-test-suite-pass-path-separator-bugs) | Tests / quality | Easy | ✅ |
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

## 3. Make the Windows test suite pass: path separator bugs

**Labels:** `windows`, `bug`, `good first issue`

**Problem.** The Windows CI (`.github/workflows/windows.yml`) has been
failing on `main` since 2026-10-29. Two of the four failing tests are bugs in
the tests themselves. They build an expected path by joining a string that
contains `/`, so on Windows they expect a path with mixed separators:

- `crates/app/src/factory_sounds_tests.rs:271`:
  `dir.join("Bombos/Bombo house.wav")`.
- `crates/app/src/lyrics.rs:307` and `:320`:
  `dir.join("Canción/Audio/voz.wav")`. Opening the project resolves the
  relative path with the system separator (`crates/app/src/library.rs`,
  `resolve`), so the loaded path is `…\Canción\Audio\voz.wav` and the
  comparison fails.

Failing run: <https://github.com/davidfdzmorilla/pulse/actions/runs/41789816848>
(`factory_sounds_tests::a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there`,
`lyrics::tests::lyrics_are_saved_and_opened_with_the_project`).

**Goal.** Both tests pass on Windows and still pass on macOS.

**Scope.** These two tests only. Fix the expected paths, not the app code. If
you find the app is the one that's wrong, say so in the issue first.

**Files.** `crates/app/src/factory_sounds_tests.rs`, `crates/app/src/lyrics.rs`.

**Steps.**
1. Build the expected paths component by component:
   `dir.join("Bombos").join("Bombo house.wav")`, and the same for
   `Canción/Audio/voz.wav`.
2. Check that line 318 of `lyrics.rs` (the saved file stores
   `"Audio/voz.wav"` with `/`) still holds. That one is intentional:
   projects store `/` on every OS.
3. Run `cargo test --release -p app factory_sounds_tests lyrics` on macOS.
4. Open the PR. The Windows CI job is the test on Windows.

**Acceptance criteria.** Both tests pass in the Windows CI job and on macOS.

**Tests.** The two tests themselves, on both CI jobs.

**Knowledge.** Basic Rust, `std::path::Path`. No Windows machine needed.

**Follow-ups (not for a first PR).** The other two Windows failures need
more care:
- `engine::tests::with_threads_a_sidechain_waits_for_its_source`
  (`crates/engine/src/engine.rs:6380`) expects worker threads to have
  "helped". In this two-track graph the second track depends on the first,
  so the audio thread can win every race, especially with Windows' coarser
  thread wake-up.
- `transcribe::tests::more_threads_give_the_same_notes`
  (`crates/stems/src/transcribe.rs:329`) requires bit-identical ONNX
  activations with 2 and 4 threads, which ONNX Runtime's x64 CPU kernels
  may not guarantee.

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
