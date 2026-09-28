//! The transcription command surface (Epic 87, AD-339–AD-350): a call site
//! over `keeper_core::transcription`.
//!
//! Registered on every target in the shared handler list. The engine is a
//! port: on macOS the FluidAudio worker in `transcribe_macos`, everywhere else
//! [`AbsentEngine`], which answers `Unsupported` — so the frontend never
//! special-cases a call, it reads `CapabilitiesVm.transcription`.
//!
//! # One job at a time
//!
//! Transcription jobs (a Files action, "Transcribe a file…", or the automatic
//! job a finished recording enqueues) share ONE worker thread and run in the
//! order they were asked for: the engine serializes its calls anyway, and two
//! jobs interleaving would only make both slower. A job started from the
//! surface streams [`TranscriptionProgressVm`] over its channel — heartbeats,
//! then exactly one terminal `done`/`failed`/`cancelled` batch — the
//! `export_start` shape. An automatic job has no channel; its progress is
//! logged.
//!
//! # The voices drive
//!
//! The bank and dictionary a transcript uses live in the enabled drive that
//! keeps voices and contains the media; media outside every such drive uses the
//! first enabled one; with none, speakers stay unknown and naming one is
//! refused with a sentence (AD-342).
//!
//! # Writes
//!
//! Every file this module writes — the transcript, its markdown twin, each
//! bank file — is written to a sibling temporary and renamed over the target,
//! so a synced drive never carries a torn file. Bank edits are planned by the
//! core (`BankPlan`) and only executed here: writes first, then deletes.
//!
//! The models are fetched from the account's config repository by
//! `account_ipc` (AD-341); this module only holds the state of that fetch.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use keeper_core::platform::Platform;
use keeper_core::recording::SessionManifest;
use keeper_core::registry;
use keeper_core::transcription::dictionary::{
    plan_accept_suggestion, plan_delete_term, plan_save_term,
};
use keeper_core::transcription::models::{self as model_files, MODELS_TOML};
use keeper_core::transcription::render;
use keeper_core::transcription::vm::{
    CorrectionResultVm, DictionaryTermVm, ModelsState, ModelsStateVm, PersonVm, TranscriptVm,
    TranscriptionPhase, TranscriptionProgressVm, TranscriptionStatusVm, VoicesDriveVm,
};
use keeper_core::transcription::{
    assemble, assign_speaker, edit_utterance, merge_speakers, plan_for_file, plan_for_session,
    reassign_utterance, rename_speaker_label, wav_bytes, wav_samples, AsrOutput, AssembleContext,
    AudioTrackInfo, Bank, BankError, BankPlan, DiarOutput, EngineError, EngineStamp,
    EngineUnavailable, ModelSet, PartResult, PartTrack, Person, SampleSource, SourceKind,
    SourcePart, SpeechEngine, TrackOrigin, TrackSelect, Transcript, TranscriptSource,
    TranscriptionLanguage, TranscriptionPlan,
};
use keeper_core::vm::{IpcError, IpcErrorCode};
use keeper_sync::SyncProfile;
use tauri::ipc::Channel;
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// A session folder is a folder holding this file (keeper-rec's ledger).
const SESSION_MANIFEST: &str = "manifest.json";

/// Where the hydrated models live inside the data directory (AD-341).
const MODELS_DIR: &str = "models";

// ---------------------------------------------------------------------------
// The engine port, per target
// ---------------------------------------------------------------------------

/// The engine of a build that has none, or whose engine could not start:
/// every call answers the one sentence `Unsupported` carries.
struct AbsentEngine;

impl AbsentEngine {
    fn refused<T>() -> Result<T, EngineError> {
        Err(EngineError(EngineUnavailable::Unsupported.sentence()))
    }
}

impl SpeechEngine for AbsentEngine {
    fn availability(&self) -> Result<(), EngineUnavailable> {
        Err(EngineUnavailable::Unsupported)
    }
    fn load(&self, _models_root: &Path, _set: &ModelSet) -> Result<(), EngineError> {
        Self::refused()
    }
    fn audio_tracks(&self, _media: &Path) -> Result<Vec<AudioTrackInfo>, EngineError> {
        Self::refused()
    }
    fn decode(
        &self,
        _media: &Path,
        _track: TrackSelect,
        _range: Option<(f64, f64)>,
    ) -> Result<Vec<f32>, EngineError> {
        Self::refused()
    }
    fn transcribe(
        &self,
        _samples: &[f32],
        _language: TranscriptionLanguage,
    ) -> Result<AsrOutput, EngineError> {
        Self::refused()
    }
    fn diarize(&self, _samples: &[f32]) -> Result<DiarOutput, EngineError> {
        Self::refused()
    }
    fn embed(&self, _samples: &[f32]) -> Result<Option<Vec<f32>>, EngineError> {
        Self::refused()
    }
}

/// The Mac's engine: one FluidAudio worker for the process. A worker that
/// cannot start leaves the Mac without transcription, said at error level.
#[cfg(target_os = "macos")]
fn platform_engine() -> Arc<dyn SpeechEngine> {
    static ENGINE: LazyLock<Arc<dyn SpeechEngine>> =
        LazyLock::new(|| match crate::transcribe_macos::MacSpeechEngine::new() {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                tracing::error!(%error, "transcription: the engine could not start");
                Arc::new(AbsentEngine)
            }
        });
    Arc::clone(&ENGINE)
}

#[cfg(not(target_os = "macos"))]
fn platform_engine() -> Arc<dyn SpeechEngine> {
    static ENGINE: LazyLock<Arc<dyn SpeechEngine>> = LazyLock::new(|| Arc::new(AbsentEngine));
    Arc::clone(&ENGINE)
}

/// Whether this machine can transcribe, asked once: the probe spawns
/// `sw_vers` on the Mac and the answer cannot change while keeper runs.
static AVAILABILITY: LazyLock<Result<(), EngineUnavailable>> =
    LazyLock::new(|| platform_engine().availability());

fn availability() -> &'static Result<(), EngineUnavailable> {
    &AVAILABILITY
}

/// `CapabilitiesVm.transcription` (AD-349).
pub fn transcription_supported() -> bool {
    availability().is_ok()
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A refusal whose sentence is the whole point, in the house envelope.
fn refused(message: impl Into<String>) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: message.into(),
        account_id: None,
        retriable: false,
    }
}

fn unsupported(reason: &EngineUnavailable) -> IpcError {
    IpcError {
        code: IpcErrorCode::Unsupported,
        message: reason.sentence(),
        account_id: None,
        retriable: false,
    }
}

fn data_dir(platform: &dyn Platform) -> Result<PathBuf, IpcError> {
    platform.data_dir().map_err(to_ipc_error)
}

fn models_root(data_dir: &Path) -> PathBuf {
    data_dir.join(MODELS_DIR)
}

/// The model set the hydrated `models.toml` names, or the default set when
/// there is none yet. A file that does not parse is refused, not guessed at.
fn model_set(models_root: &Path) -> Result<ModelSet, String> {
    match std::fs::read_to_string(models_root.join(MODELS_TOML)) {
        Ok(raw) => ModelSet::from_toml(&raw).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ModelSet::default()),
        Err(error) => Err(format!("{MODELS_TOML} could not be read: {error}")),
    }
}

/// The embedding model the bank is read under right now.
fn embedding_model(data_dir: &Path) -> String {
    model_set(&models_root(data_dir))
        .unwrap_or_default()
        .embedding_model
}

/// Load the models into the engine, refusing a set that is not all here or
/// that a hydration is part-way through replacing (it removes the completion
/// marker before its first change and writes it back last).
fn load_models(engine: &dyn SpeechEngine, data_dir: &Path) -> Result<ModelSet, String> {
    let root = models_root(data_dir);
    let set = model_set(&root)?;
    let missing = model_files::missing(&root, &set);
    if !missing.is_empty() {
        return Err(EngineUnavailable::ModelsMissing { missing }.sentence());
    }
    if keeper_sync::config_repo::completion_digest(&root).is_none() {
        return Err(MODELS_UPDATING.to_owned());
    }
    engine
        .load(&root, &set)
        .map_err(|error| error.to_string())?;
    Ok(set)
}

