//! Story 62.4: the voice turn is a state machine in `keeper-core`, the
//! microphone is a port, and every rule is pinned here against a fake port
//! — the only host this code is ever tested on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use keeper_core::vm::{VoiceStateVm, VoiceUnavailableVm};
use keeper_core::voice::{
    advance, perform, should_rearm, silence_budget, Effect, PhraseRefused, Turn, TurnEvent,
    TurnState, VoicePlatform, VoicePort, VoiceUnavailable, WakePhrase, DEFAULT_STOP_PHRASE,
    DEFAULT_WAKE_PHRASE, END_OF_UTTERANCE_PAUSE, NOTHING_HEARD_TIMEOUT,
};

// ---------------------------------------------------------------------------
// The fake port: records every call, refuses on demand.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start(Option<String>),
    Stop,
    /// The text and the language it was asked to be spoken in.
    Speak(String, String),
    StopSpeaking,
}

struct FakePort {
    calls: Mutex<Vec<Call>>,
    refuse_start: Option<VoiceUnavailable>,
    /// Set when `refuse_start`'s refusal has cleared — the permission was
    /// granted, the language changed — so the same port that refused now
    /// answers (Epic 65, AD-190).
    granted: AtomicBool,
    refuse_speak: Option<VoiceUnavailable>,
    /// Half-duplex when set, so the same fake stands in for either platform.
    half_duplex: bool,
    /// The languages this fake has voices for (Epic 64): an English one by
    /// default, so a turn that speaks has something to speak with.
    voices: Vec<String>,
    /// The locale the fake listens in.
    listening: String,
    /// What the fake's detector answers for any text.
    detected: Option<String>,
}

impl Default for FakePort {
    fn default() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            refuse_start: None,
            granted: AtomicBool::new(false),
            refuse_speak: None,
            half_duplex: false,
            voices: vec!["en-US".to_owned()],
            listening: "en-US".to_owned(),
            detected: None,
        }
    }
}

impl FakePort {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("fake port lock").clone()
    }
    fn record(&self, call: Call) {
        self.calls.lock().expect("fake port lock").push(call);
    }
    fn clear_refusal(&self) {
        self.granted.store(true, Ordering::SeqCst);
    }
    /// The start refusal in force: `refuse_start` until it clears.
    fn refusal(&self) -> Option<VoiceUnavailable> {
        if self.granted.load(Ordering::SeqCst) {
            None
        } else {
            self.refuse_start.clone()
        }
    }
}

impl VoicePort for FakePort {
    fn platform(&self) -> VoicePlatform {
        if self.half_duplex {
            VoicePlatform::MACOS
        } else {
            VoicePlatform::IOS
        }
    }
    fn availability(&self) -> Result<(), VoiceUnavailable> {
        self.refusal().map_or(Ok(()), Err)
    }
    fn locales(&self) -> keeper_core::voice::locale::DeviceLocales {
        keeper_core::voice::locale::DeviceLocales::default()
    }
    fn set_locale(&self, _requested: Option<String>) {}
    fn start_listening(&self, wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        self.record(Call::Start(wake.map(|w| w.as_str().to_owned())));
        self.refusal().map_or(Ok(()), Err)
    }
    fn stop_listening(&self) {
        self.record(Call::Stop);
    }
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }
    fn listening(&self) -> String {
        self.listening.clone()
    }
    fn detect_language(&self, _text: &str, _constraints: &[String]) -> Option<String> {
        self.detected.clone()
    }
    fn speak(&self, text: &str, language: &str) -> Result<(), VoiceUnavailable> {
        self.record(Call::Speak(text.to_owned(), language.to_owned()));
        self.refuse_speak.clone().map_or(Ok(()), Err)
    }
    fn stop_speaking(&self) {
        self.record(Call::StopSpeaking);
    }
}

fn listening(heard: &str) -> TurnState {
    TurnState::Listening {
        heard: heard.to_owned(),
    }
}

fn heard(text: &str) -> TurnState {
    TurnState::Heard {
        text: text.to_owned(),
    }
}

fn failed(reason: &str) -> TurnState {
    TurnState::Failed {
        reason: reason.to_owned(),
    }
}

/// Every state the machine has, one of each.
fn every_state() -> Vec<TurnState> {
    vec![
        TurnState::Idle,
        listening(""),
        listening("hej"),
        heard("hello"),
        TurnState::Sending { answering: false },
        TurnState::Sending { answering: true },
        TurnState::Speaking,
        failed("boom"),
    ]
}

// ---------------------------------------------------------------------------
// The happy path, then the rules by name.
// ---------------------------------------------------------------------------

/// `Idle → Listening → Heard → Sending → Speaking → Idle`, with the device
/// opened once at the start and released once at the end.
#[test]
fn voice_turn_walks_the_whole_path() {
    let (s, e) = advance(TurnState::Idle, TurnEvent::WakeMatched);
    assert_eq!(s, listening(""));
    assert_eq!(e, vec![Effect::OpenMicrophone]);

    let (s, e) = advance(s, TurnEvent::PartialHeard("what ti".to_owned()));
    assert_eq!(s, listening("what ti"));
    assert!(e.is_empty());

    let (s, e) = advance(s, TurnEvent::FinalHeard("what time is it".to_owned()));
    assert_eq!(s, heard("what time is it"));
    assert_eq!(e, vec![Effect::SendText("what time is it".to_owned())]);

    let (s, e) = advance(s, TurnEvent::Sent);
    assert_eq!(s, TurnState::Sending { answering: false });
    assert!(e.is_empty());

    let (s, e) = advance(s, TurnEvent::AnswerChunk);
    assert_eq!(s, TurnState::Sending { answering: true });
    assert!(e.is_empty());

    let (s, e) = advance(s, TurnEvent::AnswerDone("It is noon.".to_owned()));
    assert_eq!(s, TurnState::Speaking);
    assert_eq!(e, vec![Effect::Speak("It is noon.".to_owned())]);

    let (s, e) = advance(s, TurnEvent::Silence);
    assert_eq!(s, TurnState::Idle);
    assert_eq!(e, vec![Effect::ReleaseMicrophone]);
}

