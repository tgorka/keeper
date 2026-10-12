//! What the Android voice port is told (Epic 98, story 98.4; AD-414, D-36).
//!
//! The port is `voice_android.rs` in the shell over `VoicePlugin.kt` in
//! `gen/android`, and it decides nothing: it reads the device's facts once
//! ([`DeviceFacts`], refreshed when the person changes the language and when
//! keeper comes back in front), carries out what the turn asks, and hands
//! every recogniser result and error here. What those facts and errors mean
//! is this module's:
//!
//! - **Whether keeper can listen, and in which language**
//!   ([`DeviceFacts::availability`]): Android 13 or later
//!   ([`VOICE_FROM_API`]), because that is the first version whose on-device
//!   recogniser (`SpeechRecognizer.createOnDeviceSpeechRecognizer`, Android
//!   12) also listens to an app's own capture (`EXTRA_AUDIO_SOURCE`) in a
//!   segmented session — and keeper's own capture is the one with the echo
//!   canceller on it; the microphone allowed; and a language the device
//!   has installed for on-device recognition, chosen by
//!   [`super::locale::choose`] exactly as on the Apple platforms. A
//!   language the device can download but has not is
//!   [`VoiceUnavailable::NoOnDeviceModel`]; one it cannot run on its own at
//!   all is [`VoiceUnavailable::NoOnDeviceRecognition`]. There is no server
//!   recogniser to fall back to: the port never creates one, and
//!   `voice_on_device` fails the build if it does (NFR-50).
//! - **What a recogniser error is** ([`recovery`]): a session that heard
//!   nothing is started again, always — silence is not a failure; any other
//!   error is started again unless sessions keep dying the moment they
//!   start ([`RESTART_FAILS_WITHIN`], [`RESTARTS_TOLERATED`]); a refusal the
//!   person can act on is said as one.
//! - **Which session is heard** ([`Sessions`]): every start, roll and stop
//!   names a new session, and only the one named last is heard, so the late
//!   words of a session keeper has rolled — before it speaks, on continuing
//!   an answer — are dropped.
//! - **Who is speaking** ([`DeviceFacts::platform`]): the phone or the
//!   tablet, full duplex only where the device has an echo canceller.
//! - **Whether there is an answer yet, and whether keeper may act**
//!   ([`answer`], [`act`]): before the device's facts are read the answer
//!   is [`VoiceUnavailable::NotReadYet`], which is no answer at all
//!   (AD-179) — never `Unsupported`, which would erase every voice surface;
//!   behind another app nothing is heard or said
//!   ([`VoiceUnavailable::LeftTheFront`]).
//! - **What leaving the front does** ([`LEFT_THE_FRONT`]): Android gives an
//!   app without a microphone foreground service silence once it is not in
//!   front, and keeper runs none, so the port stops listening there and the
//!   switch the person left on arms the phrase again when keeper is back in
//!   front, as on the iPhone and the Mac (AD-168, AD-190).
//! - **The turn models** ([`turn_models`]): not on Android in this build, so
//!   a turn ends by the pause (98.4 #7, DW-413).

use std::time::Duration;

use serde::Deserialize;

use super::authorization::{Consent, Permission};
use super::locale::{self, DeviceLocales};
use super::platform::ANDROID_LIMITS;
use super::turn_models::TurnModelsState;
use super::{VoicePlatform, VoiceUnavailable};
use crate::org_account::layout::DeviceClass;

/// The first Android API level keeper listens on: 33, Android 13 — where
/// the on-device recogniser takes an app's own audio (`EXTRA_AUDIO_SOURCE`)
/// in a segmented session (`EXTRA_SEGMENTED_SESSION`).
pub const VOICE_FROM_API: u32 = 33;

/// [`VOICE_FROM_API`] as a person names it.
pub const VOICE_FROM: &str = "Android 13";

