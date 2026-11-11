# Demo video plan

No demo video of Pulse exists yet. The repository, the README and the docs
have no video files or video links (checked 2026-11-04). This is the plan
for recording one. It must show **a real session running in Pulse**: no
mock-ups, no sped-up renders presented as real time, no features that aren't
in the released version.

## Format

- **Length:** 60–90 seconds (target 75 s).
- **Resolution:** 1920×1080 or 2560×1440, 60 fps. The window maximised on a
  Retina display, recorded at 1:1.
- **Audio:** Pulse's own output, recorded digitally (e.g. with a loopback
  device). No music added in editing: what you hear is what Pulse plays.
- **Language:** on-screen text in English. Pulse's UI is in Spanish, so say so
  once rather than hide it.
- **Where to publish:** YouTube or Vimeo, embedded in the README as a
  thumbnail linking to the video. A muted MP4/GIF under 10 MB in
  `docs/capturas/` can autoplay on GitHub.

## Script

| Time | Scene | What's on screen | On-screen text |
|---|---|---|---|
| 0–5 s | Cold open | The example song playing in the Arrangement, playhead moving, meters live | **Pulse**: an independent DAW, written in Rust |
| 5–15 s | Session | Switch to the Session view; launch scenes in time, a clip queued and starting on the bar | Session and Arrangement |
| 15–27 s | Write | Piano roll: select a chord clip → **Acompañar** writes a bass line and a melody that follow it; play it | Bass lines and melodies that follow your chords |
| 27–39 s | Audio → MIDI | Drag a sung or hummed audio clip → Convertir a MIDI → notes appear; play them on Pulse Synth | Audio to MIDI, on your computer |
| 39–50 s | Sound | Devices panel: a Pulse instrument, then a VST3 or CLAP plugin with its own editor open (use a freely licensed plugin, e.g. Surge XT) | Built-in instruments, VST3 and CLAP |
| 50–62 s | Stems / lyrics | Split a song into stems, *or* show transcribed lyrics under a vocal in the audio editor | Stems and lyrics, offline |
| 62–72 s | Mix | Mix assistant: measure → apply → loudness reaches the chosen target | A mixing assistant that explains what it does |
| 72–80 s | End card | Logo, URL, three facts | **Free & open source (GPL-3.0)** · macOS (Apple Silicon) · Windows experimental · github.com/davidfdzmorilla/pulse |

## Features to show (all in the current release)

Each is listed in the README and covered by tests (see
[LAUNCH_READINESS_AUDIT.md](LAUNCH_READINESS_AUDIT.md#11-product)):

- Session clip launching, Arrangement
- Accompany (bass, melody over chords)
- Audio to MIDI
- built-in instruments and one third-party plugin
- stems (needs the 174 MB model downloaded beforehand) or lyrics (251 MB model)
- mix and master assistant

Don't show Windows unless it has been tested on real hardware by then.

## Screenshots needed (for the thumbnail and the README)

Already in `docs/screenshots/` in English and `docs/capturas/` in Spanish (made by `cargo test --release -p app readme_shots -- --ignored`):
`arrangement.png`, `session.png`, `piano-roll.png`, `devices.png` / `dispositivos.png`,
`audio-editor-with-lyrics.png` / `editor-de-audio-con-letra.png`, `mix-assistant.png` / `asistente-de-mezcla.png`.

To capture during recording:
- a thumbnail frame: the Arrangement mid-playback, with the logo;
- a plugin editor open inside Pulse (no screenshot of this exists yet).

## Closing message

> Pulse is free and open source. Try it, break it, tell us what you'd make
> with it. github.com/davidfdzmorilla/pulse

## Recording tips

- Use a fresh user profile or reset settings so no personal paths, plugin
  lists or recent projects appear.
- Pre-download the stem/lyrics models so no download dialog interrupts.
- Hide the dock and menu-bar clutter. Do Not Disturb on.
- Record each scene separately and cut. Keep each action visible for at least
  1.5 s.
- Show the cursor and enlarge it a little.
- Rehearse at 128 BPM in a key where the example song sounds good. Avoid any
  copyrighted song for the stems/audio-to-MIDI scenes: use your own recording
  or Pulse's example song rendered with `pulse --render`.
- Keep the plugin shown freely licensed, or get permission, and don't show
  its logo prominently.
