//! Beeper unofficial email-code login flow (Story 2.3, FR-3, AD-17).
//!
//! This module owns **all** `api.beeper.com` HTTP — nothing outside it may touch
//! the Beeper private API (the containment invariant, AD-17). It is deliberately
//! plain `reqwest`/`tokio`/`serde` so keeper-core stays tauri-free.
//!
//! The flow is two IPC round-trips whose intermediate login-request id is held
//! server-side in a [`BeeperFlowRegistry`] (keyed by email) so it never crosses
//! IPC:
//!
//! 1. [`BeeperFlowRegistry::request_code`] — `POST /user/login` (obtain a
//!    `request` id) then `POST /user/login/email` (ask Beeper to email a code),
//!    storing the request id for that email.
//! 2. [`BeeperFlowRegistry::login`] — take the stored request id, `POST
//!    /user/login/response` (submit the emailed code, obtain a JWT), then run the
//!    shared [`super::add_account`] orchestration with a [`BeeperAuthProvider`]
//!    that completes login via `org.matrix.login.jwt` against
//!    [`BEEPER_HOMESERVER`].
//!
//! **Every** failure — a non-2xx from any step, a network/transport error, a
//! request timeout, a missing/renamed JSON field (the private API changed shape),
//! an abandoned flow whose request id is gone, or a JWT/Matrix-login rejection —
//! collapses into one typed [`AuthError::BeeperUnavailable`] whose message
//! carries no secrets (never the emailed code, the JWT, or the bearer). So a
//! private-API change can only ever produce that one named, retriable state and
//! can never corrupt password/OIDC login or be observed from other accounts.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use matrix_sdk::Client;

use crate::error::{AuthError, CoreError};
use crate::platform::Platform;
use crate::vm::AccountVm;

use super::AuthProvider;

/// The fixed Beeper homeserver. A Beeper account is always on `matrix.beeper.com`
/// — the Beeper tab never asks for a homeserver.
pub const BEEPER_HOMESERVER: &str = "https://matrix.beeper.com";

/// Base URL of Beeper's unofficial login API.
const BEEPER_API_BASE: &str = "https://api.beeper.com";

/// The public bearer token every Beeper client sends on the unofficial login
/// API. This is NOT a secret — it is a well-known constant shared by all Beeper
/// clients, so it is safe in source and is not matched by the pre-commit secret
/// scanner (no `syt_` token / private key shape).
const BEEPER_BEARER: &str = "BEEPER-PRIVATE-API-PLEASE-DONT-USE";

/// Explicit per-request timeout so no Beeper HTTP call can ever hang the
/// add-account UI (long enough for a slow round-trip, short enough to fail fast).
const BEEPER_TIMEOUT: Duration = Duration::from_secs(30);

/// Registry of in-flight Beeper login flows, keyed by email (Story 2.3).
///
/// Mirrors `OAuthFlowRegistry`: the shell holds an `Arc<BeeperFlowRegistry>` in
/// its `AppState` and passes clones to the `beeper_request_code` / `login_beeper`
/// / `cancel_beeper` commands. The `request` id obtained in step 1 lives here
/// between the two IPC calls so it never crosses IPC.
///
/// Holds a single shared `reqwest::Client` (built with an explicit timeout) so
/// every Beeper HTTP call reuses one connection pool.
pub struct BeeperFlowRegistry {
    /// The shared HTTP client (explicit timeout; rustls TLS).
    http: reqwest::Client,
    /// In-flight login-request ids, keyed by the email that requested the code.
    pending: Mutex<HashMap<String, String>>,
}

