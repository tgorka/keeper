//! The layer stack: a settings value is **resolved**, not imported (Story 46.6,
//! AD-98/AD-99/AD-101).
//!
//! Before this module, `config.json` was imported over the `settings` table at
//! boot ([`crate::registry::import_config_file`]). The file won exactly once and
//! the next UI toggle erased it, which is why nobody used it. Here the files
//! keep winning: [`setting_override`] is consulted on every
//! [`crate::registry::get_setting`], so a value written in a TOML layer survives
//! every write to the table underneath it.
//!
//! # The files
//!
//! ```text
//! ~/.keeper/keeper.toml                 user, every machine, every folder
//! ~/.keeper/keeper.<host>.toml          user, THIS machine
//! <clone>/<login>/keeper.toml           your account, every device
//! <clone>/<login>/keeper.<device>.toml  your account, THIS device
//! <main>/.keeper/keeper.toml            the main sync folder, every machine
//! <main>/.keeper/keeper.<host>.toml     the main sync folder, THIS machine
//! <folder>/.keeper/keeper.toml          that folder only
//! <folder>/.keeper/keeper.<host>.toml   that folder, this machine
//! ```
//!
//! The two account files come from the clone of the signed-in person's config
//! repository (Epic 82). They are the one part of the stack that changes while
//! keeper runs: [`install_account_layers`] swaps them after a fetch.
//!
//! Precedence is that order, later wins, **per key** — a machine file that sets
//! one key does not discard the shared file's other keys.
//!
//! ```toml
//! mainSyncFolder = "/Volumes/merope/tgdrive"   # only in ~/.keeper/keeper*.toml
//!
//! [settings]                                   # settings-table keys
//! "recording.fps" = 30
//!
//! [folder]                                     # this folder's SyncProfile fields
//! recordingsSubfolder = "40-media/recordings"
//! ```
//!
//! # Two phases, because the layers below need a database the layers above configure
//!
//! The main and folder files are keyed on sync-folder paths, which live in
//! `sync.db`, which is not open until the supervisor starts — and which itself
//! needs `sync.git_path` from the settings. AD-101 cuts that cycle by recording
//! the main folder's path in the user-global layer: **phase one** ([`load_app_layers`])
//! reads `~/.keeper/`, learns where main is, and reads main's two files straight
//! off the disk with no database; **phase two** (the shell, after the engine
//! opens) layers the per-folder files for the keys a folder is allowed to set.
//! This module owns phase one and the *parser* both phases share
//! ([`parse_layer_file`]).
//!
//! # An app must boot
//!
//! `keeper-syncd`'s TOML loader refuses to start on an unknown key, and that is
//! right for a daemon: a typo'd `remoteUrl` means a tray saying "up to date"
//! over nothing synced. An app is the other case. A malformed file here yields a
//! [`LayerFault`] naming the file, the line where the parser has one, and what
//! was expected — and that *one layer* is skipped whole while every other layer
//! still applies. Nothing about a config file may keep keeper from starting,
//! because the settings UI is how you would fix it.
//!
//! Faults are never silent: [`faults`] is rendered in the settings pane and
//! logged at boot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

pub mod keys;

/// The directory holding a layer file, under `~` or under a sync folder root.
///
/// The same `.keeper/` that holds the notes vault's `index.json` and `trash/`.
/// AD-100 exempts `*.toml` directly under it from the tier-0 exclusion, so a
/// folder's config travels with the folder — which is the entire reason to put
/// it there instead of `~/.keeper/`.
pub const KEEPER_DIR: &str = ".keeper";

/// The layer files' stem: `keeper.toml` and `keeper.<host>.toml`.
pub const FILE_STEM: &str = "keeper";

/// Which file a value came from, and therefore how it is ordered against the
/// others.
///
/// **Declaration order is precedence order** — later wins. The frozen stack is
/// merged by reading the files in this order, and the live account tiers are
/// merged into it by comparing tiers (`Ord`), so reordering these variants
/// reorders the stack. [`LayerTier::AccountDescriptor`] is last only because it
/// is not a layer at all; nothing compares against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerTier {
    /// `~/.keeper/keeper.toml` — this user, every machine, every folder.
    UserGlobal,
    /// `~/.keeper/keeper.<host>.toml` — this user, this machine.
    ///
    /// Not in the epic's first sketch of the enum, and it has to be: the only
    /// place an absolute path that differs per machine can live (the owner's own
    /// `mainSyncFolder` example is a macOS-only `/Volumes` mount) and the only
    /// file `keys::layer_may_set` will accept a machine-local key in.
    UserGlobalMachine,
    /// `<clone>/<login>/keeper.toml` — this person's account, every device.
    ///
    /// Read from the per-person config repository's clone, so it is above
    /// `~/.keeper` on purpose: the repository wins over this machine's hand
    /// edits. It is below the main folder, which keeps its authority. Live, not
    /// frozen: see [`install_account_layers`].
    AccountShared,
    /// `<clone>/<login>/keeper.<device>.toml` — this person's account, this
    /// device. Machine-scoped, so it is where a machine-local key may live.
    AccountDevice,
    /// `<main>/.keeper/keeper.toml` — the designated main sync folder, shared.
    MainShared,
    /// `<main>/.keeper/keeper.<host>.toml` — the main sync folder, this machine.
    MainMachine,
    /// `<folder>/.keeper/keeper.toml` — one non-main folder, shared.
    FolderShared,
    /// `<folder>/.keeper/keeper.<host>.toml` — one non-main folder, this machine.
    FolderMachine,
    /// `~/.keeper/account.toml` — the account descriptor. **Not a layer** and
    /// not in [`LayerTier::ORDER`]: it sets no key, it only says where the
    /// account tiers come from. It exists so a broken descriptor's faults are
    /// reported in the same list, with the same wording, as a layer's.
    AccountDescriptor,
}

impl LayerTier {
    /// Every tier, in precedence order (later wins).
    pub const ORDER: [LayerTier; 8] = [
        LayerTier::UserGlobal,
        LayerTier::UserGlobalMachine,
        LayerTier::AccountShared,
        LayerTier::AccountDevice,
        LayerTier::MainShared,
        LayerTier::MainMachine,
        LayerTier::FolderShared,
        LayerTier::FolderMachine,
    ];

    /// Whether this tier's file is the `keeper.<host>.toml` of its directory.
    ///
    /// The one bit `keys::layer_may_set` needs: a machine-local key
    /// (`sync.git_path`, `hotkey.*`, `recording.destination_dir`) is legitimate
    /// in a per-machine file and refused in a shared one, because a shared file
    /// carrying an absolute path to a binary breaks the other machine.
    pub fn machine_scoped(self) -> bool {
        matches!(
            self,
            LayerTier::UserGlobalMachine
                | LayerTier::AccountDevice
                | LayerTier::MainMachine
                | LayerTier::FolderMachine
        )
    }

    /// Whether a `[settings]` table is honoured at this tier.
    ///
    /// A non-main folder may only set keys that are *about itself* (`[folder]`).
    /// That is not a courtesy: it is what stops two folders fighting over
    /// `hotkey.global`, where the winner would be whichever the supervisor
    /// happened to open last. The descriptor is not a layer and sets nothing.
    pub fn may_set_settings(self) -> bool {
        !matches!(
            self,
            LayerTier::FolderShared | LayerTier::FolderMachine | LayerTier::AccountDescriptor
        )
    }

    /// Whether `mainSyncFolder` is honoured at this tier.
    ///
    /// Only the user-global files. A folder naming the main folder is either a
    /// no-op or a loop, and it is the fact that has to be readable *before* any
    /// folder is known (AD-101). An account file may not either: the repository
    /// is shared by every device, and which directory is the main folder is a
    /// fact about one machine's disk.
    pub fn may_set_main_folder(self) -> bool {
        matches!(self, LayerTier::UserGlobal | LayerTier::UserGlobalMachine)
    }

    /// Whether a `[folder]` table means anything at this tier — i.e. whether the
    /// file lives inside a sync folder at all.
    pub fn has_folder(self) -> bool {
        matches!(
            self,
            LayerTier::MainShared
                | LayerTier::MainMachine
                | LayerTier::FolderShared
                | LayerTier::FolderMachine
        )
    }

    /// A stable human name for logs and the settings pane.
    pub fn label(self) -> &'static str {
        match self {
            LayerTier::UserGlobal => "user",
            LayerTier::UserGlobalMachine => "user, this machine",
            LayerTier::AccountShared => "your account's settings (every device)",
            LayerTier::AccountDevice => "your account's settings (this device)",
            LayerTier::MainShared => "main folder",
            LayerTier::MainMachine => "main folder, this machine",
            LayerTier::FolderShared => "folder",
            LayerTier::FolderMachine => "folder, this machine",
            LayerTier::AccountDescriptor => "account.toml",
        }
    }
}

/// The file a resolved value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSource {
    pub tier: LayerTier,
    /// The layer file itself, not its directory — a person told "this is set by
    /// a file" needs the file.
    pub path: PathBuf,
    /// The sync-folder root this layer belongs to, when it belongs to one.
    pub folder: Option<String>,
    /// The account's display name, when this layer comes from the account's
    /// config repository — "Set by a file" has to say *whose* repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The account's repository host, so "Set by a file" can say where the
    /// file really lives: the clone on disk is replaced at every sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_host: Option<String>,
}

/// A settings value resolved from a layer file rather than the `settings` table.
///
/// `value` is already in the registry's on-disk string convention
/// ([`crate::registry::scalar_setting_text`]), so it drops straight into
/// [`crate::registry::get_setting`]'s return and every typed getter keeps its
/// own parsing and clamping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingOverride {
    pub value: String,
    pub source: LayerSource,
}

/// What went wrong, in a form the UI can branch on without parsing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerFaultKind {
    /// The file exists but could not be read (permissions, an I/O error).
    Unreadable,
    /// The file is not valid TOML. The whole layer is skipped.
    Malformed,
    /// `[settings]` or `[folder]` is present but is not a table.
    NotATable,
    /// `mainSyncFolder` is not a quoted string.
    ScalarExpected,
    /// A `[settings]` value does not fit its key's declared shape: a table where
    /// a number belongs, a number outside the range the getter accepts, a codec
    /// that is not one of the codecs. Costs that key only.
    ValueShape,
    /// `keys::layer_may_set` refused the key at this tier.
    KeyRefused,
    /// A `[settings]` table in a folder that is not the main sync folder.
    SettingsInNonMainFolder,
    /// `mainSyncFolder` outside `~/.keeper/`.
    MainFolderInFolderLayer,
    /// A top-level key that is none of `mainSyncFolder`, `settings`, `folder`.
    UnknownTable,
    /// `mainSyncFolder` names a path that does not exist.
    MainFolderMissing,
    /// `mainSyncFolder` names something that is not a directory.
    MainFolderNotADirectory,
    /// Raised after install by the shell's phase two — e.g. `mainSyncFolder`
    /// names a real directory that is no sync profile.
    MainFolderNotAProfile,
}

