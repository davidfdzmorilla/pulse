//! Adding and deleting return tracks: up to `MAX_RETURNS`, as in Live. Deleting
//! one takes with it what sent to it (the tracks' sends, their automation and
//! clip envelopes, the MIDI controllers and macros on it), and the ones after it
//! move up a letter: their names too, when they start with it ("C Delay" → "B
//! Delay").

use crate::app::PulseApp;
use crate::project::{return_letter, Mixer, Project, ReturnTrack, PALETTE};
use engine::model::{AutomationLane, Clip, Param};
use engine::{Target, MAX_RETURNS};

/// A new, empty return: its letter and "Retorno".
pub fn new_return(index: usize) -> ReturnTrack {
    ReturnTrack { name: format!("{} Retorno", return_letter(index)), color: PALETTE[(index * 5 + 8) % PALETTE.len()], mixer: Mixer::default(), devices: Vec::new(), automation: Vec::new() }
}

/// What a return is called after its letter: «Reverb» in «A Reverb» (all
/// of it if it doesn't start with its letter).
pub fn bare_name(name: &str, index: usize) -> &str {
    let letter = return_letter(index).to_string();
    if name == letter { "" } else { name.strip_prefix(&format!("{letter} ")).unwrap_or(name) }
}

/// Calls return `index` `bare`, after its letter, so it keeps moving up a
/// letter with the others ("C Sala" → "B Sala").
pub fn set_name(ret: &mut ReturnTrack, index: usize, bare: &str) {
    ret.name = format!("{} {bare}", return_letter(index));
}

/// The id of the name field in return `index`'s menu.
pub fn name_field(index: usize) -> eframe::egui::Id {
    eframe::egui::Id::new(("return name", index))
}

/// The right-click menu of return `index`'s header: its name (after the
/// letter), its color and deleting it. True when «Eliminar retorno» is clicked.
pub fn header_menu(ui: &mut eframe::egui::Ui, ret: &mut ReturnTrack, index: usize) -> bool {
    ui.label("Nombre");
    let mut bare = bare_name(&ret.name, index).to_string();
    let field = ui.horizontal(|ui| {
        ui.label(return_letter(index).to_string());
        ui.add(eframe::egui::TextEdit::singleline(&mut bare).id(name_field(index)).desired_width(120.0)).on_hover_text("El nombre después de la letra. La letra cambia sola si borras un retorno anterior")
    });
    if field.inner.changed() {
        set_name(ret, index, &bare);
    }
    ui.label("Color");
    crate::session::color_picker(ui, &mut ret.color);
    ui.separator();
    let delete = ui.button("Eliminar retorno").on_hover_text("Las pistas dejan de enviarle sonido; los retornos de después suben una letra").clicked();
    if delete {
        ui.close();
    }
    delete
}

/// An effect as it goes into a chain from the browser: on a return, a reverb,
/// a delay or a Space all wet (the dry sound is the tracks' own); elsewhere,
/// as it is.
pub fn for_target(mut effect: engine::model::Effect, target: Target) -> engine::model::Effect {
    use engine::model::Effect;
    if let Target::Return(_) = target {
        match &mut effect {
            Effect::Reverb(p) => p.mix = 1.0,
            Effect::Delay(p) => p.mix = 1.0,
            Effect::Convolution(p) => p.mix = 1.0,
            _ => {}
        }
    }
    effect
}

/// Points the lanes that automate send `from` at `to` (`None`: they go).
fn move_lanes(lanes: &mut Vec<AutomationLane>, to: impl Fn(usize) -> Option<usize>) {
    lanes.retain_mut(|lane| match lane.param {
        Param::Send(i) => match to(i) {
            Some(j) => {
                lane.param = Param::Send(j);
                true
            }
            None => false,
        },
        _ => true,
    });
}

/// Takes return `index` out of `project`, and every send to it with it.
fn remove(project: &mut Project, index: usize) {
    project.returns.remove(index);
    let to = |i: usize| match i {
        i if i == index => None,
        i if i > index => Some(i - 1),
        i => Some(i),
    };
    for track in &mut project.tracks {
        track.sends.copy_within(index + 1.., index);
        track.sends[MAX_RETURNS - 1] = 0.0;
        move_lanes(&mut track.automation, to);
        let clips = track.clips.iter_mut().flatten().chain(track.arrangement.iter_mut().map(|a| &mut a.clip));
        for clip in clips {
            match clip {
                Clip::Midi(m) => move_lanes(&mut m.envelopes, to),
                Clip::Audio(a) => move_lanes(&mut a.envelopes, to),
            }
        }
    }
    relabel(project, index);
}

