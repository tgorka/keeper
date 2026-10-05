//! Story 97.2 #7 (NFR-114, F10): every spoken turn's `turn_end` record, its
//! way through the voice ring and the app log, and `measure` over a run.

use keeper_core::voice::events::{VoiceEventKind, VoiceEvents};
use keeper_core::voice::timings::{
    from_log, measure, EndedBy, TooFewTurns, TurnClock, TurnEnd, TurnFigures,
};
use keeper_core::voice::{advance, TurnEvent, TurnState};

/// Move `state` on `event` through the table and the clock at `now_ms`.
fn step(
    clock: &mut TurnClock,
    state: TurnState,
    event: TurnEvent,
    now_ms: i64,
) -> (TurnState, Option<TurnEnd>) {
    let (next, effects) = advance(state.clone(), event.clone());
    let record = clock.observe(&state, &event, &next, &effects, now_ms);
    (next, record)
}

#[test]
fn turn_timings_record_each_way_a_turn_ends() {
    let mut clock = TurnClock::default();
    let partial = |t: &str| TurnEvent::PartialHeard(t.to_owned());

    // The model ends it and the final words come. The decision is at
    // 1260; the worker runs `endAudio` 200 ms late, at 1460, and its report
    // reaches the turn at 1660; the final words come 500 ms after
    // `endAudio`. Decision and execution are two points.
    let (s, _) = step(&mut clock, TurnState::Idle, TurnEvent::WakeMatched, 0);
    let (s, _) = step(&mut clock, s, partial("what time"), 500);
    clock.speech_end(1_000);
    let (s, none) = step(&mut clock, s, TurnEvent::UtteranceEnd(1), 1_260);
    assert_eq!(none, None);
    let (s, _) = step(
        &mut clock,
        s,
        TurnEvent::AudioEnded {
            finish: 1,
            at_ms: 1_460,
        },
        1_660,
    );
    let (s, record) = step(
        &mut clock,
        s,
        TurnEvent::FinalHeard("What time?".to_owned()),
        1_960,
    );
    assert!(matches!(s, TurnState::Heard { .. }));
    assert_eq!(
        record,
        Some(TurnEnd {
            ended_by: EndedBy::Model,
            speech_end_ms: Some(1_000),
            utterance_end_ms: Some(1_260),
            finish_recognition_ms: Some(1_460),
            final_words_ms: Some(1_960),
            sent_ms: 1_960,
        })
    );

    // The model ends it and the wait for the last words runs out before
    // the port ever says it ended the audio: no execution point is claimed.
    let (s, _) = step(&mut clock, TurnState::Idle, TurnEvent::WakeMatched, 10_000);
    let (s, _) = step(&mut clock, s, partial("and"), 10_400);
    clock.speech_end(11_000);
    let (s, _) = step(&mut clock, s, TurnEvent::UtteranceEnd(3), 11_250);
    let (_, record) = step(&mut clock, s, TurnEvent::Silence, 11_850);
    let record = record.expect("sent");
    assert_eq!(record.ended_by, EndedBy::Model);
    assert_eq!(record.utterance_end_ms, Some(11_250));
    assert_eq!(record.finish_recognition_ms, None);
    assert_eq!(record.final_words_ms, None);
    assert_eq!(record.sent_ms, 11_850);

    // The pause ends it; speech that resumed after an end was no end.
    let (s, _) = step(&mut clock, TurnState::Idle, TurnEvent::WakeMatched, 20_000);
    let (s, _) = step(&mut clock, s, partial("and the second"), 20_500);
    clock.speech_end(21_000);
    clock.onset();
    let (_, record) = step(&mut clock, s.clone(), TurnEvent::Silence, 23_000);
    assert_eq!(
        record,
        Some(TurnEnd {
            ended_by: EndedBy::Pause,
            speech_end_ms: None,
            utterance_end_ms: None,
            finish_recognition_ms: None,
            final_words_ms: None,
            sent_ms: 23_000,
        })
    );

    // The recogniser's own final ends it.
    let (_, record) = step(
        &mut clock,
        s,
        TurnEvent::FinalHeard("hello".to_owned()),
        24_000,
    );
    assert_eq!(record.map(|r| r.ended_by), Some(EndedBy::Recogniser));

    // An abandoned turn leaves nothing for the next one.
    let (s, _) = step(&mut clock, TurnState::Idle, TurnEvent::WakeMatched, 30_000);
    clock.speech_end(30_500);
    let (s, _) = step(&mut clock, s, partial("x"), 30_600);
    let (_, none) = step(&mut clock, s, TurnEvent::Abandoned, 31_000);
    assert_eq!(none, None);
    let (s, _) = step(&mut clock, TurnState::Idle, TurnEvent::WakeMatched, 40_000);
    let (s, _) = step(&mut clock, s, partial("y"), 40_100);
    let (_, record) = step(&mut clock, s, TurnEvent::Silence, 42_000);
    assert_eq!(record.and_then(|r| r.speech_end_ms), None);
}

