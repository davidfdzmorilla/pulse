//! The words sung in a voice: lines with their words and when each is said,
//! read from the audio by Whisper (see `stems::whisper` and `lyrics_ui`) and
//! corrected by hand.
//!
//! The lyrics live in the project, one per audio file, with times in the
//! file's seconds: every clip that plays the file shows them where they are
//! sung, however it is cut, moved, looped or warped.

use crate::app::{ClipRef, PulseApp};
use engine::model::{AudioClip, Clip};
use serde::{Deserialize, Serialize};

/// A word and when it is said (seconds of the file).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A line (a phrase), from `start` to `end` seconds of the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub start: f64,
    pub end: f64,
    pub words: Vec<LyricWord>,
}

impl LyricLine {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// The line with `text` instead: the same words keep their times; other
    /// words share the line's time by their length.
    pub fn set_text(&mut self, text: &str) {
        let new: Vec<&str> = text.split_whitespace().collect();
        if new.len() == self.words.len() {
            for (word, text) in self.words.iter_mut().zip(new) {
                word.text = text.to_string();
            }
        } else {
            self.words = words_of(text, self.start, self.end);
        }
    }
}

/// `text`'s words over `start`–`end`, each with a share of the time by its length.
pub fn words_of(text: &str, start: f64, end: f64) -> Vec<LyricWord> {
    stems::whisper::spread_words(text, start, end).into_iter().map(|w| LyricWord { start: ms(w.start), end: ms(w.end), text: w.text }).collect()
}

/// `seconds` to the millisecond: what is kept (and saved) of a time.
pub fn ms(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

/// The lyrics of an audio file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    /// The file (saved relative to the project, like the clips' own).
    pub sample: String,
    /// The language they were read in ("es", "en"…), if known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    /// All the text, a line each.
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.lines.iter().map(LyricLine::text).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
    }

    /// Puts `lines` (read from `from` to `to` seconds) in place of the ones
    /// that start there.
    pub fn replace(&mut self, from: f64, to: f64, lines: Vec<LyricLine>) {
        self.lines.retain(|l| l.start < from - 1e-6 || l.start >= to);
        self.lines.extend(lines);
        self.lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    }

    /// Word `w` of line `l` as `text`: empty takes it away (and the line, if
    /// it was its last); several words share its time.
    pub fn set_word(&mut self, l: usize, w: usize, text: &str) {
        let Some(line) = self.lines.get_mut(l) else { return };
        let Some(word) = line.words.get(w) else { return };
        let new = match text.split_whitespace().count() {
            1 => vec![LyricWord { start: word.start, end: word.end, text: text.trim().to_string() }],
            _ => words_of(text, word.start, word.end),
        };
        line.words.splice(w..w + 1, new);
        if line.words.is_empty() {
            self.lines.remove(l);
        }
    }

    /// Line `l` as `text` (see [`LyricLine::set_text`]); empty takes it away.
    pub fn set_line(&mut self, l: usize, text: &str) {
        let Some(line) = self.lines.get_mut(l) else { return };
        line.set_text(text);
        if line.words.is_empty() {
            self.lines.remove(l);
        }
    }
}

/// Plain text, a line each, ending in a new line.
pub fn to_txt(lines: &[(f64, String)]) -> String {
    lines.iter().map(|(_, text)| format!("{text}\n")).collect()
}

/// An LRC file: each line with the time it starts, `[mm:ss.xx]`, after a title.
pub fn to_lrc(title: &str, language: &str, lines: &[(f64, String)]) -> String {
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(&format!("[ti:{title}]\n"));
    }
    if !language.is_empty() {
        out.push_str(&format!("[la:{language}]\n"));
    }
    out.push_str("[re:Pulse]\n");
    for (time, text) in lines {
        out.push_str(&format!("[{}]{text}\n", lrc_time(*time)));
    }
    out
}

