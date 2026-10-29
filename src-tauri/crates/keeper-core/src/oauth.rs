//! In-flight OIDC (OAuth 2.0 / MSC3861) callback registry and client-registration
//! metadata (Story 2.2, AD-17).
//!
//! This module is deliberately **tauri-free**: plain `tokio` one-shot channels,
//! a `std::sync::Mutex<HashMap>`, and `url` parsing. The `keeper` (Tauri) shell
//! owns the deep-link plugin and forwards each incoming `dev.tgorka.keeper:/oauth/callback`
//! URL here via [`OAuthFlowRegistry::resolve`]; the OIDC `AuthProvider` in
//! [`crate::auth`] registers a pending flow keyed by the OAuth `state` and awaits
//! its [`OAuthCallback`].
//!
//! Callbacks are matched to their flow by the OAuth `state` query parameter; a
//! spurious / late / unmatched callback is ignored (logged at debug), never a
//! crash. Registry entries are removed on resolve and on cancel; the OIDC
//! provider that registers a flow also removes its own entry on every exit path
//! (timeout, cancel, browser-open failure, error, success) via an RAII guard, so
//! no dangling senders or leaked `state` secrets accumulate.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use matrix_sdk::authentication::oauth::registration::{
    ApplicationType, ClientMetadata, Localized, OAuthGrantType,
};
use matrix_sdk::authentication::oauth::ClientRegistrationData;
use tokio::sync::{mpsc, oneshot, watch};
use url::Url;

use crate::error::CoreError;

/// The redirect URI registered for the native public client: RFC 8252's
/// private-use scheme in reverse-DNS form. Its scheme is `client_uri`'s host
/// reversed (and the app's bundle id), which is what homeservers that validate
/// native redirects (Tuwunel, MAS) require.
pub const REDIRECT_URI: &str = "dev.tgorka.keeper:/oauth/callback";

/// The client's home-page URL, advertised during dynamic client registration.
const CLIENT_URI: &str = "https://keeper.tgorka.dev/";

/// Human-readable client name presented to the user on the consent screen.
const CLIENT_NAME: &str = "keeper";

/// How long one request a sign-in makes before any browser opens may go
/// unanswered: each discovery request, and for Matrix each request of the
/// homeserver probe, the metadata reads and the client registration. A
/// server behind a network that drops keeper's packets (a tailnet ACL
/// without a grant, a VPN that is off) never refuses the connection, and
/// without this budget the sign-in waited out the five-minute browser
/// timeout saying nothing. The budget is per request, so the sentence names
/// the server that request went to — not the one asked before it (R541-03).
pub const REACH_TIMEOUT: Duration = Duration::from_secs(10);

/// `work` — one request, to `address` — within [`REACH_TIMEOUT`], or the
/// [`no_answer`] sentence naming `address`.
pub async fn within_reach<T>(address: &str, work: impl Future<Output = T>) -> Result<T, String> {
    tokio::time::timeout(REACH_TIMEOUT, work)
        .await
        .map_err(|_| {
            tracing::warn!(%address, "sign-in: a server did not answer in time");
            no_answer(address)
        })
}

/// The `host[:port]` a sign-in reaches for `raw` — the port only when the
/// address names one — or `raw` itself when it is not a URL (a bare Matrix
/// server name).
pub fn address_of(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(url) => match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            _ => raw.to_owned(),
        },
        Err(_) => raw.trim().to_owned(),
    }
}

/// What a person reads when `address` did not answer within
/// [`REACH_TIMEOUT`]: which server, and that the network between is the
/// likely cause.
pub fn no_answer(address: &str) -> String {
    format!(
        "Can't reach {address}: it did not answer within {} seconds. \
         Check that this device is on a network (or VPN) that can reach it.",
        REACH_TIMEOUT.as_secs()
    )
}

