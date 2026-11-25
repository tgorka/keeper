//! The organisation's model set on this device (story 121.3, DF/AD-16).
//!
//! The set is `config_values` rows of the general domain, as
//! `keeper_settings_api::models` defines them: `models.active` names a
//! version, `models/<version>` is its manifest (`models.toml` and the file
//! count) and `models/<version>/<path>` one file (`{size, sha256}` and its
//! blob). [`active_set`] reads them from the domain's replica — never the
//! network — and accepts a version only when it is complete: its manifest is
//! there, every file row is, and the rows hash to the version id.
//! [`hydrate`] lays one group of it out under a destination, each file
//! fetched through the blob port's download ([`FileTransfer::download`]:
//! presigned, ranged and resumed, from the store origin the server named).
//!
//! # What it promises
//!
//! * A file is published only after its whole size and sha256 matched its
//!   row, and every file is published by rename: a reader of `dest` sees the
//!   old file or the whole new one, never a torn one. A file whose bytes
//!   differ is refused, removed from staging and never placed.
//! * A file already in place is not fetched again. The group's
//!   [`state_file`] remembers which content each path holds, so a launch
//!   does not re-hash hundreds of megabytes; a path it does not remember is
//!   hashed once and adopted when it matches.
//! * Files in `dest` the set no longer names are left alone.
//! * A group is either complete or not ready: its [`complete_file`] is
//!   removed before the run's first change to `dest` and written last, after
//!   everything is in place, recording the version and the group's digest —
//!   so a half-updated set (new encoder beside an old decoder) is never
//!   loaded. Each group keeps its own marker and state. A loader keys on
//!   [`ready_digest`], read at its job's start, so a new version is taken at
//!   the next job, never under one running.
//! * A set whose paths collide caselessly (`Encoder` and `encoder`) or as
//!   a file and a folder (`a` and `a/b`) is refused whole before anything is
//!   written: on a case-insensitive volume one would overwrite the other.
//! * Every path is joined through `keeper-paths`, and one landing outside
//!   `dest` (a folder link on the way) is refused.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use keeper_ports::transfer::FileTransfer;
use keeper_ports::{Replica, ReplicaError};
use keeper_settings_api::models::{
    file_key, is_sha256_hex, manifest_key, parse_key, version_id, BlobRef, FileValue, Manifest,
    ModelFile, ModelKey, ACTIVE_KEY, MODELS_TOML, VERSION_PREFIX,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The staging directory downloads land in before they are moved into place:
/// inside `dest`, so the move is a rename on one filesystem, and kept between
/// runs so an interrupted download resumes from its `.part`.
const STORE_DIR: &str = ".keeper-hydrate-blobs";

const STATE_VERSION: u32 = 2;

/// sha256 of no bytes: the one file needing no download.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The record, inside `dest`, of which content each path of `group` holds.
#[must_use]
pub fn state_file(group: &str) -> String {
    format!(".keeper-hydrate.{group}.json")
}

/// The completion marker of `group`, inside `dest`: `{ "digest", "version" }`,
/// written last and only by a run that placed everything. Its absence means
/// `dest` may hold a mix of two sets of the group.
#[must_use]
pub fn complete_file(group: &str) -> String {
    format!(".keeper-models-complete.{group}.json")
}

/// Which top-level folders of the set one group takes. The plain files
/// directly under the set's root (`models.toml`, a README) belong to every
/// group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folders<'a> {
    /// These folders and no other.
    Only(&'a [String]),
    /// Every folder but these.
    AllBut(&'a [String]),
}

impl Folders<'_> {
    fn takes(self, folder: &str) -> bool {
        match self {
            Self::Only(names) => names.iter().any(|name| name == folder),
            Self::AllBut(names) => !names.iter().any(|name| name == folder),
        }
    }
}

/// The part of the set one run brings up to date, as a unit: one completion
/// marker and one state file per group. Runs of two groups into one `dest`
/// must not overlap — they share the staging directory.
#[derive(Clone, Copy)]
pub struct Group<'a> {
    /// Lowercase ASCII letters; names the group's marker and state file.
    pub name: &'a str,
    pub folders: Folders<'a>,
    /// What of `models.toml` this group depends on, for its digest; `None`
    /// (or an answer of `None`: it does not parse) counts the bytes whole.
    pub fingerprint: Option<Fingerprint>,
}

