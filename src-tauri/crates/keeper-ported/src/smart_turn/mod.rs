//! Smart Turn's input, rewritten in Rust (AD-396): the log-mel features the
//! end-of-turn model scores, computed the way smart-turn's `inference.py`
//! computes them — the last 8 s of 16 kHz audio, padded with silence on the
//! left when shorter, through Hugging Face transformers'
//! `WhisperFeatureExtractor(chunk_length=8)` with `do_normalize=True`.
//!
//! Modified from transformers `src/transformers/audio_utils.py`
//! (`hertz_to_mel`, `mel_to_hertz`, `_create_triangular_filter_bank`,
//! `mel_filter_bank`, `window_function`, `spectrogram`) and
//! `src/transformers/models/whisper/feature_extraction_whisper.py`
//! (`zero_mean_unit_var_norm`, `_np_extract_fbank_features`), Apache-2.0,
//! Copyright 2022–2023 The HuggingFace Inc. team. Changed: Python and numpy
//! to Rust, one fixed configuration instead of parameters, the FFT is
//! `rustfft`'s, and the buffers are allocated once per [`Features`] rather
//! than per call. `UPSTREAM.md` beside this file records the commit.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// The sample rate the model takes.
pub const RATE: usize = 16_000;

/// The audio the model hears: the last 8 s.
pub const SAMPLES: usize = 8 * RATE;

/// Mel bins per frame.
pub const MEL_BINS: usize = 80;

/// Frames: one per [`HOP`] over [`SAMPLES`], the STFT's last frame dropped.
pub const FRAMES: usize = SAMPLES / HOP;

/// The whole feature array, bin-major (`[MEL_BINS][FRAMES]`).
pub const FEATURES: usize = MEL_BINS * FRAMES;

/// The STFT's frame and FFT size.
const N_FFT: usize = 400;

/// The STFT's hop.
const HOP: usize = 160;

/// Frequency bins of a one-sided 400-point FFT.
const BINS: usize = N_FFT / 2 + 1;

/// The floor under a mel energy before its log (`mel_floor`).
const MEL_FLOOR: f64 = 1e-10;

/// The variance's guard in the normalisation (`zero_mean_unit_var_norm`).
const VARIANCE_GUARD: f64 = 1e-7;

/// One mel filter: the first FFT bin it weighs and its weights from there.
struct Filter {
    first: usize,
    weights: Vec<f64>,
}

/// The extractor: the window, the filters, the FFT plan and every buffer,
/// made once, so [`Features::compute`] allocates nothing.
pub struct Features {
    window: Vec<f64>,
    filters: Vec<Filter>,
    fft: Arc<dyn Fft<f64>>,
    /// The normalised audio with `N_FFT / 2` reflected samples each side.
    padded: Vec<f64>,
    frame: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    power: Vec<f64>,
}

impl Default for Features {
    fn default() -> Self {
        Self::new()
    }
}

