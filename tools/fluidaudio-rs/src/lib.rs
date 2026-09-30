//! keeper's fork of `fluidaudio-rs`: FluidAudio 0.17.4 on-device speech
//! models, driven from Rust through a hand-written C ABI.
//!
//! - [`FluidAudio`] owns one engine: Parakeet TDT v3 (int8) speech
//!   recognition with per-token timings, and the pyannote community-1 offline
//!   diarizer with per-speaker embeddings. Both load only from directories the
//!   caller names — this crate never downloads a model.
//! - [`audio_tracks`] and [`decode_audio`] read media through AVAssetReader,
//!   which opens video containers (`.mov` with a video track) that
//!   `AVAudioFile` refuses.
//!
//! Every call blocks. [`FluidAudio`] is `Send` but not `Sync`, and each
//! method takes `&mut self`: one owner runs one model at a time, so
//! recognition and diarization never execute concurrently (concurrent Core ML
//! managers crash in BNNS, FluidAudio #661).

use std::path::Path;

use serde::Deserialize;

#[cfg(target_os = "macos")]
mod ffi;

/// Sample rate of every buffer this crate takes or returns (mono f32).
pub const SAMPLE_RATE: u32 = 16_000;

/// Files the recognizer needs inside its directory
/// (`<root>/parakeet-tdt-0.6b-v3`), `/`-separated.
pub const ASR_REQUIRED_FILES: &[&str] = &[
    "Preprocessor.mlmodelc/coremldata.bin",
    "Preprocessor.mlmodelc/model.mil",
    "Preprocessor.mlmodelc/weights/weight.bin",
    "Encoder.mlmodelc/coremldata.bin",
    "Encoder.mlmodelc/model.mil",
    "Encoder.mlmodelc/weights/weight.bin",
    "Decoder.mlmodelc/coremldata.bin",
    "Decoder.mlmodelc/model.mil",
    "Decoder.mlmodelc/weights/weight.bin",
    "JointDecisionv3.mlmodelc/coremldata.bin",
    "JointDecisionv3.mlmodelc/model.mil",
    "JointDecisionv3.mlmodelc/weights/weight.bin",
    "parakeet_vocab.json",
];

/// Files the diarizer needs inside its directory
/// (`<root>/speaker-diarization`), `/`-separated.
pub const DIARIZER_REQUIRED_FILES: &[&str] = &[
    "Segmentation.mlmodelc/coremldata.bin",
    "Segmentation.mlmodelc/model.mil",
    "Segmentation.mlmodelc/weights/weight.bin",
    "FBank.mlmodelc/coremldata.bin",
    "FBank.mlmodelc/model.mil",
    "FBank.mlmodelc/weights/weight.bin",
    "Embedding.mlmodelc/coremldata.bin",
    "Embedding.mlmodelc/model.mil",
    "Embedding.mlmodelc/weights/weight.bin",
    "PldaRho.mlmodelc/coremldata.bin",
    "PldaRho.mlmodelc/model.mil",
    "PldaRho.mlmodelc/weights/weight.bin",
    "plda-parameters.json",
];

/// The entries of `required` that are not files under `dir`.
pub fn missing_files(dir: &Path, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|relative| !dir.join(relative).is_file())
        .map(|relative| (*relative).to_owned())
        .collect()
}

/// What went wrong, in the engine's own words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A model directory lacks files; nothing was loaded and nothing fetched.
    #[error("model files missing in {dir}: {}", missing.join(", "))]
    MissingModels { dir: String, missing: Vec<String> },
    /// The Swift side threw; the text is FluidAudio's / AVFoundation's.
    #[error("{0}")]
    Bridge(String),
    /// The bridge answered with something this crate cannot read.
    #[error("unreadable bridge result: {0}")]
    Protocol(String),
    /// A path or argument that cannot cross the C boundary.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

/// One SentencePiece piece; a leading `▁` marks the start of a word.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Token {
    pub text: String,
    /// Seconds from the start of the buffer.
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
}

