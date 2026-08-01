//! `keeper-syncd`'s TOML configuration (Story 30.2, AD-52).
//!
//! The configuration maps **1:1 onto [`SyncProfile`]**: a `[[profile]]` table
//! carries exactly the fields the app's JSON profile carries, under exactly the
//! same names, so moving a profile between the app and a server is a copy — not
//! a translation with its own bugs. That is enforced structurally rather than
//! by discipline: profile tables are handed straight to `SyncProfile`'s own
//! `Deserialize`, and the set of accepted keys is *derived from the type* at
//! runtime, so it cannot drift when a field is added.
//!
//! # Unknown keys are errors
//!
//! Serde ignores unknown fields by default, and that default is wrong here. A
//! typo in `remoteUrl` would leave the operator with a config that loads, a
//! daemon that starts, a tray that says "up to date" — and nothing synced,
//! because the profile silently took a different remote. Every table is
//! therefore closed: an unrecognised key names itself and stops startup.
//!
//! Nothing is ever partially applied. Parsing builds the complete profile set
//! or returns an error; a bad third profile does not leave two running.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use keeper_sync::profile::DEFAULT_POLL_INTERVAL_MS;
use keeper_sync::{Result, SyncError, SyncProfile};
use serde::{Deserialize, Serialize};

/// Log levels the `[daemon] logLevel` key accepts.
///
/// Checked rather than passed through: an unrecognised level would silently
/// fall back to `info`, and "I set it to debug and got nothing" is a bad hour.
const LOG_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

/// Floor for the supervisor tick.
///
/// Below about a second the scheduler spends more time waking than working, and
/// on a shared git host a fleet of daemons at 100 ms is indistinguishable from
/// abuse. The engine's own default is 15 s.
const MIN_POLL_INTERVAL_MS: u64 = 1_000;

/// A parsed, fully validated daemon configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub daemon: DaemonSettings,
    pub profiles: Vec<SyncProfile>,
}

/// The `[daemon]` table: settings that belong to the process, not to a profile.
///
/// Both spellings are accepted for each key — the canonical camelCase that
/// matches `[[profile]]`, and the snake_case an operator will reach for in a
/// TOML file. Neither is silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct DaemonSettings {
    /// How often the supervisor re-reads the journal.
    #[serde(alias = "poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Default `tracing` level, overridden by `RUST_LOG` and `--verbose`.
    #[serde(alias = "log_level")]
    pub log_level: String,
    /// An explicit `git` binary, or `None` to search `PATH` (Story 34.14).
    ///
    /// A process-wide fact, not a per-profile one: every profile shares one
    /// engine and one `GitCli`, so this belongs in `[daemon]` beside the other
    /// two settings that are about the process rather than a folder.
    ///
    /// A path that does not clear the version floor **refuses** — the daemon does
    /// not fall back to `PATH`. Naming a binary and silently getting a different
    /// one is the fault this setting exists to fix. An empty or all-whitespace
    /// value is not such a naming: it means automatic, and
    /// `LinuxPlatform::with_git_path` filters it out on the way in so that
    /// clearing the key and never writing it are one state, exactly as the app's
    /// own copy of this setting reads it back.
    #[serde(alias = "git_path")]
    pub git_path: Option<PathBuf>,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            log_level: "info".to_owned(),
            git_path: None,
        }
    }
}

/// The document as TOML sees it.
///
/// `deny_unknown_fields` here is what catches a misspelled *section* — a
/// `[[profiles]]` table (plural) would otherwise be parsed, ignored, and leave
/// the daemon running with no profiles at all.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    #[serde(default)]
    daemon: DaemonSettings,
    /// Profile tables, still untyped: they are checked for unknown keys before
    /// being handed to `SyncProfile`, so the error can name the key rather than
    /// serde's less specific "unknown field" span.
    #[serde(default, rename = "profile")]
    profile: Vec<toml::Value>,
}

/// Read and parse the configuration at `path`.
pub fn load(path: &Path) -> Result<DaemonConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(SyncError::Config(format!(
                "no configuration at {}; run `keeper-syncd init` to write a documented \
                 starter config, or pass --config",
                path.display()
            )))
        }
        Err(err) => return Err(SyncError::io("read daemon config", path, err)),
    };
    parse(&text).map_err(|err| match err {
        // Name the file. With `--config` and `KEEPER_SYNCD_CONFIG` both in play,
        // "unknown key" without a path sends people editing the wrong one.
        SyncError::Config(message) => SyncError::Config(format!("{}: {message}", path.display())),
        other => other,
    })
}

