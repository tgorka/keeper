//! The turn models (AD-410, D-36): a voice activity model that hears speech
//! start and stop, and an end-of-turn model that hears whether a sentence is
//! finished. Both come from the account's config repository (`_models/`,
//! the `vad` and `smart_turn` roles of [`crate::transcription::models`]) and
//! run on the device; without them a spoken turn ends on
//! [`END_OF_UTTERANCE_PAUSE`], as it always has.
//!
//! [`TurnModels`] is the port: the shell implements [`TurnInference`] over
//! ONNX Runtime on the Mac and the iPhone, loading the two hydrated files by
//! path, and [`spawn_turn_models`] runs it on a worker thread. It answers
//! numbers and decides nothing. [`TurnDisk::load`] is the one place that says
//! which models may be loaded at all, [`LoadSlot`] which load is published,
//! and [`turn_models_state`] whether the models are ready and, when they are
//! not, what the person is told.

use std::sync::{mpsc, Mutex, PoisonError};

use crate::transcription::models::ModelSet;
use crate::transcription::vm::{ModelsState, ModelsStateVm};

use super::turn::END_OF_UTTERANCE_PAUSE;

/// Samples in one frame the voice activity model scores: 32 ms at 16 kHz.
pub const VAD_FRAME: usize = 512;

/// The sample rate both models take.
pub const TURN_MODEL_RATE: u32 = 16_000;

/// Mel bins in the end-of-turn model's input.
pub const TURN_MEL_BINS: usize = 80;

/// Feature frames in the end-of-turn model's input: 8 s at a 10 ms hop.
pub const TURN_MEL_FRAMES: usize = 800;

/// The end-of-turn model's whole input, bin-major (`[80][800]`).
pub const TURN_FEATURES: usize = TURN_MEL_BINS * TURN_MEL_FRAMES;

/// Samples of the previous frame the voice activity model sees before each
/// new one (Silero VAD's context at 16 kHz).
pub const VAD_CONTEXT: usize = 64;

/// The voice activity model's recurrent state: `[2, 1, 128]`.
pub const VAD_STATE: usize = 2 * 128;

/// A turn model that could not answer, as a sentence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TurnModelError(pub String);

/// One stream of audio through the voice activity model. The model keeps
/// state across frames, so each stream (one capture) has its own; a new
/// capture starts from [`VadStream::reset`].
pub trait VadStream: Send {
    /// The probability, 0..1, that `frame` (16 kHz mono) holds speech.
    fn probability(&mut self, frame: &[f32; VAD_FRAME]) -> Result<f32, TurnModelError>;

    /// Forget every frame heard so far.
    fn reset(&mut self);
}

/// The two loaded turn models.
pub trait TurnModels: Send + Sync {
    /// A fresh stream through the voice activity model.
    fn vad(&self) -> Box<dyn VadStream>;

    /// The probability, 0..1, that the utterance whose log-mel features are
    /// `features` is a finished turn.
    fn turn_complete(&self, features: &[f32; TURN_FEATURES]) -> Result<f32, TurnModelError>;
}

/// What the last fetch of the turn models left behind. `Idle` means "look at
/// the disk".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnFetch {
    Idle,
    Fetching,
    Failed(String),
    NoAccount,
}

/// What is on this device for the turn models, as the shell read it once:
/// the same facts decide what is loaded ([`TurnDisk::load`]) and what the
/// person is told ([`turn_models_state`]), so the two never disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnDisk {
    /// An account is configured.
    pub account: bool,
    /// The set [`crate::transcription::models::choose_turn`] chose, or its
    /// refusal (a Settings pick, or a `models.toml` that does not parse).
    pub chosen: Result<ModelSet, String>,
    /// [`crate::transcription::models::turn_missing`] for the chosen set;
    /// empty when it was refused.
    pub missing: Vec<String>,
    /// The hydrated turn group is the one the account's clone names now.
    pub current: bool,
    /// The digest the turn group's completion marker records, if it is there.
    pub marker: Option<String>,
}

/// One hydration of the turn models, as a load is keyed: the two folders and
/// the turn group's completion digest they were hydrated under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnGeneration {
    pub vad_dir: String,
    pub smart_turn_dir: String,
    pub marker: String,
}

