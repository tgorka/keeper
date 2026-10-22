//! The account's status as the UI renders it: one state, one Rust-authored
//! sentence, and the facts beside them. The shell gathers [`AccountFacts`];
//! [`vm`] decides what they mean, so the webview never composes a status.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::descriptor::AccountDescriptor;
use super::layout::{DeviceClass, DeviceEntry};
use super::AccountError;

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
    /// The sentence reports something that went wrong (R381): the UI renders
    /// it as an alert, never as a quiet status line.
    pub problem: bool,
    /// How the person's sign-in attempt in this process ended — or that it
    /// is still running — never inferred from `state` (R387): a closed
    /// re-setup of an account already signed in here is `closed`, even
    /// while the state still reads up to date. `None` before any attempt.
    pub attempt: Option<AccountAttemptVm>,
    pub identity: Option<AccountIdentityVm>,
    /// This device, once it is registered.
    pub device: Option<AccountDeviceVm>,
    pub devices: Vec<AccountDeviceVm>,
    #[ts(type = "number | null")]
    pub last_synced_ms: Option<i64>,
    pub forge_connected: bool,
    /// `account.toml` faults, one line each.
    pub faults: Vec<String>,
    /// Drives, model providers and Matrix accounts the person uses on their
    /// other devices and not here. Empty without an account.
    pub offers: AccountOffersVm,
    /// What restoring this device from the account did, and what waits.
    /// Empty without an account.
    pub restore: AccountRestoreVm,
    /// The authorization page the sign-in in progress waits on, for the
    /// person to copy into a browser that did not come forward (story 102.9,
    /// R470). Only while it waits: a sign-in that ended took its `state`
    /// with it, and its link would answer nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sign_in_link: Option<crate::sign_in_link::SignInLinkVm>,
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
    /// "Restored 2 drives, 1 model provider and your settings from your
    /// account." once a restore ran.
    pub sentence: Option<String>,
    /// One "Waiting for {path} to restore {name}." per drive whose folder
    /// cannot be made yet.
    pub pending: Vec<String>,
    /// This device's file asks for the wake phrase and this device has it
    /// off: offer to turn listening on. Nothing turns it on by itself.
    pub listening_off: bool,
    /// The Matrix single sign-on the restore started by itself, once it
    /// started: its sentence, and its link while it waits (story 102.9,
    /// R480/02).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub matrix_sign_in: Option<RestoreSignInVm>,
}

/// The restore's Matrix sign-in, as Settings › Account says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestoreSignInVm {
    pub sentence: String,
    /// The sentence reports a failure (R381).
    pub problem: bool,
    /// It still waits for the browser: Cancel ends it.
    pub waiting: bool,
    /// Its authorization page, to copy into a browser that did not come
    /// forward; only while it waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub link: Option<crate::sign_in_link::SignInLinkVm>,
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
    pub voices: Option<String>,
    pub agents: Option<String>,
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
    /// The keeper release the device last published it runs.
    pub version: Option<String>,
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

/// How a sign-in the person started (a setup's Continue, "Sign in again")
/// ended (Epic 100, R387, R410): the outcome of its run through
/// `org_account::attempt`, closed whether the person closed the browser
/// sheet or pressed Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AccountAttemptVm {
    /// Still signing in or syncing.
    Running,
    /// Signed in and synced.
    Done,
    /// The person closed the sign-in (the browser sheet, or Cancel): quiet.
    Closed,
    /// A step failed; the account's sentence says which and why.
    Failed,
}

impl AccountAttemptVm {
    /// How an attempt that stopped on `error` ended: closed when the person
    /// closed it, failed otherwise.
    pub fn of_error(error: &AccountError) -> Self {
        match error {
            AccountError::Cancelled => Self::Closed,
            _ => Self::Failed,
        }
    }
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
    /// A sign-in's server did not answer before the browser opened; the
    /// sentence names it ([`AccountError::NoAnswer`]). Otherwise offline.
    NoAnswer(String),
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
    /// The identity provider refused the sign-in (`access_denied`), with its
    /// own words when it sent any ([`AccountError::Denied`]).
    SignInDenied {
        host: String,
        reason: Option<String>,
    },
    /// The sign-in timed out before the browser answered.
    SignInTimedOut,
    /// The browser round trip broke; the platform's reason.
    SignInInterrupted(String),
}

