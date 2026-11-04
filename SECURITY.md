# Security policy

## Supported versions

Pulse updates itself from its GitHub releases. Only the **latest release**
gets security fixes. If you run an older build, update first
(Pulse → Buscar actualizaciones…) and check whether the problem is still
there.

| Version | Supported |
|---|---|
| Latest release ([releases](https://github.com/davidfdzmorilla/pulse/releases/latest)) | Yes |
| Anything older, or a local build from an unreleased commit | No |

## What counts as a security issue

These are especially relevant in Pulse:

- **The updater** (`crates/app/src/updates.rs`, `crates/firma`). On macOS it
  only installs a download signed with the same certificate as the running
  app. On Windows it only installs a zip with a valid Ed25519 signature from
  Pulse's key. A way around either check is critical.
- **Model downloads.** The stem separation model comes from a GitHub release
  and the Whisper model from a pinned Hugging Face revision. Both are checked
  against a SHA-256 hash before use (`crates/app/src/stems_ui.rs`,
  `crates/app/src/lyrics_ui.rs`). A way to make Pulse load a different file is
  a security issue.
- **Opening untrusted files.** A crafted `.pulse` project, Ableton `.als` set,
  MIDI file or audio file that runs code, writes outside the expected folders
  or corrupts memory.
- **Plugin hosting.** VST3 and CLAP plugins are native code and run with
  Pulse's permissions. That is how plugins work in every DAW, so it is not a
  vulnerability by itself. A way for Pulse to load a plugin from a place you
  didn't choose would be.

For crashes or misbehaviour with no security impact, open a normal
[bug report](https://github.com/davidfdzmorilla/pulse/issues/new/choose).

## How to report a vulnerability

**Please don't open a public issue for a vulnerability.**

Use GitHub's private reporting: the repository's **Security** tab →
**Report a vulnerability**
(<https://github.com/davidfdzmorilla/pulse/security/advisories/new>). Only the
maintainer sees the report.

If you prefer e-mail, write to [correo@davidfdzmorilla.dev](mailto:correo@davidfdzmorilla.dev) with "Security" in the
subject.

Please include:

- the Pulse version (Acerca de Pulse, or `pulse --version`) and your OS;
- what an attacker can do, and what they need first (for example, "the victim
  opens this project");
- steps or a minimal file that reproduces it.

## What to expect

Pulse is maintained by one person, so these are goals, not guarantees:

- an acknowledgement within 7 days;
- a first assessment within 14 days;
- a fix in a new release, with credit in its release notes if you want it,
  before the details are made public.