/// Parse configuration text.
///
/// Separate from [`load`] so the parser is testable without a filesystem, and
/// so [`example`] can be checked against the very code that will read it.
pub fn parse(text: &str) -> Result<DaemonConfig> {
    let raw: RawDocument = toml::from_str(text).map_err(|err| {
        // `toml`'s Display carries the line, the column and a snippet of the
        // offending input, which is the whole "fails loudly with the offending
        // line/key" requirement — do not flatten it to one line.
        SyncError::Config(format!("cannot read the daemon configuration\n{err}"))
    })?;

    if !LOG_LEVELS.contains(&raw.daemon.log_level.as_str()) {
        return Err(SyncError::Config(format!(
            "[daemon] logLevel = \"{}\" is not a level; expected one of {}",
            raw.daemon.log_level,
            LOG_LEVELS.join(", ")
        )));
    }
    if raw.daemon.poll_interval_ms < MIN_POLL_INTERVAL_MS {
        return Err(SyncError::Config(format!(
            "[daemon] pollIntervalMs = {} is below the {MIN_POLL_INTERVAL_MS} ms floor",
            raw.daemon.poll_interval_ms
        )));
    }

    let accepted = accepted_profile_keys()?;
    let mut profiles = Vec::with_capacity(raw.profile.len());
    let mut seen_ids = BTreeSet::new();

    for (index, table) in raw.profile.into_iter().enumerate() {
        let profile = parse_profile(index, table, &accepted)?;
        if !seen_ids.insert(profile.id.clone()) {
            // Two tables with one id would collide in `sync.db`, and the
            // survivor would depend on insertion order.
            return Err(SyncError::Config(format!(
                "two [[profile]] tables share id `{}`; ids must be unique",
                profile.id
            )));
        }
        profiles.push(profile);
    }

    Ok(DaemonConfig {
        daemon: raw.daemon,
        profiles,
    })
}