/// NFR-51: an abandon edge from EVERY state releases the microphone and lands
/// in `Idle`.
#[test]
fn voice_abandon_from_every_state_releases_the_microphone() {
    for state in every_state() {
        let label = format!("{state:?}");
        let (next, effects) = advance(state, TurnEvent::Abandoned);
        assert_eq!(next, TurnState::Idle, "abandon from {label}");
        assert!(
            effects.contains(&Effect::ReleaseMicrophone),
            "abandon from {label} must release the microphone, got {effects:?}"
        );
        assert!(
            !effects.contains(&Effect::OpenMicrophone),
            "abandon from {label} must not reopen the microphone"
        );
    }
}

/// A `Failed` turn releases the device too, from every state.
#[test]
fn voice_failure_from_every_state_releases_the_microphone() {
    for state in every_state() {
        let label = format!("{state:?}");
        let (next, effects) = advance(state, TurnEvent::Failed("mic gone".to_owned()));
        assert_eq!(next, failed("mic gone"), "failure from {label}");
        assert!(
            effects.contains(&Effect::ReleaseMicrophone),
            "failure from {label} must release the microphone, got {effects:?}"
        );
    }
}

/// A bounded silence timeout ends a `Listening` turn instead of recording
/// forever: with nothing heard the turn ends and the device is released.
#[test]
fn voice_silence_with_nothing_heard_ends_the_turn() {
    let (next, effects) = advance(listening(""), TurnEvent::Silence);
    assert_eq!(next, TurnState::Idle);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
}

/// The same timeout after words were heard is the end of the utterance: what
/// was heard becomes the message.
#[test]
fn voice_silence_after_words_sends_what_was_heard() {
    let (next, effects) = advance(listening("open the pod bay doors"), TurnEvent::Silence);
    assert_eq!(next, heard("open the pod bay doors"));
    assert_eq!(
        effects,
        vec![Effect::SendText("open the pod bay doors".to_owned())]
    );
}

/// The silence budget is bounded in both `Listening` shapes and absent
/// everywhere else — `Speaking` ends when the synthesiser says so, not when a
/// clock does.
#[test]
fn voice_silence_budget_is_bounded_and_only_while_listening() {
    assert_eq!(silence_budget(&listening("")), Some(NOTHING_HEARD_TIMEOUT));
    assert_eq!(
        silence_budget(&listening("   ")),
        Some(NOTHING_HEARD_TIMEOUT)
    );
    assert_eq!(
        silence_budget(&listening("hej")),
        Some(END_OF_UTTERANCE_PAUSE)
    );
    assert!(END_OF_UTTERANCE_PAUSE < NOTHING_HEARD_TIMEOUT);
    for state in [
        TurnState::Idle,
        heard("x"),
        TurnState::Sending { answering: false },
        TurnState::Speaking,
        failed("x"),
    ] {
        assert_eq!(silence_budget(&state), None, "{state:?}");
    }
}

/// Barge-in: `SpeechDetected` while `Speaking` emits `StopSpeaking` before
/// anything else, and the turn is listening again.
#[test]
fn voice_barge_in_stops_speaking_before_anything_else() {
    let (next, effects) = advance(TurnState::Speaking, TurnEvent::SpeechDetected("hey".into()));
    assert_eq!(next, listening(""));
    assert_eq!(effects.first(), Some(&Effect::StopSpeaking));
    assert!(
        !effects.contains(&Effect::ReleaseMicrophone),
        "barge-in keeps the microphone: {effects:?}"
    );
}

/// `SpeechDetected` and `StopHeard` anywhere but `Speaking` are nothing —
/// there is nothing to interrupt.
#[test]
fn voice_speech_detected_outside_speaking_is_ignored() {
    for state in [
        TurnState::Idle,
        listening("a"),
        heard("a"),
        TurnState::Sending { answering: true },
    ] {
        for event in [
            TurnEvent::SpeechDetected("stop".to_owned()),
            TurnEvent::StopHeard,
        ] {
            let before = state.clone();
            let (next, effects) = advance(state.clone(), event);
            assert_eq!(next, before);
            assert!(effects.is_empty());
        }
    }
}

/// The stop phrase mid-answer (AD-208): the synthesiser is stopped first,
/// the device is released, and no question follows.
#[test]
fn voice_stop_heard_ends_the_turn() {
    let (next, effects) = advance(TurnState::Speaking, TurnEvent::StopHeard);
    assert_eq!(next, TurnState::Idle);
    assert_eq!(
        effects,
        vec![Effect::StopSpeaking, Effect::ReleaseMicrophone]
    );
}

