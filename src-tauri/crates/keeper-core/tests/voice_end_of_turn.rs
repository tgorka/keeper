//! Story 97.2 (AD-411): the end-of-turn detector over probabilities, the
//! resampler, and the listener from samples to `Heard`, against fake models.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use keeper_core::voice::end_of_turn::{
    hears_end_of_turn, utterance_ends, Detected, Ear, EndOfTurn, Heard, Listener, Resampler,
};
use keeper_core::voice::timings::TurnClock;
use keeper_core::voice::turn_models::{
    TurnModelError, TurnModels, VadStream, TURN_FEATURES, VAD_FRAME,
};
use keeper_core::voice::{advance, TurnEvent, TurnState};

// ---------------------------------------------------------------------------
// The detector, frame by frame. Frame `n` is delivered at `n * 32` ms.
// ---------------------------------------------------------------------------

/// Feed `probabilities`, returning `(frame, what was detected)`.
fn detect(probabilities: &[f32]) -> Vec<(usize, Detected)> {
    let mut detector = EndOfTurn::new();
    probabilities
        .iter()
        .enumerate()
        .filter_map(|(frame, &p)| {
            detector
                .feed(p, frame as i64 * 32)
                .map(|detected| (frame, detected))
        })
        .collect()
}

/// `silence` frames at 0.1, `speech` at 0.9, then `after` at 0.1.
fn utterance(silence: usize, speech: usize, after: usize) -> Vec<f32> {
    [vec![0.1; silence], vec![0.9; speech], vec![0.1; after]].concat()
}

#[test]
fn end_of_turn_timing() {
    // Onset: three frames at ≥ 0.5, reported at the first of them. Speech
    // ends at the first frame under 0.35; the score is asked seven frames
    // after it (7 × 32 ms = 224 ms, the first whole frames past 200).
    let timed = detect(&utterance(3, 10, 10));
    assert_eq!(
        timed,
        vec![
            (5, Detected::Onset { at_ms: 96 }),
            (13, Detected::SpeechEnd { at_ms: 416 }),
            (20, Detected::Score { speech_end_ms: 416 }),
        ]
    );
    let hangover_ms = timed[2].0 as i64 * 32 - 416;
    assert_eq!(hangover_ms, 224);
    assert!(hangover_ms >= 200, "the hangover is at least 200 ms");

    // An onset completing on the very frame the score would be asked
    // cancels it: speech resumed before the hangover was over.
    let mut boundary = utterance(3, 10, 5);
    boundary.extend([0.9; 3]);
    boundary.extend([0.1; 20]);
    let detected = detect(&boundary);
    assert_eq!(detected[2], (20, Detected::Onset { at_ms: 576 }));
    assert!(
        !detected[..3]
            .iter()
            .any(|(_, d)| matches!(d, Detected::Score { .. })),
        "{detected:?}"
    );

    // Exactly 0.5 starts an onset, 0.49 does not; two frames are not three.
    let onsets = |p: &[f32]| {
        detect(p)
            .into_iter()
            .map(|(frame, _)| frame)
            .collect::<Vec<_>>()
    };
    assert_eq!(onsets(&[0.5, 0.5, 0.5]), vec![2]);
    assert!(onsets(&[0.49, 0.9, 0.9, 0.1, 0.9, 0.9]).is_empty());

    // 0.35 is still speech; 0.34 ends it.
    let mut held = utterance(0, 10, 0);
    held.extend([0.35; 5]);
    assert_eq!(detect(&held).len(), 1, "only the onset");
    held.push(0.34);
    assert_eq!(
        detect(&held).last(),
        Some(&(15, Detected::SpeechEnd { at_ms: 480 }))
    );

    // At least 250 ms of speech before an end: 8 frames end, 7 are a click
    // and end nothing — and no score follows a click.
    assert_eq!(
        detect(&utterance(0, 8, 10))[1],
        (8, Detected::SpeechEnd { at_ms: 256 })
    );
    assert_eq!(detect(&utterance(0, 7, 20)).len(), 1, "only the onset");

    // A new onset inside the hangover cancels the score; the utterance goes
    // on, and its next end is scored seven frames later.
    let mut resumed = utterance(0, 10, 3);
    resumed.extend([0.9; 3]);
    resumed.extend([0.6; 4]);
    resumed.extend([0.1; 8]);
    assert_eq!(
        detect(&resumed),
        vec![
            (2, Detected::Onset { at_ms: 0 }),
            (10, Detected::SpeechEnd { at_ms: 320 }),
            (15, Detected::Onset { at_ms: 416 }),
            (20, Detected::SpeechEnd { at_ms: 640 }),
            (27, Detected::Score { speech_end_ms: 640 }),
        ]
    );

    // Two loud frames in the hangover are not an onset: the score comes.
    let mut blip = utterance(0, 10, 2);
    blip.extend([0.9, 0.9, 0.1, 0.1, 0.1, 0.1]);
    assert_eq!(
        detect(&blip).last(),
        Some(&(17, Detected::Score { speech_end_ms: 320 }))
    );

    // The verdict: ≥ 0.5 is a finished sentence.
    assert!(utterance_ends(0.5));
    assert!(!utterance_ends(0.49));
}

