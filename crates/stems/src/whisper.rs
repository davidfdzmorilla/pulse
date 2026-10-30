//! Speech to text with Whisper (OpenAI, MIT; see `models/WHISPER_NOTICE`):
//! the words sung or spoken in a recording, in Spanish, English or any of the
//! languages it knows, with when each phrase is said.
//!
//! The model is too big to build into the app: it is downloaded the first time
//! (see the app's `lyrics_ui`) into a folder with [`FILES`]. It is
//! `onnx-community/whisper-small` (int8), an encoder and a decoder with a cache
//! of what it has already read. What goes in and what comes out are worked out
//! here as OpenAI's `whisper` does them:
//!
//! - the audio at 16 kHz, in windows of 30 s, as an 80-band log-mel
//!   spectrogram ([`log_mel`], `whisper.audio.log_mel_spectrogram`);
//! - greedy decoding with timestamps (`whisper.decoding`, its
//!   `ApplyTimestampRules`, `SuppressTokens` and `SuppressBlank`), each window
//!   starting where the last whole phrase of the one before ended;
//! - the byte-level tokenizer, back to text ([`Tokenizer`]).
//!
//! Phrases get their times from the model; words get them by sharing out
//! their phrase's time by their length (the model gives no word times).

use crate::{environment, resample};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::path::Path;
use std::sync::Arc;

/// The rate the model works at.
pub const RATE: f32 = 16_000.0;
const N_FFT: usize = 400;
const HOP: usize = 160;
const BINS: usize = N_FFT / 2 + 1;
pub const MELS: usize = 80;
/// A window: 30 s, in samples and in spectrogram frames.
const CHUNK: usize = 480_000;
pub const FRAMES: usize = 3000;
/// Samples per timestamp step (20 ms).
const STEP: usize = 320;

/// The model's files, in the folder given to [`Whisper::load`].
pub const FILES: [&str; 3] = ["encoder_model_quantized.onnx", "decoder_model_merged_quantized.onnx", "tokenizer.json"];

// The multilingual vocabulary's special tokens.
const EOT: u32 = 50257;
const SOT: u32 = 50258;
const FIRST_LANGUAGE: u32 = 50259;
const TRANSLATE: u32 = 50358;
const TRANSCRIBE: u32 = 50359;
const NO_SPEECH: u32 = 50362;
const NO_TIMESTAMPS: u32 = 50363;
const TIMESTAMP_BEGIN: u32 = 50364;
/// The first timestamp may be at most 1 s in.
const MAX_INITIAL_TIMESTAMP: u32 = 50;
/// Tokens a window may have (half the model's context).
const MAX_TOKENS: usize = 224;
/// Symbols and markers the model is kept from writing (generation_config.json).
const SUPPRESS: [u32; 88] = [
    1, 2, 7, 8, 9, 10, 14, 25, 26, 27, 28, 29, 31, 58, 59, 60, 61, 62, 63, 90, 91, 92, 93, 359, 503, 522, 542, 873, 893, 902, 918, 922, 931, 1350, 1853, 1982, 2460, 2627,
    3246, 3253, 3268, 3536, 3846, 3961, 4183, 4667, 6585, 6647, 7273, 9061, 9383, 10428, 10929, 11938, 12033, 12331, 12562, 13793, 14157, 14635, 15265, 15618, 16553, 16604,
    18362, 18956, 20075, 21675, 22520, 26130, 26161, 26435, 28279, 29464, 31650, 32302, 32470, 36865, 42863, 47425, 49870, 50254, 50258, 50358, 50359, 50360, 50361, 50362,
];
/// A window the model thinks has no speech, and isn't sure of its words, is left out.
const NO_SPEECH_THRESHOLD: f32 = 0.6;
const LOGPROB_THRESHOLD: f32 = -1.0;
/// Below this loudness (RMS) a window is taken as silence and not read at all.
const SILENCE: f32 = 0.001;

/// A word and when it is said (seconds of the audio given).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A phrase: from `start` to `end` seconds, its text and its words.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// The language it was read in ("es", "en"…).
    pub language: String,
    pub segments: Vec<Segment>,
}

