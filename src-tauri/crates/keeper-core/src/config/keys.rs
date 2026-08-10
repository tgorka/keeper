//! Every `settings` key, as data (Story 46.9, AD-98).
//!
//! The layer engine next door resolves a key to a value. This module answers the
//! question that comes first and that nothing answered before: **which keys are
//! there, and which of them may a file set?**
//!
//! A layer engine that resolves *some* keys is a promise half-kept, and the half
//! that is missing is invisible — you discover it when the key you wanted is the
//! one that does not work. So the keys stop being ~40 private `const` strings
//! scattered across four modules and become one table, and a test scans the
//! crate sources for every `get_setting` / `set_setting` call site and fails, by
//! name, on any key this table does not classify. Adding a key without deciding
//! what it is now costs a red test instead of a silent gap.
//!
//! # The three scopes
//!
//! [`Scope::UserGlobal`] — a preference about the person. The same answer on
//! every machine and in every folder, so any layer file may set it.
//!
//! [`Scope::MachineLocal`] — a fact about *this* computer: an absolute path, an
//! OS-global accelerator, a row id in this machine's `sync.db`, a Keychain item.
//! Legitimate in `keeper.<host>.toml`, which is exactly what that file is for,
//! and **refused with a reason** in a shared one. Refused, not omitted: a
//! `sync.git_path` silently ignored on the second machine is the same bad hour
//! as a `sync.git_path` that silently took effect.
//!
//! [`Scope::SessionState`] — not a preference at all. A capture window's draft
//! pointer, a remembered window position, a one-time disclosure latch, the set
//! of recovered sessions somebody already dismissed. keeper writes these and
//! reads them back; a file entry would either be overwritten within the second
//! or would freeze a latch the person never saw. They are in this table rather
//! than left out of it, because "state, deliberately not settable" and "somebody
//! forgot to classify it" must not look the same to the coverage test — and
//! leaving them out is precisely how they would come to look the same.
//!
//! # There is no folder scope here, and that is the point
//!
//! The layer model has folder tiers, but **no settings-table key is about one
//! folder**. Everything a folder decides about itself — its notes vault, its
//! recordings subfolder, its branch, its quiet time — lives in that folder's
//! [`SyncProfile`](../../../keeper-sync/src/profile.rs) fields, which is what the
//! `[folder]` table in a `.keeper/keeper.toml` carries. That is *why* a
//! `[settings]` table outside the main folder is a fault rather than a merge: it
//! could only ever be a folder reaching for a key that is not about it.
//!
//! # Where the reader looks
//!
//! `docs/settings-keys.md` is generated from [`KEYS`] by [`render_docs`] and
//! pinned by a test, because a stale generated table is worse than none.

use std::fmt;

/// Who a key's value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// A preference about the person: the same on every machine, every folder.
    UserGlobal,
    /// A fact about this computer — an absolute path, an OS-global accelerator,
    /// a local `sync.db` row id, a Keychain-bound posture.
    MachineLocal,
    /// State keeper owns and rewrites. Not a preference; never file-settable.
    SessionState,
}

impl Scope {
    /// The column value in the generated docs table.
    pub fn label(self) -> &'static str {
        match self {
            Scope::UserGlobal => "user-global",
            Scope::MachineLocal => "machine-local",
            Scope::SessionState => "session state",
        }
    }
}

/// Which layer files, if any, may set a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settable {
    /// Any layer file whose tier is allowed to carry `[settings]` at all.
    AnyLayer,
    /// Only a per-machine `keeper.<host>.toml`. The string says why, in a
    /// sentence that is shown to the person who put it in the shared file.
    MachineFileOnly(&'static str),
    /// No file, ever. The string says why.
    Never(&'static str),
}

/// The shape of a key's value, as it is stored in the `settings` table.
///
/// Carried as data rather than prose so the docs table cannot drift from the
/// getters, and so [`Shape::coerce`] is the single place a TOML value becomes
/// the string the table holds. Every getter in `registry.rs` still normalises
/// on read — this is the earlier, louder gate, not a replacement for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The registry's `"1"` / `"0"` boolean convention.
    Flag01,
    /// The older `"on"` / `"off"` spelling (two legacy un-namespaced keys).
    FlagOnOff,
    /// The `"true"` / `"false"` spelling (one legacy un-namespaced key).
    FlagTrueFalse,
    /// A decimal integer, clamped by its getter to this inclusive range.
    Int { min: i64, max: i64 },
    /// One of a fixed set of stored spellings.
    Choice(&'static [&'static str]),
    /// A free-form string; empty means "cleared".
    Text,
    /// An absolute filesystem path, stored verbatim and validated at use.
    AbsolutePath,
    /// An OS accelerator string, opaque to `keeper-core`; empty means unset.
    Accelerator,
    /// A JSON document keeper wrote.
    Json,
}

impl Shape {
    /// A human description for the generated docs table.
    pub fn describe(self) -> String {
        match self {
            Shape::Flag01 => "boolean (`1`/`0`)".to_owned(),
            Shape::FlagOnOff => "boolean (`on`/`off`)".to_owned(),
            Shape::FlagTrueFalse => "boolean (`true`/`false`)".to_owned(),
            Shape::Int { min, max } => format!("integer {min}..={max}"),
            Shape::Choice(options) => {
                let rendered: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
                format!("one of {}", rendered.join(", "))
            }
            Shape::Text => "text".to_owned(),
            Shape::AbsolutePath => "absolute path".to_owned(),
            Shape::Accelerator => "accelerator".to_owned(),
            Shape::Json => "JSON".to_owned(),
        }
    }

