//! The transcript file format (AD-344): `transcript.json` beside a session's
//! segments, `<name.ext>.transcript.json` beside any other media. It is the source
//! of truth — the markdown twin is re-rendered from it on every save — and the
//! same type crosses IPC as the viewer's model.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::engine::TranscriptionLanguage;
use super::plan::TrackOrigin;
use super::words::Word;

/// The transcript schema version.
pub const TRANSCRIPT_VERSION: u32 = 1;

/// The speaker id the microphone track is attributed to (AD-345).
pub const SELF_SPEAKER_ID: &str = "ME";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Transcript {
    pub version: u32,
    pub source: TranscriptSource,
    /// ISO-8601 with offset, supplied by the shell.
    pub created_at: String,
    pub engine: EngineStamp,
    pub language: TranscriptionLanguage,
    /// Seconds.
    pub duration: f64,
    pub speakers: Vec<Speaker>,
    pub utterances: Vec<Utterance>,
    pub dictionary_applied: Vec<AppliedTerm>,
    /// A person changed something in this transcript — an edit, a reassign,
    /// a merge, a rename or a confirmation. Set by every correction and never
    /// cleared; see [`Transcript::is_corrected`].
    #[serde(default)]
    pub corrected: bool,
}

/// What was transcribed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptSource {
    pub kind: SourceKind,
    /// The media files, relative to the transcript's directory.
    pub files: Vec<String>,
    /// Where each file sits on the transcript's timeline and which of its
    /// tracks were heard as what — what turns a speaker's time span back into
    /// a clip of one file's one track ([`super::assemble::best_clip`]).
    #[serde(default)]
    pub parts: Vec<SourcePart>,
    /// What the transcript is of, for its heading: the recording session's
    /// folder name, or the file's name. Absent in older files, where the
    /// heading falls back to the first media file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
}

/// One media file on the transcript timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourcePart {
    /// Relative to the transcript's directory.
    pub file: String,
    /// Seconds from the transcript's start to this file's start.
    pub offset: f64,
    /// Seconds of audio in this file.
    pub duration: f64,
    pub tracks: Vec<PartTrack>,
}

/// One track of a [`SourcePart`] and the role it was heard in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartTrack {
    /// The audio-track index; `null` for all tracks mixed.
    pub track: Option<u32>,
    pub origin: TrackOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SourceKind {
    Recording,
    File,
}

/// Which models produced the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineStamp {
    pub asr: String,
    pub diarizer: String,
    /// The embedding model id speaker embeddings (and bank matches) belong to.
    pub embedding: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Speaker {
    /// `S1`, `S2`… for diarized clusters, [`SELF_SPEAKER_ID`] for the microphone.
    pub id: String,
    pub origin: TrackOrigin,
    pub person_id: Option<String>,
    pub name: Option<String>,
    pub status: MatchStatus,
    pub score: Option<f32>,
    pub candidates: Vec<Candidate>,
    pub embedding: Option<Vec<f32>>,
    pub clip: Option<ClipRef>,
}

/// How a speaker came to (not) name a person (AD-346).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MatchStatus {
    /// Matched a bank person at or above `AUTO_MATCH`.
    Auto,
    /// A candidate at or above `SUGGEST` exists; nobody is assigned.
    Suggested,
    /// A person confirmed it.
    Confirmed,
    /// Nothing in the bank is close enough, or there is no bank.
    Unknown,
    /// The microphone: the bank's `self` person.
    #[serde(rename = "self")]
    Me,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Candidate {
    pub person_id: String,
    pub name: String,
    pub score: f32,
}

/// A span of one file's one track that holds only this speaker — what the
/// bank stores when the speaker is confirmed (AD-347).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipRef {
    /// Relative to the transcript's directory.
    pub file: String,
    /// The audio-track index; `null` for all tracks mixed.
    pub track: Option<u32>,
    /// Seconds within `file`.
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Utterance {
    /// `u1`, `u2`… in time order.
    pub id: String,
    pub speaker: String,
    /// The track the line was heard on. Absent in a file written before it
    /// was recorded, and then filled on load from its speaker's origin
    /// ([`Transcript::from_json`]).
    #[serde(default)]
    pub origin: TrackOrigin,
    pub start: f64,
    pub end: f64,
    /// What the transcript says: the recognizer's words after the
    /// dictionary, or a person's edit.
    pub text: String,
    /// What the recognizer said, before the dictionary and any edit.
    pub asr_text: String,
    pub edited: bool,
    pub words: Vec<Word>,
}

