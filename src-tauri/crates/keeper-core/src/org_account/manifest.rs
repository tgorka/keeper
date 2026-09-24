//! Drives, bot providers and Matrix accounts as offers in the person's config
//! repository (AD-323).
//!
//! ```text
//! <login>/drives.toml    [[drive]]
//! <login>/bots.toml      [[provider]] with [[provider.bot]]
//! <login>/matrix.toml    [[account]]
//! ```
//!
//! Each record says what the thing is and which devices use it — never a
//! secret, only the *choice* of credential. A device merges its own list in
//! ([`merge_drives`], [`merge_providers`], [`merge_matrix`]) and is offered the
//! rest ([`offers`]); nothing is added by itself. Fields this build does not
//! know are kept, so a newer device's records survive an older device's sync.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::settings_sync::{
    is_scp_remote, normalize_base_url, normalize_remote, remote_has_secret, DriveRef, ProviderRef,
};
use super::state::{AccountOffersVm, DriveOfferVm, MatrixOfferVm, ProviderOfferVm};

/// Fields a record carries that this build does not know.
pub type Extra = BTreeMap<String, toml::Value>;

/// `<login>/drives.toml`.
pub fn drives_path(login: &str) -> String {
    format!("{login}/drives.toml")
}

/// `<login>/bots.toml`.
pub fn bots_path(login: &str) -> String {
    format!("{login}/bots.toml")
}

/// `<login>/matrix.toml`.
pub fn matrix_path(login: &str) -> String {
    format!("{login}/matrix.toml")
}

fn default_branch() -> String {
    "main".to_owned()
}

fn default_credential_none() -> String {
    "none".to_owned()
}

fn default_credential_own() -> String {
    "own".to_owned()
}

fn default_matrix_kind() -> String {
    "password".to_owned()
}