impl AccountProblem {
    /// The closed name of this problem's kind, for a log line.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::NoAnswer(_) => "no_answer",
            Self::NeedsSignIn(_) => "needs_sign_in",
            Self::Refused(_) => "refused",
            Self::Blocked { .. } => "blocked",
            Self::Failed(_) => "failed",
            Self::SignInDenied { .. } => "denied",
            Self::SignInTimedOut => "timed_out",
            Self::SignInInterrupted(_) => "interrupted",
        }
    }
}

/// What an account error leaves standing for the person. A sign-in the
/// person closed themselves is no problem (R381: "closed" stays quiet);
/// every other error is one, with its own sentence ([`vm`]).
pub fn problem_of(error: AccountError) -> Option<AccountProblem> {
    match error {
        AccountError::Cancelled => None,
        AccountError::Unreachable(_) => Some(AccountProblem::Offline),
        AccountError::NoAnswer(sentence) => Some(AccountProblem::NoAnswer(sentence)),
        AccountError::NeedsSignIn(sentence) => Some(AccountProblem::NeedsSignIn(sentence)),
        AccountError::Refused(sentence) => Some(AccountProblem::Refused(sentence)),
        AccountError::Internal(sentence) => Some(AccountProblem::Failed(sentence)),
        AccountError::Denied { host, reason } => {
            Some(AccountProblem::SignInDenied { host, reason })
        }
        AccountError::TimedOut => Some(AccountProblem::SignInTimedOut),
        AccountError::Interrupted(reason) => Some(AccountProblem::SignInInterrupted(reason)),
    }
}

/// The step of setting up or syncing an account a failure came from, as the
/// log line names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStep {
    /// keeper's own HTTP client could not be built.
    Client,
    SignIn,
    ForgeConnect,
    /// The repository's credential (a token, a refresh).
    Credential,
    /// The local folder the repository is cloned into.
    Folder,
    Fetch,
    Publish,
    Settings,
    /// A stored session that did not restore at launch or at Continue.
    Restore,
}

impl AccountStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::SignIn => "sign_in",
            Self::ForgeConnect => "forge_connect",
            Self::Credential => "credential",
            Self::Folder => "folder",
            Self::Fetch => "fetch",
            Self::Publish => "publish",
            Self::Settings => "settings",
            Self::Restore => "restore",
        }
    }
}

/// One failed step, logged once at WARN and turned into what it leaves
/// standing (R381, 100.1 #3). The line carries the step, the account's id
/// and the error's kind — never its sentence, a token or a URL with a code —
/// so it is safe in `keeper.log` whatever the identity provider said.
pub fn step_failed(
    step: AccountStep,
    account: &str,
    error: AccountError,
) -> Option<AccountProblem> {
    tracing::warn!(
        step = step.as_str(),
        account,
        kind = error.kind(),
        "account: a step did not finish"
    );
    problem_of(error)
}

/// A problem the shell composed itself (a repository error, a folder it
/// could not make), logged like [`step_failed`] and handed back.
pub fn step_problem(
    step: AccountStep,
    account: &str,
    problem: Option<AccountProblem>,
) -> Option<AccountProblem> {
    if let Some(problem) = &problem {
        tracing::warn!(
            step = step.as_str(),
            account,
            kind = problem.kind(),
            "account: a step did not finish"
        );
    }
    problem
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
    /// See [`AccountVm::attempt`].
    pub attempt: Option<AccountAttemptVm>,
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
    /// The link the current sign-in showed, as the platform last gave it.
    pub sign_in_link: Option<crate::sign_in_link::SignInLinkVm>,
}

/// The restore's outcome as the shell knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreFacts {
    /// `(drives, model providers)` restored, once a restore ran.
    pub restored: Option<(usize, usize)>,
    /// `(path, name)` of each drive waiting for its folder's parent.
    pub waiting: Vec<(String, String)>,
    /// See [`AccountRestoreVm::listening_off`].
    pub listening_off: bool,
    /// The single sign-on the restore started for a Matrix account, once it
    /// started (story 102.9, R480/02).
    pub matrix_sign_in: Option<RestoreSignIn>,
}

/// The Matrix single sign-on a restore starts by itself (AD-329): the
/// account's own surface carries its link and its outcome, apart from the
/// account's sign-in (`phase`), which is another flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSignIn {
    /// The attempt that owns this row ([`RestoreFacts::begin_matrix_sign_in`]):
    /// only its links and its ending are taken (R500).
    owner: u64,
    /// Where it signs in, as the account's file recorded it.
    pub homeserver: String,
    /// Its authorization page, while it waits.
    pub link: crate::sign_in_link::SignInLinkSlot,
    /// How it ended; `None` while it waits.
    pub ended: Option<RestoreSignInEnd>,
}

