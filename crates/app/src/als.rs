//! Import of Ableton Live sets (`.als`): gzip-compressed XML.
//!
//! Brings in what Pulse can represent: tempo, loop, scene names, MIDI tracks with
//! their mixer settings, session clips and arrangement clips with their notes.
//! Live's devices are proprietary, so tracks get Pulse's default instrument
//! (or the drum kit when the track holds a Drum Rack). Audio and group tracks
//! are skipped and counted in the report. The parser is lenient: anything
//! missing falls back to a default instead of failing the import.

use crate::project::{Mixer, Project, Track, PALETTE};
use engine::model::{ArrangementClip, InstrumentKind, MidiClip, Note, SynthParams};
use engine::{MAX_TRACKS, NUM_RETURNS, NUM_SCENES};
use roxmltree::{Document, Node};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub midi_tracks: usize,
    pub session_clips: usize,
    pub arrangement_clips: usize,
    pub skipped_audio_tracks: usize,
    pub skipped_group_tracks: usize,
    /// MIDI tracks beyond Pulse's track limit.
    pub dropped_tracks: usize,
    /// Session clips in scenes beyond Pulse's scene count.
    pub dropped_clips: usize,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Importadas {} pistas MIDI, {} clips de sesión y {} del Arrangement",
            self.midi_tracks, self.session_clips, self.arrangement_clips
        );
        let mut skipped = Vec::new();
        if self.skipped_audio_tracks > 0 {
            skipped.push(format!("{} pistas de audio", self.skipped_audio_tracks));
        }
        if self.skipped_group_tracks > 0 {
            skipped.push(format!("{} grupos", self.skipped_group_tracks));
        }
        if self.dropped_tracks > 0 {
            skipped.push(format!("{} pistas MIDI por encima del máximo de {MAX_TRACKS}", self.dropped_tracks));
        }
        if self.dropped_clips > 0 {
            skipped.push(format!("{} clips más allá de la escena {NUM_SCENES}", self.dropped_clips));
        }
        if !skipped.is_empty() {
            text.push_str(&format!(" · omitidos: {}", skipped.join(", ")));
        }
        text
    }
}

pub fn import(path: &Path) -> Result<(Project, ImportReport), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("No se pudo leer: {e}"))?;
    let xml = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .read_to_string(&mut text)
            .map_err(|e| format!("No se pudo descomprimir el set: {e}"))?;
        text
    } else {
        String::from_utf8(bytes).map_err(|_| "El archivo no es un set de Ableton Live".to_string())?
    };
    parse(&xml)
}

pub fn parse(xml: &str) -> Result<(Project, ImportReport), String> {
    let doc = Document::parse(xml).map_err(|e| format!("XML no válido: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Ableton" {
        return Err("El archivo no es un set de Ableton Live".into());
    }
    let set = child(root, "LiveSet").ok_or("El set no contiene LiveSet")?;
    let mut report = ImportReport::default();
    let mut project = Project::empty();
    project.tracks.clear();

    let master = child(set, "MasterTrack").or_else(|| child(set, "MainTrack"));
    if let Some(tempo) = master.and_then(|m| number(m, &["DeviceChain", "Mixer", "Tempo", "Manual"])) {
        project.tempo = tempo.clamp(20.0, 999.0);
    }
    if let Some(gain) = master.and_then(|m| number(m, &["DeviceChain", "Mixer", "Volume", "Manual"])) {
        project.master.mixer.gain = gain as f32;
    }

    if let Some(transport) = child(set, "Transport") {
        project.loop_enabled = flag(transport, &["LoopOn"]).unwrap_or(false);
        let start = number(transport, &["LoopStart"]).unwrap_or(0.0).max(0.0);
        let length = number(transport, &["LoopLength"]).unwrap_or(16.0).max(1.0);
        project.loop_start = start;
        project.loop_end = start + length;
    }

    project.scene_names = scene_names(set);

    let tracks = child(set, "Tracks").map(|t| t.children().filter(Node::is_element).collect::<Vec<_>>());
    for node in tracks.unwrap_or_default() {
        match node.tag_name().name() {
            "MidiTrack" => {
                if project.tracks.len() >= MAX_TRACKS {
                    report.dropped_tracks += 1;
                    continue;
                }
                let track = midi_track(node, project.tracks.len(), &mut report);
                project.tracks.push(track);
                report.midi_tracks += 1;
            }
            "AudioTrack" => report.skipped_audio_tracks += 1,
            "GroupTrack" => report.skipped_group_tracks += 1,
            "ReturnTrack" => {}
            _ => {}
        }
    }

    // Pulse's returns keep their own effects; only the names and levels come across.
    let returns: Vec<Node> = child(set, "Tracks")
        .map(|t| t.children().filter(|n| n.has_tag_name("ReturnTrack")).collect())
        .unwrap_or_default();
    for (ret, node) in project.returns.iter_mut().zip(returns.iter().take(NUM_RETURNS)) {
        if let Some(name) = track_name(*node) {
            ret.name = name;
        }
        ret.mixer = mixer(*node);
    }

    if project.tracks.is_empty() {
        return Err(format!(
            "El set no tiene pistas MIDI que importar ({} pistas de audio)",
            report.skipped_audio_tracks
        ));
    }
    Ok((project, report))
}

// ----- XML helpers ------------------------------------------------------------------

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(name))
}

