//! The model sets keeper loads from the account config repo (AD-340, AD-341,
//! AD-410): which directories under `<data_dir>/models/` hold the recognizer
//! and the diarizer transcription runs, which embedding model the voices bank
//! keys its vectors on, and which hold the two turn models voice runs (a
//! voice activity model and an end-of-turn model, D-36).
//!
//! The account config repo names the sets in `_models/models.toml`; keeper
//! hydrates `_models/` into `<data_dir>/models/` and the engine is never asked
//! to load a set [`missing`] still reports files for. The turn roles are
//! optional and checked apart ([`turn_missing`]): a repository with a half
//! turn set still transcribes.
//!
//! A person may pick another hydrated directory for any role in Settings
//! (`transcription.asr_model`, `transcription.diarization_model`,
//! `transcription.vad_model`, `transcription.smart_turn_model`); [`choose`]
//! and [`choose_turn`] apply those picks, and refuse one that is not complete
//! here rather than quietly falling back to the repository's.
//!
//! [`fetch_groups`] decides what a machine hydrates: the folders of the roles
//! it can run, each group with its own completion marker.

use std::path::Path;

use serde::Deserialize;

/// The config-repo directory that carries the models (git LFS).
pub const CONFIG_MODELS_DIR: &str = "_models";

/// The file inside [`CONFIG_MODELS_DIR`] naming the set.
pub const MODELS_TOML: &str = "models.toml";

const DEFAULT_ASR_DIR: &str = "parakeet-tdt-0.6b-v3";
const DEFAULT_DIARIZER_DIR: &str = "speaker-diarization";
const DEFAULT_EMBEDDING_MODEL: &str = "pyannote-community-1";

const ASR_MODELS: [&str; 4] = ["Preprocessor", "Encoder", "Decoder", "JointDecisionv3"];
const ASR_VOCAB: &str = "parakeet_vocab.json";
const DIARIZER_MODELS: [&str; 4] = ["Segmentation", "FBank", "Embedding", "PldaRho"];
const DIARIZER_PLDA: &str = "plda-parameters.json";
/// What loading a compiled model reads; `metadata.json` and `analytics/`
/// are not needed, and the weights are the file a half-hydrated set lacks.
const MLMODELC_FILES: [&str; 3] = ["coremldata.bin", "model.mil", "weights/weight.bin"];
/// The one file a turn model's directory holds that keeper loads.
const ONNX_MODEL: &str = "model.onnx";

/// The hydration group of the transcription roles: on a Mac that can
/// transcribe, every folder under `_models/` but the turn models'.
pub const TRANSCRIPTION_GROUP: &str = "transcription";
/// The hydration group of the turn roles: the folders of the two turn models.
pub const TURN_GROUP: &str = "turn";

/// The directories (relative to the models root) and the embedding model id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSet {
    pub asr_dir: String,
    pub diarizer_dir: String,
    /// The voices bank's `embeddings/<id>/` prefix: a change re-embeds from clips.
    pub embedding_model: String,
    /// The voice activity model (`[vad] dir`); `None` when the repository
    /// names none, and then voice keeps its pause rule.
    pub vad_dir: Option<String>,
    /// The end-of-turn model (`[smart_turn] dir`); `None` as for `vad_dir`.
    pub smart_turn_dir: Option<String>,
}

impl Default for ModelSet {
    fn default() -> Self {
        Self {
            asr_dir: DEFAULT_ASR_DIR.to_owned(),
            diarizer_dir: DEFAULT_DIARIZER_DIR.to_owned(),
            embedding_model: DEFAULT_EMBEDDING_MODEL.to_owned(),
            vad_dir: None,
            smart_turn_dir: None,
        }
    }
}

