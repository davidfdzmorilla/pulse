# First contributions

Scoped tasks that are ready to be picked up. Each one was found by
reading the code (file and line references are from 2026-11-04, commit
`5347594`; task 3 is from 2026-11-05, task 6 from 2026-11-09). Each one is
also a GitHub issue (tasks 1–5 published on 2026-11-06, task 6 on
2026-11-09): comment on the issue you want before starting, so two people
don't do the same work. Done tasks stay listed, marked as done.

| # | Task | Issue | Area | Difficulty | Good first issue |
|---|---|---|---|---|---|
| 1 | ~~[English versions of the install and platform docs](#1-english-versions-of-the-install-and-platform-docs)~~ Done in [#219](https://github.com/davidfdzmorilla/pulse/pull/219) | [#214](https://github.com/davidfdzmorilla/pulse/issues/214) | Documentation | Easy | ✅ |
| 2 | [Readable dim text: fix `TEXT_DIM` contrast](#2-readable-dim-text-fix-text_dim-contrast) | [#215](https://github.com/davidfdzmorilla/pulse/issues/215) | UX / accessibility | Easy | ✅ |
| 3 | [Windows CI: make the engine stress tests fast again](#3-windows-ci-make-the-engine-stress-tests-fast-again) | [#213](https://github.com/davidfdzmorilla/pulse/issues/213) | Tests / CI | Medium | |
| 4 | [`cargo check` on Linux](#4-cargo-check-on-linux) | [#216](https://github.com/davidfdzmorilla/pulse/issues/216) | Platform | Medium | |
| 5 | [`pulse --help` and errors for unknown flags](#5-pulse---help-and-errors-for-unknown-flags) | [#217](https://github.com/davidfdzmorilla/pulse/issues/217) | App / CLI | Easy | ✅ |
| 6 | [CI: move `actions/checkout` and `actions/upload-artifact` to Node.js 24](#6-ci-move-actionscheckout-and-actionsupload-artifact-to-nodejs-24) | [#224](https://github.com/davidfdzmorilla/pulse/issues/224) | CI | Easy | ✅ |

Set-up for all of them: [CONTRIBUTING.md](../CONTRIBUTING.md).

---

## 1. English versions of the install and platform docs

**Done** in [#219](https://github.com/davidfdzmorilla/pulse/pull/219) (2026-11-09), by jiangyuer018.

**Labels:** `documentation`, `good first issue`, `help wanted`

**Problem.** The README is in English, but the documents it sends people to
are only in Spanish: [`INSTALL_MACOS.md`](INSTALL_MACOS.md) (44 lines),
[`INSTALL_WINDOWS.md`](INSTALL_WINDOWS.md) (112 lines) and
[`PLATFORM_SUPPORT.md`](PLATFORM_SUPPORT.md) (74 lines). An English-speaking
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

## 3. Windows CI: make the engine stress tests fast again

**Labels:** `windows`, `performance`, `help wanted`

**Problem.** The Windows CI (`.github/workflows/windows.yml`) is green, but
it takes about an hour, and most of that is one test binary:
`crates/engine/tests/stress.rs`. Its 9 tests (7 more are ignored) take
**1,328–1,631 seconds** on Windows CI, against **82 seconds** on an
Apple Silicon Mac. Run alone on the same Windows runner,
`the_audio_threads_do_not_allocate_with_workers` takes 34 seconds. Measured
on 2026-11-05 (runs
[42019354460](https://github.com/davidfdzmorilla/pulse/actions/runs/42019354460)
and [42023990962](https://github.com/davidfdzmorilla/pulse/actions/runs/42023990962)).

What is known. The runner has 4 logical processors. The engine's worker
threads (`crates/engine/src/workers.rs`) run at Windows' time-critical
priority, as Pulse's real audio thread does (cpal raises it). In the tests,
the thread that plays the audio thread's role is a plain test thread at
normal priority, and several tests run at once, each with its own pool of
time-critical workers that spin between blocks. A likely explanation, **not
yet proven**, is that those workers keep the normal-priority test threads
off the processors. The same effect stopped workers from starting at all
until 2026-11-05 ([background](PLATFORM_SUPPORT.md#pruebas-de-windows-en-la-ci),
Spanish).

**Goal.** The stress tests take a few minutes on Windows CI and check
exactly what they check today.

**Scope.** The tests and, if the cause is in the engine, the smallest change
there that fixes it. Don't skip, ignore or loosen a test, and don't remove
the real-time checks. Changing how Pulse itself schedules its threads needs a
measured reason, because it affects real users.

**Files.** `crates/engine/tests/stress.rs`, possibly
`crates/engine/src/workers.rs` and `.github/workflows/windows.yml`.

**Steps.**
1. Measure first. Run the binary on Windows CI with `--nocapture` and
   `--test-threads=1`, and note the time of each test. A draft PR runs the
   Windows workflow.
2. Check the explanation: for example, give the stress tests' "audio
   thread" the same priority as the real one, and compare.
3. Propose the fix with the numbers from the CI log.

**Acceptance criteria.** All 9 tests still pass on both CI jobs, with no
assertion changed. The stress binary takes under 5 minutes on Windows CI.
The PR explains the cause with measurements.

**Tests.** `cargo test --release -p engine --test stress` on both CI jobs.

**Knowledge.** Rust, threads. Knowing Windows scheduling helps. No Windows
machine needed: CI is the Windows test.

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

---

## 6. CI: move `actions/checkout` and `actions/upload-artifact` to Node.js 24

**Labels:** `good first issue`, `help wanted`

**Problem.** Every CI run shows this warning:

> Node.js 20 is deprecated. The following actions target Node.js 20 but are
> being forced to run on Node.js 24: actions/checkout@11d5960…,
> actions/upload-artifact@ea165f8…

The workflows use `actions/checkout` v4.4.0 and `actions/upload-artifact`
v4.6.2, both written for Node.js 20. They still work because GitHub runs them
on Node.js 24 anyway, but that is a stopgap: they should move to versions made
for Node.js 24 before GitHub stops allowing it.

**Goal.** No Node.js 20 warning on the macOS and Windows runs, with the
workflows doing exactly what they do today.

**Scope.** Only these two actions, in both workflows. `Swatinem/rust-cache`
v2.9.3 doesn't trigger the warning: leave it as it is.

**Files.** `.github/workflows/macos.yml` and `.github/workflows/windows.yml`
(one `checkout` and two `upload-artifact` steps in each).

**Steps.**
1. Pick the newest release of each action whose `action.yml` says
   `using: node24`. At the time of writing that's `actions/checkout` v7.0.1
   and `actions/upload-artifact` v7.0.2. Check before you start, in case
   there's a newer one.
2. Read their release notes from v4 up to that version, and check whether any
   change affects how we use them: `fetch-depth: 0` (the package version
   comes from the last git tag), the artifact names, `path` lists and
   `if-no-files-found: warn`.
3. Keep the actions **pinned to a full commit SHA**, with the tag in a
   comment, as they are now:
   `uses: actions/checkout@<40-character sha> # v7.0.1`. Take the SHA from
   the tag on GitHub, not from a third-party list.
4. Open the PR. It runs both workflows, so you'll see the result.

**Acceptance criteria.**
- Both workflows pass, and their logs show no Node.js 20 warning.
- The Windows package version is still the last tag (look for `Pulse-<version>`
  in the `pulse-windows-x64` artifact), not `0.0.0`.
- Both runs still keep the same artifacts with the same names and files.
- Every `uses:` is a full SHA with its tag in a comment.
- The PR says, in a line or two, what you checked in the release notes.

**Tests.** The two CI workflows themselves. No Rust changes.

**Knowledge.** GitHub Actions YAML. No Rust needed.
