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
//! Every time the person speaks over an answer leaves one [`PauseRecord`]
//! (97.3 #10): when the voice activity model heard the onset, when the
//! answer paused, when the speech ended, when the word list decided, when
//! the answer resumed, and what the utterance was. Every answer read leaves
//! one [`Spoken`]: how long it was read aloud, pauses excluded — what a
//! count of false onsets is per. [`measure_pauses`] turns a run's records
//! into NFR-114's backchannel figures.
//!
//! Pure: every time comes in as an argument, milliseconds since the Unix
//! epoch as the shell's `voice_log::now_ms` gives them.

use serde::{Deserialize, Serialize};

use super::backchannel::Utterance;
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
    /// The pause in progress, and the answer being read.
    reading: Reading,
}

/// The points of a pause and of the answer it paused.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Reading {
    /// The voice activity model's onset, when it came before the pause.
    onset_ms: Option<i64>,
    /// The first onset of the speech being decided: where its length runs
    /// from.
    speech_from_ms: Option<i64>,
    pause_ms: Option<i64>,
    speech_end_ms: Option<i64>,
    /// When the answer last began to be read aloud, while it is.
    speaking_since_ms: Option<i64>,
    /// How long it has been read aloud, pauses excluded.
    spoken_ms: i64,
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
            let reading = std::mem::take(&mut self.reading);
            let clock = std::mem::replace(
                self,
                Self {
                    reading,
                    ..Self::default()
                },
            );
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
            let reading = std::mem::take(&mut self.reading);
            *self = Self {
                reading,
                ..Self::default()
            };
        }
        None
    }

    /// The voice activity model heard speech start over the answer at
    /// `at_ms`. The first onset of a pause is where its speech is measured
    /// from, and — when it came before the answer paused — its `onset_ms`.
    pub fn paused_onset(&mut self, at_ms: i64) {
        let reading = &mut self.reading;
        if reading.speech_from_ms.is_none() {
            reading.speech_from_ms = Some(at_ms);
        }
        if reading.onset_ms.is_none() && reading.pause_ms.is_none() {
            reading.onset_ms = Some(at_ms);
        }
    }

    /// The voice activity model heard the speech over the answer end at
    /// `at_ms`: how long it lasted, when its start was heard.
    pub fn paused_speech_end(&mut self, at_ms: i64) -> Option<u32> {
        let reading = &mut self.reading;
        reading.speech_end_ms = Some(at_ms);
        let from = reading.speech_from_ms?;
        u32::try_from(at_ms.saturating_sub(from).max(0)).ok()
    }

    /// The answer's half of a move: the turn went from `before` on `event` —
    /// as the table was handed it ([`super::Turn::table_event`]) — to
    /// `after`, handing out `effects`. The event reached the turn at
    /// `heard_ms`, and the port had carried the effects out by `now_ms`.
    /// Returns the pause's record when this move decided one, and the
    /// answer's [`Spoken`] when this move ended its reading.
    pub fn observe_reading(
        &mut self,
        before: &TurnState,
        event: &TurnEvent,
        after: &TurnState,
        effects: &[Effect],
        heard_ms: i64,
        now_ms: i64,
    ) -> (Option<PauseRecord>, Option<Spoken>) {
        let reading = &mut self.reading;
        let speaking = |state: &TurnState| matches!(state, TurnState::Speaking);
        let paused = |state: &TurnState| matches!(state, TurnState::Paused { .. });
        if !speaking(before) && speaking(after) {
            reading.speaking_since_ms = Some(now_ms);
        }
        if speaking(before) && !speaking(after) {
            if let Some(since) = reading.speaking_since_ms.take() {
                reading.spoken_ms += now_ms.saturating_sub(since).max(0);
            }
        }
        if !paused(before) && paused(after) {
            reading.pause_ms = Some(now_ms);
        }
        let mut record = None;
        if paused(before) && !paused(after) {
            let decision = match event {
                TurnEvent::Resolved(utterance) => Some(PauseDecision::of(utterance)),
                TurnEvent::StopHeard | TurnEvent::Abandoned => Some(PauseDecision::Stop),
                _ => None,
            };
            let words = matches!(event, TurnEvent::Resolved(Utterance::BackchannelByWords))
                || matches!(
                    event,
                    TurnEvent::Resolved(Utterance::Question(text)) if !text.trim().is_empty()
                );
            let kept = Reading {
                speaking_since_ms: reading.speaking_since_ms,
                spoken_ms: reading.spoken_ms,
                ..Reading::default()
            };
            let pause = std::mem::replace(reading, kept);
            record = decision
                .zip(pause.pause_ms)
                .map(|(decision, pause_ms)| PauseRecord {
                    decision,
                    onset_ms: pause.onset_ms,
                    pause_ms,
                    speech_end_ms: pause.speech_end_ms,
                    final_words_ms: words.then_some(heard_ms),
                    resume_ms: effects
                        .contains(&Effect::ContinueSpeaking)
                        .then_some(now_ms),
                });
        } else if !paused(after) && reading.pause_ms.is_none() {
            // Points heard over an answer that never paused for them — the
            // turn moved on first — are no pause's.
            reading.onset_ms = None;
            reading.speech_from_ms = None;
            reading.speech_end_ms = None;
        }
        let mut spoken = None;
        let reads = |state: &TurnState| speaking(state) || paused(state);
        if reads(before) && !reads(after) {
            spoken = Some(Spoken {
                spoken_ms: std::mem::take(&mut self.reading).spoken_ms,
            });
        }
        (record, spoken)
    }
}

