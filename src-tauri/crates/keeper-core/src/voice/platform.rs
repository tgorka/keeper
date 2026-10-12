//! The words a port's platform supplies (Epic 63, Story 63.3, AD-175).
//!
//! `keeper-core` decides what a refusal says; the port decides which device
//! it is saying it about. This module is the seam between the two: every
//! sentence in [`super::VoiceUnavailable`] is written once, with holes, and a
//! [`VoicePlatform`] fills the holes with the nouns of the device it runs on
//! — "this phone" and the Settings path on iOS, "this Mac" and the System
//! Settings path on macOS. No sentence exists twice, and this crate keeps no
//! `cfg(target_os)` (AD-55): a port answers [`super::VoicePort::platform`]
//! with the value for its platform, and that is how the platform is named
//! rather than discovered.
//!
//! The one non-lexical difference lives here too, because it is a fact about
//! the platform and not about any turn: whether the OS keeps keeper's own
//! voice out of the transcript while an utterance is read aloud
//! ([`VoicePlatform::full_duplex`]). Both platforms do, through the same
//! call: `AVAudioEngine.inputNode.setVoiceProcessingEnabled(true)`, which
//! is `kAudioUnitSubType_VoiceProcessingIO` — on iOS through the audio
//! session, on macOS on the input node directly, where its echo reference
//! is the output device's mix (Story 22.7 measured the far end dropping
//! ~24 dB on hesperia). What macOS lacks is only the `AVAudioSession`
//! around it, which is ducking and interruptions, not the canceller. A Mac
//! whose input is an aggregate device (BlackHole, Loopback) is refused the
//! unit — it builds one itself — and there the port answers half duplex
//! for that capture with [`VoicePlatform::half_duplex_sentence`] beside the
//! switch, while the turn's rule ([`super::may_record`]) stays the
//! platform's (Epic 68, AD-213).
//!
//! Android is the platform where duplex is not the platform's at all
//! (Epic 98, story 98.4): whether the capture has an echo canceller is a
//! fact of the device, so [`VoicePlatform::ANDROID`] is half duplex and the
//! port answers [`VoicePlatform::android`] with what its device offers.

use crate::org_account::layout::DeviceClass;

/// What armed listening is on Android, beside the switch and in About — one
/// sentence for every Android device (AD-169): it names both the device
/// with an echo canceller and the one without, because which one this is
/// is read from the device after the sentence may already be on screen
/// ([`VoicePlatform::android`]).
pub const ANDROID_LIMITS: &str = "Turn listening on while keeper is in front and it listens while keeper stays in front with the screen on. Switching to another app or turning the screen off stops it, and it starts again whenever keeper is back in front, after a relaunch too. Where this device has no echo canceller, keeper stops listening while it speaks, so saying the stop word over an answer does nothing and the button stops it. It stops when you turn it off. Android shows its microphone indicator the whole time, and listening uses battery.";

/// The nouns and the one capability that differ between the platforms a
/// [`super::VoicePort`] runs on.
///
/// Every field is a fact about the platform, verified where it was written
/// down, and the sentences that use them are in [`super::VoiceUnavailable`].
/// A port on a platform not named here is a new constant in this file, not
/// a new sentence anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoicePlatform {
    /// The device, as the person would call it: "phone", "Mac". Sentences
    /// say "this {noun}" and "any {noun}".
    pub noun: &'static str,
    /// What to do about a missing microphone or speech-recognition grant,
    /// as an imperative clause: where the grants live on this platform.
    pub allow: &'static str,
    /// What to do about a language whose on-device model is not installed,
    /// as an imperative clause: where the download lives on this platform.
    pub download: &'static str,
    /// What to do about a language this device has no synthesiser voice for
    /// (Epic 64, AD-182), as an imperative clause: where voices are
    /// downloaded on this platform. A different place from `download`,
    /// because recognition and synthesis are different inventories — a
    /// dictation language and a voice are installed on different pages.
    pub voice_download: &'static str,
    /// Whether the OS keeps the port's own voice out of the transcript, so
    /// the microphone may stay open while an utterance is read aloud. Where
    /// it is `false`, [`super::may_record`] answers `false` while speaking
    /// and the turn releases the microphone before it speaks (AD-175). Both
    /// Apple platforms answer `true` since AD-213; a capture that could not
    /// get voice processing is the port's fact, told through
    /// [`super::VoicePort::half_duplex`].
    pub full_duplex: bool,
    /// What armed listening costs and what ends it, as the whole sentence
    /// shown beside the switch (FR-406). Per platform because the thing
    /// that takes the microphone away has a name, and on a Mac that name is
    /// not iOS: a screenshot of the real macOS build on 2026-09-04 showed
    /// the iOS sentence sitting under a Mac's switch, saying "when iOS ends
    /// the audio session" to somebody holding a MacBook.
    pub limits: &'static str,
}

impl VoicePlatform {
    /// iOS: the phone. Grants live under the app's own Settings page; the
    /// on-device model is a dictation language download; and the audio
    /// session's voice processing lets recognition run while the answer
    /// plays.
    ///
    /// The limits are Apple's documented interruptions (Epic 65, AD-193),
    /// each named by what the person sees rather than by the notification:
    /// `UIBackgroundModes: audio` keeps an active session alive with
    /// another app in front or the screen locked; Siri and a non-mixing app
    /// deactivate it and the port re-arms when they let go (Siri sends no
    /// end, so the port asks again on a clock); an accepted call suspends
    /// the app, and a record session cannot be reactivated from the
    /// background afterwards, so it stays stopped until keeper is opened —
    /// which is what re-arms it. Force-quit and the switch are the person's
    /// own ends. The orange dot is the system's and stays regardless.
    pub const IOS: Self = Self {
        noun: "phone",
        allow: "allow both under Settings > keeper",
        download:
            "download that language under Settings > General > Keyboard > Dictation Languages",
        voice_download: "download a voice for it under Settings > Accessibility > Spoken Content > Voices",
        full_duplex: true,
        limits: "Turn listening on while keeper is in front and it keeps listening when another app is in front or the screen is locked. Siri or an app that takes the microphone pauses it and keeper resumes on its own; a phone call ends it until you open keeper again. It stops when you turn it off or when keeper is force-quit. The orange microphone indicator stays on the whole time and cannot be hidden, and listening uses battery.",
    };

