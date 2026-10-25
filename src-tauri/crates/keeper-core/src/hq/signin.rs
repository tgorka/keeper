//! Sign-in enrolment, keeper's half (109.6; Q109-01 … Q109-06 a, S-ID-1 …
//! S-ID-9): a device whose person is signed in to the account keeper
//! trusts gets its agents-server credential by signing in, not by a code.
//!
//! - **When.** With `agents.server_url` set and the account signed in,
//!   keeper enrols on its own: a device with no credential gets a new one;
//!   a paired device sends its token so the server adopts it (same client
//!   id, a new token, Q109-04 a); a device the server stopped taking — its
//!   sign-in lapsed or was suspended (S-ID-9) — signs in again the same way,
//!   silently. A device whose credential came from a sign-in attests with a
//!   fresh one before the server's window runs out (Q109-03 a).
//! - **To whom.** The account's token leaves the device only for a server
//!   whose unauthenticated `GET /auth/config` names the account's own issuer
//!   and client, and no scope the account's sign-in does not ask for
//!   (S-ID-2); only to the enrol and attest routes, as a bearer, never in a
//!   URL (S-ID-1). It is never logged and never stored here: the account's
//!   session keeps it in the keychain, and what the server answers — the
//!   device token — goes to the keychain item a pairing's lives in.
//! - **Host.** The device's host is read from the shell's provider at each
//!   enrolment, never at construction (S-ID-7, 109.1).
//! - **Pairing** stays for a person keeper cannot sign in for (no account,
//!   a server without sign-in, a refusal); 109.7 retires it.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::hq::databases::Authorization;
use crate::hq::routes::{AuthConfig, Enrolled, EnrolmentKind, RouteError};
use crate::hq::server::ServerUrl;
use crate::hq::sessions::now_ms;
use crate::hq::vm::{HqEnrolmentVm, HqSignInState, HqSignInVm};
use crate::hq::{keychain_key, lock, route_sentence, Hq, HqError, Inner, Pairing};
use crate::org_account::descriptor::AccountDescriptor;
use crate::org_account::{oidc, session, AccountError};
use crate::platform::Platform;

/// The server's attestation window unless it says otherwise (Q109-03 a).
pub const ATTEST_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// The longest an attestation timer sleeps before it reads the clock again:
/// a computer asleep does not advance a monotonic sleep.
const ATTEST_TICK: Duration = Duration::from_secs(60 * 60);

/// The wait before an attestation that could not be made is tried again.
const ATTEST_RETRY: Duration = Duration::from_secs(10 * 60);

/// The account a sign-in token would come from, as its descriptor names it,
/// and which sign-in of it. One attempt holds one of these from the first
/// check to the request it sends: the token is minted from exactly this
/// account, and an attempt whose account moved on sends nothing (R1096-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSignIn {
    /// The descriptor's `id`: another account with the same issuer is
    /// another account.
    pub account: String,
    pub issuer: String,
    pub client_id: String,
    /// Every scope the account's sign-in asks for.
    pub scopes: Vec<String>,
    /// When the person last signed in, as the provider stated it (the ID
    /// token's `auth_time`): a new sign-in is a new attempt even when the
    /// account is the same (R1096-03). `None` signed out, or unstated.
    pub signed_in_at: Option<i64>,
}

impl AccountSignIn {
    /// The same account, whichever sign-in of it.
    fn same_account(&self, other: &AccountSignIn) -> bool {
        AccountSignIn {
            signed_in_at: None,
            ..self.clone()
        } == AccountSignIn {
            signed_in_at: None,
            ..other.clone()
        }
    }
}

/// Whether this device has an account, and whether someone is signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountState {
    NoAccount,
    SignedOut(AccountSignIn),
    SignedIn(AccountSignIn),
}

/// Why no sign-in token could be had.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Nobody is signed in any more, or the sign-in is no longer good.
    #[error("{0}")]
    NeedsSignIn(String),
    /// The sign-in server could not be asked now.
    #[error("{0}")]
    Unavailable(String),
    /// The account configured now is not the one asked for: nothing was
    /// minted.
    #[error("the account changed")]
    Changed,
}

/// A sign-in token, as a future the caller can drop.
pub type TokenAnswer<'a> = Pin<Box<dyn Future<Output = Result<String, TokenError>> + Send + 'a>>;

/// The account's sign-in, as the agents server's enrolment uses it.
/// [`OrgAccount`] is the one keeper runs; tests answer with their own.
pub trait SignIn: Send + Sync {
    /// The account and whether it is signed in; reads the keychain, never
    /// the network.
    fn account(&self) -> AccountState;
    /// An access token of `account` — the one whose issuer, client and
    /// scopes the server was checked against, or [`TokenError::Changed`] —
    /// minted for this call alone: the server spends its `jti` and refuses
    /// one issued more than five minutes before it arrives (S-ID-3), so a
    /// token is never presented twice.
    fn fresh_token<'a>(&'a self, account: &'a AccountSignIn) -> TokenAnswer<'a>;
}

