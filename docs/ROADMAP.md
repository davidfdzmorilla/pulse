# Roadmap

> **Draft for the maintainer to confirm.** This lists known gaps and open
> directions taken from the repository's own status documents. It's a picture
> of where Pulse stands, not a list of promises or dates. What gets built is
> decided in issues and pull requests.

What Pulse does today: [README](../README.md#what-you-can-do). What changed in
each version: [CHANGELOG](../CHANGELOG.md).

## Platforms

| Item | State | Source |
|---|---|---|
| Windows test suite green on CI | ✅ Green on `main` since 2026-11-05 (red from 2026-10-29). The engine's stress tests still take about 27 minutes there | [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md#pruebas-de-windows-en-la-ci), [FIRST_CONTRIBUTIONS.md #3](FIRST_CONTRIBUTIONS.md#3-windows-ci-make-the-engine-stress-tests-fast-again) |
| Scripted window start on Windows CI | ✅ Passes since 2026-11-05 (it overflowed the 1 MB main-thread stack; Pulse now links with 8 MB) | [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md#pruebas-de-windows-en-la-ci) |
| Windows tested on real hardware (audio, MIDI, plugins, display scaling) | Not done | [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md#pendiente-de-verificar-en-windows) |
| Windows packages attached to releases | Not yet | [BUILD_WINDOWS.md](BUILD_WINDOWS.md) |
| Windows code signing (Authenticode) | No certificate | [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md#limitaciones-conocidas) |
| ASIO on Windows | Not enabled (needs Steinberg's SDK and license) | [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md#limitaciones-conocidas) |
| macOS notarization (Apple Developer ID) | Not done: self-signed certificate | [INSTALL_MACOS.md](INSTALL_MACOS.md) |
| Intel Macs | Not built | `rust-toolchain.toml` targets `aarch64-apple-darwin` only |
| Linux | Not supported. First step: make it compile | [FIRST_CONTRIBUTIONS.md #4](FIRST_CONTRIBUTIONS.md#4-cargo-check-on-linux) |

## Reach

| Item | State |
|---|---|
| English (and other languages) in the interface | Done (English): **Ajustes › Idioma · Language** (Spanish or English, the system's by default), the whole interface translated. Texts live in `crates/app/src/en.rs`; other languages would add a table like it |
| English user guide | Done: `docs/guide.md`, shown in the app when the interface is in English |
| Demo video | Not recorded yet: [DEMO_VIDEO_PLAN.md](DEMO_VIDEO_PLAN.md) |

## Music features

New music features are chosen as the project goes, favouring tools that work
from analysis of your own audio (the direction behind audio to MIDI, the
chord track and the mixing assistant). No list is committed yet.

<!-- MAINTAINER: add the next planned features here when decided. -->

Ideas are welcome as [feature requests](https://github.com/davidfdzmorilla/pulse/issues/new?template=02-feature-request.yml).