/// What the shell does with the turn models.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnLoad {
    /// Load exactly this hydration (or keep it, when it is loaded already).
    Load(TurnGeneration),
    /// Hold no turn models.
    Unload,
}

impl TurnDisk {
    /// Turn models are loaded only for an account, from a complete hydration
    /// that is the one the account's clone names now (D-36): files retained
    /// after the account was forgotten, or a marker the clone has moved on
    /// from, load nothing. An unreachable server does not move the clone, so
    /// models already hydrated stay usable offline.
    pub fn load(&self) -> TurnLoad {
        let Ok(set) = &self.chosen else {
            return TurnLoad::Unload;
        };
        match (&set.vad_dir, &set.smart_turn_dir, &self.marker) {
            (Some(vad), Some(smart_turn), Some(marker))
                if self.account && self.current && self.missing.is_empty() =>
            {
                TurnLoad::Load(TurnGeneration {
                    vad_dir: vad.clone(),
                    smart_turn_dir: smart_turn.clone(),
                    marker: marker.clone(),
                })
            }
            _ => TurnLoad::Unload,
        }
    }
}

/// What is known about the turn models on this device right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnModelsFacts {
    pub disk: TurnDisk,
    pub fetch: TurnFetch,
    /// The outcome of the last load of exactly the generation
    /// [`TurnDisk::load`] names: `None` before one was published.
    pub loaded: Option<Result<(), String>>,
}

/// Where the turn models are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnModelsState {
    /// Both are here, current, and loaded.
    Ready,
    /// These files or `models.toml` sections are absent; empty when every
    /// file is here but not the ones the account names now.
    Missing {
        files: Vec<String>,
    },
    /// A Settings pick or `models.toml` that cannot be used, as a sentence.
    Refused {
        sentence: String,
    },
    NoAccount,
    Fetching,
    /// A fetch or a load that failed, as a sentence.
    Failed {
        sentence: String,
    },
}

/// Readiness first: models that may be loaded and were are `Ready` even while
/// the fetch every sync starts is checking them again. A refusal outranks
/// everything, because no fetch can cure it.
pub fn turn_models_state(facts: TurnModelsFacts) -> TurnModelsState {
    let loadable = matches!(facts.disk.load(), TurnLoad::Load(_));
    if let Err(sentence) = facts.disk.chosen {
        return TurnModelsState::Refused { sentence };
    }
    if loadable {
        match facts.loaded {
            Some(Ok(())) => return TurnModelsState::Ready,
            Some(Err(why)) => {
                return TurnModelsState::Failed {
                    sentence: format!("The turn models could not be loaded: {why}"),
                }
            }
            None => {}
        }
    }
    match facts.fetch {
        TurnFetch::Fetching => TurnModelsState::Fetching,
        TurnFetch::Failed(why) => TurnModelsState::Failed {
            sentence: format!("The turn models could not be fetched: {why}"),
        },
        TurnFetch::NoAccount => TurnModelsState::NoAccount,
        TurnFetch::Idle if !facts.disk.account => TurnModelsState::NoAccount,
        TurnFetch::Idle => TurnModelsState::Missing {
            files: facts.disk.missing,
        },
    }
}

/// A load's place in line: taken before the facts are read, so the load it
/// starts can be told from a later one.
#[derive(Debug)]
pub struct Ticket(u64);

/// Where the loaded turn models are published. Loads run concurrently (one at
/// launch, one after each fetch) and take different times, so the order they
/// finish in says nothing: only the load holding the newest [`Ticket`] may
/// publish, and an obsolete one that finishes last is dropped rather than
/// replacing a newer generation.
pub struct LoadSlot<T> {
    inner: Mutex<(u64, Option<T>)>,
}