/// Twenty model turns whose `finish_recognition − speech_end` are 205, 210,
/// …, 300 ms and whose `sent − finish_recognition` are 120, 140, …, 500 ms,
/// listed out of order, and four pause turns: 2500 ms and 1800 ms after their
/// speech end (fine), 1799 ms (early), and one without models.
fn run() -> Vec<TurnEnd> {
    let mut records: Vec<TurnEnd> = [
        7, 3, 20, 1, 15, 9, 12, 5, 18, 2, 11, 16, 4, 19, 8, 14, 6, 10, 17, 13,
    ]
    .into_iter()
    .map(|i: i64| {
        let speech_end = i * 100_000;
        let finish = speech_end + 200 + 5 * i;
        TurnEnd {
            ended_by: EndedBy::Model,
            speech_end_ms: Some(speech_end),
            utterance_end_ms: Some(finish),
            finish_recognition_ms: Some(finish),
            final_words_ms: (i % 4 != 0).then_some(finish + 50),
            sent_ms: finish + 100 + 20 * i,
        }
    })
    .collect();
    for (speech_end, sent) in [
        (Some(5_000_000), 5_002_500),
        (Some(6_000_000), 6_001_800),
        (Some(7_000_000), 7_001_799),
        (None, 8_000_000),
    ] {
        records.push(TurnEnd {
            ended_by: EndedBy::Pause,
            speech_end_ms: speech_end,
            utterance_end_ms: None,
            finish_recognition_ms: None,
            final_words_ms: None,
            sent_ms: sent,
        });
    }
    records
}

#[test]
fn turn_timings_record_and_measure() {
    let records = run();
    assert_eq!(records.len(), 24);

    // Each record round-trips through the ring's detail and the app log.
    let mut ring = VoiceEvents::new();
    let mut log = String::from("2026-10-05T10:00:00Z  INFO keeper: voice: transition\n");
    for (at, record) in records.iter().enumerate() {
        ring.push(at as i64, VoiceEventKind::TurnEnd, Some(record.detail()));
        log.push_str(&format!(
            "2026-10-05T10:00:{:02}Z  INFO keeper::voice_log: {}\n",
            at % 60,
            record.log_line()
        ));
        log.push_str("2026-10-05T10:00:00Z  INFO keeper::voice_log: voice turn_end {not json}\n");
    }
    let details: Vec<TurnEnd> = ring
        .newest(usize::MAX)
        .into_iter()
        .rev()
        .map(|event| {
            assert_eq!(event.kind, "turn_end");
            TurnEnd::parse(event.detail.as_deref().expect("a detail")).expect("parses")
        })
        .collect();
    assert_eq!(details, records);
    assert_eq!(from_log(&log), records);

    // The figures, by hand: the 19th of 20 (nearest rank) is 295 and 480.
    assert_eq!(
        measure(&records),
        Ok(TurnFigures {
            turns: 24,
            model_turns: 20,
            finish_p95_ms: Some(295),
            send_p95_ms: Some(480),
            early_pauses: vec![7_001_799],
        })
    );

    // Fewer than twenty turns are refused, twenty are measured.
    assert_eq!(measure(&records[..19]), Err(TooFewTurns { turns: 19 }));
    assert!(measure(&records[..20]).is_ok());

    // Nineteen model turns (the 235/240 ms one dropped): 95 % of 19 is 18.05,
    // so the nearest rank is the 19th — the largest — not the 18th.
    let figures = measure(&records[1..]).expect("23 turns");
    assert_eq!(figures.model_turns, 19);
    assert_eq!(figures.finish_p95_ms, Some(300));
    assert_eq!(figures.send_p95_ms, Some(500));
}

/// A device run (97.2 #8): the figures over the app log a run wrote, by hand
/// on the machine that holds it — `KEEPER_VOICE_LOG=<keeper.log> cargo test
/// -p keeper-core --test voice_timings measure_a_device_run -- --ignored
/// --nocapture`.
#[test]
#[ignore = "needs the app log of a device run in KEEPER_VOICE_LOG"]
fn measure_a_device_run() {
    let path = std::env::var("KEEPER_VOICE_LOG").expect("KEEPER_VOICE_LOG");
    let text = std::fs::read_to_string(&path).expect("the app log reads");
    let figures = measure(&from_log(&text)).expect("enough turns");
    println!("{path}: {figures:#?}");
}
