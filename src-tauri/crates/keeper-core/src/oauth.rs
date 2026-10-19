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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use matrix_sdk::authentication::oauth::registration::{
    ApplicationType, ClientMetadata, Localized, OAuthGrantType,
};
use matrix_sdk::authentication::oauth::ClientRegistrationData;
use tokio::sync::oneshot;
use url::Url;

use crate::error::CoreError;
use crate::vm::SignInLinkVm;

/// The redirect URI registered for the native public client: RFC 8252's
/// private-use scheme in reverse-DNS form. Its scheme is `client_uri`'s host
/// reversed (and the app's bundle id), which is what homeservers that validate
/// native redirects (Tuwunel, MAS) require.
pub const REDIRECT_URI: &str = "dev.tgorka.keeper:/oauth/callback";

/// The client's home-page URL, advertised during dynamic client registration.
const CLIENT_URI: &str = "https://keeper.tgorka.dev/";

/// Human-readable client name presented to the user on the consent screen.
const CLIENT_NAME: &str = "keeper";

/// How long a sign-in waits for its server to answer before any browser
/// opens: discovery, and for Matrix the homeserver probe. A server behind a
/// network that drops keeper's packets (a tailnet ACL without a grant, a VPN
/// that is off) never refuses the connection, and without this budget the
/// sign-in waited out the five-minute browser timeout saying nothing.
pub const REACH_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// The platform's sign-in sheet could not be shown or ended with a system
    /// error; the string is the platform's reason.
    Failed(String),
    /// The flow was cancelled in-app before any callback arrived.
    Cancelled,
}

/// One in-flight flow: where its answer goes, and the sign-in page the
/// person can open themselves when the browser did not.
struct Pending {
    tx: oneshot::Sender<OAuthCallback>,
    link: Option<SignInLinkVm>,
    /// Registration order, so [`OAuthFlowRegistry::link`] offers the newest.
    seq: u64,
}

/// The sentence for a sign-in page keeper could not open, from the
/// platform's `error`, with the page's address taken out of it: an opener's
/// error can quote the command it ran, URL included, and the sentence is
/// shown beside the link. It is never logged — an address quoted in another
/// form would survive the replacement.
pub fn open_failure(error: &impl std::fmt::Display, url: &str) -> String {
    let words = error.to_string().replace(url, "the sign-in page");
    format!("keeper could not open your browser ({words}).")
}

/// Registry of in-flight OIDC flows keyed by their OAuth `state` secret.
///
/// Cloneable-by-`Arc` in the shell: the `keeper` crate holds an
/// `Arc<OAuthFlowRegistry>` in its `AppState` and passes clones to each
/// `login_oidc` call and to the deep-link `on_open_url` handler.
///
/// A flow also carries its authorization URL once the browser is asked to
/// open it ([`OAuthFlowRegistry::offer_link`]), so a person whose browser did
/// not open can copy it ([`OAuthFlowRegistry::link`]). It leaves with the
/// flow, on every exit path the flow's own entry leaves by.
#[derive(Default)]
pub struct OAuthFlowRegistry {
    pending: Mutex<HashMap<String, Pending>>,
    next_seq: AtomicU64,
}