/// No account: every device pairs.
pub struct NoAccount;

impl SignIn for NoAccount {
    fn account(&self) -> AccountState {
        AccountState::NoAccount
    }
    fn fresh_token<'a>(&'a self, _: &'a AccountSignIn) -> TokenAnswer<'a> {
        Box::pin(async { Err(TokenError::Changed) })
    }
}

type DescriptorFn = dyn Fn() -> Option<AccountDescriptor> + Send + Sync;
type HttpFn = dyn Fn() -> Result<reqwest::Client, String> + Send + Sync;

/// The configured organisation account (Epic 82): its descriptor and its
/// session in the keychain, through `org_account`'s own OIDC client.
pub struct OrgAccount {
    platform: Arc<dyn Platform>,
    descriptor: Box<DescriptorFn>,
    http: Box<HttpFn>,
}

impl OrgAccount {
    /// `descriptor` names the account configured now (none without one);
    /// `http` is the account's client.
    pub fn new(
        platform: Arc<dyn Platform>,
        descriptor: impl Fn() -> Option<AccountDescriptor> + Send + Sync + 'static,
        http: impl Fn() -> Result<reqwest::Client, String> + Send + Sync + 'static,
    ) -> OrgAccount {
        OrgAccount {
            platform,
            descriptor: Box::new(descriptor),
            http: Box::new(http),
        }
    }
}

fn account_of(d: &AccountDescriptor, signed_in_at: Option<i64>) -> AccountSignIn {
    AccountSignIn {
        account: d.id.clone(),
        issuer: d.auth.issuer.clone(),
        client_id: d.auth.client_id.clone(),
        scopes: d
            .auth
            .scopes
            .iter()
            .chain(&d.auth.extra_scopes)
            .cloned()
            .collect(),
        signed_in_at,
    }
}

/// A claim of a stored token's payload: our own, read back from the keychain.
fn claim(jwt: &str, name: &str) -> Option<i64> {
    use base64::Engine as _;
    let part = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get(name)?
        .as_i64()
}

impl SignIn for OrgAccount {
    fn account(&self) -> AccountState {
        let Some(d) = (self.descriptor)() else {
            return AccountState::NoAccount;
        };
        let session =
            session::load_bound_session(self.platform.as_ref(), &d).and_then(
                |session| match session {
                    Some(session) => session.check_roles(&d).map(|()| Some(session)),
                    None => Ok(None),
                },
            );
        match session {
            // A refresh keeps the original `auth_time` (OIDC Core §12.2,
            // checked in `oidc::refreshed_roles`); only a sign-in moves it.
            // Without one, the ID token's own `iat` stands in.
            Ok(Some(session)) => AccountState::SignedIn(account_of(
                &d,
                claim(&session.id_token, "auth_time").or_else(|| claim(&session.id_token, "iat")),
            )),
            Ok(None) => AccountState::SignedOut(account_of(&d, None)),
            Err(error) => {
                tracing::debug!(%error, "the account's session cannot sign this device in to the agents server");
                AccountState::SignedOut(account_of(&d, None))
            }
        }
    }

    fn fresh_token<'a>(&'a self, account: &'a AccountSignIn) -> TokenAnswer<'a> {
        Box::pin(async move {
            // Minted from the very descriptor the server was checked
            // against, or not at all: its issuer is the one the session is
            // bound to and the token endpoint is asked of (R1096-01).
            let d = (self.descriptor)().ok_or(TokenError::Changed)?;
            if !account_of(&d, None).same_account(account) {
                return Err(TokenError::Changed);
            }
            let http = (self.http)().map_err(TokenError::Unavailable)?;
            oidc::minted_access_token(self.platform.as_ref(), &http, &d)
                .await
                .map_err(|error| match error {
                    AccountError::Unreachable(_) | AccountError::NoAnswer(_) => {
                        TokenError::Unavailable(error.to_string())
                    }
                    other => TokenError::NeedsSignIn(other.to_string()),
                })
        })
    }
}

/// Why keeper will not send this account's token to a server (S-ID-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mistrust {
    /// The server trusts another issuer.
    Issuer,
    /// The server trusts another application of the issuer.
    Client,
    /// The account's sign-in does not ask for a scope the server needs.
    Scope(String),
}