/// One drive: the portable half of a sync profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DriveRecord {
    #[serde(default)]
    pub name: String,
    pub remote_url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// `account`, `own` or `none`.
    #[serde(default = "default_credential_none")]
    pub credential: String,
    /// Each role's subfolder; `Some` means the role is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recordings: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lfs_threshold_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_patterns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_over_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_ttl_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_subject_template: Option<String>,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One pinned bot of a provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BotRecord {
    pub target: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pin_order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One bot provider and its pinned bots.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderRecord {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    pub base_url: String,
    /// `account` or `own`.
    #[serde(default = "default_credential_own")]
    pub credential: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_timeout_ms: Option<u64>,
    #[serde(rename = "bot", default, skip_serializing_if = "Vec::is_empty")]
    pub bots: Vec<BotRecord>,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One Matrix account: what a prefilled sign-in needs, never a session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MatrixRecord {
    pub user_id: String,
    #[serde(default)]
    pub homeserver_url: String,
    /// `password`, `oidc` or `beeper`.
    #[serde(default = "default_matrix_kind")]
    pub kind: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A drive remote as it may travel: [`normalize_remote`]'s spelling, which
/// carries no credential. `None` for a remote that only means something on
/// this machine — a `file://` URL, a filesystem path, a Windows drive path.
pub fn portable_remote(url: &str) -> Option<String> {
    let url = url.trim();
    let remote = match url.split_once("://") {
        Some((scheme, _)) => !scheme.eq_ignore_ascii_case("file"),
        None => is_scp_remote(url),
    };
    remote.then(|| normalize_remote(url))
}

/// A homeserver URL in one spelling: scheme and host lower-cased, no trailing
/// `/`. A string that is not a URL is only trimmed.
fn normalize_homeserver(url: &str) -> String {
    let trimmed = url.trim();
    match url::Url::parse(trimmed) {
        Ok(parsed) if parsed.has_host() => format!(
            "{}{}",
            &parsed[..url::Position::BeforePath],
            parsed.path().trim_end_matches('/')
        ),
        _ => trimmed.trim_end_matches('/').to_owned(),
    }
}

/// A record the per-device merge can reason about.
trait Record: Clone + PartialEq {
    type Id: Ord + Clone;
    fn identity(&self) -> Self::Id;
    /// The identity each of `remote` is matched to this device's entries
    /// under; by default its own.
    fn matched_ids(remote: &[Self], mine: &[Self]) -> Vec<Self::Id> {
        let _ = mine;
        remote.iter().map(Record::identity).collect()
    }
    fn devices_mut(&mut self) -> &mut Vec<String>;
    /// Keep what `remote` knows and this build does not.
    fn absorb(&mut self, remote: &Self);
    /// The record as it is written, URLs in their one spelling; `None` when
    /// it must not travel at all.
    fn portable(&self) -> Option<Self>;
    /// What the record says about the thing: everything but `devices` and
    /// fields this build does not know.
    fn described(&self) -> Self;
}

fn overlay(mine: &mut Extra, remote: &Extra) {
    for (key, value) in remote {
        mine.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

impl Record for DriveRecord {
    type Id = DriveRef;
    /// Remote, branch and name: two drives of one repository stay two.
    fn identity(&self) -> DriveRef {
        DriveRef::named(&self.remote_url, &self.branch, &self.name)
    }
    /// A record whose name no drive here has — written before names were
    /// part of a drive's identity, or renamed on one device — is still this
    /// device's drive when this device has exactly one drive on its remote and
    /// branch, that drive matches no other record, and no other unmatched
    /// record shares the repository.
    fn matched_ids(remote: &[Self], mine: &[Self]) -> Vec<DriveRef> {
        let mine: BTreeSet<DriveRef> = mine.iter().map(Record::identity).collect();
        let theirs: Vec<DriveRef> = remote.iter().map(Record::identity).collect();
        theirs
            .iter()
            .map(|id| {
                if mine.contains(id) {
                    return id.clone();
                }
                let mut here = mine.iter().filter(|local| local.same_repository(id));
                let (Some(only), None) = (here.next(), here.next()) else {
                    return id.clone();
                };
                let unmatched = theirs
                    .iter()
                    .filter(|other| other.same_repository(id) && !mine.contains(other))
                    .count();
                if unmatched == 1 && !theirs.contains(only) {
                    only.clone()
                } else {
                    id.clone()
                }
            })
            .collect()
    }
    fn devices_mut(&mut self) -> &mut Vec<String> {
        &mut self.devices
    }
    fn absorb(&mut self, remote: &Self) {
        overlay(&mut self.extra, &remote.extra);
    }
    fn portable(&self) -> Option<Self> {
        let Some(remote_url) = portable_remote(&self.remote_url) else {
            tracing::warn!("a drive on a local remote is not written to the account's repository");
            return None;
        };
        Some(DriveRecord {
            remote_url,
            ..self.clone()
        })
    }
    fn described(&self) -> Self {
        DriveRecord {
            devices: Vec::new(),
            extra: Extra::new(),
            ..self.clone()
        }
    }
}

impl Record for ProviderRecord {
    type Id = ProviderRef;
    fn identity(&self) -> ProviderRef {
        ProviderRef::new(&self.kind, &self.base_url)
    }
    fn devices_mut(&mut self) -> &mut Vec<String> {
        &mut self.devices
    }
    fn absorb(&mut self, remote: &Self) {
        overlay(&mut self.extra, &remote.extra);
        for bot in &mut self.bots {
            if let Some(theirs) = remote
                .bots
                .iter()
                .find(|theirs| theirs.target == bot.target)
            {
                overlay(&mut bot.extra, &theirs.extra);
            }
        }
    }
    fn portable(&self) -> Option<Self> {
        let mut bots = self.bots.clone();
        bots.sort_by(|a, b| (a.pin_order, &a.target).cmp(&(b.pin_order, &b.target)));
        Some(ProviderRecord {
            base_url: normalize_base_url(&self.base_url),
            bots,
            ..self.clone()
        })
    }
    fn described(&self) -> Self {
        ProviderRecord {
            bots: self
                .bots
                .iter()
                .map(|bot| BotRecord {
                    extra: Extra::new(),
                    ..bot.clone()
                })
                .collect(),
            devices: Vec::new(),
            extra: Extra::new(),
            ..self.clone()
        }
    }
}

impl Record for MatrixRecord {
    type Id = String;
    fn identity(&self) -> String {
        self.user_id.trim().to_owned()
    }
    fn devices_mut(&mut self) -> &mut Vec<String> {
        &mut self.devices
    }
    fn absorb(&mut self, remote: &Self) {
        overlay(&mut self.extra, &remote.extra);
    }
    fn portable(&self) -> Option<Self> {
        Some(MatrixRecord {
            homeserver_url: normalize_homeserver(&self.homeserver_url),
            ..self.clone()
        })
    }
    fn described(&self) -> Self {
        MatrixRecord {
            devices: Vec::new(),
            extra: Extra::new(),
            ..self.clone()
        }
    }
}

/// A three-way merge of one manifest. `mine` is this device's whole list
/// (`devices` empty), `mine_base` the list it last pushed. An entry this device
/// has names `device`, and takes its fields from here only when they changed
/// here since `mine_base` — so two devices that describe one drive differently
/// do not rewrite it on every sync. An entry this device lacks stops naming it
/// when `prune_me` — never before this device has restored itself, when an
/// empty `mine` means "not restored yet", not "removed"; `devices` is pruned
/// to `known_devices` (unless that list is empty, which means it could not be
/// read); an entry no device names is dropped. Sorted by identity, URLs
/// written in their one spelling.
fn merge<T: Record>(
    remote: &[T],
    mine: &[T],
    mine_base: Option<&[T]>,
    device: &str,
    known_devices: &[String],
    prune_me: bool,
) -> Vec<T> {
    let base: BTreeMap<T::Id, T> = mine_base
        .unwrap_or_default()
        .iter()
        .filter_map(Record::portable)
        .map(|entry| (entry.identity(), entry.described()))
        .collect();
    let remote: Vec<T> = remote.iter().filter_map(Record::portable).collect();
    let mine: Vec<T> = mine.iter().filter_map(Record::portable).collect();
    let mut merged: BTreeMap<T::Id, T> = BTreeMap::new();
    for (id, entry) in T::matched_ids(&remote, &mine).into_iter().zip(remote) {
        merged.entry(id).or_insert(entry);
    }
    let mut here = BTreeSet::new();
    for entry in mine {
        let id = entry.identity();
        if !here.insert(id.clone()) {
            continue;
        }
        let mut written = match merged.remove(&id) {
            Some(mut theirs) => {
                let changed_here = base.get(&id).is_none_or(|was| *was != entry.described());
                if changed_here {
                    let devices = std::mem::take(theirs.devices_mut());
                    let mut ours = entry;
                    ours.absorb(&theirs);
                    *ours.devices_mut() = devices;
                    ours
                } else {
                    theirs
                }
            }
            None => {
                let mut ours = entry;
                ours.devices_mut().clear();
                ours
            }
        };
        written.devices_mut().push(device.to_owned());
        merged.insert(id, written);
    }
    merged
        .into_iter()
        .filter_map(|(id, mut entry)| {
            let devices = entry.devices_mut();
            if prune_me && !here.contains(&id) {
                devices.retain(|named| named != device);
            }
            if !known_devices.is_empty() {
                devices.retain(|named| named == device || known_devices.contains(named));
            }
            devices.sort();
            devices.dedup();
            (!devices.is_empty()).then_some(entry)
        })
        .collect()
}

pub fn merge_drives(
    remote: &[DriveRecord],
    mine: &[DriveRecord],
    mine_base: Option<&[DriveRecord]>,
    device: &str,
    known_devices: &[String],
    prune_me: bool,
) -> Vec<DriveRecord> {
    merge(remote, mine, mine_base, device, known_devices, prune_me)
}

pub fn merge_providers(
    remote: &[ProviderRecord],
    mine: &[ProviderRecord],
    mine_base: Option<&[ProviderRecord]>,
    device: &str,
    known_devices: &[String],
    prune_me: bool,
) -> Vec<ProviderRecord> {
    merge(remote, mine, mine_base, device, known_devices, prune_me)
}

pub fn merge_matrix(
    remote: &[MatrixRecord],
    mine: &[MatrixRecord],
    mine_base: Option<&[MatrixRecord]>,
    device: &str,
    known_devices: &[String],
    prune_me: bool,
) -> Vec<MatrixRecord> {
    merge(remote, mine, mine_base, device, known_devices, prune_me)
}

fn missing<'a, T: Record>(remote: &'a [T], mine: &[T]) -> impl Iterator<Item = &'a T> {
    let here: BTreeSet<T::Id> = mine.iter().map(Record::identity).collect();
    remote
        .iter()
        .zip(T::matched_ids(remote, mine))
        .filter(move |(_, id)| !here.contains(id))
        .map(|(entry, _)| entry)
}

/// The drives, providers and Matrix accounts in the repository whose identity
/// this device does not have. A drive whose remote carries a credential or
/// names a local path is never offered.
pub fn offers(
    remote_drives: &[DriveRecord],
    remote_providers: &[ProviderRecord],
    remote_matrix: &[MatrixRecord],
    mine_drives: &[DriveRecord],
    mine_providers: &[ProviderRecord],
    mine_matrix: &[MatrixRecord],
) -> AccountOffersVm {
    AccountOffersVm {
        drives: missing(remote_drives, mine_drives)
            .filter_map(|drive| {
                if remote_has_secret(&drive.remote_url) {
                    tracing::warn!(
                        "a drive record carries a credential in its remote; not offered"
                    );
                    return None;
                }
                let remote_url = portable_remote(&drive.remote_url)?;
                Some(DriveOfferVm {
                    key: drive.identity().reference(),
                    name: drive.name.clone(),
                    remote_url,
                    branch: drive.branch.clone(),
                    credential: drive.credential.clone(),
                    notes: drive.notes.clone(),
                    recordings: drive.recordings.clone(),
                    sessions: drive.sessions.clone(),
                    tasks: drive.tasks.clone(),
                    excludes: drive.excludes.clone(),
                    lfs_threshold_bytes: drive.lfs_threshold_bytes,
                    virtual_patterns: drive.virtual_patterns.clone(),
                    virtual_over_bytes: drive.virtual_over_bytes,
                    release_ttl_ms: drive.release_ttl_ms,
                    tags: drive.tags.clone(),
                    commit_subject_template: drive.commit_subject_template.clone(),
                    devices: drive.devices.clone(),
                })
            })
            .collect(),
        providers: missing(remote_providers, mine_providers)
            .map(|provider| {
                let mut bots: Vec<&BotRecord> = provider.bots.iter().collect();
                bots.sort_by_key(|bot| bot.pin_order);
                ProviderOfferVm {
                    key: provider.identity().reference(),
                    kind: provider.kind.clone(),
                    name: provider.name.clone(),
                    base_url: provider.base_url.clone(),
                    credential: provider.credential.clone(),
                    bots: bots.into_iter().map(|bot| bot.name.clone()).collect(),
                    devices: provider.devices.clone(),
                }
            })
            .collect(),
        matrix: missing(remote_matrix, mine_matrix)
            .map(|account| MatrixOfferVm {
                key: format!("matrix:{}", account.identity()),
                user_id: account.user_id.clone(),
                homeserver_url: account.homeserver_url.clone(),
                kind: account.kind.clone(),
                devices: account.devices.clone(),
            })
            .collect(),
    }
}

fn parse_toml<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned())?;
    toml::from_str(text).map_err(|error| format!("it cannot be read: {}", error.message()))
}