/// One thing wrong with one layer, named loudly enough to fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerFault {
    pub kind: LayerFaultKind,
    /// The file at fault.
    pub path: PathBuf,
    pub tier: Option<LayerTier>,
    pub folder: Option<String>,
    /// The offending key, when the fault is about one key rather than the file.
    pub key: Option<String>,
    /// 1-based line, when the parser gave us a span. Per-key faults have none:
    /// `toml::Table` does not carry spans, and guessing the line by searching
    /// the text for the key name would point at the wrong line the first time
    /// the key appears in a comment.
    pub line: Option<usize>,
    /// What was expected, in the words a person editing the file would use.
    pub message: String,
}

impl LayerFault {
    /// A fault raised outside the parser — the shell's phase two, mostly.
    pub fn late(
        kind: LayerFaultKind,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            tier: None,
            folder: None,
            key: None,
            line: None,
            message: message.into(),
        }
    }

    /// One line, safe to render verbatim: the file, the line if there is one,
    /// and the first line of the message.
    ///
    /// [`Display`](std::fmt::Display) is the *log* form and is deliberately
    /// multi-line for [`LayerFaultKind::Malformed`], because `toml`'s own error
    /// carries the offending input and a caret and flattening that throws away
    /// the only thing that locates the mistake. A UI wants this instead.
    ///
    /// Neither form can carry a secret: a layer file holds settings keys, and
    /// the settings table has never held secret material (passphrases live only
    /// in the Keychain).
    pub fn summary(&self) -> String {
        let head = self.message.lines().next().unwrap_or_default();
        match self.line {
            Some(line) => format!("{}:{line}: {head}", self.path.display()),
            None => format!("{}: {head}", self.path.display()),
        }
    }
}

impl std::fmt::Display for LayerFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// One layer file, parsed.
///
/// Handed back rather than folded in, because the two consumers want different
/// halves: phase one keeps `settings` and `main_sync_folder`, and the shell's
/// phase two hands `folder` — raw and untouched — to
/// `keeper_sync::profile::apply_folder_layers`, which owns the profile↔TOML
/// mapping. `keeper-sync` cannot depend on `keeper-core` (AD-40), so this side
/// deliberately does not interpret `[folder]` at all.
#[derive(Debug, Clone, Default)]
pub struct LayerFile {
    /// The file this was parsed from — folder faults raised downstream have to
    /// name it.
    pub path: PathBuf,
    pub tier: Option<LayerTier>,
    pub settings: BTreeMap<String, SettingOverride>,
    /// The `[folder]` table exactly as written.
    pub folder: Option<toml::Table>,
    pub main_sync_folder: Option<PathBuf>,
    pub faults: Vec<LayerFault>,
}

/// The phase-one result: everything knowable before `sync.db` opens.
#[derive(Debug, Clone, Default)]
pub struct AppLayers {
    /// Resolved settings-table overrides, keyed by settings key. A `BTreeMap`
    /// so [`overrides`] is sorted without a sort and so the merge is per-key by
    /// construction.
    pub overrides: BTreeMap<String, SettingOverride>,
    /// The designated main sync folder, if a user-global layer named one. Kept
    /// even when it does not exist, so the UI can show what was asked for
    /// beside the fault saying it is not there.
    pub main_folder: Option<PathBuf>,
    pub faults: Vec<LayerFault>,
}

/// This machine's short name, for the `keeper.<host>.toml` files, provenance
/// trailers and conflict filenames (moved here from the `keeper` shell in Story
/// 46.6 so every crate that needs a layer path can compile against it).
pub fn read_host_label() -> String {
    let raw = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    // macOS answers with a Bonjour name (`macbookpro.lan`); the leading label
    // keeps a commit trailer short.
    let short = raw.split('.').next().unwrap_or_default().trim();
    if short.is_empty() {
        "unknown-host".to_owned()
    } else {
        short.to_owned()
    }
}

/// The `.keeper/` directory of a home directory or a sync-folder root.
pub fn keeper_dir(root: &Path) -> PathBuf {
    root.join(KEEPER_DIR)
}

/// The two layer files in a `.keeper/` directory, in precedence order: the
/// shared file, then this machine's.
///
/// `host` is folded to a filename-safe form — a hostname may legally contain a
/// space or a slash, and `keeper./etc/passwd.toml` is not a file we want to
/// look for. Everything outside `[A-Za-z0-9._-]` becomes `-`; an empty result
/// falls back to `unknown-host`, which is exactly what [`read_host_label`]
/// would already have produced.
pub fn layer_paths(keeper_dir: &Path, host: &str) -> [PathBuf; 2] {
    [
        keeper_dir.join(format!("{FILE_STEM}.toml")),
        keeper_dir.join(format!("{FILE_STEM}.{}.toml", sanitize_host(host))),
    ]
}

fn sanitize_host(host: &str) -> String {
    let folded: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = folded.trim_matches('-');
    if trimmed.is_empty() {
        "unknown-host".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Turn a `[settings]` value into the string the table stores, using the key's
/// declared shape.
///
/// **Shape-aware, not convention-blind.** Three keys predate the `"1"`/`"0"`
/// convention and are read with a different comparison —
/// `honor_remote_deletions` and `sdk_encryption` against `"on"`,
/// `favorites_collapsed` against `"true"`. Formatting every boolean as `"1"`
/// would make `honor_remote_deletions = true` in a layer file resolve to `"1"`
/// and `archive::get_honor_remote_deletions` read it as **false**: the setting
/// silently doing the opposite of what the file says, which is worse than not
/// having the file. `keys::Shape::coerce` is the one place that translation
/// happens, so the file spelling and the stored spelling cannot drift.
///
/// A key with no declared shape cannot get here — `keys::layer_may_set` refuses
/// an unknown key first — but it is reported rather than unwrapped, because
/// nothing in a config file may panic the boot path.
fn setting_text(key: &str, value: &toml::Value) -> Result<String, String> {
    let Some(spec) = keys::spec(key) else {
        return Err(format!(
            "{key} has no declared shape, so a file cannot say what its value means"
        ));
    };
    spec.shape
        .coerce(key, value)
        .map_err(|error| error.to_string())
}

/// Every settings key in a `[settings]` table, dotted, in a stable order.
///
/// Settings keys are namespaced with dots, and TOML reads an unquoted dot as
/// nesting: `[settings]` with `recording.fps = 30` is a *sub-table* `recording`
/// holding `fps`, not a key named `recording.fps`. Both spellings — and
/// `[settings.recording]` with `fps = 30`, which is the third way to write the
/// same thing — mean one key here, because the alternative is telling somebody
/// that `recording` is not a setting when what they wrote reads correctly to
/// every other TOML tool in the world.
///
/// No settings value is a table, so the flattening is unambiguous. A key that
/// flattens to something the registry does not know still names itself in the
/// refusal.
fn flatten_settings<'a>(prefix: &str, table: &'a toml::Table) -> Vec<(String, &'a toml::Value)> {
    let mut flat = Vec::with_capacity(table.len());
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => flat.extend(flatten_settings(&key, inner)),
            _ => flat.push((key, value)),
        }
    }
    flat
}

/// The 1-based line a byte offset falls on.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// Parse one layer file's text.
///
/// Never returns an error: everything wrong lands in [`LayerFile::faults`]. A
/// TOML syntax error skips the layer whole (there is no half of a document to
/// trust); anything else is per-key, so one refused key does not cost the file.
pub fn parse_layer_file(
    path: &Path,
    tier: LayerTier,
    folder: Option<&str>,
    text: &str,
) -> LayerFile {
    let mut file = LayerFile {
        path: path.to_path_buf(),
        tier: Some(tier),
        ..LayerFile::default()
    };
    let fault = |kind: LayerFaultKind, key: Option<&str>, line: Option<usize>, message: String| {
        LayerFault {
            kind,
            path: path.to_path_buf(),
            tier: Some(tier),
            folder: folder.map(str::to_owned),
            key: key.map(str::to_owned),
            line,
            message,
        }
    };

    let document: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => {
            // `toml`'s Display carries the line, the column and a snippet of the
            // offending input — the whole "name the offending line" requirement.
            // Do not flatten it to one line.
            let line = error.span().map(|span| line_of(text, span.start));
            file.faults.push(fault(
                LayerFaultKind::Malformed,
                None,
                line,
                format!("this is not valid TOML, so the whole layer is skipped\n{error}"),
            ));
            return file;
        }
    };

    for (name, value) in &document {
        match name.as_str() {
            // syncd accepts both spellings of every key it reads; a person
            // hand-editing TOML reaches for snake_case about half the time, and
            // silently ignoring the other spelling is the worst outcome.
            "mainSyncFolder" | "main_sync_folder" => {
                if !tier.may_set_main_folder() {
                    let why = if tier.has_folder() {
                        "a sync folder cannot elect itself"
                    } else {
                        "your account's repository is shared by every device, and the main \
                         folder is a path on one of them"
                    };
                    file.faults.push(fault(
                        LayerFaultKind::MainFolderInFolderLayer,
                        Some(name),
                        None,
                        format!(
                            "{name} is only honoured in ~/.keeper/{FILE_STEM}.toml and \
                             ~/.keeper/{FILE_STEM}.<host>.toml; {why}, so this line is ignored"
                        ),
                    ));
                    continue;
                }
                match value.as_str() {
                    Some(raw) if !raw.trim().is_empty() => {
                        file.main_sync_folder = Some(PathBuf::from(raw.trim()));
                    }
                    // Blank is "cleared", which is the same state as never set —
                    // the convention every other path setting in the registry
                    // already uses.
                    Some(_) => {}
                    None => file.faults.push(fault(
                        LayerFaultKind::ScalarExpected,
                        Some(name),
                        None,
                        format!("{name} must be a quoted path string"),
                    )),
                }
            }
            "settings" => {
                let Some(table) = value.as_table() else {
                    file.faults.push(fault(
                        LayerFaultKind::NotATable,
                        Some(name),
                        None,
                        "[settings] must be a table of key = value lines".to_owned(),
                    ));
                    continue;
                };
                if !tier.may_set_settings() {
                    let named = flatten_settings("", table)
                        .into_iter()
                        .map(|(key, _)| key)
                        .collect::<Vec<_>>()
                        .join(", ");
                    file.faults.push(fault(
                        LayerFaultKind::SettingsInNonMainFolder,
                        Some(name),
                        None,
                        format!(
                            "[settings] is refused here: a folder that is not the main sync \
                             folder may only set keys about itself, in [folder]. Two folders \
                             setting one app-wide key would be decided by whichever synced \
                             last. Move these to ~/.keeper/{FILE_STEM}.toml: {named}"
                        ),
                    ));
                    continue;
                }
                for (key, raw) in flatten_settings("", table) {
                    let key = &key;
                    if let Err(reason) = keys::layer_may_set(key, tier.machine_scoped()) {
                        file.faults.push(fault(
                            LayerFaultKind::KeyRefused,
                            Some(key),
                            None,
                            reason.to_string(),
                        ));
                        continue;
                    }
                    let text = match setting_text(key, raw) {
                        Ok(text) => text,
                        Err(message) => {
                            file.faults.push(fault(
                                LayerFaultKind::ValueShape,
                                Some(key),
                                None,
                                message,
                            ));
                            continue;
                        }
                    };
                    file.settings.insert(
                        key.clone(),
                        SettingOverride {
                            value: text,
                            source: LayerSource {
                                tier,
                                path: path.to_path_buf(),
                                folder: folder.map(str::to_owned),
                                account: None,
                                repo_host: None,
                            },
                        },
                    );
                }
            }
            "folder" => {
                let Some(table) = value.as_table() else {
                    file.faults.push(fault(
                        LayerFaultKind::NotATable,
                        Some(name),
                        None,
                        "[folder] must be a table of key = value lines".to_owned(),
                    ));
                    continue;
                };
                if !tier.has_folder() {
                    let place = match tier {
                        LayerTier::AccountShared | LayerTier::AccountDevice => {
                            "your account's files"
                        }
                        _ => "~/.keeper/",
                    };
                    file.faults.push(fault(
                        LayerFaultKind::UnknownTable,
                        Some(name),
                        None,
                        format!(
                            "[folder] names no folder in {place}; folder settings belong in \
                             <folder>/.keeper/{FILE_STEM}.toml, where they travel with the folder"
                        ),
                    ));
                    continue;
                }
                file.folder = Some(table.clone());
            }
            other => file.faults.push(fault(
                LayerFaultKind::UnknownTable,
                Some(other),
                None,
                format!(
                    "unknown top-level key {other:?}; a layer file holds mainSyncFolder, \
                     [settings] and [folder]. Settings keys go inside [settings], not at the top."
                ),
            )),
        }
    }
    file
}