/// What a group reads of `models.toml`'s bytes; `None` when they do not parse.
pub type Fingerprint = fn(&[u8]) -> Option<String>;

impl std::fmt::Debug for Group<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Group")
            .field("name", &self.name)
            .field("folders", &self.folders)
            .finish_non_exhaustive()
    }
}

impl Group<'_> {
    /// Whether this group takes the set's file at `path`.
    #[must_use]
    pub fn takes(&self, path: &str) -> bool {
        match path.split_once('/') {
            None => true,
            Some((folder, _)) => self.folders.takes(folder),
        }
    }

    /// The sha256 of what this group reads of `models.toml`.
    fn manifest_oid(&self, models_toml: &str) -> String {
        let bytes = models_toml.as_bytes();
        match self.fingerprint.and_then(|fingerprint| fingerprint(bytes)) {
            Some(meaning) => sha256_hex(meaning.as_bytes()),
            None => sha256_hex(bytes),
        }
    }
}

/// One file of the active set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetFile {
    /// `/`-separated path under the set's root.
    pub path: String,
    pub size: u64,
    /// 64 lowercase hex digits.
    pub sha256: String,
}

impl SetFile {
    /// The blob id its row's `value_blob` names: `sha256:<sha256>`.
    #[must_use]
    pub fn blob_id(&self) -> String {
        format!("sha256:{}", self.sha256)
    }
}

/// The version `models.active` names, complete: every file of it, sorted by
/// path, and its `models.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSet {
    pub version: String,
    pub models_toml: String,
    pub files: Vec<SetFile>,
}

impl ActiveSet {
    /// Every file path of the set, in path order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|file| file.path.as_str())
    }
}

/// What one hydration did. Every file the group takes is counted once;
/// `models.toml` counts as copied or skipped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HydrateReport {
    /// Files fetched from the store.
    pub downloaded: u64,
    pub downloaded_bytes: u64,
    /// Files written from the rows themselves (`models.toml`, empty files).
    pub copied: u64,
    pub copied_bytes: u64,
    /// Files already in place.
    pub skipped: u64,
}

