<!--
Pulse guide, in English. It shows inside the app in Help → Pulse Guide; each new
feature adds or fixes its topic here. The Spanish original is docs/guia.md: both
must keep the same topics and headings, in the same order, because the app maps
them by position. Format: see the comment in crates/app/src/guide.rs
(# area, ## topic with intro, ### section, 1. steps,
> Tip:/Note:/Warning:, +++ fold +++, {keys}, [[links]]).
-->

# Get started

## Getting started

Pulse is a studio for making music: you compose, record, edit and mix in a single window. This topic shows you where everything is and how to get something playing in a minute.

### The Pulse window

Pulse has two views, and {Tab} switches between them:

- **Session**: looping clips you launch whenever you like, for jamming and trying out ideas. See [[Session]].
- **Arrangement**: the song's timeline, from start to finish. See [[Arrangement]].

The same pieces always surround the view:

- At the top, the **transport**: tempo, metronome, play, stop, record, loop and more. See [[Transport]].
- On the left, the **browser**: instruments, effects, plugins, samples and your library. **☰** shows or hides it. See [[Browser and library]].
- At the bottom, the **detail panel**: the editor for the selected clip, or the track's devices. {⇧Tab} switches between the two. With no clip selected, **Clip** shrinks to a strip with its tabs, and the Session or the Arrangement takes up the rest. Drag its top edge to give it more or less room: Pulse remembers the height you pick, in other windows too. Whatever doesn't fit scrolls inside with the wheel. **Maximize** ({⇧⌘E}, or double-click the edge) makes it fill almost the whole window, and the same again returns it to its height.
- At the very bottom, the **status bar**: it tells you what just happened and why something couldn't be done. When there's no message, it reminds you of the main shortcuts.

### Get something playing in a minute

1. Create a MIDI track with {⌘T}.
2. Choose an instrument in the browser: click **INSTRUMENTS**.
3. Double-click an empty slot on the track. A four-bar MIDI clip is created and opens in the editor.
4. Double-click in the editor to add notes, or press {M} and play with the computer keyboard.
5. Click the clip's **►** to launch it, and {Espacio} to stop.

> Tip: the first time, Pulse opens the example song and asks what you'd like to do (see [[#What would you like to do?]]). After that it opens your last project; you can change this in [[Settings]].

### What would you like to do?

When Pulse starts without a project, a window appears with ways to begin. It's also in **File › What Would You Like to Do?…**.