/// The returns from `from` on, named after their letter as it was (one
/// further on), take their new one.
fn relabel(project: &mut Project, from: usize) {
    for (i, ret) in project.returns.iter_mut().enumerate().skip(from) {
        let old = format!("{} ", return_letter(i + 1));
        if let Some(rest) = ret.name.strip_prefix(&old) {
            ret.name = format!("{} {rest}", return_letter(i));
        } else if ret.name == return_letter(i + 1).to_string() {
            ret.name = return_letter(i).to_string();
        }
    }
}

impl PulseApp {
    /// A new return after the others (Crear ▸ Retorno).
    pub fn add_return(&mut self) {
        let index = self.project.returns.len();
        if index >= MAX_RETURNS {
            self.status = Some(format!("Máximo de {MAX_RETURNS} retornos"));
            return;
        }
        self.project.returns.push(new_return(index));
        self.selected_target = Target::Return(index);
        self.sync_all();
    }

    /// Deletes return `index`; the tracks stop sending to it.
    pub fn delete_return(&mut self, index: usize) {
        if index >= self.project.returns.len() {
            return;
        }
        remove(&mut self.project, index);
        self.retarget(|target, param| {
            let send = |param| match param {
                Param::Send(i) if i == index => None,
                Param::Send(i) if i > index => Some(Param::Send(i - 1)),
                other => Some(other),
            };
            match target {
                Target::Return(r) if r == index => None,
                Target::Return(r) if r > index => Some((Target::Return(r - 1), param)),
                Target::Track(_) => send(param).map(|p| (target, p)),
                other => Some((other, param)),
            }
        });
        self.selected_target = match self.selected_target {
            Target::Return(r) if r > index => Target::Return(r - 1),
            Target::Return(r) if r == index => match self.project.returns.len() {
                0 => Target::Master,
                n => Target::Return(r.min(n - 1)),
            },
            other => other,
        };
        self.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_map::{Control, Mapping};

    /// Undoing a new return, which is selected: the selection goes to one that
    /// is there (the panel below used to read the gone one and Pulse closed).
    #[test]
    fn undoing_a_new_return_leaves_a_selection_that_exists() {
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        app.commit_history();
        assert_eq!(app.selected_target, Target::Return(2));
        app.undo();
        assert_eq!(app.project.returns.len(), 2);
        assert_eq!(app.selected_target, Target::Return(1));
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| crate::detail::show(&mut app, ui));
    }

