//! The folder tier: a synced folder's own `.keeper/*.toml` (Story 46.8,
//! AD-99 … AD-101).
//!
//! ```text
//! <folder>/.keeper/keeper.toml           this folder, every machine
//! <folder>/.keeper/keeper.<host>.toml    this folder, this machine
//! ```
//!
//! Both files **sync** — the machine-variant one deliberately, because that is
//! how one machine's settings get edited from another (AD-100 carves them out
//! of the `.keeper/` exclusion in [`crate::exclude`]).
//!
//! # A folder file configures the FOLDER, never the app
//!
//! `[folder]` is a partial [`SyncProfile`], overlaid onto the row `sync.db`
//! holds. `[settings]` — the app's settings-table keys — is refused here by
//! name, because a file that travels with a folder must not be able to say what
//! `hotkey.global` is: two folders would fight over it and the winner would be
//! whichever profile was read last. Only the main sync folder may carry
//! `[settings]`, and that file is read by `keeper-core`'s layer stack rather
//! than by this module, which ignores the table there.
//!
//! Not every profile field is about the folder. `localPath` is where *this*
//! clone mounted it, `volumeId` is which stick this clone adopted, `id` and
//! `remoteUrl` are what the folder *is*. A file that travels cannot carry any
//! of them, so [`folder_field_rule`] classifies **every** field — the set is
//! checked against [`super::accepted_profile_keys`] by a test, so a field added
//! to [`SyncProfile`] tomorrow forces the decision rather than defaulting into
//! whichever half is more convenient.
//!
//! # Resolved at read time, and never written back
//!
//! AD-98's whole point: a file that is *imported* wins once and is erased by the
//! next UI toggle. So the overlay happens in [`crate::db::list_profiles`] and
//! [`crate::db::get_profile`], every time, and [`as_stored`] takes it back off
//! again before anything is written — the table never learns what the file said,
//! which is what lets the file keep winning and what makes deleting the file
//! restore the stored value rather than reveal a copy of the file's.
//!
//! Uninstalled is the default: until [`install_folder_tier`] is called no folder
//! file is opened, so `keeper-syncd` and every existing test see the table
//! exactly as they always did.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{accepted_profile_keys, canonical_key, canonical_profile_fields, SyncProfile};

/// The per-folder directory both config files live in. The same name
/// [`crate::exclude`] excludes and carves `*.toml` out of.
pub const FOLDER_CONFIG_DIR: &str = ".keeper";

/// The shared file: this folder, on every machine.
const SHARED_FILE: &str = "keeper.toml";

/// Whether a folder file may set one [`SyncProfile`] field, and if not, why.
///
/// Three answers rather than a boolean, because the two refusals are different
/// promises and a person reading the error deserves the one that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderFieldRule {
    /// A fact about the folder, true on every machine that holds it. These are
    /// the fields the tier exists for.
    Allowed,
    /// Says which folder this is, or which repository it belongs to. A file
    /// inside the folder cannot be allowed to re-point the clone that read it.
    Identity,
    /// True of this clone only. Carrying it in a file that travels would apply
    /// one machine's answer to a machine where it is wrong.
    MachineLocal,
}

impl FolderFieldRule {
    /// The rule as a sentence, for the message that refuses a key.
    fn why(self) -> &'static str {
        match self {
            // Never rendered: an allowed field is not refused.
            Self::Allowed => "it is about the folder",
            Self::Identity => {
                "identity — it says which folder this is or which repository it \
                 belongs to, and a file inside the folder must not be able to re-point \
                 the clone that read it"
            }
            Self::MachineLocal => {
                "machine-local — it is true of this clone only, and this file travels \
                 to machines where it is not"
            }
        }
    }
}