/// Whether `config` names this account's issuer and client exactly, and
/// asks for no scope its sign-in does not (S-ID-2).
pub fn trusts(config: &AuthConfig, account: &AccountSignIn) -> Result<(), Mistrust> {
    if config.issuer != account.issuer {
        return Err(Mistrust::Issuer);
    }
    if config.client_id != account.client_id {
        return Err(Mistrust::Client);
    }
    match config
        .scopes
        .iter()
        .find(|scope| !account.scopes.contains(scope))
    {
        Some(missing) => Err(Mistrust::Scope(missing.clone())),
        None => Ok(()),
    }
}

fn mistrust_sentence(host: &str, mistrust: &Mistrust) -> String {
    match mistrust {
        Mistrust::Issuer | Mistrust::Client => format!(
            "{host} trusts a different sign-in than your account's, so keeper sent it nothing. \
             Pair this device with a code."
        ),
        Mistrust::Scope(scope) => format!(
            "Your account's sign-in does not ask for {scope}, which {host} needs, so keeper sent \
             it nothing. Pair this device with a code."
        ),
    }
}

/// A credential that came from a sign-in, and when it was last attested, on
/// this device's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Bound {
    pub attested_at_ms: i64,
    /// The server's window: a credential not attested within it lapses.
    pub window_ms: i64,
}

impl Bound {
    pub(crate) fn now() -> Bound {
        Bound {
            attested_at_ms: now_ms(),
            window_ms: ATTEST_WINDOW_MS,
        }
    }

    /// When keeper attests again: six sevenths into the window — six days
    /// of seven — so a day's slack outlasts a skewed clock or an offline
    /// evening, and no sooner, so a launch is not an attestation.
    pub(crate) fn due_at_ms(&self) -> i64 {
        let window = self.window_ms.max(0);
        self.attested_at_ms
            .saturating_add(window.saturating_sub(window / 7))
    }
}

/// What the last sign-in attempt of this configuration ended in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The server offers no sign-in, or this device has no account.
    NotOffered,
    /// The server trusts the account; nobody is signed in.
    SignedOut,
    /// A refusal that trying again would not change; the sentence.
    Refused(String),
    /// Nothing was decided (unreachable, a sign-in that could not be
    /// refreshed); tried again at the next reconcile.
    Failed(String),
}

/// The sign-in attempt running for a configuration.
#[derive(Debug)]
pub(crate) struct Running {
    handle: JoinHandle<()>,
    /// The account it was started for.
    account: AccountState,
    /// Set, under the service's lock, the moment its enrolment request is
    /// sent: from then on the attempt is never cancelled, and it keeps what
    /// the server answers, because the server may already have issued a
    /// device or rotated the old token (R1096-02).
    committed: Arc<AtomicBool>,
}

/// Sign-in enrolment's state for one configuration: reset by every restart,
/// which cancels the attempt running for the configuration it replaces.
#[derive(Debug, Default)]
pub(crate) struct SignInMemo {
    pub running: Option<Running>,
    pub outcome: Option<Outcome>,
    /// The account, and which sign-in of it, the outcome was reached for:
    /// another one tries again.
    pub tried: Option<AccountState>,
}

impl SignInMemo {
    /// Cancel the running attempt unless its request is already sent; a
    /// sent one finishes on its own and finds itself replaced.
    pub(crate) fn cancel(&mut self) {
        if let Some(running) = self.running.take() {
            if !running.committed.load(Ordering::SeqCst) {
                running.handle.abort();
            }
        }
    }
}

/// What an attempt asks of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// No credential: a new device.
    New,
    /// A paired device's credential, or one the server stopped taking:
    /// adopted under the same client id.
    Adopt,
}

/// What an attempt ended in.
enum Attempted {
    /// An outcome to remember for this account.
    Done(Outcome),
    /// A credential was kept (the restart that follows reads it).
    Kept,
    /// The account moved on before anything was sent: nothing to remember,
    /// and the account as it is now is considered again.
    Superseded,
}

/// What an attestation ended in.
enum Attest {
    /// Attested; the next is due by this.
    Kept(Bound),
    /// Not now; try again after [`ATTEST_RETRY`].
    Later,
    /// The configuration or the credential moved on.
    Stop,
}

fn notice_sentence(name: &str, host: Option<&str>, kind: EnrolmentKind) -> String {
    let who = match host {
        Some(host) => format!("{name} on {host}"),
        None => name.to_owned(),
    };
    match kind {
        EnrolmentKind::Enrolled => format!("New device signed in: {who}."),
        EnrolmentKind::Adopted => format!("{who} signed in and kept its place."),
        EnrolmentKind::Paired => format!("New device paired with a code: {who}."),
    }
}

fn json_sentence(error: serde_json::Error) -> HqError {
    HqError::Sentence(error.to_string())
}

