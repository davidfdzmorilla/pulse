# Pulse: launch preparation report

**Date:** 2026-11-04 · **Branch:** `funcion/preparar-lanzamiento` (from `main`
at `5347594`). **No commits, pushes or GitHub changes were made.** All the
work is uncommitted in the working tree for the maintainer to review.

The findings this work responds to: [LAUNCH_READINESS_AUDIT.md](LAUNCH_READINESS_AUDIT.md).
Task IDs (A1–A22) refer to that document's last table.

## A. Starting point

- Mature macOS product: ≈1,280 tests, and in this audit 1,196 passed and 0
  failed. The full feature set is documented.
- Main README in Spanish (English was in `README.en.md`). No logo, quick links
  or video.
- Missing community files: no Code of Conduct, security policy,
  issue/PR templates or changelog. The GitHub community profile was at 57 %.
- `CONTRIBUTING.md`: 44 lines in Spanish, with no setup or architecture.
- Windows docs out of date: they said CI was blocked by billing and that
  nothing had run on Windows. In fact CI built, ran and installed Pulse
  natively until 2026-10-29, and has been red since: 3 tests fail on every run, 3 more on some runs.
- CI Windows packages labelled `0.1.3` (shallow clone, no tags).
- No macOS CI. Discussions, private vulnerability reporting and topics are
  all off.

## B. Files

**Created**

| File | Purpose | Audit task |
|---|---|---|
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1, official text, with a maintainer note for the contact | A1 |
| `SECURITY.md` | Supported versions, Pulse-specific scope (updater, model downloads, files, plugins), private reporting | A2 |
| `CHANGELOG.md` | Keep a Changelog. **Unreleased** by hand, plus all 122 releases generated verbatim from GitHub release notes | A6 |
| `CONTRIBUTING.es.md` | The original Spanish CONTRIBUTING, unchanged except a language link | A7 |
| `README.es.md` | Spanish README (the former `README.md`) with the new top block | A5 |
| `.github/ISSUE_TEMPLATE/01-bug-report.yml` … `04-installation-platform.yml`, `config.yml` | Issue forms: bug, feature, plugin compatibility, installation/platform | A6 |
| `.github/PULL_REQUEST_TEMPLATE.md` | Summary, tests, platforms, regressions, screenshots, **audio-thread warning**, checklist | A6 |
| `.github/workflows/macos.yml` | macOS CI: build, clippy (informative), tests. On `main`/manual runs: ad hoc `Pulse.app`, zip, `SHA256SUMS.txt`, render check, artifacts | A11 |
| `scripts/changelog.py` | Regenerates the released part of `CHANGELOG.md`. `--check` mode | A6 |
| `docs/LAUNCH_READINESS_AUDIT.md` | Phase 1 audit | — |
| `docs/DOWNLOADS.md` | Downloads, platforms, architectures, install/uninstall, Gatekeeper, SHA-256 verification, Windows status levels | A8 |
| `docs/DEVELOPMENT.md` | Architecture for contributors: crates, Mermaid diagrams, audio thread rules, plugins, UI, persistence, resources, tests, build | A7 |
| `docs/FIRST_CONTRIBUTIONS.md` | Five scoped issues, ready to publish | A7 |
| `docs/GITHUB_COMMUNITY_SETUP.md` | Maintainer checklist: CoC contact, private reporting, Discussions categories, `gh label create` commands, topics, funding, branch rules | A12 |
| `docs/PROJECT_MODEL.md` | Open core and hybrid model, contribution licensing, open legal questions | — |
| `docs/ROADMAP.md` | Known gaps from the status docs (draft, no commitments) | — |
| `docs/DEMO_VIDEO_PLAN.md` | 75-second script, scenes, on-screen text, recording tips | A13 |
| `docs/LAUNCH_PREPARATION_REPORT.md` | This report | — |

**Modified**