/// S2's readiness: every file of the set is here and it is what the
/// account's clone names now.
fn models_ready(data_dir: &Path) -> bool {
    let root = models_root(data_dir);
    model_set(&root).is_ok_and(|set| model_files::missing(&root, &set).is_empty())
        && crate::account_ipc::models_current(data_dir, &root)
}

/// A fresh staging name beside a target: the house shape keeper-sync never
/// commits, and one per write, so two writers to one target never rename
/// each other's half-written file.
fn staging_path(parent: &Path) -> PathBuf {
    parent.join(format!(".keeper.{}.tmp", ulid::Ulid::new()))
}

/// Write `bytes` beside `path` and rename it over: a reader — or a sync —
/// sees the old file or the new one, never a torn one.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = staging_path(parent);
    if let Err(error) = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// A bank-relative, `/`-separated path under `root`, refusing anything that
/// could leave it. The core only ever plans plain segments; this is the
/// executor not trusting a planner it did not write.
fn bank_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let relative = Path::new(rel);
    if rel.is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!("the voices bank refused the path {rel:?}"));
    }
    Ok(root.join(relative))
}

/// Execute a bank edit the core planned: every write, then every delete.
fn execute_plan(root: &Path, plan: &BankPlan) -> Result<(), String> {
    for write in &plan.writes {
        let path = bank_path(root, &write.rel_path)?;
        write_atomic(&path, &write.bytes)
            .map_err(|error| format!("{} could not be written: {error}", write.rel_path))?;
    }
    for delete in &plan.deletes {
        let path = bank_path(root, &delete.rel_path)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("{} could not be removed: {error}", delete.rel_path));
            }
        }
    }
    Ok(())
}

fn read_transcript(path: &Path) -> Result<Transcript, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    Transcript::from_json(&raw).map_err(|error| error.to_string())
}

/// The markdown twin of a transcript's JSON (AD-344).
fn markdown_path(json: &Path) -> PathBuf {
    let name = json
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.strip_suffix(".json").unwrap_or(&name);
    json.with_file_name(format!("{stem}.md"))
}

/// JSON first — it is the source of truth — then the markdown re-rendered.
fn write_transcript(json: &Path, md: &Path, transcript: &Transcript) -> Result<(), String> {
    let body = transcript.to_json().map_err(|error| error.to_string())?;
    write_atomic(json, body.as_bytes())
        .map_err(|error| format!("{} could not be written: {error}", json.display()))?;
    write_atomic(md, render::markdown(transcript).as_bytes())
        .map_err(|error| format!("{} could not be written: {error}", md.display()))
}

fn now_stamp() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

// ---------------------------------------------------------------------------
// Voices drives
// ---------------------------------------------------------------------------

/// A drive that keeps voices, resolved.
struct VoicesDrive {
    profile_id: String,
    name: String,
    local_path: PathBuf,
    root: PathBuf,
    subfolder: String,
}

impl VoicesDrive {
    fn of(profile: &SyncProfile) -> Option<Self> {
        let voices = profile.voices.as_ref()?;
        Some(Self {
            profile_id: profile.id.clone(),
            name: profile.name.clone(),
            local_path: profile.local_path.clone(),
            root: profile.voices_root()?,
            subfolder: voices.subfolder.clone(),
        })
    }
}

/// Every enabled drive that keeps voices, in the engine's order.
fn voices_drives(platform: &Arc<dyn Platform>) -> Vec<VoicesDrive> {
    match crate::sync::engine(Arc::clone(platform)) {
        Ok(engine) => drives_of(&engine),
        Err(error) => {
            tracing::debug!(%error, "transcription: no sync engine, so no voices drive");
            Vec::new()
        }
    }
}

fn drives_of(engine: &keeper_sync::engine::Engine) -> Vec<VoicesDrive> {
    let profiles = engine.list_profiles().unwrap_or_else(|error| {
        tracing::warn!(%error, "transcription: the drives could not be listed");
        Vec::new()
    });
    profiles
        .iter()
        .filter(|profile| profile.enabled)
        .filter_map(VoicesDrive::of)
        .collect()
}

/// The contract's rule: the deepest voices drive containing `path`, else the
/// first voices drive, else none.
fn drive_for_path(drives: Vec<VoicesDrive>, path: &Path) -> Option<VoicesDrive> {
    let containing = drives
        .iter()
        .enumerate()
        .filter(|(_, drive)| path.starts_with(&drive.local_path))
        .max_by_key(|(_, drive)| drive.local_path.components().count())
        .map(|(index, _)| index);
    let index = containing.unwrap_or(0);
    drives.into_iter().nth(index)
}

fn drive_by_id(platform: &Arc<dyn Platform>, profile_id: &str) -> Result<VoicesDrive, IpcError> {
    voices_drives(platform)
        .into_iter()
        .find(|drive| drive.profile_id == profile_id)
        .ok_or_else(|| refused("That drive does not keep voices (or is paused)."))
}

const NO_VOICES_DRIVE: &str = "No drive keeps voices yet. Turn on \u{201c}This folder keeps \
     voices\u{201d} for a synced folder in Settings \u{2192} Sync to name speakers.";

// ---------------------------------------------------------------------------
// Models state
// ---------------------------------------------------------------------------

/// What the last models fetch left behind. `Idle` means "look at the disk".
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fetch {
    Idle,
    Fetching,
    Failed(String),
    NoAccount,
}

static FETCH: Mutex<Fetch> = Mutex::new(Fetch::Idle);

const NO_ACCOUNT: &str = "The transcription models come from your account's settings \
     repository. Set up an account in Settings \u{2192} Account to fetch them.";

const MODELS_UPDATING: &str = "The transcription models on this Mac are not the ones your \
     account holds now; keeper brings them up to date after the next sync.";

fn models_vm(data_dir: &Path) -> ModelsStateVm {
    let root = models_root(data_dir);
    let missing = match model_set(&root) {
        Ok(set) => model_files::missing(&root, &set),
        Err(sentence) => {
            return ModelsStateVm {
                state: ModelsState::Failed,
                sentence,
                missing: Vec::new(),
            }
        }
    };
    let current = missing.is_empty() && crate::account_ipc::models_current(data_dir, &root);
    models_state(
        missing,
        current,
        lock(&FETCH).clone(),
        crate::account_ipc::descriptor().is_some(),
    )
}

/// Readiness first: a set that is all here and current is Ready even while
/// the fetch every sync starts is checking it again.
fn models_state(missing: Vec<String>, current: bool, fetch: Fetch, account: bool) -> ModelsStateVm {
    let (state, sentence) = match fetch {
        _ if missing.is_empty() && current => (
            ModelsState::Ready,
            "The transcription models are on this Mac.".to_owned(),
        ),
        Fetch::Fetching => (
            ModelsState::Fetching,
            "Fetching the transcription models from your account\u{2026}".to_owned(),
        ),
        Fetch::Failed(sentence) => (ModelsState::Failed, sentence),
        Fetch::NoAccount => (ModelsState::NoAccount, NO_ACCOUNT.to_owned()),
        Fetch::Idle if !account => (ModelsState::NoAccount, NO_ACCOUNT.to_owned()),
        Fetch::Idle if missing.is_empty() => (ModelsState::Missing, MODELS_UPDATING.to_owned()),
        Fetch::Idle => (
            ModelsState::Missing,
            EngineUnavailable::ModelsMissing {
                missing: missing.clone(),
            }
            .sentence(),
        ),
    };
    ModelsStateVm {
        state,
        sentence,
        missing,
    }
}

/// Bring the models up to date from the config repository, in the
/// background and one fetch at a time. A machine that cannot transcribe
/// fetches nothing: hundreds of megabytes it would never load.
pub fn spawn_models_fetch(platform: Arc<dyn Platform>) {
    if !transcription_supported() {
        return;
    }
    let dest = match platform.data_dir() {
        Ok(dir) => models_root(&dir),
        Err(error) => {
            tracing::warn!(%error, "transcription: no data directory for the models");
            return;
        }
    };
    let Some(fetching) = begin_fetch() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let _fetching = fetching;
        let next = match crate::account_ipc::hydrate_models(platform, dest).await {
            Ok(report) => {
                tracing::info!(
                    fetched = report.downloaded,
                    copied = report.copied,
                    kept = report.skipped,
                    bytes = report.downloaded_bytes + report.copied_bytes,
                    "transcription: the models are up to date"
                );
                Fetch::Idle
            }
            Err(crate::account_ipc::ModelsFetchError::NoAccount) => Fetch::NoAccount,
            Err(crate::account_ipc::ModelsFetchError::Failed(sentence)) => {
                tracing::warn!(%sentence, "transcription: the models could not be fetched");
                Fetch::Failed(sentence)
            }
        };
        *lock(&FETCH) = next;
    });
}

