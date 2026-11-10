//! Follow actions in the interface: the editor of a session clip's or a
//! scene's (in the clip panel's bar and in their menus) and how they read.

use crate::app::{ClipRef, PulseApp};
use crate::i18n::{fill, tr};
use crate::theme::TEXT_DIM;
use eframe::egui::{self, RichText};
use engine::follow::{Follow, FollowKind, FollowUnit};
use engine::Command;

/// The kinds a scene can choose: the clip ones but «the same» (a scene
/// starting again is what its clips do on their own).
const SCENE_KINDS: [FollowKind; 8] = [FollowKind::None, FollowKind::Stop, FollowKind::Previous, FollowKind::Next, FollowKind::First, FollowKind::Last, FollowKind::Any, FollowKind::Other];

/// The follow action in a few words: «Siguiente tras 2 vueltas»,
/// «Siguiente 70 % · Parar 30 % tras 1 compás», «Sin acción».
pub fn summary(follow: &Follow) -> String {
    if !follow.is_active() {
        return tr("Sin acción al acabar").into();
    }
    let template = match (follow.unit, follow.count) {
        (FollowUnit::Loops, 1) => tr("{} tras {} vuelta"),
        (FollowUnit::Loops, _) => tr("{} tras {} vueltas"),
        (FollowUnit::Bars, 1) => tr("{} tras {} compás"),
        (FollowUnit::Bars, _) => tr("{} tras {} compases"),
    };
    let a = (follow.chance_a * 100.0).round() as i32;
    let what = if a >= 100 || follow.b == follow.a {
        shown(follow.a).to_string()
    } else if a <= 0 {
        shown(follow.b).to_string()
    } else {
        format!("{} {a} % · {} {} %", shown(follow.a), shown(follow.b), 100 - a)
    };
    fill(template, &[&what, &follow.count])
}

/// A kind's name in the interface's language.
fn shown(kind: FollowKind) -> &'static str {
    match kind {
        FollowKind::None => tr("Nada"),
        FollowKind::Stop => tr("Parar"),
        FollowKind::Again => tr("El mismo"),
        FollowKind::Previous => tr("Anterior"),
        FollowKind::Next => tr("Siguiente"),
        FollowKind::First => tr("Primero"),
        FollowKind::Last => tr("Último"),
        FollowKind::Any => tr("Cualquiera"),
        FollowKind::Other => tr("Otro al azar"),
    }
}

/// Edits `follow` in place; whether it changed. `scene`: a scene's (its
/// loops are its longest clip's, and it has no «the same»).
pub fn editor(ui: &mut egui::Ui, follow: &mut Follow, scene: bool) -> bool {
    let before = *follow;
    ui.set_min_width(300.0);
    ui.label(RichText::new(if scene { tr("Acción al acabar la escena") } else { tr("Acción al acabar el clip") }).strong());
    ui.horizontal(|ui| {
        ui.label(tr("Tras"));
        ui.add(egui::DragValue::new(&mut follow.count).range(1..=64).speed(0.1))
            .on_hover_text(tr("Cuántas vueltas o compases suena antes de decidir (y vuelve a decidir cada tanto si sale «Nada»)"));
        let loops = if scene { tr("vueltas del clip más largo") } else { tr("vueltas") };
        ui.selectable_value(&mut follow.unit, FollowUnit::Loops, loops).on_hover_text(tr("Veces que suena entero"));
        ui.selectable_value(&mut follow.unit, FollowUnit::Bars, tr("compases")).on_hover_text(tr("Compases del compás de la canción"));
    });
    let kinds: &[FollowKind] = if scene { &SCENE_KINDS } else { &FollowKind::ALL };
    let row = |ui: &mut egui::Ui, kind: &mut FollowKind, name: &str| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(name).strong());
            for &k in kinds {
                ui.selectable_value(kind, k, shown(k)).on_hover_text(hint(k, scene));
            }
        });
    };
    row(ui, &mut follow.a, "A");
    row(ui, &mut follow.b, "B");
    ui.horizontal(|ui| {
        ui.label(tr("Probabilidad"));
        let mut percent = (follow.chance_a * 100.0).round();
        let changed = ui
            .add(egui::Slider::new(&mut percent, 0.0..=100.0).step_by(5.0).custom_formatter(|v, _| format!("A {v:.0} % · B {:.0} %", 100.0 - v)))
            .on_hover_text(tr("Cuántas veces de cada cien sale A; el resto, B"))
            .changed();
        if changed {
            follow.chance_a = percent / 100.0;
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(summary(follow)).color(TEXT_DIM).size(10.5));
        if follow.is_active() && ui.small_button(tr("Quitar")).on_hover_text(tr("Sin acción: sigue sonando")).clicked() {
            *follow = Follow::default();
        }
    });
    *follow != before
}

