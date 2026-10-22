//! Epic 67, Story 67.1 (AD-205, AD-206): the hands-free turn finishes
//! without the screen.
//!
//! The shell (`voice_ipc::transition` → the agent send →
//! `voice_ipc::answer_complete`) is a call site over two core decisions pinned
//! here against fakes: the turn's table hands out exactly one `SendText` and,
//! once the answer is back, exactly one `Speak`; and `voice::target` says
//! which of the person's agent conversations the text goes to — or refuses
//! with the sentence. The fake agent host below plays the shell's part: it
//! takes the `SendText`, answers, and feeds the answer back the way the
//! stream's close does. No webview, no channel, no event listener is anywhere
//! in the loop.

use std::sync::Mutex;

use keeper_core::vm::VoiceStateVm;
use keeper_core::voice::target::{self, SpokenRefusal, NO_TARGET_SENTENCE};
use keeper_core::voice::{
    Effect, Turn, TurnEvent, TurnState, VoicePlatform, VoicePort, VoiceUnavailable, WakePhrase,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start,
    Stop,
    Speak(String),
    Enqueue(String),
    StopSpeaking,
}

/// A port that records what it was asked to do and refuses nothing.
#[derive(Default)]
struct FakePort {
    calls: Mutex<Vec<Call>>,
}

impl FakePort {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("fake port lock").clone()
    }
    fn record(&self, call: Call) {
        self.calls.lock().expect("fake port lock").push(call);
    }
}

impl VoicePort for FakePort {
    fn platform(&self) -> VoicePlatform {
        VoicePlatform::IOS
    }
    fn availability(&self) -> Result<(), VoiceUnavailable> {
        Ok(())
    }
    fn locales(&self) -> keeper_core::voice::locale::DeviceLocales {
        keeper_core::voice::locale::DeviceLocales::default()
    }
    fn set_locale(&self, _requested: Option<String>) {}
    fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        self.record(Call::Start);
        Ok(())
    }
    fn stop_listening(&self) {
        self.record(Call::Stop);
    }
    fn voices(&self) -> Vec<String> {
        vec!["en-US".to_owned()]
    }
    fn listening(&self) -> String {
        "en-US".to_owned()
    }
    fn detect_language(&self, _text: &str, _constraints: &[String]) -> Option<String> {
        None
    }
    fn speak(&self, text: &str, _language: &str) -> Result<(), VoiceUnavailable> {
        self.record(Call::Speak(text.to_owned()));
        Ok(())
    }
    fn enqueue(&self, text: &str, _language: &str) -> Result<(), VoiceUnavailable> {
        self.record(Call::Enqueue(text.to_owned()));
        Ok(())
    }
    fn stop_speaking(&self) {
        self.record(Call::StopSpeaking);
    }
    fn pause_speaking(&self) {}
    fn continue_speaking(&self) {}
    fn finish_recognition(&self, _finish: u64) {}
}

/// The shell's part, played by a fake: the host that receives what the turn
/// heard, opens one stream on the resolved agent conversation, and hands the
/// answer back. `streams` is every stream it opened — the test asserts on its
/// length, because "one turn, not two" is the claim.
struct FakeAgentHost {
    chosen: Option<String>,
    rooms: Vec<String>,
    answer: &'static str,
    streams: Vec<(String, String)>,
}

impl FakeAgentHost {
    /// The shell's spoken send, minus the network: resolve the target with
    /// core, open a stream (record it), and return what the close would feed
    /// the turn — the answer, or the refusal's sentence.
    fn send_spoken(&mut self, text: &str) -> Result<String, String> {
        let room = target::resolve(self.chosen.as_deref(), &self.rooms)
            .map_err(|refusal| refusal.message().to_owned())?;
        self.streams.push((room, text.to_owned()));
        Ok(self.answer.to_owned())
    }
}

/// The text the turn handed out to send, if it did.
fn sent(effects: &[Effect]) -> Option<String> {
    let mut texts = effects.iter().filter_map(|effect| match effect {
        Effect::SendText(text) => Some(text.clone()),
        _ => None,
    });
    let first = texts.next();
    assert!(texts.next().is_none(), "one heard question is one send");
    first
}

/// The whole hands-free loop with the shell's glue played inline: a phrase
/// match, an utterance, one stream on the target, its answer spoken.
#[test]
fn a_heard_question_becomes_one_stream_on_the_target_and_its_answer_is_spoken() {
    let port = FakePort::default();
    let mut turn = Turn::new(port.platform());
    let armed = turn.set_wake(Some(WakePhrase::parse("nixie").expect("phrase")));
    assert_eq!(armed, vec![Effect::OpenMicrophone]);
    let mut host = FakeAgentHost {
        chosen: Some(target::agent_target("!dm:example.org")),
        rooms: vec!["!dm:example.org".to_owned(), "!talk:example.org".to_owned()],
        answer: "Three notes and a receipt.",
        streams: Vec::new(),
    };

    // The port hears the phrase in a transcript; `Turn` makes the match.
    let effects = turn.drive(TurnEvent::FinalHeard("nixie".to_owned()), &port);
    assert_eq!(
        *turn.state(),
        TurnState::Listening {
            heard: String::new()
        }
    );
    assert_eq!(sent(&effects), None);

    // The question. `Heard` hands out the one `SendText`; the shell's
    // `transition` performs it.
    let effects = turn.drive(
        TurnEvent::FinalHeard("what did I save yesterday".to_owned()),
        &port,
    );
    let text = sent(&effects).expect("the turn hands the question out to be sent");
    assert_eq!(text, "what did I save yesterday");
    assert!(
        matches!(turn.vm(), VoiceStateVm::Heard { text: ref t, .. } if t == &text),
        "the snapshot the webview would see carries the words, for showing — not for sending"
    );

    // `send_spoken`: one stream, on the chosen agent conversation.
    let answer = host.send_spoken(&text).expect("a target exists");
    assert_eq!(host.streams.len(), 1, "one turn, not two");
    assert_eq!(host.streams[0].0, "!dm:example.org");

    // The stream's progress, as `drive` reports it (AD-186)...
    turn.drive(TurnEvent::Sent, &port);
    assert_eq!(*turn.state(), TurnState::Sending { answering: false });
    turn.drive(TurnEvent::AnswerChunk, &port);
    assert_eq!(*turn.state(), TurnState::Sending { answering: true });

    // ...and its close: `answer_complete(text)` drives the `Speak`.
    let effects = turn.drive(TurnEvent::AnswerDone(answer.clone()), &port);
    assert_eq!(effects, vec![Effect::Speak(answer.clone())]);
    assert_eq!(*turn.state(), TurnState::Speaking);
    assert_eq!(
        port.calls(),
        vec![Call::Start, Call::Speak(answer)],
        "the turn's open, then exactly one utterance — nothing else was needed (the arm's own open is `set_wake`'s effect, performed by the shell)"
    );

    // The utterance ends; the phrase is armed again by the turn's own rule.
    turn.drive(TurnEvent::Silence, &port);
    assert_eq!(*turn.state(), TurnState::Idle);
    assert!(turn.armed());
}