fn path<'a, 'i>(node: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    names.iter().try_fold(node, |n, name| child(n, name))
}

/// The `Value` attribute of the element at `names`.
fn value<'a>(node: Node<'a, '_>, names: &[&str]) -> Option<&'a str> {
    path(node, names)?.attribute("Value")
}

fn number(node: Node, names: &[&str]) -> Option<f64> {
    value(node, names)?.parse().ok()
}

fn flag(node: Node, names: &[&str]) -> Option<bool> {
    value(node, names).map(|v| v == "true")
}

fn attribute_number(node: Node, name: &str) -> Option<f64> {
    node.attribute(name)?.parse().ok()
}

// ----- Live structures ---------------------------------------------------------------

/// Live has a 70-entry color palette; Pulse folds it onto its own.
fn color(node: Node, fallback: usize) -> [u8; 3] {
    let index = number(node, &["Color"])
        .or_else(|| number(node, &["ColorIndex"]))
        .map_or(fallback, |c| c.max(0.0) as usize);
    PALETTE[index % PALETTE.len()]
}

fn track_name(track: Node) -> Option<String> {
    let name = path(track, &["Name"])?;
    [value(name, &["UserName"]), value(name, &["EffectiveName"])]
        .into_iter()
        .flatten()
        .find(|n| !n.is_empty())
        .map(str::to_string)
}

fn mixer(track: Node) -> Mixer {
    let Some(m) = path(track, &["DeviceChain", "Mixer"]) else {
        return Mixer::default();
    };
    Mixer {
        gain: number(m, &["Volume", "Manual"]).map_or(Mixer::default().gain, |v| v as f32),
        pan: number(m, &["Pan", "Manual"]).unwrap_or(0.0).clamp(-1.0, 1.0) as f32,
        active: flag(m, &["Speaker", "Manual"]).unwrap_or(true),
    }
}

fn scene_names(set: Node) -> Vec<String> {
    // Live 11+: <Scenes><Scene><Name Value=".."/></Scene>; Live 10: <SceneNames><Scene Value=".."/>.
    let names: Vec<String> = if let Some(scenes) = child(set, "Scenes") {
        scenes
            .children()
            .filter(|n| n.has_tag_name("Scene"))
            .map(|s| value(s, &["Name"]).unwrap_or_default().to_string())
            .collect()
    } else if let Some(scenes) = child(set, "SceneNames") {
        scenes
            .children()
            .filter(|n| n.has_tag_name("Scene"))
            .map(|s| s.attribute("Value").unwrap_or_default().to_string())
            .collect()
    } else {
        Vec::new()
    };
    (0..NUM_SCENES)
        .map(|i| {
            names
                .get(i)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| (i + 1).to_string())
        })
        .collect()
}

