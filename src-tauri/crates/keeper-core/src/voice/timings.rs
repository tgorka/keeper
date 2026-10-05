//! How a spoken turn ended, in clock points (AD-411, NFR-114).
//!
//! Every spoken turn that sends a question leaves one [`TurnEnd`]: when the
//! voice activity model heard speech stop, when the end-of-turn model's
//! verdict reached the turn, when the recogniser was told the audio ended,
//! when its final words came, when the question was handed on, and which
//! rule ended it. [`TurnClock`] collects the points from the turn's own
//! transitions, with the shell's clock; the record goes into the voice ring
//! as the detail of a `turn_end` row and, as the same line, into the app log
//! ([`TurnEnd::log_line`]), which is the file a device run is measured from:
//! [`from_log`] reads the records back and [`measure`] turns twenty or more
//! of them into NFR-114's figures.
//!
//! Pure: every time comes in as an argument, milliseconds since the Unix
//! epoch as the shell's `voice_log::now_ms` gives them.

use serde::{Deserialize, Serialize};

use super::turn::END_OF_UTTERANCE_PAUSE;
use super::{Effect, TurnEvent, TurnState};

/// What sent the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndedBy {
    /// The end-of-turn model heard a finished sentence.
    Model,
    /// The pause after the last partial ran out.
    Pause,
    /// The recogniser ended the utterance itself with a final transcript.
    Recogniser,
}

/// One spoken turn's end, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnEnd {
    pub ended_by: EndedBy,
    /// The voice activity model's speech-end frame, as the tap delivered it;
    /// absent without the models, or when speech resumed after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_end_ms: Option<i64>,
    /// The end-of-turn model's verdict reached the turn and it decided to
    /// finish (model turns only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utterance_end_ms: Option<i64>,
    /// The port ended the request's audio (`endAudio` ran), as it reported
    /// it; absent when it never did — no request to end, or no answer
    /// before the wait ran out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_recognition_ms: Option<i64>,
    /// The recogniser's final words came; absent when the wait for them ran
    /// out and the last partial was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_words_ms: Option<i64>,
    /// The question was handed to the conversation.
    pub sent_ms: i64,
}

/// What marks a record in the app log: the line is this, then the record.
pub const LOG_MARK: &str = "voice turn_end ";

impl TurnEnd {
    /// The record as one line of JSON — the voice ring's detail.
    pub fn detail(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A record read back from [`TurnEnd::detail`]'s line.
    pub fn parse(detail: &str) -> Option<Self> {
        serde_json::from_str(detail).ok()
    }

    /// The app log's line for this record: [`LOG_MARK`] and the detail.
    pub fn log_line(&self) -> String {
        format!("{LOG_MARK}{}", self.detail())
    }
}

/// Every record in an app log's text, oldest first: each line holding
/// [`LOG_MARK`] followed by a record. Other lines are skipped.
pub fn from_log(text: &str) -> Vec<TurnEnd> {
    text.lines()
        .filter_map(|line| line.split_once(LOG_MARK))
        .filter_map(|(_, record)| TurnEnd::parse(record.trim()))
        .collect()
}

/// The points of the turn in progress, collected as it moves.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TurnClock {
    speech_end_ms: Option<i64>,
    utterance_end_ms: Option<i64>,
    finish_recognition_ms: Option<i64>,
    final_words_ms: Option<i64>,
}

impl TurnClock {
    /// The voice activity model heard speech stop at `at_ms`.
    pub fn speech_end(&mut self, at_ms: i64) {
        self.speech_end_ms = Some(at_ms);
    }

    /// The voice activity model heard speech start again: the last speech
    /// end was a pause inside the utterance, not its end.
    pub fn onset(&mut self) {
        self.speech_end_ms = None;
    }

