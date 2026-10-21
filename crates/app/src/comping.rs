//! Comping: building the take of a loop recording out of pieces of its takes.
//!
//! Every take of a loop recording is a file as long as the loop, so a piece of
//! the clip can play any of them from the same point. Choosing take `k` over a
//! stretch of the timeline splits the clips there (see `clip_split`) and has the
//! pieces in between play take `k`; neighbouring pieces left playing the same take
//! from where the other ends become one clip again.

use crate::app::{ClipRef, PulseApp};
use crate::clip_split::{split, MIN_PART};
use engine::model::{ArrangementClip, Clip};

/// The audio clip of a recording's take `k` of a loop, if `placed` has one.
fn take(placed: &ArrangementClip, k: usize) -> Option<&str> {
    match &placed.clip {
        Clip::Audio(audio) => audio.takes.get(k).map(String::as_str),
        Clip::Midi(_) => None,
    }
}

/// `left` and `right` as one clip, if they are pieces of the same take that
/// follow on from each other (what splitting the result would give again).
fn merged(left: &ArrangementClip, right: &ArrangementClip, tempo: f64) -> Option<ArrangementClip> {
    let (Clip::Audio(l), Clip::Audio(r)) = (&left.clip, &right.clip) else { return None };
    if l.takes.is_empty() || l.takes != r.takes || l.sample != r.sample || (left.end() - right.start).abs() > 1e-6 {
        return None;
    }
    // Looping pieces and clip envelopes don't join back by just lengthening.
    if left.length > l.length + 1e-9 || right.length > r.length + 1e-9 || !l.envelopes.is_empty() || !r.envelopes.is_empty() {
        return None;
    }
    let mut whole = left.clone();
    whole.length = left.length + right.length;
    if let Clip::Audio(audio) = &mut whole.clip {
        audio.length = whole.length;
    }
    let (_, mut again) = split(&whole, right.start, tempo)?;
    let Clip::Audio(a) = &mut again.clip else { return None };
    if (a.offset - r.offset).abs() > 1e-6 {
        return None;
    }
    a.offset = r.offset;
    a.name.clone_from(&r.name);
    again.length = right.length;
    (again == *right).then_some(whole)
}

/// Has take `k` play from beat `a` to `b` on the clips of `lane` that have it.
/// Returns the indices of the pieces that now play it.
pub fn comp(lane: &mut Vec<ArrangementClip>, k: usize, a: f64, b: f64, tempo: f64) -> Vec<usize> {
    let (a, b) = (a.min(b), a.max(b));
    let mut clips: Vec<(ArrangementClip, bool)> = Vec::new();
    for placed in lane.drain(..) {
        let wanted = take(&placed, k).map(str::to_string).filter(|_| placed.start < b - MIN_PART && placed.end() > a + MIN_PART);
        let Some(wanted) = wanted else {
            clips.push((placed, false));
            continue;
        };
        let mut piece = placed;
        if let Some((before, rest)) = split(&piece, a, tempo) {
            clips.push((before, false));
            piece = rest;
        }
        let mut after = None;
        if let Some((rest, later)) = split(&piece, b, tempo) {
            piece = rest;
            after = Some(later);
        }
        if let Clip::Audio(audio) = &mut piece.clip {
            audio.sample = wanted;
            audio.name = format!("Toma {}", k + 1);
        }
        clips.push((piece, true));
        clips.extend(after.map(|later| (later, false)));
    }
    // Pieces back together where they play on from each other.
    'merging: loop {
        for i in 0..clips.len() {
            for j in 0..clips.len() {
                if i == j {
                    continue;
                }
                if let Some(whole) = merged(&clips[i].0, &clips[j].0, tempo) {
                    let comped = clips[i].1 || clips[j].1;
                    clips[i] = (whole, comped);
                    clips.remove(j);
                    continue 'merging;
                }
            }
        }
        break;
    }
    let comped = clips.iter().enumerate().filter(|(_, (_, comped))| *comped).map(|(i, _)| i).collect();
    lane.extend(clips.into_iter().map(|(clip, _)| clip));
    comped
}

/// How many take lanes a track shows: the most takes any of its clips has.
pub fn take_count(app: &PulseApp, track: usize) -> usize {
    app.project.tracks.get(track).map_or(0, |t| {
        t.arrangement
            .iter()
            .map(|c| match &c.clip {
                Clip::Audio(audio) => audio.takes.len(),
                Clip::Midi(_) => 0,
            })
            .max()
            .unwrap_or(0)
    })
}

impl PulseApp {
    /// Shows or hides a track's take lanes.
    pub fn toggle_take_lanes(&mut self, track: usize) {
        if !self.arrangement.takes.remove(&track) {
            self.arrangement.takes.insert(track);
        }
    }

