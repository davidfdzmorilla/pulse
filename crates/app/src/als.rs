//! Import of Ableton Live sets (`.als`): gzip-compressed XML.
//!
//! Brings in what Pulse can represent: tempo, loop, scene names, MIDI and audio
//! tracks with their mixer settings, session clips and arrangement clips (notes,
//! or samples with their warp settings). Live's synths are proprietary, so MIDI
//! tracks get Pulse's default instrument; the samples of Drum Racks and of
//! Simpler/Sampler devices are loaded into Pulse's Drum Rack and Sampler (a Drum
//! Rack without samples becomes the drum machine). Group tracks become groups,
//! with their tracks inside (and their mixer settings). The parser is
//! lenient: anything missing falls back to a default instead of failing.

use crate::project::{Mixer, Project, Track, PALETTE};
use engine::model::{
    rack_pad, ArrangementClip, AudioClip, Clip, InstrumentKind, MidiClip, Note, RackParams, SamplerParams, SynthParams, WarpMarker, WarpMode, RACK_FIRST_NOTE,
};
use engine::{MAX_TRACKS, MAX_RETURNS, NUM_SCENES};
use engine::meter::{Meter, TimeSig};
use roxmltree::{Document, Node};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub midi_tracks: usize,
    pub audio_tracks: usize,
    pub session_clips: usize,
    pub arrangement_clips: usize,
    pub group_tracks: usize,
    /// Tracks beyond Pulse's track limit.
    pub dropped_tracks: usize,
    /// Session clips in scenes beyond Pulse's scene count.
    pub dropped_clips: usize,
    /// Audio clips whose file could not be found on this computer.
    pub missing_samples: usize,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Importadas {} pistas MIDI y {} de audio, {} clips de sesión y {} del Arrangement",
            self.midi_tracks, self.audio_tracks, self.session_clips, self.arrangement_clips
        );
        let mut notes = Vec::new();
        if self.missing_samples > 0 {
            notes.push(format!("{} muestras no encontradas", self.missing_samples));
        }
        if self.group_tracks > 0 {
            notes.push(format!("{} grupo(s) con sus pistas", self.group_tracks));
        }
        if self.dropped_tracks > 0 {
            notes.push(format!("{} pistas omitidas por encima del máximo de {MAX_TRACKS}", self.dropped_tracks));
        }
        if self.dropped_clips > 0 {
            notes.push(format!("{} clips omitidos más allá de la escena {NUM_SCENES}", self.dropped_clips));
        }
        if !notes.is_empty() {
            text.push_str(&format!(" · {}", notes.join(", ")));
        }
        text
    }
}

/// The most a set's XML may take unpacked (real ones are a few MB).
const MAX_XML: u64 = 256 << 20;

pub fn import(path: &Path) -> Result<(Project, ImportReport), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("No se pudo leer: {e}"))?;
    let xml = if bytes.starts_with(&[0x1f, 0x8b]) {
        // Up to a limit: a few KB can unpack to gigabytes (and fill the memory).
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(MAX_XML + 1)
            .read_to_string(&mut text)
            .map_err(|e| format!("No se pudo descomprimir el set: {e}"))?;
        if text.len() as u64 > MAX_XML {
            return Err(format!("El set descomprimido pasa de {} MB: no parece un set de Live", MAX_XML >> 20));
        }
        text
    } else {
        String::from_utf8(bytes).map_err(|_| "El archivo no es un set de Ableton Live".to_string())?
    };
    parse_in(&xml, path.parent())
}

#[cfg(test)]
pub fn parse(xml: &str) -> Result<(Project, ImportReport), String> {
    parse_in(xml, None)
}

/// What clip parsing needs to know about the set.
struct Context<'a> {
    /// Folder of the `.als`, for resolving relative sample paths.
    base: Option<&'a Path>,
    tempo: f64,
}

