//! The turn models on this device (AD-410, D-36): the voice activity model
//! and the end-of-turn model the account's config repository names, loaded
//! from `<data_dir>/models/` by path and run on the CPU through ONNX Runtime.
//!
//! Whether they may be loaded, which load is published, what a turn does with
//! them and what the person is told about them is
//! `keeper_core::voice::turn_models`'s; this file reads the facts that rule
//! is applied to, opens the two files it is told to, and answers numbers.
//! The files arrive through the models fetch (`transcribe_ipc::
//! spawn_models_fetch`, the turn group); nothing here asks anyone for a
//! model, and the runtime is only ever handed a file path.
//!
//! ONNX Runtime is linked statically on the two targets it ships a library
//! for that keeper builds — Apple silicon Macs and the iPhone itself
//! (`docs/constraints-and-limitations.md`). Everywhere else (an Intel Mac,
//! the iOS simulator) voice keeps its pause rule and shows no turn-models line.

use std::path::{Path, PathBuf};

use keeper_core::registry;
use keeper_core::transcription::models::{self as model_files, TURN_GROUP};
use keeper_core::transcription::vm::ModelsStateVm;
use keeper_core::voice::turn_models::{
    turn_models_state, LoadSlot, Ticket, TurnDisk, TurnGeneration, TurnLoad, TurnModelsFacts,
};

/// Whether this build's voice runs turn models.
pub fn supported() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(
            target_os = "ios",
            target_arch = "aarch64",
            not(target_env = "sim")
        )
    ))
}

/// The file each turn role's directory holds.
const MODEL_FILE: &str = "model.onnx";

/// What one published load left: the hydration it was for, and the models or
/// why they did not load.
struct Loaded {
    root: PathBuf,
    generation: TurnGeneration,
    models: Result<runtime::Models, String>,
}

static LOADED: LoadSlot<Loaded> = LoadSlot::new();

/// The turn models' line under the voice switch, or `None` where voice runs
/// none. Blocking: it reads the settings and hashes the turn group's plain
/// files in the account's clone.
pub fn state_vm(data_dir: &Path) -> Option<ModelsStateVm> {
    if !supported() {
        return None;
    }
    let (root, disk) = disk(data_dir);
    let loaded = match disk.load() {
        TurnLoad::Load(generation) => loaded(&root, &generation),
        TurnLoad::Unload => None,
    };
    let state = turn_models_state(TurnModelsFacts {
        disk,
        fetch: crate::transcribe_ipc::turn_fetch(),
        loaded,
    });
    Some(state.vm())
}

/// Carry out core's load decision for the facts on disk now: load the
/// generation it names unless exactly that one is loaded, or hold none.
/// Blocking; run at launch and after every fetch of the turn group.
pub fn refresh(data_dir: &Path) {
    if !supported() {
        return;
    }
    let ticket = LOADED.ticket();
    let (root, disk) = disk(data_dir);
    if let Err(sentence) = &disk.chosen {
        tracing::warn!(%sentence, "voice: the turn models are refused");
    }
    match disk.load() {
        TurnLoad::Load(generation) => load(ticket, &root, generation),
        TurnLoad::Unload => {
            LOADED.publish(ticket, None);
        }
    }
}

/// Drop the loaded turn models and any load under way: the account is gone.
pub fn unload() {
    LOADED.clear();
}

/// The facts core decides on, read once: the models root and what is there.
fn disk(data_dir: &Path) -> (PathBuf, TurnDisk) {
    let root = crate::transcribe_ipc::models_root(data_dir);
    let chosen = chosen(data_dir, &root);
    let missing = match &chosen {
        Ok(set) => model_files::turn_missing(&root, set),
        Err(_) => Vec::new(),
    };
    let current = chosen.is_ok()
        && missing.is_empty()
        && crate::account_ipc::models_current(data_dir, &root, &turn_group(data_dir));
    let disk = TurnDisk {
        account: crate::account_ipc::descriptor().is_some(),
        chosen,
        missing,
        current,
        marker: keeper_sync::config_repo::completion_digest(&root, TURN_GROUP),
    };
    (root, disk)
}

/// The turn group as the account's clone and the picks name it now.
fn turn_group(data_dir: &Path) -> model_files::FetchGroup {
    let (_, repo) = crate::account_ipc::clone_models(data_dir);
    let (vad, smart_turn) = registry::get_turn_models(data_dir).unwrap_or_default();
    model_files::turn_group(&repo, &vad, &smart_turn)
}

/// The turn set to load from `root`: the hydrated `models.toml` with the
/// picks in place, or the refusal sentence.
fn chosen(data_dir: &Path, root: &Path) -> Result<model_files::ModelSet, String> {
    let repo = crate::transcribe_ipc::repo_model_set(root)?;
    let (vad, smart_turn) =
        registry::get_turn_models(data_dir).map_err(|error| error.to_string())?;
    model_files::choose_turn(root, repo, &vad, &smart_turn).map_err(|refused| refused.0)
}

