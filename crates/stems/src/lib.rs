//! Separating a song into stems (drums, bass, other, vocals) with HTDemucs, Meta's
//! hybrid transformer model, run with ONNX Runtime.
//!
//! The model file holds only the network (see scripts/stems/export_htdemucs.py):
//! the spectrogram that goes in and the inverse transform of what comes out are
//! computed here, as Demucs does them in PyTorch, and so is the cutting of a song
//! into 7.8 s segments that overlap by a quarter and are cross-faded back
//! together. The steps follow `demucs.apply.apply_model` (without shifts) and
//! `HTDemucs._spec` / `_ispec`; the tests compare each against PyTorch's results.

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::path::Path;
use std::sync::Arc;

/// The rate the model works at.
pub const SAMPLE_RATE: u32 = 44_100;
/// The sources, in the model's order.
pub const SOURCES: [&str; 4] = ["drums", "bass", "other", "vocals"];
/// A segment: 7.8 s, the length HTDemucs was trained on.
pub const SEGMENT: usize = 343_980;
const NFFT: usize = 4096;
const HOP: usize = NFFT / 4;
const BINS: usize = NFFT / 2;
/// Spectrogram frames per segment.
const FRAMES: usize = SEGMENT.div_ceil(HOP);
/// Segments overlap by this much of their length.
const OVERLAP: f32 = 0.25;

/// A stereo signal at [`SAMPLE_RATE`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Stereo {
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    fn zeros(n: usize) -> Self {
        Self { left: vec![0.0; n], right: vec![0.0; n] }
    }

    fn channel(&self, c: usize) -> &[f32] {
        if c == 0 { &self.left } else { &self.right }
    }

    fn channel_mut(&mut self, c: usize) -> &mut [f32] {
        if c == 0 { &mut self.left } else { &mut self.right }
    }
}

/// The model, loaded and ready to separate.
pub struct Separator {
    _environment: ort::environment::Environment,
    session: ort::session::Session,
    transforms: Transforms,
}

