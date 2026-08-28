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
    // Which paths may keep their content away is a fact about the repository —
    // the same `40-media/**` is bulk on every clone — so the folder tier is its
    // canonical home and both hosts honour one list (AD-132). Beside `lfsNever`
    // on purpose, and not the same question: that one says "never route this
    // through LFS", this one says "its bytes may live only in the store".
    ("virtualPatterns", FolderFieldRule::Allowed),
    ("virtualOverBytes", FolderFieldRule::Allowed),
    // How long this repository's content may stay is a fact about the
    // repository — the same media archive wants the same retention on every
    // clone — and the app and the daemon share no profile store, so the folder
    // file is the only place one answer can reach both (AD-132, FR-344).
    ("releaseTtlMs", FolderFieldRule::Allowed),
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
    /// file that is missing says nothing.
    ///
    /// # A broken file loses the keys it got wrong, and only those (Story 56.14)
    ///
    /// A file that says something a folder may not, or that trips
    /// [`SyncProfile::validate`], used to be dropped **whole** — the argument
    /// being that a layer half-applied is a configuration nobody can reason
    /// about. What that missed is which half the person can see: one misspelled
    /// top-level key, or one out-of-range number, silently discarded every other
    /// key in the file, so a `releaseTtlMs` or an `excludes` list the author had
    /// spelled perfectly simply did not take, on every clone that read the
    /// file, with the stored row's value quietly in force instead. That is the
    /// "reads the file and ignores it" failure, not a conservative one — and the
    /// data-loss direction was already closed separately, because
    /// [`folder_config_is_faulted`] makes the release sweep decline a folder
    /// whose layer is faulted whatever this returns.
    ///
    /// So a layer that fails is retried key by key through the same [`overlay`]
    /// — the identical rule table, the identical `validate`, the identical
    /// unobserved-key check, no second code path — and each key stands or falls
    /// on its own. The fault is still recorded, naming every key that fell and
    /// why.
    ///
    /// **A layer with no problems is not retried and behaves exactly as before.**
    /// The retry costs one `validate` per `[folder]` key and is paid only by a
    /// file that was going to be discarded entirely.
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
            let table = match toml::from_str::<toml::Table>(&text) {
                Ok(table) => table,
                // `toml`'s Display carries the line, the column and the
                // offending input. Do not flatten it — it is the whole
                // diagnosis. And there is nothing to salvage: a file that does
                // not parse has no keys to try one at a time.
                Err(err) => {
                    faults.push(FolderFault::new(
                        &path,
                        format!("is not readable as TOML\n{err}"),
                    ));
                    continue;
                }
            };
            match overlay(&current, &table, is_main) {
                Ok(None) => {}
                Ok(Some(applied)) => {
                    current = applied.profile;
                    owned.extend(applied.keys);
                }
                Err(problems) => {
                    let salvage = salvage_keys(&current, &table, is_main);
                    if let Some(applied) = salvage.applied {
                        current = applied.profile;
                        owned.extend(applied.keys);
                    }
                    // The whole-layer problems, because they are the ones
                    // written for a person to read, plus whatever the per-key
                    // retry could attribute more precisely and the first pass
                    // could not.
                    let mut said = problems;
                    said.extend(salvage.problems);
                    faults.push(FolderFault::new(&path, said.join("; ")));
                }
            }
        }
        FolderOutcome {
            profile: current,
            owned,
            faults,
        }
    }
}

/// What a per-key retry of a failed layer managed to keep, and what it could
/// not.
struct Salvage {
    applied: Option<Applied>,
    problems: Vec<String>,
}