impl Features {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(N_FFT);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Self {
            window: hann(),
            filters: slaney_filters(),
            fft,
            padded: vec![0.0; SAMPLES + N_FFT],
            frame: vec![Complex::default(); N_FFT],
            scratch,
            power: vec![0.0; BINS],
        }
    }

    /// The features of `samples` (16 kHz mono, oldest first) into `out`,
    /// bin-major: the last [`SAMPLES`] of them, or all of them after enough
    /// silence to make [`SAMPLES`].
    pub fn compute(&mut self, samples: &[f32], out: &mut [f32; FEATURES]) {
        self.normalise(samples);
        let mut peak = f32::NEG_INFINITY;
        for frame in 0..FRAMES {
            let start = frame * HOP;
            for ((slot, sample), weight) in self
                .frame
                .iter_mut()
                .zip(&self.padded[start..start + N_FFT])
                .zip(&self.window)
            {
                *slot = Complex::new(sample * weight, 0.0);
            }
            self.fft
                .process_with_scratch(&mut self.frame, &mut self.scratch);
            for (power, bin) in self.power.iter_mut().zip(&self.frame) {
                // numpy keeps the spectrum as complex64 before squaring it.
                let (re, im) = (f64::from(bin.re as f32), f64::from(bin.im as f32));
                *power = re.hypot(im).powi(2);
            }
            for (bin, filter) in self.filters.iter().enumerate() {
                let energy: f64 = filter
                    .weights
                    .iter()
                    .zip(&self.power[filter.first..])
                    .map(|(weight, power)| weight * power)
                    .sum();
                let value = energy.max(MEL_FLOOR).log10() as f32;
                peak = peak.max(value);
                out[bin * FRAMES + frame] = value;
            }
        }
        let floor = peak - 8.0;
        for value in out.iter_mut() {
            *value = (value.max(floor) + 4.0) / 4.0;
        }
    }

    /// The model's 8 s window of `samples`, normalised to zero mean and unit
    /// variance over the whole window (the left padding included, as
    /// upstream pads before it normalises), rounded to f32 as upstream's
    /// array is, then reflected `N_FFT / 2` samples into each side.
    fn normalise(&mut self, samples: &[f32]) {
        let heard = &samples[samples.len().saturating_sub(SAMPLES)..];
        let pad = SAMPLES - heard.len();
        let half = N_FFT / 2;
        let body = &mut self.padded[half..half + SAMPLES];
        body[..pad].fill(0.0);
        for (slot, sample) in body[pad..].iter_mut().zip(heard) {
            *slot = f64::from(*sample);
        }
        let mean = body.iter().sum::<f64>() / SAMPLES as f64;
        let variance = body.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / SAMPLES as f64;
        let scale = (variance + VARIANCE_GUARD).sqrt();
        for slot in body.iter_mut() {
            *slot = f64::from(((*slot - mean) / scale) as f32);
        }
        // numpy's "reflect": the edge sample itself is not repeated.
        for at in 0..half {
            self.padded[half - 1 - at] = self.padded[half + 1 + at];
            self.padded[half + SAMPLES + at] = self.padded[half + SAMPLES - 2 - at];
        }
    }
}

/// The periodic Hann window of `N_FFT` (`window_function(400, "hann")`).
fn hann() -> Vec<f64> {
    (0..N_FFT)
        .map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos())
        .collect()
}

/// Hertz to Slaney mels.
fn hertz_to_mel(hertz: f64) -> f64 {
    const MIN_LOG_HERTZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = 15.0;
    if hertz >= MIN_LOG_HERTZ {
        MIN_LOG_MEL + (hertz / MIN_LOG_HERTZ).ln() * (27.0 / 6.4f64.ln())
    } else {
        3.0 * hertz / 200.0
    }
}

/// Slaney mels to hertz.
fn mel_to_hertz(mel: f64) -> f64 {
    const MIN_LOG_HERTZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = 15.0;
    if mel >= MIN_LOG_MEL {
        MIN_LOG_HERTZ * ((6.4f64.ln() / 27.0) * (mel - MIN_LOG_MEL)).exp()
    } else {
        200.0 * mel / 3.0
    }
}

/// Whisper's 80 Slaney-normalised triangular filters from 0 to 8 kHz over
/// the FFT's bins (`mel_filter_bank(201, 80, 0, 8000, 16000, "slaney",
/// "slaney")`), each kept from its first non-zero weight to its last.
fn slaney_filters() -> Vec<Filter> {
    let top = hertz_to_mel(8000.0);
    let edges: Vec<f64> = (0..MEL_BINS + 2)
        .map(|at| mel_to_hertz(top * at as f64 / (MEL_BINS + 1) as f64))
        .collect();
    let bin_hertz = |bin: usize| 8000.0 * bin as f64 / (BINS - 1) as f64;
    (0..MEL_BINS)
        .map(|m| {
            let (low, centre, high) = (edges[m], edges[m + 1], edges[m + 2]);
            let norm = 2.0 / (high - low);
            let weight = |bin: usize| {
                let hertz = bin_hertz(bin);
                let down = (hertz - low) / (centre - low);
                let up = (high - hertz) / (high - centre);
                down.min(up).max(0.0) * norm
            };
            let first = (0..BINS).find(|&bin| weight(bin) > 0.0).unwrap_or(0);
            let last = (0..BINS)
                .rev()
                .find(|&bin| weight(bin) > 0.0)
                .unwrap_or(first);
            Filter {
                first,
                weights: (first..=last).map(weight).collect(),
            }
        })
        .collect()
}