/// Every [`SyncProfile`] field, and what a folder file may do with it.
///
/// Listed rather than derived, unlike [`super::accepted_profile_keys`], and
/// deliberately: "may a folder say this" is a judgement, not a fact about the
/// type. `folder_field_rules_cover_every_profile_field` asserts the list is
/// exactly the type's key set, so a new field is a compile-and-test failure
/// here rather than a silent default either way.
const FOLDER_FIELD_RULES: &[(&str, FolderFieldRule)] = &[
    // --- What the folder is -------------------------------------------------
    ("id", FolderFieldRule::Identity),
    ("name", FolderFieldRule::Identity),
    ("remoteUrl", FolderFieldRule::Identity),
    ("volumeId", FolderFieldRule::Identity),
    // --- What this clone is -------------------------------------------------
    ("localPath", FolderFieldRule::MachineLocal),
    ("direction", FolderFieldRule::MachineLocal),
    ("lane", FolderFieldRule::MachineLocal),
    ("subpaths", FolderFieldRule::MachineLocal),
    ("removable", FolderFieldRule::MachineLocal),
    ("lfsMode", FolderFieldRule::MachineLocal),
    ("lfsPruneLocal", FolderFieldRule::MachineLocal),
    ("settleMs", FolderFieldRule::MachineLocal),
    ("pollIntervalMs", FolderFieldRule::MachineLocal),
    ("authorOverride", FolderFieldRule::MachineLocal),
    ("enabled", FolderFieldRule::MachineLocal),
    // --- Repository policy: the same on every clone, or the clones disagree --
    ("branch", FolderFieldRule::Allowed),
    ("excludes", FolderFieldRule::Allowed),
    // Which paths are generated is a fact about the repository's layout, not
    // about the machine holding it: the tool that rebuilds them is committed
    // beside them. Letting the folder itself declare them is the point — a
    // repository whose index files are generated says so once, and every clone
    // converges the same way without anybody configuring a device (AD-43).
    ("regenerable", FolderFieldRule::Allowed),
    ("lfsNever", FolderFieldRule::Allowed),
    ("lfsThresholdBytes", FolderFieldRule::Allowed),
    ("commitSubjectTemplate", FolderFieldRule::Allowed),
    ("tags", FolderFieldRule::Allowed),
    // --- What the folder contains -------------------------------------------
    ("notes", FolderFieldRule::Allowed),
    ("recordings", FolderFieldRule::Allowed),
    // A sessions zone is a fact about the repository's layout, exactly as a
    // vault is: every clone holds the same `60-sessions/` tree, so a folder
    // file may say so and every machine that syncs it agrees (AD-107).
    ("sessions", FolderFieldRule::Allowed),
];

/// What a folder file may do with one canonical profile key.
///
/// `None` for a key [`SyncProfile`] does not have. Callers treat that as a
/// refusal: an unclassified key is one nobody decided about, and guessing in
/// the permissive direction is how `localPath` would arrive from a stick.
pub fn folder_field_rule(key: &str) -> Option<FolderFieldRule> {
    FOLDER_FIELD_RULES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, rule)| *rule)
}

/// The keys a folder file may set, for an error that has to list them.
fn allowed_fields() -> Vec<&'static str> {
    FOLDER_FIELD_RULES
        .iter()
        .filter(|(_, rule)| *rule == FolderFieldRule::Allowed)
        .map(|(name, _)| *name)
        .collect()
}

/// One folder config file that could not be applied, and why.
///
/// Never fatal. A folder whose file is broken syncs with the settings the
/// database holds, and this is what the settings surface shows so the person
/// who typed it finds out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderFault {
    /// The file. Always named — with five possible layers, a message that does
    /// not say which file it is about sends people editing the wrong one.
    pub path: PathBuf,
    /// Every problem in that file, joined. All of them at once rather than the
    /// first: fix-one-see-the-next over a five-line config is a bad afternoon.
    pub message: String,
}

impl FolderFault {
    fn new(path: &Path, message: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

/// The result of layering a profile's own files onto it.
#[derive(Debug, Clone)]
pub struct FolderOutcome {
    /// The profile as it is in force: the stored row with every applied layer
    /// on top. Identical to the input when there are no files, or when every
    /// file was refused.
    pub profile: SyncProfile,
    /// The canonical profile keys the files set, which is what [`as_stored`]
    /// takes back off before a write.
    pub owned: BTreeSet<String>,
    /// The files that could not be applied.
    pub faults: Vec<FolderFault>,
}

/// The folder tier for this process: which machine this is, and which folder is
/// the main one.
///
/// Cheap and cloneable on purpose — it is read on every profile load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderTier {
    host: String,
    main_folder: Option<PathBuf>,
}

impl FolderTier {
    /// `host` is this machine's short label — the `<host>` in
    /// `keeper.<host>.toml`, i.e. what `read_host_label` answers. `main_folder`
    /// is the folder `~/.keeper/keeper.toml` nominated, if it named one.
    pub fn new(host: impl Into<String>, main_folder: Option<PathBuf>) -> Self {
        Self {
            host: host.into(),
            main_folder,
        }
    }

    /// The files to read for one profile root, in precedence order: shared
    /// first, this machine second.
    ///
    /// One entry when the host label cannot make a filename. `read_host_label`
    /// never answers with an empty string or a separator, but this type accepts
    /// any string and a label smuggled in from elsewhere must not be able to
    /// name a file outside `.keeper/`.
    pub fn layer_paths(&self, local_path: &Path) -> Vec<PathBuf> {
        let dir = local_path.join(FOLDER_CONFIG_DIR);
        let mut out = vec![dir.join(SHARED_FILE)];
        let host = self.host.trim();
        if !host.is_empty() && !host.contains(['/', '\\']) {
            out.push(dir.join(format!("keeper.{host}.toml")));
        }
        out
    }

    /// Whether this profile root is the folder `~/.keeper/keeper.toml`
    /// nominated — the one folder whose file may also carry `[settings]`.
    fn is_main(&self, local_path: &Path) -> bool {
        self.main_folder
            .as_deref()
            .is_some_and(|main| main.components().eq(local_path.components()))
    }