/// `Turn` makes `StopHeard` out of a barge-in whose words contain the stop
/// phrase, and re-arms the wake phrase the way every other end does; a
/// barge-in with other words keeps FR-403's shape and listens again.
#[test]
fn voice_turn_matches_the_stop_phrase_only_while_speaking() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_wake(Some(phrase("hej keeper")));
    turn.set_stop(Some(stop_phrase(DEFAULT_STOP_PHRASE)));
    assert_eq!(turn.stop_phrase().map(WakePhrase::as_str), Some("stop"));

    // "stop" while idle is noise like any other word: nothing happens.
    assert!(turn
        .drive(TurnEvent::PartialHeard("stop".to_owned()), &port)
        .is_empty());
    assert_eq!(turn.state(), &TurnState::Idle);

    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hi".to_owned()), &port);
    turn.drive(TurnEvent::AnswerDone("a long answer".to_owned()), &port);
    // Another word mid-answer: today's barge-in.
    let effects = turn.drive(TurnEvent::SpeechDetected("wait, what".to_owned()), &port);
    assert_eq!(effects, vec![Effect::StopSpeaking, Effect::OpenMicrophone]);
    assert_eq!(turn.state(), &listening(""));

    turn.drive(TurnEvent::FinalHeard("again".to_owned()), &port);
    turn.drive(TurnEvent::AnswerDone("another answer".to_owned()), &port);
    // The stop word, as the recogniser writes it: capitalised, punctuated.
    let effects = turn.drive(TurnEvent::SpeechDetected("Stop.".to_owned()), &port);
    assert_eq!(
        effects,
        vec![
            Effect::StopSpeaking,
            Effect::ReleaseMicrophone,
            Effect::OpenMicrophone
        ],
        "stopped, released for the turn, re-armed for the phrase"
    );
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(turn.armed());
    assert_eq!(
        port.calls().last(),
        Some(&Call::Start(Some("hej keeper".to_owned())))
    );
}

/// With no stop phrase set, every barge-in asks a question — the shape
/// before Epic 67.
#[test]
fn voice_turn_without_a_stop_phrase_treats_stop_as_a_question() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.apply(TurnEvent::AnswerDone("noon".to_owned()));
    let effects = turn.apply(TurnEvent::SpeechDetected("stop".to_owned()));
    assert_eq!(effects, vec![Effect::StopSpeaking, Effect::OpenMicrophone]);
    assert_eq!(turn.state(), &listening(""));
}

/// A Polish stop word, set from `bots.stop_phrase`, matches what the
/// recogniser writes with its diacritics and its full stop.
#[test]
fn voice_turn_matches_a_polish_stop_phrase() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_stop(Some(stop_phrase("przestań")));
    turn.apply(TurnEvent::AnswerDone("długa odpowiedź".to_owned()));
    let effects = turn.apply(TurnEvent::SpeechDetected("Przestań.".to_owned()));
    assert_eq!(
        effects,
        vec![Effect::StopSpeaking, Effect::ReleaseMicrophone]
    );
    assert_eq!(turn.state(), &TurnState::Idle);
}

/// A `FinalHeard("")` does not send an empty message: the turn ends and the
/// device is released. Whitespace is empty too.
#[test]
fn voice_empty_final_transcript_does_not_send() {
    for blank in ["", "   ", "\n\t"] {
        let (next, effects) = advance(listening("x"), TurnEvent::FinalHeard(blank.to_owned()));
        assert_eq!(next, TurnState::Idle, "final {blank:?}");
        assert_eq!(effects, vec![Effect::ReleaseMicrophone], "final {blank:?}");
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendText(_))),
            "final {blank:?} must not send"
        );
    }
}

/// A `PartialHeard` never sends — it only updates what the surface shows.
#[test]
fn voice_partial_transcript_never_sends() {
    let (next, effects) = advance(
        listening(""),
        TurnEvent::PartialHeard("hello there".to_owned()),
    );
    assert_eq!(next, listening("hello there"));
    assert!(effects.is_empty(), "a partial sent something: {effects:?}");
}

/// A final transcript is trimmed before it is sent, so a recogniser's
/// trailing space does not become part of the message.
#[test]
fn voice_final_transcript_is_trimmed_before_sending() {
    let (next, effects) = advance(
        listening(""),
        TurnEvent::FinalHeard("  hello \n".to_owned()),
    );
    assert_eq!(next, heard("hello"));
    assert_eq!(effects, vec![Effect::SendText("hello".to_owned())]);
}

/// An empty answer is not spoken: the turn ends instead of reading silence.
#[test]
fn voice_empty_answer_is_not_spoken() {
    for state in [
        heard("q"),
        TurnState::Sending { answering: false },
        TurnState::Idle,
    ] {
        let (next, effects) = advance(state, TurnEvent::AnswerDone("  ".to_owned()));
        assert_eq!(next, TurnState::Idle);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Speak(_))));
    }
}

/// An answer may be read aloud without a listening turn (the person typed):
/// the device opens for barge-in before the speech starts.
#[test]
fn voice_answer_from_idle_opens_the_microphone_then_speaks() {
    let (next, effects) = advance(TurnState::Idle, TurnEvent::AnswerDone("hi".to_owned()));
    assert_eq!(next, TurnState::Speaking);
    assert_eq!(
        effects,
        vec![Effect::OpenMicrophone, Effect::Speak("hi".to_owned())]
    );
}

/// `Speaking` ends on the synthesiser's `Silence`, releasing the device.
#[test]
fn voice_speaking_ends_on_silence() {
    let (next, effects) = advance(TurnState::Speaking, TurnEvent::Silence);
    assert_eq!(next, TurnState::Idle);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
}

/// Abandon while speaking stops the speech and releases the device, in that
/// order.
#[test]
fn voice_abandon_while_speaking_stops_speech_then_releases() {
    let (_, effects) = advance(TurnState::Speaking, TurnEvent::Abandoned);
    assert_eq!(
        effects,
        vec![Effect::StopSpeaking, Effect::ReleaseMicrophone]
    );
}

/// A failed turn can be started again by the trigger.
#[test]
fn voice_failed_turn_restarts_on_trigger() {
    let (next, effects) = advance(failed("x"), TurnEvent::WakeMatched);
    assert_eq!(next, listening(""));
    assert_eq!(effects, vec![Effect::OpenMicrophone]);
}

// ---------------------------------------------------------------------------
// The driver: phrase matching while idle, re-arming, and the port.
// ---------------------------------------------------------------------------

fn phrase(raw: &str) -> WakePhrase {
    WakePhrase::parse(raw).expect("test phrase parses")
}

fn stop_phrase(raw: &str) -> WakePhrase {
    WakePhrase::parse_stop(raw).expect("test stop phrase parses")
}