impl Separator {
    /// Loads the model file (htdemucs.onnx). Uses every core but one.
    pub fn load(model: &Path) -> Result<Self, String> {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).max(1));
        let build = || -> ort::Result<(ort::environment::Environment, ort::session::Session)> {
            let environment = environment()?;
            // On the CPU, in full precision: by default ONNX Runtime picks the most
            // efficient device, which on a Mac is CoreML in half precision (its
            // spectral branch then lands 30 dB from PyTorch's, against 75 on the CPU).
            let session = ort::session::Session::builder(&environment)?
                .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)?
                .with_optimization_level(ort::session::builder::GraphOptimizationLevel::All)
                .unwrap_or_else(|e| e.recover())
                .with_intra_threads(threads)
                .unwrap_or_else(|e| e.recover())
                .commit_from_file(model)?;
            Ok((environment, session))
        };
        let (_environment, session) = build().map_err(|e| format!("No se pudo cargar el modelo de separación: {e}"))?;
        Ok(Self { _environment, session, transforms: Transforms::new() })
    }

    /// Separates `mix` (stereo, 44.1 kHz) into the four [`SOURCES`]. `progress` gets
    /// the fraction done after each segment and returns false to stop (then the
    /// result is an error).
    pub fn separate(&mut self, mix: &Stereo, mut progress: impl FnMut(f32) -> bool) -> Result<[Stereo; 4], String> {
        let n = mix.len();
        if n == 0 {
            return Ok(Default::default());
        }
        // Normalized as a whole, by the mono mix (demucs.api.Separator).
        let mono: Vec<f32> = mix.left.iter().zip(&mix.right).map(|(l, r)| (l + r) * 0.5).collect();
        let mean = mono.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
        let variance = mono.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / (n.max(2) - 1) as f64;
        let (mean, std) = (mean as f32, variance.sqrt() as f32 + 1e-8);
        let normalized = Stereo {
            left: mix.left.iter().map(|x| (x - mean) / std).collect(),
            right: mix.right.iter().map(|x| (x - mean) / std).collect(),
        };

        // Overlapping segments, cross-faded with a triangle (apply_model, split=True).
        let stride = ((1.0 - OVERLAP) * SEGMENT as f32) as usize;
        let weight = triangle(SEGMENT);
        let mut out: [Stereo; 4] = std::array::from_fn(|_| Stereo::zeros(n));
        let mut total = vec![0f32; n];
        let offsets: Vec<usize> = (0..n).step_by(stride).collect();
        for (i, &offset) in offsets.iter().enumerate() {
            let length = SEGMENT.min(n - offset);
            // The segment, centred on the chunk with real audio around it where
            // there is some (TensorChunk.padded).
            let delta = SEGMENT - length;
            let start = offset as isize - (delta / 2) as isize;
            let take = |c: &[f32]| (0..SEGMENT).map(|k| usize::try_from(start + k as isize).ok().and_then(|j| c.get(j)).copied().unwrap_or(0.0)).collect::<Vec<f32>>();
            let segment = Stereo { left: take(&normalized.left), right: take(&normalized.right) };
            let stems = self.segment(&segment)?;
            // Trimmed back to the chunk (center_trim).
            let from = delta / 2;
            for (s, stem) in stems.iter().enumerate() {
                for c in 0..2 {
                    let (dst, src) = (out[s].channel_mut(c), stem.channel(c));
                    for k in 0..length {
                        dst[offset + k] += weight[k] * src[from + k];
                    }
                }
            }
            for k in 0..length {
                total[offset + k] += weight[k];
            }
            if !progress((i + 1) as f32 / offsets.len() as f32) {
                return Err("Separación cancelada".into());
            }
        }
        for stem in &mut out {
            for c in 0..2 {
                for (x, w) in stem.channel_mut(c).iter_mut().zip(&total) {
                    *x = *x / w * std + mean;
                }
            }
        }
        Ok(out)
    }

    /// One segment of exactly [`SEGMENT`] samples through the model.
    fn segment(&mut self, mix: &Stereo) -> Result<[Stereo; 4], String> {
        let spectrogram = self.transforms.spec(mix);
        let mut flat_mix = mix.left.clone();
        flat_mix.extend_from_slice(&mix.right);
        let run = |session: &mut ort::session::Session| -> ort::Result<(Vec<f32>, Vec<f32>)> {
            let mix = ort::value::Tensor::from_array(([1usize, 2, SEGMENT], flat_mix))?;
            let mag = ort::value::Tensor::from_array(([1usize, 4, BINS, FRAMES], spectrogram))?;
            let outputs = session.run(ort::inputs!["mix" => mix, "mag" => mag])?;
            let (_, spec) = outputs["spec"].try_extract_tensor::<f32>()?;
            let (_, wave) = outputs["wave"].try_extract_tensor::<f32>()?;
            Ok((spec.to_vec(), wave.to_vec()))
        };
        let (spec, wave) = run(&mut self.session).map_err(|e| format!("Error del modelo de separación: {e}"))?;
        Ok(self.transforms.rebuild(&spec, &wave))
    }
}

/// ONNX Runtime's environment: one per process, made the first time.
fn environment() -> ort::Result<ort::environment::Environment> {
    static ENVIRONMENT: std::sync::Mutex<Option<ort::environment::Environment>> = std::sync::Mutex::new(None);
    let mut shared = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(environment) = shared.as_ref() {
        return Ok(environment.clone());
    }
    let environment = ort::init().with_name("pulse").build()?;
    *shared = Some(environment.clone());
    Ok(environment)
}

/// The cross-fade weights of a segment: up, then down, peaking at 1.
fn triangle(length: usize) -> Vec<f32> {
    let half = length / 2;
    let up = (1..=half).map(|k| k as f32);
    let down = (1..=length - half).rev().map(|k| k as f32);
    let weights: Vec<f32> = up.chain(down).collect();
    let max = weights.iter().copied().fold(0.0, f32::max);
    weights.into_iter().map(|w| w / max).collect()
}

/// The STFT and its inverse as HTDemucs uses them: 4096 points, hop 1024,
/// periodic Hann window, normalized, centred with reflection.
struct Transforms {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
}