    /// Take `k` plays from beat `a` to `b` on a track (a drag in its take lane).
    pub fn comp_takes(&mut self, track: usize, k: usize, a: f64, b: f64) {
        let tempo = self.project.tempo;
        let Some(t) = self.project.tracks.get_mut(track) else { return };
        let comped = comp(&mut t.arrangement, k, a, b, tempo);
        if comped.is_empty() {
            return;
        }
        self.sync_arrangement(track);
        self.set_clip_selection(comped.into_iter().map(|index| ClipRef::Arrangement { track, index }).collect());
        self.status = Some(format!("Suena la toma {} en ese trozo", k + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::{AudioClip, WarpMode};

    /// A loop recording from beat 4 to 8 with three takes, the last one playing.
    fn recording() -> ArrangementClip {
        ArrangementClip {
            start: 4.0,
            length: 4.0,
            clip: Clip::Audio(AudioClip {
                name: "Toma 3".into(),
                color: [200, 100, 50],
                sample: "c.wav".into(),
                length: 4.0,
                offset: 0.0,
                warp: WarpMode::Stretch,
                original_bpm: 120.0,
                gain_db: 0.0,
                transpose: 0.0,
                envelopes: Vec::new(),
                warp_markers: Vec::new(),
                takes: vec!["a.wav".into(), "b.wav".into(), "c.wav".into()],
                key: None, muted: false,
            }),
        }
    }

    /// (start, length, file, offset) of each clip, in timeline order.
    fn pieces(lane: &[ArrangementClip]) -> Vec<(f64, f64, String, f64)> {
        let mut pieces: Vec<_> = lane
            .iter()
            .map(|c| {
                let Clip::Audio(a) = &c.clip else { panic!() };
                (c.start, c.length, a.sample.clone(), (a.offset * 1e6).round() / 1e6)
            })
            .collect();
        pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
        pieces
    }

    #[test]
    fn a_stretch_of_another_take_plays_from_the_same_point() {
        let mut lane = vec![recording()];
        let comped = comp(&mut lane, 0, 5.0, 6.0, 120.0);
        assert_eq!(comped.len(), 1);
        assert_eq!((lane[comped[0]].start, lane[comped[0]].clip.name()), (5.0, "Toma 1"));
        // A beat at 120 BPM is half a second into each file.
        assert_eq!(
            pieces(&lane),
            vec![(4.0, 1.0, "c.wav".into(), 0.0), (5.0, 1.0, "a.wav".into(), 0.5), (6.0, 2.0, "c.wav".into(), 1.0)]
        );
        // Dragged the other way, over two pieces: they become one.
        comp(&mut lane, 1, 7.0, 5.5, 120.0);
        assert_eq!(
            pieces(&lane),
            vec![(4.0, 1.0, "c.wav".into(), 0.0), (5.0, 0.5, "a.wav".into(), 0.5), (5.5, 1.5, "b.wav".into(), 0.75), (7.0, 1.0, "c.wav".into(), 1.5)]
        );
    }

    #[test]
    fn choosing_back_the_take_leaves_one_clip_again() {
        let mut lane = vec![recording()];
        comp(&mut lane, 0, 5.0, 6.0, 120.0);
        comp(&mut lane, 2, 5.0, 6.0, 120.0);
        assert_eq!(lane, vec![recording()]);
        // The whole clip, another take: still one clip.
        comp(&mut lane, 1, 0.0, 16.0, 120.0);
        assert_eq!(pieces(&lane), vec![(4.0, 4.0, "b.wav".into(), 0.0)]);
    }

    #[test]
    fn comping_in_the_app_selects_the_piece_and_undoes() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let track = app.add_audio_track().unwrap();
        app.project.tracks[track].arrangement = vec![recording()];
        app.project.tempo = 120.0;
        app.commit_history();
        app.comp_takes(track, 0, 5.0, 6.0);
        let lane = &app.project.tracks[track].arrangement;
        assert_eq!(lane.len(), 3);
        let selected = app.clip_selection();
        assert_eq!(selected.len(), 1);
        let ClipRef::Arrangement { index, .. } = selected[0] else { panic!() };
        assert_eq!((lane[index].start, lane[index].clip.name()), (5.0, "Toma 1"));
        app.commit_history();
        app.undo();
        assert_eq!(app.project.tracks[track].arrangement, vec![recording()]);
    }

    #[test]
    fn clips_without_that_take_stay_as_they_are() {
        let mut other = recording();
        other.start = 8.0;
        if let Clip::Audio(a) = &mut other.clip {
            a.takes.truncate(1);
        }
        let mut lane = vec![recording(), other.clone()];
        comp(&mut lane, 2, 0.0, 12.0, 120.0);
        assert_eq!(lane[1], other);
        assert!(comp(&mut lane, 5, 0.0, 12.0, 120.0).is_empty());
    }
}