/// `_models/models.toml` could not be read as a model set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("_models/models.toml: {0}")]
pub struct ModelsTomlError(pub String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelsFile {
    asr: Option<DirSection>,
    diarizer: Option<DirSection>,
    embedding: Option<IdSection>,
    vad: Option<DirSection>,
    smart_turn: Option<DirSection>,
}

#[derive(Deserialize)]
struct DirSection {
    dir: String,
}

#[derive(Deserialize)]
struct IdSection {
    id: String,
}

impl ModelSet {
    /// Parse `_models/models.toml` (`[asr] dir`, `[diarizer] dir`,
    /// `[embedding] id`, `[vad] dir`, `[smart_turn] dir`). A missing
    /// transcription section keeps that default and a missing turn section
    /// names no turn model; every name must be one plain path segment,
    /// because each becomes a directory.
    pub fn from_toml(raw: &str) -> Result<Self, ModelsTomlError> {
        let file: ModelsFile =
            toml::from_str(raw).map_err(|error| ModelsTomlError(error.message().to_owned()))?;
        let defaults = Self::default();
        let set = Self {
            asr_dir: file.asr.map_or(defaults.asr_dir, |section| section.dir),
            diarizer_dir: file
                .diarizer
                .map_or(defaults.diarizer_dir, |section| section.dir),
            embedding_model: file
                .embedding
                .map_or(defaults.embedding_model, |section| section.id),
            vad_dir: file.vad.map(|section| section.dir),
            smart_turn_dir: file.smart_turn.map(|section| section.dir),
        };
        for (what, value) in [
            ("asr.dir", Some(&set.asr_dir)),
            ("diarizer.dir", Some(&set.diarizer_dir)),
            ("embedding.id", Some(&set.embedding_model)),
            ("vad.dir", set.vad_dir.as_ref()),
            ("smart_turn.dir", set.smart_turn_dir.as_ref()),
        ] {
            if let Some(value) = value.filter(|value| !is_plain_segment(value)) {
                return Err(ModelsTomlError(format!(
                    "`{what}` must be one folder name, not {value:?}"
                )));
            }
        }
        Ok(set)
    }
}

/// Whether `value` is a single, non-empty, non-special path segment.
pub(crate) fn is_plain_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', ':'])
        && value.trim() == value
}

/// Which role a model directory fills: the two engine roles transcription
/// loads, and the two turn roles voice loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Asr,
    Diarizer,
    Vad,
    SmartTurn,
}

impl ModelRole {
    /// The files this role's directory `dir` must hold, relative to the
    /// models root, `/`-separated.
    fn paths(self, dir: &str) -> Vec<String> {
        let (models, extra) = match self {
            Self::Asr => (ASR_MODELS, ASR_VOCAB),
            Self::Diarizer => (DIARIZER_MODELS, DIARIZER_PLDA),
            Self::Vad | Self::SmartTurn => return vec![format!("{dir}/{ONNX_MODEL}")],
        };
        let mut paths = Vec::with_capacity(models.len() * MLMODELC_FILES.len() + 1);
        for model in models {
            for file in MLMODELC_FILES {
                paths.push(format!("{dir}/{model}.mlmodelc/{file}"));
            }
        }
        paths.push(format!("{dir}/{extra}"));
        paths
    }

    /// The Settings control that picks this role's model, as a person reads it.
    fn setting(self) -> &'static str {
        match self {
            Self::Asr => "Speech model",
            Self::Diarizer => "Speaker model",
            Self::Vad => "Speech detection model",
            Self::SmartTurn => "Turn-end model",
        }
    }

    /// The `models.toml` section naming this role's directory.
    fn section(self) -> &'static str {
        match self {
            Self::Asr => "[asr]",
            Self::Diarizer => "[diarizer]",
            Self::Vad => "[vad]",
            Self::SmartTurn => "[smart_turn]",
        }
    }
}

/// Every file the engine needs, relative to the models root, `/`-separated.
/// The turn roles are not among them: transcription never waits for them.
pub fn required_paths(set: &ModelSet) -> Vec<String> {
    let mut paths = ModelRole::Asr.paths(&set.asr_dir);
    paths.extend(ModelRole::Diarizer.paths(&set.diarizer_dir));
    paths
}

/// The [`required_paths`] not present as files under `root`.
pub fn missing(root: &Path, set: &ModelSet) -> Vec<String> {
    required_paths(set)
        .into_iter()
        .filter(|relative| !root.join(relative).is_file())
        .collect()
}

/// The two turn roles and the directory `set` names for each.
fn turn_roles(set: &ModelSet) -> [(ModelRole, Option<&str>); 2] {
    [
        (ModelRole::Vad, set.vad_dir.as_deref()),
        (ModelRole::SmartTurn, set.smart_turn_dir.as_deref()),
    ]
}

/// Every file the turn models need, relative to the models root: those of
/// the turn roles `set` names.
pub fn turn_required_paths(set: &ModelSet) -> Vec<String> {
    turn_roles(set)
        .into_iter()
        .filter_map(|(role, dir)| dir.map(|dir| role.paths(dir)))
        .flatten()
        .collect()
}

/// What keeps the turn models from loading, as a person reads it: each
/// absent file, and for a role `models.toml` names no directory for, that
/// section (`[vad] in models.toml`). Empty when both are here.
pub fn turn_missing(root: &Path, set: &ModelSet) -> Vec<String> {
    let mut missing = Vec::new();
    for (role, dir) in turn_roles(set) {
        match dir {
            None => missing.push(format!("{} in {MODELS_TOML}", role.section())),
            Some(dir) => missing.extend(
                role.paths(dir)
                    .into_iter()
                    .filter(|relative| !root.join(relative).is_file()),
            ),
        }
    }
    missing
}

