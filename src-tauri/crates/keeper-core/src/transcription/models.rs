//! The model set transcription loads (AD-340, AD-341): which directories under
//! `<data_dir>/models/` hold the recognizer and the diarizer, and which
//! embedding model the voices bank keys its vectors on.
//!
//! The account config repo names the set in `_models/models.toml`; keeper
//! hydrates `_models/` into `<data_dir>/models/` and the engine is never asked
//! to load a set [`missing`] still reports files for.
//!
//! A person may pick another hydrated directory for either role in Settings
//! (`transcription.asr_model`, `transcription.diarization_model`); [`choose`]
//! applies that pick, and refuses one that is not complete here rather than
//! quietly falling back to the repository's.

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

/// The directories (relative to the models root) and the embedding model id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSet {
    pub asr_dir: String,
    pub diarizer_dir: String,
    /// The voices bank's `embeddings/<id>/` prefix: a change re-embeds from clips.
    pub embedding_model: String,
}

impl Default for ModelSet {
    fn default() -> Self {
        Self {
            asr_dir: DEFAULT_ASR_DIR.to_owned(),
            diarizer_dir: DEFAULT_DIARIZER_DIR.to_owned(),
            embedding_model: DEFAULT_EMBEDDING_MODEL.to_owned(),
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
    /// `[embedding] id`). A missing section keeps that default; every name
    /// must be one plain path segment, because each becomes a directory.
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
        };
        for (what, value) in [
            ("asr.dir", &set.asr_dir),
            ("diarizer.dir", &set.diarizer_dir),
            ("embedding.id", &set.embedding_model),
        ] {
            if !is_plain_segment(value) {
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

/// Which of the two engine roles a model directory fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Asr,
    Diarizer,
}

impl ModelRole {
    /// The files this role's directory `dir` must hold, relative to the
    /// models root, `/`-separated.
    fn paths(self, dir: &str) -> Vec<String> {
        let (models, extra) = match self {
            Self::Asr => (ASR_MODELS, ASR_VOCAB),
            Self::Diarizer => (DIARIZER_MODELS, DIARIZER_PLDA),
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
        }
    }
}

/// Every file the engine needs, relative to the models root, `/`-separated.
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
    let pick = |role: ModelRole, chosen: &str| -> Result<Option<String>, ModelChoiceRefused> {
        let chosen = chosen.trim();
        if chosen.is_empty() {
            return Ok(None);
        }
        let what = role.setting();
        let problem = if !is_plain_segment(chosen) || !root.join(chosen).is_dir() {
            "is not on this Mac"
        } else if role
            .paths(chosen)
            .iter()
            .any(|relative| !root.join(relative).is_file())
        {
            "is incomplete on this Mac"
        } else {
            return Ok(Some(chosen.to_owned()));
        };
        Err(ModelChoiceRefused(format!(
            "The {} \u{201c}{chosen}\u{201d} chosen in Settings \u{2192} Transcription {problem}. \
             Choose another {what} there, or \u{201c}From the config repository\u{201d}.",
            what.to_lowercase()
        )))
    };
    let asr = pick(ModelRole::Asr, asr)?;
    let diarizer = pick(ModelRole::Diarizer, diarizer)?;
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
}
