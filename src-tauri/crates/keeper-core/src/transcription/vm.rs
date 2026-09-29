//! The transcription surface's view models (IPC, ts-rs).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::bank::{Bank, DictionaryTerm};
use super::dictionary::DictionarySuggestion;
use super::engine::TranscriptionLanguage;
use super::model::Transcript;
use super::plan::TrackOrigin;

/// Settings → Transcription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptionStatusVm {
    /// Whether this machine can transcribe (AD-349).
    pub available: bool,
    /// Why not, as a sentence.
    pub reason: Option<String>,
    pub models: ModelsStateVm,
    pub language: TranscriptionLanguage,
    pub after_recording: bool,
    pub voices_drives: Vec<VoicesDriveVm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelsStateVm {
    pub state: ModelsState,
    pub sentence: String,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ModelsState {
    Ready,
    Missing,
    Fetching,
    Failed,
    NoAccount,
}

/// A drive that keeps voices (AD-342).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VoicesDriveVm {
    pub profile_id: String,
    pub name: String,
    /// The drive's local folder, absolute.
    pub local_path: String,
    pub voices_root: String,
    pub subfolder: String,
}

/// One batch of a transcription job's progress. A running job sends one every
/// second; `fraction` never goes backwards within a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptionProgressVm {
    pub job_id: String,
    pub phase: TranscriptionPhase,
    /// 1-based part being worked on.
    pub part: u32,
    pub parts: u32,
    pub message: Option<String>,
    /// Set on `done`.
    pub transcript_path: Option<String>,
    /// How much of the job is done, 0..=1 — an estimate from the audio's
    /// length ([`super::progress::Estimate`]); `null` before the job knows
    /// what it will hear.
    pub fraction: Option<f32>,
    /// Milliseconds since the job started running.
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// Set on a `failed` batch that stopped at a transcript keeper will not
    /// overwrite on its own (corrected, or unreadable): starting the job
    /// again with `replace` gets past it.
    pub replaceable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TranscriptionPhase {
    Queued,
    Decoding,
    Transcribing,
    Diarizing,
    Matching,
    Writing,
    Done,
    Failed,
    Cancelled,
}

/// The transcript viewer's model: the file, and the bank's people to assign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptVm {
    pub path: String,
    /// The media file or session folder to start a job on to transcribe
    /// this again (`transcription_start` with `replace`); `null` when it is
    /// no longer there.
    pub source_path: Option<String>,
    pub transcript: Transcript,
    pub people: Vec<PersonVm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PersonVm {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub is_self: bool,
    /// Voice clips kept for this person.
    pub samples: u32,
    /// Whether the current embedding model can recognize them yet.
    pub has_embedding_for_model: bool,
}

impl PersonVm {
    /// Every live person of `bank`, as the surface lists them.
    pub fn list(bank: &Bank, model: &str) -> Vec<Self> {
        bank.people
            .iter()
            .map(|person| {
                let (samples, has_embedding_for_model) = bank.sample_facts(&person.id, model);
                Self {
                    id: person.id.clone(),
                    name: person.name.clone(),
                    aliases: person.aliases.clone(),
                    is_self: person.is_self,
                    samples,
                    has_embedding_for_model,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictionaryTermVm {
    pub id: String,
    pub text: String,
    pub aliases: Vec<String>,
}

impl From<&DictionaryTerm> for DictionaryTermVm {
    fn from(term: &DictionaryTerm) -> Self {
        Self {
            id: term.id.clone(),
            text: term.text.clone(),
            aliases: term.aliases.clone(),
        }
    }
}

/// A correction's answer: the saved transcript and what the dictionary
/// could learn from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CorrectionResultVm {
    pub transcript: TranscriptVm,
    pub suggestions: Vec<DictionarySuggestion>,
}

/// What the transcript viewer's player plays: the transcript's media, part
/// by part, on the transcript's one timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptMediaVm {
    pub parts: Vec<TranscriptMediaPartVm>,
    /// Some part has a camera video.
    pub has_camera: bool,
    /// Some part's main file is a video.
    pub has_screen: bool,
}

/// One media file of the transcript and the camera recorded beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptMediaPartVm {
    /// Relative to the transcript's directory, as in `source.parts`.
    pub file: String,
    /// Seconds from the transcript's start to this part's start.
    pub offset: f64,
    /// Seconds.
    pub duration: f64,
    /// The part's own file — the screen video, or the audio when nothing was
    /// filmed (`kind` says which). `null` when no synced folder holds it, so
    /// the webview cannot be served it.
    pub screen: Option<MediaRef>,
    /// The session's camera segment with this part's index, when there is one.
    pub camera: Option<MediaRef>,
    /// The part file's audio tracks and what each was heard as; empty for a
    /// file whose tracks were heard mixed.
    pub audio_tracks: Vec<MediaAudioTrackVm>,
}

/// Where the webview is served one media file from: a synced folder and the
/// path inside it — the coordinates the Files media viewer turns into a
/// `keeper-file://` URL (`fileAssetUrl`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaRef {
    pub profile_id: String,
    /// `/`-separated, relative to the profile's folder.
    pub relative_path: String,
    pub kind: MediaRefKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MediaRefKind {
    Video,
    Audio,
}

/// One audio track of a part file: its index among the file's audio tracks
/// (`HTMLMediaElement.audioTracks`) and what it was heard as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaAudioTrackVm {
    pub index: u32,
    pub origin: TrackOrigin,
}
