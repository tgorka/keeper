//! End of turn by meaning (AD-411): when a spoken question is finished.
//!
//! The capture's tap hands every buffer to the [`Ear`] — a copy into a
//! buffer from a fixed pool, nothing else on the audio thread — and one
//! listener thread does the rest while the turn is `Listening`
//! ([`hears_end_of_turn`]): [`Resampler`] takes the input's rate to the
//! models' 16 kHz, [`Listener`] cuts 32 ms frames for the voice activity
//! model and keeps the last 8 s, [`EndOfTurn`] turns the frames'
//! probabilities into onsets, speech ends and the moment to ask the
//! end-of-turn model, and [`keeper_ported::smart_turn::Features`] makes that
//! model's input. A score of at least [`TURN_COMPLETE`] is
//! [`Heard::UtteranceEnd`], which the shell hands the turn as
//! [`super::TurnEvent::UtteranceEnd`] through [`Ear::admit`], under the
//! turn's lock and only while the listening it was heard in is the turn's
//! current one. The audio goes from the tap to this thread and the two
//! models, and nowhere else.
//!
//! The detector's rules are Silero VAD's defaults and Smart Turn's: an onset
//! is [`ONSET_FRAMES`] frames running at a probability of at least
//! [`ONSET_PROBABILITY`]; speech ends at the first frame under
//! [`OFFSET_PROBABILITY`] once [`MIN_SPEECH`] has been heard (a shorter
//! burst is a click, and ends nothing); the end-of-turn model is asked once
//! [`HANGOVER`] has passed since, unless an onset came first; and it hears
//! the turn's speech from [`PRE_SPEECH`] before its first onset, at most
//! the last 8 s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::time::Duration;

use keeper_ported::smart_turn;

use super::timings::TurnClock;
use super::turn_models::{
    TurnModelError, TurnModels, VadStream, TURN_FEATURES, TURN_MODEL_RATE, VAD_FRAME,
};
use super::{TurnEvent, TurnState};

const _: () = assert!(smart_turn::FEATURES == TURN_FEATURES);
const _: () = assert!(smart_turn::RATE == TURN_MODEL_RATE as usize);

/// A frame's speech probability at or above which it may start an onset.
pub const ONSET_PROBABILITY: f32 = 0.5;

/// A frame's speech probability under which speech may end.
pub const OFFSET_PROBABILITY: f32 = 0.35;

/// Frames running at [`ONSET_PROBABILITY`] that make an onset: 96 ms.
pub const ONSET_FRAMES: u32 = 3;

/// The speech an end needs before it: shorter is a click.
pub const MIN_SPEECH: Duration = Duration::from_millis(250);

/// How long after speech ends the end-of-turn model is asked.
pub const HANGOVER: Duration = Duration::from_millis(200);

/// The audio kept before the turn's first onset for the end-of-turn model:
/// the onset frames themselves begin inside the first word.
pub const PRE_SPEECH: Duration = Duration::from_millis(500);

/// The end-of-turn model's score at or above which the sentence is finished.
pub const TURN_COMPLETE: f32 = 0.5;

/// One frame's length.
const FRAME_MS: u64 = VAD_FRAME as u64 * 1000 / TURN_MODEL_RATE as u64;

/// [`MIN_SPEECH`] in whole frames, rounded up.
const MIN_SPEECH_FRAMES: u64 = (MIN_SPEECH.as_millis() as u64).div_ceil(FRAME_MS);

/// [`HANGOVER`] in whole frames, rounded up: the score is asked on the
/// frame this many frames after the speech-end frame, 224 ms on.
const HANGOVER_FRAMES: u64 = (HANGOVER.as_millis() as u64).div_ceil(FRAME_MS);

/// Whether the turn in `state` is one the end-of-turn models listen for:
/// only `Listening`, the one state an [`super::TurnEvent::UtteranceEnd`]
/// moves. Everywhere else the listener rests and starts afresh when the
/// next `Listening` begins.
pub fn hears_end_of_turn(state: &TurnState) -> bool {
    matches!(state, TurnState::Listening { .. })
}

/// Whether a score says the sentence is finished.
pub fn utterance_ends(score: f32) -> bool {
    score >= TURN_COMPLETE
}

