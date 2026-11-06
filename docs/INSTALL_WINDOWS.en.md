# Install Pulse on Windows

**English** · [Español](INSTALL_WINDOWS.md)

For 64-bit Windows 10 (1809 or later) and Windows 11. Support status:
[WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md). The installer is tested in CI
(silent install, `.pulse` association and uninstall), **but not yet on a real
Windows machine**.

## Install

1. Download `Pulse-<version>-windows-x64-setup.exe` from the release.
2. Open it. Pulse is not signed with a certificate, so SmartScreen warns
   ("Windows protected your PC"): **More info → Run anyway**.
3. It installs for your user, without administrator permissions, at
   `%LOCALAPPDATA%\Programs\Pulse`, where Pulse can update itself. There is no
   all-users installation (in Program Files): Pulse could not update itself
   there. If an old installation is there, Pulse tells you when updating:
   uninstall it and install the new one.
4. It creates a Start menu shortcut (and a desktop shortcut if you select it),
   and associates `.pulse` files with Pulse.

### Portable copy

`Pulse-<version>-windows-x64.zip` contains a `Pulse` folder with `pulse.exe`
and what it needs. Extract it wherever you like and open `pulse.exe`. This does
not associate `.pulse` files or create a Start menu shortcut.

## First start

- **Audio**: Pulse uses Windows' default output (WASAPI). Ajustes (Settings)
  (**Ctrl+,**) → Audio lets you change the output, input, sample rate and
  buffer. On Windows, the sample rate is the one configured for the device.
- **Microphone**: if no audio is recorded, check Configuración (Settings) → Privacidad y seguridad (Privacy & security)
  → Micrófono (Microphone) → “Permitir que las aplicaciones de escritorio accedan
  al micrófono”.
- **Plugins**: Pulse searches for VST3 and CLAP in
  `C:\Program Files\Common Files\VST3` and `CLAP`, and in
  `%LOCALAPPDATA%\Programs\Common\VST3` and `CLAP`. You can add more folders in
  Ajustes (Settings) → Plugins. Only 64-bit plugins are supported.
- **Shortcuts**: where the guide says ⌘ use Ctrl; ⇧ is Shift; ⌥ is Alt. Full
  screen: F11.

## Where each thing is stored

| What | Where |
|---|---|
| Your projects, exports, presets and samples | `Música\Pulse` (change it in Settings) |
| Settings, MIDI, found plugins and recent files | `%APPDATA%\Pulse` |
| Generated kits and downloaded stems model | `%LOCALAPPDATA%\Pulse` |
| Caches (tempo models) | `%LOCALAPPDATA%\Pulse\Cache` |
| Crash log (`pulse.log`) | `%LOCALAPPDATA%\Pulse\Logs` |
| The program | `%LOCALAPPDATA%\Programs\Pulse` |

Projects store their audio in their own folder, with relative paths, and can be
moved between a Mac and a PC. If a project is missing an audio file when opened,
the status bar says so and keeps the path.

## Update

- Pulse checks for new versions at startup in the repository's public GitHub
  releases; no account or token is needed.
- **Download and install** downloads the Windows zip (over HTTPS only), checks
  its SHA-256 digest, Pulse's own signature (separate from GitHub) and its
  version. It then updates the Visual C++ runtime if it changed and replaces
  `pulse.exe`, keeping the previous one as `pulse (anterior).exe`. If something
  fails part-way through, the old `pulse.exe` remains and works with the new
  runtime. **Go back to the previous version** to swap them.
- You can also run the new version's installer over the old one. Projects and
  settings are not touched.

## Uninstall

Configuración (Settings) → Aplicaciones (Apps) → Pulse → Desinstalar (Uninstall) (or “Uninstall Pulse” in the Start menu).
This removes the program, its shortcuts and the `.pulse` association. **It does
not remove** your projects (`Música\Pulse`) or settings. To remove those too,
delete `%APPDATA%\Pulse`, `%LOCALAPPDATA%\Pulse` and `Música\Pulse` if you no
longer want your projects.

## Troubleshooting

- **It closes by itself**: check `%LOCALAPPDATA%\Pulse\Logs\pulse.log`, which
  records every crash trace.
- **There is no sound**: check the output in Ajustes (Settings) → Audio. If another
  program uses the device in exclusive mode, Pulse cannot open it. Also try a
  larger buffer if there are dropouts.
- **A plugin does not appear**: make sure it is 64-bit and is in one of the
  folders above or one added in Ajustes (Settings). Then use **Scan plugins**.
- **A plugin editor does not open**: the plugin may not have a Windows editor.
  **Parameters…** always works.
- **`VCRUNTIME140.dll` or `MSVCP140.dll` is missing**: reinstall with the
  installer, or install the x64 “Microsoft Visual C++ Redistributable”.
- **The update fails**: the message explains why. You can always install the
  new version with its installer.

## Manual checks

What CI cannot check and must be tested on a Windows PC before release:

1. Install on a clean Windows system (without Visual Studio) and start it.
2. Audio: play the example project; change output, sample rate and buffer;
   disconnect a USB device while Pulse is playing; close Pulse.
3. Record audio (and check the microphone privacy prompt).
4. MIDI: play an instrument with a USB keyboard; use MIDI learn.
5. Plugins: Surge XT (VST3 and CLAP) or similar: load, open the editor, resize
   it, close it, save and reopen the project with its state; reopen a project
   with a missing plugin.
6. Displays: 100%, 150% and 200% scaling; move a plugin editor to another
   monitor with a different scale.
7. Double-click a `.pulse` file (including paths with spaces and accents).
8. Separate stems (download the model) and **Convert to MIDI**.
9. Export audio.
10. Update to a published version with the Windows zip, then go back to the
    previous version.
