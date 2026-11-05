# Pulse: launch readiness audit

**Date:** 2026-11-04 · **Commit audited:** `5347594` (`main`, release
v0.1.121) · **Auditor:** Claude Code, working on branch
`funcion/preparar-lanzamiento`.

> **Update, 2026-11-05.** This is a dated record and is not kept up to date.
> Since it was written, the launch work was merged (#209, #211), and both CI
> workflows are green on `main` (Windows run
> [42045040404](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040404),
> macOS run [42045040332](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040332)).
> All the Windows test failures and the scripted window start are fixed. The
> code of conduct contact, private vulnerability reporting and Discussions
> are set up. Current state: [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md#pruebas-de-windows-en-la-ci)
> and [ROADMAP.md](ROADMAP.md).

This audit describes the repository **as it was before** the launch
preparation work. What was changed in response is in
[LAUNCH_PREPARATION_REPORT.md](LAUNCH_PREPARATION_REPORT.md). Every claim
cites a file, a command or a CI run. Nothing here comes from opening the GUI:
the project's rules forbid opening app windows during automated work
(`.claude/normas-pulse.md`).

## Executive summary

Pulse is an unusually mature codebase for a project that is three weeks old
on GitHub (first commit 2026-10-14, 260 commits, 122 releases). It has
≈133k lines of Rust, ≈1,280 tests, real-time-safety and determinism tests,
an auto-updater with signature checks, crash recovery and honest platform
docs. The **macOS (Apple Silicon) product is ready to be tried by the
public.** The full test suite passed during this audit.

What wasn't ready is everything **around** the code:
- there were no community files (Code of Conduct, issue/PR templates,
  security policy, changelog);
- the main README was in Spanish;
- the Windows status docs were out of date in both directions (CI does run,
  and it has been failing for six days);
- three settings that only the maintainer can change are still pending: the
  Code of Conduct contact, private vulnerability reporting and Discussions.

The largest product barrier for an international launch is that **the
interface and the user guide are Spanish-only, with no localization layer.**

**Overall launch readiness (before this work): 5.0 / 10.** On macOS, a
"soft launch" is feasible as soon as the P0 items are closed.

## Scores (before)

| Category | Score | Why |
|---|---|---|
| Product (macOS) | 7.5 | Broad, tested feature set. Minus: Spanish-only UI, Apple Silicon only, not notarized |
| Documentation | 6 | Extensive and honest, but mostly Spanish; Windows docs out of date |
| First impression (README) | 6 | Good English README with real screenshots, but it wasn't the default (`README.md` was Spanish); no logo, no quick links, no video |
| Download experience | 6 | Signed macOS zip on every release, self-updating; Gatekeeper step required; no Windows package |
| Community readiness | 3 | GitHub community profile 57 %: only README, CONTRIBUTING, LICENSE |
| Contributor experience | 5 | Clear workflow rules, but CONTRIBUTING was 44 lines in Spanish with no setup, architecture or first tasks |
| Cross-platform readiness | 4 | Windows builds and runs on CI but tests are red; no real-hardware tests; no Linux; no Intel Mac |
| **Overall** | **5.0** | |

---

## 1. Product

### 1.1 Product

Classification key:
- **VERIFIED**: automated tests pass on macOS in this audit (`cargo test
  --release --locked --workspace`: 1,196 passed, 0 failed, 84 ignored), and
  `docs/PLATFORM_SUPPORT.md` marks it verified on macOS.
- **PARTIALLY VERIFIED**: the logic is tested, but the hardware or model part
  is only verified by the maintainer by hand, or its accuracy tests are
  ignored by default.
- **IMPLEMENTED_UNVERIFIED**: the code exists and compiles for the platform,
  but it hasn't run there.
- **EXPERIMENTAL / NOT_IMPLEMENTED**: as named.

| Feature | Status (macOS) | Evidence |
|---|---|---|
| Session view, clip/scene launching, follow actions | VERIFIED | `crates/app/src/session.rs`, `scene.rs`; tests `a_high_scene_launches`, `launches_while_recording_are_written_onto_the_arrangement` |
| Arrangement, takes/comping, automation | VERIFIED | `arrangement.rs`, `comping.rs`, `automation_rec.rs`; `comping_in_the_app_selects_the_piece_and_undoes` |
| Piano roll, scales, CC/pitch-bend/aftertouch lanes | VERIFIED | `piano_roll.rs`, `expression.rs`, `crates/engine/src/expression_tests.rs` |
| 12 built-in instruments (Synth, Super Saw, Acid, FM, Wavetable, Polychrome, Modular, Keys, Sampler, Kick, Drum Machine, Drum Pads) | VERIFIED | `crates/engine/src/model.rs` `InstrumentKind`; `every_factory_preset_sounds_and_leaves_headroom`, `every_model_plays_in_tune` |
| 23 built-in effects (README said 22) | VERIFIED | `Effect::catalog() -> [Effect; 23]` in `crates/engine/src/model.rs:3448` |
| VST3 and CLAP hosting (load, sound, state) | VERIFIED with the test plugin; PARTIALLY VERIFIED with third-party plugins (maintainer, Surge XT) | `crates/engine/src/plugin.rs` tests using `crates/testplug`; `clap_plugins_work_like_vst3_ones` |
| Plugin editors (NSView) | PARTIALLY VERIFIED: needs a window, so no automated test | `crates/app/src/editor.rs`; `docs/PLATFORM_SUPPORT.md` |
| Plugin crash rescue, out-of-process scan | VERIFIED | `plugin_guard.rs`; `crates/app/tests/scan_plugin.rs` |
| Audio I/O and recording (CoreAudio) | PARTIALLY VERIFIED: logic tested offline; device path verified by the maintainer | `recording.rs`; `docs/PLATFORM_SUPPORT.md` |
| MIDI input, Launchpad/APC control | PARTIALLY VERIFIED: parsing and mapping tested; hardware by the maintainer | `input.rs` `running_status_clock_and_sysex`, `grid_io.rs` |
| Offline render, WAV/stem/MIDI export | VERIFIED | `export.rs`, `export_identity_tests.rs`. During this audit the bundled binary rendered the example song (42 MB WAV, peak −0.3 dBFS) |
| Ableton `.als` import | VERIFIED | `als.rs` `imports_tempo_loop_scenes_and_mixer` |
| Save/load, autosave/recovery, project versions, undo | VERIFIED | `persist_tests.rs` `audit_rich_project_round_trip_is_bit_identical`; `recovery.rs`; `versions.rs` |
| Groups, returns, sidechain, latency compensation | VERIFIED | `groups.rs`, `sidechain.rs`; `a_late_effect_keeps_its_track_in_time_with_the_rest` |
| Mix/master assistant, LUFS | VERIFIED | `mix_assist.rs`; `crates/stems/src/loudness.rs` `a_sine_at_1khz_measures_as_the_standard_says` |
| Audio to MIDI (pYIN, Basic Pitch) | VERIFIED | `crates/stems/src/pitch.rs`, `transcribe.rs` `finds_the_notes_basic_pitch_finds` |
| Stem separation (HTDemucs, 174 MB download) | PARTIALLY VERIFIED: model accuracy tests are `#[ignore]` (need `PULSE_STEMS_DIR`) | `crates/stems/src/lib.rs`, `crates/app/src/stems_ui.rs` |
| Lyrics transcription (Whisper, 251 MB download) | PARTIALLY VERIFIED: end-to-end tests ignored (need the model and macOS `say`) | `crates/stems/src/whisper.rs`, `crates/app/src/lyrics.rs` |
| Drum AI / Generate beat, tempo detection, key/chords, Accompany, hum to song, play with your voice | VERIFIED | `crates/stems/src/drums.rs`, `crates/beats`, `key.rs`, `chord.rs`, `accompany.rs`, `hum_song.rs`, `voice_play.rs` |
| Auto-updater (macOS, codesign identity check) | PARTIALLY VERIFIED: logic tested; live tests ignored; used by the maintainer for 121 updates | `crates/app/src/updates.rs` |
| First-run screen, templates, lessons | VERIFIED | `welcome.rs`, `templates.rs`, `lessons.rs` |
| Real-time safety, determinism across buffer sizes/threads | VERIFIED | `crates/engine/tests/stress.rs` (9 pass, 7 long ones ignored) |
| **Windows** (everything above) | EXPERIMENTAL | See §3 |
| Interface localization (any language but Spanish) | NOT_IMPLEMENTED | No i18n layer; UI strings are Spanish literals (e.g. `crates/app/src/about.rs:51`) |
| Linux, Intel macOS, Windows ARM64 | NOT_IMPLEMENTED | `rust-toolchain.toml` targets; `docs/PLATFORM_SUPPORT.md` |
| ASIO, AU, VST2 | NOT_IMPLEMENTED | `docs/WINDOWS_SUPPORT.md`; no AU/VST2 code in `crates/*/src` |

**What sets Pulse apart** (each point checked in code, without comparing
quality or performance against other DAWs):
- local, offline analysis tools built into the DAW: audio→MIDI, stems,
  lyrics with timing, tempo, key and chords;
- composing helpers that follow the song's chords (Accompany, chord track);
- an explaining mix/master assistant;
- an engine designed for deterministic rendering across buffer sizes and
  thread counts, with automated tests covering it;
- free software with no accounts.

### Main limitations

1. Spanish-only interface and guide.
2. macOS: Apple Silicon only, self-signed and not notarized.
3. Windows: no release package; tests red on CI; nothing tested on real
   hardware.
4. The two large models download on first use (about 425 MB in total).
5. A single maintainer.

## 2. README

| Question | Before | Evidence |
|---|---|---|
| Does a producer understand Pulse in 10 s? | Partly. The Spanish default README explained it well, but in Spanish | `README.md` was Spanish, English was in `README.en.md` |
| Does a developer see why to care? | Partly. Rust, the engine claims and the AI models were there, but deep in the page | README "Engine" and "AI models" sections |
| Quick download? | No direct link at the top. The download only appeared inside `docs/INSTALL_MACOS.md` (Spanish) | — |
| Real demo? | Six real screenshots generated by a test (`crates/app/src/readme_shots.rs`); no video | `docs/capturas/` |
| Supported platforms? | Yes, in a "Systems" section at the bottom | — |
| Docs and community? | Guide linked; no Discussions, no CoC, no templates | `gh api …/community/profile` → 57 % |
| What makes Pulse different? | Implicit in the feature list; no positioning line | — |
| Badges | None (fine; none were fake) | — |

## 3. Distribution

### macOS

| Item | State | Evidence |
|---|---|---|
| Target / arch | `aarch64-apple-darwin` only, macOS ≥ 13.4 | `rust-toolchain.toml`, `.cargo/config.toml`, `packaging/Info.plist` |
| Bundle | `scripts/bundle.sh` → `Pulse.app` with `dist` profile, stripped binary, separate dSYM, check that no `$HOME` paths remain | `scripts/bundle.sh` |
| Dynamic deps | ONNX Runtime is linked statically by `ort`. `otool -L` on the bundled `pulse` lists only `/System/Library` frameworks and `/usr/lib` libraries | this audit (ad hoc bundle built with `scripts/bundle.sh`) |
| Models/resources | Basic Pitch, drums and Beat This! are embedded. HTDemucs and Whisper are downloaded with a SHA-256 check | `crates/app/src/stems_ui.rs`, `lyrics_ui.rs` |
| Package | `Pulse-X.Y.Z.zip` + `-simbolos.zip` on every release (122 releases) | `gh release view v0.1.121` |
| Checksums | GitHub publishes a SHA-256 digest per asset; no `SHA256SUMS` file | `gh api …/releases/latest` → `digest: sha256:…` |
| Signing | Self-signed "Pulse Local Signing". **Not Developer ID, not notarized** | `docs/PUBLICAR.md`, `scripts/release.sh` |
| Gatekeeper | One-time "Open Anyway", documented | `docs/INSTALL_MACOS.md` |
| DMG | None | — |
| Install/uninstall docs | Yes (Spanish) | `docs/INSTALL_MACOS.md` |
| CI | **None for macOS.** macOS was tested only locally before each PR | `.github/workflows/` had only `windows.yml` |

### Windows

| Level | State | Evidence |
|---|---|---|
| Source available | ✅ | — |
| Cross-compilation verified | ✅ `cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets` passes (re-run in this audit; only the expected llvm-rc warning) | this audit |
| Native build verified | ✅ on CI (`windows-2022`, MSVC) | run [40853681048](https://github.com/davidfdzmorilla/pulse/actions/runs/40853681048), 2026-10-29 |
| Native execution verified | ✅ on CI, headless: `pulse.exe --render` wrote a 5.5 MB WAV; window started and ran `scripts/ci/arranque.txt` in 42 s | same run, log |
| Installation verified | ✅ on CI: silent install, `.pulse` association, Start-menu link, uninstall keeps settings | same run |
| Test suite | ❌ **Red on `main` since 2026-10-29 15:29** (first failing push: PR #174, after the multi-core engine #173). 4 failures in the latest run | run [41789816848](https://github.com/davidfdzmorilla/pulse/actions/runs/41789816848) |
| Audio and plugin QA | ❌ Not done: no Windows machine | `docs/WINDOWS_SUPPORT.md` |
| Published package | ❌ No release has a Windows asset | `gh release view` (all versions) |
| Runtime | VC++ runtime DLLs packaged next to `pulse.exe`; DirectML delay-loaded | `scripts/package-windows.ps1`, `.cargo/config.toml` |
| Signing | No Authenticode. The updater requires Pulse's Ed25519 `.firma` | `crates/firma`, `crates/app/src/updates.rs` |
| CI artifact version | ❌ CI packages say `0.1.3`, because the shallow checkout has no tags and falls back to `Cargo.toml` | run 40853681048 log: "Instalado: … versión 0.1.3" |

**Docs out of date:**
- `docs/PLATFORM_SUPPORT.md` and `docs/WINDOWS_SUPPORT.md` said CI was
  "blocked by GitHub billing" and that nothing had run on Windows.
- `PLATFORM_SUPPORT.md` said Windows updates were "SHA-256; sin firma", but
  the code requires an Ed25519 signature (`updates.rs:465`).

## 4. GitHub community

| Item | In repo (before) | Needs web setting | State |
|---|---|---|---|
| README / LICENSE (GPL-3.0) | ✅ | — | — |
| CONTRIBUTING.md | ✅ (Spanish, 44 lines) | — | — |
| CODE_OF_CONDUCT.md | ❌ | contact address needed | — |
| SECURITY.md | ❌ | private vulnerability reporting | **disabled** |
| Issue templates / PR template | ❌ | — | — |
| CHANGELOG.md | ❌ (release notes exist on GitHub) | — | — |
| FUNDING.yml | ❌ | sponsorship account | none defined |
| Discussions | — | ✅ | **disabled** |
| Labels | defaults + `accessibility` | ✅ | no area or platform labels |
| Topics, website | — | ✅ | none |
| Developer docs | partial (README "Code", `docs/PUBLICAR.md`, `.claude/normas-pulse.md`) | — | — |

## 5. Code quality signals

| Check | Result |
|---|---|
| `cargo test --release --locked --workspace` (macOS) | ✅ 1,196 passed, 0 failed, 84 ignored |
| `cargo check` for Windows (cross) | ✅ |
| `cargo fmt --all -- --check` | ❌ by design: 188 files differ. The project keeps a hand style and documents not using rustfmt (`CONTRIBUTING.md`, `docs/WINDOWS_SUPPORT.md`) |
| `cargo clippy --release --workspace --all-targets` | Compiles; ≈280 warnings in non-test code (app 182, engine 80, stems 12, testplug 4, beats 1). Mostly style: `collapsible_if` ×121, `redundant_guard` ×33 |
| `TODO`/`FIXME`/`todo!()`/`unimplemented!()` | 0 |
| Pinned pre-release dependency | `ort = "=2.0.0-rc.14"` (`crates/stems/Cargo.toml`) |
| `Cargo.toml` version | `0.1.3` placeholder; the real version comes from git tags (`crates/app/build.rs`) |

## 6. Hybrid model and licensing

- `LICENSE`: GPL-3.0 text; README states GPL-3.0-or-later. Consistent.
- `CONTRIBUTING.md` already had a DCO sign-off plus an **extra non-exclusive
  license to the author allowing relicensing, including commercial**. This
  supports a hybrid model and doesn't contradict the GPL. Its
  enforceability as a CONTRIBUTING clause (not a signed CLA) needs legal
  review. None of the 30 most recent commits has a `Signed-off-by` line
  (they are the maintainer's own, so that's no problem yet).
- Third-party notices: generated (`scripts/notices.py`), shipped in both
  packages, checked by a test.
- Models: Basic Pitch (Apache-2.0), HTDemucs (MIT), Whisper (MIT), Beat
  This! (MIT), and the drum model trained by Pulse on Lakh MIDI + Groove MIDI
  (CC BY 4.0, attributed in `crates/stems/models/DRUMS_NOTICE`). Lakh's
  underlying songs are commercial, which is a point for legal review if the
  model is ever used commercially.
- Trademarks: the README note covers Ableton/VST/Roland. "OTT" (Xfer
  Records) appears as an effect/preset name in the guide.
- No legal entity exists. The copyright holder is an individual.

## 7. Risks

| Risk | Impact | Likelihood |
|---|---|---|
| English-speaking users open a Spanish-only app | High churn after download | High |
| Gatekeeper "Open Anyway" step on every first install | Drop-off, "is this malware?" questions | High |
| Windows users expect a download | Disappointment, issues | High once promoted |
| Windows CI red while contributors open PRs | Contributors think they broke something | High |
| Single maintainer, very fast release cadence (122 releases in 3 weeks) | Review bottleneck, regressions | Medium |
| Contribution relicensing clause challenged | Legal uncertainty for a future commercial offer | Low–medium |
| Loss of the signing certificate / Ed25519 key | Every install stops accepting updates | Low, documented in `docs/PUBLICAR.md` |

## 8. Prioritised tasks

| ID | Priority | Task | Owner | Done in this branch? |
|---|---|---|---|---|
| A1 | **P0** | Code of Conduct with a real moderation contact | Maintainer (contact) | File added; **contact pending** |
| A2 | **P0** | Security policy + enable private vulnerability reporting | Maintainer (setting) | File added; **setting pending** |
| A3 | **P0** | Merge this documentation into `main` (nothing is public until then) | Maintainer | Pending |
| A4 | **P0** (before the first external merge) | Legal review of the contribution licence clause | Maintainer + lawyer | Documented in `docs/PROJECT_MODEL.md` |
| A5 | P1 | English README as default, clear positioning, quick links, platform table | — | ✅ |
| A6 | P1 | Issue forms, PR template, CHANGELOG | — | ✅ |
| A7 | P1 | English CONTRIBUTING, architecture doc, first-contribution tasks | — | ✅ |
| A8 | P1 | Downloads page with checksums, Gatekeeper and uninstall | — | ✅ `docs/DOWNLOADS.md` |
| A9 | P1 | Correct the Windows status docs | — | ✅ |
| A10 | P1 | Get Windows CI green (4 tests) | Contributors / maintainer | Scoped in `FIRST_CONTRIBUTIONS.md` #3 |
| A11 | P1 | macOS CI for outside PRs | — | ✅ `.github/workflows/macos.yml` (not yet run) |
| A12 | P1 | Enable Discussions, create labels and topics | Maintainer | Steps in `docs/GITHUB_COMMUNITY_SETUP.md` |
| A13 | P1 | Record the demo video | Maintainer | Plan in `docs/DEMO_VIDEO_PLAN.md` |
| A14 | P1 | Start a localization layer (English UI) | Contributors | Mentioned in the roadmap; not scoped as an issue |
| A15 | P2 | Apple Developer ID + notarization | Maintainer (costs money) | Documented |
| A16 | P2 | Windows real-hardware QA, then attach Windows packages to releases | Maintainer / testers | Documented |
| A17 | P2 | CI packages carry the right version; CI checksums | — | ✅ `fetch-depth: 0`, `SHA256SUMS.txt` |
| A18 | P2 | `SHA256SUMS` file on releases (`scripts/release.sh`) | Maintainer (the release script is off-limits to agents) | Recommended |
| A19 | P2 | `TEXT_DIM` contrast (fails WCAG AA on two surfaces) | Contributors | `FIRST_CONTRIBUTIONS.md` #2 |
| A20 | P3 | Linux `cargo check`; Intel Mac build; DMG | Contributors | #4 scoped |
| A21 | P3 | Reduce clippy warnings and make clippy gating | Contributors | — |
| A22 | P3 | Leave `ort` RC pin when 2.0 is stable | Maintainer | — |