/// One dictionary replacement and how often it fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppliedTerm {
    pub from: String,
    pub to: String,
    pub count: u32,
}

/// A transcript file that cannot be read as one.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptFormatError {
    #[error("The transcript is not valid: {0}")]
    Invalid(#[from] serde_json::Error),
    #[error("The transcript was written by a newer keeper (version {0}).")]
    NewerVersion(u32),
}

impl Transcript {
    /// Parse a transcript file, refusing a version this build does not know.
    /// A line written without its origin takes its speaker's, and a speaker
    /// an earlier merge left behind is dropped ([`Self::drop_absorbed`]).
    pub fn from_json(raw: &str) -> Result<Self, TranscriptFormatError> {
        let mut value: serde_json::Value = serde_json::from_str(raw)?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |version| u32::try_from(version).unwrap_or(u32::MAX));
        if version > TRANSCRIPT_VERSION {
            return Err(TranscriptFormatError::NewerVersion(version));
        }
        fill_utterance_origins(&mut value);
        let mut transcript: Self = serde_json::from_value(value)?;
        transcript.drop_absorbed();
        Ok(transcript)
    }

    /// Drop every speaker without a line whose person another speaker heard
    /// on the same track carries, with lines — what a same-person merge left
    /// behind before merges removed it. A lineless speaker nobody else
    /// stands for (unnamed, or the only one naming its person) stays.
    fn drop_absorbed(&mut self) {
        let speaks = |id: &str| self.utterances.iter().any(|u| u.speaker == id);
        let absorbed: Vec<String> = self
            .speakers
            .iter()
            .filter(|speaker| {
                speaker.person_id.is_some()
                    && !speaks(&speaker.id)
                    && self.speakers.iter().any(|other| {
                        other.id != speaker.id
                            && other.origin == speaker.origin
                            && other.person_id == speaker.person_id
                            && speaks(&other.id)
                    })
            })
            .map(|speaker| speaker.id.clone())
            .collect();
        self.speakers
            .retain(|speaker| !absorbed.contains(&speaker.id));
    }

    /// Whether a person has changed this transcript: the flag every
    /// correction sets, or — for a file older than the flag — an edited line
    /// or a confirmed speaker.
    pub fn is_corrected(&self) -> bool {
        self.corrected
            || self.utterances.iter().any(|utterance| utterance.edited)
            || self
                .speakers
                .iter()
                .any(|speaker| speaker.status == MatchStatus::Confirmed)
    }

    /// The file bytes: pretty JSON with a trailing newline, so a synced
    /// transcript diffs line by line.
    pub fn to_json(&self) -> Result<String, TranscriptFormatError> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }

    pub fn speaker(&self, id: &str) -> Option<&Speaker> {
        self.speakers.iter().find(|speaker| speaker.id == id)
    }

    /// The name a person reads for a speaker: its name, "You" for the
    /// microphone, else "Speaker N".
    pub fn display_name(speaker: &Speaker) -> String {
        if let Some(name) = speaker.name.as_deref().filter(|name| !name.is_empty()) {
            return name.to_owned();
        }
        if speaker.id == SELF_SPEAKER_ID {
            return "You".to_owned();
        }
        match speaker.id.strip_prefix('S') {
            Some(number) if !number.is_empty() => format!("Speaker {number}"),
            _ => speaker.id.clone(),
        }
    }
}

