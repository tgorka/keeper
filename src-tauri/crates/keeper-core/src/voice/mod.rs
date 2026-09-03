//! Voice (Epic 62, Stories 62.4–62.6): a turn you can speak, decided here.
//!
//! A **turn** is: the phrase is heard, the microphone opens, what was said
//! becomes text, the text goes to the bot as an ordinary message, the answer
//! streams back, the answer is spoken. The state machine over those steps is
//! [`turn::advance`]; the phrase that starts it is [`phrase::WakePhrase`]; and
//! [`Turn`] is the driver the shell holds — it applies events, matches the
//! phrase while idle, and turns the machine's [`Effect`]s into calls on a
//! [`VoicePort`].
//!
//! Everything that touches CoreAudio or the Speech framework is a port
//! implementation in the `keeper` shell (`voice_ios.rs`). This module names
//! no Apple symbol and links no Apple crate: it is tested on the dev host
//! against a fake port, which is the only way any of it gets tested at all
//! (AD-55/AD-56 — the shell is a call site and decides nothing).
//!
//! # What the port never does
//!
//! Recognition happens **on the device or not at all** (FR-402, NFR-50). A
//! port whose on-device model is missing answers
//! [`VoiceUnavailable::NoOnDeviceModel`] with the locale, and the surface
//! tells the person which language to download; there is no server path to
//! fall back to, here or in the port, because `docs/egress.md` names every
//! destination keeper contacts and Apple's speech servers are not on it.
//!
//! # What armed listening is, and what it costs
//!
//! iOS does not let an app *start* recording from the background, so the
//! phrase is armed by a deliberate act with keeper in front (FR-405: off
//! until chosen). A session armed that way keeps running when another app
//! comes in front or the screen locks, because the shell declares the audio
//! background mode and records in the play-and-record category. What that
//! costs — the microphone indicator, the battery — is said once, in
//! [`LISTENING_LIMITS`], beside the switch (FR-406).

pub mod authorization;
pub mod phrase;
pub mod turn;

use std::sync::Arc;

pub use authorization::{authorize, next_ask, Ask, Consent, ConsentPort, Permission};
pub use phrase::{PhraseRefused, WakePhrase};
pub use turn::{
    advance, silence_budget, Effect, TurnEvent, TurnState, END_OF_UTTERANCE_PAUSE,
    NOTHING_HEARD_TIMEOUT,
};

use crate::vm::{VoiceStateVm, VoiceUnavailableVm};

/// The phrase a fresh install listens for once the switch is turned on. One
/// word, on purpose — see [`phrase::MIN_WORDS`].
pub const DEFAULT_WAKE_PHRASE: &str = "nixie";

/// What armed listening does and does not do on the phone (FR-406, AD-169).
///
/// **The one place these words live.** The surface shows this beside the
/// switch — not in a tooltip, not only in Settings — and `docs/ios.md` quotes
/// it, so the sentence a person reads before turning listening on and the
/// sentence the docs make are the same sentence. Every clause is a fact
/// about iOS, not a to-do: listening cannot be *started* from the background,
/// so it is armed here; once armed it continues while another app is in
/// front or the screen is locked; it ends when the person turns it off, when
/// iOS ends the audio session, or when keeper is force-quit; the system's
/// microphone indicator is on the whole time and no app can hide it; and an
/// open microphone costs battery.
pub const LISTENING_LIMITS: &str = "Turn listening on while keeper is in front and it keeps listening when another app is in front or the screen is locked. It stops when you turn it off, when iOS ends the audio session, or when keeper is force-quit. The microphone indicator stays on the whole time and cannot be hidden, and listening uses battery.";

/// Why the port cannot listen or speak right now.
///
/// Each `Display` is the sentence the surface shows. `NoOnDeviceModel` is the
/// one that matters: it names the locale so the person knows which language
/// to download, and says why keeper will not simply use the network instead.
///
/// `NoRecognizer` is the one that must not be confused with any other. A
/// build whose Xcode project does not link `Speech.framework` still compiles
/// and still ships every `voice_*` command, because objc2 reaches
/// `SFSpeechRecognizer` by name at run time and nothing in the link line
/// demands the framework; the class is simply never registered. Such a
/// build once reported `Unsupported` — the desktop's sentence — and voice
/// looked configured and inert with nothing anyone could act on. So the
/// port names the missing class as a defect of the build, in words that
/// cannot be mistaken for a permission or a language download.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceUnavailable {
    /// The person has not allowed the microphone or speech recognition, or
    /// the device restricts them.
    #[error(
        "keeper is not allowed to use the microphone or speech recognition on this phone — allow both under Settings > keeper"
    )]
    NotAuthorized,
    /// The recogniser exists for the locale but has no on-device model, and
    /// keeper refuses the server fallback.
    #[error(
        "on-device speech recognition for {locale} is not on this phone — download that language under Settings > General > Keyboard > Dictation Languages; keeper never sends your voice to a server"
    )]
    NoOnDeviceModel {
        /// The locale identifier the recogniser was asked for (e.g. `pl_PL`).
        locale: String,
    },
    /// The device has no audio input route.
    #[error("no microphone is available on this device")]
    NoMicrophone,
    /// `SFSpeechRecognizer` is not registered in this process: the build
    /// was linked without `Speech.framework`. A defect of the build, not of
    /// the phone, its settings or its languages.
    #[error(
        "this build of keeper was made without Apple's Speech framework, so it cannot recognise speech on any phone — no setting or language download changes that; install a build whose Xcode project links Speech.framework"
    )]
    NoRecognizer,
    /// This build has no voice port.
    #[error("voice is not available in this build")]
    Unsupported,
}