    #[test]
    fn deleting_a_return_takes_its_sends_and_moves_the_others_up() {
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        app.add_return();
        assert_eq!(app.project.returns.len(), 4);
        assert_eq!(app.project.returns[3].name, "D Retorno");
        assert_eq!(app.selected_target, Target::Return(3));
        let t = &mut app.project.tracks[0];
        t.sends[..4].copy_from_slice(&[0.1, 0.2, 0.3, 0.4]);
        let mut lane = AutomationLane::new(Param::Send(2));
        lane.points.push(engine::model::AutomationPoint { beat: 0.0, value: 0.5, curve: 0.0 });
        t.automation.push(lane.clone());
        t.automation.push(AutomationLane { param: Param::Send(1), ..lane.clone() });
        if let Some(Clip::Midi(m)) = &mut t.clips[0] {
            m.envelopes.push(AutomationLane { param: Param::Send(3), ..lane.clone() });
        }
        app.project.midi_map.push(Mapping::new(20, Control::Param { target: Target::Track(0), param: Param::Send(2) }));
        app.project.midi_map.push(Mapping::new(21, Control::Param { target: Target::Return(1), param: Param::Pan }));
        app.project.midi_map.push(Mapping::new(22, Control::Param { target: Target::Return(3), param: Param::Pan }));
        app.selected_target = Target::Return(2);

        app.delete_return(1);
        assert_eq!(app.project.returns.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A Reverb", "B Retorno", "C Retorno"]);
        let t = &app.project.tracks[0];
        assert_eq!(t.sends[..4], [0.1, 0.3, 0.4, 0.0]);
        assert_eq!(t.automation.iter().map(|l| l.param).collect::<Vec<_>>(), [Param::Send(1)], "B's lane went, C's is B's now");
        let Some(Clip::Midi(m)) = &t.clips[0] else { panic!() };
        assert_eq!(m.envelopes.last().map(|l| l.param), Some(Param::Send(2)));
        let controls: Vec<Control> = app.project.midi_map.iter().map(|m| m.control).collect();
        assert_eq!(controls, [Control::Param { target: Target::Track(0), param: Param::Send(1) }, Control::Param { target: Target::Return(2), param: Param::Pan }]);
        assert_eq!(app.selected_target, Target::Return(1), "the selected return, where it is now");

        // Down to none, and up to the most.
        for _ in 0..3 {
            app.delete_return(0);
        }
        assert!(app.project.returns.is_empty());
        assert_eq!(app.selected_target, Target::Master);
        for _ in 0..MAX_RETURNS + 1 {
            app.add_return();
        }
        assert_eq!(app.project.returns.len(), MAX_RETURNS);
        assert_eq!(app.project.returns[11].name, "L Retorno");
    }

    #[test]
    fn projects_with_two_returns_save_as_before() {
        let mut project = Project::demo();
        let text = project.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value["format"].as_u64().unwrap() < 5);
        assert_eq!(value["tracks"][0]["sends"].as_array().unwrap().len(), 2, "what a Pulse with two returns reads");

        project.returns.push(new_return(2));
        project.tracks[0].sends[2] = 0.5;
        let text = project.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], 5, "an older Pulse asks to be updated");
        // Fades: 6 (a Pulse that reads 5 would play the clip without them).
        let mut faded = Project::demo();
        let mut clip = engine::model::AudioClip { hard_start: false, fades: engine::model::Fades::new(1.0, 0.0), name: "a".into(), color: [0; 3], sample: "a.wav".into(), length: 4.0, offset: 0.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
        faded.tracks[0].arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Audio(clip.clone()) });
        assert_eq!(serde_json::from_str::<serde_json::Value>(&faded.to_json().unwrap()).unwrap()["format"], 6);
        clip.fades = engine::model::Fades::NONE;
        faded.tracks[0].arrangement.last_mut().unwrap().clip = Clip::Audio(clip);
        assert!(serde_json::from_str::<serde_json::Value>(&faded.to_json().unwrap()).unwrap()["format"].as_u64().unwrap() < 5);
        assert_eq!(value["tracks"][0]["sends"].as_array().unwrap().len(), 3);
        let again = Project::from_json(&text).unwrap();
        assert_eq!(again.returns.len(), 3);
        assert_eq!(again.tracks[0].sends, project.tracks[0].sends);
        // One return only: also 5 (a Pulse that reads 4 needs two).
        project.returns.truncate(1);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&project.to_json().unwrap()).unwrap()["format"], 5);
        // An EQ Eight, on the master too: 7 (a Pulse that reads 6 doesn't know it).
        let mut eq = Project::demo();
        eq.master.devices.push(engine::model::Device::new(engine::model::Effect::EqEight(Default::default())));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&eq.to_json().unwrap()).unwrap()["format"], 7);
        // A Gate or a Transient, also inside a synth: 8.
        let format = |p: &Project| serde_json::from_str::<serde_json::Value>(&p.to_json().unwrap()).unwrap()["format"].clone();
        let mut gated = Project::demo();
        gated.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::Gate(Default::default())));
        assert_eq!(format(&gated), 8);
        let mut synth = Project::demo();
        let mut wave = engine::model::WaveParams::default();
        wave.fx[0] = Some(engine::model::Device::new(engine::model::Effect::Transient(Default::default())));
        synth.tracks[0].instrument = engine::model::InstrumentKind::Wavetable(wave);
        assert_eq!(format(&synth), 8);
        // A De-esser: 12.
        let mut deessed = Project::demo();
        deessed.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::DeEsser(Default::default())));
        assert_eq!(format(&deessed), 12);
        // A Utility as it was: as before; its lows in mono or a channel flipped, 13.
        let mut utility = Project::demo();
        utility.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::Utility(Default::default())));
        assert_eq!(format(&utility), 2);
        utility.tracks[1].devices[0] = engine::model::Device::new(engine::model::Effect::Utility(engine::model::UtilityParams { bass_mono_hz: 120.0, ..Default::default() }));
        assert_eq!(format(&utility), 13);
        utility.tracks[1].devices[0] = engine::model::Device::new(engine::model::Effect::Utility(engine::model::UtilityParams { flip_right: true, ..Default::default() }));
        assert_eq!(format(&utility), 13);
        // A bus compressor, a multiband or a convolution reverb: 14.
        for effect in [engine::model::Effect::Glue(Default::default()), engine::model::Effect::Multiband(Default::default()), engine::model::Effect::Convolution(Default::default())] {
            let mut glued = Project::demo();
            glued.master.devices.push(engine::model::Device::new(effect));
            assert_eq!(format(&glued), 14);
        }
        // A Vocoder or a voice shift: 17.
        for effect in [engine::model::Effect::Vocoder(Default::default()), engine::model::Effect::VoiceShift(Default::default())] {
            let mut voiced = Project::demo();
            voiced.tracks[1].devices.push(engine::model::Device::new(effect));
            assert_eq!(format(&voiced), 17);
        }
        // A Vocoder playing the MIDI notes: 18 (an older one would play its fixed note).
        let mut following = Project::demo();
        following.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::Vocoder(engine::model::VocoderParams { midi: true, ..Default::default() })));
        assert_eq!(format(&following), 18);
        // A MIDI clip with a controller lane (a recorded bend): 15, and it reads back.
        let mut bent = Project::demo();
        let Some(Clip::Midi(clip)) = bent.tracks[0].clips[0].as_mut() else { panic!("the demo starts with a MIDI clip") };
        clip.controls.push(engine::model::ControlLane { control: engine::model::MidiControl::PitchBend, points: vec![engine::model::AutomationPoint { beat: 1.0, value: -0.5, curve: 0.0 }] });
        assert_eq!(format(&bent), 15);
        assert_eq!(Project::from_json(&bent.to_json().unwrap()).unwrap().tracks[0].clips[0], bent.tracks[0].clips[0]);
        // A clip split in the middle of a fade: each part a piece of its curve, 9.
        let mut cut = Project::demo();
        let clip = engine::model::AudioClip { hard_start: false, fades: engine::model::Fades::new(4.0, 0.0), name: "a".into(), color: [0; 3], sample: "a.wav".into(), length: 8.0, offset: 0.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
        let (left, right) = crate::clip_split::split(&engine::model::ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(clip) }, 2.0, 120.0).unwrap();
        cut.tracks[0].arrangement = vec![left, right];
        assert_eq!(format(&cut), 9);
        let again = Project::from_json(&cut.to_json().unwrap()).unwrap();
        assert_eq!(again.tracks[0].arrangement, cut.tracks[0].arrangement, "the pieces of the curve are kept");
    }

    #[test]
    fn a_third_return_is_heard() {
        use engine::model::{Device, Effect};
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        app.project.returns[2].devices.push(Device::new(Effect::Utility(Default::default())));
        app.project.tracks[0].sends[2] = 1.0;
        app.sync_all();
        let commands = crate::sync::project_commands(&app.project, &mut crate::sync::SampleCache::default(), 48_000.0, &Default::default());
        assert!(commands.iter().any(|c| matches!(c, engine::Command::SetSends { track: 0, sends } if sends[2] == 1.0)));
        assert!(commands.iter().any(|c| matches!(c, engine::Command::ReplaceChain { target: Target::Return(2), .. })));
    }

    /// A return is renamed from its header in the Session, as a track is:
    /// right-click, type the name; its letter stays in front and moves up
    /// when one before it goes (from the 2026-11 audit: it stayed «C Retorno»).
    #[test]
    fn a_return_is_renamed_from_its_header() {
        use eframe::egui::{self, pos2, vec2, Event, Modifiers, PointerButton, Rect};
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
            time += 0.05;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(2400.0, 1000.0))), time: Some(time), events, ..Default::default() };
            ctx.run_ui(input, |ui| crate::session::show(app, ui))
        };
        let texts = |output: &egui::FullOutput| {
            let mut texts = Vec::new();
            fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                    _ => {}
                }
            }
            output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
            texts
        };
        frame(&mut app, vec![]);
        let shown = texts(&frame(&mut app, vec![]));
        let (_, header) = shown.iter().find(|(t, _)| t == "C Retorno").cloned().expect("the new return's header");
        let at = header.center();
        frame(&mut app, vec![Event::PointerMoved(at)]);
        for pressed in [true, false] {
            frame(&mut app, vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE }]);
        }
        let menu = texts(&frame(&mut app, vec![]));
        for item in ["Nombre", "Color", "Eliminar retorno"] {
            assert!(menu.iter().any(|(t, _)| t == item), "{item}: {:?}", menu.iter().map(|t| &t.0).collect::<Vec<_>>());
        }
        ctx.memory_mut(|m| m.request_focus(name_field(2)));
        frame(&mut app, vec![]);
        let backspace = Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, vec![backspace; "Retorno".len()]);
        frame(&mut app, vec![Event::Text("Sala".into())]);
        frame(&mut app, vec![]);
        assert_eq!(app.project.returns[2].name, "C Sala");
        // A return before it goes: it is «B Sala» now.
        app.delete_return(0);
        assert_eq!(app.project.returns.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A Delay", "B Sala"]);
        assert_eq!(bare_name("B Sala", 1), "Sala");
        assert_eq!(bare_name("Bus", 1), "Bus", "a name without its letter is all of it");
    }

    /// The return's menu, renamed, drawn off screen: `PULSE_SHOTS=<dir>`.
    #[test]
    #[ignore]
    fn return_menu_shot() {
        use eframe::egui::{self, pos2, vec2, Event, Modifiers, PointerButton, Rect};
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        set_name(&mut app.project.returns[2], 2, "Sala");
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let size = vec2(1000.0, 720.0);
        let mut canvas = crate::offscreen::Canvas::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
            time += 0.05;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)), time: Some(time), events, ..Default::default() };
            ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| egui::ScrollArea::horizontal().horizontal_scroll_offset(600.0).show(ui, |ui| crate::session::show(app, ui)));
            })
        };
        let mut header = None;
        for _ in 0..3 {
            let output = frame(&mut app, vec![]);
            fn find(shape: &egui::Shape, out: &mut Option<Rect>) {
                match shape {
                    egui::Shape::Text(t) if t.galley.text() == "C Sala" => *out = Some(Rect::from_min_size(t.pos, t.galley.size())),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, out)),
                    _ => {}
                }
            }
            output.shapes.iter().for_each(|s| find(&s.shape, &mut header));
            canvas.skip(&output);
        }
        let at = header.expect("«C Sala» on screen").center();
        for pressed in [true, false] {
            let output = frame(&mut app, vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE }]);
            canvas.skip(&output);
        }
        let output = frame(&mut app, vec![Event::PointerMoved(at)]);
        canvas.skip(&output);
        let output = frame(&mut app, vec![]);
        canvas.draw(&ctx, output, size, 1.0).save_png(&dir.join("retorno-menu.png")).unwrap();
    }

    /// A reverb, a delay or a Space goes into a return all wet, and into a
    /// track as it comes; other effects the same everywhere.
    #[test]
    fn send_effects_go_into_returns_all_wet() {
        use engine::model::{Device, Effect};
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        for effect in Effect::catalog() {
            let on_return = for_target(effect, Target::Return(2));
            let on_track = for_target(effect, Target::Track(0));
            assert_eq!(on_track, effect);
            match on_return {
                Effect::Reverb(p) => assert_eq!(p.mix, 1.0),
                Effect::Delay(p) => assert_eq!(p.mix, 1.0),
                Effect::Convolution(p) => assert_eq!(p.mix, 1.0),
                other => assert_eq!(other, effect),
            }
            if let Effect::Convolution(p) = on_track {
                assert!(p.mix < 0.5, "on a track, some of it");
            }
            app.add_device(Target::Return(2), Device::new(on_return));
        }
        // The OTT comes in at 30 %; its «OTT clásico» is still the full one.
        let ott = Effect::catalog().into_iter().find_map(|e| if let Effect::Multiband(p) = e { Some(p) } else { None }).unwrap();
        assert_eq!(ott.depth, 0.3);
        let presets = engine::model::MultibandParams::presets();
        assert_eq!(presets.iter().find(|(n, _)| *n == "OTT · OTT clásico").unwrap().1.depth, 1.0);
        assert_eq!(presets.iter().find(|(n, _)| *n == "OTT · OTT suave").unwrap().1.depth, 0.3);
    }
}