/// A turn started by the button takes the same path: `WakeMatched` is the
/// control standing in for the phrase, and everything after it is the same.
#[test]
fn a_button_turn_takes_the_same_path() {
    let port = FakePort::default();
    let mut turn = Turn::new(port.platform());
    let mut host = FakeAgentHost {
        chosen: Some(target::agent_target("!talk:example.org")),
        rooms: vec!["!talk:example.org".to_owned()],
        answer: "Nothing yet.",
        streams: Vec::new(),
    };
    turn.drive(TurnEvent::WakeMatched, &port);
    let effects = turn.drive(TurnEvent::FinalHeard("anything new".to_owned()), &port);
    let text = sent(&effects).expect("sent");
    let answer = host.send_spoken(&text).expect("a target exists");
    assert_eq!(
        host.streams,
        vec![("!talk:example.org".to_owned(), "anything new".to_owned())]
    );
    let effects = turn.drive(TurnEvent::AnswerDone(answer.clone()), &port);
    assert_eq!(effects, vec![Effect::Speak(answer)]);
}

/// No target: the turn is refused with the sentence, which is what the
/// switch's refusal line shows and the ring records; the device is released
/// and nothing was sent anywhere.
#[test]
fn no_target_refuses_with_the_sentence_and_sends_nothing() {
    let port = FakePort::default();
    let mut turn = Turn::new(port.platform());
    turn.set_wake(Some(WakePhrase::parse("nixie").expect("phrase")));
    let mut host = FakeAgentHost {
        chosen: None,
        rooms: vec!["!dm:example.org".to_owned()],
        answer: "never",
        streams: Vec::new(),
    };
    turn.drive(TurnEvent::WakeMatched, &port);
    let effects = turn.drive(TurnEvent::FinalHeard("hello".to_owned()), &port);
    let text = sent(&effects).expect("sent");

    let refusal = host.send_spoken(&text).expect_err("nothing to talk to");
    assert_eq!(refusal, NO_TARGET_SENTENCE);
    assert!(refusal.contains("under Speak to"));
    assert!(host.streams.is_empty(), "nothing is sent to anyone nobody chose");

    // `answer_failed(sentence)`: the turn ends on the sentence.
    let effects = turn.drive(TurnEvent::Failed(refusal.clone()), &port);
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    assert_eq!(
        turn.vm(),
        VoiceStateVm::Failed {
            reason: refusal.clone()
        }
    );
    assert!(
        !port.calls().contains(&Call::Speak("never".to_owned()))
            && port.calls().last() == Some(&Call::Stop)
    );
    // The same refusal wording is core's, so the ring and the surface agree.
    assert_eq!(SpokenRefusal::NoTarget.message(), refusal);
}

/// A stream that fails after the send ends the turn with its reason rather
/// than leaving it in `Sending` with the microphone held (the stall the
/// code map recorded).
#[test]
fn a_failed_stream_ends_the_turn_with_its_reason() {
    let port = FakePort::default();
    let mut turn = Turn::new(port.platform());
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hello".to_owned()), &port);
    turn.drive(TurnEvent::Sent, &port);
    assert!(turn.awaiting_send());
    let effects = turn.drive(
        TurnEvent::Failed("The answer stopped before it finished.".to_owned()),
        &port,
    );
    assert_eq!(effects, vec![Effect::ReleaseMicrophone]);
    assert!(!turn.awaiting_send());
    assert!(matches!(turn.state(), TurnState::Failed { .. }));
}

/// Stop pressed on the answer is the question abandoned: nothing spoken, the
/// device released, the phrase re-armed.
#[test]
fn a_stopped_stream_abandons_the_turn_and_rearms() {
    let port = FakePort::default();
    let mut turn = Turn::new(port.platform());
    turn.set_wake(Some(WakePhrase::parse("nixie").expect("phrase")));
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hello".to_owned()), &port);
    turn.drive(TurnEvent::Sent, &port);
    let effects = turn.drive(TurnEvent::Abandoned, &port);
    assert_eq!(
        effects,
        vec![Effect::ReleaseMicrophone, Effect::OpenMicrophone]
    );
    assert_eq!(*turn.state(), TurnState::Idle);
    assert!(turn.armed());
    assert!(!port
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Speak(_))));
}