/// One model directory under the models root that could fill a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDir {
    pub id: String,
    /// Every file the role needs is here.
    pub complete: bool,
}

/// The directories under `root` holding any of a role's files, per role and
/// sorted by id. One holding none of them is not that role's model at all;
/// one holding some is listed as incomplete, so a half-hydrated model is
/// seen rather than silently missing.
pub fn available(root: &Path) -> (Vec<ModelDir>, Vec<ModelDir>) {
    let mut ids: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|id| is_plain_segment(id) && !id.starts_with('.'))
        .collect();
    ids.sort();
    let scan = |role: ModelRole| -> Vec<ModelDir> {
        ids.iter()
            .filter_map(|id| {
                let paths = role.paths(id);
                let present = paths
                    .iter()
                    .filter(|relative| root.join(relative).is_file())
                    .count();
                (present > 0).then(|| ModelDir {
                    id: id.clone(),
                    complete: present == paths.len(),
                })
            })
            .collect()
    };
    (scan(ModelRole::Asr), scan(ModelRole::Diarizer))
}

/// A model picked in Settings that cannot be loaded here, as a sentence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ModelChoiceRefused(pub String);

/// The set to load: `repo` (what `models.toml` names) with each non-empty
/// Settings pick in place of its role's directory. A pick that is not a
/// complete model of its role under `root` is refused, naming it and where
/// to change it — never replaced by the repository's.
///
/// The voices bank keys its vectors by embedding model; a speaker model
/// other than the repository's may embed differently, so the bank is keyed
/// by that directory's id instead and re-embeds from its clips.
pub fn choose(
    root: &Path,
    repo: ModelSet,
    asr: &str,
    diarizer: &str,
) -> Result<ModelSet, ModelChoiceRefused> {
    let asr = pick(root, ModelRole::Asr, asr)?;
    let diarizer = pick(root, ModelRole::Diarizer, diarizer)?;
    let mut set = repo;
    if let Some(dir) = asr {
        set.asr_dir = dir;
    }
    if let Some(dir) = diarizer.filter(|dir| *dir != set.diarizer_dir) {
        set.embedding_model.clone_from(&dir);
        set.diarizer_dir = dir;
    }
    Ok(set)
}

/// The turn models to load: `repo` with each non-empty pick
/// (`transcription.vad_model`, `transcription.smart_turn_model`) in place of
/// its role's directory, refused exactly as [`choose`] refuses — a pick that
/// is not complete here is never replaced by the repository's. The picks
/// travel to every device, and every device that runs turn models fetches
/// the picked folders ([`turn_folders`]), so the refusal names the device,
/// not the Mac. Settings has no picker for them: a pick is the settings key,
/// and the refusal names it and how to replace or clear it.
pub fn choose_turn(
    root: &Path,
    repo: ModelSet,
    vad: &str,
    smart_turn: &str,
) -> Result<ModelSet, ModelChoiceRefused> {
    let vad = pick(root, ModelRole::Vad, vad)?;
    let smart_turn = pick(root, ModelRole::SmartTurn, smart_turn)?;
    let mut set = repo;
    if vad.is_some() {
        set.vad_dir = vad;
    }
    if smart_turn.is_some() {
        set.smart_turn_dir = smart_turn;
    }
    Ok(set)
}

/// `chosen` as `role`'s directory: `None` when blank, the name when it is a
/// complete model of the role under `root`, refused otherwise.
fn pick(root: &Path, role: ModelRole, chosen: &str) -> Result<Option<String>, ModelChoiceRefused> {
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return Ok(None);
    }
    let problem = if !is_plain_segment(chosen) || !root.join(chosen).is_dir() {
        "is not on"
    } else if role
        .paths(chosen)
        .iter()
        .any(|relative| !root.join(relative).is_file())
    {
        "is incomplete on"
    } else {
        return Ok(Some(chosen.to_owned()));
    };
    let what = role.setting();
    let sentence = match role {
        ModelRole::Asr | ModelRole::Diarizer => format!(
            "The {} \u{201c}{chosen}\u{201d} chosen in Settings \u{2192} Transcription {problem} \
             this Mac. Choose another {what} there, or \u{201c}From the config repository\u{201d}.",
            what.to_lowercase()
        ),
        ModelRole::Vad | ModelRole::SmartTurn => {
            let key = match role {
                ModelRole::Vad => crate::registry::TRANSCRIPTION_VAD_MODEL_KEY,
                _ => crate::registry::TRANSCRIPTION_SMART_TURN_MODEL_KEY,
            };
            format!(
                "The {} \u{201c}{chosen}\u{201d} set by `{key}` {problem} this device. Set `{key}` \
                 in your account's settings.toml to another folder of _models/, or remove it to \
                 use the one {} in models.toml names.",
                what.to_lowercase(),
                role.section()
            )
        }
    };
    Err(ModelChoiceRefused(sentence))
}