/// Marks a fetch under way; `None` when one already is.
fn begin_fetch() -> Option<FetchingGuard> {
    let mut fetch = lock(&FETCH);
    if *fetch == Fetch::Fetching {
        return None;
    }
    *fetch = Fetch::Fetching;
    Some(FetchingGuard)
}

/// Held by the fetch task: a task that panics, or is dropped with the
/// runtime, still lets the next fetch start instead of leaving `Fetching`
/// behind for good.
struct FetchingGuard;

impl Drop for FetchingGuard {
    fn drop(&mut self) {
        let mut fetch = lock(&FETCH);
        if *fetch == Fetch::Fetching {
            *fetch = Fetch::Idle;
        }
    }
}

fn status_vm(platform: &Arc<dyn Platform>) -> Result<TranscriptionStatusVm, IpcError> {
    let dir = data_dir(platform.as_ref())?;
    let reason = availability()
        .as_ref()
        .err()
        .map(EngineUnavailable::sentence);
    Ok(TranscriptionStatusVm {
        available: reason.is_none(),
        reason,
        models: models_vm(&dir),
        language: registry::get_transcription_language(&dir).map_err(to_ipc_error)?,
        after_recording: registry::get_transcription_after_recording(&dir).map_err(to_ipc_error)?,
        voices_drives: voices_drives(platform)
            .into_iter()
            .map(|drive| VoicesDriveVm {
                profile_id: drive.profile_id,
                name: drive.name,
                local_path: drive.local_path.to_string_lossy().into_owned(),
                voices_root: drive.root.to_string_lossy().into_owned(),
                subfolder: drive.subfolder,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

struct Job {
    id: String,
    target: PathBuf,
    /// `None` for an automatic job: its progress is logged only.
    channel: Option<Channel<TranscriptionProgressVm>>,
    cancel: Arc<AtomicBool>,
    platform: Arc<dyn Platform>,
}

struct Jobs {
    next_id: AtomicU64,
    /// Every job queued or running, by id — what a cancel finds.
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    queue: mpsc::Sender<Job>,
}

static JOBS: LazyLock<Jobs> = LazyLock::new(|| {
    let (queue, inbox) = mpsc::channel::<Job>();
    let spawned = std::thread::Builder::new()
        .name("keeper-transcription-jobs".into())
        .spawn(move || {
            for job in inbox {
                // A panicking job still ends in its one terminal batch, and
                // the worker lives on for the jobs queued behind it.
                let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&job)));
                if ran.is_err() {
                    job.report(
                        TranscriptionPhase::Failed,
                        0,
                        0,
                        Some("Transcription stopped unexpectedly.".to_owned()),
                        None,
                    );
                }
                lock(&JOBS.cancels).remove(&job.id);
            }
        });
    if let Err(error) = spawned {
        // The sender then has no receiver: every enqueue fails with a sentence.
        tracing::error!(%error, "transcription: the job worker could not start");
    }
    Jobs {
        next_id: AtomicU64::new(1),
        cancels: Mutex::new(HashMap::new()),
        queue,
    }
});

/// Queue a job; answers its id. Refused only when the worker is gone.
fn enqueue(
    platform: Arc<dyn Platform>,
    target: PathBuf,
    channel: Option<Channel<TranscriptionProgressVm>>,
) -> Result<String, String> {
    let jobs = &*JOBS;
    let id = format!(
        "transcribe-{}",
        jobs.next_id.fetch_add(1, Ordering::Relaxed)
    );
    let cancel = Arc::new(AtomicBool::new(false));
    lock(&jobs.cancels).insert(id.clone(), Arc::clone(&cancel));
    let job = Job {
        id: id.clone(),
        target,
        channel,
        cancel,
        platform,
    };
    job.report(TranscriptionPhase::Queued, 0, 0, None, None);
    if jobs.queue.send(job).is_err() {
        lock(&jobs.cancels).remove(&id);
        return Err("The transcription worker is not running; restart keeper.".to_owned());
    }
    Ok(id)
}

/// How a job stopped short.
enum Stop {
    Cancelled,
    Failed(String),
}

impl From<String> for Stop {
    fn from(sentence: String) -> Self {
        Self::Failed(sentence)
    }
}

impl From<EngineError> for Stop {
    fn from(error: EngineError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl Job {
    fn report(
        &self,
        phase: TranscriptionPhase,
        part: u32,
        parts: u32,
        message: Option<String>,
        transcript_path: Option<String>,
    ) {
        tracing::info!(
            job = %self.id,
            target = %self.target.display(),
            ?phase,
            part,
            parts,
            message = message.as_deref().unwrap_or(""),
            "transcription: progress"
        );
        if let Some(channel) = &self.channel {
            // A closed channel is a surface that went away; the job goes on.
            let _ = channel.send(TranscriptionProgressVm {
                job_id: self.id.clone(),
                phase,
                part,
                parts,
                message,
                transcript_path,
            });
        }
    }

    fn check(&self) -> Result<(), Stop> {
        if self.cancel.load(Ordering::SeqCst) {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Run one job to its one terminal batch.
fn run_job(job: &Job) {
    match transcribe(job) {
        Ok(path) => job.report(
            TranscriptionPhase::Done,
            0,
            0,
            None,
            Some(path.to_string_lossy().into_owned()),
        ),
        Err(Stop::Cancelled) => job.report(TranscriptionPhase::Cancelled, 0, 0, None, None),
        Err(Stop::Failed(sentence)) => {
            tracing::warn!(job = %job.id, %sentence, "transcription: the job failed");
            job.report(TranscriptionPhase::Failed, 0, 0, Some(sentence), None);
        }
    }
}

/// Every read-modify-write of a transcript — each correction, each assign —
/// and a job's final write run under this, so none of them writes over a
/// change another made between its read and its write.
static TRANSCRIPT_WRITES: Mutex<()> = Mutex::new(());

/// Q2: a transcript somebody corrected is theirs; an untouched one is ours
/// to replace.
fn refuse_overwrite(out_json: &Path) -> Result<(), Stop> {
    if !out_json.is_file() {
        return Ok(());
    }
    let existing = read_transcript(out_json).map_err(|error| {
        Stop::Failed(format!(
            "{error} keeper will not overwrite a transcript it cannot read. Delete or rename \
             the transcript to transcribe again."
        ))
    })?;
    if existing.is_corrected() {
        return Err(Stop::Failed(
            "This transcript has corrections in it, so keeper will not overwrite it. Delete or \
             rename the transcript to transcribe again."
                .to_owned(),
        ));
    }
    Ok(())
}

/// Q5: every bank clip without an embedding for the current model gets one,
/// so a model change recognizes people from their kept clips.
fn re_embed_bank(engine: &dyn SpeechEngine, root: &Path, bank: &Bank, model: &str) -> bool {
    let mut wrote = false;
    for (person, clip, rel) in bank.missing_embeddings(model) {
        let embedded = bank_path(root, &rel)
            .and_then(|path| std::fs::read(&path).map_err(|error| error.to_string()))
            .and_then(|bytes| wav_samples(&bytes).map_err(|error| error.to_string()))
            .and_then(|samples| engine.embed(&samples).map_err(|error| error.to_string()));
        let plan = match embedded {
            Ok(Some(vector)) => bank.plan_embedding(&person, &clip, vector, model),
            Ok(None) => {
                tracing::info!(%rel, "transcription: a bank clip is too short to embed");
                continue;
            }
            Err(error) => {
                tracing::warn!(%rel, %error, "transcription: a bank clip could not be embedded");
                continue;
            }
        };
        match plan
            .map_err(|error| error.to_string())
            .and_then(|plan| execute_plan(root, &only_absent(root, plan)))
        {
            Ok(()) => wrote = true,
            Err(error) => {
                tracing::warn!(%rel, %error, "transcription: a bank embedding was not stored");
            }
        }
    }
    wrote
}

/// A re-embed plan without the files already there: the core plans an
/// embedding deterministically, so one another device wrote first is the
/// same file, and rewriting it would only churn the sync.
fn only_absent(root: &Path, mut plan: BankPlan) -> BankPlan {
    plan.writes
        .retain(|write| !bank_path(root, &write.rel_path).is_ok_and(|path| path.exists()));
    plan
}

/// The job itself: plan, load, hear every part, match, assemble, write.
fn transcribe(job: &Job) -> Result<PathBuf, Stop> {
    let engine = platform_engine();
    if let Err(reason) = availability() {
        return Err(Stop::Failed(reason.sentence()));
    }
    let data_dir = job
        .platform
        .data_dir()
        .map_err(|error| Stop::Failed(error.to_string()))?;

    let is_session = job.target.is_dir() && job.target.join(SESSION_MANIFEST).is_file();
    let plan = if is_session {
        let manifest =
            SessionManifest::load(&job.target).map_err(|error| Stop::Failed(error.to_string()))?;
        plan_for_session(&job.target, &manifest)
    } else {
        plan_for_file(&job.target)
    }
    .map_err(|refusal| Stop::Failed(refusal.to_string()))?;
    refuse_overwrite(&plan.out_json)?;
    job.check()?;

    let parts = u32::try_from(plan.parts.len()).unwrap_or(u32::MAX);
    let drive = drive_for_path(voices_drives(&job.platform), &job.target);
    let set = load_models(engine.as_ref(), &data_dir)?;
    let language = registry::get_transcription_language(&data_dir).unwrap_or_default();

    let mut heard: Vec<PartResult> = Vec::new();
    let mut source_parts: Vec<SourcePart> = Vec::with_capacity(plan.parts.len());
    let mut offset = 0.0_f64;
    for (index, part) in plan.parts.iter().enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        job.check()?;
        job.report(TranscriptionPhase::Decoding, number, parts, None, None);
        let probed = engine.audio_tracks(&part.file)?;
        let duration = probed
            .iter()
            .map(|track| track.duration_s)
            .fold(0.0_f64, f64::max);
        let roles = part.resolve(&probed);
        for (track, origin) in &roles {
            job.check()?;
            let samples = engine.decode(&part.file, *track, None)?;
            job.check()?;
            job.report(TranscriptionPhase::Transcribing, number, parts, None, None);
            let asr = engine.transcribe(&samples, language)?;
            // The microphone beside system audio is the person recording
            // (AD-345): nothing to diarize.
            let diar = if *origin == TrackOrigin::Microphone {
                None
            } else {
                job.check()?;
                job.report(TranscriptionPhase::Diarizing, number, parts, None, None);
                Some(engine.diarize(&samples)?)
            };
            heard.push(PartResult {
                offset,
                origin: *origin,
                asr,
                diar,
            });
        }
        source_parts.push(SourcePart {
            file: part.relative_name(),
            offset,
            duration,
            tracks: roles
                .iter()
                .map(|(track, origin)| PartTrack {
                    track: track.index(),
                    origin: *origin,
                })
                .collect(),
        });
        offset += duration;
    }

    job.check()?;
    job.report(TranscriptionPhase::Matching, parts, parts, None, None);
    let bank = drive.as_ref().map(|drive| {
        let bank = Bank::load(&drive.root);
        for warning in &bank.warnings {
            tracing::warn!(%warning, drive = %drive.name, "transcription: a bank file was skipped");
        }
        if re_embed_bank(engine.as_ref(), &drive.root, &bank, &set.embedding_model) {
            Bank::load(&drive.root)
        } else {
            bank
        }
    });
    let terms = bank.as_ref().map_or(&[][..], |bank| &bank.terms[..]);
    let transcript = assemble(
        &heard,
        bank.as_ref(),
        terms,
        AssembleContext {
            source: TranscriptSource {
                kind: plan.source,
                files: source_parts.iter().map(|part| part.file.clone()).collect(),
                parts: source_parts,
                title: plan_title(&plan),
            },
            created_at: now_stamp(),
            engine: EngineStamp {
                asr: set.asr_dir.clone(),
                diarizer: set.diarizer_dir.clone(),
                embedding: set.embedding_model.clone(),
            },
            language,
        },
    );

    job.check()?;
    job.report(TranscriptionPhase::Writing, parts, parts, None, None);
    write_fresh_transcript(&plan.out_json, &plan.out_md, &transcript)?;
    Ok(plan.out_json)
}

/// A job's write: under [`TRANSCRIPT_WRITES`], and only if nobody corrected
/// the transcript while the job was hearing it — the check at the job's
/// start is minutes old by now.
fn write_fresh_transcript(json: &Path, md: &Path, transcript: &Transcript) -> Result<(), Stop> {
    let _writing = lock(&TRANSCRIPT_WRITES);
    refuse_overwrite(json)?;
    write_transcript(json, md, transcript).map_err(Stop::Failed)
}

/// The after-recording hook (AD-348), from `RecordingSink::finalize`: when
/// the setting is on, this Mac can transcribe, the models are here and the
/// session's drive keeps voices, the session is queued. Never blocks the
/// caller and never fails it — every "no" is a log line, and a session that
/// was not transcribed is not retried (the Files action does it).
pub fn after_recording(platform: Arc<dyn Platform>, folder: PathBuf, profile_id: Option<String>) {
    if !transcription_supported() {
        return;
    }
    let Some(profile_id) = profile_id else {
        tracing::info!("transcription: this session is not on a drive, so it is not transcribed");
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(dir) = platform.data_dir() else {
            return;
        };
        match registry::get_transcription_after_recording(&dir) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(%error, "transcription: the after-recording setting could not be read");
                return;
            }
        }
        // The engine the session committed through is already open; the hook
        // never opens one of its own.
        let keeps_voices = crate::sync::engine_if_open().is_some_and(|engine| {
            drives_of(&engine)
                .iter()
                .any(|drive| drive.profile_id == profile_id)
        });
        if !keeps_voices {
            tracing::info!(
                profile = %profile_id,
                "transcription: this session's drive keeps no voices, so it is not transcribed"
            );
            return;
        }
        if !models_ready(&dir) {
            tracing::info!(
                "transcription: the models are not on this Mac, so the session is not transcribed"
            );
            return;
        }
        match enqueue(Arc::clone(&platform), folder, None) {
            Ok(id) => tracing::info!(job = %id, "transcription: the finished session is queued"),
            Err(sentence) => tracing::warn!(%sentence, "transcription: the session was not queued"),
        }
    });
}

// ---------------------------------------------------------------------------
// Commands: status, models, settings, jobs
// ---------------------------------------------------------------------------

/// Run a command's body on the blocking pool (AD-34-5): every one of them
/// reads the registry, sync.db or a voices bank — which may sit on an
/// external volume — and none may freeze the window while it does.
async fn off_main<T: Send + 'static>(
    body: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    crate::ipc::off_async_runtime(body).await?
}

/// Settings → Transcription.
#[tauri::command]
pub async fn transcription_status(
    state: State<'_, AppState>,
) -> Result<TranscriptionStatusVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || status_vm(&platform)).await
}