impl Default for BeeperFlowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BeeperFlowRegistry {
    /// Construct an empty registry with an HTTP client carrying the Beeper
    /// request timeout. Built once at startup (`AppState::new`); a build failure
    /// means a fundamentally broken TLS backend that the whole app cannot proceed
    /// past, so we surface it loudly rather than silently ship a timeout-less
    /// client (which would forfeit the mandatory per-request timeout invariant).
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(BEEPER_TIMEOUT)
            .build()
            .expect("failed to build the Beeper HTTP client (broken TLS backend)");
        Self {
            http,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Store a request id for `email`, replacing any prior one (a Retry restarts
    /// the flow, so a stale id is simply overwritten).
    fn store(&self, email: &str, request: String) {
        self.lock().insert(email.to_owned(), request);
    }

    /// Take (remove and return) the request id for `email`, if present. Returns
    /// [`AuthError::BeeperUnavailable`] when absent — a `login` without a prior
    /// `request_code` (an abandoned/expired flow) has nothing to complete.
    fn take(&self, email: &str) -> Result<String, AuthError> {
        self.lock().remove(email).ok_or_else(|| {
            AuthError::BeeperUnavailable("no pending Beeper login for this email".to_owned())
        })
    }

    /// Step 1–2: obtain a login-request id and ask Beeper to email a code.
    ///
    /// `POST /user/login` → parse `request`; `POST /user/login/email`
    /// `{request,email}`; store the request id keyed by `email`. Any non-2xx,
    /// transport error, timeout, or shape change surfaces as
    /// [`AuthError::BeeperUnavailable`].
    pub async fn request_code(&self, email: &str) -> Result<(), CoreError> {
        // Step 1: start a login, obtaining the opaque request id.
        let body = self
            .post_json("/user/login", &serde_json::json!({}))
            .await?;
        let request = parse_login_request(&body)?;

        // Step 2: ask Beeper to email a login code to `email`.
        self.post_ok("/user/login/email", &login_email_body(&request, email))
            .await?;

        self.store(email, request);
        Ok(())
    }

    /// Step 3 + add-account: submit the emailed code, obtain the JWT, and run the
    /// shared account pipeline.
    ///
    /// Takes the stored request id for `email` (missing → `BeeperUnavailable`),
    /// `POST /user/login/response` `{request,response:code}` → parse the `token`
    /// JWT, then `super::add_account(platform, BEEPER_HOMESERVER,
    /// BeeperAuthProvider{jwt})`. The registry entry is removed by `take` before
    /// the network call, so a failed or abandoned attempt leaves no residue.
    pub async fn login(
        &self,
        platform: &dyn Platform,
        email: &str,
        code: &str,
    ) -> Result<AccountVm, CoreError> {
        let request = self.take(email)?;
        let body = self
            .post_json("/user/login/response", &login_response_body(&request, code))
            .await?;
        let jwt = parse_jwt(&body)?;

        let provider = BeeperAuthProvider { jwt };
        super::add_account(platform, BEEPER_HOMESERVER, &provider).await
    }

    /// Clear every pending flow (the `cancel_beeper` command). Leaves zero
    /// residue: an abandoned flow's stored request id is dropped.
    pub fn cancel_all(&self) {
        self.lock().clear();
    }

    /// POST a JSON body to a Beeper API path with the public bearer, returning the
    /// 2xx response body as a string. Any transport/timeout/non-2xx failure →
    /// [`AuthError::BeeperUnavailable`] (secret-free message).
    async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<String, AuthError> {
        let url = format!("{BEEPER_API_BASE}{path}");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(BEEPER_BEARER)
            .json(body)
            .send()
            .await
            .map_err(|_| beeper_unavailable("could not reach the Beeper login service"))?;
        if !resp.status().is_success() {
            return Err(beeper_unavailable(
                "the Beeper login service returned an error",
            ));
        }
        resp.text()
            .await
            .map_err(|_| beeper_unavailable("could not read the Beeper login response"))
    }

    /// POST a JSON body and require only a 2xx (no body parsing).
    async fn post_ok(&self, path: &str, body: &serde_json::Value) -> Result<(), AuthError> {
        self.post_json(path, body).await.map(|_| ())
    }

    /// Lock helper that recovers a poisoned mutex (the map is only ever mutated
    /// under short critical sections), avoiding an `.unwrap()`.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl std::fmt::Debug for BeeperFlowRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.pending.lock().map(|p| p.len()).unwrap_or(0);
        f.debug_struct("BeeperFlowRegistry")
            .field("pending", &len)
            .finish()
    }
}

/// The Beeper `AuthProvider` (Story 2.3, AD-17).
///
/// Completes login by exchanging the Beeper JWT for a Matrix session via
/// matrix-sdk's `login_custom("org.matrix.login.jwt", …)`. The resulting
/// `MatrixSession` is extracted by `add_account` via the existing
/// `StoredSession::from_client` path (→ `Password` variant), so persistence,
/// restore, and sign-out need no changes. Holds the JWT only for the single
/// login; it is never persisted, returned, or logged.
pub struct BeeperAuthProvider {
    /// The Beeper-issued JWT obtained from `/user/login/response`.
    pub jwt: String,
}

impl AuthProvider for BeeperAuthProvider {
    async fn authenticate(
        &self,
        client: &Client,
        _platform: &dyn Platform,
    ) -> Result<(), CoreError> {
        let mut data = matrix_sdk::ruma::serde::JsonObject::new();
        data.insert(
            "token".to_owned(),
            serde_json::Value::String(self.jwt.clone()),
        );
        // Use fixed, secret-free messages: a raw matrix-sdk error Display can echo
        // server-returned request context, and this arm is the one path that holds
        // the JWT — never interpolate `e.to_string()` here (honours the secret-free
        // contract asserted in this module's and `error.rs`'s docs). The raw error
        // is logged separately at debug for diagnostics, never carried in the
        // `AuthError` that crosses the `CoreError`/IPC boundary.
        let builder = client
            .matrix_auth()
            .login_custom("org.matrix.login.jwt", data)
            .map_err(|e| {
                tracing::debug!(error = %e, "beeper: login_custom builder rejected the JWT payload");
                beeper_unavailable("could not start the Beeper JWT login")
            })?;
        builder
            .initial_device_display_name("keeper")
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "beeper: Matrix JWT login was rejected");
                beeper_unavailable("the Beeper JWT login was rejected")
            })?;
        Ok(())
    }

    fn provider(&self) -> crate::vm::Provider {
        crate::vm::Provider::Beeper
    }
}

