//! The model set transcription loads (AD-340, AD-341): which directories under
//! `<data_dir>/models/` hold the recognizer and the diarizer, and which
//! embedding model the voices bank keys its vectors on.
//!
//! The account config repo names the set in `_models/models.toml`; keeper
//! hydrates `_models/` into `<data_dir>/models/` and the engine is never asked
//! to load a set [`missing`] still reports files for.

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

/// Every file the engine needs, relative to the models root, `/`-separated.
pub fn required_paths(set: &ModelSet) -> Vec<String> {
    let mut paths =
        Vec::with_capacity((ASR_MODELS.len() + DIARIZER_MODELS.len()) * MLMODELC_FILES.len() + 2);
    let mut compiled = |dir: &str, models: [&str; 4]| {
        for model in models {
            for file in MLMODELC_FILES {
                paths.push(format!("{dir}/{model}.mlmodelc/{file}"));
            }
        }
    };
    compiled(&set.asr_dir, ASR_MODELS);
    compiled(&set.diarizer_dir, DIARIZER_MODELS);
    paths.push(format!("{}/{ASR_VOCAB}", set.asr_dir));
    paths.push(format!("{}/{DIARIZER_PLDA}", set.diarizer_dir));
    paths
}

/// The [`required_paths`] not present as files under `root`.
pub fn missing(root: &Path, set: &ModelSet) -> Vec<String> {
    required_paths(set)
        .into_iter()
        .filter(|relative| !root.join(relative).is_file())
        .collect()
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
}
