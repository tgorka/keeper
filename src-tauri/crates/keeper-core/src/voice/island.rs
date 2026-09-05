//! What the phone's Live Activity shows, and when (Epic 65, Story 65.5,
//! FR-453–FR-455, AD-194).
//!
//! The card in the Dynamic Island and on the lock screen is drawn by Swift
//! — ActivityKit has no other door — but Swift decides nothing: it receives
//! a [`Word`] and a sentence and draws them. Which word a snapshot wants,
//! and whether that is a start, an update, an end or nothing, is [`step`],
//! pure over the previous word and the new snapshot, so it is tested here
//! on the dev host rather than only on a phone (AD-55/AD-56). The shell's
//! `voice_island` carries the decision out.
//!
//! The rules, and why:
//!
//! - Started when something is worth showing and nothing is shown: the
//!   phrase armed, or a turn started by hand. Apple lets an app start a Live
//!   Activity only in the foreground; both of those are taps with keeper in
//!   front (FR-405).
//! - Updated on every change of word, and only then: a level reading, or
//!   a re-pushed snapshot (a pane remount re-pushes the current state), is
//!   [`Step::Keep`], so the system's update budget is spent on transitions.
//!   A turn that ends re-armed is an update to `armed`, never a new request
//!   — the phone may be in the background by then.
//! - Ended when the switch goes off, at once. A failure ends a running card
//!   too, with the failure's sentence left on it for [`FAILURE_LINGER`]:
//!   `Failed` is a state the turn leaves only on the next trigger, and a
//!   lock screen that said "stopped" for the rest of the day would be the
//!   fixture AD-185 refuses. A failure never *starts* a card: a hand-started
//!   turn that failed with nothing armed has nothing to leave behind.

use std::time::Duration;

use crate::vm::VoiceStateVm;

/// How long a failure's sentence stays on the card before the system
/// removes it. Longer than the Mac pill's linger: a lock-screen card floats
/// over nothing, and a sentence about why the ear stopped is worth a glance
/// at the phone.
pub const FAILURE_LINGER: Duration = Duration::from_secs(30);

/// When to end the activity and request a fresh one. Apple ends every Live
/// Activity at eight hours; a quarter of an hour of margin covers a timer
/// that fires late on a phone that was asleep.
pub const RENEW_AFTER: Duration = Duration::from_secs(7 * 3600 + 45 * 60);

/// The state word the card shows. The stable string form ([`Word::as_str`])
/// is what the extension switches on (`KeeperIslandLiveActivity.swift`), so
/// it never changes without both sides changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// The microphone is open, waiting for the phrase.
    Armed,
    /// A turn is running and the recogniser is transcribing.
    Listening,
    /// The transcript is final and on its way.
    Heard,
    /// The message is with the model and nothing has come back yet.
    Thinking,
    /// The answer has begun to arrive.
    Answering,
    /// The answer is being read aloud.
    Speaking,
    /// The turn ended on an error; the card carries the reason.
    Failed,
    /// Not listening: the final word of a card that is being removed.
    Off,
}

impl Word {
    /// The string the bridge and the extension agree on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::Listening => "listening",
            Self::Heard => "heard",
            Self::Thinking => "thinking",
            Self::Answering => "answering",
            Self::Speaking => "speaking",
            Self::Failed => "failed",
            Self::Off => "off",
        }
    }
}

/// What a snapshot asks of the activity, given what it was showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing: the word is unchanged, or there is nothing to show.
    Keep,
    /// Request an activity showing this word.
    Start(Word),
    /// Update the running activity to this word.
    Update(Word),
    /// End the running activity, leaving `last` on the card for `linger` —
    /// at once when `linger` is zero.
    End { last: Word, linger: Duration },
}

/// Which word a snapshot wants, or `None` when the card has no business
/// being there.
#[must_use]
pub fn word(state: &VoiceStateVm) -> Option<Word> {
    match state {
        VoiceStateVm::Idle {
            listening_for_wake: false,
            ..
        } => None,
        VoiceStateVm::Idle {
            listening_for_wake: true,
            ..
        } => Some(Word::Armed),
        VoiceStateVm::Listening { .. } => Some(Word::Listening),
        VoiceStateVm::Heard { .. } => Some(Word::Heard),
        VoiceStateVm::Sending { answering: false } => Some(Word::Thinking),
        VoiceStateVm::Sending { answering: true } => Some(Word::Answering),
        VoiceStateVm::Speaking => Some(Word::Speaking),
        VoiceStateVm::Failed { .. } => Some(Word::Failed),
    }
}

/// The start/update/end decision, pure over the word the activity shows
/// (`None` when there is no activity) and the new snapshot.
#[must_use]
pub fn step(showing: Option<Word>, state: &VoiceStateVm) -> Step {
    match (showing, word(state)) {
        (None, None) => Step::Keep,
        (Some(_), None) => Step::End {
            last: Word::Off,
            linger: Duration::ZERO,
        },
        (None, Some(Word::Failed)) => Step::Keep,
        (Some(_), Some(Word::Failed)) => Step::End {
            last: Word::Failed,
            linger: FAILURE_LINGER,
        },
        (None, Some(next)) => Step::Start(next),
        (Some(previous), Some(next)) if previous == next => Step::Keep,
        (Some(_), Some(next)) => Step::Update(next),
    }
}