impl From<&VoiceUnavailable> for VoiceUnavailableVm {
    fn from(why: &VoiceUnavailable) -> Self {
        let message = why.to_string();
        match why {
            VoiceUnavailable::NotAuthorized => VoiceUnavailableVm::NotAuthorized { message },
            VoiceUnavailable::NoOnDeviceModel { locale } => VoiceUnavailableVm::NoOnDeviceModel {
                locale: locale.clone(),
                message,
            },
            VoiceUnavailable::NoMicrophone => VoiceUnavailableVm::NoMicrophone { message },
            VoiceUnavailable::NoRecognizer => VoiceUnavailableVm::NoRecognizer { message },
            VoiceUnavailable::Unsupported => VoiceUnavailableVm::Unsupported { message },
        }
    }
}

/// Where a port delivers what it heard.
///
/// The port calls it from whatever thread the recogniser or synthesiser
/// answers on; the shell's sink hands the event to [`Turn::drive`] on its own
/// terms. A port never holds the turn's lock.
pub type EventSink = Arc<dyn Fn(TurnEvent) + Send + Sync>;

/// The impure half. iOS implements it over `SFSpeechRecognizer`,
/// `AVAudioEngine` and `AVSpeechSynthesizer`; tests implement it over a fake.
/// `keeper-core` never touches CoreAudio.
///
/// A port reports back through the [`EventSink`] it was constructed with:
/// [`TurnEvent::PartialHeard`] and [`TurnEvent::FinalHeard`] from
/// recognition, [`TurnEvent::SpeechDetected`] when a new transcript starts
/// while it is speaking, [`TurnEvent::Silence`] when an utterance it was
/// speaking ends, [`TurnEvent::Failed`] when the OS fails it. It never emits
/// `WakeMatched` — matching is [`Turn`]'s — and never runs a silence clock;
/// that is the shell's, budgeted by [`silence_budget`].
pub trait VoicePort: Send + Sync {
    /// Whether listening and speaking can work right now, and if not, why.
    fn availability(&self) -> Result<(), VoiceUnavailable>;

    /// Open the microphone and start (or restart) recognition on a fresh
    /// request. `wake` is a vocabulary hint for the recogniser — the phrase's
    /// words are what it should expect to hear — never a mode: the port
    /// transcribes, and the turn decides what a transcript means.
    fn start_listening(&self, wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable>;

    /// End recognition and let the microphone and the audio session go.
    /// Idempotent.
    fn stop_listening(&self);

    /// Read `text` aloud. The microphone, if open, stays open for barge-in.
    fn speak(&self, text: &str) -> Result<(), VoiceUnavailable>;

    /// Stop reading aloud immediately. Idempotent.
    fn stop_speaking(&self);
}

/// The turn the shell holds: the machine's state plus the wake phrase, and
/// the two rules that sit between an event and the table.
///
/// **Matching**: while `Idle` with a phrase set, a transcript that contains
/// the phrase becomes [`TurnEvent::WakeMatched`]; every other transcript in
/// `Idle` is noise and is ignored. **Re-arming**: when a turn ends — on its
/// own (silence, an empty transcript, an empty answer, the spoken answer
/// ending) or because the person stopped it — and a phrase is set, the
/// microphone is released for the turn (NFR-51) and opened again for the
/// phrase, because the switch is a standing choice and a stop ends *this
/// turn*, not the listening (Story 62.5: someone driving cannot come back
/// to re-arm it). Only a failure leaves the device released: a port that
/// refused to open would refuse again, and re-arming would be a loop. The
/// switch, not a stop, is what turns listening off.
#[derive(Debug, Default)]
pub struct Turn {
    state: TurnState,
    wake: Option<WakePhrase>,
    /// Whether the last effect touching the device opened it. What the
    /// surface's "listening for the phrase" indicator reads.
    microphone_open: bool,
}

impl Turn {
    /// An idle turn with no phrase.
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the turn is.
    pub fn state(&self) -> &TurnState {
        &self.state
    }