/// The `words` of a phrase from `start` to `end`, each with a share of its
/// time by its length (a space counts as a letter).
pub fn spread_words(text: &str, start: f64, end: f64) -> Vec<Word> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let total: usize = words.iter().map(|w| w.chars().count() + 1).sum();
    let span = (end - start).max(0.0);
    let mut at = 0usize;
    words
        .iter()
        .map(|w| {
            let from = start + span * at as f64 / total.max(1) as f64;
            at += w.chars().count() + 1;
            let to = start + span * (at - 1) as f64 / total.max(1) as f64;
            Word { start: from, end: to.max(from), text: w.to_string() }
        })
        .collect()
}

/// The 80-band log-mel spectrogram of up to 30 s of audio at 16 kHz (padded
/// with silence to 30 s): `MELS` rows of `FRAMES`, row by row, on Whisper's
/// scale (log10, at most 8 below the loudest, then (x + 4) / 4).
pub struct Mel {
    filters: Vec<[f32; BINS]>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl Default for Mel {
    fn default() -> Self {
        Self::new()
    }
}

impl Mel {
    pub fn new() -> Self {
        // A periodic Hann window.
        let window = (0..N_FFT).map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()).map(|w| w as f32).collect();
        Self { filters: mel_filters(), window, fft: FftPlanner::new().plan_fft_forward(N_FFT) }
    }

    pub fn log_mel(&self, audio: &[f32]) -> Vec<f32> {
        let mut padded = vec![0.0f32; CHUNK];
        let n = audio.len().min(CHUNK);
        padded[..n].copy_from_slice(&audio[..n]);
        // Centred frames: reflected at the edges (numpy's "reflect").
        let half = N_FFT / 2;
        let at = |i: isize| -> f32 {
            let len = CHUNK as isize;
            let j = if i < 0 { -i } else if i >= len { 2 * (len - 1) - i } else { i };
            padded[j as usize]
        };
        let mut mel = vec![0.0f32; MELS * FRAMES];
        let mut buffer = vec![Complex32::default(); N_FFT];
        let mut scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
        let mut power = [0.0f32; BINS];
        for frame in 0..FRAMES {
            let start = (frame * HOP) as isize - half as isize;
            for (k, b) in buffer.iter_mut().enumerate() {
                *b = Complex32::new(at(start + k as isize) * self.window[k], 0.0);
            }
            self.fft.process_with_scratch(&mut buffer, &mut scratch);
            for (p, b) in power.iter_mut().zip(&buffer) {
                *p = b.norm_sqr();
            }
            for (m, filter) in self.filters.iter().enumerate() {
                let energy: f32 = filter.iter().zip(&power).map(|(f, p)| f * p).sum();
                mel[m * FRAMES + frame] = energy.max(1e-10).log10();
            }
        }
        let max = mel.iter().copied().fold(f32::MIN, f32::max);
        for v in &mut mel {
            *v = (v.max(max - 8.0) + 4.0) / 4.0;
        }
        mel
    }
}

/// The spectrogram of `audio` (see [`Mel`]).
pub fn log_mel(audio: &[f32]) -> Vec<f32> {
    Mel::new().log_mel(audio)
}

/// Slaney's mel scale (librosa's default): straight below 1 kHz, logarithmic above.
fn hz_to_mel(hz: f64) -> f64 {
    let (f_sp, min_log_hz, min_log_mel, step) = (200.0 / 3.0, 1000.0, 15.0, 6.4f64.ln() / 27.0);
    if hz >= min_log_hz { min_log_mel + (hz / min_log_hz).ln() / step } else { hz / f_sp }
}

fn mel_to_hz(mel: f64) -> f64 {
    let (f_sp, min_log_hz, min_log_mel, step) = (200.0 / 3.0, 1000.0, 15.0, 6.4f64.ln() / 27.0);
    if mel >= min_log_mel { min_log_hz * (step * (mel - min_log_mel)).exp() } else { mel * f_sp }
}

/// Triangles between mel-spaced points from 0 to 8 kHz, each scaled to the
/// same area (librosa.filters.mel, norm="slaney").
fn mel_filters() -> Vec<[f32; BINS]> {
    let top = RATE as f64 / 2.0;
    let points: Vec<f64> = (0..MELS + 2).map(|i| mel_to_hz(hz_to_mel(0.0) + (hz_to_mel(top) - hz_to_mel(0.0)) * i as f64 / (MELS + 1) as f64)).collect();
    (0..MELS)
        .map(|m| {
            let (low, centre, high) = (points[m], points[m + 1], points[m + 2]);
            let scale = 2.0 / (high - low);
            std::array::from_fn(|k| {
                let hz = k as f64 * top / (BINS - 1) as f64;
                let down = (hz - low) / (centre - low);
                let up = (high - hz) / (high - centre);
                (down.min(up).max(0.0) * scale) as f32
            })
        })
        .collect()
}