/// Fetch (or bring up to date) the models from the account's config
/// repository; answers the status with the fetch under way.
#[tauri::command]
pub async fn transcription_models_fetch(
    state: State<'_, AppState>,
) -> Result<TranscriptionStatusVm, IpcError> {
    if let Err(reason) = availability() {
        return Err(unsupported(reason));
    }
    if crate::account_ipc::descriptor().is_none() {
        *lock(&FETCH) = Fetch::NoAccount;
    } else {
        spawn_models_fetch(Arc::clone(&state.platform));
    }
    let platform = Arc::clone(&state.platform);
    off_main(move || status_vm(&platform)).await
}

/// Write the two transcription settings through the registry, where the
/// account's settings sync observes them. `None` leaves a setting alone.
#[tauri::command]
pub async fn transcription_settings_set(
    state: State<'_, AppState>,
    language: Option<TranscriptionLanguage>,
    after_recording: Option<bool>,
) -> Result<TranscriptionStatusVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        let dir = data_dir(platform.as_ref())?;
        if let Some(language) = language {
            registry::set_transcription_language(&dir, language).map_err(to_ipc_error)?;
        }
        if let Some(enabled) = after_recording {
            registry::set_transcription_after_recording(&dir, enabled).map_err(to_ipc_error)?;
        }
        status_vm(&platform)
    })
    .await
}