/// Build the `AuthError::BeeperUnavailable` for a secret-free failure message.
fn beeper_unavailable(msg: &str) -> AuthError {
    AuthError::BeeperUnavailable(msg.to_owned())
}

/// Build the `{request,email}` body for `POST /user/login/email`.
fn login_email_body(request: &str, email: &str) -> serde_json::Value {
    serde_json::json!({ "request": request, "email": email })
}

/// Build the `{request,response}` body for `POST /user/login/response` (the
/// `response` field carries the emailed code).
fn login_response_body(request: &str, code: &str) -> serde_json::Value {
    serde_json::json!({ "request": request, "response": code })
}

/// Parse the `request` id from a `/user/login` response body. A missing/renamed
/// field (the private API changed shape) → [`AuthError::BeeperUnavailable`].
///
/// A pure function so the shape-change path is testable without a network.
fn parse_login_request(body: &str) -> Result<String, AuthError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| beeper_unavailable("the Beeper login response was not valid JSON"))?;
    value
        .get("request")
        .and_then(|v| v.as_str())
        // An empty-but-present field is itself a shape change (e.g. a partial /
        // pending-status body) — fail fast here rather than storing/POSTing a
        // blank request id downstream.
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| beeper_unavailable("the Beeper login response was missing a request id"))
}

