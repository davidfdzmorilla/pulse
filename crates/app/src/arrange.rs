//! Automatic arrangement: from the Session's scenes to a song on the
//! Arrangement. A structure (electronic, pop, short) is laid out in sections;
//! each takes the scene that fits its energy (the fullest for the chorus or
//! drop, a lighter one for verses, the lightest for the intro) and only the
//! tracks whose role it wants (no bass in an intro, no drums in a break).
//! Builds end with a snare roll, and the beat before a drop is cut. The plan can
//! be changed in its window before it is laid out; laying it out replaces the
//! Arrangement (⌘Z brings it back).

use crate::app::PulseApp;
use crate::project::Track;
use crate::theme::{ACCENT, TEXT_DIM};
use eframe::egui::{self, RichText};
use engine::model::{ArrangementClip, Clip, InstrumentKind, MidiClip, Note};
use engine::NUM_SCENES;

/// What a track does in the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Drums,
    Bass,
    Harmony,
    Lead,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Drums => "batería",
            Role::Bass => "bajo",
            Role::Harmony => "armonía",
            Role::Lead => "melodía",
        }
    }

    /// How much it adds to a scene's energy.
    fn weight(self) -> f64 {
        match self {
            Role::Drums => 3.0,
            Role::Bass => 3.0,
            Role::Harmony => 1.5,
            Role::Lead => 2.0,
        }
    }
}

fn has(name: &str, words: &[&str]) -> bool {
    let lower = name.to_lowercase();
    words.iter().any(|w| lower.contains(w))
}

/// A track's role: from its instrument, its name (or its audio loops'), and
/// how low its notes are.
pub fn role(track: &Track) -> Role {
    if track.is_drums() || matches!(track.instrument, InstrumentKind::Kick(_)) || has(&track.name, &["drum", "bater", "kick", "bombo", "perc", "hat", "snare", "caja", "beat"]) {
        return Role::Drums;
    }
    // Audio: what its loops are. Drum loops come in in Beats, or say so in
    // their name («Tops_150»); a loop that says nothing is harmony.
    let loops: Vec<&engine::model::AudioClip> = track.clips.iter().flatten().filter_map(|c| if let Clip::Audio(a) = c { Some(a) } else { None }).collect();
    if !loops.is_empty() {
        let file = |a: &engine::model::AudioClip| std::path::Path::new(&a.sample).file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let says = |words: &[&str]| loops.iter().any(|a| has(&a.name, words) || has(&file(a), words));
        if loops.iter().any(|a| a.warp == engine::model::WarpMode::Beats || crate::loop_key::is_drums(&a.name) || crate::loop_key::is_drums(&file(a))) {
            return Role::Drums;
        }
        if says(&["bass", "bajo", "sub"]) {
            return Role::Bass;
        }
        if says(&["lead", "melod", "vox", "voz", "vocal"]) {
            return Role::Lead;
        }
    }
    if matches!(track.instrument, InstrumentKind::Acid(_)) || has(&track.name, &["bass", "bajo", "sub"]) {
        return Role::Bass;
    }
    if has(&track.name, &["lead", "melod", "vox", "voz", "vocal", "solo"]) {
        return Role::Lead;
    }
    // MIDI notes: low ones are a bass; one at a time, a lead; together, harmony.
    let notes: Vec<&Note> = track.clips.iter().flatten().filter_map(|c| if let Clip::Midi(m) = c { Some(&m.notes) } else { None }).flatten().collect();
    if notes.is_empty() {
        return Role::Harmony;
    }
    let mean = notes.iter().map(|n| n.pitch as f64).sum::<f64>() / notes.len() as f64;
    if mean < 48.0 {
        return Role::Bass;
    }
    let together = notes.iter().filter(|a| notes.iter().filter(|b| (b.start - a.start).abs() < 1e-6).count() > 1).count();
    if together * 2 > notes.len() { Role::Harmony } else { Role::Lead }
}

/// Which scene a section takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    /// The fullest scene: chorus, drop.
    Main,
    /// A lighter one: verse.
    Verse,
    /// The lightest: intro, outro.
    Light,
}

/// A structure's section: its name, bars, scene, the roles that play, and
/// whether it ends in a roll (a build) or a cut (the beat before what follows).
struct Shape {
    name: &'static str,
    bars: u32,
    base: Base,
    roles: &'static [Role],
    roll: bool,
    cut: bool,
}