/// Phase one: `~/.keeper/`, then — if a user-global file named one — the main
/// sync folder's two files, read straight off the disk with no database
/// (AD-101).
///
/// `home` is the user's home directory; `host` is [`read_host_label`]. Pure in
/// the sense that matters: it discovers nothing on its own, it reads exactly the
/// six paths those two arguments name. It cannot fail — every problem is a
/// [`LayerFault`] in the result.
pub fn load_app_layers(home: &Path, host: &str) -> AppLayers {
    let mut layers = AppLayers::default();
    let user_dir = keeper_dir(home);
    let [user_shared, user_machine] = layer_paths(&user_dir, host);
    apply_file(&mut layers, &user_shared, LayerTier::UserGlobal, None);
    apply_file(
        &mut layers,
        &user_machine,
        LayerTier::UserGlobalMachine,
        None,
    );

    let Some(declared) = layers.main_folder.clone() else {
        return layers;
    };
    // A leading `~/` is what a person hand-editing a config file writes. Nothing
    // expands it for us here, and silently looking for a directory literally
    // named `~` is the kind of failure that costs an afternoon.
    let main = match declared.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => declared.clone(),
    };
    layers.main_folder = Some(main.clone());

    let source = user_machine_source(&user_shared, &user_machine, &main);
    match std::fs::metadata(&main) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            layers.faults.push(LayerFault {
                kind: LayerFaultKind::MainFolderNotADirectory,
                message: format!(
                    "mainSyncFolder = {:?} is not a directory, so its layer files were not read",
                    main.display().to_string()
                ),
                ..source
            });
            return layers;
        }
        Err(_) => {
            layers.faults.push(LayerFault {
                kind: LayerFaultKind::MainFolderMissing,
                message: format!(
                    "mainSyncFolder = {:?} does not exist, so its layer files were not read \
                     (an unmounted volume looks exactly like this)",
                    main.display().to_string()
                ),
                ..source
            });
            return layers;
        }
    }

    let folder = main.display().to_string();
    let main_dir = keeper_dir(&main);
    let [main_shared, main_machine] = layer_paths(&main_dir, host);
    apply_file(
        &mut layers,
        &main_shared,
        LayerTier::MainShared,
        Some(&folder),
    );
    apply_file(
        &mut layers,
        &main_machine,
        LayerTier::MainMachine,
        Some(&folder),
    );
    layers
}

/// The skeleton fault for a `mainSyncFolder` problem: blame whichever
/// user-global file is actually on disk, since that is the one to edit.
fn user_machine_source(shared: &Path, machine: &Path, main: &Path) -> LayerFault {
    let (path, tier) = if machine.exists() {
        (machine, LayerTier::UserGlobalMachine)
    } else {
        (shared, LayerTier::UserGlobal)
    };
    LayerFault {
        kind: LayerFaultKind::MainFolderMissing,
        path: path.to_path_buf(),
        tier: Some(tier),
        folder: Some(main.display().to_string()),
        key: Some("mainSyncFolder".to_owned()),
        line: None,
        message: String::new(),
    }
}

/// Read one file and fold it into `layers`.
///
/// An absent file is the normal case and is silent. `extend` on a `BTreeMap`
/// overwrites **per key**, which is the whole of the merge rule: a machine file
/// setting one key leaves the shared file's other keys standing.
fn apply_file(layers: &mut AppLayers, path: &Path, tier: LayerTier, folder: Option<&str>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            layers.faults.push(LayerFault {
                kind: LayerFaultKind::Unreadable,
                path: path.to_path_buf(),
                tier: Some(tier),
                folder: folder.map(str::to_owned),
                key: None,
                line: None,
                message: format!("could not be read, so this layer was skipped: {error}"),
            });
            return;
        }
    };
    let file = parse_layer_file(path, tier, folder, &text);
    layers.faults.extend(file.faults);
    if let Some(main) = file.main_sync_folder {
        layers.main_folder = Some(main);
    }
    layers.overrides.extend(file.settings);
}

/// The installed stack.
///
/// A `OnceLock`, not an `RwLock`. There is exactly one writer, [`install`], and
/// it runs before anything reads: after phase one the files it holds never
/// change, because the only later *file* layers are per-folder and a folder may
/// not set a settings key at all. So a lock would guard nothing, and it is not
/// free — an `RwLock` read is a read-modify-write on a shared cacheline, and
/// [`setting_override`] is called by every one of the ~40 typed getters on the
/// startup path, several of them in loops. `OnceLock::get` is one acquire load.
///
/// Two things genuinely mutate after install: the fault list (`push_fault`,
/// for the shell's phase two), which gets a `Mutex` off the hot path, and the
/// account tiers, which live beside this stack in [`ACCOUNT`] rather than in it.
static LAYERS: OnceLock<AppLayers> = OnceLock::new();

/// Faults raised after [`install`], by the shell's phase two.
static LATE_FAULTS: Mutex<Vec<LayerFault>> = Mutex::new(Vec::new());

fn late_faults() -> std::sync::MutexGuard<'static, Vec<LayerFault>> {
    // Poisoning here means some other thread panicked while pushing a fault. The
    // fault list is append-only prose; there is no invariant left half-broken,
    // and dropping the settings pane's diagnostics because of it helps nobody.
    LATE_FAULTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Install the phase-one stack, once, before the first settings read.
///
/// A second call is ignored and logged rather than panicking: a duplicate
/// install is a wiring bug in a code path that must not be able to stop the app
/// from starting.
pub fn install(layers: AppLayers) {
    if LAYERS.set(layers).is_err() {
        tracing::warn!("config: the layer stack is already installed; ignoring the second install");
    }
}

/// The resolved value for `key`, or `None` when no layer sets it.
///
/// Consulted by [`crate::registry::get_setting`] **before** it opens a database
/// connection.
pub fn setting_override(key: &str) -> Option<SettingOverride> {
    let frozen = frozen_override(key);
    with_account_layers(|account| {
        let Some(account) = account.and_then(|layers| layers.overrides.get(key)) else {
            return frozen;
        };
        match frozen {
            Some(frozen) if frozen.source.tier > account.source.tier => Some(frozen),
            _ => Some(account.clone()),
        }
    })
}

fn frozen_override(key: &str) -> Option<SettingOverride> {
    #[cfg(test)]
    if let Some(layers) = test_layers() {
        return layers.overrides.get(key).cloned();
    }
    LAYERS.get()?.overrides.get(key).cloned()
}

/// Every key a layer sets, with where it came from, sorted by key.
///
/// The settings pane reads this to mark a control "set by a file" instead of
/// letting a person move a slider that will not take.
pub fn overrides() -> Vec<(String, LayerSource)> {
    let frozen: Vec<(String, LayerSource)> = with_installed(|layers| {
        layers
            .overrides
            .iter()
            .map(|(key, over)| (key.clone(), over.source.clone()))
            .collect()
    })
    .unwrap_or_default();
    with_account_layers(|account| {
        let Some(account) = account else {
            return frozen;
        };
        let mut merged: BTreeMap<String, LayerSource> = frozen.into_iter().collect();
        for (key, over) in &account.overrides {
            match merged.get(key) {
                Some(existing) if existing.tier > over.source.tier => {}
                _ => {
                    merged.insert(key.clone(), over.source.clone());
                }
            }
        }
        merged.into_iter().collect()
    })
}

/// The designated main sync folder, for the shell's phase two.
pub fn main_folder() -> Option<PathBuf> {
    with_installed(|layers| layers.main_folder.clone()).flatten()
}

/// Everything wrong with the layer files: phase one's faults, then the
/// account's (the descriptor and identity faults the shell set, then the
/// account files' own), then any the shell added afterwards.
pub fn faults() -> Vec<LayerFault> {
    let mut all = with_installed(|layers| layers.faults.clone()).unwrap_or_default();
    with_account_slot(|slot| {
        all.extend(read_lock(&slot.faults).iter().cloned());
        if let Some(layers) = read_lock(&slot.layers).as_ref() {
            all.extend(layers.faults.iter().cloned());
        }
    });
    all.extend(late_faults().iter().cloned());
    all
}