/// The outcome of the published load, when it was of exactly `generation`.
fn loaded(root: &Path, generation: &TurnGeneration) -> Option<Result<(), String>> {
    LOADED.read(|loaded| {
        loaded
            .filter(|loaded| loaded.root == root && loaded.generation == *generation)
            .map(|loaded| loaded.models.as_ref().map(|_| ()).map_err(Clone::clone))
    })
}

/// Open `generation`'s two files and publish them under `ticket`. A load a
/// newer one overtook is dropped by the slot; one whose files a new
/// hydration began replacing meanwhile is dropped here, and that
/// hydration's own refresh loads what it brings.
fn load(ticket: Ticket, root: &Path, generation: TurnGeneration) {
    if loaded(root, &generation).is_some() {
        return;
    }
    let models = runtime::open(
        &root.join(&generation.vad_dir).join(MODEL_FILE),
        &root.join(&generation.smart_turn_dir).join(MODEL_FILE),
    );
    match &models {
        Ok(_) => tracing::info!(
            vad = generation.vad_dir,
            smart_turn = generation.smart_turn_dir,
            "voice: the turn models are loaded"
        ),
        Err(error) => tracing::warn!(%error, "voice: the turn models could not be loaded"),
    }
    let marker = keeper_sync::config_repo::completion_digest(root, TURN_GROUP);
    if marker.as_deref() != Some(generation.marker.as_str()) {
        tracing::info!("voice: the turn models changed while loading; not published");
        return;
    }
    let published = LOADED.publish(
        ticket,
        Some(Loaded {
            root: root.to_path_buf(),
            generation,
            models,
        }),
    );
    if !published {
        tracing::info!("voice: a newer turn models load overtook this one; not published");
    }
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "ios", target_arch = "aarch64", not(target_env = "sim"))
)))]
mod runtime {
    use std::path::Path;

    /// No runtime is linked here, so nothing is ever loaded.
    pub type Models = std::convert::Infallible;

    pub fn open(_vad: &Path, _smart_turn: &Path) -> Result<Models, String> {
        Err("ONNX Runtime is not part of this build".to_owned())
    }
}

#[cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "ios", target_arch = "aarch64", not(target_env = "sim"))
))]
mod runtime {
    use std::path::Path;

    use keeper_core::voice::turn_models::{
        spawn_turn_models, TurnGraph, TurnInference, TurnModelError, WorkerTurnModels,
        TURN_MEL_BINS, TURN_MEL_FRAMES, TURN_MODEL_RATE,
    };
    use ort::session::{Session, SessionOutputs};
    use ort::value::TensorRef;

    pub type Models = WorkerTurnModels;

    /// Both sessions from their files, on the CPU and one thread each: no
    /// execution provider is registered, so nothing shares the Neural Engine
    /// with transcription. The files are read by path only, and a graph whose
    /// tensors are not named as keeper feeds and reads them is refused here.
    pub fn open(vad: &Path, smart_turn: &Path) -> Result<Models, String> {
        let sessions = OrtSessions {
            vad: session(vad, TurnGraph::Vad)?,
            smart_turn: session(smart_turn, TurnGraph::SmartTurn)?,
        };
        spawn_turn_models(sessions).map_err(|error| error.0)
    }

    struct OrtSessions {
        vad: Session,
        smart_turn: Session,
    }

    impl TurnInference for OrtSessions {
        fn vad(&mut self, input: &[f32], state: &mut [f32]) -> Result<f32, TurnModelError> {
            run_vad(&mut self.vad, input, state)
        }

        fn turn(&mut self, features: &[f32]) -> Result<f32, TurnModelError> {
            run_turn(&mut self.smart_turn, features)
        }
    }

    fn session(path: &Path, graph: TurnGraph) -> Result<Session, String> {
        let failed = |error: ort::Error| format!("{}: {error}", path.display());
        let session = Session::builder()
            .map_err(failed)?
            .with_intra_threads(1)
            .map_err(|error| failed(error.into()))?
            .with_inter_threads(1)
            .map_err(|error| failed(error.into()))?
            .commit_from_file(path)
            .map_err(failed)?;
        graph
            .check(
                session.inputs().iter().map(|input| input.name()),
                session.outputs().iter().map(|output| output.name()),
            )
            .map_err(|refused| format!("{}: {refused}", path.display()))?;
        Ok(session)
    }