/// Why the model set could not be read or brought here.
#[derive(Debug, thiserror::Error)]
pub enum ModelsError {
    /// The version `models.active` names is not complete in the replica, or
    /// its rows do not hash to it: nothing of it is used.
    #[error("the model set {version} is not complete: {detail}")]
    Incomplete { version: String, detail: String },
    /// Two paths of the set name one entry on a case-insensitive volume, or
    /// one is a file and a folder at once.
    #[error("the model set names {first} and {second}, which would overwrite each other here")]
    Collision { first: String, second: String },
    /// A file's bytes are not the ones its row names; nothing of it was placed.
    #[error("{path} arrived with other bytes than the model set names ({detail})")]
    Integrity { path: String, detail: String },
    /// A group named with anything but lowercase letters.
    #[error("a model group is named with lowercase letters, not {0:?}")]
    Group(String),
    /// `keeper-paths` refused where a file would land.
    #[error("{path} cannot be placed: {detail}")]
    Path { path: String, detail: String },
    /// The blob port could not bring a file.
    #[error("{path} could not be fetched: {error}")]
    Transfer { path: String, error: ReplicaError },
    /// The replica could not be read.
    #[error(transparent)]
    Replica(#[from] ReplicaError),
    /// The run was asked to stop.
    #[error("the model fetch was stopped")]
    Cancelled,
    /// A local file could not be read or written.
    #[error("{0}")]
    Storage(String),
}

fn storage(what: &str, path: &Path, error: &std::io::Error) -> ModelsError {
    ModelsError::Storage(format!("cannot {what} {}: {error}", path.display()))
}

fn incomplete(version: &str, detail: impl Into<String>) -> ModelsError {
    ModelsError::Incomplete {
        version: version.to_owned(),
        detail: detail.into(),
    }
}

/// A JSON column as the replica answers it: its text parsed, or the value.
fn json_column(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

/// The set `models.active` names in `replica` (the general domain's), read
/// from the replica alone. `Ok(None)` when no version is active. A version
/// that is not complete, whose rows do not hash to its id, or whose paths
/// collide is refused ([`ModelsError::Incomplete`], [`ModelsError::Collision`]).
pub fn active_set(replica: &dyn Replica) -> Result<Option<ActiveSet>, ModelsError> {
    let rows = replica.query(
        "SELECT value FROM config_values WHERE key = ?",
        &[json!(ACTIVE_KEY)],
    )?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let version = match json_column(row.get("value")) {
        Value::Null => return Ok(None),
        Value::String(version) => version,
        other => {
            return Err(incomplete(
                &other.to_string(),
                "models.active is not a version id",
            ))
        }
    };
    if !is_sha256_hex(&version) {
        return Err(incomplete(&version, "models.active is not a version id"));
    }

    let rows = replica.query(
        "SELECT value, value_blob FROM config_values WHERE key = ?",
        &[json!(manifest_key(&version))],
    )?;
    let Some(row) = rows.first() else {
        return Err(incomplete(&version, "its manifest is not here"));
    };
    if !json_column(row.get("value_blob")).is_null() {
        return Err(incomplete(&version, "its manifest carries a blob"));
    }
    let manifest: Manifest = serde_json::from_value(json_column(row.get("value")))
        .map_err(|error| incomplete(&version, format!("its manifest does not read: {error}")))?;

    // Every key under `models/<version>/`, compared as bytes: `/` + 1 is `0`.
    let low = format!("{VERSION_PREFIX}{version}/");
    let high = format!("{VERSION_PREFIX}{version}0");
    let rows = replica.query(
        "SELECT key, value, value_blob FROM config_values WHERE key >= ? AND key < ? ORDER BY key",
        &[json!(low), json!(high)],
    )?;
    let mut files = Vec::with_capacity(rows.len());
    for row in &rows {
        let key = row.get("key").and_then(Value::as_str).unwrap_or_default();
        files.push(file_of(&version, key, row)?);
    }
    if u64::try_from(files.len()).ok() != Some(manifest.file_count) {
        return Err(incomplete(
            &version,
            format!(
                "{} of its {} file rows are here",
                files.len(),
                manifest.file_count
            ),
        ));
    }
    let listed: Vec<ModelFile> = files
        .iter()
        .map(|file: &SetFile| ModelFile {
            path: file.path.clone(),
            size: file.size,
            sha256: file.sha256.clone(),
        })
        .collect();
    let computed = version_id(&listed, &manifest.models_toml)
        .map_err(|refusal| incomplete(&version, format!("its rows are refused: {refusal:?}")))?;
    if computed != version {
        return Err(incomplete(&version, format!("its rows hash to {computed}")));
    }
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    check_collisions(files.iter().map(|file| file.path.as_str()))?;
    Ok(Some(ActiveSet {
        version,
        models_toml: manifest.models_toml,
        files,
    }))
}

/// One file row of `version`, checked against the contract: its key, its
/// value and a blob that names exactly its content.
fn file_of(
    version: &str,
    key: &str,
    row: &serde_json::Map<String, Value>,
) -> Result<SetFile, ModelsError> {
    let path = match parse_key(key) {
        Ok(ModelKey::File { version: of, path }) if of == version => path,
        _ => {
            return Err(incomplete(
                version,
                format!("{key:?} is not one of its file rows"),
            ))
        }
    };
    let value: FileValue = serde_json::from_value(json_column(row.get("value")))
        .map_err(|error| incomplete(version, format!("{path}: {error}")))?;
    if !is_sha256_hex(&value.sha256) {
        return Err(incomplete(
            version,
            format!("{path}: its sha256 is malformed"),
        ));
    }
    let blob: BlobRef = serde_json::from_value(json_column(row.get("value_blob")))
        .map_err(|error| incomplete(version, format!("{path}: its blob: {error}")))?;
    if blob.blob_id != format!("sha256:{}", value.sha256) || blob.byte_length != value.size {
        return Err(incomplete(
            version,
            format!("{path}: its blob names other content than its row"),
        ));
    }
    debug_assert_eq!(file_key(version, &path), key);
    Ok(SetFile {
        path,
        size: value.size,
        sha256: value.sha256,
    })
}

/// Refuse two paths naming one entry under `keeper_paths::names::name_fold`
/// (the rule `syncular/apfs-caseless-sweep.txt` measured), and a path that
/// is both a file and a folder of another.
pub fn check_collisions<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<(), ModelsError> {
    let mut files: BTreeMap<String, &str> = BTreeMap::new();
    let mut folders: BTreeMap<String, &str> = BTreeMap::new();
    for path in paths {
        let segments: Vec<String> = path
            .split('/')
            .map(keeper_paths::names::name_fold)
            .collect();
        let folded = segments.join("/");
        if let Some(first) = files.insert(folded.clone(), path) {
            return Err(ModelsError::Collision {
                first: first.to_owned(),
                second: path.to_owned(),
            });
        }
        if let Some(first) = folders.get(&folded) {
            return Err(ModelsError::Collision {
                first: (*first).to_owned(),
                second: path.to_owned(),
            });
        }
        for depth in 1..segments.len() {
            let folder = segments[..depth].join("/");
            if let Some(first) = files.get(&folder) {
                return Err(ModelsError::Collision {
                    first: (*first).to_owned(),
                    second: path.to_owned(),
                });
            }
            folders.entry(folder).or_insert(path);
        }
    }
    Ok(())
}

/// sha256 over `"<path>\t<sha256>\n"` for every file `group` takes and
/// `models.toml` (by what the group reads of it), in path order: what the
/// group's marker records. A new version that changes nothing the group
/// takes keeps its digest, so its loaders keep what they hold.
#[must_use]
pub fn group_digest(set: &ActiveSet, group: Group<'_>) -> String {
    let mut listing: Vec<(&str, String)> = set
        .files
        .iter()
        .filter(|file| group.takes(&file.path))
        .map(|file| (file.path.as_str(), file.sha256.clone()))
        .collect();
    listing.push((MODELS_TOML, group.manifest_oid(&set.models_toml)));
    listing.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut text = String::new();
    for (path, oid) in listing {
        text.push_str(path);
        text.push('\t');
        text.push_str(&oid);
        text.push('\n');
    }
    sha256_hex(text.as_bytes())
}

/// What a group's [`complete_file`] holds.
#[derive(Debug, Serialize, Deserialize)]
struct Complete {
    digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

fn read_complete(dest: &Path, group: &str) -> Option<Complete> {
    let bytes = std::fs::read(dest.join(complete_file(group))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The digest the last complete hydration of `group` into `dest` recorded,
/// or `None` while that group is not known to be complete — what a model
/// loader keys on at its job's start, so a changed set is loaded afresh.
#[must_use]
pub fn ready_digest(dest: &Path, group: &str) -> Option<String> {
    read_complete(dest, group).map(|complete| complete.digest)
}

/// The version the last complete hydration of `group` into `dest` came
/// from, or `None` while the group is not complete (or was completed before
/// versions were recorded).
#[must_use]
pub fn ready_version(dest: &Path, group: &str) -> Option<String> {
    read_complete(dest, group).and_then(|complete| complete.version)
}

/// Whether `dest` holds, completely, what `group` takes of `set`: its
/// marker records the digest `set` gives the group now.
#[must_use]
pub fn is_current(set: &ActiveSet, group: Group<'_>, dest: &Path) -> bool {
    ready_digest(dest, group.name).is_some_and(|digest| digest == group_digest(set, group))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    version: u32,
    /// Path → the sha256 of what it holds.
    objects: BTreeMap<String, String>,
}

/// Bring `dest` up to date with `group`'s part of `set`. Each file the group
/// takes and `dest` does not hold already is fetched through `transfer` into
/// the staging directory, checked whole (size and sha256) and renamed into
/// place; `models.toml` is written from the manifest. One file failing does
/// not stop the others: everything that did arrive is placed and recorded,
/// and the first failure is returned, so the next run fetches only what is
/// missing. `interrupt` is looked at between files.
///
/// The group's [`complete_file`] is removed before `dest` is first changed
/// and on any failure, and written — last — only when every file of the
/// group is in place. Blocking: run it off any async worker.
pub fn hydrate(
    set: &ActiveSet,
    group: Group<'_>,
    transfer: &dyn FileTransfer,
    dest: &Path,
    interrupt: &AtomicBool,
) -> Result<HydrateReport, ModelsError> {
    if group.name.is_empty() || !group.name.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err(ModelsError::Group(group.name.to_owned()));
    }
    check_collisions(set.paths())?;
    std::fs::create_dir_all(dest).map_err(|error| storage("create", dest, &error))?;
    let complete = dest.join(complete_file(group.name));
    let state_path = dest.join(state_file(group.name));
    let previous = read_state(&state_path);
    let mut next = State {
        version: STATE_VERSION,
        objects: BTreeMap::new(),
    };
    let mut report = HydrateReport::default();
    let mut first_failure: Option<ModelsError> = None;

    for file in set.files.iter().filter(|file| group.takes(&file.path)) {
        if interrupt.load(Ordering::Relaxed) {
            write_state(&state_path, &next)?;
            unmark(&complete)?;
            return Err(ModelsError::Cancelled);
        }
        let target = place_of(dest, &file.path)?;
        let remembered = previous.objects.get(&file.path).map(String::as_str);
        if holds(remembered, file, &target)? {
            report.skipped += 1;
            next.objects.insert(file.path.clone(), file.sha256.clone());
            continue;
        }
        unmark(&complete)?;
        let placed = if file.size == 0 {
            place_empty(file, &target).map(|()| {
                report.copied += 1;
            })
        } else {
            fetch_and_place(file, transfer, dest, &target).map(|()| {
                report.downloaded += 1;
                report.downloaded_bytes += file.size;
            })
        };
        match placed {
            Ok(()) => {
                next.objects.insert(file.path.clone(), file.sha256.clone());
            }
            Err(error) => {
                if first_failure.is_none() {
                    first_failure = Some(error);
                }
            }
        }
    }

    // `models.toml` from the manifest, which every group reads.
    let manifest_target = place_of(dest, MODELS_TOML)?;
    let bytes = set.models_toml.as_bytes();
    if std::fs::read(&manifest_target).is_ok_and(|held| held == bytes) {
        report.skipped += 1;
    } else {
        let same_meaning = group.fingerprint.is_some_and(|fingerprint| {
            std::fs::read(&manifest_target)
                .ok()
                .and_then(|held| fingerprint(&held))
                .is_some_and(|held| fingerprint(bytes) == Some(held))
        });
        if !same_meaning {
            unmark(&complete)?;
        }
        write_atomic(&manifest_target, bytes)?;
        report.copied += 1;
        report.copied_bytes += u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    }

    // Written whatever the fetches did: what was placed before a failure is
    // in place, and the next run should not fetch or hash it again.
    write_state(&state_path, &next)?;
    if let Some(error) = first_failure {
        unmark(&complete)?;
        return Err(error);
    }
    let marker = serde_json::to_vec_pretty(&Complete {
        digest: group_digest(set, group),
        version: Some(set.version.clone()),
    })
    .map_err(|error| ModelsError::Storage(format!("cannot encode the marker: {error}")))?;
    write_atomic(&complete, &marker)?;
    Ok(report)
}

/// Where `path` lands under `dest`, through `keeper-paths`: its plain
/// segments joined, and refused when what exists of it lands outside
/// `dest` (a folder link on the way), elsewhere inside it, or a link points
/// nowhere.
fn place_of(dest: &Path, path: &str) -> Result<PathBuf, ModelsError> {
    let refused = |detail: String| ModelsError::Path {
        path: path.to_owned(),
        detail,
    };
    let target =
        keeper_paths::lexical_join(dest, path).map_err(|error| refused(error.to_string()))?;
    let landed = keeper_paths::landing(dest, path).map_err(|error| refused(error.to_string()))?;
    if !lands_as_asked(&landed, path) {
        return Err(refused("it lands elsewhere on this disk".to_owned()));
    }
    Ok(target)
}

/// Whether the names `path` landed on are the names it asked for, compared
/// as a case-insensitive volume compares them: on APFS the landing of an
/// existing `Vocab.json` asked for as `vocab.json` reads back in the case
/// stored on disk, and a version that only changes a path's case must not
/// be refused there for ever.
fn lands_as_asked(landed: &[String], path: &str) -> bool {
    let asked: Vec<&str> = path.split('/').collect();
    landed.len() == asked.len()
        && landed
            .iter()
            .zip(asked)
            .all(|(landed, asked)| keeper_paths::names::same_entry_folded(landed, asked))
}

/// Whether `target` already holds `file`. A remembered digest with the right
/// length is believed without reading the file; anything else of the right
/// length is hashed.
fn holds(remembered: Option<&str>, file: &SetFile, target: &Path) -> Result<bool, ModelsError> {
    let Ok(meta) = std::fs::symlink_metadata(target) else {
        return Ok(false);
    };
    if !meta.is_file() || meta.len() != file.size {
        return Ok(false);
    }
    if remembered == Some(file.sha256.as_str()) {
        return Ok(true);
    }
    Ok(digest_of(target)? == file.sha256)
}

fn place_empty(file: &SetFile, target: &Path) -> Result<(), ModelsError> {
    if file.sha256 != EMPTY_SHA256 {
        return Err(ModelsError::Integrity {
            path: file.path.clone(),
            detail: "an empty file with another digest".to_owned(),
        });
    }
    create_parent(target)?;
    write_atomic(target, &[])
}

/// Fetch `file` into staging (resumed from its `.part`), check its whole
/// size and sha256, and rename it onto `target`. A staged body already
/// whole is used without a request.
fn fetch_and_place(
    file: &SetFile,
    transfer: &dyn FileTransfer,
    dest: &Path,
    target: &Path,
) -> Result<(), ModelsError> {
    let store = dest.join(STORE_DIR);
    std::fs::create_dir_all(&store).map_err(|error| storage("create", &store, &error))?;
    let staged = store.join(&file.sha256);
    if verify(file, &staged).is_err() {
        // A staged body that is not whole is the download's to resume from
        // its `.part`; one that is wrong is dropped first.
        let _ = std::fs::remove_file(&staged);
        transfer
            .download(&file.blob_id(), file.size, &staged, &mut |_| {})
            .map_err(|error| ModelsError::Transfer {
                path: file.path.clone(),
                error,
            })?;
        if let Err(detail) = verify(file, &staged) {
            let _ = std::fs::remove_file(&staged);
            return Err(ModelsError::Integrity {
                path: file.path.clone(),
                detail,
            });
        }
    }
    create_parent(target)?;
    std::fs::rename(&staged, target).map_err(|error| storage("place", target, &error))
}

/// `Ok` when `path` is a regular file of `file`'s size and sha256; else why not.
fn verify(file: &SetFile, path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_owned());
    }
    if meta.len() != file.size {
        return Err(format!(
            "{} bytes where {} were named",
            meta.len(),
            file.size
        ));
    }
    let digest = digest_of(path).map_err(|error| error.to_string())?;
    if digest != file.sha256 {
        return Err(format!("sha256 {digest}"));
    }
    Ok(())
}

fn digest_of(path: &Path) -> Result<String, ModelsError> {
    let mut reader = std::fs::File::open(path).map_err(|error| storage("read", path, &error))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|error| storage("read", path, &error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn create_parent(target: &Path) -> Result<(), ModelsError> {
    match target.parent() {
        Some(parent) => {
            std::fs::create_dir_all(parent).map_err(|error| storage("create", parent, &error))
        }
        None => Ok(()),
    }
}

/// Write `bytes` beside `target` and rename it over.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), ModelsError> {
    use std::io::Write;
    let parent = target
        .parent()
        .ok_or_else(|| ModelsError::Storage(format!("{} has no folder", target.display())))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| storage("stage", target, &error))?;
    staged
        .write_all(bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|error| storage("write", target, &error))?;
    staged
        .persist(target)
        .map_err(|error| storage("place", target, &error.error))?;
    Ok(())
}

/// Remove a group's completion marker; absent already is fine.
fn unmark(complete: &Path) -> Result<(), ModelsError> {
    match std::fs::remove_file(complete) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage("remove", complete, &error)),
    }
}