    /// Turn a TOML value into the string the `settings` table stores.
    ///
    /// Booleans are written as booleans and integers as integers — nobody should
    /// have to type `"1"` into a config file to turn something on — and this is
    /// the one place that translation happens, so the file spelling and the
    /// stored spelling cannot come to disagree. The stored spelling is also
    /// accepted verbatim, because a person copying a value out of the table they
    /// were shown should not be told it is wrong.
    ///
    /// An out-of-range integer is an error rather than a clamp: the getter
    /// clamps a row that rotted, but a number a person typed into a file they
    /// can see is worth a sentence back.
    pub fn coerce(self, key: &str, value: &toml::Value) -> Result<String, ShapeError> {
        let refuse = |expected: &str| {
            Err(ShapeError {
                message: format!(
                    "{key} expects {expected}, and the file has {}",
                    describe_toml(value)
                ),
            })
        };
        match self {
            Shape::Flag01 => match value {
                toml::Value::Boolean(on) => Ok(if *on { "1" } else { "0" }.to_owned()),
                toml::Value::String(raw) if raw == "1" || raw == "0" => Ok(raw.clone()),
                _ => refuse("a boolean (`true` or `false`)"),
            },
            Shape::FlagOnOff => match value {
                toml::Value::Boolean(on) => Ok(if *on { "on" } else { "off" }.to_owned()),
                toml::Value::String(raw) if raw == "on" || raw == "off" => Ok(raw.clone()),
                _ => refuse("a boolean (`true` or `false`)"),
            },
            Shape::FlagTrueFalse => match value {
                toml::Value::Boolean(on) => Ok(if *on { "true" } else { "false" }.to_owned()),
                toml::Value::String(raw) if raw == "true" || raw == "false" => Ok(raw.clone()),
                _ => refuse("a boolean (`true` or `false`)"),
            },
            Shape::Int { min, max } => {
                let found = match value {
                    toml::Value::Integer(n) => *n,
                    toml::Value::String(raw) => match raw.parse::<i64>() {
                        Ok(n) => n,
                        Err(_) => return refuse(&format!("a whole number from {min} to {max}")),
                    },
                    _ => return refuse(&format!("a whole number from {min} to {max}")),
                };
                if found < min || found > max {
                    return Err(ShapeError {
                        message: format!("{key} accepts {min} to {max}, and the file has {found}"),
                    });
                }
                Ok(found.to_string())
            }
            Shape::Choice(options) => {
                let candidate = match value {
                    toml::Value::String(raw) => raw.clone(),
                    toml::Value::Integer(n) => n.to_string(),
                    _ => String::new(),
                };
                if options.contains(&candidate.as_str()) {
                    Ok(candidate)
                } else {
                    let rendered: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
                    refuse(&format!("one of {}", rendered.join(", ")))
                }
            }
            Shape::Text | Shape::AbsolutePath | Shape::Accelerator | Shape::Json => match value {
                toml::Value::String(raw) => Ok(raw.clone()),
                _ => refuse("a string"),
            },
        }
    }
}

/// What a TOML value is, for the error sentence.
fn describe_toml(value: &toml::Value) -> String {
    match value {
        toml::Value::String(raw) => format!("the string \"{raw}\""),
        toml::Value::Integer(n) => format!("the number {n}"),
        toml::Value::Float(n) => format!("the number {n}"),
        toml::Value::Boolean(b) => format!("the boolean {b}"),
        toml::Value::Datetime(d) => format!("the date {d}"),
        toml::Value::Array(_) => "an array".to_owned(),
        toml::Value::Table(_) => "a table".to_owned(),
    }
}

/// A value in a layer file that does not fit its key's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeError {
    message: String,
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ShapeError {}

/// Why a layer file may not set a key.
///
/// Every variant names the key and says why in one sentence, because this text
/// is what the layer loader puts in its fault report and what the settings pane
/// shows the person who wrote the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    /// A fact about one machine, found in a file both machines read.
    MachineLocal {
        /// The refused key.
        key: String,
        /// Why it cannot be shared.
        why: &'static str,
    },
    /// State keeper owns; no file may set it at any tier.
    NotAPreference {
        /// The refused key.
        key: String,
        /// Why it is not a preference.
        why: &'static str,
    },
    /// Not a settings key this build knows — most often a typo.
    Unknown {
        /// The unrecognised key, verbatim.
        key: String,
    },
}

impl RefusalReason {
    /// The key that was refused.
    pub fn key(&self) -> &str {
        match self {
            RefusalReason::MachineLocal { key, .. }
            | RefusalReason::NotAPreference { key, .. }
            | RefusalReason::Unknown { key } => key,
        }
    }
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefusalReason::MachineLocal { key, why } => write!(
                f,
                "{key} is machine-local ({why}); set it in keeper.<host>.toml, not a shared file"
            ),
            RefusalReason::NotAPreference { key, why } => write!(
                f,
                "{key} is not a setting ({why}); keeper writes it and reads it back, so a file entry would not survive"
            ),
            RefusalReason::Unknown { key } => write!(
                f,
                "{key} is not a keeper setting; check the spelling against docs/settings-keys.md"
            ),
        }
    }
}

impl std::error::Error for RefusalReason {}

/// One classified `settings` key, or one key *family*.
///
/// A family's [`key`](KeySpec::key) is the prefix, ending in a dot, and matches
/// `notes.read.<note_id>` and its thousands of siblings. Without this shape the
/// coverage test would report one unclassified key per note in the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    /// The exact key, or — when [`family`](KeySpec::family) — its dotted prefix.
    pub key: &'static str,
    /// Whether [`key`](KeySpec::key) is a prefix matching many real keys.
    pub family: bool,
    /// Who the value belongs to.
    pub scope: Scope,
    /// Which files may set it.
    pub settable: Settable,
    /// The shape of the stored value.
    pub shape: Shape,
    /// The stored string a reader falls back to when the row is absent, or `""`
    /// when absent is its own state ("no choice made").
    pub default: &'static str,
    /// One line, for the docs table.
    pub summary: &'static str,
    /// A TOML value expression for the docs table, or `""` when no file may set
    /// the key and an example would be an invitation to write one.
    pub example: &'static str,
}

impl KeySpec {
    /// Whether this spec covers `key`.
    pub fn matches(&self, key: &str) -> bool {
        if self.family {
            key.len() > self.key.len() && key.starts_with(self.key)
        } else {
            key == self.key
        }
    }

    /// How the key is written in prose and in the docs table.
    pub fn display_key(&self) -> String {
        if self.family {
            format!("{}<…>", self.key)
        } else {
            self.key.to_owned()
        }
    }
}