impl Hq {
    /// Sign-in enrolment's view of this configuration (Settings).
    pub(super) fn sign_in_vm(inner: &Inner) -> HqSignInVm {
        let Some(server) = &inner.server else {
            return HqSignInVm::unavailable();
        };
        let host = server.host();
        let bound = inner
            .pairing
            .as_ref()
            .is_some_and(|pairing| pairing.bound.is_some());
        let vm = |state, sentence: String| HqSignInVm {
            state,
            sentence: Some(sentence),
        };
        if bound && !inner.authorization.removed {
            return vm(
                HqSignInState::Enrolled,
                format!("Signed in: {host} knows this device by your account."),
            );
        }
        if inner.sign_in.running.is_some() {
            return vm(
                HqSignInState::Enrolling,
                format!("Signing this device in to {host}…"),
            );
        }
        match &inner.sign_in.outcome {
            None | Some(Outcome::NotOffered) => HqSignInVm::unavailable(),
            Some(Outcome::SignedOut) => vm(
                HqSignInState::SignedOut,
                format!(
                    "Sign in to your account under Account, and keeper connects this device to \
                     {host}. There is no code to type."
                ),
            ),
            Some(Outcome::Refused(sentence)) | Some(Outcome::Failed(sentence)) => {
                vm(HqSignInState::Refused, sentence.clone())
            }
        }
    }

    /// Enrol, adopt or sign in again if this configuration wants it and the
    /// account allows it; a no-op otherwise. Never waits: the attempt runs
    /// on its own, and a credential it keeps restarts the service. An
    /// attempt running for another account — another one, or another
    /// sign-in of it — is cancelled first, unless its request is sent.
    pub(super) fn consider_sign_in(&self) {
        let account = self.sign_in.account();
        let mut inner = lock(&self.inner);
        let Some(server) = inner.server.clone() else {
            return;
        };
        if let Some(running) = &inner.sign_in.running {
            if running.account == account || running.committed.load(Ordering::SeqCst) {
                return;
            }
            inner.sign_in.cancel();
        }
        let ask = match &inner.pairing {
            None => Ask::New,
            Some(pairing) if pairing.bound.is_none() || inner.authorization.removed => Ask::Adopt,
            // Signed in already: attestation keeps it.
            Some(_) => return,
        };
        if account == AccountState::NoAccount {
            let changed = inner.sign_in.outcome.take().is_some();
            inner.sign_in.tried = None;
            drop(inner);
            if changed {
                self.publish();
            }
            return;
        }
        let settled = matches!(
            inner.sign_in.outcome,
            Some(Outcome::NotOffered | Outcome::SignedOut | Outcome::Refused(_))
        );
        if settled && inner.sign_in.tried.as_ref() == Some(&account) {
            return;
        }
        let generation = inner.generation;
        let pairing = inner.pairing.clone();
        let committed = Arc::new(AtomicBool::new(false));
        let me = self.me.clone();
        let (mine, started) = (Arc::clone(&committed), account.clone());
        // Spawned and recorded under the lock, so the attempt finds itself
        // running when it first takes it.
        let handle = tokio::spawn(async move {
            let Some(hq) = me.upgrade() else { return };
            let attempted = hq
                .sign_in_attempt(generation, &server, pairing, ask, &started, &mine)
                .await;
            let mut inner = lock(&hq.inner);
            let current = inner.generation == generation
                && inner
                    .sign_in
                    .running
                    .as_ref()
                    .is_some_and(|running| Arc::ptr_eq(&running.committed, &mine));
            if !current {
                // A restart or another account replaced it — or the restart
                // its own kept credential ran.
                return;
            }
            inner.sign_in.running = None;
            let again = match attempted {
                Attempted::Done(outcome) => {
                    inner.sign_in.outcome = Some(outcome);
                    inner.sign_in.tried = Some(started);
                    false
                }
                Attempted::Kept => false,
                Attempted::Superseded => {
                    inner.sign_in.outcome = None;
                    inner.sign_in.tried = None;
                    true
                }
            };
            drop(inner);
            hq.publish();
            if again {
                hq.consider_sign_in();
            }
        });
        inner.sign_in.running = Some(Running {
            handle,
            account,
            committed,
        });
        drop(inner);
        self.publish();
    }

    /// Whether the attempt holding `committed` may send its request now:
    /// its configuration is the current one, it is still the running
    /// attempt, and the account is still the one it was started for. If so,
    /// it is marked sent, under the same lock a restart cancels under.
    fn commit(&self, generation: u64, account: &AccountState, committed: &Arc<AtomicBool>) -> bool {
        let now = self.sign_in.account();
        let inner = lock(&self.inner);
        let current = inner.generation == generation
            && inner
                .sign_in
                .running
                .as_ref()
                .is_some_and(|running| Arc::ptr_eq(&running.committed, committed));
        if !current || now != *account {
            return false;
        }
        committed.store(true, Ordering::SeqCst);
        true
    }