| File | Change | Audit task |
|---|---|---|
| `README.md` | Now the English README (former `README.en.md`, removed). New top block: logo, positioning line, description, 3 verifiable badges, quick links, main screenshot, status note, platform table, getting started. "22 effects" → 23. CI and contributing lines updated. The rest of the technical content is kept | A5 |
| `CONTRIBUTING.md` | English, expanded: philosophy, language policy, requirements, build/run/test commands, formatting/clippy, architecture, PR flow, bug reporting. The licence section is a translation that defers to the Spanish original | A7 |
| `.github/workflows/windows.yml` | `fetch-depth: 0` (real version in CI packages); `SHA256SUMS.txt` step, uploaded with the packages | A17 |
| `docs/PLATFORM_SUPPORT.md` | New "CI" level. Windows rows updated with the evidence: build, tests (red), start-up, render, package, updater signature | A9 |
| `docs/WINDOWS_SUPPORT.md` | Summary rewritten with the CI facts and run links; row 18 updated; a note that items 1–6 of the pending list were done by CI | A9 |
| `docs/INSTALL_WINDOWS.md`, `docs/BUILD_WINDOWS.md` | One sentence each: tested on CI, not on a real machine | A9 |

**Not touched, on purpose:**
- `scripts/release.sh`: the project's agent rules forbid it.
- `LICENSE` and the meaning of the contribution terms.
- `docs/guia.md`: no user-visible app change.
- Any Rust code.
- `cargo fmt`: the project uses a hand style.

## C. Presentation

- **10-second test.** The first screen now says what Pulse is ("An
  independent DAW for creative music production"), what it has (own engine,
  VST3/CLAP, local creative tools), where it runs, and that the UI is
  Spanish. Then it shows the Arrangement.
- **Badges.** Only verifiable ones: latest release (live from GitHub), licence
  (matches `LICENSE`), platforms (matches the platform table). There is no CI
  badge while Windows CI is red. Add one when both workflows are green.
- **Quick links.** Download, Getting started, Features, Documentation,
  Roadmap, Contributing, Changelog and Discussions. The Discussions link was
  an HTML comment until Discussions was enabled (2026-11-04), so there was
  never a broken link.
- **Video.** None exists, so none is linked. There is a plan instead.
- **Languages.** English is the default (`README.md`, `CONTRIBUTING.md`).
  Spanish is kept and cross-linked (`README.es.md`, `CONTRIBUTING.es.md`).
  The detailed install/Windows docs and the guide stay Spanish: their
  translation is first-contribution task #1.

## D. Distribution

| Package | How | Tested here |
|---|---|---|
| macOS release zip (signed "Pulse Local Signing") | `scripts/release.sh` (maintainer) | Not run (needs the certificate and publishes). The existing release v0.1.121 has a SHA-256 digest on GitHub |
| macOS CI zip (ad hoc) | `.github/workflows/macos.yml` | **Steps run locally**, simulating CI with a non-existent signing identity: `bundle.sh` built Pulse 0.1.121 with an ad hoc signature. `ditto` made the zip, `shasum` the checksum, and the bundled binary rendered the example song (42 MB WAV, −0.3 dBFS). **The workflow itself has not run on GitHub** (nothing pushed) |
| Windows zip + installer | `scripts/package-windows.ps1`, `.github/workflows/windows.yml` | Not run here (no Windows). The CI log of 2026-10-29 shows it working. The new checksum step and `fetch-depth` have not run yet |
| DMG | — | Not made. The updater consumes the zip, and a DMG adds nothing while the app isn't notarized. It would also be a second distribution path (P3) |

Not claimed: notarization, Developer ID, Authenticode, or Windows stability.

## E. GitHub Community

**Ready in the repository:** Code of Conduct, security policy, 4 issue forms
+ config, PR template, changelog, contributing guide, first issues.

**Needs the maintainer on github.com** (steps in
[GITHUB_COMMUNITY_SETUP.md](GITHUB_COMMUNITY_SETUP.md)):
1. ~~the Code of Conduct contact~~: done, `correo@davidfdzmorilla.dev`;
2. ~~private vulnerability reporting~~: enabled;
3. ~~Discussions and its 8 categories~~: enabled;
4. ~~8 new labels~~: created on 2026-11-04;
5. ~~topics, social preview and a shorter description~~: done;
6. optionally `FUNDING.yml` (no account defined) and a ruleset on `main`.

## F. Contribution

Documentation: `CONTRIBUTING.md` (EN), `CONTRIBUTING.es.md`,
`docs/DEVELOPMENT.md`, PR template.

The five proposed first issues ([FIRST_CONTRIBUTIONS.md](FIRST_CONTRIBUTIONS.md)):

1. **English versions of the install and platform docs.** Documentation,
   easy, good first issue.
