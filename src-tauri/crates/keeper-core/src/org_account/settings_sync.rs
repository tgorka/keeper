//! A person's preferences as two files in their config repository, merged per
//! key with the `settings` table of every device (AD-320..AD-322).
//!
//! ```text
//! <login>/settings.toml             every user-global key
//! <login>/settings.<device>.toml    this device's machine-local keys
//! _template/settings.toml           seeds settings.toml when it is absent
//! _template/settings/<class>.toml   seeds a device file when no device of the
//!                                   same class has one
//! ```
//!
//! The files hold preferences, not pins: a value is applied into the table, so
//! the settings pane can still change it, and `keeper.toml` keeps winning on
//! read wherever it sets the same key. Local identifiers — a sync profile id, a
//! bot id, a provider id — travel as references that name the same drive or
//! endpoint on every device ([`to_portable`], [`from_portable`]).
//!
//! Everything here is pure except [`Catalog::with_bots`] and [`local_values`],
//! which read `keeper.db`; the shell moves the bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::bots::{self, store};
use crate::config::keys::{self, Scope, Shape};
use crate::error::CoreError;
use crate::registry::{self, EmbeddingModel};

use super::layout::{self, DeviceClass, RepoFiles};

/// Which of the two files a key lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SyncedFile {
    /// `settings.toml`: the same answer on every device.
    Shared,
    /// `settings.<device>.toml`: this device's answer.
    Device,
}

/// Keys whose scope would keep them off the repository, or send them to every
/// device, but that describe this device and so travel in its own file: the
/// at-rest posture, the git executable, the microphone and its language, and
/// the notes list's last choices. A restore brings them back; the microphone
/// is never switched on by one ([`from_portable`]).
const DEVICE_KEYS: &[&str] = &[
    "sdk_encryption",
    "sync.git_path",
    "bots.wake_enabled",
    "bots.voice_locale",
    "notes.hide_service_files",
    "notes.include_private",
];

/// The wake phrase's switch: an open microphone is a person's tap, never a
/// sync's.
const WAKE_ENABLED_KEY: &str = "bots.wake_enabled";

const SYNC_CREDENTIAL_PREFIX: &str = "sync.credential_source.";
const BOTS_CREDENTIAL_PREFIX: &str = "bots.provider_credential_source.";

/// The one portable value of a credential-source row: this account's token.
/// Anything else — the keychain, another account — is the row's absence.
const ACCOUNT_CREDENTIAL: &str = "account";

/// The file `key` is synced in, or `None` when it never leaves this device —
/// session state, every other key family, and keys this build does not know.
/// The two credential-source families live in the device file, keyed by a
/// drive or provider reference there and by a local id here.
pub fn synced_file(key: &str) -> Option<SyncedFile> {
    let spec = keys::spec(key)?;
    if spec.family {
        return [SYNC_CREDENTIAL_PREFIX, BOTS_CREDENTIAL_PREFIX]
            .contains(&spec.key)
            .then_some(SyncedFile::Device);
    }
    if DEVICE_KEYS.contains(&spec.key) {
        return Some(SyncedFile::Device);
    }
    match spec.scope {
        Scope::UserGlobal => Some(SyncedFile::Shared),
        Scope::MachineLocal => Some(SyncedFile::Device),
        Scope::SessionState => None,
    }
}

/// Every exact key synced in `file`; the credential-source families are read
/// by prefix ([`stored_rows`]).
pub fn synced_keys(file: SyncedFile) -> impl Iterator<Item = &'static str> {
    keys::KEYS
        .iter()
        .map(|spec| spec.key)
        .filter(move |key| synced_file(key) == Some(file))
}

/// `<login>/settings.toml`.
pub fn shared_path(login: &str) -> String {
    format!("{login}/settings.toml")
}

/// `<login>/settings.<device>.toml`.
pub fn device_path(login: &str, device: &str) -> String {
    format!("{login}/settings.{device}.toml")
}

const TEMPLATE_SHARED: &str = "_template/settings.toml";

fn template_class(class: DeviceClass) -> String {
    format!("_template/settings/{}.toml", class.as_str())
}

const HEADER: &str = "# keeper keeps this file in step with your devices. Edit it freely; keeper merges it key by key.\n\
# Values here are preferences, not locks — keeper.toml is where a setting is pinned.\n";

/// One synced file's contents.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    /// Key → stored spelling (the string the `settings` table holds), with
    /// local identifiers already in their portable form.
    pub values: BTreeMap<String, String>,
    /// `[settings]` entries this build does not apply — an unknown key, a key
    /// of the other file, a value that does not fit its key's shape — kept
    /// verbatim so a newer device's file survives an older device's sync.
    pub preserved: BTreeMap<String, toml::Value>,
    /// Top-level items other than `[settings]`, kept verbatim for the same
    /// reason.
    pub other: toml::Table,
}

impl Values {
    /// Read a synced file. An `Err` is a sentence about a file keeper cannot
    /// read at all; the caller must leave that file alone rather than replace
    /// what a person is halfway through editing.
    pub fn parse(bytes: &[u8], file: SyncedFile) -> Result<Values, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned())?;
        let mut other: toml::Table = toml::from_str(text)
            .map_err(|error| format!("it is not valid TOML: {}", error.message()))?;
        let mut parsed = Values::default();
        match other.remove("settings") {
            None => {}
            Some(toml::Value::Table(table)) => {
                let mut flat = Vec::with_capacity(table.len());
                flatten("", table, &mut flat);
                for (key, value) in flat {
                    let stored = keys::spec(&key)
                        .filter(|_| synced_file(&key) == Some(file))
                        .and_then(|spec| spec.shape.coerce(&key, &value).ok())
                        .filter(|stored| portable_credential_row(&key, stored));
                    match stored {
                        Some(stored) => {
                            parsed.values.insert(key, stored);
                        }
                        None => {
                            parsed.preserved.insert(key, value);
                        }
                    }
                }
            }
            Some(_) => return Err("its settings entry is not a table".to_owned()),
        }
        parsed.other = other;
        Ok(parsed)
    }

    /// The file's bytes: the header, then everything in sorted key order, so an
    /// unchanged file renders identically on every device.
    pub fn render(&self) -> Result<String, String> {
        let mut settings = toml::Table::new();
        for (key, value) in &self.preserved {
            settings.insert(key.clone(), value.clone());
        }
        for (key, stored) in &self.values {
            let value = match keys::spec(key) {
                Some(spec) => spec.shape.to_toml(stored),
                None => toml::Value::String(stored.clone()),
            };
            settings.insert(key.clone(), value);
        }
        let mut document = self.other.clone();
        document.insert("settings".to_owned(), toml::Value::Table(settings));
        let body = toml::to_string(&document).map_err(|error| error.to_string())?;
        Ok(format!("{HEADER}{body}"))
    }

    /// The values alone, as the JSON object a settings base is stored as.
    pub fn to_base_json(&self) -> String {
        serde_json::to_string(&self.values).unwrap_or_else(|_| "{}".to_owned())
    }

    /// A stored settings base, or `None` when it does not parse.
    pub fn from_base_json(raw: &str) -> Option<Values> {
        serde_json::from_str::<BTreeMap<String, String>>(raw)
            .ok()
            .map(|values| Values {
                values,
                ..Values::default()
            })
    }
}

/// `[settings.recording] fps = 30` and `"recording.fps" = 30` are one key. No
/// settings value is a table, so the flattening is unambiguous.
fn flatten(prefix: &str, table: toml::Table, into: &mut Vec<(String, toml::Value)>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => flatten(&key, inner, into),
            other => into.push((key, other)),
        }
    }
}

/// Who wins a key both sides hold on a first sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstSync {
    /// The repository's value is pulled: the settings exist, so take them.
    RemoteWins,
    /// This device's value is kept: a template only fills gaps.
    LocalWins,
}

/// What one merge decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// The file as it should now read.
    pub file: Values,
    /// Table writes (`Some`) and deletes (`None`), in stored spellings with
    /// local ids. A value that does not resolve on this device is in
    /// [`file`](Merged::file) and not here.
    pub apply: Vec<(String, Option<String>)>,
    /// The file differs from the repository's (an absent file always does).
    pub changed: bool,
    /// The base to record once the file is pushed.
    pub base_if_pushed: Values,
    /// The base to record when the push failed: pulled keys advance, pushed
    /// keys stay at their old base so the next sync pushes them again.
    pub base_if_not_pushed: Values,
}

