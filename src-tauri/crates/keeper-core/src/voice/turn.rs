//! One voice turn as a state machine (FR-401, AD-165, NFR-51).
//!
//! `Idle → Listening → Heard → Sending → Speaking → Idle`, decided here as a
//! pure function over a state and an event, so the whole of it is tested on
//! the dev host and the shell only carries out the [`Effect`]s it is handed.
//!
//! # The rules the table pins
//!
//! - **Abandon releases the device from every state** (NFR-51). There is no
//!   state in which `Abandoned` leaves the microphone open — including `Idle`,
//!   where a wake phrase may be holding it.
//! - **A turn cannot record forever.** `Listening` has a silence budget
//!   ([`silence_budget`]); when the shell's timer fires it feeds [`Silence`],
//!   and the turn either sends what it heard or ends.
//! - **Barge-in stops speech first.** `SpeechDetected` while `Speaking`
//!   yields [`Effect::StopSpeaking`] before any other effect, because the
//!   person started talking and nothing should still be talking over them.
//! - **Nothing empty is sent.** A final transcript that is blank ends the turn
//!   without a message; a partial transcript never sends at all.
//! - **A failure releases the device too.** `Failed` is a state, but it is one
//!   the microphone has already been released from.
//! - **The port never records its own answer** (AD-175). Whether the device
//!   may be open while an utterance is read aloud is [`may_record`]'s answer,
//!   and it depends on the platform: iOS keeps keeper's voice out of the
//!   transcript, so the microphone stays open for barge-in; macOS has nothing
//!   that does, so it is released before the `Speak`. The table below is
//!   platform-free; [`super::Turn`] applies the rule over it.
//!
//! [`Silence`]: TurnEvent::Silence

use std::time::Duration;

use super::VoicePlatform;

/// How long `Listening` waits for the first word before giving up.
///
/// Long enough to think, short enough that a phrase heard by mistake does not
/// leave the microphone open across a conversation.
pub const NOTHING_HEARD_TIMEOUT: Duration = Duration::from_secs(8);

/// How long `Listening` waits after the last partial transcript before
/// treating the pause as the end of the utterance.
///
/// The on-device recogniser does not end an utterance by itself in a
/// continuous request; this pause is what makes a spoken sentence become a
/// sent message without a button.
pub const END_OF_UTTERANCE_PAUSE: Duration = Duration::from_millis(1800);

/// Where a turn is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TurnState {
    /// No turn. The microphone is closed unless a wake phrase is holding it.
    #[default]
    Idle,
    /// The microphone is open and the recogniser is transcribing.
    Listening {
        /// The latest partial transcript, shown to the person as it forms and
        /// promoted to the sent text when the pause ends the utterance.
        heard: String,
    },
    /// What was said is text, handed to the conversation, not yet sent.
    Heard {
        /// The final transcript.
        text: String,
    },
    /// The message went to the model and the answer is streaming back.
    Sending {
        /// Whether the first piece of the answer has arrived — the difference
        /// between a model thinking and one that has started (AD-186).
        answering: bool,
    },
    /// The answer is being read aloud. Whether the microphone is open for
    /// barge-in meanwhile is [`may_record`]'s answer for the platform.
    Speaking,
    /// The turn ended on an error. The device is already released.
    Failed {
        /// Why, in a sentence the surface can show.
        reason: String,
    },
}

/// What happened, as the port, the surface and the conversation report it.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    /// The trigger: the wake phrase was heard, or the person pressed the mic
    /// control that stands in for it (Story 62.6).
    WakeMatched,
    /// The recogniser has an interim transcript.
    PartialHeard(String),
    /// The recogniser has a final transcript for this utterance.
    FinalHeard(String),
    /// The conversation accepted the text and the request went out.
    Sent,
    /// A piece of the answer arrived.
    AnswerChunk,
    /// The input level in `0.0..=1.0`, smoothed and rate-limited by the
    /// port's [`super::level::Meter`]. Not a transition: [`super::Turn`]
    /// records it for the snapshot and the table ignores it everywhere.
    Level(f32),
    /// The whole answer arrived — the text to read aloud.
    AnswerDone(String),
    /// The person started speaking (barge-in while `Speaking`).
    SpeechDetected,
    /// The person stopped the turn.
    Abandoned,
    /// Nothing was heard for the state's silence budget, or the spoken answer
    /// came to its end.
    Silence,
    /// The port or the conversation failed.
    Failed(String),
}

/// What the shell must do next, in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start (or restart) capture and recognition.
    OpenMicrophone,
    /// Stop capture and recognition and let the audio session go.
    ReleaseMicrophone,
    /// Hand this text to the conversation as the person's message.
    SendText(String),
    /// Read this text aloud.
    Speak(String),
    /// Stop reading aloud, now, mid-word.
    StopSpeaking,
}