/// Transcribe a media file or a recording session folder (absolute path).
/// Answers the job id at once; everything after streams over `channel`,
/// ending in exactly one terminal batch.
#[tauri::command]
pub fn transcription_start(
    state: State<'_, AppState>,
    path: String,
    channel: Channel<TranscriptionProgressVm>,
) -> Result<String, IpcError> {
    if let Err(reason) = availability() {
        return Err(unsupported(reason));
    }
    enqueue(
        Arc::clone(&state.platform),
        PathBuf::from(path),
        Some(channel),
    )
    .map_err(refused)
}

/// Cancel a queued or running job. Unknown or finished ids are not an error:
/// the surface can race a cancel against the job's own end.
#[tauri::command]
pub fn transcription_cancel(job_id: String) -> Result<(), IpcError> {
    if let Some(cancel) = lock(&JOBS.cancels).get(&job_id) {
        cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands: the transcript viewer
// ---------------------------------------------------------------------------

/// The viewer's model for the transcript at `path`, with the people of the
/// voices drive it belongs to.
fn transcript_vm(
    platform: &Arc<dyn Platform>,
    path: &Path,
    transcript: Transcript,
) -> TranscriptVm {
    let people = drive_for_path(voices_drives(platform), path)
        .map(|drive| PersonVm::list(&Bank::load(&drive.root), &transcript.engine.embedding))
        .unwrap_or_default();
    TranscriptVm {
        path: path.to_string_lossy().into_owned(),
        transcript,
        people,
    }
}

/// Read, correct, save — under [`TRANSCRIPT_WRITES`]: the one shape every
/// correction takes. `R` is whatever an edit answers beside the transcript.
fn correct<R>(
    platform: &Arc<dyn Platform>,
    path: &Path,
    edit: impl FnOnce(Transcript) -> Result<(Transcript, R), String>,
) -> Result<(TranscriptVm, R), IpcError> {
    let (corrected, answer) = {
        let _writing = lock(&TRANSCRIPT_WRITES);
        let transcript = read_transcript(path).map_err(refused)?;
        let (corrected, answer) = edit(transcript).map_err(refused)?;
        write_transcript(path, &markdown_path(path), &corrected).map_err(refused)?;
        (corrected, answer)
    };
    Ok((transcript_vm(platform, path, corrected), answer))
}

/// A correction that answers only the transcript.
fn correct_only(
    platform: &Arc<dyn Platform>,
    path: &Path,
    edit: impl FnOnce(Transcript) -> Result<Transcript, String>,
) -> Result<TranscriptVm, IpcError> {
    correct(platform, path, |t| edit(t).map(|t| (t, ()))).map(|(vm, ())| vm)
}

#[tauri::command]
pub async fn transcript_read(
    state: State<'_, AppState>,
    path: String,
) -> Result<TranscriptVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        let path = PathBuf::from(path);
        let transcript = read_transcript(&path).map_err(refused)?;
        Ok(transcript_vm(&platform, &path, transcript))
    })
    .await
}

/// Replace a line's text; answers the one-word substitutions the dictionary
/// could learn. Never touches the bank (AD-347).
#[tauri::command]
pub async fn transcript_edit_utterance(
    state: State<'_, AppState>,
    path: String,
    utterance_id: String,
    text: String,
) -> Result<CorrectionResultVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        let (transcript, suggestions) = correct(&platform, Path::new(&path), |t| {
            edit_utterance(t, &utterance_id, &text).map_err(|error| error.to_string())
        })?;
        Ok(CorrectionResultVm {
            transcript,
            suggestions,
        })
    })
    .await
}

#[tauri::command]
pub async fn transcript_reassign_utterance(
    state: State<'_, AppState>,
    path: String,
    utterance_id: String,
    speaker_id: String,
) -> Result<TranscriptVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        correct_only(&platform, Path::new(&path), |t| {
            reassign_utterance(t, &utterance_id, &speaker_id).map_err(|error| error.to_string())
        })
    })
    .await
}

#[tauri::command]
pub async fn transcript_merge_speakers(
    state: State<'_, AppState>,
    path: String,
    from_id: String,
    into_id: String,
) -> Result<TranscriptVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        correct_only(&platform, Path::new(&path), |t| {
            merge_speakers(t, &from_id, &into_id).map_err(|error| error.to_string())
        })
    })
    .await
}

/// Name a speaker in this transcript only; an empty label clears it. No bank
/// write — naming a person is [`transcript_assign_speaker`].
#[tauri::command]
pub async fn transcript_rename_speaker(
    state: State<'_, AppState>,
    path: String,
    speaker_id: String,
    label: String,
) -> Result<TranscriptVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        correct_only(&platform, Path::new(&path), |t| {
            rename_speaker_label(t, &speaker_id, &label).map_err(|error| error.to_string())
        })
    })
    .await
}

/// The transcript's heading: a session's folder name, or the file's name.
fn plan_title(plan: &TranscriptionPlan) -> Option<String> {
    let named = match plan.source {
        SourceKind::Recording => plan.out_json.parent()?.file_name(),
        SourceKind::File => plan.parts.first()?.file.file_name(),
    };
    named.map(|name| name.to_string_lossy().into_owned())
}

/// Who a confirmed speaker is: a person already in the bank, or one to make.
enum Naming {
    Existing(String),
    New(String),
}

/// A person confirmed who a speaker is (AD-347): an existing person
/// (`person_id`) or a new one (`new_name`). The speaker's best clip and its
/// embedding go into the bank, then the transcript records the person.
#[tauri::command]
pub async fn transcript_assign_speaker(
    state: State<'_, AppState>,
    path: String,
    speaker_id: String,
    person_id: Option<String>,
    new_name: Option<String>,
) -> Result<TranscriptVm, IpcError> {
    let naming = match (new_name.as_deref().map(str::trim), person_id) {
        (Some(name), _) if !name.is_empty() => Naming::New(name.to_owned()),
        (_, Some(id)) => Naming::Existing(id),
        _ => return Err(refused("Choose a person, or give the speaker a name.")),
    };
    let platform = Arc::clone(&state.platform);
    off_main(move || assign(&platform, Path::new(&path), &speaker_id, naming)).await
}

fn assign(
    platform: &Arc<dyn Platform>,
    path: &Path,
    speaker_id: &str,
    naming: Naming,
) -> Result<TranscriptVm, IpcError> {
    let drive =
        drive_for_path(voices_drives(platform), path).ok_or_else(|| refused(NO_VOICES_DRIVE))?;
    let snapshot = read_transcript(path).map_err(refused)?;
    let speaker = snapshot.speaker(speaker_id).cloned().ok_or_else(|| {
        refused(format!(
            "That speaker is no longer in the transcript ({speaker_id})."
        ))
    })?;
    // Cut (and embed) before anything is written: a clip that cannot be had
    // costs the bank a sample, never a half-made person.
    let sample = cut_sample(platform, &drive, path, &speaker);
    let (person, plan) = plan_assignment(
        Bank::load(&drive.root),
        naming,
        sample,
        &snapshot.engine.embedding,
        speaker.origin == TrackOrigin::Microphone,
    )?;
    let assigned = commit_assignment(path, speaker_id, &person, &drive.root, &plan)?;
    Ok(transcript_vm(platform, path, assigned))
}

/// A speaker's voice, cut from its media before the bank is touched.
struct CutSample {
    wav: Vec<u8>,
    /// `None` while a transcription job holds the engine: the clip is kept
    /// alone and the next job's re-embed (Q5) gives it its embedding.
    embedding: Option<Vec<f32>>,
    source: SampleSource,
}