impl Transforms {
    fn new() -> Self {
        let mut planner = FftPlanner::new();
        let window = (0..NFFT).map(|k| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * k as f32 / NFFT as f32).cos()).collect();
        Self { forward: planner.plan_fft_forward(NFFT), inverse: planner.plan_fft_inverse(NFFT), window }
    }

    /// `HTDemucs._spec` and `_magnitude` (complex as channels): channels
    /// [left re, left im, right re, right im] × 2048 bins × 336 frames.
    fn spec(&self, mix: &Stereo) -> Vec<f32> {
        let pad = HOP / 2 * 3;
        let mut out = vec![0f32; 4 * BINS * FRAMES];
        let scale = 1.0 / (NFFT as f32).sqrt();
        let mut buffer = vec![Complex32::default(); NFFT];
        for c in 0..2 {
            // Demucs's padding, then torch.stft's own centring: both reflections.
            let x = reflect(mix.channel(c), pad, pad + FRAMES * HOP - SEGMENT);
            let x = reflect(&x, NFFT / 2, NFFT / 2);
            // Frames 2..2+336 of the 340 torch.stft gives.
            for t in 0..FRAMES {
                let start = (t + 2) * HOP;
                for k in 0..NFFT {
                    buffer[k] = Complex32::new(x[start + k] * self.window[k], 0.0);
                }
                self.forward.process(&mut buffer);
                for f in 0..BINS {
                    out[((2 * c) * BINS + f) * FRAMES + t] = buffer[f].re * scale;
                    out[((2 * c + 1) * BINS + f) * FRAMES + t] = buffer[f].im * scale;
                }
            }
        }
        out
    }

    /// The four sources from the model's outputs: `HTDemucs._mask` (complex as
    /// channels) and `_ispec`, plus the time branch. `spec` is 4 sources × 4
    /// channels × 2048 × 336, `wave` 4 sources × 2 channels × [`SEGMENT`].
    fn rebuild(&self, spec: &[f32], wave: &[f32]) -> [Stereo; 4] {
        let pad = HOP / 2 * 3;
        // _ispec adds an empty top bin and two empty frames on each side.
        let frames = FRAMES + 4;
        let length = HOP * frames - HOP; // what torch.istft returns for these frames
        let scale = (NFFT as f32).sqrt() / NFFT as f32;
        let mut buffer = vec![Complex32::default(); NFFT];
        std::array::from_fn(|s| {
            let mut stem = Stereo::zeros(SEGMENT);
            for c in 0..2 {
                let re = |f: usize, t: usize| spec[(((s * 4) + 2 * c) * BINS + f) * FRAMES + t];
                let im = |f: usize, t: usize| spec[(((s * 4) + 2 * c + 1) * BINS + f) * FRAMES + t];
                // Overlap-add with the window, over the centred length.
                let mut signal = vec![0f32; length + NFFT];
                let mut envelope = vec![0f32; length + NFFT];
                for t in 0..frames {
                    let inner = t.checked_sub(2).filter(|&t| t < FRAMES);
                    for f in 0..=BINS {
                        let value = match inner {
                            Some(t) if f < BINS => Complex32::new(re(f, t), im(f, t)),
                            _ => Complex32::default(),
                        };
                        // A real signal: DC and Nyquist are real, the rest mirrored.
                        let value = if f == 0 || f == BINS { Complex32::new(value.re, 0.0) } else { value };
                        buffer[f] = value;
                        if f > 0 && f < BINS {
                            buffer[NFFT - f] = value.conj();
                        }
                    }
                    self.inverse.process(&mut buffer);
                    let at = t * HOP;
                    for k in 0..NFFT {
                        signal[at + k] += buffer[k].re * scale * self.window[k];
                        envelope[at + k] += self.window[k] * self.window[k];
                    }
                }
                // Centred: the first NFFT/2 samples are padding; then Demucs's pad.
                let out = stem.channel_mut(c);
                let time = &wave[(s * 2 + c) * SEGMENT..(s * 2 + c + 1) * SEGMENT];
                for k in 0..SEGMENT {
                    let j = NFFT / 2 + pad + k;
                    let e = envelope[j];
                    let x = if e > 1e-11 { signal[j] / e } else { 0.0 };
                    out[k] = x + time[k];
                }
            }
            stem
        })
    }
}