    /// macOS: the Mac. The microphone grant lives in System Settings >
    /// Privacy & Security > Microphone; on-device recognition depends on
    /// Dictation being on and its language downloaded, under System
    /// Settings > Keyboard > Dictation; and the input node's voice
    /// processing keeps keeper's answer out of its own microphone (AD-213,
    /// revising AD-175), so the port records while it speaks.
    pub const MACOS: Self = Self {
        noun: "Mac",
        allow: "allow the microphone under System Settings > Privacy & Security > Microphone",
        download: "turn Dictation on and download that language under System Settings > Keyboard > Dictation",
        voice_download: "download a voice for it under System Settings > Accessibility > Spoken Content > System Voice",
        full_duplex: true,
        // No screen-lock clause: with no `AVAudioSession` there is no
        // documented macOS rule about a locked screen that this session
        // verified, and a sentence beside a switch is the wrong place to
        // guess. What IS known is stated.
        limits: "Turn listening on while keeper is in front and it keeps listening when another app is in front. It stops when you turn it off, when the system takes the microphone away, or when keeper quits. The microphone indicator in the menu bar stays on the whole time and cannot be hidden, and listening uses battery.",
    };

    /// Android: the phone or the tablet (Epic 98, story 98.4; AD-414, D-36).
    /// The microphone grant lives on the app's own permissions page; the
    /// on-device model is a language the system's on-device speech
    /// recognition downloads; voices are the text-to-speech engine's.
    ///
    /// Half duplex here, because nothing about Android as a platform keeps
    /// keeper's answer out of its microphone: that is a fact of the device —
    /// whether `AcousticEchoCanceler.isAvailable()` — which the port reads at
    /// run time and answers through [`VoicePlatform::android`] (the codemap's
    /// Q15). The settings paths differ between makers, so each names the
    /// page by what it is and gives the usual path as an example.
    ///
    /// The limits are Android's own rule for the microphone: an app with no
    /// foreground service of the microphone type hears silence once it
    /// leaves the front, and keeper runs none. So listening stops when
    /// keeper leaves the front or the screen goes off, and starts again,
    /// still switched on, when keeper is back in front (AD-168, AD-190).
    pub const ANDROID: Self = Self {
        noun: "phone",
        allow: "allow the microphone for keeper under Settings > Apps > keeper > Permissions",
        download: "download that language in the system's on-device speech recognition settings (Settings > System > Languages & input on most devices)",
        voice_download: "install a voice for it in the system's text-to-speech settings (Settings > Accessibility > Text-to-speech output on most devices)",
        full_duplex: false,
        limits: ANDROID_LIMITS,
    };

    /// The Android platform a port on a device of `class` answers, where
    /// `echo_cancelled` is whether the device offers an acoustic echo
    /// canceller for keeper's capture (`AcousticEchoCanceler.isAvailable()`).
    /// With one, the microphone stays open while keeper speaks and the
    /// person can talk over an answer; without one, [`super::may_record`]
    /// answers `false` while speaking and the turn releases the microphone
    /// before it speaks. [`ANDROID_LIMITS`] says both.
    #[must_use]
    pub fn android(class: DeviceClass, echo_cancelled: bool) -> Self {
        Self {
            noun: match class {
                DeviceClass::Tablet => "tablet",
                DeviceClass::Mobile | DeviceClass::Desktop => "phone",
            },
            full_duplex: echo_cancelled,
            ..Self::ANDROID
        }
    }

    /// The platform of a port that has none — the shell's absent port on a
    /// target with no voice at all. Its one sentence,
    /// [`super::VoiceUnavailable::Unsupported`], names no device, so these
    /// words are never shown; they exist so that a port without a platform
    /// is still a port, without an `Option` in every sentence, and they
    /// name no OS because the targets they cover change as ports arrive.
    /// Half-duplex, because nothing is known to arbitrate.
    pub const ABSENT: Self = Self {
        noun: "device",
        allow: "allow both in the system's privacy settings",
        download: "download that language in the system's dictation settings",
        voice_download: "download a voice for it in the system's spoken-content settings",
        full_duplex: false,
        // Never shown: a port with no platform refuses before a switch is
        // ever drawn. Present so that every platform answers every question.
        limits: "Listening is not available on this device.",
    };

    /// The sentence beside the switch when a port on this platform could not
    /// keep its own voice out of the transcript for the capture it has up —
    /// on a Mac, voice processing refused on `device`, an aggregate input
    /// (Story 22.7: the unit builds its own aggregate and will not sit on
    /// one). Names the device, says what is lost (the word) and what is not
    /// (the button, the hotkey, the tray still stop an answer, AD-212).
    /// Decided once here; the port supplies only the device's name.
    #[must_use]
    pub fn half_duplex_sentence(&self, device: &str) -> String {
        format!(
            "keeper cannot keep its own voice out of {device}, so saying the stop word while this {noun} speaks does nothing; the button, the hotkey and the tray still stop an answer. Choose a plain microphone as the input device to talk over an answer.",
            noun = self.noun
        )
    }
}