/// Parses a set; `base` is the folder it lives in, used to find its samples.
pub fn parse_in(xml: &str, base: Option<&Path>) -> Result<(Project, ImportReport), String> {
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
    if let Some(meter) = master.and_then(meter_of) {
        project.meter = meter;
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
    project.name_scenes();
    // Live's locators: the markers.
    if let Some(locators) = path(set, &["Locators", "Locators"]) {
        for locator in locators.children().filter(|n| n.has_tag_name("Locator")) {
            let Some(beat) = number(locator, &["Time"]) else { continue };
            let name = value(locator, &["Name"]).map_or_else(|| format!("Marcador {}", project.markers.len() + 1), str::to_string);
            project.markers.push(crate::project::SongMarker { beat: beat.max(0.0), name });
        }
        project.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }

    let ctx = Context { base, tempo: project.tempo };
    let tracks = child(set, "Tracks").map(|t| t.children().filter(Node::is_element).collect::<Vec<_>>());
    // Each group's depth by its id: a track's TrackGroupId says which it is in
    // (Live keeps a group's tracks right after it, as Pulse does).
    let mut group_depths: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    for node in tracks.unwrap_or_default() {
        let depth = value(node, &["TrackGroupId"]).and_then(|id| group_depths.get(id)).map_or(0, |d| d + 1);
        match node.tag_name().name() {
            "MidiTrack" | "AudioTrack" => {
                if project.tracks.len() >= MAX_TRACKS {
                    report.dropped_tracks += 1;
                    continue;
                }
                let mut track = track(node, project.tracks.len(), &ctx, &mut report);
                if track.is_audio() {
                    report.audio_tracks += 1;
                } else {
                    report.midi_tracks += 1;
                }
                track.depth = depth;
                project.tracks.push(track);
            }
            "GroupTrack" => {
                if project.tracks.len() >= MAX_TRACKS {
                    report.dropped_tracks += 1;
                    continue;
                }
                let index = project.tracks.len();
                let mut group = Track::new_group(track_name(node).unwrap_or_else(|| format!("Grupo {}", report.group_tracks + 1)), color(node, index));
                group.mixer = mixer(node);
                group.depth = depth;
                if let Some(id) = node.attribute("Id") {
                    group_depths.insert(id.to_string(), depth);
                }
                report.group_tracks += 1;
                project.tracks.push(group);
            }
            _ => {}
        }
    }
    crate::groups::normalize(&mut project.tracks);

    // Pulse's returns keep their own effects; only the names and levels come across.
    let returns: Vec<Node> = child(set, "Tracks")
        .map(|t| t.children().filter(|n| n.has_tag_name("ReturnTrack")).collect())
        .unwrap_or_default();
    // Beyond them, as many empty ones as the set has (up to the most there can be).
    for (i, node) in returns.iter().take(MAX_RETURNS).enumerate() {
        if project.returns.len() <= i {
            project.returns.push(crate::returns::new_return(i));
        }
        let ret = &mut project.returns[i];
        if let Some(name) = track_name(*node) {
            ret.name = name;
        }
        ret.mixer = mixer(*node);
    }

    if project.tracks.iter().all(|t| t.is_group()) {
        return Err("El set no tiene pistas MIDI ni de audio que importar".into());
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

/// A number from the set; «NaN» or «inf» (which Rust reads as numbers) are not.
fn number(node: Node, names: &[&str]) -> Option<f64> {
    value(node, names)?.parse().ok().filter(|n: &f64| n.is_finite())
}

/// A time signature as Live saves it in a number: (beats − 1) + 99 × log2(note).
fn live_sig(value: f64) -> Option<TimeSig> {
    let v = value.round() as i64;
    (v >= 0).then(|| TimeSig::new((v % 99 + 1) as u32, 1u32 << (v / 99).min(5)))
}

/// The set's time signature, and its changes along the song (the master's
/// automation of it).
fn meter_of(master: Node) -> Option<Meter> {
    let node = path(master, &["DeviceChain", "Mixer", "TimeSignature"])?;
    let older = || {
        let sig = path(node, &["TimeSignatures", "RemoteableTimeSignature"])?;
        Some(TimeSig::new(number(sig, &["Numerator"])? as u32, number(sig, &["Denominator"])? as u32))
    };
    let mut meter = Meter::new(number(node, &["Manual"]).and_then(live_sig).or_else(older)?);
    let Some(id) = path(node, &["AutomationTarget"]).and_then(|t| t.attribute("Id")) else { return Some(meter) };
    let Some(envelopes) = path(master, &["AutomationEnvelopes", "Envelopes"]) else { return Some(meter) };
    let Some(envelope) = envelopes.children().filter(|n| n.has_tag_name("AutomationEnvelope")).find(|e| value(*e, &["EnvelopeTarget", "PointeeId"]) == Some(id)) else {
        return Some(meter);
    };
    let Some(events) = path(envelope, &["Automation", "Events"]) else { return Some(meter) };
    let mut changes: Vec<(f64, TimeSig)> =
        events.children().filter(|n| n.has_tag_name("EnumEvent")).filter_map(|e| Some((attribute_number(e, "Time")?, live_sig(attribute_number(e, "Value")?)?))).collect();
    changes.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (time, sig) in changes {
        // Live's first event sits far before the song: it is the signature at the start.
        let bar = if time <= 0.0 { 0 } else { meter.bar_at(meter.next_bar(time - 1e-6)).0.max(0) as u32 };
        meter = meter.with_change(bar, sig);
    }
    Some(meter)
}

fn flag(node: Node, names: &[&str]) -> Option<bool> {
    value(node, names).map(|v| v == "true")
}

fn attribute_number(node: Node, name: &str) -> Option<f64> {
    node.attribute(name)?.parse().ok().filter(|n: &f64| n.is_finite())
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

fn track(node: Node, index: usize, ctx: &Context, report: &mut ImportReport) -> Track {
    let audio = node.has_tag_name("AudioTrack");
    let kind = if audio { "Audio" } else { "MIDI" };
    let name = track_name(node).unwrap_or_else(|| format!("{} {kind}", index + 1));
    let color = color(node, index * 5);
    let mut track = if audio {
        Track::new_audio(name, color)
    } else {
        let (instrument, samples) = instrument(node, ctx, report);
        Track { samples, ..Track::new(name, color, instrument) }
    };
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
            let Some(value) = path(outer, &["ClipSlot", "Value"]) else { continue };
            let clip = if let Some(midi) = child(value, "MidiClip") {
                Clip::Midi(session_clip(midi, color))
            } else if let Some(audio) = child(value, "AudioClip") {
                Clip::Audio(audio_clip(audio, color, None, ctx, report))
            } else {
                continue;
            };
            if slot >= NUM_SCENES {
                report.dropped_clips += 1;
                continue;
            }
            track.clips[slot] = Some(clip);
            report.session_clips += 1;
        }
    }
    let arranged = sequencer.and_then(|s| path(s, &["ClipTimeable", "ArrangerAutomation", "Events"]));
    for clip in arranged.into_iter().flat_map(|e| e.children().filter(Node::is_element)) {
        let placed = match clip.tag_name().name() {
            "MidiClip" => arrangement_clip(clip, color),
            "AudioClip" => arrangement_span(clip).map(|(start, end)| ArrangementClip {
                start,
                length: end - start,
                clip: Clip::Audio(audio_clip(clip, color, Some(end - start), ctx, report)),
            }),
            _ => None,
        };
        if let Some(placed) = placed {
            track.arrangement.push(placed);
            report.arrangement_clips += 1;
        }
    }
    track
}

/// The first sample zone inside a Simpler or Sampler.
fn first_part<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.descendants().find(|n| n.has_tag_name("MultiSamplePart") && child(*n, "SampleRef").is_some())
}

/// The instrument for a MIDI track and the files it plays, from Live's devices.
fn instrument(track: Node, ctx: &Context, report: &mut ImportReport) -> (InstrumentKind, Vec<String>) {
    let mut found = |part: Node| -> String {
        let path = resolve_sample(part, ctx.base);
        if !path.is_file() {
            report.missing_samples += 1;
        }
        path.to_string_lossy().into_owned()
    };
    if let Some(rack) = track.descendants().find(|n| n.has_tag_name("DrumGroupDevice")) {
        let mut samples = Vec::new();
        let branches = rack.descendants().filter(|n| n.has_tag_name("DrumBranch"));
        for (index, branch) in branches.enumerate() {
            // Live stores the pad note mirrored: 128 - MIDI note.
            let note = number(branch, &["BranchInfo", "ReceivingNote"]).map(|n| 128.0 - n);
            let pad = match note {
                Some(note) if (0.0..128.0).contains(&note) => rack_pad(note as u8),
                _ => rack_pad(RACK_FIRST_NOTE + index as u8),
            };
            let (Some(pad), Some(part)) = (pad, first_part(branch)) else { continue };
            if samples.len() <= pad {
                samples.resize(pad + 1, String::new());
            }
            samples[pad] = found(part);
        }
        if samples.iter().any(|s| !s.is_empty()) {
            return (InstrumentKind::DrumRack(RackParams::default()), samples);
        }
        return (InstrumentKind::DrumMachine(Default::default()), Vec::new());
    }
    let sampler = track.descendants().find(|n| n.has_tag_name("OriginalSimpler") || n.has_tag_name("MultiSampler"));
    if let Some(part) = sampler.and_then(first_part) {
        let root = number(part, &["RootKey"]).unwrap_or(60.0) as f32;
        let params = SamplerParams { root: root.clamp(0.0, 127.0), ..SamplerParams::default() };
        return (InstrumentKind::Sampler(params), vec![found(part)]);
    }
    (InstrumentKind::Synth(SynthParams::default()), Vec::new())
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
                velocity: (velocity / 127.0).clamp(0.0, 1.0) as f32, chance: 1.0,
            });
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