/// The one bank edit an assign makes: the person (made, when new) and the
/// sample, planned together so they land together. A sample the bank
/// refuses is logged and left out; the person still lands. Confirming the
/// microphone's speaker names the person recording, so when the bank has no
/// one marked as me yet, that person becomes me in the same plan.
fn plan_assignment(
    mut bank: Bank,
    naming: Naming,
    sample: Option<CutSample>,
    model: &str,
    microphone: bool,
) -> Result<(Person, BankPlan), IpcError> {
    let (person, mut plan) = match naming {
        Naming::New(name) => {
            let (person, plan) = bank
                .create_person(&name)
                .map_err(|error| refused(error.to_string()))?;
            // The sample is planned for the person before its file exists.
            bank.people.push(person.clone());
            (person, plan)
        }
        Naming::Existing(id) => {
            let person = bank
                .person(&id)
                .cloned()
                .ok_or_else(|| refused(format!("Nobody with id {id} is in the voices bank.")))?;
            (person, BankPlan::default())
        }
    };
    if let Some(sample) = sample {
        let planned = match sample.embedding {
            Some(vector) => bank.add_sample(&person.id, sample.wav, vector, model, sample.source),
            None => bank.add_clip_only(&person.id, sample.wav, sample.source),
        };
        match planned {
            Ok((_, sample_plan)) => {
                plan.writes.extend(sample_plan.writes);
                plan.deletes.extend(sample_plan.deletes);
            }
            Err(error) => {
                tracing::warn!(%error, person = %person.id, "transcription: the voice sample was not stored");
            }
        }
    }
    if microphone && !bank.people.iter().any(|someone| someone.is_self) {
        match bank.set_self(&person.id) {
            Ok(self_plan) => {
                plan.writes.extend(self_plan.writes);
                plan.deletes.extend(self_plan.deletes);
            }
            Err(error) => {
                tracing::warn!(%error, person = %person.id, "transcription: the person was not marked as me");
            }
        }
    }
    Ok((person, plan))
}

/// The transcript half of an assign, under [`TRANSCRIPT_WRITES`]: the
/// transcript is read again, so an edit that landed while the clip was being
/// cut is kept, and a speaker merged away meanwhile writes nothing — neither
/// the bank nor the transcript.
fn commit_assignment(
    path: &Path,
    speaker_id: &str,
    person: &Person,
    bank_root: &Path,
    plan: &BankPlan,
) -> Result<Transcript, IpcError> {
    let _writing = lock(&TRANSCRIPT_WRITES);
    let fresh = read_transcript(path).map_err(refused)?;
    let assigned =
        assign_speaker(fresh, speaker_id, person).map_err(|error| refused(error.to_string()))?;
    execute_plan(bank_root, plan).map_err(refused)?;
    write_transcript(path, &markdown_path(path), &assigned).map_err(refused)?;
    Ok(assigned)
}

/// Whether a transcription job is queued or running.
fn jobs_busy() -> bool {
    !lock(&JOBS.cancels).is_empty()
}

/// The speaker's clip, cut from the media it was heard in, with its
/// embedding (the transcript's, or the engine's for the microphone and any
/// speaker without one). Every "cannot" — no clip long enough, a machine
/// that cannot decode, media moved or turned into a pointer, a clip too
/// quiet — is a log line and no sample: the name still lands.
fn cut_sample(
    platform: &Arc<dyn Platform>,
    drive: &VoicesDrive,
    transcript_path: &Path,
    speaker: &keeper_core::transcription::Speaker,
) -> Option<CutSample> {
    let Some(clip) = speaker.clip.as_ref() else {
        tracing::info!(speaker = %speaker.id, "transcription: no clip long enough to keep for this speaker");
        return None;
    };
    if availability().is_err() {
        tracing::info!(
            "transcription: this machine cannot cut a voice clip; the bank is unchanged"
        );
        return None;
    }
    let engine = platform_engine();
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new(""));
    let media = dir.join(&clip.file);
    let track = clip.track.map_or(TrackSelect::MixAll, TrackSelect::Index);
    let samples = match engine.decode(&media, track, Some((clip.start, clip.end))) {
        Ok(samples) => samples,
        Err(error) => {
            tracing::warn!(speaker = %speaker.id, %error, "transcription: the voice clip could not be cut; the bank is unchanged");
            return None;
        }
    };
    let embedding = match &speaker.embedding {
        Some(vector) => Some(vector.clone()),
        // The engine is one queue: embedding now would wait behind the job.
        None if jobs_busy() => {
            tracing::info!(speaker = %speaker.id, "transcription: a job holds the engine; the clip is kept and embedded by the next job");
            None
        }
        None => match embed_now(engine.as_ref(), platform, &samples) {
            Ok(Some(vector)) => Some(vector),
            Ok(None) => {
                tracing::info!(speaker = %speaker.id, "transcription: the clip is too quiet to recognize anyone by");
                return None;
            }
            Err(error) => {
                tracing::warn!(speaker = %speaker.id, %error, "transcription: the voice clip could not be embedded; the bank is unchanged");
                return None;
            }
        },
    };
    Some(CutSample {
        wav: wav_bytes(&samples),
        embedding,
        source: SampleSource {
            transcript: drive_relative(&drive.local_path, transcript_path),
            start: clip.start,
            end: clip.end,
        },
    })
}

fn embed_now(
    engine: &dyn SpeechEngine,
    platform: &Arc<dyn Platform>,
    samples: &[f32],
) -> Result<Option<Vec<f32>>, String> {
    let dir = platform.data_dir().map_err(|error| error.to_string())?;
    load_models(engine, &dir)?;
    engine.embed(samples).map_err(|error| error.to_string())
}