/// The state a previous run left; empty when there is none or it cannot be
/// read — which costs one hash per file, never a wrong skip. A state written
/// by the config repository's hydration (version 1) names LFS oids, which
/// are the same sha256 of the content, so it is read alike.
fn read_state(path: &Path) -> State {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<State>(&bytes).ok())
        .unwrap_or_default()
}

fn write_state(path: &Path, state: &State) -> Result<(), ModelsError> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| ModelsError::Storage(format!("cannot encode the state: {error}")))?;
    write_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caseless_and_file_folder_collisions_are_refused() {
        assert!(check_collisions(["asr/Encoder.bin", "asr/decoder.bin"]).is_ok());
        assert!(matches!(
            check_collisions(["asr/Encoder.bin", "asr/encoder.bin"]),
            Err(ModelsError::Collision { .. })
        ));
        assert!(matches!(check_collisions(["ASR/a", "asr/b"]), Ok(())));
        assert!(matches!(
            check_collisions(["a", "a/b"]),
            Err(ModelsError::Collision { .. })
        ));
        assert!(matches!(
            check_collisions(["A/b", "a"]),
            Err(ModelsError::Collision { .. })
        ));
    }

    /// APFS simulated: `landing` reads an existing entry back in the case
    /// stored on disk, so a version renaming `Vocab.json` to `vocab.json`
    /// (or folder `ASR` to `asr`) lands on the old spelling. That is the
    /// same entry under the contract's caseless rule, never "elsewhere".
    #[test]
    fn a_case_only_rename_lands_as_asked_on_a_caseless_volume() {
        let names = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
        assert!(lands_as_asked(&names(&["Vocab.json"]), "vocab.json"));
        assert!(lands_as_asked(
            &names(&["ASR", "encoder.bin"]),
            "asr/encoder.bin"
        ));
        assert!(lands_as_asked(
            &names(&["asr", "encoder.bin"]),
            "asr/encoder.bin"
        ));
        assert!(!lands_as_asked(
            &names(&["other", "encoder.bin"]),
            "asr/encoder.bin"
        ));
        assert!(!lands_as_asked(&names(&["encoder.bin"]), "asr/encoder.bin"));
        assert!(!lands_as_asked(
            &names(&["asr", "x", "encoder.bin"]),
            "asr/encoder.bin"
        ));
    }

    #[test]
    fn a_group_takes_its_folders_and_every_plain_file() {
        let turn = ["vad".to_owned()];
        let group = Group {
            name: "turn",
            folders: Folders::Only(&turn),
            fingerprint: None,
        };
        assert!(group.takes("README"));
        assert!(group.takes("vad/model.onnx"));
        assert!(!group.takes("asr/encoder.bin"));
    }
}
