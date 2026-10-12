//! Story 98.4 (AD-414, D-36): what the Android voice port is told, decided
//! in `keeper_core::voice` and tested here, where the port cannot run —
//! joining a segmented recogniser's results (#5), whether and in which
//! language keeper listens, what a recogniser error is, the voice an answer
//! is read in, and the turn models' line on Android (#7).

use std::time::Duration;

use keeper_core::transcription::vm::ModelsState;
use keeper_core::voice::android::{
    act, answer, permission, recovery, turn_models, DeviceFacts, Recovery, Sessions,
    RESTARTS_TOLERATED, RESTART_FAILS_WITHIN, VOICE_FROM_API,
};
use keeper_core::voice::segments::{Segments, KEEP_CLOSED};
use keeper_core::voice::{Consent, Permission, VoiceUnavailable};

// ---------------------------------------------------------------------------
// Segments (#5).
// ---------------------------------------------------------------------------

/// What a port reports to the turn as the results arrive, in order.
fn heard(results: &[(&str, &str)]) -> Vec<String> {
    let mut segments = Segments::new();
    results
        .iter()
        .map(|(kind, text)| match *kind {
            "partial" => segments.partial(text),
            "segment" => segments.segment(text),
            other => panic!("unknown result kind {other}"),
        })
        .collect()
}

#[test]
fn segmented_results_join() {
    // Partials replace each other; a segment's result closes it; the next
    // segment's partials go after it, and nothing is lost at the join.
    let seen = heard(&[
        ("partial", "what is"),
        ("partial", "what is the"),
        ("segment", "what is the weather"),
        ("partial", "in"),
        ("partial", "in Warsaw"),
        ("segment", "in Warsaw today"),
    ]);
    assert_eq!(
        seen,
        [
            "what is",
            "what is the",
            "what is the weather",
            "what is the weather in",
            "what is the weather in Warsaw",
            "what is the weather in Warsaw today",
        ]
    );
}

#[test]
fn a_segment_result_replaces_its_partial_instead_of_repeating_it() {
    let seen = heard(&[("partial", "turn on"), ("segment", "turn on the lights")]);
    assert_eq!(seen.last().map(String::as_str), Some("turn on the lights"));
}

#[test]
fn a_word_said_twice_across_a_join_is_kept_twice() {
    let seen = heard(&[
        ("segment", "no"),
        ("partial", "no more"),
        ("segment", "no more"),
    ]);
    assert_eq!(seen.last().map(String::as_str), Some("no no more"));
}

#[test]
fn silence_closes_a_segment_without_adding_anything() {
    let mut segments = Segments::new();
    segments.segment("hello");
    assert_eq!(segments.segment("   "), "hello");
    assert_eq!(segments.partial(""), "hello");
    assert_eq!(segments.partial("there"), "hello there");
    assert_eq!(Segments::new().utterance(), "");
}

// ---------------------------------------------------------------------------
// The port's facts, errors and turn models (#4, #6, #7).
// ---------------------------------------------------------------------------

fn ready() -> DeviceFacts {
    DeviceFacts {
        sdk_int: 34,
        microphone: "granted".to_owned(),
        recogniser: true,
        installed: vec!["pl-PL".to_owned(), "en-US".to_owned()],
        downloadable: vec!["de-DE".to_owned()],
        system: "pl-PL".to_owned(),
        echo_canceller: true,
        voices: vec!["pl-PL".to_owned(), "en-US".to_owned(), "pl-PL".to_owned()],
    }
}

#[test]
fn an_older_android_is_refused_before_anything_else_is_asked() {
    let facts = DeviceFacts {
        sdk_int: VOICE_FROM_API - 1,
        microphone: "denied".to_owned(),
        ..ready()
    };
    assert_eq!(
        facts.availability(None),
        Err(VoiceUnavailable::SystemTooOld {
            needs: "Android 13".to_owned()
        })
    );
    assert_eq!(
        DeviceFacts {
            sdk_int: VOICE_FROM_API,
            ..ready()
        }
        .availability(None),
        Ok("pl-PL".to_owned())
    );
}

#[test]
fn listening_needs_the_microphone_granted_not_just_askable() {
    for state in ["prompt", "prompt-with-rationale", "denied", ""] {
        let facts = DeviceFacts {
            microphone: state.to_owned(),
            ..ready()
        };
        assert_eq!(
            facts.availability(None),
            Err(VoiceUnavailable::NotAuthorized),
            "{state}"
        );
    }
    assert_eq!(permission("granted"), Permission::Granted);
    assert_eq!(permission("denied"), Permission::Denied);
    assert_eq!(
        permission("prompt-with-rationale"),
        Permission::NotDetermined
    );
    assert_eq!(
        ready().consent(),
        Consent {
            speech: Permission::Granted,
            microphone: Permission::Granted
        }
    );
}