/// Every `settings` key this build reads or writes.
///
/// Ordered by namespace so the generated docs read like the settings pane, not
/// like the order somebody happened to add things. The coverage test asserts
/// this list and the crate sources agree in **both** directions: no key in the
/// sources is missing here, and no key here has stopped being used.
pub const KEYS: &[KeySpec] = &[
    // ---- legacy, un-namespaced ------------------------------------------
    KeySpec {
        key: "sdk_encryption",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::Never(
            "the posture is keyed to a per-account passphrase in this machine's Keychain, so \
             flipping it is a re-key of the local store, not a toggle",
        ),
        shape: Shape::FlagOnOff,
        default: "",
        summary: "At-rest encryption posture for the local matrix-sdk store; absent means unchosen, which is what gates the first-run question.",
        example: "",
    },
    KeySpec {
        key: "honor_remote_deletions",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::FlagOnOff,
        default: "off",
        summary: "Whether a remote redaction also removes the archived copy locally.",
        example: "true",
    },
    KeySpec {
        key: "favorites_collapsed",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::FlagTrueFalse,
        default: "false",
        summary: "Whether the Favorites section of the room list starts collapsed.",
        example: "true",
    },
    // ---- debug -----------------------------------------------------------
    KeySpec {
        key: "debug.mode",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "0",
        summary: "On-disk event and error logging. Read before anything else at boot, so a file can turn it on for the boot that goes wrong.",
        example: "true",
    },
    // ---- hotkeys ---------------------------------------------------------
    KeySpec {
        key: "hotkey.global",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(HOTKEY_WHY),
        shape: Shape::Accelerator,
        default: "Control+Alt+Space",
        summary: "The OS-global summon accelerator.",
        example: "\"Control+Alt+Space\"",
    },
    KeySpec {
        key: "hotkey.recording",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(HOTKEY_WHY),
        shape: Shape::Accelerator,
        default: "",
        summary: "The OS-global Start/Stop Recording accelerator; empty means unset.",
        example: "\"Control+Shift+R\"",
    },
    KeySpec {
        key: "hotkey.capture",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(HOTKEY_WHY),
        shape: Shape::Accelerator,
        default: "",
        summary: "The OS-global Quick Capture accelerator; empty means unset.",
        example: "\"Control+Alt+N\"",
    },
    // ---- incognito -------------------------------------------------------
    KeySpec {
        key: "incognito.global",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "0",
        summary: "Suppress read receipts and typing notifications everywhere.",
        example: "true",
    },
    // ---- notes -----------------------------------------------------------
    KeySpec {
        key: "notes.active_vault",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(PROFILE_ID_WHY),
        shape: Shape::Text,
        default: "",
        summary: "Which notes vault the notes surface is showing, as a sync-profile id.",
        example: "\"01J8Z5R0Q9WQ4C3S0PNK7T2A1B\"",
    },
    KeySpec {
        key: "notes.capture_draft.",
        family: true,
        scope: Scope::SessionState,
        settable: Settable::Never(
            "it points at the note one live capture window is holding, and is cleared the \
             moment that thought is filed",
        ),
        shape: Shape::Json,
        default: "",
        summary: "Per capture window: the note it holds and the body creation gave it.",
        example: "",
    },
    KeySpec {
        key: "notes.capture_placement.",
        family: true,
        scope: Scope::SessionState,
        settable: Settable::Never(
            "it is where a person last dragged one capture window, rewritten on every dismissal",
        ),
        shape: Shape::Text,
        default: "",
        summary: "Per capture window: its remembered position and whether the position is locked.",
        example: "",
    },
    KeySpec {
        key: "notes.read.",
        family: true,
        scope: Scope::SessionState,
        settable: Settable::Never(
            "it is this device's record of which revision of a note it has already shown you, \
             and it must never travel — that is what makes an edit from the other machine unread",
        ),
        shape: Shape::Text,
        default: "",
        summary: "Per note: the revision this device has acknowledged.",
        example: "",
    },
    // ---- notifications ---------------------------------------------------
    KeySpec {
        key: "notify.previews_enabled",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "1",
        summary: "Whether native notifications include the message body.",
        example: "false",
    },
    KeySpec {
        key: "notify.dnd_global",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "0",
        summary: "Global Do-Not-Disturb: post nothing at all.",
        example: "true",
    },
    KeySpec {
        key: "notify.dock_badge_mode",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Choice(&["all", "mentions", "off"]),
        default: "all",
        summary: "What the dock badge counts.",
        example: "\"mentions\"",
    },
    // ---- recording -------------------------------------------------------
    KeySpec {
        key: "recording.destination_dir",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(
            "it is an absolute path, and /Volumes/merope/… does not exist on the other machine",
        ),
        shape: Shape::AbsolutePath,
        default: "",
        summary: "Where recordings are written; absent means the shell's platform default.",
        example: "\"/Users/tgorka/Movies/keeper\"",
    },
    KeySpec {
        key: "recording.destination_profile_id",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(PROFILE_ID_WHY),
        shape: Shape::Text,
        default: "",
        summary: "The sync profile that holds this machine's recordings; overrides the plain folder above.",
        example: "\"01J8Z5R0Q9WQ4C3S0PNK7T2A1B\"",
    },
    KeySpec {
        key: "recording.path_template",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Text,
        default: "",
        summary: "The path template for a session's folder; absent means the shipped default.",
        example: "\"{yyyy}/{MM}/{yyyy-MM-dd HH.mm} {title}\"",
    },
    KeySpec {
        key: "recording.fps",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Choice(&["10", "15", "30", "60"]),
        default: "30",
        summary: "Capture frame rate.",
        example: "30",
    },
    KeySpec {
        key: "recording.codec",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Choice(&["h264", "hevc"]),
        default: "h264",
        summary: "Video codec; `hevc` uses hardware encode on Apple Silicon.",
        example: "\"hevc\"",
    },
    KeySpec {
        key: "recording.scale_percent",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Choice(&["25", "50", "75", "100"]),
        default: "100",
        summary: "Capture scale as a percentage of the source resolution.",
        example: "50",
    },
    KeySpec {
        key: "recording.segment_mb",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Int {
            min: 100,
            max: 5000,
        },
        default: "500",
        summary: "Segment size in MB; a session is written as a chain of segments this large.",
        example: "500",
    },
    KeySpec {
        key: "recording.duration_cap_minutes",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Int { min: 1, max: 600 },
        default: "30",
        summary: "Fallback cap on a single segment's duration, in minutes.",
        example: "30",
    },
    KeySpec {
        key: "recording.echo_cancellation",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "0",
        summary: "Acoustic echo cancellation on the microphone track; costs a mono track and voice-band noise suppression.",
        example: "true",
    },
    // ---- sync ------------------------------------------------------------
    KeySpec {
        key: "sync.git_path",
        family: false,
        scope: Scope::MachineLocal,
        settable: Settable::MachineFileOnly(
            "it is an absolute path to a binary, and /opt/homebrew/bin/git is not where git is \
             on the other machine",
        ),
        shape: Shape::AbsolutePath,
        default: "",
        summary: "An explicit git binary for folder sync; absent means search PATH.",
        example: "\"/opt/homebrew/bin/git\"",
    },
    KeySpec {
        key: "sync.list_folded",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Int { min: 1, max: 50 },
        default: "10",
        summary: "Rows a folder card's lists show before the fold.",
        example: "10",
    },
    KeySpec {
        key: "sync.list_unfolded",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Int {
            min: 10,
            max: 1000,
        },
        default: "100",
        summary: "Rows a folder card's lists show once unfolded.",
        example: "100",
    },
    // ---- system ----------------------------------------------------------
    KeySpec {
        key: "system.menu_bar_presence",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Flag01,
        default: "0",
        summary: "Whether keeper keeps a menu-bar (tray) presence.",
        example: "true",
    },
    // ---- ui --------------------------------------------------------------
    KeySpec {
        key: "ui.ios_sync_disclosure_shown",
        family: false,
        scope: Scope::SessionState,
        settable: Settable::Never(
            "it is a one-time latch keeper sets after showing a disclosure, and pre-setting it \
             in a file would suppress a card the person never saw",
        ),
        shape: Shape::Flag01,
        default: "0",
        summary: "Whether the one-time iOS no-background-sync disclosure has been shown.",
        example: "",
    },
    KeySpec {
        key: "ui.recovered_sessions_acknowledged",
        family: false,
        scope: Scope::SessionState,
        settable: Settable::Never(
            "it is the set of recovered recording sessions somebody has already dismissed, \
             rewritten on every dismissal",
        ),
        shape: Shape::Json,
        default: "[]",
        summary: "Recovered recording sessions the person has acknowledged.",
        example: "",
    },
    // ---- undo send -------------------------------------------------------
    KeySpec {
        key: "undo_send.window",
        family: false,
        scope: Scope::UserGlobal,
        settable: Settable::AnyLayer,
        shape: Shape::Int { min: 0, max: 60 },
        default: "10",
        summary: "How long a sent message is held before it dispatches, in seconds.",
        example: "10",
    },
];