/// Retry one failed layer's `[folder]` keys one at a time (Story 56.14).
///
/// Every key goes through [`overlay`] on its own, against the profile as the
/// keys before it left it, so each one meets the same rule table, the same
/// `SyncProfile::validate` and the same unobserved-key check it would have met
/// in the whole layer. Nothing about what is permitted changes; only how much
/// of a bad file survives.
///
/// # Order
///
/// `toml::Table` is sorted, so keys are tried alphabetically. That is
/// deterministic — the same file gives the same answer on every clone, which is
/// the property that matters — and it happens to be the order `validate`'s
/// cross-field rules want: `notes` before `recordings` before `sessions` is
/// exactly the sequence its overlap checks are written in.
///
/// Keys outside `[folder]` are not retried. They are not profile fields, they
/// were already reported by the whole-layer pass, and the whole point of this
/// retry is that a misspelled top-level key must stop taking `[folder]` with
/// it.
fn salvage_keys(profile: &SyncProfile, table: &toml::Table, is_main: bool) -> Salvage {
    let Some(fields) = table
        .iter()
        .find(|(key, _)| canonical_key(key) == "folder")
        .and_then(|(_, value)| value.as_table())
    else {
        return Salvage {
            applied: None,
            problems: Vec::new(),
        };
    };

    let mut current = profile.clone();
    let mut keys = BTreeSet::new();
    let mut problems = Vec::new();
    for (key, value) in fields {
        let mut one = toml::Table::new();
        let mut folder = toml::Table::new();
        folder.insert(key.clone(), value.clone());
        one.insert("folder".to_owned(), toml::Value::Table(folder));
        match overlay(&current, &one, is_main) {
            // Nothing to say — an empty nested table, which the whole-layer
            // pass would also have skipped.
            Ok(None) => {}
            Ok(Some(applied)) => {
                current = applied.profile;
                keys.extend(applied.keys);
            }
            Err(mut said) => problems.append(&mut said),
        }
    }

    if keys.is_empty() {
        return Salvage {
            applied: None,
            // Every key failed, so the whole-layer pass already said all of
            // this. Repeating it would print each problem twice.
            problems: Vec::new(),
        };
    }
    Salvage {
        applied: Some(Applied {
            profile: current,
            keys,
        }),
        problems,
    }
}

/// One layer, applied.
struct Applied {
    profile: SyncProfile,
    keys: BTreeSet<String>,
}

