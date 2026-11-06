# Install Pulse on macOS

**English** · [Español](INSTALL_MACOS.md)

For Macs with Apple Silicon and macOS 13.4 (Ventura) or later.

## Install

1. Download `Pulse-<version>.zip` from the latest release at
   [github.com/davidfdzmorilla/pulse/releases](https://github.com/davidfdzmorilla/pulse/releases)
   and open it: `Pulse.app` appears.
2. **Move it to Applications before opening it** (your own `~/Applications`, or
   the system folder). When opened from Downloads, macOS runs it from a
   read-only temporary copy and Pulse cannot update itself (it tells you).
3. Open it. Pulse is signed with its own certificate, not an Apple certificate,
   so macOS refuses to open it the first time ("cannot be verified"). Go to
   **Ajustes del Sistema (System Settings) → Privacidad y seguridad (Privacy & Security)**, scroll to the Pulse warning and
   click **Abrir igualmente (Open Anyway)** (on macOS 15, right-click → Open no longer works).
   You only need to do this once.

   From the terminal, the equivalent is:
   `xattr -dr com.apple.quarantine /Applications/Pulse.app`.

4. The first time you record, macOS asks for microphone access. Allow it.

## Update

Pulse checks for new versions at startup and updates itself from its window
(Pulse → Buscar actualizaciones… (Check for updates…)), without a GitHub account. Updates do not ask
for **Abrir igualmente (Open Anyway)** again: they are signed with the same certificate, and Pulse
will not install an update without that signature. The previous version remains
as `Pulse (anterior)` next to the new one, so you can go back to it.

## Where things are stored

- Your projects, samples and exports: `~/Music/Pulse` (change it in Settings).
- Settings, kits and models: `~/Library/Application Support/Pulse`.
- If Pulse fails: `~/Library/Logs/Pulse/pulse.log`; if it exits unexpectedly
  (for example, because of a plugin), macOS saves a report in **Console → Crash
  Reports** (`~/Library/Logs/DiagnosticReports/pulse-*.ips`). That file says
  which line of code failed (use `scripts/simbolos.py` with
  [PUBLICAR.md](PUBLICAR.md)); send it when reporting a failure.

## Uninstall

Drag `Pulse.app` (and `Pulse (anterior)`, if present) to the Trash. Your
projects remain in `~/Music/Pulse`; settings remain in the folders above.