/// Shared reason for the three OS-global accelerators.
const HOTKEY_WHY: &str =
    "an OS-global accelerator is registered with this machine's window server, and two \
     machines cannot agree on one that is free on both";

/// Shared reason for the two keys that hold a sync-profile id.
const PROFILE_ID_WHY: &str =
    "it names a row in this machine's sync.db, and the same folder is a different profile id \
     on the other machine";

/// The spec covering `key`, exact match preferred over a family prefix.
pub fn spec(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter()
        .find(|spec| !spec.family && spec.matches(key))
        .or_else(|| KEYS.iter().find(|spec| spec.family && spec.matches(key)))
}

/// Whether `key` is a settings key this build knows.
pub fn is_known(key: &str) -> bool {
    spec(key).is_some()
}

/// Whether a layer file may set `key`.
///
/// `machine_scoped` is true for the per-machine `keeper.<host>.toml` files at
/// any tier and false for the shared ones. That one bit is the whole difference
/// for the six machine-local keys: the per-machine file is exactly where an
/// absolute path or an accelerator belongs, and a shared file is exactly where
/// it must be refused out loud.
///
/// An unknown key is refused rather than ignored, for the reason
/// `keeper-syncd`'s config gives for `deny_unknown_fields`: a typo in
/// `recordng.fps` leaves a file that loads, an app that starts, and a setting
/// that never took. The caller turns this into a named, non-fatal fault and
/// skips the key — never the file, and never the boot.
pub fn layer_may_set(key: &str, machine_scoped: bool) -> Result<(), RefusalReason> {
    let Some(spec) = spec(key) else {
        return Err(RefusalReason::Unknown {
            key: key.to_owned(),
        });
    };
    match spec.settable {
        Settable::AnyLayer => Ok(()),
        Settable::MachineFileOnly(why) => {
            if machine_scoped {
                Ok(())
            } else {
                Err(RefusalReason::MachineLocal {
                    key: key.to_owned(),
                    why,
                })
            }
        }
        Settable::Never(why) => Err(RefusalReason::NotAPreference {
            key: key.to_owned(),
            why,
        }),
    }
}

/// The generated body of `docs/settings-keys.md`.
///
/// Generated rather than hand-kept because a hand-kept table of forty keys is a
/// table that is wrong by the third story after the one that wrote it; pinned by
/// a test because a generated table nobody regenerates is worse than none.
pub fn render_docs() -> String {
    let mut out = String::new();
    out.push_str(GENERATED_HEADER);

    out.push_str("\n## Keys a file may set\n\n");
    out.push_str("| Key | Scope | Shape | Default | Example |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for spec in KEYS
        .iter()
        .filter(|spec| matches!(spec.settable, Settable::AnyLayer))
    {
        push_row(&mut out, spec);
    }

    out.push_str("\n## Keys only `keeper.<host>.toml` may set\n\n");
    out.push_str(
        "These are facts about one computer. A shared file that sets one of them is a named \
         fault, not a silent skip.\n\n",
    );
    out.push_str("| Key | Scope | Shape | Default | Example |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for spec in KEYS
        .iter()
        .filter(|spec| matches!(spec.settable, Settable::MachineFileOnly(_)))
    {
        push_row(&mut out, spec);
    }
    out.push_str("\nWhy each one is refused from a shared file:\n\n");
    for spec in KEYS.iter() {
        if let Settable::MachineFileOnly(why) = spec.settable {
            out.push_str(&format!("- `{}` — {}\n", spec.display_key(), why));
        }
    }

    out.push_str("\n## Keys no file may set\n\n");
    out.push_str(
        "Not preferences. keeper writes these and reads them back, so a file entry would either \
         be overwritten within the second or would freeze a latch nobody saw. They are listed \
         here, rather than left out, so that \"deliberately not settable\" and \"nobody \
         classified it\" do not look the same.\n\n",
    );
    out.push_str("| Key | Shape | What it holds | Why no file sets it |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for spec in KEYS.iter() {
        if let Settable::Never(why) = spec.settable {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                spec.display_key(),
                spec.shape.describe(),
                spec.summary,
                why
            ));
        }
    }

    out.push_str(FOOTER);
    out
}

