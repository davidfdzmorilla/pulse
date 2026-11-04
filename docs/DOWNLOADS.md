# Downloading Pulse

All builds are published on the
[GitHub releases page](https://github.com/davidfdzmorilla/pulse/releases).
Pulse is free software (GPL-3.0-or-later) and free of charge.

## What you can download

| Platform | Architecture | File | Status |
|---|---|---|---|
| macOS 13.4 (Ventura) or later | Apple Silicon (arm64) | `Pulse-<version>.zip` | **Supported.** Published with every release. |
| macOS | Intel (x86_64) | — | Not built yet. |
| Windows 10 1809+ / 11 | x64 | `Pulse-<version>-windows-x64-setup.exe`, `Pulse-<version>-windows-x64.zip` | **Experimental, not published in releases yet.** See [Windows](#windows-experimental). |
| Windows | ARM64 | — | Not supported. |
| Linux | — | — | Not supported. |

Each release also has `Pulse-<version>-simbolos.zip`. It holds the debugging
symbols used to read crash reports, and you don't need it to run Pulse.

## macOS

### Requirements

- A Mac with Apple Silicon (M1 or later) and macOS 13.4 or later.
- About 100 MB of disk space for the app (a ~50 MB download). The optional stem separation model
  (174 MB) and lyrics model (251 MB) are downloaded the first time you use
  those features, after asking.

### Install

1. Download `Pulse-<version>.zip` from the
   [latest release](https://github.com/davidfdzmorilla/pulse/releases/latest)
   and open it. You get `Pulse.app`.
2. **Move it to Applications before opening it** (`/Applications` or
   `~/Applications`). If you open it from Downloads, macOS runs it from a
   read-only temporary copy and Pulse can't update itself (it tells you).
3. Open it. The first time, macOS refuses ("cannot be verified").
4. Go to **System Settings → Privacy & Security**, scroll to the message about
   Pulse and click **Open Anyway**. On macOS 15 and later, right-click → Open
   no longer works for this. You only need to do it once. From a terminal,
   this does the same: `xattr -dr com.apple.quarantine /Applications/Pulse.app`.
5. The first time you record, macOS asks for microphone access. Allow it.

#### Why the Gatekeeper warning

Pulse is **signed with its own self-signed certificate ("Pulse Local
Signing"). It is not signed with an Apple Developer ID and not notarized by
Apple.** Gatekeeper therefore can't vouch for it. The signature still matters:
an installed Pulse only accepts updates signed with that same certificate.

### Update

Pulse checks for new versions when it starts and updates itself
(Pulse → Buscar actualizaciones…). It needs no GitHub account. Updates don't
trigger the Gatekeeper step again. The previous version is kept as
"Pulse (anterior)" next to the new one, so you can go back.

### Uninstall

1. Drag `Pulse.app` (and `Pulse (anterior).app`, if present) to the Trash.
2. Optionally, remove its data:
   - settings, kits and downloaded models: `~/Library/Application Support/Pulse`
   - caches: `~/Library/Caches/Pulse`
   - logs: `~/Library/Logs/Pulse`
3. Your projects, samples and exports are in `~/Music/Pulse` (or the folder you
   chose in Settings). Uninstalling never deletes them.

More detail, in Spanish: [INSTALL_MACOS.md](INSTALL_MACOS.md).

## Windows (experimental)

**Windows packages are not attached to releases yet.** What has been checked
so far (details in [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md)):

| Level | Status |
|---|---|
| Source available | Yes |
| Cross-compilation from macOS (`cargo check`, `cargo xwin build`) | Verified |
| Native build (MSVC, GitHub's `windows-2022` runner) | Verified in CI |
| Native execution: `pulse.exe --render`, window start driven by a script | Verified in CI (no audio device) |
| Installer: silent install, `.pulse` association, uninstall keeps settings | Verified in CI |
| Test suite on Windows | **Failing** on `main` since 2026-10-29: 3 tests fail on every run and 3 more on some runs ([details](FIRST_CONTRIBUTIONS.md#3-windows-audio-to-midi-with-any-number-of-threads)). Two of the constant ones were test bugs. They are fixed on the launch branch but haven't run on Windows yet |
| Real audio (WASAPI), MIDI hardware, third-party plugins, display scaling | **Not verified**: no Windows machine has tried them yet |

If you want to help test it:

- **Build it yourself:** [BUILD_WINDOWS.md](BUILD_WINDOWS.md) (Spanish).
- **Or take a CI build:** open a recent green run of the
  [Windows workflow](https://github.com/davidfdzmorilla/pulse/actions/workflows/windows.yml)
  and download the `pulse-windows-x64` artifact. You need to be signed in to
  GitHub. CI builds are test builds: they are not signed with Pulse's update
  key and may not report the right version number.

Install and uninstall steps: [INSTALL_WINDOWS.md](INSTALL_WINDOWS.md)
(Spanish). Windows SmartScreen warns because Pulse has no code-signing
certificate: **More info → Run anyway**.

Please report what you find with the
[Installation or platform issue](https://github.com/davidfdzmorilla/pulse/issues/new?template=04-installation-platform.yml)
form.

## Verifying a download (SHA-256)

GitHub computes a SHA-256 digest for every file attached to a release. It
shows next to each file on the release page, and you can also get it with
the GitHub CLI:

```bash
gh release view v0.1.121 --repo davidfdzmorilla/pulse --json assets \
  --jq '.assets[] | "\(.digest)  \(.name)"'
```

Compare it with the file you downloaded:

```bash
# macOS
shasum -a 256 ~/Downloads/Pulse-0.1.121.zip
```

```powershell
# Windows (PowerShell)
Get-FileHash .\Pulse-0.1.121-windows-x64.zip -Algorithm SHA256
```

The two values must be identical. CI artifacts include a `SHA256SUMS.txt`
file.

The checksum proves the file is the one published. On macOS, who made it is
proven by the code signature, which the updater checks. On Windows, it is
proven by the `.firma` Ed25519 signature next to each Windows zip.

## Known limitations

- The **interface and the guide are in Spanish**.
- macOS: Apple Silicon only, no Apple notarization.
- Windows: WASAPI shared mode only (no ASIO), unsigned, and not yet tested
  on real hardware.
- Plugins: 64-bit VST3 and CLAP only (no AU, no VST2).

## Reporting problems

- Installation, start-up, update: [Installation or platform issue](https://github.com/davidfdzmorilla/pulse/issues/new?template=04-installation-platform.yml)
- Anything else: [issue forms](https://github.com/davidfdzmorilla/pulse/issues/new/choose)

Include `pulse.log`. It is in `~/Library/Logs/Pulse/` on macOS and
`%LOCALAPPDATA%\Pulse\Logs` on Windows.