/// The listener listens while the turn is `Listening`, and only then.
#[test]
fn end_of_turn_listens_only_while_listening() {
    let words = TurnState::Listening {
        heard: "hej".to_owned(),
    };
    assert!(hears_end_of_turn(&TurnState::Listening {
        heard: String::new()
    }));
    assert!(hears_end_of_turn(&words));
    for state in [
        TurnState::Idle,
        TurnState::Finishing {
            heard: "hej".to_owned(),
            finish: 1,
            audio_ended_ms: None,
        },
        TurnState::Heard {
            text: "hej".to_owned(),
        },
        TurnState::Sending { answering: true },
        TurnState::Speaking,
        TurnState::Failed {
            reason: "x".to_owned(),
        },
    ] {
        assert!(!hears_end_of_turn(&state), "{state:?}");
    }
}

// ---------------------------------------------------------------------------
// The resampler.
// ---------------------------------------------------------------------------

fn tone(rate: u32, hertz: f64, seconds: f64) -> Vec<f32> {
    let n = (f64::from(rate) * seconds) as usize;
    (0..n)
        .map(|i| {
            (0.5 * (2.0 * std::f64::consts::PI * hertz * i as f64 / f64::from(rate)).sin()) as f32
        })
        .collect()
}

fn resample(rate: u32, input: &[f32], chunks: &[usize]) -> Vec<f32> {
    let mut resampler = Resampler::new(rate);
    let mut out = Vec::new();
    let mut rest = input;
    for &size in chunks.iter().cycle() {
        if rest.is_empty() {
            break;
        }
        let (now, later) = rest.split_at(size.min(rest.len()));
        resampler.push(now, &mut out);
        rest = later;
    }
    out
}

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

/// Rising zero crossings per second.
fn hertz(samples: &[f32], rate: f64) -> f64 {
    let rising = samples
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    rising as f64 * rate / samples.len() as f64
}

#[test]
fn resampler_keeps_speech_and_drops_what_16_khz_cannot_hold() {
    // At 16 kHz it is the input itself.
    let input = tone(16_000, 440.0, 0.5);
    assert_eq!(resample(16_000, &input, &[1024]), input);

    for rate in [48_000, 44_100, 24_000, 8_000] {
        let input = tone(rate, 440.0, 2.0);
        let out = resample(rate, &input, &[1024]);
        let expected = input.len() as f64 * 16_000.0 / f64::from(rate);
        assert!(
            (out.len() as f64 - expected).abs() < 120.0,
            "{rate}: {} samples for {expected}",
            out.len()
        );
        // Away from the start, the tone keeps its level and its pitch.
        let steady = &out[1600..out.len() - 200];
        let level = rms(steady);
        assert!(
            (level - 0.5 / 2f64.sqrt()).abs() < 0.005,
            "{rate}: rms {level}"
        );
        let pitch = hertz(steady, 16_000.0);
        assert!((pitch - 440.0).abs() < 2.0, "{rate}: {pitch} Hz");

        // Buffers of any size make the same samples.
        assert_eq!(resample(rate, &input, &[1, 997, 4800, 31]), out, "{rate}");
    }

    // A 12 kHz tone at 48 kHz is above the output's 8 kHz: filtered, not
    // folded back to 4 kHz.
    let out = resample(48_000, &tone(48_000, 12_000.0, 1.0), &[1024]);
    assert!(rms(&out[800..]) < 0.005, "aliased: {}", rms(&out[800..]));
}

// ---------------------------------------------------------------------------
// The listener, over fake models: loud frames are speech, and the end-of-turn
// model answers a fixed score.
// ---------------------------------------------------------------------------

struct LoudIsSpeech;

impl VadStream for LoudIsSpeech {
    fn probability(&mut self, frame: &[f32; VAD_FRAME]) -> Result<f32, TurnModelError> {
        Ok(if rms(frame) > 0.05 { 0.9 } else { 0.0 })
    }
    fn reset(&mut self) {}
}