/// Merge one synced file (AD-320).
///
/// `remote` is the file at the fetched tip. `seed` stands in for it only on
/// this device's first sync (`base` is `None`); a file that disappeared after
/// this device synced it is recreated from this device, deleting nothing.
/// `local` is this device's rows for the file's keys in portable form, `base`
/// the file as this device last agreed it. `resolve` answers a portable
/// value's stored spelling on this device, or `None` when it names something
/// this device does not have — the shell passes [`from_portable`].
///
/// Later syncs are a three-way merge per key: an unchanged local value takes
/// the remote one, a changed local value is pushed, and when both changed the
/// device syncing now wins. A remote value this device cannot apply — it names
/// something missing here, or this build cannot read it — is kept in the file
/// and not applied, and both bases record this device's own value for the
/// key: nothing is dirty, no deletion is pushed, two devices that each cannot
/// resolve the other's value do not take turns overwriting it, and once the
/// value resolves here the unchanged local side takes it.
pub fn merge(
    remote: Option<&Values>,
    seed: Option<(Values, FirstSync)>,
    local: &Values,
    base: Option<&Values>,
    resolve: &dyn Fn(&str, &str) -> Option<String>,
) -> Merged {
    let (theirs, first) = match (remote, base) {
        (Some(remote), Some(_)) => (remote.clone(), None),
        (Some(remote), None) => (remote.clone(), Some(FirstSync::RemoteWins)),
        (None, Some(base)) => (
            Values {
                values: base.values.clone(),
                ..Values::default()
            },
            None,
        ),
        (None, None) => match seed {
            Some((seeded, precedence)) => (seeded, Some(precedence)),
            None => (Values::default(), Some(FirstSync::LocalWins)),
        },
    };
    let no_base = BTreeMap::new();
    let base_values = base.map_or(&no_base, |base| &base.values);
    let keys: BTreeSet<&String> = theirs
        .values
        .keys()
        .chain(local.values.keys())
        .chain(base_values.keys())
        .collect();

    let mut file = Values {
        values: BTreeMap::new(),
        preserved: theirs.preserved.clone(),
        other: theirs.other.clone(),
    };
    let mut apply = Vec::new();
    let mut base_if_pushed = BTreeMap::new();
    let mut base_if_not_pushed = BTreeMap::new();
    for key in keys {
        let r = theirs.values.get(key);
        // A value this build cannot read sits in `preserved`: it is not an
        // absence, so it must never read as a deletion.
        let unreadable = r.is_none() && theirs.preserved.contains_key(key.as_str());
        let b = base_values.get(key);
        let l = local.values.get(key);
        let take_remote = match first {
            Some(FirstSync::RemoteWins) => r.is_some() || unreadable,
            Some(FirstSync::LocalWins) => l.is_none(),
            None => l == b,
        };
        let (mut pushed_base, mut kept_base) = if take_remote { (r, r) } else { (l, b) };
        if take_remote {
            if let Some(value) = r {
                file.values.insert(key.clone(), value.clone());
            }
        } else {
            file.preserved.remove(key.as_str());
            if let Some(value) = l {
                file.values.insert(key.clone(), value.clone());
            }
        }
        if take_remote && (unreadable || r != l) {
            match r.map(|portable| resolve(key, portable)) {
                Some(Some(stored)) => apply.push((key.clone(), Some(stored))),
                None if !unreadable => apply.push((key.clone(), None)),
                // Not applied: this device keeps its own value, and recording
                // that as the base keeps it from being pushed over a value
                // only another device (or another build) can use.
                _ => {
                    pushed_base = l;
                    kept_base = l;
                }
            }
        }
        if let Some(value) = pushed_base {
            base_if_pushed.insert(key.clone(), value.clone());
        }
        if let Some(value) = kept_base {
            base_if_not_pushed.insert(key.clone(), value.clone());
        }
    }
    let changed = remote != Some(&file);
    Merged {
        file,
        apply,
        changed,
        base_if_pushed: Values {
            values: base_if_pushed,
            ..Values::default()
        },
        base_if_not_pushed: Values {
            values: base_if_not_pushed,
            ..Values::default()
        },
    }
}

/// The seed for an absent `settings.<me>.toml` (AD-322): the file of the
/// same-class device that changed it most recently (`last_change` answers a
/// repo-relative path's newest commit time; `None` is oldest, ties go to the
/// first slug), taken as the settings; otherwise the class template, filling
/// gaps only; otherwise nothing. A file keeper cannot read is skipped.
pub fn seed_device(
    files: &dyn RepoFiles,
    login: &str,
    class: DeviceClass,
    me: &str,
    last_change: &dyn Fn(&str) -> Option<i64>,
) -> Option<(Values, FirstSync)> {
    let mut candidates: Vec<(Option<i64>, String, Values)> = layout::devices(files, login)
        .into_iter()
        .filter(|device| device.slug != me && device.class == Some(class))
        .filter_map(|device| {
            let rel = device_path(login, &device.slug);
            if files.is_non_regular(&rel) {
                return None;
            }
            let values = Values::parse(&files.read(&rel)?, SyncedFile::Device).ok()?;
            Some((last_change(&rel), device.slug, values))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if let Some((_, _, values)) = candidates.into_iter().next() {
        return Some((values, FirstSync::RemoteWins));
    }
    let template = Values::parse(&files.read(&template_class(class))?, SyncedFile::Device).ok()?;
    Some((template, FirstSync::LocalWins))
}

/// The seed for an absent `settings.toml`: `_template/settings.toml`, filling
/// gaps only, or nothing.
pub fn seed_shared(files: &dyn RepoFiles) -> Option<(Values, FirstSync)> {
    let template = Values::parse(&files.read(TEMPLATE_SHARED)?, SyncedFile::Shared).ok()?;
    Some((template, FirstSync::LocalWins))
}

/// A git remote as one spelling per repository: scheme and host lower-cased,
/// a trailing `/` and `.git` removed, and no credential. An http(s) URL loses
/// its whole userinfo, because its user field is routinely the token. An
/// `ssh` URL or an scp-style `git@host:path` keeps its user — that is how the
/// repository is reached — and loses only a password. A local path only loses
/// its `.git`.
pub fn normalize_remote(url: &str) -> String {
    let url = url.trim();
    let trim = |path: &str| -> String {
        let path = path.trim_end_matches('/');
        path.strip_suffix(".git")
            .unwrap_or(path)
            .trim_end_matches('/')
            .to_owned()
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return match scp_parts(url) {
            Some((Some(user), host_path)) => {
                format!("{}@{}", without_password(user), trim(host_path))
            }
            _ => trim(url),
        };
    };
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (user, host) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, authority),
    };
    let host = host.to_ascii_lowercase();
    match user {
        Some(user) if is_ssh_scheme(&scheme) => {
            format!("{scheme}://{}@{host}{}", without_password(user), trim(path))
        }
        _ => format!("{scheme}://{host}{}", trim(path)),
    }
}

fn is_ssh_scheme(scheme: &str) -> bool {
    scheme == "ssh" || scheme.starts_with("git+ssh") || scheme.starts_with("ssh+")
}

fn without_password(user: &str) -> &str {
    user.split_once(':').map_or(user, |(name, _)| name)
}

/// Whether a remote without a scheme is an scp-style `[user@]host:path`
/// rather than a filesystem path.
pub(super) fn is_scp_remote(url: &str) -> bool {
    scp_parts(url).is_some()
}

/// An scp-style remote `[user@]host:path` as `(user, "host:path")`; `None` for
/// anything else, including a Windows drive path (`C:\x`, a one-letter host).
fn scp_parts(url: &str) -> Option<(Option<&str>, &str)> {
    let (user, host_path) = match url.split_once('@') {
        Some((user, rest)) if !user.contains('/') => (Some(user), rest),
        _ => (None, url),
    };
    let (host, _) = host_path.split_once(':')?;
    (host.len() > 1 && !host.contains(['/', '\\'])).then_some((user, host_path))
}

/// Whether a remote carries a credential: any userinfo in an http(s) (or
/// other non-ssh) URL, a password in an ssh or scp-style one.
pub fn remote_has_secret(url: &str) -> bool {
    let url = url.trim();
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
            match authority.rsplit_once('@') {
                Some((user, _)) => {
                    !is_ssh_scheme(&scheme.to_ascii_lowercase()) || user.contains(':')
                }
                None => false,
            }
        }
        None => matches!(scp_parts(url), Some((Some(user), _)) if user.contains(':')),
    }
}

/// A provider's base URL in the one spelling the bots store keeps
/// ([`bots::parse_base_url`]); a string that does not parse is only trimmed.
pub fn normalize_base_url(url: &str) -> String {
    bots::parse_base_url(url).map_or_else(
        |_| url.trim().trim_end_matches('/').to_owned(),
        |parsed| parsed.normalized,
    )
}

/// A drive as every device names it: its remote, its branch and its name, so
/// two drives of one repository (`tgdrive`, `tgdrive-light`) stay two.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriveRef {
    remote_url: String,
    branch: String,
    name: Option<String>,
}

/// Between a drive reference's branch and its name: a character no git
/// branch name may contain, so neither can be mistaken for the other.
const NAME_SEPARATOR: char = '^';

impl DriveRef {
    /// A drive by its remote and branch alone.
    pub fn new(remote_url: &str, branch: &str) -> Self {
        DriveRef {
            remote_url: normalize_remote(remote_url),
            branch: branch.trim().to_owned(),
            name: None,
        }
    }

    /// A drive by its remote, branch and name; a blank name names nothing.
    pub fn named(remote_url: &str, branch: &str, name: &str) -> Self {
        let name = name.trim();
        DriveRef {
            name: (!name.is_empty()).then(|| name.to_owned()),
            ..DriveRef::new(remote_url, branch)
        }
    }

    /// `drive:<normalized remote>#<branch>`, then `^<name>` when it is named.
    pub fn reference(&self) -> String {
        match &self.name {
            Some(name) => format!(
                "drive:{}#{}{NAME_SEPARATOR}{name}",
                self.remote_url, self.branch
            ),
            None => format!("drive:{}#{}", self.remote_url, self.branch),
        }
    }