fn render_toml<T: Serialize>(header: &str, file: &T) -> Result<String, String> {
    let body = toml::to_string(file).map_err(|error| error.to_string())?;
    Ok(format!("{header}{body}"))
}

const DRIVES_HEADER: &str =
    "# The drives you use, and on which devices. keeper offers each one to your other devices;\n\
# it never adds one by itself, and no credential is ever written here.\n";
const BOTS_HEADER: &str =
    "# Your bot providers and their bots, and on which devices. keeper offers each one to your\n\
# other devices; it never adds one by itself, and no key is ever written here.\n";
const MATRIX_HEADER: &str =
    "# Your Matrix accounts, and on which devices. keeper offers a prefilled sign-in on your\n\
# other devices; no session or password is ever written here.\n";

/// `drives.toml`. An `Err` is a file keeper cannot read; leave it alone.
/// Equality is value equality: two files that parse to the same records are
/// equal whatever their formatting or comments.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DrivesFile {
    #[serde(rename = "drive", default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<DriveRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl DrivesFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        parse_toml(bytes)
    }

    /// Refused when any remote carries a credential: that must never reach
    /// the repository, whatever handed the record in.
    pub fn render(&self) -> Result<String, String> {
        if self
            .drives
            .iter()
            .any(|drive| remote_has_secret(&drive.remote_url))
        {
            return Err(
                "a drive's remote carries a credential, so drives.toml is not written".to_owned(),
            );
        }
        render_toml(DRIVES_HEADER, self)
    }
}