    /// The phrase set, if any.
    pub fn wake(&self) -> Option<&WakePhrase> {
        self.wake.as_ref()
    }

    /// Whether the microphone is open right now, per the effects handed out.
    pub fn microphone_open(&self) -> bool {
        self.microphone_open
    }

    /// Set or clear the wake phrase.
    ///
    /// While `Idle` the device follows the phrase — set opens it, clear
    /// releases it — because a phrase nobody is listening for is a switch
    /// that does nothing (AD-27). Mid-turn the phrase is only recorded; the
    /// turn's own end decides whether to re-arm.
    pub fn set_wake(&mut self, wake: Option<WakePhrase>) -> Vec<Effect> {
        self.wake = wake;
        let effects = if matches!(self.state, TurnState::Idle) {
            if self.wake.is_some() {
                vec![Effect::OpenMicrophone]
            } else {
                vec![Effect::ReleaseMicrophone]
            }
        } else {
            Vec::new()
        };
        self.note_device(&effects);
        effects
    }

    /// Apply `event` and return what the shell must do, in order.
    pub fn apply(&mut self, event: TurnEvent) -> Vec<Effect> {
        let event = match (&self.state, &self.wake, &event) {
            (
                TurnState::Idle,
                Some(wake),
                TurnEvent::PartialHeard(t) | TurnEvent::FinalHeard(t),
            ) if wake.matches(t) => TurnEvent::WakeMatched,
            _ => event,
        };
        let turn_ended = matches!(
            event,
            TurnEvent::Silence
                | TurnEvent::FinalHeard(_)
                | TurnEvent::AnswerDone(_)
                | TurnEvent::Abandoned
        );
        let (next, mut effects) = advance(std::mem::take(&mut self.state), event);
        if matches!(next, TurnState::Idle)
            && turn_ended
            && self.wake.is_some()
            && effects.contains(&Effect::ReleaseMicrophone)
        {
            effects.push(Effect::OpenMicrophone);
        }
        self.state = next;
        self.note_device(&effects);
        effects
    }

    /// Apply `event`, carry out the effects on `port`, and if the port refuses
    /// one, fail the turn — which releases the device — and carry that out
    /// too. Returns every effect that was attempted, for the caller's log.
    pub fn drive(&mut self, event: TurnEvent, port: &dyn VoicePort) -> Vec<Effect> {
        let mut effects = self.apply(event);
        if let Err(why) = perform(&effects, port, self.wake.as_ref()) {
            let recovery = self.apply(TurnEvent::Failed(why.to_string()));
            // A failed release cannot fail: `stop_*` are infallible.
            let _ = perform(&recovery, port, self.wake.as_ref());
            effects.extend(recovery);
        }
        effects
    }

    /// The surface's snapshot of this turn.
    pub fn vm(&self) -> VoiceStateVm {
        match &self.state {
            TurnState::Idle => VoiceStateVm::Idle {
                wake: self.wake.as_ref().map(|w| w.as_str().to_owned()),
                listening_for_wake: self.wake.is_some() && self.microphone_open,
            },
            TurnState::Listening { heard } => VoiceStateVm::Listening {
                heard: heard.clone(),
            },
            TurnState::Heard { text } => VoiceStateVm::Heard { text: text.clone() },
            TurnState::Sending => VoiceStateVm::Sending,
            TurnState::Speaking => VoiceStateVm::Speaking,
            TurnState::Failed { reason } => VoiceStateVm::Failed {
                reason: reason.clone(),
            },
        }
    }

    fn note_device(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::OpenMicrophone => self.microphone_open = true,
                Effect::ReleaseMicrophone => self.microphone_open = false,
                _ => {}
            }
        }
    }
}

/// Carry out `effects` on `port`, in order, stopping at the first refusal.
///
/// [`Effect::SendText`] is not a port call: the text reaches the conversation
/// through the state the shell streams (`VoiceStateVm::Heard`), so here it is
/// nothing to do. Releases and stops are infallible by the port's contract,
/// so the only errors are an `OpenMicrophone` or a `Speak` the device refused.
pub fn perform(
    effects: &[Effect],
    port: &dyn VoicePort,
    wake: Option<&WakePhrase>,
) -> Result<(), VoiceUnavailable> {
    for effect in effects {
        match effect {
            Effect::OpenMicrophone => port.start_listening(wake)?,
            Effect::ReleaseMicrophone => port.stop_listening(),
            Effect::SendText(_) => {}
            Effect::Speak(text) => port.speak(text)?,
            Effect::StopSpeaking => port.stop_speaking(),
        }
    }
    Ok(())
}