/// The transition table.
///
/// Every `(state, event)` pair has an answer; pairs not named in a state's arm
/// are ignored there — a late `PartialHeard` after the utterance ended, a
/// `Sent` while nothing was heard — and leave the state and the device as
/// they were.
pub fn advance(state: TurnState, event: TurnEvent) -> (TurnState, Vec<Effect>) {
    use Effect::{OpenMicrophone, ReleaseMicrophone, StopSpeaking};
    use TurnEvent::{
        Abandoned, AnswerChunk, AnswerDone, Failed, FinalHeard, PartialHeard, Sent, Silence,
        SpeechDetected, WakeMatched,
    };

    match (state, event) {
        // -- Abandon and failure: the same answer from every state, first so
        //    no later arm can forget them (NFR-51). ---------------------------
        (TurnState::Speaking, Abandoned) => {
            (TurnState::Idle, vec![StopSpeaking, ReleaseMicrophone])
        }
        (_, Abandoned) => (TurnState::Idle, vec![ReleaseMicrophone]),
        (TurnState::Speaking, Failed(reason)) => (
            TurnState::Failed { reason },
            vec![StopSpeaking, ReleaseMicrophone],
        ),
        (_, Failed(reason)) => (TurnState::Failed { reason }, vec![ReleaseMicrophone]),

        // -- Idle and Failed: a trigger starts a turn; an answer can be read
        //    aloud without one (the person typed, and wants to hear it). ------
        (TurnState::Idle | TurnState::Failed { .. }, WakeMatched) => (
            TurnState::Listening {
                heard: String::new(),
            },
            vec![OpenMicrophone],
        ),
        (TurnState::Idle | TurnState::Failed { .. }, AnswerDone(text)) => {
            speak_or_end(text, vec![OpenMicrophone])
        }
        (state @ (TurnState::Idle | TurnState::Failed { .. }), _) => (state, Vec::new()),

        // -- Listening --------------------------------------------------------
        (TurnState::Listening { .. }, PartialHeard(heard)) => {
            (TurnState::Listening { heard }, Vec::new())
        }
        (TurnState::Listening { .. }, FinalHeard(text)) => send_or_end(text),
        (TurnState::Listening { heard }, Silence) => send_or_end(heard),
        (state @ TurnState::Listening { .. }, _) => (state, Vec::new()),

        // -- Heard ------------------------------------------------------------
        (TurnState::Heard { .. }, Sent) => (TurnState::Sending { answering: false }, Vec::new()),
        // The answer may arrive without a `Sent` in between: `Sending` is a
        // progress marker for the surface, not a gate on the answer.
        (TurnState::Heard { .. }, AnswerDone(text)) => speak_or_end(text, Vec::new()),
        (state @ TurnState::Heard { .. }, _) => (state, Vec::new()),

        // -- Sending ----------------------------------------------------------
        (TurnState::Sending { .. }, AnswerChunk) => {
            (TurnState::Sending { answering: true }, Vec::new())
        }
        (TurnState::Sending { .. }, AnswerDone(text)) => speak_or_end(text, Vec::new()),
        (state @ TurnState::Sending { .. }, _) => (state, Vec::new()),

        // -- Speaking ---------------------------------------------------------
        // Barge-in: stop talking before anything else, then listen again. On
        // a full-duplex platform the microphone is already open and
        // `OpenMicrophone` restarts recognition on a fresh request so the
        // answer's tail is not transcribed as speech; on a half-duplex one it
        // was released before the speech and this opens it.
        (TurnState::Speaking, SpeechDetected) => (
            TurnState::Listening {
                heard: String::new(),
            },
            vec![StopSpeaking, OpenMicrophone],
        ),
        (TurnState::Speaking, Silence) => (TurnState::Idle, vec![ReleaseMicrophone]),
        (TurnState::Speaking, _) => (TurnState::Speaking, Vec::new()),
    }
}

/// A final transcript either becomes the message or ends the turn — never an
/// empty message.
fn send_or_end(text: String) -> (TurnState, Vec<Effect>) {
    let text = text.trim();
    if text.is_empty() {
        (TurnState::Idle, vec![Effect::ReleaseMicrophone])
    } else {
        let text = text.to_owned();
        (
            TurnState::Heard { text: text.clone() },
            vec![Effect::SendText(text)],
        )
    }
}

/// An answer either gets read aloud or, when there is nothing to say, ends the
/// turn. `before` is what must happen first when the microphone is not yet
/// open.
fn speak_or_end(text: String, mut before: Vec<Effect>) -> (TurnState, Vec<Effect>) {
    let text = text.trim();
    if text.is_empty() {
        return (TurnState::Idle, vec![Effect::ReleaseMicrophone]);
    }
    before.push(Effect::Speak(text.to_owned()));
    (TurnState::Speaking, before)
}

/// How long the shell may let `state` sit without a new event before it feeds
/// [`TurnEvent::Silence`] — `None` where silence means nothing.
///
/// `Listening` is the only state with a budget: [`NOTHING_HEARD_TIMEOUT`]
/// until the first word, [`END_OF_UTTERANCE_PAUSE`] after it. `Speaking`'s
/// `Silence` comes from the synthesiser finishing, not from a clock, and
/// `Sending` is bounded by the conversation's own read timeout.
pub fn silence_budget(state: &TurnState) -> Option<Duration> {
    match state {
        TurnState::Listening { heard } if heard.trim().is_empty() => Some(NOTHING_HEARD_TIMEOUT),
        TurnState::Listening { .. } => Some(END_OF_UTTERANCE_PAUSE),
        _ => None,
    }
}

/// AD-175, the half-duplex rule: whether the port may have the microphone
/// open while the turn is in `state` on `platform`.
///
/// `false` in `Failed` everywhere: the turn ended on an error and the
/// device is already released. `false` in `Speaking` on a platform that is
/// not [`VoicePlatform::full_duplex`]: nothing there keeps the answer out
/// of the microphone, so a port that recorded while it spoke would hear
/// itself, and the first transcript it produced would stop its own speech.
/// `true` everywhere else, including `Idle` (a wake phrase may hold the
/// device) and `Speaking` where the OS arbitrates, which is what the iOS
/// port has always done.
pub fn may_record(platform: &VoicePlatform, state: &TurnState) -> bool {
    match state {
        TurnState::Failed { .. } => false,
        TurnState::Speaking => platform.full_duplex,
        TurnState::Idle
        | TurnState::Listening { .. }
        | TurnState::Heard { .. }
        | TurnState::Sending { .. } => true,
    }
}