2. **Readable dim text: fix `TEXT_DIM` contrast.** UX/accessibility, easy,
   good first issue. Measured 3.92:1 and 3.23:1 on two surfaces, against
   WCAG AA's 4.5:1.
3. **Windows: audio to MIDI with any number of threads.** Tests, medium.
   It replaces the original path-separator task, which this branch has
   already fixed (see the pre-merge review below).
4. **`cargo check` on Linux.** Platform, medium.
5. **`pulse --help` and errors for unknown flags.** CLI, easy, good first
   issue.

None has been published.

## G. Remaining risks

- **Spanish-only interface:** the biggest barrier for an international
  audience. It needs a localization layer, which is a real project rather
  than a first issue.
- **Gatekeeper friction** on every first install until Developer ID
  notarization (costs USD 99/year: maintainer decision).
- **Windows:** red CI, never run on real hardware, no published package.
- **Contribution licence clause:** needs legal review before merging
  outside code (questions listed in `PROJECT_MODEL.md`).
- **Single maintainer** with a very fast release cadence. Outside PRs need
  review time.
- **macOS CI is unproven** until its first run. Two things to watch there:
  the `macos-15` runner's speed for the full test suite, and whether
  `bundle.sh`'s `$HOME`-path check passes on the runner.

## H. Next steps (in order)

1. **P0** Review this branch and commit it. Suggested title, in Spanish as
   the release notes require: *"Pulse listo para recibir usuarios y
   colaboradores: README en inglés, guías y plantillas"*. Open the PR and
   watch the first `macOS` workflow run.
2. ~~**P0** Enable private vulnerability reporting~~: done, and the Code
   of Conduct contact is set (`correo@davidfdzmorilla.dev`).
3. **P0** Get legal advice on the contribution licence clause before the
   first external merge.
4. **P1** Publish the five first issues (Discussions and labels: done).
5. **P1** Open the PR so Windows CI checks the two path-test fixes. Then
   fix the remaining Windows failures (issue #3 for the ONNX one; the
   intermittent engine ones need the maintainer). Then add CI badges.
6. **P1** Record the demo video and add it to the README.
7. **P1** Plan the localization layer (English UI).
8. **P2** Developer ID + notarization; Windows hardware QA, then Windows
   packages on releases; a `SHA256SUMS` file in `release.sh`.
9. **P3** Linux check, Intel Mac build, clippy clean-up.

## I. Final evaluation (after this work)

| Area | Score | Explanation |
|---|---|---|
| Product readiness | 7.5 | Unchanged by this work: broad and well-tested on macOS. Held back by the Spanish-only UI and Apple Silicon only |
| Documentation | 8 | English entry points, architecture, downloads, honest status. The guide and detailed platform docs are still Spanish-only |
| First impression | 7.5 | Clear positioning, logo, quick links, real screenshots, honest status. No video yet |
| Download experience | 6.5 | One-zip macOS install with checksums and clear Gatekeeper steps. Still not notarized, and no Windows download |
| Community readiness | 6.5 | All files in place; three web settings pending, two of them P0 |
| Contributor experience | 8 | Setup, tests, architecture, audio-thread rules, five scoped tasks, PR template, CI on both OSes |
| Cross-platform readiness | 4.5 | macOS solid. Windows builds/installs on CI but tests are red and nothing has run on hardware. No Linux |
| **Overall launch readiness** | **6.5** | Ready for a soft launch on macOS once the P0 items are done and this branch is merged. A wide international push should wait for the video and, ideally, an English UI |

## Checks run for this report

