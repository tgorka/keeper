//! The transcription surface's view models (IPC, ts-rs).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::bank::{Bank, DictionaryTerm};
use super::dictionary::DictionarySuggestion;
use super::engine::TranscriptionLanguage;
use super::model::Transcript;
use super::plan::TrackOrigin;
use crate::notes::media_block::{MediaPicture, MediaSound};

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

/// `keeper://transcript-written`'s payload (AD-357): a transcript file was
/// written — a job's result, a correction, or a redo's fresh file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptWrittenVm {
    /// The transcript's absolute path, as `TranscriptVm.path` and
    /// `MediaBlockVm.transcriptPath` spell it.
    pub path: String,
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
    /// Relative to the transcript's directory, as in `source.parts` — or,
    /// for a media block's `[[part]]`, the path the block names.
    pub file: String,
    /// Seconds from the transcript's start to this part's start.
    pub offset: f64,
    /// Seconds. `0` when nothing has measured the file yet — a recording
    /// not transcribed whose manifest has no sample bounds, or a `[[part]]`
    /// with no transcript — and the player reads it off the media.
    pub duration: f64,
    /// The part's own file — the screen video, or the audio when nothing was
    /// filmed (`kind` says which). `null` when neither a synced folder nor
    /// the recordings index serves it, so the webview cannot be served it.
    pub screen: Option<MediaRef>,
    /// The session's camera segment with this part's index, when there is one.
    pub camera: Option<MediaRef>,
    /// The part file's audio tracks and what each was heard as; empty for a
    /// file whose tracks were heard mixed.
    pub audio_tracks: Vec<MediaAudioTrackVm>,
    /// Whether the part's bytes are on this device. `false` for a pointer
    /// the sync has not downloaded: the player says so and never hands it to
    /// a `<video>` (88.1).
    pub here: bool,
}

/// Where the webview is served one media file from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "via", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum MediaRef {
    /// A synced folder and the path inside it — the coordinates the Files
    /// media viewer turns into a `keeper-file://` URL (`fileAssetUrl`).
    File {
        profile_id: String,
        /// `/`-separated, relative to the profile's folder.
        relative_path: String,
        kind: MediaRefKind,
    },
    /// A file of an indexed recording outside every synced folder, served
    /// over `keeper-recording://` by the recording's identity, as a
    /// recording note's embeds always were (AD-353).
    Recording {
        session_id: String,
        /// `/`-separated, relative to the recordings destination root — the
        /// frame `recording_note_targets` answers in.
        relative_path: String,
        kind: MediaRefKind,
    },
}

impl MediaRef {
    pub fn kind(&self) -> MediaRefKind {
        match self {
            Self::File { kind, .. } | Self::Recording { kind, .. } => *kind,
        }
    }
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

/// A media block in a note, resolved (AD-353, AD-359): what it plays, the
/// transcript's lines inside its window, and its markers. Text only — no
/// word timings, embeddings or candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaBlockVm {
    /// The block's `title`, else the recording's or the transcript's.
    pub title: Option<String>,
    /// The recording the block plays, when it plays one.
    pub session_id: Option<String>,
    /// The transcript's absolute path — the one `keeper://transcript-written`
    /// carries — and, before it is written, where it will be. `null` for
    /// media no single transcript belongs to.
    pub transcript_path: Option<String>,
    /// Whether that transcript exists.
    pub transcribed: bool,
    /// What `transcription_start` takes to transcribe this, when it has no
    /// transcript yet and can have one; `null` otherwise.
    pub transcribe_path: Option<String>,
    /// Seconds on the source's clock; `0` when not known yet.
    pub duration: f64,
    pub window: MediaWindowVm,
    pub picture: Option<MediaPicture>,
    pub sound: Option<MediaSound>,
    pub media: TranscriptMediaVm,
    /// The transcript's lines overlapping the window, in order.
    pub lines: Vec<MediaLineVm>,
    pub speakers: Vec<MediaSpeakerVm>,
    pub markers: Vec<MediaMarkerVm>,
}

/// `[from, to)` in seconds; `to` is `null` for the end.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaWindowVm {
    pub from: f64,
    pub to: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaLineVm {
    pub id: String,
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaSpeakerVm {
    pub id: String,
    /// The name a person reads: a name, "You", or "Speaker N".
    pub name: String,
    pub origin: TrackOrigin,
}

/// A named moment (`to` is `null`) or a named window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaMarkerVm {
    pub name: String,
    pub from: f64,
    pub to: Option<f64>,
}