impl<T> LoadSlot<T> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new((0, None)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, Option<T>)> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A ticket newer than every one handed out before.
    pub fn ticket(&self) -> Ticket {
        let mut inner = self.lock();
        inner.0 += 1;
        Ticket(inner.0)
    }

    /// Publish `value` (`None`: hold nothing) if no newer ticket was handed
    /// out since `ticket`. `false` when the load is obsolete and was dropped.
    pub fn publish(&self, ticket: Ticket, value: Option<T>) -> bool {
        let mut inner = self.lock();
        if inner.0 != ticket.0 {
            return false;
        }
        inner.1 = value;
        true
    }

    /// Hold nothing, and drop every load under way.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.0 += 1;
        inner.1 = None;
    }

    /// `f` over what is published now.
    pub fn read<R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        f(self.lock().1.as_ref())
    }
}

impl<T> Default for LoadSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The two graphs a turn model may be, and the tensors keeper feeds and
/// reads by name. A file that loads but names them otherwise is refused at
/// load, so it never reads as ready and no inference looks up a name the
/// graph lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnGraph {
    Vad,
    SmartTurn,
}

impl TurnGraph {
    /// The inputs keeper feeds.
    pub const fn inputs(self) -> &'static [&'static str] {
        match self {
            Self::Vad => &["input", "state", "sr"],
            Self::SmartTurn => &["input_features"],
        }
    }

    /// The outputs keeper reads: the probability first.
    pub const fn outputs(self) -> &'static [&'static str] {
        match self {
            Self::Vad => &["output", "stateN"],
            Self::SmartTurn => &["logits"],
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Vad => "voice activity model",
            Self::SmartTurn => "end-of-turn model",
        }
    }

    /// Whether a graph with these input and output names is one keeper can
    /// run as this model; the refusal names what is absent and what is there.
    pub fn check<'a>(
        self,
        inputs: impl IntoIterator<Item = &'a str>,
        outputs: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), TurnModelError> {
        for (kind, wanted, have) in [
            (
                "input",
                self.inputs(),
                inputs.into_iter().collect::<Vec<_>>(),
            ),
            ("output", self.outputs(), outputs.into_iter().collect()),
        ] {
            if let Some(absent) = wanted.iter().find(|name| !have.contains(name)) {
                return Err(TurnModelError(format!(
                    "the {} has no {kind} \u{201c}{absent}\u{201d} (it has: {})",
                    self.noun(),
                    have.join(", ")
                )));
            }
        }
        Ok(())
    }
}

/// The raw inferences of one loaded pair of models, run on the worker
/// thread that owns them. The shell implements it over ONNX Runtime.
pub trait TurnInference: Send + 'static {
    /// The speech probability of `input` ([`VAD_CONTEXT`] samples of the
    /// previous frame, then the frame), updating `state` ([`VAD_STATE`]
    /// values) in place.
    fn vad(&mut self, input: &[f32], state: &mut [f32]) -> Result<f32, TurnModelError>;

    /// The end-of-turn score of `features` ([`TURN_FEATURES`] values).
    fn turn(&mut self, features: &[f32]) -> Result<f32, TurnModelError>;
}

/// `inference` on a worker thread of its own, as [`TurnModels`]: a model
/// runs one inference at a time, and the callers (a capture's frames, a
/// turn's end) never wait on each other's locks. If the worker stops, every
/// waiting and later call is an error, never a wait.
pub fn spawn_turn_models(
    inference: impl TurnInference,
) -> Result<WorkerTurnModels, TurnModelError> {
    let (requests, inbox) = mpsc::channel::<Request>();
    let mut inference = inference;
    std::thread::Builder::new()
        .name("keeper-turn-models".into())
        .spawn(move || {
            for request in inbox {
                match request {
                    Request::Vad {
                        input,
                        mut state,
                        reply,
                    } => {
                        let probability = inference.vad(&input, &mut state);
                        let answer = VadAnswer {
                            probability,
                            input,
                            state,
                            reply: reply.clone(),
                        };
                        let _ = reply.send(answer);
                    }
                    Request::Turn { features, reply } => {
                        let _ = reply.send(inference.turn(&features));
                    }
                }
            }
        })
        .map_err(|error| TurnModelError(format!("cannot start the turn models worker: {error}")))?;
    Ok(WorkerTurnModels { requests })
}

enum Request {
    Vad {
        input: Vec<f32>,
        state: Vec<f32>,
        reply: mpsc::SyncSender<VadAnswer>,
    },
    Turn {
        features: Vec<f32>,
        reply: mpsc::SyncSender<Result<f32, TurnModelError>>,
    },
}