fn midi_track(node: Node, index: usize, report: &mut ImportReport) -> Track {
    let name = track_name(node).unwrap_or_else(|| format!("{} MIDI", index + 1));
    let drums = node.descendants().any(|n| n.has_tag_name("DrumGroupDevice"));
    let instrument = if drums {
        InstrumentKind::Drums
    } else {
        InstrumentKind::Synth(SynthParams::default())
    };
    let mut track = Track::new(name, color(node, index * 5), instrument);
    track.mixer = mixer(node);
    if let Some(sends) = path(node, &["DeviceChain", "Mixer", "Sends"]) {
        let holders = sends.children().filter(|n| n.has_tag_name("TrackSendHolder"));
        for (send, holder) in track.sends.iter_mut().zip(holders) {
            // Live stores sends as linear gain with a -70 dB floor.
            let amount = number(holder, &["Send", "Manual"]).unwrap_or(0.0) as f32;
            *send = if amount <= 0.000_32 { 0.0 } else { amount.clamp(0.0, 1.0) };
        }
    }
    track.solo = flag(node, &["DeviceChain", "Mixer", "SoloSink"]).unwrap_or(false);

    let sequencer = path(node, &["DeviceChain", "MainSequencer"]);
    if let Some(slots) = sequencer.and_then(|s| child(s, "ClipSlotList")) {
        let slots = slots.children().filter(|n| n.has_tag_name("ClipSlot"));
        for (slot, outer) in slots.enumerate() {
            let Some(clip) = path(outer, &["ClipSlot", "Value", "MidiClip"]) else { continue };
            if slot >= NUM_SCENES {
                report.dropped_clips += 1;
                continue;
            }
            track.clips[slot] = Some(session_clip(clip, track.color));
            report.session_clips += 1;
        }
    }
    let arranged = sequencer.and_then(|s| path(s, &["ClipTimeable", "ArrangerAutomation", "Events"]));
    for clip in arranged.into_iter().flat_map(|e| e.children().filter(|n| n.has_tag_name("MidiClip"))) {
        if let Some(placed) = arrangement_clip(clip, track.color) {
            track.arrangement.push(placed);
            report.arrangement_clips += 1;
        }
    }
    track
}

/// The clip's content window in clip time: (start, length).
fn content_window(clip: Node, fallback_length: f64) -> (f64, f64) {
    let loop_start = number(clip, &["Loop", "LoopStart"]).unwrap_or(0.0);
    let loop_end = number(clip, &["Loop", "LoopEnd"]).unwrap_or(loop_start + fallback_length);
    let looping = flag(clip, &["Loop", "LoopOn"]).unwrap_or(true);
    let length = if looping {
        loop_end - loop_start
    } else {
        // An unlooped clip plays from its start to the end marker (or its visible length).
        number(clip, &["Loop", "OutMarker"]).map_or(fallback_length, |out| out - loop_start)
    };
    (loop_start, if length > 0.0 { length } else { fallback_length.max(4.0) })
}

