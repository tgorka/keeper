//! The macOS speech engine (AD-339): `keeper_core::transcription::SpeechEngine`
//! on FluidAudio, through the vendored `tools/fluidaudio-rs` fork.
//!
//! One dedicated worker thread owns the FluidAudio handle for the engine's
//! whole life. Loading, recognition, diarization and embedding are requests
//! sent to it over a channel, each answered on its own reply channel, so they
//! run strictly one after another: Core ML managers running concurrently
//! crash in BNNS (FluidAudio #661). Decoding media needs no model and runs on
//! the caller's thread through AVAssetReader.
//!
//! Models load only from `<data_dir>/models` and only when every file is
//! present; the fork never downloads (AD-341). The FFI `unsafe` lives in the
//! fork, so this module is `unsafe`-free.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::LazyLock;
use std::time::Instant;

use fluidaudio_rs::FluidAudio;
use keeper_core::transcription::models::missing;
use keeper_core::transcription::{
    AsrOutput, AsrToken, AudioTrackInfo, DiarOutput, DiarSegment, DiarSpeaker, EngineError,
    EngineUnavailable, ModelSet, SpeechEngine, TrackSelect, TranscriptionLanguage,
};

/// The oldest macOS the engine runs on: the community-1 diarizer crashes in
/// BNNS on macOS 14 every time (FluidAudio #878, fixed in 15).
const MINIMUM_MACOS: u32 = 15;

/// The on-device engine. Cheap to construct: the worker thread starts at once
/// but creates the FluidAudio handle only for its first request.
pub struct MacSpeechEngine {
    requests: mpsc::Sender<Request>,
}

enum Request {
    Load {
        root: PathBuf,
        set: ModelSet,
        reply: mpsc::SyncSender<Result<(), EngineError>>,
    },
    Transcribe {
        samples: Vec<f32>,
        language: TranscriptionLanguage,
        reply: mpsc::SyncSender<Result<AsrOutput, EngineError>>,
    },
    Diarize {
        samples: Vec<f32>,
        reply: mpsc::SyncSender<Result<DiarOutput, EngineError>>,
    },
    Embed {
        samples: Vec<f32>,
        reply: mpsc::SyncSender<Result<Option<Vec<f32>>, EngineError>>,
    },
}

impl MacSpeechEngine {
    pub fn new() -> Result<Self, EngineError> {
        let (requests, inbox) = mpsc::channel();
        std::thread::Builder::new()
            .name("keeper-transcribe".into())
            .spawn(move || Worker::default().run(inbox))
            .map_err(|error| {
                EngineError(format!("cannot start the transcription worker: {error}"))
            })?;
        Ok(Self { requests })
    }

    /// Sends one request and blocks for its answer.
    fn ask<T>(
        &self,
        build: impl FnOnce(mpsc::SyncSender<Result<T, EngineError>>) -> Request,
    ) -> Result<T, EngineError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .send(build(reply))
            .map_err(|_| worker_gone())?;
        answer.recv().map_err(|_| worker_gone())?
    }
}

fn worker_gone() -> EngineError {
    EngineError("the transcription worker stopped".to_owned())
}

impl SpeechEngine for MacSpeechEngine {
    fn availability(&self) -> Result<(), EngineUnavailable> {
        if !fluidaudio_rs::is_apple_silicon() {
            return Err(EngineUnavailable::NeedsAppleSilicon);
        }
        if !macos_supports_transcription(macos_major()) {
            return Err(EngineUnavailable::NeedsNewerMacos {
                minimum: MINIMUM_MACOS.to_string(),
            });
        }
        Ok(())
    }

    fn load(&self, models_root: &Path, set: &ModelSet) -> Result<(), EngineError> {
        self.ask(|reply| Request::Load {
            root: models_root.to_path_buf(),
            set: set.clone(),
            reply,
        })
    }

    fn audio_tracks(&self, media: &Path) -> Result<Vec<AudioTrackInfo>, EngineError> {
        let tracks = fluidaudio_rs::audio_tracks(media).map_err(engine_error)?;
        Ok(tracks
            .into_iter()
            .map(|track| AudioTrackInfo {
                index: track.index,
                channels: track.channels,
                duration_s: track.duration,
            })
            .collect())
    }

    fn decode(
        &self,
        media: &Path,
        track: TrackSelect,
        range: Option<(f64, f64)>,
    ) -> Result<Vec<f32>, EngineError> {
        let started = Instant::now();
        let samples =
            fluidaudio_rs::decode_audio(media, track.index(), range).map_err(engine_error)?;
        tracing::info!(
            media = %media.display(),
            ?track,
            ?range,
            audio_s = samples.len() as f64 / f64::from(fluidaudio_rs::SAMPLE_RATE),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "transcription: decoded"
        );
        Ok(samples)
    }