/// One restore-started Matrix sign-in, from before it is spawned until it
/// ends (story 102.9, R500). It owns its row in [`RestoreFacts`] while that
/// row is still its own: a later restore's attempt, or the account's
/// replacement, takes the row, and from then on this attempt's late link and
/// ending change nothing. It also owns Cancel from its creation — the latch
/// is taken before the row offers Cancel, so a Cancel during server
/// discovery, before the flow is registered, still ends it.
#[derive(Debug)]
pub struct RestoreSignInAttempt {
    owner: u64,
    cancel: crate::oauth::CancelLatch,
}

/// How a restore sign-in running on its own reaches the account's facts: the
/// shell's account state behind its lock (one `update`), or a test's.
pub type RestoreFactsAccess =
    std::sync::Arc<dyn Fn(&mut dyn FnMut(&mut RestoreFacts)) + Send + Sync>;

/// Where one attempt's authorization page goes: its own row, never another
/// attempt's.
#[derive(Clone)]
pub struct RestoreSignInLinks {
    owner: u64,
    facts: RestoreFactsAccess,
}

/// How the restore's Matrix sign-in ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreSignInEnd {
    SignedIn,
    /// The person pressed Cancel (or closed the sheet).
    Cancelled,
    /// Its deadline passed: its link no longer works (R481).
    TimedOut,
    /// Its server did not answer before any browser opened; the sentence
    /// names it ([`crate::oauth::no_answer`]). There never was a link.
    NoAnswer(String),
    /// Anything else, with keeper's sentence for it.
    Failed(String),
}

/// Compose the state and its sentence. Precedence, first match wins: a
/// sign-in in progress; a policy stop; a dead grant; a sync in progress; a
/// repository that needs connecting — said as the failure when connecting it
/// was tried and failed; offline or a failed sync; up to date.
pub fn vm(facts: &AccountFacts) -> AccountVm {
    let devices: Vec<AccountDeviceVm> = facts
        .devices
        .iter()
        .map(|entry| AccountDeviceVm {
            slug: entry.slug.clone(),
            name: entry.name.clone(),
            class: entry.class,
            platform: entry.platform.clone(),
            version: entry.version.clone(),
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
            problem: sentence.is_some(),
            sentence,
            attempt: facts.attempt,
            identity: None,
            device: None,
            devices: Vec::new(),
            last_synced_ms: None,
            forge_connected: false,
            faults: facts.faults.clone(),
            offers: AccountOffersVm::default(),
            restore: AccountRestoreVm::default(),
            sign_in_link: None,
            revision: next_revision(),
        };
    };

    let (state, sentence, problem) = compose(facts, &d.name);
    AccountVm {
        configured: true,
        id: Some(d.id.clone()),
        name: Some(d.name.clone()),
        issuer_host: Some(d.issuer_host()),
        repo_host: Some(d.repo_host()),
        repo_mode: Some(d.repo_mode().to_owned()),
        state,
        sentence: Some(sentence),
        problem,
        attempt: facts.attempt,
        identity: facts.identity.clone(),
        device,
        devices,
        last_synced_ms: facts.last_synced_ms,
        forge_connected: facts.forge_connected,
        faults: facts.faults.clone(),
        offers: facts.offers.clone(),
        restore: restore_vm(&facts.restore),
        sign_in_link: facts.sign_in_link.clone(),
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
            counted(providers, "model provider", "model providers"),
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
        matrix_sign_in: facts.matrix_sign_in.as_ref().map(RestoreSignIn::vm),
    }
}

impl RestoreFacts {
    /// Start the restore's Matrix sign-in to `homeserver` on `flows` — the
    /// Matrix sign-in registry `cancel_oidc` drains. Call it before spawning
    /// the sign-in: the new attempt owns the row from here on, and Cancel
    /// reaches it from here on, before its flow is even registered (R500).
    /// An earlier attempt still running loses the row.
    pub fn begin_matrix_sign_in(
        &mut self,
        homeserver: &str,
        flows: &crate::oauth::OAuthFlowRegistry,
    ) -> RestoreSignInAttempt {
        static OWNERS: AtomicU64 = AtomicU64::new(1);
        let owner = OWNERS.fetch_add(1, Ordering::Relaxed);
        let cancel = flows.cancel_latch();
        self.matrix_sign_in = Some(RestoreSignIn {
            owner,
            homeserver: homeserver.to_owned(),
            link: crate::sign_in_link::SignInLinkSlot::default(),
            ended: None,
        });
        RestoreSignInAttempt { owner, cancel }
    }

