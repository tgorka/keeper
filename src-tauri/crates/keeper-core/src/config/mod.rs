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
//! <main>/.keeper/keeper.toml            the main sync folder, every machine
//! <main>/.keeper/keeper.<host>.toml     the main sync folder, THIS machine
//! <folder>/.keeper/keeper.toml          that folder only
//! <folder>/.keeper/keeper.<host>.toml   that folder, this machine
//! ```
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
//! Three of the six files are keyed on sync-folder paths, which live in
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
use std::sync::{Mutex, OnceLock};

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
/// **Declaration order is precedence order** — later wins. `resolve` relies on
/// nothing but iterating the tiers in this order, so reordering these variants
/// reorders the stack.
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
    /// `<main>/.keeper/keeper.toml` — the designated main sync folder, shared.
    MainShared,
    /// `<main>/.keeper/keeper.<host>.toml` — the main sync folder, this machine.
    MainMachine,
    /// `<folder>/.keeper/keeper.toml` — one non-main folder, shared.
    FolderShared,
    /// `<folder>/.keeper/keeper.<host>.toml` — one non-main folder, this machine.
    FolderMachine,
}

impl LayerTier {
    /// Every tier, in precedence order (later wins).
    pub const ORDER: [LayerTier; 6] = [
        LayerTier::UserGlobal,
        LayerTier::UserGlobalMachine,
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
            LayerTier::UserGlobalMachine | LayerTier::MainMachine | LayerTier::FolderMachine
        )
    }

    /// Whether a `[settings]` table is honoured at this tier.
    ///
    /// A non-main folder may only set keys that are *about itself* (`[folder]`).
    /// That is not a courtesy: it is what stops two folders fighting over
    /// `hotkey.global`, where the winner would be whichever the supervisor
    /// happened to open last.
    pub fn may_set_settings(self) -> bool {
        !matches!(self, LayerTier::FolderShared | LayerTier::FolderMachine)
    }

    /// Whether `mainSyncFolder` is honoured at this tier.
    ///
    /// Only the user-global files. A folder naming the main folder is either a
    /// no-op or a loop, and it is the fact that has to be readable *before* any
    /// folder is known (AD-101).
    pub fn may_set_main_folder(self) -> bool {
        matches!(self, LayerTier::UserGlobal | LayerTier::UserGlobalMachine)
    }

    /// Whether a `[folder]` table means anything at this tier — i.e. whether the
    /// file lives inside a sync folder at all.
    pub fn has_folder(self) -> bool {
        !matches!(self, LayerTier::UserGlobal | LayerTier::UserGlobalMachine)
    }

    /// A stable human name for logs and the settings pane.
    pub fn label(self) -> &'static str {
        match self {
            LayerTier::UserGlobal => "user",
            LayerTier::UserGlobalMachine => "user, this machine",
            LayerTier::MainShared => "main folder",
            LayerTier::MainMachine => "main folder, this machine",
            LayerTier::FolderShared => "folder",
            LayerTier::FolderMachine => "folder, this machine",
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
                    file.faults.push(fault(
                        LayerFaultKind::MainFolderInFolderLayer,
                        Some(name),
                        None,
                        format!(
                            "{name} is only honoured in ~/.keeper/{FILE_STEM}.toml and \
                             ~/.keeper/{FILE_STEM}.<host>.toml; a sync folder cannot elect \
                             itself, so this line is ignored"
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
                    file.faults.push(fault(
                        LayerFaultKind::UnknownTable,
                        Some(name),
                        None,
                        format!(
                            "[folder] names no folder in ~/.keeper/; folder settings belong in \
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
/// it runs before anything reads: after phase one the resolved set never
/// changes, because the only later layers are per-folder and a folder may not
/// set a settings key at all. So a lock would guard nothing, and it is not free
/// — an `RwLock` read is a read-modify-write on a shared cacheline, and
/// [`setting_override`] is called by every one of the ~40 typed getters on the
/// startup path, several of them in loops. `OnceLock::get` is one acquire load.
///
/// The one thing that genuinely mutates after install is the fault list
/// (`push_fault`, for the shell's phase two), so that — and only that — gets a
/// `Mutex`, off the hot path.
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
    with_installed(|layers| {
        layers
            .overrides
            .iter()
            .map(|(key, over)| (key.clone(), over.source.clone()))
            .collect()
    })
    .unwrap_or_default()
}

/// The designated main sync folder, for the shell's phase two.
pub fn main_folder() -> Option<PathBuf> {
    with_installed(|layers| layers.main_folder.clone()).flatten()
}

/// Everything wrong with the layer files: phase one's faults, then any the shell
/// added afterwards.
pub fn faults() -> Vec<LayerFault> {
    let mut all = with_installed(|layers| layers.faults.clone()).unwrap_or_default();
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
}