/// What the utterance that paused an answer was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseDecision {
    /// No words, short speech: resumed by its duration.
    BackchannelDuration,
    /// Words the list holds: resumed.
    BackchannelWords,
    /// The stop phrase, or a stop by hand: the answer stopped, nothing
    /// asked.
    Stop,
    /// Anything else: the answer stopped and the words were asked.
    Question,
    /// No words, too little speech to be anyone: resumed (Q8).
    FalseOnset,
}

impl PauseDecision {
    fn of(utterance: &Utterance) -> Self {
        match utterance {
            Utterance::FalseOnset => Self::FalseOnset,
            Utterance::BackchannelByDuration => Self::BackchannelDuration,
            Utterance::BackchannelByWords => Self::BackchannelWords,
            Utterance::Question(_) => Self::Question,
        }
    }
}

/// One pause, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseRecord {
    pub decision: PauseDecision,
    /// The first of the voice activity model's three onset frames, as the
    /// tap delivered it; absent without the models, or when the
    /// recogniser's words paused the answer first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_ms: Option<i64>,
    /// `PauseSpeaking` was carried out.
    pub pause_ms: i64,
    /// The voice activity model's speech-end frame; absent without the
    /// models, or before it heard one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_end_ms: Option<i64>,
    /// The words that decided — a backchannel by the list, or a question —
    /// reached the turn, before any of the decision was carried out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_words_ms: Option<i64>,
    /// `ContinueSpeaking` was carried out; absent on a stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_ms: Option<i64>,
}

/// What marks a pause record in the app log.
pub const PAUSE_LOG_MARK: &str = "voice pause ";

impl PauseRecord {
    /// The record as one line of JSON — the voice ring's detail.
    pub fn detail(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A record read back from [`PauseRecord::detail`]'s line.
    pub fn parse(detail: &str) -> Option<Self> {
        serde_json::from_str(detail).ok()
    }

    /// The app log's line for this record: [`PAUSE_LOG_MARK`] and the detail.
    pub fn log_line(&self) -> String {
        format!("{PAUSE_LOG_MARK}{}", self.detail())
    }
}

/// One answer's reading: how long it was read aloud, pauses excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spoken {
    pub spoken_ms: i64,
}

/// What marks a [`Spoken`] record in the app log.
pub const SPOKEN_LOG_MARK: &str = "voice spoken ";

impl Spoken {
    /// The app log's line for this record.
    pub fn log_line(&self) -> String {
        format!(
            "{SPOKEN_LOG_MARK}{}",
            serde_json::to_string(self).unwrap_or_default()
        )
    }
}

/// Every pause record and the total reading time in an app log's text.
pub fn pauses_from_log(text: &str) -> (Vec<PauseRecord>, i64) {
    let mut pauses = Vec::new();
    let mut spoken_ms = 0;
    for line in text.lines() {
        if let Some((_, record)) = line.split_once(PAUSE_LOG_MARK) {
            pauses.extend(PauseRecord::parse(record.trim()));
        } else if let Some((_, record)) = line.split_once(SPOKEN_LOG_MARK) {
            if let Ok(spoken) = serde_json::from_str::<Spoken>(record.trim()) {
                spoken_ms += spoken.spoken_ms;
            }
        }
    }
    (pauses, spoken_ms)
}

/// NFR-114's backchannel figures over a device run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseFigures {
    /// Every pause measured.
    pub pauses: usize,
    /// p95 of `pause_ms − onset_ms` over pauses with an onset: speech over
    /// the answer to the answer pausing. NFR-114: ≤ 150.
    pub pause_p95_ms: Option<i64>,
    /// p95 of `resume_ms − speech_end_ms` over pauses resumed by their
    /// duration. NFR-114: ≤ 300.
    pub duration_resume_p95_ms: Option<i64>,
    /// p95 of `resume_ms − final_words_ms` over pauses resumed by their
    /// words. NFR-114: ≤ 300.
    pub words_resume_p95_ms: Option<i64>,
    /// The false onsets counted.
    pub false_onsets: usize,
    /// How long answers were read aloud over the run, pauses excluded.
    pub spoken_ms: i64,
}

impl PauseFigures {
    /// False onsets per ten minutes of reading aloud; `None` when nothing
    /// was read.
    pub fn false_onsets_per_ten_minutes(&self) -> Option<f64> {
        (self.spoken_ms > 0).then(|| self.false_onsets as f64 * 600_000.0 / self.spoken_ms as f64)
    }
}

/// Why [`measure_pauses`] gave no figures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{pauses} pauses measured; NFR-114 is measured over at least {MEASURED_TURNS}")]
pub struct TooFewPauses {
    pub pauses: usize,
}

/// NFR-114's backchannel figures over `records` and `spoken_ms` of reading
/// aloud, or a refusal under [`MEASURED_TURNS`] pauses.
pub fn measure_pauses(
    records: &[PauseRecord],
    spoken_ms: i64,
) -> Result<PauseFigures, TooFewPauses> {
    if records.len() < MEASURED_TURNS {
        return Err(TooFewPauses {
            pauses: records.len(),
        });
    }
    let of = |decision: PauseDecision| {
        records
            .iter()
            .filter(move |record| record.decision == decision)
    };
    Ok(PauseFigures {
        pauses: records.len(),
        pause_p95_ms: p95(records
            .iter()
            .filter_map(|r| Some(r.pause_ms - r.onset_ms?))
            .collect()),
        duration_resume_p95_ms: p95(of(PauseDecision::BackchannelDuration)
            .filter_map(|r| Some(r.resume_ms? - r.speech_end_ms?))
            .collect()),
        words_resume_p95_ms: p95(of(PauseDecision::BackchannelWords)
            .filter_map(|r| Some(r.resume_ms? - r.final_words_ms?))
            .collect()),
        false_onsets: of(PauseDecision::FalseOnset).count(),
        spoken_ms,
    })
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
