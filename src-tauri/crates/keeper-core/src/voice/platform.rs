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
//! ([`VoicePlatform::full_duplex`]). iOS does, through the audio session's
//! voice processing, which is what makes barge-in possible there; macOS has
//! no `AVAudioSession`, so nothing does, and [`super::may_record`] closes
//! the microphone for the duration of every utterance.

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
    /// Whether the OS keeps the port's own voice out of the transcript, so
    /// the microphone may stay open while an utterance is read aloud. Where
    /// it is `false`, [`super::may_record`] answers `false` while speaking
    /// and the turn releases the microphone before it speaks (AD-175).
    pub full_duplex: bool,
}

impl VoicePlatform {
    /// iOS: the phone. Grants live under the app's own Settings page; the
    /// on-device model is a dictation language download; and the audio
    /// session's voice processing lets recognition run while the answer
    /// plays.
    pub const IOS: Self = Self {
        noun: "phone",
        allow: "allow both under Settings > keeper",
        download:
            "download that language under Settings > General > Keyboard > Dictation Languages",
        full_duplex: true,
    };

    /// macOS: the Mac. The microphone grant lives in System Settings >
    /// Privacy & Security > Microphone; on-device recognition depends on
    /// Dictation being on and its language downloaded, under System
    /// Settings > Keyboard > Dictation; and with no `AVAudioSession` there
    /// is nothing to keep keeper's answer out of its own microphone, so the
    /// port never records while it speaks (AD-175).
    pub const MACOS: Self = Self {
        noun: "Mac",
        allow: "allow the microphone under System Settings > Privacy & Security > Microphone",
        download: "turn Dictation on and download that language under System Settings > Keyboard > Dictation",
        full_duplex: false,
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
        full_duplex: false,
    };
}
