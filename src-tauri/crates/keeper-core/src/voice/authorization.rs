//! When to ask for the microphone and the recogniser (FR-408, AD-171).
//!
//! Two permissions stand between a person and a voice turn on iOS: the
//! recogniser's (`NSSpeechRecognitionUsageDescription`) and the microphone's
//! (`NSMicrophoneUsageDescription`). The OS shows each dialog once and
//! records the answer; keeper's part is deciding **when** to trigger the
//! first one and **what to do** with a recorded refusal. Both decisions are
//! here, tested against a fake, and the shell's port only reads and asks.
//!
//! The rules, each a refusal of an alternative:
//!
//! - **On the first deliberate voice act, never at launch.** [`authorize`] is
//!   called from the microphone control and the wake-phrase switch and from
//!   nowhere else; a permission dialog on a cold start, before the person has
//!   seen what voice is, is the pattern that gets a permission denied.
//! - **The recogniser first, then the microphone.** The recogniser's dialog
//!   is the one that says "speech recognition" in so many words, which is
//!   what the person is deciding about; the microphone's follows as its
//!   obvious consequence. Two dialogs in the other order ask for a
//!   microphone with no stated purpose.
//! - **Never twice.** A permission the OS has recorded — granted or denied —
//!   is never asked for again: the OS would not show the dialog anyway, and a
//!   port that kept asking would spin. A denial is a state the surface
//!   renders with its remedy ([`VoiceUnavailable::NotAuthorized`] names
//!   Settings), not an event to retry.

use super::VoiceUnavailable;

/// What the OS has recorded for one permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Never asked: the dialog has not been shown.
    NotDetermined,
    /// Allowed.
    Granted,
    /// Refused by the person, or restricted by the device. Either way the
    /// remedy is Settings, so the two are one state here.
    Denied,
}

/// Both permissions, as recorded right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consent {
    /// The recogniser (`SFSpeechRecognizer.authorizationStatus`).
    pub speech: Permission,
    /// The microphone (`AVAudioSession.recordPermission`).
    pub microphone: Permission,
}

/// Which dialog to show next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// The recogniser's dialog.
    Speech,
    /// The microphone's dialog.
    Microphone,
}

/// The impure half of asking: the port reads what the OS recorded and shows
/// one dialog at a time. iOS implements it over `requestAuthorization:` and
/// `requestRecordPermission:`; tests implement it over a fake.
pub trait ConsentPort: Send + Sync {
    /// What the OS has recorded, or why this build cannot ask at all.
    fn consent(&self) -> Result<Consent, VoiceUnavailable>;

    /// Show one dialog and wait for the answer. The OS calls the completion
    /// on an arbitrary thread; the port blocks its caller until then, so the
    /// caller must not be the main thread.
    fn ask(&self, ask: Ask) -> Permission;
}

/// The decision: which dialog to show next, `None` when both permissions are
/// granted, or the refusal when either is denied.
///
/// A denial anywhere ends the question — asking for the microphone after the
/// recogniser was refused would show a dialog for a feature that cannot
/// work.
pub fn next_ask(consent: &Consent) -> Result<Option<Ask>, VoiceUnavailable> {
    if consent.speech == Permission::Denied || consent.microphone == Permission::Denied {
        return Err(VoiceUnavailable::NotAuthorized);
    }
    if consent.speech == Permission::NotDetermined {
        return Ok(Some(Ask::Speech));
    }
    if consent.microphone == Permission::NotDetermined {
        return Ok(Some(Ask::Microphone));
    }
    Ok(None)
}

/// The most dialogs one deliberate act may show: one per permission.
const MOST_ASKS: usize = 2;

/// Ask for whatever is still undetermined, in order, and stop at the first
/// refusal. Returns `Ok(())` only when both permissions are granted.
///
/// Bounded at [`MOST_ASKS`] dialogs: a port whose answer is still
/// `NotDetermined` after its own dialog (which the OS never does) is treated
/// as a refusal rather than asked again.
pub fn authorize(port: &dyn ConsentPort) -> Result<(), VoiceUnavailable> {
    for _ in 0..MOST_ASKS {
        let consent = port.consent()?;
        match next_ask(&consent)? {
            None => return Ok(()),
            Some(ask) => {
                if port.ask(ask) != Permission::Granted {
                    return Err(VoiceUnavailable::NotAuthorized);
                }
            }
        }
    }
    match next_ask(&port.consent()?)? {
        None => Ok(()),
        Some(_) => Err(VoiceUnavailable::NotAuthorized),
    }
}