/// One row of the two settable tables.
fn push_row(out: &mut String, spec: &KeySpec) {
    let default = if spec.default.is_empty() {
        "*(absent)*".to_owned()
    } else {
        format!("`{}`", spec.default)
    };
    out.push_str(&format!(
        "| `{}` | {} | {} | {} | `\"{}\" = {}` |\n",
        spec.display_key(),
        spec.scope.label(),
        spec.shape.describe(),
        default,
        spec.key,
        spec.example
    ));
    out.push_str(&format!("| | | | | {} |\n", spec.summary));
}

/// The prose above the generated tables. Part of the pinned output, so editing
/// the checked-in file by hand fails the pin test — edit this instead.
const GENERATED_HEADER: &str = r#"# Settings keys

<!-- GENERATED by keeper_core::config::keys::render_docs. Do not edit by hand:
     `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`
     rewrites this file from the registry, and a test fails if the two differ. -->

Every value keeper keeps in its `settings` table, and which config file — if any
— may set it.

Files are read in this order, and a later one wins:

```
~/.keeper/keeper.toml                 you, on every machine, in every folder
~/.keeper/keeper.<host>.toml          you, on this machine
<main>/.keeper/keeper.toml            the main sync folder, on every machine
<main>/.keeper/keeper.<host>.toml     the main sync folder, on this machine
<folder>/.keeper/keeper.toml          that folder, on every machine
<folder>/.keeper/keeper.<host>.toml   that folder, on this machine
```

`<host>` is this machine's short hostname. A file looks like this:

```toml
mainSyncFolder = "/Volumes/merope/tgdrive"   # only in ~/.keeper/keeper.toml

[settings]
"recording.fps" = 30
"notify.dnd_global" = true

[folder]                                     # this folder's own sync settings
recordingsSubfolder = "40-media/recordings"
```

A `[settings]` table is accepted in `~/.keeper/` and in the **main** sync
folder. In any other folder it is a fault that names itself, because no key
below is about one folder: everything a folder decides about itself lives in
`[folder]`, which is that folder's sync profile.

A key set by a file keeps winning. It is not imported into the table once at
boot and then lost to the next toggle — the settings pane shows the control as
file-controlled instead, and says which file.
"#;

/// The prose below the generated tables.
const FOOTER: &str = r#"
## Booleans and numbers

Write them as booleans and numbers, not as the strings the table stores:
`"notify.dnd_global" = true`, not `= "1"`. The stored spelling is accepted too,
so a value copied out of the table above is never rejected for being right.

