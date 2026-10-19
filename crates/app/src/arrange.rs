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

/// A track's role: from its instrument, its name, and how low its notes are.
pub fn role(track: &Track) -> Role {
    if track.is_drums() || has(&track.name, &["drum", "bater", "kick", "bombo", "perc", "hat", "snare", "caja", "beat"]) {
        return Role::Drums;
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
            let scene = match shape.base {
                Base::Main => main,
                Base::Verse => verse,
                Base::Light => light,
            };
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

/// The note a drum track's snare is: the one played most on beats 2 and 4.
fn snare(track: &Track) -> u8 {
    let mut count = [0u32; 128];
    for clip in track.clips.iter().flatten() {
        if let Clip::Midi(m) = clip {
            for n in &m.notes {
                if (n.start.rem_euclid(2.0) - 1.0).abs() < 0.1 {
                    count[n.pitch as usize & 127] += 1;
                }
            }
        }
    }
    (0..128).max_by_key(|&p| count[p]).filter(|&p| count[p] > 0).map_or(38, |p| p as u8)
}

/// A bar's snare roll: eighths, then sixteenths, getting louder.
fn roll(pitch: u8, color: [u8; 3]) -> MidiClip {
    let mut clip = MidiClip::new("Redoble", 4.0, color);
    let mut beat = 0.0;
    while beat < 4.0 - 1e-9 {
        let step = if beat < 2.0 { 0.5 } else { 0.25 };
        clip.notes.push(Note { start: beat, length: step * 0.8, pitch, velocity: (0.45 + 0.13 * beat as f32).min(1.0) });
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
        for (k, section) in sections.iter().enumerate() {
            let length = section.bars as f64 * 4.0;
            self.project.markers.push(crate::project::SongMarker { beat: start, name: section.name.clone() });
            // A cut: the last beat silent, before what comes next.
            let cut = if section.cut && k + 1 < sections.len() { 1.0 } else { 0.0 };
            for (t, &source) in section.tracks.iter().enumerate() {
                let Some(track) = self.project.tracks.get_mut(t) else { continue };
                let Some(clip) = source.and_then(|s| track.clips.get(s).cloned().flatten()) else { continue };
                let rolls = section.roll && track.is_drums() && matches!(clip, Clip::Midi(_)) && section.bars > 1;
                let body = if rolls { length - 4.0 } else { length - cut };
                track.arrangement.push(ArrangementClip { start, length: body, clip: clip.clone() });
                if rolls {
                    let pitch = snare(track);
                    track.arrangement.push(ArrangementClip { start: start + length - 4.0, length: 4.0 - cut, clip: Clip::Midi(roll(pitch, track.color)) });
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
        self.status = Some(format!("Canción de {} compases en {} partes en el Arrangement (⌘Z la deshace)", (start / 4.0) as u32, sections.len()));
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
        if ui.button(label).on_hover_text("⌘Z lo deshace").clicked() {
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