/// A transcript that contains the phrase while idle starts a turn; the
/// microphone is (re)opened on a fresh request with the phrase as the hint.
#[test]
fn voice_driver_matches_the_phrase_while_idle() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    let armed = turn.set_wake(Some(phrase("hej keeper")));
    assert_eq!(armed, vec![Effect::OpenMicrophone]);
    perform(&armed, &port, turn.wake()).expect("arming succeeds");

    let effects = turn.drive(TurnEvent::PartialHeard("no keeper here".to_owned()), &port);
    assert!(effects.is_empty(), "noise while idle: {effects:?}");
    assert_eq!(turn.state(), &TurnState::Idle);

    let effects = turn.drive(TurnEvent::PartialHeard("Hej, Keeper!".to_owned()), &port);
    assert_eq!(effects, vec![Effect::OpenMicrophone]);
    assert_eq!(turn.state(), &listening(""));
    assert_eq!(
        port.calls(),
        vec![
            Call::Start(Some("hej keeper".to_owned())),
            Call::Start(Some("hej keeper".to_owned())),
        ]
    );
}

/// With no phrase set, a transcript while idle is noise.
#[test]
fn voice_driver_ignores_transcripts_while_idle_without_a_phrase() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    let effects = turn.drive(TurnEvent::FinalHeard("hej keeper".to_owned()), &port);
    assert!(effects.is_empty());
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(port.calls().is_empty());
}

/// When a turn ends on its own and a phrase is set, the microphone is
/// released and opened again for the phrase; the surface sees it listening.
#[test]
fn voice_driver_rearms_the_phrase_after_a_turn_ends_on_its_own() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_wake(Some(phrase("hej keeper")));
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::Silence, &port);
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(turn.microphone_open());
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Idle {
            wake: Some("hej keeper".to_owned()),
            listening_for_wake: true,
        }
    );
    assert_eq!(
        port.calls().last(),
        Some(&Call::Start(Some("hej keeper".to_owned())))
    );
}

/// NFR-51 at the driver, with a standing switch (Story 62.5): a turn the
/// person stopped releases the turn's microphone — the port sees `Stop` —
/// and then, because the phrase is still set, opens it again for the phrase.
/// A stop ends this turn; the switch is what ends listening.
#[test]
fn voice_driver_rearms_the_phrase_after_abandon() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_wake(Some(phrase("hej keeper")));
    turn.drive(TurnEvent::WakeMatched, &port);
    let effects = turn.drive(TurnEvent::Abandoned, &port);
    assert_eq!(
        effects,
        vec![Effect::ReleaseMicrophone, Effect::OpenMicrophone]
    );
    assert!(turn.microphone_open());
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Idle {
            wake: Some("hej keeper".to_owned()),
            listening_for_wake: true,
        }
    );
    let calls = port.calls();
    assert_eq!(
        &calls[calls.len() - 2..],
        &[Call::Stop, Call::Start(Some("hej keeper".to_owned()))]
    );
}

/// A stop with no phrase set stays stopped: nothing to re-arm for.
#[test]
fn voice_driver_abandon_without_a_phrase_stays_released() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    let effects = turn.drive(TurnEvent::Abandoned, &port);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    assert!(!turn.microphone_open());
    assert_eq!(port.calls().last(), Some(&Call::Stop));
}

/// A failed turn does not re-arm even with a phrase set: the port that
/// refused would refuse again, and the surface says the phrase is set but
/// not listened for.
#[test]
fn voice_driver_does_not_rearm_after_failure_because_the_port_would_refuse_again() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_wake(Some(phrase("hej keeper")));
    turn.drive(TurnEvent::WakeMatched, &port);
    let effects = turn.drive(TurnEvent::Failed("the device refused".to_owned()), &port);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    assert!(!turn.microphone_open());
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Failed {
            reason: "the device refused".to_owned()
        }
    );
    assert_eq!(port.calls().last(), Some(&Call::Stop));
}

/// Clearing the phrase while idle releases the device.
#[test]
fn voice_driver_clearing_the_phrase_releases_the_microphone() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.set_wake(Some(phrase("hej keeper")));
    let effects = turn.set_wake(None);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    perform(&effects, &port, turn.wake()).expect("release is infallible");
    assert!(!turn.microphone_open());
    assert_eq!(turn.wake(), None);
}

/// Setting the phrase mid-turn changes nothing about the device now.
#[test]
fn voice_driver_setting_the_phrase_mid_turn_touches_nothing() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.apply(TurnEvent::WakeMatched);
    assert!(turn.set_wake(Some(phrase("hej keeper"))).is_empty());
    assert_eq!(turn.state(), &listening(""));
}