    /// The row, while `owner` still owns it.
    fn matrix_sign_in_of(&mut self, owner: u64) -> Option<&mut RestoreSignIn> {
        self.matrix_sign_in
            .as_mut()
            .filter(|sign_in| sign_in.owner == owner)
    }
}

impl RestoreSignInAttempt {
    /// The latch to hand the sign-in (`auth::login_oidc`), taken when the
    /// attempt began.
    pub fn cancel(&self) -> crate::oauth::CancelLatch {
        self.cancel.clone()
    }

    /// Run `sign_in` — given where its links go — and record how it ended,
    /// both only while this attempt still owns its row. The ending's
    /// sentence is on the account's surface; a failure's WARN is here (R381).
    pub async fn run<F, Fut>(self, facts: RestoreFactsAccess, sign_in: F)
    where
        F: FnOnce(RestoreSignInLinks) -> Fut,
        Fut: Future<Output = Result<(), crate::error::CoreError>>,
    {
        use crate::error::{AuthError, CoreError};

        let links = RestoreSignInLinks {
            owner: self.owner,
            facts: facts.clone(),
        };
        let end = match sign_in(links).await {
            Ok(()) => RestoreSignInEnd::SignedIn,
            Err(CoreError::Auth(AuthError::OAuthCancelled)) => {
                tracing::info!("account: the restored Matrix sign-in was cancelled");
                RestoreSignInEnd::Cancelled
            }
            Err(error) => {
                tracing::warn!(reason = %error, "account: the restored Matrix sign-in did not finish");
                match error {
                    CoreError::Auth(AuthError::OAuthTimedOut) => RestoreSignInEnd::TimedOut,
                    CoreError::Auth(AuthError::NoAnswer(sentence)) => {
                        RestoreSignInEnd::NoAnswer(sentence)
                    }
                    error => RestoreSignInEnd::Failed(error.to_string()),
                }
            }
        };
        let owner = self.owner;
        (*facts)(&mut |restore| {
            if let Some(sign_in) = restore.matrix_sign_in_of(owner) {
                sign_in.ended = Some(end.clone());
            }
        });
    }
}

impl RestoreSignInLinks {
    /// One `Platform::show_sign_in_link` call of this attempt's sign-in.
    pub fn show(&self, flow: u64, link: Option<&crate::sign_in_link::SignInLinkVm>) {
        (*self.facts)(&mut |restore| {
            if let Some(sign_in) = restore.matrix_sign_in_of(self.owner) {
                sign_in.link.apply(flow, link);
            }
        });
    }
}

impl RestoreSignIn {
    fn vm(&self) -> RestoreSignInVm {
        let host = url::Url::parse(&self.homeserver)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| self.homeserver.clone());
        let again = "Sign in to it again from Add an account.";
        let (sentence, problem) = match &self.ended {
            None => (
                format!("Finish signing in to your Matrix account on {host} in the browser."),
                false,
            ),
            Some(RestoreSignInEnd::SignedIn) => (
                format!("Signed in to your Matrix account on {host}."),
                false,
            ),
            Some(RestoreSignInEnd::Cancelled) => (
                format!("The sign-in to your Matrix account on {host} was cancelled. {again}"),
                false,
            ),
            Some(RestoreSignInEnd::TimedOut) => (
                format!(
                    "Signing in to your Matrix account on {host} timed out before the browser answered, so its link no longer works. {again}"
                ),
                true,
            ),
            Some(RestoreSignInEnd::NoAnswer(sentence)) => (format!("{sentence} {again}"), true),
            Some(RestoreSignInEnd::Failed(reason)) => (
                format!(
                    "Signing in to your Matrix account on {host} did not finish ({reason}). {again}"
                ),
                true,
            ),
        };
        RestoreSignInVm {
            sentence,
            problem,
            waiting: self.ended.is_none(),
            link: self.link.link().cloned(),
        }
    }
}