fn clip_name(clip: Node) -> Option<String> {
    value(clip, &["Name"]).filter(|n| !n.is_empty()).map(str::to_string)
}

fn clip_color(clip: Node, track_color: [u8; 3]) -> [u8; 3] {
    if child(clip, "Color").is_some() || child(clip, "ColorIndex").is_some() {
        color(clip, 0)
    } else {
        track_color
    }
}

fn session_clip(clip: Node, track_color: [u8; 3]) -> MidiClip {
    let visible = match (number(clip, &["CurrentStart"]), number(clip, &["CurrentEnd"])) {
        (Some(start), Some(end)) if end > start => end - start,
        _ => 4.0,
    };
    let (start, length) = content_window(clip, visible);
    MidiClip {
        groove: None,
        envelopes: Vec::new(),
        name: clip_name(clip).unwrap_or_else(|| "MIDI".into()),
        length,
        notes: notes(clip, start, length),
        color: clip_color(clip, track_color), muted: false,
    }
}

/// Where an arrangement clip sits on the timeline, in beats.
fn arrangement_span(clip: Node) -> Option<(f64, f64)> {
    let start = number(clip, &["CurrentStart"]).or_else(|| attribute_number(clip, "Time"))?;
    let end = number(clip, &["CurrentEnd"])?;
    (end > start).then_some((start.max(0.0), end))
}