    /// Read a reference back: the branch runs from the last `#` to the first
    /// `^`, and the name is the rest. `None` for anything else.
    pub fn parse(reference: &str) -> Option<DriveRef> {
        let (remote, rest) = reference.strip_prefix("drive:")?.rsplit_once('#')?;
        Some(match rest.split_once(NAME_SEPARATOR) {
            Some((branch, name)) => DriveRef::named(remote, branch, name),
            None => DriveRef::new(remote, rest),
        })
    }

    /// The same remote and branch, whatever the name.
    pub(super) fn same_repository(&self, other: &DriveRef) -> bool {
        self.remote_url == other.remote_url && self.branch == other.branch
    }
}

/// `scheme://host[:port]` of a URL, the port only when it is not the
/// scheme's default; `None` for anything without a host (an scp remote, a
/// path).
pub fn url_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    parsed.host_str()?;
    Some(parsed.origin().ascii_serialization()).filter(|origin| origin != "null")
}

/// A bot provider as every device names it: its kind and base URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderRef {
    kind: String,
    base_url: String,
}

impl ProviderRef {
    pub fn new(kind: &str, base_url: &str) -> Self {
        ProviderRef {
            kind: kind.trim().to_owned(),
            base_url: normalize_base_url(base_url),
        }
    }

    /// `provider:<kind>:<normalized base URL>`.
    pub fn reference(&self) -> String {
        format!("provider:{}:{}", self.kind, self.base_url)
    }

    /// The base URL's origin ([`url_origin`]).
    pub fn origin(&self) -> Option<String> {
        url_origin(&self.base_url)
    }

    /// `bot:<kind>:<normalized base URL>#<target>`.
    pub fn bot_reference(&self, target: &str) -> String {
        format!("bot:{}:{}#{}", self.kind, self.base_url, target)
    }
}

/// One of this device's drives, as the shell reads it from `sync.db`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalDrive {
    pub profile_id: String,
    pub remote_url: String,
    pub branch: String,
    pub name: String,
    /// The folder it syncs.
    pub local_path: String,
}

impl LocalDrive {
    fn key(&self) -> DriveRef {
        DriveRef::named(&self.remote_url, &self.branch, &self.name)
    }
}

/// This device's drives, providers and bots, by local id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub drives: Vec<LocalDrive>,
    /// Whether `drives` is this device's real drive list. When it could not
    /// be read, no drive reference is translated either way, so a missing
    /// list is never mistaken for drives the person removed.
    pub drives_known: bool,
    /// `(provider id, provider)`.
    pub providers: Vec<(String, ProviderRef)>,
    /// `(bot id, provider id, target)`.
    pub bots: Vec<(String, String, String)>,
    /// The configured account. A credential-source row travels only when it
    /// is bound to this account, and comes back bound to it.
    pub account_id: Option<String>,
    /// The account descriptor's issuer, repository and forge origins
    /// ([`url_origin`]). A pulled credential source binds the account's
    /// token only to a drive or provider at one of them.
    pub trusted_origins: Vec<String>,
    /// `(forge source id, its web_base origin)` of every source this device
    /// can get a token from ([`crate::forges::token_origins`]). A pulled
    /// `forge:<id>` binds a drive to that source only when the source is
    /// here and the drive's remote is at its origin; otherwise it stays
    /// unapplied, and the drive keeps the credential that works.
    pub forge_origins: Vec<(String, String)>,
}

impl Catalog {
    /// `drives` from the shell (keeper-core cannot see `sync.db`), `None`
    /// when the drive list could not be read; providers and bots from
    /// `keeper.db`. A provider of a kind this build does not speak has no base
    /// URL to name it by, and is left out. The shell sets
    /// [`account_id`](Catalog::account_id) and
    /// [`trusted_origins`](Catalog::trusted_origins).
    pub fn with_bots(
        data_dir: &Path,
        drives: Option<Vec<LocalDrive>>,
    ) -> Result<Catalog, CoreError> {
        let providers = store::list_providers(data_dir)?
            .rows
            .into_iter()
            .map(|row| {
                let provider = row.provider;
                let named = ProviderRef::new(provider.kind.as_registry_str(), &provider.base_url);
                (provider.id, named)
            })
            .collect();
        let bots = store::list_bots(data_dir)?
            .into_iter()
            .map(|bot| (bot.id, bot.provider_id, bot.target))
            .collect();
        Ok(Catalog {
            drives_known: drives.is_some(),
            drives: drives.unwrap_or_default(),
            providers,
            bots,
            account_id: None,
            trusted_origins: Vec::new(),
            forge_origins: Vec::new(),
        })
    }

    fn drive(&self, profile_id: &str) -> Option<&LocalDrive> {
        self.drives
            .iter()
            .find(|drive| drive.profile_id == profile_id)
    }

    /// How every device names drive `profile_id`: its remote, branch and name.
    pub fn drive_reference(&self, profile_id: &str) -> Option<String> {
        Some(self.drive(profile_id)?.key().reference())
    }

    /// The drive here that `reference` names. A named reference: the drive
    /// of that name on its remote and branch. An unnamed one (written before
    /// names travelled): the only drive on its remote and branch; with more
    /// than one it names none of them.
    pub fn resolve_drive(&self, reference: &str) -> Option<&str> {
        let wanted = DriveRef::parse(reference)?;
        if wanted.name.is_some() {
            return self
                .drives
                .iter()
                .find(|drive| drive.key() == wanted)
                .map(|drive| drive.profile_id.as_str());
        }
        let mut matches = self
            .drives
            .iter()
            .filter(|drive| drive.key().same_repository(&wanted));
        match (matches.next(), matches.next()) {
            (Some(only), None) => Some(&only.profile_id),
            _ => None,
        }
    }

    fn trusted(&self, origin: Option<String>) -> bool {
        origin.is_some_and(|origin| self.trusted_origins.contains(&origin))
    }

    fn provider(&self, id: &str) -> Option<&ProviderRef> {
        self.providers
            .iter()
            .find(|(provider_id, _)| provider_id == id)
            .map(|(_, provider)| provider)
    }
}

/// Keys holding a sync-profile id.
const DRIVE_KEYS: &[&str] = &[
    "notes.active_vault",
    "tasks.ledger_vault",
    "recording.destination_profile_id",
];
const VOICE_TARGET_KEY: &str = "bots.voice_target";
const EMBEDDING_MODEL_KEY: &str = "notes.embedding_model";

/// A credential-source key split into its family prefix and the rest: a
/// local id in the table, a drive or provider reference in the file.
fn credential_key(key: &str) -> Option<(&'static str, &str)> {
    [SYNC_CREDENTIAL_PREFIX, BOTS_CREDENTIAL_PREFIX]
        .into_iter()
        .find_map(|prefix| {
            let rest = key.strip_prefix(prefix)?;
            (!rest.is_empty()).then_some((prefix, rest))
        })
}

/// Whether a file row is one this build applies: every row but a
/// credential-source one, which must name a drive or provider by reference
/// and hold `account` — or, for a drive, `forge:<source id>`.
fn portable_credential_row(key: &str, stored: &str) -> bool {
    match credential_key(key) {
        None => true,
        Some((prefix, reference)) => {
            if prefix == SYNC_CREDENTIAL_PREFIX {
                reference.starts_with("drive:")
                    && (stored == ACCOUNT_CREDENTIAL
                        || crate::forges::forge_credential_id(stored).is_some())
            } else {
                stored == ACCOUNT_CREDENTIAL && reference.starts_with("provider:")
            }
        }
    }
}

/// A credential-source row as it travels: keyed by the drive's or
/// provider's reference, holding `account` — or a drive's `forge:<id>`,
/// verbatim. `None` when the row is not bound to the configured account (it
/// means the keychain, which is the row's absence) or names nothing this
/// device can describe.
fn portable_credential(key: &str, stored: &str, catalog: &Catalog) -> Option<(String, String)> {
    let (prefix, id) = credential_key(key)?;
    if prefix == SYNC_CREDENTIAL_PREFIX && crate::forges::forge_credential_id(stored).is_some() {
        let reference = catalog.drive_reference(id)?;
        return Some((format!("{prefix}{reference}"), stored.to_owned()));
    }
    let account = catalog.account_id.as_deref()?;
    if stored != registry::credential_source_value(account) {
        return None;
    }
    let reference = if prefix == SYNC_CREDENTIAL_PREFIX {
        catalog.drive_reference(id)?
    } else {
        catalog.provider(id)?.reference()
    };
    Some((
        format!("{prefix}{reference}"),
        ACCOUNT_CREDENTIAL.to_owned(),
    ))
}

/// The key a travelling key is stored under here: a credential-source key
/// names its drive or provider by local id, every other key is itself. `None`
/// when the drive or provider is not on this device. The shell writes a
/// merge's [`apply`](Merged::apply) through this.
pub fn stored_key(key: &str, catalog: &Catalog) -> Option<String> {
    let Some((prefix, reference)) = credential_key(key) else {
        return Some(key.to_owned());
    };
    let id = if prefix == SYNC_CREDENTIAL_PREFIX {
        if !catalog.drives_known {
            return None;
        }
        catalog.resolve_drive(reference)?
    } else {
        catalog
            .providers
            .iter()
            .find(|(_, provider)| provider.reference() == reference)
            .map(|(id, _)| id.as_str())?
    };
    Some(format!("{prefix}{id}"))
}