/// The recognizer's answer for one buffer.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Transcription {
    pub text: String,
    pub confidence: f32,
    pub tokens: Vec<Token>,
}

/// A span one speaker cluster speaks in, seconds from the buffer start.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SpeakerSegment {
    /// Cluster label local to this buffer (`S1`, `S2`, …).
    pub speaker: String,
    pub start: f64,
    pub end: f64,
}

/// A cluster's mean embedding (256-d for community-1).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SpeakerEmbedding {
    pub speaker: String,
    pub embedding: Vec<f32>,
}

/// Who spoke when, plus one embedding per cluster. Silence is empty.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Diarization {
    pub segments: Vec<SpeakerSegment>,
    pub speakers: Vec<SpeakerEmbedding>,
}

/// One audio track of a media file; `index` counts audio tracks only.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct AudioTrack {
    pub index: u32,
    pub channels: u32,
    /// Seconds.
    pub duration: f64,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Deserialize)]
struct EmbeddingReply {
    embedding: Option<Vec<f32>>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, Error> {
    serde_json::from_str(json).map_err(|error| Error::Protocol(error.to_string()))
}

/// Whether this build runs on Apple silicon — Parakeet refuses anything else.
pub fn is_apple_silicon() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

#[cfg(target_os = "macos")]
pub use engine::{audio_tracks, decode_audio, FluidAudio};

#[cfg(target_os = "macos")]
mod engine {
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::path::Path;
    use std::ptr::{self, NonNull};

    use super::{
        ffi, missing_files, parse, AudioTrack, Diarization, EmbeddingReply, Error, Transcription,
        ASR_REQUIRED_FILES, DIARIZER_REQUIRED_FILES,
    };

    /// One FluidAudio engine. Load the models, then call it from its owner.
    pub struct FluidAudio {
        handle: NonNull<c_void>,
        asr_loaded: bool,
        diarizer_loaded: bool,
    }

    // SAFETY: the Swift object has no thread affinity; it is only unsafe to
    // use from two threads at once, which `!Sync` plus `&mut self` rule out.
    unsafe impl Send for FluidAudio {}

    impl FluidAudio {
        pub fn new() -> Result<Self, Error> {
            // SAFETY: returns a retained Swift object pointer, owned by `Self`.
            let raw = unsafe { ffi::fluidaudio_engine_create() };
            let handle = NonNull::new(raw).ok_or_else(|| Error::Bridge("no engine".into()))?;
            Ok(Self {
                handle,
                asr_loaded: false,
                diarizer_loaded: false,
            })
        }

        pub fn is_asr_loaded(&self) -> bool {
            self.asr_loaded
        }

        pub fn is_diarizer_loaded(&self) -> bool {
            self.diarizer_loaded
        }

        /// Loads Parakeet TDT v3 int8 from `dir` (the `parakeet-tdt-0.6b-v3`
        /// folder itself). Refuses before touching Core ML when a file is
        /// missing. The first load of a model compiles it for the Neural
        /// Engine (tens of seconds); later processes reuse the OS cache.
        pub fn load_asr(&mut self, dir: &Path) -> Result<(), Error> {
            require(dir, ASR_REQUIRED_FILES)?;
            let dir = c_path(dir)?;
            // SAFETY: valid handle and NUL-terminated path for the call's duration.
            call(|err| unsafe {
                ffi::fluidaudio_load_asr(self.handle.as_ptr(), dir.as_ptr(), err)
            })?;
            self.asr_loaded = true;
            Ok(())
        }

        /// Transcribes 16 kHz mono samples. `language` (`"en"`, `"pl"`, any
        /// FluidAudio `Language` code) turns on Parakeet's script filter;
        /// `None` lets the model choose. Buffers shorter than 0.3 s — below
        /// Parakeet's minimum — are an empty transcription.
        pub fn transcribe(
            &mut self,
            samples: &[f32],
            language: Option<&str>,
        ) -> Result<Transcription, Error> {
            let language = language
                .map(|code| {
                    CString::new(code).map_err(|_| Error::InvalidArgument("language".into()))
                })
                .transpose()?;
            let language_ptr = language.as_ref().map_or(ptr::null(), |code| code.as_ptr());
            let json = call_json(|out, err| unsafe {
                // SAFETY: samples/len describe a live slice; language is null or a live C string.
                ffi::fluidaudio_transcribe(
                    self.handle.as_ptr(),
                    samples.as_ptr(),
                    samples.len(),
                    language_ptr,
                    out,
                    err,
                )
            })?;
            parse(&json)
        }