/// Which top-level folders under `_models/` a hydration group takes; its
/// plain files (`models.toml`) belong to every group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folders {
    Only(Vec<String>),
    AllBut(Vec<String>),
}

/// One part of `_models/` hydrated as a unit, with its own completion marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchGroup {
    pub name: &'static str,
    pub folders: Folders,
}

impl FetchGroup {
    /// What `models.toml` means to this group, for its completion digest:
    /// the sections of its own roles, so editing the other group's sections
    /// never makes this group look out of date. `None` when the file does
    /// not parse; then its bytes count whole.
    pub fn manifest_fingerprint(&self) -> fn(&[u8]) -> Option<String> {
        if self.name == TURN_GROUP {
            turn_manifest
        } else {
            transcription_manifest
        }
    }
}

fn parse_manifest(raw: &[u8]) -> Option<ModelSet> {
    ModelSet::from_toml(std::str::from_utf8(raw).ok()?).ok()
}

fn transcription_manifest(raw: &[u8]) -> Option<String> {
    let set = parse_manifest(raw)?;
    Some(format!(
        "asr={:?}\ndiarizer={:?}\nembedding={:?}\n",
        set.asr_dir, set.diarizer_dir, set.embedding_model
    ))
}

fn turn_manifest(raw: &[u8]) -> Option<String> {
    let set = parse_manifest(raw)?;
    Some(format!(
        "vad={:?}\nsmart_turn={:?}\n",
        set.vad_dir, set.smart_turn_dir
    ))
}

/// The turn models' folders a device fetches: the ones `repo` names and the
/// ones picked in settings, sorted, each once. A pick that is not a folder
/// name is left out ([`choose_turn`] refuses it), and so is a folder the
/// repository names for a transcription role: that folder is
/// transcription's, so no folder is in both groups.
pub fn turn_folders(repo: &ModelSet, vad: &str, smart_turn: &str) -> Vec<String> {
    let mut folders: Vec<String> = [
        repo.vad_dir.as_deref(),
        repo.smart_turn_dir.as_deref(),
        Some(vad.trim()),
        Some(smart_turn.trim()),
    ]
    .into_iter()
    .flatten()
    .filter(|name| is_plain_segment(name) && !names_for_transcription(repo, name))
    .map(str::to_owned)
    .collect();
    folders.sort();
    folders.dedup();
    folders
}

fn names_for_transcription(repo: &ModelSet, folder: &str) -> bool {
    folder == repo.asr_dir || folder == repo.diarizer_dir
}

/// The transcription group: every folder of the clone's `_models/`
/// (`clone_models`) but the turn models' — the ones [`turn_folders`] names,
/// and any other folder holding a turn model's file (an older version kept
/// beside the one `models.toml` names now). So the speech and speaker models
/// a person may pick are all here, the two groups never share a folder, and
/// no turn model, picked or replaced, makes transcription look half-updated.
pub fn transcription_group(
    clone_models: &Path,
    repo: &ModelSet,
    vad: &str,
    smart_turn: &str,
) -> FetchGroup {
    let mut turn = turn_folders(repo, vad, smart_turn);
    if let Ok(listing) = std::fs::read_dir(clone_models) {
        turn.extend(
            listing
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter(|entry| entry.path().join(ONNX_MODEL).is_file())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| !names_for_transcription(repo, name)),
        );
    }
    turn.sort();
    turn.dedup();
    FetchGroup {
        name: TRANSCRIPTION_GROUP,
        folders: Folders::AllBut(turn),
    }
}

/// The turn group: the folders [`turn_folders`] names and nothing else.
pub fn turn_group(repo: &ModelSet, vad: &str, smart_turn: &str) -> FetchGroup {
    FetchGroup {
        name: TURN_GROUP,
        folders: Folders::Only(turn_folders(repo, vad, smart_turn)),
    }
}