struct FakeModels {
    score: f32,
    asked: AtomicUsize,
    /// The loudest feature of the last input: silence alone is flat.
    peak: Mutex<f32>,
}

impl FakeModels {
    fn scoring(score: f32) -> Arc<Self> {
        Arc::new(Self {
            score,
            asked: AtomicUsize::new(0),
            peak: Mutex::new(f32::NAN),
        })
    }
}

impl TurnModels for FakeModels {
    fn vad(&self) -> Box<dyn VadStream> {
        Box::new(LoudIsSpeech)
    }
    fn turn_complete(&self, features: &[f32; TURN_FEATURES]) -> Result<f32, TurnModelError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        *self.peak.lock().expect("lock") = features.iter().copied().fold(f32::MIN, f32::max);
        Ok(self.score)
    }
}

/// 0.5 s of silence, 1 s of a 220 Hz tone, 1 s of silence, at `rate`.
fn spoken(rate: u32) -> Vec<f32> {
    let n = |s: f64| (f64::from(rate) * s) as usize;
    [vec![0.0; n(0.5)], tone(rate, 220.0, 1.0), vec![0.0; n(1.0)]].concat()
}

/// Hear `audio` in tap-sized buffers, each delivered at the time its last
/// sample was captured.
fn listen(listener: &mut Listener, audio: &[f32], rate: u32) -> Vec<Heard> {
    let mut heard = Vec::new();
    let mut at = 0usize;
    for buffer in audio.chunks(1024) {
        at += buffer.len();
        let at_ms = (at as u64 * 1000 / u64::from(rate)) as i64;
        listener.hear(buffer, rate, at_ms, &mut |event| heard.push(event));
    }
    heard
}

#[test]
fn listener_ends_the_turn_on_a_finished_sentence_only() {
    for rate in [48_000, 16_000] {
        let models = FakeModels::scoring(0.8);
        let mut listener = Listener::new(Arc::clone(&models) as Arc<dyn TurnModels>);
        let heard = listen(&mut listener, &spoken(rate), rate);
        let [Heard::Onset { at_ms: onset }, Heard::SpeechEnd { at_ms: end }, Heard::UtteranceEnd {
            speech_end_ms,
            score,
        }] = heard[..]
        else {
            panic!("{rate}: {heard:?}");
        };
        assert!((500..=640).contains(&onset), "{rate}: onset {onset}");
        assert!((1500..=1600).contains(&end), "{rate}: end {end}");
        assert_eq!(speech_end_ms, end);
        assert_eq!(score, 0.8);
        assert_eq!(models.asked.load(Ordering::SeqCst), 1);
        assert!(
            *models.peak.lock().expect("lock") > 1.0,
            "the model heard the tone"
        );
    }

    // An unfinished sentence is reported, and is no end of the turn.
    let models = FakeModels::scoring(0.3);
    let mut listener = Listener::new(Arc::clone(&models) as Arc<dyn TurnModels>);
    let heard = listen(&mut listener, &spoken(48_000), 48_000);
    assert!(
        matches!(heard[..], [Heard::Onset { .. }, Heard::SpeechEnd { .. }, Heard::Unfinished { score, .. }] if score == 0.3),
        "{heard:?}"
    );
}

/// Where an attached ear's listener tells what it heard.
type Tell = Arc<dyn Fn(u64, Heard) + Send + Sync>;

/// Collect what an attached ear's listener tells, with its generation.
fn telling() -> (Tell, mpsc::Receiver<(u64, Heard)>) {
    let (tell, told) = mpsc::channel();
    let tell = Mutex::new(tell);
    let heard: Tell = Arc::new(move |generation, event| {
        let _ = tell.lock().expect("lock").send((generation, event));
    });
    (heard, told)
}