/// Give every utterance object without an `origin` its speaker's — the one
/// thing a line written before the field existed can be said to have been
/// heard on. A speaker the file does not list leaves the default.
fn fill_utterance_origins(value: &mut serde_json::Value) {
    let origins: Vec<(String, serde_json::Value)> = value
        .get("speakers")
        .and_then(serde_json::Value::as_array)
        .map(|speakers| {
            speakers
                .iter()
                .filter_map(|speaker| {
                    Some((
                        speaker.get("id")?.as_str()?.to_owned(),
                        speaker.get("origin")?.clone(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(utterances) = value
        .get_mut("utterances")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for utterance in utterances {
        let Some(object) = utterance.as_object_mut() else {
            continue;
        };
        if object.contains_key("origin") {
            continue;
        }
        let speaker = object.get("speaker").and_then(serde_json::Value::as_str);
        if let Some((_, origin)) = origins.iter().find(|(id, _)| Some(id.as_str()) == speaker) {
            object.insert("origin".to_owned(), origin.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcript_from_a_newer_keeper_is_refused_not_misread() {
        let raw = r#"{"version":2,"source":{"kind":"file","files":[]},"createdAt":"","engine":{"asr":"","diarizer":"","embedding":""},"language":"auto","duration":0,"speakers":[],"utterances":[],"dictionaryApplied":[]}"#;
        assert!(matches!(
            Transcript::from_json(raw),
            Err(TranscriptFormatError::NewerVersion(2))
        ));
        let current = raw.replacen("\"version\":2", "\"version\":1", 1);
        let transcript = Transcript::from_json(&current).expect("v1 without parts reads");
        assert!(transcript.source.parts.is_empty());
    }

    #[test]
    fn a_line_saved_without_its_origin_takes_its_speakers_and_old_edits_count_as_corrected() {
        let raw = r#"{"version":1,"source":{"kind":"recording","files":[]},"createdAt":"","engine":{"asr":"","diarizer":"","embedding":""},"language":"auto","duration":2,
            "speakers":[{"id":"ME","origin":"microphone","personId":null,"name":null,"status":"self","score":null,"candidates":[],"embedding":null,"clip":null},
                        {"id":"S1","origin":"system","personId":null,"name":null,"status":"unknown","score":null,"candidates":[],"embedding":null,"clip":null}],
            "utterances":[{"id":"u1","speaker":"ME","start":0,"end":1,"text":"hi","asrText":"hi","edited":false,"words":[]},
                          {"id":"u2","speaker":"S1","start":1,"end":2,"text":"yo","asrText":"yo","edited":false,"words":[]},
                          {"id":"u3","speaker":"S1","origin":"mixed","start":2,"end":2,"text":"x","asrText":"x","edited":false,"words":[]}],
            "dictionaryApplied":[]}"#;
        let t = Transcript::from_json(raw).expect("reads");
        let origins: Vec<TrackOrigin> = t.utterances.iter().map(|u| u.origin).collect();
        assert_eq!(
            origins,
            [
                TrackOrigin::Microphone,
                TrackOrigin::System,
                TrackOrigin::Mixed
            ],
            "a written origin is kept, an absent one is the speaker's"
        );
        assert!(!t.corrected);
        assert!(!t.is_corrected());
        let edited = Transcript::from_json(&raw.replacen("\"edited\":false", "\"edited\":true", 1))
            .expect("reads");
        assert!(edited.is_corrected(), "an edit from before the flag counts");
        let confirmed =
            Transcript::from_json(&raw.replacen("\"unknown\"", "\"confirmed\"", 1)).expect("reads");
        assert!(confirmed.is_corrected(), "a confirmation counts");
    }

    #[test]
    fn a_lineless_speaker_whose_person_another_voice_on_its_track_carries_is_dropped_on_read() {
        let speaker = |id: &str, origin: &str, person: &str| {
            let person = if person.is_empty() {
                "null".to_owned()
            } else {
                format!("\"{person}\"")
            };
            format!(
                r#"{{"id":"{id}","origin":"{origin}","personId":{person},"name":null,"status":"confirmed","score":null,"candidates":[],"embedding":null,"clip":null}}"#
            )
        };
        let speakers = [
            speaker("S1", "system", "KELLY"),
            // What the kelly-sync file held: Kelly again, with no line.
            speaker("S2", "system", "KELLY"),
            // Kelly on the microphone, lineless: another track, kept.
            speaker("S3", "microphone", "KELLY"),
            // Unnamed and lineless, and a person nobody else carries: kept.
            speaker("S4", "system", ""),
            speaker("S5", "system", "BO"),
        ]
        .join(",");
        let raw = format!(
            r#"{{"version":1,"source":{{"kind":"recording","files":[]}},"createdAt":"","engine":{{"asr":"","diarizer":"","embedding":""}},"language":"auto","duration":2,
            "speakers":[{speakers}],
            "utterances":[{{"id":"u1","speaker":"S1","start":0,"end":1,"text":"hi","asrText":"hi","edited":false,"words":[]}}],
            "dictionaryApplied":[]}}"#
        );
        let t = Transcript::from_json(&raw).expect("reads");
        let ids: Vec<&str> = t.speakers.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["S1", "S3", "S4", "S5"]);
    }
}