/// A port that refuses to open fails the turn, and the failure releases what
/// the port may have half-opened — the device is never left in doubt.
#[test]
fn voice_driver_fails_the_turn_when_the_port_refuses_to_listen() {
    let port = FakePort {
        refuse_start: Some(VoiceUnavailable::NoOnDeviceModel {
            locale: "pl_PL".to_owned(),
        }),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    let effects = turn.drive(TurnEvent::WakeMatched, &port);
    assert_eq!(
        effects,
        vec![Effect::OpenMicrophone, Effect::ReleaseMicrophone]
    );
    match turn.state() {
        TurnState::Failed { reason } => {
            assert!(reason.contains("pl_PL"), "names the locale: {reason}");
            assert!(
                reason.contains("never sends your voice to a server"),
                "says why there is no fallback: {reason}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(port.calls(), vec![Call::Start(None), Call::Stop]);
    assert!(!turn.microphone_open());
}

/// A port that refuses to speak fails the turn the same way, and the failure
/// stops the speech it may have started.
#[test]
fn voice_driver_fails_the_turn_when_the_port_refuses_to_speak() {
    let port = FakePort {
        refuse_speak: Some(VoiceUnavailable::Unsupported),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hi".to_owned()), &port);
    turn.drive(TurnEvent::AnswerDone("hello".to_owned()), &port);
    assert!(matches!(turn.state(), TurnState::Failed { .. }));
    assert_eq!(
        port.calls(),
        vec![
            Call::Start(None),
            Call::Speak("hello".to_owned(), "en-US".to_owned()),
            Call::StopSpeaking,
            Call::Stop,
        ]
    );
}

/// The driver's port calls for a whole turn, in order: barge-in stops the
/// synthesiser before restarting recognition.
#[test]
fn voice_driver_orders_port_calls_for_barge_in() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hi".to_owned()), &port);
    turn.drive(TurnEvent::AnswerDone("a long answer".to_owned()), &port);
    turn.drive(TurnEvent::SpeechDetected("hang on".to_owned()), &port);
    assert_eq!(
        port.calls(),
        vec![
            Call::Start(None),
            Call::Speak("a long answer".to_owned(), "en-US".to_owned()),
            Call::StopSpeaking,
            Call::Start(None),
        ]
    );
    assert_eq!(turn.state(), &listening(""));
}

/// Every machine state has a projection, and `Idle` carries what the surface
/// must say about the phrase.
#[test]
fn voice_state_projects_to_its_view_model() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Idle {
            wake: None,
            listening_for_wake: false
        }
    );
    turn.apply(TurnEvent::WakeMatched);
    turn.apply(TurnEvent::PartialHeard("he".to_owned()));
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Listening {
            heard: "he".to_owned(),
            level: None,
        }
    );
    turn.apply(TurnEvent::FinalHeard("hello".to_owned()));
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Heard {
            text: "hello".to_owned(),
            level: None,
        }
    );
    turn.apply(TurnEvent::Sent);
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: false });
    turn.apply(TurnEvent::AnswerChunk);
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: true });
    turn.apply(TurnEvent::AnswerDone("x".to_owned()));
    assert_eq!(turn.vm(), VoiceStateVm::Speaking);
    turn.apply(TurnEvent::Failed("boom".to_owned()));
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Failed {
            reason: "boom".to_owned()
        }
    );
}