/// `x` with `left` and `right` samples mirrored on each side (without repeating
/// the edge), as torch's reflect padding. Inputs shorter than the padding get
/// zeros first (demucs's pad1d).
fn reflect(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = Vec::with_capacity(n + left + right);
    let at = |i: isize| -> f32 {
        if n < 2 {
            return 0.0;
        }
        let period = 2 * (n as isize - 1);
        let mut i = i.rem_euclid(period);
        if i >= n as isize {
            i = period - i;
        }
        x[i as usize]
    };
    for i in -(left as isize)..(n + right) as isize {
        out.push(at(i));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_matches_torch() {
        // torch.nn.functional.pad([1, 2, 3, 4], (2, 3), mode="reflect")
        assert_eq!(reflect(&[1.0, 2.0, 3.0, 4.0], 2, 3), vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    fn segments_cross_fade_to_one() {
        let w = triangle(10);
        assert_eq!(w.len(), 10);
        assert_eq!(w[4], 1.0);
        assert_eq!(w[5], 1.0);
        assert!(w[0] > 0.0 && w[0] < 0.25);
    }

    #[test]
    fn spectrum_and_back_give_the_signal() {
        // A segment through spec and rebuild (as if the model returned the mix
        // as one source and nothing in its time branch) comes back the same.
        let t = Transforms::new();
        let mix = Stereo {
            left: (0..SEGMENT).map(|i| (i as f32 * 0.01).sin() * 0.5).collect(),
            right: (0..SEGMENT).map(|i| (i as f32 * 0.003).cos() * 0.3).collect(),
        };
        let spectrum = t.spec(&mix);
        let mut spec = vec![0f32; 4 * spectrum.len()];
        spec[..spectrum.len()].copy_from_slice(&spectrum);
        let wave = vec![0f32; 4 * 2 * SEGMENT];
        let stems = t.rebuild(&spec, &wave);
        // Away from the edges (where the reflection differs) it is the input.
        let error = (NFFT..SEGMENT - NFFT).map(|i| (stems[0].left[i] - mix.left[i]).abs().max((stems[0].right[i] - mix.right[i]).abs())).fold(0.0, f32::max);
        assert!(error < 1e-4, "{error}");
        assert!(stems[1].left.iter().all(|&x| x == 0.0));
    }

    /// Reference results from PyTorch (scripts/stems/make_reference.py) in
    /// PULSE_STEMS_DIR, with the model.
    fn reference(name: &str) -> Option<Vec<f32>> {
        let dir = std::env::var("PULSE_STEMS_DIR").ok()?;
        let bytes = std::fs::read(Path::new(&dir).join(name)).ok()?;
        Some(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    fn max_error(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    fn peak(a: &[f32]) -> f32 {
        a.iter().map(|x| x.abs()).fold(0.0, f32::max)
    }

    #[test]
    #[ignore = "needs PULSE_STEMS_DIR with the model and PyTorch's references"]
    fn one_segment_matches_pytorch() {
        let mix = reference("segment_mix.f32").expect("references");
        let mix = Stereo { left: mix[..SEGMENT].to_vec(), right: mix[SEGMENT..].to_vec() };
        let t = Transforms::new();
        let spectrogram = t.spec(&mix);
        let mag = reference("segment_mag.f32").unwrap();
        println!("spectrogram: max error {} (peak {})", max_error(&spectrogram, &mag), peak(&mag));
        assert!(max_error(&spectrogram, &mag) < 1e-3 * peak(&mag));

        // Rebuilding from PyTorch's model outputs.
        let (spec, wave, stems) = (reference("segment_spec.f32").unwrap(), reference("segment_wave.f32").unwrap(), reference("segment_stems.f32").unwrap());
        let rebuilt: Vec<f32> = t.rebuild(&spec, &wave).iter().flat_map(|s| s.left.iter().chain(&s.right).copied().collect::<Vec<_>>()).collect();
        println!("rebuilt stems: max error {} (peak {})", max_error(&rebuilt, &stems), peak(&stems));
        assert!(max_error(&rebuilt, &stems) < 1e-3 * peak(&stems));

        // And through the ONNX model here.
        let dir = std::env::var("PULSE_STEMS_DIR").unwrap();
        let started = std::time::Instant::now();
        let mut separator = Separator::load(&Path::new(&dir).join("htdemucs.onnx")).unwrap();
        println!("loaded in {:.1} s", started.elapsed().as_secs_f32());
        let started = std::time::Instant::now();
        let _ = separator.segment(&mix).unwrap();
        println!("first segment in {:.1} s", started.elapsed().as_secs_f32());
        let started = std::time::Instant::now();
        let ours: Vec<f32> = separator.segment(&mix).unwrap().iter().flat_map(|s| s.left.iter().chain(&s.right).copied().collect::<Vec<_>>()).collect();
        println!("second segment in {:.1} s", started.elapsed().as_secs_f32());
        let signal = stems.iter().map(|x| x * x).sum::<f32>();
        let noise = ours.iter().zip(&stems).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
        println!("model here: max error {} (peak {}), {:.1} dB", max_error(&ours, &stems), peak(&stems), 10.0 * (signal / noise).log10());
        assert!(max_error(&ours, &stems) < 1e-2 * peak(&stems));
    }

    #[test]
    #[ignore = "needs PULSE_STEMS_DIR with the model and PyTorch's references"]
    fn the_network_matches_pytorch_given_the_same_inputs() {
        let dir = std::env::var("PULSE_STEMS_DIR").unwrap();
        let (mix, mag) = (reference("segment_mix.f32").unwrap(), reference("segment_mag.f32").unwrap());
        let (spec, wave) = (reference("segment_spec.f32").unwrap(), reference("segment_wave.f32").unwrap());
        let mut separator = Separator::load(&Path::new(&dir).join("htdemucs.onnx")).unwrap();
        let mix = ort::value::Tensor::from_array(([1usize, 2, SEGMENT], mix)).unwrap();
        let mag = ort::value::Tensor::from_array(([1usize, 4, BINS, FRAMES], mag)).unwrap();
        let outputs = separator.session.run(ort::inputs!["mix" => mix, "mag" => mag]).unwrap();
        let (shape, ours_spec) = outputs["spec"].try_extract_tensor::<f32>().unwrap();
        assert_eq!(**shape, [1, 4, 4, BINS as i64, FRAMES as i64]);
        let (_, ours_wave) = outputs["wave"].try_extract_tensor::<f32>().unwrap();
        let snr = |a: &[f32], b: &[f32]| 10.0 * (b.iter().map(|x| x * x).sum::<f32>() / a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>()).log10();
        let (spec_snr, wave_snr) = (snr(ours_spec, &spec), snr(ours_wave, &wave));
        println!("spec {spec_snr:.1} dB, wave {wave_snr:.1} dB");
        assert!(spec_snr > 60.0 && wave_snr > 60.0);
    }

    #[test]
    #[ignore = "needs PULSE_STEMS_DIR with the model and PyTorch's references"]
    fn a_song_matches_pytorch() {
        let mix = reference("song_mix.f32").expect("references");
        let n = mix.len() / 2;
        let mix = Stereo { left: mix[..n].to_vec(), right: mix[n..].to_vec() };
        let expected = reference("song_stems.f32").unwrap();
        let dir = std::env::var("PULSE_STEMS_DIR").unwrap();
        let mut separator = Separator::load(&Path::new(&dir).join("htdemucs.onnx")).unwrap();
        let started = std::time::Instant::now();
        let mut steps = 0;
        let stems = separator.separate(&mix, |_| {
            steps += 1;
            true
        }).unwrap();
        println!("{:.1} s of audio in {:.1} s ({steps} segments)", n as f32 / SAMPLE_RATE as f32, started.elapsed().as_secs_f32());
        for (s, name) in SOURCES.iter().enumerate() {
            let ours: Vec<f32> = stems[s].left.iter().chain(&stems[s].right).copied().collect();
            let theirs = &expected[s * 2 * n..(s + 1) * 2 * n];
            let signal = theirs.iter().map(|x| x * x).sum::<f32>();
            let noise = ours.iter().zip(theirs).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
            let snr = 10.0 * (signal / noise.max(1e-20)).log10();
            println!("{name}: {snr:.1} dB from PyTorch's");
            assert!(snr > 40.0, "{name}: {snr} dB");
        }
    }
}