        /// Loads the pyannote community-1 offline diarizer from `dir` (the
        /// `speaker-diarization` folder itself), never downloading.
        pub fn load_diarizer(&mut self, dir: &Path) -> Result<(), Error> {
            require(dir, DIARIZER_REQUIRED_FILES)?;
            let dir = c_path(dir)?;
            // SAFETY: valid handle and NUL-terminated path for the call's duration.
            call(|err| unsafe {
                ffi::fluidaudio_load_diarizer(self.handle.as_ptr(), dir.as_ptr(), err)
            })?;
            self.diarizer_loaded = true;
            Ok(())
        }

        /// Diarizes 16 kHz mono samples: segments plus one mean embedding per
        /// speaker cluster.
        pub fn diarize(&mut self, samples: &[f32]) -> Result<Diarization, Error> {
            let json = call_json(|out, err| unsafe {
                // SAFETY: samples/len describe a live slice.
                ffi::fluidaudio_diarize(
                    self.handle.as_ptr(),
                    samples.as_ptr(),
                    samples.len(),
                    out,
                    err,
                )
            })?;
            parse(&json)
        }

        /// The dominant speaker's embedding in a single-speaker clip, from the
        /// same pipeline as [`Self::diarize`]; `None` when it finds no speech.
        pub fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, Error> {
            let json = call_json(|out, err| unsafe {
                // SAFETY: samples/len describe a live slice.
                ffi::fluidaudio_embed(
                    self.handle.as_ptr(),
                    samples.as_ptr(),
                    samples.len(),
                    out,
                    err,
                )
            })?;
            Ok(parse::<EmbeddingReply>(&json)?.embedding)
        }
    }

    impl Drop for FluidAudio {
        fn drop(&mut self) {
            // SAFETY: releases the reference `fluidaudio_engine_create` retained.
            unsafe { ffi::fluidaudio_engine_destroy(self.handle.as_ptr()) }
        }
    }

    /// The audio tracks of `media`, in file order.
    pub fn audio_tracks(media: &Path) -> Result<Vec<AudioTrack>, Error> {
        let media = c_path(media)?;
        // SAFETY: NUL-terminated path for the call's duration.
        let json = call_json(|out, err| unsafe {
            ffi::fluidaudio_audio_tracks(media.as_ptr(), out, err)
        })?;
        parse(&json)
    }

    /// Decodes `media` to 16 kHz mono f32: one audio track (`Some(index)`)
    /// or every audio track mixed (`None`), over `range` seconds or whole.
    pub fn decode_audio(
        media: &Path,
        track: Option<u32>,
        range: Option<(f64, f64)>,
    ) -> Result<Vec<f32>, Error> {
        let media = c_path(media)?;
        let track = match track {
            None => -1,
            Some(index) => i32::try_from(index)
                .map_err(|_| Error::InvalidArgument(format!("track {index}")))?,
        };
        let (start, end) = match range {
            Some((start, end)) if start >= 0.0 && end > start => (start, end),
            Some((start, end)) => {
                return Err(Error::InvalidArgument(format!("range {start}..{end}")))
            }
            None => (-1.0, -1.0),
        };
        let mut samples: Vec<f32> = Vec::new();
        call(|err| unsafe {
            // SAFETY: `samples` outlives the call and is only touched by `sink`.
            ffi::fluidaudio_decode(
                media.as_ptr(),
                track,
                start,
                end,
                (&mut samples as *mut Vec<f32>).cast(),
                sink,
                err,
            )
        })?;
        Ok(samples)
    }

