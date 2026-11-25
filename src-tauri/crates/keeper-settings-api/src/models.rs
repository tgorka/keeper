//! Model sets published as `config_values` rows of the general (`makidrive`)
//! domain (story 121.3, DF/AD-16).
//!
//! A model set is one version, published whole:
//!
//! * `models/<version>/<path>` — one row per file: value `{"size", "sha256"}`
//!   and `value_blob` the file's blob, whose `blobId` is `sha256:<sha256>` and
//!   whose `byteLength` is `size`;
//! * `models/<version>` — the manifest: value `{"models_toml", "file_count"}`,
//!   written only once every file row is present and the rows hash to
//!   `<version>`;
//! * `models.active` — the version id, a JSON string, written last and only
//!   naming a version whose manifest is present.
//!
//! `<version>` is [`version_id`]: the sha256 over the sorted `(path, size,
//! sha256)` list and `models.toml`'s bytes. A version's rows are never
//! rewritten (`config.model_sealed`); only `models.active` moves. Every write
//! is the operator's admin path in the general domain alone.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::blob::BlobAnswer;
use crate::config::{names_domain, ConfigWriter};
use crate::refusal::Refusal;
use crate::scope::CallerKind;
use crate::write::DomainKind;
use crate::Limits;

/// The key naming the published version every member's device loads.
pub const ACTIVE_KEY: &str = "models.active";

/// The prefix of a version's manifest and file rows.
pub const VERSION_PREFIX: &str = "models/";

/// The file the manifest carries inline; never a file row of its own.
pub const MODELS_TOML: &str = "models.toml";

/// The domain-separation line [`version_id`] hashes first.
pub const VERSION_DOMAIN: &str = "keeper-models-v1";

/// Maximum UTF-8 byte length of a model key (as every `config_values` key).
const MAX_KEY_BYTES: usize = 256;

/// One file of a model set: its path under the set's root, its byte length,
/// and the lowercase hex sha256 of its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// `/`-separated path under the set's root.
    pub path: String,
    /// Byte length.
    pub size: u64,
    /// 64 lowercase hex digits.
    pub sha256: String,
}

/// A file row's value: what the file must be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileValue {
    /// Byte length.
    pub size: u64,
    /// 64 lowercase hex digits.
    pub sha256: String,
}

/// A manifest row's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `models.toml`'s text.
    pub models_toml: String,
    /// How many file rows the version has.
    pub file_count: u64,
}

/// A Syncular blob reference as `config_values.value_blob` holds it:
/// canonical `{blobId, byteLength, mediaType?, name?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BlobRef {
    /// `sha256:<64 lowercase hex>`.
    pub blob_id: String,
    /// Byte length.
    pub byte_length: u64,
    /// Optional media type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A parsed model key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelKey {
    /// `models.active`.
    Active,
    /// `models/<version>`.
    Manifest {
        /// The version id.
        version: String,
    },
    /// `models/<version>/<path>`.
    File {
        /// The version id.
        version: String,
        /// The file's path under the set's root.
        path: String,
    },
}

/// One write of a `config_values` row in the model namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWrite<'a> {
    /// The row's key.
    pub key: &'a str,
    /// The row's `value`.
    pub value: &'a Value,
    /// The row's `value_blob`.
    pub blob: Option<&'a BlobRef>,
}

/// What the server already holds of one version, read before a write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionState {
    /// The version's file rows already written.
    #[serde(default)]
    pub files: Vec<ModelFile>,
    /// The version's manifest, once written.
    #[serde(default)]
    pub manifest: Option<Manifest>,
}

/// Whether `key` is in the model namespace (`models.active` or `models/…`).
/// [`crate::config::check_config_write`] refuses these keys: they are written
/// through [`check_model_write`] only.
#[must_use]
pub fn is_model_key(key: &str) -> bool {
    key == ACTIVE_KEY || key.starts_with(VERSION_PREFIX)
}

/// Whether `text` is 64 lowercase hex digits.
#[must_use]
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Validate a file path under a set's root: `/`-separated segments of
/// `[A-Za-z0-9._-]`, none empty, none starting with `.`, and not
/// [`MODELS_TOML`] itself (the manifest carries it).
pub fn check_path(path: &str) -> Result<(), Refusal> {
    if path.is_empty() || path == MODELS_TOML {
        return Err(Refusal::ConfigMalformed);
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment.starts_with('.') {
            return Err(Refusal::ConfigMalformed);
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(Refusal::ConfigMalformed);
        }
    }
    Ok(())
}