/// The outcome of an in-flight OIDC flow, delivered over its one-shot channel.
///
/// A `Redirect` carries the full callback URL (which the SDK re-parses to
/// extract `code`+`state` and perform the PKCE token exchange). `Error` is the
/// identity provider's own `error=` answer; `Failed` is the platform's browser
/// round trip ending without one (Epic 100, R381: a refusal and a broken round
/// trip read differently). `Cancelled` is produced by
/// [`OAuthFlowRegistry::cancel_all`] and [`OAuthFlowRegistry::cancel`].
#[derive(Debug)]
pub enum OAuthCallback {
    /// The browser redirected back with a (matched) callback URL.
    Redirect(String),
    /// The callback carried an `error=` parameter: the provider's error code
    /// and, when it sent one, its non-secret `error_description`.
    Error {
        error: String,
        description: Option<String>,
    },
    /// The platform's round trip ended without an answer of this flow's — a
    /// callback without its `state`, or (for a flow that shows no link) a
    /// sign-in sheet that could not be shown; the string is the reason.
    Failed(String),
    /// The flow was cancelled in-app before any callback arrived.
    Cancelled,
}

/// Registry of in-flight OIDC flows keyed by their OAuth `state` secret.
///
/// Cloneable-by-`Arc` in the shell: the `keeper` crate holds an
/// `Arc<OAuthFlowRegistry>` in its `AppState` and passes clones to each
/// `login_oidc` call and to the deep-link `on_open_url` handler.
pub struct OAuthFlowRegistry {
    pending: Mutex<HashMap<String, Waiting>>,
    /// How many times [`cancel_all`](OAuthFlowRegistry::cancel_all) ran: a
    /// sign-in that took a [`CancelLatch`] before a cancel sees it, even one
    /// still discovering its server with no entry in `pending` yet.
    cancels: watch::Sender<u64>,
    /// Told the `state` of every flow that stops waiting
    /// ([`on_end`](OAuthFlowRegistry::on_end)).
    ended: OnceLock<EndHook>,
}

/// What [`OAuthFlowRegistry::on_end`] calls.
type EndHook = Box<dyn Fn(&str) + Send + Sync>;

impl Default for OAuthFlowRegistry {
    fn default() -> Self {
        Self {
            pending: Mutex::default(),
            cancels: watch::channel(0).0,
            ended: OnceLock::new(),
        }
    }
}

/// A sign-in's hold on Cancel from the moment it starts (story 102.9, R500):
/// a [`cancel_all`](OAuthFlowRegistry::cancel_all) after the latch was taken
/// stays seen — while the server is discovered and the client registered,
/// before the flow has a registry entry to drain — so the sign-in never
/// opens a browser afterwards and ends as cancelled. A cancel before the
/// latch was taken belongs to an earlier sign-in and is not seen.
#[derive(Debug, Clone)]
pub struct CancelLatch(watch::Receiver<u64>);

impl CancelLatch {
    /// Whether a cancel came since the latch was taken.
    pub fn is_cancelled(&self) -> bool {
        self.0.has_changed().unwrap_or(false)
    }

    /// Resolves once a cancel came since the latch was taken (at once if one
    /// already did); never while none does.
    pub async fn cancelled(&self) {
        let mut seen = self.0.clone();
        if seen.changed().await.is_err() {
            // The registry is gone: nobody can press Cancel any more.
            std::future::pending::<()>().await;
        }
    }
}

/// Identity of one registration, even if its OAuth `state` is reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowId(u64);

impl FlowId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// One flow waiting for its callback.
struct Waiting {
    id: FlowId,
    answer: oneshot::Sender<OAuthCallback>,
    /// A sign-in whose link the person can use by hand
    /// ([`OAuthFlowRegistry::register_with_link`]): a page that could not be
    /// shown is told here, and the flow keeps waiting.
    unshown: Option<mpsc::UnboundedSender<String>>,
}