/// What the detector made of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Detected {
    /// Speech started; `at_ms` is the first of the onset's frames.
    Onset { at_ms: i64 },
    /// Speech stopped at this frame.
    SpeechEnd { at_ms: i64 },
    /// The hangover after the speech end at `speech_end_ms` passed: ask
    /// the end-of-turn model now.
    Score { speech_end_ms: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Silence,
    Speech { since: u64 },
    Hangover { since: u64, end: u64, end_ms: i64 },
}

/// The frame-by-frame detector over the voice activity model's
/// probabilities. Frame-exact: it counts frames, never reads a clock, and a
/// frame's `at_ms` only labels what it reports.
#[derive(Debug, Clone)]
pub struct EndOfTurn {
    phase: Phase,
    /// The frame being fed.
    frame: u64,
    /// Frames running at [`ONSET_PROBABILITY`], and the first one's time.
    run: u32,
    run_ms: i64,
}

impl Default for EndOfTurn {
    fn default() -> Self {
        Self::new()
    }
}

impl EndOfTurn {
    pub fn new() -> Self {
        Self {
            phase: Phase::Silence,
            frame: 0,
            run: 0,
            run_ms: 0,
        }
    }

    /// The next frame's speech `probability`, delivered at `at_ms`.
    pub fn feed(&mut self, probability: f32, at_ms: i64) -> Option<Detected> {
        let frame = self.frame;
        self.frame += 1;
        let onset = self.onset(probability, at_ms);
        match self.phase {
            Phase::Silence => {
                let since = onset?;
                self.phase = Phase::Speech { since };
                Some(Detected::Onset { at_ms: self.run_ms })
            }
            Phase::Speech { since } => {
                if probability >= OFFSET_PROBABILITY {
                    None
                } else if frame - since >= MIN_SPEECH_FRAMES {
                    self.phase = Phase::Hangover {
                        since,
                        end: frame,
                        end_ms: at_ms,
                    };
                    Some(Detected::SpeechEnd { at_ms })
                } else {
                    self.phase = Phase::Silence;
                    None
                }
            }
            Phase::Hangover { since, end, end_ms } => {
                if onset.is_some() {
                    self.phase = Phase::Speech { since };
                    Some(Detected::Onset { at_ms: self.run_ms })
                } else if frame - end >= HANGOVER_FRAMES {
                    self.phase = Phase::Silence;
                    Some(Detected::Score {
                        speech_end_ms: end_ms,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Count the run of onset frames; `Some(first frame)` when this frame
    /// completes an onset.
    fn onset(&mut self, probability: f32, at_ms: i64) -> Option<u64> {
        if probability < ONSET_PROBABILITY {
            self.run = 0;
            return None;
        }
        if self.run == 0 {
            self.run_ms = at_ms;
        }
        self.run += 1;
        (self.run >= ONSET_FRAMES && !matches!(self.phase, Phase::Speech { .. }))
            .then(|| self.frame - u64::from(self.run))
    }
}

/// Zeros of the resampling kernel on each side of a sample.
const KERNEL_ZEROS: f64 = 16.0;

/// The passband kept below the output's Nyquist frequency.
const PASSBAND: f64 = 0.95;

/// A windowed-sinc resampler from one input rate to [`TURN_MODEL_RATE`],
/// low-passed below the output's Nyquist frequency when it decimates, and
/// carried across buffers so their seams do not show.
#[derive(Debug, Clone)]
pub struct Resampler {
    from: u32,
    /// The kernel's cut-off, as a fraction of the input's Nyquist frequency.
    cutoff: f64,
    /// Input samples on each side the kernel reaches.
    half: usize,
    /// The input not yet consumed, from the first sample the next output
    /// still needs, after `half` samples of silence before the first.
    pending: Vec<f32>,
    /// How many input samples were dropped from the front of `pending`.
    dropped: u64,
    /// Outputs made so far. Output `n` sits at input position
    /// `n × from / 16000`, kept as an exact ratio so where buffers split
    /// never changes a sample.
    made: u64,
}

impl Resampler {
    /// A resampler from `from` Hz.
    pub fn new(from: u32) -> Self {
        let step = f64::from(from) / f64::from(TURN_MODEL_RATE);
        let cutoff = if step > 1.0 { PASSBAND / step } else { 1.0 };
        let half = (KERNEL_ZEROS / cutoff).ceil() as usize;
        Self {
            from,
            cutoff,
            half,
            pending: vec![0.0; half],
            dropped: 0,
            made: 0,
        }
    }

    /// The rate this resampler takes.
    pub fn rate(&self) -> u32 {
        self.from
    }

    /// Resample `input`, appending what it completes to `out`. The output
    /// trails the input by the kernel's half-width.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.from == TURN_MODEL_RATE {
            out.extend_from_slice(input);
            return;
        }
        self.pending.extend_from_slice(input);
        let rate = u64::from(TURN_MODEL_RATE);
        loop {
            let position = self.made * u64::from(self.from);
            // The output's centre in `pending`, and how far past it.
            let centre = (self.half as u64 + position / rate - self.dropped) as usize;
            if centre + self.half >= self.pending.len() {
                break;
            }
            let at = centre as f64 + (position % rate) as f64 / rate as f64;
            let mut sum = 0.0;
            for k in centre + 1 - self.half..=centre + self.half {
                sum += f64::from(self.pending[k]) * self.kernel(at - k as f64);
            }
            out.push(sum as f32);
            self.made += 1;
        }
        let next = self.half as u64 + self.made * u64::from(self.from) / rate - self.dropped;
        let consumed = (next as usize + 1).saturating_sub(self.half);
        self.pending.drain(..consumed);
        self.dropped += consumed as u64;
    }

    /// The low-pass sinc at `t` input samples from its centre, under a
    /// Blackman window as wide as the kernel.
    fn kernel(&self, t: f64) -> f64 {
        use std::f64::consts::PI;
        let x = self.cutoff * t;
        let sinc = if x.abs() < 1e-12 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        };
        let w = t / self.half as f64;
        if w.abs() >= 1.0 {
            return 0.0;
        }
        let window = 0.42 + 0.5 * (PI * w).cos() + 0.08 * (2.0 * PI * w).cos();
        self.cutoff * sinc * window
    }
}

/// What the listener tells the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    /// Speech started, at the first onset frame.
    Onset { at_ms: i64 },
    /// Speech stopped at this frame.
    SpeechEnd { at_ms: i64 },
    /// The end-of-turn model judged the utterance that stopped at
    /// `speech_end_ms` finished: the turn's [`super::TurnEvent::UtteranceEnd`].
    UtteranceEnd { speech_end_ms: i64, score: f32 },
    /// It judged it unfinished; the pause decides.
    Unfinished { speech_end_ms: i64, score: f32 },
    /// A model failed; the listener starts afresh and the pause decides.
    Failed(TurnModelError),
}

/// The last 8 s at 16 kHz, oldest overwritten first.
struct Ring {
    samples: Vec<f32>,
    /// Samples written since the listener started.
    written: u64,
}

impl Ring {
    fn new() -> Self {
        Self {
            samples: vec![0.0; smart_turn::SAMPLES],
            written: 0,
        }
    }

    fn push(&mut self, sample: f32) {
        let len = self.samples.len() as u64;
        self.samples[(self.written % len) as usize] = sample;
        self.written += 1;
    }

    /// The samples written since `from`, at most the ring's length, oldest
    /// first, into `out`.
    fn since(&self, from: u64, out: &mut Vec<f32>) {
        let len = self.samples.len() as u64;
        let from = from.max(self.written.saturating_sub(len));
        out.clear();
        for at in from..self.written {
            out.push(self.samples[(at % len) as usize]);
        }
    }
}

/// One capture's way from samples to [`Heard`]: synchronous, so the whole
/// of it is tested over a fake [`TurnModels`]; [`Ear::attach`] runs it on
/// its own thread.
pub struct Listener {
    models: Arc<dyn TurnModels>,
    vad: Box<dyn VadStream>,
    resampler: Option<Resampler>,
    detector: EndOfTurn,
    /// Resampled samples not yet framed.
    frame: Vec<f32>,
    resampled: Vec<f32>,
    ring: Ring,
    /// The ring position of the turn's first onset, less [`PRE_SPEECH`].
    start: Option<u64>,
    features: smart_turn::Features,
    window: Vec<f32>,
    input: Box<[f32; TURN_FEATURES]>,
}

impl Listener {
    pub fn new(models: Arc<dyn TurnModels>) -> Self {
        let vad = models.vad();
        Self {
            models,
            vad,
            resampler: None,
            detector: EndOfTurn::new(),
            frame: Vec::with_capacity(VAD_FRAME),
            resampled: Vec::new(),
            ring: Ring::new(),
            start: None,
            features: smart_turn::Features::new(),
            window: Vec::with_capacity(smart_turn::SAMPLES),
            input: vec![0.0; TURN_FEATURES]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!("TURN_FEATURES values")),
        }
    }

    /// Forget everything heard: a new turn, or a gap in the audio.
    pub fn reset(&mut self) {
        self.vad.reset();
        self.resampler = None;
        self.detector = EndOfTurn::new();
        self.frame.clear();
        self.ring = Ring::new();
        self.start = None;
    }

    /// Hear `samples` at `rate` Hz, delivered by the tap at `at_ms`, and
    /// tell `heard` what they amounted to.
    pub fn hear(&mut self, samples: &[f32], rate: u32, at_ms: i64, heard: &mut dyn FnMut(Heard)) {
        if self.resampler.as_ref().is_none_or(|r| r.rate() != rate) {
            self.resampler = Some(Resampler::new(rate));
        }
        let resampler = self.resampler.get_or_insert_with(|| Resampler::new(rate));
        self.resampled.clear();
        resampler.push(samples, &mut self.resampled);
        let resampled = std::mem::take(&mut self.resampled);
        for &sample in &resampled {
            self.ring.push(sample);
            self.frame.push(sample);
            if self.frame.len() == VAD_FRAME {
                if let Err(error) = self.frame_heard(at_ms, heard) {
                    self.reset();
                    heard(Heard::Failed(error));
                    break;
                }
                self.frame.clear();
            }
        }
        self.resampled = resampled;
    }

    fn frame_heard(
        &mut self,
        at_ms: i64,
        heard: &mut dyn FnMut(Heard),
    ) -> Result<(), TurnModelError> {
        let frame: &[f32; VAD_FRAME] = self.frame[..]
            .try_into()
            .unwrap_or_else(|_| unreachable!("a full frame"));
        let probability = self.vad.probability(frame)?;
        match self.detector.feed(probability, at_ms) {
            None => {}
            Some(Detected::Onset { at_ms }) => {
                if self.start.is_none() {
                    let pre = PRE_SPEECH.as_millis() as u64 * u64::from(TURN_MODEL_RATE) / 1000;
                    let onset = self
                        .ring
                        .written
                        .saturating_sub(u64::from(ONSET_FRAMES) * VAD_FRAME as u64);
                    self.start = Some(onset.saturating_sub(pre));
                }
                heard(Heard::Onset { at_ms });
            }
            Some(Detected::SpeechEnd { at_ms }) => heard(Heard::SpeechEnd { at_ms }),
            Some(Detected::Score { speech_end_ms }) => {
                self.ring.since(self.start.unwrap_or(0), &mut self.window);
                self.features.compute(&self.window, &mut self.input);
                let score = self.models.turn_complete(&self.input)?;
                heard(if utterance_ends(score) {
                    Heard::UtteranceEnd {
                        speech_end_ms,
                        score,
                    }
                } else {
                    Heard::Unfinished {
                        speech_end_ms,
                        score,
                    }
                });
            }
        }
        Ok(())
    }
}

/// Samples per buffer in the pool: 85 ms at 48 kHz, more than a tap holds.
const CHUNK: usize = 4096;

/// Buffers in the pool: over a second at 48 kHz, so the end-of-turn model's
/// tens of milliseconds never make the tap drop audio.
const POOL: usize = 48;

/// One buffer of the tap's audio on its way to the listener.
struct Chunk {
    samples: Box<[f32; CHUNK]>,
    len: usize,
    rate: u32,
    at_ms: i64,
    /// The listening it was heard in ([`Ear::hears`]).
    generation: u64,
    /// The first buffer after audio was dropped: the listener starts afresh.
    fresh: bool,
}

/// The two ends of the listener's queue that the tap holds.
struct Line {
    full: SyncSender<Chunk>,
    empty: Receiver<Chunk>,
}

/// Where the capture's tap puts its audio for the end-of-turn models. One
/// per process, held by the shell; [`Ear::hear`] is the only call the audio
/// thread makes, and it never allocates, never waits and never decides:
/// with no models attached, or the turn not `Listening`, it returns at
/// once, and with no free buffer — or the line busy being replaced — it
/// drops the audio, counts it ([`Ear::dropped`]), and the listener starts
/// afresh on the next.
///
/// Every listening has a generation ([`Ear::hears`]): the tap stamps each
/// buffer with it and the listener every event it makes of one. The
/// generation moves on when the turn starts or stops listening, when a
/// listener is attached or detached and when the capture is rebuilt, so
/// audio still queued from an earlier one is skipped unheard and a verdict
/// on it the listener was still computing is refused ([`Ear::admit`]).
pub struct Ear {
    line: Mutex<Option<Line>>,
    /// Odd while the turn listens, even while it rests.
    generation: AtomicU64,
    fresh: AtomicBool,
    dropped: AtomicU64,
}

impl Default for Ear {
    fn default() -> Self {
        Self::new()
    }
}

impl Ear {
    pub const fn new() -> Self {
        Self {
            line: Mutex::new(None),
            generation: AtomicU64::new(0),
            fresh: AtomicBool::new(true),
            dropped: AtomicU64::new(0),
        }
    }