/// Token numbers back to text: GPT-2's byte-level tokens, each character one
/// byte, for the text ones; special tokens and timestamps are left out.
pub struct Tokenizer {
    tokens: Vec<String>,
}

/// The character each byte is written as in the vocabulary (GPT-2's
/// `bytes_to_unicode`), backwards: char → byte.
fn byte_of(c: char) -> Option<u8> {
    let c = c as u32;
    let printable = |b: u32| (b'!' as u32..=b'~' as u32).contains(&b) || (0xA1..=0xAC).contains(&b) || (0xAE..=0xFF).contains(&b);
    if c < 256 && printable(c) {
        return Some(c as u8);
    }
    // The rest of the bytes, in order, from U+0100.
    let n = c.checked_sub(256)?;
    (0u32..256).filter(|&b| !printable(b)).nth(n as usize).map(|b| b as u8)
}

impl Tokenizer {
    /// From the tokens by number.
    pub fn new(tokens: Vec<String>) -> Self {
        Self { tokens }
    }

    /// From Hugging Face's tokenizer.json: its vocabulary and added tokens.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("tokenizer.json no válido: {e}"))?;
        let mut tokens: Vec<String> = Vec::new();
        let mut put = |id: usize, token: &str| {
            if tokens.len() <= id {
                tokens.resize(id + 1, String::new());
            }
            tokens[id] = token.to_string();
        };
        let vocab = value.pointer("/model/vocab").and_then(|v| v.as_object()).ok_or("tokenizer.json sin vocabulario")?;
        for (token, id) in vocab {
            put(id.as_u64().ok_or("tokenizer.json no válido")? as usize, token);
        }
        for added in value.get("added_tokens").and_then(|v| v.as_array()).into_iter().flatten() {
            if let (Some(id), Some(content)) = (added.get("id").and_then(|v| v.as_u64()), added.get("content").and_then(|v| v.as_str())) {
                put(id as usize, content);
            }
        }
        Ok(Self { tokens })
    }

    /// The text of the text tokens in `ids`.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for &id in ids.iter().filter(|&&id| id < EOT) {
            if let Some(token) = self.tokens.get(id as usize) {
                bytes.extend(token.chars().filter_map(byte_of));
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// The token of language `code` ("es"), if the model knows it.
    pub fn language(&self, code: &str) -> Option<u32> {
        let wanted = format!("<|{code}|>");
        (FIRST_LANGUAGE..TRANSLATE).find(|&id| self.tokens.get(id as usize).is_some_and(|t| *t == wanted))
    }

    /// The code of a language token ("<|es|>" → "es").
    pub fn language_code(&self, id: u32) -> String {
        self.tokens.get(id as usize).map(|t| t.trim_start_matches("<|").trim_end_matches("|>").to_string()).unwrap_or_default()
    }
}

/// What reading a window gave: its tokens, how sure the model was of them, and
/// how likely it found there was no speech.
struct Decoded {
    tokens: Vec<u32>,
    avg_logprob: f32,
    no_speech: f32,
}

pub struct Whisper {
    _environment: ort::environment::Environment,
    encoder: ort::session::Session,
    decoder: ort::session::Session,
    tokenizer: Tokenizer,
    mel: Mel,
    layers: usize,
    heads: usize,
    head_size: usize,
}

const CANCELLED: &str = "Cancelado";

