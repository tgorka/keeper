//! In-flight OIDC (OAuth 2.0 / MSC3861) callback registry and client-registration
//! metadata (Story 2.2, AD-17).
//!
//! This module is deliberately **tauri-free**: plain `tokio` one-shot channels,
//! a `std::sync::Mutex<HashMap>`, and `url` parsing. The `keeper` (Tauri) shell
//! owns the deep-link plugin and forwards each incoming `keeper://oauth/callback`
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
use std::sync::Mutex;

use matrix_sdk::authentication::oauth::registration::{
    ApplicationType, ClientMetadata, Localized, OAuthGrantType,
};
use matrix_sdk::authentication::oauth::ClientRegistrationData;
use tokio::sync::oneshot;
use url::Url;

use crate::error::CoreError;

/// The custom-scheme redirect URI registered for the native public client.
pub const REDIRECT_URI: &str = "keeper://oauth/callback";

/// The client's home-page URL, advertised during dynamic client registration.
const CLIENT_URI: &str = "https://keeper.tgorka.dev/";

/// Human-readable client name presented to the user on the consent screen.
const CLIENT_NAME: &str = "keeper";

/// The outcome of an in-flight OIDC flow, delivered over its one-shot channel.
///
/// A `Redirect` carries the full callback URL (which the SDK re-parses to
/// extract `code`+`state` and perform the PKCE token exchange). `Error` carries
/// a non-secret server-reported error string. `Cancelled` is produced by
/// [`OAuthFlowRegistry::cancel_all`].
#[derive(Debug)]
pub enum OAuthCallback {
    /// The browser redirected back with a (matched) callback URL.
    Redirect(String),
    /// The callback carried an `error=` parameter; the string is the server's
    /// non-secret error description.
    Error(String),
    /// The flow was cancelled in-app before any callback arrived.
    Cancelled,
}

/// Registry of in-flight OIDC flows keyed by their OAuth `state` secret.
///
/// Cloneable-by-`Arc` in the shell: the `keeper` crate holds an
/// `Arc<OAuthFlowRegistry>` in its `AppState` and passes clones to each
/// `login_oidc` call and to the deep-link `on_open_url` handler.
#[derive(Default)]
pub struct OAuthFlowRegistry {
    pending: Mutex<HashMap<String, oneshot::Sender<OAuthCallback>>>,
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
        let mut pending = self.lock();
        pending.insert(state, tx);
        rx
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
    /// `error=` param) or [`OAuthCallback::Redirect`]. Returns `true` if a flow
    /// was matched and notified, `false` for an unparsable URL, a URL with no
    /// `state`, or a `state` matching no in-flight flow (all ignored, no crash).
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
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "state" => state = Some(value.into_owned()),
                "error" => error = Some(value.into_owned()),
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

        let outcome = match error {
            Some(err) => OAuthCallback::Error(err),
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
            pending.drain().map(|(_, tx)| tx).collect()
        };
        for tx in drained {
            // Ignore send errors: a dropped receiver is already-cancelled.
            let _ = tx.send(OAuthCallback::Cancelled);
        }
    }

    /// Lock helper that recovers a poisoned mutex (a panicking send would only
    /// have left a fully-formed map behind), avoiding an `.unwrap()`.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, oneshot::Sender<OAuthCallback>>> {
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
/// `keeper://oauth/callback` redirect, the required `client_uri`, and a
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

        let url = "keeper://oauth/callback?code=authcode&state=abc123";
        assert!(registry.resolve(url), "matching state should resolve");

        match rx.await.expect("sender not dropped") {
            OAuthCallback::Redirect(got) => assert_eq!(got, url),
            other => panic!("expected Redirect, got {other:?}"),
        }
        // Entry removed: a second resolve of the same state is a no-op.
        assert!(!registry.resolve(url), "flow already resolved/removed");
    }

    /// A callback carrying `error=` resolves as an `Error` with the server string.
    #[tokio::test]
    async fn resolve_forwards_error_param() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("s1".to_owned());

        assert!(registry.resolve("keeper://oauth/callback?error=access_denied&state=s1"));
        match rx.await.expect("sender not dropped") {
            OAuthCallback::Error(e) => assert_eq!(e, "access_denied"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    /// A callback whose `state` matches no in-flight flow is ignored (no crash).
    #[test]
    fn resolve_spurious_state_is_ignored() {
        let registry = OAuthFlowRegistry::new();
        let _rx = registry.register("real".to_owned());
        assert!(
            !registry.resolve("keeper://oauth/callback?code=x&state=nope"),
            "unknown state must not match"
        );
        // The real flow is untouched.
        assert!(registry.resolve("keeper://oauth/callback?code=x&state=real"));
    }

    /// A callback with no `state` param, and an unparsable URL, are both ignored.
    #[test]
    fn resolve_missing_state_and_bad_url_are_ignored() {
        let registry = OAuthFlowRegistry::new();
        let _rx = registry.register("s".to_owned());
        assert!(!registry.resolve("keeper://oauth/callback?code=x"));
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
        assert!(!registry.resolve("keeper://oauth/callback?state=a"));
    }

    /// Dropping the receiver (flow ended) leaves a stale sender; resolving it is
    /// a harmless no-op that returns `false` and does not panic.
    #[test]
    fn resolve_after_receiver_dropped_is_noop() {
        let registry = OAuthFlowRegistry::new();
        let rx = registry.register("gone".to_owned());
        drop(rx);
        assert!(
            !registry.resolve("keeper://oauth/callback?code=x&state=gone"),
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
        assert!(!registry.resolve("keeper://oauth/callback?code=x&state=leak"));
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
}