/// The refusal reasons cross the wire with their sentence and, for the
/// missing model, the locale.
#[test]
fn voice_unavailable_projects_its_sentence_and_locale() {
    let vm = VoiceUnavailable::NoOnDeviceModel {
        locale: "pl_PL".to_owned(),
    }
    .vm(&VoicePlatform::IOS);
    match vm {
        VoiceUnavailableVm::NoOnDeviceModel { locale, message } => {
            assert_eq!(locale, "pl_PL");
            assert!(message.contains("pl_PL"));
            assert!(message.contains("download"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        VoiceUnavailable::NotAuthorized.vm(&VoicePlatform::IOS),
        VoiceUnavailableVm::NotAuthorized { .. }
    ));
    let sink: keeper_core::voice::EventSink = Arc::new(|_| {});
    sink(TurnEvent::Silence);
}

/// A build linked without `Speech.framework` is its own kind of refusal,
/// worded as a defect of the build. It is neither `unsupported` (the
/// desktop's honest "no port here", which the surface hides) nor a missing
/// model (a language download), because a voice feature that is inert for a
/// build reason once reported exactly the desktop's sentence and nobody
/// could see the difference.
#[test]
fn voice_missing_recognizer_is_named_as_a_build_defect() {
    let why = VoiceUnavailable::NoRecognizer;
    let message = why.message(&VoicePlatform::IOS);
    assert!(message.contains("Speech"), "{message}");
    assert!(message.contains("build"), "{message}");
    assert!(!message.contains("Settings"), "{message}");
    assert!(!message.contains("download that language"), "{message}");
    assert_ne!(
        message,
        VoiceUnavailable::Unsupported.message(&VoicePlatform::IOS)
    );
    match why.vm(&VoicePlatform::IOS) {
        VoiceUnavailableVm::NoRecognizer { message: carried } => assert_eq!(carried, message),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The phrase.
// ---------------------------------------------------------------------------

/// Case and diacritics do not matter: "hej keeper" and "Hej Kééper" are the
/// same phrase, and "Hej, Kééper!" heard mid-sentence matches it.
#[test]
fn voice_phrase_is_case_and_diacritic_insensitive() {
    let a = phrase("hej keeper");
    let b = phrase("Hej Kééper");
    assert_eq!(a, b);
    assert_eq!(a.as_str(), "hej keeper");
    assert!(a.matches("Hej Kééper"));
    assert!(a.matches("HEJ KEEPER"));
    assert!(b.matches("hej keeper"));
}

/// Polish letters fold to what a person without the key would type — "ł" has
/// no NFD decomposition and is the one the table must carry by hand.
#[test]
fn voice_phrase_folds_polish_letters() {
    let p = phrase("słuchaj keeper");
    assert_eq!(p.as_str(), "sluchaj keeper");
    assert!(p.matches("Słuchaj, Keeper"));
    assert!(p.matches("sluchaj keeper"));
    assert_eq!(phrase("Zażółć gęślą").as_str(), "zazolc gesla");
}

/// A phrase occurring mid-sentence still matches.
#[test]
fn voice_phrase_matches_mid_sentence() {
    let p = phrase("hej keeper");
    assert!(p.matches("okay so, hej keeper, what time is it"));
    assert!(p.matches("hej keeper"));
    assert!(p.matches("…hej keeper"));
}

/// A substring of a longer word does not match: whole words only.
#[test]
fn voice_phrase_does_not_match_inside_a_longer_word() {
    let p = phrase("hej keeper");
    assert!(!p.matches("hej keepers"));
    assert!(!p.matches("ahej keeper"));
    assert!(!p.matches("hejkeeper"));
    assert!(!p.matches("hej keep"));
    assert!(!p.matches(""));
}

/// Whitespace and punctuation collapse: the typed phrase and the heard
/// transcript agree however either was spaced.
#[test]
fn voice_phrase_collapses_whitespace_and_punctuation() {
    let p = phrase("  hej \t\n keeper  ");
    assert_eq!(p.as_str(), "hej keeper");
    assert!(p.matches("hej—keeper"));
    assert!(p.matches("hej.  keeper?"));
    assert_eq!(p.words().collect::<Vec<_>>(), vec!["hej", "keeper"]);
}

/// Refusals, by name, each with a reason a person can act on.
#[test]
fn voice_phrase_refuses_an_empty_phrase() {
    for raw in ["", "   ", "…!?", "\n"] {
        assert_eq!(WakePhrase::parse(raw), Err(PhraseRefused::Empty), "{raw:?}");
    }
    let reason = PhraseRefused::Empty.to_string();
    assert!(reason.contains("type a phrase"), "{reason}");
}

/// One word is allowed on purpose: the default phrase is one word.
#[test]
fn voice_phrase_accepts_a_single_word() {
    let p = phrase("Nixie");
    assert_eq!(p.as_str(), "nixie");
    assert!(p.matches("nixie, what time is it"));
    assert!(!p.matches("nixies"));
    assert_eq!(phrase(DEFAULT_WAKE_PHRASE).as_str(), DEFAULT_WAKE_PHRASE);
}

#[test]
fn voice_phrase_refuses_too_few_letters() {
    let refused = WakePhrase::parse("ok go").expect_err("four letters is refused");
    assert_eq!(
        refused,
        PhraseRefused::TooShort {
            letters: 4,
            minimum: 5,
            normalised: "ok go".to_owned()
        }
    );
    let reason = refused.to_string();
    assert!(reason.contains("at least 5 letters"), "{reason}");
    assert!(reason.contains("\"ok go\""), "names what it saw: {reason}");
    // The everyday words a phrase must not be.
    for raw in ["go", "ok", "hey", "okay"] {
        assert!(WakePhrase::parse(raw).is_err(), "{raw:?} is everyday talk");
    }
    // Exactly the minimum is accepted.
    assert!(WakePhrase::parse("ok gog").is_ok());
}

/// The stop phrase's rule is shorter (AD-208): "stop" — four letters, which
/// the wake rule refuses — is the default and must parse; two letters do
/// not; matching normalises the way the wake phrase does.
#[test]
fn voice_stop_phrase_parses_short_words_and_matches_normalised() {
    assert!(WakePhrase::parse(DEFAULT_STOP_PHRASE).is_err());
    let stop = stop_phrase(DEFAULT_STOP_PHRASE);
    assert_eq!(stop.as_str(), "stop");
    for heard in ["Stop.", "stop", "STOP,", "okay stop now", "Stop!"] {
        assert!(stop.matches(heard), "{heard:?}");
    }
    for heard in ["stopped", "nonstop", "", "sto"] {
        assert!(!stop.matches(heard), "{heard:?}");
    }
    assert_eq!(stop_phrase("Przestań").as_str(), "przestan");
    let refused = WakePhrase::parse_stop("no").expect_err("two letters is refused");
    assert_eq!(
        refused,
        PhraseRefused::TooShort {
            letters: 2,
            minimum: 3,
            normalised: "no".to_owned()
        }
    );
    assert!(refused.to_string().contains("at least 3 letters"));
    assert_eq!(WakePhrase::parse_stop("  "), Err(PhraseRefused::Empty));
}

#[test]
fn voice_phrase_refuses_too_many_words() {
    let refused =
        WakePhrase::parse("hey there my dear old keeper").expect_err("six words is refused");
    assert_eq!(refused, PhraseRefused::TooManyWords { words: 6 });
    assert!(refused.to_string().contains("5 words or fewer"));
    assert!(WakePhrase::parse("hey there my dear keeper").is_ok());
}

/// Letters are what count: "a b" says "use more letters", not "one word".
#[test]
fn voice_phrase_counts_letters_not_words() {
    assert_eq!(
        WakePhrase::parse("a b"),
        Err(PhraseRefused::TooShort {
            letters: 2,
            minimum: 5,
            normalised: "a b".to_owned()
        })
    );
}

/// The sentence beside the switch (FR-406) states every fact the epic
/// established, and none of them as "not yet". Since Epic 65 (AD-193) the
/// interruptions are named by behaviour — a call ends it until keeper is
/// opened, Siri and a non-mixing app pause it and keeper resumes — and the
/// vaguer "when iOS ends the audio session" is gone.
#[test]
fn voice_limits_sentence_states_every_fact_and_no_to_do() {
    let s = VoicePlatform::IOS.limits;
    assert!(s.contains("in front"), "armed with keeper in front: {s}");
    assert!(
        s.contains("another app is in front") && s.contains("screen is locked"),
        "continues in the background and locked: {s}"
    );
    assert!(
        s.contains("Siri") && s.contains("takes the microphone") && s.contains("pauses it"),
        "Siri and a non-mixing app pause it: {s}"
    );
    assert!(
        s.contains("resumes on its own"),
        "keeper resumes after a pause: {s}"
    );
    assert!(
        s.contains("phone call") && s.contains("until you open keeper again"),
        "a call ends it until the next open: {s}"
    );
    assert!(
        s.contains("turn it off") && s.contains("force-quit"),
        "names the person's own ends: {s}"
    );
    assert!(
        !s.contains("audio session"),
        "the interruptions are named by behaviour, not by the session: {s}"
    );
    assert!(
        s.contains("orange")
            && s.contains("microphone indicator")
            && s.contains("cannot be hidden"),
        "{s}"
    );
    assert!(s.contains("battery"), "{s}");
    for weasel in ["not yet", "for now", "coming", "later"] {
        assert!(
            !s.contains(weasel),
            "a refusal, not a to-do: {weasel:?} in {s}"
        );
    }
}

// ---------------------------------------------------------------------------
// Epic 64, Story 64.2: the voice an answer is spoken in is core's decision.
// ---------------------------------------------------------------------------

/// The case the epic opens with, through the driver: an English listener,
/// a Polish answer, a Mac with a Polish voice — the port is told `pl-PL`.
#[test]
fn voice_driver_speaks_a_detected_language_in_its_own_voice() {
    let port = FakePort {
        voices: vec!["en-US".to_owned(), "pl-PL".to_owned()],
        detected: Some("pl".to_owned()),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(
        TurnEvent::FinalHeard("what's the weather".to_owned()),
        &port,
    );
    turn.drive(TurnEvent::AnswerDone("Pada deszcz.".to_owned()), &port);
    assert_eq!(turn.state(), &TurnState::Speaking);
    assert_eq!(
        port.calls(),
        vec![
            Call::Start(None),
            Call::Speak("Pada deszcz.".to_owned(), "pl-PL".to_owned()),
        ]
    );
}

/// The same answer where no Polish voice exists: the turn fails with the
/// refusal that names Polish and the download page, and the port is never
/// asked to speak — not in English, not at all (AD-27).
#[test]
fn voice_driver_refuses_rather_than_speak_in_the_wrong_voice() {
    let port = FakePort {
        detected: Some("pl".to_owned()),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(
        TurnEvent::FinalHeard("what's the weather".to_owned()),
        &port,
    );
    turn.drive(TurnEvent::AnswerDone("Pada deszcz.".to_owned()), &port);
    match turn.state() {
        TurnState::Failed { reason } => {
            assert!(reason.contains("no voice for Polish (pl)"), "{reason}");
            assert!(reason.contains("Spoken Content"), "{reason}");
        }
        other => panic!("expected a failed turn, got {other:?}"),
    }
    assert!(
        !port
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Speak(..))),
        "nothing was spoken: {:?}",
        port.calls()
    );
}

/// The turn knows when a send belongs to it: after it heard something and
/// until the answer is spoken — the rule the per-turn instruction hangs on.
#[test]
fn voice_turn_awaits_a_send_between_heard_and_speaking() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    assert!(!turn.awaiting_send());
    turn.apply(TurnEvent::WakeMatched);
    assert!(!turn.awaiting_send());
    turn.apply(TurnEvent::FinalHeard("hello".to_owned()));
    assert!(turn.awaiting_send());
    turn.apply(TurnEvent::Sent);
    assert!(turn.awaiting_send());
    turn.apply(TurnEvent::AnswerDone("hi".to_owned()));
    assert!(!turn.awaiting_send());
    turn.apply(TurnEvent::Silence);
    assert!(!turn.awaiting_send());
}

// ---------------------------------------------------------------------------
// Story 64.3: a turn that has a level and a middle (AD-186).
// ---------------------------------------------------------------------------

/// A turn passes through `Sending` between `Heard` and `Speaking`, the
/// shell feeding `Sent` when the request leaves and `AnswerChunk` on the
/// first token — the middle the surface shows as "thinking", then
/// "answering", instead of a gap.
#[test]
fn voice_turn_has_a_middle_between_heard_and_speaking() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::MACOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("what time is it".to_owned()), &port);
    assert_eq!(turn.state(), &heard("what time is it"));

    assert!(turn.drive(TurnEvent::Sent, &port).is_empty());
    assert_eq!(turn.state(), &TurnState::Sending { answering: false });
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: false });

    assert!(turn.drive(TurnEvent::AnswerChunk, &port).is_empty());
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: true });
    // A second chunk changes nothing: the first is the one that matters.
    assert!(turn.drive(TurnEvent::AnswerChunk, &port).is_empty());
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: true });

    turn.drive(TurnEvent::AnswerDone("It is noon.".to_owned()), &port);
    assert_eq!(turn.state(), &TurnState::Speaking);
    assert_eq!(turn.vm(), VoiceStateVm::Speaking);
}