fn notes(clip: Node, window_start: f64, length: f64) -> Vec<Note> {
    let mut notes = Vec::new();
    let Some(tracks) = path(clip, &["Notes", "KeyTracks"]) else { return notes };
    for key_track in tracks.children().filter(|n| n.has_tag_name("KeyTrack")) {
        let Some(pitch) = number(key_track, &["MidiKey"]) else { continue };
        let Some(events) = child(key_track, "Notes") else { continue };
        for event in events.children().filter(|n| n.has_tag_name("MidiNoteEvent")) {
            if event.attribute("IsEnabled") == Some("false") {
                continue;
            }
            let (Some(time), Some(duration)) = (attribute_number(event, "Time"), attribute_number(event, "Duration")) else {
                continue;
            };
            let start = time - window_start;
            if start < 0.0 || start >= length {
                continue;
            }
            let velocity = attribute_number(event, "Velocity").unwrap_or(100.0);
            notes.push(Note {
                start,
                length: duration.min(length - start).max(0.01),
                pitch: pitch.clamp(0.0, 127.0) as u8,
                velocity: (velocity / 127.0).clamp(0.0, 1.0) as f32,
            });
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

fn clip_name(clip: Node) -> String {
    value(clip, &["Name"]).filter(|n| !n.is_empty()).unwrap_or("MIDI").to_string()
}

fn session_clip(clip: Node, track_color: [u8; 3]) -> MidiClip {
    let visible = match (number(clip, &["CurrentStart"]), number(clip, &["CurrentEnd"])) {
        (Some(start), Some(end)) if end > start => end - start,
        _ => 4.0,
    };
    let (start, length) = content_window(clip, visible);
    MidiClip {
        name: clip_name(clip),
        length,
        notes: notes(clip, start, length),
        color: if child(clip, "Color").is_some() || child(clip, "ColorIndex").is_some() {
            color(clip, 0)
        } else {
            track_color
        },
    }
}

fn arrangement_clip(clip: Node, track_color: [u8; 3]) -> Option<ArrangementClip> {
    let start = number(clip, &["CurrentStart"]).or_else(|| attribute_number(clip, "Time"))?;
    let end = number(clip, &["CurrentEnd"])?;
    if end <= start {
        return None;
    }
    let mut content = session_clip(clip, track_color);
    let (window_start, length) = content_window(clip, end - start);
    content.length = length;
    content.notes = notes(clip, window_start, length);
    Some(ArrangementClip {
        start: start.max(0.0),
        length: end - start,
        clip: content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A trimmed-down set in the layout Live 11 writes.
    const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5" MinorVersion="11.0_433" Creator="Ableton Live 11.3">
  <LiveSet>
    <Tracks>
      <MidiTrack Id="8">
        <Name><EffectiveName Value="Drums"/><UserName Value=""/></Name>
        <Color Value="1"/>
        <DeviceChain>
          <Mixer>
            <Sends>
              <TrackSendHolder Id="0"><Send><Manual Value="0.5"/></Send></TrackSendHolder>
              <TrackSendHolder Id="1"><Send><Manual Value="0.0003162277571"/></Send></TrackSendHolder>
            </Sends>
            <Speaker><Manual Value="true"/></Speaker>
            <Volume><Manual Value="0.5011872"/></Volume>
            <Pan><Manual Value="-0.25"/></Pan>
          </Mixer>
          <MainSequencer>
            <ClipSlotList>
              <ClipSlot Id="0"><ClipSlot><Value>
                <MidiClip Id="0" Time="0">
                  <CurrentStart Value="0"/><CurrentEnd Value="4"/>
                  <Loop><LoopStart Value="0"/><LoopEnd Value="4"/><LoopOn Value="true"/></Loop>
                  <Name Value="Beat"/>
                  <Notes><KeyTracks>
                    <KeyTrack Id="0"><Notes>
                      <MidiNoteEvent Time="0" Duration="0.25" Velocity="127" IsEnabled="true"/>
                      <MidiNoteEvent Time="2" Duration="0.25" Velocity="100" IsEnabled="true"/>
                      <MidiNoteEvent Time="3" Duration="0.25" Velocity="100" IsEnabled="false"/>
                    </Notes><MidiKey Value="36"/></KeyTrack>
                    <KeyTrack Id="1"><Notes>
                      <MidiNoteEvent Time="1" Duration="0.25" Velocity="90"/>
                    </Notes><MidiKey Value="38"/></KeyTrack>
                  </KeyTracks></Notes>
                </MidiClip>
              </Value></ClipSlot></ClipSlot>
              <ClipSlot Id="1"><ClipSlot><Value/></ClipSlot></ClipSlot>
            </ClipSlotList>
            <ClipTimeable><ArrangerAutomation><Events>
              <MidiClip Id="1" Time="8">
                <CurrentStart Value="8"/><CurrentEnd Value="24"/>
                <Loop><LoopStart Value="4"/><LoopEnd Value="8"/><LoopOn Value="true"/></Loop>
                <Name Value=""/>
                <Notes><KeyTracks>
                  <KeyTrack Id="0"><Notes>
                    <MidiNoteEvent Time="4.5" Duration="1" Velocity="64"/>
                    <MidiNoteEvent Time="2" Duration="1" Velocity="64"/>
                  </Notes><MidiKey Value="60"/></KeyTrack>
                </KeyTracks></Notes>
              </MidiClip>
            </Events></ArrangerAutomation></ClipTimeable>
          </MainSequencer>
          <DeviceChain><Devices><DrumGroupDevice Id="3"/></Devices></DeviceChain>
        </DeviceChain>
      </MidiTrack>
      <AudioTrack Id="9"><Name><EffectiveName Value="Vocals"/></Name></AudioTrack>
      <ReturnTrack Id="2">
        <Name><EffectiveName Value="A-Reverb"/><UserName Value="Hall"/></Name>
        <DeviceChain><Mixer><Volume><Manual Value="0.25"/></Volume></Mixer></DeviceChain>
      </ReturnTrack>
    </Tracks>
    <MasterTrack>
      <DeviceChain><Mixer>
        <Tempo><Manual Value="128"/></Tempo>
        <Volume><Manual Value="1"/></Volume>
      </Mixer></DeviceChain>
    </MasterTrack>
    <Transport><LoopOn Value="true"/><LoopStart Value="8"/><LoopLength Value="16"/></Transport>
    <Scenes>
      <Scene Id="0"><Name Value="Intro"/></Scene>
      <Scene Id="1"><Name Value=""/></Scene>
    </Scenes>
  </LiveSet>
</Ableton>"#;

    #[test]
    fn imports_tempo_loop_scenes_and_mixer() {
        let (project, report) = parse(SET).unwrap();
        assert_eq!(project.tempo, 128.0);
        assert!(project.loop_enabled);
        assert_eq!((project.loop_start, project.loop_end), (8.0, 24.0));
        assert_eq!(project.scene_names[0], "Intro");
        assert_eq!(project.scene_names[1], "2");
        assert_eq!(project.returns[0].name, "Hall");

        let drums = &project.tracks[0];
        assert_eq!(drums.name, "Drums");
        assert_eq!(drums.instrument, InstrumentKind::Drums);
        assert!((drums.mixer.gain - 0.501).abs() < 0.001);
        assert_eq!(drums.mixer.pan, -0.25);
        assert_eq!(drums.sends, [0.5, 0.0]);
        assert_eq!(report.skipped_audio_tracks, 1);
        assert_eq!(report.midi_tracks, 1);
    }

    #[test]
    fn imports_session_clip_notes() {
        let (project, report) = parse(SET).unwrap();
        let clip = project.tracks[0].clips[0].as_ref().unwrap();
        assert_eq!(clip.name, "Beat");
        assert_eq!(clip.length, 4.0);
        // The disabled note is skipped; notes are sorted by time.
        let starts: Vec<(f64, u8)> = clip.notes.iter().map(|n| (n.start, n.pitch)).collect();
        assert_eq!(starts, vec![(0.0, 36), (1.0, 38), (2.0, 36)]);
        assert_eq!(clip.notes[0].velocity, 1.0);
        assert!(project.tracks[0].clips[1].is_none());
        assert_eq!(report.session_clips, 1);
    }

    #[test]
    fn imports_arrangement_clips_relative_to_their_loop() {
        let (project, report) = parse(SET).unwrap();
        let placed = &project.tracks[0].arrangement[0];
        assert_eq!((placed.start, placed.length), (8.0, 16.0));
        assert_eq!(placed.clip.length, 4.0);
        // Only the note inside the loop window [4, 8) survives, shifted to clip time.
        assert_eq!(placed.clip.notes.len(), 1);
        assert_eq!(placed.clip.notes[0].start, 0.5);
        assert_eq!(placed.clip.name, "MIDI");
        assert_eq!(report.arrangement_clips, 1);
    }

    #[test]
    fn reads_gzip_compressed_sets() {
        let dir = std::env::temp_dir().join(format!("pulse-als-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("set.als");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(SET.as_bytes()).unwrap();
        std::fs::write(&path, encoder.finish().unwrap()).unwrap();
        let (project, _) = import(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(project.tempo, 128.0);
    }

    #[test]
    fn older_scene_names_and_color_index_are_understood() {
        let set = r#"<Ableton><LiveSet>
            <Tracks><MidiTrack><ColorIndex Value="3"/></MidiTrack></Tracks>
            <SceneNames><Scene Id="0" Value="Verse"/></SceneNames>
        </LiveSet></Ableton>"#;
        let (project, _) = parse(set).unwrap();
        assert_eq!(project.scene_names[0], "Verse");
        assert_eq!(project.tracks[0].color, PALETTE[3]);
        assert_eq!(project.tracks[0].name, "1 MIDI");
    }

    #[test]
    fn rejects_sets_without_midi_and_non_live_files() {
        let audio_only = r#"<Ableton><LiveSet><Tracks><AudioTrack/></Tracks></LiveSet></Ableton>"#;
        assert!(parse(audio_only).unwrap_err().contains("1 pistas de audio"));
        assert!(parse("<html/>").is_err());
    }
}
