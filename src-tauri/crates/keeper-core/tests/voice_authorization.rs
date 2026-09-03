//! Story 62.6 (FR-408): the microphone and the recogniser are asked for by
//! name, once, in order, and a refusal is a state — pinned against a fake
//! consent port, the only host this decision is ever tested on.

use std::sync::Mutex;

use keeper_core::voice::{
    authorize, next_ask, Ask, Consent, ConsentPort, Permission, VoiceUnavailable,
};

/// A port whose recorded state is a script: `answers` is what each dialog
/// returns, and every dialog shown is recorded so the tests can count them.
struct FakeConsent {
    consent: Mutex<Consent>,
    answers: Mutex<Vec<Permission>>,
    asked: Mutex<Vec<Ask>>,
    unavailable: Option<VoiceUnavailable>,
}

impl FakeConsent {
    fn new(speech: Permission, microphone: Permission, answers: &[Permission]) -> Self {
        Self {
            consent: Mutex::new(Consent { speech, microphone }),
            answers: Mutex::new(answers.to_vec()),
            asked: Mutex::new(Vec::new()),
            unavailable: None,
        }
    }
    fn asked(&self) -> Vec<Ask> {
        self.asked.lock().expect("fake lock").clone()
    }
}

impl ConsentPort for FakeConsent {
    fn consent(&self) -> Result<Consent, VoiceUnavailable> {
        match &self.unavailable {
            Some(why) => Err(why.clone()),
            None => Ok(*self.consent.lock().expect("fake lock")),
        }
    }
    fn ask(&self, ask: Ask) -> Permission {
        self.asked.lock().expect("fake lock").push(ask);
        let mut answers = self.answers.lock().expect("fake lock");
        let answer = if answers.is_empty() {
            Permission::NotDetermined
        } else {
            answers.remove(0)
        };
        let mut consent = self.consent.lock().expect("fake lock");
        match ask {
            Ask::Speech => consent.speech = answer,
            Ask::Microphone => consent.microphone = answer,
        }
        answer
    }
}

fn both(speech: Permission, microphone: Permission) -> Consent {
    Consent { speech, microphone }
}

#[test]
fn voice_consent_granted_asks_nothing() {
    assert_eq!(
        next_ask(&both(Permission::Granted, Permission::Granted)),
        Ok(None)
    );
}

#[test]
fn voice_consent_asks_the_recogniser_before_the_microphone() {
    assert_eq!(
        next_ask(&both(Permission::NotDetermined, Permission::NotDetermined)),
        Ok(Some(Ask::Speech))
    );
    assert_eq!(
        next_ask(&both(Permission::Granted, Permission::NotDetermined)),
        Ok(Some(Ask::Microphone))
    );
    assert_eq!(
        next_ask(&both(Permission::NotDetermined, Permission::Granted)),
        Ok(Some(Ask::Speech))
    );
}

#[test]
fn voice_consent_denial_anywhere_is_a_refusal_not_a_question() {
    for consent in [
        both(Permission::Denied, Permission::NotDetermined),
        both(Permission::NotDetermined, Permission::Denied),
        both(Permission::Denied, Permission::Granted),
        both(Permission::Granted, Permission::Denied),
        both(Permission::Denied, Permission::Denied),
    ] {
        assert_eq!(
            next_ask(&consent),
            Err(VoiceUnavailable::NotAuthorized),
            "{consent:?}"
        );
    }
}

#[test]
fn voice_authorize_on_a_fresh_phone_shows_both_dialogs_once_in_order() {
    let port = FakeConsent::new(
        Permission::NotDetermined,
        Permission::NotDetermined,
        &[Permission::Granted, Permission::Granted],
    );
    assert_eq!(authorize(&port), Ok(()));
    assert_eq!(port.asked(), vec![Ask::Speech, Ask::Microphone]);
    // A second deliberate act asks nothing: the OS recorded both.
    assert_eq!(authorize(&port), Ok(()));
    assert_eq!(port.asked(), vec![Ask::Speech, Ask::Microphone]);
}

#[test]
fn voice_authorize_stops_at_the_first_refusal() {
    let port = FakeConsent::new(
        Permission::NotDetermined,
        Permission::NotDetermined,
        &[Permission::Denied],
    );
    assert_eq!(authorize(&port), Err(VoiceUnavailable::NotAuthorized));
    assert_eq!(port.asked(), vec![Ask::Speech]);
    // And never asks again: the refusal is recorded, the remedy is Settings.
    assert_eq!(authorize(&port), Err(VoiceUnavailable::NotAuthorized));
    assert_eq!(port.asked(), vec![Ask::Speech]);
}

#[test]
fn voice_authorize_asks_only_for_what_is_still_undetermined() {
    let port = FakeConsent::new(
        Permission::Granted,
        Permission::NotDetermined,
        &[Permission::Granted],
    );
    assert_eq!(authorize(&port), Ok(()));
    assert_eq!(port.asked(), vec![Ask::Microphone]);
}

#[test]
fn voice_authorize_never_spins_on_a_port_that_answers_nothing() {
    let port = FakeConsent::new(Permission::NotDetermined, Permission::NotDetermined, &[]);
    assert_eq!(authorize(&port), Err(VoiceUnavailable::NotAuthorized));
    assert_eq!(port.asked(), vec![Ask::Speech]);
}

#[test]
fn voice_authorize_passes_an_absent_port_through() {
    let mut port = FakeConsent::new(Permission::Granted, Permission::Granted, &[]);
    port.unavailable = Some(VoiceUnavailable::Unsupported);
    assert_eq!(authorize(&port), Err(VoiceUnavailable::Unsupported));
    assert!(port.asked().is_empty());
}

#[test]
fn voice_refusal_names_its_remedy() {
    let message = VoiceUnavailable::NotAuthorized.to_string();
    assert!(message.contains("Settings"), "{message}");
    assert!(message.contains("microphone"), "{message}");
    assert!(message.contains("speech recognition"), "{message}");
}