    /// One attempt: the server's sign-in checked against the account, a
    /// token minted from that same account, the enrolment, the credential
    /// kept.
    async fn sign_in_attempt(
        &self,
        generation: u64,
        server: &ServerUrl,
        pairing: Option<Pairing>,
        ask: Ask,
        state: &AccountState,
        committed: &Arc<AtomicBool>,
    ) -> Attempted {
        let host = server.host().to_owned();
        let answer = {
            let _contact = self.contacts.hold(server);
            let config = match self.routes.auth_config(server).await {
                Ok(config) => config,
                Err(RouteError::Refused { status: 404, .. }) => {
                    return Attempted::Done(Outcome::NotOffered)
                }
                Err(error) => {
                    return Attempted::Done(Outcome::Failed(route_sentence(server, &error)))
                }
            };
            let account = match state {
                AccountState::NoAccount => return Attempted::Done(Outcome::NotOffered),
                AccountState::SignedOut(account) | AccountState::SignedIn(account) => account,
            };
            if let Err(mistrust) = trusts(&config, account) {
                tracing::warn!(
                    ?mistrust,
                    "the agents server trusts another sign-in; nothing was sent to it"
                );
                return Attempted::Done(Outcome::Refused(mistrust_sentence(&host, &mistrust)));
            }
            if matches!(state, AccountState::SignedOut(_)) {
                return Attempted::Done(Outcome::SignedOut);
            }
            let token = match self.sign_in.fresh_token(account).await {
                Ok(token) => token,
                Err(TokenError::NeedsSignIn(_)) => return Attempted::Done(Outcome::SignedOut),
                Err(TokenError::Unavailable(why)) => {
                    return Attempted::Done(Outcome::Failed(format!(
                        "keeper could not reach your account's sign-in: {why}"
                    )))
                }
                Err(TokenError::Changed) => return Attempted::Superseded,
            };
            if !self.commit(generation, state, committed) {
                tracing::info!(
                    "the account or the server changed while signing in; nothing was sent"
                );
                return Attempted::Superseded;
            }
            let device_token = match ask {
                Ask::New => None,
                Ask::Adopt => pairing.as_ref().map(|pairing| pairing.token.as_str()),
            };
            // The host as the OS names it now, not when keeper started.
            let device_host = (self.device_host)();
            self.routes
                .enroll(
                    server,
                    &token,
                    &self.device_name,
                    &device_host,
                    device_token,
                )
                .await
        };
        let enrolled = match answer {
            Ok(enrolled) => enrolled,
            Err(RouteError::Refused { code, status }) => {
                tracing::info!(%code, status, "the agents server refused to sign this device in");
                return Attempted::Done(match code.as_str() {
                    "hq.signin_disabled" => Outcome::NotOffered,
                    "hq.device_unknown" => Outcome::Refused(format!(
                        "{host} no longer knows this device, so signing in cannot keep it. \
                         Remove this server's copies, and keeper signs it in as a new device."
                    )),
                    "hq.adopt.other_person" => Outcome::Refused(format!(
                        "This device was paired for someone other than the account signed in \
                         here, so {host} would not move it onto your sign-in."
                    )),
                    "hq.not_a_person" => Outcome::Refused(format!(
                        "{host} does not list your account as one of its people. Pair this \
                         device with a code."
                    )),
                    _ => Outcome::Failed(format!("{host} refused the sign-in: {code}.")),
                });
            }
            Err(error) => return Attempted::Done(Outcome::Failed(route_sentence(server, &error))),
        };
        match self.keep_enrolled(server, enrolled).await {
            Ok(()) => Attempted::Kept,
            Err(error) => {
                tracing::warn!(%error, "the agents server's credential could not be kept");
                Attempted::Done(Outcome::Failed(error.to_string()))
            }
        }
    }