    fn line(&self) -> std::sync::MutexGuard<'_, Option<Line>> {
        self.line.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Follow the turn: listen while [`hears_end_of_turn`] says so for
    /// `state`, rest otherwise. Starting or stopping moves the generation
    /// on. Called under the lock the turn moves under, so a generation
    /// [`Ear::admit`] accepts there is the turn's current listening.
    pub fn follow(&self, state: &TurnState) {
        let listening = u64::from(hears_end_of_turn(state));
        let _ = self
            .generation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |generation| {
                (generation % 2 != listening).then_some(generation + 1)
            });
    }

    /// Whether what was heard in `generation` is still the turn's: it was
    /// heard while the turn listened, and that listening is the current one.
    pub fn hears(&self, generation: u64) -> bool {
        generation % 2 == 1 && generation == self.generation.load(Ordering::SeqCst)
    }

    /// What the listener heard in `generation` does to the turn: its clock
    /// points go to `clock`, and a finished sentence is the event to move
    /// the turn with. Nothing at all when that listening is over — its
    /// clock points would land on another turn's record, and its verdict
    /// would end a sentence nobody finished. Called under the turn's lock,
    /// with the move it returns made before the lock is let go.
    pub fn admit(
        &self,
        generation: u64,
        heard: &Heard,
        clock: &mut TurnClock,
    ) -> Option<TurnEvent> {
        if !self.hears(generation) {
            return None;
        }
        match *heard {
            Heard::Onset { .. } => clock.onset(),
            Heard::SpeechEnd { at_ms } => clock.speech_end(at_ms),
            Heard::UtteranceEnd { speech_end_ms, .. } => {
                clock.speech_end(speech_end_ms);
                return Some(TurnEvent::UtteranceEnd(generation));
            }
            Heard::Unfinished { .. } | Heard::Failed(_) => {}
        }
        None
    }