/// The probability, and the buffers and reply sender handed back for the
/// next frame.
struct VadAnswer {
    probability: Result<f32, TurnModelError>,
    input: Vec<f32>,
    state: Vec<f32>,
    reply: mpsc::SyncSender<VadAnswer>,
}

fn worker_gone() -> TurnModelError {
    TurnModelError("the turn models worker stopped".to_owned())
}

/// [`spawn_turn_models`]'s answer.
pub struct WorkerTurnModels {
    requests: mpsc::Sender<Request>,
}

impl TurnModels for WorkerTurnModels {
    fn vad(&self) -> Box<dyn VadStream> {
        let (reply, answers) = mpsc::sync_channel(1);
        Box::new(WorkerVadStream {
            requests: self.requests.clone(),
            answers,
            parked: Some(Parked {
                input: vec![0.0; VAD_CONTEXT + VAD_FRAME],
                state: vec![0.0; VAD_STATE],
                reply,
            }),
        })
    }

    fn turn_complete(&self, features: &[f32; TURN_FEATURES]) -> Result<f32, TurnModelError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .send(Request::Turn {
                features: features.to_vec(),
                reply,
            })
            .map_err(|_| worker_gone())?;
        answer.recv().map_err(|_| worker_gone())?
    }
}

/// What a stream holds between frames: its buffers and the only sender of
/// its answers. The sender travels with each frame, so while the worker has
/// the frame the stream holds none, and a worker that stops drops it — the
/// wait for the answer ends instead of lasting forever.
struct Parked {
    input: Vec<f32>,
    state: Vec<f32>,
    reply: mpsc::SyncSender<VadAnswer>,
}

/// One capture's stream: the model's state and the last frame's tail.
struct WorkerVadStream {
    requests: mpsc::Sender<Request>,
    answers: mpsc::Receiver<VadAnswer>,
    /// `None` only while a frame is with the worker, or after it stopped.
    parked: Option<Parked>,
}

impl VadStream for WorkerVadStream {
    fn probability(&mut self, frame: &[f32; VAD_FRAME]) -> Result<f32, TurnModelError> {
        let Parked {
            mut input,
            state,
            reply,
        } = self.parked.take().ok_or_else(worker_gone)?;
        input.copy_within(VAD_FRAME.., 0);
        input[VAD_CONTEXT..].copy_from_slice(frame);
        self.requests
            .send(Request::Vad {
                input,
                state,
                reply,
            })
            .map_err(|_| worker_gone())?;
        let answer = self.answers.recv().map_err(|_| worker_gone())?;
        self.parked = Some(Parked {
            input: answer.input,
            state: answer.state,
            reply: answer.reply,
        });
        answer.probability
    }

    fn reset(&mut self) {
        if let Some(parked) = &mut self.parked {
            parked.input.fill(0.0);
            parked.state.fill(0.0);
        }
    }
}

impl TurnModelsState {
    /// The one line Settings shows under the voice switch (UX-DR142). Every
    /// state but `Ready` says what a turn does instead.
    pub fn sentence(&self) -> String {
        let fallback = fallback();
        match self {
            Self::Ready => "Turn models ready".to_owned(),
            Self::Missing { files } if files.is_empty() => format!(
                "Turn models out of date \u{2014} keeper brings them up to date after the next \
                 sync, and {fallback} until then"
            ),
            Self::Missing { files } => {
                format!(
                    "Turn models missing: {} \u{2014} {fallback}",
                    files.join(", ")
                )
            }
            Self::Refused { sentence } => format!("{sentence} Until then, {fallback}."),
            Self::NoAccount => format!(
                "No turn models without an account \u{2014} {fallback}. They come from your \
                 account's settings repository (Settings \u{2192} Account)."
            ),
            Self::Fetching => format!(
                "Fetching the turn models from your account\u{2026} {fallback} until they are here"
            ),
            Self::Failed { sentence } => format!("{sentence} \u{2014} {fallback}"),
        }
    }