    /// Read this profile's folder files and layer them onto it.
    ///
    /// Never fails and never touches the disk beyond opening the two files. A
    /// file that is missing says nothing; a file that is broken, or that says
    /// something a folder may not, is dropped **whole** and reported — a layer
    /// half-applied is a configuration nobody can reason about, and the
    /// alternative to dropping it is deciding on the user's behalf which half
    /// of their file they meant.
    pub fn apply(&self, profile: &SyncProfile) -> FolderOutcome {
        let is_main = self.is_main(&profile.local_path);
        let mut current = profile.clone();
        let mut owned = BTreeSet::new();
        let mut faults = Vec::new();
        for path in self.layer_paths(&profile.local_path) {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                // The ordinary case: most folders have no file at all.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    faults.push(FolderFault::new(&path, format!("cannot be read: {err}")));
                    continue;
                }
            };
            match overlay(&current, &text, is_main) {
                Ok(None) => {}
                Ok(Some(applied)) => {
                    current = applied.profile;
                    owned.extend(applied.keys);
                }
                Err(problems) => faults.push(FolderFault::new(&path, problems.join("; "))),
            }
        }
        FolderOutcome {
            profile: current,
            owned,
            faults,
        }
    }
}

/// One layer, applied.
struct Applied {
    profile: SyncProfile,
    keys: BTreeSet<String>,
}