    /// The tap's buffers dropped since the process started: no free buffer
    /// in the pool, or the line busy being replaced.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }

    /// The capture was rebuilt: what comes is not continuous with what was
    /// heard, so the listener starts afresh and nothing it was still making
    /// of the old capture reaches the turn.
    pub fn interrupt(&self) {
        self.generation.fetch_add(2, Ordering::SeqCst);
    }

    fn drop_audio(&self) {
        self.fresh.store(true, Ordering::SeqCst);
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }

    /// The tap's buffer: `samples` (channel 0) at `rate` Hz, delivered at
    /// `at_ms`. Called on the audio thread.
    pub fn hear(&self, samples: impl Iterator<Item = f32>, rate: u32, at_ms: i64) {
        if self.generation.load(Ordering::SeqCst).is_multiple_of(2) {
            return;
        }
        let line = match self.line.try_lock() {
            Ok(line) => line,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                self.drop_audio();
                return;
            }
        };
        let Some(line) = line.as_ref() else {
            return;
        };
        // Read under the line's lock: `attach` moves both together.
        let generation = self.generation.load(Ordering::SeqCst);
        let mut samples = samples.peekable();
        while samples.peek().is_some() {
            let Ok(mut chunk) = line.empty.try_recv() else {
                self.drop_audio();
                return;
            };
            chunk.len = 0;
            for (slot, sample) in chunk.samples.iter_mut().zip(samples.by_ref()) {
                *slot = sample;
                chunk.len += 1;
            }
            chunk.rate = rate;
            chunk.at_ms = at_ms;
            chunk.generation = generation;
            chunk.fresh = self.fresh.swap(false, Ordering::SeqCst);
            // Every buffer in the pool fits in the queue at once, so this
            // fails only when the listener is gone.
            if line.full.try_send(chunk).is_err() {
                self.drop_audio();
                return;
            }
        }
    }

    /// Listen through `models` on a thread of its own, in place of any
    /// listener before, telling `heard` — on that thread, in the order
    /// heard — what it hears and in which generation. Audio queued for a
    /// listening that is over is skipped unheard.
    pub fn attach(
        &'static self,
        models: Arc<dyn TurnModels>,
        heard: Arc<dyn Fn(u64, Heard) + Send + Sync>,
    ) -> Result<(), TurnModelError> {
        let (full, inbox) = mpsc::sync_channel::<Chunk>(POOL);
        let (returns, empty) = mpsc::sync_channel::<Chunk>(POOL);
        for _ in 0..POOL {
            let _ = returns.send(Chunk {
                samples: vec![0.0; CHUNK]
                    .into_boxed_slice()
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("CHUNK values")),
                len: 0,
                rate: 0,
                at_ms: 0,
                generation: 0,
                fresh: true,
            });
        }
        std::thread::Builder::new()
            .name("keeper-end-of-turn".into())
            .spawn(move || {
                let mut listener = Listener::new(models);
                let mut hearing = None;
                for chunk in inbox {
                    let generation = chunk.generation;
                    if self.hears(generation) {
                        if chunk.fresh || hearing != Some(generation) {
                            listener.reset();
                            hearing = Some(generation);
                        }
                        listener.hear(
                            &chunk.samples[..chunk.len],
                            chunk.rate,
                            chunk.at_ms,
                            &mut |event| heard(generation, event),
                        );
                    }
                    // Never blocks: the pool is exactly the channel's size.
                    let _ = returns.send(chunk);
                }
            })
            .map_err(|error| {
                TurnModelError(format!("cannot start the end-of-turn listener: {error}"))
            })?;
        let before = {
            let mut line = self.line();
            let before = line.replace(Line { full, empty });
            self.generation.fetch_add(2, Ordering::SeqCst);
            before
        };
        // The old queue is let go outside the lock the tap tries.
        drop(before);
        Ok(())
    }

    /// Stop listening: the listener thread skips what is queued and ends.
    pub fn detach(&self) {
        let before = {
            let mut line = self.line();
            let before = line.take();
            self.generation.fetch_add(2, Ordering::SeqCst);
            before
        };
        drop(before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tap never waits on the line's lock: while `attach` or `detach`
    /// holds it, the buffer is dropped and counted.
    #[test]
    fn the_tap_drops_rather_than_waits_for_the_line() {
        static EAR: Ear = Ear::new();
        EAR.follow(&TurnState::Listening {
            heard: String::new(),
        });
        let held = EAR.line();
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            EAR.hear([0.0f32; 512].into_iter(), 16_000, 0);
            let _ = done.send(());
        });
        let returned = finished.recv_timeout(Duration::from_secs(5));
        drop(held);
        assert!(returned.is_ok(), "the tap waited on the lock");
        assert_eq!(EAR.dropped(), 1);
    }
}