/// `Sent` and `AnswerChunk` fed where they mean nothing — a typed message
/// leaving while no voice turn runs — move nothing and touch no device.
#[test]
fn voice_sent_and_chunk_outside_a_turn_are_ignored() {
    for state in [TurnState::Idle, listening("hej"), TurnState::Speaking] {
        for event in [TurnEvent::Sent, TurnEvent::AnswerChunk] {
            let (next, effects) = advance(state.clone(), event);
            assert_eq!(next, state);
            assert!(effects.is_empty());
        }
    }
}

/// The level rides the snapshot while the device is open for a turn —
/// `Listening` and `Heard` — clamped to `0.0..=1.0`, and is dropped the
/// moment the turn moves on, so no state with the microphone released ever
/// shows one. It is never a transition and never an effect.
#[test]
fn voice_level_rides_listening_and_heard_only() {
    let mut turn = Turn::new(VoicePlatform::IOS);
    assert!(turn.apply(TurnEvent::Level(0.5)).is_empty());
    assert_eq!(turn.level(), None, "idle carries no level");

    turn.apply(TurnEvent::WakeMatched);
    assert_eq!(turn.level(), None, "unmeasured until the first reading");
    assert!(turn.apply(TurnEvent::Level(0.3)).is_empty());
    assert_eq!(turn.state(), &listening(""));
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Listening {
            heard: String::new(),
            level: Some(0.3),
        }
    );
    turn.apply(TurnEvent::Level(1.7));
    assert_eq!(turn.level(), Some(1.0), "clamped above");
    turn.apply(TurnEvent::Level(-0.2));
    assert_eq!(turn.level(), Some(0.0), "clamped below");

    turn.apply(TurnEvent::PartialHeard("what".to_owned()));
    turn.apply(TurnEvent::Level(0.6));
    turn.apply(TurnEvent::FinalHeard("what time".to_owned()));
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Heard {
            text: "what time".to_owned(),
            level: Some(0.6),
        },
        "the last reading stays on `heard`: the device is still open"
    );
    turn.apply(TurnEvent::Level(0.1));
    assert_eq!(turn.level(), Some(0.1));

    turn.apply(TurnEvent::Sent);
    assert_eq!(turn.level(), None, "sending has no microphone to meter");
    assert_eq!(turn.vm(), VoiceStateVm::Sending { answering: false });
    turn.apply(TurnEvent::Level(0.9));
    assert_eq!(turn.level(), None, "a late reading is dropped");

    turn.apply(TurnEvent::AnswerDone("noon".to_owned()));
    turn.apply(TurnEvent::SpeechDetected("wait".to_owned()));
    assert_eq!(turn.state(), &listening(""));
    assert_eq!(turn.level(), None, "a fresh listening starts unmeasured");
}

