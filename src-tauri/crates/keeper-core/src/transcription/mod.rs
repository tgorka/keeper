//! On-device transcription with speaker recognition (AD-339…AD-350).
//!
//! Everything here is platform-free: the engine is a port ([`SpeechEngine`])
//! the macOS shell implements in-process over FluidAudio (AD-339) and every
//! other target answers `Unsupported`. This module plans what to transcribe
//! ([`plan`]), turns the engine's answers into a [`Transcript`]
//! ([`assemble`]), reads and plans writes to the synced voices bank and
//! dictionary ([`bank`], [`dictionary`]), applies a person's corrections
//! ([`corrections`]) and renders the markdown twin ([`render`]).
//!
//! - AD-340/341: models are hydrated from the account config repo's
//!   `_models/` into `<data_dir>/models/` ([`models`]); nothing downloads
//!   from anywhere else.
//! - AD-342/343: a drive "keeps voices" in a subfolder; the bank is one file
//!   per fact there, embeddings under a per-model prefix, clips model-free.
//! - AD-344: `transcript.json` (+ `.md`) beside the media.
//! - AD-345: beside system audio the microphone is the person recording;
//!   its echo of the far end is dropped. It is diarized too: the voice that
//!   is the bank's self person (or, failing that, talks most) is `ME`, any
//!   other voice in the room is a numbered speaker of its own.
//! - AD-346: speakers — `ME` included, when its voice is known — match people
//!   by centroid cosine ([`AUTO_MATCH`], [`SUGGEST`]); clusters of one track
//!   are one voice at [`SAME_VOICE`] and link across parts at [`LINK`].
//! - AD-347: only a person's confirmation writes to the bank; edits only
//!   suggest dictionary terms.
//! - AD-348/350: transcription after recording is on by default and happens
//!   on this Mac only — no network destination is added.

pub mod assemble;
pub mod bank;
pub mod corrections;
pub mod dictionary;
pub mod engine;
pub mod media;
pub mod model;
pub mod models;
pub mod plan;
pub mod progress;
pub mod render;
pub mod vm;
pub mod words;

pub use assemble::{assemble, best_clip, AssembleContext, PartResult, LINK, SAME_VOICE};
pub use bank::{
    wav_bytes, wav_samples, Bank, BankDelete, BankError, BankPlan, BankWrite, Confirmation,
    DictionaryTerm, EmbeddingSample, MatchResult, Naming, Person, SampleSource, Tombstone,
    VoiceSample, AUTO_MATCH, SUGGEST,
};
pub use corrections::{
    add_speaker, assign_speaker, edit_utterance, insert_utterance_after, merge_speakers,
    reassign_utterance, rename_speaker_label, split_utterance, CorrectionError,
};
pub use dictionary::DictionarySuggestion;
pub use engine::{
    AsrOutput, AsrToken, AudioTrackInfo, DiarOutput, DiarSegment, DiarSpeaker, EngineError,
    EngineUnavailable, SpeechEngine, TrackSelect, TranscriptionLanguage,
};
pub use model::{
    AppliedTerm, Candidate, ClipRef, EngineStamp, MatchStatus, PartTrack, SourceKind, SourcePart,
    Speaker, Transcript, TranscriptSource, Utterance, TRANSCRIPT_VERSION,
};
pub use models::{ModelSet, CONFIG_MODELS_DIR};
pub use plan::{
    plan_for_file, plan_for_session, transcript_source, PlanPart, PlanRefusal, TrackOrigin,
    TranscriptionPlan,
};
pub use words::Word;