/// Feed `audio` to `ear` in tap-sized buffers, paced like a tap so the pool
/// never runs dry on a slow debug build.
fn feed(ear: &Ear, audio: &[f32], rate: u32) {
    for buffer in audio.chunks(1024) {
        ear.hear(buffer.iter().copied(), rate, 0);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What the listener tells, up to and including its first verdict.
fn until_verdict(told: &mpsc::Receiver<(u64, Heard)>) -> Vec<(u64, Heard)> {
    let mut events = Vec::new();
    while let Ok(event) = told.recv_timeout(Duration::from_secs(20)) {
        let verdict = matches!(
            event.1,
            Heard::UtteranceEnd { .. } | Heard::Unfinished { .. }
        );
        events.push(event);
        if verdict {
            break;
        }
    }
    events
}

/// The listening the ear is in now.
fn current(ear: &Ear) -> u64 {
    (0..1_000)
        .find(|&generation| ear.hears(generation))
        .expect("the ear listens")
}

fn listening(heard: &str) -> TurnState {
    TurnState::Listening {
        heard: heard.to_owned(),
    }
}

/// The ear passes audio to the listener thread only while the turn is
/// `Listening`; with no models attached it passes nothing. Every event
/// carries the listening it was heard in.
#[test]
fn ear_hears_only_while_the_turn_listens() {
    static EAR: Ear = Ear::new();
    let rate = 48_000;
    let audio = spoken(rate);
    EAR.follow(&listening(""));
    feed(&EAR, &audio, rate); // nothing attached: dropped at once.

    let (heard, told) = telling();
    EAR.attach(FakeModels::scoring(0.9), heard)
        .expect("listener starts");
    EAR.follow(&TurnState::Speaking);
    feed(&EAR, &audio, rate);
    EAR.follow(&listening(""));
    let generation = current(&EAR);
    feed(&EAR, &audio, rate);

    let events = until_verdict(&told);
    EAR.detach();
    assert!(
        matches!(
            events[..],
            [
                (a, Heard::Onset { .. }),
                (b, Heard::SpeechEnd { .. }),
                (c, Heard::UtteranceEnd { .. })
            ] if [a, b, c] == [generation; 3]
        ),
        "one utterance, the one heard while listening: {events:?}"
    );
    assert!(!EAR.hears(generation), "detached");
    assert_eq!(EAR.dropped(), 0, "paced like a tap, nothing dropped");
}

/// A voice activity model that counts the frames it hears.
struct Counting(Arc<AtomicUsize>);

impl VadStream for Counting {
    fn probability(&mut self, frame: &[f32; VAD_FRAME]) -> Result<f32, TurnModelError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        LoudIsSpeech.probability(frame)
    }
    fn reset(&mut self) {}
}

/// Models whose end-of-turn verdict waits to be let go: the test holds an
/// inference in flight while the turn moves on.
struct Gated {
    frames: Arc<AtomicUsize>,
    asked: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl TurnModels for Gated {
    fn vad(&self) -> Box<dyn VadStream> {
        Box::new(Counting(Arc::clone(&self.frames)))
    }
    fn turn_complete(&self, _features: &[f32; TURN_FEATURES]) -> Result<f32, TurnModelError> {
        let _ = self.asked.lock().expect("lock").send(());
        self.release
            .lock()
            .expect("lock")
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| TurnModelError("never released".to_owned()))?;
        Ok(0.9)
    }
}

/// A verdict still being computed when the turn leaves `Listening` —
/// abandoned and restarted, or barged in on — is the old listening's, and
/// the turn refuses it; audio queued behind it is skipped unheard; and a
/// listener replaced or detached mid-verdict is refused the same way. While
/// the verdict blocks the listener, the tap drops and counts audio it has
/// no buffer for instead of waiting.
#[test]
fn ear_refuses_what_an_ended_listening_heard() {
    static EAR: Ear = Ear::new();
    let rate = 16_000;
    let frames = Arc::new(AtomicUsize::new(0));
    let (asked_tx, asked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let models = Arc::new(Gated {
        frames: Arc::clone(&frames),
        asked: Mutex::new(asked_tx),
        release: Mutex::new(released),
    });
    let (heard, told) = telling();
    EAR.attach(models, heard).expect("listener starts");
    let mut clock = TurnClock::default();

    for leave in [TurnState::Idle, TurnState::Speaking] {
        EAR.follow(&listening(""));
        let old = current(&EAR);
        feed(&EAR, &spoken(rate), rate);
        asked
            .recv_timeout(Duration::from_secs(20))
            .expect("the verdict is in flight");
        // Speech queued behind the verdict, in the old listening.
        feed(&EAR, &tone(rate, 220.0, 0.5), rate);
        let heard_before = frames.load(Ordering::SeqCst);

        // The person abandons the turn and starts another, or barges in.
        EAR.follow(&leave);
        EAR.follow(&listening(""));
        let new = current(&EAR);
        assert_ne!(new, old);
        release.send(()).expect("release");

        // The new listening's sentence, its verdict let go at once.
        release.send(()).expect("release");
        feed(&EAR, &spoken(rate), rate);
        let _ = asked.recv_timeout(Duration::from_secs(20));
        let mut events = until_verdict(&told);
        events.extend(until_verdict(&told));
        let generations: Vec<u64> = events.iter().map(|(g, _)| *g).collect();
        assert_eq!(
            generations,
            [old, old, old, new, new, new],
            "nothing more from the old listening: {events:?}"
        );
        // The old verdict reaches the turn and is refused; its frames
        // were never heard. The new one is the turn's.
        let (_, stale) = &events[2];
        assert_eq!(EAR.admit(old, stale, &mut clock), None);
        assert_eq!(clock, TurnClock::default());
        let (_, fresh) = &events[5];
        assert_eq!(
            EAR.admit(new, fresh, &mut clock),
            Some(TurnEvent::UtteranceEnd(new))
        );
        let spoken_frames = spoken(rate).len() / VAD_FRAME;
        assert!(
            frames.load(Ordering::SeqCst) - heard_before <= spoken_frames + 4,
            "the queued tone was skipped"
        );
        clock = TurnClock::default();
    }

    // A replaced listener's verdict in flight is refused too, and while it
    // blocks the tap drops what it has no buffer for, without waiting.
    EAR.follow(&listening(""));
    let old = current(&EAR);
    feed(&EAR, &spoken(rate), rate);
    asked
        .recv_timeout(Duration::from_secs(20))
        .expect("the verdict is in flight");
    let dropped = EAR.dropped();
    let flood = tone(rate, 220.0, 5.0);
    for buffer in flood.chunks(1024) {
        EAR.hear(buffer.iter().copied(), rate, 0);
    }
    assert!(
        EAR.dropped() > dropped,
        "the pool ran dry and the tap dropped"
    );
    let (heard, _told) = telling();
    EAR.attach(FakeModels::scoring(0.9), heard)
        .expect("listener replaced");
    assert!(!EAR.hears(old));
    release.send(()).expect("release");
    let events = until_verdict(&told);
    let (generation, verdict) = events.last().expect("the old verdict");
    assert_eq!(*generation, old);
    assert_eq!(EAR.admit(old, verdict, &mut clock), None);
    EAR.detach();
}

/// The listener's events change the turn's clock only while their
/// listening is the turn's: an onset delivered after the sentence moved
/// the turn to `Finishing` cannot erase its speech end, and a speech end
/// delivered after the question was sent cannot land on the next turn.
#[test]
fn ear_keeps_late_clock_points_off_the_turn() {
    let ear = Ear::new();
    let mut clock = TurnClock::default();
    let mut state = listening("what time");
    let step = |clock: &mut TurnClock, state: &mut TurnState, event: TurnEvent, now: i64| {
        let (next, effects) = advance(state.clone(), event.clone());
        let record = clock.observe(state, &event, &next, &effects, now);
        *state = next;
        ear.follow(state);
        record
    };
    ear.follow(&state);
    let first = current(&ear);
    assert_eq!(
        ear.admit(first, &Heard::SpeechEnd { at_ms: 1_000 }, &mut clock),
        None
    );
    let finished = Heard::UtteranceEnd {
        speech_end_ms: 1_000,
        score: 0.9,
    };
    let event = ear
        .admit(first, &finished, &mut clock)
        .expect("the turn's own verdict");
    assert_eq!(event, TurnEvent::UtteranceEnd(first));
    step(&mut clock, &mut state, event, 1_250);
    // An onset that was heard just before the verdict, delivered after it.
    assert_eq!(
        ear.admit(first, &Heard::Onset { at_ms: 900 }, &mut clock),
        None
    );
    // A speech end of the same listening, delivered late.
    assert_eq!(
        ear.admit(first, &Heard::SpeechEnd { at_ms: 1_100 }, &mut clock),
        None
    );
    let record = step(
        &mut clock,
        &mut state,
        TurnEvent::FinalHeard("what time is it".to_owned()),
        1_500,
    )
    .expect("sent");
    assert_eq!(record.speech_end_ms, Some(1_000));

    // The next turn: the first turn's late speech end is not its.
    state = TurnState::Idle;
    step(&mut clock, &mut state, TurnEvent::WakeMatched, 2_000);
    step(
        &mut clock,
        &mut state,
        TurnEvent::PartialHeard("and".to_owned()),
        2_100,
    );
    assert_eq!(
        ear.admit(first, &Heard::SpeechEnd { at_ms: 1_400 }, &mut clock),
        None
    );
    let record = step(&mut clock, &mut state, TurnEvent::Silence, 4_000).expect("sent");
    assert_eq!(record.speech_end_ms, None);
}