#[test]
fn a_downloadable_language_is_a_download_and_an_absent_one_names_what_runs() {
    let facts = ready();
    assert_eq!(facts.availability(Some("en-US")), Ok("en-US".to_owned()));
    assert_eq!(
        facts.availability(Some("de_DE")),
        Err(VoiceUnavailable::NoOnDeviceModel {
            locale: "de-DE".to_owned()
        })
    );
    assert_eq!(
        facts.availability(Some("fr-FR")),
        Err(VoiceUnavailable::NoOnDeviceRecognition {
            locale: "fr-FR".to_owned(),
            on_device: vec!["en-US".to_owned(), "pl-PL".to_owned()],
        })
    );
}

#[test]
fn a_device_without_an_on_device_recogniser_runs_no_language() {
    let facts = DeviceFacts {
        recogniser: false,
        ..ready()
    };
    assert_eq!(
        facts.availability(Some("de-DE")),
        Err(VoiceUnavailable::NoOnDeviceRecognition {
            locale: "de-DE".to_owned(),
            on_device: Vec::new(),
        })
    );
    assert!(facts.locales().on_device.is_empty());
}

#[test]
fn an_answer_is_read_only_in_a_voice_the_device_has() {
    let facts = ready();
    assert_eq!(facts.voices(), ["en-US", "pl-PL"]);
    assert_eq!(facts.voice_for("pl_pl"), Ok("pl-PL".to_owned()));
    assert_eq!(
        facts.voice_for("de-DE"),
        Err(VoiceUnavailable::NoVoice {
            language: "de-DE".to_owned()
        })
    );
}

#[test]
fn android_ends_a_turn_by_the_pause_and_says_so() {
    let state = turn_models();
    assert_eq!(
        state.sentence(),
        "Turn models are not available on Android; keeper waits 1.8 s after you stop"
    );
    let vm = state.vm();
    assert_eq!(vm.state, ModelsState::Unavailable);
    assert!(vm.missing.is_empty());
}

/// A session that died at once, and one that ran for a while.
const AT_ONCE: Duration = Duration::from_millis(200);
const LIVED: Duration = Duration::from_secs(30);

/// Feed `recovery` the same error `times` times, each session living
/// `lived`, the way the port threads `in_a_row` through; the last verdict.
fn errors_in_a_row(code: i32, times: u32, lived: Duration, facts: &DeviceFacts) -> Recovery {
    let mut in_a_row = 0;
    let mut verdict = Recovery::Again { in_a_row };
    for _ in 0..times {
        verdict = recovery(code, in_a_row, lived, "pl_PL", facts);
        if let Recovery::Again { in_a_row: next } = verdict {
            in_a_row = next;
        }
    }
    verdict
}

#[test]
fn a_recogniser_error_is_tried_again_a_few_times_or_said() {
    let facts = ready();
    let cannot_run = Recovery::Refuse(VoiceUnavailable::NoOnDeviceRecognition {
        locale: "pl-PL".to_owned(),
        on_device: vec!["en-US".to_owned(), "pl-PL".to_owned()],
    });
    // A client or busy error, or one the rule does not name, from sessions
    // that die at once: tried again a few times, then said.
    for code in [5, 8, 14, 99] {
        assert_eq!(
            errors_in_a_row(code, RESTARTS_TOLERATED + 1, AT_ONCE, &facts),
            cannot_run,
            "{code}"
        );
        assert!(
            matches!(
                errors_in_a_row(code, RESTARTS_TOLERATED, AT_ONCE, &facts),
                Recovery::Again { .. }
            ),
            "{code}"
        );
    }
    for code in [1, 2, 4, 10, 11, 12] {
        assert_eq!(
            recovery(code, 0, LIVED, "pl_PL", &facts),
            cannot_run,
            "{code}"
        );
    }
    assert_eq!(
        recovery(3, 0, LIVED, "pl-PL", &facts),
        Recovery::Refuse(VoiceUnavailable::NoMicrophone)
    );
    assert_eq!(
        recovery(9, 0, LIVED, "pl-PL", &facts),
        Recovery::Refuse(VoiceUnavailable::NotAuthorized)
    );
    assert_eq!(
        recovery(13, 0, LIVED, "pl_PL", &facts),
        Recovery::Refuse(VoiceUnavailable::NoOnDeviceModel {
            locale: "pl-PL".to_owned()
        })
    );
}