/// A stored value in the form it travels in. `None` when it names a drive,
/// bot or provider this device does not know ([`local_values`] then takes the
/// base's value). A blank value (a cleared choice) and every key without a
/// local id pass through unchanged. Credential-source rows change their key
/// as well and are translated by [`local_values`].
pub fn to_portable(key: &str, stored: &str, catalog: &Catalog) -> Option<String> {
    let id = stored.trim();
    if id.is_empty() {
        return Some(stored.to_owned());
    }
    if DRIVE_KEYS.contains(&key) {
        return catalog.drive_reference(id);
    }
    match key {
        VOICE_TARGET_KEY => {
            let (_, provider_id, target) = catalog.bots.iter().find(|(bot_id, ..)| bot_id == id)?;
            Some(catalog.provider(provider_id)?.bot_reference(target))
        }
        EMBEDDING_MODEL_KEY => {
            let model: EmbeddingModel = serde_json::from_str(stored).ok()?;
            let provider = catalog.provider(&model.provider)?;
            serde_json::to_string(&EmbeddingModel {
                provider: provider.reference(),
                model: model.model,
            })
            .ok()
        }
        _ => Some(stored.to_owned()),
    }
}

/// A travelling value as this device stores it. `None` — kept in the file and
/// not applied — when its reference names nothing here; when it is an
/// absolute path that does not exist here, as a directory or a file (the git
/// executable: a regular executable file outside every drive's folder); when
/// it would switch the wake phrase on; or when it is a credential source for
/// a drive or provider this device lacks, one that is not at a trusted
/// origin of the account, or with no account configured. A credential source
/// comes back as `account:<this account>`; its key goes through
/// [`stored_key`].
pub fn from_portable(key: &str, portable: &str, catalog: &Catalog) -> Option<String> {
    if let Some((prefix, _)) = credential_key(key) {
        let stored = stored_key(key, catalog)?;
        let id = &stored[prefix.len()..];
        // The account's token goes only where the account itself lives: a
        // repository file must not send it to a host the person never chose.
        let origin = if prefix == SYNC_CREDENTIAL_PREFIX {
            url_origin(&catalog.drive(id)?.remote_url)
        } else {
            catalog.provider(id)?.origin()
        };
        // A forge connection likewise, only to a drive on that forge.
        if let Some(source) = crate::forges::forge_credential_id(portable) {
            if prefix != SYNC_CREDENTIAL_PREFIX {
                return None;
            }
            let forge_origin = catalog
                .forge_origins
                .iter()
                .find(|(id, _)| id == source)
                .map(|(_, origin)| origin);
            return (origin.is_some() && origin.as_ref() == forge_origin)
                .then(|| portable.to_owned());
        }
        if !catalog.trusted(origin) {
            return None;
        }
        let account = catalog.account_id.as_deref().filter(|id| !id.is_empty())?;
        return (portable == ACCOUNT_CREDENTIAL)
            .then(|| registry::credential_source_value(account));
    }
    if portable.trim().is_empty() {
        return Some(portable.to_owned());
    }
    if DRIVE_KEYS.contains(&key) {
        if !catalog.drives_known {
            return None;
        }
        return catalog.resolve_drive(portable).map(str::to_owned);
    }
    match key {
        // Only a person's tap opens the microphone; "off" travels freely.
        WAKE_ENABLED_KEY if portable == "1" => None,
        VOICE_TARGET_KEY => catalog
            .bots
            .iter()
            .find(|(_, provider_id, target)| {
                catalog
                    .provider(provider_id)
                    .is_some_and(|provider| provider.bot_reference(target) == portable)
            })
            .map(|(bot_id, ..)| bot_id.clone()),
        EMBEDDING_MODEL_KEY => {
            let model: EmbeddingModel = serde_json::from_str(portable).ok()?;
            let (provider_id, _) = catalog
                .providers
                .iter()
                .find(|(_, provider)| provider.reference() == model.provider)?;
            serde_json::to_string(&EmbeddingModel {
                provider: provider_id.clone(),
                model: model.model,
            })
            .ok()
        }
        GIT_PATH_KEY => git_executable(portable, catalog).then(|| portable.to_owned()),
        _ if keys::spec(key).is_some_and(|spec| spec.shape == Shape::AbsolutePath) => {
            let path = Path::new(portable);
            (path.is_dir() || path.is_file()).then(|| portable.to_owned())
        }
        _ => Some(portable.to_owned()),
    }
}

const GIT_PATH_KEY: &str = "sync.git_path";

/// Whether a pulled git path may be run here: a regular executable file
/// (through a link, as Homebrew installs it), and neither the path nor what it
/// points at inside any drive's folder, where anyone who can push to that
/// drive could put one.
fn git_executable(portable: &str, catalog: &Catalog) -> bool {
    let path = Path::new(portable);
    let (Ok(meta), Ok(real)) = (std::fs::metadata(path), std::fs::canonicalize(path)) else {
        return false;
    };
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = true;
    if !meta.is_file() || !executable || !catalog.drives_known {
        return false;
    }
    !catalog
        .drives
        .iter()
        .filter(|drive| !drive.local_path.is_empty())
        .any(|drive| {
            let folder = Path::new(&drive.local_path);
            let real_folder =
                std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
            path.starts_with(folder) || real.starts_with(&real_folder)
        })
}

/// Whether this device's file asks for the wake phrase while this device has
/// it off (`stored_wake` is its `bots.wake_enabled` row): the account view
/// then offers to turn listening on, and nothing turns it on by itself.
pub fn listening_off(device_file: &Values, stored_wake: Option<&str>) -> bool {
    device_file.values.get(WAKE_ENABLED_KEY).map(String::as_str) == Some("1")
        && stored_wake != Some("1")
}

/// The `settings` table's own rows for every synced key, both files, the
/// credential-source families included — the snapshot a merge's local side
/// is read from, keyed as the table keys them.
pub fn stored_rows(data_dir: &Path) -> Result<BTreeMap<String, String>, CoreError> {
    let keys: Vec<&str> = synced_keys(SyncedFile::Shared)
        .chain(synced_keys(SyncedFile::Device))
        .collect();
    let mut rows = registry::stored_settings(data_dir, &keys)?;
    for prefix in [SYNC_CREDENTIAL_PREFIX, BOTS_CREDENTIAL_PREFIX] {
        rows.extend(registry::stored_settings_by_prefix(data_dir, prefix)?);
    }
    Ok(rows)
}

/// This device's side of one synced file: the `settings` table's own rows (not
/// what the layer files make them read as) for the file's keys, in portable
/// form. `base` is the file as this device last synced it.
pub fn local_values(
    data_dir: &Path,
    file: SyncedFile,
    catalog: &Catalog,
    base: Option<&Values>,
) -> Result<Values, CoreError> {
    let keys: Vec<&str> = synced_keys(file).collect();
    let mut rows = registry::stored_settings(data_dir, &keys)?;
    if file == SyncedFile::Device {
        for prefix in [SYNC_CREDENTIAL_PREFIX, BOTS_CREDENTIAL_PREFIX] {
            rows.extend(registry::stored_settings_by_prefix(data_dir, prefix)?);
        }
    }
    Ok(local_from_rows(rows, catalog, base))
}