impl Whisper {
    /// Loads the model from `dir` (with [`FILES`]).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).clamp(1, 8));
        let tokenizer = std::fs::read_to_string(dir.join(FILES[2])).map_err(|e| format!("No se pudo leer el tokenizador: {e}"))?;
        let tokenizer = Tokenizer::from_json(&tokenizer)?;
        let build = || -> ort::Result<(ort::environment::Environment, ort::session::Session, ort::session::Session)> {
            let environment = environment()?;
            let session = |file: &str| -> ort::Result<ort::session::Session> {
                ort::session::Session::builder(&environment)?
                    .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)?
                    .with_intra_threads(threads)
                    .unwrap_or_else(|e| e.recover())
                    .commit_from_file(dir.join(file))
            };
            let (encoder, decoder) = (session(FILES[0])?, session(FILES[1])?);
            Ok((environment, encoder, decoder))
        };
        let (_environment, encoder, decoder) = build().map_err(|e| format!("No se pudo cargar el modelo de letras: {e}"))?;
        // Its size, from the cache's inputs: [batch, heads, length, head size].
        let layers = decoder.inputs().iter().filter(|i| i.name().ends_with(".decoder.key")).count();
        let shape = decoder.inputs().iter().find(|i| i.name() == "past_key_values.0.decoder.key").and_then(|i| i.dtype().tensor_shape()).map(|s| s.to_vec()).unwrap_or_default();
        let (heads, head_size) = match shape[..] {
            [_, h, _, d] if h > 0 && d > 0 => (h as usize, d as usize),
            _ => return Err("El modelo de letras no es el esperado".into()),
        };
        Ok(Self { _environment, encoder, decoder, tokenizer, mel: Mel::new(), layers, heads, head_size })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    /// The words in `mono` (at `rate` Hz), in `language` ("es", "en") or the
    /// one it hears. `progress` gets the fraction done and returns false to
    /// stop (then the result is an error).
    pub fn transcribe(&mut self, mono: &[f32], rate: f32, language: Option<&str>, mut progress: impl FnMut(f32) -> bool) -> Result<Transcript, String> {
        let audio = resample(mono, rate, RATE);
        let total = audio.len();
        let mut language = match language {
            Some(code) => Some(self.tokenizer.language(code).ok_or_else(|| format!("El modelo no conoce el idioma «{code}»"))?),
            None => None,
        };
        let mut segments = Vec::new();
        let mut seek = 0usize;
        while seek < total {
            let window = &audio[seek..(seek + CHUNK).min(total)];
            let length = window.len();
            let at = |fraction: f32| (seek as f32 + fraction * length as f32) / total as f32;
            if !progress(at(0.0)) {
                return Err(CANCELLED.into());
            }
            let rms = (window.iter().map(|x| x * x).sum::<f32>() / length.max(1) as f32).sqrt();
            // Too short to hold a word, or silent.
            if length < STEP * 10 || rms < SILENCE {
                seek += length;
                continue;
            }
            let encoded = self.encode(window)?;
            let lang = match language {
                Some(l) => l,
                None => *language.insert(self.detect_language(&encoded)?),
            };
            let decoded = self.decode(&encoded, lang, &mut |f| progress(at(0.1 + 0.9 * f)))?;
            if decoded.no_speech > NO_SPEECH_THRESHOLD && decoded.avg_logprob < LOGPROB_THRESHOLD {
                seek += length;
                continue;
            }
            let (found, advance) = self.segments(&decoded.tokens, length);
            let offset = seek as f64 / RATE as f64;
            for (start, end, text) in found {
                let (start, end) = (offset + start, offset + end);
                let text = text.trim().to_string();
                if !text.is_empty() {
                    segments.push(Segment { start, end, words: spread_words(&text, start, end), text });
                }
            }
            seek += advance.clamp(STEP, length);
        }
        progress(1.0);
        Ok(Transcript { language: language.map(|l| self.tokenizer.language_code(l)).unwrap_or_default(), segments })
    }

    /// The encoder's reading of a window: 1500 × the model's width.
    fn encode(&mut self, window: &[f32]) -> Result<Vec<f32>, String> {
        let mel = self.mel.log_mel(window);
        let input = ort::value::Tensor::from_array(([1usize, MELS, FRAMES], mel)).map_err(|e| e.to_string())?;
        let outputs = self.encoder.run(ort::inputs!["input_features" => input]).map_err(|e| format!("Error del modelo de letras: {e}"))?;
        let (_, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(data.to_vec())
    }

    /// One pass of the decoder over `ids` after what `cache` holds (none: the
    /// first). Gives the logits of each id, and the cache with them in.
    ///
    /// The cross-attention's part of the cache comes from the first pass only:
    /// the later ones give a placeholder for it.
    fn step(&mut self, encoded: &[f32], ids: &[u32], cache: Option<Vec<ort::value::DynValue>>) -> Result<(Vec<f32>, Vec<ort::value::DynValue>), String> {
        let error = |e: ort::Error| format!("Error del modelo de letras: {e}");
        let width = encoded.len() / 1500;
        let mut inputs: Vec<(std::borrow::Cow<str>, ort::session::SessionInputValue)> = Vec::new();
        let ids: Vec<i64> = ids.iter().map(|&i| i as i64).collect();
        inputs.push(("input_ids".into(), ort::value::Tensor::from_array(([1usize, ids.len()], ids)).map_err(error)?.into()));
        inputs.push(("encoder_hidden_states".into(), ort::value::TensorRef::from_array_view(([1usize, 1500, width], encoded)).map_err(error)?.into()));
        let first = cache.is_none();
        let names = self.cache_names();
        match &cache {
            Some(values) => {
                for (name, value) in names.iter().zip(values) {
                    inputs.push((name.0.clone().into(), value.into()));
                }
            }
            None => {
                for name in &names {
                    let empty = ort::value::Tensor::<f32>::from_array(([1usize, self.heads, 0, self.head_size], Vec::<f32>::new())).map_err(error)?;
                    inputs.push((name.0.clone().into(), empty.into()));
                }
            }
        }
        inputs.push(("use_cache_branch".into(), ort::value::Tensor::from_array(([1usize], vec![!first])).map_err(error)?.into()));
        let mut outputs = self.decoder.run(inputs).map_err(error)?;
        let (_, logits) = outputs["logits"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let logits = logits.to_vec();
        let mut fresh = Vec::with_capacity(names.len());
        for (_, output) in &names {
            fresh.push(outputs.remove(output).ok_or("El modelo de letras no devolvió su caché")?);
        }
        drop(outputs);
        let next = match cache {
            None => fresh,
            Some(old) => old.into_iter().zip(fresh).zip(&names).map(|((old, new), (_, name))| if name.contains(".encoder.") { old } else { new }).collect(),
        };
        Ok((logits, next))
    }

    /// The cache's names, as inputs and as outputs, layer by layer.
    fn cache_names(&self) -> Vec<(String, String)> {
        let mut names = Vec::new();
        for layer in 0..self.layers {
            for part in ["decoder.key", "decoder.value", "encoder.key", "encoder.value"] {
                names.push((format!("past_key_values.{layer}.{part}"), format!("present.{layer}.{part}")));
            }
        }
        names
    }

    /// The language the window is in: the likeliest language token after the start.
    fn detect_language(&mut self, encoded: &[f32]) -> Result<u32, String> {
        let (logits, _) = self.step(encoded, &[SOT], None)?;
        let best = (FIRST_LANGUAGE..TRANSLATE).max_by(|&a, &b| logits[a as usize].total_cmp(&logits[b as usize])).unwrap_or(FIRST_LANGUAGE);
        Ok(best)
    }

    /// Greedy decoding of a window, with timestamps.
    fn decode(&mut self, encoded: &[f32], language: u32, progress: &mut dyn FnMut(f32) -> bool) -> Result<Decoded, String> {
        let prompt = [SOT, language, TRANSCRIBE];
        let (logits, mut cache) = self.step(encoded, &prompt, None)?;
        let vocab = logits.len() / prompt.len();
        // How likely "no speech" is, right after the start.
        let no_speech = softmax_at(&logits[..vocab], NO_SPEECH as usize);
        let mut row = logits[(prompt.len() - 1) * vocab..].to_vec();
        let mut tokens: Vec<u32> = Vec::new();
        let mut sum_logprob = 0.0f32;
        while tokens.len() < MAX_TOKENS {
            apply_rules(&mut row, &tokens);
            let logprobs = log_softmax(&row);
            let next = (0..row.len()).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap_or(EOT as usize) as u32;
            sum_logprob += logprobs[next as usize];
            if next == EOT {
                break;
            }
            tokens.push(next);
            if !progress(tokens.len() as f32 / MAX_TOKENS as f32) {
                return Err(CANCELLED.into());
            }
            let (logits, next_cache) = self.step(encoded, &[next], Some(cache))?;
            cache = next_cache;
            row = logits;
        }
        let avg_logprob = sum_logprob / (tokens.len() + 1) as f32;
        Ok(Decoded { tokens, avg_logprob, no_speech })
    }

    /// The phrases in a window's tokens (seconds of the window, text), and how
    /// far to go on (samples): to the end of the last whole phrase, or past the
    /// window when it ended cleanly.
    fn segments(&self, tokens: &[u32], length: usize) -> (Vec<(f64, f64, String)>, usize) {
        segments_of(tokens, length, |ids| self.tokenizer.decode(ids))
    }
}

/// See [`Whisper::segments`].
fn segments_of(tokens: &[u32], length: usize, decode: impl Fn(&[u32]) -> String) -> (Vec<(f64, f64, String)>, usize) {
    let seconds = |t: u32| (t - TIMESTAMP_BEGIN) as f64 * STEP as f64 / RATE as f64;
    let window_end = length as f64 / RATE as f64;
    let mut found = Vec::new();
    let (mut start, mut text): (Option<u32>, Vec<u32>) = (None, Vec::new());
    let mut last_closing: Option<u32> = None;
    for &t in tokens {
        if t >= TIMESTAMP_BEGIN {
            if text.is_empty() {
                start = Some(t);
            } else {
                found.push((start.map_or(0.0, seconds), seconds(t), decode(&text)));
                text.clear();
                start = None;
                last_closing = Some(t);
            }
        } else if t < EOT {
            text.push(t);
        }
    }
    let ends_cleanly = tokens.len() >= 2 && tokens[tokens.len() - 1] >= TIMESTAMP_BEGIN && tokens[tokens.len() - 2] < TIMESTAMP_BEGIN;
    let advance = match last_closing {
        // Text after the last phrase that never closed: read again from there.
        Some(t) if !text.is_empty() || !ends_cleanly => (t - TIMESTAMP_BEGIN) as usize * STEP,
        Some(_) => length,
        None => {
            // No timestamps that close a phrase: all of it, to the window's end.
            if !text.is_empty() {
                found.push((start.map_or(0.0, seconds), window_end, decode(&text)));
            }
            length
        }
    };
    let advance = if advance == 0 { length } else { advance };
    for f in &mut found {
        f.1 = f.1.min(window_end).max(f.0);
    }
    (found, advance)
}

/// The probability of `index` in `logits`.
fn softmax_at(logits: &[f32], index: usize) -> f32 {
    let max = logits.iter().copied().fold(f32::MIN, f32::max);
    let sum: f32 = logits.iter().map(|&l| (l - max).exp()).sum();
    (logits[index] - max).exp() / sum
}

fn log_softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::MIN, f32::max);
    let sum: f32 = logits.iter().map(|&l| (l - max).exp()).sum();
    let log = sum.ln() + max;
    logits.iter().map(|&l| l - log).collect()
}