    fn transcribe(
        &self,
        samples: &[f32],
        language: TranscriptionLanguage,
    ) -> Result<AsrOutput, EngineError> {
        // The worker owns the handle, so the buffer crosses by value.
        let samples = samples.to_vec();
        self.ask(|reply| Request::Transcribe {
            samples,
            language,
            reply,
        })
    }

    fn diarize(&self, samples: &[f32]) -> Result<DiarOutput, EngineError> {
        let samples = samples.to_vec();
        self.ask(|reply| Request::Diarize { samples, reply })
    }

    fn embed(&self, samples: &[f32]) -> Result<Option<Vec<f32>>, EngineError> {
        let samples = samples.to_vec();
        self.ask(|reply| Request::Embed { samples, reply })
    }
}

/// The worker's state: the one FluidAudio handle and what it has loaded.
#[derive(Default)]
struct Worker {
    engine: Option<FluidAudio>,
    /// The root, the set and the hydration's completion digest they were
    /// loaded at: weights replaced in place under the same directory names
    /// change the digest, so they are loaded afresh.
    loaded: Option<(PathBuf, ModelSet, Option<String>)>,
}

impl Worker {
    fn run(mut self, inbox: mpsc::Receiver<Request>) {
        // Ends when the engine (every sender) is dropped; the handle goes with it.
        for request in inbox {
            // A caller that stopped waiting is not an error; its answer is dropped.
            match request {
                Request::Load { root, set, reply } => {
                    let _ = reply.send(self.load(root, set));
                }
                Request::Transcribe {
                    samples,
                    language,
                    reply,
                } => {
                    let _ = reply.send(self.transcribe(&samples, language));
                }
                Request::Diarize { samples, reply } => {
                    let _ = reply.send(self.diarize(&samples));
                }
                Request::Embed { samples, reply } => {
                    let _ = reply.send(self.embed(&samples));
                }
            }
        }
        tracing::debug!("transcription: worker stopped");
    }

    fn engine(&mut self) -> Result<&mut FluidAudio, EngineError> {
        if self.engine.is_none() {
            self.engine = Some(FluidAudio::new().map_err(engine_error)?);
        }
        self.engine
            .as_mut()
            .ok_or_else(|| EngineError("the engine could not be created".to_owned()))
    }

    /// The loaded engine, or the sentence saying models were never loaded.
    fn loaded_engine(&mut self) -> Result<&mut FluidAudio, EngineError> {
        if self.loaded.is_none() {
            return Err(EngineError(
                "the transcription models are not loaded".to_owned(),
            ));
        }
        self.engine()
    }

    /// Whether exactly this set, as this hydration left it, is loaded.
    fn holds(&self, root: &Path, set: &ModelSet, digest: Option<&str>) -> bool {
        self.loaded
            .as_ref()
            .is_some_and(|(loaded_root, loaded_set, loaded_digest)| {
                loaded_root == root && loaded_set == set && loaded_digest.as_deref() == digest
            })
    }

    fn load(&mut self, root: PathBuf, set: ModelSet) -> Result<(), EngineError> {
        let digest = keeper_sync::config_repo::completion_digest(
            &root,
            keeper_core::transcription::models::TRANSCRIPTION_GROUP,
        );
        if self.holds(&root, &set, digest.as_deref()) {
            return Ok(());
        }
        let absent = missing(&root, &set);
        if !absent.is_empty() {
            return Err(EngineError(format!(
                "the transcription models are incomplete ({} file(s) missing, first {})",
                absent.len(),
                absent[0]
            )));
        }
        // A half-finished reload must not leave the old set marked as loaded.
        self.loaded = None;
        let engine = self.engine()?;
        let started = Instant::now();
        engine
            .load_asr(&root.join(&set.asr_dir))
            .map_err(engine_error)?;
        let asr_ms = started.elapsed().as_millis() as u64;
        let started = Instant::now();
        engine
            .load_diarizer(&root.join(&set.diarizer_dir))
            .map_err(engine_error)?;
        tracing::info!(
            root = %root.display(),
            asr = %set.asr_dir,
            diarizer = %set.diarizer_dir,
            asr_ms,
            diarizer_ms = started.elapsed().as_millis() as u64,
            "transcription: models loaded"
        );
        self.loaded = Some((root, set, digest));
        Ok(())
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        language: TranscriptionLanguage,
    ) -> Result<AsrOutput, EngineError> {
        let engine = self.loaded_engine()?;
        let started = Instant::now();
        let result = engine
            .transcribe(samples, language_code(language))
            .map_err(engine_error)?;
        tracing::info!(
            audio_s = audio_seconds(samples),
            elapsed_ms = started.elapsed().as_millis() as u64,
            tokens = result.tokens.len(),
            language = language.as_wire(),
            "transcription: recognized"
        );
        Ok(AsrOutput {
            text: result.text,
            confidence: result.confidence,
            tokens: result
                .tokens
                .into_iter()
                .map(|token| AsrToken {
                    text: token.text,
                    start: token.start,
                    end: token.end,
                    confidence: token.confidence,
                })
                .collect(),
        })
    }

