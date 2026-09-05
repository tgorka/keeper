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
