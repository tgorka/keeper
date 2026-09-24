//! The account's status as the UI renders it: one state, one Rust-authored
//! sentence, and the facts beside them. The shell gathers [`AccountFacts`];
//! [`vm`] decides what they mean, so the webview never composes a status.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::descriptor::AccountDescriptor;
use super::layout::{DeviceClass, DeviceEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AccountStateVm {
    /// No account is configured.
    None,
    SignedOut,
    SigningIn,
    Syncing,
    Ready,
    /// Signed in; the repository could not be reached, last settings applied.
    Offline,
    /// The grant is dead, or the repository needs connecting again.
    NeedsSignIn,
    /// Policy stopped it: a missing role, someone else's directory.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "OrgAccountVm")]
pub struct AccountVm {
    pub configured: bool,
    pub id: Option<String>,
    pub name: Option<String>,
    pub issuer_host: Option<String>,
    pub repo_host: Option<String>,
    /// `same`, `oauth` or `none`.
    pub repo_mode: Option<String>,
    pub state: AccountStateVm,
    pub sentence: Option<String>,
    pub identity: Option<AccountIdentityVm>,
    /// This device, once it is registered.
    pub device: Option<AccountDeviceVm>,
    pub devices: Vec<AccountDeviceVm>,
    #[ts(type = "number | null")]
    pub last_synced_ms: Option<i64>,
    pub forge_connected: bool,
    /// `account.toml` faults, one line each.
    pub faults: Vec<String>,
    /// Drives, bot providers and Matrix accounts the person uses on their
    /// other devices and not here. Empty without an account.
    pub offers: AccountOffersVm,
    /// What restoring this device from the account did, and what waits.
    /// Empty without an account.
    pub restore: AccountRestoreVm,
    /// Increases with every VM this process composes, so a subscriber that
    /// receives two out of order keeps the newer one.
    #[ts(type = "number")]
    pub revision: u64,
}

/// What the account's repository offers this device (AD-323). Nothing is
/// added by itself: a drive needs a folder, a Matrix account a sign-in, a
/// provider a key unless it uses the account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountOffersVm {
    pub drives: Vec<DriveOfferVm>,
    pub providers: Vec<ProviderOfferVm>,
    pub matrix: Vec<MatrixOfferVm>,
}

/// This device coming back from the account (AD-329), in Rust-composed
/// sentences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountRestoreVm {
    /// "Restored 2 drives, 1 bot provider and your settings from your
    /// account." once a restore ran.
    pub sentence: Option<String>,
    /// One "Waiting for {path} to restore {name}." per drive whose folder
    /// cannot be made yet.
    pub pending: Vec<String>,
    /// This device's file asks for the wake phrase and this device has it
    /// off: offer to turn listening on. Nothing turns it on by itself.
    pub listening_off: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriveOfferVm {
    /// `drive:<normalized remote>#<branch>^<name>`.
    pub key: String,
    pub name: String,
    pub remote_url: String,
    pub branch: String,
    /// `account`, `own` or `none`.
    pub credential: String,
    /// Each role's subfolder; `Some` means the role is on.
    pub notes: Option<String>,
    pub recordings: Option<String>,
    pub sessions: Option<String>,
    pub tasks: Option<String>,
    pub excludes: Vec<String>,
    #[ts(type = "number | null")]
    pub lfs_threshold_bytes: Option<u64>,
    pub virtual_patterns: Option<Vec<String>>,
    #[ts(type = "number | null")]
    pub virtual_over_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub release_ttl_ms: Option<u64>,
    pub tags: Vec<String>,
    pub commit_subject_template: Option<String>,
    /// The device slugs that use it.
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderOfferVm {
    /// `provider:<kind>:<normalized base URL>`.
    pub key: String,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    /// `account` or `own`.
    pub credential: String,
    /// Bot names, in pin order.
    pub bots: Vec<String>,
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MatrixOfferVm {
    /// `matrix:<user id>`.
    pub key: String,
    pub user_id: String,
    pub homeserver_url: String,
    /// `password`, `oidc` or `beeper`.
    pub kind: String,
    pub devices: Vec<String>,
}

/// The next [`AccountVm::revision`]: monotonic per process, never 0.
fn next_revision() -> u64 {
    static REVISION: AtomicU64 = AtomicU64::new(0);
    REVISION.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountIdentityVm {
    pub login: String,
    pub display_name: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountDeviceVm {
    pub slug: String,
    pub name: String,
    pub class: Option<DeviceClass>,
    pub platform: Option<String>,
    pub this_device: bool,
}

/// The confirmation sheet: everything a person needs to decide whether to
/// trust a setup link, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountSetupVm {
    pub setup_id: String,
    pub name: String,
    pub issuer_host: String,
    pub repo_host: String,
    pub repo_mode: String,
    pub device_name: String,
    pub device_class: DeviceClass,
    /// This device is already known on this install.
    pub registered: bool,
    /// The display name of the account this install has now, when confirming
    /// would replace it (sign it out and forget it here). `None` for a first
    /// setup, or for the same account under a new display name.
    pub replaces: Option<String>,
    /// The host GitHub access comes from, when the descriptor names a
    /// `[github_broker]`: the sheet says "Gets GitHub access from <host>".
    pub broker_host: Option<String>,
    /// Every other host a repository token would be sent to: the web and
    /// API hosts of each GitHub source the descriptor adds, in the switcher's
    /// order, once each. The account's own forge is `repo_host`.
    pub forge_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountShareVm {
    pub link: String,
    pub qr_svg: Option<String>,
}

/// What the shell is doing right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccountPhase {
    #[default]
    Idle,
    SigningIn,
    Syncing,
}

/// The last thing that went wrong, if it still stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountProblem {
    /// The repository (or issuer) could not be reached.
    Offline,
    /// The sign-in needs repeating; the message is the reason, for logs.
    NeedsSignIn(String),
    /// A policy refusal, already a sentence ([`super::AccountError::Refused`]).
    Refused(String),
    /// `<login>/user.toml` records someone else.
    Blocked {
        login: String,
        recorded: Option<String>,
    },
    /// A sync failed for a reason that is neither offline nor policy.
    Failed(String),
}

/// Whatever the shell knows. `now_ms` makes the offline sentence's time
/// deterministic to test.
#[derive(Debug, Clone, Default)]
pub struct AccountFacts {
    pub descriptor: Option<AccountDescriptor>,
    pub faults: Vec<String>,
    pub identity: Option<AccountIdentityVm>,
    pub phase: AccountPhase,
    pub problem: Option<AccountProblem>,
    pub devices: Vec<DeviceEntry>,
    /// This device's slug.
    pub this_device: Option<String>,
    pub last_synced_ms: Option<i64>,
    pub now_ms: i64,
    pub forge_connected: bool,
    /// `oauth` mode: the repository needs its own connection.
    pub forge_needed: bool,
    /// What the last sync found on the person's other devices.
    pub offers: AccountOffersVm,
    /// What restoring this device did and what waits.
    pub restore: RestoreFacts,
}

/// The restore's outcome as the shell knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreFacts {
    /// `(drives, bot providers)` restored, once a restore ran.
    pub restored: Option<(usize, usize)>,
    /// `(path, name)` of each drive waiting for its folder's parent.
    pub waiting: Vec<(String, String)>,
    /// See [`AccountRestoreVm::listening_off`].
    pub listening_off: bool,
}

/// Compose the state and its sentence. Precedence, first match wins: a
/// sign-in in progress; a policy stop; a dead grant; a sync in progress; a
/// repository that needs connecting; offline or a failed sync; up to date.
pub fn vm(facts: &AccountFacts) -> AccountVm {
    let devices: Vec<AccountDeviceVm> = facts
        .devices
        .iter()
        .map(|entry| AccountDeviceVm {
            slug: entry.slug.clone(),
            name: entry.name.clone(),
            class: entry.class,
            platform: entry.platform.clone(),
            this_device: facts.this_device.as_deref() == Some(entry.slug.as_str()),
        })
        .collect();
    let device = devices.iter().find(|d| d.this_device).cloned();

    let Some(d) = &facts.descriptor else {
        let sentence = (!facts.faults.is_empty()).then(|| {
            "keeper could not use account.toml, so no account is set up. Fix the file or paste a setup link again."
                .to_owned()
        });
        return AccountVm {
            configured: false,
            id: None,
            name: None,
            issuer_host: None,
            repo_host: None,
            repo_mode: None,
            state: AccountStateVm::None,
            sentence,
            identity: None,
            device: None,
            devices: Vec::new(),
            last_synced_ms: None,
            forge_connected: false,
            faults: facts.faults.clone(),
            offers: AccountOffersVm::default(),
            restore: AccountRestoreVm::default(),
            revision: next_revision(),
        };
    };

    let (state, sentence) = compose(facts, &d.name);
    AccountVm {
        configured: true,
        id: Some(d.id.clone()),
        name: Some(d.name.clone()),
        issuer_host: Some(d.issuer_host()),
        repo_host: Some(d.repo_host()),
        repo_mode: Some(d.repo_mode().to_owned()),
        state,
        sentence: Some(sentence),
        identity: facts.identity.clone(),
        device,
        devices,
        last_synced_ms: facts.last_synced_ms,
        forge_connected: facts.forge_connected,
        faults: facts.faults.clone(),
        offers: facts.offers.clone(),
        restore: restore_vm(&facts.restore),
        revision: next_revision(),
    }
}

fn restore_vm(facts: &RestoreFacts) -> AccountRestoreVm {
    let counted = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let sentence = facts.restored.map(|(drives, providers)| {
        let mut parts: Vec<String> = [
            counted(drives, "drive", "drives"),
            counted(providers, "bot provider", "bot providers"),
        ]
        .into_iter()
        .flatten()
        .collect();
        parts.push("your settings".to_owned());
        let last = parts.pop().unwrap_or_default();
        let list = if parts.is_empty() {
            last
        } else {
            format!("{} and {last}", parts.join(", "))
        };
        format!("Restored {list} from your account.")
    });
    AccountRestoreVm {
        sentence,
        pending: facts
            .waiting
            .iter()
            .map(|(path, name)| format!("Waiting for {path} to restore {name}."))
            .collect(),
        listening_off: facts.listening_off,
    }
}

fn compose(facts: &AccountFacts, name: &str) -> (AccountStateVm, String) {
    use AccountStateVm as S;

    if facts.phase == AccountPhase::SigningIn {
        return (
            S::SigningIn,
            format!("Finish signing in to {name} in the browser."),
        );
    }
    match &facts.problem {
        Some(AccountProblem::Refused(sentence)) => return (S::Blocked, sentence.clone()),
        Some(AccountProblem::Blocked { login, .. }) => {
            return (
                S::Blocked,
                format!(
                    "This sign-in belongs to someone else: {login}/user.toml records a different account. Settings from the repository were not loaded."
                ),
            );
        }
        Some(AccountProblem::NeedsSignIn(_)) => {
            return (
                S::NeedsSignIn,
                "Sign in again to keep your settings in sync.".to_owned(),
            );
        }
        _ => {}
    }
    if facts.identity.is_none() {
        return (
            S::SignedOut,
            format!("Sign in to {name} to use your settings on this device."),
        );
    }
    if facts.phase == AccountPhase::Syncing {
        return (S::Syncing, "Syncing your settings…".to_owned());
    }
    if facts.forge_needed && !facts.forge_connected {
        return (
            S::NeedsSignIn,
            "Reconnect the repository to keep your settings in sync.".to_owned(),
        );
    }
    let since = facts
        .last_synced_ms
        .map(|at| format!("from {}", clock(at, facts.now_ms)))
        .unwrap_or_else(|| "kept on this device".to_owned());
    match &facts.problem {
        Some(AccountProblem::Offline) => (S::Offline, format!("Offline — using settings {since}.")),
        Some(AccountProblem::Failed(reason)) => (
            S::Offline,
            format!("Your settings could not be synced ({reason}). Using settings {since}."),
        ),
        _ if facts.last_synced_ms.is_none() => (
            S::Syncing,
            "Your settings have not been synced yet.".to_owned(),
        ),
        _ => (S::Ready, "Up to date.".to_owned()),
    }
}

/// `14:02` today, `22 Sep, 14:02` before today — local time.
fn clock(at_ms: i64, now_ms: i64) -> String {
    let (Some(at), Some(now)) = (
        Local.timestamp_millis_opt(at_ms).single(),
        Local.timestamp_millis_opt(now_ms).single(),
    ) else {
        return "the last sync".to_owned();
    };
    if at.date_naive() == now.date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%-d %b, %H:%M").to_string()
    }
}

/// The confirmation sheet for a resolved setup input. `previous` is the
/// descriptor this install has now, if any.
pub fn setup_vm(
    setup_id: String,
    d: &AccountDescriptor,
    previous: Option<&AccountDescriptor>,
    device_name: String,
    device_class: DeviceClass,
    registered: bool,
) -> AccountSetupVm {
    AccountSetupVm {
        setup_id,
        name: d.name.clone(),
        issuer_host: d.issuer_host(),
        repo_host: d.repo_host(),
        repo_mode: d.repo_mode().to_owned(),
        device_name,
        device_class,
        registered,
        replaces: previous
            .filter(|previous| d.replaces(previous))
            .map(|previous| previous.name.clone()),
        broker_host: d.github_broker.as_ref().map(|broker| broker.host()),
        forge_hosts: forge_hosts(d),
    }
}

fn forge_hosts(d: &AccountDescriptor) -> Vec<String> {
    use crate::forges::{sources, TokenVia, BUILTIN_GITHUB_CLIENT_ID};
    let mut hosts: Vec<String> = Vec::new();
    for source in sources(Some(d), BUILTIN_GITHUB_CLIENT_ID) {
        if source.via == TokenVia::AccountForge {
            continue;
        }
        for url in [&source.web_base, &source.api_base] {
            let host = crate::forges::host_of(url);
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::org_account::descriptor::parse_json;

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    fn descriptor() -> AccountDescriptor {
        parse_json(
            r#"{ "version": 1, "id": "acme", "name": "Acme",
                 "auth": { "issuer": "https://id.acme.dev", "client_id": "keeper" },
                 "config": { "url": "https://git.acme.dev/people/keeper-config.git" } }"#,
        )
        .expect("descriptor")
    }

    fn signed_in() -> AccountFacts {
        AccountFacts {
            descriptor: Some(descriptor()),
            identity: Some(AccountIdentityVm {
                login: "tgorka".to_owned(),
                display_name: "Tomasz Gorka".to_owned(),
                email: None,
                roles: vec!["keeper".to_owned()],
            }),
            last_synced_ms: Some(1_790_000_000_000),
            now_ms: 1_790_000_000_000,
            ..AccountFacts::default()
        }
    }

    #[test]
    fn every_composed_vm_is_newer_than_the_one_before() {
        let first = vm(&signed_in());
        let second = vm(&AccountFacts::default());
        let third = vm(&signed_in());
        assert!(first.revision > 0);
        assert!(second.revision > first.revision);
        assert!(third.revision > second.revision);
    }

    #[test]
    fn no_descriptor_is_no_account_and_a_broken_file_says_so() {
        let vm = vm(&AccountFacts::default());
        assert!(!vm.configured);
        assert_eq!((vm.state, vm.sentence), (AccountStateVm::None, None));

        let broken = super::vm(&AccountFacts {
            faults: vec!["account.toml:3: expected a value".to_owned()],
            ..AccountFacts::default()
        });
        assert_eq!(broken.state, AccountStateVm::None);
        assert!(broken.sentence.is_some());
        assert_eq!(broken.faults.len(), 1);
    }

    #[test]
    fn a_configured_account_without_a_sign_in_is_signed_out() {
        let vm = vm(&AccountFacts {
            descriptor: Some(descriptor()),
            ..AccountFacts::default()
        });
        assert_eq!(vm.state, AccountStateVm::SignedOut);
        assert_eq!(vm.issuer_host.as_deref(), Some("id.acme.dev"));
        assert_eq!(vm.repo_host.as_deref(), Some("git.acme.dev"));
        assert_eq!(vm.repo_mode.as_deref(), Some("same"));
    }

    #[test]
    fn policy_stops_outrank_everything_but_a_sign_in_in_progress() {
        let mut facts = signed_in();
        facts.phase = AccountPhase::Syncing;
        facts.problem = Some(AccountProblem::Blocked {
            login: "tgorka".to_owned(),
            recorded: Some("someone-else".to_owned()),
        });
        let blocked = vm(&facts);
        assert_eq!(blocked.state, AccountStateVm::Blocked);
        assert!(blocked
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains("tgorka/user.toml")));

        facts.problem = Some(AccountProblem::Refused("no role".to_owned()));
        assert_eq!(vm(&facts).sentence.as_deref(), Some("no role"));

        facts.phase = AccountPhase::SigningIn;
        assert_eq!(vm(&facts).state, AccountStateVm::SigningIn);
    }

    #[test]
    fn a_dead_grant_outranks_a_sync_and_a_sync_outranks_offline() {
        let mut facts = signed_in();
        facts.phase = AccountPhase::Syncing;
        facts.problem = Some(AccountProblem::NeedsSignIn("refresh rejected".to_owned()));
        assert_eq!(vm(&facts).state, AccountStateVm::NeedsSignIn);

        facts.problem = Some(AccountProblem::Offline);
        assert_eq!(vm(&facts).state, AccountStateVm::Syncing);

        facts.phase = AccountPhase::Idle;
        let offline = vm(&facts);
        assert_eq!(offline.state, AccountStateVm::Offline);
        let sentence = offline.sentence.expect("sentence");
        assert!(
            sentence.starts_with("Offline — using settings from "),
            "{sentence}"
        );
        assert!(
            !sentence.contains(", "),
            "same day shows the time only: {sentence}"
        );

        facts.now_ms += 3 * DAY_MS;
        let older = vm(&facts).sentence.expect("sentence");
        assert!(older.contains(", "), "an older sync names its day: {older}");
    }

    #[test]
    fn an_unconnected_forge_needs_sign_in_and_a_clean_sync_is_up_to_date() {
        let mut facts = signed_in();
        assert_eq!(
            (vm(&facts).state, vm(&facts).sentence.as_deref()),
            (AccountStateVm::Ready, Some("Up to date."))
        );
        facts.forge_needed = true;
        assert_eq!(vm(&facts).state, AccountStateVm::NeedsSignIn);
        facts.forge_connected = true;
        assert_eq!(vm(&facts).state, AccountStateVm::Ready);
    }

    #[test]
    fn this_device_is_marked_among_the_devices() {
        let mut facts = signed_in();
        facts.devices = ["home-mac", "work-mac"]
            .map(|slug| DeviceEntry {
                slug: slug.to_owned(),
                name: slug.to_owned(),
                class: Some(DeviceClass::Desktop),
                platform: None,
            })
            .to_vec();
        facts.this_device = Some("work-mac".to_owned());
        let vm = vm(&facts);
        assert_eq!(vm.device.map(|d| d.slug).as_deref(), Some("work-mac"));
        assert_eq!(vm.devices.iter().filter(|d| d.this_device).count(), 1);
    }

    #[test]
    fn the_restore_line_counts_what_came_back_and_names_what_waits() {
        let restored = |drives, providers| {
            let mut facts = signed_in();
            facts.restore.restored = Some((drives, providers));
            vm(&facts).restore.sentence
        };
        assert_eq!(
            restored(2, 1).as_deref(),
            Some("Restored 2 drives, 1 bot provider and your settings from your account.")
        );
        assert_eq!(
            restored(1, 0).as_deref(),
            Some("Restored 1 drive and your settings from your account.")
        );
        assert_eq!(
            restored(0, 3).as_deref(),
            Some("Restored 3 bot providers and your settings from your account.")
        );
        assert_eq!(
            restored(0, 0).as_deref(),
            Some("Restored your settings from your account.")
        );

        let mut facts = signed_in();
        facts.restore.waiting = vec![("/Volumes/T7/tgdrive".to_owned(), "tgdrive".to_owned())];
        facts.restore.listening_off = true;
        let restore = vm(&facts).restore;
        assert_eq!(restore.sentence, None, "no restore ran");
        assert_eq!(
            restore.pending,
            ["Waiting for /Volumes/T7/tgdrive to restore tgdrive."]
        );
        assert!(restore.listening_off);

        facts.descriptor = None;
        assert_eq!(
            vm(&facts).restore,
            AccountRestoreVm::default(),
            "no account, nothing"
        );
    }

    fn setup(previous: Option<&AccountDescriptor>, new: &AccountDescriptor) -> AccountSetupVm {
        setup_vm(
            "s".to_owned(),
            new,
            previous,
            "mac".to_owned(),
            DeviceClass::Desktop,
            false,
        )
    }

    /// The sheet names the account a link would end, and only then.
    #[test]
    fn the_setup_sheet_names_the_account_it_would_replace() {
        let old = descriptor();
        assert_eq!(
            setup(None, &old).replaces,
            None,
            "a first setup replaces nothing"
        );
        assert_eq!(
            setup(Some(&old), &old).replaces,
            None,
            "the same account again"
        );
        let renamed = AccountDescriptor {
            name: "Acme Corp".to_owned(),
            ..descriptor()
        };
        assert_eq!(
            setup(Some(&old), &renamed).replaces,
            None,
            "a new display name only"
        );
        let other = AccountDescriptor {
            id: "globex".to_owned(),
            name: "Globex".to_owned(),
            ..descriptor()
        };
        assert_eq!(setup(Some(&old), &other).replaces.as_deref(), Some("Acme"));
    }

    /// A setup under the same id that points anywhere else ends the old
    /// session; only a new display name keeps it.
    #[test]
    fn any_sign_in_or_repository_change_replaces_the_account() {
        let old = descriptor();
        let renamed = AccountDescriptor {
            name: "Acme Corp".to_owned(),
            ..descriptor()
        };
        assert!(!renamed.replaces(&old));
        assert!(!descriptor().replaces(&old));

        let mut other_issuer = descriptor();
        other_issuer.auth.issuer = "https://id.evil.example".to_owned();
        let mut other_token = descriptor();
        other_token.auth.endpoints.token = Some("https://id.acme.dev/other-token".to_owned());
        let mut other_repo = descriptor();
        other_repo.config.url = "https://git.evil.example/keeper-config.git".to_owned();
        let other_id = AccountDescriptor {
            id: "globex".to_owned(),
            ..descriptor()
        };
        for new in [other_issuer, other_token, other_repo, other_id] {
            assert!(new.replaces(&old), "{new:?}");
        }
    }

    /// Every host a repository token would reach is on the sheet before the
    /// person confirms.
    #[test]
    fn the_setup_sheet_names_every_forge_host_a_token_would_reach() {
        assert!(setup(None, &descriptor()).forge_hosts.is_empty());
        let mut d = descriptor();
        d.forges = serde_json::from_value(serde_json::json!([
            { "kind": "github", "id": "ghe", "web_base": "https://ghe.acme.dev",
              "api_base": "https://ghe.acme.dev/api/v3", "client_id": "Iv1.ghe" },
            { "kind": "github", "id": "work", "web_base": "https://work.acme.dev",
              "api_base": "https://work.acme.dev/api/v3", "client_id": "Iv1.w" },
            // No broker and no client: keeper never sends it a token.
            { "kind": "github", "id": "idle", "web_base": "https://idle.acme.dev",
              "api_base": "https://idle.acme.dev/api/v3" }
        ]))
        .expect("forges");
        assert_eq!(
            setup(None, &d).forge_hosts,
            ["ghe.acme.dev", "work.acme.dev"]
        );
        d.github_broker = Some(crate::org_account::descriptor::GithubBroker {
            url: "https://b.acme.dev".to_owned(),
            app: None,
        });
        assert_eq!(
            setup(None, &d).forge_hosts,
            [
                "ghe.acme.dev",
                "work.acme.dev",
                "github.com",
                "api.github.com"
            ]
        );
    }
}