/// R98AV1-02: a quiet room is not a failing recogniser. Armed listening
/// hears silence timeouts for hours, at once or after a full session, and
/// is never refused for them.
#[test]
fn silence_is_never_counted_toward_a_refusal() {
    let facts = ready();
    for code in [6, 7] {
        for lived in [AT_ONCE, LIVED] {
            assert!(
                matches!(
                    errors_in_a_row(code, 100, lived, &facts),
                    Recovery::Again { .. }
                ),
                "{code} after {lived:?}"
            );
        }
        // Nor does it add to a count already running.
        assert_eq!(
            recovery(code, RESTARTS_TOLERATED, AT_ONCE, "pl_PL", &facts),
            Recovery::Again {
                in_a_row: RESTARTS_TOLERATED
            }
        );
    }
}

/// R98AV1-02: only a session that died at once counts; one that lived was
/// working, and starts the count again however many came before it.
#[test]
fn only_sessions_that_die_at_once_count_toward_a_refusal() {
    let facts = ready();
    assert!(matches!(
        errors_in_a_row(5, 100, LIVED, &facts),
        Recovery::Again { in_a_row: 0 }
    ));
    assert_eq!(
        recovery(5, RESTARTS_TOLERATED, RESTART_FAILS_WITHIN, "pl_PL", &facts),
        Recovery::Again { in_a_row: 0 }
    );
    assert!(matches!(
        recovery(
            5,
            RESTARTS_TOLERATED,
            RESTART_FAILS_WITHIN - Duration::from_millis(1),
            "pl_PL",
            &facts
        ),
        Recovery::Refuse(_)
    ));
}

/// R98AV1-01: every start, roll and stop names a new session, and only the
/// one named last is heard while listening — a rolled session's late words
/// are dropped, and nothing follows a stop.
#[test]
fn only_the_session_named_last_is_heard() {
    let mut sessions = Sessions::new();
    assert_eq!(sessions.roll(), None, "nothing listening, nothing to roll");
    let first = sessions.open();
    assert!(sessions.hears(first));
    let Some(rolled) = sessions.roll() else {
        panic!("a listening capture rolls");
    };
    assert!(
        !sessions.hears(first),
        "the rolled session's words are dropped"
    );
    assert!(sessions.hears(rolled));
    assert_eq!(sessions.current(), rolled);
    sessions.close();
    assert!(!sessions.listening());
    assert!(!sessions.hears(rolled), "nothing is heard after a stop");
    assert!(!sessions.hears(sessions.current()));
    assert_eq!(sessions.roll(), None);
    let reopened = sessions.open();
    assert!(sessions.hears(reopened) && reopened != first && reopened != rolled);
}

/// R98AV1-04: an armed day of segments keeps only the last `KEEP_CLOSED`,
/// while a question of ordinary length is kept whole.
#[test]
fn an_armed_day_of_segments_keeps_only_the_last_window() {
    let mut segments = Segments::new();
    let question = ["what is", "the weather", "in Warsaw today"];
    for part in question {
        segments.segment(part);
    }
    assert_eq!(segments.utterance(), "what is the weather in Warsaw today");

    let mut room = Segments::new();
    for n in 0..KEEP_CLOSED * 10 {
        room.segment(&format!("w{n}"));
    }
    let heard = room.partial("now");
    let words: Vec<&str> = heard.split(' ').collect();
    assert_eq!(words.len(), KEEP_CLOSED + 1);
    assert_eq!(words[0], format!("w{}", KEEP_CLOSED * 9));
    assert_eq!(words.last(), Some(&"now"));
}

/// R98AV1-06: before the device is read there is no answer — never
/// `Unsupported`, which would erase every voice surface — and once read the
/// facts answer.
#[test]
fn a_device_not_read_yet_is_no_answer_not_unsupported() {
    assert_eq!(answer(None, None), Err(VoiceUnavailable::NotReadYet));
    assert!(!VoiceUnavailable::NotReadYet.answered());
    assert!(VoiceUnavailable::Unsupported.answered());
    assert!(VoiceUnavailable::LeftTheFront.answered());
    assert_eq!(answer(Some(&ready()), None), Ok("pl-PL".to_owned()));
    assert_eq!(
        answer(Some(&ready()), Some("de-DE")),
        ready().availability(Some("de-DE"))
    );
}

/// R98AV1-05: behind another app nothing is heard or read aloud, whatever
/// has been read; in front, the facts are acted on once they are in.
#[test]
fn behind_another_app_nothing_is_heard_or_said() {
    let facts = ready();
    assert_eq!(
        act(Some(&facts), false),
        Err(VoiceUnavailable::LeftTheFront)
    );
    assert_eq!(act(None, false), Err(VoiceUnavailable::LeftTheFront));
    assert_eq!(act(None, true), Err(VoiceUnavailable::NotReadYet));
    assert_eq!(act(Some(&facts), true), Ok(&facts));
}