/// Record a fault discovered after install — phase two's "`mainSyncFolder` names
/// no profile", for instance. Safe before install, too.
pub fn push_fault(fault: LayerFault) {
    tracing::error!(%fault, "config: layer fault");
    late_faults().push(fault);
}

fn with_installed<T>(read: impl FnOnce(&AppLayers) -> T) -> Option<T> {
    #[cfg(test)]
    if let Some(layers) = test_layers() {
        return Some(read(&layers));
    }
    LAYERS.get().map(read)
}

// ---------------------------------------------------------------------------
// The account tiers (Epic 82, AD-309)
// ---------------------------------------------------------------------------

/// Where the signed-in person's two layer files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLayerSource {
    /// `<clone>/<login>` — this person's own directory in the config repository.
    pub dir: PathBuf,
    /// This device's name, the `<device>` of `keeper.<device>.toml`.
    pub device: String,
    /// The account's display name, for "Set by a file" and the fault list.
    pub account_name: String,
    /// The config repository's host (`git.acme.dev`).
    pub repo_host: String,
}

/// The account's two files, parsed and merged (device over shared, per key).
#[derive(Debug, Clone, Default)]
pub struct AccountLayers {
    pub overrides: BTreeMap<String, SettingOverride>,
    pub faults: Vec<LayerFault>,
}

/// Read `keeper.toml` and `keeper.<device>.toml` from the person's directory,
/// with the same parser, the same `[settings]` rights and the same faults as
/// `~/.keeper/`. `mainSyncFolder` and `[folder]` are refused by the tiers'
/// predicates, not by anything here. Cannot fail; an absent file is silent.
///
/// A layer file (or the directory itself) that is a symlink is a fault, never
/// followed: anyone who can push to the repository could otherwise point the
/// account's settings at a file outside the clone.
pub fn load_account_layers(src: &AccountLayerSource) -> AccountLayers {
    let mut read = AppLayers::default();
    // `layer_paths` folds the device name to a filename the way it folds a
    // host label, so a device called `../x` cannot leave the directory.
    let [shared, device] = layer_paths(&src.dir, &src.device);
    let dir_is_link = std::fs::symlink_metadata(&src.dir).is_ok_and(|m| m.file_type().is_symlink());
    for (path, tier) in [
        (shared, LayerTier::AccountShared),
        (device, LayerTier::AccountDevice),
    ] {
        let not_a_file =
            dir_is_link || std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file());
        if not_a_file {
            read.faults.push(LayerFault {
                kind: LayerFaultKind::Unreadable,
                path,
                tier: Some(tier),
                folder: None,
                key: None,
                line: None,
                message: "is a link or a folder in the settings repository, not a file, so this layer was skipped".to_owned(),
            });
            continue;
        }
        apply_file(&mut read, &path, tier, None);
    }
    let mut overrides = read.overrides;
    for over in overrides.values_mut() {
        over.source.account = Some(src.account_name.clone());
        over.source.repo_host = Some(src.repo_host.clone());
    }
    AccountLayers {
        overrides,
        faults: read.faults,
    }
}

/// Swap the account tiers in, live. `None` clears them (sign-out, or an
/// identity that no longer matches). Keys read on every access take effect at
/// once; keys consumed only at boot take effect at the next launch.
pub fn install_account_layers(layers: Option<AccountLayers>) {
    match &layers {
        Some(layers) => {
            for fault in &layers.faults {
                tracing::warn!(%fault, "config: account layer fault");
            }
            tracing::info!(
                overrides = layers.overrides.len(),
                faults = layers.faults.len(),
                "config: account layers installed"
            );
        }
        None => tracing::info!("config: account layers cleared"),
    }
    with_account_slot(|slot| {
        let installed = layers.is_some();
        let previous = {
            let mut guard = slot
                .layers
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::replace(&mut *guard, layers)
        };
        slot.installed.store(installed, Ordering::Release);
        drop(previous);
    });
}

/// Replace the account's own faults — a broken `account.toml`, a sign-in that
/// belongs to someone else — which are reported beside the layer faults.
pub fn set_account_faults(faults: Vec<LayerFault>) {
    for fault in &faults {
        tracing::warn!(%fault, "config: account fault");
    }
    with_account_slot(|slot| {
        *slot
            .faults
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = faults;
    });
}

/// The live account state.
///
/// An `RwLock`, because unlike [`LAYERS`] this has a writer after boot: a fetch
/// that changed the person's files. `installed` mirrors `layers.is_some()` so
/// [`setting_override`] takes no lock at all when no account is installed —
/// an install without an account pays one atomic load over what it paid before.
struct AccountSlot {
    installed: AtomicBool,
    layers: RwLock<Option<AccountLayers>>,
    faults: RwLock<Vec<LayerFault>>,
}

impl AccountSlot {
    const fn new() -> Self {
        Self {
            installed: AtomicBool::new(false),
            layers: RwLock::new(None),
            faults: RwLock::new(Vec::new()),
        }
    }
}

#[cfg(not(test))]
static ACCOUNT: AccountSlot = AccountSlot::new();

#[cfg(not(test))]
fn with_account_slot<T>(read: impl FnOnce(&AccountSlot) -> T) -> T {
    read(&ACCOUNT)
}

// Per-thread in tests, for the reason the frozen stack's overlay is: each test
// swaps accounts freely without racing its neighbours' reads. The swap, the
// lock and the merge are the production code; only the slot's address differs.
#[cfg(test)]
thread_local! {
    static ACCOUNT: AccountSlot = const { AccountSlot::new() };
}

#[cfg(test)]
fn with_account_slot<T>(read: impl FnOnce(&AccountSlot) -> T) -> T {
    ACCOUNT.with(read)
}