/// `seconds` as LRC writes it: minutes, seconds and hundredths.
pub fn lrc_time(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).round() as u64;
    format!("{:02}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

/// The part of its file a clip plays (seconds), as far as the file goes.
pub fn region(audio: &AudioClip, tempo: f64, duration: f64) -> (f64, f64) {
    let from = audio.offset.max(0.0);
    let to = audio.source_seconds(audio.length, tempo).min(duration);
    (from, to.max(from))
}

impl PulseApp {
    /// The lyrics of an audio file, if it has some.
    pub fn lyrics_of(&self, sample: &str) -> Option<&Lyrics> {
        self.project.lyrics.iter().find(|l| l.sample == sample)
    }

    /// The lyrics of the file `clip` plays.
    pub fn clip_lyrics(&self, clip: ClipRef) -> Option<&Lyrics> {
        match self.clip(clip) {
            Some(Clip::Audio(audio)) => self.lyrics_of(&audio.sample),
            _ => None,
        }
    }

    /// Changes the lyrics of `sample` (made if it has none, taken away if left
    /// without lines). One step to undo.
    pub fn edit_lyrics(&mut self, sample: &str, change: impl FnOnce(&mut Lyrics)) {
        let i = match self.project.lyrics.iter().position(|l| l.sample == sample) {
            Some(i) => i,
            None => {
                self.project.lyrics.push(Lyrics { sample: sample.to_string(), language: String::new(), lines: Vec::new() });
                self.project.lyrics.len() - 1
            }
        };
        change(&mut self.project.lyrics[i]);
        if self.project.lyrics[i].lines.is_empty() {
            self.project.lyrics.remove(i);
        }
    }

    /// The lines `clip` plays, with when each starts: in the song for a clip
    /// on the Arrangement, from the clip's start for one in the Session.
    pub fn clip_lyric_lines(&self, clip: ClipRef) -> Vec<(f64, String)> {
        let Some(Clip::Audio(audio)) = self.clip(clip) else { return Vec::new() };
        let Some(lyrics) = self.lyrics_of(&audio.sample) else { return Vec::new() };
        let tempo = self.project.tempo.max(1.0);
        let start = match clip {
            ClipRef::Arrangement { track, index } => self.project.tracks.get(track).and_then(|t| t.arrangement.get(index)).map_or(0.0, |p| p.start),
            ClipRef::Session { .. } => 0.0,
        };
        lyrics
            .lines
            .iter()
            .filter_map(|line| {
                let beat = audio.beats_until(line.start, tempo);
                // A line that starts just before the clip (its first word cut) still counts.
                let ends = audio.beats_until(line.end, tempo);
                (ends > 0.0 && beat < audio.length).then(|| ((start + beat.max(0.0)) * 60.0 / tempo, line.text()))
            })
            .filter(|(_, text)| !text.is_empty())
            .collect()
    }

    /// The text of `clip`'s lyrics, a line each.
    pub fn clip_lyrics_text(&self, clip: ClipRef) -> String {
        to_txt(&self.clip_lyric_lines(clip)).trim_end().to_string()
    }

    /// Writes `clip`'s lyrics to `path`, as LRC if it ends in .lrc, as text if not.
    pub fn write_lyrics(&mut self, clip: ClipRef, path: &std::path::Path) {
        let lines = self.clip_lyric_lines(clip);
        let name = self.clip(clip).map(|c| c.name().to_string()).unwrap_or_default();
        let language = self.clip_lyrics(clip).map(|l| l.language.clone()).unwrap_or_default();
        let lrc = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrc"));
        let text = if lrc { to_lrc(&name, &language, &lines) } else { to_txt(&lines) };
        let file = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        self.status = Some(match crate::store::write_atomic(path, text.as_bytes()) {
            Ok(()) => format!("Letra guardada en «{file}»"),
            Err(e) => format!("No se pudo guardar la letra: {e}"),
        });
    }

    /// Asks where, and writes `clip`'s lyrics there as `.txt` or `.lrc`.
    pub fn export_lyrics(&mut self, clip: ClipRef, lrc: bool) {
        let name = self.clip(clip).map(|c| c.name().to_string()).unwrap_or_default();
        let (label, extension) = if lrc { ("LRC (letra con tiempos)", "lrc") } else { ("Texto", "txt") };
        let dialog = rfd::FileDialog::new().add_filter(label, &[extension]).set_file_name(format!("{name}.{extension}"));
        let Some(path) = crate::files::save_file(dialog) else { return };
        self.write_lyrics(clip, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start: f64, end: f64, text: &str) -> LyricLine {
        LyricLine { start, end, words: words_of(text, start, end) }
    }

    #[test]
    fn lrc_times_are_minutes_seconds_and_hundredths() {
        assert_eq!(lrc_time(0.0), "00:00.00");
        assert_eq!(lrc_time(12.345), "00:12.35");
        assert_eq!(lrc_time(75.5), "01:15.50");
        assert_eq!(lrc_time(3599.999), "60:00.00");
        assert_eq!(lrc_time(-1.0), "00:00.00");
        let lrc = to_lrc("Voz", "es", &[(1.0, "Hola mundo".into()), (62.25, "Adiós".into())]);
        assert_eq!(lrc, "[ti:Voz]\n[la:es]\n[re:Pulse]\n[00:01.00]Hola mundo\n[01:02.25]Adiós\n");
        assert_eq!(to_txt(&[(1.0, "Hola mundo".into()), (62.25, "Adiós".into())]), "Hola mundo\nAdiós\n");
    }

    #[test]
    fn words_are_corrected_keeping_their_time() {
        let mut lyrics = Lyrics { sample: "/v.wav".into(), language: "es".into(), lines: vec![line(1.0, 3.0, "mi corazon late"), line(4.0, 5.0, "eh")] };
        let before = lyrics.lines[0].words[1].clone();
        lyrics.set_word(0, 1, "corazón");
        let after = &lyrics.lines[0].words[1];
        assert_eq!((after.text.as_str(), after.start, after.end), ("corazón", before.start, before.end));
        // Two words in one's place share its time.
        lyrics.set_word(0, 2, "late fuerte");
        assert_eq!(lyrics.lines[0].text(), "mi corazón late fuerte");
        assert!((lyrics.lines[0].words[3].end - before.end).abs() > 0.0 && lyrics.lines[0].words[3].end <= 3.0);
        // Emptied, a word goes; a line without words too.
        lyrics.set_word(1, 0, "");
        assert_eq!(lyrics.lines.len(), 1);
        // A whole line: same number of words keeps the times.
        let times: Vec<f64> = lyrics.lines[0].words.iter().map(|w| w.start).collect();
        lyrics.set_line(0, "tu corazón late lento");
        assert_eq!(lyrics.lines[0].words.iter().map(|w| w.start).collect::<Vec<_>>(), times);
        lyrics.set_line(0, "otra cosa");
        assert_eq!(lyrics.lines[0].words.len(), 2);
        assert_eq!(lyrics.text(), "otra cosa");
    }

    #[test]
    fn a_new_reading_replaces_the_lines_it_covers() {
        let mut lyrics = Lyrics { sample: "/v.wav".into(), language: String::new(), lines: vec![line(1.0, 2.0, "a"), line(5.0, 6.0, "b"), line(9.0, 10.0, "c")] };
        lyrics.replace(4.0, 8.0, vec![line(4.5, 5.5, "nueva"), line(6.0, 7.0, "otra")]);
        assert_eq!(lyrics.text(), "a\nnueva\notra\nc");
    }

    /// The lines a clip plays, at their time in the song.
    #[test]
    fn a_clip_gives_its_lines_at_their_time_in_the_song() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.project.tempo = 120.0;
        let t = app.add_audio_track().unwrap();
        let audio = AudioClip { hard_start: false, fades: Default::default(), name: "Voz".into(), color: [1, 2, 3], sample: "/x/voz.wav".into(), length: 8.0, offset: 2.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, warp_markers: Vec::new(), takes: Vec::new(), key: None, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), muted: false };
        app.project.tracks[t].arrangement.push(engine::model::ArrangementClip { start: 16.0, length: 8.0, clip: Clip::Audio(audio) });
        app.edit_lyrics("/x/voz.wav", |l| l.lines = vec![line(0.5, 1.0, "antes"), line(2.5, 4.0, "dentro uno"), line(5.0, 5.8, "dentro dos"), line(7.0, 8.0, "después")]);
        let clip = ClipRef::Arrangement { track: t, index: 0 };
        // Beat 16 at 120 BPM is second 8; the clip starts 2 s into the file.
        assert_eq!(app.clip_lyric_lines(clip), vec![(8.5, "dentro uno".to_string()), (11.0, "dentro dos".to_string())]);
        assert_eq!(app.clip_lyrics_text(clip), "dentro uno\ndentro dos");
        // Written out.
        let dir = std::env::temp_dir().join(format!("pulse-lyrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        app.write_lyrics(clip, &dir.join("voz.lrc"));
        assert_eq!(std::fs::read_to_string(dir.join("voz.lrc")).unwrap(), "[ti:Voz]\n[re:Pulse]\n[00:08.50]dentro uno\n[00:11.00]dentro dos\n");
        crate::script::answer_dialog(vec![dir.join("voz.txt")]);
        app.export_lyrics(clip, false);
        assert_eq!(std::fs::read_to_string(dir.join("voz.txt")).unwrap(), "dentro uno\ndentro dos\n");
        // Every line taken away: no lyrics left for the file.
        app.edit_lyrics("/x/voz.wav", |l| l.lines.clear());
        assert!(app.project.lyrics.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saved with the project (its file relative to the project's folder, as
    /// the clips'), and opened again the same; only then is the file format 16.
    #[test]
    fn lyrics_are_saved_and_opened_with_the_project() {
        let dir = std::env::temp_dir().join(format!("pulse-lyrics-save-{}", std::process::id()));
        let file = dir.join("Canción/Canción.pulse");
        let voice = dir.join("Canción/Audio/voz.wav").to_string_lossy().into_owned();
        let mut project = crate::project::Project::demo();
        project.save(&file).unwrap();
        assert!(!std::fs::read_to_string(&file).unwrap().contains("\"format\": 16"), "no lyrics, no new format");
        let mut lyrics = Lyrics { sample: voice.clone(), language: "es".into(), lines: vec![line(1.0, 2.5, "Bailando bajo la luna"), line(3.0, 4.0, "contigo")] };
        lyrics.set_word(0, 0, "Bailamos");
        project.lyrics.push(lyrics.clone());
        project.save(&file).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"format\": 16"), "a Pulse that would lose them refuses the file");
        assert!(text.contains("\"sample\": \"Audio/voz.wav\""), "relative, like the clips");
        let opened = crate::project::Project::load(&file).unwrap();
        assert_eq!(opened.lyrics, vec![lyrics]);
        assert_eq!(opened.lyrics[0].text(), "Bailamos bajo la luna\ncontigo");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