    fn diarize(&mut self, samples: &[f32]) -> Result<DiarOutput, EngineError> {
        let engine = self.loaded_engine()?;
        let started = Instant::now();
        let result = engine.diarize(samples).map_err(engine_error)?;
        tracing::info!(
            audio_s = audio_seconds(samples),
            elapsed_ms = started.elapsed().as_millis() as u64,
            segments = result.segments.len(),
            speakers = result.speakers.len(),
            "transcription: diarized"
        );
        Ok(DiarOutput {
            segments: result
                .segments
                .into_iter()
                .map(|segment| DiarSegment {
                    speaker: segment.speaker,
                    start: segment.start,
                    end: segment.end,
                })
                .collect(),
            speakers: result
                .speakers
                .into_iter()
                .map(|speaker| DiarSpeaker {
                    speaker: speaker.speaker,
                    embedding: speaker.embedding,
                })
                .collect(),
        })
    }

    fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, EngineError> {
        let engine = self.loaded_engine()?;
        let started = Instant::now();
        let embedding = engine.embed(samples).map_err(engine_error)?;
        tracing::info!(
            audio_s = audio_seconds(samples),
            elapsed_ms = started.elapsed().as_millis() as u64,
            found = embedding.is_some(),
            "transcription: embedded"
        );
        Ok(embedding)
    }
}

fn engine_error(error: fluidaudio_rs::Error) -> EngineError {
    EngineError(error.to_string())
}

fn audio_seconds(samples: &[f32]) -> f64 {
    samples.len() as f64 / f64::from(fluidaudio_rs::SAMPLE_RATE)
}

/// Parakeet's token-language filter code; `Auto` filters nothing.
fn language_code(language: TranscriptionLanguage) -> Option<&'static str> {
    match language {
        TranscriptionLanguage::Auto => None,
        TranscriptionLanguage::English => Some("en"),
        TranscriptionLanguage::Polish => Some("pl"),
    }
}

/// The version floor: a known macOS major ≥ [`MINIMUM_MACOS`]. An unreadable
/// probe is `false` — safe-hide, as for recording (`macos_version.rs`).
fn macos_supports_transcription(major: Option<u32>) -> bool {
    matches!(major, Some(major) if major >= MINIMUM_MACOS)
}

/// The running macOS major from `sw_vers -productVersion`, probed once.
fn macos_major() -> Option<u32> {
    static MAJOR: LazyLock<Option<u32>> = LazyLock::new(|| {
        let major = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|version| crate::macos_version::parse_macos_major(&version));
        if major.is_none() {
            tracing::warn!("macOS version probe failed; hiding transcription (safe-hide)");
        }
        major
    });
    *MAJOR
}

#[cfg(test)]
mod tests {
    use super::{macos_supports_transcription, Worker};
    use crate::macos_version::parse_macos_major;
    use keeper_core::transcription::ModelSet;
    use std::path::{Path, PathBuf};

    /// The diarizer's macOS 14 crash sets the floor: 14 and an unreadable
    /// probe refuse, 15 and later (including 26's jump) pass.
    #[test]
    fn transcription_floor_is_macos_15() {
        assert!(!macos_supports_transcription(parse_macos_major("14.8.7")));
        assert!(!macos_supports_transcription(None));
        assert!(macos_supports_transcription(parse_macos_major("15.0")));
        assert!(macos_supports_transcription(parse_macos_major("26.1")));
    }

    /// Weights replaced in place under the same names (a new completion
    /// digest) are loaded afresh; the same hydration is not loaded twice.
    #[test]
    fn a_new_hydration_of_the_same_set_is_loaded_again() {
        let root = PathBuf::from("/data/models");
        let set = ModelSet::default();
        let worker = Worker {
            engine: None,
            loaded: Some((root.clone(), set.clone(), Some("aa".to_owned()))),
        };
        assert!(worker.holds(&root, &set, Some("aa")));
        assert!(
            !worker.holds(&root, &set, Some("bb")),
            "the weights changed"
        );
        assert!(!worker.holds(&root, &set, None), "a hydration is part-way");
        assert!(!worker.holds(Path::new("/elsewhere"), &set, Some("aa")));
        assert!(!Worker::default().holds(&root, &set, Some("aa")));
    }
}