| Check | Result |
|---|---|
| `cargo test --release --locked --workspace --no-fail-fast` | ✅ 1,196 passed, 0 failed, 84 ignored (macOS 15, Apple Silicon) |
| `cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets` | ✅ (only the expected llvm-rc warning) |
| `cargo clippy --release --locked --workspace --all-targets` | ✅ compiles; ≈280 warnings in non-test code (informative, as in CI) |
| `cargo fmt --all -- --check` | ❌ expected: 188 files differ from rustfmt's style (the project doesn't use rustfmt). Nothing was reformatted |
| YAML (`.github/**/*.yml`) | ✅ parsed with Ruby's YAML (Psych). `actionlint` isn't installed, so the workflows weren't linted beyond parsing |
| TOML (`Cargo.toml`, crate manifests, `rust-toolchain.toml`, `.cargo/config.toml`) | ✅ parsed with Python's `tomllib` (unchanged files) |
| JSON | No JSON files were added or changed |
| Relative links, anchors, images in Markdown | ✅ 0 problems (script in the session scratchpad; `docs/guia.md` excluded: it uses the app's `[[…]]` links) |
| External URLs in new/changed files | ✅ checked with `curl -L` (per-version changelog links sampled: v0.1.1, v0.1.60 and v0.1.121 give 200). The only 404s are expected: `blob/main/SECURITY.md` and `blob/main/docs/DOWNLOADS.md` (exist once this branch is merged), and the Discussions URLs, which appear only in an HTML comment and in the setup instructions, until Discussions is enabled |
| `python3 scripts/changelog.py --check` | ✅ |
| macOS CI packaging steps (local simulation) | ✅ |

## Pre-merge review (2026-11-04)

A last pass over this branch, before the PR.

| Item | Status | Change |
|---|---|---|
| Task 3 of `FIRST_CONTRIBUTIONS.md` used `cargo test --release -p app factory_sounds_tests lyrics` (Cargo: "unexpected argument 'lyrics'") | **Corrected** | Two commands (`… factory_sounds_tests`, `… lyrics`), both run: 4 and 10 tests pass. The task itself was replaced, because its two tests are now fixed (next row). The new task 3 is the ONNX thread-count test |
| Windows path tests `a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there`, `lyrics_are_saved_and_opened_with_the_project` | **Corrected** (macOS); **not verifiable** on Windows from here | CI logs show Pulse returning native paths (`…\Bombos\Bombo house.wav`), while the tests expected `…\Bombos/Bombo house.wav`. So the bug was in the tests, not a product incompatibility. The expected paths are now joined part by part. No product code changed |
| Absolute determinism claims (README EN/ES, CONTRIBUTING, DEVELOPMENT, audit) | **Corrected** | "Designed for deterministic rendering … with automated tests covering this behavior". DEVELOPMENT notes that plugins and ONNX models are outside it |
| `SECURITY.md` / `CODE_OF_CONDUCT.md` notices were HTML comments, invisible on GitHub | **Corrected** | Visible note that private reporting is not enabled yet. CoC contact set to `correo@davidfdzmorilla.dev` (maintainer's choice), also the e-mail fallback in `SECURITY.md` |
| README/CONTRIBUTING said macOS CI packages every PR | **Corrected** | It packages only on `main`/manual runs |
| Owner-only actions | **Pending, manual** | Table at the top of [GITHUB_COMMUNITY_SETUP.md](GITHUB_COMMUNITY_SETUP.md#owner-actions-at-a-glance) |

Windows failures on `main` (CI runs 41627764299 to 41794967564, 2026-11-03/04):
- **On every run:** the two path tests above, and
  `transcribe::tests::more_threads_give_the_same_notes` (ONNX activations
  not bit-identical between 2 and 3 threads on x64).
- **On some runs:** `engine::tests::with_threads_a_sidechain_waits_for_its_source`,
  `the_audio_threads_do_not_allocate_with_workers` (`stress.rs:669`, "the
  workers allocate") and `transcribe::tests::finds_the_notes_basic_pitch_finds`.

None was hidden or skipped. The intermittent no-allocation failure is a
real-time-safety signal that needs the maintainer's attention.

Checks: `cargo test --release --locked -p app -p stems` ✅ (725 + 1 + 48
passed, 0 failed); `cargo check --release --target x86_64-pc-windows-msvc -p
app --tests` ✅; YAML ✅; local links/anchors ✅ (0 problems);
`scripts/changelog.py --check` ✅; `git diff --check` ✅.

### First macOS CI run (PR #209)

[Run 41805367312](https://github.com/davidfdzmorilla/pulse/actions/runs/41805367312)
built everything and passed every test but one:
`transcribe::tests::finds_the_notes_basic_pitch_finds` (`crates/stems/src/transcribe.rs:291`).
On GitHub's `macos-15` runner (Apple M1, 3 cores) the note 67 of the
reference melody starts at 0.9868 s instead of 0.9752 s, one frame
(≈11.6 ms) early. The test allows 1 ms. It passes on the maintainer's Mac.
It also fails intermittently on Windows with the same note (0.9984 s).
This is a pre-existing sensitivity of the test (or of the port) to the CPU
running ONNX Runtime, and this branch doesn't cause it. It is left visible,
not skipped, pending the maintainer's decision.