fn arrangement_clip(clip: Node, track_color: [u8; 3]) -> Option<ArrangementClip> {
    let (start, end) = arrangement_span(clip)?;
    let mut content = session_clip(clip, track_color);
    let (window_start, length) = content_window(clip, end - start);
    content.length = length;
    content.notes = notes(clip, window_start, length);
    Some(ArrangementClip {
        start,
        length: end - start,
        clip: Clip::Midi(content),
    })
}

/// Tempo implied by a clip's warp markers: (bpm, seconds, beats) of the first
/// marker, and the markers themselves when there are more than two (an uneven warp).
fn warp_reference(clip: Node) -> Option<(f64, f64, f64, Vec<WarpMarker>)> {
    let mut markers: Vec<(f64, f64)> = path(clip, &["WarpMarkers"])?
        .children()
        .filter(|n| n.has_tag_name("WarpMarker"))
        .filter_map(|m| Some((attribute_number(m, "SecTime")?, attribute_number(m, "BeatTime")?)))
        .collect();
    markers.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (&(first_sec, first_beat), &(last_sec, last_beat)) = (markers.first()?, markers.last()?);
    // The average tempo between the outer ones; between them each stretch keeps its own.
    let rising = markers.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1);
    let warp = if markers.len() > 2 && rising { markers.iter().map(|&(seconds, beat)| WarpMarker { beat, seconds }).collect() } else { Vec::new() };
    (last_beat > first_beat && last_sec > first_sec)
        .then(|| ((last_beat - first_beat) / (last_sec - first_sec) * 60.0, first_sec, first_beat, warp))
}