/// What the model may write next, after `tokens` (OpenAI's rules): no
/// symbols nor markers, no blank start, timestamps in pairs and never going
/// back, the first one within a second, and a timestamp whenever one is
/// likelier than any word.
fn apply_rules(logits: &mut [f32], tokens: &[u32]) {
    let ban = |logits: &mut [f32], range: std::ops::Range<usize>| {
        let n = logits.len();
        for l in &mut logits[range.start.min(n)..range.end.min(n)] {
            *l = f32::NEG_INFINITY;
        }
    };
    let n = logits.len();
    for &t in SUPPRESS.iter().chain([&NO_TIMESTAMPS]) {
        if (t as usize) < n {
            logits[t as usize] = f32::NEG_INFINITY;
        }
    }
    let ts = TIMESTAMP_BEGIN as usize;
    if tokens.is_empty() {
        // A start with a blank or the end is no start; and it starts with a timestamp.
        logits[220] = f32::NEG_INFINITY;
        logits[EOT as usize] = f32::NEG_INFINITY;
        ban(logits, 0..ts);
        ban(logits, ts + MAX_INITIAL_TIMESTAMP as usize + 1..n);
        return;
    }
    let is_ts = |t: u32| t >= TIMESTAMP_BEGIN;
    let last = is_ts(tokens[tokens.len() - 1]);
    let penultimate = tokens.len() < 2 || is_ts(tokens[tokens.len() - 2]);
    if last {
        if penultimate {
            // Two timestamps: text now.
            ban(logits, ts..n);
        } else {
            // A phrase just closed: another timestamp or the end.
            ban(logits, 0..EOT as usize);
        }
    }
    if let Some(&latest) = tokens.iter().rev().find(|&&t| is_ts(t)) {
        // Never back; and after a phrase's start, not on the same step.
        let from = if last && !penultimate { latest } else { latest + 1 };
        ban(logits, ts..from as usize);
    }
    // A timestamp when they are, together, likelier than any one word.
    let logprobs = log_softmax(logits);
    let max_text = logprobs[..ts].iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let timestamps = logprobs[ts..].iter().copied().fold(f32::NEG_INFINITY, |acc, l| {
        let (a, b) = if acc > l { (acc, l) } else { (l, acc) };
        if a == f32::NEG_INFINITY { a } else { a + (b - a).exp().ln_1p() }
    });
    if timestamps > max_text {
        ban(logits, 0..ts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference's test signal (scripts/lyrics/make_reference.py).
    fn signal() -> Vec<f32> {
        let n = 2 * RATE as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / RATE as f64;
                let mut v = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() + 0.25 * (i as f64 / n as f64) * (2.0 * std::f64::consts::PI * 3000.0 * t).sin();
                if i == RATE as usize {
                    v += 0.9;
                }
                v as f32
            })
            .collect()
    }

    /// The same spectrogram as Hugging Face's WhisperFeatureExtractor.
    #[test]
    fn log_mel_matches_whisper() {
        let mel = log_mel(&signal());
        assert_eq!(mel.len(), MELS * FRAMES);
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/whisper_mel.txt")).unwrap();
        let mut rows = 0;
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let numbers: Vec<f64> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
            let frame = numbers[0] as usize;
            for (m, &expected) in numbers[1..].iter().enumerate() {
                let got = mel[m * FRAMES + frame] as f64;
                assert!((got - expected).abs() < 2e-3, "frame {frame}, band {m}: {got} vs {expected}");
            }
            rows += 1;
        }
        assert_eq!(rows, 9);
    }

    #[test]
    fn mel_filters_cover_the_band_like_librosa() {
        let filters = mel_filters();
        assert_eq!(filters.len(), 80);
        // Slaney's scale: 1 kHz is mel 15, both ways.
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-9);
        assert!((mel_to_hz(hz_to_mel(5000.0)) - 5000.0).abs() < 1e-6);
        // Every band has something, the lowest starts at 0 Hz.
        assert!(filters.iter().all(|f| f.iter().any(|&v| v > 0.0)));
        assert_eq!(filters[0][0], 0.0);
    }

    /// Byte-level tokens: "Ġ" is a space, two characters can be one letter.
    #[test]
    fn tokens_decode_to_text() {
        let mut tokens: Vec<String> = ["ĠMi", "Ġcora", "zÃ³n", "Ġlate", ",", "Ġseñor", "Ã±"].iter().map(|s| s.to_string()).collect();
        tokens.resize(EOT as usize, String::new());
        tokens.push("<|endoftext|>".into());
        tokens.push("<|startoftranscript|>".into());
        tokens.push("<|en|>".into());
        tokens.push("<|zh|>".into());
        tokens.push("<|de|>".into());
        tokens.push("<|es|>".into());
        let tokenizer = Tokenizer::new(tokens);
        assert_eq!(tokenizer.decode(&[0, 1, 2, 3, 4]), " Mi corazón late,");
        // Special tokens and timestamps are not text.
        assert_eq!(tokenizer.decode(&[SOT, 0, 6, TIMESTAMP_BEGIN + 5, EOT]), " Miñ");
        assert_eq!(tokenizer.language("es"), Some(FIRST_LANGUAGE + 3));
        assert_eq!(tokenizer.language_code(FIRST_LANGUAGE), "en");
        assert_eq!(tokenizer.language("fr"), None);
        // A multi-byte letter split over two tokens comes back whole.
        assert_eq!(byte_of('Ġ'), Some(b' '));
        assert_eq!(byte_of('Ã'), Some(0xC3));
        assert_eq!(byte_of('a'), Some(b'a'));
    }

    #[test]
    fn phrases_come_from_timestamp_pairs() {
        let ts = |s: f64| TIMESTAMP_BEGIN + (s / 0.02).round() as u32;
        let decode = |ids: &[u32]| ids.iter().map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let length = 30 * 16_000;
        // Two whole phrases, then the window ends cleanly.
        let (found, advance) = segments_of(&[ts(0.0), 1, 2, ts(2.0), ts(2.5), 3, ts(4.0)], length, decode);
        assert_eq!(found, vec![(0.0, 2.0, "w1 w2".into()), (2.5, 4.0, "w3".into())]);
        assert_eq!(advance, length);
        // A phrase cut off by the window: read again from the end of the last whole one.
        let (found, advance) = segments_of(&[ts(0.0), 1, ts(26.0), ts(27.0), 2, 3], length, decode);
        assert_eq!(found.len(), 1);
        assert_eq!(advance, (26.0 * 16_000.0) as usize);
        // No timestamps closing anything: all of it, to the window's end.
        let (found, advance) = segments_of(&[ts(1.0), 1, 2], 10 * 16_000, decode);
        assert_eq!(found, vec![(1.0, 10.0, "w1 w2".into())]);
        assert_eq!(advance, 10 * 16_000);
    }

    #[test]
    fn words_share_their_phrase_by_length() {
        let words = spread_words(" yo te quiero ", 10.0, 22.0);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["yo", "te", "quiero"]);
        assert_eq!(words[0].start, 10.0);
        // 3 + 3 + 7 letters and spaces over 12 s.
        assert!((words[1].start - (10.0 + 12.0 * 3.0 / 13.0)).abs() < 1e-9);
        assert!(words[2].end <= 22.0 && words[2].end > 21.0);
        assert!(words.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn the_rules_keep_timestamps_in_order() {
        let n = TIMESTAMP_BEGIN as usize + 1501;
        // At the start: a timestamp within the first second.
        let mut logits = vec![0.0f32; n];
        apply_rules(&mut logits, &[]);
        assert!(logits[..TIMESTAMP_BEGIN as usize].iter().all(|l| l.is_infinite()));
        assert!(logits[TIMESTAMP_BEGIN as usize + 50].is_finite() && logits[TIMESTAMP_BEGIN as usize + 51].is_infinite());
        // After text and a timestamp: another timestamp (not earlier) or the end.
        let mut logits = vec![0.0f32; n];
        (logits[100], logits[EOT as usize]) = (10.0, 10.0);
        apply_rules(&mut logits, &[TIMESTAMP_BEGIN, 100, TIMESTAMP_BEGIN + 40]);
        assert!(logits[100].is_infinite() && logits[EOT as usize].is_finite());
        assert!(logits[TIMESTAMP_BEGIN as usize + 39].is_infinite() && logits[TIMESTAMP_BEGIN as usize + 40].is_finite());
        // After two timestamps: text.
        let mut logits = vec![0.0f32; n];
        logits[100] = 10.0;
        apply_rules(&mut logits, &[TIMESTAMP_BEGIN, 100, TIMESTAMP_BEGIN + 40, TIMESTAMP_BEGIN + 40]);
        assert!(logits[TIMESTAMP_BEGIN as usize..].iter().all(|l| l.is_infinite()));
        assert!(logits[100].is_finite());
    }

    /// With the model downloaded (PULSE_LYRICS_MODEL, or Pulse's own folder),
    /// a voice made by macOS's `say` is read back.
    #[test]
    #[ignore = "needs the lyrics model and macOS's say"]
    fn reads_a_spoken_phrase() {
        let dir = std::env::var_os("PULSE_LYRICS_MODEL").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join("Library/Application Support/Pulse/Models/whisper-small")
        });
        let mut whisper = Whisper::load(&dir).unwrap();
        for (voice, text, language) in [("Monica", "Bailando bajo la luna, mi corazón late contigo.", "es"), ("Samantha", "Dancing under the moonlight, my heart beats with you.", "en")] {
            let wav = std::env::temp_dir().join(format!("pulse-say-{voice}-{}.wav", std::process::id()));
            let status = std::process::Command::new("say").args(["-v", voice, "-o"]).arg(&wav).args(["--data-format=LEI16@22050", text]).status().unwrap();
            assert!(status.success());
            let mut reader = hound::WavReader::open(&wav).unwrap();
            let rate = reader.spec().sample_rate as f32;
            let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
            let started = std::time::Instant::now();
            let transcript = whisper.transcribe(&audio, rate, None, |_| true).unwrap();
            println!("{voice} ({:.1} s of audio, {:.1} s): {:?}", audio.len() as f32 / rate, started.elapsed().as_secs_f32(), transcript);
            assert_eq!(transcript.language, language);
            let heard: String = transcript.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
            for word in ["luna", "corazón", "moonlight", "heart"] {
                if text.to_lowercase().contains(word) {
                    assert!(heard.contains(word), "«{word}» in «{heard}»");
                }
            }
            let _ = std::fs::remove_file(&wav);
        }
    }
}