- **Listen to the example**: opens "Luces de la ciudad", the example song (see [[#The example song]]).
- **Hum an idea**: a project with an armed audio track and the metronome on, plus the lesson that takes you from your melody to a song (see [[From humming to song]]).
- **Remix a song**: you pick an audio file, it goes onto an Arrangement track and Pulse separates it into drums, bass, vocals and the rest (see [[Slicing and separating stems]]).
- **START FROM A TEMPLATE**: an empty project or one of the templates, with its instruments, effects and mix ready to go (see [[Projects and the Pulse folder#Templates]]).
- **LEARN**: start one of the lessons (see [[#Lessons]]).
- **OPEN**: your recent projects, or any other with **Open a project…**.

If you switch projects with unsaved changes, Pulse asks first. To stop it appearing at startup, untick **Show at startup** (or do it in **Settings › Projects**). {Esc} or **Close** closes it and leaves you in the example song.

### The example song

"Luces de la ciudad" ("City Lights") is a short melodic techno track in D minor, at 122 BPM, made only with Pulse's own sounds. It's there to hear what Pulse can do and to take apart:

- **Session**: three scenes, **Intro**, **Drop** and **Break**, to launch and mix as you like.
- **Arrangement**: the finished song, two and a half minutes long: intro, build-up, drop, break with the vocal, build-up, drop and outro, with risers and impacts on the **Transitions** track.
- **Chords and accompaniment**: the chords (Dm B♭ F C) are on **Chords**; the rolling bass, the pad and the melody were written from them by [[Accompaniment|Accompany]].
- **Automation**: the pad's filter opens on every build-up, and the melody has brightness and pitch bend in the clip.
- **Vocal with lyrics**: the **Vocals** track sings in the break, in Spanish, with its lyrics (see [[Lyrics]]).
- **Mix**: a drum group with bus compressor, space and delay returns, and the master with compressor and limiter. Try it with the [[Mix and Master Assistant]].

Whatever you change doesn't touch the original: when you save, {⌘S} asks you where. To get it back exactly as it was, use **File › What Would You Like to Do?… › Listen to the example**.

### Lessons

**Help › Lessons** opens a panel on the right with short lessons, a few minutes each:

1. **Your first beat**: a drum track, a clip and a beat.
2. **Chords and bass**: the key, some chords and a bass line built from them.
3. **Record your voice and tune it**: a take, with its notes put in place.
4. **From humming to song**: from a hummed melody to four tracks.
5. **Mix with the assistant**: analyze, apply and compare.
6. **Export your song**: from the Arrangement to a file.

Each step says what to do and how. Steps tick themselves off as you do them, on your own project: for example, "Create a clip" is ticked when the drum track has one more clip than when the lesson started. Things Pulse can't see (that you've listened to the result) you tick yourself by clicking their circle, and another click unticks them. When you finish a lesson, it's marked with ✓ and you can move on to the next one. **More in the guide** opens its topic.

> Tip: a lesson starts with your project as it is. You can follow it in the example song or in a new project.

> Tip: in menus, to reach an option in a submenu (**⏵**) you can move diagonally: while the pointer heads towards the open submenu, passing over other options doesn't change it. If you pause for a moment on another option with **⏵**, its own submenu opens.

> Note: on Windows, the menus are in the bar at the top of the Pulse window, where the guide says Finder, read File Explorer, and full screen is {F11}. The keys in the guide are already shown as on your system (⌘ is Ctrl, ⇧ is Shift, ⌥ is Alt and ⌫ is Delete).

## Transport

The transport is the bar at the top: from there you play, stop, record and control the tempo of the whole song. If the window is narrow, it wraps onto two rows.

### Play, stop and record

1. Click **► Play** or press {Espacio} to start playback.
2. Click **■ Stop** or press {Espacio} again. Stop halts everything, recording included, and returns to the start marker.
3. To record into the Arrangement, click **● Record** or press {F9}. See [[Recording]].

### The controls, from left to right

| Control | What it's for |
|---|---|
| **Tempo** | The song's speed, from 20 to 999 BPM. Drag or type. |
| **Time signature** | 4/4, 3/4, 6/8, 7/8… The song's, or the one at the start if it changes later on. See [[#Change the time signature]]. |
| **●** next to the tempo | The **metronome**. |
| **Launch quantization** | When the clips you launch start: None, 1/16 … 1 Bar, 2 Bars. By default, 1 Bar. |
| **Position** | Where the song is (bar. beat. sixteenth), counted in its time signature. During the count-in it shows "Count N". |
| **► Play** / **■ Stop** | {Espacio}. |
| **● Record** | {F9}: records into the Arrangement. |
| **Capture MIDI** | {⇧⌘C}: a clip with what you just played, even if you weren't recording. It lights up when there's something to capture. See [[Recording#Capturing what you just played]]. |
| **Count-in** | No count-in, or 1, 2 or 4 bars of metronome before recording. |
| **A** (Automation Arm) | When it's on, recording also records the controls you move. See [[Automation]]. |
| **⟲ Loop** | Repeats part of the Arrangement. Next to it, its start and length; you pick the region in the strip at the top of the Arrangement. |
| **Punch in** / **punch out** | The two red buttons next to the loop: when recording into the Arrangement, only record from the loop's start and/or up to its end. See [[Recording]]. |
| **Rec Q** | Quantizes the start of the MIDI notes you record: Off, 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32. |
| **Swing** | From 0 to 100 %, with its grid (1/8 or 1/16): delays the offbeats so the groove "swings". |
| **Scale** | The song's key, and whether new loops follow it. See [[Key and scale]]. |
| **► Arrangement** | Lights up when needed: returns the tracks playing Session clips to the Arrangement, and turns back on any automation you've overridden by hand. |
| **Keyboard** | The computer keyboard as a MIDI keyboard ({M}). See [[Computer keyboard]]. |
| **Voice** | Play by singing, humming or whistling into the microphone, or play drums by beatboxing. See [[Playing with your voice]]. |
| **MIDI** | On if MIDI keyboards are connected. Click to look for them again. |
| **Map** | Map the knobs and buttons of a MIDI controller. See [[MIDI controllers]]. |
| **☰**, **Session** / **Arrangement** | On the right: the browser, the view and the [[#The master meter]]. |

### The master meter

At the far right, the master meter tells you how loud the song is while it plays:

- The **bar** is its level. The little line that stays put is the **highest peak**, with its value in dB beside it.
- If the mix goes over **0 dBFS**, the bar ends in red and the peak stays **red** even once it's no longer that loud: the mix is clipping. Lower the faders or put a limiter on the master (the [[Mix and Master Assistant]] does it for you).
- **Click** the meter or the peak to clear it and start watching again.
- Next to it you see the short-term **loudness**, in LUFS: that of the last 3 seconds, measured the way the assistant measures it. Compare it with your song's destination (−14 LUFS for Spotify, −9 for a club); the drop usually sits 1 or 2 LU above the song's average.

+++ Details
- The peak is that of what comes out of the master, before the output's safety clipper: if it reads +2.7, the mix really does go 2.7 dB over 0.
- The loudness doesn't count the metronome or what you preview in the browser.
+++

### Audio load and dropouts

Next to the master meter, **CPU** tells you how hard the audio is working: the time Pulse takes to make each chunk of sound, compared with how long that chunk lasts. The bar is the average and the little line is the peak.

- Above 70 % it turns orange, and above 90 % red: you're close to the sound breaking up.
- If a chunk arrives late, it counts as a **dropout** (in red, beside it).
- **Click** opens **Performance**: what each track, its instrument, each effect and each plugin takes up, heaviest first. **Reset** starts counting dropouts again.

If you get dropouts, raise the buffer in **Settings › Audio**, freeze the heavy tracks (see [[Tracks#Freeze a track]]) or remove plugins.

+++ Details
- A dropout is a chunk that Pulse took longer to make than it lasts, or a call from the sound card that came in late.
- The per-track breakdown is only measured while **Performance** is open: closed, it costs nothing.
- Plugins play one after another in the audio core: spreading the work across cores doesn't help them.
+++

### Change the time signature

1. Open the **Time signature** menu, next to the tempo, and pick one: 4/4, 3/4, 2/4, 6/8, 12/8, 5/4, 7/8, 9/8, 5/8, 7/4 or 2/2. Under **Other**, any other: the beats and the note value.
2. To change it partway through the song, right-click the Arrangement's marker strip, over the bar where it changes, and choose **Time signature from bar N**. The change shows on the ruler, next to the bar number.
3. To remove a change, right-click that bar and choose **Remove the time signature change**.

Everything follows the time signature:

- the ruler and the grid;
- snapping when you move clips and the loop;
- **Slice** by bars;
- launch quantization ("1 Bar" waits for the bar that's playing);
- the metronome and the count-in;
- the rounding of takes;
- new clips, which are four bars long (or however many you choose in [[Settings]]);
- the position in the transport bar;
- plugins, which receive the time signature.

In the MIDI editor, the ruler and the loop count the bars where the clip sits; Session clips count those of the starting time signature.

The generators also write in the time signature:

- **Generate beat** makes its bars in the clip's time signature. The first pulse gets the kick and the second the snare: beat 2 in 3/4, the second pulse in 6/8. The hi-hats play eighth notes and the fill goes on the last pulse.
- The **accompaniment** plays the bass by pulses (dotted quarters in 6/8, groups of 2+2+3 in 7/8) and accents the first of each bar.
- **From humming to song** puts one chord per bar.
- **Auto-Arrange** counts its sections in the project's bars, and the fill lasts the last bar.
- **Transitions** are measured in their own bars.

> Note: the drum AI can only play in 4/4. In other time signatures, **Generar ritmo** uses the written beat and tells you so.

+++ How it counts
- Beats are quarter notes: a bar of 3/4 lasts 3, one of 6/8 also 3 (six eighth notes) and one of 7/8, 3.5.
- The metronome clicks each **pulse**: the quarter note in 4/4 and 3/4, the dotted quarter in compound meters (6/8, 9/8, 12/8) and the eighth note in irregular ones (5/8, 7/8). The first of each bar sounds higher.
- A change always goes at the start of a bar. If you change an earlier bar, the later changes stay at the same bar number.
- A song can have up to 16 changes.
+++

### Tempo that changes through the song

If the tempo is automated, the tempo field shows the one playing at each moment, with a red border. If you move it by hand, the automation stops being in charge until you click **► Arrangement**. See [[Automation]].

### Swing

Swing delays the offbeats on playback, without changing the notes you've written. At 100 % it's a triplet shuffle. It also applies when exporting, and it works for every MIDI clip except those with their own groove. See [[MIDI editor]].

> Note: when there's a new version of Pulse, "↑ Pulse X disponible" ("Pulse X available") appears on the right of the status bar. See [[Updates]].

## Session

The Session view is a grid of clips that play in a loop and that you launch whenever you like: it's for jamming, trying combinations and playing live.

Each track is a column of slots, and each row is a **scene**. You see the scenes you use plus one empty one (8 at least, up to 16); when you put a clip in the last one, another appears. On the right are the **+** button (new MIDI track), the returns (**A Reverb** and **B Delay** in a new project) with their own **+** (new return), and the **Master** column with the scenes.

With many tracks, the returns and the Master stay pinned on the right, and the mixer at the bottom, with the **●** arm buttons in view. If the tracks don't fit, the returns get narrower (their full name shows on hover) to leave them more room. The wheel scrolls the clips up and down, and {⇧ + rueda} or the trackpad, sideways; the mixer scrolls with them. When you drag a clip near the edge, the view scrolls by itself.

With the editor open and little room, the Session keeps at least six scenes in view: the mixer gives way first and scrolls with the wheel.

### Launch and stop clips

1. Click a clip's **►**. It starts at the next quantization point (and starts the transport if it was stopped). While it waits, the ► blinks, and a bar at the bottom shows how far it has got.
2. To stop a track, click the square in an empty slot or **Stop track**, under its column.
3. To stop everything, click **Stop all clips**, under the Master.

### Create, select and move clips

- **Double-click an empty slot**: on a MIDI track it creates an empty four-bar clip named after the track; on an audio track it opens the file picker. How long new clips are is set in [[Settings#Projects]], and each clip's own length in **Loop** in the [[MIDI editor]].
- **Click a clip**: selects it and opens it in the editor. {⌘ clic} or {⇧ clic} adds clips to or removes them from the selection.
- **Drag a clip**: moves the selected clips to other slots; with {⌥} it copies them. The target slots turn blue if they fit and red if they don't. Each clip needs a free slot on a track of its type (audio or MIDI).
- **Drag from an empty slot**: draws a box that selects the clips and tracks it touches.

### Clip menu

Right-click a clip:

- **Launch**.
- **Copy to Arrangement**: puts it at the end of its track's Arrangement, on a bar.
- **Accompany**: a bass, an arpeggio, a pad, rhythmic chords or a melody for its chords. See [[Accompaniment]].
- **Duplicate** ({⌘D}): copies it to the next free slot below.
- **Delete** ({Supr} or {⌫}).
- **Deactivate clip**: it stays in place but doesn't play. See [[Arrangement]].
- **Follow action**: moves on to another clip by itself after a few loops. See [[#Follow actions]].
- Colors.
- Audio clips only: **Separate into stems…**, **Lyrics ▸**, **Consolidate**, **Reverse**, **Detect tempo** and **Use this clip's tempo in the project**. See [[Slicing and separating stems]], [[Lyrics]] and [[Audio tempo]].
- MIDI clips only: **Export MIDI…**. See [[MIDI files]].

### Launch scenes

- Clicking a scene in the Master column launches it: each track plays its clip in that row, and the ones with no clip there stop.
- Right-click the scene: **Scene name** and its **follow action** (for example, the next scene after 8 bars).

### Follow actions

A clip can move on to another by itself once it has played for a while: right-click the clip → **Follow action**, or the **»** button in the clip panel's bar.

- **After**: how many loops of the clip, or how many bars, it plays before deciding.
- **A** and **B**: what it does then: **Next**, **Previous**, **First**, **Last**, **Any**, **Other** (at random), **Play again** (starts again), **Stop** or **No action**.
- **Chance**: how many times out of a hundred A comes up; the rest, B. At 100 % it's always A.
- Clips and scenes with an action show **»** on the right.
- Actions work within the clip's **group**: the consecutive clips on its track, with no gaps. Next, from the last one, goes back to the first.
- Scenes have them too (right-click the scene): Next launches the scene below on every track.

+++ Details
- The change lands in time with launch quantization: if it decides mid-bar with 1-bar quantization, it waits for the next bar.
- If it comes up **Nada** (or **Otro al azar** with no other clip in the group), it keeps playing and decides again after another stretch.
- Launching a clip by hand wins over the pending action, and **Stop all clips** ends the scenes' actions.
- The randomness repeats the same way every time you start the transport and when exporting: you get what you heard.
- A scene counts loops by its longest clip, and Next, from the last scene with clips, goes back to the first.
- The action travels with the clip when you move or duplicate it, and goes with it when you delete it.
+++

### Record into a slot

1. Arm the track with its **●** button. Its empty slots show a red dot.
2. Click a slot's red dot. The take starts at the next quantization point, after the count-in if there is one and the transport was stopped. The slot turns red with "Recording · bar N".
3. Click again to finish. The take is rounded to whole bars and keeps playing in a loop.

> Note: you can only record into empty slots. To record onto the timeline, see [[Recording]].

## Arrangement

The Arrangement is the song's timeline: each track is a row, and clips are laid out from left to right in the order they play.

At the top is the **loop strip**, below it the **marker** strip and then the **ruler** with the bar numbers and, under it, the **Chords** strip (see [[Chord track]]). In the toolbar you have **Zoom**, **Show all** (sets the zoom so the whole song fits) and **Follow playhead**, which scrolls the view with the playhead during playback. At minimum zoom a five-minute song fits even in a narrow window; the ruler then numbers every 4, 8 or 16 bars.

> Tip: the zoom also sets the grid. Zoomed in, clips snap to beats; zoomed out, to bars.

### Move around the song

- **Click the ruler**: moves the playhead and the start marker.
- **Drag on the ruler**: scrubs through the song.
- **Click a marker**: the playhead jumps to it.
- **Wheel**: scrolls the tracks up and down. {⇧ + rueda}, or the trackpad, scrolls sideways. The ruler and the **Master** row stay pinned, and when you create or pick a track out of view, the view goes to it.
- When you drag clips near the edge of the view, it scrolls by itself: sideways and, when moving clips, up and down too.

### Repeat a section with the loop

1. Drag in the top strip to mark the region. The loop turns on by itself.
2. Click **⟲** in the transport to turn it on or off.

When it wraps around, the clips' notes end at the loop's end; the ones you're holding on the keyboard keep sounding.

### Mark the song's sections

**Markers** name the parts of the song (intro, verse, chorus…). They live in the strip below the loop strip.

- **Double-click** the strip: a new marker there.
- **Drag** a marker: moves it along the grid.
- **Right-click** a marker: rename it, **Go here**, delete it, or add a **Transition here ▸**. See [[Transitions]].
- **Right-click** the empty strip: **Add marker here** and **Transition here ▸** at the nearest bar.

> Note: [[Auto-Arrange]] puts a marker on each section, and when you [[Importing from Ableton Live]] its locators come along.

### Add and move clips

- **Double-click a gap**: on a MIDI track it creates an empty four-bar clip (the length set in [[Settings#Projects]]), which ends earlier if there's another clip further on; on an audio track it opens the file picker.
- **Click**: selects the clip and opens it in the editor. {⌘ clic} or {⇧ clic}: several.
- **Drag in a gap**: selection box for clips and tracks ({⌘} or {⇧} add to it).
- **Drag a clip**: moves the selected ones, also to other tracks of the same type. {⌥} + drag copies them.
- **Right edge** (the cursor changes to ↔): changes the length. If the clip is longer than its content, the content repeats in a loop; lines mark each repetition.
- **Drop audio files** (from the Finder or the browser): on an audio track, the clip starts at the nearest grid line (a beat or a bar, depending on the zoom); in an empty space, they create a new track. With {⌘} or {⌥} held, it lands exactly where you drop it. MIDI files drop the same way.

To copy, paste, split and join, see [[Selecting, copying and pasting clips]] and [[Split, join and consolidate]].

### Fade-ins and fade-outs

A fade makes an audio clip come in from silence or go out to it, with no abrupt cuts.

1. Select an audio clip. Two small white squares appear under its title, in the corners.
2. Drag the left one inwards for the fade-in, and the right one for the fade-out. The volume curve is drawn over the waveform, and what it removes is shaded.
3. To remove a fade, double-click its square.

They move freely; with {⇧}, along the grid.

+++ How it works
- They're measured in beats, so they follow the tempo.
- They use an equal-power curve, with no volume dip in the middle.
- Together they never take up more than the clip.
- A project with fades asks to update in earlier versions of Pulse, which would play it without them.
+++

### Clip menu

Right-click a clip:

- **Takes** (on loop recordings): pick the take that plays, and **Show the takes in rows…**. See [[Takes and comping]].
- **Accompany**: a bass, an arpeggio, a pad, rhythmic chords or a melody for its chords. See [[Accompaniment]].
- **Split here** ({⌘E} cuts where you clicked), **Slice ▸** (into equal pieces) and **Join N clips** ({⌘J}).
- **Deactivate clip** ({0}): see below.
- **Copy**, **Cut**, **Duplicate** ({⌘D}) and **Delete** ({⌫}).
- With a single clip: **Name** and **Color**.
- Audio clips only: **Separate into stems…**, **Lyrics ▸** (see [[Lyrics]]), **Detect tempo**, **Use this clip's tempo in the project**, **Consolidate** and **Reverse**.
- MIDI clips only: **Export MIDI…**. See [[MIDI files]].
- **Convert to audio on a new track**: what the selected clips play becomes audio. See [[Tracks#Convert a range to audio and resample]].

### Deactivate a clip without deleting it

**Deactivate clip** ({0}) leaves the clip where it is, grayed out, but silent: not its notes, its audio or its automation, not even when exporting. Do it again to activate it.

- With several selected, it deactivates them all (or activates them, if they were all already deactivated).
- It's also in the Session clips' menu.

+++ Details
If you deactivate a clip while it plays (or move, trim or delete an audio clip that's playing), it goes silent with a short fade, without a click.
+++

### Gap menu

Right-click a gap on a track:

- **Paste here**, if there's something copied.
- **Add sample here…** (audio tracks) or **New MIDI clip here** (MIDI tracks).
- **Transition here ▸**: a riser, a downlifter, an impact or a reverse cymbal at the nearest bar. See [[Transitions]].

### Track header

The header buttons, from left to right:

- **Number**: the track activator; when it's off, the track is silent.
- **S**: solo.
- **●**: arm for recording.
- **In / Auto / Off**: monitoring (audio only); each click moves to the next mode. See [[Recording]].
- **A**: shows the automation lane. See [[Automation]].

Right-click the header: **Name**, **Color**, **Show automation**/**Hide automation**, **Show takes**/**Hide takes**, **Duplicate**, **Delete**, **Freeze**, **Convert the range to audio** and **Resample to a new track**. Right-click the Master's header: **Resample the Master**. See [[Tracks]].

#### Track height

- Drag the bottom edge of a header to make that track taller or shorter. Double-clicking the edge returns it to the height of the others.
- {⌥ + rueda} over the Arrangement changes the height of all of them.

+++ Details
- Height goes from 40 to 200 points; the Master row's height doesn't change.
- A track's own height goes with it: if you move it, delete others or duplicate it (the copy comes out just as tall), it doesn't change. It's saved with the project.
- It belongs to the view, like scrolling: {⌘Z} doesn't undo it, and undoing something else doesn't change it. A deleted track that comes back with {⌘Z} comes back with its height.
- The {⌥ + rueda} height, the one for all tracks, is also saved with the project: when you open it, the tracks look as you left them. A project without it opens with the usual height.
+++

> Note: tracks that are playing Session clips look dimmed in the Arrangement. **► Arrangement**, in the transport, brings them back.

## Tracks

Tracks are the song's lanes: each has its instrument or its audio, its effects and its fader. Here you learn to create, order, group and freeze them.

### Create and order tracks

- {⌘T}: new MIDI track, with Pulse Synth.
- {⇧⌘T}: new audio track.
- **Select**: click the header. {⌘ clic} adds or removes; {⇧ clic} selects a range.
- **Move**: drag the header. All selected tracks move (not while recording).
- **Name and color**: right-click the header.
- **Duplicate Track** / **Delete Track**: right-click the header or the **Create** menu. With several selected, they act on all of them. Copies are named "<name> copia".

> Note: you can have up to 64 tracks, and there's always at least one left. {⌘Z} undoes.

### Group tracks

A group gathers several tracks into one: they play through **its fader and its effects**, like a bus, and fold away to save room.

1. Select the tracks.
2. Press {⌘G} (or use the **Create** menu, or right-click the header › **Group track**). They move into a new group where the first one was.
3. Click **▾** on the group's header to fold it; **▸** unfolds it.

- **Ungroup** ({⇧⌘G}): removes the group; its tracks stay where they are.
- **Take out of the group** (right-click a track in the group): moves it out, right below.
- **Put a track in**: drop it right below the group's header.
- **Dragging** the group moves it with all its tracks. **Duplicating** or **deleting** a group also duplicates or deletes its tracks.
- You can group groups (groups inside groups).

+++ How a group works
- A group's tracks sit right below it, with a bar in the group's color on their header (and indented in the Arrangement). Folded, its columns are hidden in the Session and its rows in the Arrangement.
- The group has a **mixer** (volume, pan, sends, activator, solo), an **effect chain** (in Devices) and **automation**, like any track.
- A **solo** on the group plays all its tracks; a solo on one of its tracks keeps the group playing.
- **Deactivating** the group silences everything in it, including what its tracks send to the returns.
- **Session**: the group's slots light up where its tracks have clips; a click launches the ones in that scene (and stops the group's other tracks). Its ■ stops all its tracks.
- **Arrangement**: the group's row shows its tracks' clips in miniature. You can't put clips on it.
- A group has no instrument or clips: it can't be armed and doesn't receive notes.
+++

### Add a rumble to the kick

The **rumble** is the low boom glued to the kick in techno. Pulse builds it from your kick.

1. Right-click the header of the kick track.
2. Choose **Create rumble**. A new track appears below.
3. If it's too loud for your kick, lower its fader. Adjust the reverb, drive or filter to taste.

The rumble **follows the kick**: it has no clips of its own, so if you change, remove or launch other kicks, it follows them, and if you mute the kick track it mutes along with it. When the kick stops (in a break), it keeps sounding for about another bar and fades out.

+++ How it works
- The track plays what the kick track plays, through a long, fully wet reverb, a Tube saturation with 4× oversampling and a 24 dB filter at 180 Hz, with a Ducker that gets out of the way on each hit.
- The saturation evens out its level: it sits about 5 dB below a hard techno kick. With a softer kick, lower its fader.
- It can't be frozen, because it has no clips of its own.
- A project with a rumble only opens in this version of Pulse or newer.
+++

### Move a drum sound to its own track

With a **Drum Machine**, you can move one of its sounds (the kick, for example) to a track of its own, to give it its own EQ, saturation and fader, or so that a Ducker listens only to the kick.

1. Right-click the header of the drum track.
2. Choose **Move to its own track ▸** and the sound. A new track appears below that plays it.
3. Repeat with other sounds if you like; each one goes to its own track.

In the Drum Machine, a moved sound shows an arrow (**Kick →**); hovering over it tells you which track it plays on. If you delete the new track (or undo), the sound goes back to the drums.

+++ How it works
- The new track has no clips: it plays that sound as it comes out of the Drum Machine, before the drums' effects, and the drums stop playing it. It starts with the drums' fader and pan, so it's just as loud as before. The pattern is still a single clip on the drum track.
- If you mute the drums, their moved sounds are muted too.
- When exporting by track, each sound comes out in its track's stem.
- While it has moved sounds, the drum track can't be frozen.
- A sound plays on only one track: a sound's track isn't duplicated on its own (it would play twice). Duplicate it together with the drums, for example by grouping them and duplicating the group: the copy plays its sounds from the copied drums, and the sidechains inside also listen to the copies.
- A project with moved sounds only opens in this version of Pulse or newer.
+++

### Freeze a track

**Freeze** renders what a track's instrument and effects play to audio, and the track then plays that audio: it stops using CPU. Volume, pan and sends stay live.

1. Right-click the header › **Freeze track**.
2. The track gets a blue frame and ✱. Its instrument and effects can't be edited.
3. To go back, use **Unfreeze track** (or the **Unfreeze** button in the bottom panel).

- **Flatten to audio**: the track becomes an audio track with the frozen audio, for good.

> Warning: if you change the content of a frozen track, it unfreezes by itself. The same happens if you change the project's tempo, swing or key, or what its sidechain listens to.

> Note: only one track freezes at a time. An audio track with no effects doesn't need freezing.

+++ How it works
- It also unfreezes if what its sidechain listens to changes: if that track listens to another, or if it's a group and you change its tracks.
- It sounds the same with sidechain too: whatever triggers it is taken into account when freezing (a group with all its tracks, and the track that triggers the one that triggers it).
- With tempo changes, the frozen audio follows the tempo automation as it played.
- Session clips' volume, pan and send envelopes stay live; those of Arrangement clips are baked into the audio.
- The first hit plays in full (without the clips' fade-in) and, when exporting, the frozen tail (reverb, delay) is exported complete.
+++

### Convert a range to audio and resample

Turn what a section of the song plays into audio, on one or more tracks, a group or the Master, to slice it, reverse it, process it as audio or save CPU.

The **range** is that of the clips selected in the Arrangement or, if there are none, that of the loop. The menu tells you which bars it will take.

1. Select the clips (or set the loop over the range).
2. Right-click the header of the track or group › **Convert the range to audio**. With several tracks selected, it converts them all. It's also in the clips' menu: **Convert to audio on a new track**.
3. Below each one appears an audio track with what was playing, with the same volume, pan, sends and automation, inside the same groups. The clips in that range are cut at its edges and deactivated: the song sounds the same.

- **Resample to a new track** does the same but touches nothing: the new track comes out muted (turn it on with its number to hear it instead of the original).
- **Resample the Master** (right-click the Master's header): the whole song over that range, as when exporting it, on a new muted audio track.
- {⌘Z} undoes it.

+++ How converting to audio works
- It renders as when freezing: the track's instrument and effects (for a group, its tracks and its effects), without its volume, pan or sends, which stay live on the new track. With the effects' tail: the new clip lasts as long as the reverb or delay does.
- The audio is set to Re-Pitch at the project's tempo, which plays back identically sample for sample. If you change the tempo later, switch it to Stretch in the audio editor so its pitch doesn't change. With tempo automation, it follows the tempo as it played.
- Sends from a group's tracks to the returns aren't included in the group's audio.
- Whatever the track triggered on another track via sidechain (a ducker listening to it) stops being triggered in that range, because its clips are deactivated.
- The files go to `Samples/Convertidos`, inside the project folder.
+++
# Recording and playing

## Recording

Record audio from a microphone or an interface, notes from a MIDI keyboard, and the moves you make on the controls. You can record in the Arrangement, on the timeline, or into a Session slot.

### Choosing which tracks record

The **armed** tracks record: the ones with the ● button in their header turned on.

- If you press record with no track armed, the selected tracks get armed.
- The notes you play live go to the armed tracks. If there are none, they go to the selected one.

### Recording in the Arrangement

1. Arm the track with ●.
2. Put the cursor where you want to start.
3. Press {F9} or the transport's **●**. Pulse starts playing and recording from the cursor, after the count-in if you've chosen one.
4. Press {F9} or **●** again to stop recording without stopping the music, or {Espacio} to stop everything.

What gets recorded:

- **MIDI**: a "Toma" ("Take") clip that starts where you started and is rounded to whole bars. **Rec Q**, the transport's record quantization, is applied to it. The pitch bend, mod wheel, aftertouch and any other controls you move stay in the clip, under the notes. See [[Expression: pitch bend and controls]].
- **Audio**: one clip per armed audio track.
- **Automation**: with **A** (Automation Arm) on in the transport, the controls you move are recorded too. See [[Automation]].

> Note: new material goes on top. Existing clips aren't deleted or trimmed.

### Capturing what you just played

Pulse always listens to what you play on the armed track (or the selected one, if none is armed), even when you're not recording. If you improvise something you like, keep it:

1. Play. The transport's **Capture MIDI** button (next to ●) lights up when there's something to capture.
2. Press that button or {⇧⌘C}.
3. You get a "Captura" ("Capture") clip with the last thing you played: in the track's first free slot (in the Session) or in the Arrangement.

- **With the music playing**: the clip starts at the bar of the first note and lasts whole bars (1, 2, 4 or 8). In the Arrangement it lands where you played it.
- **With the transport stopped**: the first note is the start of the bar. If the project is still empty, Pulse works out the tempo from what you played and sets it in the project (if it's close to the current one, it keeps the current one).
- Only the last phrase is captured: anything before a silence of more than 3 seconds is left out. At most, the last 8 bars.
- What's been captured isn't captured again. {⌘Z} removes the clip.

+++ How capturing works
- Pulse stores the notes you play in the interface, along with when you played them (from the MIDI keyboard, the computer keyboard or your voice). The audio thread does no extra work.
- The sustain pedal lengthens notes just like when recording. A key you're still holding ends when you capture.
- The tempo is searched between 70 and 160 BPM: the one that puts the notes on an eighth-note grid, and between two equally good ones, the one closest to 120.
- **Rec Q** isn't applied: the notes stay as you played them. Quantize them in the editor if you like.
+++

### Recording into a Session slot

On an armed track, empty slots show a red dot.

1. Click the dot. The take starts at the next quantization point, after the count-in if there is one and the transport was stopped.
2. The slot turns red with "Recording · bar N".
3. Click again to finish. The take is rounded to whole bars and keeps playing in a loop, with the bend and controls you played (see [[Expression: pitch bend and controls]]).

You can only record into empty slots. More about slots and clips in [[Session]].

### Turning a Session jam into an Arrangement

While you record in the Arrangement with {F9}, the Session clips that are playing are written into the Arrangement, on their track and at the exact moment they played. That way you can improvise a song by launching clips and scenes and have it laid out on the timeline.

- It doesn't matter whether you launch clips one by one, with scenes, or they were already playing before you started recording. In that last case they're written from where the recording started.
- If only Session clips are playing, recording doesn't arm any track.
- Whatever was in the Arrangement on that track in that range is replaced.
- With the loop on, each clip is written only once. Punch in/out applies too.

> Tip: when you finish, the tracks keep playing the Session. Press **► Arrangement** in the transport to hear what you recorded.

### Punch in / out

Use it to re-record just one part without touching what's around it.

1. Mark the part with the loop region, in the strip at the top of the Arrangement.
2. Turn on **Punch in**, **Punch out** or both: they're the two red buttons next to the loop in the transport.
3. Put the cursor a little before the part and record. You hear what comes before so you can come in on time, but the take starts right at the loop start.

- **Punch in**: whatever plays before the loop start isn't recorded.
- **Punch out**: recording stops by itself at the end of the loop and playback goes on.
- They work with the loop on or off. With the loop on and punch in, the take starts at the loop start (what comes before isn't kept as a separate clip) and each pass is a take: see [[Takes and comping]].
- In the Arrangement, red lines mark where recording starts and ends.
- They work for audio, MIDI and automation, and are saved with the project.

### Count-in

Next to the record button, choose **No count-in**, **Count-in 1 bar**, **Count-in 2 bars** or **Count-in 4 bars**. Before starting, the metronome plays for that many bars, with the first beat accented, and the position shows "Count N".

- It works for the Arrangement and for Session slots when the transport is stopped.
- Stopping cancels it.

### Recording audio

- Choose the input in **Settings › Input (audio recording)**. It opens when you arm an audio track.
- Recordings are WAV files in `Samples/Grabaciones`, inside the project folder. If you haven't saved the project yet, they go to `Sin guardar/Grabaciones` and are moved when you save.

> Warning: the first time, macOS asks for permission to use the microphone. On Windows, if nothing gets recorded, check **Settings › Privacy & security › Microphone** and turn on "Let desktop apps access your microphone".

### Hearing your input: monitoring

**Monitoring** decides whether you hear the input (your voice, your guitar) through Pulse. It only exists on audio tracks: in the Session it's three buttons above the mixer; in the Arrangement, one button that cycles through the modes.

| Mode | What you hear |
|---|---|
| **Auto** (default) | The input while the track is armed. |
| **In** | Always the input, instead of the clips. |
| **Off** | Never the input: listen to it directly from the microphone or the interface. |

With the track armed, its meter shows the input level.

> Tip: use headphones, so the microphone doesn't pick up what's coming out of the speakers.

### Getting takes in time: latency

**Latency** is the small delay the system adds when taking sound in and sending it out. Pulse subtracts it so what you record lands where you played it.

**Settings** shows the input and output latency, and has the **Recording offset** (from −200 to +200 ms):

- If audio takes land late, raise it.
- If they land early, lower it.

### Latency compensation

Some effects put their sound out slightly late. Without compensation, their track would sound behind the others. Pulse makes everything sound together.

- The song comes out as late as its slowest path. **Settings › Audio** shows that delay as **effects**, next to the latencies.
- To play or sing live through one of those effects without delaying the rest, turn off **Compensate for effect latency** in **Settings › Audio**. Exporting always compensates.

+++ How it works
- Effects that add delay: the **Limiter** (1.5 ms), the **Saturator** at 4× (36 samples), the **Tuner**, **Voice: Pitch and Formants** (32 ms), the **Transition** (128 samples) and many plugins (Pulse asks them how much).
- Each track waits for the slowest one: inside its group, for the group's tracks; outside, for every track in the project.
- Sends arrive together at the return tracks, and what comes out of them arrives in time with the rest.
- The metronome is delayed along with the song.
- The takes you record and the notes you play are placed where you heard them.
- When exporting and freezing, the file starts right in its place, without the delay.
- The Tuner is compensated with its latency for an average voice (about 5 ms). Since it shifts the pitch by reading a period earlier or later, a low voice or a bass ends up a few milliseconds later.
+++

## Takes and comping

Record the same part several times in a row and keep the best of each. That's called **comping**: building one perfect take out of several.

### Recording several takes

1. Turn on the loop in the Arrangement over the part you want to record.
2. Arm an audio track and record with {F9}.
3. Let it go round several times: each pass is a **take**.

When you stop:

- Over the loop you get one clip with all the takes ("Take 1…N"). The last one plays.
- What you recorded before the loop start ends up in a separate clip.
- A half-finished pass counts if it lasts at least half a beat. What you didn't get to record stays silent.

> Note: MIDI has no takes. The notes from all passes are merged into the same clip (this is called overdub). With controls, a pass only replaces what earlier passes had in the range where you moved that control.

### Choosing a whole take

Right-click the clip › **Takes** › the one you want. The one playing has a ✓.

### Putting together the best of each take

When recording finishes, **one row per take** appears under the track, with its waveform. What's playing is shown highlighted.

1. **Drag** over a part of a row: that take plays in that part. The clip is cut there and the part reads that take from the same point.
2. Repeat with any parts you want from other takes.
3. To use one take for the whole clip, **click** its row.

- Adjacent parts that end up playing the same take are joined automatically.
- Show or hide the rows with **Show takes** (right-click the track header) or **Show the takes in rows…** (right-click the clip).
- {⌘Z} undoes.

+++ Details
Switching from one take to another uses a short crossfade, so you don't hear the cut.
+++

## MIDI controllers

Play with a MIDI keyboard and control Pulse with your controller's knobs, faders and buttons. With macros, a single knob moves several controls at once.

### Playing with a MIDI keyboard

Plug it in and play: Pulse listens to **all** connected MIDI keyboards, on all channels.

- The transport's **MIDI** button lights up if there's one. Click it to look for them again.
- Notes go to the armed tracks or, if there are none, to the selected one.
- **Pitch bend**: ±2 semitones on Pulse's instruments (the Modular has its own range).
- **Mod wheel** (CC 1): vibrato.
- **Brightness** (CC 74) and **aftertouch**: open the filter on Pulse Synth, Super Saw, Acid Bass, Wavetable, Polychrome and Sampler, and add brightness to FM Synth and Pulse Keys. See [[Expression: pitch bend and controls#How it sounds]].
- **Expression** (CC 11): the instrument's volume, like an expression pedal.
- When recording, all of this stays in the clip. See [[Expression: pitch bend and controls]].
- **Sustain pedal** (CC 64): keys you release with the pedal down keep sounding until you lift it, on any instrument. When recording, each note is kept as long as it sounded, so it plays back the same without the pedal. Playing a sustained key again ends the previous note.
- The Kick Designer and the drum kits don't use bend or the mod wheel. Plugins receive everything.

### Mapping a knob to a control (MIDI learn)

1. Press **Map** in the transport (or **Transport › Assign MIDI Controls**). The **MIDI Map** window opens and the mappable controls are marked with an outline.
2. Click a Pulse control: a knob, a fader, a plugin parameter or a transport button. The click picks it without changing it.
3. Turn a knob or press a button on the controller.
4. Repeat with other controls and press **Done**.

> Tip: the order doesn't matter. If you turn the knob first, you have 10 seconds to touch the control.

- You can map **Play**, **Stop**, **Record**, **Loop**, **Metronome** and **Automation Arm**.
- Each CC moves a single control: mapping one that's already in use moves it to the new control. With **One knob for several controls** ticked, it's added to what it already moves: a single knob to open the filter, raise the reverb and lower the volume at the same time.
- The list shows the mappings. **×** removes one.
- **From** and **To**: the part of the control's range the knob covers, in %. For example, a filter from 20 % to 70 % so it never closes fully or opens all the way. With **From** above **To**, the knob moves it in reverse.
- **Smoothing**: how long the control takes to reach where you put the knob, up to 2 s. It removes the steps of a 0–127 knob and turns a quick gesture into a slow sweep.
- A mapped CC no longer reaches the instrument.
- Parameter mappings are saved with the project; transport mappings apply to every project.

+++ Details
A mapping follows its control: if you move, duplicate, group or delete tracks, or move effects in the chain, the knob keeps moving the same thing. If you delete the track or the effect, its mapping goes with it. One that no longer points to anything (from an old project) shows as "(no longer exists)" and does nothing; remove it with **×**.
+++

### Moving several controls with a macro

**Transport › Macros…** opens the project's eight knobs. Each one moves the controls you assign to it at once, each control over its own range: a single gesture to open the filter, raise the reverb and add saturation in the break.

1. Press **controls** under a macro.
2. Click the knobs, faders or plugin parameters you want it to move. They're marked with an outline, and the click picks them without changing them.
3. Press **controls** again to finish.

- In the list, **From** and **To** say where the macro puts each control at its minimum and maximum, in % of its range. With From greater than To it goes in reverse: the macro opens one filter while closing another.
- **×** removes a control. **Name** is what you'll see on the knob.
- A macro can be mapped to a MIDI knob like any control: **Map**, click the macro and turn the knob.
- When recording automation, the movement of each control the macro moves is recorded.
- What a macro moves follows it when you move tracks or effects, and goes away if you delete them.
- Macros are saved with the project.

## Grid controllers

Plug in a Launchpad or an APC and launch Session clips with its pads, with no setup.

- Pulse recognizes the **Launchpad Mini MK3**, **Launchpad X**, **Launchpad Pro MK3**, **APC mini**, **APC mini mk2** and **APC40 mkII** by their port name. If you plug one in with Pulse open, press **Look for MIDI devices again** in [[Settings]].
- The pads launch clips (an empty pad stops its track, or records if it's armed), the right-hand column launches scenes and the arrows move the area it controls. In the Session, an orange box with its name marks that area.
- The lights follow the Session: the clip's color dimmed when it's stopped, lit while it plays, blinking while it waits its turn, and off if the slot is empty.
- Its pads don't play notes: the keyboard and the other controllers keep playing the instruments.

### Each model

- **Launchpad Mini MK3 and Launchpad X**: **Session** turns the bottom row into track stop buttons, and the last scene button into stop all. Pressing Session again switches it back to clips.
- **Launchpad Pro MK3**: the row below the grid stops the tracks and **Stop Clip** stops everything. The arrows are ← → at the top and ↑ ↓ on the left.
- **APC mini and APC mini mk2**: the buttons below stop the tracks; **Shift** plus the first four move the area (↑ ↓ ← →); Shift plus the last scene button stop everything.
- **APC40 mkII**: its grid is 8 × 5. **Clip Stop** stops each track, **Stop All Clips** stops everything and the bank buttons move the area.

### Another pad controller

1. Open [[Settings]] → **Grid controllers**.
2. Press **Learn as a grid** next to its port.
3. Press three pads: the top-left one, the one to its right and the one below it.

From then on its pads launch clips and light up with notes, if the controller understands them. **Forget the grid** sets it back to playing notes.

+++ Details
- The area can also be moved with the arrows in Settings, which show which tracks and scenes it covers.
- When you quit Pulse, the controllers are switched off and go back to their normal mode.
- Pulse puts the Launchpads in programmer mode and the APC40 in its DAW mode; the APC minis don't need it.
+++

## Computer keyboard

Play notes with the computer keyboard when you don't have a MIDI keyboard at hand. Press {M} or the transport's **Keyboard** button to turn it on or off.

| Keys | What they do |
|---|---|
| {A} {S} {D} {F} {G} {H} {J} {K} {L} {;} | White keys, from C |
| {W} {E} {T} {Y} {U} {O} {P} | Black keys |
| {Z} / {X} | Octave down / up |
| {C} / {V} | Velocity down / up |

- The button shows the base note and the velocity, for example "Keyboard C3 · 100".
- Notes play on the armed tracks or, if there are none, on the selected one, and are recorded like those from a MIDI keyboard.

> Note: while you're typing in a text field, the keyboard doesn't play.

## Playing with your voice

Use your voice as a MIDI keyboard: sing, hum or whistle into the microphone and the track's instrument plays, note by note, as you sing. You can also beatbox and play a drum kit.

### Playing notes by singing

1. Select a track with an instrument, or arm the ones you want to play.
2. Press **Voice** in the transport.
3. Sing, hum or whistle. The button shows the note that's playing, for example "Voice · A2".

> Tip: use **headphones**. If the microphone hears the speakers, it also plays what's coming out of them, especially the bass.

- A note holds even if your voice wavers a little around it, and changes when you settle on another. If you stop for an instant and sing again, it's a new note ("la-la").
- How loud you sing is the note's **velocity**.
- With a **Scale** in the project, notes you sing out of tune go to the nearest scale note. Notes you sing in tune stay, even if they're not in the scale. See [[Key and scale]].
- The first time, macOS asks for permission to use the microphone.

### Recording what you sing

Record as with a MIDI keyboard: arm a MIDI track and record ({F9} or into a slot) while you sing. With **Follow the pitch**, the bend is recorded into the clip too.

### Options (right-click Voice)

- **Octave**: −1 or −2 to play a bass with a high voice; +1 or +2 for a lead with a low voice.
- **Chords**: **Triads** or **With sevenths**. Each note you sing plays with its chord: it's the root, with the third and fifth (and seventh) of the project's **Scale** on top. In C major, a D gives D minor and a G with sevenths gives G7. With no scale, or with a note outside it, the chord is major (with a major seventh). Chords are recorded note by note, as if you'd played them. For low chords, lower the octave.
- **Follow the pitch**: your voice's vibrato and slides toward another note bend the note that's playing (pitch bend, up to ±2 semitones). If you sing slightly out of tune, it sounds out of tune like your voice; if the **Scale** has corrected the note, it sounds in tune and moves from there. A slow slide passes through the notes in between. You hear it live and, when recording, the bend stays in the clip with the notes.

+++ How it works
- A note starts sounding about 25–35 ms after you start singing it. The lowest ones take a little longer: the voice has to go through a couple of cycles.
- Your voice is tracked on its own thread, even with Pulse behind another window.
- To change notes you have to settle on another for about 30 ms; for an octave jump, about 60.
+++

### Playing drums by beatboxing

**Right-click** **Voice** › **Drums (beatbox)**. Instead of singing notes, beatbox and play the Drum Machine on the selected track or the armed ones.

| Sound | Instrument | Note |
|---|---|---|
| "boom" | Kick | C1 |
| "pf" or "ka" | Snare | D1 |
| "ts" | Closed hi-hat | F#1 |

- How hard you do it is the velocity. The button shows the last sound, for example "Voice · Snare".
- **Record** as with a keyboard: arm the track and record ({F9} or into a slot) while you beatbox. Each hit is stored as a short note.
- Octave, chords and follow the pitch are for notes: they don't apply to drums.

#### Teaching your sounds

If Pulse doesn't recognize some of your sounds well:

1. Right-click **Voice** and press **Kick**, **Snare** or **Hi-hat**.
2. Make that sound 4 times, as you would when playing.

From then on that sound of yours plays that instrument. Taught sounds have a ✓ and are saved in Pulse's settings. Teaching it again replaces the previous one; **Forget what you taught** goes back to the built-in sounds.

+++ Details
Each hit sounds about 15–20 ms after you make it.
+++

> Tip: to turn something already recorded into MIDI, use **Convert to MIDI › Melody**, which analyzes it more carefully and gets cleaner notes. For a beatbox recorded as audio, **Convert to MIDI › Beatbox**. See [[Audio to MIDI]].

# Editing

## Selecting, copying and pasting clips

Pick one or more clips and copy, move or duplicate them, on the same track, between tracks or from one view to the other. It works the same in Session and in the Arrangement.

### Selecting clips

- **Click** a clip: selects it.
- {⌘ clic} or {⇧ clic}: adds clips to or removes them from the selection.
- **Drag from an empty space**: draws a box and selects what's inside.

### Copy, cut, paste and duplicate

| Shortcut | What it does |
|---|---|
| {⌘C} / {⌘X} / {⌘V} | Copy, cut and paste the selected clips, also across several tracks at once and from one view to the other |
| {⌘D} | Duplicate |
| {⌫} | Delete |

- **Pasting in the Arrangement**: the clips go right after the selected ones or, if there are none, at the cursor's bar. Each copied track goes to its matching track and the clips keep their spacing.
- **Paste here** (right-click an empty space): pastes at that exact point.
- **Pasting in Session**: from the selected slot downward; if they don't fit, further down.
- **Duplicate** in the Arrangement puts the copy right after. With several clips, the whole block goes after the last one, without moving them relative to each other. In Session, the copy goes to the next free slot.

> Note: in the clip editor, if notes are selected, these shortcuts act on the notes. While you type in a text field, the shortcuts belong to the field.

## Split, join and consolidate

Cut a clip into pieces, merge several into one, or turn what plays into a new audio file. It's all done from the Arrangement, with shortcuts or by right-clicking the clip.

### Cutting a clip into two or more pieces

1. Click the clip where you want to cut. An orange mark appears, snapped to the grid; with {⌥ clic}, exactly where you clicked.
2. Press {⌘E} (**Split Clip**).
3. For more pieces, repeat: click, {⌘E}, click, {⌘E}…

- If you haven't clicked the clip, {⌘E} cuts at the playhead (the one you move in the ruler).
- **Split here** (right-click the clip) cuts where you clicked.

### Slicing into equal parts

Right-click a clip → **Slice ▸**: **Every beat**, **Every 2 beats**, **Every bar**, **Every 2 bars** or **Every 4 bars**, following the grid lines.

- With several clips selected, it slices them all.
- The pieces stay selected, so you can move them or delete the ones you don't need.
- Each part sounds the same as before and the files aren't touched.

+++ Details
- A MIDI note that crosses the cut ends there, and what's left of it plays at the start of the right-hand part: you hear its attack again.
+++

### Joining clips

{⌘J} (**Join Clips**) merges two or more clips on the same track into one, from the start of the first to the end of the last.

- MIDI clips merge their notes.
- Audio clips are rendered to a new file, with silence in the gaps.

### Consolidating and reversing audio

- **Consolidate** (right-click an audio clip): what the clip plays, with its warp, transposition, gain and fades, goes into a new audio file with just its part.
- **Reverse** (right-click one or more audio clips): creates a new file, "… (invertido)" ("reversed"), with what the clip plays from end to start. Use it for a cymbal, a vocal or a hit that swells into the change, like "reverse" effects. Its clip envelopes are flipped with it, and reversing it again brings it back to how it sounded, with its name.

> Warning: when consolidating (and also when joining or reversing), the clip loses its takes. For now, join, consolidate and reverse don't apply the track's effects.

+++ How fades work when cutting
- When consolidating, joining or reversing, the clip's fades are baked into the new file; the new clip has no fades, so they aren't applied twice.
- When you **split** a clip with fades, the left part keeps the fade-in and the right part the fade-out. If you cut in the middle of a fade, each part keeps its piece of the curve: together they sound the same as the whole clip. A project with a fade split like that only opens in this version of Pulse or a newer one.
- **Consolidate** or **Reverse** on a piece of a split clip stores a bit more sound on each side in its file, so it keeps crossfading with its neighbor at the cut.
- Two back-to-back audio clips with no gap between them (like the parts of a split clip or comped take pieces) crossfade into each other for 10 ms at the cut instead of each fading to silence. When you join them, the new file keeps that crossfade.
+++

## Transitions

Effects for section changes (risers, downlifters, impacts), made to fit your song: in its tempo and in its key.

### Adding a transition

1. In the Arrangement, right-click a marker, the marker strip or an empty space on a track.
2. Choose **Transition here ▸** and the type.

| Transition | What it is |
|---|---|
| **Riser** (1, 2, 4 or 8 bars) | Noise and a tone rising from low to high and from soft to loud, ending right on that bar. The riser before the drop |
| **Downlifter** (1, 2 or 4 bars) | The opposite, from that bar: it falls and fades out (downlifter) |
| **Impact** | A low hit that drops in pitch, with noise and a tail, two bars long, from that bar |
| **Reverse cymbal** (1, 2 or 4 bars) | A backwards cymbal that swells into that bar |

### Where they go

- Each transition is an audio clip in your tempo on a **Transitions** track, which is created the first time; later ones go to it (also to a **Transiciones** track, from a project made in Spanish).
- If you change the tempo, the transition follows it.
- The files go to `Samples/Transiciones` inside the project.
- The [[Mix and Master Assistant]] treats that track as effects: it doesn't compress or mix it like a bass.

> Tip: they play at −6 dBFS. Turn them down with the track's fader and give them reverb or delay with the sends (see [[Mixer]]).

+++ Details
- The tone is in the project's key, if it has one ([[Key and scale]]), and in A otherwise.
- If a riser doesn't fit entirely before the start of the song, it starts at the start.
+++

## Automation

Automation makes a control move by itself while the song plays: the volume rising in the chorus, a filter opening, the tempo speeding up. You draw it with points on a lane or record it by moving the control.

### Drawing automation on a track

1. In the Arrangement, press the **A** button in the track header: its automation lane appears.
2. Choose the parameter from the dropdown: volume, pan, sends or any knob of the instrument, the effects or a plugin. With many parameters, a search box appears. The ones that already have automation are marked with ●.
3. Click the lane to add points.

| Action | What it does |
|---|---|
| Click | Adds a point |
| Drag a point | Moves it |
| Double-click a point | Deletes it |
| {⌥ + arrastrar} a segment | Curves it: upward bows the curve out, downward sinks it. For more natural fades and rises |
| {⌥ + doble clic} on a segment | Makes it straight again |

- **Clear automation** (in the header or by right-clicking the lane) removes it.
- **Hide lane** hides it without deleting it.

### Moving an automated control by hand

Automated controls have a red dot and move by themselves. If you move one by hand, its automation is **overridden**: the dot becomes hollow and the lane turns gray. Press **► Arrangement** in the transport to put it back in charge.

### Changing the tempo during the song

The last row of the Arrangement is the **Master**. Its **A** button shows its automation: **Tempo**, volume, pan and its effects' knobs.

- On the **Tempo** lane, points at different tempos make jumps (a slow intro and a faster chorus). A line between two points is an accelerando or a ritardando; {⌥ + arrastrar} curves it.
- The lane goes from 40 to 240 BPM.
- When you **export**, the song lasts as long as its tempos make it.
- Moving the transport's tempo by hand overrides the automation (its outline shows this) until you press **► Arrangement**.

> Warning: audio with warp **Off** doesn't follow the tempo. It plays at its own speed, also during tempo automation, without going out of tune or jumping; but if you change the tempo by hand while it's playing, it can jump. In a song with tempo changes, set the audio that should follow it to **Stretch** (see [[Audio editor]]).

+++ What follows the tempo
Everything follows tempo automation: MIDI clips, audio in Stretch, Beats or Re-Pitch, the metronome, delays and arpeggiators.
+++

### Recording automation

1. Turn on **A** (Automation Arm) in the transport.
2. Record in the Arrangement (see [[Recording]]).
3. Move any control: with the mouse, with a mapped MIDI knob or in a plugin's window.

- What you record replaces the points that were in that range.
- With the loop on, each pass replaces the previous one where you move the control and leaves what was there where you don't touch it.
- When recording a clip in Session, that track's controls are recorded as clip envelopes.

### Clip envelopes

Automation inside a clip, which repeats with it every time it plays.

1. Open the clip in the editor.
2. In the **Lane** dropdown, choose **Clip envelopes** (the other option is **Velocity**; in an audio clip, **None**).
3. Draw as on the track: click adds a point, dragging moves it, double-clicking deletes it, {⌥ + arrastrar} on a segment curves it and {⌥ + doble clic} straightens it.

- **Clear envelope** removes it.
- Envelopes are only in charge while the clip plays. When it ends, volume, pan, sends and parameters go back to what the mixer and the track's automation say.

## MIDI editor

Where you write and edit a MIDI clip's notes. It opens when you select a MIDI clip; on drum tracks, each row has the name of its sound.

When you open a clip, and when you write notes with **Chords**, **Generate beat**, **Accompany** or **Another variation**, the editor goes to where the notes are: on a drum kit, to the rows that play (kick and snare first, if they don't all fit).

### Writing and editing notes

| Action | What it does |
|---|---|
| Double-click an empty spot | Creates a note |
| Double-click a note | Deletes it |
| Click | Selects ({⇧ clic} adds) |
| Drag a note | Moves it ({⌥}: copies it) |
| Drag the right edge | Lengthens the note |
| Drag an empty spot | Selection box |
| Click an empty spot | Deselects and places the orange marker, where {⌘V} pastes |
| Click a key on the keyboard on the left | You hear it |

- **Draw** mode ({B}): click adds or deletes notes; click and drag adds a note and lengthens it.

### Editor shortcuts

| Shortcut | What it does |
|---|---|
| {⌘A} | Select all |
| {⌘C} / {⌘X} / {⌘V} | Copy, cut and paste (also between clips) |
| {⌘D} | Duplicate |
| {⌫} | Delete |
| {Esc} | Deselect |
| {← →} | Move along the grid ({⇧}: one bar) |
| {↑ ↓} | Transpose ({⇧}: one octave). With **Fold**, one scale note |
| {⌘U} | Quantize |
| {R} | Roll on the selected notes, like the last one |

When you move notes with the arrows, the view follows them: if they were about to go off the top, the bottom or a side, it scrolls just enough to keep them visible. While they're visible, it doesn't move.

### Rolls

Trap hi-hat and snare rolls, in one step.

1. Select the notes (click, or a box over the range). For a one-beat roll, a one-beat note is enough.
2. Right-click → **Roll** **1/16**, **1/16T**, **1/32**, **1/32T** or **1/64**. It's also in the **Roll** button on the bar.
3. Each note is split into notes of that length that fill it. {R} repeats the last roll.

- **Velocity**: **Grows** (soft to loud, like a roll coming in), **Fades** (loud to soft) or **Flat**. The ramp goes from 35 % to 100 % of the note's velocity.
- The roll stays selected: choose another length and it's redone entirely with it.
- {⌘Z} undoes it.

+++ How it splits the notes
- Selected notes on the same row that follow each other with no gap form a single roll. That's why a finished roll can be redone with another length.
- The ramp's velocity comes from the loudest note in the range.
- A note shorter than the chosen length stays as it is.
+++

#### Rolls that rise in pitch

In a **Drum Pads** or **Drum Machine** clip, the roll menu also has **Tone** (the pitch): **Same**, **Up 5**, **Up 12** or **Down 12**, the semitones of the last hit.

1. Choose the pitch and then the roll length, as usual.
2. Each hit sounds a little higher (or lower) than the previous one: the rising trap hi-hat.

- The pitch goes in the clip's **pitch bend**: it rises from the first hit to the last and returns to its place when the roll ends. You can see and tweak it on its lane (see [[Expression: pitch bend and controls]]).
- Each hit takes the bend there is when it starts, and only pads (or sounds) with **Bend** follow it: the kick that falls in the middle of the roll doesn't go out of tune.
- If the roll's pad didn't follow the bend, it now follows it by an octave (**Bend 12**).

### Transforming notes

**Transform**, on the editor bar, changes the selected notes (or all of them, if none are selected) in one of these ways:

| Transformation | What it does |
|---|---|
| **Strum** | The notes of each chord come in one after another, **Up** (low to high) or **Down**, over the time you choose; **Curve** bunches them at the start or the end. The chord's end doesn't change. |
| **Humanize** | Nudges each note's timing and velocity a little at random. The same **Seed** always gives the same result; **Another** tries a different one. |
| **Velocity** | **Ramp** from one velocity to another, **Compress / expand** the differences from the average, or **Flip** (loud becomes soft). |
| **Length** | **Legato** (each note reaches the next) or **Multiply** its length (×¼ for staccato, ×2…). |
| **Ornaments** | A **Grace note** right before each note (in scale steps if there's a key), or a **Flam**: the same note, earlier and softer. |
| **Arpeggiate** | Each chord, note by note (**Up**, **Down** or **Up and down**), at the grid and length you choose. |
| **Connect** | Fills the gaps between one note and the next with scale steps (chromatic if the project has no key). |
| **Invert** | **Pitches**: high goes low (within the scale, if there's a key). **Retrograde**: end to start. |

1. Press **Transform** and pick one. The clip changes immediately: you see it and, if it's playing, you hear it.
2. Move its settings until you like it.
3. **Apply** keeps it (a single {⌘Z} undoes it). **Cancel**, or closing the menu, leaves the notes as they were.

- With a right-click on the notes, **Transform ▸** applies any of them immediately, with the last settings you chose.
- Quantize, groove (with its humanize that differs on every pass), rolls and **To scale** have their own buttons on the bar.

### More room for the notes

When you open a clip, the detail panel has room for two octaves of notes, and the Session or the Arrangement stays visible above. With no clip, the panel collapses to a strip and returns to its height when you open another.

- **Maximize** the editor: the button on the right of the panel, {⇧⌘E} or double-click its top edge. It takes up almost the whole window; the same brings it back to its height.
- **Taller or shorter**: drag the edge. Pulse remembers the height for next time.
- **Taller or shorter rows**: {⌥ + rueda} over the notes, or the **Height** slider. The row under the mouse stays where it is.
- **The bar** takes one row if it fits and two at most. In a narrow window, the least-used controls go into **⋯**; they work the same there and the menu doesn't close when you use them.

+++ How Pulse decides the height
- With no height chosen, the panel takes 45 % of the window or what two octaves of the editor need, whichever is more, and leaves at least about 110 points for the Session or the Arrangement.
- The height you choose by dragging is saved as a proportion of the window in Pulse's settings, not in the project: it applies to every project and any window size.
- Maximized, it leaves a strip of the view above so you know where you are.
- Rows go from about 8.5 points, still readable, to two and a half times their normal height.
- In **⋯**, the grid, note and groove appear as lists, because a dropdown can't open inside another menu. The dropdowns (Loop, Grid and Chords) never leave the bar.
+++

### Changing velocity

Choose **Velocity** on the bottom lane and drag up or down near a note. Selected notes change together.

### The editor bar

- The clip's **Name**.
- **Loop**: 1, 2, 4, 8 or 16 bars (from the bar where the clip is), or any number of sixteenths in the **/16** field. A 3/16, 5/16 or 7/16 clip over 4/4 shifts every bar: good for polyrhythms.
- **Grid** (1/4 to 1/64, with 1/8, 1/16 and 1/32 triplets) and **Note** (the length of new notes). Each clip remembers its own: if you set 1/16T on the hi-hats, the bass stays at 1/16. A clip you haven't opened yet takes the grid its notes are written in.
- **Draw** ({B}): pencil mode.
- **Quantize** ({⌘U}) with its **Strength** (%) and **note ends** (also quantizes the end of the notes). It acts on the selected notes or, if none, on all of them.
- **Chance** (%): on how many loop passes each note plays (see "Notes that don't always play", below).
- **-12, -1, +1, +12**: transpose. **×2**: doubles the loop by copying the notes. **Roll**: see [[MIDI editor#Rolls]]. **Transform**: see [[MIDI editor#Transforming notes]]. **Zoom** (the width of the beats) and **Height** (the height of the rows).
- **⋯**: whatever doesn't fit on the bar. Hover over it to see what's inside.
- With a scale in the project, **Fold** and **To scale** (see [[Key and scale]]).
- **Chords**: writes chords from the scale, whole progressions, and suggests the next one (see [[Chords]]).
- On drum tracks, **Generate beat** (see [[Generating beats]]).
- Under the ruler, a strip shows the **chord** playing in each range.

### Notes that don't always play

With **Chance** you choose on what percentage of loop passes each selected note plays (or all of them, if nothing is selected). A hat at 60 % or a ghost snare note at 30 % makes the loop vary by itself. Notes with a probability look paler, with their %.

+++ How the randomness works
- It's random, but always the same randomness: the song sounds the same every time you play it, when you export it and when you freeze the track.
- Each copy of a clip in the Arrangement has its own randomness: if you duplicate a hi-hat loop at 60 %, each copy plays different notes.
+++

### Giving a clip groove

**Groove** changes when and how hard the notes sound as they play, without moving them.

- **The project's**: the transport's swing (see [[Transport]]).
- **One of its own for this clip**, with:
  - **Swing** and its grid.
  - **Accents**: softer offbeats.
  - **Humanize**: small random shifts in timing and velocity, different on every pass.
- Ready-made grooves: **Straight**, **Swing 1/16**, **MPC 1/16**, **Shuffle 1/8** and **Human**.
- **From your recordings**: the grooves you've extracted from audio (see the next section). Each note that falls on the grid moves like its hit in the recording and takes its strength. **Time** and **Dynamics** say how much: 100 %, the same as the recording; 50 %, halfway. **Humanize** is added on top.

### Extracting the groove from audio

Copy the feel of a drum loop, a break or your beatbox and apply it to any MIDI clip.

1. Right-click an audio clip → **Extract groove**.
2. Pulse listens to where each hit lands relative to the grid and how hard, and saves it in the project with the clip's name.
3. In any MIDI clip's **Groove**, choose it under **From your recordings**.

- Pulse knows whether the loop moves in **sixteenths** or in **eighths** (a shuffle). The status bar says so, along with how late its offbeats are: "in sixteenths, the offbeats 30 % of a step late".

> Warning: a project with grooves extracted from audio doesn't open in Pulse versions from before this feature (format 11), so they aren't lost.

+++ How it measures it
- It analyzes the hits step by step across one bar.
- If the whole recording is a little late or early (the clip starts a few milliseconds before the hit), that doesn't count: what it saves is how its hits deviate from the beats.
- It follows the clip's warp: if the loop is fitted to the tempo, the groove comes from how it sounds that way.
+++

## Expression: pitch bend and controls

What you do with the keyboard's wheels and controls while recording stays in the clip, just like the notes: pitch bend, mod wheel, aftertouch and any CC. You can also draw it by hand under the notes.

### Recording it

1. Record MIDI as usual, in the Arrangement or into a slot (see [[Recording]]).
2. Move the bend, the mod wheel or whatever you like while you play.
3. When you finish, the editor opens the clip with the first recorded control under the notes.

- It's stored as it sounded, with fewer points: the ones that don't change the shape are removed.
- Recording in a loop, each pass replaces a control only where you moved it. Everything else stays.
- The **sustain pedal** isn't recorded as a control: it lengthens the notes, which then sound the same without the pedal.

### Seeing and drawing it

1. Open the clip in the editor.
2. In **Lane**, under **Clip MIDI expression**, choose a control the clip has (◆) or add one: **Pitch bend**, **Mod wheel**, **Aftertouch**, **Brightness**, **Expression**, **Breath** or **+ CC** with its number.
3. Draw on the lane:

| Tool | What it does |
|---|---|
| **Pencil** | Drag to draw freehand |
| **Line** | Drag from one point to another: a straight ramp |
| Drag a point | Moves it |
| Double-click a point | Deletes it |
| {⌥ + arrastrar} between two points | Curves that segment ({⌥ + doble clic}: straight again) |

- **Grid**: what you draw snaps to the editor's grid.
- **Clear lane** removes that control from the clip.
- Pitch bend snaps to the center as you get close to it, like a real wheel.

### How it sounds

- Controls repeat with the clip on every pass, also in the Arrangement and in launched clips.
- When you stop or the clip ends, the bend returns to center, and the mod wheel and aftertouch to zero: a note is never left out of tune.
- On Pulse's instruments: **bend** bends ±2 semitones, the **mod wheel** adds vibrato and **expression** (CC 11) lowers the volume.
- **Brightness** (CC 74) at center changes nothing; up adds brightness and down takes it away. **Aftertouch** adds on top:

| Instrument | Brightness (CC 74) | Aftertouch |
|---|---|---|
| Pulse Synth, Super Saw, Acid Bass, Wavetable, Polychrome | Opens or closes the filter | Opens the filter |
| Sampler | Opens or closes the filter; downward it darkens even if the filter is open | Opens the filter |
| FM Synth | Raises or lowers the modulation index: more or fewer harmonics, by the same amount on every preset | More index: it shines on long notes too |
| Pulse Keys (electric pianos) | More or less "bark" and a brighter or darker tone, without jumping in volume | Tremolo |
| Pulse Keys (piano) | Notes you play with brightness up come out with a harder hammer, and with it down, a softer one; it also lifts or dulls the highs of what's already sounding, without raising the volume | Tremolo |
| Pulse Keys (organ) | Pulls out or pushes in the high drawbars (4' and up) and opens or closes the tone filter | Vibrato, like a Hammond's scanner: deeper the harder you press, at the same volume |
- The **Modular Synth** and **plugins** receive all controls and do whatever they have assigned.
- **Exporting MIDI** writes them; when you **import** a MIDI file, its bend, aftertouch and controls arrive as lanes.

> Warning: a project with controls in its clips doesn't open in Pulse versions from before this feature (format 15), so they aren't lost.

+++ Details
- Controls are read every 64 samples, always on the same samples: it sounds the same at any buffer size and when exporting.
- On FM Synth and Pulse Keys, brightness and aftertouch glide over about 5 ms, so a jump doesn't click. At center and with no aftertouch, presets sound exactly the same as without them.
- While a clip with a control plays, that control is in charge; the keyboard can move it, but the clip sets it again as soon as its curve changes.
- Between two recorded values with a long pause, the value holds until just before the next one: a jump stays a jump.
+++

## Audio editor

Where you set how an audio clip sounds: whether it follows the tempo, its pitch, its volume, its fades and its tuning. It opens when you select an audio clip.

### Making audio follow the tempo

Choose the **Warp**:

| Warp | What it does |
|---|---|
| **Off** | Plays at its own speed, without following the tempo |
| **Re-Pitch** | Follows the tempo by changing speed and pitch, like a record |
| **Stretch** | Follows the tempo while keeping the pitch |
| **Beats** | For drums: each hit starts where it should and plays at its own speed, without the echo Stretch leaves when the tempo changes a lot. If the tempo is slower, a short silence is left between hits |

- **Original BPM**, with **÷2** and **×2** if the detector landed on double or half.
- **Tempo: Detect / Use in the project** (see [[Audio tempo]]).

### The clip's controls

- **Length** in beats, and **Fit to file**.
- **View**: **The clip** or **Whole file**. In "Whole file", clicking or dragging on the waveform moves the clip's **start**.
- **Start** in seconds. It can be negative, to start a bar before the file.
- **Fades** **in** and **out**, in beats: the same as the handles in the Arrangement. In Session they're applied on every loop pass.
- **Gain** (−36 to +12 dB) and **Transp.** (±24 semitones, Stretch only).
- The audio's **Key**, **Follow the scale** and **Detect** (see [[Key and scale]]).
- Warp **Markers** and **Remove** (see below).
- **Slice → To Drum Pads** and **Separate into stems…** (see [[Slicing and separating stems]]).
- **Tuning → Edit notes…**: tune a vocal note by note (see below).
- **Lane**: clip envelopes (see [[Automation]]).

### Tuning a vocal note by note

Like Melodyne: on a clip with a vocal, or an instrument that plays one note at a time, you see each note as a block and move it wherever you want.

1. Select the clip and press **Tuning → Edit notes…**. The first time, it analyzes the voice for a few seconds.
2. The waveform is replaced by its **notes**: each one is a block over the keyboard, at the pitch it sounds. Below it runs the pitch you sang (gray line) and, if you've changed it, how it will sound (in the clip's color).
3. Drag a note up or down, or double-click to tune it.
4. **← Waveform** goes back to the audio editor. A tuned clip shows **Edit notes… ✓**.

| Action | What it does |
|---|---|
| Drag a note | Changes its note, in whole semitones: move it just a little and an out-of-tune note becomes in tune. With {⌥}, it moves freely, in cents |
| Double-click a note | Tunes it to the nearest note of the project's **Scale** (with no scale, to the nearest semitone) |
| Click | Picks a note |
| {⇧ clic} or {⌘ clic} | Adds notes to or removes them from the selection. Dragging one of the selected notes moves them all |

- **Tune**, **No vibrato** and **As sung** act on the selected notes or, if none, on all of them. **No vibrato** holds each note still on its pitch; **As sung** returns it to how it was.
- With a note selected, at the top you see where it was sung and where it's going ("A2 +30 cents → A2") and its **vibrato**: 100 % as sung, 0 % none, over 100 % more pronounced.
- {⌘Z} undoes instantly.

> Tip: to tune live while you sing, or a whole track without going note by note, use the **Tuner** effect (see [[Devices and effects]]).

+++ How it works
- The voice keeps its timbre when you move it (its vowels don't change, it doesn't sound "chipmunky") and keeps the same length. What you don't touch stays exactly the same.
- When you let go, the clip switches to playing a new file with the tuned vocal, in the project's **Afinados** ("Tuned") folder. Copies of that clip change with it; other clips using the original recording don't.
- When you save, tuned files that nothing uses anymore are deleted.
+++

### Fitting audio whose tempo varies

With warp markers you pin moments of the audio to beats of the song. They go in the strip under the waveform, with Warp set to anything other than **Off**.

1. Double-click the strip: adds a marker. The first one also adds one at the start and one at the end.
2. Drag a marker: that point of the audio lands on that beat, and the sections on either side speed up or slow down.
3. Double-click a marker: removes it.

## Audio tempo

When you import or drop audio, Pulse finds its tempo and bars and adjusts it to play in time with your song. It takes about one second per minute of audio.

### What it does on import

- If it finds a clear rhythm, the clip switches to **Stretch** at that tempo, or to **Beats** if it's a drum loop (by its name, or because it has no key).
- The clip starts at the bar of the first downbeat and lasts whole bars.
- If the tempo varies, it puts a warp marker on every bar.
- If it doesn't find a clear rhythm (a vocal, an ambience), the clip goes back to **Off** and plays at its own speed, as long as the file.

### Correcting it by hand

- **Detect tempo** (right-click an audio clip, or **Detectar** in the [[Audio editor]]): analyzes it again.
- **Use this clip's tempo in the project**: sets the project to the clip's tempo.

> Tip: on a loop of up to 32 s, if the file name states its tempo ("Tops 150", "Bass_128bpm") and at that tempo it lasts exact bars, Pulse uses that tempo even if the detector hears another.

+++ How it picks the tempo
- A short loop (up to 32 s) lasts whole bars.
- If its **name states its tempo** and at that tempo it lasts exact bars, the name wins, whatever the detector says (some tops without a kick might come out at 245). Also when slicing it into the Drum Rack.
- Otherwise, if the tempo found is close to the one that makes it last exactly 1, 2, 3, 4, 6, 8, 12 or 16 bars, it uses that one: a 4-bar loop at 150 stays at 150, not 151.
- If the detector is further off, but the loop lasts exact bars at the project's tempo, it uses the project's.
- While it's being analyzed, a short audio file is treated as a loop of whole bars: at the tempo in its name if it states one and lasts exact bars at it, or of 1, 2, 4 or 8 bars at the tempo closest to the project's.
  If in the end it doesn't find a clear rhythm, it undoes that assumption: the clip goes back to **Off**, unless you changed it yourself in the meantime.
+++
# Compose

## Key and scale

The scale is your song's key. With it, the MIDI Editor shows you which notes fit, chords write themselves, and the loops you add play in your key.

### Choosing the scale

1. In the transport bar, open **Scale**.
2. Click **Detect in the song** to let Pulse find it, or pick one of the 24 by hand (major and minor).
3. To remove it, choose **No scale**.

**Detect in the song** listens to the notes in your MIDI clips (leaving out drums) and to audio with a clear key. The status bar tells you which scale it picked, and warns you if it isn't clear.

> Note: E minor and G major use the same notes. If you get the relative key and it's not the one you want, pick it by hand.

The scale is saved with the project and can be undone with {⌘Z}. Drum tracks don't use it.

Chords (in the editor, in **Chords** and on sliced pads) and scales are spelled as in the key signature: with flats in keys that have flats (in F minor, **Db** and **Eb**) and with sharps in keys that have sharps (in E major, **C#** and **G#**). The leading tone of a minor scale is always a sharp (**C#** in D minor). With no scale, or in C major and A minor, the usual names are used (C#, Eb, F#, Ab, Bb).

### Writing notes in the scale

With a scale in the project, the [[MIDI editor]] helps you stay inside it:

- Rows for the scale's notes look lighter, and the tonic's rows are orange, with their name.
- **Fold** shows only the scale's rows, all with their names, plus the rows of any notes you already have outside it, so none get lost. When you drag notes up or down, or use {↑} {↓} and **-1 / +1**, they jump from scale note to scale note: there's no way to step outside.
- **To scale** moves notes that aren't in the scale to the nearest scale note: the selected ones or, with no selection, all of them.

In [[Audio to MIDI]], **Melody** uses the project's scale instead of looking for one.

### Loops in your key

Audio you add (by dragging it, from the browser or with Import) is put into the project's scale, just as it's put to its tempo. A piano loop in F minor plays in A minor in your song without you doing anything.

- If the project **has no scale** yet, it takes the key of the first loop that has one, and the following loops adjust to it.
- If you **change the project's scale**, every loop that follows it moves with it.
- **Drums** aren't touched, even if their name says a key: a hats loop "Tops_150_Dm" stays as it is.

In each clip's [[Audio editor]]:

- **Key** shows the file's key, and **Detect** looks for it again.
- **Follow the scale** fits it to the project's scale or leaves it in its own.
- Moving **Transp.** by hand makes it stop following the scale.

To keep new loops from being adjusted, untick **Fit new loops to the scale** in the transport's **Scale** menu.

+++ How it works
- Pulse looks for **each file's key** first in its name, the way loop libraries write it ("Pad_120_Am", "Keys F#m", "Bass_E", "124Gm"), and if it's not there, in its sound, when it's clear. If the name says one key and the sound a very different one, the sound wins.
- The clip is **transposed by the shortest path**, at most 6 semitones, to the project's scale. If one is major and the other minor, it goes to the relative key: a loop in E minor in a song in G major doesn't move.
- Audio counts as drums if it has a drum name (kick, snare, hat, perc, drum, tops, groove, shuffle, rumble, fill, conga…) or has no notes that stand out.
+++

## Chords

Pulse reads the chords in your clips while you edit and helps you write them: one click per chord, with suggestions for what comes next and ready-made progressions.

### Seeing a clip's chords

In the [[MIDI editor]], below the ruler, a strip shows the chord playing in each stretch: **Am**, **G7**, **C/E** (C with E in the bass)… With a scale in the project, also its degree: **Am · vi**.

- It updates as you edit.
- It marks at most one chord every half bar.
- Loose melody notes on top don't change the chord.

To get the chords from audio (a song, a stem, a piano loop), use [[Audio to MIDI]] → **Chords**.

### Writing a progression

You need a **Scale** in the project (see [[Key and scale]]).

1. Click an empty spot in the editor where you want to start: the orange marker goes there.
2. Open the **Chords** menu in the MIDI Editor's bar.
3. Click a chord: it's written at the marker, and the marker moves to the end of the chord.
4. Keep clicking: click, click, click builds the progression.

The new notes stay selected, so you can move or transpose them.

- If the chords don't fit, the **clip's loop gets longer** in whole bars until they all fit (in the Arrangement, the clip too, up to the next one). A four-chord progression in a one-bar clip makes it four bars.
- {⌘Z} removes the chords and puts the orange marker back where it was; {⇧⌘Z} puts them back.
- A clip that doesn't have its own name yet is named after its progression: "Am F C G".

> Tip: the chords that usually follow the one before the marker are shown in orange ("After G usually come: C, Am"). If you don't know where to go next, start with those.

### Chords menu options

- The **seven chords of the scale**, with their degree (I, ii, iii, IV, V, vi, vii°).
- In minor, also the **major V** (from harmonic minor), the dominant that pulls strongly to the tonic.
- **With sevenths**: four-note chords instead of triads (Cmaj7, Dm7, G7…).
- **Length** of each chord: 2 beats, 1 bar or 2 bars.
- Ready-made **Progressions**:

| Scale | Progressions |
|---|---|
| Major | Pop (I–V–vi–IV), Ballad, '50s, Jazz (ii–V–I), Rock and Canon |
| Minor | Epic (i–VI–III–VII), Andalusian (i–VII–VI–V), Minor, Dramatic and Lament |

+++ How it voices the notes
Each chord puts the bass at the bottom and the other notes close together, in the inversion nearest to the previous chord's. That way the voices move as little as possible, the way a pianist would play it.
+++

## Chord track

The chords of the whole song in a strip of the [[Arrangement]], below the ruler. Write them by hand or take them from audio, and let the bass, pads and melodies follow them.

### Writing the chords

- **Double-click** an empty spot in the **Chords** strip: it writes a one-bar chord (or up to the next one). It's the tonic of the **Scale** or, if there's already a chord before it, the one that usually comes next.
- **Right-click** the strip: the seven chords of the scale by degree (I, ii, iii…), with the ones that usually come next in orange. Each click writes one and moves to the next spot: click, click, click builds the progression.
- In that menu, **Name** accepts any chord: **Am**, **F#m7**, **G/B**, **Bb**, **Gsus4** or, with a scale, its degree (**vi**, **V7**). Click **Set** (or Return).
- **Length**: half a bar, 1, 2 or 4 bars. **With sevenths**: four-note chords.
- **Progressions**: the ones from the [[Chords]] menu, starting where you clicked. With **Repeat to the end of the song**, they fill the whole song.
- Right-click **on a chord**: pick another one to change it, or **Delete chord**.
- **Drag** a chord to move it, or its **right edge** to lengthen or shorten it. A click moves the playhead to its start.

Names follow the project's key (**Bb** in F major, **A#** in B major) and, if there's room, show their degree: **F · VI**. A chord outside the scale shows in violet. {⌘Z} undoes any change.

### Taking them from audio or a clip

- Right-click a clip in the Arrangement (a piano or guitar recording, a song, a MIDI clip) → **Its chords to the chord track**. Pulse listens to them and writes them where the clip is, with their harmonic rhythm: a new chord each time it changes (at most every two beats).
- Right-click the strip → **Take the chords from** → a track: the chords of all its clips.
- **All the song's MIDI clips**: the chords you get by combining the MIDI tracks, without the drums.

> Tip: the full workflow in three clicks: audio with chords → **Its chords to the chord track** → right-click the strip → **Accompany the whole song** → **Bass: root**, and again → **Pad**.

### Making the song follow them

**Accompany**: right-click the strip → **Accompany the whole song** (or **Accompany "Chorus"**, the section between that marker and the next) → a part. It writes a bass, an arpeggio, a pad, rhythmic chords or a melody in a clip of that length, on the empty track for its role or on a new one at the end (see [[Accompaniment]]). If you change chords later, **Another variation** in the editor rewrites it with the new ones.

**Tracks that follow the chords**: right-click a MIDI track's header → **Follow the chord track** and a mode. Its notes don't change: what you hear changes, bar by bar, even if the clip is a one-bar loop.

| Mode | What it does | For |
|---|---|---|
| **Chord notes** | Each note goes to the nearest chord note, without repeating notes in the same hit. | Pads and chords |
| **Chord scale** | Each note goes to the nearest scale note, with the chord notes included. | Melodies and arpeggios |
| **Bass: move the riff** | The whole riff moves to the bass note of each chord and keeps its shape. The clip's most frequent note acts as the root. | Basslines |

Its clips say **follows the chords**. To keep what you hear:

- **Follow the chord track** → **Write it into the notes**: the notes of all its clips become the ones you hear, and the track stops following the chords.
- Right-click a MIDI clip → **Fit to the chord track** → a mode: just that clip.

+++ More details
- [[From humming to song]] on an Arrangement clip writes its chords on the chord track. If there are already chords there and the melody fits them (at least half of what it sings are chord notes), it uses those instead of making up new ones.
- [[Auto-Arrange]] writes the chords playing in the song it builds on the chord track.
- A melody you ask for with **Accompany** on an empty MIDI clip in the Arrangement plays over the chord track if there are chords there.
- A note that comes in a hair before the chord change (less than a 64th note) already counts for the new chord. Where there's no chord, notes play as they are.
- A project with a chord track is saved in format 22: an earlier version of Pulse can't open it.
+++

## Accompaniment

Pulse plays along with you: from a clip with chords (or from the [[Chord track]], for the whole song) it writes a bass, an arpeggio, a pad, rhythmic chords or a melody that follows them bar by bar.

### Adding a part

1. Right-click a clip with chords: MIDI, or audio (a piano, a guitar, a song).
2. Choose **Accompany** and a part.
3. The part is written in the same Session slot or in the same stretch of the Arrangement.

Which track does it go on?

- If there's an **empty track for its role** (a template's **Bass** track, or a "Pad", "Arpeggio" or "Chords" track, or their Spanish names), on that one, with its sound.
- If not, on a new track right below, named after the part: **Bass**, **Pad**, **Arpeggio** or **Chords** (or **Bass 2** if there's already one).

The clip is named after what it plays ("Bass: offbeat"). A new track comes with an instrument that suits it: Acid for basslines, FM for arpeggios and SuperSaw for the pad. Change it like any other.

> Tip: to make the bass or pad pump with the kick, right-click its header and choose **Sidechain from the kick** (see [[Devices and effects#Sidechain from the kick, in one click]]).

Each role has its own color, the same in every song: blue for basslines, turquoise for arpeggios, violet for the pad, lilac for rhythmic chords and orange for the melody. If the chords already have that color, the new track takes the next one in the palette.

It comes in at the fader level of the chords' track (the arpeggio and the pad 1.5 dB below), so that adding a bass, a pad and an arpeggio doesn't push the mix over 0 dB.

### The parts

| Part | What it plays |
|---|---|
| **Bass: root** | The root on every beat, with the odd octave and passing notes into the next chord. |
| **Bass: octaves** | Root and octave in eighth notes (disco). |
| **Bass: offbeat** | On the offbeats (house). |
| **Bass: walking** | One note per beat walking through the chord; it reaches the next one by a semitone. |
| **Bass: rolling** | Sixteenths on the three parts of each beat that aren't the kick (triplets in one take out of four), with the offbeat accented and octaves and fifths that change with each take. The bass of techno and melodic techno. Comes with the Acid "Bass · Rolling techno". |
| **Bass: gallop** | A dotted eighth and a sixteenth on every beat, the first of the bar accented: the gallop of rock, EBM and tech house. Comes with the Acid "Bass · Fat saturated". |
| **Arpeggio up** and **Arpeggio up and down** | The chord notes one after another. |
| **Pad** | Sustained chords, with smooth voice leading. |
| **Chord stabs** | Short chords with a syncopated rhythm. |

### Melody

The last four parts of **Accompany** write the main melody: the lead, the hook or the topline. They go to the empty **Melody** (or **Lead**) track if there is one; if not, to a new **Melody** track with a factory lead that suits the style, 3 dB below the chords.

| Style | What it plays | Default sound |
|---|---|---|
| **Melody: hook** | A short, syncopated one-bar riff that repeats (house, techno). | Pulse Synth "Pluck · Short" |
| **Melody: singable** | A two-bar phrase with long notes that you could sing (pop). | FM "Lead · FM flute" |
| **Melody: trance lead** | Eighths and sixteenths jumping between the chord notes. | Super Saw "Lead · Classic trance" |
| **Melody: call and response** | One bar asks and the next answers with the same rhythm. | Pulse Synth "Lead · Bright saw" |

It sounds like a song, not random notes:

- On every strong beat it plays a note of the chord that's sounding; in between, scale notes moving stepwise. It never clashes a semitone against the chord on a strong beat.
- A one- or two-bar motif repeats through the whole clip and adapts to each chord.
- The melody has a contour: it rises when the motif rises and falls when it falls, with a high point in each phrase. On strong beats it picks, among the chord notes, the one heading in that direction, not always the nearest. No note lasts more than half the melody, not even with **Few notes**.
- Phrases come in pairs: the first stays open and the second resolves to the tonic or the chord's root, just like the end of the clip.
- It follows the clip's time signature (4/4, 3/4, 6/8…) and its length.

In the melody's [[MIDI editor]], next to **Otra variación**, you have three drop-downs:

- **Style**: the same take in another style. The clip is renamed if it's still named after the style. If the track was created by **Accompany** and you haven't touched its instrument, the lead also switches to the new style's default sound. {⌘Z} undoes both. If you've changed the sound, or the melody is on a track you already had, the instrument stays as it is.
- **Density**: **Few notes**, **Normal** or **Many notes**.
- **Register**: **Low** (C2–G3), **Medium** (A2–E4) or **High** (G3–D5), with the editor's note names (C3 is middle C).

Changing them rewrites the same take; **Otra variación** writes a different one.

> Tip: want the melody in a specific clip? Create an empty MIDI clip on the lead's track, in the same scene (Session) or above the chords' stretch (Arrangement), and choose **Accompany** and a melody on it. Pulse writes the melody into that clip over the chords playing alongside it on the other tracks.

+++ How it picks the notes
The key is the project's **Scale** if the chords fit it; if not, the key that best fits the chords. Over a chord outside the scale (E major in A minor), the melody uses its notes (G#) instead of the scale notes that would rub against them (G). An offbeat note that holds over into the next chord anticipates it, as long as it doesn't rub against the chord underneath; a note on a strong beat only holds over if the next chord also has it.
+++

### Trying another version

In the new clip's [[MIDI editor]], **Otra variación** rewrites it a different way over the same chords, as many times as you like, until you edit it.

> Tip: chords are read the same way as in the [[Chords]] strip. With a **Scale** in the project, they come out better.

## Generating beats

**Generate beat** writes a drum beat in the style you choose: house, techno, trap, reggaeton… It can write it as is, or have Pulse's AI play it, with variations and fills like a drummer.

### Generating a beat

1. Open a clip on a drum track (Drum Machine or Drum Pads) in the [[MIDI editor]].
2. In **Generate beat**, choose the **Style**, the **Level** and how **Long** it is.
3. Click **Generate**. The clip's notes are replaced by the beat, and a clip that doesn't have its own name yet takes the style's name ("Techno").
4. **Another version** moves to another beat of the same style and level. {⌘Z} brings back the previous ones.

> Tip: each style sounds like itself at its own tempo. **Project tempo to… BPM** sets it.

### Options

- **Estilo** (styles): House, Techno, Hardgroove, Hard techno, Trap, Drum & Bass, Hip hop, Pop / Rock, Reggaetón and Breakbeat.
- **Level**: **Basic** (just the essentials), **Medium** and **Intense** (more hits, percussion and ghost notes).
- **Length**: 1, 2, 4 or 8 bars. The rhythm repeats, and the **fourth bar** turns the phrase around with a variation.
- **Fill at the end of each phrase**: on the last beat of every 8 bars (or of the clip, if it's 4 bars or longer), each style's own fill. Reggaeton doesn't get one.
- **The kick follows the bass**: adds kicks where the bass starts its notes (in eighths), if there's a bass track playing along with the clip.
- **Sounds**: which instruments the beat can use: kick, snare, clap, hi-hat, ride, crash, rim, toms, shaker, tambourine, cowbell, congas and clave.
  - Each style comes with its own ticked (techno without ride or cowbell, pop with crash, toms and tambourine), and you can remove or add whichever you like.
  - The ones the kit doesn't have are shown crossed out.
  - With percussion only (no kick, snare, clap or hi-hat), each ticked sound plays its own pattern, fuller at each level: a percussion track to play over another track's drums.
  - With **crash** ticked and a clip of 4 bars or more, it plays at the start of each phrase.
- **Style's sound**: on a Drum Machine, it sets the style's machine (**808** for trap, hip hop and reggaeton; **909** for the rest) tuned the way the style sounds: trap's long, deep kick, techno's dry one, drum & bass's high snare… If you've tuned the machine yourself, it only switches machine and keeps your settings.

It works with any kit: Pulse finds each sound by its name, in Spanish or English, with or without accents (kick/bombo, snare/caja, clap/palmada, hat/charles, tom, rim/aro, conga/bongó, clave/bloque de madera…), and uses whatever's there. If a ticked sound isn't there, it uses the closest one you have.

### Letting the AI play it

Tick **Let the AI play it** and Pulse's drum model listens to the style's beat and plays it its own way: it keeps it going, varies it, and throws in fills and changes where phrases end. Each **Otra versión** is a different performance.

- With **Básico** it plays it safer; with **Intenso** it lets loose more.
- The kick the beat has on every beat (the four-on-the-floor of house, techno, hardgroove or hard techno) always stays: the AI varies the rest.
- It plays with the ticked **Sonidos**. Toms come in on fills; from **Medio** up, tambourine and shaker too, and on **Intenso**, congas and cowbell as well.
- It plays in the background: meanwhile the button says **Generating…** and you can keep working. The beat lands in its clip as soon as it's ready; if you're in another clip, when you go back to it.

> Note: the AI runs inside Pulse and works without internet.

+++ What the styles are like
- The beats are written by hand, the way a drummer in each style would play them: house's quarter-note kick and offbeat open hi-hat, trap's snare on 3 and hi-hat rolls, drum & bass's two-step, reggaeton's dembow…, with their accents and ghost notes.
- **Hardgroove** (142 BPM) rolls toms and congas over the quarter-note kick, with a bit of swing, and hi-hats that only play on some passes (per-note probability), so the loop never repeats exactly.
- **Trap** (140 BPM) is half-time, with the snare on 3. With **Caja** (snare) and **Palmada** (clap) ticked, both play together on 3, as is done in trap.
- **Trap hi-hat rolls** change every bar: sixteenth and eighth triplets, 32nds, 32nd triplets and, on **Intenso**, 64ths. Each roll speeds up, moves around from bar to bar, and there are more at the end of each four-bar phrase. On **Básico** there's just the odd one; on **Intenso**, one or two per bar. From **Medio** up, an open hi-hat comes in now and then.
- The other styles' fills (hard techno's claps, for example) also build up to the hit.
- **Hard techno** (152 BPM) has a deep, long, clipped kick (with Drive, so it cuts through the mix) that gallops on Intenso, and clap fills at the end of each phrase.
- Hip hop has **swing**, and house a little: it's set in the clip's **Groove**.
- The AI is a small network trained on the drums of about 70,000 songs (Lakh MIDI Dataset) and on real drummers (Groove MIDI Dataset).
+++

## From humming to song

Hum, sing or whistle a melody and Pulse makes a song from it: the melody as notes, chords, bass and drums in the style you choose.

### Making a song from your melody

1. Record the melody on an audio track **with the metronome**, at the project's tempo (see [[Recording]]). A MIDI clip with a melody works too.
2. Right-click the clip → **From humming to song** → a style (House, Techno, Pop / Rock, Reggaetón, Trap…).
3. Four new tracks appear below its track, in the same slot or the same stretch of the Arrangement.

With an audio clip, Pulse first listens to the melody for a few seconds. The status bar tells you which chords it used ("C F G C"). It's all a single step: {⌘Z} removes it.

### The four tracks

- **Melodía** (melody): what you hummed, as notes, on a synth. Each note snaps to its sixteenth, even if you come in a bit late or sing a bit off pitch. If a note is extra or missing, fix it in the [[MIDI editor]].
- **Acordes** (chords): one chord from the scale per bar, the one that fits the melody best in that bar. Depending on the style, a sustained pad or rhythmic chords.
- **Bajo** (bass): over the chords, in the style's way: on the offbeats in house and techno, on the root in pop or trap, in octaves in breakbeat.
- **Batería** (drums): a beat in the style on a Drum Machine with its sound, with a fill at the end of every eight-bar phrase (or of the clip, if it's shorter).

Each part is a normal track: change its instrument, edit the notes, or write another bass or pad over its chords with [[Accompaniment|Accompany]] (and then **Otra variación** in its editor).

The tracks come in with their faders down (the drums at −8 dB, the rest at −10.5 dB), so the song leaves headroom on the master and doesn't clip.

+++ How it picks the chords
- Melody notes on strong beats count more.
- Where the melody doesn't make it clear, it uses chords that usually follow each other. It starts and ends on the tonic.
- In minor, the V can be major (E in A minor).
- The scale comes from the melody if the project doesn't have one, and becomes the project's scale. If the project has one, it uses that.
+++

## Audio to MIDI

Pull the notes out of any audio (a voice, a piano, drums, your beatbox) and put them on a new MIDI track, to play them with another sound or edit them.

### Converting a clip

1. Right-click an audio clip → **Convert to MIDI**.
2. Choose what you want to extract (see the table).
3. The notes appear on a new MIDI track right below, in the same place: the same slot in Session, the same stretch in the Arrangement.

It takes a few seconds and runs on this Mac, without internet; the status bar shows the progress. You can keep working meanwhile. If you convert the same clip again, for example with another melody option, it's instant: Pulse remembers what it just heard. The notes follow the clip's tempo (its warp), and each note's loudness becomes its velocity.

| Option | What for |
|---|---|
| **Melody** | One note at a time: a voice, humming or a solo instrument. Sing an idea, convert it and play it with any synth. |
| **Melody, unsnapped** | The same, but each note goes to the nearest semitone instead of to the scale. For melodies with notes outside the key, or a well-tuned instrument. |
| **Harmony** | All the notes at once: chords, piano or guitar. |
| **Chords** | The chords of a song or an instrument, one per change (at most every half bar), with the bass and smooth voice leading. It ignores drums and short hits. |
| **Drums** | Each hit becomes a kick, snare or hi-hat, depending on where its energy is, on a Drum Machine. |
| **Beatbox** | Your recorded beatbox becomes drums. |

> Tip: it pairs well with [[Slicing and separating stems|Separate into stems]]. Separate a song and convert the bass or the vocal to MIDI.

### Beatbox to drums

- **"boom"** is the kick, **"pf"** or **"ka"** the snare and **"ts"** the hi-hat, just as with **Voice** in drum mode (see [[Playing with your voice]]) and with the sounds you've taught it there.
- Each hit snaps to its sixteenth, so you don't need to nail the timing. Record with the metronome.
- It doesn't matter how loud you recorded: the loudest hit plays at full velocity and the rest in proportion.
- The status bar tells you how many hits of each it placed.

### Which key the notes come out in

In **Melodía**, each note goes to the nearest note in the song's scale, so singing a little sharp or flat doesn't change the note. Notes you sing in tune stay, even if they're not in the scale.

The key is the project's **Scale** if you've chosen one (see [[Key and scale]]). If not, it comes from your voice, the project's MIDI notes and audio with a clear key (drums don't count). The status bar tells you which one it used. **Acordes** also uses the project's scale if there is one; if not, the audio's.

+++ How it works
- The melody tracks the voice's pitch frame by frame with pYIN, the method used by singing transcription tools, even with very low voices. It splits it into notes where you settle on another note or attack again.
- Harmony is found by Basic Pitch, a model by Spotify (Apache 2.0 license) that ships inside the app.
+++

## Slicing and separating stems

Two ways to split up audio: into hits, to play them from pads, or into its instruments (drums, bass, vocals and the rest).

### Slicing a loop into pads

1. Open the clip in the [[Audio editor]].
2. In **Slice**, choose how to cut: **Transients** (at each hit), **1/16**, **1/8** or **1/4**.
3. With **Transients**, adjust the **sensitivity**: higher gives more slices (it also cuts at soft hits); lower, only at strong ones.
4. Click **To Drum Pads**.

The clip is cut into hits, up to 16, each on a pad of a **Drum Pads** on a new track, "… troceado" ("… sliced"). A MIDI clip plays them in order in the same place, with each note on the grid. When you change the tempo, the groove follows without stretching the audio, and in the [[MIDI editor]] you can reorder or repeat hits.

- **Chord loops**: by transients, it also cuts where the chord changes, even if there's no hit. To slice sustained chords on beats, use **1/4** or **1/8**.
- **Pad names**: the slice number and its chord ("05 · Eb") or, for drums, where it starts in the loop ("05 · 2.1" is bar 2, beat 1). That way you can tell them apart in the editor's rows. Chords are spelled as in the project's scale (Db in F minor).
- If a grid gives more than 16 slices, it widens until they fit, and the status bar says so (for example, "cada 1/8: a 1/16 no caben en 16 pads", "every 1/8: at 1/16 they don't fit in 16 pads").

+++ How it finds the slices
- A hit is a rise in level in some frequency band, weighted toward the highs. Sensitivity lowers or raises the threshold (from four times to a quarter of normal); at 50 % it cuts as usual.
- A chord change counts if the audio has a clear key and the new chord lasts at least two beats (not a passing note). The cut falls on whatever attack is right there.
- A note less than 20 ms from a sixteenth snaps to it.
+++

Drum Pads also has **Slice File…**, to cut audio straight into its pads.

#### Adjusting the slices by hand

Tick **Edit slices**, below "Slice". The slices appear numbered over the waveform, with a strip below:

- **Drag** a slice to move it.
- **Double-click** a slice to remove it; on an empty spot, to add one (up to 16, one per pad).
- **A Drum Pads** then slices at your cuts.

> Warning: changing the mode (Transients, 1/16…) finds the slices again, and you lose any you've moved.

### Separating into stems

A stem is one part of the mix on its own. Pulse extracts four: **Batería** (drums), **Bajo** (bass), **Voz** (vocals) and **Resto** (other).

1. Right-click an audio clip → **Separate into stems…**, or use the button in the audio editor.
2. Wait: you can keep working meanwhile. **Cancel** stops it.
3. Four new tracks appear below, **Batería**, **Bajo**, **Resto** and **Voz**, with the clip in the same place, and the original is deactivated so everything sounds the same.

Each new track is a copy of the original, with its effects, volume, pan, sends, automation and group, and the clip keeps its envelopes.

- It runs on this Mac, without sending anything to the internet.
- The first time, it downloads the model (174 MB) from GitHub, no account needed. If the connection drops or you cancel, next time it picks up where it left off.
- One separation runs at a time, and it needs 4 free tracks (64 tracks max).

## Lyrics

Pulse listens to a vocal clip and writes down what's sung, in Spanish or English, with the timing of each line. The lyrics appear below the waveform and along the clip, and you can correct, copy or export them.

### Transcribing a clip's lyrics

1. Right-click an audio clip → **Lyrics ▸ Transcribe lyrics…**. There's also the **Transcribe…** button in the **Lyrics** row of the [[Audio editor]].
2. Choose the **Language**: **Automatic** recognizes it by listening to the beginning; if you know it, choose **Spanish** or **English**.
3. If you've already separated a song into stems, tick **Isolate the vocals first**: it reads only the vocal and is far more accurate when there's music in the background.
4. Click **Transcribe**. You can keep working meanwhile; **Cancel** stops it.

The part of the file that plays in the clip is transcribed. If you transcribe again, the lines in that part are replaced and the rest are kept.

- It runs on this computer, without sending the audio to the internet.
- The first time, it asks before downloading the speech model (251 MB) from Hugging Face. If the connection drops or you cancel, next time it picks up where it left off.
- Without the isolate option, background music confuses the model: separate into stems first ([[Slicing and separating stems#Separating into stems]]) and transcribe the **Voz** track.

### Viewing the lyrics

- In the [[Audio editor]], a strip below the waveform shows each word where it's sung, with an orange mark at the start of each line.
- In the Arrangement, the words appear along the bottom of the clip. Words that don't fit are hidden: zoom in to see them all.

The lyrics travel with the audio file: clips that use it show them in place even if you cut, move, repeat or stretch them.

### Correcting words

- **Click a word** in the editor's strip: type the right one and press {Intro}. Empty, the word is removed; with spaces, it becomes several. {Esc} leaves it as it was.
- **Lyrics ▸ Edit lyrics…**, or **Edit…** in the editor, opens the lyrics line by line with their timing: correct any line. Leave one empty to remove it.

Everything can be undone with {⌘Z}.

### Copying and exporting

In **Lyrics ▸** or in the lyrics window:

- **Copy lyrics**: all the clip's lyrics to the clipboard, one line per line.
- **Export .txt…**: the lyrics as text.
- **Export .lrc…**: the lyrics with each line's timing, for karaoke and players. On an Arrangement clip, the times are the song's; on a Session clip, from the start of the clip.

+++ How it works
- The model is OpenAI's Whisper small (MIT license), in onnx-community's 8-bit quantized version. It's stored in Pulse's data folder, in `Models/whisper-small`, and each file is checked against its SHA-256 fingerprint before use.
- The audio is resampled to 16 kHz and read in 30-second windows. Each window starts where the last complete line of the previous one ended.
- The model gives the timing of each line. Each word's timing is spread within its line according to its length, so it's approximate.
- Silent parts are skipped, and a window where the model hears no voice is discarded.
- A project with lyrics is saved in format 16: an earlier version of Pulse refuses to open it, instead of opening it and losing the lyrics.
+++

## Auto-Arrange

When you have your ideas in Session scenes, **Create › Auto-Arrange…** turns them into a whole song in the Arrangement: intro, verses, choruses or drops, with build-ups and breaks.

### Creating the song

1. Open **Create › Auto-Arrange…**.
2. Choose a **structure** (see below).
3. Check the table: you can change the **bars** of each section, **which tracks play** in each one (checkboxes), and the fills and cuts.
4. Click **Create**. Pulse takes you to the Arrangement, with a **marker** named after each section and the loop over the whole song.

The structures:

- **Electronic**: intro, build-up, drop, break, build-up, drop, outro.
- **Pop**: intro, verse, chorus, verse, chorus, bridge, chorus, ending.
- **Short**: intro, A, B, ending.

> Warning: **Create** replaces whatever was in the Arrangement. {⌘Z} brings it back.

> Tip: name your scenes after the sections ("Intro", "Drop", "Estribillo", "Break"…) and each section will use its own.

### What it does in each part

- Each section takes a scene (see "How it chooses" below).
- Each section only gets the tracks that belong in it by **role**: no bass in the intro, no drums in the break… In the chorus or drop everything plays: if a track has no clip in that scene, it brings in its clip from another one.
- Drums play in every section that calls for them, even if its scene has no drums: it brings in the drums from the lightest scene that has them. In **Electronic**, the intro and outro have kick and percussion so a DJ can mix, and the build-up has drums for its fill.
- **Build-ups**: with **Roll at the end**, the last bar of the drums is a snare **roll** that speeds up and grows, and the rest of the drums drop out underneath. If you remove the drums from that section, there's no roll.
- **In trap** (snare or clap on 3), **Roll at the end** doesn't add a snare roll: the last bar keeps its rhythm up to beat 3, the hi-hats do a rising triplet roll, and the last beat is left as a **gap** before the chorus.
- **Cuts**: the last beat before the drop is silent, so the drop hits hard.

+++ How it chooses
- **Scenes**: each section takes the scene named after it ("Break" for the break, "Intro", "Drop", "Estribillo"…; also "Build" or "Subida" for the build-up). If there's none, one named after a section with the same energy ("Groove" for the verse). Failing that, the fullest one (most drums, bass and melody) is the chorus or drop, the next one the verse, and the lightest the intro.
- **Roles**: Pulse works out each track's role (drums, bass, harmony, melody) from its instrument, name and notes. The Kick Designer is drums. An audio track is whatever its loops are: a drum loop (in **Beats**, or with a drum name like "Tops_150") is drums, even if the track is called "6 Audio". A Drum Pads that comes from slicing (**Trocear**) a sample with notes (chords, a melody) is harmony, not drums: it doesn't get into the verses as drums and doesn't get a fill. A sliced break is still drums.
- **Audio that doesn't last whole bars**: audio that lasts whole bars loops through the whole section. Audio that doesn't (a vocal set to **Off**, 30.75 beats long) plays once at the start of each whole-bar phrase (1, 2, 4, 8… bars) and stays silent until the next, so the second pass doesn't come in early.
- **Fills**: they're snare rolls, or clap rolls if the snare isn't on 2 and 4; never kick. What drops out underneath is the rest of the drums: the kick on its track and the loops.
+++
# Sound

## Instruments

Instruments are what plays on a MIDI track. Pulse comes with synths, samplers and drum machines.

### Adding an instrument

1. Select a MIDI track.
2. In the browser, open **INSTRUMENTS** and click one.

The large editors (**Editor ⤢**) open one at a time inside the window.

### Synths

| Instrument | What it's for |
|---|---|
| **Pulse Synth** | A simple subtractive synth. |
| **Pulse Keys** | Piano, electric pianos and organ. See [[Pulse Keys]]. |
| **Super Saw** | Up to 7 detuned saws: leads and pads. |
| **Acid Bass** | Monophonic bass with a resonant filter, accent and slide. |
| **FM Synth** | Two operators with feedback: bells, electric pianos. The harder you play, the brighter it sounds. |
| **Wavetable Synth** and **Polychrome Synth** | See [[Wavetable and Polychrome]]. |
| **Modular Synth** | See [[Modular Synth]]. |
| **Kick Designer** | Kicks. See [[Kick Designer]]. |

### Sampler

Plays a sample across the whole keyboard, from its root note.

- Drag an audio file onto its drop zone or use **Load…**.
- Modes: **Classic** (plays while you hold the note), **One-shot** (plays all the way through) and **Loop**.
- Voices: **Poly** (several notes at once), **Mono** (each note cuts the previous one) and **Legato** (a note that overlaps the previous one glides to its pitch without restarting the sample or the envelope; after a silence it starts at its own pitch). **Glide** is how long the slide takes; in Mono it always slides from the previous note. See [[Wavetable and Polychrome#Glide and legato notes (the 808)]].
- Controls: Root, Tune, Start, filter, ADSR and Glide.
- **Presets › FACTORY › 808** has three 808s made with [[Pulse Sounds]]: **Long with glide** (Legato, for trap slides), **Short and dirty** (saturated, short, fast glide) and **Punchy** (Mono: every note with its own punch). The first time you pick one, Pulse takes a moment to prepare its sound. In {⌘F}, search for them with "808".

#### Multisample

For instruments recorded note by note (a piano, some strings):

1. Load or drag **several** samples at once.
2. They're spread across the keyboard by the note in their name: "Piano C3", "Rhodes_F#2", "Strings Db4" (C3 is middle C).
3. Click a key on the map to hear it.

- The map shows the zones over the keyboard; high velocities are at the top.
- **−12 / +12** shift the whole map by an octave, for libraries that call middle C "C4".
- **Zones…** lets you set each sample's notes, root and velocities by hand.
- Up to 32 samples. Loading a single one goes back to normal mode.

+++ How the samples are spread
- Each sample covers the keys up to halfway to its neighbors and plays as is on its own note.
- Several on the same note are **velocity layers**: the softest at the bottom.
- Samples with no note in their name go one per key from C3 up.
+++

### Drum Machine

16 synthesized sounds: kick, rimshot, snare, clap, closed and open hats, three toms, cowbell, crash, ride, shaker, tambourine, conga and clave.

- Click a sound to hear it. They're named as in **Generar ritmo** (Generate rhythm): Kick, Snare, Clap, Hi-hat, Open hi-hat, toms, Cowbell, Tambourine…
- Each sound has **Tune**, **Decay**, **Level** and **Drive**. Drive clips it like a clipper: dirtier and punchier; Level is still its volume.
- **Bend**: how many semitones the sound rises with the pitch bend all the way up (0: it ignores it). Each hit takes the bend there is when it starts, so a roll rises in pitch hit by hit (see [[MIDI editor#Rolls that rise in pitch]]).
- **Machine** changes all the sounds at once:
  - **808**: long, round kick, clap, cowbell. Hip hop, trap, reggaeton.
  - **909**: punchy kick and bright hi-hats. House, techno.
  - **Classic**: Pulse's original sounds.
- To move a sound to its own track (and give it its own effects), right-click the track header → **Move to its own track ▸**.

> Note: in projects from when the Drum Machine had 8 sounds, the 8 new ones come in with Level at 0, so they sound as they used to. Turn them up if you want to hear them.

### Drum Pads

16 pads with your samples. Pad 1 is at the bottom left and starts at C1.

- Click a pad to hear and select it.
- Drag one or more audio files onto the pads.
- Per pad: **Tune**, **Length**, **Level**, **Pan** and **choke** group: pads in the same group cut each other off, like an open and a closed hi-hat.
- **Bend** (per pad): how many semitones the pad rises with the pitch bend all the way up (0: it ignores it). Each hit takes the bend there is when it starts, for rising rolls (see [[MIDI editor#Rolls that rise in pitch]]).
- **Chromatic** (per pad): the notes that aren't pads (from E2 up, and below C1) play that pad, one semitone each; at C3 it plays at its own pitch. Each note cuts the previous one, like an 808. That's how you play an 808 from the pads: put an 808 from [[Pulse Sounds]] on a pad, mark it **Chromatic** and write the line in the editor's **Chromatic** rows (the library's 808s are in C: C3 plays C).
- **Kits** loads a whole kit: the **Basic kit** (the Drum Machine's sounds) or one of the ten genre kits from [[Pulse Sounds]]. If the track still has the name it got when created ("1 MIDI"), it takes the kit's name ("Techno Kit").
- **Slice File…** slices a loop across the pads (see [[Slicing and separating stems]]).

## Pulse Keys

A piano, two electric pianos and an organ, synthesized by Pulse: they load no samples, take up little space and respond to how you play.

### Putting it on a track

1. Select a MIDI track.
2. In the browser, open **INSTRUMENTS** and click **Pulse Keys**.
3. Pick the **Model** on the left of the device, or one of its 34 sounds in **Presets › FACTORY**, grouped into Piano, Tines, Reeds and Organ: pop, ballad, jazz, film or lo-fi pianos, soul, funk or chill electric pianos, and blues, reggae, church, rock or house organs.

When you change model you start from its default sound; the volume stays as it was.

| Model | Sounds like |
|---|---|
| **Piano** | An acoustic grand piano. |
| **Tines** | A tine electric piano, like a Rhodes: round, with a bell on the attack. |
| **Reeds** | A reed electric piano, like a Wurlitzer: more nasal and with bite. |
| **Organ** | A tonewheel organ, like a Hammond, with a rotary speaker. |

### Playing with expression

- How hard you play each note changes its volume and, on the piano and electric pianos, its brightness too: soft sounds dark and hard sounds bright. **Dynamics** sets how much; at 0, every note sounds the same.
- **Sustain pedal**: the notes you let go keep ringing until you lift the pedal, and they're recorded that long. See [[MIDI controllers#Playing with a MIDI keyboard]].
- Pitch bend and mod wheel (vibrato), as on Pulse's other instruments.

### Piano

- **Tone**: how hard the hammers are. Higher is brighter.
- **Length**: how long notes take to die away on their own while you hold them.
- **Release**: how long a note rings after you let go.
- **Hammer**: the hammer's knock at the start of the note.
- **Strings**: how far the strings of each note are detuned from each other, up to about 7 cents. A little is a freshly tuned piano; a lot, a bar piano.
- **Body**: the resonance of the soundboard and the lid.
- **Width**: lows on the left and highs on the right, as if sitting at the piano.

> Note: as on a real piano, the highest keys (from F#5 up) have no dampers and keep ringing after you let go.

### Electric pianos

**Tines** and **Reeds** share controls:

- **Bell**: the metallic shine of the attack.
- **Bark**: how much the pickup growls when you play hard. On soft notes you barely notice it.
- **Tone**, **Length**, **Release** and **Hammer**, as on the piano.
- **Tremolo** and **Trem. rate**: on Tines the sound pans from side to side; on Reeds, the volume goes up and down.
- **Drive**: saturates the amp.

### Organ

1. Pull out the **drawbars** (the nine bars) by dragging them down: 0 is pushed in and 8 is all the way out. Double-click one to pull it out or push it in all the way.
2. Pick the rotary speaker's speed with **Slow** or **Fast**.

- The drawbars, from left to right: 16' (an octave below), 5⅓', 8' (the note), 4', 2⅔', 2', 1⅗', 1⅓' and 1'. The brown ones are the lows, the white ones the octaves and the black ones the fifths and thirds.
- You can move them while you play: they change the notes already ringing too.
- **Percussion**: a bright click at the start of the note, on the **2nd** or **3rd** harmonic. As on the real organ, it only sounds when no other key is held: play legato so it doesn't repeat.
- **Perc. decay**: how long the percussion lasts.
- **Click**: the snap of the contacts when you press and release.
- **Leslie**: how much goes through the rotary speaker. When you change speed, the horn takes about a second to get there and the drum several. The speed can be automated (**Leslie fast**).
- **Tone** cuts highs and **Drive** saturates, for a rock organ.
- The organ doesn't respond to how hard you play.

> Tip: some well-known registrations (each digit is a drawbar): **888000000** jazz, **888800000** gospel, **838000000** blues, **886000000** church, **800000888** whistle, **006802000** ballad and **888888888** all out. They're among the factory presets.

+++ How it works
- There are no samples: everything is synthesized as you play.
- **Piano**: a physical model of strings (waveguides). Each key has two strings in the bass and three in the rest, tuned a few cents apart. One gives up its energy quickly and the others sustain and beat: that's the piano's double decay. Each string is a delay loop closed by a loss filter (highs die out first), a dispersion filter (the string's stiffness stretches the harmonics upward) and a fine-tuning filter. The tuning is stretched too, like a piano tuned by ear. The hammer pushes the string for a few milliseconds: shorter and drier the harder you play, which is why it sounds brighter. It strikes about an eighth of the way along the string, which removes some harmonics. The soundboard sounds separately: dozens of wood modes that give the knock of the attack (**Hammer**), plus five resonances between 98 and 1700 Hz and two short reflections (**Body**). The dampers mute the highs first.
- **Tines and Reeds**: the tine or reed vibrates with few harmonics plus an inharmonic bell partial that dies out in tenths of a second. Then it goes through an off-center pickup: large swings come out bent and even harmonics appear, which are the "bark".
- **Organ**: each drawbar is a pure-sine tonewheel with its own phase. Above the last wheel (about 5.9 kHz), the drawbars fold back an octave, as on the original. The rotary speaker splits lows and highs at 800 Hz. The horn (highs) spins at 0.8 Hz on slow and 6.8 Hz on fast; the drum (lows), at 0.67 and 5.9 Hz. There's Doppler effect and two microphones for stereo.
- Up to 32 notes play at once; if another is needed, the quietest one is reused.
+++

## Kick Designer

A synth just for kicks: you draw how the pitch and volume fall, pick the body, the click and the distortion, and export it as a WAV if you like.

In the track's chain it has a small editor; **Editor ⤢** opens the full one.

### Drawing the curves

The **Tone**, **Amp** and **Drive** tabs have a curve each:

- Drag the points to move them.
- Double-click to add a point, or double-click a point to delete it.
- Drag a segment's circle to bend it.
- Right-click resets the curve.
- In the large editor, **Snap to grid** snaps the points.

### Controls

- **Body**: sine, triangle, square or saw, with phase and harmonics.
- **Sub**: a low end that follows the body.
- **Click**: 12 synthesized ones, or a sample of your own (drag it onto the click box).
- **Key**: the note you play tunes the body, instead of Tune.
- **Distortion** (Soft, Hard, Fold, Tube, Crush), **EQ**, **Punch** and **Limiter**.
- **Length**: free or synced to the tempo, from 1/16 to one bar.
- An oscilloscope shows the attack, and a spectrum shows the fundamental and its note.
- **Export WAV…** saves one hit as a file.

It comes with 18 factory presets by style (see [[Presets]]).

## Wavetable and Polychrome

Two big synths with the same engine. **Wavetable** is the general-purpose one; **Polychrome** organizes it into several engines, with two filters and a sequencer.

### Wavetable Synth

A wavetable is a series of waveforms the oscillator moves through: as it moves through them, the sound changes timbre.

- **Two oscillators**. Each can be **Wavetable**, **Sample**, **Granular**, **Spectral**, **Analog** or **Harmonic**, with Pos, octave, semitone, fine, level, pan, unison and warp (Sync, Bend, PWM, Mirror, Quantize, FM).
- **Sub**, **noise** and **filter** (9 modes, with its curve). **Comb** mode is tuned to its cutoff across the whole keyboard; with lots of resonance and a short burst of noise, it makes a plucked string.
- 3 envelopes and 4 **drawable LFOs** (right-click for basic shapes).
- **Arpeggiator**, and Poly, Mono and Legato modes with Glide.
- **FX**: up to 6 built-in effects, saved with the preset.
- **MATRIX**: 16 modulation routes (source, amount, destination).

It comes with 38 factory presets by category: Bass, Lead, Pad, Pluck, Keys, Arpeggio and FX (see [[Presets]]). In several of them, **Macro 1** does the most useful thing for the sound: opens the filter, changes the vowel or sets the wobble depth.

#### Your own wavetables

- It comes with 12 wavetables. **Load wavetable…** opens your own.
- **Wavetable editor…** lets you draw them: draw keyframes with the mouse (or start from the current table's) and the table morphs smoothly from one to the next. It's saved as a standard wavetable WAV.

#### Drag-to-modulate

1. Find the **SOURCES** row (for example "✚ LFO 1").
2. Drag the source onto any knob.
3. The knob shows colored rings. Right-click it to adjust or remove each modulation.

### Polychrome Synth

The same engine as Wavetable, on **SYNTH**, **SEQ**, **FX** and **MATRIX** pages, with more on top:

- **Two filters**, in series or parallel, with a balance between them.
- **Random** sources (S&H and smooth) and 2 **combiners** that mix two sources.
- **16-step sequencer** (SEQ): note, velocity, gate, probability, slide and a modulation lane.
  - Draw the steps by dragging over them.
  - While you hold a key, it plays the steps transposed to it.
  - **Vary** changes notes at random within the scale.

It comes with 34 factory presets by category: analog brass, strings and leads, basses, harmonic organs, bells and flutes, dual filters (vowels, crossovers and a plucked string), sequences, pads and effects (see [[Presets]]).

### Glide and legato notes (the 808)

For a trap 808 that slides between notes, use Polychrome's **Bass · 808 with glide** preset, or a sample-based one in the **Sampler**: **808 · Long with glide** or **808 · Short and dirty** (in {⌘F}, search for "808", "sub" or "glide").

1. Set the synth to **Legato** and turn up **Glide** (the time it takes to reach the new note).
2. In the editor, write separate notes as usual: each one starts at its own pitch.
3. For a slide, lengthen the note until it overlaps the next one. The next one slides from it without a new attack.

| Mode | With Glide |
|---|---|
| **Poly** | Several notes at once; each slides from the previous one |
| **Mono** | One note at a time; all of them slide from the previous one, even after a silence |
| **Legato** | One note at a time; only overlapping notes slide. After a silence, or if a note starts exactly where the other ends, there's no slide |

The same goes for Wavetable, Modular and the Sampler (in the Sampler, Poly doesn't slide). Acid always slides when two notes overlap.

In the **Sampler**, the slide gradually changes the speed at which it reads the sample, with no jumps: the sample keeps playing from where it was, with no clicks.

+++ When two notes overlap
- A note overlaps another if it starts while the previous one is still held: on the keyboard, if you haven't let go of it; in a clip, if the previous one hasn't reached its end yet.
- If the previous one ends less than 2 ms later, they count as back to back and don't slide: that way two notes that touch on the grid don't get tied by a rounding error.
+++

## Modular Synth

A stack-based modular synth: you stack modules that generate and process sound, from top to bottom, and modulate them by dragging. **Editor ⤢** opens the editor.

### Building the sound

The stack is read from top to bottom, inside **groups**; each group starts from scratch. It holds up to 32 modules.

- **Generate** sound: Analog, Wavetable, Sample, Granular and Noise.
- **Process** what's above: Filter, Nonlinear Filter and Distortion.
- **Route**: Mix, Aux and **Output** (with destination Lane 1–3, Master or Sideband).

Each module has buttons to enable it, **▲ ▼** to move it, **⧉** to duplicate it, **×** to remove it and **+** to insert another below. **+ Add module** adds one at the end.

### Modulation

- **With audio**: drag a module's green **⟿** handle onto another's Phase, Harmonic, Shift, Level or Semi (for FM, ring mod…), or onto an Aux's Input.
- **With modulators** (up to 32): Envelope, LFO, Curve, Random, Audio Follower, Pitch Tracker, Note, Velocity, Pressure, Pitch Wheel, MIDI CC, Macro, Remap, limits, Scale, Sample & Hold…
- **8 macros**.

To modulate a knob with a modulator:

1. Drag the modulator's **✚** onto any knob, including other modulators' knobs.
2. In each destination's **⋯** you set the curve and the limits.
3. Right-click a modulated knob to adjust it.

### Voice and effects

- **Voice**: Poly, Mono or Legato, polyphony, pitch bend range, glide and **unison** (several modes, including chords and Shepard).
- **Effect lanes**: 3 lanes with up to 8 effects each, with Poly, Mute, Solo and a send to another lane or the Master.

## Presets

A preset saves an instrument's sound so you can use it in other projects. You manage them from the **Presets** menu in each instrument's bar and in the large editors.

### Saving your sound

1. Open **Presets**.
2. Under **MY PRESETS**, type a name.
3. Press **Save** or {Intro}. If the name already exists, it's overwritten.

**×** next to a preset deletes it.

### Using a preset

- **FACTORY**: the sounds that come with Pulse, for all its instruments except the Sampler and the Drum Pads (which play your samples). They're grouped by category (Bass, Lead, Pad, Keys…, or by style for drums): hover over one to see its sounds. Instruments with few presets show them in a single list.
- **MY PRESETS**: your own.
- In the browser (**LIBRARY › Presets**), double-clicking a preset applies it to the selected MIDI track.

They're saved in Pulse's `Presets` folder (see [[Projects and the Pulse folder]]).

> Warning: presets that use samples remember where they are. If you move the samples, the preset loses them.

> Note: plugins don't use this menu: they save their state with the project.

## VST3 and CLAP plugins

Pulse loads instruments and effects from other makers in VST3 and CLAP format, the standard audio plugin formats.

### Finding your plugins

1. In the browser, open the **PLUGINS (VST3 · CLAP)** section.
2. Press **Search for VST3 plugins**.

It searches the system plugin folders and the ones you add in **Settings › Plugins** (see [[Settings]]).

> Note: during the scan, each plugin is loaded in a separate process. If one hangs or quits while loading, Pulse doesn't quit: it tells you which one it was and leaves it out of later scans until you update or reinstall it.

### Using a plugin

- Clicking an **instrument** puts it on the selected MIDI track.
- Clicking an **effect** adds it to the end of the chain.

On the plugin's device:

- **Editor** opens the plugin's own window, floating over Pulse.
- **Parameters…** opens all its parameters in a list, with a search box.
- 6 knobs show its main parameters, automated ones first.

All its parameters can be automated (see [[Automation]]) and mapped to a MIDI controller (see [[MIDI controllers]]). Its state is saved with the project.

Plugins know the tempo, which bar the song is at, and whether it's playing or stopped. Their synced delays, LFOs and sequencers follow the song even after a jump, in a loop or when exporting a section.

If a plugin is no longer installed, its device says so and stays silent.

A track or return with a plugin is always processed on the audio core, not on the others (see [[Settings]]): some plugins only accept being called from there.

### Sidechain into a plugin

A compressor, a Trackspacer or a Kickstart with a **side input (sidechain)** can listen to another track, just like Pulse's effects:

1. On the plugin's device, open **Sidechain** and choose the track (for example, the kick).
2. Turn on the plugin's own external sidechain, if it has that button (many call it "External" or "SC").

The plugin hears it after its effects and before its fader, even if it's muted. **None** gives it silence. The selector only appears if the plugin has a side input.

+++ How it works
- It listens to that track in the same block of audio, like Pulse's effects. If that track in turn listens to this one (or is its group), it's heard 64 samples late: it's marked "(loop)".
- It's saved with the project, undone with {⌘Z}, and a duplicated track keeps listening to the same thing.
- A project with a sidechain into a plugin only opens in this version of Pulse or a newer one.
+++

### Multiple outputs: each to its own track

Drum and sampler plugins (Battery, Kontakt, a VST drum machine) can send each sound to an output. Each output can go to its own track, with its own effects, sends and fader:

1. In the plugin, send each sound to its output (in its editor).
2. Right-click the plugin track's header → **Move to its own track ▸** and choose the output. A new track appears below that plays it.

The main output stays on the plugin's track. An output that no track plays isn't heard.

+++ How it works
- As with the Drum Machine (see [[Tracks#Move a drum sound to its own track]]): the new track has no clips, follows the plugin and is muted with it.
- If the plugin delays its sound, the outputs arrive just as late and Pulse compensates them like the main one: everything plays together.
- There's room for 15 outputs besides the main one.
+++

### If a plugin crashes Pulse

Before loading a plugin, Pulse notes it on disk, and while it plays it knows which one it's in. If Pulse suddenly quits because of a plugin, when you reopen it it tells you which one it was and lets you choose:

- **Block it**: it doesn't load in any project until you unblock it in **Settings › Plugins**.
- **Just this once**: it doesn't load until you quit Pulse.
- **Load it**: as usual.

A plugin that doesn't load leaves its track silent, but stays in the project with its settings: once unblocked, it sounds as it did.

> Note: if Pulse hangs while loading a plugin and you quit it yourself, it detects that too.

### Plugins from other folders

A plugin is code that runs inside Pulse. That's why a project only loads plugins from your plugin folders: the system ones and the ones in **Settings › Plugins**.

If a project (from someone else, for example) points to a plugin in another folder, Pulse doesn't load it and tells you. If you trust it, add its folder in **Settings › Plugins** and it loads right away.

+++ System folders
| System | Folders |
|---|---|
| macOS | `/Library/Audio/Plug-Ins/VST3` and `CLAP`, and your user's ones in `~/Library` |
| Windows | `C:\Program Files\Common Files\VST3` and `CLAP`, and your user's ones in `%LOCALAPPDATA%\Programs\Common` |

Pulse for Windows is 64-bit: 32-bit plugins don't load.
+++

# Mixing

## Mixer

The mixer sets how loud each track is, where it sits between the speakers and how much it sends to the reverb and the delay. It's under each column of the [[Session]].

### Each track's controls

From top to bottom:

- **In / Auto / Off** (audio tracks only): whether you hear what comes in through the audio interface. See [[Recording]].
- **A**, **B**…: the **sends** to the return tracks, one per return and two by two. The higher, the more of that track's sound goes to the reverb, the delay or whatever the return has.
- **Pan**: panning, left or right.
- Volume **fader**, with its meter. Double-click sets it to 0 dB; hold {⇧} for finer moves.
- **Track number**: it's the activator. When off, the track is silent.
- **S** (solo): only this track plays (plus any others in solo).
- **●**: arms the track for recording.

> Tip: return tracks and the Master have their own effect chain too. Click their header and look at the **Devices** tab ([[Devices and effects]]).

### Using return tracks

A **return track** is a shared effects track: several tracks send it part of their sound (with sends A, B…) and play through the same reverb or delay. A new project comes with two, **A Reverb** and **B Delay**.

1. Turn up a track's **A** send to give it room, or **B** to give it echo.
2. For a new return, use **Create › Return Track** ({⌥⌘T}) or the **+** to the right of the returns. It appears empty, as "C Retorno".
3. Put effects on it in **Devices** and turn up the sends of the tracks you want.

To rename it or change its color: right-click its header and type in **Name**. The letter stays in front: type "Hall" and the return becomes "C Hall".

To remove one: right-click its header › **Delete return**.

> Warning: when you delete a return, the tracks stop sending it sound, and its send automation, clip envelopes and the MIDI controls and macros that moved it are lost.

+++ Details
- A project can have up to 12 returns (A to L), or none.
- When you delete one, the returns after it move up a letter, and names that start with its letter change it ("C Delay" becomes "B Delay", and so does one you've renamed: "C Hall" becomes "B Hall").
- A project with two returns still opens in earlier versions of Pulse. With more or fewer, earlier versions ask you to update.
+++

### Moving the knobs

- Drag up or down to turn it; hold {⇧} for fine adjustment.
- Double-click resets it to its default value.
- A red dot means it's automated. If the dot is hollow, you've moved it by hand and the automation is overridden (see [[Automation]]).

## Devices and effects

The **Devices** tab ({⇧Tab} in the bottom panel) shows the effect chain of the selected track, return or Master. On a MIDI track the instrument comes first and then the effects, from left to right.

### Adding and ordering effects

1. Select the track, the return or the Master.
2. In the browser, open **AUDIO EFFECTS** and click an effect: it goes at the end of the chain.
3. On each effect, the **dot** turns it on or off, **◄ ►** move it and **×** removes it.

If you click an instrument in the browser, it replaces the one on the selected MIDI track (see [[Instruments]]).

### What each effect does

| Effect | What it's for |
|---|---|
| **EQ Three** | Boosting or cutting lows, mids and highs, fast. |
| **EQ Eight** | Precise EQ: removing excess low end, a ringing frequency, adding brightness. |
| **Compressor** | Evening out loud and soft parts so the track neither gets lost nor jumps out. |
| **Bus Compressor** | Gluing a drum kit, a group or the whole mix (see [[Bus Compressor]]). |
| **Multiband (OTT)** | Squashing and brightening OTT-style: lifts the quiet and pushes down the loud in three bands (see [[Multiband (OTT)]]). |
| **Limiter** | Raising the volume without going over a ceiling; for the master. |
| **Gate** | Silencing background noise between hits or phrases. |
| **Transient** | More or less punch and more or less tail on each hit. |
| **Tuner** | Tuning a voice or an instrument to the scale (like Auto-Tune). |
| **De-esser** | Taming the s's and hiss in a voice. |
| **Voice: Pitch and Formants** | Changing a voice's pitch and, separately, its formants: deep voice, chipmunk, backing vocals (see [[Voice: Pitch and Formants]]). |
| **Vocoder** | The robot voice: your voice over a synth that sings its notes (see [[Vocoder]]). |
| **Saturator** | Warming up, dirtying or breaking up the sound. |
| **Bitcrusher** | Low-bit digital sound. |
| **Auto Filter** | A filter that sweeps in time; acid sweeps. |
| **Ducker** | Making a track duck or "pump" with the kick. |
| **Chorus** and **Phaser** | Movement and width. |
| **Delay** | Tempo-synced echo. |
| **Reverb** | Room. |
| **Space** | Convolution reverb: real rooms, plate and spring, with no metallic sound. |
| **Transition** | Tape stop and start, repeats (stutter) and cuts in time, for section changes (see [[Tape stop, repeat and cut]]). |
| **Utility** | Volume, stereo width, mono lows and phase fixes. |

#### EQ Eight

An eight-band equalizer, like Live's EQ Eight. The graph shows the curve (orange) over the live **spectrum** of what comes out of the EQ (gray): you see where the excess lows or the ringing frequency are.

1. Drag a band's **numbered dot**: left and right change the frequency; up and down, the gain. Hold {⌥} and up and down change the **Q** (how narrow it is).
2. Double-click a dot to turn the band on or off. Bands that are off look hollow.
3. The **1–8** buttons under the graph choose which band the controls on the right show: its **shape** and its **Freq**, **Gain** and **Q** dials. **Output** raises or lowers everything at the end.

The shapes:

- **Low cut** and **High cut**: remove what's below or above the frequency.
- **Low shelf** and **High shelf**: boost or cut everything below or above.
- **Bell**: boosts or cuts around the frequency.
- **Notch**: removes one very specific frequency (a 50 Hz hum, a whine).

> Tip: a new EQ Eight doesn't change the sound. Turn on band 1 (a low cut at 40 Hz) to remove lows you can't hear but that take up room.

+++ Details
- The cuts are 48 dB per octave (almost a wall) or 12 (gentle). A Q above 0.71 adds a bump at the cutoff.
- On the shelves, a Q above 0.71 marks the edge with a small bump, about 3 dB at most.
- The graph goes from −15 to +15 dB. If the curve goes higher (a cut with lots of Q, several bells together), the top shows how far it goes, for example "+25 dB".
- A new one has bands 2–7 on and flat, and the cuts (1 and 8) off. Bands at 0 dB cost nothing.
- The 25 dials can be automated and mapped to MIDI like any effect's, and the dots follow the automation.
- The spectrum is drawn for the first EQ Eight in the chain you're looking at, and starts from scratch when you change track. It listens to both channels: what only sounds in stereo shows too. A full-scale tone reads 0 dB, and the lows look just as defined at 96 or 192 kHz.
- It's not among the built-in effects of the Wavetable, the Polychrome or the Modular's lanes, which have at most six controls per effect.
- A project with an EQ Eight doesn't open in an earlier Pulse: it asks you to update.
+++

#### Compressor and Limiter

The **Compressor** turns down whatever goes over the threshold, so loud and soft parts end up closer together. Controls: **Threshold**, **Ratio**, **Attack**, **Release** and **Makeup** (gain to win back volume), and **Sidechain** (see below).

The **Limiter** raises the volume with **Gain** without letting anything go over the ceiling (**Ceiling**). **Release** is how long it takes to let go.

Both have a **GR** meter on the right: how many dB they're turning the volume down at each moment.

+++ All parameters
- Compressor: Threshold −60 to 0 dB (−18), Ratio 1:1 to 20:1 (4:1), Attack 0.1–200 ms (10), Release 5–2000 ms (120), Makeup 0–24 dB.
- Limiter: Gain 0–24 dB, Ceiling −12 to 0 dB (−0.3), Release 10–1000 ms (120).
+++

#### Gate

A **noise gate**: it only lets sound through when it goes over the threshold, and closes when it drops. It removes background noise between a voice's phrases, an amp's hum or a snare's tail.

- **Threshold**: the level at which it opens.
- **Return**: how far below the threshold it has to drop to close. That way it doesn't keep opening and closing on a sound hovering around the threshold.
- **Attack**: how long it takes to open fully (very short: the hit isn't lost).
- **Hold**: how long it stays open after the sound drops.
- **Release**: how long it takes to close fully.
- **Floor**: what it lets through when closed (−80 dB: nothing; −20 dB: lowers the background without removing it).

> Tip: with **Sidechain**, it opens when another track plays, not its own: a pad that chops in time with the hi-hats (the "trance gate"), or a bass that only plays when the kick does.

#### Transient

Changes the hit and the tail of each sound, with no threshold, whatever its volume. A sustained sound, with no hits, passes through unchanged.

- **Attack**: above 0 %, the start of each hit is louder (punchier drums); below, softer (rounds off a snare or a very spiky guitar).
- **Sustain**: above 0 %, what follows the hit lasts longer (more room, more body); below, it's shortened (tighter, drier drums).
- **Output**: the volume at the end.

#### Tuner

A vocal tuner, like Auto-Tune. It pulls each note to the nearest note in the scale, live or on what's already recorded. It works with a voice or with any instrument that plays one note at a time (a bass, a violin, whistling). Put it on an audio track.

- **Scale**: **Project's** (the one from [[Key and scale]]), **Chromatic** (the nearest note, whatever it is) or a specific scale (C major, A minor…).
- **Speed**: how long it takes to reach the note. At 0 ms it goes there instantly and every note change is a jump: the robotic effect of pop and trap. At 20–50 ms it tunes without being noticed; at 100–400 ms, it only corrects what drifts far off.
- **Humanize**: the higher it is, the more freedom it gives a note you hold (its vibrato stays), while short notes stay in tune.
- **Amount**: how much of the way it goes: at 100 % it reaches the note; at 50 %, it stops halfway.
- **Presets**, in its header, to get started: **Hard (0 ms)**, the robotic jump of trap; **Fast**, tuned with no steps (pop); **Natural**, which leaves the vibrato on long notes; and **Soft**, which only brings back what strays. They keep the scale you've chosen.

+++ How it works
- With **Project's**, if the project has no scale, it uses all notes, and changes as soon as you give it one.
- A note that isn't in the scale goes to the nearest one in the scale: an out-of-tune F# in C major sounds as G.
- It tracks from C2 (65 Hz) to C#6 (1100 Hz). Anything without pitch (breaths, s's, silence) passes through unchanged.
- It sounds slightly late, a bit more than one period of the note: about 5 ms with a mid voice (220 Hz), 10 with a low one (110 Hz) and up to 17 with a bass's lowest notes. The other tracks wait those 5 ms for it (see latency compensation in [[Recording]]).
- It's not among the synths' built-in effects, since they already play in tune.
- A project with a Tuner doesn't open in an earlier Pulse: it asks you to update.
+++

#### De-esser

Tames the s's and hiss in a voice ("s", "ch", "z") when they jump out, without dulling the rest.

- **Frequency**: where it starts listening and turning down (2–16 kHz). A low voice's s's are usually at 4–6 kHz; a high voice's, at 6–9 kHz.
- **Threshold**: the level of that high part at which it starts turning it down.
- **Range**: the most it turns it down (up to 24 dB). About 6–8 dB is enough; more, and the voice sounds lispy.

> Tip: put it after the compressor, because compressing brings up the s's.

+++ How it works
- It splits the voice into what's below and above Frequency, listens only to the high part and turns it down while it's over the threshold.
- While it isn't acting, it doesn't change the sound at all, and the voice below Frequency is never touched.
- A project with a De-esser doesn't open in an earlier Pulse: it asks you to update.
+++

#### Saturator and Bitcrusher

The **Saturator** has **Drive** (how hard it pushes), **Output** and five **types**:

- **Soft**: the classic, gentle one.
- **Hard**: hard clipping.
- **Fold**: wavefolding, bright.
- **Tube**: asymmetric and warm, like a valve.
- **Crush**: bit reduction.

The **4×** button keeps heavy drive from dirtying the highs.

The **Bitcrusher** reduces the **bits** and the sample rate (**Downsample**): the sound of old digital machines.

+++ Details
- **4×** works at four times the sample rate to avoid aliasing (the false highs that appear with heavy saturation). It uses a bit more CPU, under 1 % of a core per track, and adds 36 samples of latency.
- Saturator: Drive 0–36 dB (6), Output −36 to +6 dB (−3), Dry/Wet.
- Bitcrusher: Bits 2–16 (8), Downsample 1–40× (4), Dry/Wet.
+++

#### Auto Filter

A low-pass, high-pass or band-pass filter, with a tempo-synced LFO that moves the cutoff on its own.

- **24 dB** cuts twice as much per octave.
- **Drive** saturates what goes into the filter.
- With **Res** above 95 %, the filter rings on its own (self-oscillates) at its frequency: the whistle of acid sweeps. Its level stays bounded even if you push the cutoff all the way up or automate it in jumps.

#### Ducker

Makes the track **pump**: it drops the volume and brings it back, in time.

- Without a sidechain, it's synced to the tempo (**Rate**).
- With **Sidechain**, it pumps with every hit of the chosen track (usually the kick): it drops sharply and comes back. **Returns in** (the menu, 1/4 by default) is the time of that return; the **Release** knob is how much of that time it spends rising (100 %: all of it).
- **Threshold**: the softest hit that makes it pump. Raise it if it pumps on things that aren't the kick.
- **Lows only** (on by default): it only listens below 600 Hz, so it pumps with the kick and not with the hi-hats or claps on the same track.

+++ Details
- It comes back within Release even if the kick's tail is still ringing, so with long kicks at 140–150 BPM it still breathes between hits.
- Threshold: it pumps fully from 12 dB above the threshold.
- Turn off **Lows only** with a hardstyle kick, whose tail is as loud in the lows as the hit.
- With the whole Drum Machine as the sidechain, the low toms and congas move it a bit too. If that bothers you, move the kick to its own track (**Move to its own track ▸ Kick**) and listen to that.
+++

#### Chorus, Phaser, Delay and Reverb

- **Chorus**: doubles the sound with small variations; widens it and makes it move.
- **Phaser**: a swirling sweep, like a whirlpool.
- **Delay**: echo with a synced time (from 1/16 to one bar), **Ping Pong** (bounces from side to side), feedback (how many repeats) and filter.
- **Reverb**: room, with size, damping and width.

+++ All parameters
- Chorus: Rate 0.05–5 Hz, Depth, Delay 2–20 ms, Dry/Wet.
- Phaser: Rate 0.05–8 Hz, Depth, Freq 100–4000 Hz, Feedback, Dry/Wet.
- Delay: Feedback (up to 95 %), Filter 500–18,000 Hz, Dry/Wet.
- Reverb: Size, Damping, Width, Dry/Wet.
+++

#### Space (convolution reverb)

A reverb that sounds like a real place: Pulse computes each space's response (how a hit bounces off its walls and how it dies away), and the sound goes through it. It's more natural and smoother than the **Reverb**, which can sound metallic on long tails. It's light: about 1 % of a core, even with an 8-second tail, and nothing while no sound reaches it. It adds no latency.

1. Pick the **Space**:
   - **Ambience**: just the early reflections; air and body with no tail.
   - **Room** and **Studio**: close and natural, for drums and guitars.
   - **Hall** and **Big hall**: the long, round reverb.
   - **Cathedral**: very long, and the lows last longer.
   - **Plate**: dense from the very first instant and bright; the one for vocals and snare.
   - **Spring**: the "boing" of dub and surf.
   - **Infinite**: a tail of many seconds that fades in slowly, for pads and ambient.
2. Set **Decay** (how long the tail lasts, in seconds) and **Size** (bigger means the early reflections arrive later).
3. With **Pre-delay** you separate the voice from its reverb; with **Low Cut** and **High Cut** you remove muddy lows and harsh highs.

The display shows the early reflections (lines) and how the tail falls in the lows (blue), mids (orange) and highs (yellow): in a real space, the highs die out first.

**Presets** has settings by use: **Vocals · Bright plate**, **Drums · Room**, **Pads · Cathedral**, **Ambience · Infinite**, and the **Return Track · …** ones, fully wet, for a return.

- On a return it comes in with **Dry/Wet** at 100 %: leave it there and send more or less from each track. On a track it comes in at 30 %. **Reverb** and **Delay** do the same.
- When you change the space, **Decay** or **Size**, Pulse computes the new space and switches to it seamlessly in under half a second. That's why those three can't be automated; the rest can.
- It goes in mono and comes out in stereo: a track panned left gets its reverb spread across both sides, as in a room.

+++ All parameters
- Space: Ambience, Room, Studio, Hall, Big hall, Cathedral, Plate, Spring, Infinite.
- Decay: from 25 % to 200 % of the space's length (up to a 10 s tail).
- Size: 50–150 %.
- Pre-delay 0–250 ms, Low Cut 20–2000 Hz (20 Hz: off), High Cut 1000–20,000 Hz (20,000 Hz: off), Width, Dry/Wet.
+++

> Note: a project that uses Space doesn't open in an earlier Pulse: it asks you to update.

#### Utility

Volume (**Gain**) and stereo width (**Width**: 0 % is mono, 200 % very wide), plus two fixes:

- **Bass Mono**: below that frequency, the sound is mono (0 Hz: off). Stereo lows, in a club or on a phone, get lost or boom differently; at 100–150 Hz they settle without changing what's above.
- **Invert R**: flips the right channel. It fixes a track recorded with one channel reversed, which almost disappears in mono.

> Note: a project that uses Bass Mono or Invert R doesn't open in an earlier Pulse: it asks you to update.

### Saving and loading effect chains

A chain is a track's row of effects with their settings, in order, so you can use it on another track or in another project: your vocal chain, your drum bus chain, your master chain.

To save it:

1. Select the track, the return or the Master and open **Devices**.
2. At the top right of the panel, press **Save Chain…**.
3. Type a name, for example "My vocal chain". On a MIDI track, check **With the instrument** if you also want to save the instrument and its MIDI effects.
4. Press **Save** or {Enter}.

To load it:

- In the browser, **CHAINS** section: clicking one adds its effects **at the end** of the target's chain. Hold {⌥} to **replace** the effects that were there.
- Or search for it: type part of its name and press {Enter} (or {⌥}{Enter} to replace). See [[Browser and library#Search]].
- If the chain includes an instrument, it can only go on a MIDI track. On any other target the effects are loaded and the status bar says so.
- It's all one {⌘Z} step.

Pulse comes with six factory chains, made with its presets:

| Chain | What it's for |
|---|---|
| **Voice · Pop** | A clean, upfront vocal: EQ, compression, de-esser, a touch of OTT, short delay and plate. |
| **Voice · Radio** | A phone or old-radio voice: mids only, saturated and mono, with a limiter at the end so it never goes over −1 dB. |
| **Drum bus · Punch** | More attack and drums that sound glued together. |
| **Bass · Sidechain and saturation** | A warm bass that pumps with the beat, with mono lows. |
| **Master · Club** | Glue, brightness, mono lows and a limiter that raises the volume without going over -1 dB true peak (what streaming platforms ask for). |
| **Pad · Wide and far** | An open pad, pushed back: chorus, delay and big hall, wide without disappearing in mono. |

To delete one of your chains: right-click it › **Delete the chain**.

+++ Details
- Yours are saved as `.json` files in Pulse's folder, in `Presets/Cadenas`. You can copy them to another computer.
- The **Ducker** in "Bass · Sidechain and saturation" pumps to the tempo, in quarter notes. If you want it to follow your kick, choose the kick's track in its **Sidechain**.
- Sidechains aren't saved: they point to tracks in a project. When you save, the status bar warns you.
- Plugins are saved with their state. If the plugin isn't installed when you load the chain, it's skipped and the status bar says so.
- An effect your version of Pulse doesn't know (a chain made with a newer version) or a damaged file breaks nothing: the rest is loaded.
+++

### MIDI effects

They change the notes before they reach the instrument, on MIDI tracks.

1. Select the MIDI track.
2. In the browser, open **MIDI EFFECTS** and click one. It appears in Devices, before the instrument.
3. Like audio effects: the dot turns them on, **◄ ►** move them and **×** removes them. Up to 4 per track, in order.

- **Arpeggiator**: plays the notes you hold one by one, in time.
  - **Order**: up, down, up and down, as played, or random. **Rate**: from 1/4 to 1/32, with triplets. **Octaves** (1–4) and the **Length** of each note.
- **Chord**: turns each note into a chord.
  - **In key**: adds its third and fifth (and **seventh**) from the scale, so the chords fit the song (in C major, a D gives D–F–A, a G gives G–B–D). The scale can be the project's or one of its own.
  - Without "In key": the **Added semitones**, with shortcuts (Major, Minor, Fifth, Octave, Sus4, Seventh). **Added velocity**: how loud the new notes are.
- **Scale**: moves notes that fall outside the scale to the nearest one. With it, no note is ever out of key. It uses the project's scale or one of its own.

> Tip: they combine. **Chord + Arpeggiator** arpeggiates chords from a single note.

+++ Details
- With the transport running, the Arpeggiator follows the grid; when stopped, it starts when you play and follows the tempo.
- **Random**, with the transport running, gives each grid step the same note every time: the song sounds the same each time you play it and when you export it.
- They work with the keyboard and with clips.
- When recording, the notes you play are recorded, before the effects.
+++

### Sidechain: making one track duck under another

The **Compressor**, the **Bus Compressor**, the **Gate** and the **Ducker** can listen to another track instead of their own (and the **Vocoder** can sing with its sound, see [[Vocoder]]). The classic case: the bass or the pad ducks every time the kick hits, so they don't mask each other (and that house "pumping").

1. Put a Compressor, a Bus Compressor, a Gate or a Ducker on the track that should duck (the bass).
2. In its **Sidechain** menu, choose the track that drives it (the kick).

- It listens to the chosen track's sound **after its effects and before its fader**: it works even if that track is muted or its fader is down. You can have a kick just for triggering the sidechain.
- It can listen to a **group**: it hears the sum of its tracks after the group's effects.

### Sidechain from the kick, in one click

1. Right-click the header of the track that should pump (the bass, the pad).
2. Choose **Sidechain from the kick**.

Pulse finds the song's kick and puts a **Ducker** with club-style pumping at the end of the track's effects (before a Limiter, if there is one): it drops sharply on every kick, at 80 %, and comes back over an eighth note of the tempo. Undo it with {⌘Z}.

- **The kick** is the track that is one: a Kick Designer, a track called "Kick" or "Bombo", or a kick moved out of a drum kit. It prefers one that has clips.
- **With a Drum Machine**, it first moves its kick to its own track (as in [[Tracks#Move a drum sound to its own track]]), so the hi-hats and snare don't move it. It tells you when it does, and {⌘Z} undoes it all at once.
- **With Drum Pads or a drum loop**, which can't be split, it listens to the whole track, only its lows (below 600 Hz): almost just the kick. It tells you this too.
- If the track already ducks under that kick, it doesn't add another.
- It isn't offered on the kick's own track or on a group that contains it.

+++ Details
- It keeps working with another track in solo, in stems and when freezing.
- The source is remembered even if you move, group or rename tracks.
- It works on group tracks, returns and the Master too.
- **Loops.** If the track listens to its own group, or to a track that in turn listens to it, the source can't play before it. In the **Sidechain** menu those tracks are marked **(loop)**, and the title turns orange if you choose one. They're always heard **64 samples late** (1.3 ms at 48 kHz): the same sound with any buffer size, any number of cores and when exporting. An effect on a group that listens to one of its tracks isn't a loop: it hears it on time.
+++

## Bus Compressor

A compressor for glue: it makes a drum kit, a group of synths or the whole mix sound like one thing, like the bus compressor on an SSL console (Live's Glue Compressor). Put it on the drum **group** or on the **Master**.

### Using it on the drum bus

1. Group the drums (see [[Tracks#Group tracks]]) and select the group.
2. In the browser, **AUDIO EFFECTS** → **Bus Compressor**. Or open **Presets** in its bar and choose **Drum bus**.
3. Lower **Threshold** until the **GR** meter shows 2–4 dB on the hits.
4. Raise **Makeup** by as much as it went down, so you can compare at the same volume by turning it on and off.

> Tip: with **Attack** at 10–30 ms the start of each hit gets through before it compresses: more punch. At 0.1–1 ms it squashes everything.

### The controls

- **Ratio**: **2:1** glues without being noticed (master, groups), **4:1** is the classic for drums and **10:1** almost limits (for parallel).
- **Attack** (0.01–30 ms) and **Release** (0.1–1.2 s): how long it takes to clamp down and to let go.
- **Auto**: automatic release. It lets go fast after a single hit and slowly when it's been compressing for a while, so it doesn't pump. With it on, the **Release** knob does nothing.
- **Makeup**: wins back the volume it takes away.
- **Knee**: how gently it kicks in. At 0 dB it compresses hard as soon as it crosses the threshold; with more, it eases in gradually.
- **SC Filter**: the compressor stops hearing the lows below this frequency. At 80–150 Hz the kick no longer pulls the whole kit down on every hit. All the way down (20 Hz) it's off.
- **Dry/Wet**: blends in the uncompressed sound. At 30–50 % with heavy compression you get **parallel compression**: the body squashed and the hits intact.
- **Clip**: soft-clips the peaks at the output, below 0 dBFS.
- **Sidechain**: makes it listen to another track (see [[Devices and effects#Sidechain: making one track duck under another]]).

### Presets

| Preset | What it's for |
|---|---|
| **Drum bus** | 4:1, 10 ms attack, Auto release and filter at 90 Hz: glued drums without losing the kick. |
| **Hard parallel** | 10:1, very fast and squashed, at 40 %: body and room without touching the hits. |
| **Punch (slow attack)** | 30 ms attack: the hits jump forward. |
| **Glue the mix** | Gentle 2:1 on the master, 1–2 dB of GR. |
| **Master with clip** | Like the previous one plus **Clip**: a bit more volume. |
| **Synths together** and **Vocals** | For groups of synths or vocals. |

+++ Details
- It detects the peaks of both channels together (it doesn't shift the stereo image) and adds no latency.
- The dials can be automated and mapped to MIDI; Ratio, Auto and Clip are chosen on the device.
- Threshold −40 to 0 dB (−12), Attack 0.01–30 ms (10), Release 0.1–1.2 s (0.4), Makeup 0–20 dB, Knee 0–12 dB (3), SC Filter 20–1000 Hz (off), Dry/Wet 0–100 % (100).
- A project with a Bus Compressor doesn't open in an earlier Pulse: it asks you to update.
+++

## Multiband (OTT)

The "OTT" sound of EDM: it splits the sound into lows, mids and highs and, in each band, **pushes down the loud and lifts the quiet**. Everything ends up upfront, dense and bright: pads that breathe, leads that cut through, tails and little noises that come to light. It's Live's OTT preset and Xfer's OTT plugin.

### Using it

1. Put it on the track (a pad, a lead, a future bass bass) or on a return.
2. It comes in with **Depth** at 30 %, where people usually start. Move it between 20 and 40 %; at 100 % (the **Classic OTT** preset) it's over the top on purpose.
3. Move **Time**: shorter is more aggressive and pumps; longer, smoother.
4. Set the volume with **Output** to compare it on and off.

> Tip: on a pad, lower **Low Out** and raise **High Out** a little: brightness without more mud.

> Warning: it lifts the quiet parts by up to 18 dB plus its gains. Tails, noise and reverbs become much more audible; if it's too much, lower **Upward**.

### The controls

- **Depth**: how much it does. At 0 % it changes nothing; at 100 %, everything.
- **Time**: multiplies the attack and release times of the three bands (×0.1 to ×10).
- **Downward** and **Upward**: how much it pushes down the loud and how much it lifts the quiet, separately.
- **Input** and **Output**: volume at the input (pushes harder into compression) and at the output.
- **Dry/Wet**: blends in the unprocessed sound, in phase with the processed one.
- **Low/Mid** and **Mid/High**: where the bands split (88 Hz and 2.5 kHz to start with).
- **Low In**, **Mid In**, **High In**: how much goes into each band; more means more compression in that band.
- **Low Out**, **Mid Out**, **High Out**: each band's volume at the output.

On the right, the **L**, **M** and **H** meters show what each band is doing: upward (green) it's lifting it, downward (orange) it's pushing it down.

### Presets

| Preset | What it's for |
|---|---|
| **Classic OTT** | Everything at 100 %: the full OTT sound. |
| **Gentle OTT** | At 30 %: denser without being noticed. |
| **Downward only** | No upward: squashes the loud without lifting the noise. |
| **Shine on pads** | Less lows and more highs, slower: open pads. |
| **Cutting lead** | Fast, with the mids and highs upfront. |
| **Fat bass** | Split at 120 Hz and highs down: a dense bass that doesn't hiss. |
| **Crushed drums** | In parallel, at 60 %. |
| **Close voice** | Very little, for an upfront voice. |

+++ Details
- The bands are split with 24 dB per octave Linkwitz-Riley filters; with no compression they sum flat (the three together sound like the input). It adds no latency.
- Each band listens to its RMS level. Anything quieter than −90 dBFS isn't lifted: silence stays silent.
- The 15 dials can be automated and mapped to MIDI.
- A project with a Multiband doesn't open in an earlier Pulse: it asks you to update.
+++
## Vocoder

The robot voice of house, techno and electro: the vocoder lays your voice over a synth, which "sings" with its notes and the shape of your words. Put it on the vocal track; the sound that sings (the **carrier**) is its own oscillator, on a fixed note or playing your MIDI notes, or a synth on another track.

### Using it with its oscillator

1. On the vocal track, **AUDIO EFFECTS** → **Vocoder**. Or open **PRESETS** in its bar and choose **Classic robot**.
2. Leave **Carrier** on **Own** and choose its sound: **Saw** (the classic robot), **Super saw** (seven detuned saws per note: a choir) or **Noise** (a whispered voice, with no pitch).
3. Choose the notes: **Note** is the lowest one, and the menu below it is the chord (**One note**, **Octaves**, **Fifth**, **Major**, **Minor**, **Minor 7**).
4. To make the "s" and "t" sounds intelligible, raise **Sibilance**.

> Tip: the note and the chord can be automated: draw **Note** on the track so it changes with the song.

### Singing the MIDI notes

Its oscillator plays the notes you play or draw, like a keyboard vocoder: up to 8 at once, so chords work.

1. In **Carrier**, choose **MIDI notes**.
2. Below **Saw**, choose where the notes come from:
   - **This track**: the notes in its MIDI clips and the ones you play on it live (armed or selected for playing).
   - **Another track**: the usual choice with the vocal on an audio track. Draw the chords on a MIDI track and choose it here. If you don't want to hear its synth, mute it: its notes still arrive.
3. Sing or play the vocal: it comes out at the pitch of each note.

With no notes, the vocoder goes fully silent, "s" sounds included. Each note fades in and out with a short envelope (3 ms at the start, 25 ms at the end), so it doesn't click. **Note** and the chord do nothing; **Detune** does, with the super saw.

> Tip: the preset **MIDI notes · Chords you play** already comes with the super saw on **MIDI notes**: all that's left is choosing the chord track.

### Singing a synth's notes

1. Put a synth on another track and play or draw the chords. If you don't want to hear it on its own, mute it: the vocoder still hears it.
2. In the vocal's Vocoder, choose that track in **Carrier**.

Now the voice sings what the synth plays, note by note, and goes silent when the synth does. That way it follows a melody or a progression without you playing the vocoder.

- It listens to that track's sound after its effects and before its fader, just like a sidechain (see [[Devices and effects#Sidechain: making one track duck under another]]).
- A synth with lots of harmonics (a saw, an open pad) is more intelligible than a dark one.

### The controls

- **Bands** (8–40): how many bands it splits the sound into. Few, the old-school robot; many, a clearer voice.
- **Low** and **High**: the lowest and the highest band (100 Hz and 8 kHz by default).
- **Formant**: shifts the synth's bands against the voice's, in semitones. Up, a smaller throat; down, a monster.
- **Attack** and **Release**: how fast each band follows the voice. Short, crisper and more robotic; long, smoother and more legato.
- **Sibilance**: how much of the voice's highs (above 5 kHz) is mixed in, for the "s", "t" and "ch" sounds.
- **Note** and **Detune**: the note of its oscillator and how detuned the super saw's saws are. With a synth as the carrier they do nothing; with **MIDI notes**, **Note** doesn't either.
- **Output** and **Dry/Wet**: the output volume and how much of the unprocessed voice comes through.

### Presets

| Preset | What for |
|---|---|
| **Classic robot** | Saw and 16 bands: the classic robot voice. |
| **French house** | Super saw in octaves, fast: the French house robot. |
| **Monster** and **Goblin** | Formants down and a low note, or up and high. |
| **String choir** | Super saw on a minor chord, slow: a pad choir. |
| **Major chord** and **Minor seventh** | Chords for choruses. |
| **Whisper** | Noise: the whispered voice, with no pitch. |
| **8-band vocoder** | Few bands: the 70s vocoder. |
| **Crisp 40 bands** | Every band and very fast: almost everything is intelligible. |
| **Synth from another track** | For use with a synth as the carrier: choose the track in **Carrier**. |
| **Chords you play** | Super saw that plays your MIDI notes: choose the chord track. |

+++ Details
- Two banks of 24 dB per octave band-pass filters, spread evenly in octaves between Low and High. Each band of the voice follows its level and applies that level to the same band of the synth.
- The synth's bands are evened out first (by 40 dB at most), so a dark synth sounds as clear as a bright one. If the synth goes silent, the vocoder goes silent.
- It sounds just as loud on low and high notes: a high note has few harmonics (and nothing below it), and the vocoder makes up for it gradually (over about 150 ms, 6 dB at most).
- With no voice, silence comes out. It doesn't delay the sound.
- The own oscillator plays a fixed note or chord (with **Note**), or with **MIDI notes**, the notes that reach the track or the one you choose: the same ones its instrument would hear, after its MIDI effects, on the exact sample where they start.
- With **MIDI notes** from another track, the vocoder waits for that track, like a sidechain. If listening to it would close a loop (it's marked "(loop)"), its notes don't reach it. A frozen track no longer gives notes: it's audio.
- With **MIDI notes**, a new note when 8 are already sounding takes over the one that's been playing longest.
- Bands 8–40 (20), Low 50–1000 Hz (100), High 1–16 kHz (8), Attack 0.5–100 ms (4), Release 5–1000 ms (60), Formant ±12 st, Sibilance 0–100 % (30), Note C0–C5 (C2), Detune 0–100 % (30), Output ±24 dB, Dry/Wet 0–100 % (100).
- A project with a Vocoder doesn't open in an older Pulse: it asks you to update. With **MIDI notes**, it doesn't open either in one whose Vocoder didn't have them yet.
+++

## Voice: Pitch and Formants

Changes a voice's pitch and, separately, its formants: the size of the throat that sings. With the pitch you make a voice an octave lower or build a choir; with the formants, a chipmunk or demon voice without changing the note, or the other way round.

### Using it

1. On the vocal track, **AUDIO EFFECTS** → **Voice: Pitch and Formants**.
2. Move **Pitch** (in semitones, ±24) and, if needed, **Fine** (in cents, ±100) to fine-tune.
3. Move **Formant** (±12 semitones): up it sounds smaller, like a child or a chipmunk; down, bigger, like a giant or a demon. The pitch doesn't change.
4. To double the voice or make a choir, lower **Dry/Wet**: the original voice and the shifted one play together and in time.

> Tip: to make a voice sound natural higher or lower, move the formants a little in the same direction as the pitch (a third or half as much). Pitch alone, without the formants, sounds like the same person singing on another note.

### Presets

| Preset | What for |
|---|---|
| **Low voice** | 4 semitones down and the formants a little lower. |
| **Chipmunk** | Pitch and formants way up. |
| **Demon** | Pitch and formants way down. |
| **Octave down** and **Octave up** | The same voice an octave lower or higher. |
| **Female formant** and **Male formant** | Only the formants, 3 semitones up or down. |
| **Child** | Pitch and formants 5 semitones up. |
| **Octave chorus** | The octave below mixed with the voice. |
| **Fifth up** | A second voice a fifth above. |
| **Doubled** | The voice 10 cents higher, under the original: thicker. |

+++ Details
- It hears the voice's period every 5 ms (like the Tuner) and splits it into grains two periods long, centered on each pulse of the voice. To change the pitch, it places them closer together or further apart; to change the formants, it reads each grain faster or slower.
- It tracks voices from 70 to 1000 Hz. Where there's no pitch ("s" sounds, breaths, silence), it doesn't change the pitch: it doesn't turn noise into a buzz.
- It delays the sound by 32 ms with any setting. Pulse delays the other tracks by the same amount (see [[Recording#Latency compensation]]). If you sing live through it, you'll hear yourself with that delay.
- It works with one voice at a time: on a choir or a chord instrument it sounds odd.
- Pitch ±24 st, Fine ±100 ct, Formant ±12 st, Output ±24 dB, Dry/Wet 0–100 % (100). Everything can be automated.
- A project with this effect doesn't open in an older Pulse: it asks you to update.
+++

## Tape stop, repeat and cut

The **Transition** effect does what you hear at section changes: the tape slowing down until it stops, the snippet repeating faster and faster or lower and lower, the gap before the drop. It turns on with an automation lane and plays in time, right where it should.

### Using it

1. On the track (or the master), **AUDIO EFFECTS** → **Transition**, or search with {⌘F} for "tape stop", "stutter", "stop" or "repeat".
2. Choose the **Mode**: **Tape stop**, **Tape start**, **Repeat** or **Cut**.
3. In the Arrangement, open the track's automation and choose **Transition › Active**. Draw a step: up from where you want it to start and down where it ends.
4. While it's on, it acts; when it turns off, the usual sound comes back, right on that beat.

> Tip: the effect's **Active** button turns it on by hand, to try it while playing. To have it land the same every time, use the lane.

### The modes

| Mode | What it does |
|---|---|
| **Tape stop** | Pitch and speed drop together until stopping over the **Length** (from 1/16 to 2 bars). After that, silence until you turn it off. |
| **Tape start** | The reverse: from a standstill up to full speed over the **Length**; then it hands over to the live sound. |
| **Repeat** | Takes the **Grid** slice (1/2 to 1/64, with "T" triplets) where you turn it on, from its grid line, and repeats it on every grid step. |
| **Cut** | Silence, or with **Filter**, only what the filter lets through: **High-pass** removes the lows (the 808), **Low-pass** keeps only them. |

### The controls

- **Curve** (tape): at 0 the speed drops (or rises) evenly; turned up it brakes early; turned down it holds and drops at the end.
- **Decay** (repeat): how much quieter each repeat gets, in dB.
- **Pitch** (repeat): how much each repeat goes up or down, in semitones. At −1, each one a semitone lower: the falling roll.
- **Gate** (repeat): the part of each repeat that plays. Below 100 %, choppy.
- **Filter**, **Sweep** and **Res**: the filter on the repeats (or the cut). **Sweep** moves it on every repeat, in octaves: up it opens, down it closes.
- **Dry/Wet**: below 100 %, you also hear the original sound (on the cut, how much it cuts).

### Presets

They all come turned off: load the preset and turn it on with the lane or with **Active**.

| Preset | What for |
|---|---|
| **Tape stop · 1/4**, **1/2**, **1 bar** and **2 bars** | The tape stopping over that time: the end of a section, the brake before the drop. |
| **Tape stop · Brake** | Half a beat and braking early: the record stopped by hand. |
| **Tape start · 1/2** and **1 bar** | The return: the sound starts up from a standstill. |
| **Repeat · 1/4**, **1/8** and **1/16** | The classic stutter, on that grid. |
| **Repeat · Falling 1/16** | Each repeat a semitone lower and a bit quieter. |
| **Repeat · Rising 1/32** | A fast roll that rises in pitch: the build into the drop. |
| **Repeat · 1/8 triplet** and **Chopped 1/16 triplet** | Triplet repeats, the second one choppy (trap). |
| **Repeat · Fading echo** | Eighth notes that get quieter and darker. |
| **Repeat · Opening roll** | Thirty-second notes with no lows that open up more and more (EDM). |
| **Repeat · Stutter** | Choppy 1/64 slices, with some of the original: the stuttering voice. |
| **Cut · Before the drop** | The silence before the drop. |
| **Cut · No lows before the drop** | Removes the bass and the kick (below 300 Hz) and keeps the rest. |
| **Cut · Radio filter** | Only the mids, like a radio. |
| **Cut · Lows only** | Only what's below 250 Hz. |

> Tip: for a roll that speeds up (1/8, 1/16, 1/32…), change the **Grid** while it repeats: the following repeats use the new one. To have it land the same every time, put several Transitions in a row, each with its own grid, and turn them on one after another with their lanes.

+++ Details
- It plays 128 samples late (2.7 ms at 48 kHz), with any setting and even when off; Pulse delays the other tracks by the same amount (see [[Recording#Latency compensation]]). That way it knows in advance when the lane turns it on: a step drawn on the grid (down to 1/64) starts and ends exactly in place, and whatever comes in after a cut arrives whole, without losing the hit.
- Turning on and off, it fades over 1 ms; between repeats, over 2 ms: no clicks.
- The tape stop lowers the volume at the end, once it's below a tenth of its speed, so it doesn't end in a crackle.
- The tape start finishes behind the live sound (by the time it took to start up) and crossfades into it over 30 ms.
- When you change the **Grid** while it repeats, the following repeats get shorter; never longer than the first one.
- With the transport stopped, the **Active** button runs at the project's tempo. If you stop the song with the effect on, it turns off.
- It keeps the last 10 seconds: a very long tape stop at a very slow tempo gets shortened to fit.
- Curve ±1, Decay 0–24 dB, Pitch ±12 st, Gate 5–100 %, Filter 30 Hz–18 kHz, Sweep ±3 octaves, Res 0–90 %, Dry/Wet 0–100 % (100). **Active** turns on from 50 %. Everything can be automated.
- A project with this effect doesn't open in an older Pulse: it asks you to update.
+++

## Mix and Master Assistant

The assistant listens to your song, tells you how it sounds and what to fix before you release it, and fixes it with one button. Everything it does is ordinary volumes, panning and effects that you can tweak or remove.

### Using it

1. Open **File › Mix and Master Assistant…**.
2. Choose the **Target**: where the song is going to be heard (Spotify, club…).
3. Click **Analyze the mix**.
4. Read the **Advice** and click **Apply** on the ones you want, or **Apply all** to have it do them in order.

After each **Apply** it analyzes again on its own, so you can see the result. If the report doesn't fit on the screen, the window scrolls.

> Tip: everything the assistant does can be undone with {⌘Z}, step by step.

### What it measures

To analyze, Pulse plays the song (or the loop) silently and faster than real time, and measures the whole mix and each track on its own:

- **Loudness** (LUFS): how loud the song is perceived. It's the measure Spotify, YouTube or Apple Music use to even out the volume of every song.
- **True peak** (dBTP): the highest point of the signal, including between samples. That's where distortion starts when converting to MP3 or AAC.
- **Range** (LU): how much the volume changes between sections.
- **Loudest**: the loudest 3 seconds.
- **Stereo** (correlation): 1 is mono; below 0, part of the mix is lost when you listen in mono.

In **Tracks** you see the peak, loudness and stereo of each one.

+++ How it measures
- It measures the whole mix and each track on its own at the same time, one per processor core.
- Tracks are measured dry, without what they send to the returns: the reverb or delay tail doesn't count as a soft part of the vocal.
- Tracks are measured before the master, without its limiter or its fader: a limiter on each track on its own would raise the quiet ones and throw off the balance.
- The mix is measured after the master, and so is each section of the song.
- When analyzing again, it only replays the tracks that have changed. If only a track's fader has changed, not even that: its measurements shift by those dB.
+++

### Choosing the target

| Target | Loudness |
|---|---|
| Spotify / YouTube / Tidal | −14 LUFS |
| Apple Music | −16 LUFS |
| Club / DJ / SoundCloud | −9 LUFS |
| **Hard techno / rave** | −7 LUFS |
| Radio and TV | −23 LUFS |
| **Like the reference** | that of your reference song |

In all of them, the true peak stays below −1 dBTP.

### Comparing yourself to a reference

In **Reference**, **Choose a song…** loads a song that sounds the way you want. Pulse compares its loudness and its **balance of lows, mids and highs** with your mix's. In the octave graph, the bars are your mix and the line is the reference. **Apply** puts an **EQ Three** on the master that brings your balance closer to the reference's.

### The advice

Each piece of advice says what's going on and, if it can be fixed, comes with an **Apply** button. When something is already fine, it shows in green.

Several pieces of advice depend on each track's **role** (vocal, bass, kick, pad…). Pulse works it out from its name ("Kick", "Bass", "Vocals", "Backing vocals", "Pad", "Hats"…, in English or Spanish), from its instrument (the Kick Designer is the kick, the Acid the bass, a Drum Machine the drums) or, failing that, from how it sounds (almost all lows: a bass; almost all highs: percussion). Tracks with no clear role ("other") are left alone.

Only an audio track (or a Sampler playing a recording) sings. A MIDI track with a synth is a **lead** even if it's called "Voz…" (or "Vocal…"), like the melody that **Audio to MIDI** pulls out of a vocal ("Voz · Melodía"): it gets no vocal compressor, no de-esser and no echoes at phrase ends. "Backing vocals" on a synth are a pad.

> Tip: give your tracks clear names ("Vocals", "Bass", "Kick") and the assistant will guess better.

#### Loudness

The song sounds quieter or louder than the target asks for. **Apply** puts a **Limiter** on the master and finds its gain. It goes up to the target's loudness, but without squashing the song: if it would have to push it too hard, it stops short and tells you what to do (raise the mix before the master, compress the bus or saturate the drums and the bass). Then the advice no longer comes with a button: more limiting isn't the answer.

+++ How it works it out
- It plays the mix once up to the limiter and tries several gains at once on that recording, measuring the loudness, the true peak and each section of the song for each one.
- At most, the limiter takes **6 dB off the peaks**. If the mix arrives with headroom (its peaks below the ceiling), that headroom is added on top, because it doesn't squeeze anything.
- The **sections** stay as they were: the drop or the chorus above the break, the intro and the build. Each section can get closer to the drop by a third of how far below it was (from 0.5 to 2.5 LU), and the build stays at least 1.15 LU below. Without markers, it looks at the loudness range: it can lose a third.
- The Limiter watches the **true peak**, including between samples, and lowers the gain smoothly over 2 ms before each peak, with no steps. The ceiling sits at −1.3 dB and the mix's true peak, below −1 dBTP.
- For targets of −9 LUFS or louder (club, hard techno), it also puts a **Saturator Hard 4×** just before the Limiter, which clips 5 dB off the peaks (the kick's), and shortens the release to 40 ms. That's how the genre is mastered: the limiter alone, with a kick every 400 ms, pumps instead of raising the volume, squashes the drop against the break and lets true peaks through.
- The clipping adapts to the mix's level: if it reaches the master with its peaks below 0 dBFS, the Saturator drives it by those extra dB (and brings it back down at the output), so it really clips those 5 dB.
- If the mix is above the target: on a streaming one it warns you that the platform will turn it down; on a club one, that squeezing it harder only loses punch.
+++

#### Starting mix

Puts each track where a mix usually starts (volume, pan and lows), so you can carry on by ear from there. The advice says what it would change on each track and **Apply** does it all at once.

+++ How it works it out
- **Volume**: each role at its usual level next to the kick (or the drums, or the vocal if there are none): the vocal just as loud, the bass and the drums 2 dB below, the lead 3, the chords 5, the backing vocals 7, pads and percussion 8, effects 10. A drum track without the kick (you've moved it to its own track: hats, claps… are left) goes 6 dB below it. If there are several tracks with the same role, they share that level. It doesn't raise any above −1 dBFS or past the top of its fader. It only changes what's more than 1.5 dB off. Groups don't move: their tracks do.
- **Without burying the mids**: a track with notes or a voice (vocal, backing vocals, lead, chords, pad) is lowered by **6 dB** at most from where it was before the assistant, unless it was louder than the kick: then it can go down to its level. Whatever it holds back from lowering, it says so ("no lower").
- **Lows against mids**: before applying, it works out how the mix would sound and compares the 63 Hz octave (kick and bass) with the mids (500 Hz to 2 kHz) against a typical club mix, where the lows sit about 3 dB above. If it goes over by more than 3 dB, it undoes part of those tracks' cuts until it's back within range.
- **Pan**: the kick, the bass and the vocal, in the center. If there are two or more chord, pad, backing vocal, percussion or lead tracks in the center, it spreads them left and right. Whatever you've already panned yourself, it leaves alone.
- **Sliced pads**: a Drum Pads that comes from **Slice** on a sample with notes counts as chords, not drums: it gets no drum compressor and isn't raised like the drums.
- **Lows**: whatever doesn't need them (vocal, backing vocals, chords, pads, leads, percussion, effects) and still has something below about 60 Hz gets an **EQ Eight** with a low cut: 90 Hz on the vocal, 250 Hz on percussion, 150 Hz on the rest.
+++

#### Tracks over 0 dB

A track that goes over 0 dB clips as soon as it passes through something that can't take it. **Apply** turns its volume down.

#### Clashing lows

Two tracks fill the lows (below 120 Hz) at the same time and get muddy. If one is the drums, **Apply** gives the other one a **Ducker** sidechained to the drums, so it ducks every time they hit. Otherwise, it cuts the lows of the one with less of them with an **EQ Three**.

+++ How it works it out
- It counts as drums a Drum Machine, a Drum Rack (except the slices of a sample with notes), the **Kick Designer** (the kick on its own track) and an audio track with a drum name ("Kick", "Tops", "Perc"…).
- If both are drums and one is the Kick Designer, the kick keeps the lows and the other one's get cut.
- If one already ducks under the other with a sidechain (the bass under the kick, or a rumble), it's not a clash: it says so in green.
+++

#### Masking in the mids

Two tracks play at the same time in the same range and at a similar level, like a vocal and some chords, or a synth and a guitar: neither one comes through clearly. The advice says at which frequency, and **Apply** carves a gap there in the track that gives way, so the other one is heard without turning down either.

+++ How it works it out
- It looks for pairs playing from 250 Hz to 4 kHz within 6 dB of each other for most of the time they overlap.
- The gap is 4 dB, a narrow bell on the **EQ Eight** of the track that gives way (on a new one if it has none).
- The one that isn't the star gives way: the vocal wins over everything, then the lead, the backing vocals, the drums, the bass, the chords, the pads, the percussion and the effects. Between two with the same role, the quieter one gives way.
- The gap goes where the star is loudest among the ranges where they mask each other: its body, which is what it needs to be heard.
- It doesn't count drums against percussion (they take turns) or the kick or the bass (their place is the lows: that's what **Clashing lows** is for).
- If the track already has a gap there, it shows in green.
+++

#### Starting compression

The vocal, the backing vocals, the lead, the bass or the drums go from very soft to very loud: in the mix some parts get lost and others jump out. **Apply** gives them a **Compressor** set to how they sound, which keeps them sounding about as loud.

+++ How it works it out
- It kicks in when there are more than 8 dB between the soft and loud parts on the vocal and the backing vocals, 6 on the bass, 10 on the lead and the drums.
- It takes 5 dB off the loud parts on the vocal, 4 on the backing vocals and the bass, 3 on the lead and the drums. The threshold is worked out from the level before the fader and the pan, which is what the compressor hears.
- Ratio 3:1 on the vocal and the backing vocals, 2.5:1 on the lead and 4:1 on the bass and the drums. Fast attack on vocals (5 ms) and slow on the drums (25 ms), to let the hit through.
- Release of a sixteenth note at the song's tempo (an eighth on the bass) and gain to win back half of what it takes off.
- If the track already has a compressor or a Bus Compressor (without sidechain), it shows in green. The **Multiband (OTT)** doesn't count: it's a sound, not what holds a track's level in place. A track that ducks under another with a sidechain (a rumble or a bass pumping with the kick) isn't compressed: its level moves on purpose.
+++

#### Bus Compressor

A gentle **Bus Compressor** glues together whatever passes through it. The assistant suggests it in two places, and only if there isn't one already (or a Compressor):

- **On the drum bus**: a group with two or more tracks, all drums or percussion. It uses the "Drum bus" preset (4:1, the detector with no lows below 90 Hz) with the threshold about 12 dB below the group's peaks.
- **On the master**, for targets of −11 LUFS or louder (club, hard techno): the "Glue the mix" preset (2:1, slow attack) at the start of the chain, before the limiter, with the threshold about 8 dB below the mix's peaks. It takes 2 or 3 dB off the hits and the limiter reaches the loudness without squashing it.

#### Harsh esses

On a vocal or backing vocals, the esses ("s", "ch", "z") sound almost as loud as the vowels and pierce, especially after compression. **Apply** puts a **De-esser** set to the voice. The advice says how much it'll bring them down.

+++ How it works it out
- It kicks in when the esses are within 8 dB of the vowels.
- It sets the De-esser at the frequency of its esses: 4.5, 6 or 7.5 kHz, depending on where they are.
- It works out the threshold to bring them down about 6 dB, but never so much that it touches the vowels' brightness.
- If the track already has a De-esser, it shows in green.
+++

#### Level the vocal

The vocal changes a lot from one phrase to the next against the backing: some phrases get lost and others cover the music. **Apply** writes volume automation on its track, like someone riding the fader phrase by phrase.

+++ How it works it out
- It kicks in when some phrase is off by at least 2 dB.
- It measures each phrase: where the vocal plays, and how far it sits above the rest of the mix at that moment.
- It brings each phrase to where most of them are, by 6 dB up or down at most.
- The changes fall in the silences between phrases, so you don't hear them. It follows tempo changes.
- If the track already has volume automation, it leaves it alone (it shows in green). If the assistant later raises or lowers that track, it moves its automation too.
+++

#### Starting space

Tracks that sound dry, sending nothing to any return, all sit up front and the mix sounds flat. **Apply** sets their sends to the reverb and the delay according to their role, and creates those returns if they're missing.

+++ How it works it out
- Vocal: reverb 22 % and delay 14 %. Backing vocals: reverb 32 % and delay 8 %, so they sit behind. Lead: reverb 20 % and delay 16 %. Effects: reverb 30 % and delay 12 %.
- Pads: reverb 28 %. Chords: 18 %. Percussion: 12 %. Drums: 8 %.
- The kick and the bass, dry: if they send more than 15 % to the reverb, it warns you that it muddies the lows and removes it.
- Tracks that already send to some return stay as they are, and so do groups (their tracks do the sending).
- It uses the return that has a Reverb or a **Space**, and the one that has a Delay. If one is missing and there's room, it creates it: a **Space** in a hall, fully wet and with no lows (below 250 Hz) or extreme highs (above 9 kHz), and a dotted-eighth **Delay** in time, which gets darker on each repeat and has no lows either. If there's already a reverb, it doesn't create another one.
+++

#### Echoes at phrase ends

The "delay throws" trick: the last word of a phrase repeats as an echo and fills the silence that follows, while the sung vocal stays clean. **Apply** writes automation on the vocal's send to the delay.

+++ How it works it out
- It looks for phrases that end before a silence long enough for the echo: at least the delay time plus 0.75 s, longer than a breath.
- The send opens to 40 % on the last word of those phrases and goes back to where it was as soon as they end.
- It uses the return that has a Delay; if there isn't one, it creates it (dotted eighth in time).
- If the track already has an automated send, it leaves it alone (it shows in green).
+++

#### Sections that grow

If the song has markers, it measures each section. The chorus or the drop should sound bigger than what leads into it; otherwise, the song goes nowhere. **Apply** writes volume automation on the master that lowers the sections leading into the chorus just enough.

+++ How it works it out
- The chorus or the drop should sound above the verses, builds and pre-choruses. If they're less than 1 LU apart, it lowers those sections until they're 1.5 LU above, by 6 dB at most.
- It goes by the marker's name: "Chorus", "Drop" or "Hook" (or "Estribillo", "Coro") are what should grow; "Verse", "Build", "Pre…" or "Groove" (or "Estrofa", "Subida") are what leads into it. "Pre-estribillo" (pre-chorus) and "Subida al drop" (build to the drop) count as what leads into it.
- The intro, the break, the bridge and the outro are left alone, because they can be softer on purpose.
- It lowers those sections instead of raising the chorus: the master sits after the limiter, and raising it would clip.
- If the master already has volume automation, it leaves it alone.
+++

#### Phase

The mix or some track is out of phase: its two channels cancel each other out and, in mono (a phone, a club), it gets lost. **Apply** fixes it on each track with its **Utility** (the last one it has, or a new one).

+++ How it works it out
- One channel flipped (correlation below −0.5): it inverts the right one.
- Too wide (below −0.2): it narrows it just enough to lose little in mono (correlation 0.2).
- Stereo lows (a tenth or more of the track is lows, and its side is within 10 dB of its center): it makes them mono below 120 Hz.
+++

#### Nearly mono mix

If everything sits in the center, the mix ends up narrow. The advice shows up when, above **200 Hz**, the correlation goes over **0.9** (below that, the kick, the bass and the 808 are mono on purpose). It says what it would widen, and **Apply** does it.

+++ How it works it out
- It widens what usually goes to the sides: hats and percussion, chords or the sample, pads, leads and melodies, backing vocals and effects. If there's only one track with that role in the center, it goes to one side; the next, to the other, taking turns.
- How much: percussion 30 %, chords 35 %, pads and effects 40 %, lead 25 %, backing vocals 50 %.
- A panned mono sound is still mono: the note tracks that sound mono (chords, pads, melodies, backing vocals, effects) also get a gentle **Chorus** (35 %), which makes their two sides different. Hats and percussion are only moved.
- The kick, the bass (and the 808), the snare and the drums, and the lead vocal stay in the center.
- It doesn't touch what you've already panned or what's already wide. If there are two or more tracks with the same role, the **Starting mix** spreads them.
- The reverb and delay returns already sound wide: it doesn't move them.
+++

#### Dynamics

Warns you if the mix is very squeezed or has sections very far apart in volume. It comes with no button: it's for you to check by hand.

### Apply all

**Apply all** (next to "Advice") makes the advice's changes one after another, in the order of a mix. It measures again after each one, because each change decides the next:

1. Tracks over 0 dB.
2. Starting mix, and widening a nearly mono mix.
3. Lows.
4. Gaps in the mids: all at once, one per pair of tracks, where they mask each other most.
5. Compression, and the drum bus compressor.
6. Esses.
7. Levelling the vocal.
8. Space.
9. A volume readjustment, because cutting, ducking and compressing move them.
10. Echoes at phrase ends.
11. The master bus compressor.
12. Sections that grow.
13. The master: the EQ if there's a reference, and the target's loudness with the limiter.

When it's done it tells you what it did, and it doesn't ask you again for what it just did: what's left in the list are suggestions to apply by hand, if you want. Each step can be undone with {⌘Z}.

While it changes tracks, it only replays the ones it has changed, and the whole mix only when it needs it (for the sections and the loudness). On a 3-minute song it takes about 30 seconds.

### Listening before and after

**Listen: Before · After** appears as soon as the assistant changes something. It lets you hear the mix as it was before its first change and as it is now.

1. Hit play.
2. Switch from **Before** to **After** while it plays.
3. When you're done, click **Stop comparing**.

Both play at the same volume: the louder one is turned down to match the other. That way you compare how it sounds and not how loud, because louder always seems better.

> Note: comparing doesn't touch the project or the {⌘Z} history. **Stop comparing**, closing the window or changing anything in the mix brings it back to how it is. If the mix has changed without analyzing it again, it warns you that they aren't matched.
# Files

## Browser and library

The panel on the left: this is where you get instruments, effects, plugins and samples, and where you explore your library. Show or hide it with **☰**.

At the top it shows the **target**: the selected track, which is where whatever you pick goes.

### What's in the browser

- At the top, the **search** (see [[#Search]]).
- **FAVORITES**: what you've marked with the star (see [[#Favorites]]).
- **INSTRUMENTS**, **AUDIO EFFECTS** and **PLUGINS**: click one to put it on the track. See [[Instruments]], [[Devices and effects]] and [[VST3 and CLAP plugins]].
- **CHAINS**: saved effect chains, Pulse's and yours. See [[Devices and effects#Saving and loading effect chains]].
- **SAMPLES**: **Add sample…**.
- **LIBRARY**: the Samples, [[Pulse Sounds]], Projects, Exports and Presets folders, plus your own.
- **TRACKS**: new MIDI or audio track.

To add a folder of your own, click **Add folder…**. To remove it, right-click › **Remove from the browser**.

### Search

Press {⌘F} (or click the field at the top) and type: results appear as you type, without pressing {Enter}.

- It searches everything: instruments, MIDI and audio effects, instrument and effect presets, chains, Pulse Sounds, your samples and folders, and plugins.
- Accents and capitals don't matter, and the start of each word is enough: "bus comp" finds **Bus Compressor**, and "spa" finds **Space**.
- It also understands words from other programs: "reverb", "ott", "glue", "echo", "sidechain".
- Results are grouped by type (**AUDIO EFFECTS**, **PRESETS**, **FILES**…), with the closest match first. Next to the name it shows where it comes from: a preset's instrument, a sample's folder.

With the keyboard:

| Key | What it does |
|---|---|
| {↑} {↓} | Choose a result. If it's a sample, you hear it. |
| {Enter} | Puts the result on the target, like a double-click. |
| {⌥}{Enter} | With a chain, replaces the effects that were there. |
| {Esc} | Clears the search. Another {Esc} gives the keyboard back to Pulse. |

With the mouse: click chooses (and plays a sample), double-click puts it on the target, and dragging takes a sample, a MIDI file or a preset wherever you drop it, just like from their folders.

+++ Details
- An effect goes at the end of the target's chain; an instrument, a preset or a kit goes to the selected MIDI track.
- New samples show up in the search within a few seconds.
- To avoid slowing Pulse down, it looks at up to 50,000 files in your folders and shows up to 40 results of each type.
+++

### Favorites

Star what you always use and you'll have it at the top of the browser, under **FAVORITES**.

- Hover over a row and click the **☆** on the right, or right-click › **Add to favorites**. It works for effects, instruments, presets, chains, kits, plugins and files, in the search results too.
- To remove it: click the **★** or right-click › **Remove from favorites**.
- In **FAVORITES**, one click puts it on the target. Samples work as in their folders: click to listen and double-click to use them.
- They're saved with your Pulse settings, not in the project: you have them in all your projects.

### Listening to and using files

| File | Click | Double-click | Drag |
|---|---|---|---|
| Audio | Plays it (another click stops it) | Adds it to the selected audio track | To a slot, a track, a pad or the Sampler |
| MIDI (.mid) | — | Brings its parts in on new tracks | To a MIDI track: all its notes in one clip there |
| Project | — | Opens it | — |
| Preset (or kit) | — | Applies it to the selected track | To a MIDI track: applies it to it |
| Chain (in Presets › Chains) | — | Adds its effects to the target | To a track: adds them to it |

- {↑} {↓}: after clicking a file, they move to the previous or next one and play it. A click outside the browser gives the arrows back to the rest of Pulse.
- For .mid files, see [[MIDI files]].

### Listening to loops in tempo

With **In tempo** ticked (at the top, on by default), loops play at the project's tempo and looped, the way they'd sound in a clip.

- If the song is playing, they come in on time with it, on its bar. With the loop on, they jump back with it, at the same instant and with no click.
- Next to the checkbox it shows what tempo the loop is at.
- The first time you play a file it takes a moment to sound, while Pulse detects its tempo.
- Unticked, or with files longer than 32 s, each file plays once at its own speed.

+++ Details
It works for loops up to 32 s. Drum loops are stretched in Beats mode, as in a clip.
+++

### Dragging from the Finder

| You drop… | Where | What happens |
|---|---|---|
| An audio file | On a slot or an audio track | It goes there |
| An audio file | On a pad or the Sampler | It's loaded |
| An audio file | On an empty space | Creates an audio track |
| A `.mid` | On an empty space, or onto a MIDI track | Brings its parts in on new tracks or, onto a MIDI track, all its notes in one clip (see [[MIDI files]]) |
| A `.pulse` | Anywhere | It opens |
| An `.als` | Anywhere | It's imported (see [[Importing from Ableton Live]]) |

- Several audio files go one after another. In Session, if they don't fit in the 16 slots, they continue on a new track.
- A **folder** brings in the audio files it contains, including those in its subfolders, sorted by name.
- The status bar says how many were added and which couldn't be read.
- Before opening a `.pulse` or importing an `.als`, Pulse asks if there are unsaved changes.
- Opening with a double-click in the Finder or dropping on the Dock icon also works.

## Pulse Sounds

A sample library that comes with Pulse: drum kits by genre, kicks, snares, hats, percussion, 808s, basses, chords and effects. Pulse synthesized all of it with its own instruments and effects: there are no recordings of anyone, and you can use it in your songs without limits.

### Opening it

1. In the browser, open **LIBRARY › Pulse Sounds**.
2. The first time, Pulse creates the sounds: a bar shows how many it has done. It takes a few seconds and doesn't stop anything; you can keep working.
3. Done: click to listen and drag them to a track, a pad or the Sampler, like any sample (see [[Browser and library#Listening to and using files]]).

The library search finds them too: try "techno kick", "808" or "riser".

### What's in it

| Folder | What it has |
|---|---|
| Kicks | One per style: house, saturated techno and techno with rumble, trap, drum and bass, lo-fi, garage, afro, 80s, minimal, boom bap and electro |
| Snares and claps | Snares, claps, rimshots, a snap and the 80s snare with gated reverb |
| Hats and cymbals | Closed and open hats for each style, crash and ride |
| Percussion | Congas, bongo, cowbells, clave, woodblock, shakers, tambourine, toms, a metallic percussion, a blip and a drop |
| 808s and subs | Long, saturated, short and punchy 808s, and two subs, all in C |
| Basses | Bass hits in C: acid, reese, FM, organ, techno, saw, slap and bouncy |
| Stabs and chords | Chords with the note in the name: house and organ stabs, dub chord, rave, fifths, electric piano, lo-fi, brass and 80s |
| FX | 4- and 8-bar risers (at 120 BPM), noise rise, downlifter, impact, reverse crash, noise sweep, sub drop, laser, zap, wind and vinyl noise |
| Loops | Each kit's beat, two bars long, with the tempo in the name |
| Kits | The ten kits, to load into the Drum Pads |

- The sounds in each folder are at a similar level, peaking below −0.3 dB: you can swap one for another without rebalancing the mix.
- Pitched sounds carry their note in the name, tuned to within 10 cents: "Long 808 (C)" is a C. The 808s and basses are at C1, and the chords at C3: in the [[Instruments#Sampler|Sampler]], set **Root** to that note and each key will play its own pitch.
- Loops play in tempo with **In tempo** (see [[Browser and library#Listening to loops in tempo]]): their tempo is in the name, like "House 124".

### Loading a kit into the Drum Pads

Each kit fills the 16 pads in the usual order: kick on C1, rimshot, snare, clap, a second snare, toms, closed and open hat (the closed one cuts the open one), shaker, percussion, crash, an effect or chord, and ride.

- In the **Drum Pads**, open **Kits** and pick one. Hover over it to read how it sounds.
- Or in the browser, under **Pulse Sounds › Kits**: double-click loads the kit on the selected track, and dragging it onto a MIDI track puts it on that track.

| Kit | How it sounds |
|---|---|
| House | Round, punchy kick, long clap with room, offbeat open hat, shaker, congas and an organ stab |
| Techno | Saturated kick with rumble, metallic hats, plate clap, distorted rimshot and cowbell |
| Trap | Short kick and long 808 in C, dry snare and clap, snap and crisp hats for rolls |
| Drum & Bass | Dry kick, tight cracking snare, soft snare for ghost notes, short hats and ride |
| Lo-Fi | Everything muffled, saturated and lower-resolution, plus a worn piano chord and vinyl noise |
| UK Garage | Short bouncy kick, crisp rimshot and snare, fast hats, shaker and tambourine |
| Afro House | Warm kick, congas, bongo, shaker, clave, cowbell and toms |
| Synthwave | Snare and clap with 80s gated reverb, big toms and an 80s chord |
| Minimal | Clicky kick, tiny rimshots and hats, woodblock, blips and drops |
| Hip Hop | Boom bap: hard-hitting kick, fat snare with room, muffled hats and an electric piano chord |

> Tip: each kit's loop, in **Loops**, is that same kit playing its beat. Listen to it to choose a kit before loading it.

+++ Where they are
Pulse creates them in its data folder (`~/Library/Application Support/Pulse/Sonidos de Pulse` on macOS, `%LOCALAPPDATA%\Pulse\Sonidos de Pulse` on Windows), as 24-bit 48 kHz WAV: about 47 MB. When a song uses one and you save it, it's copied to the project folder, like any audio.

If a new version of Pulse brings changed sounds, it creates them in another folder, alongside: your songs keep sounding with the old ones.
+++

## Projects and the Pulse folder

How to save and open your songs, start from a template, and where each file lives. Pulse makes copies on its own and lets you recover your work if something goes wrong.

### Saving and opening

Almost everything is in the **File** menu:

| Shortcut | What it does |
|---|---|
| {⌘N} | New project (or from the template chosen in Settings) |
| {⌘O} | Open |
| {⌘S} | Save |
| {⇧⌘S} | Save As |
| {⌥⌘S} | Versions (see [[Project versions]]) |
| {⌘Z} / {⇧⌘Z} | Undo / redo (**Edit** menu) |

- **Open Recent**: the last 10 projects.
- **Revert to Saved**: discards the changes since the last save.
- **New from Template ▸** and **Save as Template…**: see below.
- **Show Pulse Folder** opens it in the Finder (in File Explorer on Windows).
- The window title has a * if there are unsaved changes. Changes you make inside a plugin count too: in its editor, with a controller or in **Parámetros…** (Parameters…).
- Before you lose unsaved changes, Pulse asks: **Save**, **Don't Save** or **Cancel**.

> Tip: you can undo while playing. Only what you undo changes: the other tracks stay as they were, with their reverb and delay tails and their plugins. The same goes for adding a track.

When you save, all the audio the project uses is copied to its folder; audio recorded in an unsaved project is moved. {⌘Z} after saving undoes your last change, and the clips keep pointing to their audio in the project folder.

### The Pulse folder

All your work lives in the **Pulse folder**: by default Music/Pulse (`~/Music/Pulse` on macOS; your user's Music folder on Windows). You change it in [[Settings]].

| Folder | What it holds |
|---|---|
| `Proyectos/<nombre>/` | Each project, with its audio in `Samples/` and its versions in `Versiones/` |
| `Plantillas/<nombre>/` | Your templates, each with its audio |
| `Exportaciones` | What you export |
| `Presets`, `Wavetables`, `Samples` | Your own |
| `Sin guardar` | What an unsaved project records or creates; when you save it, it moves to its folder |

- Inside each project's `Samples/`: Importados, Grabaciones, Stems, Trozos, Consolidados and Congelados (imported, recordings, stems, slices, consolidated and frozen).
- A project's folder can be copied whole to another computer, also from macOS to Windows and back.

> Warning: if some audio is missing when you open a project (it was outside its folder or on another computer), Pulse says so in the status bar and keeps its path. Put the file back in place or import it again.

+++ Settings, logs and crash reports
Settings and what Pulse generates aren't your work and aren't in the Pulse folder:
- macOS: `~/Library/Application Support/Pulse`.
- Windows: settings in `%APPDATA%\Pulse`, and kits and models in `%LOCALAPPDATA%\Pulse`.

If Pulse closes because of a bug, it writes what happened to `pulse.log` (`~/Library/Logs/Pulse` on macOS, `%LOCALAPPDATA%\Pulse\Logs` on Windows), with the version, the system and the commit. If it quits suddenly without writing it (a crashing plugin, for example), on macOS there's a report in **Console › Crash Reports** (`~/Library/Logs/DiagnosticReports/pulse-*.ips`).
+++

### Templates

A **template** is a project to start from. It saves your tracks, instruments, effects, Drum Pads and Sampler samples, mix, sends, returns, master, tempo, key, scenes, macros and MIDI mappings. It doesn't save the content: no clips, arrangement, automation or markers.

To start from one: **File › New from Template ▸**, or **File › What Would You Like to Do?…**, where each one tells you what it includes when you hover over it. Pulse comes with ten, and yours go below a line:

| Template | Tempo | What it includes |
|---|---|---|
| **Beat** | 124 | 909 drums with compressor, acid bass, chords and melody, with sends to the reverb and the delay |
| **Record Vocals** | 90 | A backing track for the instrumental and two audio tracks, Vocals and Backing vocals, with EQ, compressor and a send to the reverb |
| **House** | 124 | House kit, bass, organ and pad pumping on every beat, pluck, a room and a dotted delay |
| **Techno** | 130 | Techno kit, rolling bass, acid, minor 7 stab and dark atmosphere, with a big room |
| **Trap** | 140 | Trap kit, a Kick Designer 808 that follows the notes (with saturation), bells, pad and a vocal |
| **Reggaeton** | 95 | Drum Machine with the dembow, sub bass, marimba, lead and a vocal |
| **Pop** | 110 | 80s drums, FM bass, piano, wide pad, vocals and backing vocals |
| **Lo-Fi** | 85 | Lo-Fi kit with some bitcrusher and swing, lo-fi piano, warm bass, texture and tape on the master (gentle saturation and highs) |
| **Drum & Bass** | 174 | Drum & Bass kit, reese without lows, sub and pad |
| **Podcast** | — | Two voices with gate, EQ, compressor and de-esser, and a music track that ducks when the first one speaks; mono master, louder |

In the genre templates, the faders leave headroom and the master ends in a limiter: whatever you write plays without clipping. The kits are the ones from [[Pulse Sounds]]: the first time you pick a template with a kit, its sounds get ready in a moment.

To create one:

1. Set up the project the way you want to start your songs.
2. **File › Save as Template…** and give it a name.

- The samples it uses are copied to the template's folder, so it keeps working even if you delete or move the project. Your project doesn't change.
- With the name of one that already exists, it replaces it, samples included (copies don't pile up). With the name of one of Pulse's, yours comes first.
- A project started from a template is unsaved: {⌘S} asks where and takes its samples to its folder.

To make {⌘N} always start from a template, choose it in **Settings › Projects › New project (⌘N)**. **Empty** is two MIDI tracks, as always. If that template can't be opened (damaged, deleted or from a newer Pulse), {⌘N} opens an empty project and tells you at the bottom.

> Note: to delete or rename a template, delete or rename its folder in `Plantillas` (with **Show Pulse Folder**). When renaming, also rename the `.pulse` inside, which has the same name as the folder.

### Autosave and recovery

Every 30 seconds, if there are unsaved changes, Pulse saves a separate copy. Your `.pulse` file isn't touched until you save.

If Pulse closes without saving or asking (a crash, the computer shuts down), when you open it again it asks if you want to recover the changes:

- **Recover**: brings the project back as it was at the last autosave, marked as unsaved. Save it with {⌘S} to keep it.
- **Discard**: sets it aside as `descartado.pulse` in the same folder, just in case, until next time.

Recordings are protected too: while you record audio, what comes in is written to disk as it goes. If Pulse closes halfway, the take keeps what was recorded up to that moment.

+++ Details
- The copies go to the `Recuperación` folder in the settings: `~/Library/Application Support/Pulse/Recuperación` on macOS, `%LOCALAPPDATA%\Pulse\Recuperación` on Windows.
- If the copy can't be opened (for example, a newer version of Pulse made it), it's set aside as "autoguardado.pulse (dañado)" (damaged) and the next autosave doesn't overwrite it.
- The recording in progress is written to `Samples/Grabaciones` as "Grabación sin terminar…" (unfinished recording). When you stop, it becomes the usual take and that copy is deleted. If Pulse closes halfway, it stays there, and the recovery prompt mentions it. If the take can't be written when you stop (disk full), that copy isn't deleted either and the status bar says where it is.
- If Pulse closed while reopening the last project at startup, next time it doesn't reopen it on its own, because that project may be what's closing it. Open it from **Open Recent** whenever you want.
- If a track's sound stops being valid (NaN, infinity or an absurd level, above +60 dBFS, due to a damaged file, a crashing plugin or a filter that blows up), Pulse mutes it on the spot, clears its effects and says so in the status bar. Everything else keeps playing.
- Saving is all or nothing: Pulse first writes a complete copy and only then replaces the file. A crash, a full disk or a power cut halfway through saving leaves the previous file intact.
- If the project had any invalid number (NaN or infinity, for example from a damaged Live set), it's saved as 0 and the status bar says how many.
- If a settings or list file has something Pulse doesn't understand (a value from a newer version, a damaged entry), only that is lost: the rest is kept and a copy of the original is left as "… (dañado)" (damaged). If nothing can be read, Pulse sets it aside under that name instead of overwriting it.
+++

## Project versions

Pulse keeps your song's history: try ideas without fear and go back to any point. Open the window with **File › Versions…** ({⌥⌘S}).

### Saving a version

1. Type a name, for example "With the new bass", and a note if you like.
2. Click **Save version**. The project's current state is saved.

Named versions stay forever.

Also, every time you save with {⌘S}, an **automatic** version is made, if something changed since the previous one. The last 20 are kept.

### Seeing what changed

- Each version says **when** it was made and **what changed** from the previous one: "Tempo 120 → 124", "+ Track "Voz"" (a track called Voz was added), ""Bass": 1 → 2 clips in the Session, mix"…
- **Compare with now**: what has changed from that version to the project as it is now.

### Going back to a version

**Go back to this one** opens that version in place of what's there. Save to keep it.

> Tip: nothing is lost. What was there is first saved as a named version ("Antes de volver a…", before going back to…), which automatic versions never push out, and {⌘Z} undoes it.

- **Rename**: giving an automatic version a name makes it stay forever.
- **Delete**: asks for confirmation.

> Note: versions live in the project folder (`Versiones/`) and use the same audio. The project has to be saved in its folder: if it's a loose `.pulse` from before, use **Save As…**.

## Importing from Ableton Live

Open an Ableton Live set (`.als`) in Pulse with its tracks, clips and mix. Use {⌘I} (**File › Import Live Set (.als)…**) or drag the `.als` into the window.

> Warning: it replaces the current project. The `.als` isn't touched.

### What gets imported

- Tempo, time signature (and its changes), loop and scenes.
- MIDI and audio tracks, with name, color, volume, pan, sends, activator and solo.
- Session and Arrangement clips, with their notes or samples and their warp.
- **Groups**, as Pulse groups, with their tracks inside and their mixer.
- **Locators**, as markers.
- Returns, with name and volume. The first two keep Pulse's reverb and delay; the rest, up to 12, arrive empty.
- Drum Racks with samples become Pulse's Drum Pads; Simpler and Sampler become the Sampler.

### What doesn't get imported

- Live's synths: those tracks get Pulse Synth.
- Effects and automation.

Samples are looked up by their path and, if the set comes from another computer, by their name in the set's folder.

## MIDI files

Pulse opens and saves standard MIDI files (**.mid**). Bring in parts from another program (a progression, a drum part, a whole song) or take yours out.

### Bringing in a MIDI file

Use **File › Import MIDI…**, or drag the .mid from the Finder or from Pulse's browser.

- **On an empty space, or with Import**: each part of the file goes to a new MIDI track. A part is each channel of each track in the file. In the Arrangement they go at the start; in the Session, in the first slot.
- **Onto a MIDI track** (an empty slot or its lane, at the time where you drop): all the notes go in a single clip, right there.

How they end up:

- New tracks are named as in the file. Without a name, they're named after their General MIDI sound ("Bass", "Piano", "Strings"…).
- Channel 10 is drums and goes to a drum machine.
- Pianos, electric pianos and organs (by their General MIDI sound) come in with [[Pulse Keys]] and a similar preset. The other parts play with the synth: swap it for any instrument you like.
- The file's sustain pedal lengthens the notes it holds, just like when playing them.
- Each clip lasts until the bar where its last note ends.
- If the project is empty, it takes the file's tempo and time signature (with their changes). If not, it plays at the project's tempo and time signature, and tells you which ones the file had.

> Note: tempo changes inside the file aren't brought in.

### Taking the MIDI out

- **File › Export MIDI…** saves the song: each MIDI track with clips in the Arrangement is a track in the file, with its name, at the project's tempo and with its bars (and their changes).
- Right-click a MIDI clip › **Export MIDI…** saves just that clip.

+++ Details
- Drums go on channel 10 so other programs recognize them.
- A clip that lasts longer in the Arrangement than itself repeats as it plays. Deactivated clips aren't included.
+++

## Exporting audio

Turn your song into an audio file to listen to, share or release, or into one track per file (stems) to mix it elsewhere. Open the dialog with {⇧⌘R} (**File › Export Audio…**).

> Tip: before exporting, go through the [[Mix and Master Assistant]].

### Exporting the song

1. Press {⇧⌘R}.
2. In **What**, leave **Full mix**.
3. In **Range**, choose **Whole song**.
4. Choose the **Format** and click **Export…**.

By default it's saved in the `Exportaciones` folder. The dialog remembers what you chose last time (format, range, stems, normalization…), across sessions too.

The **Arrangement** is exported, not the Session, with the tail of reverbs and delays. The export sounds exactly like what you hear, and Pulse warns you if the mix clips.

### What to export

- **Full mix**: the song in one file.
- **One track per file (stems)**: each track as it sounds soloed, with its sends and its group.
  - Deactivated tracks (or those in a deactivated group) aren't exported, nor are groups as a separate stem: their tracks already go through them.
  - If you have a track soloed, only the tracks that sound with that solo are exported, as in the mix.
  - **With the master chain**: untick it so the summed stems give the mix. If a group or a return compresses or saturates, summed they come close but don't match exactly, and the dialog warns you.

### Which part

- **Whole song**.
- **Loop region**.
- **By section**: one file for each part of the song, from one marker to the next (the last one, to the end), with the marker's number and name: "Tema - 1 Intro.wav", "Tema - 2 Drop.wav"… Anything before the first marker comes out as "Inicio" (Start). With stems, one batch per section: "Tema - 2 Drop - 1 Drums.wav".

### Exporting versions

If the project has named versions (see [[Project versions]]), tick under **Versions** the ones you also want to export. Each one comes out as it was (its tempo, its tracks, its mix) and with its name at the end of the file: "Tema - With the new bass.wav". **Only the versions (not the current state)** leaves out the current state.

### Format

| Format | What for | Options |
|---|---|---|
| **WAV** | Maximum quality, lossless | 16 bits (with dither), 24 or 32 bits (float) |
| **FLAC** | Lossless, like WAV, and about half the size | 16 or 24 bits |
| **MP3** | Listening and sharing | 192, 256 or 320 kbps |

- All three, at **44.1** or **48 kHz**.
- **Normalize**: brings the peak to a value. Not available for stems.

> Note: an MP3 comes out normalized, at most to −1 dBFS: decoding it raises it slightly, and at 0 it would clip.

### Files that already exist

- If, when exporting stems, sections or versions, there are already files with those names from a previous export, Pulse asks before replacing them.
- Two versions with the same name come out in different files ("… - Mezcla.wav", "… - Mezcla 2.wav").
- Files appear complete or not at all. If you cancel or it fails (disk full), nothing is left half-written.

+++ How it works
- **Dither**: in 16-bit WAV it's different in each file, so the noise of the stems doesn't add up in phase.
- **Gapless MP3**: it carries the information to play back without gaps. It starts and ends on the same sample as the WAV, without the silence the encoder adds, so it fits in a session or a loop.
- **Stems in parallel**: they're rendered at the same time, one per processor core, and each one only with what can sound in it (its track, its groups and the tracks its sidechains listen to). 16 stems take little more than the song once. They sound exactly the same as soloed.
- **Plugins**: if the project has plugins, stems are rendered one at a time, and their copies are closed on Pulse's main thread, as VST3 plugins require (some crash otherwise).
- **Sections**: each section starts with what was already sounding (including a long note that started many bars earlier) and ends with its tail. It starts and ends exactly on its markers, even if the tempo is automated. A track that doesn't play in a section gets no stem for it, instead of a silent file; at the end, the message says how many were skipped.
- **Tails**: it waits as long as the slowest delay in the project before considering the tail finished, so the echoes of a long delay fit too.
- **Start**: it begins at full volume from the first sample. When exporting the **Loop region**, it plays from a few bars earlier, so what was already sounding (long notes, tails) is there from the start.
- **Same as what you hear**: sample for sample, at any buffer size. Automation is applied every 64 samples, at the same points, and the LFOs of the Auto Filter and the Phaser move at the same instants. The Polychrome, with a buffer that isn't a multiple of 64, may move its modulation a few samples earlier or later: it's inaudible.
- **Safe writing**: while exporting, files are written hidden alongside and only take their names when all of them are done.
+++

# Pulse

## Settings

Where you choose the language, the sound card, MIDI devices, plugin folders and how Pulse starts. Open them with {⌘,} (**Pulse › Settings…**); changes apply immediately.

### Language

**Idioma · Language** chooses between Spanish and English. By default it's **Same as the system**: Spanish if your system is in Spanish, English if it's in any other language. The change shows immediately, in the menu bar too.

The whole interface is in English: menus, windows, editors, devices, messages and Pulse's own presets, kits and sounds, and the names of scales and chords, which in English use letters ("D minor", "Dm", "Bb/D"). The names Pulse gives what it creates (the tracks, clips, markers and returns of its templates, the example song, Auto-Arrange, Accompany, Audio to MIDI and transitions) are in the interface's language when they're created: "Bass", "Vocals", "Chorus". After that, what you save doesn't change with the language: a project, its chords and the names of its tracks and clips (yours, and the ones Pulse gave them) open the same in Spanish and in English, so a project made in English keeps "Bass" in Spanish, and one made in Spanish keeps "Bajo". The example song's title and lyrics are always Spanish, as its voice sings them. This guide is shown in the interface's language (English or Spanish).

### Audio

- **Output** and **Input (audio recording)** (the input is the one used for recording).
- **Sample rate** and **Buffer size**: smaller responds sooner; if you hear dropouts, raise it.
- **Cores for audio**: how many cores Pulse spreads the work across (see below). **Automatic** is usually best.
- **Measured latencies**, including that of the **effects**.
- **Recording offset** and **Compensate for effect latency** (see [[Recording]]).

When the project gets heavy, Pulse spreads the tracks and returns across several processor cores (up to half of the ones it has, and at most six). That way many more tracks, instruments and effects fit without dropouts, and it sounds exactly the same as on a single core (that's how it exports): what you hear is what you export. With little to process, it does everything on one and lets the others rest. It starts with the tracks that cost the most, so a heavy one isn't left running alone at the end. By default it uses up to half the cores, and at most six; in **Cores for audio** you can choose another number. On macOS, the helper cores work in step with the sound card, like the audio one, so they hold up without dropouts even when other programs keep the computer busy.

### MIDI

The connected devices, and a button to look for them again.

Below, **Grid controllers**: the Launchpads and APCs in use with the area of the Session they control (and arrows to move it), and the other ports, to learn one as a grid. See [[Grid controllers]].

### Plugins

The folders where Pulse looks for plugins, and **Find plugins** (see [[VST3 and CLAP plugins]]).

Below, the plugins **blocked** because they closed Pulse: **Unblock** loads them again (see [[VST3 and CLAP plugins#If a plugin crashes Pulse]]).

### Pulse folder

Where it is, **Change…** (what's already there doesn't move) and **Show in Finder** (see [[Projects and the Pulse folder]]).

### Projects

- Open the last project at startup.
- Show "What would you like to do?" when starting without a project (see [[Getting started#What would you like to do?]]).
- Which template {⌘N} starts from (see [[Projects and the Pulse folder]]).
- **New MIDI clips**: how many bars a MIDI clip is when you create it by double-clicking or with **New MIDI clip here** (1, 2, 4 or 8; four by default).
- Check for updates at startup.

## Updates

Pulse looks for new versions on its own and installs them with one click, keeping the previous one in case you want to go back.

### Installing a new version

1. Pulse checks at startup and every 6 hours. You can also check with **Pulse › Check for Updates…**.
2. If there's one, the window shows what's new. Choose **Download and install**, **Later** or **Skip this version**.
3. When it's done, click **Restart now**. If there are unsaved changes, it asks first.

The first time a new version starts, it shows what's new.

### Going back to the previous version

The previous version is kept as "Pulse (anterior)" (previous). **Go back to the previous version** restores it.

> Warning: if you open a project saved with a newer version in an older one, the status bar says so and, before saving over it, Pulse asks: that version doesn't know the new features and they would be lost. **Save As…** saves a copy without touching the original. Settings it doesn't know are kept.

### If something isn't working

- If Pulse can't replace itself where it is, it says so before downloading anything. This happens if you open it without moving it out of Downloads (macOS runs it from a temporary copy: move it to Applications) or if it's in a folder your user can't write to.
- If something fails while switching versions, everything stays as it was, including the saved previous version.
- Only versions that have a package for your system are announced.

+++ GitHub access (optional)
You don't need a GitHub account: the repository is public. **GitHub access (optional)** is only useful if GitHub says there are too many requests (it has an hourly limit). It uses the `gh auth login` session or a token pasted there, which is stored in the Keychain on macOS and in Credential Manager on Windows. If the token expires, Pulse carries on without it.
+++

+++ On Windows, and security checks
- On Windows, `pulse.exe` is replaced, and the previous one stays as "pulse (anterior).exe" in the installation folder.
- Before installing, Pulse checks that the download is the published one (its SHA-256 fingerprint) and that it's the announced version.
+++

## About Pulse

**Pulse › About Pulse** shows the version and the license. Pulse is free software (GPL-3.0 or later): you can use it, study it, modify it and share it.

- **License (GPL-3.0)** opens the full license text.
- **Third-party components** opens the licenses of what Pulse includes: the libraries, ONNX Runtime, the AI models and the fonts.
- The code is on GitHub (the link in the window).

> Note: if you distribute a modified version of Pulse, share it with its source code too, under the same license.

## Keyboard shortcuts

All of Pulse's shortcuts on one page. **Help › Keyboard Shortcuts** shows this list in a window, and the shortcuts with {⌘} are also in the menu bar.

### Project and files

| Shortcut | What it does |
|---|---|
| {⌘N} | New project |
| {⌘O} | Open |
| {⌘S} / {⇧⌘S} | Save / Save As |
| {⌥⌘S} | Project versions |
| {⌘I} | Import Live set |
| {⇧⌘R} | Export audio |
| {⌘,} | Settings (audio, MIDI, plugins) |
| {⌘Q} | Quit |

### Editing

| Shortcut | What it does |
|---|---|
| {⌘Z} / {⇧⌘Z} | Undo / Redo |
| {⌘X} / {⌘C} / {⌘V} | Cut / Copy / Paste the selected clips (or the selected notes in the editor) |
| {⌘D} | Duplicate |
| {⌘E} / {⌘J} | Split the clip where you clicked / join clips (Arrangement) |
| {0} | Deactivate or activate the selected clips |
| {⌫} | Delete |

### Tracks and selection

| Shortcut | What it does |
|---|---|
| {⌘T} / {⇧⌘T} | New MIDI / audio track |
| {⌥⌘T} | New return |
| {⌘G} | Group the selected tracks |
| {⇧⌘G} | Ungroup the selected group |
| {⌘ clic} / {⇧ clic} | Select several tracks (on their headers) or several clips |
| Drag a header | Move the selected tracks |
| Drag on an empty space | Select clips and tracks with a box |
| Drag a clip / {⌥} | Move the selected clips / copy them |

### Views and transport

| Shortcut | What it does |
|---|---|
| {Tab} | Switch between Session and Arrangement |
| {⇧Tab} | Switch between the clip and the devices |
| {⇧⌘E} | Maximize the clip editor or restore its height (also double-click the panel's edge) |
| {⌃⌘F} | Enter or leave full screen |
| {Espacio} | Play / Stop |
| {F9} | Record into the Arrangement |
| {⇧⌘C} | Capture MIDI: a clip with what you just played |

### Clip editor

| Shortcut | What it does |
|---|---|
| {⌘A} | Select all the clip's notes |
| {⌘U} | Quantize notes |
| {B} | Pencil |
| {R} | Roll the selected notes, like the last one |
| {← →} / {↑ ↓} | Move / transpose notes ({⇧}: bar or octave) |
| {Esc} | Deselect notes |
| {⌥ + rueda} over the notes | Row height |

### Computer keyboard

| Shortcut | What it does |
|---|---|
| {M} | Computer keyboard as MIDI keyboard |
| {A S D F G H J K L} | White keys |
| {W E T Y U O P} | Black keys |
| {Z} / {X} | Octave down / up |
| {C} / {V} | Velocity down / up |

More in [[Computer keyboard]].