/// The sentence that goes with the word: a failure's reason; otherwise
/// nothing.
#[must_use]
pub fn detail(state: &VoiceStateVm) -> &str {
    match state {
        VoiceStateVm::Failed { reason } => reason,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle(armed: bool) -> VoiceStateVm {
        VoiceStateVm::Idle {
            wake: Some("nixie".to_owned()),
            listening_for_wake: armed,
        }
    }

    fn listening(level: Option<f32>) -> VoiceStateVm {
        VoiceStateVm::Listening {
            heard: String::new(),
            level,
        }
    }

    fn failed() -> VoiceStateVm {
        VoiceStateVm::Failed {
            reason: "The microphone is in use.".to_owned(),
        }
    }

    #[test]
    fn every_turn_state_has_a_word_and_idle_unarmed_has_none() {
        assert_eq!(word(&idle(false)), None);
        assert_eq!(word(&idle(true)), Some(Word::Armed));
        assert_eq!(word(&listening(None)), Some(Word::Listening));
        assert_eq!(
            word(&VoiceStateVm::Heard {
                text: "hi".to_owned(),
                level: Some(0.2)
            }),
            Some(Word::Heard)
        );
        assert_eq!(
            word(&VoiceStateVm::Sending { answering: false }),
            Some(Word::Thinking)
        );
        assert_eq!(
            word(&VoiceStateVm::Sending { answering: true }),
            Some(Word::Answering)
        );
        assert_eq!(word(&VoiceStateVm::Speaking), Some(Word::Speaking));
        assert_eq!(word(&failed()), Some(Word::Failed));
    }

    #[test]
    fn arming_starts_and_disarming_ends() {
        assert_eq!(step(None, &idle(true)), Step::Start(Word::Armed));
        // The same snapshot again (a pane remount re-pushes it): nothing.
        assert_eq!(step(Some(Word::Armed), &idle(true)), Step::Keep);
        assert_eq!(
            step(Some(Word::Armed), &idle(false)),
            Step::End {
                last: Word::Off,
                linger: Duration::ZERO
            }
        );
        assert_eq!(step(None, &idle(false)), Step::Keep);
    }

    #[test]
    fn a_turn_updates_the_word_and_ends_re_armed_as_an_update() {
        assert_eq!(
            step(Some(Word::Armed), &listening(Some(0.1))),
            Step::Update(Word::Listening)
        );
        assert_eq!(
            step(
                Some(Word::Listening),
                &VoiceStateVm::Heard {
                    text: "hi".to_owned(),
                    level: None
                }
            ),
            Step::Update(Word::Heard)
        );
        assert_eq!(
            step(
                Some(Word::Heard),
                &VoiceStateVm::Sending { answering: false }
            ),
            Step::Update(Word::Thinking)
        );
        assert_eq!(
            step(
                Some(Word::Thinking),
                &VoiceStateVm::Sending { answering: true }
            ),
            Step::Update(Word::Answering)
        );
        assert_eq!(
            step(Some(Word::Answering), &VoiceStateVm::Speaking),
            Step::Update(Word::Speaking)
        );
        // Back to armed-and-waiting: the card stays, the word changes — no
        // new request, which the background would refuse.
        assert_eq!(
            step(Some(Word::Speaking), &idle(true)),
            Step::Update(Word::Armed)
        );
    }

    #[test]
    fn a_level_reading_changes_nothing() {
        assert_eq!(
            step(Some(Word::Listening), &listening(Some(0.3))),
            Step::Keep
        );
        assert_eq!(
            step(Some(Word::Listening), &listening(Some(0.9))),
            Step::Keep
        );
        assert_eq!(
            step(Some(Word::Speaking), &VoiceStateVm::Speaking),
            Step::Keep
        );
    }

    #[test]
    fn a_hand_started_turn_shows_and_hides_with_the_turn() {
        assert_eq!(step(None, &listening(None)), Step::Start(Word::Listening));
        assert_eq!(
            step(Some(Word::Listening), &idle(false)),
            Step::End {
                last: Word::Off,
                linger: Duration::ZERO
            }
        );
    }

    #[test]
    fn a_failure_ends_the_card_with_its_sentence_and_never_starts_one() {
        assert_eq!(
            step(Some(Word::Listening), &failed()),
            Step::End {
                last: Word::Failed,
                linger: FAILURE_LINGER
            }
        );
        assert_eq!(
            step(Some(Word::Armed), &failed()),
            Step::End {
                last: Word::Failed,
                linger: FAILURE_LINGER
            }
        );
        // Nothing was showing — a refused arm, or a hand-started turn that
        // failed — leaves no card, and a re-push of the failure is the same.
        assert_eq!(step(None, &failed()), Step::Keep);
        assert_eq!(detail(&failed()), "The microphone is in use.");
        assert_eq!(detail(&idle(true)), "");
        assert!(FAILURE_LINGER > Duration::ZERO);
    }

    #[test]
    fn the_renewal_lands_before_apples_eight_hours() {
        assert!(RENEW_AFTER < Duration::from_secs(8 * 3600));
        assert!(RENEW_AFTER > Duration::from_secs(7 * 3600));
    }

    #[test]
    fn the_words_are_the_strings_the_extension_switches_on() {
        for (word, text) in [
            (Word::Armed, "armed"),
            (Word::Listening, "listening"),
            (Word::Heard, "heard"),
            (Word::Thinking, "thinking"),
            (Word::Answering, "answering"),
            (Word::Speaking, "speaking"),
            (Word::Failed, "failed"),
            (Word::Off, "off"),
        ] {
            assert_eq!(word.as_str(), text);
        }
    }
}
