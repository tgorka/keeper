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
//! Recognition happens **on the device or not at all** (FR-402, NFR-50). The
//! language it runs in is [`locale::choose`]'s answer — the person's choice
//! when it can run here, otherwise a refusal that names what can — and a
//! port whose on-device model is missing answers
//! [`VoiceUnavailable::NoOnDeviceModel`] with the locale, so the surface
//! tells the person which language to download. There is no server path to
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
//! [`platform::VoicePlatform::limits`], beside the switch (FR-406) - per
//! platform, because what takes the microphone away has a name and it is not
//! the same name on a Mac as on a phone.

pub mod authorization;
pub mod events;
pub mod level;
pub mod locale;
pub mod phrase;
pub mod platform;
pub mod speech;
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

/// Why the port cannot listen or speak right now.
///
/// Each variant is a cause; the sentence the surface shows is
/// [`VoiceUnavailable::message`], written once per cause with the
/// platform's nouns filled in from a [`VoicePlatform`] (Story 63.3): the
/// same cause reads "this phone … Settings > keeper" on iOS and "this Mac …
/// System Settings > Privacy & Security > Microphone" on macOS, and no
/// sentence exists twice. `NoOnDeviceModel` names the locale so the person
/// knows which language to download, and says why keeper will not simply
/// use the network instead.
///
/// `NoOnDeviceRecognition` is its neighbour and stays distinct from it: the
/// recogniser for that locale reports `supportsOnDeviceRecognition == false`,
/// which is the OS saying it has no on-device asset for the language ("No
/// Assistant asset for language pl-PL" in its own log). Adding the language
/// to the system's dictation languages may make one available — the
/// evidence does not say it always does, so the sentence says "may" — and
/// the other way out is to choose a language this device can already run,
/// which the refusal carries and names ([`locale::choose`]).
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
    /// The recogniser exists for the locale and can run on the device, but
    /// its model is not available right now, and keeper refuses the server
    /// fallback.
    NoOnDeviceModel {
        /// The locale identifier the recogniser was asked for (e.g. `pl-PL`).
        locale: String,
    },
    /// The recogniser for the locale reports no on-device recognition
    /// (`supportsOnDeviceRecognition == false`), or the locale is not one
    /// the framework lists at all, and keeper refuses the server fallback.
    /// Carries what can run instead.
    NoOnDeviceRecognition {
        /// The locale identifier that was asked for, in canonical form
        /// (e.g. `pl-PL`).
        locale: String,
        /// Every locale this device can run on its own, in the port's
        /// sorted order; empty when none can.
        on_device: Vec<String>,
    },
    /// The device has no audio input route.
    NoMicrophone,
    /// `SFSpeechRecognizer` is not registered in this process: the build
    /// was linked without `Speech.framework`. A defect of the build, not of
    /// the device, its settings or its languages.
    NoRecognizer,
    /// The answer is in a language this device has no synthesiser voice
    /// for (Epic 64, AD-182), so it stays on the screen: a voice of another
    /// language reading it is the failure the epic opens with, never a
    /// fallback. Carries the language as [`speech::choose_voice`] named it.
    NoVoice {
        /// The detected language, as the detector spelled it (`pl`).
        language: String,
    },
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
            voice_download,
            ..
        } = platform;
        match self {
            Self::NotAuthorized => format!(
                "keeper is not allowed to use the microphone or speech recognition on this {noun} — {allow}"
            ),
            Self::NoOnDeviceModel { locale } => format!(
                "on-device speech recognition for {locale} is not on this {noun} — {download}; keeper never sends your voice to a server"
            ),
            Self::NoOnDeviceRecognition { locale, on_device } if on_device.is_empty() => format!(
                "this {noun} has no on-device speech recognition for {locale} or for any other language, and keeper never sends your voice to a server — {download}, which may add it"
            ),
            Self::NoOnDeviceRecognition { locale, on_device } => format!(
                "this {noun} has no on-device speech recognition for {locale}, and keeper never sends your voice to a server — {download}, which may add it, or choose a language this {noun} can already run on its own: {}",
                on_device.join(", ")
            ),
            Self::NoMicrophone => "no microphone is available on this device".to_owned(),
            Self::NoVoice { language } => format!(
                "this {noun} has no voice for {}, so the answer stays on the screen instead of being read aloud — {voice_download}",
                speech::describe(language)
            ),
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
            Self::NoOnDeviceRecognition { locale, .. } => {
                VoiceUnavailableVm::NoOnDeviceRecognition {
                    locale: locale.clone(),
                    message,
                }
            }
            Self::NoMicrophone => VoiceUnavailableVm::NoMicrophone { message },
            Self::NoVoice { language } => VoiceUnavailableVm::NoVoice {
                language: language.clone(),
                message,
            },
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
/// platforms are named, not discovered (AD-175). The two locale methods are
/// required for the same reason: a port that defaulted to "no locale can
/// run here" or that silently ignored the person's choice would be a port
/// nobody noticed was wrong.
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

    /// The locales this device can recognise on its own, and the system's,
    /// as the OS spells them ([`locale::DeviceLocales`]). Enumerating them
    /// means constructing one recogniser per supported locale — measured
    /// at 0.41 s for 63 on a Mac — so a port answers from a cache it fills
    /// once, not per probe.
    fn locales(&self) -> locale::DeviceLocales;

    /// Record the person's choice of locale (`bots.voice_locale`; `None` is
    /// "choose for me"). The port hands it to [`locale::choose`] with its
    /// own enumeration before every availability probe and every request,
    /// and builds the recogniser for the answer; it decides nothing itself.
    fn set_locale(&self, requested: Option<String>);

    /// Open the microphone and start (or restart) recognition on a fresh
    /// request. `wake` is a vocabulary hint for the recogniser — the phrase's
    /// words are what it should expect to hear — never a mode: the port
    /// transcribes, and the turn decides what a transcript means.
    fn start_listening(&self, wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable>;

    /// End recognition and let the microphone and the audio session go.
    /// Idempotent.
    fn stop_listening(&self);

    /// The languages this device has a synthesiser voice for, as the
    /// framework spells them (`pl-PL`, `en-US`), each once, sorted.
    /// Enumerating the voices is a framework call the port caches the way
    /// it caches [`VoicePort::locales`], refreshed on the same deliberate
    /// act, so a voice downloaded while keeper runs is seen the next time
    /// the language is chosen.
    fn voices(&self) -> Vec<String>;

    /// The locale recognition runs in — [`locale::in_force`]'s answer over
    /// the person's choice and this device's enumeration. The language a
    /// spoken turn was asked in, and the one its answer is spoken in when
    /// the text's own language cannot be told ([`speech::choose_voice`]).
    fn listening(&self) -> String;

    /// The dominant language of `text`, from among `constraints` (language
    /// subtags, `pl`, `en`), or `None` when the detector cannot tell.
    /// On-device or not at all (AD-188): the port never sends the text
    /// anywhere to have it classified. A port with no detector answers
    /// `None`, which [`speech::choose_voice`] reads as "the listening
    /// language".
    fn detect_language(&self, text: &str, constraints: &[String]) -> Option<String>;

    /// Read `text` aloud in the voice for `language` — one of
    /// [`VoicePort::voices`], chosen by [`speech::choose_voice`]. A port
    /// whose framework answers no voice for it refuses with
    /// [`VoiceUnavailable::NoVoice`] rather than reading in the default
    /// voice (AD-27): a mismatched voice is the exact failure Epic 64 opens
    /// with. Whether the microphone is open meanwhile is the turn's decision
    /// ([`may_record`]): on a full-duplex platform it stays open for
    /// barge-in; on a half-duplex one the turn released it first.
    fn speak(&self, text: &str, language: &str) -> Result<(), VoiceUnavailable>;

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
/// refused to open would refuse again, and re-arming would be a loop — it is
/// tried again by [`should_rearm`] once the refusal has cleared, not before.
/// The switch, not a stop, is what turns listening off. **Half-duplex**
/// (AD-175): where [`may_record`] says the device may not be open in the
/// state the table moved to, the turn does not open it and releases it if
/// it was — before the `Speak`, so the port never records its own answer.
/// On iOS the rule allows every state the table opens the device in, so
/// nothing there changes. **Level** (Story 64.3, AD-186): a
/// [`TurnEvent::Level`] is not a transition. It is recorded while the device
/// is open for a turn — `Listening` and `Heard` — and cleared when the turn
/// moves anywhere else, so a snapshot never carries a level from a
/// microphone that is closed.
#[derive(Debug)]
pub struct Turn {
    platform: VoicePlatform,
    state: TurnState,
    wake: Option<WakePhrase>,
    /// Whether the last effect touching the device opened it. What the
    /// surface's "listening for the phrase" indicator reads.
    microphone_open: bool,
    /// The last level the port reported while `Listening` or `Heard`;
    /// `None` before the first reading and in every other state.
    level: Option<f32>,
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
            level: None,
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

    /// The input level the snapshot carries: the port's last reading while
    /// the device is open for a turn, `None` before the first and elsewhere.
    pub fn level(&self) -> Option<f32> {
        self.level
    }

    /// Whether a send made now belongs to this turn — the turn heard
    /// something and is waiting for the answer to speak (Epic 64, AD-182).
    /// The same predicate the surface applies before it reads an answer
    /// aloud, so the instruction on the request and the voice on the answer
    /// are decided by one rule.
    pub fn awaiting_send(&self) -> bool {
        matches!(
            self.state,
            TurnState::Heard { .. } | TurnState::Sending { .. }
        )
    }

    /// Whether the standing choice is in force (Epic 65, AD-190): a phrase
    /// is set and either the device is open for it, or a turn is running,
    /// which re-arms it on its own end. False with no phrase, and false in
    /// `Idle` or `Failed` with the device released — the two states a port's
    /// refusal leaves behind, and the only ones [`should_rearm`] acts in.
    pub fn armed(&self) -> bool {
        self.wake.is_some()
            && (self.microphone_open
                || !matches!(self.state, TurnState::Idle | TurnState::Failed { .. }))
    }

    /// Set or clear the wake phrase.
    ///
    /// While `Idle` the device follows the phrase — set opens it, clear
    /// releases it — because a phrase nobody is listening for is a switch
    /// that does nothing (AD-27). `Failed` is the same resting place with a
    /// reason attached (the device is already released), so it follows the
    /// phrase too and moves to `Idle`: that is how a refusal at arming is
    /// tried again once it clears (AD-190), and how turning the switch off
    /// takes the stale reason with it. Mid-turn the phrase is only recorded;
    /// the turn's own end decides whether to re-arm.
    pub fn set_wake(&mut self, wake: Option<WakePhrase>) -> Vec<Effect> {
        self.wake = wake;
        let effects = if matches!(self.state, TurnState::Idle | TurnState::Failed { .. }) {
            self.state = TurnState::Idle;
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
    ///
    /// A [`TurnEvent::Level`] never reaches the table: it is recorded for
    /// the snapshot where the device is open for a turn and dropped
    /// elsewhere, with no effects either way.
    pub fn apply(&mut self, event: TurnEvent) -> Vec<Effect> {
        if let TurnEvent::Level(level) = event {
            if self.meters() {
                self.level = Some(level.clamp(0.0, 1.0));
            }
            return Vec::new();
        }
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
        if !self.meters() {
            self.level = None;
        }
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
                level: self.level,
            },
            TurnState::Heard { text } => VoiceStateVm::Heard {
                text: text.clone(),
                level: self.level,
            },
            TurnState::Sending { answering } => VoiceStateVm::Sending {
                answering: *answering,
            },
            TurnState::Speaking => VoiceStateVm::Speaking,
            TurnState::Failed { reason } => VoiceStateVm::Failed {
                reason: reason.clone(),
            },
        }
    }

    /// Whether the state is one whose snapshot carries a level: the device
    /// is open for a turn, not merely for the phrase.
    fn meters(&self) -> bool {
        matches!(
            self.state,
            TurnState::Listening { .. } | TurnState::Heard { .. }
        )
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
/// nothing to do. [`Effect::Speak`] is where the voice is chosen (AD-182,
/// AD-183): the port supplies its inventory, the listening locale and the
/// text's detected language, [`speech::choose_voice`] decides, and the port
/// is told which language to speak in. Releases and stops are infallible by
/// the port's contract, so the only errors are an `OpenMicrophone` or a
/// `Speak` the device refused — including a language it has no voice for.
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
            Effect::Speak(text) => {
                let voices = port.voices();
                let listening = port.listening();
                let detected =
                    port.detect_language(text, &speech::constraints(&listening, &voices));
                let language = speech::choose_voice(detected.as_deref(), &listening, &voices)?;
                port.speak(text, &language)?;
            }
            Effect::StopSpeaking => port.stop_speaking(),
        }
    }
    Ok(())
}

/// Whether keeper arms the phrase again without being asked (Epic 65,
/// Story 65.2, AD-190).
///
/// `bots.wake_enabled` is the person's `intent`, persisted as chosen even
/// when the port refused at arming time. A refusal is shown, not saved as
/// "no" — so when it clears (a grant, a language change, keeper back in
/// front, the port's own resume) the shell asks this and re-runs the arm.
/// Three facts, one answer: never with the intent off — that is the one
/// "no" the person did say — never while [`Turn::armed`] already holds, and
/// never while the port still refuses, because a refusal re-tried on every
/// foreground would be the loop the turn's own re-arm rule avoids.
pub fn should_rearm(intent: bool, armed: bool, refusal_cleared: bool) -> bool {
    intent && !armed && refusal_cleared
}