// ---------------------------------------------------------------------------
// Epic 65, Story 65.2 (AD-190): the switch persists intent, and the phrase is
// armed again when the refusal clears.
// ---------------------------------------------------------------------------

/// The shell's `arm`: set the phrase, carry the effects out, and on a refusal
/// fail the turn so the device is released and the reason is on the snapshot.
fn arm(turn: &mut Turn, port: &FakePort, wake: Option<WakePhrase>) -> Vec<Effect> {
    let effects = turn.set_wake(wake);
    if let Err(why) = perform(&effects, port, turn.wake()) {
        return turn.drive(TurnEvent::Failed(why.message(turn.platform())), port);
    }
    effects
}

/// The rule is three facts and one answer: never with the intent off, never
/// while armed, never while the port still refuses.
#[test]
fn voice_should_rearm_only_with_intent_on_not_armed_and_the_refusal_cleared() {
    assert!(should_rearm(true, false, true));
    assert!(
        !should_rearm(false, false, true),
        "intent off is the one no the person said"
    );
    assert!(
        !should_rearm(true, true, true),
        "already armed: nothing to do"
    );
    assert!(
        !should_rearm(true, false, false),
        "still refused: re-trying would loop"
    );
    assert!(!should_rearm(false, true, false));
    assert!(!should_rearm(false, false, false));
}

/// The owner's phone: the microphone not yet granted, the switch turned on.
/// The turn keeps the phrase (the intent), fails with the reason, and is
/// not armed. When the grant comes, arming again — from `Failed`, which used
/// to record the phrase and open nothing — opens the device for the phrase
/// and the snapshot says so.
#[test]
fn voice_driver_rearms_from_a_refusal_once_it_clears() {
    let port = FakePort {
        refuse_start: Some(VoiceUnavailable::NotAuthorized),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    let effects = arm(&mut turn, &port, Some(phrase("nixie")));
    assert_eq!(
        effects,
        vec![Effect::ReleaseMicrophone],
        "the refusal released the device"
    );
    assert!(matches!(turn.state(), TurnState::Failed { .. }));
    assert_eq!(
        turn.wake().map(WakePhrase::as_str),
        Some("nixie"),
        "the intent is kept"
    );
    assert!(!turn.armed());
    assert!(!turn.microphone_open());

    // Still refused: the rule says no, and arming anyway fails the same way.
    assert!(!should_rearm(
        true,
        turn.armed(),
        port.availability().is_ok()
    ));

    port.clear_refusal();
    assert!(should_rearm(
        true,
        turn.armed(),
        port.availability().is_ok()
    ));
    let effects = arm(&mut turn, &port, Some(phrase("nixie")));
    assert_eq!(effects, vec![Effect::OpenMicrophone]);
    assert_eq!(turn.state(), &TurnState::Idle, "the stale reason is gone");
    assert!(turn.armed());
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Idle {
            wake: Some("nixie".to_owned()),
            listening_for_wake: true,
        }
    );
    assert_eq!(
        port.calls(),
        vec![
            Call::Start(Some("nixie".to_owned())),
            Call::Stop,
            Call::Start(Some("nixie".to_owned())),
        ]
    );
    // Armed now: a second foreground, a second grant, changes nothing.
    assert!(!should_rearm(
        true,
        turn.armed(),
        port.availability().is_ok()
    ));
}

/// Turning the switch off from a refusal clears the reason with it: the
/// resting state is `Idle`, the device released, no phrase held.
#[test]
fn voice_driver_clearing_the_phrase_from_a_refusal_rests_idle() {
    let port = FakePort {
        refuse_start: Some(VoiceUnavailable::NotAuthorized),
        ..FakePort::default()
    };
    let mut turn = Turn::new(VoicePlatform::IOS);
    arm(&mut turn, &port, Some(phrase("nixie")));
    let effects = arm(&mut turn, &port, None);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(turn.wake().is_none());
    assert!(!turn.armed());
}

/// A turn in progress with the phrase set is armed in the sense that
/// matters — its own end re-opens the device — so a foreground or a grant
/// that lands mid-turn arms nothing and disturbs nothing.
#[test]
fn voice_driver_is_armed_while_a_turn_runs_and_not_before_the_phrase_is_set() {
    let port = FakePort::default();
    let mut turn = Turn::new(VoicePlatform::MACOS);
    assert!(!turn.armed(), "no phrase, nothing standing");
    arm(&mut turn, &port, Some(phrase("nixie")));
    assert!(turn.armed());
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("what time".to_owned()), &port);
    assert!(matches!(turn.state(), TurnState::Heard { .. }));
    assert!(
        turn.armed(),
        "mid-turn on a half-duplex Mac, the device released: still standing"
    );
    assert!(
        turn.set_wake(Some(phrase("nixie"))).is_empty(),
        "mid-turn, only recorded"
    );
    assert!(matches!(turn.state(), TurnState::Heard { .. }));
}