    /// As the voice settings carry it: transcription's model-state shape, a
    /// refusal shown as a failure, as transcription shows its own.
    pub fn vm(&self) -> ModelsStateVm {
        let state = match self {
            Self::Ready => ModelsState::Ready,
            Self::Missing { .. } => ModelsState::Missing,
            Self::Refused { .. } | Self::Failed { .. } => ModelsState::Failed,
            Self::NoAccount => ModelsState::NoAccount,
            Self::Fetching => ModelsState::Fetching,
        };
        let missing = match self {
            Self::Missing { files } => files.clone(),
            _ => Vec::new(),
        };
        ModelsStateVm {
            state,
            sentence: self.sentence(),
            missing,
        }
    }
}

/// What a spoken turn does without the models: "keeper waits 1.8 s after you
/// stop", from the pause itself.
fn fallback() -> String {
    let millis = END_OF_UTTERANCE_PAUSE.as_millis();
    let seconds = millis / 1000;
    let tenths = (millis % 1000) / 100;
    format!("keeper waits {seconds}.{tenths} s after you stop")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> ModelSet {
        ModelSet {
            vad_dir: Some("silero-vad".to_owned()),
            smart_turn_dir: Some("smart-turn-v3".to_owned()),
            ..ModelSet::default()
        }
    }

    /// Both files here, hydrated, current, for an account.
    fn disk() -> TurnDisk {
        TurnDisk {
            account: true,
            chosen: Ok(set()),
            missing: Vec::new(),
            current: true,
            marker: Some("digest-b".to_owned()),
        }
    }

    fn facts(disk: TurnDisk) -> TurnModelsFacts {
        TurnModelsFacts {
            disk,
            fetch: TurnFetch::Idle,
            loaded: Some(Ok(())),
        }
    }

    fn absent() -> Vec<String> {
        vec!["silero-vad/model.onnx".to_owned()]
    }

    #[test]
    fn the_turn_models_state_follows_the_facts_in_order() {
        use TurnModelsState as S;
        let state = |facts: TurnModelsFacts| turn_models_state(facts);

        assert_eq!(state(facts(disk())), S::Ready);
        // Ready while a sync's fetch checks again.
        let checking = TurnModelsFacts {
            fetch: TurnFetch::Fetching,
            ..facts(disk())
        };
        assert_eq!(state(checking), S::Ready);
        // Here and current but never loaded: not ready, whatever the disk says.
        let unloaded = TurnModelsFacts {
            loaded: None,
            ..facts(disk())
        };
        assert_eq!(state(unloaded), S::Missing { files: Vec::new() });
        let broken = TurnModelsFacts {
            loaded: Some(Err("not an ONNX model".to_owned())),
            fetch: TurnFetch::Fetching,
            ..facts(disk())
        };
        assert_eq!(
            state(broken),
            S::Failed {
                sentence: "The turn models could not be loaded: not an ONNX model".to_owned()
            }
        );
        // Every file here but not what the account names now.
        let stale = facts(TurnDisk {
            current: false,
            ..disk()
        });
        assert_eq!(state(stale), S::Missing { files: Vec::new() });
        // A refusal outranks a fetch and a loaded set.
        let refused = TurnModelsFacts {
            fetch: TurnFetch::Fetching,
            ..facts(TurnDisk {
                chosen: Err("The pick is gone.".to_owned()),
                ..disk()
            })
        };
        assert_eq!(
            state(refused),
            S::Refused {
                sentence: "The pick is gone.".to_owned()
            }
        );

        let missing = |fetch, account| TurnModelsFacts {
            fetch,
            ..facts(TurnDisk {
                account,
                missing: absent(),
                ..disk()
            })
        };
        assert_eq!(state(missing(TurnFetch::Fetching, true)), S::Fetching);
        assert_eq!(
            state(missing(TurnFetch::Failed("401".to_owned()), true)),
            S::Failed {
                sentence: "The turn models could not be fetched: 401".to_owned()
            }
        );
        assert_eq!(state(missing(TurnFetch::NoAccount, true)), S::NoAccount);
        assert_eq!(state(missing(TurnFetch::Idle, false)), S::NoAccount);
        assert_eq!(
            state(missing(TurnFetch::Idle, true)),
            S::Missing { files: absent() }
        );
    }

    /// The loader and the line answer from the same rule: an account, and the
    /// hydration the clone names now.
    #[test]
    fn turn_models_load_only_for_the_account_and_its_current_hydration() {
        let generation = TurnGeneration {
            vad_dir: "silero-vad".to_owned(),
            smart_turn_dir: "smart-turn-v3".to_owned(),
            marker: "digest-b".to_owned(),
        };
        assert_eq!(disk().load(), TurnLoad::Load(generation.clone()));

        // The account forgotten, its files and marker retained on disk: no
        // load, and a load still published reads as no account, not ready.
        let forgotten = TurnDisk {
            account: false,
            ..disk()
        };
        assert_eq!(forgotten.load(), TurnLoad::Unload);
        assert_eq!(
            turn_models_state(facts(forgotten)),
            TurnModelsState::NoAccount
        );

        // A marker from before the clone moved on: no load until the new
        // hydration completes.
        let stale = TurnDisk {
            current: false,
            ..disk()
        };
        assert_eq!(stale.load(), TurnLoad::Unload);
        assert_ne!(turn_models_state(facts(stale)), TurnModelsState::Ready);
        assert_eq!(
            TurnDisk {
                marker: None,
                ..disk()
            }
            .load(),
            TurnLoad::Unload
        );
        assert_eq!(
            TurnDisk {
                missing: absent(),
                ..disk()
            }
            .load(),
            TurnLoad::Unload
        );
        assert_eq!(
            TurnDisk {
                chosen: Err("refused".to_owned()),
                ..disk()
            }
            .load(),
            TurnLoad::Unload
        );

        // The server unreachable, the clone where it was: still usable.
        let offline = TurnModelsFacts {
            fetch: TurnFetch::Failed("connection refused".to_owned()),
            ..facts(disk())
        };
        assert_eq!(offline.disk.load(), TurnLoad::Load(generation));
        assert_eq!(turn_models_state(offline), TurnModelsState::Ready);
    }

    /// A launch load of generation A that finishes after a fetch's load of
    /// generation B is dropped: B stays published.
    #[test]
    fn an_obsolete_load_never_replaces_a_newer_one() {
        let slot = LoadSlot::new();
        let launch = slot.ticket();
        let fetched = slot.ticket();
        assert!(slot.publish(fetched, Some("B")));
        assert!(!slot.publish(launch, Some("A")), "A is obsolete");
        assert_eq!(slot.read(|loaded| loaded.copied()), Some("B"));

        // An obsolete unload is dropped too.
        let old = slot.ticket();
        let new = slot.ticket();
        assert!(slot.publish(new, Some("C")));
        assert!(!slot.publish(old, None));
        assert_eq!(slot.read(|loaded| loaded.copied()), Some("C"));

        // Forgetting the account drops what is loaded and what is loading.
        let loading = slot.ticket();
        slot.clear();
        assert!(!slot.publish(loading, Some("D")));
        assert_eq!(slot.read(|loaded| loaded.copied()), None);
    }

    #[test]
    fn a_graph_with_other_names_is_refused_at_load() {
        assert_eq!(
            TurnGraph::Vad.check(["input", "state", "sr"], ["output", "stateN"]),
            Ok(())
        );
        assert_eq!(
            TurnGraph::SmartTurn.check(["input_features"], ["logits"]),
            Ok(())
        );
        assert_eq!(
            TurnGraph::Vad.check(["input", "state", "sr"], ["probability", "stateN"]),
            Err(TurnModelError(
                "the voice activity model has no output \u{201c}output\u{201d} (it has: \
                 probability, stateN)"
                    .to_owned()
            ))
        );
        let renamed = TurnGraph::SmartTurn
            .check(["input_features"], ["score"])
            .expect_err("refused");
        assert!(
            renamed.0.contains("no output \u{201c}logits\u{201d}"),
            "{renamed}"
        );
        let input = TurnGraph::SmartTurn
            .check(["audio"], ["logits"])
            .expect_err("refused");
        assert!(
            input.0.contains("no input \u{201c}input_features\u{201d}"),
            "{input}"
        );
    }

    /// Answers 0.5, echoing the frame's shaping; panics when told to.
    struct Fake {
        seen: std::sync::Arc<Mutex<Vec<Vec<f32>>>>,
        panic_on_vad: bool,
    }

    impl TurnInference for Fake {
        fn vad(&mut self, input: &[f32], state: &mut [f32]) -> Result<f32, TurnModelError> {
            assert!(!self.panic_on_vad, "the graph has no output named output");
            self.seen.lock().expect("lock").push(input.to_vec());
            state[0] += 1.0;
            Ok(0.5)
        }

        fn turn(&mut self, _features: &[f32]) -> Result<f32, TurnModelError> {
            Ok(0.75)
        }
    }

    /// `f` on another thread, failing instead of hanging past five seconds.
    fn within_five_seconds<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
        let (done, result) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(f());
        });
        result
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("answered within five seconds, not waiting forever")
    }

    #[test]
    fn the_worker_carries_the_context_and_the_state_between_frames() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let models = spawn_turn_models(Fake {
            seen: std::sync::Arc::clone(&seen),
            panic_on_vad: false,
        })
        .expect("spawned");
        let mut stream = models.vad();
        let first: [f32; VAD_FRAME] = std::array::from_fn(|i| i as f32);
        assert_eq!(stream.probability(&first), Ok(0.5));
        assert_eq!(stream.probability(&[-1.0; VAD_FRAME]), Ok(0.5));
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen[0].len(), VAD_CONTEXT + VAD_FRAME);
        assert!(seen[0][..VAD_CONTEXT].iter().all(|&x| x == 0.0));
        assert_eq!(seen[0][VAD_CONTEXT..], first);
        // The second frame sees the last 64 samples of the first.
        assert_eq!(seen[1][..VAD_CONTEXT], first[VAD_FRAME - VAD_CONTEXT..]);
        assert_eq!(models.turn_complete(&[0.0; TURN_FEATURES]), Ok(0.75));
    }

    /// A worker that dies on a frame it accepted: that frame and every later
    /// one is an error, and nothing waits forever.
    #[test]
    fn a_worker_that_stops_mid_frame_is_an_error_not_a_wait() {
        let outcome = within_five_seconds(|| {
            let models = spawn_turn_models(Fake {
                seen: std::sync::Arc::default(),
                panic_on_vad: true,
            })
            .expect("spawned");
            let mut stream = models.vad();
            let first = stream.probability(&[0.0; VAD_FRAME]);
            let second = stream.probability(&[0.0; VAD_FRAME]);
            let turn = models.turn_complete(&[0.0; TURN_FEATURES]);
            (first, second, turn)
        });
        assert_eq!(outcome.0, Err(worker_gone()));
        assert_eq!(outcome.1, Err(worker_gone()));
        assert_eq!(outcome.2, Err(worker_gone()));
    }

    #[test]
    fn every_state_but_ready_says_what_a_turn_does_instead() {
        assert_eq!(TurnModelsState::Ready.sentence(), "Turn models ready");
        assert_eq!(
            TurnModelsState::Missing { files: absent() }.sentence(),
            "Turn models missing: silero-vad/model.onnx \u{2014} keeper waits 1.8 s after you stop"
        );
        for state in [
            TurnModelsState::Missing { files: Vec::new() },
            TurnModelsState::Refused {
                sentence: "The pick is gone.".to_owned(),
            },
            TurnModelsState::NoAccount,
            TurnModelsState::Fetching,
            TurnModelsState::Failed {
                sentence: "No.".to_owned(),
            },
        ] {
            let sentence = state.sentence();
            assert!(
                sentence.contains("keeper waits 1.8 s after you stop"),
                "{state:?}: {sentence}"
            );
        }
        let vm = TurnModelsState::Refused {
            sentence: "The pick is gone.".to_owned(),
        }
        .vm();
        assert_eq!(vm.state, ModelsState::Failed);
        assert!(vm.sentence.starts_with("The pick is gone."));
        let vm = TurnModelsState::Missing { files: absent() }.vm();
        assert_eq!((vm.state, vm.missing), (ModelsState::Missing, absent()));
    }
}