/// The state, its sentence, and whether that sentence reports a problem.
fn compose(facts: &AccountFacts, name: &str) -> (AccountStateVm, String, bool) {
    use AccountStateVm as S;

    if facts.phase == AccountPhase::SigningIn {
        return (
            S::SigningIn,
            format!("Finish signing in to {name} in the browser."),
            false,
        );
    }
    match &facts.problem {
        Some(AccountProblem::Refused(sentence)) => return (S::Blocked, sentence.clone(), true),
        Some(AccountProblem::Blocked { login, .. }) => {
            return (
                S::Blocked,
                format!(
                    "This sign-in belongs to someone else: {login}/user.toml records a different account. Settings from the repository were not loaded."
                ),
                true,
            );
        }
        Some(AccountProblem::SignInDenied { host, reason }) => {
            let sentence = match reason {
                Some(reason) => format!("{host} refused to sign you in to {name}: {reason}."),
                None => format!(
                    "{host} refused to sign you in to {name}. Ask whoever runs {name} to let your account use keeper."
                ),
            };
            return (S::Blocked, sentence, true);
        }
        Some(AccountProblem::SignInTimedOut) => {
            return (
                S::NeedsSignIn,
                format!(
                    "Signing in to {name} timed out before the browser answered. Sign in again."
                ),
                true,
            );
        }
        Some(AccountProblem::SignInInterrupted(reason)) => {
            return (
                S::NeedsSignIn,
                format!(
                    "Signing in to {name} did not come back from the browser ({reason}). Sign in again."
                ),
                true,
            );
        }
        Some(AccountProblem::NeedsSignIn(_)) => {
            return (
                S::NeedsSignIn,
                "Sign in again to keep your settings in sync.".to_owned(),
                true,
            );
        }
        _ => {}
    }
    if facts.identity.is_none() {
        // Nobody is signed in, so a sign-in is what failed: say that, never
        // the neutral instruction alone (R381, 100.1 #4).
        return match &facts.problem {
            Some(AccountProblem::Offline) => (
                S::Offline,
                format!("keeper could not reach {name} to sign you in. Check the connection, then sign in again."),
                true,
            ),
            // Which server, on which port: what a network rule blocks.
            Some(AccountProblem::NoAnswer(sentence)) => (
                S::Offline,
                format!("{sentence} Then sign in again."),
                true,
            ),
            Some(AccountProblem::Failed(reason)) => (
                S::NeedsSignIn,
                format!(
                    "Signing in to {name} failed ({}). Sign in again.",
                    reason.trim_end_matches('.')
                ),
                true,
            ),
            _ => (
                S::SignedOut,
                format!("Sign in to {name} to use your settings on this device."),
                false,
            ),
        };
    }
    if facts.phase == AccountPhase::Syncing {
        return (S::Syncing, "Syncing your settings…".to_owned(), false);
    }
    if facts.forge_needed && !facts.forge_connected {
        // Connecting the repository is what failed — its sign-in page did
        // not open, or its server could not be reached — so that failure is
        // what is said, as a problem, never the neutral instruction alone
        // (R387). An account whose repository was connected before keeps its
        // quiet offline status below.
        return match &facts.problem {
            Some(AccountProblem::Offline) => (
                S::Offline,
                format!("keeper could not reach {name}'s repository to connect it. Check the connection, then sign in again."),
                true,
            ),
            Some(AccountProblem::NoAnswer(sentence)) => (
                S::Offline,
                format!("{sentence} Then sign in again."),
                true,
            ),
            Some(AccountProblem::Failed(reason)) => (
                S::NeedsSignIn,
                format!(
                    "Connecting {name}'s repository failed ({}). Sign in again.",
                    reason.trim_end_matches('.')
                ),
                true,
            ),
            _ => (
                S::NeedsSignIn,
                "Reconnect the repository to keep your settings in sync.".to_owned(),
                false,
            ),
        };
    }
    let since = facts
        .last_synced_ms
        .map(|at| format!("from {}", clock(at, facts.now_ms)))
        .unwrap_or_else(|| "kept on this device".to_owned());
    match &facts.problem {
        Some(AccountProblem::Offline | AccountProblem::NoAnswer(_)) => (
            S::Offline,
            format!("Offline — using settings {since}."),
            false,
        ),
        Some(AccountProblem::Failed(reason)) => (
            S::Offline,
            format!("Your settings could not be synced ({reason}). Using settings {since}."),
            true,
        ),
        _ if facts.last_synced_ms.is_none() => (
            S::Syncing,
            "Your settings have not been synced yet.".to_owned(),
            false,
        ),
        _ => (S::Ready, "Up to date.".to_owned(), false),
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

    /// R381 / 100.1 #2, #4: every way a sign-in fails but the person's own
    /// close is a problem whose sentence names the account; the close, and
    /// only the close, leaves the quiet signed-out instruction.
    #[test]
    fn a_failed_sign_in_is_a_problem_naming_the_account_and_a_closed_one_is_not() {
        let signed_out = |problem: Option<AccountProblem>| {
            vm(&AccountFacts {
                descriptor: Some(descriptor()),
                problem,
                ..AccountFacts::default()
            })
        };
        let closed = signed_out(problem_of(AccountError::Cancelled));
        assert_eq!(closed.state, AccountStateVm::SignedOut);
        assert!(!closed.problem);

        let cases = [
            (
                AccountError::Denied {
                    host: "id.acme.dev".to_owned(),
                    reason: Some("User is not allowed to use this project".to_owned()),
                },
                AccountStateVm::Blocked,
                "id.acme.dev refused to sign you in to Acme: User is not allowed to use this project.",
            ),
            (
                AccountError::Denied {
                    host: "id.acme.dev".to_owned(),
                    reason: None,
                },
                AccountStateVm::Blocked,
                "id.acme.dev refused to sign you in to Acme. Ask whoever runs Acme to let your account use keeper.",
            ),
            (
                AccountError::TimedOut,
                AccountStateVm::NeedsSignIn,
                "Signing in to Acme timed out before the browser answered. Sign in again.",
            ),
            (
                AccountError::Interrupted("no browser".to_owned()),
                AccountStateVm::NeedsSignIn,
                "Signing in to Acme did not come back from the browser (no browser). Sign in again.",
            ),
            (
                AccountError::Unreachable("keeper could not reach id.acme.dev.".to_owned()),
                AccountStateVm::Offline,
                "keeper could not reach Acme to sign you in. Check the connection, then sign in again.",
            ),
            (
                AccountError::NoAnswer(crate::oauth::no_answer("id.acme.dev:8451")),
                AccountStateVm::Offline,
                "Can't reach id.acme.dev:8451: it did not answer within 10 seconds. Check that \
                 this device is on a network (or VPN) that can reach it. Then sign in again.",
            ),
            (
                AccountError::Internal("keeper could not open the sign-in page: denied.".to_owned()),
                AccountStateVm::NeedsSignIn,
                "Signing in to Acme failed (keeper could not open the sign-in page: denied). Sign in again.",
            ),
        ];
        for (error, state, sentence) in cases {
            let vm = signed_out(problem_of(error));
            assert_eq!(
                (vm.state, vm.sentence.as_deref(), vm.problem),
                (state, Some(sentence), true)
            );
        }
    }

    #[test]
    fn a_signed_in_offline_account_is_a_status_and_a_failed_sync_a_problem() {
        let mut facts = signed_in();
        facts.problem = Some(AccountProblem::Offline);
        assert!(!vm(&facts).problem);
        facts.problem = Some(AccountProblem::Failed("disk full".to_owned()));
        assert!(vm(&facts).problem);
        facts.problem = None;
        assert!(!vm(&facts).problem);
    }

    /// R387 (FIX 06): signed in, but connecting the `oauth` repository failed
    /// during setup — its page did not open, or its server was unreachable.
    /// That failure is the account's sentence, as a problem; the neutral
    /// "Reconnect" instruction is only for a repository nobody tried yet. A
    /// cached account whose repository was connected and is simply offline
    /// stays a quiet status.
    #[test]
    fn a_failed_repository_connection_is_the_problem_not_the_neutral_reconnect() {
        let forge_down = |problem: Option<AccountProblem>| {
            let mut facts = signed_in();
            facts.forge_needed = true;
            facts.forge_connected = false;
            facts.problem = problem;
            vm(&facts)
        };
        let opened = forge_down(problem_of(AccountError::Internal(
            "keeper could not open the repository's sign-in page.".to_owned(),
        )));
        assert_eq!(
            (opened.state, opened.sentence.as_deref(), opened.problem),
            (
                AccountStateVm::NeedsSignIn,
                Some("Connecting Acme's repository failed (keeper could not open the repository's sign-in page). Sign in again."),
                true
            )
        );
        let unreachable = forge_down(problem_of(AccountError::Unreachable(
            "keeper could not reach git.acme.dev.".to_owned(),
        )));
        assert_eq!(
            (unreachable.state, unreachable.sentence.as_deref(), unreachable.problem),
            (
                AccountStateVm::Offline,
                Some("keeper could not reach Acme's repository to connect it. Check the connection, then sign in again."),
                true
            )
        );
        let untried = forge_down(None);
        assert_eq!(
            (untried.state, untried.problem),
            (AccountStateVm::NeedsSignIn, false)
        );

        let mut cached = signed_in();
        cached.forge_needed = true;
        cached.forge_connected = true;
        cached.problem = problem_of(AccountError::Unreachable("offline".to_owned()));
        let cached = vm(&cached);
        assert_eq!(
            (cached.state, cached.problem),
            (AccountStateVm::Offline, false)
        );
    }

    /// The single sign-on a restore starts by itself has its own place on
    /// the account's surface: its link while it waits, beside Cancel, apart
    /// from the account's own sign-in; once it ended, its outcome and no
    /// link (102.9, R480/02, R481).
    #[test]
    fn the_restores_matrix_sign_in_carries_its_own_link_and_outcome() {
        let link = crate::sign_in_link::SignInLinkVm {
            url: "https://mas.acme.dev/authorize?state=r1".to_owned(),
            not_opened: Some(crate::sign_in_link::NOT_OPENED.to_owned()),
        };
        let mut facts = signed_in();
        let flows = crate::oauth::OAuthFlowRegistry::new();
        let _attempt = facts
            .restore
            .begin_matrix_sign_in("https://matrix.acme.dev", &flows);
        let restore = facts.restore.matrix_sign_in.as_mut().expect("started");
        restore.link.apply(7, Some(&link));
        let waiting = vm(&facts);
        assert_ne!(
            waiting.state,
            AccountStateVm::SigningIn,
            "the account's own state"
        );
        assert_eq!(waiting.sign_in_link, None, "not the account's sign-in");
        let shown = waiting.restore.matrix_sign_in.expect("its own place");
        assert!(shown.waiting && !shown.problem);
        assert_eq!(shown.link, Some(link.clone()));
        assert!(
            shown.sentence.contains("matrix.acme.dev"),
            "{}",
            shown.sentence
        );

        let restore = facts.restore.matrix_sign_in.as_mut().expect("started");
        restore.link.apply(7, None);
        restore.ended = Some(RestoreSignInEnd::TimedOut);
        // A delivery after the end draws nothing.
        restore.link.apply(7, Some(&link));
        let ended = vm(&facts).restore.matrix_sign_in.expect("its outcome");
        assert!(!ended.waiting && ended.problem);
        assert_eq!(ended.link, None);
        assert!(ended
            .sentence
            .ends_with("Sign in to it again from Add an account."));
    }

    /// A restore's single sign-on whose homeserver did not answer ends with
    /// the sentence naming it, as a problem, with no link — the sign-in never
    /// reached a page to finish by hand.
    #[tokio::test]
    async fn a_restore_sign_in_whose_server_did_not_answer_names_it() {
        use crate::error::{AuthError, CoreError};
        use std::sync::{Arc, Mutex};

        let flows = crate::oauth::OAuthFlowRegistry::new();
        let shared = Arc::new(Mutex::new(RestoreFacts::default()));
        let access: RestoreFactsAccess = {
            let shared = Arc::clone(&shared);
            Arc::new(move |change: &mut dyn FnMut(&mut RestoreFacts)| {
                change(&mut shared.lock().expect("facts"));
            })
        };
        let attempt = shared
            .lock()
            .expect("facts")
            .begin_matrix_sign_in("https://matrix.acme.dev:8448", &flows);
        let sentence = crate::oauth::no_answer("matrix.acme.dev:8448");
        let answer = sentence.clone();
        attempt
            .run(access, |_| async move {
                Err(CoreError::Auth(AuthError::NoAnswer(answer)))
            })
            .await;

        let row = restore_vm(&shared.lock().expect("facts"))
            .matrix_sign_in
            .expect("its outcome");
        assert!(!row.waiting && row.problem);
        assert_eq!(row.link, None);
        assert_eq!(
            row.sentence,
            format!("{sentence} Sign in to it again from Add an account.")
        );
    }

    /// R500 (R102D2-01): a restore sign-in that is still running when the
    /// account is replaced and a new restore starts its own never writes the
    /// new one's row — neither its late link nor its ending — while the new
    /// attempt's own link and ending still land.
    #[tokio::test]
    async fn an_earlier_restore_sign_in_never_writes_the_row_a_later_one_owns() {
        use crate::error::{AuthError, CoreError};
        use crate::sign_in_link::SignInLinkVm;
        use std::sync::{Arc, Mutex};

        let flows = crate::oauth::OAuthFlowRegistry::new();
        let shared = Arc::new(Mutex::new(RestoreFacts::default()));
        let access: RestoreFactsAccess = {
            let shared = Arc::clone(&shared);
            Arc::new(move |change: &mut dyn FnMut(&mut RestoreFacts)| {
                change(&mut shared.lock().expect("facts"));
            })
        };
        let link = |state: &str| SignInLinkVm {
            url: format!("https://mas.acme.dev/authorize?state={state}"),
            not_opened: None,
        };

        let a = shared
            .lock()
            .expect("facts")
            .begin_matrix_sign_in("https://matrix.acme.dev", &flows);
        let (release_a, held_a) = tokio::sync::oneshot::channel::<()>();
        let late_link = link("a");
        let run_a = a.run(Arc::clone(&access), |links| async move {
            // A waits; meanwhile the account is replaced and B starts.
            let _ = held_a.await;
            links.show(1, Some(&late_link));
            Err(CoreError::Auth(AuthError::OAuthTimedOut))
        });
        let replace_then_start_b = async {
            // The account's replacement installs fresh facts …
            *shared.lock().expect("facts") = RestoreFacts::default();
            // … and a new restore starts B before A answers.
            let b = shared
                .lock()
                .expect("facts")
                .begin_matrix_sign_in("https://matrix.acme.dev", &flows);
            let before = shared.lock().expect("facts").clone();
            release_a.send(()).expect("A is still held");
            (b, before)
        };
        let ((), (b, before)) = tokio::join!(run_a, replace_then_start_b);

        let after = shared.lock().expect("facts").clone();
        assert_eq!(after, before, "A's link and ending leave B's row alone");
        let row = restore_vm(&after).matrix_sign_in.expect("B's row");
        assert!(row.waiting, "B still waits, with its Cancel");
        assert_eq!(row.link, None, "A's link is not drawn on B's row");

        let own_link = link("b");
        let seen = Arc::clone(&shared);
        b.run(Arc::clone(&access), |links| async move {
            links.show(2, Some(&own_link));
            let drawn = seen
                .lock()
                .expect("facts")
                .matrix_sign_in
                .as_ref()
                .and_then(|s| s.link.link().cloned());
            assert_eq!(drawn, Some(own_link), "B's own link lands");
            Err(CoreError::Auth(AuthError::OAuthCancelled))
        })
        .await;
        let row = restore_vm(&shared.lock().expect("facts"))
            .matrix_sign_in
            .expect("B's row");
        assert!(!row.waiting, "B's own ending lands");
        assert!(row.sentence.contains("was cancelled"), "{}", row.sentence);
    }

    /// R387 (FIX 07): the person closing the sign-in — the native sheet's
    /// close is `Cancelled` — ends the attempt `closed`, while any other
    /// error ends it `failed`; and an account already signed in here keeps
    /// composing "Up to date", so the attempt, not the state, is what says it
    /// was closed.
    #[test]
    fn a_closed_re_setup_of_a_signed_in_account_is_closed_not_done() {
        assert_eq!(
            AccountAttemptVm::of_error(&AccountError::Cancelled),
            AccountAttemptVm::Closed
        );
        for failed in [
            AccountError::TimedOut,
            AccountError::Interrupted("no browser".to_owned()),
            AccountError::Unreachable("offline".to_owned()),
        ] {
            assert_eq!(
                AccountAttemptVm::of_error(&failed),
                AccountAttemptVm::Failed
            );
        }
        let mut facts = signed_in();
        facts.problem = problem_of(AccountError::Cancelled);
        facts.attempt = Some(AccountAttemptVm::of_error(&AccountError::Cancelled));
        let after = vm(&facts);
        assert_eq!(after.state, AccountStateVm::Ready);
        assert_eq!(after.attempt, Some(AccountAttemptVm::Closed));
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
                version: None,
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
            Some("Restored 2 drives, 1 model provider and your settings from your account.")
        );
        assert_eq!(
            restored(1, 0).as_deref(),
            Some("Restored 1 drive and your settings from your account.")
        );
        assert_eq!(
            restored(0, 3).as_deref(),
            Some("Restored 3 model providers and your settings from your account.")
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