/// Convert one `[[profile]]` table into a validated [`SyncProfile`].
fn parse_profile(
    index: usize,
    table: toml::Value,
    accepted: &BTreeSet<String>,
) -> Result<SyncProfile> {
    // TOML and JSON are the same data model for everything a profile contains,
    // so bouncing through `serde_json::Value` costs one small allocation and
    // buys the exact deserializer the app uses — no second code path that can
    // interpret a field differently.
    let value = serde_json::to_value(&table)
        .map_err(|err| SyncError::Config(format!("[[profile]] #{}: {err}", index + 1)))?;
    let serde_json::Value::Object(fields) = value else {
        return Err(SyncError::Config(format!(
            "[[profile]] #{} is not a table",
            index + 1
        )));
    };

    // A label for diagnostics, read before validation so even a broken table
    // can be pointed at by name.
    let label = fields
        .get("name")
        .or_else(|| fields.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(|name| format!(" ({name})"))
        .unwrap_or_default();

    let mut canonical = serde_json::Map::with_capacity(fields.len());
    for (key, field) in fields {
        let name = canonical_key(&key);
        if !accepted.contains(&name) {
            return Err(SyncError::Config(format!(
                "[[profile]] #{}{label}: unknown key `{key}`; accepted keys are {}",
                index + 1,
                accepted.iter().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        if canonical.insert(name.clone(), field).is_some() {
            // `local_path` and `localPath` in one table: the survivor would
            // depend on map ordering, so refuse rather than pick.
            return Err(SyncError::Config(format!(
                "[[profile]] #{}{label}: key `{name}` is given twice \
                 (camelCase and snake_case spellings are the same key)",
                index + 1
            )));
        }
    }

    let profile: SyncProfile = serde_json::from_value(serde_json::Value::Object(canonical))
        .map_err(|err| SyncError::Config(format!("[[profile]] #{}{label}: {err}", index + 1)))?;
    // The engine's own validator, not a second copy of the rules: a config-file
    // profile must clear exactly the bar an app-created one clears.
    profile.validate().map_err(|err| match err {
        SyncError::Config(message) => {
            SyncError::Config(format!("[[profile]] #{}{label}: {message}", index + 1))
        }
        other => other,
    })?;
    Ok(profile)
}

/// Every key a `[[profile]]` table may carry, derived from [`SyncProfile`].
///
/// Derived, not listed. A hand-maintained list is a list that goes stale the
/// first time the engine gains a field, and the failure mode is the worst one
/// available: the daemon rejects a profile the app just wrote.
fn accepted_profile_keys() -> Result<BTreeSet<String>> {
    let probe = SyncProfile::new("id", "name", "/", "remote");
    let value = serde_json::to_value(&probe).map_err(|err| {
        SyncError::Config(format!("cannot enumerate the accepted profile keys: {err}"))
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map.into_iter().map(|(key, _)| key).collect()),
        _ => Err(SyncError::Config(
            "cannot enumerate the accepted profile keys: a profile is not an object".to_owned(),
        )),
    }
}

/// Fold a snake_case key onto the camelCase spelling `SyncProfile` uses.
///
/// TOML convention is snake_case and the profile schema is camelCase (it is the
/// app's JSON schema). Accepting both keeps the 1:1 copy-a-table promise while
/// not making an operator write `localPath` in a `.toml` file.
fn canonical_key(key: &str) -> String {
    if !key.contains('_') {
        return key.to_owned();
    }
    let mut out = String::with_capacity(key.len());
    for (index, part) in key.split('_').filter(|part| !part.is_empty()).enumerate() {
        let mut chars = part.chars();
        match chars.next() {
            None => {}
            Some(first) if index == 0 => {
                out.push(first);
                out.push_str(chars.as_str());
            }
            Some(first) => {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

/// A documented starter configuration, written by `keeper-syncd init`.
///
/// This string is load-bearing: it is the first and often the only
/// documentation an operator reads. A test parses it through [`parse`], because
/// an example that does not load is worse than no example at all.
pub fn example() -> String {
    // Kept as one literal rather than assembled, so what a reader sees here is
    // byte-for-byte what lands in `config.toml`.
    r#"# keeper-syncd configuration
#
# Every [[profile]] table maps 1:1 onto a keeper sync profile, so a profile can
# be moved between the desktop app and this daemon by copying the table.
# Both spellings of every key are accepted: `localPath` (matching the app's own
# profile schema) and `local_path`. An unrecognised key is an error, not a
# warning -- a silently ignored typo is how a folder ends up never syncing.
#
# Credentials are NEVER stored here. The daemon reads them from
#   $KEEPER_SYNC_SECRET_SYNC_<PROFILE ID>_CREDENTIAL
# or from a 0600 file at
#   $XDG_CONFIG_HOME/keeper-sync/secrets/sync_<profile id>_credential
# A secret file readable by group or others is refused.

[daemon]
# How often the supervisor re-reads the work journal, in milliseconds.
pollIntervalMs = 15000
# trace | debug | info | warn | error. RUST_LOG and --verbose override this.
logLevel = "info"
# Which `git` to drive. Left out, the daemon probes every `git` on PATH in order
# and uses the first that is at least 2.42 -- an executable file called `git` is
# not necessarily a git this engine can drive, so the first hit is not the
# answer. Set this when PATH puts an old or broken git first. A gitPath that is
# missing, unrunnable or below 2.42 REFUSES and says so; it never falls back to
# PATH, because naming a binary and silently getting a different one is exactly
# the fault this key exists to fix. `keeper-syncd doctor` prints what was chosen.
# gitPath = "/usr/local/bin/git"

# ---------------------------------------------------------------------------
# The sample profiles below are COMMENTED OUT on purpose.
#
# `keeper-syncd init` must produce a configuration that is immediately valid,
# and a live profile pointing at a path that does not exist would make the very
# first `doctor` and `sync` fail on a fresh install. Uncomment one and edit it,
# or let `keeper-syncd add` append a real table for you.
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# A normal two-way folder: local edits are pushed, remote edits are applied,
# and a genuine divergence becomes a conflict copy rather than a prompt.
# ---------------------------------------------------------------------------
# [[profile]]
# id = "01JQ8ZK9V3M4N5P6R7S8T9W0XY"
# name = "documents"
# localPath = "/srv/keeper/documents"
# remoteUrl = "https://forgejo.example.com/dev/documents.git"
# branch = "main"
# direction = "bidirectional"      # bidirectional | pushOnly | pullOnly
# lane = "main"                    # main | worktree
# lfsMode = "materialize"          # materialize | pointerOnly | disabled
# Files at or above this size are tracked through git-LFS (4 MiB).
# lfsThresholdBytes = 4194304
# Globs that must never go through LFS, whatever their size. The rule keeper
# records in .gitattributes is per-EXTENSION, so one oversized note would write
# `*.md filter=lfs` and every note in the repository stops being diffable.
# Protect the formats you need to read as text -- especially when a low
# threshold is set to catch bulk media. Same dialect as gitignore: a pattern
# with no `/` matches the basename at any depth, one with `/` is anchored at the
# repository root. The trade-off is real: a matched file stays an ordinary git
# blob however large it grows, and gitoxide has no streaming object read.
# lfsNever = ["*.md", "*.txt"]
# Release local LFS objects once the remote holds them. On the machine where
# content originates every LFS file exists twice -- worktree and object store --
# because the clean path must read the bytes to compute the pointer. This
# reclaims the second copy. The worktree keeps every file; an object is released
# only when its content is still in the worktree AND the journal owes no
# transfer for it, so rebuilding costs one local read. The honest trade: the
# drive stops being self-sufficient, because restoring a file the worktree later
# loses then needs the network.
# lfsPruneLocal = false
# How long a file must stop changing before it is considered complete.
# settleMs = 5000
# pollIntervalMs = 15000
# Only these repository subpaths are checked out. Empty means the whole repo.
# subpaths = []
# Extra exclusion globs, on top of the built-in partial-download set.
# excludes = ["*.tmp", "node_modules/**"]
# Extra `Keeper-Tag:` trailers stamped on every commit this profile makes.
# tags = ["server"]
# removable = false
# enabled = true
# Overrides the derived git author for this profile.
# authorOverride = "Build Box <builds@example.com>"

# ---------------------------------------------------------------------------
# A bot lane: an agent writes into a linked worktree on a generated branch and
# hands off to a human by pull request. One-way by construction -- nothing from
# the remote is ever applied here.
# ---------------------------------------------------------------------------
# [[profile]]
# id = "01JQ8ZK9V3M4N5P6R7S8T9W0Z1"
# name = "agent-drafts"
# localPath = "/srv/keeper/agent-drafts"
# remoteUrl = "https://forgejo.example.com/dev/handbook.git"
# branch = "main"
# direction = "pushOnly"           # a worktree lane requires pushOnly
# lane = "worktree"
# lfsMode = "disabled"
# subpaths = ["docs"]
# enabled = true

# ---------------------------------------------------------------------------
# A removable volume. `removable = true` is what makes an unplugged drive a
# pause instead of a 40 GB deletion: with the volume marker absent the profile
# stops, the journal is kept, and nothing is staged, committed or pushed.
# ---------------------------------------------------------------------------
# [[profile]]
# id = "01JQ8ZK9V3M4N5P6R7S8T9W0Z2"
# name = "field-drive"
# localPath = "/media/dev/FIELD/captures"
# remoteUrl = "https://forgejo.example.com/dev/captures.git"
# branch = "main"
# direction = "pushOnly"
# lane = "main"
# lfsMode = "materialize"
# removable = true
# Removable media get a longer settle window: their mtime granularity is worse.
# settleMs = 10000
# enabled = true
# `volumeId` names the marker at the volume's mount root. Leave it out: the
# daemon mints it on first sight of the media and remembers it across restarts,
# and setting it by hand only makes sense to point a profile at a volume that is
# already marked — a wrong value reads as "some other stick is mounted here" and
# stops the profile rather than syncing it.
# volumeId = "01JQ8ZK9V3M4N5P6R7S8T9W0Z3"
"#
    .to_owned()
}

/// Render one profile as a `[[profile]]` table, for `keeper-syncd add`.
///
/// Appending text rather than re-serializing the whole document is deliberate:
/// `toml::to_string` would round-trip the file and drop every comment in it,
/// including the ones [`example`] wrote to explain the keys.
pub fn render_profile_table(profile: &SyncProfile) -> Result<String> {
    let value = serde_json::to_value(profile).map_err(|err| {
        SyncError::Config(format!("cannot render profile `{}`: {err}", profile.id))
    })?;
    let serde_json::Value::Object(fields) = value else {
        return Err(SyncError::Config(
            "cannot render profile: a profile is not an object".to_owned(),
        ));
    };

    let mut out = String::from("\n[[profile]]\n");
    // BTreeMap for a stable key order: two `add` runs must produce diffable
    // files, and serde_json's Map is insertion-ordered only with a feature we
    // do not enable.
    let ordered: BTreeMap<_, _> = fields.into_iter().collect();
    for (key, field) in ordered {
        // A `None` author override has no TOML representation; omitting it is
        // exactly what `#[serde(default)]` expects to see on the way back in.
        if field.is_null() {
            continue;
        }
        out.push_str(&key);
        out.push_str(" = ");
        out.push_str(&render_scalar(&field)?);
        out.push('\n');
    }
    Ok(out)
}

fn render_scalar(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::Bool(flag) => Ok(flag.to_string()),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::String(text) => Ok(toml_string(text)),
        serde_json::Value::Array(items) => {
            let rendered = items
                .iter()
                .map(render_scalar)
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            Ok(format!("[{rendered}]"))
        }
        other => Err(SyncError::Config(format!(
            "cannot render `{other}` as a TOML value"
        ))),
    }
}

/// Quote a string as a TOML basic string.
///
/// Hand-rolled because the alternative is pulling the whole document through a
/// serializer that would strip the file's comments. Escapes exactly the set
/// TOML requires in a basic string.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // TOML forbids raw control characters in a basic string.
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_sync::profile::LfsMode;
    use keeper_sync::{SyncDirection, SyncLane};

    const MINIMAL: &str = r#"
[[profile]]
id = "01PROFILE"
name = "docs"
localPath = "/srv/docs"
remoteUrl = "https://example.com/docs.git"
branch = "main"
direction = "bidirectional"
lane = "main"
lfsMode = "materialize"
"#;

    #[test]
    fn a_toml_profile_equals_the_json_profile_it_was_copied_from() {
        // The 1:1 promise, asserted directly: the same field values written as
        // a TOML table and as the app's JSON must produce the identical struct.
        let toml_text = r#"
[[profile]]
id = "01PROFILE"
name = "captures"
localPath = "/srv/captures"
remoteUrl = "https://example.com/captures.git"
branch = "trunk"
direction = "pushOnly"
lane = "main"
subpaths = ["raw", "edits"]
excludes = ["*.tmp"]
removable = true
lfsMode = "pointerOnly"
lfsThresholdBytes = 1048576
settleMs = 7000
pollIntervalMs = 20000
tags = ["field"]
authorOverride = "Rig <rig@example.com>"
enabled = false
"#;
        let json = r#"{
            "id": "01PROFILE",
            "name": "captures",
            "localPath": "/srv/captures",
            "remoteUrl": "https://example.com/captures.git",
            "branch": "trunk",
            "direction": "pushOnly",
            "lane": "main",
            "subpaths": ["raw", "edits"],
            "excludes": ["*.tmp"],
            "removable": true,
            "lfsMode": "pointerOnly",
            "lfsThresholdBytes": 1048576,
            "settleMs": 7000,
            "pollIntervalMs": 20000,
            "tags": ["field"],
            "authorOverride": "Rig <rig@example.com>",
            "enabled": false
        }"#;

        let from_toml = parse(toml_text).expect("toml").profiles;
        let from_json: SyncProfile = serde_json::from_str(json).expect("json");

        assert_eq!(from_toml, vec![from_json]);
        // Spot-check the enums actually landed, not just that the two agree.
        assert_eq!(from_toml[0].direction, SyncDirection::PushOnly);
        assert_eq!(from_toml[0].lane, SyncLane::Main);
        assert_eq!(from_toml[0].lfs_mode, LfsMode::PointerOnly);
    }

    #[test]
    fn snake_case_keys_are_the_same_keys() {
        let snake = r#"
[[profile]]
id = "01PROFILE"
name = "docs"
local_path = "/srv/docs"
remote_url = "https://example.com/docs.git"
branch = "main"
direction = "bidirectional"
lane = "main"
lfs_mode = "materialize"
lfs_threshold_bytes = 2048
settle_ms = 6000
"#;
        let parsed = parse(snake).expect("snake_case must parse").profiles;

        assert_eq!(parsed[0].local_path, Path::new("/srv/docs"));
        assert_eq!(parsed[0].lfs_threshold_bytes, 2048);
        assert_eq!(parsed[0].settle_ms, 6000);
    }

    #[test]
    fn the_same_key_in_both_spellings_is_refused_rather_than_picked() {
        let both = format!("{MINIMAL}settleMs = 6000\nsettle_ms = 9000\n");

        let err = parse(&both).expect_err("an ambiguous table must not load");

        assert!(err.to_string().contains("settleMs"), "{err}");
    }

    #[test]
    fn an_unknown_profile_key_is_an_error_naming_it() {
        let text = format!("{MINIMAL}remoteUrlz = \"https://typo.example.com/x.git\"\n");

        let err = parse(&text).expect_err("a typo must not be ignored");

        let message = err.to_string();
        assert_eq!(err.code(), "config");
        assert!(
            message.contains("remoteUrlz"),
            "must name the key: {message}"
        );
        // And it must tell the operator what was allowed instead.
        assert!(
            message.contains("remoteUrl"),
            "must list valid keys: {message}"
        );
    }

    #[test]
    fn an_unknown_top_level_section_is_an_error() {
        // `[[profiles]]` (plural) is the classic version of this mistake: it
        // parses, it is ignored, and the daemon runs with zero profiles.
        let err = parse("[[profiles]]\nid = \"x\"\n").expect_err("plural section must fail");

        assert!(err.to_string().contains("profiles"), "{err}");
    }

    #[test]
    fn an_unknown_daemon_key_is_an_error() {
        let err = parse("[daemon]\npollIntervalMS = 5000\n").expect_err("wrong case must fail");

        assert!(err.to_string().contains("pollIntervalMS"), "{err}");
    }

    #[test]
    fn a_missing_required_key_is_an_error_naming_the_field() {
        let text = r#"
[[profile]]
id = "01PROFILE"
name = "docs"
localPath = "/srv/docs"
branch = "main"
direction = "bidirectional"
lane = "main"
lfsMode = "materialize"
"#;

        let err = parse(text).expect_err("a profile with no remote must not load");

        assert!(err.to_string().contains("remoteUrl"), "{err}");
    }

    #[test]
    fn the_engines_own_validator_runs_on_a_config_profile() {
        // A worktree lane is only meaningful one-way (AD-50). The rule lives in
        // `SyncProfile::validate`; this asserts the config path actually calls
        // it rather than reimplementing a subset.
        let text = MINIMAL.replace("lane = \"main\"", "lane = \"worktree\"");

        let err = parse(&text).expect_err("worktree + bidirectional must not load");

        assert!(err.to_string().contains("pushOnly"), "{err}");
    }

    #[test]
    fn a_later_bad_profile_prevents_every_earlier_one_from_loading() {
        // "Never partially applied": one broken table must not leave the good
        // ones running, or the operator gets a half-configured daemon.
        let text =
            format!("{MINIMAL}\n[[profile]]\nid = \"01SECOND\"\nname = \"broken\"\nnope = true\n");

        assert!(parse(&text).is_err());
    }

    #[test]
    fn duplicate_profile_ids_are_refused() {
        let text = format!("{MINIMAL}{MINIMAL}");

        let err = parse(&text).expect_err("two profiles cannot share one id");

        assert!(err.to_string().contains("01PROFILE"), "{err}");
    }

    #[test]
    fn daemon_defaults_apply_when_the_table_is_absent() {
        let config = parse(MINIMAL).expect("parse");

        assert_eq!(config.daemon.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(config.daemon.log_level, "info");
    }

    #[test]
    fn a_nonsense_log_level_is_refused_rather_than_silently_defaulted() {
        let text = format!("[daemon]\nlogLevel = \"verbose\"\n{MINIMAL}");

        let err = parse(&text).expect_err("an unknown level must not fall back");

        let message = err.to_string();
        assert!(message.contains("verbose"), "{message}");
        assert!(
            message.contains("debug"),
            "must list the real levels: {message}"
        );
    }

    #[test]
    fn a_hot_poll_interval_is_refused() {
        let text = format!("[daemon]\npollIntervalMs = 50\n{MINIMAL}");

        assert!(parse(&text).is_err());
    }

    #[test]
    fn the_snake_case_daemon_spelling_is_also_accepted() {
        let text = format!("[daemon]\npoll_interval_ms = 30000\nlog_level = \"debug\"\n{MINIMAL}");

        let config = parse(&text).expect("snake_case daemon keys must parse");

        assert_eq!(config.daemon.poll_interval_ms, 30_000);
        assert_eq!(config.daemon.log_level, "debug");
    }

    #[test]
    fn a_fresh_example_registers_no_live_profiles() {
        // Load-bearing: `keeper-syncd init` writes this, and an example that
        // does not load turns first-run into a support ticket. The samples are
        // commented out on purpose — a live profile pointing at a path that
        // does not exist would make the very first `doctor` and `sync` on a
        // fresh install fail.
        let config = parse(&example()).expect("the shipped example must parse");
        assert!(
            config.profiles.is_empty(),
            "a fresh init must not register any profile"
        );
    }

    #[test]
    fn the_commented_example_profiles_are_themselves_valid() {
        // The stronger property: the samples a user uncomments must actually
        // work. A commented-out example that would not parse is worse than no
        // example at all, and nothing else in the suite would catch it.
        // Uncomment only the TOML-shaped lines: a `[[profile]]` header or a
        // `key = value`. Stripping `# ` from every line would also uncomment
        // the prose, which is not TOML and never was.
        let uncommented: String = example()
            .lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest)
                    if rest.starts_with("[[profile]]")
                        || rest.split_once(" = ").is_some_and(|(key, _)| {
                            !key.is_empty()
                                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        }) =>
                {
                    rest
                }
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let config = parse(&uncommented).expect("the commented samples must be valid TOML");

        assert_eq!(config.profiles.len(), 3);
        for profile in &config.profiles {
            profile
                .validate()
                .expect("every example profile must be valid");
        }
        // They must demonstrate the three shapes they claim to document.
        assert!(config.profiles.iter().any(|p| p.removable));
        assert!(config.profiles.iter().any(|p| p.lane == SyncLane::Worktree));
        assert!(config
            .profiles
            .iter()
            .any(|p| p.direction == SyncDirection::Bidirectional));
    }

    #[test]
    fn a_rendered_profile_table_parses_back_into_the_same_profile() {
        // `keeper-syncd add` appends this text to the config; if it does not
        // round-trip, the next startup rejects the profile just added.
        let mut profile = SyncProfile::new("01ADDED", "added", "/srv/added", "https://x/y.git");
        profile.subpaths = vec!["docs".to_owned()];
        profile.excludes = vec!["a \"quoted\" glob".to_owned()];
        profile.tags = vec!["cli".to_owned()];
        profile.author_override = Some("Box <box@example.com>".to_owned());

        let text = render_profile_table(&profile).expect("render");
        let reparsed = parse(&text).expect("a rendered table must parse").profiles;

        assert_eq!(reparsed, vec![profile]);
    }

    #[test]
    fn a_rendered_profile_omits_an_absent_author_override() {
        let profile = SyncProfile::new("01ADDED", "added", "/srv/added", "https://x/y.git");

        let text = render_profile_table(&profile).expect("render");

        // TOML has no null; emitting one would make the file unparseable.
        assert!(!text.contains("authorOverride"), "{text}");
        assert_eq!(parse(&text).expect("parse").profiles, vec![profile]);
    }

    #[test]
    fn canonical_key_folds_snake_case_onto_the_profile_schema() {
        assert_eq!(canonical_key("lfs_threshold_bytes"), "lfsThresholdBytes");
        assert_eq!(canonical_key("localPath"), "localPath");
        assert_eq!(canonical_key("id"), "id");
        assert_eq!(canonical_key("subpaths"), "subpaths");
    }

    #[test]
    fn load_names_the_file_it_could_not_find() {
        let root = tempfile::tempdir().expect("temp dir");
        let missing = root.path().join("config.toml");

        let err = load(&missing).expect_err("a missing config must be an error");

        let message = err.to_string();
        assert_eq!(err.code(), "config");
        assert!(message.contains("config.toml"), "{message}");
        assert!(
            message.contains("keeper-syncd init"),
            "must say how to fix it: {message}"
        );
    }

    #[test]
    fn load_prefixes_a_parse_error_with_the_offending_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("config.toml");
        std::fs::write(&path, "[daemon]\nnope = 1\n").expect("write");

        let err = load(&path).expect_err("an unknown key must fail");

        assert!(err.to_string().contains("config.toml"), "{err}");
    }
}