    /// The turn moved from `before` on `event` to `after`, handing out
    /// `effects`, at `now_ms`. Returns the turn's record when this move sent
    /// its question.
    ///
    /// The decision and its execution are two points: `utterance_end_ms` is
    /// when the turn decided to finish, `finish_recognition_ms` when the
    /// port says `endAudio` ran — the moment the turn's acknowledgement
    /// carries, not the moment it reached the turn.
    pub fn observe(
        &mut self,
        before: &TurnState,
        event: &TurnEvent,
        after: &TurnState,
        effects: &[Effect],
        now_ms: i64,
    ) -> Option<TurnEnd> {
        if effects
            .iter()
            .any(|effect| matches!(effect, Effect::FinishRecognition(_)))
        {
            self.utterance_end_ms = Some(now_ms);
        }
        if let (
            TurnState::Finishing {
                audio_ended_ms: None,
                ..
            },
            TurnState::Finishing {
                audio_ended_ms: Some(ended),
                ..
            },
        ) = (before, after)
        {
            self.finish_recognition_ms = Some(*ended);
        }
        if matches!(before, TurnState::Finishing { .. })
            && matches!(event, TurnEvent::FinalHeard(_))
        {
            self.final_words_ms = Some(now_ms);
        }
        let sent = effects
            .iter()
            .any(|effect| matches!(effect, Effect::SendText(_)));
        if sent {
            let ended_by = if self.utterance_end_ms.is_some() {
                EndedBy::Model
            } else if matches!(event, TurnEvent::Silence) {
                EndedBy::Pause
            } else {
                EndedBy::Recogniser
            };
            let clock = std::mem::take(self);
            return Some(TurnEnd {
                ended_by,
                speech_end_ms: clock.speech_end_ms,
                utterance_end_ms: clock.utterance_end_ms,
                finish_recognition_ms: clock.finish_recognition_ms,
                final_words_ms: clock.final_words_ms,
                sent_ms: now_ms,
            });
        }
        if !matches!(
            after,
            TurnState::Listening { .. } | TurnState::Finishing { .. }
        ) {
            *self = Self::default();
        }
        None
    }
}

/// The fewest turns [`measure`] gives figures for (F10).
pub const MEASURED_TURNS: usize = 20;

/// NFR-114's end-of-turn figures over a device run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFigures {
    /// Every record measured.
    pub turns: usize,
    /// Those the end-of-turn model ended.
    pub model_turns: usize,
    /// p95 of `finish_recognition_ms − speech_end_ms` over model turns that
    /// have both: keeper stopping waiting for more speech. NFR-114: ≤ 300.
    pub finish_p95_ms: Option<i64>,
    /// p95 of `sent_ms − finish_recognition_ms` over model turns: the wait
    /// for the last words. NFR-114: ≤ 600.
    pub send_p95_ms: Option<i64>,
    /// The `sent_ms` of every pause turn sent less than
    /// [`END_OF_UTTERANCE_PAUSE`] after its speech end: a trailing-off
    /// question the pause should have waited for. NFR-114: none.
    pub early_pauses: Vec<i64>,
}

/// Why [`measure`] gave no figures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{turns} turns measured; NFR-114 is measured over at least {MEASURED_TURNS}")]
pub struct TooFewTurns {
    pub turns: usize,
}

/// NFR-114's figures over `records`, or a refusal under [`MEASURED_TURNS`].
/// A p95 is the nearest rank: the smallest value at or above 95 % of the
/// sorted values.
pub fn measure(records: &[TurnEnd]) -> Result<TurnFigures, TooFewTurns> {
    if records.len() < MEASURED_TURNS {
        return Err(TooFewTurns {
            turns: records.len(),
        });
    }
    let model: Vec<&TurnEnd> = records
        .iter()
        .filter(|record| record.ended_by == EndedBy::Model)
        .collect();
    let finish = model
        .iter()
        .filter_map(|r| Some(r.finish_recognition_ms? - r.speech_end_ms?))
        .collect();
    let send = model
        .iter()
        .filter_map(|r| Some(r.sent_ms - r.finish_recognition_ms?))
        .collect();
    let pause_ms = i64::try_from(END_OF_UTTERANCE_PAUSE.as_millis()).unwrap_or(i64::MAX);
    let early_pauses = records
        .iter()
        .filter(|r| r.ended_by == EndedBy::Pause)
        .filter(|r| {
            r.speech_end_ms
                .is_some_and(|end| r.sent_ms - end < pause_ms)
        })
        .map(|r| r.sent_ms)
        .collect();
    Ok(TurnFigures {
        turns: records.len(),
        model_turns: model.len(),
        finish_p95_ms: p95(finish),
        send_p95_ms: p95(send),
        early_pauses,
    })
}

/// The nearest-rank 95th percentile, `None` of nothing.
fn p95(mut values: Vec<i64>) -> Option<i64> {
    values.sort_unstable();
    let rank = (values.len() * 95).div_ceil(100);
    values.get(rank.checked_sub(1)?).copied()
}