    /// Keep what an enrolment answered — in the keychain item of the server
    /// it was sent to, where a pairing's token lives, replacing it — and
    /// restart on it if that server is still the one set. A sent request is
    /// never thrown away: the server may already have issued the device or
    /// rotated the old token (R1096-02).
    async fn keep_enrolled(&self, server: &ServerUrl, enrolled: Enrolled) -> Result<(), HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        let data_dir = self.data_dir()?;
        let record = Pairing {
            token: enrolled.token,
            actor_id: enrolled.discovery.actor_id,
            client_id: enrolled.discovery.client_id,
            name: self.device_name.clone(),
            bound: Some(Bound::now()),
        };
        let stored = serde_json::to_string(&record).map_err(json_sentence)?;
        self.platform.keychain_set(&keychain_key(server), &stored)?;
        Authorization {
            removed: false,
            open: enrolled.discovery.databases.clone(),
        }
        .save(&data_dir, server)?;
        tracing::info!(
            adopted = enrolled.adopted,
            databases = enrolled.discovery.databases.len(),
            "this device signed in to its agents server"
        );
        let set = {
            let mut inner = lock(&self.inner);
            let set = inner.server.as_ref() == Some(server);
            if set {
                inner.refusals.clear();
            }
            set
        };
        if set {
            self.restart().await?;
        }
        Ok(())
    }

    /// Keep a credential that came from a sign-in attested: wake when it is
    /// due ([`Bound::due_at_ms`]), attest, wait for the next. Held with the
    /// configuration's tasks, so a restart plans it again.
    pub(super) fn schedule_attest(&self, inner: &mut Inner, bound: Bound) {
        let generation = inner.generation;
        let me = self.me.clone();
        inner.tasks.push(tokio::spawn(async move {
            let mut bound = bound;
            loop {
                let wait = bound.due_at_ms().saturating_sub(now_ms());
                if wait > 0 {
                    let wait = Duration::from_millis(u64::try_from(wait).unwrap_or(u64::MAX));
                    tokio::time::sleep(wait.min(ATTEST_TICK)).await;
                    continue;
                }
                let Some(hq) = me.upgrade() else { return };
                match hq.attest(generation).await {
                    Attest::Kept(next) => bound = next,
                    Attest::Later => tokio::time::sleep(ATTEST_RETRY).await,
                    Attest::Stop => return,
                }
            }
        }));
    }

    async fn attest(&self, generation: u64) -> Attest {
        let state = self.sign_in.account();
        let account = match &state {
            AccountState::SignedIn(account) => account,
            // Signing in again is the person's to do; until then the
            // credential may lapse, and the next sign-in adopts it back.
            _ => return Attest::Later,
        };
        let (server, pairing) = {
            let inner = lock(&self.inner);
            if inner.generation != generation {
                return Attest::Stop;
            }
            match (&inner.server, &inner.pairing) {
                (Some(server), Some(pairing)) => (server.clone(), pairing.clone()),
                _ => return Attest::Stop,
            }
        };
        let answer = {
            let _contact = self.contacts.hold(&server);
            let config = match self.routes.auth_config(&server).await {
                Ok(config) => config,
                Err(error) => {
                    tracing::info!(%error, "the agents server's sign-in could not be read; attesting later");
                    return Attest::Later;
                }
            };
            if let Err(mistrust) = trusts(&config, account) {
                tracing::warn!(
                    ?mistrust,
                    "the agents server trusts another sign-in; nothing was sent to it"
                );
                return Attest::Later;
            }
            let token = match self.sign_in.fresh_token(account).await {
                Ok(token) => token,
                Err(error) => {
                    tracing::info!(%error, "no sign-in token to attest with; attesting later");
                    return Attest::Later;
                }
            };
            // The account the server was checked against is still the one
            // signed in, and the configuration is still this one (R1096-01).
            if self.sign_in.account() != state {
                tracing::info!("the account changed while attesting; nothing was sent");
                return Attest::Later;
            }
            if lock(&self.inner).generation != generation {
                return Attest::Stop;
            }
            self.routes.attest(&server, &token, &pairing.token).await
        };
        match answer {
            Ok(attested) => {
                let window = attested.lapses_at_ms - attested.attested_at_ms;
                let bound = Bound {
                    attested_at_ms: now_ms(),
                    window_ms: if window > 0 { window } else { ATTEST_WINDOW_MS },
                };
                match self
                    .keep_attested(generation, &server, &pairing, bound)
                    .await
                {
                    Ok(true) => {
                        tracing::info!("this device's sign-in was attested to its agents server");
                        Attest::Kept(bound)
                    }
                    Ok(false) => Attest::Stop,
                    Err(error) => {
                        tracing::warn!(%error, "the attestation could not be kept");
                        Attest::Later
                    }
                }
            }
            // Paired, never signed in: an enrolment adopts it instead.
            Err(RouteError::Refused { status: 409, code }) if code == "hq.attest.not_enrolled" => {
                self.forget_bound(generation, &server, &pairing).await;
                Attest::Stop
            }
            // The device token itself is unknown: revoked, or rotated by
            // another adoption. Discovery marks it removed, and a sign-in
            // adopts it back where the server still can (R1096-04).
            Err(RouteError::Refused { status: 401, code }) if code == "hq.device_unknown" => {
                tracing::warn!(%code, "the agents server no longer knows this device's credential; attesting stops");
                let mut inner = lock(&self.inner);
                if inner.generation == generation {
                    self.schedule_discovery(&mut inner, Duration::ZERO);
                }
                Attest::Stop
            }
            // The person or the app is not one the server takes: retrying
            // would mint a token every ten minutes for the same answer. The
            // credential lapses, and the sign-in after it says why.
            Err(RouteError::Refused { status: 403, code }) => {
                tracing::warn!(%code, "the agents server refused this sign-in's attestation; attesting stops");
                Attest::Stop
            }
            // Any other refusal judged the sign-in token, not the device
            // (`hq.signin.stale`, `.replayed`, …), or nothing was decided:
            // ask again whether the device is known, and attest again later.
            Err(error) => {
                tracing::info!(%error, "the attestation did not go through; attesting later");
                if matches!(error, RouteError::Refused { status: 401, .. }) {
                    let mut inner = lock(&self.inner);
                    if inner.generation == generation {
                        self.schedule_discovery(&mut inner, Duration::ZERO);
                    }
                }
                Attest::Later
            }
        }
    }

    /// Record a new attestation on the kept credential; `false` when the
    /// configuration or the credential moved on meanwhile.
    async fn keep_attested(
        &self,
        generation: u64,
        server: &ServerUrl,
        pairing: &Pairing,
        bound: Bound,
    ) -> Result<bool, HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut inner = lock(&self.inner);
        if inner.generation != generation || inner.pairing.as_ref() != Some(pairing) {
            return Ok(false);
        }
        let record = Pairing {
            bound: Some(bound),
            ..pairing.clone()
        };
        let stored = serde_json::to_string(&record).map_err(json_sentence)?;
        self.platform.keychain_set(&keychain_key(server), &stored)?;
        inner.pairing = Some(record);
        Ok(true)
    }

    /// The server says this credential never came from a sign-in: keep it
    /// as a pairing, and let an enrolment adopt it.
    async fn forget_bound(&self, generation: u64, server: &ServerUrl, pairing: &Pairing) {
        {
            let _lifecycle = self.lifecycle.lock().await;
            let mut inner = lock(&self.inner);
            if inner.generation != generation || inner.pairing.as_ref() != Some(pairing) {
                return;
            }
            let record = Pairing {
                bound: None,
                ..pairing.clone()
            };
            match serde_json::to_string(&record) {
                Ok(stored) => {
                    if let Err(error) = self.platform.keychain_set(&keychain_key(server), &stored) {
                        tracing::warn!(%error, "the agents server pairing could not be kept");
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "the agents server pairing could not be kept")
                }
            }
            inner.pairing = Some(record);
        }
        self.consider_sign_in();
    }

    /// The person's newest "new device" notices from their other devices
    /// (S-ID-8): signed in, adopted, or paired with a code. A server
    /// without the route has none.
    pub async fn enrolments(&self) -> Result<Vec<HqEnrolmentVm>, HqError> {
        let (server, pairing) = self.paired()?;
        let answer = {
            let _contact = self.contacts.hold(&server);
            self.routes.enrolments(&server, &pairing.token).await
        };
        let notices = match answer {
            Ok(notices) => notices,
            Err(RouteError::Refused { status: 404, .. }) => Vec::new(),
            Err(error) => return Err(HqError::Sentence(route_sentence(&server, &error))),
        };
        Ok(notices
            .into_iter()
            .filter(|notice| !notice.this_device)
            .map(|notice| HqEnrolmentVm {
                sentence: notice_sentence(&notice.name, notice.host.as_deref(), notice.kind),
                client_id: notice.client_id,
                at_ms: notice.at_ms,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> AccountSignIn {
        AccountSignIn {
            account: "acme".to_owned(),
            issuer: "https://id.example.org".to_owned(),
            client_id: "keeper".to_owned(),
            scopes: vec!["openid".to_owned(), "urn:x:aud".to_owned()],
            signed_in_at: Some(1),
        }
    }

    /// The account the descriptor provider names now, swappable.
    fn org(
        p: &Arc<crate::forges::testing::FakePlatform>,
        now: &Arc<std::sync::Mutex<AccountDescriptor>>,
    ) -> OrgAccount {
        let now = Arc::clone(now);
        OrgAccount::new(
            Arc::clone(p) as Arc<dyn Platform>,
            move || Some(now.lock().expect("descriptor").clone()),
            || Ok(crate::forges::testing::http()),
        )
    }

    #[tokio::test]
    async fn org_account_mints_only_for_the_account_the_server_was_checked_against() {
        let p = Arc::new(crate::forges::testing::FakePlatform::default());
        let d = crate::forges::testing::signed_in_account(&p, "");
        let now = Arc::new(std::sync::Mutex::new(d.clone()));
        let org = org(&p, &now);
        let AccountState::SignedIn(checked) = org.account() else {
            panic!("signed in");
        };
        // Same account: it goes on to mint (here: no refresh token, so the
        // sign-in is no longer good — but it was the right account to ask).
        assert!(matches!(
            org.fresh_token(&checked).await,
            Err(TokenError::NeedsSignIn(_))
        ));
        // Another issuer is configured meanwhile: nothing is minted at all.
        let mut other = d.clone();
        other.auth.issuer = "https://id.elsewhere.dev".to_owned();
        *now.lock().expect("descriptor") = other;
        assert_eq!(org.fresh_token(&checked).await, Err(TokenError::Changed));
        // So is another account with the same issuer.
        let mut renamed = d;
        renamed.id = "other".to_owned();
        *now.lock().expect("descriptor") = renamed;
        assert_eq!(org.fresh_token(&checked).await, Err(TokenError::Changed));
    }

    #[test]
    fn a_new_sign_in_moves_the_account_state_and_a_refresh_does_not() {
        use crate::org_account::session::{load_session, store_session};
        use base64::Engine as _;
        let p = Arc::new(crate::forges::testing::FakePlatform::default());
        let d = crate::forges::testing::signed_in_account(&p, "");
        let now = Arc::new(std::sync::Mutex::new(d.clone()));
        let org = org(&p, &now);
        let id_token = |claims: serde_json::Value| {
            let b64 = |v: &serde_json::Value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())
            };
            format!(
                "{}.{}.sig",
                b64(&serde_json::json!({"alg": "RS256"})),
                b64(&claims)
            )
        };
        let signed_in_as = |claims: serde_json::Value| {
            let mut session = load_session(p.as_ref(), &d.id)
                .expect("reads")
                .expect("session");
            session.id_token = id_token(claims);
            store_session(p.as_ref(), &d.id, &session).expect("stores");
            match org.account() {
                AccountState::SignedIn(account) => account.signed_in_at,
                other => panic!("{other:?}"),
            }
        };
        let first = signed_in_as(serde_json::json!({"auth_time": 100, "iat": 100}));
        // A refresh: a new ID token, the same `auth_time`.
        let refreshed = signed_in_as(serde_json::json!({"auth_time": 100, "iat": 5000}));
        let again = signed_in_as(serde_json::json!({"auth_time": 9000, "iat": 9000}));
        assert_eq!(first, Some(100));
        assert_eq!(refreshed, first, "a refresh is the same sign-in");
        assert_eq!(again, Some(9000), "a new sign-in is a new state");
    }

    fn config(issuer: &str, client: &str, scopes: &[&str]) -> AuthConfig {
        AuthConfig {
            issuer: issuer.to_owned(),
            client_id: client.to_owned(),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn a_token_goes_only_to_a_server_naming_the_accounts_own_issuer_client_and_scopes() {
        let a = account();
        assert_eq!(
            trusts(&config("https://id.example.org", "keeper", &["openid"]), &a),
            Ok(())
        );
        // Exact strings: a trailing slash is another issuer.
        assert_eq!(
            trusts(&config("https://id.example.org/", "keeper", &[]), &a),
            Err(Mistrust::Issuer)
        );
        assert_eq!(
            trusts(&config("https://id.example.org", "other", &[]), &a),
            Err(Mistrust::Client)
        );
        assert_eq!(
            trusts(
                &config("https://id.example.org", "keeper", &["openid", "roles"]),
                &a
            ),
            Err(Mistrust::Scope("roles".to_owned()))
        );
    }

    #[test]
    fn an_attestation_is_due_six_days_into_seven_and_not_before() {
        let day = 24 * 60 * 60 * 1000;
        let bound = Bound {
            attested_at_ms: 1_000,
            window_ms: 7 * day,
        };
        assert_eq!(bound.due_at_ms(), 1_000 + 6 * day);
        // The server's own window rules.
        let short = Bound {
            attested_at_ms: 0,
            window_ms: 70,
        };
        assert_eq!(short.due_at_ms(), 60);
    }

    #[test]
    fn a_notice_names_the_device_its_host_and_how_it_came() {
        assert_eq!(
            notice_sentence("keeper", Some("kalypso"), EnrolmentKind::Enrolled),
            "New device signed in: keeper on kalypso."
        );
        assert_eq!(
            notice_sentence("keeper", Some("hesperia"), EnrolmentKind::Adopted),
            "keeper on hesperia signed in and kept its place."
        );
        assert_eq!(
            notice_sentence("keeper", None, EnrolmentKind::Paired),
            "New device paired with a code: keeper."
        );
    }
}