fn hint(kind: FollowKind, scene: bool) -> &'static str {
    match (kind, scene) {
        (FollowKind::None, _) => tr("Sigue sonando"),
        (FollowKind::Stop, false) => tr("Para la pista"),
        (FollowKind::Stop, true) => tr("Para todos los clips"),
        (FollowKind::Again, _) => tr("Vuelve a empezar el mismo clip"),
        (FollowKind::Previous, false) => tr("El clip de encima (del último pasa al primero, y al revés)"),
        (FollowKind::Next, false) => tr("El clip de debajo (del último vuelve al primero)"),
        (FollowKind::First, false) => tr("El primero del grupo: los clips seguidos, sin huecos"),
        (FollowKind::Last, false) => tr("El último del grupo: los clips seguidos, sin huecos"),
        (FollowKind::Any, false) => tr("Uno del grupo al azar, este incluido"),
        (FollowKind::Other, false) => tr("Otro del grupo al azar, nunca este"),
        (FollowKind::Previous, true) => tr("La escena de encima"),
        (FollowKind::Next, true) => tr("La escena de debajo (de la última con clips vuelve a la primera)"),
        (FollowKind::First, true) => tr("La primera escena"),
        (FollowKind::Last, true) => tr("La última escena con clips"),
        (FollowKind::Any, true) => tr("Una escena al azar, esta incluida"),
        (FollowKind::Other, true) => tr("Otra escena al azar, nunca esta"),
    }
}

impl PulseApp {
    /// Gives session clip `slot` of `track` a follow action (none: default).
    pub fn set_follow(&mut self, track: usize, slot: usize, follow: Follow) {
        let Some(t) = self.project.tracks.get_mut(track) else { return };
        if t.clips[slot].is_none() {
            return;
        }
        t.follow[slot] = follow;
        self.audio.send(Command::SetFollow { track, slot, follow });
    }

    pub fn set_scene_follow(&mut self, scene: usize, follow: Follow) {
        self.project.scene_follow[scene] = follow;
        self.audio.send(Command::SetSceneFollow { scene, follow });
    }