/// How many sessions in a row may die at once ([`RESTART_FAILS_WITHIN`])
/// before the error is said instead of the session started again.
pub const RESTARTS_TOLERATED: u32 = 3;

/// A session that ended in an error sooner than this after it started died
/// at once: only such sessions count toward [`RESTARTS_TOLERATED`]. One that
/// lived longer was working, and its end starts the count again.
pub const RESTART_FAILS_WITHIN: Duration = Duration::from_secs(1);

/// Why a turn on Android ended without anyone stopping it: keeper left the
/// front, where Android would give it only silence.
pub const LEFT_THE_FRONT: &str =
    "keeper stopped listening when it left the front; it listens again when you come back to it";

/// What the port's Kotlin half reads about the device (`VoicePlugin.facts`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceFacts {
    /// `Build.VERSION.SDK_INT`.
    pub sdk_int: u32,
    /// The microphone grant as Tauri's plugin reports it: `granted`,
    /// `denied`, `prompt` or `prompt-with-rationale`.
    pub microphone: String,
    /// `SpeechRecognizer.isOnDeviceRecognitionAvailable`.
    pub recogniser: bool,
    /// The languages the on-device recogniser has installed
    /// (`RecognitionSupport.installedOnDeviceLanguages`).
    pub installed: Vec<String>,
    /// The languages it could run once downloaded (`supportedOnDeviceLanguages`
    /// and `pendingOnDeviceLanguages`).
    pub downloadable: Vec<String>,
    /// The system locale (`Locale.getDefault().toLanguageTag()`).
    pub system: String,
    /// `AcousticEchoCanceler.isAvailable()`.
    pub echo_canceller: bool,
    /// The languages of the text-to-speech voices that need no network and
    /// are installed.
    pub voices: Vec<String>,
}

/// What the plugin reports for a permission, as the OS recorded it. A
/// permission Android may still show its dialog for — never asked, or asked
/// once and refused without "don't ask again" — is not determined: one
/// deliberate act may still ask.
pub fn permission(state: &str) -> Permission {
    match state {
        "granted" => Permission::Granted,
        "denied" => Permission::Denied,
        _ => Permission::NotDetermined,
    }
}

impl DeviceFacts {
    /// Both permissions [`super::authorize`] asks about. Android has no
    /// separate grant for speech recognition — the on-device recogniser
    /// listens to keeper's capture, not to the microphone — so only the
    /// microphone is ever asked for.
    pub fn consent(&self) -> Consent {
        Consent {
            speech: Permission::Granted,
            microphone: permission(&self.microphone),
        }
    }

    /// The locales for [`super::locale::choose`]: the system's, and every
    /// installed on-device language, sorted, so the first is the same on
    /// every launch. None when the device has no on-device recogniser.
    pub fn locales(&self) -> DeviceLocales {
        let mut on_device = if self.recogniser {
            self.installed.clone()
        } else {
            Vec::new()
        };
        on_device.sort();
        on_device.dedup();
        DeviceLocales {
            system: self.system.clone(),
            on_device,
        }
    }

    /// The language keeper listens in, or why it cannot listen: the system
    /// first, then the grant, then the language.
    pub fn availability(&self, requested: Option<&str>) -> Result<String, VoiceUnavailable> {
        if self.sdk_int < VOICE_FROM_API {
            return Err(VoiceUnavailable::SystemTooOld {
                needs: VOICE_FROM.to_owned(),
            });
        }
        if permission(&self.microphone) != Permission::Granted {
            return Err(VoiceUnavailable::NotAuthorized);
        }
        let DeviceLocales { system, on_device } = self.locales();
        locale::choose(requested, &system, &on_device).map_err(|refused| match refused {
            VoiceUnavailable::NoOnDeviceRecognition { locale, .. }
                if self.recogniser
                    && self
                        .downloadable
                        .iter()
                        .any(|offered| locale::same(offered, &locale)) =>
            {
                VoiceUnavailable::NoOnDeviceModel { locale }
            }
            other => other,
        })
    }