/// A row naming something this device cannot describe right now — a removed
/// bot or provider, or any drive while the drive list is unreadable — is not
/// the person deleting the key: it reads as the base's value, so it is neither
/// pushed as a deletion nor as a change. A credential-source row travels under
/// its drive's or provider's reference, and only when bound to this account.
fn local_from_rows(
    rows: BTreeMap<String, String>,
    catalog: &Catalog,
    base: Option<&Values>,
) -> Values {
    let based = |key: &str| base.and_then(|base| base.values.get(key)).cloned();
    let mut values = BTreeMap::new();
    for (key, stored) in rows {
        if let Some((prefix, _)) = credential_key(&key) {
            if prefix == SYNC_CREDENTIAL_PREFIX && !catalog.drives_known {
                continue;
            }
            if let Some((portable_key, value)) = portable_credential(&key, &stored, catalog) {
                values.insert(portable_key, value);
            }
            continue;
        }
        if !catalog.drives_known && DRIVE_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let Some(portable) = to_portable(&key, &stored, catalog).or_else(|| based(&key)) {
            values.insert(key, portable);
        }
    }
    if !catalog.drives_known {
        let drive_rows = base
            .into_iter()
            .flat_map(|base| &base.values)
            .filter(|(key, _)| {
                DRIVE_KEYS.contains(&key.as_str()) || key.starts_with(SYNC_CREDENTIAL_PREFIX)
            });
        for (key, value) in drive_rows {
            values.insert(key.clone(), value.clone());
        }
    }
    Values {
        values,
        ..Values::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> Values {
        Values {
            values: pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Values::default()
        }
    }

    fn applied(pairs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.map(str::to_owned)))
            .collect()
    }

    /// Everything resolves to itself.
    fn verbatim(_key: &str, value: &str) -> Option<String> {
        Some(value.to_owned())
    }

    #[test]
    fn keys_are_synced_by_scope_and_device_linked_ones_in_the_device_file() {
        for key in [
            "recording.codec",
            "notify.previews_enabled",
            "notes.embedding_model",
            "bots.voice_target",
        ] {
            assert_eq!(synced_file(key), Some(SyncedFile::Shared), "{key}");
        }
        for key in [
            "hotkey.global",
            "notes.active_vault",
            "tasks.ledger_vault",
            "recording.destination_dir",
            "recording.destination_profile_id",
            "sdk_encryption",
            "sync.git_path",
            "bots.wake_enabled",
            "bots.voice_locale",
            "notes.hide_service_files",
            "notes.include_private",
            "sync.credential_source.01P",
            "sync.credential_source.drive:https://github.com/acme/notes#main",
            "bots.provider_credential_source.01PROV",
        ] {
            assert_eq!(synced_file(key), Some(SyncedFile::Device), "{key}");
        }
        for key in [
            "ui.first_run_setup_skipped",
            "notes.read.01ABC",
            "notes.capture_draft.main",
            "notes.pristine.01ABC",
            "account.acme.last_synced_ms",
            "sync.credential_source.",
            "recordng.fps",
        ] {
            assert_eq!(synced_file(key), None, "{key}");
        }
        assert!(
            synced_keys(SyncedFile::Device).all(|key| !key.ends_with('.')),
            "families are read by prefix, never as a key"
        );
    }

    #[test]
    fn a_pulled_wake_phrase_switch_turns_listening_off_and_never_on() {
        let catalog = Catalog::default();
        let resolve = |k: &str, v: &str| from_portable(k, v, &catalog);

        let on = values(&[(WAKE_ENABLED_KEY, "1")]);
        let merged = merge(
            Some(&on),
            None,
            &values(&[(WAKE_ENABLED_KEY, "0")]),
            None,
            &resolve,
        );
        assert!(merged.apply.is_empty(), "\"1\" is never applied");
        assert_eq!(merged.file, on, "it stays in the file");
        assert_eq!(
            merged.base_if_pushed,
            values(&[(WAKE_ENABLED_KEY, "0")]),
            "the base records this device's own value"
        );
        assert!(listening_off(&merged.file, Some("0")));
        assert!(listening_off(&merged.file, None));
        assert!(!listening_off(&merged.file, Some("1")));

        let off = values(&[(WAKE_ENABLED_KEY, "0")]);
        let local = values(&[(WAKE_ENABLED_KEY, "1")]);
        let merged = merge(Some(&off), None, &local, Some(&local), &resolve);
        assert_eq!(merged.apply, applied(&[(WAKE_ENABLED_KEY, Some("0"))]));
        assert!(!listening_off(&merged.file, Some("0")));
    }

    #[test]
    fn a_path_is_applied_only_where_it_exists_and_git_only_as_an_executable_outside_drives() {
        let dir = std::env::temp_dir().join(format!("keeper-a1-{}", std::process::id()));
        let drive = dir.join("drive");
        std::fs::create_dir_all(&drive).expect("dirs");
        let catalog = Catalog {
            drives_known: true,
            drives: vec![local("01D", "https://x/y", "y", &drive.to_string_lossy())],
            ..Catalog::default()
        };
        let file = |path: &Path, mode: u32| {
            std::fs::write(path, b"#!/bin/sh\n").expect("file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                    .expect("mode");
            }
            path.to_string_lossy().into_owned()
        };
        let git = file(&dir.join("git"), 0o755);
        let git_path =
            |path: &str, catalog: &Catalog| from_portable("sync.git_path", path, catalog);
        assert_eq!(git_path(&git, &catalog).as_deref(), Some(git.as_str()));
        assert_eq!(
            git_path(&git, &Catalog::default()),
            None,
            "an unreadable drive list cannot vouch for it"
        );
        #[cfg(unix)]
        assert_eq!(
            git_path(&file(&dir.join("plain"), 0o644), &catalog),
            None,
            "not executable"
        );
        assert_eq!(
            git_path(&file(&drive.join("git"), 0o755), &catalog),
            None,
            "inside a drive anyone with push access could have put it"
        );
        assert_eq!(git_path(&dir.to_string_lossy(), &catalog), None, "a folder");
        let missing = dir.join("no-git").to_string_lossy().into_owned();
        assert_eq!(git_path(&missing, &catalog), None);
        // Other paths still take a folder or a file.
        let here = dir.to_string_lossy();
        assert_eq!(
            from_portable("recording.destination_dir", &here, &catalog).as_deref(),
            Some(here.as_ref())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn account_catalog() -> Catalog {
        Catalog {
            account_id: Some("acme".to_owned()),
            trusted_origins: vec![
                "https://github.com".to_owned(),
                "http://localhost:11434".to_owned(),
            ],
            ..catalog()
        }
    }

    const DRIVE_CRED: &str =
        "sync.credential_source.drive:https://github.com/acme/notes#main^Notes";
    const PROVIDER_CRED: &str =
        "bots.provider_credential_source.provider:ollama:http://localhost:11434";

    #[test]
    fn credential_sources_travel_by_reference_and_come_back_bound_to_this_account() {
        let catalog = account_catalog();
        let rows: BTreeMap<String, String> = [
            ("sync.credential_source.01DRIVE", "account:acme"),
            ("bots.provider_credential_source.01PROV", "account:acme"),
            // Bound to another account: the keychain, which is no row.
            ("sync.credential_source.01OLD", "account:globex"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let local = local_from_rows(rows, &catalog, None);
        assert_eq!(
            local,
            values(&[(PROVIDER_CRED, "account"), (DRIVE_CRED, "account")])
        );

        for (portable, stored) in [
            (DRIVE_CRED, "sync.credential_source.01DRIVE"),
            (PROVIDER_CRED, "bots.provider_credential_source.01PROV"),
        ] {
            assert_eq!(stored_key(portable, &catalog).as_deref(), Some(stored));
            assert_eq!(
                from_portable(portable, "account", &catalog).as_deref(),
                Some("account:acme")
            );
        }
        assert_eq!(
            stored_key("recording.codec", &catalog).as_deref(),
            Some("recording.codec")
        );

        // A drive this device does not have: kept in the file, not applied,
        // and the base records this device's own (absent) value.
        let elsewhere = "sync.credential_source.drive:https://github.com/acme/other#main";
        assert_eq!(stored_key(elsewhere, &catalog), None);
        assert_eq!(from_portable(elsewhere, "account", &catalog), None);
        let remote = values(&[(elsewhere, "account"), (DRIVE_CRED, "account")]);
        let resolve = |k: &str, v: &str| from_portable(k, v, &catalog);
        let merged = merge(Some(&remote), None, &Values::default(), None, &resolve);
        assert_eq!(merged.file, remote);
        assert_eq!(
            merged.apply,
            applied(&[(DRIVE_CRED, Some("account:acme"))]),
            "only the drive here is bound"
        );
        assert_eq!(merged.base_if_pushed, values(&[(DRIVE_CRED, "account")]));

        // Without an account nothing is bound.
        let signed_out = Catalog {
            account_id: None,
            ..account_catalog()
        };
        assert_eq!(from_portable(DRIVE_CRED, "account", &signed_out), None);

        // A drive or provider away from the account's own hosts is never
        // bound from a file, though the person may still choose it here.
        for untrusted in [
            vec!["http://localhost:11434".to_owned()],
            vec![
                "https://github.com:8443".to_owned(),
                "http://localhost:11434".to_owned(),
            ],
        ] {
            let catalog = Catalog {
                trusted_origins: untrusted,
                ..account_catalog()
            };
            assert_eq!(from_portable(DRIVE_CRED, "account", &catalog), None);
            assert_eq!(
                from_portable(PROVIDER_CRED, "account", &catalog).as_deref(),
                Some("account:acme")
            );
        }
        let catalog = Catalog {
            trusted_origins: vec!["https://github.com".to_owned()],
            ..account_catalog()
        };
        assert_eq!(from_portable(PROVIDER_CRED, "account", &catalog), None);
        let catalog = account_catalog();
        let other_account: BTreeMap<String, String> = [(
            "bots.provider_credential_source.01PROV".to_owned(),
            "account:globex".to_owned(),
        )]
        .into();
        assert!(
            local_from_rows(other_account, &catalog, None)
                .values
                .is_empty(),
            "a row bound to another account is the keychain: nothing travels"
        );

        // A hand-written value other than `account` is kept, never applied.
        let text = format!("[settings]\n\"{DRIVE_CRED}\" = \"keychain\"\n");
        let parsed = Values::parse(text.as_bytes(), SyncedFile::Device).expect("parses");
        assert!(parsed.values.is_empty());
        assert!(parsed.preserved.contains_key(DRIVE_CRED));
    }

    fn two_drives() -> Catalog {
        let remote = "https://git.acme.dev/tg/tgdrive.git";
        Catalog {
            drives_known: true,
            drives: vec![
                local("01FULL", remote, "tgdrive", "/Users/tg/tgdrive"),
                local(
                    "01LIGHT",
                    remote,
                    "tgdrive-light",
                    "/Users/tg/tgdrive-light",
                ),
                local(
                    "01SOLO",
                    "https://git.acme.dev/tg/solo",
                    "solo",
                    "/Users/tg/solo",
                ),
            ],
            ..Catalog::default()
        }
    }

    #[test]
    fn two_drives_of_one_repository_are_told_apart_by_name() {
        let catalog = two_drives();
        let full = catalog.drive_reference("01FULL").expect("full");
        let light = catalog.drive_reference("01LIGHT").expect("light");
        assert_eq!(full, "drive:https://git.acme.dev/tg/tgdrive#main^tgdrive");
        assert_eq!(
            light,
            "drive:https://git.acme.dev/tg/tgdrive#main^tgdrive-light"
        );
        assert_eq!(
            catalog.drive_reference("01SOLO").as_deref(),
            Some("drive:https://git.acme.dev/tg/solo#main^solo"),
            "every reference carries the name"
        );

        for key in DRIVE_KEYS {
            let resolve = |reference: &str| from_portable(key, reference, &catalog);
            assert_eq!(resolve(&full).as_deref(), Some("01FULL"));
            assert_eq!(resolve(&light).as_deref(), Some("01LIGHT"));
            assert_eq!(
                resolve("drive:https://git.acme.dev/tg/tgdrive#main"),
                None,
                "an unnamed reference to two drives names neither"
            );
            assert_eq!(
                resolve("drive:https://git.acme.dev/tg/solo#main").as_deref(),
                Some("01SOLO"),
                "an unnamed reference to the only drive on its repository resolves"
            );
            assert_eq!(
                resolve("drive:https://git.acme.dev/tg/tgdrive#main^gone"),
                None
            );
        }
    }

    #[test]
    fn a_branch_is_never_read_as_a_name() {
        let catalog = Catalog {
            drives_known: true,
            drives: vec![
                LocalDrive {
                    branch: "feature@x".to_owned(),
                    ..local("01AT", "https://git.acme.dev/r", "r", "/r1")
                },
                LocalDrive {
                    branch: "feature".to_owned(),
                    ..local("01X", "https://git.acme.dev/r", "x", "/r2")
                },
            ],
            ..Catalog::default()
        };
        let at = catalog.drive_reference("01AT").expect("at");
        assert_eq!(at, "drive:https://git.acme.dev/r#feature@x^r");
        assert_eq!(catalog.resolve_drive(&at), Some("01AT"));
        assert_eq!(
            catalog.resolve_drive("drive:https://git.acme.dev/r#feature@x"),
            Some("01AT")
        );
    }

    #[test]
    fn a_file_round_trips_and_keeps_what_this_build_does_not_apply() {
        let text = "version = 2\n\
                    [settings]\n\
                    \"recording.codec\" = \"hevc\"\n\
                    \"notify.previews_enabled\" = true\n\
                    \"recording.scale_percent\" = 7\n\
                    \"hotkey.global\" = \"Cmd+K\"\n\
                    \"future.key\" = [1, 2]\n\
                    [settings.recording]\n\
                    fps = 30\n";
        let parsed = Values::parse(text.as_bytes(), SyncedFile::Shared).expect("parses");
        assert_eq!(
            parsed.values,
            values(&[
                ("notify.previews_enabled", "1"),
                ("recording.codec", "hevc"),
                ("recording.fps", "30"),
            ])
            .values
        );
        let preserved: Vec<&str> = parsed.preserved.keys().map(String::as_str).collect();
        assert_eq!(
            preserved,
            ["future.key", "hotkey.global", "recording.scale_percent"],
            "unknown, other-file and misshaped keys are kept, not applied"
        );

        let rendered = parsed.render().expect("renders");
        assert!(rendered.starts_with(HEADER), "{rendered}");
        assert!(rendered.contains("[settings]\n"), "{rendered}");
        assert!(
            rendered.contains("\"notify.previews_enabled\" = true\n"),
            "{rendered}"
        );
        assert!(rendered.contains("\"recording.fps\" = 30\n"), "{rendered}");
        assert!(rendered.contains("version = 2\n"), "{rendered}");
        let again = Values::parse(rendered.as_bytes(), SyncedFile::Shared).expect("reparses");
        assert_eq!(again, parsed);
        assert_eq!(
            again.render().expect("renders"),
            rendered,
            "rendering is stable"
        );

        assert!(Values::parse(b"[settings\n", SyncedFile::Shared).is_err());
        assert!(Values::parse(b"settings = 1\n", SyncedFile::Shared).is_err());
    }

    #[test]
    fn a_first_sync_against_existing_settings_pulls_them() {
        let remote = values(&[("recording.codec", "hevc"), ("recording.fps", "30")]);
        let local = values(&[("recording.codec", "h264"), ("recording.camera", "1")]);
        let merged = merge(Some(&remote), None, &local, None, &verbatim);
        assert_eq!(
            merged.file,
            values(&[
                ("recording.camera", "1"),
                ("recording.codec", "hevc"),
                ("recording.fps", "30"),
            ])
        );
        assert_eq!(
            merged.apply,
            applied(&[
                ("recording.codec", Some("hevc")),
                ("recording.fps", Some("30"))
            ])
        );
        assert!(merged.changed);
        assert_eq!(merged.base_if_pushed, merged.file);
    }

    #[test]
    fn a_template_seed_only_fills_gaps() {
        let seed = values(&[("recording.codec", "hevc"), ("recording.fps", "30")]);
        let local = values(&[("recording.codec", "h264")]);
        let merged = merge(
            None,
            Some((seed, FirstSync::LocalWins)),
            &local,
            None,
            &verbatim,
        );
        assert_eq!(
            merged.file,
            values(&[("recording.codec", "h264"), ("recording.fps", "30")])
        );
        assert_eq!(merged.apply, applied(&[("recording.fps", Some("30"))]));
        assert!(merged.changed, "an absent file is always written");
    }

    #[test]
    fn a_later_sync_merges_each_key_three_ways() {
        let base = values(&[
            ("recording.codec", "h264"),
            ("recording.fps", "30"),
            ("recording.scale_percent", "50"),
            ("recording.camera", "1"),
            ("recording.microphone", "1"),
        ]);
        let remote = values(&[
            ("recording.codec", "hevc"),       // changed there
            ("recording.fps", "30"),           // changed here
            ("recording.scale_percent", "75"), // changed on both
            ("recording.microphone", "1"),     // deleted here
                                               // recording.camera deleted there
        ]);
        let local = values(&[
            ("recording.codec", "h264"),
            ("recording.fps", "60"),
            ("recording.scale_percent", "100"),
            ("recording.camera", "1"),
        ]);
        let merged = merge(Some(&remote), None, &local, Some(&base), &verbatim);
        assert_eq!(
            merged.file,
            values(&[
                ("recording.codec", "hevc"),
                ("recording.fps", "60"),
                ("recording.scale_percent", "100"),
            ])
        );
        assert_eq!(
            merged.apply,
            applied(&[
                ("recording.camera", None),
                ("recording.codec", Some("hevc"))
            ])
        );
        assert!(merged.changed);
        assert_eq!(merged.base_if_pushed, merged.file);
        assert_eq!(
            merged.base_if_not_pushed,
            values(&[
                ("recording.codec", "hevc"),
                ("recording.fps", "30"),
                ("recording.scale_percent", "50"),
                ("recording.microphone", "1"),
            ]),
            "pulled keys advance; pushed keys keep their old base and stay dirty"
        );

        // The failed push is retried: the same local changes win again.
        let retry = merge(
            Some(&remote),
            None,
            &values(&[
                ("recording.codec", "hevc"),
                ("recording.fps", "60"),
                ("recording.scale_percent", "100"),
            ]),
            Some(&merged.base_if_not_pushed),
            &verbatim,
        );
        assert_eq!(retry.file, merged.file);
        assert!(retry.apply.is_empty());
    }

    const BOT: &str = "bot:ollama:http://localhost:11434#llama3";

    /// Device B has no such bot.
    fn b_resolves(key: &str, value: &str) -> Option<String> {
        (key != VOICE_TARGET_KEY || value != BOT).then(|| value.to_owned())
    }

    #[test]
    fn a_value_this_device_cannot_apply_stays_in_the_file_and_is_never_pushed_over() {
        // Device A pushed its voice target; B lacks that bot.
        let remote = values(&[(VOICE_TARGET_KEY, BOT), ("recording.codec", "hevc")]);
        let first = merge(Some(&remote), None, &Values::default(), None, &b_resolves);
        assert_eq!(first.file, remote, "kept in the file");
        assert_eq!(
            first.apply,
            applied(&[("recording.codec", Some("hevc"))]),
            "not applied"
        );
        assert!(!first.changed);
        let base_is_local = values(&[("recording.codec", "hevc")]);
        assert_eq!(
            first.base_if_pushed, base_is_local,
            "the base is B's own (absent) value"
        );
        assert_eq!(first.base_if_not_pushed, base_is_local);

        // B's second sync: its table still lacks the key, which is not a delete.
        let second = merge(
            Some(&first.file),
            None,
            &values(&[("recording.codec", "hevc")]),
            Some(&first.base_if_pushed),
            &b_resolves,
        );
        assert_eq!(second.file, remote);
        assert!(!second.changed, "B pushes nothing");
        assert!(second.apply.is_empty());
        assert_eq!(second.base_if_pushed, base_is_local);

        // B adds the bot: the reference resolves, and the value is applied.
        let third = merge(
            Some(&second.file),
            None,
            &values(&[("recording.codec", "hevc")]),
            Some(&second.base_if_pushed),
            &verbatim,
        );
        assert_eq!(third.apply, applied(&[(VOICE_TARGET_KEY, Some(BOT))]));
        assert!(!third.changed);
    }

    #[test]
    fn two_devices_that_cannot_resolve_each_others_value_settle() {
        const X: &str = "bot:ollama:http://a.local:11434#x"; // only on A
        const Y: &str = "bot:ollama:http://b.local:11434#y"; // only on B
        fn a_resolves(key: &str, value: &str) -> Option<String> {
            (value != Y).then(|| format!("{key}:{value}"))
        }
        fn b_resolves(key: &str, value: &str) -> Option<String> {
            (value != X).then(|| format!("{key}:{value}"))
        }
        let resolvers = [a_resolves, b_resolves];
        let locals = [
            values(&[(VOICE_TARGET_KEY, X)]),
            values(&[(VOICE_TARGET_KEY, Y)]),
        ];

        // Both once agreed on a value both could use, then each chose its own.
        let agreed = values(&[(VOICE_TARGET_KEY, "")]);
        let mut remote = agreed.clone();
        let mut bases = [agreed.clone(), agreed];
        let mut pushes = Vec::new();
        for round in 0..3 {
            for device in 0..2 {
                let merged = merge(
                    Some(&remote),
                    None,
                    &locals[device],
                    Some(&bases[device]),
                    &resolvers[device],
                );
                assert!(
                    merged.apply.is_empty(),
                    "neither can apply the other's value"
                );
                if merged.changed {
                    pushes.push((round, device));
                    remote = merged.file;
                }
                bases[device] = merged.base_if_pushed;
            }
        }
        assert_eq!(
            pushes,
            [(0, 0), (0, 1)],
            "after the first round nothing is pushed"
        );
        assert_eq!(
            remote,
            values(&[(VOICE_TARGET_KEY, Y)]),
            "the device syncing last won"
        );
    }

    #[test]
    fn device_a_keeps_its_value_while_b_syncs() {
        // A resolves everything; after B's no-op sync the file is untouched, so
        // A's next merge neither pulls nor pushes.
        let file = values(&[(VOICE_TARGET_KEY, BOT)]);
        let a = merge(Some(&file), None, &file, Some(&file), &verbatim);
        assert!(!a.changed);
        assert!(a.apply.is_empty());
        assert_eq!(a.file, file);
    }

    fn local(id: &str, remote: &str, name: &str, path: &str) -> LocalDrive {
        LocalDrive {
            profile_id: id.to_owned(),
            remote_url: remote.to_owned(),
            branch: "main".to_owned(),
            name: name.to_owned(),
            local_path: path.to_owned(),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            drives_known: true,
            drives: vec![local(
                "01DRIVE",
                "https://tg:secret@GitHub.com/acme/notes.git/",
                "Notes",
                "/Users/tg/notes",
            )],
            providers: vec![(
                "01PROV".to_owned(),
                ProviderRef::new("ollama", "HTTP://LocalHost:11434/"),
            )],
            bots: vec![("01BOT".to_owned(), "01PROV".to_owned(), "llama3".to_owned())],
            account_id: None,
            trusted_origins: Vec::new(),
            forge_origins: Vec::new(),
        }
    }

    #[test]
    fn a_forge_connection_travels_verbatim_and_binds_only_a_drive_on_that_forge() {
        let catalog = Catalog {
            forge_origins: vec![("github".to_owned(), "https://github.com".to_owned())],
            ..catalog()
        };
        // No account is needed: the connection is this device's own.
        let rows: BTreeMap<String, String> = [(
            "sync.credential_source.01DRIVE".to_owned(),
            "forge:github".to_owned(),
        )]
        .into();
        assert_eq!(
            local_from_rows(rows, &catalog, None),
            values(&[(DRIVE_CRED, "forge:github")])
        );
        let text = format!("[settings]\n\"{DRIVE_CRED}\" = \"forge:github\"\n");
        let parsed = Values::parse(text.as_bytes(), SyncedFile::Device).expect("parses");
        assert_eq!(
            parsed.values.get(DRIVE_CRED).map(String::as_str),
            Some("forge:github")
        );

        assert_eq!(
            from_portable(DRIVE_CRED, "forge:github", &catalog).as_deref(),
            Some("forge:github")
        );
        // Another origin, an unknown source, or a bot provider: never bound.
        let elsewhere = Catalog {
            forge_origins: vec![("github".to_owned(), "https://ghe.acme.dev".to_owned())],
            ..catalog.clone()
        };
        assert_eq!(from_portable(DRIVE_CRED, "forge:github", &elsewhere), None);
        // A source this device cannot get a token from is not in the catalog.
        let unusable = Catalog {
            forge_origins: Vec::new(),
            ..catalog.clone()
        };
        assert_eq!(from_portable(DRIVE_CRED, "forge:github", &unusable), None);
        assert_eq!(from_portable(DRIVE_CRED, "forge:gitlab", &catalog), None);
        assert_eq!(from_portable(PROVIDER_CRED, "forge:github", &catalog), None);
        // Spellings of the same origin bind; look-alikes never do.
        for (remote, binds) in [
            ("https://github.com:443/acme/notes.git", true),
            ("HTTPS://GITHUB.COM/acme/notes", true),
            ("https://github.com.evil.com/acme/notes.git", false),
            ("https://github.com@evil.com/acme/notes.git", false),
            ("https://github.com./acme/notes.git", false),
            ("https://github.com:8443/acme/notes.git", false),
            ("http://github.com/acme/notes.git", false),
        ] {
            let spelt = Catalog {
                drives: vec![local("01DRIVE", remote, "Notes", "/Users/tg/notes")],
                ..catalog.clone()
            };
            let key = format!(
                "{SYNC_CREDENTIAL_PREFIX}{}",
                spelt.drive_reference("01DRIVE").expect("reference")
            );
            assert_eq!(
                from_portable(&key, "forge:github", &spelt).is_some(),
                binds,
                "{remote}"
            );
        }
        let provider_row = format!("[settings]\n\"{PROVIDER_CRED}\" = \"forge:github\"\n");
        let parsed = Values::parse(provider_row.as_bytes(), SyncedFile::Device).expect("parses");
        assert!(parsed.values.is_empty());
    }

    #[test]
    fn local_ids_travel_as_references_both_ways() {
        let catalog = catalog();
        for key in DRIVE_KEYS {
            let portable = to_portable(key, "01DRIVE", &catalog).expect("known drive");
            assert_eq!(portable, "drive:https://github.com/acme/notes#main^Notes");
            assert_eq!(
                from_portable(key, &portable, &catalog).as_deref(),
                Some("01DRIVE")
            );
            assert_eq!(
                to_portable(key, "01OTHER", &catalog),
                None,
                "{key}: unknown id"
            );
            assert_eq!(
                from_portable(key, "drive:https://github.com/acme/other#main", &catalog),
                None,
                "{key}: unresolved reference"
            );
        }

        let bot = to_portable(VOICE_TARGET_KEY, "01BOT", &catalog).expect("known bot");
        assert_eq!(bot, BOT);
        assert_eq!(
            from_portable(VOICE_TARGET_KEY, &bot, &catalog).as_deref(),
            Some("01BOT")
        );
        assert_eq!(to_portable(VOICE_TARGET_KEY, "01GONE", &catalog), None);

        let stored = r#"{"provider":"01PROV","model":"nomic-embed-text"}"#;
        let portable = to_portable(EMBEDDING_MODEL_KEY, stored, &catalog).expect("known provider");
        assert_eq!(
            portable,
            r#"{"provider":"provider:ollama:http://localhost:11434","model":"nomic-embed-text"}"#
        );
        assert_eq!(
            from_portable(EMBEDDING_MODEL_KEY, &portable, &catalog).as_deref(),
            Some(stored)
        );
        assert_eq!(
            to_portable(
                EMBEDDING_MODEL_KEY,
                r#"{"provider":"01X","model":"m"}"#,
                &catalog
            ),
            None
        );

        assert_eq!(
            to_portable("recording.codec", "hevc", &catalog).as_deref(),
            Some("hevc")
        );
        assert_eq!(
            to_portable("notes.active_vault", "", &catalog).as_deref(),
            Some("")
        );
    }

    #[test]
    fn remote_spellings_normalize_to_one_without_a_credential() {
        for (raw, want) in [
            (
                "https://tg@GitHub.com/acme/notes.git",
                "https://github.com/acme/notes",
            ),
            (
                "https://oauth2:glpat-SECRET@gitlab.com/me/notes.git",
                "https://gitlab.com/me/notes",
            ),
            (
                "HTTPS://github.com/acme/notes/",
                "https://github.com/acme/notes",
            ),
            ("git@github.com:acme/notes.git", "git@github.com:acme/notes"),
            (
                "git:hunter2@github.com:acme/notes",
                "git@github.com:acme/notes",
            ),
            (
                "ssh://git:hunter2@GitHub.com/acme/notes.git",
                "ssh://git@github.com/acme/notes",
            ),
        ] {
            assert_eq!(normalize_remote(raw), want, "{raw}");
            assert!(!remote_has_secret(&normalize_remote(raw)), "{raw}");
        }
        for secret in [
            "https://oauth2:glpat-SECRET@gitlab.com/me/notes.git",
            "https://glpat-SECRET@gitlab.com/me/notes.git",
            "ssh://git:hunter2@github.com/acme/notes",
            "git:hunter2@github.com:acme/notes",
        ] {
            assert!(remote_has_secret(secret), "{secret}");
        }
        for clean in [
            "ssh://git@github.com/acme/notes",
            "git@github.com:acme/notes",
            "/home/tg/notes",
        ] {
            assert!(!remote_has_secret(clean), "{clean}");
        }
    }

    #[test]
    fn a_remote_value_this_build_cannot_read_is_kept_and_never_a_deletion() {
        let mut remote = values(&[("recording.fps", "30")]);
        remote.preserved.insert(
            "recording.codec".to_owned(),
            toml::Value::String("av1".to_owned()),
        );
        let base = values(&[("recording.codec", "hevc"), ("recording.fps", "30")]);

        let untouched = merge(Some(&remote), None, &base, Some(&base), &verbatim);
        assert!(untouched.apply.is_empty(), "nothing deleted here");
        assert!(!untouched.changed, "nothing pushed");
        assert_eq!(untouched.file, remote, "the newer value stays in the file");
        assert_eq!(untouched.base_if_pushed, base);
        assert_eq!(untouched.base_if_not_pushed, base);

        let first = merge(Some(&remote), None, &base, None, &verbatim);
        assert!(first.apply.is_empty());
        assert_eq!(first.base_if_pushed, base);

        // A change made here replaces it.
        let local = values(&[("recording.codec", "h264"), ("recording.fps", "30")]);
        let pushed = merge(Some(&remote), None, &local, Some(&base), &verbatim);
        assert_eq!(pushed.file, local);
        assert!(pushed.changed);
    }

    #[test]
    fn a_stale_or_unknown_local_reference_reads_as_the_base() {
        let base = values(&[
            (VOICE_TARGET_KEY, BOT),
            (
                "notes.active_vault",
                "drive:https://github.com/acme/notes#main",
            ),
        ]);
        let rows = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };

        // The bot is gone here, but its row still names it.
        let local = local_from_rows(
            rows(&[(VOICE_TARGET_KEY, "01GONE")]),
            &catalog(),
            Some(&base),
        );
        assert_eq!(
            local.values.get(VOICE_TARGET_KEY).map(String::as_str),
            Some(BOT)
        );
        let local = local_from_rows(rows(&[(VOICE_TARGET_KEY, "01GONE")]), &catalog(), None);
        assert_eq!(local.values.get(VOICE_TARGET_KEY), None);

        // The drive list could not be read: every drive key is the base's.
        let unknown = Catalog {
            drives_known: false,
            ..catalog()
        };
        let local = local_from_rows(
            rows(&[("tasks.ledger_vault", "01DRIVE")]),
            &unknown,
            Some(&base),
        );
        assert_eq!(
            local.values,
            values(&[(
                "notes.active_vault",
                "drive:https://github.com/acme/notes#main"
            )])
            .values
        );
        assert_eq!(
            from_portable(
                "notes.active_vault",
                "drive:https://github.com/acme/notes#main",
                &unknown
            ),
            None
        );
    }

    #[test]
    fn a_file_that_disappeared_is_recreated_from_this_device_not_seeded() {
        let base = values(&[("recording.codec", "hevc"), ("recording.fps", "30")]);
        let local = values(&[("recording.codec", "h264")]);
        let seed = values(&[("recording.codec", "av1"), ("recording.fps", "60")]);
        let merged = merge(
            None,
            Some((seed, FirstSync::RemoteWins)),
            &local,
            Some(&base),
            &verbatim,
        );
        assert_eq!(merged.file, local);
        assert!(merged.changed);
        assert!(
            merged.apply.is_empty(),
            "nothing local is replaced or deleted"
        );
    }

    #[test]
    fn formatting_and_comments_do_not_make_a_file_differ() {
        let rendered = values(&[("recording.codec", "hevc"), ("recording.fps", "30")])
            .render()
            .expect("renders");
        let hand = "# my notes\n[settings.recording]\nfps = 30 # smooth\ncodec = 'hevc'\n";
        assert_eq!(
            Values::parse(hand.as_bytes(), SyncedFile::Shared).expect("hand"),
            Values::parse(rendered.as_bytes(), SyncedFile::Shared).expect("rendered")
        );
    }

    #[test]
    fn a_path_is_applied_only_where_its_directory_exists() {
        let catalog = Catalog::default();
        let here = std::env::temp_dir();
        let here = here.to_string_lossy();
        assert_eq!(
            from_portable("recording.destination_dir", &here, &catalog).as_deref(),
            Some(here.as_ref())
        );
        let missing = std::env::temp_dir().join("keeper-no-such-dir-8f2c1e");
        assert_eq!(
            from_portable(
                "recording.destination_dir",
                &missing.to_string_lossy(),
                &catalog
            ),
            None
        );
        // Merged with the real resolver, the missing directory stays in the
        // file and is neither applied nor pushed over.
        let remote = values(&[("recording.destination_dir", &missing.to_string_lossy())]);
        let resolve = |k: &str, v: &str| from_portable(k, v, &catalog);
        let merged = merge(Some(&remote), None, &Values::default(), None, &resolve);
        assert_eq!(merged.file, remote);
        assert!(merged.apply.is_empty());
    }

    /// A worktree as a map of repo-relative paths to text.
    #[derive(Default)]
    struct Tree(BTreeMap<String, String>);

    impl Tree {
        fn with(mut self, rel: &str, text: &str) -> Self {
            self.0.insert(rel.to_owned(), text.to_owned());
            self
        }
    }

    impl RepoFiles for Tree {
        fn read(&self, rel: &str) -> Option<Vec<u8>> {
            self.0.get(rel).map(|text| text.as_bytes().to_vec())
        }

        fn list_dir(&self, rel: &str) -> Vec<String> {
            let prefix = format!("{rel}/");
            self.0
                .keys()
                .filter_map(|path| path.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_owned)
                .collect()
        }
    }

    fn device(tree: Tree, slug: &str, class: &str, codec: &str) -> Tree {
        tree.with(
            &format!("tg/devices/{slug}.toml"),
            &format!("name = \"{slug}\"\nclass = \"{class}\"\n"),
        )
        .with(
            &format!("tg/settings.{slug}.toml"),
            &format!("[settings]\n\"hotkey.global\" = \"{codec}\"\n"),
        )
    }

    fn hotkey(seed: &Option<(Values, FirstSync)>) -> Option<(&str, FirstSync)> {
        seed.as_ref()
            .map(|(values, first)| (values.values["hotkey.global"].as_str(), *first))
    }

    #[test]
    fn a_new_device_is_seeded_from_the_latest_device_of_its_class() {
        let tree = device(Tree::default(), "old-mac", "desktop", "Cmd+O");
        let tree = device(tree, "new-mac", "desktop", "Cmd+N");
        let tree = device(tree, "ipad", "tablet", "Cmd+I");
        let tree = tree.with(
            "_template/settings/desktop.toml",
            "[settings]\n\"hotkey.global\" = \"Cmd+T\"\n",
        );
        let times = |rel: &str| match rel {
            "tg/settings.old-mac.toml" => Some(100),
            "tg/settings.new-mac.toml" => Some(200),
            "tg/settings.ipad.toml" => Some(300),
            _ => None,
        };
        let seed = seed_device(&tree, "tg", DeviceClass::Desktop, "studio", &times);
        assert_eq!(hotkey(&seed), Some(("Cmd+N", FirstSync::RemoteWins)));

        let seed = seed_device(&tree, "tg", DeviceClass::Desktop, "new-mac", &times);
        assert_eq!(
            hotkey(&seed),
            Some(("Cmd+O", FirstSync::RemoteWins)),
            "never itself"
        );

        let undated = |_: &str| None;
        let seed = seed_device(&tree, "tg", DeviceClass::Desktop, "studio", &undated);
        assert_eq!(
            hotkey(&seed),
            Some(("Cmd+N", FirstSync::RemoteWins)),
            "ties by slug"
        );
    }

    #[test]
    fn without_a_same_class_device_the_template_fills_gaps_or_nothing_seeds() {
        let tree = device(Tree::default(), "ipad", "tablet", "Cmd+I").with(
            "_template/settings/desktop.toml",
            "[settings]\n\"hotkey.global\" = \"Cmd+T\"\n",
        );
        let never = |_: &str| None;
        let seed = seed_device(&tree, "tg", DeviceClass::Desktop, "mac", &never);
        assert_eq!(hotkey(&seed), Some(("Cmd+T", FirstSync::LocalWins)));
        assert!(seed_device(&tree, "tg", DeviceClass::Mobile, "phone", &never).is_none());

        assert!(seed_shared(&tree).is_none());
        let tree = tree.with(
            "_template/settings.toml",
            "[settings]\n\"recording.codec\" = \"hevc\"\n",
        );
        let seed = seed_shared(&tree).expect("template");
        assert_eq!(seed.1, FirstSync::LocalWins);
        assert_eq!(seed.0.values["recording.codec"], "hevc");
    }
}