    /// The f32 values of output `name`; an absent name is an error, never a
    /// panic.
    fn output<'s>(
        outputs: &'s SessionOutputs<'_>,
        name: &str,
        model: &str,
    ) -> Result<&'s [f32], TurnModelError> {
        let value = outputs
            .get(name)
            .ok_or_else(|| TurnModelError(format!("{model}: no output \u{201c}{name}\u{201d}")))?;
        value
            .try_extract_tensor::<f32>()
            .map(|(_, values)| values)
            .map_err(|error| TurnModelError(format!("{model}: {error}")))
    }

    /// The sample rate input of the voice activity model, a scalar.
    static VAD_RATE: [i64; 1] = [TURN_MODEL_RATE as i64];

    const VAD: &str = "voice activity model";

    fn run_vad(
        session: &mut Session,
        input: &[f32],
        state: &mut [f32],
    ) -> Result<f32, TurnModelError> {
        let failed = |error: ort::Error| TurnModelError(format!("{VAD}: {error}"));
        let (inputs, names) = (TurnGraph::Vad.inputs(), TurnGraph::Vad.outputs());
        let outputs = session
            .run(ort::inputs![
                inputs[0] => TensorRef::from_array_view(([1usize, input.len()], input)).map_err(failed)?,
                inputs[1] => TensorRef::from_array_view(([2usize, 1, 128], &*state)).map_err(failed)?,
                inputs[2] => TensorRef::from_array_view(((), &VAD_RATE[..])).map_err(failed)?,
            ])
            .map_err(failed)?;
        let probability = output(&outputs, names[0], VAD)?;
        let next = output(&outputs, names[1], VAD)?;
        if next.len() != state.len() {
            return Err(TurnModelError(format!(
                "{VAD}: a state of {} values, not {}",
                next.len(),
                state.len()
            )));
        }
        state.copy_from_slice(next);
        probability
            .first()
            .copied()
            .ok_or_else(|| TurnModelError(format!("{VAD}: no probability")))
    }

    const SMART_TURN: &str = "end-of-turn model";

    fn run_turn(session: &mut Session, features: &[f32]) -> Result<f32, TurnModelError> {
        let failed = |error: ort::Error| TurnModelError(format!("{SMART_TURN}: {error}"));
        let outputs = session
            .run(ort::inputs![
                TurnGraph::SmartTurn.inputs()[0] => TensorRef::from_array_view(
                    ([1usize, TURN_MEL_BINS, TURN_MEL_FRAMES], features)
                ).map_err(failed)?,
            ])
            .map_err(failed)?;
        output(&outputs, TurnGraph::SmartTurn.outputs()[0], SMART_TURN)?
            .first()
            .copied()
            .ok_or_else(|| TurnModelError(format!("{SMART_TURN}: no score")))
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use keeper_core::voice::turn_models::{TurnModels, TURN_FEATURES, VAD_FRAME};

        use super::*;

        /// 97.1 #10, on real hardware with the real files: both sessions load
        /// from a hydrated models directory and the voice activity model tells
        /// silence from speech. The models are the organisation's and are not
        /// in the repository, so this runs by hand (on hesperia):
        /// `KEEPER_TEST_MODELS_DIR=<data_dir>/models KEEPER_TEST_SPEECH=<16 kHz
        /// mono PCM16 WAV of someone speaking> cargo test -p keeper --lib
        /// turn_models_load_and_answer -- --ignored`. Nothing is fetched.
        #[test]
        #[ignore = "needs the turn models in KEEPER_TEST_MODELS_DIR and a recording in KEEPER_TEST_SPEECH"]
        fn turn_models_load_and_answer() {
            let root = PathBuf::from(
                std::env::var("KEEPER_TEST_MODELS_DIR").expect("KEEPER_TEST_MODELS_DIR"),
            );
            let speech =
                std::fs::read(std::env::var("KEEPER_TEST_SPEECH").expect("KEEPER_TEST_SPEECH"))
                    .expect("the recording");
            let speech =
                keeper_core::transcription::wav_samples(&speech).expect("16 kHz mono PCM16");
            let set = keeper_core::transcription::models::ModelSet::from_toml(
                &std::fs::read_to_string(root.join("models.toml")).expect("models.toml"),
            )
            .expect("parses");
            let (vad, smart_turn) = (
                set.vad_dir.expect("[vad] in models.toml"),
                set.smart_turn_dir.expect("[smart_turn] in models.toml"),
            );
            let models = open(
                &root.join(vad).join(super::super::MODEL_FILE),
                &root.join(smart_turn).join(super::super::MODEL_FILE),
            )
            .expect("both sessions load");

            let peak = |samples: &[f32]| {
                let mut stream = models.vad();
                let (frames, _) = samples.as_chunks::<VAD_FRAME>();
                frames
                    .iter()
                    .map(|frame| stream.probability(frame).expect("scores"))
                    .fold(0.0_f32, f32::max)
            };
            let silence = peak(&[0.0; TURN_MODEL_RATE as usize]);
            assert!(silence < 0.1, "silence scored {silence}");
            let spoken = peak(&speech);
            assert!(spoken > 0.5, "speech scored {spoken}");

            let score = models
                .turn_complete(&[0.0; TURN_FEATURES])
                .expect("the end-of-turn model answers");
            assert!((0.0..=1.0).contains(&score), "{score}");
        }
    }
}
