//! The speech-engine port (AD-339): what the shell's on-device engine does,
//! named without a single platform symbol.
//!
//! One implementation exists — the macOS shell's FluidAudio worker — and every
//! other target answers [`EngineUnavailable::Unsupported`] through an absent
//! port, so the command list is the same everywhere and nothing here is gated.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::models::ModelSet;

/// The on-device speech engine. Calls are blocking and may take minutes; the
/// shell runs them on a worker, never on the async runtime.
pub trait SpeechEngine: Send + Sync {
    /// Whether this machine can transcribe at all, before any model is asked
    /// for. `Ok` says nothing about the models; [`Self::load`] does.
    fn availability(&self) -> Result<(), EngineUnavailable>;
    /// Loads (idempotent) the model set found under `models_root` (= <data_dir>/models).
    fn load(&self, models_root: &Path, set: &ModelSet) -> Result<(), EngineError>;
    /// The audio tracks of a media file, in file order.
    fn audio_tracks(&self, media: &Path) -> Result<Vec<AudioTrackInfo>, EngineError>;
    /// 16 kHz mono f32. `range` in seconds within the file (None = whole).
    fn decode(
        &self,
        media: &Path,
        track: TrackSelect,
        range: Option<(f64, f64)>,
    ) -> Result<Vec<f32>, EngineError>;
    /// Speech to text with per-piece timings.
    fn transcribe(
        &self,
        samples: &[f32],
        language: TranscriptionLanguage,
    ) -> Result<AsrOutput, EngineError>;
    /// Who spoke when, plus one embedding per cluster.
    fn diarize(&self, samples: &[f32]) -> Result<DiarOutput, EngineError>;
    /// One embedding for a single-speaker clip (dominant speaker), None when too short/silent.
    fn embed(&self, samples: &[f32]) -> Result<Option<Vec<f32>>, EngineError>;
}

/// One audio track of a media file. `index` is the track's position among
/// the file's AUDIO tracks (0 = the first audio track), which is the only
/// identity keeper-rec's files give a track (G1 §4: no track metadata).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioTrackInfo {
    pub index: u32,
    pub channels: u32,
    pub duration_s: f64,
}

/// Which audio a decode reads: one track, or every audio track mixed down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSelect {
    Index(u32),
    MixAll,
}

impl TrackSelect {
    /// The track index a clip reference records: `None` for a mix.
    pub fn index(self) -> Option<u32> {
        match self {
            Self::Index(index) => Some(index),
            Self::MixAll => None,
        }
    }
}

/// One SentencePiece piece with its timing; '▁' marks a word start.
#[derive(Debug, Clone, PartialEq)]
pub struct AsrToken {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
}

/// The recognizer's answer for one buffer, times relative to its start.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AsrOutput {
    pub text: String,
    pub confidence: f32,
    pub tokens: Vec<AsrToken>,
}

/// A span one diarization cluster speaks in, relative to the buffer start.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarSegment {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
}

/// One diarization cluster's embedding (256-d for community-1).
#[derive(Debug, Clone, PartialEq)]
pub struct DiarSpeaker {
    pub speaker: String,
    pub embedding: Vec<f32>,
}

/// The diarizer's answer for one buffer. Cluster labels are local to it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiarOutput {
    pub segments: Vec<DiarSegment>,
    pub speakers: Vec<DiarSpeaker>,
}

/// The language transcription expects (`transcription.language`). Passed to
/// Parakeet as its token-language filter; `Auto` filters nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TranscriptionLanguage {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "pl")]
    Polish,
}

impl TranscriptionLanguage {
    /// The wire and settings spelling.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::Polish => "pl",
        }
    }

    /// Parse the wire spelling; anything else is `None`.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "en" => Some(Self::English),
            "pl" => Some(Self::Polish),
            _ => None,
        }
    }
}

/// Why this machine cannot transcribe. Each variant carries its own sentence
/// so the shell never composes refusal copy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineUnavailable {
    #[error("Transcription is available only in keeper for Mac.")]
    Unsupported,
    #[error("Transcription needs a Mac with Apple silicon.")]
    NeedsAppleSilicon,
    #[error("Transcription needs macOS {minimum} or later.")]
    NeedsNewerMacos { minimum: String },
    #[error("The transcription models are not on this Mac yet ({} file(s) missing).", missing.len())]
    ModelsMissing { missing: Vec<String> },
}

impl EngineUnavailable {
    /// The sentence a person reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }
}

/// An engine call that failed, with the engine's own words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_crosses_the_wire_as_its_settings_spelling() {
        for language in [
            TranscriptionLanguage::Auto,
            TranscriptionLanguage::English,
            TranscriptionLanguage::Polish,
        ] {
            let json = serde_json::to_string(&language).expect("serialize");
            assert_eq!(json, format!("\"{}\"", language.as_wire()));
            assert_eq!(
                TranscriptionLanguage::from_wire(language.as_wire()),
                Some(language)
            );
        }
        assert_eq!(TranscriptionLanguage::from_wire("de"), None);
    }
}