const ALL: &[Role] = &[Role::Drums, Role::Bass, Role::Harmony, Role::Lead];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Electronic,
    Pop,
    Short,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::Electronic, Template::Pop, Template::Short];

    pub fn label(self) -> &'static str {
        match self {
            Template::Electronic => "Electrónica (intro, subida, drop, break, drop, outro)",
            Template::Pop => "Pop (intro, estrofa, estribillo, puente, final)",
            Template::Short => "Corta (intro, A, B, final)",
        }
    }

    fn shapes(self) -> &'static [Shape] {
        use Role::*;
        match self {
            Template::Electronic => &[
                Shape { name: "Intro", bars: 16, base: Base::Light, roles: &[Drums, Harmony], roll: false, cut: false },
                Shape { name: "Subida", bars: 8, base: Base::Verse, roles: &[Drums, Harmony, Lead], roll: true, cut: true },
                Shape { name: "Drop", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Break", bars: 16, base: Base::Verse, roles: &[Harmony, Lead], roll: false, cut: false },
                Shape { name: "Subida", bars: 8, base: Base::Verse, roles: &[Drums, Harmony, Lead], roll: true, cut: true },
                Shape { name: "Drop", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Outro", bars: 16, base: Base::Light, roles: &[Drums, Harmony], roll: false, cut: false },
            ],
            Template::Pop => &[
                Shape { name: "Intro", bars: 8, base: Base::Light, roles: &[Harmony, Lead], roll: false, cut: false },
                Shape { name: "Estrofa", bars: 16, base: Base::Verse, roles: &[Drums, Bass, Harmony], roll: true, cut: false },
                Shape { name: "Estribillo", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Estrofa", bars: 16, base: Base::Verse, roles: &[Drums, Bass, Harmony], roll: true, cut: false },
                Shape { name: "Estribillo", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Puente", bars: 8, base: Base::Verse, roles: &[Harmony, Lead], roll: true, cut: true },
                Shape { name: "Estribillo", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Final", bars: 8, base: Base::Light, roles: &[Harmony, Lead], roll: false, cut: false },
            ],
            Template::Short => &[
                Shape { name: "Intro", bars: 8, base: Base::Light, roles: &[Drums, Harmony], roll: true, cut: false },
                Shape { name: "A", bars: 16, base: Base::Verse, roles: ALL, roll: true, cut: true },
                Shape { name: "B", bars: 16, base: Base::Main, roles: ALL, roll: false, cut: false },
                Shape { name: "Final", bars: 8, base: Base::Light, roles: &[Drums, Harmony], roll: false, cut: false },
            ],
        }
    }
}

/// A section as it will be laid out: editable in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub bars: u32,
    pub scene: usize,
    /// For each track (by index), the scene whose clip it plays, if it plays:
    /// the section's, or, in a full section, another of its own.
    pub tracks: Vec<Option<usize>>,
    pub roll: bool,
    pub cut: bool,
}

/// Words in scene names that say which part they are.
const MAIN_WORDS: [&str; 5] = ["drop", "chorus", "estribillo", "coro", "hook"];
const VERSE_WORDS: [&str; 4] = ["verse", "estrofa", "verso", "groove"];
const LIGHT_WORDS: [&str; 6] = ["intro", "break", "outro", "final", "puente", "bridge"];

/// The words of a scene named for a section itself: a «Break» scene is the
/// break, whatever else there is.
fn section_words(section: &str) -> &'static [&'static str] {
    match section {
        "Intro" => &["intro"],
        "Subida" => &["subida", "build", "riser"],
        "Drop" => &["drop"],
        "Break" => &["break"],
        "Outro" => &["outro"],
        "Estrofa" => &["estrofa", "verse", "verso"],
        "Estribillo" => &["estribillo", "chorus", "coro"],
        "Puente" => &["puente", "bridge"],
        "Final" => &["final", "outro", "end"],
        _ => &[],
    }
}

/// The scenes that have clips, fullest first, by the energy of their tracks.
fn scenes_by_energy(tracks: &[Track], roles: &[Role]) -> Vec<usize> {
    let energy = |s: usize| -> f64 {
        tracks.iter().zip(roles).filter(|(t, _)| !t.is_group() && t.clips[s].is_some()).map(|(_, r)| r.weight()).sum()
    };
    let mut scenes: Vec<usize> = (0..NUM_SCENES).filter(|&s| tracks.iter().any(|t| t.clips[s].is_some())).collect();
    // Fullest first; equal ones in their order.
    scenes.sort_by(|&a, &b| energy(b).total_cmp(&energy(a)).then(a.cmp(&b)));
    scenes
}

/// The sections of `template` for the project's scenes; empty without clips.
/// Scenes named for a part («Drop», «Estrofa», «Intro»…) are taken for it;
/// otherwise the fullest is the chorus or drop, the next the verse, the
/// lightest the intro.
pub fn plan(tracks: &[Track], scene_names: &[String], template: Template) -> Vec<Section> {
    let roles: Vec<Role> = tracks.iter().map(role).collect();
    let scenes = scenes_by_energy(tracks, &roles);
    let Some(&fullest) = scenes.first() else { return Vec::new() };
    let named = |words: &[&str]| scenes.iter().copied().find(|&s| scene_names.get(s).is_some_and(|n| has(n, words)));
    let main = named(&MAIN_WORDS).unwrap_or(fullest);
    let verse = named(&VERSE_WORDS).or_else(|| scenes.iter().copied().find(|&s| s != main)).unwrap_or(main);
    let light = named(&LIGHT_WORDS).or_else(|| scenes.iter().rev().copied().find(|&s| s != main)).unwrap_or(main);
    template
        .shapes()
        .iter()
        .map(|shape| {
            // A scene named for the section is its scene; else the one its
            // energy asks for.
            let scene = named(section_words(shape.name)).unwrap_or(match shape.base {
                Base::Main => main,
                Base::Verse => verse,
                Base::Light => light,
            });
            let tracks = tracks
                .iter()
                .zip(&roles)
                .map(|(t, r)| {
                    if t.is_group() || !shape.roles.contains(r) {
                        return None;
                    }
                    // A full section has every track that has something: from its
                    // own scene, or the fullest of its others.
                    let full = shape.base == Base::Main;
                    let own = t.clips[scene].is_some().then_some(scene);
                    own.or_else(|| full.then(|| scenes.iter().copied().find(|&s| t.clips[s].is_some())).flatten())
                })
                .collect();
            Section { name: shape.name.to_string(), bars: shape.bars, scene, tracks, roll: shape.roll, cut: shape.cut }
        })
        .collect()
}

/// The note a drum track's snare is: its snare or clap if they play on the
/// backbeat (the bar's even pulses: 2 and 4 in 4/4, the second dotted beat in
/// 6/8); else the one played most there more than on the other pulses (not a kick
/// on every beat, which would roll the kick). `sig` is the song's meter.
fn snare(track: &Track, sig: engine::meter::TimeSig) -> u8 {
    use engine::model::drum_note::{CLAP, KICK, SNARE};
    let (mut backbeat, mut downbeat) = ([0i32; 128], [0i32; 128]);
    let pulses = sig.pulses();
    for clip in track.clips.iter().flatten() {
        if let Clip::Midi(m) = clip {
            for n in &m.notes {
                let at = n.start.rem_euclid(sig.bar());
                let Some(k) = pulses.iter().position(|p| (at - p.0).abs() < 0.1 || (at - p.0 - sig.bar()).abs() < 0.1) else { continue };
                if k % 2 == 1 {
                    backbeat[n.pitch as usize & 127] += 1;
                } else {
                    downbeat[n.pitch as usize & 127] += 1;
                }
            }
        }
    }
    if let Some(p) = [SNARE, CLAP].into_iter().filter(|&p| backbeat[p as usize] > 0).max_by_key(|&p| backbeat[p as usize]) {
        return p;
    }
    (0..128)
        .filter(|&p| p != KICK as usize && p != 35)
        .map(|p| (p, backbeat[p] - downbeat[p]))
        .filter(|&(_, score)| score > 0)
        .max_by_key(|&(_, score)| score)
        .map_or(SNARE, |(p, _)| p as u8)
}

/// A bar's snare roll, `bar` beats long: eighths, then sixteenths, getting louder.
fn roll(pitch: u8, color: [u8; 3], bar: f64) -> MidiClip {
    let mut clip = MidiClip::new("Redoble", bar, color);
    let mut beat = 0.0;
    while beat < bar - 1e-9 {
        let step = if beat < bar / 2.0 { 0.5 } else { 0.25 };
        clip.notes.push(Note { start: beat, length: step * 0.8, pitch, velocity: (0.45 + 0.52 * (beat / bar) as f32).min(1.0), chance: 1.0 });
        beat += step;
    }
    clip
}

impl PulseApp {
    /// Lays the sections out on the Arrangement, replacing what it had.
    pub fn lay_out(&mut self, sections: &[Section]) {
        if sections.is_empty() {
            return;
        }
        for track in &mut self.project.tracks {
            track.arrangement.clear();
        }
        let mut start = 0.0;
        self.project.markers.clear();
        let meter = self.project.meter;
        for (k, section) in sections.iter().enumerate() {
            // Its bars in the song's meter, and the last one's length (for the roll).
            let length = meter.add_bars(start, section.bars as i64) - start;
            let last_bar = meter.bar_length_at(meter.add_bars(start, section.bars as i64 - 1));
            self.project.markers.push(crate::project::SongMarker { beat: start, name: section.name.clone() });
            // A cut: the last beat silent, before what comes next.
            let cut = if section.cut && k + 1 < sections.len() { 1.0 } else { 0.0 };
            // A build's last bar is the roll alone: the kick and the other
            // drums stop under it.
            let rolling = section.roll && section.bars > 1 && section.tracks.iter().enumerate().any(|(t, s)| s.is_some() && self.project.tracks.get(t).is_some_and(|t| t.is_drums()));
            for (t, &source) in section.tracks.iter().enumerate() {
                let Some(track) = self.project.tracks.get_mut(t) else { continue };
                let Some(clip) = source.and_then(|s| track.clips.get(s).cloned().flatten()) else { continue };
                let rolls = rolling && track.is_drums() && matches!(clip, Clip::Midi(_));
                let body = if rolling && role(track) == Role::Drums { length - last_bar } else { length - cut };
                track.arrangement.push(ArrangementClip { start, length: body, clip: clip.clone() });
                if rolls {
                    let pitch = snare(track, meter.first());
                    track.arrangement.push(ArrangementClip { start: start + length - last_bar, length: last_bar - cut, clip: Clip::Midi(roll(pitch, track.color, last_bar)) });
                }
            }
            start += length;
        }
        self.project.loop_start = 0.0;
        self.project.loop_end = start;
        self.selected_clip = None;
        self.view = crate::app::View::Arrangement;
        self.sync_all();
        self.sync_loop();
        self.back_to_arrangement();
        let bars: u32 = sections.iter().map(|s| s.bars).sum();
        self.status = Some(crate::theme::keys(&format!("Canción de {bars} compases en {} partes en el Arrangement (⌘Z la deshace)", sections.len())));
    }
}

/// The window's state.
#[derive(Default)]
pub struct Window {
    pub open: bool,
    template: Option<Template>,
    sections: Vec<Section>,
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.arrange.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Arreglo automático")
        .id(egui::Id::new("arrange"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([760.0, 520.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| contents(app, ui));
    app.arrange.open &= open;
}

fn contents(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("De las escenas de Session a una canción en el Arrangement. Elige una estructura, retócala y créala.").color(TEXT_DIM));
    ui.horizontal(|ui| {
        for template in Template::ALL {
            if ui.selectable_label(app.arrange.template == Some(template), template.label()).clicked() {
                app.arrange.template = Some(template);
                app.arrange.sections = plan(&app.project.tracks, &app.project.scene_names, template);
            }
        }
    });
    let Some(_) = app.arrange.template else { return };
    if app.arrange.sections.is_empty() {
        ui.label(RichText::new("No hay clips en las escenas de Session de los que hacer una canción.").color(ACCENT));
        return;
    }
    // Tracks that ever play (rows), sections (columns).
    let tracks = &app.project.tracks;
    let shown: Vec<usize> = (0..tracks.len()).filter(|&t| !tracks[t].is_group() && tracks[t].clips.iter().any(Option::is_some)).collect();
    let names: Vec<String> = shown.iter().map(|&t| format!("{} ({})", tracks[t].name, role(&tracks[t]).label())).collect();
    let scene_names = app.project.scene_names.clone();
    let sections = &mut app.arrange.sections;
    egui::ScrollArea::both().max_height(380.0).show(ui, |ui| {
        egui::Grid::new("arrange grid").striped(true).spacing(egui::vec2(10.0, 4.0)).show(ui, |ui| {
            ui.label("");
            for s in sections.iter_mut() {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&s.name).strong());
                    ui.add(egui::DragValue::new(&mut s.bars).range(1..=64).suffix(" c."));
                    let scene = scene_names.get(s.scene).cloned().unwrap_or_default();
                    ui.label(RichText::new(format!("Escena {scene}")).color(TEXT_DIM).size(10.5));
                });
            }
            ui.end_row();
            for (row, &t) in shown.iter().enumerate() {
                ui.label(&names[row]);
                for s in sections.iter_mut() {
                    // Its clip in the section's scene, or else its fullest one.
                    let clips = &app.project.tracks[t].clips;
                    let fallback = if clips[s.scene].is_some() { Some(s.scene) } else { (0..NUM_SCENES).find(|&k| clips[k].is_some()) };
                    if let Some(source) = s.tracks.get_mut(t) {
                        let mut plays = source.is_some();
                        let other = source.is_some_and(|k| k != s.scene);
                        let hint = if other { "Toca su clip de otra escena (aquí no tiene)" } else { "Suena en esta parte" };
                        if ui.add_enabled(fallback.is_some(), egui::Checkbox::without_text(&mut plays)).on_hover_text(hint).changed() {
                            *source = if plays { fallback } else { None };
                        }
                    }
                }
                ui.end_row();
            }
            ui.label(RichText::new("Redoble al final").color(TEXT_DIM));
            for s in sections.iter_mut() {
                ui.checkbox(&mut s.roll, "").on_hover_text("El último compás de la batería, un redoble de caja que sube");
            }
            ui.end_row();
            ui.label(RichText::new("Corte antes de lo siguiente").color(TEXT_DIM));
            for s in sections.iter_mut() {
                ui.checkbox(&mut s.cut, "").on_hover_text("El último tiempo en silencio, para que lo que sigue entre con fuerza");
            }
            ui.end_row();
        });
    });
    ui.separator();
    let bars: u32 = app.arrange.sections.iter().map(|s| s.bars).sum();
    let replaces = app.project.tracks.iter().any(|t| !t.arrangement.is_empty());
    ui.horizontal(|ui| {
        let label = if replaces { "Crear (sustituye el Arrangement)" } else { "Crear en el Arrangement" };
        if ui.button(label).on_hover_text(crate::theme::keys("⌘Z lo deshace")).clicked() {
            let sections = app.arrange.sections.clone();
            app.lay_out(&sections);
            app.arrange.open = false;
        }
        ui.label(RichText::new(format!("{bars} compases")).color(TEXT_DIM));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn roles_come_from_instruments_names_and_notes() {
        let demo = Project::demo();
        let roles: Vec<(String, Role)> = demo.tracks.iter().map(|t| (t.name.clone(), role(t))).collect();
        let of = |name: &str| roles.iter().find(|(n, _)| n == name).map(|r| r.1);
        assert_eq!(of("Drums"), Some(Role::Drums));
        assert_eq!(of("Bass"), Some(Role::Bass));
        assert_eq!(of("Chords"), Some(Role::Harmony));
        assert_eq!(of("Lead"), Some(Role::Lead));
    }

    #[test]
    fn the_fullest_scene_is_the_drop_and_breaks_have_no_drums() {
        let demo = Project::demo();
        let sections = plan(&demo.tracks, &demo.scene_names, Template::Electronic);
        assert_eq!(sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Intro", "Subida", "Drop", "Break", "Subida", "Drop", "Outro"]);
        let drums = demo.tracks.iter().position(|t| t.name == "Drums").unwrap();
        let bass = demo.tracks.iter().position(|t| t.name == "Bass").unwrap();
        let drop = &sections[2];
        // The demo's second scene is called «Drop»: it is the drop's, and the bass
        // (which has no clip there) brings its own from the other scene.
        assert_eq!(drop.scene, 1);
        assert_eq!((drop.tracks[drums], drop.tracks[bass]), (Some(1), Some(0)));
        let brk = &sections[3];
        assert!(brk.tracks[drums].is_none() && brk.tracks[bass].is_none());
        assert!(sections[0].tracks[bass].is_none(), "no bass in the intro");
        assert_eq!(sections[0].scene, 0, "«Intro» for the intro");
        // Unnamed scenes: the fullest is the drop (the first: drums, bass, chords).
        let mut unnamed = demo.clone();
        unnamed.scene_names = (1..=NUM_SCENES).map(|i| i.to_string()).collect();
        assert_eq!(plan(&unnamed.tracks, &unnamed.scene_names, Template::Electronic)[2].scene, 0);
        // Without clips, nothing.
        let empty = Project::empty();
        assert!(plan(&empty.tracks, &empty.scene_names, Template::Pop).is_empty());
    }

    /// A hard techno track in scenes (from the 2026-10b audit): its kick on a
    /// Kick Designer track, the rest of the beat (claps rolling, no kick) on a
    /// Drum Machine, an acid bass, stabs, and a loop of tops on an audio track
    /// with no name. The tops are drums, the «Break» scene is the break, the
    /// build rolls the clap, and the kick stops under the roll.
    #[test]
    fn a_hard_techno_song_in_scenes() {
        use crate::drum_gen::{self, Level, Style};
        use engine::model::{AudioClip, KickParams, WarpMode};
        let mut project = Project::empty();
        project.tempo = 147.0;
        let kit = drum_gen::Kit::from_rows(&engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect::<Vec<_>>());
        let mut drums = Track::new("Drums", [0; 3], InstrumentKind::DrumMachine(Style::HardTechno.sound()));
        let mut sounds = Style::HardTechno.sounds();
        sounds[drum_gen::Sound::Kick as usize] = false;
        for (scene, level) in [(0, Level::Basic), (1, Level::Medium), (2, Level::Intense)] {
            let settings = drum_gen::Settings { style: Style::HardTechno, level, bars: 4, sounds, ..Default::default() };
            let (notes, groove) = drum_gen::generate(&kit, &settings, 16.0, &[], 0, engine::meter::TimeSig::COMMON);
            drums.clips[scene] = Some(Clip::Midi(MidiClip { notes, groove, ..MidiClip::new("Beat", 16.0, [0; 3]) }));
        }
        let mut kick = Track::new("Bd", [0; 3], InstrumentKind::Kick(KickParams::presets().into_iter().find(|(n, _)| *n == "Hard · Kick").unwrap().1));
        let mut four = MidiClip::new("4/4", 16.0, [0; 3]);
        four.notes = (0..16).map(|b| Note { start: b as f64, length: 0.25, pitch: 36, velocity: 1.0, chance: 1.0 }).collect();
        for scene in 0..3 {
            kick.clips[scene] = Some(Clip::Midi(four.clone()));
        }
        let mut bass = Track::new("Acid", [0; 3], InstrumentKind::Acid(Default::default()));
        bass.clips[1] = Some(Clip::Midi(MidiClip { notes: vec![Note { start: 0.5, length: 0.25, pitch: 29, velocity: 1.0, chance: 1.0 }], ..MidiClip::new("Bass", 16.0, [0; 3]) }));
        bass.clips[2] = bass.clips[1].clone();
        let mut stab = Track::new("Stab", [0; 3], InstrumentKind::Synth(Default::default()));
        let chord: Vec<Note> = [65u8, 68, 72].iter().map(|&p| Note { start: 0.5, length: 0.25, pitch: p, velocity: 0.8, chance: 1.0 }).collect();
        stab.clips[2] = Some(Clip::Midi(MidiClip { notes: chord, ..MidiClip::new("Stab", 16.0, [0; 3]) }));
        stab.clips[3] = stab.clips[2].clone();
        let mut audio = Track::new_audio("6 Audio", [0; 3]);
        let tops = AudioClip {
            hard_start: false,
            fades: Default::default(),
            name: "Tops_150".into(),
            color: [0; 3],
            sample: "/loops/Tops_150.wav".into(),
            length: 16.0,
            offset: 0.0,
            warp: WarpMode::Beats,
            original_bpm: 150.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
            muted: false,
        };
        for scene in 0..3 {
            audio.clips[scene] = Some(Clip::Audio(tops.clone()));
        }
        project.tracks = vec![kick, drums, bass, stab, audio];
        for (scene, name) in ["Intro", "Groove", "Drop", "Break"].into_iter().enumerate() {
            project.scene_names[scene] = name.into();
        }
        assert_eq!(project.tracks.iter().map(role).collect::<Vec<_>>(), [Role::Drums, Role::Drums, Role::Bass, Role::Harmony, Role::Drums]);
        let sections = plan(&project.tracks, &project.scene_names, Template::Electronic);
        let brk = sections.iter().find(|s| s.name == "Break").unwrap();
        assert_eq!(brk.scene, 3, "the «Break» scene");
        assert_eq!(brk.tracks, [None, None, None, Some(3), None], "no drums or bass: the stabs");
        let mut app = PulseApp::headless(project);
        app.lay_out(&sections);
        let roll = app.project.tracks[1].arrangement.iter().find_map(|a| if let Clip::Midi(m) = &a.clip { (m.name == "Redoble").then(|| m.notes[0].pitch) } else { None });
        assert_eq!(roll, Some(engine::model::drum_note::CLAP), "the claps roll, not a kick");
        // The first build (bars 17 to 24): the kick and the tops stop for its last bar.
        let (build_end, last_bar) = (24.0 * 4.0, 23.0 * 4.0);
        for t in [0, 4] {
            let lane = &app.project.tracks[t].arrangement;
            assert!(lane.iter().all(|c| c.start >= build_end || c.end() <= last_bar + 1e-9), "{}: {:?}", app.project.tracks[t].name, lane.iter().map(|c| (c.start, c.end())).collect::<Vec<_>>());
        }
    }

    /// In 3/4 the sections are bars of three and the roll fills the last one.
    #[test]
    fn a_waltz_lays_out_in_its_bars() {
        let mut app = PulseApp::headless(Project::demo());
        app.project.meter = engine::meter::Meter::new(engine::meter::TimeSig::new(3, 4));
        let sections = plan(&app.project.tracks, &app.project.scene_names, Template::Electronic);
        app.lay_out(&sections);
        let drums = app.project.tracks.iter().position(|t| t.name == "Drums").unwrap();
        let lane = &app.project.tracks[drums].arrangement;
        assert_eq!((lane[0].start, lane[0].length), (0.0, 48.0), "16 bars of 3");
        let roll = lane.iter().find(|c| c.clip.name() == "Redoble").expect("a roll");
        assert_eq!((roll.start, roll.length), (48.0 + 21.0, 2.0), "the build's last bar, a beat short");
        assert_eq!(app.project.loop_end, 96.0 * 3.0);
    }

    #[test]
    fn the_window_shows_the_plan() {
        let mut app = PulseApp::headless(Project::demo());
        app.open_panel(crate::app::Panel::Arrange);
        app.arrange.template = Some(Template::Pop);
        app.arrange.sections = plan(&app.project.tracks, &app.project.scene_names, Template::Pop);
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let output = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        for expected in ["Estribillo", "Puente", "Bass (bajo)"] {
            assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
        }
        assert!(texts.iter().any(|t| t.starts_with("Crear")));
    }

    #[test]
    fn laying_out_makes_the_song_with_rolls_and_cuts() {
        let mut app = PulseApp::headless(Project::demo());
        let sections = plan(&app.project.tracks, &app.project.scene_names, Template::Electronic);
        app.lay_out(&sections);
        let drums = app.project.tracks.iter().position(|t| t.name == "Drums").unwrap();
        let lane = &app.project.tracks[drums].arrangement;
        // Intro 16 bars (64 beats) from 0; the first build (8 bars) from 64 ends
        // with a roll in its last bar, cut a beat short; the drop from 96.
        assert_eq!(lane[0].start, 0.0);
        assert_eq!(lane[0].length, 64.0);
        let roll = lane.iter().find(|c| c.clip.name() == "Redoble").expect("a roll");
        assert_eq!((roll.start, roll.length), (64.0 + 28.0, 3.0));
        assert!(lane.iter().any(|c| c.start == 96.0), "the drop");
        // The bass only plays in the drops.
        let bass = app.project.tracks.iter().position(|t| t.name == "Bass").unwrap();
        let starts: Vec<f64> = app.project.tracks[bass].arrangement.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![96.0, 256.0]);
        assert_eq!(app.project.loop_end, 384.0, "96 bars");
        // A marker at each part.
        let markers: Vec<(f64, &str)> = app.project.markers.iter().map(|m| (m.beat, m.name.as_str())).collect();
        assert_eq!(markers[..3], [(0.0, "Intro"), (64.0, "Subida"), (96.0, "Drop")]);
        assert_eq!(markers.len(), 7);
        assert!(app.view == crate::app::View::Arrangement);
    }
}