impl OAuthFlowRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// The latch a sign-in takes when it starts, before anything it awaits:
    /// every later [`cancel_all`](OAuthFlowRegistry::cancel_all) reaches it.
    pub fn cancel_latch(&self) -> CancelLatch {
        CancelLatch(self.cancels.subscribe())
    }

    /// Call `hook` with a flow's `state` whenever that flow stops waiting,
    /// however it stops: answered, failed, cancelled, removed by its
    /// sign-in's guard (its deadline, an error, a sign-in dropped), or
    /// replaced by a flow registered under the same `state`. It runs once per
    /// flow, after the registry's lock is let go, on whichever thread ended
    /// the flow. The iPhone shell closes that flow's sign-in sheet from it,
    /// so a sheet never outlives the sign-in it answers (`sign_in_sheet`,
    /// R608-02). One hook per registry: `false`, and `hook` dropped, when
    /// one is already set.
    pub fn on_end(&self, hook: impl Fn(&str) + Send + Sync + 'static) -> bool {
        self.ended.set(Box::new(hook)).is_ok()
    }

    /// The flow under `state` has stopped waiting.
    fn tell_ended(&self, state: &str) {
        if let Some(hook) = self.ended.get() {
            hook(state);
        }
    }

    /// Register a pending flow keyed by its `state` secret, returning the
    /// receiver the caller awaits. The caller MUST pair this with [`remove`]
    /// (typically via an RAII guard) so the entry is dropped on every exit path:
    /// a matched callback removes it, but a timeout/cancel/error/browser-open
    /// failure does not. If a stale entry with the same `state` somehow exists it
    /// is replaced (its old sender dropped → its receiver resolves `Err`, treated
    /// as cancelled by the caller). A page that could not be shown ends this
    /// flow ([`not_shown`]).
    ///
    /// [`remove`]: OAuthFlowRegistry::remove
    /// [`not_shown`]: OAuthFlowRegistry::not_shown
    pub fn register(&self, state: String) -> oneshot::Receiver<OAuthCallback> {
        let (answer, rx) = oneshot::channel();
        let replaced = self.lock().insert(
            state.clone(),
            Waiting {
                id: FlowId::next(),
                answer,
                unshown: None,
            },
        );
        if replaced.is_some() {
            self.tell_ended(&state);
        }
        rx
    }

    /// [`register`], for a sign-in that shows its link to the person (story
    /// 102.9, R480): a page that could not be shown does not end it. Each
    /// [`not_shown`] reason arrives on the second receiver instead, and the
    /// flow keeps waiting for the callback the pasted link brings, until its
    /// deadline or a cancel.
    ///
    /// [`register`]: OAuthFlowRegistry::register
    /// [`not_shown`]: OAuthFlowRegistry::not_shown
    pub fn register_with_link(
        &self,
        state: String,
    ) -> (
        oneshot::Receiver<OAuthCallback>,
        mpsc::UnboundedReceiver<String>,
    ) {
        let (answer, rx) = oneshot::channel();
        let (unshown, unshown_rx) = mpsc::unbounded_channel();
        let replaced = self.lock().insert(
            state.clone(),
            Waiting {
                id: FlowId::next(),
                answer,
                unshown: Some(unshown),
            },
        );
        if replaced.is_some() {
            self.tell_ended(&state);
        }
        (rx, unshown_rx)
    }

    /// Remove a pending flow's entry, if present. Idempotent — removing an absent
    /// `state` is a no-op. Called by the OIDC provider's guard so an
    /// abandoned/timed-out/errored flow leaves no dangling sender or leaked
    /// `state` secret in the map (a matched callback already removes it).
    pub fn remove(&self, state: &str) {
        let removed = self.lock().remove(state).is_some();
        if removed {
            self.tell_ended(state);
        }
    }

    /// Whether a flow still waits under `state`: a sign-in sheet that has not
    /// opened yet stops trying once its flow has ended (`sign_in_sheet`).
    pub fn waits(&self, state: &str) -> bool {
        self.lock().contains_key(state)
    }

    /// The registration currently waiting under `state`. Native sheets keep
    /// this identity so delayed cleanup cannot mistake a replacement for
    /// the flow that ended.
    pub fn flow_id(&self, state: &str) -> Option<FlowId> {
        self.lock().get(state).map(|waiting| waiting.id)
    }

    /// Forward an incoming callback URL to its matching in-flight flow.
    ///
    /// Parses the `state` query parameter, looks up the pending sender, removes
    /// it, and sends either [`OAuthCallback::Error`] (if the URL carries an
    /// `error=` param, with its `error_description`) or
    /// [`OAuthCallback::Redirect`]. Returns `true` if a flow was matched and
    /// notified, `false` for an unparsable URL, a URL with no `state`, or a
    /// `state` matching no in-flight flow (all ignored, no crash).
    pub fn resolve(&self, url: &str) -> bool {
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                tracing::debug!(error = %e, "oauth callback: unparsable URL ignored");
                return false;
            }
        };

        let mut state: Option<String> = None;
        let mut error: Option<String> = None;
        let mut description: Option<String> = None;
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "state" => state = Some(value.into_owned()),
                "error" => error = Some(value.into_owned()),
                "error_description" => description = Some(value.into_owned()),
                _ => {}
            }
        }

        let Some(state) = state else {
            tracing::debug!("oauth callback: no state param; ignored");
            return false;
        };

        let sender = {
            let mut pending = self.lock();
            pending.remove(&state)
        };

        let Some(sender) = sender else {
            tracing::debug!("oauth callback: state matched no in-flight flow; ignored");
            return false;
        };
        self.tell_ended(&state);

        let outcome = match error {
            Some(error) => OAuthCallback::Error { error, description },
            None => OAuthCallback::Redirect(url.to_owned()),
        };
        // A send error means the receiver was already dropped (flow ended); the
        // entry is already removed, so this is a harmless late callback.
        if sender.answer.send(outcome).is_err() {
            tracing::debug!("oauth callback: receiver already dropped; ignored");
            return false;
        }
        true
    }

    /// Cancel every in-flight flow: send [`OAuthCallback::Cancelled`] to each
    /// pending sender and clear the registry. Used by the `cancel_oidc` command.
    /// It is latched first, so a sign-in that holds a [`CancelLatch`] but has
    /// no entry yet — still discovering its server — is cancelled too.
    pub fn cancel_all(&self) {
        self.cancels
            .send_modify(|cancels| *cancels = cancels.wrapping_add(1));
        let drained: Vec<_> = self.lock().drain().collect();
        for (state, waiting) in drained {
            self.tell_ended(&state);
            // Ignore send errors: a dropped receiver is already-cancelled.
            let _ = waiting.answer.send(OAuthCallback::Cancelled);
        }
    }

    /// Cancel one in-flight flow: send [`OAuthCallback::Cancelled`] to the
    /// sender registered under `state` and remove it. An unknown `state` is a
    /// no-op. Used when the platform's auth session reports that the person
    /// dismissed it, so a sibling flow (the Matrix login, the other account
    /// leg) keeps waiting.
    pub fn cancel(&self, state: &str) {
        let sender = self.lock().remove(state);
        if let Some(waiting) = sender {
            self.tell_ended(state);
            // A dropped receiver is already-cancelled.
            let _ = waiting.answer.send(OAuthCallback::Cancelled);
        }
    }

    /// End one in-flight flow as a failed round trip: send
    /// [`OAuthCallback::Failed`] with the platform's `reason` to the sender
    /// registered under `state` and remove it. Returns whether a flow was
    /// waiting. Used when the platform's sign-in sheet handed back something
    /// that is not this flow's answer, so the waiting flow ends at once — and
    /// reads as a broken round trip, never as the identity provider's
    /// refusal.
    pub fn fail(&self, state: &str, reason: &str) -> bool {
        let sender = self.lock().remove(state);
        if sender.is_some() {
            self.tell_ended(state);
        }
        // A dropped receiver is an already-ended flow.
        sender.is_some_and(|waiting| {
            waiting
                .answer
                .send(OAuthCallback::Failed(reason.to_owned()))
                .is_ok()
        })
    }

    /// The platform could not show the page of the flow under `state` — a
    /// sign-in sheet or Auth Tab that was queued and then refused, or could
    /// not open (story 102.9, R480). A flow registered
    /// [`with its link`](OAuthFlowRegistry::register_with_link) is told and
    /// keeps waiting: its link is still good in any browser. Any other flow
    /// (a sign-out's end-session round trip) ends as [`fail`] ends it.
    /// Returns whether a flow was waiting.
    ///
    /// [`fail`]: OAuthFlowRegistry::fail
    pub fn not_shown(&self, state: &str, reason: &str) -> bool {
        let pending = self.lock();
        if let Some(unshown) = pending.get(state).and_then(|w| w.unshown.as_ref()) {
            return unshown.send(reason.to_owned()).is_ok();
        }
        drop(pending);
        self.fail(state, reason)
    }

    /// Lock helper that recovers a poisoned mutex (a panicking send would only
    /// have left a fully-formed map behind), avoiding an `.unwrap()`.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiting>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl std::fmt::Debug for OAuthFlowRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.pending.lock().map(|p| p.len()).unwrap_or(0);
        f.debug_struct("OAuthFlowRegistry")
            .field("pending", &len)
            .finish()
    }
}

