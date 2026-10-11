//! Whether what the person said over an answer is a backchannel (AD-411,
//! ruling R14, story 97.3).
//!
//! Speech over an answer pauses it at once ([`super::turn`]); what was said
//! then decides what follows. A *backchannel* — "mhm", "yeah" — resumes the
//! answer; the stop phrase ends it ([`super::Turn`] matches it first);
//! anything else stops it and is the next question. This file holds the
//! rule, pure:
//!
//! - **The words.** The whole utterance, folded the way the wake and stop
//!   phrases are ([`super::phrase::normalise`]: case, diacritics, every
//!   non-letter a word boundary), equals an entry of the list for the
//!   language recognition runs in ([`backchannels`]). The list is per
//!   language because "no" is Polish for "yeah" and English for "no", and
//!   resuming over an objection is the failure this exists to prevent. "Uh
//!   huh" is an entry, so a two-word utterance is a backchannel only when the
//!   pair is listed.
//! - **The duration**, with the turn models only. An utterance the
//!   recogniser gave no words for and the voice activity model measured at
//!   under [`BACKCHANNEL_SPEECH`] is a backchannel — the "mhm" no recogniser
//!   spells. Words, when there are any, are the list's to judge: a short "no"
//!   in English is still an objection.
//! - **A false onset** — no words and under [`FALSE_ONSET_SPEECH`], or no
//!   words and no speech the voice activity model ever ended — resumes the
//!   answer as a backchannel would, and is counted apart (Q8): it is echo or
//!   a click, not the person.
//!
//! The list and the duration are a word list and a number, not an acoustic
//! model (DW-403).

use std::time::Duration;

use super::locale;
use super::phrase::normalise;

/// Speech shorter than this, with no words, is a backchannel (with the
/// turn models): the duration rule's line.
pub const BACKCHANNEL_SPEECH: Duration = Duration::from_millis(600);

/// Speech shorter than this, with no words, is a false onset: the pause had
/// nothing in it.
pub const FALSE_ONSET_SPEECH: Duration = Duration::from_millis(250);

/// What an utterance that paused an answer turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Utterance {
    /// No words and too little speech to be anyone: resume.
    FalseOnset,
    /// No words and a short burst of speech: resume.
    BackchannelByDuration,
    /// Words the list holds: resume.
    BackchannelByWords,
    /// Anything else: stop the answer and ask this, the words heard. Blank
    /// when the person spoke at length and the recogniser gave no words.
    Question(String),
}

impl Utterance {
    /// Whether the answer resumes after it.
    pub fn resumes(&self) -> bool {
        !matches!(self, Self::Question(_))
    }
}

/// The backchannels of `language` (a language subtag, `en`, `pl`), folded
/// as [`normalise`] folds an utterance. A language with no list of its own
/// has the two that need no language.
pub fn backchannels(language: &str) -> &'static [&'static str] {
    match language {
        "en" => &["mhm", "uh huh", "yeah", "yep", "aha", "okay", "right"],
        "pl" => &["mhm", "aha", "tak", "no", "okej", "jasne"],
        _ => &["mhm", "aha"],
    }
}

/// Whether `words` — what the recogniser heard, blank when it heard none —
/// over `speech_ms` of speech the voice activity model measured (`None`
/// without the turn models, or before it heard the speech end) is a
/// backchannel in the language of the locale `listening` (97.3 #3).
pub fn is_backchannel(words: &str, speech_ms: Option<u32>, listening: &str) -> bool {
    let folded = normalise(words);
    let folded = folded.trim();
    if folded.is_empty() {
        return speech_ms.is_some_and(|ms| u128::from(ms) < BACKCHANNEL_SPEECH.as_millis());
    }
    backchannels(&locale::language(listening)).contains(&folded)
}

/// What the utterance is once nothing more of it is coming — the
/// recogniser's final words came, or the pause's decision window ran out.
pub fn settle(words: &str, speech_ms: Option<u32>, listening: &str) -> Utterance {
    if normalise(words).trim().is_empty() {
        return match speech_ms {
            None => Utterance::FalseOnset,
            Some(ms) if u128::from(ms) < FALSE_ONSET_SPEECH.as_millis() => Utterance::FalseOnset,
            Some(ms) if u128::from(ms) < BACKCHANNEL_SPEECH.as_millis() => {
                Utterance::BackchannelByDuration
            }
            Some(_) => Utterance::Question(String::new()),
        };
    }
    if is_backchannel(words, speech_ms, listening) {
        Utterance::BackchannelByWords
    } else {
        Utterance::Question(words.trim().to_owned())
    }
}

/// What the utterance is at the moment the voice activity model heard it
/// end after `speech_ms`: decided there only when the recogniser gave no
/// words and the speech was short — the duration's verdict. Words wait for
/// the recogniser's last of them; long wordless speech waits for words.
pub fn at_speech_end(words: &str, speech_ms: u32) -> Option<Utterance> {
    if !normalise(words).trim().is_empty() {
        return None;
    }
    let ms = u128::from(speech_ms);
    if ms < FALSE_ONSET_SPEECH.as_millis() {
        Some(Utterance::FalseOnset)
    } else if ms < BACKCHANNEL_SPEECH.as_millis() {
        Some(Utterance::BackchannelByDuration)
    } else {
        None
    }
}