/// Parse the `token` JWT from a `/user/login/response` body. A missing/renamed
/// field (the private API changed shape) → [`AuthError::BeeperUnavailable`].
///
/// A pure function so the shape-change path is testable without a network. The
/// error message never echoes the (absent) token.
fn parse_jwt(body: &str) -> Result<String, AuthError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| beeper_unavailable("the Beeper login response was not valid JSON"))?;
    value
        .get("token")
        .and_then(|v| v.as_str())
        // An empty-but-present token is a shape change — reject it here instead of
        // handing an empty JWT to `login_custom` for a doomed round-trip.
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| beeper_unavailable("the Beeper login response was missing a token"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_take_round_trip() {
        let registry = BeeperFlowRegistry::new();
        registry.store("alice@beeper.com", "req-123".to_owned());
        let taken = registry
            .take("alice@beeper.com")
            .expect("request id present");
        assert_eq!(taken, "req-123");
    }

    #[test]
    fn take_removes_the_entry() {
        let registry = BeeperFlowRegistry::new();
        registry.store("alice@beeper.com", "req-123".to_owned());
        let _ = registry.take("alice@beeper.com").expect("first take");
        // A second take of the same email now has nothing.
        let err = registry.take("alice@beeper.com");
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn take_missing_is_beeper_unavailable() {
        let registry = BeeperFlowRegistry::new();
        let err = registry.take("nobody@beeper.com");
        assert!(
            matches!(err, Err(AuthError::BeeperUnavailable(_))),
            "taking a missing flow must be BeeperUnavailable"
        );
    }

    #[test]
    fn store_replaces_stale_request_id() {
        let registry = BeeperFlowRegistry::new();
        registry.store("alice@beeper.com", "old".to_owned());
        registry.store("alice@beeper.com", "new".to_owned());
        assert_eq!(registry.take("alice@beeper.com").expect("present"), "new");
    }

    #[test]
    fn cancel_all_clears_pending() {
        let registry = BeeperFlowRegistry::new();
        registry.store("a@beeper.com", "r1".to_owned());
        registry.store("b@beeper.com", "r2".to_owned());
        registry.cancel_all();
        assert!(matches!(
            registry.take("a@beeper.com"),
            Err(AuthError::BeeperUnavailable(_))
        ));
        assert!(matches!(
            registry.take("b@beeper.com"),
            Err(AuthError::BeeperUnavailable(_))
        ));
    }

    #[test]
    fn parse_login_request_reads_valid_id() {
        let id = parse_login_request(r#"{"request":"abc-123"}"#).expect("parse request");
        assert_eq!(id, "abc-123");
    }

    #[test]
    fn parse_login_request_missing_field_is_shape_change_error() {
        // A 2xx body whose `request` field was renamed/removed — the shape-change
        // path must collapse to BeeperUnavailable, not a panic or wrong success.
        let err = parse_login_request(r#"{"requestId":"abc-123"}"#);
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn parse_login_request_non_json_is_error() {
        let err = parse_login_request("not json at all");
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn parse_login_request_empty_field_is_shape_change_error() {
        // A present-but-empty `request` is drift (partial/pending body) — reject it
        // rather than storing/POSTing a blank request id.
        let err = parse_login_request(r#"{"request":""}"#);
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn parse_jwt_reads_valid_token() {
        let jwt = parse_jwt(r#"{"token":"eyJhbGc.header.sig"}"#).expect("parse token");
        assert_eq!(jwt, "eyJhbGc.header.sig");
    }

    #[test]
    fn parse_jwt_missing_field_is_shape_change_error() {
        // The `token` field renamed/removed — shape change → BeeperUnavailable.
        let err = parse_jwt(r#"{"jwt":"eyJhbGc"}"#);
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn parse_jwt_empty_field_is_shape_change_error() {
        // A present-but-empty `token` is drift — reject it rather than handing an
        // empty JWT to login_custom.
        let err = parse_jwt(r#"{"token":""}"#);
        assert!(matches!(err, Err(AuthError::BeeperUnavailable(_))));
    }

    #[test]
    fn parse_jwt_missing_message_never_echoes_a_token() {
        // Defensive: the secret-free contract — the error string must not carry a
        // token value even in the failure path (there is none, but assert the
        // message is the fixed secret-free copy).
        match parse_jwt(r#"{"jwt":"eyJhbGc"}"#) {
            Err(AuthError::BeeperUnavailable(msg)) => {
                assert!(!msg.contains("eyJhbGc"), "message must not echo a token");
            }
            other => panic!("expected BeeperUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn login_email_body_shape() {
        let body = login_email_body("req-1", "alice@beeper.com");
        assert_eq!(body["request"], "req-1");
        assert_eq!(body["email"], "alice@beeper.com");
    }

    #[test]
    fn login_response_body_carries_code_as_response() {
        let body = login_response_body("req-1", "424242");
        assert_eq!(body["request"], "req-1");
        // The emailed code is sent under the `response` field.
        assert_eq!(body["response"], "424242");
    }
}