    /// The platform the port answers on a device of `class`.
    pub fn platform(&self, class: DeviceClass) -> VoicePlatform {
        VoicePlatform::android(class, self.echo_canceller)
    }

    /// The installed voices' languages, each once, sorted.
    pub fn voices(&self) -> Vec<String> {
        let mut voices = self.voices.clone();
        voices.sort();
        voices.dedup();
        voices
    }

    /// The voice to read an answer in `language` with — one of
    /// [`DeviceFacts::voices`], as the engine spells it — or the refusal
    /// that keeps the answer on the screen (AD-182).
    pub fn voice_for(&self, language: &str) -> Result<String, VoiceUnavailable> {
        self.voices
            .iter()
            .find(|voice| locale::same(voice, language))
            .cloned()
            .ok_or_else(|| VoiceUnavailable::NoVoice {
                language: language.to_owned(),
            })
    }
}

/// What the port does after the recogniser reported `code`
/// (`SpeechRecognizer.ERROR_*`) for a session in `locale`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// Start a fresh session on the same capture: nothing the person can do
    /// about it, and nothing heard is lost. `in_a_row` is the count of
    /// sessions that died at once, to hand to [`recovery`] next time.
    Again {
        /// Sessions in a row, this one included, that ended in an error
        /// within [`RESTART_FAILS_WITHIN`] of starting.
        in_a_row: u32,
    },
    /// Stop listening, and say why.
    Refuse(VoiceUnavailable),
}

/// The rule for a recogniser error: `in_a_row` is how many sessions before
/// this one died at once (as the last [`Recovery::Again`] said, `0` after a
/// result), and `lived` how long this one ran before the error.
///
/// A session that heard no words (`ERROR_SPEECH_TIMEOUT` 6,
/// `ERROR_NO_MATCH` 7) is started again every time: a quiet room is not a
/// failing recogniser, and armed listening waits through hours of it. A
/// client or busy error (5, 8) and anything this rule does not name are
/// started again too, but a session that died within
/// [`RESTART_FAILS_WITHIN`] counts, and after [`RESTARTS_TOLERATED`] of
/// those in a row the error is said, so a recogniser that fails at once
/// every time is not a loop; a session that lived starts the count again.
/// The rest are refusals the person can act on: the audio (3) is the
/// microphone; the permission (9) is the grant; a language the recogniser
/// has not downloaded (13) is the download. A language it does not support
/// (12), and a network or server error (1, 2, 4, 10, 11) — which from an
/// on-device recogniser means it could not run here, and keeper never lets
/// it try a server — are the language that cannot run on this device.
pub fn recovery(
    code: i32,
    in_a_row: u32,
    lived: Duration,
    locale: &str,
    facts: &DeviceFacts,
) -> Recovery {
    let cannot_run = || VoiceUnavailable::NoOnDeviceRecognition {
        locale: locale::canonical(locale),
        on_device: facts.locales().on_device,
    };
    let at_once = lived < RESTART_FAILS_WITHIN;
    match code {
        3 => Recovery::Refuse(VoiceUnavailable::NoMicrophone),
        9 => Recovery::Refuse(VoiceUnavailable::NotAuthorized),
        13 => Recovery::Refuse(VoiceUnavailable::NoOnDeviceModel {
            locale: locale::canonical(locale),
        }),
        1 | 2 | 4 | 10 | 11 | 12 => Recovery::Refuse(cannot_run()),
        6 | 7 => Recovery::Again {
            in_a_row: if at_once { in_a_row } else { 0 },
        },
        _ if !at_once => Recovery::Again { in_a_row: 0 },
        _ if in_a_row >= RESTARTS_TOLERATED => Recovery::Refuse(cannot_run()),
        _ => Recovery::Again {
            in_a_row: in_a_row.saturating_add(1),
        },
    }
}