/// `path` relative to the drive, `/`-joined; the bare name when it is not
/// inside the drive (media from another place, a bank on the first drive).
fn drive_relative(local_path: &Path, path: &Path) -> String {
    match path.strip_prefix(local_path) {
        Ok(relative) => relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Commands: the voices bank and the dictionary
// ---------------------------------------------------------------------------

fn people(platform: &Arc<dyn Platform>, drive: &VoicesDrive) -> Result<Vec<PersonVm>, IpcError> {
    let model = embedding_model(&data_dir(platform.as_ref())?);
    Ok(PersonVm::list(&Bank::load(&drive.root), &model))
}

/// Plan one bank edit off the main thread, execute it, answer the people as
/// they now are.
async fn edit_people(
    platform: Arc<dyn Platform>,
    profile_id: String,
    plan: impl FnOnce(&Bank) -> Result<BankPlan, BankError> + Send + 'static,
) -> Result<Vec<PersonVm>, IpcError> {
    off_main(move || {
        let drive = drive_by_id(&platform, &profile_id)?;
        let bank = Bank::load(&drive.root);
        let plan = plan(&bank).map_err(|error| refused(error.to_string()))?;
        execute_plan(&drive.root, &plan).map_err(refused)?;
        people(&platform, &drive)
    })
    .await
}

#[tauri::command]
pub async fn voices_people(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Vec<PersonVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || {
        let drive = drive_by_id(&platform, &profile_id)?;
        people(&platform, &drive)
    })
    .await
}

#[tauri::command]
pub async fn voices_person_rename(
    state: State<'_, AppState>,
    profile_id: String,
    person_id: String,
    name: String,
) -> Result<Vec<PersonVm>, IpcError> {
    edit_people(Arc::clone(&state.platform), profile_id, move |bank| {
        bank.rename_person(&person_id, &name)
    })
    .await
}

#[tauri::command]
pub async fn voices_person_delete(
    state: State<'_, AppState>,
    profile_id: String,
    person_id: String,
) -> Result<Vec<PersonVm>, IpcError> {
    edit_people(Arc::clone(&state.platform), profile_id, move |bank| {
        bank.delete_person(&person_id)
    })
    .await
}

#[tauri::command]
pub async fn voices_person_set_self(
    state: State<'_, AppState>,
    profile_id: String,
    person_id: String,
) -> Result<Vec<PersonVm>, IpcError> {
    edit_people(Arc::clone(&state.platform), profile_id, move |bank| {
        bank.set_self(&person_id)
    })
    .await
}

#[tauri::command]
pub async fn voices_people_merge(
    state: State<'_, AppState>,
    profile_id: String,
    from_id: String,
    into_id: String,
) -> Result<Vec<PersonVm>, IpcError> {
    edit_people(Arc::clone(&state.platform), profile_id, move |bank| {
        bank.merge_people(&from_id, &into_id)
    })
    .await
}

fn terms(drive: &VoicesDrive) -> Vec<DictionaryTermVm> {
    Bank::load(&drive.root)
        .terms
        .iter()
        .map(DictionaryTermVm::from)
        .collect()
}

/// Plan one dictionary edit off the main thread, execute it, answer the
/// terms as they now are.
async fn edit_terms(
    platform: Arc<dyn Platform>,
    profile_id: String,
    plan: impl FnOnce(&Bank) -> Result<BankPlan, BankError> + Send + 'static,
) -> Result<Vec<DictionaryTermVm>, IpcError> {
    off_main(move || {
        let drive = drive_by_id(&platform, &profile_id)?;
        let bank = Bank::load(&drive.root);
        let plan = plan(&bank).map_err(|error| refused(error.to_string()))?;
        execute_plan(&drive.root, &plan).map_err(refused)?;
        Ok(terms(&drive))
    })
    .await
}

#[tauri::command]
pub async fn dictionary_terms(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Vec<DictionaryTermVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    off_main(move || Ok(terms(&drive_by_id(&platform, &profile_id)?))).await
}

#[tauri::command]
pub async fn dictionary_term_save(
    state: State<'_, AppState>,
    profile_id: String,
    id: Option<String>,
    text: String,
    aliases: Vec<String>,
) -> Result<Vec<DictionaryTermVm>, IpcError> {
    edit_terms(Arc::clone(&state.platform), profile_id, move |bank| {
        plan_save_term(&bank.terms, id.as_deref(), &text, &aliases).map(|(_, plan)| plan)
    })
    .await
}

#[tauri::command]
pub async fn dictionary_term_delete(
    state: State<'_, AppState>,
    profile_id: String,
    id: String,
) -> Result<Vec<DictionaryTermVm>, IpcError> {
    edit_terms(Arc::clone(&state.platform), profile_id, move |bank| {
        plan_delete_term(&bank.terms, &id)
    })
    .await
}

#[tauri::command]
pub async fn dictionary_accept_suggestion(
    state: State<'_, AppState>,
    profile_id: String,
    from: String,
    to: String,
) -> Result<Vec<DictionaryTermVm>, IpcError> {
    edit_terms(Arc::clone(&state.platform), profile_id, move |bank| {
        plan_accept_suggestion(&bank.terms, &from, &to).map(|(_, plan)| plan)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(id: &str, local: &str) -> VoicesDrive {
        VoicesDrive {
            profile_id: id.to_owned(),
            name: id.to_owned(),
            local_path: PathBuf::from(local),
            root: PathBuf::from(local).join("voices"),
            subfolder: "voices".to_owned(),
        }
    }

    #[test]
    fn media_uses_the_deepest_voices_drive_holding_it_else_the_first() {
        let drives = || {
            vec![
                drive("tgdrive", "/Volumes/merope/tgdrive"),
                drive("neura", "/Volumes/merope/neuradrive"),
                drive("nested", "/Volumes/merope/neuradrive/70-comms"),
            ]
        };
        let pick =
            |path: &str| drive_for_path(drives(), Path::new(path)).map(|drive| drive.profile_id);
        assert_eq!(
            pick("/Volumes/merope/neuradrive/40-media/call.mov").as_deref(),
            Some("neura")
        );
        assert_eq!(
            pick("/Volumes/merope/neuradrive/70-comms/meetings/a/transcript.json").as_deref(),
            Some("nested")
        );
        assert_eq!(
            pick("/Users/t/Downloads/talk.mp3").as_deref(),
            Some("tgdrive")
        );
        // A sibling whose name only starts like a drive is not inside it.
        assert_eq!(
            pick("/Volumes/merope/tgdrive-light/x.m4a").as_deref(),
            Some("tgdrive")
        );
        assert!(drive_for_path(Vec::new(), Path::new("/x.mov")).is_none());
    }

    #[test]
    fn the_bank_executor_refuses_a_path_that_could_leave_the_bank() {
        let root = Path::new("/drive/voices");
        assert_eq!(
            bank_path(root, "people/01J.json").expect("plain"),
            root.join("people/01J.json")
        );
        for bad in [
            "",
            "../people/x.json",
            "/etc/passwd",
            "people/../../x",
            "./x",
        ] {
            assert!(bank_path(root, bad).is_err(), "{bad:?} must be refused");
        }
    }

    const UNTOUCHED: &str = r#"{"version":1,"source":{"kind":"file","files":["call.mov"]},"createdAt":"","engine":{"asr":"a","diarizer":"d","embedding":"e"},"language":"auto","duration":1,"speakers":[{"id":"S1","origin":"mixed","personId":null,"name":null,"status":"auto","score":0.8,"candidates":[],"embedding":null,"clip":null}],"utterances":[{"id":"u1","speaker":"S1","start":0,"end":1,"text":"hi","asrText":"hi","edited":false,"words":[]}],"dictionaryApplied":[]}"#;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keeper-transcribe-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_corrected_transcript_is_refused_and_an_untouched_one_is_replaced() {
        let dir = scratch();
        let json = dir.join("call.mov.transcript.json");
        assert!(refuse_overwrite(&json).is_ok(), "nothing there yet");

        std::fs::write(&json, UNTOUCHED).expect("write");
        assert!(
            refuse_overwrite(&json).is_ok(),
            "an untouched transcript is ours to replace"
        );

        for (corrected, why) in [
            (
                UNTOUCHED.replace("\"edited\":false", "\"edited\":true"),
                "an edit is theirs",
            ),
            (
                UNTOUCHED.replace("\"status\":\"auto\"", "\"status\":\"confirmed\""),
                "a confirmed speaker is theirs",
            ),
            (
                UNTOUCHED.replace("{\"version\":1,", "{\"version\":1,\"corrected\":true,"),
                "a merge, a move or a label is theirs too",
            ),
            (
                "not json".to_owned(),
                "an unreadable transcript is never overwritten blind",
            ),
        ] {
            std::fs::write(&json, corrected).expect("write");
            assert!(
                matches!(refuse_overwrite(&json), Err(Stop::Failed(_))),
                "{why}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The start-of-job check is minutes old when the job writes: a
    /// correction made in between must survive the job's write.
    #[test]
    fn a_job_never_writes_over_a_correction_made_while_it_ran() {
        let dir = scratch();
        let json = dir.join("call.mov.transcript.json");
        let md = markdown_path(&json);
        let heard = Transcript::from_json(UNTOUCHED).expect("transcript");
        assert!(write_fresh_transcript(&json, &md, &heard).is_ok());

        let (edited, _) =
            edit_utterance(read_transcript(&json).expect("read"), "u1", "hello there")
                .expect("edit");
        write_transcript(&json, &md, &edited).expect("the correction lands mid-job");
        assert!(matches!(
            write_fresh_transcript(&json, &md, &heard),
            Err(Stop::Failed(_))
        ));
        assert_eq!(
            read_transcript(&json).expect("read").utterances[0].text,
            "hello there",
            "the correction survives the job"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An assign cuts its clip from a snapshot, then records the person on
    /// the transcript as it is by then: an edit made meanwhile stays, and a
    /// speaker that has gone meanwhile writes nothing anywhere.
    #[test]
    fn an_assign_keeps_an_edit_made_while_its_clip_was_cut() {
        let dir = scratch();
        let json = dir.join("call.mov.transcript.json");
        let root = dir.join("voices");
        std::fs::write(&json, UNTOUCHED).expect("write");
        let (person, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Anna".to_owned()),
            None,
            "e",
            false,
        )
        .expect("plan");

        assert!(commit_assignment(&json, "S9", &person, &root, &plan).is_err());
        assert!(
            Bank::load(&root).people.is_empty(),
            "a speaker that is gone makes no person"
        );

        let (edited, _) =
            edit_utterance(read_transcript(&json).expect("read"), "u1", "hello there")
                .expect("edit");
        write_transcript(&json, &markdown_path(&json), &edited).expect("edit lands");
        commit_assignment(&json, "S1", &person, &root, &plan).expect("assign");
        let on_disk = read_transcript(&json).expect("read");
        assert_eq!(on_disk.utterances[0].text, "hello there");
        assert_eq!(
            on_disk.speaker("S1").and_then(|s| s.person_id.clone()),
            Some(person.id.clone())
        );
        assert!(Bank::load(&root).person(&person.id).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sample(start: f64, embedding: Option<Vec<f32>>) -> CutSample {
        CutSample {
            wav: wav_bytes(&[0.1; 1600]),
            embedding,
            source: SampleSource {
                transcript: "call.mov.transcript.json".to_owned(),
                start,
                end: start + 3.0,
            },
        }
    }

    /// A new person and their voice land as one plan; a voice that could not
    /// be had still names the person; a voice heard while a job holds the
    /// engine is kept for the next job to embed.
    #[test]
    fn a_new_person_and_their_voice_are_one_plan() {
        let dir = scratch();
        let root = dir.join("voices");
        let model = "wespeaker";

        let (anna, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Anna".to_owned()),
            Some(sample(1.0, Some(vec![1.0, 0.0, 0.0]))),
            model,
            false,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");
        let bank = Bank::load(&root);
        assert_eq!(bank.person(&anna.id).map(|p| p.name.as_str()), Some("Anna"));
        assert_eq!(bank.sample_facts(&anna.id, model), (1, true));

        let (bob, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Bob".to_owned()),
            None,
            model,
            false,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");
        assert_eq!(Bank::load(&root).sample_facts(&bob.id, model), (0, false));

        let (_, plan) = plan_assignment(
            Bank::load(&root),
            Naming::Existing(bob.id.clone()),
            Some(sample(9.0, None)),
            model,
            false,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");
        let bank = Bank::load(&root);
        assert_eq!(bank.sample_facts(&bob.id, model), (1, false));
        assert_eq!(
            bank.missing_embeddings(model).len(),
            1,
            "the next job embeds the kept clip"
        );

        assert!(plan_assignment(
            Bank::load(&root),
            Naming::Existing("01NOBODY".to_owned()),
            None,
            model,
            false
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirming_the_microphone_speaker_marks_that_person_as_me_once() {
        let dir = std::env::temp_dir().join(format!("keeper-assign-self-{}", ulid::Ulid::new()));
        let root = dir.join("voices");
        std::fs::create_dir_all(&root).expect("root");

        let (me, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Tomasz".to_owned()),
            None,
            "m",
            true,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");
        assert!(Bank::load(&root).person(&me.id).is_some_and(|p| p.is_self));

        // A second person confirmed on a microphone (another Mac's owner)
        // does not take "me" away from the first.
        let (other, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Marta".to_owned()),
            None,
            "m",
            true,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");
        let bank = Bank::load(&root);
        assert!(bank.person(&me.id).is_some_and(|p| p.is_self));
        assert!(bank.person(&other.id).is_some_and(|p| !p.is_self));

        // A speaker from the call never becomes me.
        let dir2 = dir.join("second");
        std::fs::create_dir_all(&dir2).expect("root2");
        let (them, plan) = plan_assignment(
            Bank::load(&dir2),
            Naming::New("Kelly".to_owned()),
            None,
            "m",
            false,
        )
        .expect("plan");
        execute_plan(&dir2, &plan).expect("execute");
        assert!(Bank::load(&dir2)
            .person(&them.id)
            .is_some_and(|p| !p.is_self));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An engine that hears nothing and embeds every clip alike.
    struct EmbedOnly;

    impl SpeechEngine for EmbedOnly {
        fn availability(&self) -> Result<(), EngineUnavailable> {
            Ok(())
        }
        fn load(&self, _: &Path, _: &ModelSet) -> Result<(), EngineError> {
            Ok(())
        }
        fn audio_tracks(&self, _: &Path) -> Result<Vec<AudioTrackInfo>, EngineError> {
            AbsentEngine::refused()
        }
        fn decode(
            &self,
            _: &Path,
            _: TrackSelect,
            _: Option<(f64, f64)>,
        ) -> Result<Vec<f32>, EngineError> {
            AbsentEngine::refused()
        }
        fn transcribe(
            &self,
            _: &[f32],
            _: TranscriptionLanguage,
        ) -> Result<AsrOutput, EngineError> {
            AbsentEngine::refused()
        }
        fn diarize(&self, _: &[f32]) -> Result<DiarOutput, EngineError> {
            AbsentEngine::refused()
        }
        fn embed(&self, _: &[f32]) -> Result<Option<Vec<f32>>, EngineError> {
            Ok(Some(vec![0.0, 1.0, 0.0]))
        }
    }

    #[test]
    fn a_clip_kept_while_a_job_ran_is_embedded_by_the_next_job_once() {
        let dir = scratch();
        let root = dir.join("voices");
        let model = "wespeaker";
        let (bob, plan) = plan_assignment(
            Bank::load(&root),
            Naming::New("Bob".to_owned()),
            Some(sample(2.0, None)),
            model,
            false,
        )
        .expect("plan");
        execute_plan(&root, &plan).expect("execute");

        assert!(re_embed_bank(&EmbedOnly, &root, &Bank::load(&root), model));
        let bank = Bank::load(&root);
        assert_eq!(bank.sample_facts(&bob.id, model), (1, true));
        assert!(bank.missing_embeddings(model).is_empty());
        assert!(
            !re_embed_bank(&EmbedOnly, &root, &bank, model),
            "nothing is left to embed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_re_embed_leaves_a_file_another_device_wrote_alone() {
        let dir = scratch();
        let root = dir.join("voices");
        std::fs::create_dir_all(root.join("embeddings")).expect("dir");
        std::fs::write(root.join("embeddings/a.json"), "theirs").expect("theirs");
        let write = |rel: &str| keeper_core::transcription::BankWrite {
            rel_path: rel.to_owned(),
            bytes: b"ours".to_vec(),
        };
        let plan = BankPlan {
            writes: vec![write("embeddings/a.json"), write("embeddings/b.json")],
            deletes: Vec::new(),
        };
        execute_plan(&root, &only_absent(&root, plan)).expect("execute");
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).expect("read");
        assert_eq!(read("embeddings/a.json"), "theirs");
        assert_eq!(read("embeddings/b.json"), "ours");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staging_file_is_never_committed_and_never_shared() {
        let excludes = keeper_sync::exclude::ExcludeSet::new(&[]).expect("excludes");
        let dir = Path::new("/drive/70-comms/meeting");
        let first = staging_path(dir);
        let second = staging_path(dir);
        assert_ne!(
            first, second,
            "two writers to one target get two temporaries"
        );
        for temp in [&first, &second] {
            let relative = temp.strip_prefix("/drive").expect("inside the drive");
            assert!(
                excludes.is_excluded(relative),
                "{} would be committed",
                relative.display()
            );
        }
    }

    /// Readiness wins over a fetch in flight (every sync starts one), and a
    /// set that is all here but not what the account holds is not Ready.
    #[test]
    fn models_that_are_here_and_current_are_ready_even_mid_fetch() {
        let state = |missing: &[&str], current, fetch, account| {
            models_state(
                missing.iter().map(|m| (*m).to_owned()).collect(),
                current,
                fetch,
                account,
            )
            .state
        };
        assert_eq!(state(&[], true, Fetch::Fetching, true), ModelsState::Ready);
        assert_eq!(
            state(&[], true, Fetch::Failed("x".to_owned()), true),
            ModelsState::Ready
        );
        assert_eq!(
            state(&[], false, Fetch::Fetching, true),
            ModelsState::Fetching
        );
        assert_eq!(state(&[], false, Fetch::Idle, true), ModelsState::Missing);
        assert_eq!(
            state(&["asr/x"], false, Fetch::Idle, true),
            ModelsState::Missing
        );
        assert_eq!(
            state(&["asr/x"], false, Fetch::Failed("x".to_owned()), true),
            ModelsState::Failed
        );
        assert_eq!(
            state(&["asr/x"], false, Fetch::Idle, false),
            ModelsState::NoAccount
        );
    }

    #[test]
    fn a_fetch_that_dies_lets_the_next_one_start() {
        let first = begin_fetch().expect("nothing is fetching yet");
        assert!(begin_fetch().is_none(), "one fetch at a time");
        let died = std::thread::spawn(move || {
            let _held = first;
            panic!("the fetch task died");
        })
        .join();
        assert!(died.is_err());
        let next = begin_fetch();
        assert!(next.is_some(), "a dead fetch holds nothing");
        drop(next);
        assert_eq!(*lock(&FETCH), Fetch::Idle);
    }

    #[test]
    fn a_transcripts_markdown_twin_sits_beside_it() {
        assert_eq!(
            markdown_path(Path::new("/s/2026-09-28 1400/transcript.json")),
            PathBuf::from("/s/2026-09-28 1400/transcript.md")
        );
        assert_eq!(
            markdown_path(Path::new("/d/call.mov.transcript.json")),
            PathBuf::from("/d/call.mov.transcript.md")
        );
    }
}