/// Build the redirect [`Url`] for the native public client.
pub fn redirect_uri() -> Result<Url, CoreError> {
    Url::parse(REDIRECT_URI)
        .map_err(|e| CoreError::Internal(format!("invalid OAuth redirect URI: {e}")))
}

/// Build the dynamic client-registration metadata for keeper as a public native
/// client (RFC 7591 automatic registration).
///
/// `ApplicationType::Native`, a single `AuthorizationCode` grant with the
/// `dev.tgorka.keeper:/oauth/callback` redirect, the required `client_uri`, and a
/// `client_name`. The SDK's serializer forces `token_endpoint_auth_method:
/// "none"` and adds `refresh_token` + `response_types: ["code"]`, so no
/// confidential secret is ever embedded.
pub fn registration_data() -> Result<ClientRegistrationData, CoreError> {
    let redirect = redirect_uri()?;
    let client_uri = Url::parse(CLIENT_URI)
        .map_err(|e| CoreError::Internal(format!("invalid OAuth client URI: {e}")))?;

    let mut metadata = ClientMetadata::new(
        ApplicationType::Native,
        vec![OAuthGrantType::AuthorizationCode {
            redirect_uris: vec![redirect],
        }],
        Localized::new(client_uri, []),
    );
    metadata.client_name = Some(Localized::new(CLIENT_NAME.to_owned(), []));

    let raw = matrix_sdk::ruma::serde::Raw::new(&metadata)
        .map_err(|e| CoreError::Internal(format!("could not serialize client metadata: {e}")))?;
    Ok(ClientRegistrationData::from(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A callback URL whose `state` matches an in-flight flow is forwarded as a
    /// `Redirect` carrying the full URL, and the entry is removed afterwards.
    #[tokio::test]
    async fn resolve_matches_by_state_and_forwards_redirect() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("abc123".to_owned());

        let url = "dev.tgorka.keeper:/oauth/callback?code=authcode&state=abc123";
        assert!(registry.resolve(url), "matching state should resolve");

        match rx.await.expect("sender not dropped") {
            OAuthCallback::Redirect(got) => assert_eq!(got, url),
            other => panic!("expected Redirect, got {other:?}"),
        }
        // Entry removed: a second resolve of the same state is a no-op.
        assert!(!registry.resolve(url), "flow already resolved/removed");
    }

    /// A callback carrying `error=` resolves as an `Error` with the server's
    /// code and, when present, its `error_description`.
    #[tokio::test]
    async fn resolve_forwards_error_param() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("s1".to_owned());

        assert!(registry.resolve(
            "dev.tgorka.keeper:/oauth/callback?error=access_denied&error_description=Not%20for%20you&state=s1"
        ));
        match rx.await.expect("sender not dropped") {
            OAuthCallback::Error { error, description } => {
                assert_eq!(error, "access_denied");
                assert_eq!(description.as_deref(), Some("Not for you"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    /// `fail` ends only the named flow, as a failed round trip with its reason.
    #[tokio::test]
    async fn fail_ends_one_flow_as_a_failed_round_trip() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("a".to_owned());
        let _other = registry.register("b".to_owned());

        assert!(registry.fail("a", "no browser"));
        assert!(!registry.fail("a", "twice"), "the flow already ended");
        match rx.await.expect("sender not dropped") {
            OAuthCallback::Failed(reason) => assert_eq!(reason, "no browser"),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=b"));
    }

    /// A page that could not be shown leaves a sign-in that shows its link
    /// waiting — the pasted link's callback still finishes it — and ends any
    /// other flow (a sign-out's) as a failed round trip (R480).
    #[tokio::test]
    async fn a_page_not_shown_ends_only_a_flow_without_a_link() {
        let registry = OAuthFlowRegistry::new();
        let (mut linked, mut unshown) = registry.register_with_link("signin".to_owned());
        let plain = registry.register("signout".to_owned());

        assert!(registry.not_shown("signin", "presentationContextInvalid"));
        assert_eq!(
            unshown.try_recv().ok().as_deref(),
            Some("presentationContextInvalid")
        );
        assert!(linked.try_recv().is_err(), "the sign-in still waits");
        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=signin"));
        assert!(matches!(
            linked.await.expect("sender not dropped"),
            OAuthCallback::Redirect(_)
        ));

        assert!(registry.not_shown("signout", "no window"));
        match plain.await.expect("sender not dropped") {
            OAuthCallback::Failed(reason) => assert_eq!(reason, "no window"),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(!registry.not_shown("signout", "again"), "it already ended");
    }

    /// A callback whose `state` matches no in-flight flow is ignored (no crash).
    #[test]
    fn resolve_spurious_state_is_ignored() {
        let registry = OAuthFlowRegistry::new();
        let _rx = registry.register("real".to_owned());
        assert!(
            !registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=nope"),
            "unknown state must not match"
        );
        // The real flow is untouched.
        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=real"));
    }

    /// A callback with no `state` param, and an unparsable URL, are both ignored.
    #[test]
    fn resolve_missing_state_and_bad_url_are_ignored() {
        let registry = OAuthFlowRegistry::new();
        let _rx = registry.register("s".to_owned());
        assert!(!registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x"));
        assert!(!registry.resolve("not a url at all"));
    }

    /// `cancel_all` delivers `Cancelled` to every pending flow and clears them.
    #[tokio::test]
    async fn cancel_all_cancels_every_pending_flow() {
        let registry = OAuthFlowRegistry::new();
        let rx1 = registry.register("a".to_owned());
        let rx2 = registry.register("b".to_owned());

        registry.cancel_all();

        assert!(matches!(
            rx1.await.expect("sender not dropped"),
            OAuthCallback::Cancelled
        ));
        assert!(matches!(
            rx2.await.expect("sender not dropped"),
            OAuthCallback::Cancelled
        ));
        // Registry is empty: a subsequent callback matches nothing.
        assert!(!registry.resolve("dev.tgorka.keeper:/oauth/callback?state=a"));
    }

    /// A Cancel pressed while a sign-in has no entry yet still reaches it,
    /// and only the sign-ins that had started: the next one starts clean
    /// (story 102.9, R500).
    #[tokio::test]
    async fn a_cancel_before_registration_reaches_only_the_sign_ins_already_started() {
        let registry = OAuthFlowRegistry::new();
        let started = registry.cancel_latch();
        assert!(!started.is_cancelled());

        registry.cancel_all();
        let next = registry.cancel_latch();

        assert!(started.is_cancelled(), "the cancel is latched for it");
        tokio::time::timeout(std::time::Duration::from_secs(5), started.cancelled())
            .await
            .expect("a latched cancel resolves at once");
        assert!(
            !next.is_cancelled(),
            "a sign-in after the cancel is not cancelled"
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), next.cancelled())
                .await
                .is_err(),
            "and waits for a cancel of its own"
        );
    }

    /// `cancel` ends exactly the named flow; its sibling keeps waiting.
    #[tokio::test]
    async fn cancel_ends_one_flow_and_leaves_the_rest() {
        let registry = OAuthFlowRegistry::new();
        let rx_a = registry.register("a".to_owned());
        let _rx_b = registry.register("b".to_owned());

        registry.cancel("a");
        registry.cancel("unknown");

        assert!(matches!(
            rx_a.await.expect("sender not dropped"),
            OAuthCallback::Cancelled
        ));
        assert!(!registry.resolve("dev.tgorka.keeper:/oauth/callback?state=a"));
        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=b"));
    }

    /// Dropping the receiver (flow ended) leaves a stale sender; resolving it is
    /// a harmless no-op that returns `false` and does not panic.
    #[test]
    fn resolve_after_receiver_dropped_is_noop() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("gone".to_owned());
        drop(rx);
        assert!(
            !registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=gone"),
            "resolving a flow whose receiver dropped returns false"
        );
    }

    /// `remove` purges a pending entry so an abandoned/timed-out flow leaves no
    /// residue (the guard path in `authenticate` relies on this). Idempotent.
    #[test]
    fn remove_purges_pending_entry() {
        let registry = OAuthFlowRegistry::new();
        let _rx = registry.register("leak".to_owned());
        registry.remove("leak");
        // The entry is gone: a later callback for that state matches nothing.
        assert!(!registry.resolve("dev.tgorka.keeper:/oauth/callback?code=x&state=leak"));
        // Idempotent: removing an already-absent state is a harmless no-op.
        registry.remove("leak");
    }

    /// The registration metadata is a native app with exactly the keeper
    /// redirect URI, a public (`none`) client, and the keeper client name.
    ///
    /// `ClientMetadata` is serialize-only (it round-trips into a
    /// `ClientMetadataSerializeHelper`), so we assert against the serialized JSON
    /// held in the `Raw` rather than deserializing back.
    #[test]
    fn registration_metadata_is_native_with_exact_redirect() {
        let data = registration_data().expect("build registration data");
        let json: serde_json::Value =
            serde_json::from_str(data.metadata.json().get()).expect("metadata is valid JSON");

        assert_eq!(
            json.get("application_type").and_then(|v| v.as_str()),
            Some("native"),
            "must register as a native app"
        );
        let redirects = json
            .get("redirect_uris")
            .and_then(|v| v.as_array())
            .expect("redirect_uris array present");
        assert_eq!(redirects.len(), 1);
        assert_eq!(redirects[0].as_str(), Some(REDIRECT_URI));
        assert_eq!(
            json.get("token_endpoint_auth_method")
                .and_then(|v| v.as_str()),
            Some("none"),
            "public client: no confidential secret"
        );
        assert_eq!(
            json.get("client_name").and_then(|v| v.as_str()),
            Some(CLIENT_NAME)
        );
    }

    #[test]
    fn the_address_names_the_host_and_the_port_it_names() {
        assert_eq!(
            address_of("https://electra.siren-alsephina.ts.net:8451"),
            "electra.siren-alsephina.ts.net:8451"
        );
        assert_eq!(address_of("https://id.acme.dev/realms/x"), "id.acme.dev");
        assert_eq!(address_of("http://127.0.0.1:8008/"), "127.0.0.1:8008");
        // A bare Matrix server name is not a URL: named as typed.
        assert_eq!(address_of(" matrix.acme.dev "), "matrix.acme.dev");
    }

    #[test]
    fn a_server_that_does_not_answer_is_named_with_its_port() {
        assert_eq!(
            no_answer("electra.siren-alsephina.ts.net:8451"),
            "Can't reach electra.siren-alsephina.ts.net:8451: it did not answer within 10 \
             seconds. Check that this device is on a network (or VPN) that can reach it."
        );
    }

    /// Every way a flow stops waiting tells the end hook once, with its
    /// `state` — the guard's `remove` included, which is how a sign-in's
    /// deadline ends it (R608-02) — and nothing that leaves a flow waiting,
    /// or touches none, tells it anything.
    #[test]
    fn every_way_a_flow_ends_tells_the_end_hook_once() {
        let registry = OAuthFlowRegistry::new();
        let ended = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let told = std::sync::Arc::clone(&ended);
        assert!(registry.on_end(move |state| {
            told.lock().expect("not poisoned").push(state.to_owned());
        }));
        assert!(!registry.on_end(|_| {}), "one hook per registry");
        let ends = || std::mem::take(&mut *ended.lock().expect("not poisoned"));

        let _deadline = registry.register_with_link("deadline".to_owned());
        registry.remove("deadline");
        registry.remove("deadline");
        assert_eq!(ends(), ["deadline"], "the guard's remove, once");

        let _answered = registry.register("answered".to_owned());
        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=c&state=answered"));
        let _cancelled = registry.register("cancelled".to_owned());
        registry.cancel("cancelled");
        let _failed = registry.register("failed".to_owned());
        assert!(registry.fail("failed", "broken"));
        let _unshown = registry.register("unshown".to_owned());
        assert!(registry.not_shown("unshown", "refused"));
        assert_eq!(ends(), ["answered", "cancelled", "failed", "unshown"]);

        let (_linked, _reasons) = registry.register_with_link("linked".to_owned());
        assert!(registry.not_shown("linked", "refused"));
        assert!(!registry.resolve("dev.tgorka.keeper:/oauth/callback?code=c&state=nobody"));
        registry.cancel("nobody");
        assert!(!registry.fail("nobody", "broken"));
        assert_eq!(ends(), Vec::<String>::new(), "the linked flow still waits");

        let _replaced = registry.register("linked".to_owned());
        assert_eq!(ends(), ["linked"], "replaced under its own state");

        let _other = registry.register("other".to_owned());
        registry.cancel_all();
        let mut drained = ends();
        drained.sort();
        assert_eq!(drained, ["linked", "other"]);
    }
}
