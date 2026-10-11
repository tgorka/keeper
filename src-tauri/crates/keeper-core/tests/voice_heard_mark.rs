//! Story 97.3 #6 (AD-411, Q3, Q17): how far the person heard a spoken agent
//! answer, and which moves stop it.

use keeper_core::agents::events::CONTENT_VERSION;
use keeper_core::agents::log::HeardReason;
use keeper_core::agents::spoken::HeardMark;
use keeper_core::voice::backchannel::Utterance;
use keeper_core::voice::{Effect, TurnEvent, TurnState};

/// 97.3 #6 (Q3): the mark counts the sentences the synthesiser read to
/// their end, in the order they were handed to it, and ends where the
/// last of them does; the one it was cut off in is not counted, and
/// none finished is 0. A stop or a question over the answer marks it; an
/// answer heard to its end or ended by a failure does not.
#[test]
fn heard_until_counts_finished_sentences() {
    let mut mark = HeardMark::new("@me:x".to_owned(), "!room:x".to_owned());
    mark.handed(4);
    assert_eq!(mark.content(HeardReason::Stop), None, "no anchor yet");
    mark.anchored("$answer");
    let content = mark.content(HeardReason::Stop).expect("anchored");
    assert_eq!((content.heard_until, content.sentence), (0, 0));
    assert_eq!(content.anchor.as_str(), "$answer");
    assert_eq!(content.v, CONTENT_VERSION);

    mark.handed(15);
    mark.spoken();
    mark.handed(20);
    mark.spoken();
    // Stopped mid-sentence three.
    let content = mark.content(HeardReason::BargeIn).expect("anchored");
    assert_eq!((content.heard_until, content.sentence), (15, 2));
    assert_eq!(content.reason, HeardReason::BargeIn);
    mark.spoken();
    mark.spoken();
    let content = mark.content(HeardReason::Stop).expect("anchored");
    assert_eq!(
        (content.heard_until, content.sentence),
        (20, 3),
        "never past what was handed"
    );

    let speaking = TurnState::Speaking;
    let paused = TurnState::Paused {
        heard: "no wait".to_owned(),
        speech_ms: None,
    };
    let stop = [Effect::StopSpeaking, Effect::ReleaseMicrophone];
    assert_eq!(
        HeardMark::reason(&speaking, &TurnEvent::StopHeard, &stop),
        Some(HeardReason::Stop)
    );
    assert_eq!(
        HeardMark::reason(&paused, &TurnEvent::Abandoned, &stop),
        Some(HeardReason::Stop)
    );
    let asked = TurnEvent::Resolved(Utterance::Question("no wait".to_owned()));
    assert_eq!(
        HeardMark::reason(
            &paused,
            &asked,
            &[Effect::StopSpeaking, Effect::SendText("no wait".to_owned())]
        ),
        Some(HeardReason::BargeIn)
    );
    // Long wordless speech stops the answer with nothing to ask: a stop.
    assert_eq!(
        HeardMark::reason(
            &paused,
            &TurnEvent::Resolved(Utterance::Question(" ".to_owned())),
            &stop
        ),
        Some(HeardReason::Stop)
    );
    assert_eq!(
        HeardMark::reason(&speaking, &TurnEvent::Silence, &[Effect::ReleaseMicrophone]),
        None,
        "heard to its end"
    );
    assert_eq!(
        HeardMark::reason(&speaking, &TurnEvent::Failed("gone".to_owned()), &stop),
        None
    );
    assert_eq!(
        HeardMark::reason(
            &paused,
            &TurnEvent::Resolved(Utterance::BackchannelByWords),
            &[Effect::ContinueSpeaking]
        ),
        None
    );
    assert_eq!(
        HeardMark::reason(&TurnState::Idle, &TurnEvent::Abandoned, &stop),
        None
    );
    // Only a move that stopped the speech marks: one that did not leave it
    // playing.
    assert_eq!(HeardMark::reason(&speaking, &asked, &[]), None);
}