/// `bots.toml`. An `Err` is a file keeper cannot read; leave it alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BotsFile {
    #[serde(rename = "provider", default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl BotsFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        parse_toml(bytes)
    }

    pub fn render(&self) -> Result<String, String> {
        render_toml(BOTS_HEADER, self)
    }
}

/// `matrix.toml`. An `Err` is a file keeper cannot read; leave it alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatrixFile {
    #[serde(rename = "account", default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<MatrixRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl MatrixFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        parse_toml(bytes)
    }

    pub fn render(&self) -> Result<String, String> {
        render_toml(MATRIX_HEADER, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(url: &str, name: &str, devices: &[&str]) -> DriveRecord {
        DriveRecord {
            name: name.to_owned(),
            remote_url: url.to_owned(),
            branch: "main".to_owned(),
            credential: "account".to_owned(),
            notes: Some("notes".to_owned()),
            devices: devices.iter().map(|d| (*d).to_owned()).collect(),
            ..DriveRecord::default()
        }
    }

    fn names(records: &[DriveRecord]) -> Vec<(&str, Vec<&str>)> {
        records
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.devices.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_device_joins_what_it_has_leaves_what_it_lacks_and_empties_are_dropped() {
        let mut shared = drive("https://github.com/acme/a", "A", &["studio"]);
        shared
            .extra
            .insert("future".to_owned(), toml::Value::Integer(1));
        let remote = [
            shared,
            drive("https://github.com/acme/b", "B", &["mac", "studio"]),
            drive("https://github.com/acme/c", "C", &["mac"]),
        ];
        let mine = [
            // Another spelling of A, renamed here: the same drive, last writer wins.
            drive("https://tg@GitHub.com/acme/a.git", "A renamed", &[]),
            drive("https://github.com/acme/d", "D", &[]),
        ];
        let merged = merge_drives(&remote, &mine, None, "mac", &[], true);
        assert_eq!(
            names(&merged),
            [
                ("A renamed", vec!["mac", "studio"]),
                ("B", vec!["studio"]),
                ("D", vec!["mac"]),
            ],
            "C named only this device and is gone"
        );
        assert_eq!(
            merged[0].remote_url, "https://github.com/acme/a",
            "written normalized"
        );
        assert_eq!(
            merged[0].extra.get("future"),
            Some(&toml::Value::Integer(1)),
            "a field this build does not know survives"
        );
    }

    fn sync_rounds(
        laptop: &DriveRecord,
        phone: &DriveRecord,
        rounds: usize,
    ) -> (usize, Vec<DriveRecord>) {
        let known = ["laptop".to_owned(), "phone".to_owned()];
        let mut remote: Vec<DriveRecord> = Vec::new();
        let mut bases: [Option<Vec<DriveRecord>>; 2] = [None, None];
        let devices = [("laptop", laptop), ("phone", phone)];
        let mut pushes = 0;
        for _ in 0..rounds {
            for (index, (slug, mine)) in devices.iter().enumerate() {
                let mine = std::slice::from_ref(*mine);
                let merged =
                    merge_drives(&remote, mine, bases[index].as_deref(), slug, &known, true);
                if merged != remote {
                    pushes += 1;
                    remote = merged;
                }
                bases[index] = Some(mine.to_vec());
            }
        }
        (pushes, remote)
    }

    #[test]
    fn two_devices_that_describe_one_drive_differently_settle() {
        let mut laptop = drive("https://github.com/acme/notes.git", "Notes", &[]);
        laptop.credential = "own".to_owned();
        let phone = drive("https://github.com/acme/notes", "notes", &[]);
        let (pushes, file) = sync_rounds(&laptop, &phone, 3);
        assert_eq!(pushes, 2, "each device writes once, then nothing changes");
        assert_eq!(file[0].devices, ["laptop", "phone"]);

        // A change made on the laptop afterwards still travels.
        let mut remote = file;
        let base = [laptop.clone()];
        laptop.name = "Work notes".to_owned();
        let known = ["laptop".to_owned(), "phone".to_owned()];
        remote = merge_drives(&remote, &[laptop], Some(&base), "laptop", &known, true);
        assert_eq!(remote[0].name, "Work notes");
    }

    #[test]
    fn membership_is_pruned_to_registered_devices() {
        let remote = [drive(
            "https://github.com/acme/a",
            "A",
            &["laptop", "old-mac"],
        )];
        let known = ["laptop".to_owned(), "studio".to_owned()];
        let merged = merge_drives(&remote, &[], None, "studio", &known, true);
        assert_eq!(
            names(&merged),
            [("A", vec!["laptop"])],
            "a renamed device's old slug goes"
        );
        let kept = merge_drives(&remote, &[], None, "studio", &[], true);
        assert_eq!(
            names(&kept),
            [("A", vec!["laptop", "old-mac"])],
            "an unreadable device list prunes nothing"
        );
    }

    #[test]
    fn a_drive_remote_travels_without_a_credential_and_a_local_one_not_at_all() {
        for (raw, want) in [
            (
                "https://oauth2:glpat-SECRET@GitLab.com/me/notes.git",
                Some("https://gitlab.com/me/notes"),
            ),
            (
                "git@github.com:acme/notes.git",
                Some("git@github.com:acme/notes"),
            ),
            (
                "ssh://git@github.com/acme/notes",
                Some("ssh://git@github.com/acme/notes"),
            ),
            ("file:///home/tg/notes", None),
            ("/home/tg/notes.git", None),
            ("../notes", None),
            ("notes", None),
            ("C:\\Users\\tg\\notes", None),
            ("D:/notes", None),
        ] {
            assert_eq!(portable_remote(raw).as_deref(), want, "{raw}");
        }

        let secret = drive(
            "https://oauth2:glpat-SECRET@gitlab.com/me/notes.git",
            "N",
            &["x"],
        );
        let local = drive("/home/tg/notes", "L", &["x"]);
        let file = DrivesFile {
            drives: vec![secret.clone()],
            extra: Extra::new(),
        };
        assert!(
            file.render().is_err(),
            "the second lock refuses a credential"
        );

        let merged = merge_drives(
            &[secret.clone(), local.clone()],
            &[],
            None,
            "mac",
            &[],
            true,
        );
        assert_eq!(merged.len(), 1, "a local remote is dropped");
        assert_eq!(merged[0].remote_url, "https://gitlab.com/me/notes");
        assert!(DrivesFile {
            drives: merged,
            extra: Extra::new(),
        }
        .render()
        .is_ok());

        let offered = offers(&[secret, local], &[], &[], &[], &[], &[]);
        assert!(offered.drives.is_empty(), "neither is offered");
    }

    #[test]
    fn provider_bots_come_from_this_device_and_keep_unknown_fields() {
        let mut theirs = BotRecord {
            target: "llama3".to_owned(),
            name: "Llama".to_owned(),
            ..BotRecord::default()
        };
        theirs
            .extra
            .insert("voice".to_owned(), toml::Value::String("low".to_owned()));
        let remote = [ProviderRecord {
            kind: "ollama".to_owned(),
            name: "Home".to_owned(),
            base_url: "http://localhost:11434".to_owned(),
            credential: "own".to_owned(),
            bots: vec![theirs],
            devices: vec!["studio".to_owned()],
            ..ProviderRecord::default()
        }];
        let mine = [ProviderRecord {
            kind: "ollama".to_owned(),
            name: "Home Ollama".to_owned(),
            base_url: "HTTP://LocalHost:11434/".to_owned(),
            credential: "own".to_owned(),
            bots: vec![BotRecord {
                target: "llama3".to_owned(),
                name: "Llama 3".to_owned(),
                ..BotRecord::default()
            }],
            ..ProviderRecord::default()
        }];
        let merged = merge_providers(&remote, &mine, None, "mac", &[], true);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Home Ollama");
        assert_eq!(merged[0].base_url, "http://localhost:11434");
        assert_eq!(merged[0].devices, ["mac", "studio"]);
        assert_eq!(merged[0].bots[0].name, "Llama 3");
        assert_eq!(
            merged[0].bots[0].extra.get("voice"),
            Some(&toml::Value::String("low".to_owned()))
        );

        let only_mac = [MatrixRecord {
            user_id: "@tg:acme.dev".to_owned(),
            devices: vec!["mac".to_owned()],
            ..MatrixRecord::default()
        }];
        let gone = merge_matrix(&only_mac, &[], None, "mac", &[], true);
        assert!(gone.is_empty());
        // Before this device has restored itself, lacking an entry is not
        // having removed it.
        let kept = merge_matrix(&only_mac, &[], None, "mac", &[], false);
        assert_eq!(kept, only_mac);
        let drives = [drive("https://github.com/acme/a", "A", &["mac"])];
        assert_eq!(merge_drives(&drives, &[], None, "mac", &[], false), drives);

        let written = merge_matrix(
            &[],
            &[MatrixRecord {
                user_id: "@tg:acme.dev".to_owned(),
                homeserver_url: "HTTPS://Matrix.Acme.dev/".to_owned(),
                ..MatrixRecord::default()
            }],
            None,
            "mac",
            &[],
            true,
        );
        assert_eq!(written[0].homeserver_url, "https://matrix.acme.dev");
    }

    #[test]
    fn manifests_round_trip_with_unknown_fields() {
        let text = "schema = 2\n\
                    [[provider]]\n\
                    kind = \"hermes\"\n\
                    name = \"Work\"\n\
                    base_url = \"https://gw.acme.dev\"\n\
                    credential = \"account\"\n\
                    devices = [\"mac\"]\n\
                    rate = 3\n\
                    [[provider.bot]]\n\
                    target = \"ops\"\n\
                    name = \"Ops\"\n\
                    pin_order = 1\n\
                    glyph = \"x\"\n";
        let parsed = BotsFile::parse(text.as_bytes()).expect("parses");
        assert_eq!(parsed.extra.get("schema"), Some(&toml::Value::Integer(2)));
        assert_eq!(
            parsed.providers[0].extra.get("rate"),
            Some(&toml::Value::Integer(3))
        );
        assert_eq!(
            parsed.providers[0].bots[0].extra.get("glyph"),
            Some(&toml::Value::String("x".to_owned()))
        );
        let rendered = parsed.render().expect("renders");
        assert!(rendered.starts_with(BOTS_HEADER));
        assert!(rendered.contains("[[provider.bot]]"), "{rendered}");
        assert_eq!(
            BotsFile::parse(rendered.as_bytes()).expect("reparses"),
            parsed
        );

        let drives = DrivesFile {
            drives: vec![drive("https://github.com/acme/a", "A", &["mac"])],
            extra: Extra::new(),
        };
        let rendered = drives.render().expect("renders");
        assert!(rendered.contains("[[drive]]"), "{rendered}");
        assert_eq!(
            DrivesFile::parse(rendered.as_bytes()).expect("reparses"),
            drives
        );

        assert!(MatrixFile::parse(b"[[account]]\nkind = 1\n").is_err());
    }

    #[test]
    fn offers_are_what_the_repository_has_and_this_device_does_not() {
        let remote_drives = [
            drive("https://github.com/acme/a", "A", &["studio"]),
            drive("https://github.com/acme/b", "B", &["studio"]),
        ];
        let mine_drives = [drive("https://tg@GITHUB.com/acme/a.git/", "A", &[])];
        let remote_providers = [ProviderRecord {
            kind: "ollama".to_owned(),
            name: "Home".to_owned(),
            base_url: "http://localhost:11434".to_owned(),
            credential: "account".to_owned(),
            bots: vec![
                BotRecord {
                    target: "b".to_owned(),
                    name: "Second".to_owned(),
                    pin_order: 2,
                    ..BotRecord::default()
                },
                BotRecord {
                    target: "a".to_owned(),
                    name: "First".to_owned(),
                    pin_order: 1,
                    ..BotRecord::default()
                },
            ],
            devices: vec!["studio".to_owned()],
            ..ProviderRecord::default()
        }];
        let remote_matrix = [
            MatrixRecord {
                user_id: "@tg:acme.dev".to_owned(),
                homeserver_url: "https://matrix.acme.dev".to_owned(),
                kind: "password".to_owned(),
                devices: vec!["studio".to_owned()],
                ..MatrixRecord::default()
            },
            MatrixRecord {
                user_id: "@here:acme.dev".to_owned(),
                ..MatrixRecord::default()
            },
        ];
        let mine_matrix = [MatrixRecord {
            user_id: "@here:acme.dev".to_owned(),
            ..MatrixRecord::default()
        }];
        let offered = offers(
            &remote_drives,
            &remote_providers,
            &remote_matrix,
            &mine_drives,
            &[],
            &mine_matrix,
        );
        let drive_keys: Vec<&str> = offered.drives.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(drive_keys, ["drive:https://github.com/acme/b#main^B"]);
        assert_eq!(offered.providers.len(), 1);
        assert_eq!(
            offered.providers[0].key,
            "provider:ollama:http://localhost:11434"
        );
        assert_eq!(offered.providers[0].bots, ["First", "Second"], "pin order");
        let matrix_keys: Vec<&str> = offered.matrix.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(matrix_keys, ["matrix:@tg:acme.dev"]);

        let none = offers(
            &remote_drives,
            &remote_providers,
            &remote_matrix,
            &remote_drives,
            &remote_providers,
            &remote_matrix,
        );
        assert_eq!(none, AccountOffersVm::default());
    }

    #[test]
    fn two_drives_of_one_repository_stay_two() {
        let url = "https://git.acme.dev/tg/tgdrive";
        let mine = [drive(url, "tgdrive", &[]), drive(url, "tgdrive-light", &[])];
        let merged = merge_drives(&[], &mine, None, "hesperia", &[], true);
        assert_eq!(
            names(&merged),
            [
                ("tgdrive", vec!["hesperia"]),
                ("tgdrive-light", vec!["hesperia"])
            ]
        );

        // A device with only one of them keeps both in the file and is
        // offered the other, under a key that names it.
        let merged = merge_drives(&merged, &mine[..1], Some(&mine[..1]), "phone", &[], true);
        assert_eq!(
            names(&merged),
            [
                ("tgdrive", vec!["hesperia", "phone"]),
                ("tgdrive-light", vec!["hesperia"])
            ]
        );
        let offered = offers(&merged, &[], &[], &mine[..1], &[], &[]);
        let keys: Vec<&str> = offered.drives.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(
            keys,
            ["drive:https://git.acme.dev/tg/tgdrive#main^tgdrive-light"]
        );
    }

    #[test]
    fn an_older_record_without_a_matching_name_matches_the_one_drive_of_its_repository() {
        let url = "https://git.acme.dev/tg/tgdrive";
        let old = [drive(url, "Drive", &["studio"])];
        let one = [drive(url, "tgdrive", &[])];
        let merged = merge_drives(&old, &one, Some(&one), "hesperia", &[], true);
        assert_eq!(
            names(&merged),
            [("Drive", vec!["hesperia", "studio"])],
            "the same drive, not a second one"
        );
        assert!(offers(&old, &[], &[], &one, &[], &[]).drives.is_empty());

        // With two drives here the old record is ambiguous and stays its own.
        let two = [drive(url, "tgdrive", &[]), drive(url, "tgdrive-light", &[])];
        let merged = merge_drives(&old, &two, None, "hesperia", &[], true);
        assert_eq!(merged.len(), 3);
    }
}
