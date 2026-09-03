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
pub mod platform;
pub mod turn;

use std::sync::Arc;

pub use authorization::{authorize, next_ask, Ask, Consent, ConsentPort, Permission};
pub use phrase::{PhraseRefused, WakePhrase};
pub use platform::VoicePlatform;
pub use turn::{
    advance, may_record, silence_budget, Effect, TurnEvent, TurnState, END_OF_UTTERANCE_PAUSE,
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
/// Each variant is a cause; the sentence the surface shows is
/// [`VoiceUnavailable::message`], written once per cause with the
/// platform's nouns filled in from a [`VoicePlatform`] (Story 63.3): the
/// same cause reads "this phone … Settings > keeper" on iOS and "this Mac …
/// System Settings > Privacy & Security > Microphone" on macOS, and no
/// sentence exists twice. `NoOnDeviceModel` is the one that matters: it
/// names the locale so the person knows which language to download, and
/// says why keeper will not simply use the network instead.
///
/// `NoOnDeviceRecognition` is its neighbour and must stay distinct from it:
/// a recogniser that reports `supportsOnDeviceRecognition == false` cannot
/// run on the device whatever is downloaded, so the sentence must not send
/// the person to a download that changes nothing (AD-175).
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceUnavailable {
    /// The person has not allowed the microphone or speech recognition, or
    /// the device restricts them.
    NotAuthorized,
    /// The recogniser exists for the locale but has no on-device model, and
    /// keeper refuses the server fallback.
    NoOnDeviceModel {
        /// The locale identifier the recogniser was asked for (e.g. `pl_PL`).
        locale: String,
    },
    /// The recogniser for the locale reports that it cannot run on the
    /// device at all (`supportsOnDeviceRecognition == false`), and keeper
    /// refuses the server fallback. No download changes this one.
    NoOnDeviceRecognition {
        /// The locale identifier the recogniser was asked for (e.g. `pl_PL`).
        locale: String,
    },
    /// The device has no audio input route.
    NoMicrophone,
    /// `SFSpeechRecognizer` is not registered in this process: the build
    /// was linked without `Speech.framework`. A defect of the build, not of
    /// the device, its settings or its languages.
    NoRecognizer,
    /// This build has no voice port.
    Unsupported,
}

impl VoiceUnavailable {
    /// The sentence the surface shows, with `platform`'s nouns in it.
    ///
    /// Every hole is one of [`VoicePlatform`]'s fields; the words around the
    /// holes are the same on every platform. `NoRecognizer` names the kind
    /// of device only to say that no device of that kind is at fault — the
    /// defect is the build's — and `Unsupported` has nothing to name.
    pub fn message(&self, platform: &VoicePlatform) -> String {
        let VoicePlatform {
            noun,
            allow,
            download,
            ..
        } = platform;
        match self {
            Self::NotAuthorized => format!(
                "keeper is not allowed to use the microphone or speech recognition on this {noun} — {allow}"
            ),
            Self::NoOnDeviceModel { locale } => format!(
                "on-device speech recognition for {locale} is not on this {noun} — {download}; keeper never sends your voice to a server"
            ),
            Self::NoOnDeviceRecognition { locale } => format!(
                "speech recognition for {locale} cannot run on this {noun} itself, only through a server, and keeper never sends your voice to a server — downloading a language does not change that"
            ),
            Self::NoMicrophone => "no microphone is available on this device".to_owned(),
            Self::NoRecognizer => format!(
                "this build of keeper was made without Apple's Speech framework, so it cannot recognise speech on any {noun} — no setting or language download changes that; install a build whose Xcode project links Speech.framework"
            ),
            Self::Unsupported => "voice is not available in this build".to_owned(),
        }
    }

    /// The refusal as the surface receives it: the kind, the locale where
    /// there is one, and [`VoiceUnavailable::message`].
    pub fn vm(&self, platform: &VoicePlatform) -> VoiceUnavailableVm {
        let message = self.message(platform);
        match self {
            Self::NotAuthorized => VoiceUnavailableVm::NotAuthorized { message },
            Self::NoOnDeviceModel { locale } => VoiceUnavailableVm::NoOnDeviceModel {
                locale: locale.clone(),
                message,
            },
            Self::NoOnDeviceRecognition { locale } => VoiceUnavailableVm::NoOnDeviceRecognition {
                locale: locale.clone(),
                message,
            },
            Self::NoMicrophone => VoiceUnavailableVm::NoMicrophone { message },
            Self::NoRecognizer => VoiceUnavailableVm::NoRecognizer { message },
            Self::Unsupported => VoiceUnavailableVm::Unsupported { message },
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
/// A port names the platform it runs on ([`VoicePort::platform`]), and that
/// is required rather than defaulted: a default would be a platform nobody
/// named, and the whole of Story 63.3 is that the differences between the
/// platforms are named, not discovered (AD-175).
///
/// A port reports back through the [`EventSink`] it was constructed with:
/// [`TurnEvent::PartialHeard`] and [`TurnEvent::FinalHeard`] from
/// recognition, [`TurnEvent::SpeechDetected`] when a new transcript starts
/// while it is speaking, [`TurnEvent::Silence`] when an utterance it was
/// speaking ends, [`TurnEvent::Failed`] when the OS fails it. It never emits
/// `WakeMatched` — matching is [`Turn`]'s — and never runs a silence clock;
/// that is the shell's, budgeted by [`silence_budget`].
pub trait VoicePort: Send + Sync {
    /// The platform this port runs on: the nouns its refusals name and
    /// whether the OS keeps its own voice out of the transcript.
    fn platform(&self) -> VoicePlatform;

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

    /// Read `text` aloud. Whether the microphone is open meanwhile is the
    /// turn's decision ([`may_record`]): on a full-duplex platform it stays
    /// open for barge-in; on a half-duplex one the turn released it first.
    fn speak(&self, text: &str) -> Result<(), VoiceUnavailable>;

    /// Stop reading aloud immediately. Idempotent.
    fn stop_speaking(&self);
}

/// The turn the shell holds: the machine's state plus the wake phrase, and
/// the three rules that sit between an event and the table.
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
/// switch, not a stop, is what turns listening off. **Half-duplex**
/// (AD-175): where [`may_record`] says the device may not be open in the
/// state the table moved to, the turn does not open it and releases it if
/// it was — before the `Speak`, so the port never records its own answer.
/// On iOS the rule allows every state the table opens the device in, so
/// nothing there changes.
#[derive(Debug)]
pub struct Turn {
    platform: VoicePlatform,
    state: TurnState,
    wake: Option<WakePhrase>,
    /// Whether the last effect touching the device opened it. What the
    /// surface's "listening for the phrase" indicator reads.
    microphone_open: bool,
}

impl Turn {
    /// An idle turn with no phrase, on `platform` — the port's own answer,
    /// [`VoicePort::platform`].
    pub fn new(platform: VoicePlatform) -> Self {
        Self {
            platform,
            state: TurnState::Idle,
            wake: None,
            microphone_open: false,
        }
    }

    /// The platform this turn's refusals and its half-duplex rule are for.
    pub fn platform(&self) -> &VoicePlatform {
        &self.platform
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
        if !may_record(&self.platform, &next) && device_after(self.microphone_open, &effects) {
            // AD-175: the table would leave the device open in a state this
            // platform may not record in. Do not open it, and close it if it
            // is open — first, so it is closed before the `Speak`.
            effects.retain(|effect| *effect != Effect::OpenMicrophone);
            if self.microphone_open {
                effects.insert(0, Effect::ReleaseMicrophone);
            }
        }
        self.state = next;
        self.note_device(&effects);
        debug_assert!(
            !self.microphone_open || may_record(&self.platform, &self.state),
            "the device is open in a state the platform may not record in"
        );
        effects
    }

    /// Apply `event`, carry out the effects on `port`, and if the port refuses
    /// one, fail the turn — which releases the device — and carry that out
    /// too. Returns every effect that was attempted, for the caller's log.
    pub fn drive(&mut self, event: TurnEvent, port: &dyn VoicePort) -> Vec<Effect> {
        let mut effects = self.apply(event);
        if let Err(why) = perform(&effects, port, self.wake.as_ref()) {
            let recovery = self.apply(TurnEvent::Failed(why.message(&self.platform)));
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
        self.microphone_open = device_after(self.microphone_open, effects);
    }
}

/// Whether the device is open once `effects` have been carried out, given
/// that it is `open` now.
fn device_after(open: bool, effects: &[Effect]) -> bool {
    effects.iter().fold(open, |open, effect| match effect {
        Effect::OpenMicrophone => true,
        Effect::ReleaseMicrophone => false,
        _ => open,
    })
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