/// What a machine hydrates: the transcription group where it can
/// transcribe, the turn group where its voice runs turn models, nothing
/// where it does neither. `clone_models` is the clone's `_models/`, `repo`
/// the set its `models.toml` names now and `vad`/`smart_turn` the picks.
pub fn fetch_groups(
    transcribes: bool,
    takes_turns: bool,
    clone_models: &Path,
    repo: &ModelSet,
    vad: &str,
    smart_turn: &str,
) -> Vec<FetchGroup> {
    let mut groups = Vec::with_capacity(2);
    if transcribes {
        groups.push(transcription_group(clone_models, repo, vad, smart_turn));
    }
    if takes_turns {
        groups.push(turn_group(repo, vad, smart_turn));
    }
    groups
}

/// The embedding model the voices bank is read under: the one [`choose`]
/// loads for the speaker model picked in Settings. Only that pick matters —
/// a refused speech model does not hide the people — and a refused speaker
/// model is refused here too, never read as the repository's bank.
pub fn bank_embedding(
    root: &Path,
    repo: ModelSet,
    diarizer: &str,
) -> Result<String, ModelChoiceRefused> {
    choose(root, repo, "", diarizer).map(|set| set.embedding_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_models_toml_overrides_only_what_it_names() {
        let set = ModelSet::from_toml("[embedding]\nid = \"wespeaker-v2\"\n").expect("parse");
        assert_eq!(set.embedding_model, "wespeaker-v2");
        assert_eq!(set.asr_dir, DEFAULT_ASR_DIR);
        assert_eq!(set.diarizer_dir, DEFAULT_DIARIZER_DIR);
    }

    #[test]
    fn a_models_toml_naming_a_path_instead_of_a_folder_is_refused() {
        for bad in ["../elsewhere", "a/b", "", "."] {
            let raw = format!("[asr]\ndir = {bad:?}\n");
            assert!(ModelSet::from_toml(&raw).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn missing_lists_exactly_the_absent_files() {
        let root = std::env::temp_dir().join(format!("keeper-models-{}", ulid::Ulid::new()));
        let set = ModelSet::default();
        let all = required_paths(&set);
        let present = &all[..all.len() - 1];
        for relative in present {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }
        assert_eq!(missing(&root, &set), vec![all[all.len() - 1].clone()]);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// A models root holding `role`'s files under `dir`, all but `skip` of them.
    fn hydrate(root: &Path, role: ModelRole, dir: &str, skip: usize) {
        let paths = role.paths(dir);
        for relative in &paths[..paths.len() - skip] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }
    }

    fn models_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("keeper-models-{}", ulid::Ulid::new()))
    }

    #[test]
    fn an_empty_choice_keeps_the_repositorys_set() {
        let root = models_root();
        let repo = ModelSet::from_toml("[embedding]\nid = \"wespeaker-v2\"\n").expect("parse");
        assert_eq!(choose(&root, repo.clone(), "", "  "), Ok(repo));
    }

    #[test]
    fn a_complete_choice_replaces_its_roles_directory() {
        let root = models_root();
        hydrate(&root, ModelRole::Asr, "parakeet-tdt-0.6b-v4", 0);
        hydrate(&root, ModelRole::Diarizer, "diarizer-next", 0);

        let set = choose(&root, ModelSet::default(), "parakeet-tdt-0.6b-v4", "").expect("asr");
        assert_eq!(set.asr_dir, "parakeet-tdt-0.6b-v4");
        assert_eq!(set.diarizer_dir, DEFAULT_DIARIZER_DIR);
        assert_eq!(set.embedding_model, DEFAULT_EMBEDDING_MODEL);

        // Another speaker model keys the bank by its own id, so vectors from
        // two embedding networks are never compared.
        let set = choose(&root, ModelSet::default(), "", "diarizer-next").expect("diarizer");
        assert_eq!(set.asr_dir, DEFAULT_ASR_DIR);
        assert_eq!(set.diarizer_dir, "diarizer-next");
        assert_eq!(set.embedding_model, "diarizer-next");

        // Picking the repository's own speaker model changes nothing.
        hydrate(&root, ModelRole::Diarizer, DEFAULT_DIARIZER_DIR, 0);
        let set = choose(&root, ModelSet::default(), "", DEFAULT_DIARIZER_DIR).expect("same");
        assert_eq!(set, ModelSet::default());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_missing_or_incomplete_choice_is_refused_by_name() {
        let root = models_root();
        hydrate(&root, ModelRole::Asr, "half-asr", 1);
        hydrate(&root, ModelRole::Diarizer, "speaker-only", 0);
        let refusal = |asr: &str, diarizer: &str| {
            choose(&root, ModelSet::default(), asr, diarizer)
                .expect_err("refused")
                .0
        };

        let gone = refusal("parakeet-gone", "");
        assert!(gone.contains("\u{201c}parakeet-gone\u{201d}"), "{gone}");
        assert!(gone.contains("is not on this Mac"), "{gone}");
        assert!(gone.contains("Settings \u{2192} Transcription"), "{gone}");
        assert!(gone.contains("speech model"), "{gone}");

        let half = refusal("half-asr", "");
        assert!(half.contains("\u{201c}half-asr\u{201d} chosen"), "{half}");
        assert!(half.contains("is incomplete"), "{half}");

        // A directory of the other role is not a model of this one.
        let wrong = refusal("", "half-asr");
        assert!(wrong.contains("speaker model"), "{wrong}");
        assert!(refusal("speaker-only", "").contains("is incomplete"));
        assert!(refusal("../speaker-only", "").contains("is not on this Mac"));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_scan_lists_each_roles_directories_and_marks_incomplete_ones() {
        let root = models_root();
        hydrate(&root, ModelRole::Asr, DEFAULT_ASR_DIR, 0);
        hydrate(&root, ModelRole::Asr, "half-asr", 3);
        hydrate(&root, ModelRole::Diarizer, DEFAULT_DIARIZER_DIR, 0);
        std::fs::create_dir_all(root.join("empty")).expect("mkdir");
        std::fs::create_dir_all(root.join(".staging").join("x")).expect("mkdir");
        std::fs::write(root.join(MODELS_TOML), b"").expect("toml");

        let dir = |id: &str, complete| ModelDir {
            id: id.to_owned(),
            complete,
        };
        let (asr, diarizer) = available(&root);
        assert_eq!(
            asr,
            [dir("half-asr", false), dir(DEFAULT_ASR_DIR, true)],
            "sorted, and only directories holding speech-model files"
        );
        assert_eq!(diarizer, [dir(DEFAULT_DIARIZER_DIR, true)]);
        assert_eq!(available(&root.join("absent")), (Vec::new(), Vec::new()));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_bank_is_read_under_the_speaker_pick_and_refused_with_it() {
        let root = models_root();
        hydrate(&root, ModelRole::Diarizer, "diarizer-next", 0);
        let repo = || ModelSet::from_toml("[embedding]\nid = \"wespeaker-v2\"\n").expect("parse");

        assert_eq!(
            bank_embedding(&root, repo(), "").as_deref(),
            Ok("wespeaker-v2")
        );
        assert_eq!(
            bank_embedding(&root, repo(), "diarizer-next").as_deref(),
            Ok("diarizer-next")
        );
        let refused = bank_embedding(&root, repo(), "diarizer-gone").expect_err("refused");
        assert!(
            refused.0.contains("\u{201c}diarizer-gone\u{201d}"),
            "{}",
            refused.0
        );
        assert!(refused.0.contains("speaker model"), "{}", refused.0);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    const TURN_TOML: &str =
        "[vad]\ndir = \"silero-vad\"\n\n[smart_turn]\ndir = \"smart-turn-v3\"\n";

    #[test]
    fn models_toml_turn_roles() {
        let set = ModelSet::from_toml(TURN_TOML).expect("parse");
        assert_eq!(set.vad_dir.as_deref(), Some("silero-vad"));
        assert_eq!(set.smart_turn_dir.as_deref(), Some("smart-turn-v3"));
        assert_eq!(
            set.asr_dir, DEFAULT_ASR_DIR,
            "transcription keeps its defaults"
        );

        let without = ModelSet::from_toml("[asr]\ndir = \"parakeet\"\n").expect("parse");
        assert_eq!((without.vad_dir, without.smart_turn_dir), (None, None));
        assert_eq!(ModelSet::from_toml("").expect("empty"), ModelSet::default());

        for section in ["vad", "smart_turn"] {
            for bad in ["../elsewhere", "a/b", "a\\b", "c:", "", ".", ".."] {
                let raw = format!("[{section}]\ndir = {bad:?}\n");
                let refused = ModelSet::from_toml(&raw).expect_err("refused");
                assert!(refused.0.contains(&format!("`{section}.dir`")), "{refused}");
            }
        }
    }

    #[test]
    fn turn_roles_do_not_change_transcriptions_required_files() {
        let plain = ModelSet::default();
        let with_turn = ModelSet::from_toml(TURN_TOML).expect("parse");
        assert_eq!(required_paths(&with_turn), required_paths(&plain));

        // Every transcription file present, the turn set half there: still
        // nothing missing for transcription.
        let root = models_root();
        hydrate(&root, ModelRole::Asr, DEFAULT_ASR_DIR, 0);
        hydrate(&root, ModelRole::Diarizer, DEFAULT_DIARIZER_DIR, 0);
        hydrate(&root, ModelRole::Vad, "silero-vad", 0);
        assert!(missing(&root, &with_turn).is_empty());
        assert_eq!(
            turn_missing(&root, &with_turn),
            ["smart-turn-v3/model.onnx"]
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn turn_missing_lists_exactly_the_absent_files() {
        let root = models_root();
        let set = ModelSet::from_toml(TURN_TOML).expect("parse");
        assert_eq!(
            turn_required_paths(&set),
            ["silero-vad/model.onnx", "smart-turn-v3/model.onnx"]
        );
        assert_eq!(
            turn_missing(&root, &set),
            ["silero-vad/model.onnx", "smart-turn-v3/model.onnx"]
        );
        hydrate(&root, ModelRole::SmartTurn, "smart-turn-v3", 0);
        assert_eq!(turn_missing(&root, &set), ["silero-vad/model.onnx"]);
        hydrate(&root, ModelRole::Vad, "silero-vad", 0);
        assert!(turn_missing(&root, &set).is_empty(), "both here: ready");

        // A repository naming no turn model: the sections are what is missing.
        let none = ModelSet::default();
        assert!(turn_required_paths(&none).is_empty());
        assert_eq!(
            turn_missing(&root, &none),
            ["[vad] in models.toml", "[smart_turn] in models.toml"]
        );
        let vad_only = ModelSet::from_toml("[vad]\ndir = \"silero-vad\"\n").expect("parse");
        assert_eq!(
            turn_missing(&root, &vad_only),
            ["[smart_turn] in models.toml"]
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn turn_model_pick_refuses_an_incomplete_folder() {
        let root = models_root();
        let repo = || ModelSet::from_toml(TURN_TOML).expect("parse");
        hydrate(&root, ModelRole::Vad, "silero-vad-v7", 0);
        std::fs::create_dir_all(root.join("smart-turn-v4")).expect("mkdir");
        std::fs::write(root.join("smart-turn-v4/LICENSE"), b"BSD").expect("licence");

        let set = choose_turn(&root, repo(), " silero-vad-v7 ", "").expect("complete pick");
        assert_eq!(set.vad_dir.as_deref(), Some("silero-vad-v7"));
        assert_eq!(set.smart_turn_dir.as_deref(), Some("smart-turn-v3"));
        assert_eq!(choose_turn(&root, repo(), "", ""), Ok(repo()));
        // A pick fills a role the repository leaves empty.
        let set = choose_turn(&root, ModelSet::default(), "silero-vad-v7", "").expect("pick");
        assert_eq!(set.vad_dir.as_deref(), Some("silero-vad-v7"));
        assert_eq!(set.smart_turn_dir, None);

        let refusal = |vad: &str, smart_turn: &str| {
            choose_turn(&root, repo(), vad, smart_turn)
                .expect_err("refused")
                .0
        };
        // There is no picker for the turn roles: the sentence names the key
        // and how to replace or clear it, never a control that does not exist.
        let half = refusal("", "smart-turn-v4");
        assert_eq!(
            half,
            "The turn-end model \u{201c}smart-turn-v4\u{201d} set by \
             `transcription.smart_turn_model` is incomplete on this device. Set \
             `transcription.smart_turn_model` in your account's settings.toml to another folder \
             of _models/, or remove it to use the one [smart_turn] in models.toml names."
        );
        let gone = refusal("silero-gone", "");
        assert_eq!(
            gone,
            "The speech detection model \u{201c}silero-gone\u{201d} set by \
             `transcription.vad_model` is not on this device. Set `transcription.vad_model` in \
             your account's settings.toml to another folder of _models/, or remove it to use the \
             one [vad] in models.toml names."
        );
        assert!(refusal("../silero-vad-v7", "").contains("is not on this device"));
        // The speech model's sentence is unchanged by the turn roles.
        hydrate(&root, ModelRole::Asr, "half-asr", 1);
        let asr = choose(&root, ModelSet::default(), "half-asr", "")
            .expect_err("refused")
            .0;
        assert!(
            asr.contains("chosen in Settings \u{2192} Transcription is incomplete on this Mac"),
            "{asr}"
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn each_machine_fetches_the_roles_it_can_run() {
        let clone = models_root();
        let repo = ModelSet::from_toml(TURN_TOML).expect("parse");
        let turn_dirs = || vec!["silero-vad".to_owned(), "smart-turn-v3".to_owned()];
        let transcription = FetchGroup {
            name: TRANSCRIPTION_GROUP,
            folders: Folders::AllBut(turn_dirs()),
        };
        let turn = FetchGroup {
            name: TURN_GROUP,
            folders: Folders::Only(turn_dirs()),
        };
        let groups = |transcribes, takes_turns, repo: &ModelSet, vad: &str, smart_turn: &str| {
            fetch_groups(transcribes, takes_turns, &clone, repo, vad, smart_turn)
        };

        // A Mac on macOS 15 or later: both groups.
        assert_eq!(
            groups(true, true, &repo, "", ""),
            [transcription.clone(), turn.clone()]
        );
        // The phone, and a Mac on macOS 14: the turn models only.
        assert_eq!(groups(false, true, &repo, "", ""), [turn]);
        // A Mac that transcribes but runs no turn models never fetches them.
        assert_eq!(groups(true, false, &repo, "", ""), [transcription]);
        // Neither: nothing.
        assert!(groups(false, false, &repo, "", "").is_empty());

        assert_eq!(
            turn_folders(&repo, "smart-turn-v3", ""),
            turn_dirs(),
            "a pick of the repository's own folder is fetched once"
        );
        // A repository naming no turn model: the phone fetches only the plain
        // files (models.toml), and transcription takes every folder.
        assert_eq!(
            groups(true, true, &ModelSet::default(), "", ""),
            [
                FetchGroup {
                    name: TRANSCRIPTION_GROUP,
                    folders: Folders::AllBut(Vec::new()),
                },
                FetchGroup {
                    name: TURN_GROUP,
                    folders: Folders::Only(Vec::new()),
                },
            ]
        );
    }

    /// A folder is in one group or the other, never both: a picked turn
    /// model is the turn group's and transcription leaves it out, so its
    /// missing object can never fail transcription's hydration.
    #[test]
    fn a_picked_turn_model_is_never_transcriptions() {
        let clone = models_root();
        let repo = ModelSet::from_toml(TURN_TOML).expect("parse");
        let groups = fetch_groups(true, true, &clone, &repo, " silero-vad-v7 ", "../x");
        let picked = vec![
            "silero-vad".to_owned(),
            "silero-vad-v7".to_owned(),
            "smart-turn-v3".to_owned(),
        ];
        assert_eq!(
            groups,
            [
                FetchGroup {
                    name: TRANSCRIPTION_GROUP,
                    folders: Folders::AllBut(picked.clone()),
                },
                FetchGroup {
                    name: TURN_GROUP,
                    folders: Folders::Only(picked),
                },
            ]
        );
        // A turn pick naming a transcription folder stays transcription's.
        assert_eq!(
            turn_folders(&repo, DEFAULT_ASR_DIR, ""),
            ["silero-vad", "smart-turn-v3"]
        );
    }

    /// Switching `[smart_turn]` to a new version, the old folder kept in the
    /// repository: transcription's group and its view of `models.toml` stay
    /// as they were, so its digest does not move.
    #[test]
    fn a_turn_model_version_switch_leaves_transcriptions_group_alone() {
        let clone = models_root();
        for dir in ["silero-vad", "smart-turn-v3", "smart-turn-v4"] {
            hydrate(&clone, ModelRole::Vad, dir, 0);
        }
        hydrate(&clone, ModelRole::Asr, DEFAULT_ASR_DIR, 0);
        let before = TURN_TOML;
        let after = "[vad]\ndir = \"silero-vad\"\n\n[smart_turn]\ndir = \"smart-turn-v4\"\n";
        let group = |raw: &str| {
            transcription_group(&clone, &ModelSet::from_toml(raw).expect("parse"), "", "")
        };
        assert_eq!(group(before), group(after));
        assert_eq!(
            group(after).folders,
            Folders::AllBut(vec![
                "silero-vad".to_owned(),
                "smart-turn-v3".to_owned(),
                "smart-turn-v4".to_owned(),
            ])
        );

        let transcription = group(before).manifest_fingerprint();
        let turn = turn_group(&ModelSet::default(), "", "").manifest_fingerprint();
        assert_eq!(
            transcription(before.as_bytes()),
            transcription(after.as_bytes())
        );
        assert_ne!(turn(before.as_bytes()), turn(after.as_bytes()));
        // A transcription change is transcription's, not the turn group's.
        let asr = format!("{before}\n[asr]\ndir = \"parakeet-next\"\n");
        assert_ne!(
            transcription(before.as_bytes()),
            transcription(asr.as_bytes())
        );
        assert_eq!(turn(before.as_bytes()), turn(asr.as_bytes()));
        // A file that does not parse has no meaning to offer: its bytes count.
        assert_eq!(transcription(b"[asr\n"), None);
        std::fs::remove_dir_all(&clone).expect("cleanup");
    }
}