/// Apply one folder file's text to `profile`.
///
/// `Ok(None)` when the file has nothing to say. `Err` carries every problem in
/// it, each a full sentence naming the key and the rule; the caller prefixes
/// the file.
fn overlay(
    profile: &SyncProfile,
    text: &str,
    is_main: bool,
) -> std::result::Result<Option<Applied>, Vec<String>> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        // `toml`'s Display carries the line, the column and the offending
        // input. Do not flatten it — it is the whole diagnosis.
        Err(err) => return Err(vec![format!("is not readable as TOML\n{err}")]),
    };

    let mut problems = Vec::new();
    let mut requested = None;
    for (key, value) in &table {
        match canonical_key(key).as_str() {
            "folder" => requested = Some(value),
            // The main folder's `[settings]` belongs to keeper-core's layer
            // stack, which reads this same file for that table. Silence here is
            // not indifference: reporting it would give one file two faults
            // from two crates that cannot see each other.
            "settings" if is_main => {}
            "settings" => problems.push(settings_refusal(value)),
            "mainSyncFolder" => problems.push(
                "may not set `mainSyncFolder`: which folder is the main one is a fact about \
                 this machine's account, honoured only in `~/.keeper/keeper.toml`"
                    .to_owned(),
            ),
            other => problems.push(format!(
                "unknown top-level key `{key}`; a folder file carries `[folder]`{}, and \
                 `{other}` is neither",
                if is_main {
                    " and, in the main sync folder, `[settings]`"
                } else {
                    ""
                }
            )),
        }
    }

    let Some(requested) = requested else {
        return if problems.is_empty() {
            Ok(None)
        } else {
            Err(problems)
        };
    };
    let Some(fields) = requested.as_table() else {
        problems.push("`folder` is not a table; write it as `[folder]`".to_owned());
        return Err(problems);
    };
    if fields.is_empty() && problems.is_empty() {
        return Ok(None);
    }

    // TOML and JSON are the same data model for everything a profile contains,
    // so bouncing through `serde_json::Value` buys the exact deserializer the
    // app and the daemon use — no second code path that could read a field
    // differently.
    let value = match serde_json::to_value(fields) {
        Ok(Value::Object(map)) => map,
        Ok(_) | Err(_) => {
            problems.push("`[folder]` is not a table of fields".to_owned());
            return Err(problems);
        }
    };
    let accepted = match accepted_profile_keys() {
        Ok(accepted) => accepted,
        Err(err) => {
            problems.push(err.to_string());
            return Err(problems);
        }
    };
    let canonical = match canonical_profile_fields(value, &accepted) {
        Ok(canonical) => canonical,
        Err(err) => {
            problems.push(format!("[folder] {err}"));
            return Err(problems);
        }
    };

    let mut overlay = serde_json::Map::with_capacity(canonical.len());
    for (name, field) in canonical {
        match folder_field_rule(&name) {
            Some(FolderFieldRule::Allowed) => {
                overlay.insert(name, field);
            }
            rule => problems.push(format!(
                "[folder] may not set `{name}`: {}. A folder file may set {}",
                rule.unwrap_or(FolderFieldRule::MachineLocal).why(),
                allowed_fields().join(", ")
            )),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    if overlay.is_empty() {
        return Ok(None);
    }

    let requested = Value::Object(overlay);
    let mut merged = match serde_json::to_value(profile) {
        Ok(merged) => merged,
        Err(err) => return Err(vec![format!("[folder] cannot be applied: {err}")]),
    };
    merge(&mut merged, &requested);
    let next: SyncProfile = match serde_json::from_value(merged) {
        Ok(next) => next,
        Err(err) => return Err(vec![format!("[folder] {err}")]),
    };
    // The engine's own validator, on load as well as on write, exactly as
    // `SyncProfile::validate` promises — a hand-edited file must clear the bar
    // an app-created profile clears, and nothing else in this path checks a
    // journal template or a quiet-hours window.
    if let Err(err) = next.validate() {
        return Err(vec![format!("[folder] {err}")]);
    }

    // An override that did not take is worse than one that was refused: the
    // file says one thing and the folder does another, with nothing on screen.
    // This catches what the key check above cannot see — a misspelling *inside*
    // a nested table, and a field dropped by an enum that did not want it.
    let applied = match serde_json::to_value(&next) {
        Ok(applied) => applied,
        Err(err) => return Err(vec![format!("[folder] cannot be applied: {err}")]),
    };
    let mut unobserved = Vec::new();
    collect_unobserved(&requested, Some(&applied), "", &mut unobserved);
    if !unobserved.is_empty() {
        return Err(vec![format!(
            "[folder] set {} and the profile did not take {}; check the spelling of the \
             nested keys",
            unobserved.join(", "),
            if unobserved.len() == 1 { "it" } else { "them" }
        )]);
    }

    let Value::Object(requested) = requested else {
        unreachable!("built from a map directly above")
    };
    Ok(Some(Applied {
        keys: requested.keys().cloned().collect(),
        profile: next,
    }))
}

/// The refusal a `[settings]` table in a non-main folder earns.
///
/// Names the keys, because the rule only makes sense once you see what you
/// wrote: `hotkey.global` in two folders is two folders fighting over one
/// shortcut, and the winner would be whichever profile was read last.
fn settings_refusal(value: &toml::Value) -> String {
    let keys: Vec<&str> = value
        .as_table()
        .map(|table| table.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let named = if keys.is_empty() {
        "an empty `[settings]` table".to_owned()
    } else {
        format!("`[settings]` key(s) {}", keys.join(", "))
    };
    format!(
        "may not carry {named}: a folder that is not the main sync folder may only set \
         keys about itself, or two folders would fight over one app-wide setting. Move \
         them to `~/.keeper/keeper.toml` or to the main sync folder's file"
    )
}

/// Overlay `overlay` onto `base`, recursing into tables.
///
/// Deep on purpose. A `[folder.notes]` that sets only `subfolder` must keep the
/// journal template the profile already had; replacing the whole `notes` object
/// would silently reset every sibling field to its serde default, which is the
/// kind of "I changed one thing and something else moved" this epic exists to
/// stop. Arrays replace wholesale — `excludes` is a list, and appending to it
/// would leave no way to remove an entry.
fn merge(base: &mut Value, overlay: &Value) {
    if let (Value::Object(target), Value::Object(source)) = (&mut *base, overlay) {
        for (key, value) in source {
            match target.get_mut(key) {
                Some(slot) => merge(slot, value),
                None => {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        return;
    }
    *base = overlay.clone();
}

/// Every leaf `requested` asked for that `applied` does not carry, as dotted
/// paths.
fn collect_unobserved(requested: &Value, applied: Option<&Value>, at: &str, out: &mut Vec<String>) {
    let Some(applied) = applied else {
        out.push(at.to_owned());
        return;
    };
    if let (Value::Object(want), Value::Object(got)) = (requested, applied) {
        for (key, value) in want {
            let path = if at.is_empty() {
                key.clone()
            } else {
                format!("{at}.{key}")
            };
            collect_unobserved(value, got.get(key), &path, out);
        }
        return;
    }
    if requested != applied {
        out.push(at.to_owned());
    }
}

// ---------------------------------------------------------------------------
// The process-wide tier
// ---------------------------------------------------------------------------

static TIER: LazyLock<RwLock<Option<FolderTier>>> = LazyLock::new(|| RwLock::new(None));
static FAULTS: LazyLock<Mutex<BTreeMap<PathBuf, FolderFault>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Arm the folder tier for this process (AD-101).
///
/// Call it once, at startup, as soon as the host label and the main folder are
/// known — it needs neither the database nor the engine, because the folder
/// path arrives with the profile that is being read. Until it is called, every
/// profile loads exactly as `sync.db` holds it.
pub fn install_folder_tier(tier: FolderTier) {
    let mut guard = TIER
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(tier);
}

/// The installed tier, or `None` when the folder tier is not armed.
pub fn installed_folder_tier() -> Option<FolderTier> {
    TIER.read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Every folder config file that is currently broken, newest read wins.
///
/// A live snapshot rather than a log: an entry appears the first time a profile
/// whose file is broken is read, and disappears the first time that same file
/// reads cleanly, so the settings surface never shows a fault the user has
/// already fixed.
pub fn folder_faults() -> Vec<FolderFault> {
    FAULTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .cloned()
        .collect()
}

/// One profile as it is **in force**: the stored row with its folder files
/// layered on top.
///
/// The funnel every read goes through, and a clone-free no-op when the tier is
/// not armed.
pub fn in_force(profile: SyncProfile) -> SyncProfile {
    let Some(tier) = installed_folder_tier() else {
        return profile;
    };
    let candidates = tier.layer_paths(&profile.local_path);
    let outcome = tier.apply(&profile);
    let mut faults = FAULTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for path in candidates {
        faults.remove(&path);
    }
    for fault in outcome.faults {
        faults.insert(fault.path.clone(), fault);
    }
    outcome.profile
}

/// One profile as it must be **stored**: whatever the folder files currently
/// say, taken back off.
///
/// This is AD-98 enforced at the write funnel. Every read hands out an overlaid
/// profile, so every read-modify-write — `set_enabled` is the plain one — would
/// otherwise copy the file's values into the row, and deleting the file later
/// would reveal a copy of it rather than the value the user chose. The table
/// never learns what the file said.
///
/// `base` is the row as it stands before this write, or `None` for a profile
/// being created; an owned field falls back to the type's own default then,
/// which is the only honest answer for a value nobody has ever stored.
pub fn as_stored(incoming: &SyncProfile, base: Option<&SyncProfile>) -> SyncProfile {
    let Some(tier) = installed_folder_tier() else {
        return incoming.clone();
    };
    let owned = tier.apply(incoming).owned;
    if owned.is_empty() {
        return incoming.clone();
    }
    let (Ok(mut json), Ok(restore)) = (
        serde_json::to_value(incoming),
        serde_json::to_value(base.cloned().unwrap_or_else(|| {
            SyncProfile::new(
                &incoming.id,
                &incoming.name,
                &incoming.local_path,
                &incoming.remote_url,
            )
        })),
    ) else {
        return incoming.clone();
    };
    let (Some(target), Some(source)) = (json.as_object_mut(), restore.as_object()) else {
        return incoming.clone();
    };
    let mut shadowed = Vec::new();
    for key in &owned {
        let Some(kept) = source.get(key) else {
            continue;
        };
        if target.get(key) == Some(kept) {
            continue;
        }
        shadowed.push(key.as_str());
        target.insert(key.clone(), kept.clone());
    }
    if !shadowed.is_empty() {
        // Not silent, and not a refusal either: the write happened for every
        // other field. The person needs to know the one they changed is owned
        // by a file, and where that file is.
        tracing::warn!(
            profile = %incoming.id,
            fields = %shadowed.join(", "),
            folder = %incoming.local_path.display(),
            "sync: a folder config file owns these fields; the change was not stored"
        );
    }
    serde_json::from_value(json).unwrap_or_else(|_| incoming.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{DEFAULT_JOURNAL_TEMPLATE, DEFAULT_LFS_THRESHOLD_BYTES};

    fn profile(root: &Path) -> SyncProfile {
        SyncProfile::new("01J", "tgdrive", root, "https://example.invalid/r.git")
    }

    fn tier() -> FolderTier {
        FolderTier::new("hesperia", None)
    }

    /// Write one folder config file and return the profile as it is in force,
    /// plus whatever the tier refused.
    fn applied(text: &str, tier: &FolderTier) -> (tempfile::TempDir, FolderOutcome) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE), text).expect("write");
        let outcome = tier.apply(&profile(&root));
        (dir, outcome)
    }

    fn only_fault(outcome: &FolderOutcome) -> &FolderFault {
        assert_eq!(
            outcome.faults.len(),
            1,
            "expected exactly one refused file, got {:?}",
            outcome.faults
        );
        &outcome.faults[0]
    }

    /// The list is a judgement, but it must be a judgement about *every* field.
    /// A field added to `SyncProfile` with no entry here would otherwise fall
    /// into whichever branch the code happened to take.
    #[test]
    fn folder_field_rules_cover_every_profile_field() {
        let accepted = accepted_profile_keys().expect("keys");
        let classified: BTreeSet<String> = FOLDER_FIELD_RULES
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        assert_eq!(
            classified, accepted,
            "every SyncProfile field needs a folder-tier rule: decide whether a folder \
             may say it, and add it to FOLDER_FIELD_RULES"
        );
        assert_eq!(
            classified.len(),
            FOLDER_FIELD_RULES.len(),
            "a field is listed twice"
        );
    }

    /// The four the owner named, refused by name and with the rule in the
    /// sentence — not dropped, not clamped, not "ignored unknown key".
    #[test]
    fn a_folder_file_cannot_set_a_machine_local_or_identity_field() {
        for (key, literal, rule) in [
            ("localPath", "\"/somewhere/else\"", "machine-local"),
            ("volumeId", "\"01VOLUME\"", "identity"),
            ("id", "\"01OTHER\"", "identity"),
            (
                "remoteUrl",
                "\"https://elsewhere.invalid/r.git\"",
                "identity",
            ),
        ] {
            let (_dir, outcome) = applied(&format!("[folder]\n{key} = {literal}\n"), &tier());
            let fault = only_fault(&outcome);
            assert!(
                fault.message.contains(&format!("may not set `{key}`")),
                "the refusal must name the key: {}",
                fault.message
            );
            assert!(
                fault.message.contains(rule),
                "the refusal must name the rule: {}",
                fault.message
            );
            assert!(
                fault.path.ends_with(".keeper/keeper.toml"),
                "the refusal must name the file: {}",
                fault.path.display()
            );
        }
    }

    /// `localPath` specifically: the refusal is not cosmetic, the profile must
    /// still point where this machine mounted the folder.
    #[test]
    fn a_refused_layer_leaves_the_stored_profile_exactly_as_it_was() {
        let (dir, outcome) = applied(
            "[folder]\nlocalPath = \"/somewhere/else\"\ntags = [\"kept-out\"]\n",
            &tier(),
        );
        assert_eq!(
            outcome.profile.local_path,
            dir.path(),
            "the folder cannot move itself"
        );
        assert!(
            outcome.profile.tags.is_empty(),
            "a layer with a refused key is dropped whole, not half-applied"
        );
        assert!(outcome.owned.is_empty());
    }

    /// The owner's own constraint, and the thing that stops two folders
    /// fighting over one shortcut.
    #[test]
    fn a_non_main_folder_may_not_carry_settings() {
        let (_dir, outcome) = applied(
            "[settings]\n\"hotkey.global\" = \"cmd+shift+k\"\n[folder]\ntags = [\"work\"]\n",
            &tier(),
        );
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains("hotkey.global"),
            "the refusal must name the key: {}",
            fault.message
        );
        assert!(
            fault.message.contains("main sync folder"),
            "the refusal must name the rule: {}",
            fault.message
        );
        assert!(fault.path.ends_with(".keeper/keeper.toml"));
        assert!(
            outcome.profile.tags.is_empty(),
            "the `[folder]` half goes with it, so the file is fixed rather than \
             half-honoured"
        );
    }

    /// The main folder's file carries both tables, and the settings half is
    /// keeper-core's business. Refusing it here would give one file two faults
    /// from two crates that cannot see each other.
    #[test]
    fn the_main_folders_own_file_may_carry_settings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[settings]\n\"recording.fps\" = 30\n[folder]\ntags = [\"work\"]\n",
        )
        .expect("write");
        let tier = FolderTier::new("hesperia", Some(root.clone()));
        let outcome = tier.apply(&profile(&root));
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.tags, vec!["work".to_owned()]);
    }

    /// `mainSyncFolder` is answered by the user-global file and nowhere else. A
    /// folder that could nominate itself as main could then set `[settings]`.
    #[test]
    fn a_folder_file_may_not_nominate_the_main_sync_folder() {
        let (_dir, outcome) = applied("mainSyncFolder = \"/Volumes/x\"\n", &tier());
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains("mainSyncFolder"),
            "{}",
            fault.message
        );
        assert!(
            fault.message.contains("~/.keeper/keeper.toml"),
            "{}",
            fault.message
        );
    }

    /// The tier exists for these. Repository policy has to be the same on both
    /// clones or the clones disagree about what they are committing.
    #[test]
    fn a_folder_file_sets_the_repository_policy_and_what_the_folder_holds() {
        let (_dir, outcome) = applied(
            r#"
[folder]
branch = "trunk"
excludes = ["*.psd"]
lfs_never = ["*.csv"]
lfsThresholdBytes = 1048576
commitSubjectTemplate = "keeper: {profile}"
tags = ["work", "media"]

[folder.notes]
subfolder = "40-notes"

[folder.recordings]
subfolder = "40-media/recordings"

[folder.sessions]
subfolder = "60-sessions"
"#,
            &tier(),
        );
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        let p = &outcome.profile;
        assert_eq!(p.branch, "trunk");
        assert_eq!(p.excludes, vec!["*.psd".to_owned()]);
        assert_eq!(p.lfs_never, vec!["*.csv".to_owned()]);
        assert_eq!(p.lfs_threshold_bytes, 1_048_576);
        assert_eq!(p.commit_subject_template, "keeper: {profile}");
        assert_eq!(p.tags, vec!["work".to_owned(), "media".to_owned()]);
        assert_eq!(
            p.notes.as_ref().expect("notes").subfolder,
            "40-notes",
            "a folder may declare that it holds a vault"
        );
        assert_eq!(
            p.recordings.as_ref().expect("recordings").subfolder,
            "40-media/recordings"
        );
        assert_eq!(
            p.sessions.as_ref().expect("sessions").subfolder,
            "60-sessions",
            "a folder may declare that it holds a sessions zone"
        );
        assert_eq!(
            outcome.owned.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "branch",
                "commitSubjectTemplate",
                "excludes",
                "lfsNever",
                "lfsThresholdBytes",
                "notes",
                "recordings",
                "sessions",
                "tags",
            ]
        );
    }

    /// snake_case is what a person writes in a TOML file, and it is the same
    /// key. The fold is `keeper-syncd`'s, shared rather than copied.
    #[test]
    fn snake_case_and_camel_case_are_one_key() {
        let (_dir, outcome) = applied("[folder]\nlfs_threshold_bytes = 512\n", &tier());
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.lfs_threshold_bytes, 512);

        let (_dir, both) = applied(
            "[folder]\nlfs_threshold_bytes = 512\nlfsThresholdBytes = 512\n",
            &tier(),
        );
        assert!(
            only_fault(&both).message.contains("given twice"),
            "{}",
            only_fault(&both).message
        );
    }

    /// A partial nested table keeps its siblings. Replacing the whole `notes`
    /// object would silently reset the journal template to its default, which
    /// is the "I changed one thing and something else moved" failure the epic
    /// is about.
    #[test]
    fn a_partial_nested_table_keeps_the_fields_it_did_not_mention() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[folder.notes]\nsubfolder = \"40-notes\"\n",
        )
        .expect("write");
        let mut stored = profile(&root);
        stored.notes = Some(crate::profile::NotesConfig {
            subfolder: "notes".to_owned(),
            journal_template: "diary/{yyyy}.md".to_owned(),
            ..Default::default()
        });
        let outcome = tier().apply(&stored);
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        let notes = outcome.profile.notes.as_ref().expect("notes");
        assert_eq!(notes.subfolder, "40-notes");
        assert_eq!(
            notes.journal_template, "diary/{yyyy}.md",
            "the sibling the file did not mention must survive"
        );
        assert_ne!(notes.journal_template, DEFAULT_JOURNAL_TEMPLATE);
    }

    /// `validate()` runs on load, not only on write, so a hand-edited file
    /// cannot smuggle a profile the engine cannot act on. Nothing else in this
    /// path would have caught an absolute vault subfolder.
    #[test]
    fn the_overlay_goes_through_the_engines_own_validator() {
        let (_dir, outcome) = applied("[folder.notes]\nsubfolder = \"/etc\"\n", &tier());
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains("subfolder"),
            "the validator's own sentence must survive: {}",
            fault.message
        );
        assert!(
            outcome.profile.notes.is_none(),
            "an invalid layer is dropped whole"
        );
    }

    /// The same, one level further in: an unknown commit-subject placeholder is
    /// refused rather than rendered into every commit the folder makes.
    #[test]
    fn a_validator_rule_below_the_top_level_still_refuses_the_layer() {
        let (_dir, outcome) = applied(
            "[folder]\ncommitSubjectTemplate = \"sync {nope}\"\n",
            &tier(),
        );
        assert!(only_fault(&outcome).message.contains("{nope}"));
    }

    /// A misspelling *inside* a nested table cannot be caught by the top-level
    /// key check, and serde ignores it. Silently doing nothing is the failure
    /// this epic exists to end, so it is a fault.
    #[test]
    fn a_key_the_profile_did_not_take_is_a_fault_rather_than_a_silent_no_op() {
        let (_dir, outcome) = applied("[folder.notes]\nsubfoldr = \"40-notes\"\n", &tier());
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains("notes.subfoldr"),
            "the dotted path of the key that did nothing must be named: {}",
            fault.message
        );
    }

    /// A broken file is a fault, never a startup failure and never a partial
    /// application.
    #[test]
    fn a_malformed_file_is_reported_and_skipped_whole() {
        let (_dir, outcome) = applied("[folder\ntags = [\"work\"]\n", &tier());
        let fault = only_fault(&outcome);
        assert!(fault.message.contains("not readable as TOML"));
        assert!(outcome.profile.tags.is_empty());
    }

    /// No file at all is the ordinary case and says nothing.
    #[test]
    fn a_folder_with_no_file_is_untouched() {
        let dir = tempfile::tempdir().expect("temp dir");
        let stored = profile(dir.path());
        let outcome = tier().apply(&stored);
        assert!(outcome.faults.is_empty());
        assert!(outcome.owned.is_empty());
        assert_eq!(outcome.profile, stored);
    }

    /// The precedence AD-99 lists: shared first, this machine second, later
    /// wins. Both files sync, so a machine that has never run keeper still gets
    /// its own settings from the machine that wrote them.
    #[test]
    fn the_machine_file_wins_over_the_shared_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let keeper = root.join(FOLDER_CONFIG_DIR);
        std::fs::create_dir_all(&keeper).expect("create .keeper");
        std::fs::write(
            keeper.join(SHARED_FILE),
            "[folder]\nbranch = \"trunk\"\ntags = [\"shared\"]\n",
        )
        .expect("write shared");
        std::fs::write(
            keeper.join("keeper.hesperia.toml"),
            "[folder]\ntags = [\"this-machine\"]\n",
        )
        .expect("write machine");
        let outcome = tier().apply(&profile(&root));
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.tags, vec!["this-machine".to_owned()]);
        assert_eq!(
            outcome.profile.branch, "trunk",
            "the shared file still wins where the machine file is silent"
        );
        // Another machine's file is not this machine's.
        let other = FolderTier::new("mnemosyne", None).apply(&profile(&root));
        assert_eq!(other.profile.tags, vec!["shared".to_owned()]);
    }

    /// A host label that could name a file outside `.keeper/` names no file at
    /// all. `read_host_label` never produces one; this type accepts any string.
    #[test]
    fn an_unusable_host_label_yields_only_the_shared_file() {
        for host in ["", "   ", "../../etc", "a/b"] {
            let paths = FolderTier::new(host, None).layer_paths(Path::new("/folder"));
            assert_eq!(paths.len(), 1, "host {host:?} must not name a second file");
            assert_eq!(paths[0], Path::new("/folder/.keeper/keeper.toml"));
        }
    }

    /// AD-98 at the write funnel. Every read hands out an overlaid profile, so
    /// without this the first UI toggle copies the file's values into the row —
    /// and deleting the file would then reveal a copy of it rather than the
    /// value the user chose.
    #[test]
    fn a_write_never_stores_what_the_file_said() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[folder]\nlfsThresholdBytes = 512\ntags = [\"from-the-file\"]\n",
        )
        .expect("write");
        let _tier = TierGuard::armed(FolderTier::new("hesperia", None));

        let stored = profile(&root);
        let in_force_now = in_force(stored.clone());
        assert_eq!(
            in_force_now.lfs_threshold_bytes, 512,
            "the file wins on read"
        );

        // The read-modify-write every settings toggle performs.
        let mut edited = in_force_now.clone();
        edited.enabled = false;
        let to_store = as_stored(&edited, Some(&stored));
        assert!(!to_store.enabled, "the field the user changed is stored");
        assert_eq!(
            to_store.lfs_threshold_bytes, DEFAULT_LFS_THRESHOLD_BYTES,
            "the row keeps the value it had, never the file's"
        );
        assert!(to_store.tags.is_empty(), "nor the file's tags");

        // A field the file owns, changed by hand, is kept out and said out loud
        // rather than dropped where nobody can see it.
        let mut shadowed = in_force_now;
        shadowed.tags = vec!["typed-in-the-ui".to_owned()];
        assert!(as_stored(&shadowed, Some(&stored)).tags.is_empty());

        // A profile being created has no row to fall back to, so an owned field
        // takes the type's own default.
        assert_eq!(
            as_stored(&edited, None).lfs_threshold_bytes,
            DEFAULT_LFS_THRESHOLD_BYTES
        );
    }

    /// Uninstalled is the default and is a true no-op: `keeper-syncd` and every
    /// test that predates the tier must see the table exactly as it is.
    #[test]
    fn an_uninstalled_tier_reads_and_writes_the_table_unchanged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[folder]\nlfsThresholdBytes = 512\n",
        )
        .expect("write");
        let _tier = TierGuard::disarmed();
        let stored = profile(&root);
        assert_eq!(in_force(stored.clone()), stored);
        assert_eq!(as_stored(&stored, None), stored);
    }

    /// Faults are a live snapshot, not a log: an entry disappears the moment
    /// the file it names reads cleanly, so nobody chases a fault they fixed.
    #[test]
    fn a_fixed_file_stops_being_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let file = root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE);
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(&file, "[folder]\nlocalPath = \"/elsewhere\"\n").expect("write");
        let _tier = TierGuard::armed(FolderTier::new("hesperia", None));

        let stored = profile(&root);
        in_force(stored.clone());
        assert!(
            folder_faults().iter().any(|fault| fault.path == file),
            "the broken file is reported"
        );

        std::fs::write(&file, "[folder]\ntags = [\"work\"]\n").expect("rewrite");
        assert_eq!(in_force(stored).tags, vec!["work".to_owned()]);
        assert!(
            !folder_faults().iter().any(|fault| fault.path == file),
            "and stops being reported once it is fixed"
        );
    }

    /// The tier is process-wide, so the three tests that arm it take a lock and
    /// leave it disarmed behind them.
    ///
    /// A guard rather than a call at the end of each test, for the reason every
    /// shared-global test suite eventually learns: a failing assertion unwinds
    /// past the call and every later test inherits an armed tier, which turns
    /// one red into a page of them. The pure tests above never look at the
    /// global and are deliberately not serialized.
    static TIER_TEST: Mutex<()> = Mutex::new(());

    struct TierGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl TierGuard {
        fn disarmed() -> Self {
            let guard = TIER_TEST
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            reset_tier();
            Self(guard)
        }

        fn armed(tier: FolderTier) -> Self {
            let guard = Self::disarmed();
            install_folder_tier(tier);
            guard
        }
    }

    impl Drop for TierGuard {
        fn drop(&mut self) {
            reset_tier();
        }
    }

    fn reset_tier() {
        *TIER
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        FAULTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}