impl OAuthFlowRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pending flow keyed by its `state` secret, returning the
    /// receiver the caller awaits. The caller MUST pair this with [`remove`]
    /// (typically via an RAII guard) so the entry is dropped on every exit path:
    /// a matched callback removes it, but a timeout/cancel/error/browser-open
    /// failure does not. If a stale entry with the same `state` somehow exists it
    /// is replaced (its old sender dropped → its receiver resolves `Err`, treated
    /// as cancelled by the caller).
    ///
    /// [`remove`]: OAuthFlowRegistry::remove
    pub fn register(&self, state: String) -> oneshot::Receiver<OAuthCallback> {
        let (tx, rx) = oneshot::channel();
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut pending = self.lock();
        pending.insert(
            state,
            Pending {
                tx,
                link: None,
                seq,
            },
        );
        rx
    }

    /// Keep `url` with the flow waiting under `state` as the sign-in page the
    /// person can open themselves. Call it just before the browser is asked
    /// to open the page; an unknown `state` is a no-op.
    ///
    /// The URL carries the flow's `state`, PKCE challenge and nonce but no
    /// secret a third party could finish the sign-in with (the verifier
    /// stays in keeper), so it may be shown to the person at the keyboard. It
    /// is still never logged.
    pub fn offer_link(&self, state: &str, url: &str) {
        if let Some(flow) = self.lock().get_mut(state) {
            flow.link = Some(SignInLinkVm {
                url: url.to_owned(),
                open_failed: None,
            });
        }
    }

    /// Record that keeper could not open the flow's page itself, and why, so
    /// the link is offered with the reason instead of as a fallback. An
    /// unknown `state`, or one without a link, is a no-op.
    pub fn open_failed(&self, state: &str, reason: &str) {
        if let Some(link) = self.lock().get_mut(state).and_then(|f| f.link.as_mut()) {
            link.open_failed = Some(reason.to_owned());
        }
    }

    /// The sign-in page of the newest flow still waiting, if any offered one.
    pub fn link(&self) -> Option<SignInLinkVm> {
        self.lock()
            .values()
            .filter(|flow| flow.link.is_some())
            .max_by_key(|flow| flow.seq)
            .and_then(|flow| flow.link.clone())
    }

    /// Remove a pending flow's entry, if present. Idempotent — removing an absent
    /// `state` is a no-op. Called by the OIDC provider's guard so an
    /// abandoned/timed-out/errored flow leaves no dangling sender or leaked
    /// `state` secret in the map (a matched callback already removes it).
    pub fn remove(&self, state: &str) {
        let _ = self.lock().remove(state);
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
            pending.remove(&state).map(|flow| flow.tx)
        };

        let Some(sender) = sender else {
            tracing::debug!("oauth callback: state matched no in-flight flow; ignored");
            return false;
        };

        let outcome = match error {
            Some(error) => OAuthCallback::Error { error, description },
            None => OAuthCallback::Redirect(url.to_owned()),
        };
        // A send error means the receiver was already dropped (flow ended); the
        // entry is already removed, so this is a harmless late callback.
        if sender.send(outcome).is_err() {
            tracing::debug!("oauth callback: receiver already dropped; ignored");
            return false;
        }
        true
    }

    /// Cancel every in-flight flow: send [`OAuthCallback::Cancelled`] to each
    /// pending sender and clear the registry. Used by the `cancel_oidc` command.
    pub fn cancel_all(&self) {
        let drained: Vec<_> = {
            let mut pending = self.lock();
            pending.drain().map(|(_, flow)| flow.tx).collect()
        };
        for tx in drained {
            // Ignore send errors: a dropped receiver is already-cancelled.
            let _ = tx.send(OAuthCallback::Cancelled);
        }
    }

    /// Cancel one in-flight flow: send [`OAuthCallback::Cancelled`] to the
    /// sender registered under `state` and remove it. An unknown `state` is a
    /// no-op. Used when the platform's auth session reports that the person
    /// dismissed it, so a sibling flow (the Matrix login, the other account
    /// leg) keeps waiting.
    pub fn cancel(&self, state: &str) {
        let sender = self.lock().remove(state).map(|flow| flow.tx);
        if let Some(tx) = sender {
            // A dropped receiver is already-cancelled.
            let _ = tx.send(OAuthCallback::Cancelled);
        }
    }

    /// End one in-flight flow as a failed round trip: send
    /// [`OAuthCallback::Failed`] with the platform's `reason` to the sender
    /// registered under `state` and remove it. Returns whether a flow was
    /// waiting. Used when the platform's sign-in sheet could not be shown or
    /// handed back something that is not this flow's answer, so the waiting
    /// flow ends at once — and reads as a broken round trip, never as the
    /// identity provider's refusal.
    pub fn fail(&self, state: &str, reason: &str) -> bool {
        let sender = self.lock().remove(state).map(|flow| flow.tx);
        // A dropped receiver is an already-ended flow.
        sender.is_some_and(|tx| tx.send(OAuthCallback::Failed(reason.to_owned())).is_ok())
    }

    /// Lock helper that recovers a poisoned mutex (a panicking send would only
    /// have left a fully-formed map behind), avoiding an `.unwrap()`.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
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

    const PAGE: &str =
        "https://id.acme.dev:8451/oauth/v2/authorize?client_id=k&state=s1&code_challenge=c&nonce=n";

    /// The page offered with a flow is the newest waiting flow's, carries
    /// keeper's reason once it could not open it, and leaves with the flow
    /// on every exit path: resolve, cancel, cancel_all, fail and remove.
    #[test]
    fn a_flow_offers_its_sign_in_page_until_it_ends() {
        let registry = OAuthFlowRegistry::new();
        assert_eq!(registry.link(), None, "nothing waits");

        let _rx = registry.register("s1".to_owned());
        assert_eq!(registry.link(), None, "no page before the browser is asked");
        registry.offer_link("s1", PAGE);
        assert_eq!(
            registry.link(),
            Some(SignInLinkVm {
                url: PAGE.to_owned(),
                open_failed: None
            })
        );
        registry.open_failed("s1", "keeper could not open your browser (no handler).");
        assert_eq!(
            registry.link().and_then(|l| l.open_failed).as_deref(),
            Some("keeper could not open your browser (no handler).")
        );

        // A newer flow's page is the one offered; the older returns once the
        // newer has ended.
        let _rx2 = registry.register("s2".to_owned());
        registry.offer_link("s2", "https://id.acme.dev/authorize?state=s2");
        assert_eq!(
            registry.link().map(|l| l.url).as_deref(),
            Some("https://id.acme.dev/authorize?state=s2")
        );
        registry.remove("s2");
        assert_eq!(registry.link().map(|l| l.url).as_deref(), Some(PAGE));

        // Unknown states change nothing.
        registry.offer_link("nobody", "https://evil.example/");
        registry.open_failed("nobody", "x");
        assert_eq!(registry.link().map(|l| l.url).as_deref(), Some(PAGE));

        assert!(registry.resolve("dev.tgorka.keeper:/oauth/callback?code=c&state=s1"));
        assert_eq!(
            registry.link(),
            None,
            "a resolved flow takes its page along"
        );

        let ends: [fn(&OAuthFlowRegistry); 4] = [
            |r: &OAuthFlowRegistry| r.cancel("s"),
            |r: &OAuthFlowRegistry| r.cancel_all(),
            |r: &OAuthFlowRegistry| {
                r.fail("s", "sheet failed");
            },
            |r: &OAuthFlowRegistry| r.remove("s"),
        ];
        for end in ends {
            let _rx = registry.register("s".to_owned());
            registry.offer_link("s", PAGE);
            assert!(registry.link().is_some());
            end(&registry);
            assert_eq!(registry.link(), None);
        }
    }

    /// The page carries the flow's state, challenge and nonce: shown to the
    /// person, never printed into a log by a `{:?}`.
    #[test]
    fn a_sign_in_link_never_debug_prints_its_url() {
        let link = SignInLinkVm {
            url: PAGE.to_owned(),
            open_failed: Some("no browser".to_owned()),
        };
        let printed = format!("{link:?}");
        assert!(!printed.contains("state=s1"), "{printed}");
        assert!(!printed.contains("id.acme.dev"), "{printed}");
        assert!(printed.contains("no browser"), "{printed}");
    }

    /// An opener that quotes its command line does not carry the page's
    /// address into the sentence (shown, and logged).
    #[test]
    fn an_open_failure_never_quotes_the_page() {
        let error = format!("Launcher \"/usr/bin/open\" \"{PAGE}\" failed with exit status 1");
        let sentence = open_failure(&error, PAGE);
        assert!(!sentence.contains("state=s1"), "{sentence}");
        assert_eq!(
            sentence,
            "keeper could not open your browser (Launcher \"/usr/bin/open\" \"the sign-in page\" \
             failed with exit status 1)."
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
}