/// `timeline_length` is the clip's span on the arrangement, if it is placed there.
fn audio_clip(clip: Node, track_color: [u8; 3], timeline_length: Option<f64>, ctx: &Context, report: &mut ImportReport) -> AudioClip {
    let sample = resolve_sample(clip, ctx.base);
    if sample.as_os_str().is_empty() || !sample.exists() {
        report.missing_samples += 1;
    }
    let loop_start = number(clip, &["Loop", "LoopStart"]).unwrap_or(0.0);
    let loop_end = number(clip, &["Loop", "LoopEnd"]).unwrap_or(loop_start + 4.0);
    let looping = flag(clip, &["Loop", "LoopOn"]).unwrap_or(true);
    let warped = flag(clip, &["IsWarped"]).unwrap_or(true);

    let (warp, original_bpm, offset, length, warp_markers) = if warped {
        let (bpm, first_sec, first_beat, markers) = warp_reference(clip).unwrap_or((ctx.tempo, 0.0, 0.0, Vec::new()));
        let length = match (looping, timeline_length) {
            (false, Some(span)) => span,
            _ => loop_end - loop_start,
        };
        let mode = if number(clip, &["WarpMode"]) == Some(3.0) { WarpMode::Repitch } else { WarpMode::Stretch };
        // With uneven markers, set from them below.
        let offset = first_sec + (loop_start - first_beat) * 60.0 / bpm;
        (mode, bpm, offset, length, markers)
    } else {
        // Unwarped clips keep their start and loop markers in seconds.
        let seconds = match (looping, timeline_length) {
            (false, Some(span)) => span * 60.0 / ctx.tempo,
            _ => loop_end - loop_start,
        };
        (WarpMode::Off, ctx.tempo, loop_start, seconds * ctx.tempo / 60.0, Vec::new())
    };

    let volume = number(clip, &["SampleVolume"]).unwrap_or(1.0);
    let name = clip_name(clip)
        .or_else(|| sample.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Audio".into());
    let mut audio = AudioClip { hard_start: false, fades: Default::default(),
        envelopes: Vec::new(),
        name,
        color: clip_color(clip, track_color),
        sample: sample.to_string_lossy().into_owned(),
        length: if length > 0.0 { length } else { 4.0 },
        offset: offset.max(0.0),
        warp,
        original_bpm,
        warp_markers,
        takes: Vec::new(),
        key: None,
        gain_db: if volume > 0.0 { (20.0 * volume.log10()) as f32 } else { -70.0 },
        transpose: number(clip, &["PitchCoarse"]).unwrap_or(0.0) as f32, muted: false,
    };
    if audio.has_markers() {
        audio.offset = audio.file_seconds(loop_start).max(0.0);
    }
    audio
}

/// Finds a clip's audio file: its absolute path, its path relative to the set
/// (Live 11+ string, or Live 10 path elements), or a search by name in the set folder.
fn resolve_sample(clip: Node, base: Option<&Path>) -> PathBuf {
    let Some(file) = path(clip, &["SampleRef", "FileRef"]) else { return PathBuf::new() };
    let name = value(file, &["Name"]).filter(|n| !n.is_empty());
    let mut candidates = Vec::new();
    if let Some(absolute) = value(file, &["Path"]).filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(absolute));
    }
    if let Some(base) = base {
        if let Some(relative) = value(file, &["RelativePath"]).filter(|p| !p.is_empty()) {
            candidates.push(base.join(relative));
        }
        if let (Some(elements), Some(name)) = (child(file, "RelativePath"), name) {
            let mut joined = base.to_path_buf();
            for element in elements.children().filter(|n| n.has_tag_name("RelativePathElement")) {
                match element.attribute("Dir") {
                    Some("") | None => joined.push(".."),
                    Some(dir) => joined.push(dir),
                }
            }
            joined.push(name);
            candidates.push(joined);
        }
    }
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        return found.clone();
    }
    let file_name = name.map(str::to_string).or_else(|| {
        candidates.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
    });
    if let (Some(base), Some(file_name)) = (base, &file_name) {
        if let Some(found) = find_file(base, file_name, 4) {
            return found;
        }
    }
    candidates.into_iter().next().unwrap_or_else(|| PathBuf::from(file_name.unwrap_or_default()))
}