    /// The selected clip's follow action, in the clip panel's bar: a button
    /// that says it and opens its editor. Session clips only.
    pub(crate) fn follow_button(&mut self, ui: &mut egui::Ui) {
        let Some(ClipRef::Session { track, slot }) = self.selected_clip.filter(|&c| self.clip(c).is_some()) else { return };
        let mut follow = self.project.tracks[track].follow[slot];
        let label = format!("» {}", summary(&follow));
        let changed = crate::widgets::field_menu_button(ui, &label, |ui| editor(ui, &mut follow, false));
        if changed == Some(true) {
            self.set_follow(track, slot, follow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn summaries_say_what_happens() {
        assert_eq!(summary(&Follow::default()), "Sin acción al acabar");
        let next = Follow { a: FollowKind::Next, count: 2, ..Default::default() };
        assert_eq!(summary(&next), "Siguiente tras 2 vueltas");
        let both = Follow { b: FollowKind::Stop, chance_a: 0.7, count: 1, unit: FollowUnit::Bars, ..next };
        assert_eq!(summary(&both), "Siguiente 70 % · Parar 30 % tras 1 compás");
        assert_eq!(summary(&Follow { chance_a: 0.0, ..both }), "Parar tras 1 compás");
    }

    /// A project's follow actions reach the engine after its clips (which
    /// come without), and only tracks that have some send them.
    #[test]
    fn a_project_s_follow_actions_reach_the_engine_after_its_clips() {
        let mut project = Project::demo();
        let t = (0..project.tracks.len()).find(|&t| project.tracks[t].clips[0].is_some()).unwrap();
        project.tracks[t].follow[0] = Follow { a: FollowKind::Next, ..Default::default() };
        project.scene_follow[2] = Follow { a: FollowKind::First, ..Default::default() };
        let commands = crate::sync::project_commands(&project, &mut Default::default(), 48_000.0, &Default::default());
        let clip = commands.iter().position(|c| matches!(c, Command::SetClip { track, slot: 0, .. } if *track == t)).unwrap();
        let follows: Vec<usize> = commands.iter().enumerate().filter(|(_, c)| matches!(c, Command::SetFollows { .. })).map(|(i, _)| i).collect();
        assert_eq!(follows.len(), 1, "one track has some");
        assert!(follows[0] > clip);
        assert!(matches!(&commands[follows[0]], Command::SetFollows { track, follow } if *track == t && follow[0].a == FollowKind::Next));
        assert!(commands.iter().any(|c| matches!(c, Command::SetSceneFollows(f) if f[2].a == FollowKind::First)));
    }

    /// Rendered offline from a project: after one loop of a silent clip the
    /// next one sounds from its due frame, and a random choice sounds the
    /// same in every render.
    #[test]
    fn rendered_offline_the_follow_action_falls_on_time_and_repeats() {
        use engine::model::{Clip, InstrumentKind, MidiClip, Note, SynthParams};
        let render = |kind: FollowKind| {
            let mut project = Project::empty();
            project.tracks.truncate(1);
            project.tracks[0].instrument = InstrumentKind::Synth(SynthParams::default());
            project.tracks[0].clips[0] = Some(Clip::Midi(MidiClip::new("Silencio", 4.0, [0; 3])));
            for slot in 1..4 {
                let mut clip = MidiClip::new("Nota", 4.0, [0; 3]);
                clip.notes.push(Note { start: 0.0, length: 1.0, pitch: 55 + slot as u8 * 5, velocity: 1.0, chance: 1.0 });
                project.tracks[0].clips[slot] = Some(Clip::Midi(clip));
                project.tracks[0].follow[slot] = Follow { a: FollowKind::Any, ..Default::default() };
            }
            project.tracks[0].follow[0] = Follow { a: kind, ..Default::default() };
            let mut renderer = engine::OfflineRenderer::new(48_000.0);
            for command in crate::sync::project_commands(&project, &mut Default::default(), 48_000.0, &Default::default()) {
                renderer.send(command);
            }
            renderer.send(Command::LaunchClip { track: 0, slot: 0 });
            let (mut l, mut r) = (Vec::new(), Vec::new());
            // Ten loops: 20 s at 120 BPM.
            renderer.render(20 * 48_000, &mut l, &mut r);
            l
        };
        let left = render(FollowKind::Next);
        let first = left.iter().position(|s| s.abs() > 1e-4).expect("the next clip sounds");
        assert!((96_000..96_004).contains(&first), "four beats in: {first}");
        let other = render(FollowKind::Other);
        assert_eq!(other, render(FollowKind::Other), "the same draws in every render");
    }

    /// A follow action is the project's: it saves (format 21), undoes, goes
    /// with its clip when it moves and goes away with it.
    #[test]
    fn follow_actions_save_undo_and_move_with_their_clip() {
        let mut app = PulseApp::headless(Project::demo());
        let next = Follow { a: FollowKind::Next, count: 2, ..Default::default() };
        let t = (0..app.project.tracks.len()).find(|&t| app.project.tracks[t].clips[0].is_some()).unwrap();
        assert!(!app.project.to_json().unwrap().contains("\"format\": 21"));
        app.set_follow(t, 0, next);
        app.set_scene_follow(1, Follow { a: FollowKind::Next, ..Default::default() });
        let json = app.project.to_json().unwrap();
        assert!(json.contains("\"format\": 21"), "a Pulse that doesn't know them would lose them");
        let loaded = Project::from_json(&json).unwrap();
        assert_eq!(loaded.tracks[t].follow[0], next);
        assert_eq!(loaded.scene_follow[1].a, FollowKind::Next);
        // Without any, nothing is written.
        assert!(!Project::demo().to_json().unwrap().contains("follow"));

        app.commit_history();
        app.set_follow(t, 0, Follow::default());
        app.commit_history();
        app.undo();
        assert_eq!(app.project.tracks[t].follow[0], next, "undone");

        // Moved to an empty slot: it goes too.
        let free = (1..engine::NUM_SCENES).find(|&s| app.project.tracks[t].clips[s].is_none()).unwrap();
        assert!(app.move_session_clips(&[(t, 0)], 0, free as isize, false));
        assert_eq!(app.project.tracks[t].follow[free], next);
        assert_eq!(app.project.tracks[t].follow[0], Follow::default());
        // Deleted: gone with it.
        app.set_clip(t, free, None);
        assert_eq!(app.project.tracks[t].follow[free], Follow::default());
        // An empty slot takes none.
        app.set_follow(t, free, next);
        assert_eq!(app.project.tracks[t].follow[free], Follow::default());
    }
}
