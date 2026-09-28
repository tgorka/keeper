//! The transcription surface's view models (IPC, ts-rs).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::bank::{Bank, DictionaryTerm};
use super::dictionary::DictionarySuggestion;
use super::engine::TranscriptionLanguage;
use super::model::Transcript;

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

/// One batch of a transcription job's progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