    extern "C" fn sink(context: *mut c_void, data: *const f32, len: usize) {
        if context.is_null() || data.is_null() || len == 0 {
            return;
        }
        // SAFETY: `context` is the `Vec<f32>` `decode_audio` passed; `data`
        // points at `len` floats valid for this callback.
        unsafe {
            let samples = &mut *context.cast::<Vec<f32>>();
            samples.extend_from_slice(std::slice::from_raw_parts(data, len));
        }
    }

    fn require(dir: &Path, required: &[&str]) -> Result<(), Error> {
        let missing = missing_files(dir, required);
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::MissingModels {
                dir: dir.display().to_string(),
                missing,
            })
        }
    }

    fn c_path(path: &Path) -> Result<CString, Error> {
        let text = path
            .to_str()
            .ok_or_else(|| Error::InvalidArgument(format!("non-UTF-8 path {}", path.display())))?;
        CString::new(text).map_err(|_| Error::InvalidArgument(format!("path {text:?}")))
    }

    /// Takes ownership of a bridge-allocated string.
    fn take_string(raw: *mut c_char) -> Option<String> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: the bridge hands out strdup'd, NUL-terminated UTF-8, freed once here.
        let text = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        unsafe { ffi::fluidaudio_free_string(raw) };
        Some(text)
    }

    fn call(f: impl FnOnce(*mut *mut c_char) -> i32) -> Result<(), Error> {
        let mut err: *mut c_char = ptr::null_mut();
        let status = f(&mut err);
        let message = take_string(err);
        if status == 0 {
            Ok(())
        } else {
            Err(Error::Bridge(message.unwrap_or_else(|| {
                format!("bridge call failed ({status})")
            })))
        }
    }

    fn call_json(
        f: impl FnOnce(*mut *mut c_char, *mut *mut c_char) -> i32,
    ) -> Result<String, Error> {
        let mut out: *mut c_char = ptr::null_mut();
        call(|err| f(&mut out, err))?;
        take_string(out).ok_or_else(|| Error::Protocol("empty result".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_names_exactly_the_absent_ones() {
        let dir = std::env::temp_dir().join(format!("fluidaudio-rs-test-{}", std::process::id()));
        let weights = dir.join("Encoder.mlmodelc/weights");
        std::fs::create_dir_all(&weights).unwrap();
        std::fs::write(dir.join("Encoder.mlmodelc/coremldata.bin"), b"").unwrap();
        std::fs::write(dir.join("parakeet_vocab.json"), b"{}").unwrap();
        // A directory where a file belongs does not count as present.
        std::fs::create_dir_all(dir.join("Encoder.mlmodelc/model.mil")).unwrap();

        let missing = missing_files(&dir, ASR_REQUIRED_FILES);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(!missing.contains(&"Encoder.mlmodelc/coremldata.bin".to_owned()));
        assert!(!missing.contains(&"parakeet_vocab.json".to_owned()));
        assert!(missing.contains(&"Encoder.mlmodelc/model.mil".to_owned()));
        assert!(missing.contains(&"Encoder.mlmodelc/weights/weight.bin".to_owned()));
        assert_eq!(missing.len(), ASR_REQUIRED_FILES.len() - 2);
    }

    #[test]
    fn bridge_json_parses_into_the_public_types() {
        let asr: Transcription = parse(
            r#"{"text":"Hi","confidence":0.9,"tokens":[{"text":"▁Hi","start":0.08,"end":0.4,"confidence":0.95}]}"#,
        )
        .unwrap();
        assert_eq!(asr.tokens[0].text, "▁Hi");
        let diar: Diarization = parse(
            r#"{"segments":[{"speaker":"S1","start":0,"end":1.5}],"speakers":[{"speaker":"S1","embedding":[0.5,-0.5]}]}"#,
        )
        .unwrap();
        assert_eq!(diar.speakers[0].embedding, vec![0.5, -0.5]);
        let none: EmbeddingReply = parse(r#"{"embedding":null}"#).unwrap();
        assert!(none.embedding.is_none());
        assert!(matches!(
            parse::<Transcription>("{}"),
            Err(Error::Protocol(_))
        ));
    }
}