/// Parse a model key; `config.malformed` for a key outside the namespace or
/// not of its grammar.
pub fn parse_key(key: &str) -> Result<ModelKey, Refusal> {
    if key == ACTIVE_KEY {
        return Ok(ModelKey::Active);
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(Refusal::ConfigMalformed);
    }
    let rest = key
        .strip_prefix(VERSION_PREFIX)
        .ok_or(Refusal::ConfigMalformed)?;
    let (version, path) = match rest.split_once('/') {
        Some((version, path)) => (version, Some(path)),
        None => (rest, None),
    };
    if !is_sha256_hex(version) {
        return Err(Refusal::ConfigMalformed);
    }
    match path {
        None => Ok(ModelKey::Manifest {
            version: version.to_owned(),
        }),
        Some(path) => {
            check_path(path)?;
            Ok(ModelKey::File {
                version: version.to_owned(),
                path: path.to_owned(),
            })
        }
    }
}

/// The key of `version`'s manifest row.
#[must_use]
pub fn manifest_key(version: &str) -> String {
    format!("{VERSION_PREFIX}{version}")
}

/// The key of `version`'s file row for `path`.
#[must_use]
pub fn file_key(version: &str, path: &str) -> String {
    format!("{VERSION_PREFIX}{version}/{path}")
}

/// The canonical text [`version_id`] hashes: `keeper-models-v1\n`, then one
/// `<path>\t<size>\t<sha256>\n` line per file sorted by the path's bytes,
/// then an empty line, then `models.toml`'s bytes. A path holds no tab or
/// newline, so the first empty line ends the list.
///
/// Refuses (`config.malformed`) an empty set, a malformed path or digest,
/// and a path named twice.
pub fn canonical_text(files: &[ModelFile], models_toml: &str) -> Result<String, Refusal> {
    if files.is_empty() {
        return Err(Refusal::ConfigMalformed);
    }
    let mut sorted: Vec<&ModelFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let mut text = String::with_capacity(VERSION_DOMAIN.len() + 2 + models_toml.len());
    text.push_str(VERSION_DOMAIN);
    text.push('\n');
    let mut previous: Option<&str> = None;
    for file in sorted {
        check_path(&file.path)?;
        if !is_sha256_hex(&file.sha256) || previous == Some(file.path.as_str()) {
            return Err(Refusal::ConfigMalformed);
        }
        previous = Some(file.path.as_str());
        text.push_str(&file.path);
        text.push('\t');
        text.push_str(&file.size.to_string());
        text.push('\t');
        text.push_str(&file.sha256);
        text.push('\n');
    }
    text.push('\n');
    text.push_str(models_toml);
    Ok(text)
}

