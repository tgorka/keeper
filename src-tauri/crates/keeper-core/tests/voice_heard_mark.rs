//! Story 97.3 #6 (AD-411, Q3, Q17), as 108.2 carries it (R-HQ-68): how far
//! the person heard a spoken agent answer — the heard row's metadata — and
//! which moves stop it.

use keeper_core::agents::log::HeardReason;
use keeper_core::hq::sessions::AgentSessionRef;
use keeper_core::hq::spoken::HeardMark;
use keeper_core::voice::backchannel::Utterance;
use keeper_core::voice::{Effect, TurnEvent, TurnState};
use serde_json::json;

/// 97.3 #6 (Q3), R-HQ-68: the mark counts the sentences the synthesiser
/// read to their end, in the order they were handed to it, and ends where
/// the last of them does; the one it was cut off in is not counted, and none
/// finished is 0. It names the answer by the question its turn answers and
/// is the heard row's metadata: `kind: heard` and the contract's
/// `heard_until` object. A stop or a question over the answer marks it; an
/// answer heard to its end or ended by a failure does not.
#[test]
fn heard_until_counts_finished_sentences() {
    let key = AgentSessionRef {
        database: "tgdrive".to_owned(),
        session_id: "dev1.7".to_owned(),
    };
    let mut mark = HeardMark::new(key.clone(), "01QUESTION".to_owned());
    mark.handed(4);
    assert_eq!(mark.metadata(HeardReason::Stop), None, "no answer yet");
    mark.anchored();
    assert_eq!(
        mark.metadata(HeardReason::Stop),
        Some(
            json!({"kind": "heard", "message": "01QUESTION", "heard_until": 0,
                    "sentence": 0, "reason": "stop"})
        )
    );
    assert_eq!(mark.key(), &key);

    mark.handed(15);
    mark.spoken();
    mark.handed(20);
    mark.spoken();
    // Stopped mid-sentence three.
    assert_eq!(
        mark.metadata(HeardReason::BargeIn),
        Some(
            json!({"kind": "heard", "message": "01QUESTION", "heard_until": 15,
                    "sentence": 2, "reason": "barge_in"})
        )
    );
    mark.spoken();
    mark.spoken();
    let at_most = mark.metadata(HeardReason::Stop).expect("answered");
    assert_eq!(
        (
            at_most["heard_until"].as_u64(),
            at_most["sentence"].as_u64()
        ),
        (Some(20), Some(3)),
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