/// Apply one folder file's parsed `[folder]` table to `profile`.
///
/// `Ok(None)` when the table has nothing to say. `Err` carries every problem in
/// it, each a full sentence naming the key and the rule; the caller prefixes
/// the file.
///
/// Takes the parsed table rather than the text so [`salvage_keys`] can hand it
/// one key at a time without re-serializing: the retry has to meet every guard
/// in here identically, which a second entry point would not guarantee.
fn overlay(
    profile: &SyncProfile,
    table: &toml::Table,
    is_main: bool,
) -> std::result::Result<Option<Applied>, Vec<String>> {
    let mut problems = Vec::new();
    let mut requested = None;
    for (key, value) in table {
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

/// Does this folder's own config layer currently read as broken?
///
/// The one caller is the release sweep, and the one reason is that a caller
/// which deletes data must not read a permissive default out of a file the
/// reader could not parse. `releaseTtlMs` is a folder-tier [`Allowed`] field
/// (FR-344, AD-132) and `0` is the documented way for a repository to say
/// *never release my content* — but [`FolderTier::apply`] and [`in_force`] are
/// all-or-nothing: one `validate()` failure or one unknown key anywhere in
/// `.keeper/keeper.toml` discards the **whole** layer, so the committed
/// `releaseTtlMs = 0` stops applying and the profile's own 24 h default takes
/// over. A typo in a file that travels between clones would otherwise turn
/// "keep everything" into "delete after a day" on every machine that reads it.
/// This is the first folder field whose default deletes data, so the sweep
/// fails **closed** on it (Story 56.5).
///
/// **Deliberately per-folder, not global.** [`folder_faults`] is a process-wide
/// snapshot covering every profile that has been read, and another folder's
/// broken file says nothing about this one — a machine with two folders, one of
/// them mistyped, must still release from the healthy one. So the answer is
/// whether any recorded fault names one of *this* folder's own layer paths,
/// derived through [`FolderTier::layer_paths`] exactly as [`in_force`] derives
/// the candidates it clears, rather than from a second path list that could
/// drift out of agreement with it.
///
/// `false` when the tier is not armed: nothing can be faulted if nothing is
/// layered, which is [`in_force`]'s own answer in that case.
///
/// [`Allowed`]: FolderFieldRule::Allowed
pub fn folder_config_is_faulted(local_path: &Path) -> bool {
    let Some(tier) = installed_folder_tier() else {
        return false;
    };
    let candidates = tier.layer_paths(local_path);
    let faults = FAULTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    candidates.iter().any(|path| faults.contains_key(path))
}

/// The canonical profile keys this folder's own files currently set.
///
/// The **read**-side counterpart of [`as_stored`]'s strip: the same
/// [`FolderOutcome::owned`] set, asked *before* a write rather than during one.
/// [`as_stored`] can only put the prior value back and say so in a log line no
/// user reads (AD-98 leaves it no other option — the table must never learn
/// what the file said), so a surface with no way to ask this question offers an
/// editable control, accepts the edit, reports success and silently reverts.
/// With the question answerable, the surface can say *a file decides this*
/// instead, disable the control, and never send the key at all — at which point
/// the `tracing::warn!` never has to fire for a change somebody could see.
///
/// The keys are canonical camelCase [`SyncProfile`] field names — whatever
/// [`super::canonical_key`] folds a file's spelling to — so they compare
/// directly against the JSON a request carries. Only [`Allowed`] keys can ever
/// appear: a layer that names an [`Identity`] or [`MachineLocal`] field is
/// refused and dropped whole by [`FolderTier::apply`], so nothing it refused
/// reaches this set.
///
/// This re-reads the folder's TOML layers, which [`in_force`] has already read
/// on this profile's way out of [`crate::db::list_profiles`]. Cheap — two
/// `read_to_string`s of a file the OS has cached — and deliberately not cached
/// here, because the file can be edited under a running app: the two answers
/// are "you may edit this" and "a file decides this", and a stale *permission*
/// is the wrong direction to be wrong in.
///
/// Faults are not this function's business — [`folder_config_is_faulted`]
/// answers that one. A layer that failed to parse sets nothing, so its keys are
/// correctly absent here: the value in force came from the table, and the table
/// is exactly what the surface may still edit.
///
/// The empty set when the tier is not armed, which is [`in_force`]'s own answer
/// in that case: nothing is layered, so nothing is owned.
///
/// [`Allowed`]: FolderFieldRule::Allowed
/// [`Identity`]: FolderFieldRule::Identity
/// [`MachineLocal`]: FolderFieldRule::MachineLocal
pub fn owned_fields(profile: &SyncProfile) -> BTreeSet<String> {
    let Some(tier) = installed_folder_tier() else {
        return BTreeSet::new();
    };
    tier.apply(profile).owned
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
    use crate::profile::{
        DEFAULT_JOURNAL_TEMPLATE, DEFAULT_LFS_THRESHOLD_BYTES, DEFAULT_RELEASE_TTL_MS,
        MIN_RELEASE_TTL_MS,
    };

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
    /// still point where this machine mounted the folder — and since Story
    /// 56.14 the refused key falls alone instead of taking the legal key beside
    /// it down with it.
    #[test]
    fn a_refused_local_path_falls_alone_and_cannot_move_the_folder() {
        let (dir, outcome) = applied(
            "[folder]\nlocalPath = \"/somewhere/else\"\ntags = [\"work\"]\n",
            &tier(),
        );
        assert_eq!(
            outcome.profile.local_path,
            dir.path(),
            "the folder cannot move itself"
        );
        assert!(
            !outcome.owned.contains("localPath"),
            "and a key the tier refused is never owned: {:?}",
            outcome.owned
        );
        assert_eq!(
            outcome.profile.tags,
            vec!["work".to_owned()],
            "while the key beside it stands on its own"
        );
    }

    /// A repository may declare how long its own content stays (Story 56.5,
    /// FR-344, AD-132).
    ///
    /// The reason it is a folder-tier field rather than a per-machine one: the
    /// app and the daemon share no profile store, so a retention window set in
    /// one is invisible to the other, and the file that travels with the
    /// repository is the only place one answer reaches both.
    #[test]
    fn a_folder_file_may_set_the_release_ttl() {
        let (_dir, outcome) = applied("[folder]\nreleaseTtlMs = 3600000\n", &tier());
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.release_ttl_ms, 3_600_000);
        assert_eq!(
            outcome.profile.effective_release_ttl_ms(),
            Some(3_600_000),
            "and it is the window in force, not just a stored number"
        );
        assert!(
            outcome.owned.iter().any(|key| key == "releaseTtlMs"),
            "the file owns the field, so a save from the app is told it cannot \
             move it: {:?}",
            outcome.owned
        );

        // Zero travels too: a repository that says "never release my content"
        // must be able to say it once and have every clone agree.
        let (_dir, outcome) = applied("[folder]\nreleaseTtlMs = 0\n", &tier());
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.effective_release_ttl_ms(), None);
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
        assert_eq!(
            outcome.profile.tags,
            vec!["work".to_owned()],
            "while the `[folder]` half no longer goes down with a top-level key \
             it has nothing to do with (Story 56.14)"
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

    /// A layer that trips one rule keeps the keys it got right (Story 56.14).
    ///
    /// Both refusal paths, because they leave [`overlay`] from different
    /// places: the rule table's `may not set`, and `SyncProfile::validate`'s
    /// own sentence. Without the fix `overlay` returned `Err` for the whole
    /// layer, so `outcome.profile` was the stored row untouched — `branch`
    /// still read `main` and `excludes` still read empty — and a key the author
    /// had spelled perfectly silently did not take, on every clone that read
    /// the file.
    #[test]
    fn a_layer_that_trips_one_rule_keeps_the_keys_it_got_right() {
        // The rule table's refusal: `branch` is repository policy and
        // `localPath` is where this clone mounted the folder, and only the
        // first is a folder file's to state.
        let (dir, outcome) = applied(
            "[folder]\nbranch = \"trunk\"\nlocalPath = \"/somewhere/else\"\n",
            &tier(),
        );
        assert_eq!(
            outcome.profile.branch, "trunk",
            "the key that stands on its own is in force"
        );
        assert!(
            outcome.owned.contains("branch"),
            "and is owned, so the surface disables its control rather than \
             offering an edit that reverts: {:?}",
            outcome.owned
        );
        assert_eq!(
            outcome.profile.local_path,
            dir.path(),
            "while the refused key changed nothing"
        );
        assert!(
            !outcome.owned.contains("localPath"),
            "and is not owned: {:?}",
            outcome.owned
        );
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains("localPath"),
            "the fault still names the key that fell: {}",
            fault.message
        );

        // `SyncProfile::validate`'s refusal, one level deeper than the rule
        // table: a non-zero TTL below the floor is one somebody typed, and it
        // falls without taking the `excludes` list with it.
        let (_dir, outcome) = applied(
            "[folder]\nexcludes = [\"*.psd\"]\nreleaseTtlMs = 500\n",
            &tier(),
        );
        assert_eq!(
            outcome.profile.excludes,
            vec!["*.psd".to_owned()],
            "the well-formed key is in force"
        );
        assert!(
            outcome.owned.contains("excludes"),
            "and owned: {:?}",
            outcome.owned
        );
        assert_eq!(
            outcome.profile.release_ttl_ms, DEFAULT_RELEASE_TTL_MS,
            "the out-of-range one is not, so the stored row's value stands"
        );
        assert!(
            !outcome.owned.contains("releaseTtlMs"),
            "nor is it owned: {:?}",
            outcome.owned
        );
        let fault = only_fault(&outcome);
        assert!(
            fault.message.contains(&MIN_RELEASE_TTL_MS.to_string())
                && fault.message.contains("500"),
            "the fault names the floor and the value that missed it: {}",
            fault.message
        );
    }

    /// A layer with no problems is not retried and behaves exactly as it did
    /// before the salvage existed (Story 56.14).
    ///
    /// Without the fix this test passes identically, which is the point of it:
    /// it pins the untouched path, so the retry stays confined to the failure
    /// arm rather than quietly becoming the way every profile read applies its
    /// folder file — one `SyncProfile::validate` per key, on every read, paid
    /// by files that have nothing wrong with them.
    #[test]
    fn a_layer_with_no_problems_applies_whole_and_reports_nothing() {
        let (_dir, outcome) = applied(
            "[folder]\nbranch = \"trunk\"\nexcludes = [\"*.psd\"]\nreleaseTtlMs = 3600000\n",
            &tier(),
        );
        assert!(outcome.faults.is_empty(), "{:?}", outcome.faults);
        assert_eq!(outcome.profile.branch, "trunk");
        assert_eq!(outcome.profile.excludes, vec!["*.psd".to_owned()]);
        assert_eq!(outcome.profile.release_ttl_ms, 3_600_000);
        let expected: BTreeSet<String> = ["branch", "excludes", "releaseTtlMs"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            outcome.owned, expected,
            "every key the file set is owned, and nothing else"
        );
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

    /// A folder file the reader could not parse makes *this* folder's config
    /// read as faulted, so a caller that deletes data can fail closed on it
    /// (Story 56.5, FR-344).
    ///
    /// The shape that matters is a repository which committed
    /// `releaseTtlMs = 0` — "never release my content" — beside one mistyped
    /// key. Since Story 56.14 that retention is salvaged from the broken layer
    /// rather than discarded with it, but the fault is still recorded, and the
    /// fault is what the sweep reads: a caller that deletes bytes may not take
    /// its permissive default from a file its own reader could not agree with.
    #[test]
    fn a_broken_file_makes_this_folders_config_read_as_faulted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let file = root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE);
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            &file,
            "[folder]\nreleaseTtlMs = 0\nlsfThresholdBytes = 512\n",
        )
        .expect("write");
        let _tier = TierGuard::armed(tier());

        let stored = profile(&root);
        let outcome = tier().apply(&stored);
        assert_eq!(only_fault(&outcome).path, file, "the fault names this file");
        assert_eq!(
            outcome.profile.effective_release_ttl_ms(),
            None,
            "the committed `releaseTtlMs = 0` survives the mistyped key beside \
             it, and the fault below is what holds the sweep closed"
        );

        assert!(
            !folder_config_is_faulted(&root),
            "nothing is faulted until a read records it"
        );
        in_force(stored.clone());
        assert!(
            folder_config_is_faulted(&root),
            "the recorded fault names one of this folder's own layer paths"
        );
        assert!(
            !folder_config_is_faulted(Path::new("/some/other/folder")),
            "and says nothing about a folder whose own files are fine — one \
             mistyped file must not hold every other folder closed"
        );

        std::fs::write(&file, "[folder]\nreleaseTtlMs = 0\n").expect("rewrite");
        assert_eq!(
            in_force(stored).effective_release_ttl_ms(),
            None,
            "the fixed file's retention is in force"
        );
        assert!(
            !folder_config_is_faulted(&root),
            "and a file that reads cleanly stops holding the sweep closed"
        );
    }

    /// With the tier not armed nothing is layered, so nothing about this folder
    /// can be faulted — [`in_force`]'s own answer, and the sweep's (Story 56.5).
    ///
    /// The fault is recorded first and the tier taken away afterwards, so the
    /// early return is what makes the answer `false` rather than an empty
    /// snapshot: a version that derived the layer paths without first asking
    /// whether anything is layered would pass against an empty map and fail
    /// here.
    #[test]
    fn an_unarmed_tier_reports_no_folder_config_fault() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let file = root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE);
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(&file, "[folder]\nlsfThresholdBytes = 512\n").expect("write");
        let _tier = TierGuard::armed(tier());

        in_force(profile(&root));
        assert!(folder_config_is_faulted(&root), "the file is broken");

        // Only the tier goes away; the recorded fault stays exactly where it is.
        *TIER
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        assert!(
            folder_faults().iter().any(|fault| fault.path == file),
            "the snapshot still holds the fault"
        );
        assert!(!folder_config_is_faulted(&root));
    }

    /// The read side of AD-98: a surface may ask which keys a file decides
    /// *before* offering a control over them, rather than discovering it from a
    /// log line after [`as_stored`] has quietly put the old value back.
    #[test]
    fn owned_fields_names_exactly_the_keys_a_folder_file_sets() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[folder]\nvirtualPatterns = [\"40-media/**\"]\nreleaseTtlMs = 3600000\n",
        )
        .expect("write");
        let _tier = TierGuard::armed(tier());

        let stored = profile(&root);
        let expected: BTreeSet<String> = ["releaseTtlMs", "virtualPatterns"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            owned_fields(&stored),
            expected,
            "exactly the two the file set, and nothing the file was silent about"
        );
        assert_eq!(
            in_force(stored).release_ttl_ms,
            3_600_000,
            "and the value the disabled control shows is the file's, because \
             every read is already overlaid"
        );
    }

    /// Two ways to own nothing, and they must not be confused: a folder with no
    /// file, and a process where nothing is layered at all.
    ///
    /// The second is [`in_force`]'s own answer with the tier unarmed, and the
    /// one `keeper-syncd` and every pre-tier caller sees.
    #[test]
    fn owned_fields_is_empty_with_no_file_and_with_no_tier() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let stored = profile(&root);

        let armed = TierGuard::armed(tier());
        assert!(
            owned_fields(&stored).is_empty(),
            "a folder with no `.keeper/` file has nothing owned"
        );
        drop(armed);

        let _disarmed = TierGuard::disarmed();
        std::fs::create_dir_all(root.join(FOLDER_CONFIG_DIR)).expect("create .keeper");
        std::fs::write(
            root.join(FOLDER_CONFIG_DIR).join(SHARED_FILE),
            "[folder]\nvirtualPatterns = [\"40-media/**\"]\n",
        )
        .expect("write");
        assert!(
            owned_fields(&stored).is_empty(),
            "and with nothing layered nothing can be owned, however much the \
             file says — the table is still the whole truth"
        );
    }

    /// A key the overlay refused is not owned — and since Story 56.14 the legal
    /// key beside it is, because a layer is no longer dropped whole: what the
    /// file could not say is still the table's to edit, and what it could say
    /// is the file's.
    ///
    /// Two layers, because the refusal is per-file and the answer must say so —
    /// a version that gave up on the first refusal would drop the machine
    /// layer's honest `virtualPatterns` too, and disable a control the file has
    /// no say over.
    #[test]
    fn owned_fields_never_names_a_key_the_overlay_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let config = root.join(FOLDER_CONFIG_DIR);
        std::fs::create_dir_all(&config).expect("create .keeper");
        std::fs::write(
            config.join(SHARED_FILE),
            "[folder]\nsettleMs = 9000\nvirtualOverBytes = 1048576\n",
        )
        .expect("write shared");
        std::fs::write(
            config.join("keeper.hesperia.toml"),
            "[folder]\nvirtualPatterns = [\"40-media/**\"]\n",
        )
        .expect("write host");
        let _tier = TierGuard::armed(tier());

        let owned = owned_fields(&profile(&root));
        assert!(
            !owned.contains("settleMs"),
            "a folder file may not set a machine-local key, so it cannot own \
             one: {owned:?}"
        );
        let expected: BTreeSet<String> = ["virtualOverBytes", "virtualPatterns"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            owned, expected,
            "the allowed key that shared the refused layer is kept, and one \
             refused file still says nothing about the other"
        );
    }

    /// The tier is process-wide, so every test that arms or disarms it takes a
    /// lock and leaves it disarmed behind them.
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