/// A set's version id: the lowercase hex sha256 of [`canonical_text`]. The
/// order the files are given in does not matter; any change to a path, a
/// size, a digest or `models.toml` gives another id.
pub fn version_id(files: &[ModelFile], models_toml: &str) -> Result<String, Refusal> {
    let text = canonical_text(files, models_toml)?;
    Ok(hex(&Sha256::digest(text.as_bytes())))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Refuse every writer but the operator's admin path, and every domain but
/// the general one.
fn check_writer(writer: &ConfigWriter, domain: DomainKind) -> Result<(), Refusal> {
    if matches!(writer, ConfigWriter::Admin) && domain == DomainKind::General {
        Ok(())
    } else {
        Err(Refusal::ConfigWriterRefused)
    }
}

/// Check one admin-path upload grant for a model file: the operator's admin
/// path in the general domain only (`config.writer_refused`), a well-formed
/// digest (`config.malformed`), at most `modelFileMaxBytes`
/// (`config.model_too_large`). The grant binds one PUT to `size` and `sha256`.
pub fn check_model_upload(
    writer: &ConfigWriter,
    domain: DomainKind,
    size: u64,
    sha256: &str,
    limits: &Limits,
) -> Result<(), Refusal> {
    check_writer(writer, domain)?;
    if !is_sha256_hex(sha256) {
        return Err(Refusal::ConfigMalformed);
    }
    if size > limits.model_file_max_bytes {
        return Err(Refusal::ModelTooLarge);
    }
    Ok(())
}

/// Whether a manifest's value, measured as compact JSON in UTF-8 bytes, is
/// longer than `settingValueMaxBytes`.
fn manifest_too_large(manifest: &Manifest, limits: &Limits) -> bool {
    serde_json::to_string(manifest).map_or(true, |text| {
        text.len() as u64 > limits.setting_value_max_bytes
    })
}

/// Check a set before anything of it is written, in this order: every path
/// and digest well formed, no path twice, its manifest value within
/// `settingValueMaxBytes`, and every file row's key within the key bound
/// (`config.malformed`); no key and no `models.toml` naming a domain
/// (`config.names_domain`); no file above `modelFileMaxBytes`
/// (`config.model_too_large`). Returns its version id. Every refusal
/// `check_model_write` can give a well-formed set's own rows comes here
/// first; `hq-admin models put` runs it before its first upload.
pub fn check_set(
    files: &[ModelFile],
    models_toml: &str,
    domains: &[&str],
    limits: &Limits,
) -> Result<String, Refusal> {
    let version = version_id(files, models_toml)?;
    let manifest = Manifest {
        models_toml: models_toml.to_owned(),
        file_count: files.len() as u64,
    };
    if manifest_too_large(&manifest, limits) {
        return Err(Refusal::ConfigMalformed);
    }
    if files
        .iter()
        .any(|file| file_key(&version, &file.path).len() > MAX_KEY_BYTES)
    {
        return Err(Refusal::ConfigMalformed);
    }
    let manifest_key = format!("models/{version}");
    if names_domain(&manifest_key, domains)
        || names_domain(models_toml, domains)
        || files
            .iter()
            .any(|file| names_domain(&format!("{manifest_key}/{}", file.path), domains))
    {
        return Err(Refusal::ConfigNamesDomain);
    }
    if files
        .iter()
        .any(|file| file.size > limits.model_file_max_bytes)
    {
        return Err(Refusal::ModelTooLarge);
    }
    Ok(version)
}

/// Check one `config_values` write in the model namespace, in this order:
///
/// 1. writer and domain (`config.writer_refused`);
/// 2. key grammar (`config.malformed`);
/// 3. a key or `models.toml` naming a domain (`config.names_domain`);
/// 4. the row's value and blob shape (`config.malformed`; a file above
///    `modelFileMaxBytes` is `config.model_too_large`);
/// 5. sealing and completeness against `version`, what the server holds of
///    the version the key names (for `models.active`, the version its value
///    names): a file row or manifest already present, or a file row after
///    the manifest, is `config.model_sealed`; a manifest whose count or id
///    the present rows do not match, or `models.active` naming a version
///    with no manifest, is `config.model_incomplete`.
pub fn check_model_write(
    writer: &ConfigWriter,
    domain: DomainKind,
    write: &ModelWrite<'_>,
    version: &VersionState,
    domains: &[&str],
    limits: &Limits,
) -> Result<(), Refusal> {
    check_writer(writer, domain)?;
    let parsed = parse_key(write.key)?;
    if names_domain(write.key, domains) {
        return Err(Refusal::ConfigNamesDomain);
    }
    match parsed {
        ModelKey::File { path, .. } => {
            let file: FileValue = serde_json::from_value(write.value.clone())
                .map_err(|_| Refusal::ConfigMalformed)?;
            if !is_sha256_hex(&file.sha256) {
                return Err(Refusal::ConfigMalformed);
            }
            let blob = write.blob.ok_or(Refusal::ConfigMalformed)?;
            if blob.blob_id != format!("sha256:{}", file.sha256) || blob.byte_length != file.size {
                return Err(Refusal::ConfigMalformed);
            }
            if file.size > limits.model_file_max_bytes {
                return Err(Refusal::ModelTooLarge);
            }
            if version.manifest.is_some() || version.files.iter().any(|f| f.path == path) {
                return Err(Refusal::ModelSealed);
            }
            Ok(())
        }
        ModelKey::Manifest { version: id } => {
            let manifest: Manifest = serde_json::from_value(write.value.clone())
                .map_err(|_| Refusal::ConfigMalformed)?;
            if write.blob.is_some() || manifest_too_large(&manifest, limits) {
                return Err(Refusal::ConfigMalformed);
            }
            if names_domain(&manifest.models_toml, domains) {
                return Err(Refusal::ConfigNamesDomain);
            }
            if version.manifest.is_some() {
                return Err(Refusal::ModelSealed);
            }
            let complete = version.files.len() as u64 == manifest.file_count
                && version_id(&version.files, &manifest.models_toml).as_deref() == Ok(id.as_str());
            if complete {
                Ok(())
            } else {
                Err(Refusal::ModelIncomplete)
            }
        }
        ModelKey::Active => {
            let named = write.value.as_str().ok_or(Refusal::ConfigMalformed)?;
            if write.blob.is_some() || !is_sha256_hex(named) {
                return Err(Refusal::ConfigMalformed);
            }
            match &version.manifest {
                Some(manifest) if manifest.file_count == version.files.len() as u64 => Ok(()),
                _ => Err(Refusal::ModelIncomplete),
            }
        }
    }
}

/// Who asks to read a `config_values.value_blob`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigBlobReader {
    /// Kind of caller.
    pub kind: CallerKind,
    /// Whether the caller is a member of the row's domain.
    pub member: bool,
}

/// Decide a `config_values.value_blob` read through the generic download
/// route: granted to a person who is a member of the row's domain (for a
/// model, the `makidrive` domain); anyone else — a non-member person, an
/// agent, a tool, the service — gets the absent-blob answer.
#[must_use]
pub fn read_config_blob(reader: &ConfigBlobReader) -> BlobAnswer {
    if reader.kind == CallerKind::Person && reader.member {
        BlobAnswer::Granted
    } else {
        BlobAnswer::Absent
    }
}