fn find_file(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    if let Some(entry) = entries.iter().find(|e| e.file_name().to_string_lossy() == name) {
        return Some(entry.path());
    }
    if depth == 0 {
        return None;
    }
    entries
        .iter()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .find_map(|e| find_file(&e.path(), name, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A small file that unpacks to more than any set (a «zip bomb») is
    /// refused without filling the memory.
    #[test]
    fn a_set_that_unpacks_to_too_much_is_refused() {
        let path = std::env::temp_dir().join(format!("pulse-als-bomb-{}.als", std::process::id()));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let zeros = vec![b' '; 1 << 20];
        for _ in 0..(MAX_XML >> 20) + 4 {
            gz.write_all(&zeros).unwrap();
        }
        let bytes = gz.finish().unwrap();
        assert!(bytes.len() < 2 << 20, "{}", bytes.len());
        std::fs::write(&path, bytes).unwrap();
        let error = import(&path).err().unwrap();
        assert!(error.contains("no parece un set de Live"), "{error}");
        std::fs::remove_file(&path).ok();
    }

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
        assert!(drums.instrument.is_drums());
        assert!((drums.mixer.gain - 0.501).abs() < 0.001);
        assert_eq!(drums.mixer.pan, -0.25);
        assert_eq!(drums.sends[..2], [0.5, 0.0]);
        assert_eq!(report.audio_tracks, 1);
        assert_eq!(report.midi_tracks, 1);
        assert!(project.tracks[1].is_audio());
        assert_eq!(project.tracks[1].name, "Vocals");
    }

    #[test]
    fn imports_session_clip_notes() {
        let (project, report) = parse(SET).unwrap();
        let Some(Clip::Midi(clip)) = project.tracks[0].clips[0].as_ref() else { panic!("expected a MIDI clip") };
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
        let Clip::Midi(clip) = &placed.clip else { panic!("expected a MIDI clip") };
        assert_eq!(clip.length, 4.0);
        // Only the note inside the loop window [4, 8) survives, shifted to clip time.
        assert_eq!(clip.notes.len(), 1);
        assert_eq!(clip.notes[0].start, 0.5);
        assert_eq!(clip.name, "MIDI");
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
    fn locators_become_markers() {
        let set = r#"<Ableton><LiveSet>
            <Tracks><MidiTrack/></Tracks>
            <Locators><Locators>
                <Locator Id="1"><Time Value="32"/><Name Value="Drop"/></Locator>
                <Locator Id="0"><Time Value="0"/><Name Value="Intro"/></Locator>
            </Locators></Locators>
        </LiveSet></Ableton>"#;
        let (project, _) = parse(set).unwrap();
        let markers: Vec<(f64, &str)> = project.markers.iter().map(|m| (m.beat, m.name.as_str())).collect();
        assert_eq!(markers, vec![(0.0, "Intro"), (32.0, "Drop")]);
    }

    #[test]
    fn groups_come_with_their_tracks() {
        let set = r#"<Ableton><LiveSet><Tracks>
            <GroupTrack Id="10"><Name><EffectiveName Value="Drums"/></Name><TrackGroupId Value="-1"/></GroupTrack>
            <MidiTrack Id="11"><Name><EffectiveName Value="Kick"/></Name><TrackGroupId Value="10"/></MidiTrack>
            <GroupTrack Id="12"><Name><EffectiveName Value="Tops"/></Name><TrackGroupId Value="10"/></GroupTrack>
            <MidiTrack Id="13"><Name><EffectiveName Value="Hats"/></Name><TrackGroupId Value="12"/></MidiTrack>
            <MidiTrack Id="14"><Name><EffectiveName Value="Bass"/></Name><TrackGroupId Value="-1"/></MidiTrack>
        </Tracks></LiveSet></Ableton>"#;
        let (project, report) = parse(set).unwrap();
        let layout: Vec<(String, u8, bool)> = project.tracks.iter().map(|t| (t.name.clone(), t.depth, t.is_group())).collect();
        assert_eq!(
            layout,
            vec![("Drums".into(), 0, true), ("Kick".into(), 1, false), ("Tops".into(), 1, true), ("Hats".into(), 2, false), ("Bass".into(), 0, false)]
        );
        assert_eq!(report.group_tracks, 2);
        assert_eq!(&crate::groups::parents(&project.tracks)[..5], &[None, Some(0), Some(0), Some(2), None]);
    }

    #[test]
    fn rejects_empty_sets_and_non_live_files() {
        let empty = r#"<Ableton><LiveSet><Tracks><GroupTrack/></Tracks></LiveSet></Ableton>"#;
        assert!(parse(empty).is_err());
        assert!(parse("<html/>").is_err());
    }

    /// An audio track with a warped session loop and an unwarped arrangement clip.
    fn audio_set(sample_path: &str) -> String {
        format!(r#"<Ableton><LiveSet>
          <Tracks><AudioTrack>
            <Name><EffectiveName Value="Loops"/></Name>
            <DeviceChain><MainSequencer>
              <ClipSlotList><ClipSlot><ClipSlot><Value>
                <AudioClip Time="0">
                  <CurrentStart Value="0"/><CurrentEnd Value="8"/>
                  <Loop><LoopStart Value="4"/><LoopEnd Value="12"/><LoopOn Value="true"/></Loop>
                  <Name Value="Break"/>
                  <SampleRef><FileRef><RelativePath Value="Samples/break.wav"/><Path Value="{sample_path}"/></FileRef></SampleRef>
                  <WarpMarkers><WarpMarker SecTime="0" BeatTime="0"/><WarpMarker SecTime="8" BeatTime="16"/></WarpMarkers>
                  <IsWarped Value="true"/><WarpMode Value="3"/>
                  <PitchCoarse Value="-2"/><SampleVolume Value="0.5"/>
                </AudioClip>
              </Value></ClipSlot></ClipSlot></ClipSlotList>
              <ClipTimeable><ArrangerAutomation><Events>
                <AudioClip Time="16">
                  <CurrentStart Value="16"/><CurrentEnd Value="24"/>
                  <Loop><LoopStart Value="1.5"/><LoopEnd Value="5.5"/><LoopOn Value="false"/></Loop>
                  <SampleRef><FileRef><Name Value="break.wav"/></FileRef></SampleRef>
                  <IsWarped Value="false"/>
                </AudioClip>
              </Events></ArrangerAutomation></ClipTimeable>
            </MainSequencer></DeviceChain>
          </AudioTrack></Tracks>
          <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
        </LiveSet></Ableton>"#)
    }

    #[test]
    fn imports_warped_audio_clips() {
        let (project, report) = parse(&audio_set("/nowhere/break.wav")).unwrap();
        let track = &project.tracks[0];
        assert!(track.is_audio());
        let Some(Clip::Audio(clip)) = &track.clips[0] else { panic!("expected an audio clip") };
        assert_eq!(clip.name, "Break");
        // Markers say 16 beats in 8 seconds: 120 BPM; loop start at beat 4 is 2 s into the file.
        assert_eq!(clip.original_bpm, 120.0);
        assert_eq!(clip.offset, 2.0);
        assert_eq!(clip.length, 8.0);
        assert_eq!(clip.warp, WarpMode::Repitch);
        assert_eq!(clip.transpose, -2.0);
        assert!((clip.gain_db + 6.02).abs() < 0.01);
        assert_eq!(report.audio_tracks, 1);
        assert_eq!(report.missing_samples, 2);
    }

    #[test]
    fn uneven_warp_markers_are_kept() {
        let set = audio_set("/nowhere/break.wav").replace(
            r#"<WarpMarker SecTime="8" BeatTime="16"/>"#,
            r#"<WarpMarker SecTime="1" BeatTime="4"/><WarpMarker SecTime="8" BeatTime="16"/>"#,
        );
        let (project, _) = parse(&set).unwrap();
        let Some(Clip::Audio(clip)) = &project.tracks[0].clips[0] else { panic!("expected an audio clip") };
        assert_eq!(clip.warp_markers.len(), 3);
        // The loop starts on beat 4: 1 s into the file, where its marker is.
        assert_eq!(clip.offset, 1.0);
        assert!((clip.source_seconds(6.0, 120.0) - (1.0 + 7.0 / 12.0 * 6.0)).abs() < 1e-9);
    }

    #[test]
    fn unwarped_clips_use_seconds_and_samples_are_found_next_to_the_set() {
        let dir = std::env::temp_dir().join(format!("pulse-als-audio-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples/Imported")).unwrap();
        std::fs::write(dir.join("Samples/Imported/break.wav"), b"stub").unwrap();
        let (project, report) = parse_in(&audio_set("/nowhere/break.wav"), Some(&dir)).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        let placed = &project.tracks[0].arrangement[0];
        let Clip::Audio(clip) = &placed.clip else { panic!("expected an audio clip") };
        assert_eq!((placed.start, placed.length), (16.0, 8.0));
        assert_eq!(clip.warp, WarpMode::Off);
        // Unwarped: the loop start is in seconds, the 8-beat span is 4 s at 120 BPM.
        assert_eq!(clip.offset, 1.5);
        assert_eq!(clip.length, 8.0);
        // Both clips are found by file name inside the set folder, even though the
        // session clip's absolute path points to another computer.
        // By components: Windows writes the path found with «\\».
        let found = |sample: &str| std::path::Path::new(sample).ends_with("Samples/Imported/break.wav");
        assert!(found(&clip.sample), "{}", clip.sample);
        let Some(Clip::Audio(session)) = &project.tracks[0].clips[0] else { panic!("expected an audio clip") };
        assert!(found(&session.sample), "{}", session.sample);
        assert_eq!(report.missing_samples, 0);
    }

    #[test]
    fn drum_rack_and_simpler_samples_come_across() {
        let set = r#"<Ableton><LiveSet><Tracks>
          <MidiTrack><Name><EffectiveName Value="Kit"/></Name><DeviceChain><DeviceChain><Devices>
            <DrumGroupDevice><Branches>
              <DrumBranch><DeviceChain><MidiToAudioDeviceChain><Devices><OriginalSimpler><Player><MultiSampleMap><SampleParts>
                <MultiSamplePart><RootKey Value="60"/><SampleRef><FileRef><Path Value="/nowhere/kick.wav"/></FileRef></SampleRef></MultiSamplePart>
              </SampleParts></MultiSampleMap></Player></OriginalSimpler></Devices></MidiToAudioDeviceChain></DeviceChain>
              <BranchInfo><ReceivingNote Value="92"/></BranchInfo></DrumBranch>
              <DrumBranch><DeviceChain><MidiToAudioDeviceChain><Devices><OriginalSimpler><Player><MultiSampleMap><SampleParts>
                <MultiSamplePart><SampleRef><FileRef><Path Value="/nowhere/snare.wav"/></FileRef></SampleRef></MultiSamplePart>
              </SampleParts></MultiSampleMap></Player></OriginalSimpler></Devices></MidiToAudioDeviceChain></DeviceChain>
              <BranchInfo><ReceivingNote Value="90"/></BranchInfo></DrumBranch>
            </Branches></DrumGroupDevice>
          </Devices></DeviceChain></DeviceChain></MidiTrack>
          <MidiTrack><Name><EffectiveName Value="Keys"/></Name><DeviceChain><DeviceChain><Devices>
            <OriginalSimpler><Player><MultiSampleMap><SampleParts>
              <MultiSamplePart><RootKey Value="48"/><SampleRef><FileRef><Path Value="/nowhere/piano.wav"/></FileRef></SampleRef></MultiSamplePart>
            </SampleParts></MultiSampleMap></Player></OriginalSimpler>
          </Devices></DeviceChain></DeviceChain></MidiTrack>
        </Tracks></LiveSet></Ableton>"#;
        let (project, report) = parse(set).unwrap();
        let kit = &project.tracks[0];
        assert!(matches!(kit.instrument, InstrumentKind::DrumRack(_)));
        // Notes 36 (kick) and 38 (snare) land on pads 1 and 3.
        assert_eq!(kit.sample(0), Some("/nowhere/kick.wav"));
        assert_eq!(kit.sample(1), None);
        assert_eq!(kit.sample(2), Some("/nowhere/snare.wav"));
        let keys = &project.tracks[1];
        let InstrumentKind::Sampler(params) = keys.instrument else { panic!("expected a sampler") };
        assert_eq!(params.root, 48.0);
        assert_eq!(keys.sample(0), Some("/nowhere/piano.wav"));
        assert_eq!(report.missing_samples, 3);
    }

    /// The set's time signature (Live's number for it) and its changes, from
    /// the master's automation; Live 9's older form too.
    #[test]
    fn time_signatures_come_across() {
        // 3/4 is 2 + 99 × 2 = 200; 7/8 is 6 + 99 × 3 = 303; 4/4 is 201.
        let set = r#"<Ableton><LiveSet><Tracks><MidiTrack/></Tracks><MasterTrack>
            <AutomationEnvelopes><Envelopes><AutomationEnvelope>
              <EnvelopeTarget><PointeeId Value="77"/></EnvelopeTarget>
              <Automation><Events>
                <EnumEvent Time="-63072000" Value="200"/>
                <EnumEvent Time="6" Value="303"/>
                <EnumEvent Time="13" Value="201"/>
              </Events></Automation>
            </AutomationEnvelope></Envelopes></AutomationEnvelopes>
            <DeviceChain><Mixer><Tempo><Manual Value="100"/></Tempo>
              <TimeSignature><Manual Value="200"/><AutomationTarget Id="77"/></TimeSignature>
            </Mixer></DeviceChain></MasterTrack></LiveSet></Ableton>"#;
        let (project, _) = parse(set).unwrap();
        let expected = Meter::new(TimeSig::new(3, 4)).with_change(2, TimeSig::new(7, 8)).with_change(4, TimeSig::COMMON);
        assert_eq!(project.meter, expected);
        let older = r#"<Ableton><LiveSet><Tracks><MidiTrack/></Tracks><MasterTrack><DeviceChain><Mixer>
            <TimeSignature><TimeSignatures><RemoteableTimeSignature><Numerator Value="6"/><Denominator Value="8"/></RemoteableTimeSignature></TimeSignatures></TimeSignature>
            </Mixer></DeviceChain></MasterTrack></LiveSet></Ableton>"#;
        assert_eq!(parse(older).unwrap().0.meter, Meter::new(TimeSig::new(6, 8)));
        assert!(parse(&audio_set("/nowhere/a.wav")).unwrap().0.meter.is_common(), "none said: 4/4");
    }

    #[test]
    fn numbers_a_set_cannot_hold_are_ignored() {
        let dir = std::env::temp_dir().join(format!("pulse-als-nan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for tempo in ["NaN", "inf", "1e999"] {
            let path = dir.join("set.als");
            let set = format!(r#"<Ableton><LiveSet><Tracks><MidiTrack/></Tracks><MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="{tempo}"/></Tempo><Volume><Manual Value="{tempo}"/></Volume></Mixer></DeviceChain></MasterTrack></LiveSet></Ableton>"#);
            std::fs::write(&path, set).unwrap();
            let (project, _) = import(&path).unwrap();
            assert!(project.tempo.is_finite() && project.master.mixer.gain.is_finite(), "{tempo}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