/// Which of the recogniser's sessions the port is hearing (R98AV1-01).
///
/// The port names every session it starts — opening the microphone,
/// rolling to a fresh session on the same capture, the next session after
/// one ended — and the Kotlin half tags every result with that name. Only
/// the session named last is heard, and only while listening: a session
/// keeper rolled before it speaks, or on continuing a paused answer, may
/// still deliver its open segment's words after it was destroyed, and those
/// are words already settled, so they are dropped rather than heard as the
/// person talking over the answer — the failure the iPhone's fresh request
/// fixed (Q11). A stop is never followed by words for the same reason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sessions {
    /// The name of the session started last.
    current: u64,
    /// Whether a capture is up.
    listening: bool,
}

impl Sessions {
    /// Nothing listening.
    pub fn new() -> Self {
        Self::default()
    }

    /// The microphone opens: a new session, heard. Returns its name.
    pub fn open(&mut self) -> u64 {
        self.current += 1;
        self.listening = true;
        self.current
    }

    /// A fresh session on the capture that is up — before keeper speaks, on
    /// continuing an answer, after a session ended. Returns its name, or
    /// `None` with nothing listening: there is no session to roll.
    pub fn roll(&mut self) -> Option<u64> {
        if !self.listening {
            return None;
        }
        self.current += 1;
        Some(self.current)
    }

    /// The microphone is released: nothing is heard until the next
    /// [`Sessions::open`].
    pub fn close(&mut self) {
        self.current += 1;
        self.listening = false;
    }

    /// Whether a capture is up.
    pub fn listening(&self) -> bool {
        self.listening
    }

    /// The name of the session started last.
    pub fn current(&self) -> u64 {
        self.current
    }

    /// Whether a result tagged `session` is heard.
    pub fn hears(&self, session: u64) -> bool {
        self.listening && session == self.current
    }
}

/// The answer to "can keeper listen, and in which language" with what the
/// port has read: [`VoiceUnavailable::NotReadYet`] before the device's facts
/// are in — a question not answered, which the shell keeps unanswered
/// (AD-179) — and never `Unsupported`, which says this build has no voice
/// at all and erases every voice surface (R98AV1-06).
pub fn answer(
    facts: Option<&DeviceFacts>,
    requested: Option<&str>,
) -> Result<String, VoiceUnavailable> {
    facts
        .ok_or(VoiceUnavailable::NotReadYet)?
        .availability(requested)
}

/// The facts to act on — open the microphone, read an answer aloud — or why
/// not: behind another app nothing is heard or said
/// ([`VoiceUnavailable::LeftTheFront`]: Android gives keeper only silence
/// there, and an answer read behind another app is read to nobody who
/// asked; R98AV1-05), and before the facts are read there is nothing to act
/// on yet.
pub fn act(facts: Option<&DeviceFacts>, in_front: bool) -> Result<&DeviceFacts, VoiceUnavailable> {
    if !in_front {
        return Err(VoiceUnavailable::LeftTheFront);
    }
    facts.ok_or(VoiceUnavailable::NotReadYet)
}

/// The turn models on Android: this build links no runtime for them, so a
/// spoken turn ends by the pause rule (98.4 #7; DW-413).
pub fn turn_models() -> TurnModelsState {
    TurnModelsState::Unavailable {
        platform: "Android".to_owned(),
    }
}

/// The Android disclosure's line about voice on a device called `noun`
/// (`disclosure::android`): what voice is here, then [`ANDROID_LIMITS`]
/// itself — the sentence beside the switch, never a paraphrase of it
/// (AD-169), as the iPhone's About line is `VoicePlatform::IOS.limits`.
pub fn disclosure_line(noun: &str) -> String {
    format!(
        "Voice works from {VOICE_FROM}: speech is recognised on this {noun} and never sent to a server, and the answer is read aloud. There are no turn models here, so keeper waits for a pause after you stop. {ANDROID_LIMITS}"
    )
}