fn with_account_layers<T>(read: impl FnOnce(Option<&AccountLayers>) -> T) -> T {
    with_account_slot(|slot| {
        if !slot.installed.load(Ordering::Acquire) {
            return read(None);
        }
        read(read_lock(&slot.layers).as_ref())
    })
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    // A writer only ever replaces the whole value, so a poisoned lock holds
    // either the old account or the new one — never half of either.
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Test-only overlay
// ---------------------------------------------------------------------------
//
// `LAYERS` is a `OnceLock` on purpose and therefore cannot be re-set, so tests
// get a thread-local instead. That is not a weaker substitute: `cargo test`
// gives each test its own thread, so a thread-local overlay is isolated by
// construction where a resettable global would need a mutex every test had to
// remember to take. The production `OnceLock` path is still covered — by
// `install_then_read_resolves_through_the_process_global`, the one test allowed
// to spend it.

#[cfg(test)]
thread_local! {
    static TEST_LAYERS: std::cell::RefCell<Option<std::sync::Arc<AppLayers>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_layers() -> Option<std::sync::Arc<AppLayers>> {
    TEST_LAYERS.with(|slot| slot.borrow().clone())
}

/// Removes the thread's test overlay when dropped, so a failing assertion cannot
/// leak it into the next test on the same thread.
#[cfg(test)]
pub(crate) struct TestLayerGuard;

#[cfg(test)]
impl Drop for TestLayerGuard {
    fn drop(&mut self) {
        TEST_LAYERS.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Install `layers` for this thread only. Hold the guard for the test's body.
#[cfg(test)]
pub(crate) fn install_for_test(layers: AppLayers) -> TestLayerGuard {
    TEST_LAYERS.with(|slot| *slot.borrow_mut() = Some(std::sync::Arc::new(layers)));
    TestLayerGuard
}

/// Build an `AppLayers` from `(key, value, tier)` triples, for tests that do not
/// care where the files were.
#[cfg(test)]
pub(crate) fn layers_from(entries: &[(&str, &str, LayerTier)]) -> AppLayers {
    let mut layers = AppLayers::default();
    for (key, value, tier) in entries {
        layers.overrides.insert(
            (*key).to_owned(),
            SettingOverride {
                value: (*value).to_owned(),
                source: LayerSource {
                    tier: *tier,
                    path: PathBuf::from(format!("/test/{}.toml", tier.label())),
                    folder: None,
                    account: None,
                    repo_host: None,
                },
            },
        );
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory per test, mirroring `registry`'s helper.
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("keeper-config-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the scratch dir");
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create the layer dir");
        std::fs::write(path, text).expect("write the layer file");
    }

    fn value(layers: &AppLayers, key: &str) -> Option<String> {
        layers.overrides.get(key).map(|o| o.value.clone())
    }

    fn tier_of(layers: &AppLayers, key: &str) -> Option<LayerTier> {
        layers.overrides.get(key).map(|o| o.source.tier)
    }

    // -- precedence -------------------------------------------------------

    /// Story 46.6 / AD-99. The order the owner wrote is the order we implement:
    /// user → main-shared → main-machine, later wins. Every layer sets
    /// `recording.fps`, so the winner names the whole chain, and each layer also
    /// sets a key only it sets, so a wrong *merge* (whole-file replace instead of
    /// per-key) shows up in the same assertion.
    #[test]
    fn later_layers_win_per_key_across_the_whole_stack() {
        let home = temp_dir();
        let main = home.join("tgdrive");
        std::fs::create_dir_all(&main).expect("create the main folder");
        write(
            &keeper_dir(&home).join("keeper.toml"),
            &format!(
                "mainSyncFolder = {:?}\n\
                 [settings]\n\
                 \"recording.fps\" = 10\n\
                 \"notify.previews_enabled\" = true\n",
                main.display().to_string()
            ),
        );
        write(
            &keeper_dir(&home).join("keeper.testbox.toml"),
            "[settings]\n\"recording.fps\" = 15\n\"undo_send.window\" = 5\n",
        );
        write(
            &keeper_dir(&main).join("keeper.toml"),
            "[settings]\n\"recording.fps\" = 30\n\"recording.codec\" = \"hevc\"\n",
        );
        write(
            &keeper_dir(&main).join("keeper.testbox.toml"),
            "[settings]\n\"recording.fps\" = 60\n",
        );

        let layers = load_app_layers(&home, "testbox");
        assert!(layers.faults.is_empty(), "faults: {:?}", layers.faults);
        // The last layer wins the contested key...
        assert_eq!(value(&layers, "recording.fps").as_deref(), Some("60"));
        assert_eq!(
            tier_of(&layers, "recording.fps"),
            Some(LayerTier::MainMachine)
        );
        // ...and every earlier layer keeps the keys it alone set. A per-FILE
        // merge would have dropped all three of these.
        assert_eq!(
            value(&layers, "notify.previews_enabled").as_deref(),
            Some("1")
        );
        assert_eq!(value(&layers, "undo_send.window").as_deref(), Some("5"));
        assert_eq!(value(&layers, "recording.codec").as_deref(), Some("hevc"));
        assert_eq!(
            tier_of(&layers, "notify.previews_enabled"),
            Some(LayerTier::UserGlobal)
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The precedence claim, stated as an ordering rather than an outcome: for
    /// any two adjacent tiers the later one wins. This is what fails when the
    /// tiers are read in the wrong order but each individual file still parses.
    #[test]
    fn every_adjacent_pair_of_app_tiers_resolves_to_the_later_one() {
        let app_tiers = [
            LayerTier::UserGlobal,
            LayerTier::UserGlobalMachine,
            LayerTier::MainShared,
            LayerTier::MainMachine,
        ];
        for pair in app_tiers.windows(2) {
            let (earlier, later) = (pair[0], pair[1]);
            let home = temp_dir();
            let main = home.join("tgdrive");
            std::fs::create_dir_all(&main).expect("create the main folder");
            write(
                &keeper_dir(&home).join("keeper.toml"),
                &format!("mainSyncFolder = {:?}\n", main.display().to_string()),
            );
            for (tier, fps) in [(earlier, 240), (later, 480)] {
                let dir = match tier {
                    LayerTier::UserGlobal | LayerTier::UserGlobalMachine => keeper_dir(&home),
                    _ => keeper_dir(&main),
                };
                let name = if tier.machine_scoped() {
                    "keeper.testbox.toml"
                } else {
                    "keeper.toml"
                };
                let mut text = String::new();
                if tier == LayerTier::UserGlobal {
                    text.push_str(&format!(
                        "mainSyncFolder = {:?}\n",
                        main.display().to_string()
                    ));
                }
                text.push_str(&format!("[settings]\n\"recording.segment_mb\" = {fps}\n"));
                write(&dir.join(name), &text);
            }
            let layers = load_app_layers(&home, "testbox");
            assert_eq!(
                value(&layers, "recording.segment_mb").as_deref(),
                Some("480"),
                "{later:?} must beat {earlier:?}"
            );
            assert_eq!(tier_of(&layers, "recording.segment_mb"), Some(later));
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    // -- faults are never fatal -------------------------------------------

    /// A malformed file costs exactly its own layer. The layer above it and the
    /// layer below it both still apply, and the fault names the file and a line.
    #[test]
    fn a_malformed_layer_is_skipped_whole_and_takes_nothing_with_it() {
        let home = temp_dir();
        let main = home.join("tgdrive");
        std::fs::create_dir_all(&main).expect("create the main folder");
        write(
            &keeper_dir(&home).join("keeper.toml"),
            &format!(
                "mainSyncFolder = {:?}\n[settings]\n\"recording.fps\" = 10\n\"recording.codec\" = \"h264\"\n",
                main.display().to_string()
            ),
        );
        // Valid first line, then a syntax error: proves the layer is dropped
        // WHOLE rather than up to the bad line.
        write(
            &keeper_dir(&home).join("keeper.testbox.toml"),
            "[settings]\n\"recording.fps\" = 15\nthis is not toml =\n",
        );
        write(
            &keeper_dir(&main).join("keeper.toml"),
            "[settings]\n\"undo_send.window\" = 7\n",
        );

        let layers = load_app_layers(&home, "testbox");
        // The broken layer contributed nothing, not even its good line.
        assert_eq!(value(&layers, "recording.fps").as_deref(), Some("10"));
        // Both healthy layers still applied — including the one AFTER the fault.
        assert_eq!(value(&layers, "recording.codec").as_deref(), Some("h264"));
        assert_eq!(value(&layers, "undo_send.window").as_deref(), Some("7"));

        let fault = layers
            .faults
            .iter()
            .find(|f| f.kind == LayerFaultKind::Malformed)
            .expect("a malformed fault");
        assert!(fault.path.ends_with("keeper.testbox.toml"));
        assert_eq!(fault.line, Some(3), "the fault must name the bad line");
        assert!(
            format!("{fault}").contains("keeper.testbox.toml:3"),
            "Display must carry path and line, got {fault}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// An absent file is the normal case, not a fault, and an empty stack
    /// resolves nothing rather than failing.
    #[test]
    fn no_files_at_all_is_silent() {
        let home = temp_dir();
        let layers = load_app_layers(&home, "testbox");
        assert!(layers.overrides.is_empty());
        assert!(layers.faults.is_empty());
        assert_eq!(layers.main_folder, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// An unmounted volume is indistinguishable from a typo, and both must boot.
    ///
    /// The absent path is built INSIDE this test's own temp home rather than
    /// named as a literal. The literal here used to be `/Volumes/merope/tgdrive`
    /// — a real volume on the machine keeper ships from, so the test asserted
    /// "missing" against a folder that was present and failed on the only host
    /// that could run the shell. A path this test creates the parent of, and
    /// deliberately does not create, is absent on every OS by construction.
    #[test]
    fn a_main_folder_that_is_not_there_faults_without_losing_the_user_layer() {
        let home = temp_dir();
        let absent = home.join("an-unmounted-volume");
        assert!(!absent.exists(), "the fixture must not exist to be missing");
        write(
            &keeper_dir(&home).join("keeper.toml"),
            &format!(
                "mainSyncFolder = {:?}\n[settings]\n\"recording.fps\" = 30\n",
                absent.to_string_lossy()
            ),
        );
        let layers = load_app_layers(&home, "testbox");
        assert_eq!(value(&layers, "recording.fps").as_deref(), Some("30"));
        // The declared path is KEPT so the UI can show what was asked for.
        assert_eq!(layers.main_folder, Some(absent));
        assert_eq!(
            layers.faults.iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![LayerFaultKind::MainFolderMissing]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A hand-edited config file says `~/tgdrive`, and nothing else expands it.
    #[test]
    fn a_tilde_main_folder_is_resolved_against_home() {
        let home = temp_dir();
        let main = home.join("tgdrive");
        std::fs::create_dir_all(&main).expect("create the main folder");
        write(
            &keeper_dir(&home).join("keeper.toml"),
            "mainSyncFolder = \"~/tgdrive\"\n",
        );
        write(
            &keeper_dir(&main).join("keeper.toml"),
            "[settings]\n\"recording.fps\" = 30\n",
        );
        let layers = load_app_layers(&home, "testbox");
        assert_eq!(layers.main_folder, Some(main.clone()));
        assert_eq!(value(&layers, "recording.fps").as_deref(), Some("30"));
        assert!(layers.faults.is_empty(), "{:?}", layers.faults);
        let _ = std::fs::remove_dir_all(&home);
    }

    // -- the value mapping -------------------------------------------------

    /// A boolean is written as a boolean and lands in **the spelling its own
    /// getter reads**. Three of these keys disagree about how to spell true, and
    /// nobody editing a file should have to know which.
    ///
    /// This is the regression for a real defect: a convention-blind mapping made
    /// every boolean `"1"`, so `honor_remote_deletions = true` resolved to `"1"`
    /// and `archive::get_honor_remote_deletions` — which compares against
    /// `"on"` — read it as **false**. The setting did the opposite of what the
    /// file said, silently.
    #[test]
    fn each_boolean_lands_in_the_spelling_its_own_getter_reads() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[settings]\n\
             \"notify.previews_enabled\" = true\n\
             \"notify.dnd_global\" = false\n\
             honor_remote_deletions = true\n\
             favorites_collapsed = false\n",
        );
        assert!(file.faults.is_empty(), "{:?}", file.faults);
        let text = |k: &str| file.settings.get(k).map(|o| o.value.clone());
        // The `"1"`/`"0"` convention the namespaced keys use...
        assert_eq!(text("notify.previews_enabled").as_deref(), Some("1"));
        assert_eq!(text("notify.dnd_global").as_deref(), Some("0"));
        // ...and the two legacy spellings that predate it.
        assert_eq!(text("honor_remote_deletions").as_deref(), Some("on"));
        assert_eq!(text("favorites_collapsed").as_deref(), Some("false"));
    }

    /// Numbers and strings: written as themselves, stored as decimal text and
    /// verbatim. The stored spelling is also accepted, so a value copied out of
    /// the settings pane can be pasted into the file.
    #[test]
    fn numbers_and_strings_land_as_the_table_spells_them() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[settings]\n\
             \"recording.segment_mb\" = 800\n\
             \"undo_send.window\" = \"5\"\n\
             \"recording.codec\" = \"hevc\"\n\
             \"recording.fps\" = 30\n",
        );
        assert!(file.faults.is_empty(), "{:?}", file.faults);
        let text = |k: &str| file.settings.get(k).map(|o| o.value.clone());
        assert_eq!(text("recording.segment_mb").as_deref(), Some("800"));
        assert_eq!(text("undo_send.window").as_deref(), Some("5"));
        assert_eq!(text("recording.codec").as_deref(), Some("hevc"));
        assert_eq!(text("recording.fps").as_deref(), Some("30"));
    }

    /// TOML reads an unquoted dot as nesting, so there are three correct ways to
    /// write one namespaced key and a person will use all three. They mean the
    /// same key. The alternative is telling somebody that `recording` is not a
    /// setting when what they wrote reads correctly to every other TOML tool.
    #[test]
    fn a_dotted_key_means_the_same_thing_however_it_is_written() {
        let spellings = [
            "[settings]\n\"recording.segment_mb\" = 800\n",
            "[settings]\nrecording.segment_mb = 800\n",
            "[settings.recording]\nsegment_mb = 800\n",
        ];
        for text in spellings {
            let file = parse_layer_file(
                Path::new("/x/keeper.toml"),
                LayerTier::UserGlobal,
                None,
                text,
            );
            assert!(file.faults.is_empty(), "{text:?}: {:?}", file.faults);
            assert_eq!(
                file.settings
                    .get("recording.segment_mb")
                    .map(|o| o.value.as_str()),
                Some("800"),
                "{text:?}"
            );
        }
    }

    /// A key that flattens to something the registry does not know still names
    /// itself — the nesting must not swallow the typo into a bare `recordng`.
    #[test]
    fn a_typo_inside_a_nested_settings_table_names_the_whole_key() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[settings]\nrecording.frames_per_second = 30\n",
        );
        assert!(file.settings.is_empty());
        let fault = file.faults.first().expect("a fault");
        assert_eq!(fault.kind, LayerFaultKind::KeyRefused);
        assert_eq!(fault.key.as_deref(), Some("recording.frames_per_second"));
    }

    /// Where the TOML layer and `config.json` agree, and — deliberately — where
    /// they do not.
    ///
    /// For every key on the `"1"`/`"0"` convention the two paths must land the
    /// same text, or two config files would mean two different things by the
    /// same line. For the three legacy on/off keys they diverge, and the TOML
    /// answer is the correct one: `config.json` goes through
    /// `registry::scalar_setting_text`, which is convention-blind and writes
    /// `"1"` for a key whose reader compares against `"on"`. That bug is left
    /// where it is on purpose — `config.json` is the layer AD-98 replaces, it
    /// sits at the bottom of the stack, and changing what it writes would move
    /// settings on a machine already running one.
    #[test]
    fn the_toml_layer_agrees_with_config_json_except_on_the_legacy_spellings() {
        let agree: [(&str, &str, serde_json::Value); 3] = [
            ("notify.previews_enabled", "true", serde_json::json!(true)),
            ("notify.dnd_global", "false", serde_json::json!(false)),
            ("recording.segment_mb", "800", serde_json::json!(800)),
        ];
        for (key, literal, json) in agree {
            let file = parse_layer_file(
                Path::new("/x/keeper.toml"),
                LayerTier::UserGlobal,
                None,
                &format!("[settings]\n{key:?} = {literal}\n"),
            );
            let from_toml = file
                .settings
                .get(key)
                .map(|o| o.value.clone())
                .unwrap_or_else(|| panic!("{key} parsed, faults {:?}", file.faults));
            let from_json = crate::registry::scalar_setting_text(&json).expect("a json scalar");
            assert_eq!(from_toml, from_json, "{key} must import the same both ways");
        }

        // The divergence, pinned rather than tolerated silently.
        for (key, stored, legacy) in [
            ("honor_remote_deletions", "on", "1"),
            ("favorites_collapsed", "true", "1"),
        ] {
            let file = parse_layer_file(
                Path::new("/x/keeper.toml"),
                LayerTier::UserGlobal,
                None,
                &format!("[settings]\n{key} = true\n"),
            );
            assert_eq!(
                file.settings.get(key).map(|o| o.value.as_str()),
                Some(stored),
                "the TOML layer must use the spelling {key}'s getter reads"
            );
            assert_eq!(
                crate::registry::scalar_setting_text(&serde_json::json!(true)).as_deref(),
                Some(legacy),
                "config.json's legacy mapping is unchanged"
            );
        }
    }

    /// A value that does not fit its key's shape is named and costs only that
    /// key: an array where a codec belongs, a `nan` where a scale belongs, and a
    /// number outside the range its getter accepts. The last is the interesting
    /// one — the getter would clamp it, but a number a person typed into a file
    /// they can see is worth a sentence back instead.
    #[test]
    fn a_value_of_the_wrong_shape_faults_by_name_and_costs_only_its_own_key() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[settings]\n\
             \"recording.fps\" = 30\n\
             \"recording.codec\" = [1, 2]\n\
             \"recording.scale_percent\" = nan\n\
             \"recording.segment_mb\" = 99999\n",
        );
        assert_eq!(file.settings.len(), 1, "{:?}", file.settings);
        assert_eq!(
            file.settings.get("recording.fps").map(|o| o.value.as_str()),
            Some("30")
        );
        let named: Vec<_> = file
            .faults
            .iter()
            .filter(|f| f.kind == LayerFaultKind::ValueShape)
            .filter_map(|f| f.key.clone())
            .collect();
        assert_eq!(
            named,
            vec![
                "recording.codec",
                "recording.scale_percent",
                "recording.segment_mb"
            ]
        );
        // The sentence has to be usable: name the key and the range.
        let ranged = file
            .faults
            .iter()
            .find(|f| f.key.as_deref() == Some("recording.segment_mb"))
            .expect("the range fault");
        assert!(
            ranged.message.contains("5000") && ranged.message.contains("99999"),
            "{}",
            ranged.message
        );
    }

    /// `[settings]` written as a scalar is a shape error too, not a panic.
    #[test]
    fn a_settings_key_that_is_not_a_table_faults() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "settings = 3\n",
        );
        assert_eq!(
            file.faults.iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![LayerFaultKind::NotATable]
        );
    }

    /// A settings key written at the top level instead of inside `[settings]` is
    /// the most likely hand-edit mistake there is; it must say so.
    #[test]
    fn an_unknown_top_level_key_names_itself() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "\"recording.fps\" = 30\n",
        );
        assert!(file.settings.is_empty());
        let fault = file.faults.first().expect("a fault");
        assert_eq!(fault.kind, LayerFaultKind::UnknownTable);
        assert_eq!(fault.key.as_deref(), Some("recording.fps"));
        assert!(fault.message.contains("[settings]"), "{}", fault.message);
    }

    // -- scoping -----------------------------------------------------------

    /// AD-99's constraint: a non-main folder may only set keys about itself.
    /// Loud and named, not a silent ignore — and `[folder]` beside it still
    /// survives, because that is the table the folder IS allowed to write.
    #[test]
    fn settings_in_a_non_main_folder_is_a_named_fault_and_folder_survives() {
        for tier in [LayerTier::FolderShared, LayerTier::FolderMachine] {
            let file = parse_layer_file(
                Path::new("/vault/.keeper/keeper.toml"),
                tier,
                Some("/vault"),
                "[settings]\n\"hotkey.global\" = \"Ctrl+Space\"\n\
                 [folder]\nrecordingsSubfolder = \"40-media/recordings\"\n",
            );
            assert!(
                file.settings.is_empty(),
                "{tier:?} must not contribute settings"
            );
            let fault = file
                .faults
                .iter()
                .find(|f| f.kind == LayerFaultKind::SettingsInNonMainFolder)
                .unwrap_or_else(|| panic!("{tier:?} must fault, got {:?}", file.faults));
            assert!(
                fault.message.contains("hotkey.global"),
                "the fault must name the refused keys: {}",
                fault.message
            );
            assert_eq!(fault.folder.as_deref(), Some("/vault"));
            assert!(file.folder.is_some(), "[folder] must still be handed on");
        }
    }

    /// `[settings]` IS allowed in the main sync folder's files — that is the
    /// difference the tier exists to express.
    #[test]
    fn settings_in_the_main_folder_is_allowed() {
        for tier in [LayerTier::MainShared, LayerTier::MainMachine] {
            let file = parse_layer_file(
                Path::new("/main/.keeper/keeper.toml"),
                tier,
                Some("/main"),
                "[settings]\n\"recording.fps\" = 30\n",
            );
            assert!(file.faults.is_empty(), "{tier:?}: {:?}", file.faults);
            assert_eq!(file.settings.len(), 1);
        }
    }

    /// A folder cannot elect itself the main folder.
    #[test]
    fn main_sync_folder_is_refused_outside_the_user_layer() {
        for tier in [
            LayerTier::MainShared,
            LayerTier::MainMachine,
            LayerTier::FolderShared,
            LayerTier::FolderMachine,
        ] {
            let file = parse_layer_file(
                Path::new("/f/.keeper/keeper.toml"),
                tier,
                Some("/f"),
                "mainSyncFolder = \"/elsewhere\"\n",
            );
            assert_eq!(file.main_sync_folder, None, "{tier:?}");
            assert_eq!(
                file.faults.iter().map(|f| f.kind).collect::<Vec<_>>(),
                vec![LayerFaultKind::MainFolderInFolderLayer],
                "{tier:?}"
            );
        }
    }

    /// The per-machine user file is a real tier and may name the main folder —
    /// the whole point, since a mount path differs between a Mac and a Linux box.
    #[test]
    fn the_per_machine_user_file_may_name_the_main_folder_and_wins() {
        let home = temp_dir();
        let mac = home.join("Volumes-merope");
        let this = home.join("mnt-tgdrive");
        std::fs::create_dir_all(&mac).expect("create mac main");
        std::fs::create_dir_all(&this).expect("create this main");
        write(
            &keeper_dir(&home).join("keeper.toml"),
            &format!("mainSyncFolder = {:?}\n", mac.display().to_string()),
        );
        write(
            &keeper_dir(&home).join("keeper.testbox.toml"),
            &format!("mainSyncFolder = {:?}\n", this.display().to_string()),
        );
        write(
            &keeper_dir(&this).join("keeper.toml"),
            "[settings]\n\"recording.fps\" = 30\n",
        );
        let layers = load_app_layers(&home, "testbox");
        assert_eq!(layers.main_folder, Some(this));
        assert_eq!(value(&layers, "recording.fps").as_deref(), Some("30"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// `[folder]` in `~/.keeper/` names no folder, and saying nothing would leave
    /// the owner waiting for a setting that never applies.
    #[test]
    fn a_folder_table_in_the_user_layer_faults() {
        let file = parse_layer_file(
            Path::new("/home/x/.keeper/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[folder]\nrecordingsSubfolder = \"r\"\n",
        );
        assert!(file.folder.is_none());
        assert_eq!(
            file.faults.iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![LayerFaultKind::UnknownTable]
        );
    }

    /// `keys::layer_may_set` decides; the parser reports its refusal by name and
    /// keeps going. `sdk_encryption` is the case that matters most: the row only
    /// describes whether a passphrase exists in THIS machine's Keychain, and a
    /// file cannot create that item.
    #[test]
    fn a_refused_key_faults_by_name_and_the_rest_of_the_file_applies() {
        let file = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            "[settings]\nsdk_encryption = \"on\"\n\"recording.fps\" = 30\n",
        );
        assert!(!file.settings.contains_key("sdk_encryption"));
        assert_eq!(
            file.settings.get("recording.fps").map(|o| o.value.as_str()),
            Some("30")
        );
        let fault = file
            .faults
            .iter()
            .find(|f| f.kind == LayerFaultKind::KeyRefused)
            .expect("a refusal");
        assert_eq!(fault.key.as_deref(), Some("sdk_encryption"));
        assert!(
            fault.message.contains("sdk_encryption"),
            "the refusal must name the key: {}",
            fault.message
        );
    }

    /// The three un-namespaced keys that predate `recording.*` / `notify.*`,
    /// through the whole load path: two resolve, and `sdk_encryption` — which is
    /// keyed to a Keychain item a file cannot create — is refused by name.
    #[test]
    fn the_legacy_un_namespaced_keys_resolve_or_are_refused_by_name() {
        let home = temp_dir();
        write(
            &keeper_dir(&home).join("keeper.toml"),
            "[settings]\n\
             honor_remote_deletions = true\n\
             favorites_collapsed = false\n\
             sdk_encryption = \"on\"\n",
        );
        let layers = load_app_layers(&home, "testbox");
        assert_eq!(
            value(&layers, "honor_remote_deletions").as_deref(),
            Some("on")
        );
        assert_eq!(
            value(&layers, "favorites_collapsed").as_deref(),
            Some("false")
        );
        assert_eq!(value(&layers, "sdk_encryption"), None);
        let refusal = layers
            .faults
            .iter()
            .find(|f| f.kind == LayerFaultKind::KeyRefused)
            .expect("sdk_encryption must be refused");
        assert_eq!(refusal.key.as_deref(), Some("sdk_encryption"));
        assert!(
            refusal.message.contains("Keychain"),
            "the refusal must say why: {}",
            refusal.message
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A machine-local key is legitimate in `keeper.<host>.toml` and refused in a
    /// shared file — the whole reason the machine tier exists.
    #[test]
    fn a_machine_local_key_is_accepted_only_in_a_machine_file() {
        let text = "[settings]\n\"sync.git_path\" = \"/opt/homebrew/bin/git\"\n";
        let shared = parse_layer_file(
            Path::new("/x/keeper.toml"),
            LayerTier::UserGlobal,
            None,
            text,
        );
        assert!(shared.settings.is_empty());
        assert_eq!(
            shared.faults.iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![LayerFaultKind::KeyRefused]
        );
        let machine = parse_layer_file(
            Path::new("/x/keeper.testbox.toml"),
            LayerTier::UserGlobalMachine,
            None,
            text,
        );
        assert!(machine.faults.is_empty(), "{:?}", machine.faults);
        assert_eq!(
            machine
                .settings
                .get("sync.git_path")
                .map(|o| o.value.as_str()),
            Some("/opt/homebrew/bin/git")
        );
    }

    /// No file at any tier arms the microphone: a pin of `bots.wake_enabled`
    /// — this machine's, the account's or the main folder's — is refused by
    /// name and never reaches the resolved settings.
    #[test]
    fn no_layer_may_switch_the_wake_phrase_on() {
        let text = "[settings]\n\"bots.wake_enabled\" = true\n";
        for tier in [
            LayerTier::UserGlobal,
            LayerTier::UserGlobalMachine,
            LayerTier::AccountShared,
            LayerTier::AccountDevice,
            LayerTier::MainShared,
            LayerTier::MainMachine,
        ] {
            let file = parse_layer_file(Path::new("/x/keeper.testbox.toml"), tier, None, text);
            assert!(!file.settings.contains_key("bots.wake_enabled"), "{tier:?}");
            let refusal = file
                .faults
                .iter()
                .find(|f| f.kind == LayerFaultKind::KeyRefused)
                .unwrap_or_else(|| panic!("{tier:?} must refuse it"));
            assert_eq!(refusal.key.as_deref(), Some("bots.wake_enabled"));
        }
    }

    // -- paths and the host label ------------------------------------------

    #[test]
    fn a_host_label_is_always_produced_and_is_a_short_name() {
        // Provenance identifies the machine; an empty or dotted label makes
        // every commit trailer either useless or noisy — and a dotted label
        // would put a second extension in `keeper.<host>.toml`.
        let label = read_host_label();
        assert!(!label.is_empty());
        assert!(
            !label.contains('.'),
            "expected a short label, got {label:?}"
        );
    }

    /// A hostname is not a filename. Nothing a machine can be called may reach
    /// outside the `.keeper/` directory we meant to read.
    #[test]
    fn a_hostile_host_label_cannot_escape_the_keeper_directory() {
        let dir = Path::new("/home/x/.keeper");
        for host in ["../../etc/passwd", "my host", "", "///", "a/b"] {
            let [shared, machine] = layer_paths(dir, host);
            assert_eq!(shared, dir.join("keeper.toml"));
            assert_eq!(
                machine.parent(),
                Some(dir),
                "{host:?} escaped to {}",
                machine.display()
            );
            let name = machine
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a file name");
            assert!(name.starts_with("keeper."), "{name}");
            assert!(name.ends_with(".toml"), "{name}");
        }
    }

    // -- the process-global ------------------------------------------------

    /// The one test that spends the `OnceLock`: `install` then read, through the
    /// real production path rather than the thread-local test overlay. Also
    /// covers `overrides`, `faults` and `push_fault`.
    ///
    /// The keys are deliberately fictitious. `LAYERS` is process-global and
    /// cannot be un-set, so a real key installed here would silently shadow the
    /// table for every other test in this binary — which is precisely the bug
    /// this module exists to create on purpose, and exactly what a test must not
    /// do to its neighbours.
    #[test]
    fn install_then_read_resolves_through_the_process_global() {
        let mut layers = layers_from(&[
            ("zz.install_probe.alpha", "30", LayerTier::UserGlobal),
            ("zz.install_probe.beta", "hevc", LayerTier::MainMachine),
        ]);
        layers.main_folder = Some(PathBuf::from("/zz-install-probe"));
        layers.faults.push(LayerFault::late(
            LayerFaultKind::Malformed,
            "/x/keeper.toml",
            "from phase one",
        ));
        install(layers);
        // A second install is ignored, not a panic.
        install(AppLayers::default());

        assert_eq!(
            setting_override("zz.install_probe.alpha").map(|o| o.value),
            Some("30".to_owned())
        );
        assert_eq!(setting_override("zz.install_probe.absent"), None);
        assert_eq!(main_folder(), Some(PathBuf::from("/zz-install-probe")));
        assert_eq!(
            overrides()
                .into_iter()
                .map(|(key, source)| (key, source.tier))
                .collect::<Vec<_>>(),
            vec![
                ("zz.install_probe.alpha".to_owned(), LayerTier::UserGlobal),
                ("zz.install_probe.beta".to_owned(), LayerTier::MainMachine),
            ]
        );
        push_fault(LayerFault::late(
            LayerFaultKind::MainFolderNotAProfile,
            "/x/keeper.toml",
            "from phase two",
        ));
        let messages: Vec<_> = faults().into_iter().map(|f| f.message).collect();
        assert_eq!(messages, vec!["from phase one", "from phase two"]);
    }

    /// The thread-local overlay the rest of the suite uses is the same lookup,
    /// and it is gone the moment the guard drops.
    #[test]
    fn the_test_overlay_is_scoped_to_its_guard() {
        {
            let _guard =
                install_for_test(layers_from(&[("debug.mode", "1", LayerTier::UserGlobal)]));
            assert_eq!(
                setting_override("debug.mode").map(|o| o.value),
                Some("1".to_owned())
            );
        }
        assert_eq!(setting_override("debug.mode"), None);
    }

    // -- the account tiers (Epic 82) ---------------------------------------

    /// Clears this thread's account slot when dropped, so a failing assertion
    /// cannot leak an installed account into the next test on the thread.
    struct AccountGuard;

    impl Drop for AccountGuard {
        fn drop(&mut self) {
            install_account_layers(None);
            set_account_faults(Vec::new());
        }
    }

    /// A home with a main folder, and an account directory beside it.
    struct AccountFixture {
        home: PathBuf,
        main: PathBuf,
        account: PathBuf,
    }

    impl AccountFixture {
        fn new() -> Self {
            let home = temp_dir();
            let main = home.join("tgdrive");
            std::fs::create_dir_all(&main).expect("create the main folder");
            let account = home.join("clone").join("tgorka");
            std::fs::create_dir_all(&account).expect("create the account dir");
            write(
                &keeper_dir(&home).join("keeper.toml"),
                &format!("mainSyncFolder = {:?}\n", main.display().to_string()),
            );
            Self {
                home,
                main,
                account,
            }
        }

        /// The file `tier` reads, on host `testbox` / device `laptop`.
        fn path(&self, tier: LayerTier) -> PathBuf {
            match tier {
                LayerTier::UserGlobal => keeper_dir(&self.home).join("keeper.toml"),
                LayerTier::UserGlobalMachine => keeper_dir(&self.home).join("keeper.testbox.toml"),
                LayerTier::AccountShared => self.account.join("keeper.toml"),
                LayerTier::AccountDevice => self.account.join("keeper.laptop.toml"),
                LayerTier::MainShared => keeper_dir(&self.main).join("keeper.toml"),
                LayerTier::MainMachine => keeper_dir(&self.main).join("keeper.testbox.toml"),
                other => panic!("{other:?} is not an app or account tier"),
            }
        }

        fn set(&self, tier: LayerTier, settings: &str) {
            let mut text = String::new();
            if tier == LayerTier::UserGlobal {
                text.push_str(&format!(
                    "mainSyncFolder = {:?}\n",
                    self.main.display().to_string()
                ));
            }
            text.push_str("[settings]\n");
            text.push_str(settings);
            write(&self.path(tier), &text);
        }

        fn source(&self) -> AccountLayerSource {
            AccountLayerSource {
                dir: self.account.clone(),
                device: "laptop".to_owned(),
                account_name: "Acme".to_owned(),
                repo_host: "git.acme.dev".to_owned(),
            }
        }

        /// Install the frozen stack and the account tiers the way the shell does.
        fn install(&self) -> (TestLayerGuard, AccountGuard) {
            let frozen = install_for_test(load_app_layers(&self.home, "testbox"));
            install_account_layers(Some(load_account_layers(&self.source())));
            (frozen, AccountGuard)
        }
    }

    impl Drop for AccountFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    fn resolved(key: &str) -> Option<(String, LayerTier)> {
        setting_override(key).map(|o| (o.value, o.source.tier))
    }

    /// [`faults`] without the process-global late faults, which
    /// `install_then_read_resolves_through_the_process_global` pushes from
    /// another thread. Late faults only ever grow, so a snapshot taken after the
    /// read covers everything that read could have seen.
    fn own_faults() -> Vec<LayerFault> {
        let all = faults();
        let late = late_faults().clone();
        all.into_iter().filter(|f| !late.contains(f)).collect()
    }

    /// The owner's precedence, pair by pair across the six app and account
    /// tiers: account beats `~/.keeper` (both files), the device file beats the
    /// account's shared file, and the main folder beats the account. Each pair is
    /// checked through `setting_override` and through `overrides()`, because the
    /// two merge the live tiers separately and the badge must agree with the value.
    #[test]
    fn every_adjacent_pair_across_user_account_and_main_resolves_to_the_later_one() {
        let tiers = [
            LayerTier::UserGlobal,
            LayerTier::UserGlobalMachine,
            LayerTier::AccountShared,
            LayerTier::AccountDevice,
            LayerTier::MainShared,
            LayerTier::MainMachine,
        ];
        assert_eq!(&LayerTier::ORDER[..6], &tiers);
        for pair in tiers.windows(2) {
            let (earlier, later) = (pair[0], pair[1]);
            let fixture = AccountFixture::new();
            fixture.set(earlier, "\"recording.segment_mb\" = 240\n");
            fixture.set(later, "\"recording.segment_mb\" = 480\n");
            let _installed = fixture.install();
            assert_eq!(
                resolved("recording.segment_mb"),
                Some(("480".to_owned(), later)),
                "{later:?} must beat {earlier:?}"
            );
            assert_eq!(
                overrides()
                    .into_iter()
                    .map(|(key, source)| (key, source.tier))
                    .collect::<Vec<_>>(),
                vec![("recording.segment_mb".to_owned(), later)],
                "overrides() must name the same winner as setting_override for {earlier:?} vs {later:?}"
            );
        }
    }

    /// Non-adjacent and per-key: the main folder's shared file beats the
    /// account's device file, `~/.keeper`'s machine file loses to the account's
    /// shared file, and a key only `~/.keeper` sets is not hidden by an account
    /// that sets other keys.
    #[test]
    fn account_tiers_merge_per_key_into_the_frozen_stack() {
        let fixture = AccountFixture::new();
        fixture.set(
            LayerTier::UserGlobal,
            "\"undo_send.window\" = 5\n\"recording.codec\" = \"h264\"\n",
        );
        fixture.set(LayerTier::UserGlobalMachine, "\"recording.fps\" = 10\n");
        fixture.set(
            LayerTier::AccountShared,
            "\"recording.fps\" = 15\n\"recording.codec\" = \"hevc\"\n",
        );
        fixture.set(LayerTier::AccountDevice, "\"recording.segment_mb\" = 240\n");
        fixture.set(LayerTier::MainShared, "\"recording.segment_mb\" = 480\n");
        let _installed = fixture.install();
        assert!(own_faults().is_empty(), "faults: {:?}", own_faults());

        assert_eq!(
            resolved("recording.fps"),
            Some(("15".to_owned(), LayerTier::AccountShared))
        );
        assert_eq!(
            resolved("recording.codec"),
            Some(("hevc".to_owned(), LayerTier::AccountShared))
        );
        assert_eq!(
            resolved("undo_send.window"),
            Some(("5".to_owned(), LayerTier::UserGlobal))
        );
        assert_eq!(
            resolved("recording.segment_mb"),
            Some(("480".to_owned(), LayerTier::MainShared))
        );
        assert_eq!(
            overrides()
                .into_iter()
                .map(|(key, source)| (key, source.tier))
                .collect::<Vec<_>>(),
            vec![
                ("recording.codec".to_owned(), LayerTier::AccountShared),
                ("recording.fps".to_owned(), LayerTier::AccountShared),
                ("recording.segment_mb".to_owned(), LayerTier::MainShared),
                ("undo_send.window".to_owned(), LayerTier::UserGlobal),
            ]
        );
    }

    /// An account's value names the account and its file, and belongs to no
    /// folder — "Set by a file" has to say whose repository to edit.
    #[test]
    fn an_account_override_names_its_account_and_its_file() {
        let fixture = AccountFixture::new();
        fixture.set(LayerTier::AccountDevice, "\"recording.fps\" = 30\n");
        let _installed = fixture.install();
        let source = setting_override("recording.fps")
            .expect("the account sets it")
            .source;
        assert_eq!(source.account.as_deref(), Some("Acme"));
        assert_eq!(source.path, fixture.account.join("keeper.laptop.toml"));
        assert_eq!(source.folder, None);
    }

    /// AD-101 holds in the repository too: an account file cannot elect the main
    /// folder, cannot carry `[folder]`, and cannot put a machine-local key in the
    /// file every device reads. Each refusal is a fault naming the account
    /// file, and the rest of the file still applies.
    #[test]
    fn main_sync_folder_folder_tables_and_shared_machine_keys_are_refused_in_account_files() {
        let fixture = AccountFixture::new();
        write(
            &fixture.path(LayerTier::AccountShared),
            "mainSyncFolder = \"/elsewhere\"\n\
             [settings]\n\
             \"recording.fps\" = 30\n\
             \"sync.git_path\" = \"/opt/homebrew/bin/git\"\n\
             [folder]\n\
             recordingsSubfolder = \"x\"\n",
        );
        write(
            &fixture.path(LayerTier::AccountDevice),
            "main_sync_folder = \"/elsewhere\"\n\
             [settings]\n\
             \"sync.git_path\" = \"/usr/bin/git\"\n",
        );
        let layers = load_account_layers(&fixture.source());
        let mut kinds: Vec<_> = layers
            .faults
            .iter()
            .map(|f| (f.kind, f.tier, f.key.clone()))
            .collect();
        kinds.sort_by_key(|(_, _, key)| key.clone());
        assert_eq!(
            kinds,
            vec![
                (
                    LayerFaultKind::UnknownTable,
                    Some(LayerTier::AccountShared),
                    Some("folder".to_owned())
                ),
                (
                    LayerFaultKind::MainFolderInFolderLayer,
                    Some(LayerTier::AccountShared),
                    Some("mainSyncFolder".to_owned())
                ),
                (
                    LayerFaultKind::MainFolderInFolderLayer,
                    Some(LayerTier::AccountDevice),
                    Some("main_sync_folder".to_owned())
                ),
                (
                    LayerFaultKind::KeyRefused,
                    Some(LayerTier::AccountShared),
                    Some("sync.git_path".to_owned())
                ),
            ]
        );
        // The good lines survived, and the machine-local key landed from the
        // device file, which is the one place it may live.
        assert_eq!(
            layers
                .overrides
                .get("recording.fps")
                .map(|o| o.value.as_str()),
            Some("30")
        );
        assert_eq!(
            layers
                .overrides
                .get("sync.git_path")
                .map(|o| (o.value.as_str(), o.source.tier)),
            Some(("/usr/bin/git", LayerTier::AccountDevice))
        );

        // Installed, the faults join the settings pane's list and the main
        // folder is still the one ~/.keeper named.
        let _installed = fixture.install();
        assert_eq!(main_folder(), Some(fixture.main.clone()));
        let listed: Vec<_> = own_faults().into_iter().map(|f| f.path).collect();
        assert_eq!(listed.len(), 4, "{listed:?}");
        assert!(listed.iter().all(|path| path.starts_with(&fixture.account)));
    }

    /// A layer file the repository holds as a symlink is never followed: its
    /// target (here outside the clone) applies nothing, and the fault names it.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_account_layer_is_a_fault_and_never_followed() {
        let fixture = AccountFixture::new();
        let outside = fixture.home.join("outside.toml");
        write(&outside, "[settings]\n\"recording.fps\" = 60\n");
        std::os::unix::fs::symlink(&outside, fixture.path(LayerTier::AccountShared)).expect("link");
        fixture.set(LayerTier::AccountDevice, "\"recording.segment_mb\" = 240\n");

        let layers = load_account_layers(&fixture.source());

        assert_eq!(layers.overrides.get("recording.fps"), None);
        assert_eq!(
            layers
                .overrides
                .get("recording.segment_mb")
                .map(|o| o.value.as_str()),
            Some("240"),
            "the regular device file still applies"
        );
        assert_eq!(
            layers
                .faults
                .iter()
                .map(|f| (f.kind, f.tier, f.path.clone()))
                .collect::<Vec<_>>(),
            vec![(
                LayerFaultKind::Unreadable,
                Some(LayerTier::AccountShared),
                fixture.path(LayerTier::AccountShared)
            )]
        );
    }

    /// The tiers are live: a fetch that changed the files swaps the new values
    /// in, and clearing them (sign-out) hands every key back to `~/.keeper` and
    /// drops the account's faults with them.
    #[test]
    fn swapping_and_clearing_the_account_layers_takes_effect_on_the_next_read() {
        let fixture = AccountFixture::new();
        fixture.set(LayerTier::UserGlobal, "\"recording.fps\" = 10\n");
        fixture.set(LayerTier::AccountShared, "\"recording.fps\" = 30\n");
        let (_frozen, _account) = fixture.install();
        assert_eq!(
            resolved("recording.fps"),
            Some(("30".to_owned(), LayerTier::AccountShared))
        );

        fixture.set(
            LayerTier::AccountShared,
            "\"recording.fps\" = 60\n\"recording.codec\" = \"banana\"\n",
        );
        install_account_layers(Some(load_account_layers(&fixture.source())));
        assert_eq!(
            resolved("recording.fps"),
            Some(("60".to_owned(), LayerTier::AccountShared))
        );
        assert_eq!(
            own_faults().iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![LayerFaultKind::ValueShape]
        );

        set_account_faults(vec![LayerFault::late(
            LayerFaultKind::Malformed,
            "/h/.keeper/account.toml",
            "the descriptor is broken",
        )]);
        assert_eq!(own_faults().len(), 2);

        install_account_layers(None);
        assert_eq!(
            resolved("recording.fps"),
            Some(("10".to_owned(), LayerTier::UserGlobal))
        );
        assert_eq!(
            overrides()
                .into_iter()
                .map(|(key, source)| (key, source.tier))
                .collect::<Vec<_>>(),
            vec![("recording.fps".to_owned(), LayerTier::UserGlobal)]
        );
        // The descriptor fault is the shell's to clear, not the layers'.
        assert_eq!(
            own_faults()
                .into_iter()
                .map(|f| f.message)
                .collect::<Vec<_>>(),
            vec!["the descriptor is broken"]
        );
        set_account_faults(Vec::new());
        assert!(own_faults().is_empty());
    }

    /// No account, or an account installed and then cleared, answers exactly
    /// what the frozen stack alone answers — values, sources and faults.
    #[test]
    fn no_account_answers_exactly_what_the_frozen_stack_answers() {
        let fixture = AccountFixture::new();
        fixture.set(LayerTier::UserGlobal, "\"recording.fps\" = 10\n");
        fixture.set(
            LayerTier::MainShared,
            "\"recording.codec\" = \"hevc\"\n\"nope\" = 1\n",
        );
        let frozen = load_app_layers(&fixture.home, "testbox");
        let _layers = install_for_test(frozen.clone());
        let snapshot = || {
            (
                setting_override("recording.fps"),
                setting_override("recording.codec"),
                overrides(),
                own_faults(),
            )
        };
        let before = snapshot();
        assert_eq!(before.0.as_ref(), frozen.overrides.get("recording.fps"));
        assert_eq!(before.3, frozen.faults);

        fixture.set(LayerTier::AccountShared, "\"recording.fps\" = 30\n");
        install_account_layers(Some(load_account_layers(&fixture.source())));
        let _account = AccountGuard;
        assert_ne!(snapshot(), before);
        install_account_layers(None);
        assert_eq!(snapshot(), before);
    }
}