An integer outside its range is a named fault rather than a silent clamp. A row
already in the database is still clamped on read — a value that rotted degrades
to the documented default — but a number a person typed into a file they can see
is worth a sentence back.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Coverage: every settings key the crates actually use is classified here.
    ///
    /// This module is the story. `keeper-syncd`'s config derives its accepted-key
    /// set from the type at runtime and so cannot drift; that trick is not
    /// available here, because these keys are `const` strings in function bodies
    /// and two of them are built with `format!`. The honest substitute is to read
    /// the source — the pattern `src/test/command-registration.test.ts` and
    /// `src/test/file-scheme-registration.test.ts` already set in this repo, both
    /// of which exist because the alternative was a silent gap.
    mod coverage {
        use super::*;
        use std::collections::{BTreeMap, BTreeSet};
        use std::path::{Path, PathBuf};

        /// The crates whose `src/` may touch the `settings` table.
        ///
        /// `keeper-sync` and `keeper-syncd` are scanned even though they hold no
        /// call site today: the day one of them grows one is exactly the day this
        /// test needs to notice.
        const SCANNED_CRATES: &[&str] = &["keeper-core", "keeper", "keeper-sync", "keeper-syncd"];

        /// The one place a key is not knowable from the source, with its reason.
        ///
        /// `import_config_file` writes whatever keys `config.json` holds — that is
        /// what AD-98 replaces, and until it is gone it is a real dynamic site. A
        /// *second* entry here would need a second reason, and adding one without
        /// writing that reason fails this test by name.
        const DYNAMIC_SITES: &[(&str, &str)] =
            &[("keeper-core/src/registry.rs", "import_config_file")];

        /// A resolved settings key at one call site.
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
        enum Site {
            /// A key the source names exactly.
            Key(String),
            /// A key family, named by its dotted prefix.
            Family(String),
            /// A key only known at runtime; must be in [`DYNAMIC_SITES`].
            Dynamic(String),
        }

        fn crates_dir() -> PathBuf {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("keeper-core sits inside the crates directory")
                .to_path_buf()
        }

        /// Every `.rs` file under a directory, recursively.
        fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
            for entry in entries {
                let path = entry.expect("a readable directory entry").path();
                if path.is_dir() {
                    rust_files(&path, into);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    into.push(path);
                }
            }
        }

        /// The production half of a source file: everything before its test
        /// module.
        ///
        /// Split on the sole `#[cfg(test)]\nmod tests` opener, the same marker
        /// `send.rs`'s source scan uses. Asserted to occur at most once, because a
        /// second one would make this silently truncate the scanned slice — and a
        /// scanner that quietly stops scanning is the failure mode this whole
        /// module exists to prevent.
        fn production(path: &Path, source: &str) -> String {
            const MARKER: &str = "#[cfg(test)]\nmod tests";
            assert!(
                source.matches(MARKER).count() <= 1,
                "{} has more than one `{MARKER}` opener; the production/test split \
                 in this scanner would truncate at the first one and hide call sites \
                 after it",
                path.display()
            );
            source
                .split(MARKER)
                .next()
                .expect("split always yields a first part")
                .to_owned()
        }

        /// The argument list of the call whose `(` is at `open`, split at depth 1.
        fn call_args(source: &str, open: usize) -> Vec<String> {
            let bytes: Vec<char> = source[open + 1..].chars().collect();
            let mut args = Vec::new();
            let mut current = String::new();
            let mut depth = 1usize;
            let mut in_string = false;
            let mut escaped = false;
            for ch in bytes {
                if in_string {
                    current.push(ch);
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        in_string = false;
                    }
                    continue;
                }
                match ch {
                    '"' => {
                        in_string = true;
                        current.push(ch);
                    }
                    '(' | '[' | '{' => {
                        depth += 1;
                        current.push(ch);
                    }
                    ')' | ']' | '}' => {
                        depth -= 1;
                        if depth == 0 {
                            args.push(current.trim().to_owned());
                            return args;
                        }
                        current.push(ch);
                    }
                    ',' if depth == 1 => {
                        args.push(current.trim().to_owned());
                        current.clear();
                    }
                    _ => current.push(ch),
                }
            }
            panic!("unterminated call argument list at byte {open}");
        }

        /// The value of `const NAME: &str = "…";` in this file.
        fn const_value(source: &str, name: &str) -> Option<String> {
            let needle = format!("const {name}: &str = \"");
            let at = source.find(&needle)? + needle.len();
            let rest = &source[at..];
            let end = rest.find('"')?;
            Some(rest[..end].to_owned())
        }

        /// The first `format!("…")` literal inside `fn name`, with a leading
        /// `{CONST}` interpolation resolved and everything from the first
        /// remaining `{` dropped — which is exactly the family prefix.
        fn family_prefix(source: &str, name: &str) -> Option<String> {
            let at = source.find(&format!("fn {name}("))?;
            let body = &source[at..];
            let start = body.find("format!(\"")? + "format!(\"".len();
            let rest = &body[start..];
            let end = rest.find('"')?;
            let template = &rest[..end];

            let mut prefix = String::new();
            let mut remainder = template;
            if let Some(stripped) = remainder.strip_prefix('{') {
                let close = stripped.find('}')?;
                let ident = &stripped[..close];
                if ident.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    prefix.push_str(&const_value(source, ident)?);
                    remainder = &stripped[close + 1..];
                }
            }
            let literal_end = remainder.find('{').unwrap_or(remainder.len());
            prefix.push_str(&remainder[..literal_end]);
            Some(prefix)
        }

        /// The name of the function a byte offset sits inside.
        fn enclosing_fn(source: &str, at: usize) -> String {
            let head = &source[..at];
            let mut best = String::from("<top level>");
            let mut cursor = 0usize;
            while let Some(found) = head[cursor..].find("fn ") {
                let start = cursor + found + "fn ".len();
                let name: String = head[start..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    best = name;
                }
                cursor = start;
            }
            best
        }

        /// Every settings-key call site in the workspace's production sources.
        fn scan() -> Vec<(String, usize, Site)> {
            let root = crates_dir();
            let mut files = Vec::new();
            for crate_name in SCANNED_CRATES {
                rust_files(&root.join(crate_name).join("src"), &mut files);
            }
            files.sort();

            let mut sites = Vec::new();
            for path in files {
                let relative = path
                    .strip_prefix(&root)
                    .expect("scanned files live under the crates directory")
                    .to_string_lossy()
                    .replace('\\', "/");
                let raw = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read {relative}: {error}"));
                let source = production(&path, &raw);

                for name in ["get_setting", "set_setting"] {
                    let needle = format!("{name}(");
                    let mut cursor = 0usize;
                    while let Some(found) = source[cursor..].find(&needle) {
                        let at = cursor + found;
                        cursor = at + needle.len();
                        // The definitions of the two functions are not call sites.
                        if source[..at].ends_with("fn ") {
                            continue;
                        }
                        // `pub fn get_setting` also matches `notes_read_mark_get`
                        // style neighbours only by suffix; require a word boundary.
                        let preceding = source[..at].chars().next_back();
                        if preceding.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let args = call_args(&source, at + needle.len() - 1);
                        let Some(raw_key) = args.get(1) else {
                            continue;
                        };
                        let key_expr = raw_key.trim().trim_start_matches('&').trim();
                        let site = if let Some(literal) = key_expr
                            .strip_prefix('"')
                            .and_then(|rest| rest.split('"').next())
                        {
                            Site::Key(literal.to_owned())
                        } else if key_expr
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                            && !key_expr.is_empty()
                        {
                            match const_value(&source, key_expr) {
                                Some(value) => Site::Key(value),
                                None => Site::Dynamic(enclosing_fn(&source, at)),
                            }
                        } else if let Some(fn_name) = key_expr.split('(').next().filter(|n| {
                            !n.is_empty()
                                && n.chars().all(|c| {
                                    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
                                })
                                && key_expr.contains('(')
                        }) {
                            match family_prefix(&source, fn_name) {
                                Some(prefix) => Site::Family(prefix),
                                None => Site::Dynamic(enclosing_fn(&source, at)),
                            }
                        } else {
                            Site::Dynamic(enclosing_fn(&source, at))
                        };
                        let line = source[..at].matches('\n').count() + 1;
                        sites.push((relative.clone(), line, site));
                    }
                }
            }
            sites
        }

        #[test]
        fn the_scanner_actually_scans() {
            // A source-reading test that reads no source passes silently, which
            // would be the exact defect it exists to catch. Three anchors: a
            // floor on the call sites found, one key that only resolves if
            // `const` lookup works, and one family that only resolves if the
            // `format!` walk works.
            let sites = scan();
            assert!(
                sites.len() >= 60,
                "only {} settings call sites found; the scanner has stopped reading the sources",
                sites.len()
            );
            assert!(
                sites
                    .iter()
                    .any(|(_, _, site)| matches!(site, Site::Key(k) if k == "recording.fps")),
                "the `const` resolution path found nothing"
            );
            assert!(
                sites
                    .iter()
                    .any(|(_, _, site)| matches!(site, Site::Family(p) if p == "notes.read.")),
                "the key-family resolution path found nothing"
            );
        }

        #[test]
        fn every_settings_key_in_the_sources_is_classified() {
            let mut unclassified: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (file, line, site) in scan() {
                let key = match &site {
                    Site::Key(key) => key.clone(),
                    Site::Family(prefix) => format!("{prefix}<…>"),
                    Site::Dynamic(_) => continue,
                };
                let known = match &site {
                    Site::Key(key) => is_known(key),
                    Site::Family(prefix) => KEYS
                        .iter()
                        .any(|spec| spec.family && spec.key == prefix.as_str()),
                    Site::Dynamic(_) => true,
                };
                if !known {
                    unclassified
                        .entry(key)
                        .or_default()
                        .push(format!("{file}:{line}"));
                }
            }
            assert!(
                unclassified.is_empty(),
                "these settings keys are used but not classified in `config::keys::KEYS` — \
                 add a `KeySpec` saying what each one is, and if a file must never set it, \
                 say so with `Settable::Never` and a reason:\n{}",
                unclassified
                    .iter()
                    .map(|(key, sites)| format!("  {key}  ({})", sites.join(", ")))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }

        #[test]
        fn every_classified_key_is_still_used() {
            // The other direction. A registry that only grows becomes a list of
            // keys that used to exist, and a docs table of settings that do
            // nothing is worse than no table.
            let sites = scan();
            let mut orphans = Vec::new();
            for spec in KEYS {
                let used = sites.iter().any(|(_, _, site)| match site {
                    Site::Key(key) => !spec.family && key == spec.key,
                    Site::Family(prefix) => spec.family && prefix == spec.key,
                    Site::Dynamic(_) => false,
                });
                if !used {
                    orphans.push(spec.display_key());
                }
            }
            assert!(
                orphans.is_empty(),
                "these keys are classified but no longer read or written anywhere; \
                 delete their `KeySpec`: {orphans:?}"
            );
        }

        #[test]
        fn every_dynamic_call_site_is_declared_with_a_reason() {
            let found: BTreeSet<(String, String)> = scan()
                .into_iter()
                .filter_map(|(file, _, site)| match site {
                    Site::Dynamic(function) => Some((file, function)),
                    _ => None,
                })
                .collect();
            let declared: BTreeSet<(String, String)> = DYNAMIC_SITES
                .iter()
                .map(|(file, function)| ((*file).to_owned(), (*function).to_owned()))
                .collect();
            assert_eq!(
                found, declared,
                "a settings write whose key is only known at runtime cannot be covered by \
                 this test, so every one of them is listed in `DYNAMIC_SITES` with the \
                 reason it has to be dynamic. Left = found in the sources, right = declared."
            );
        }
    }

    #[test]
    fn no_key_is_listed_twice_and_families_end_in_a_dot() {
        let mut seen = std::collections::BTreeSet::new();
        for spec in KEYS {
            assert!(seen.insert(spec.key), "{} is listed twice", spec.key);
            if spec.family {
                assert!(
                    spec.key.ends_with('.'),
                    "{} is a family, so its prefix must end in a dot or it would match \
                     `notes.readable` as well as `notes.read.<id>`",
                    spec.key
                );
            }
        }
    }

    #[test]
    fn a_family_matches_its_children_and_not_its_prefix() {
        // The prefix alone is not a key: `notes.read.` with nothing after it is
        // not a note's read mark, and matching it would let a file set a row
        // nothing ever reads.
        assert!(spec("notes.read.").is_none());
        assert_eq!(spec("notes.read.01ABC").map(|s| s.key), Some("notes.read."));
        assert_eq!(
            spec("notes.capture_draft.draft").map(|s| s.key),
            Some("notes.capture_draft.")
        );
        // A neighbour that merely starts with the same letters is not a child.
        assert!(spec("notes.readable").is_none());
    }

    #[test]
    fn machine_local_keys_are_refused_from_a_shared_file_and_accepted_from_this_machines() {
        // The half of coverage that matters more: a key that must not be
        // file-settable is classified as such, and says why.
        for key in [
            "hotkey.global",
            "hotkey.recording",
            "hotkey.capture",
            "recording.destination_dir",
            "recording.destination_profile_id",
            "sync.git_path",
            "notes.active_vault",
        ] {
            let refusal = layer_may_set(key, false).expect_err("refused from a shared file");
            assert!(
                matches!(refusal, RefusalReason::MachineLocal { .. }),
                "{key} should be refused as machine-local, got {refusal:?}"
            );
            let sentence = refusal.to_string();
            assert!(
                sentence.contains(key),
                "the refusal names the key: {sentence}"
            );
            assert!(
                sentence.contains("keeper.<host>.toml"),
                "the refusal says where it belongs instead: {sentence}"
            );
            layer_may_set(key, true).expect("accepted from this machine's own file");
        }
    }

    #[test]
    fn state_is_refused_from_every_file_including_this_machines() {
        for key in [
            "sdk_encryption",
            "ui.ios_sync_disclosure_shown",
            "ui.recovered_sessions_acknowledged",
            "notes.capture_draft.draft",
            "notes.capture_placement.draft",
            "notes.read.01ABC",
        ] {
            for machine_scoped in [false, true] {
                let refusal = layer_may_set(key, machine_scoped).expect_err("no file may set this");
                assert!(
                    matches!(refusal, RefusalReason::NotAPreference { .. }),
                    "{key} should be refused as state, got {refusal:?}"
                );
                assert!(
                    refusal.to_string().contains(key),
                    "the refusal names the key"
                );
            }
        }
    }

    #[test]
    fn an_unknown_key_names_itself() {
        // The typo case, and the reason unknown keys are an error rather than a
        // shrug: `recordng.fps` in a file that loads is a setting that never took.
        let refusal = layer_may_set("recordng.fps", false).expect_err("a typo is refused");
        assert_eq!(
            refusal,
            RefusalReason::Unknown {
                key: "recordng.fps".to_owned()
            }
        );
        assert!(refusal.to_string().contains("recordng.fps"));
        assert!(refusal.to_string().contains("docs/settings-keys.md"));
    }

    #[test]
    fn ordinary_preferences_are_settable_from_any_file() {
        for key in ["recording.fps", "debug.mode", "notify.dnd_global"] {
            layer_may_set(key, false).expect("a shared file may set a preference");
            layer_may_set(key, true).expect("so may this machine's file");
        }
    }

    /// Parse a bare TOML value expression, as it would appear on the right of
    /// `key = …` in a layer file.
    fn toml_value(expression: &str) -> toml::Value {
        let document: toml::Value = toml::from_str(&format!("v = {expression}"))
            .unwrap_or_else(|error| panic!("`{expression}` is not a TOML value: {error}"));
        document.get("v").expect("the parsed table holds v").clone()
    }

    #[test]
    fn every_documented_example_is_a_value_its_own_key_accepts() {
        // The docs table's example column is data, not decoration: each example
        // is fed through the coercion the layer loader uses. A shape and an
        // example that disagree fail here rather than in somebody's config file.
        for spec in KEYS {
            match spec.settable {
                Settable::AnyLayer | Settable::MachineFileOnly(_) => {
                    assert!(
                        !spec.example.is_empty(),
                        "{} may be set from a file, so the docs need an example",
                        spec.key
                    );
                    spec.shape
                        .coerce(spec.key, &toml_value(spec.example))
                        .unwrap_or_else(|error| {
                            panic!(
                                "the documented example for {} is rejected: {error}",
                                spec.key
                            )
                        });
                }
                Settable::Never(_) => assert!(
                    spec.example.is_empty(),
                    "{} may never be set from a file, so an example would be an invitation",
                    spec.key
                ),
            }
        }
    }

    #[test]
    fn a_boolean_is_written_as_a_boolean_and_stored_as_the_table_spells_it() {
        assert_eq!(
            Shape::Flag01.coerce("debug.mode", &toml_value("true")),
            Ok("1".to_owned())
        );
        assert_eq!(
            Shape::Flag01.coerce("debug.mode", &toml_value("false")),
            Ok("0".to_owned())
        );
        assert_eq!(
            Shape::FlagOnOff.coerce("honor_remote_deletions", &toml_value("true")),
            Ok("on".to_owned())
        );
        assert_eq!(
            Shape::FlagTrueFalse.coerce("favorites_collapsed", &toml_value("true")),
            Ok("true".to_owned())
        );
        // The stored spelling is accepted too — a value copied out of the docs
        // table must not be rejected for being right.
        assert_eq!(
            Shape::Flag01.coerce("debug.mode", &toml_value("\"1\"")),
            Ok("1".to_owned())
        );
    }

    #[test]
    fn an_out_of_range_number_says_the_range_instead_of_clamping() {
        // The getter clamps a row that rotted. A number a person typed into a
        // file they can see gets a sentence back instead.
        let error = Shape::Int { min: 1, max: 600 }
            .coerce("recording.duration_cap_minutes", &toml_value("6000"))
            .expect_err("out of range");
        let sentence = error.to_string();
        assert!(sentence.contains("recording.duration_cap_minutes"));
        assert!(sentence.contains("1 to 600"), "{sentence}");
        assert!(sentence.contains("6000"), "{sentence}");
    }

    #[test]
    fn a_choice_outside_the_set_lists_the_set() {
        let error = Shape::Choice(&["h264", "hevc"])
            .coerce("recording.codec", &toml_value("\"av1\""))
            .expect_err("not a legal codec");
        assert!(error.to_string().contains("`h264`"), "{error}");
        assert!(error.to_string().contains("`hevc`"), "{error}");
        assert!(error.to_string().contains("av1"), "{error}");
        // An integer choice may be written as an integer: nobody types "30".
        assert_eq!(
            Shape::Choice(&["10", "15", "30", "60"]).coerce("recording.fps", &toml_value("30")),
            Ok("30".to_owned())
        );
    }

    #[test]
    fn a_layer_file_lands_each_boolean_in_the_spelling_its_own_getter_reads() {
        // The end of "every setting reaches the file". Reaching it is not
        // enough — it has to arrive in the spelling the getter compares
        // against, and three keys predate the `"1"`/`"0"` convention:
        // `honor_remote_deletions` and `sdk_encryption` are `"on"`/`"off"`
        // (archive/mod.rs, auth.rs), `favorites_collapsed` is
        // `"true"`/`"false"` (keeper/src/ipc.rs). A shape-blind boolean
        // mapping writes `"1"` for all of them, and `== Some("on")` then reads
        // the value a person set to `true` as FALSE. Silent, and inverted,
        // which is the worst pair.
        let parsed = crate::config::parse_layer_file(
            std::path::Path::new("/home/tester/.keeper/keeper.toml"),
            crate::config::LayerTier::UserGlobal,
            None,
            "[settings]\n\
             \"honor_remote_deletions\" = true\n\
             \"favorites_collapsed\" = true\n\
             \"debug.mode\" = true\n\
             \"recording.fps\" = 60\n",
        );
        assert!(
            parsed.faults.is_empty(),
            "nothing in that file is wrong: {:?}",
            parsed.faults
        );
        let stored = |key: &str| {
            parsed
                .settings
                .get(key)
                .unwrap_or_else(|| panic!("{key} resolved from the file"))
                .value
                .clone()
        };
        assert_eq!(stored("honor_remote_deletions"), "on");
        assert_eq!(stored("favorites_collapsed"), "true");
        assert_eq!(stored("debug.mode"), "1");
        assert_eq!(stored("recording.fps"), "60");
    }

    mod docs {
        use super::*;

        fn docs_path() -> std::path::PathBuf {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../docs/settings-keys.md")
        }

        #[test]
        fn the_checked_in_table_matches_the_registry() {
            let checked_in = std::fs::read_to_string(docs_path()).expect(
                "docs/settings-keys.md exists; regenerate it with \
                 `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`",
            );
            assert_eq!(
                checked_in,
                render_docs(),
                "docs/settings-keys.md is stale. Regenerate it with \
                 `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`"
            );
        }

        /// Rewrite the checked-in table from the registry. Not a test; the
        /// generator, parked where the person who just failed the pin above is
        /// already looking.
        #[test]
        #[ignore = "generator, not a check: rewrites docs/settings-keys.md"]
        fn regenerate() {
            std::fs::write(docs_path(), render_docs()).expect("write docs/settings-keys.md");
        }
    }
}
