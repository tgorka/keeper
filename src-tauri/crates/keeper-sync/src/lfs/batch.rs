//! The git-LFS batch API (Story 25.2, AD-46).
//!
//! `POST <endpoint>/objects/batch` is the only negotiation step in LFS: the
//! client says what it wants, the server answers with a per-object action (an
//! href plus headers) or a per-object error. Everything in
//! [`crate::lfs::basic`] happens against those hrefs.
//!
//! The wire contract is `git-lfs/docs/api/batch.md` at `main @ d72db1e5`; the
//! server behaviours encoded here were verified against forgejo
//! `main @ 10beaf54`. Four of them will silently break a client that assumes
//! the happy path, and each carries a comment where it is handled:
//!
//! * the LFS media type must be **first** in `Accept` or Forgejo answers 415;
//! * a missing `transfer` key means `basic`, and Forgejo always omits it;
//! * an object with **neither** `actions` nor `error` on an upload batch means
//!   the server already has the content — success, not a malformed response;
//! * `413` is both "batch too large" and "quota exceeded", distinguished only
//!   by the response body.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{Result, SyncError};
use crate::lfs::endpoint;

/// The LFS JSON media type.
pub const MEDIA_TYPE: &str = "application/vnd.git-lfs+json";

/// The exact `Accept` value we send on every LFS JSON request.
///
/// **The media type must come first.** Forgejo's `services/lfs/server.go:59`
/// compares `strings.Split(header, ";")[0]` against [`MEDIA_TYPE`] and answers
/// **415 Unsupported Media Type** on anything else — so this value passes while
/// `*/*, application/vnd.git-lfs+json` does not. reqwest sends no default
/// `Accept` at all, so it must be set explicitly on every request, including
/// the `verify` POST in [`crate::lfs::basic`]. This is byte-for-byte Forgejo's
/// own `modules/lfs/shared.go` `AcceptHeader`.
pub const LFS_ACCEPT: &str = "application/vnd.git-lfs+json;q=0.9, */*;q=0.8";

/// `lfs.transfer.batchSize` default (research §5.10).
pub const DEFAULT_BATCH_SIZE: usize = 100;

/// Fallback wait when a `429` carries no parseable `Retry-After`.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Upper bound on an honoured `Retry-After`.
///
/// Upstream does not cap it. We do, because an unbounded sleep inside a daemon
/// is indistinguishable from a hang — and it costs nothing: the journal
/// re-drives the unit anyway (AD-49), so a cap only means we look again sooner.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// Ceiling on a batch **response** body.
///
/// A batch answer is metadata — 100 objects is tens of kilobytes — so this only
/// guards against a broken or hostile server. Object *content* never comes
/// through this module.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Ceiling on an **error** body. Error responses carry a `message` and two
/// optional URLs; anything larger is noise we refuse to allocate for.
pub(crate) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Which way the objects are moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Download,
    Upload,
}

impl Operation {
    /// The `actions` key this operation looks for.
    pub fn action_key(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Upload => "upload",
        }
    }
}

/// One object in a batch request.
///
/// The canonical request schema sets `additionalProperties: false`, so this
/// struct must carry **nothing** beyond `oid` and `size` — a stray field is a
/// 422 from a strict server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectId {
    pub oid: String,
    pub size: u64,
}

impl ObjectId {
    pub fn new(oid: impl Into<String>, size: u64) -> Self {
        Self {
            oid: oid.into(),
            size,
        }
    }
}

/// The ref a batch is being performed for; exists only so ref-aware
/// authorization can scope a token (LFS >= 2.4). Servers must tolerate it
/// being absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefSpec {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
struct BatchRequest<'a> {
    operation: Operation,
    transfers: &'a [&'a str],
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    reference: Option<RefSpec>,
    objects: &'a [ObjectId],
    hash_algo: &'a str,
}

/// A batch response. Always HTTP 200 on the happy path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<String>,
    #[serde(default)]
    pub objects: Vec<ObjectSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash_algo: Option<String>,
}

impl BatchResponse {
    /// The negotiated transfer adapter.
    ///
    /// **A missing `transfer` means `basic`.** This is not a defensive default:
    /// Forgejo's `BatchHandler` never reads the client's `transfers` list and
    /// leaves the field `omitempty`, so the key is absent from every real
    /// Forgejo response.
    pub fn transfer(&self) -> &str {
        self.transfer.as_deref().unwrap_or("basic")
    }
}

/// One object in a batch response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSpec {
    pub oid: String,
    pub size: u64,
    /// `true` means the `href` is pre-signed and **must not** get credentials
    /// re-attached.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Actions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ObjectError>,
}

/// What the client should do with one object once the batch has answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// The server handed over an action; perform the transfer.
    Transfer,
    /// Nothing to do — the server already holds this content.
    AlreadyPresent,
}

impl ObjectSpec {
    /// The action for `operation`, if the server offered one.
    pub fn action(&self, operation: Operation) -> Option<&Action> {
        let actions = self.actions.as_ref()?;
        match operation {
            Operation::Download => actions.download.as_ref(),
            Operation::Upload => actions.upload.as_ref(),
        }
    }

    /// The `verify` action, if the server asked for a verification callback.
    pub fn verify_action(&self) -> Option<&Action> {
        self.actions.as_ref()?.verify.as_ref()
    }

    /// Classify this object.
    ///
    /// The spec allows exactly one of `actions` / `error` — **or neither**. The
    /// neither case is the one that bites: on an *upload* batch it means the
    /// server already holds the content and the object is a success we skip,
    /// while on a *download* batch it is a server that owes us an href and did
    /// not supply one.
    ///
    /// `host` only feeds error messages; it never appears on the success path.
    pub fn disposition(&self, operation: Operation, host: &str) -> Result<Disposition> {
        // An `error` wins over any action: the spec forbids both being present,
        // and if a server sends both, refusing to transfer is the safe reading.
        if let Some(error) = &self.error {
            return Err(error.to_sync_error(&self.oid, host));
        }
        if self.action(operation).is_some() {
            return Ok(Disposition::Transfer);
        }
        match operation {
            Operation::Upload => Ok(Disposition::AlreadyPresent),
            Operation::Download => Err(SyncError::Config(format!(
                "{host} offered no download action for LFS object {}",
                self.oid
            ))),
        }
    }
}

/// The action set for one object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<Action>,
}

/// One transfer action: where to go and what to send.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub href: String,
    /// Headers the server wants echoed on the transfer request.
    ///
    /// Forgejo copies the batch request's `Authorization` in here verbatim and
    /// removes it only for pre-signed S3 hrefs, so this map is usually how the
    /// credential reaches the data transfer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub header: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// When an action's href stops working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expiry {
    /// Seconds from when the batch response was received.
    In(i64),
    /// An RFC 3339 timestamp, kept as text.
    ///
    /// Not parsed: this crate carries no date library, Forgejo never sends the
    /// field, and inventing an RFC 3339 parser to support a case we have never
    /// observed would be more code than the feature is worth. A consumer that
    /// needs it can parse the string.
    At(String),
    Never,
}

impl Action {
    /// `expires_in` **takes precedence over** `expires_at` — the spec is
    /// explicit, and a server may legitimately send both.
    pub fn expiry(&self) -> Expiry {
        if let Some(seconds) = self.expires_in {
            return Expiry::In(seconds);
        }
        match &self.expires_at {
            Some(at) => Expiry::At(at.clone()),
            None => Expiry::Never,
        }
    }
}

/// A per-object error, whose codes mirror HTTP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectError {
    pub code: i64,
    pub message: String,
}

impl ObjectError {
    /// Map a per-object code onto the engine's taxonomy.
    ///
    /// The server's `message` is deliberately **not** interpolated: it is
    /// attacker-influenced text and `error.rs` requires that a message never
    /// carry anything but ids, hosts, paths, counts and status codes.
    fn to_sync_error(&self, oid: &str, host: &str) -> SyncError {
        match self.code {
            404 => SyncError::Config(format!("{host} does not have LFS object {oid}")),
            409 => SyncError::Config(format!(
                "{host} disagrees about the hash algorithm for LFS object {oid}"
            )),
            410 => SyncError::Config(format!("LFS object {oid} was removed on {host}")),
            // Forgejo returns a per-object 422 for `LFS_MAX_FILE_SIZE`, which
            // only an administrator can change.
            422 => SyncError::Config(format!("{host} rejected LFS object {oid} as invalid")),
            other => SyncError::Network {
                host: host.to_owned(),
                reason: format!("LFS object {oid} failed with code {other}"),
            },
        }
    }
}

/// What a batch endpoint's HTTP status tells the client to do.
#[derive(Debug)]
pub enum BatchAction {
    /// 2xx — read the body.
    Proceed,
    /// 401 — refresh credentials and retry exactly once. `challenge` is
    /// whichever authentication challenge the server sent; see
    /// [`auth_challenge`] for why that is two header names and not one.
    Reauthenticate { challenge: Option<String> },
    /// 413 with a "too many objects" body — halve the batch and retry.
    HalveBatch,
    /// 429 — wait this long, then retry.
    RetryAfter(Duration),
    /// Surface this error; [`SyncError::retriability`] decides what happens next.
    Fail(SyncError),
}

/// Map a batch-endpoint response onto a client action.
///
/// Pure, so the whole status table is testable without a server.
pub fn classify_status(
    status: u16,
    host: &str,
    challenge: Option<&str>,
    retry_after: Option<&str>,
    body: &str,
) -> BatchAction {
    match status {
        200..=299 => BatchAction::Proceed,
        401 => BatchAction::Reauthenticate {
            challenge: challenge.map(str::to_owned),
        },
        // Read access but not write. The token itself was accepted — that is
        // what separates this from the 401 above — so the remedy is a scope,
        // not a new token, and `Forbidden` is what says so. Only a human can
        // grant it, and a fine-grained Forgejo PAT must carry repo scope
        // explicitly.
        403 => BatchAction::Fail(SyncError::Forbidden {
            host: host.to_owned(),
        }),
        // Ambiguous by design: Forgejo answers 404 both when `LFS_START_SERVER`
        // is off and when a private repository is invisible to these
        // credentials. Saying "authentication rejected" would be a guess, so
        // the message names both possibilities instead.
        404 => BatchAction::Fail(SyncError::Config(format!(
            "{host} has no LFS server for this repository, or it is not visible to these credentials"
        ))),
        // 406 is upstream's answer to a bad `Accept`, 415 is Forgejo's. Either
        // means we built the header wrong, which is a bug in us — hence the
        // test that pins the header value.
        406 | 415 => BatchAction::Fail(SyncError::Config(format!(
            "{host} rejected the `{MEDIA_TYPE}` media type"
        ))),
        // One status, two unrelated conditions. Forgejo returns 413 for both
        // `LFS_MAX_BATCH_SIZE` and a per-owner quota, so the **body** is the
        // only discriminator — halving the batch forever would otherwise be the
        // response to a full account.
        413 => {
            if body.to_ascii_lowercase().contains("quota exceeded") {
                BatchAction::Fail(SyncError::Quota {
                    host: host.to_owned(),
                })
            } else {
                BatchAction::HalveBatch
            }
        }
        422 => BatchAction::Fail(SyncError::Config(format!(
            "{host} rejected every object in this LFS batch as invalid"
        ))),
        429 => BatchAction::RetryAfter(retry_after_delay(retry_after)),
        501 => BatchAction::Fail(SyncError::Config(format!(
            "{host} does not implement the LFS batch API"
        ))),
        // Out of storage / bandwidth quota exceeded. Transient in principle,
        // but only a human clears it, so `Quota` is what surfaces the notice.
        507 | 509 => BatchAction::Fail(SyncError::Quota {
            host: host.to_owned(),
        }),
        500..=599 => BatchAction::Fail(SyncError::Network {
            host: host.to_owned(),
            reason: format!("server error {status}"),
        }),
        // An unexpected 4xx is the client's fault and will not fix itself, so
        // it must not be classified as transient.
        other => BatchAction::Fail(SyncError::Config(format!(
            "{host} answered the LFS batch API with an unexpected status {other}"
        ))),
    }
}

/// `Retry-After` as a delta.
///
/// Only the delta-seconds form is parsed. The HTTP-date form is legal but rare,
/// and this crate has no date library; falling back to a fixed wait is both
/// safe and honest, where a wrong date parse would not be.
pub fn retry_after_delay(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(DEFAULT_RETRY_AFTER, Duration::from_secs)
        .min(MAX_RETRY_AFTER)
}

/// Headers every LFS **JSON** request carries.
///
/// Shared with the `verify` POST in [`crate::lfs::basic`]: Forgejo
/// force-injects the `Accept` header on its own verify actions as a workaround
/// for git-lfs#3662, and a client that omits it gets a 415. `host` only feeds
/// an error message.
pub fn json_headers(auth: Option<&str>, host: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::with_capacity(4);
    headers.insert(ACCEPT, HeaderValue::from_static(LFS_ACCEPT));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(MEDIA_TYPE));
    headers.insert(USER_AGENT, HeaderValue::from_static(crate::AGENT));
    if let Some(auth) = auth {
        headers.insert(AUTHORIZATION, sensitive_auth(auth, host)?);
    }
    Ok(headers)
}

/// Build an `Authorization` value, marked sensitive so the `http` stack's own
/// diagnostics redact it.
pub fn sensitive_auth(auth: &str, host: &str) -> Result<HeaderValue> {
    // The failing value is never echoed: it is the credential (AD-53).
    let mut value = HeaderValue::from_str(auth).map_err(|_| SyncError::Auth {
        host: host.to_owned(),
    })?;
    value.set_sensitive(true);
    Ok(value)
}

/// Supplies a fresh `Authorization` header value after a 401.
///
/// Matches the crate's sink convention (`Box<dyn Fn(..) + Send + Sync>`, see
/// [`crate::progress::ProgressSink`]). The argument is the challenge
/// [`auth_challenge`] read off the rejection; `None` back means "no fresh
/// credential", which turns the 401 into [`SyncError::Auth`] rather than a
/// retry storm against a git host.
pub type AuthRefresh = Box<dyn Fn(Option<&str>) -> Option<String> + Send + Sync>;

/// A client for one repository's batch endpoint.
pub struct BatchClient {
    client: reqwest::Client,
    endpoint: Url,
    /// The full `Authorization` header value.
    ///
    /// Behind a lock only so a 401 refresh sticks for subsequent batches; a
    /// re-auth that had to be repeated per request would double the round trips
    /// against a server with short-lived tokens.
    auth: std::sync::RwLock<Option<String>>,
    reference: Option<RefSpec>,
    refresh: Option<AuthRefresh>,
}

impl std::fmt::Debug for BatchClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written so the credential can never reach a log line (AD-53).
        f.debug_struct("BatchClient")
            .field("endpoint", &self.endpoint.as_str())
            .field(
                "authenticated",
                &self.auth.read().is_ok_and(|a| a.is_some()),
            )
            .finish()
    }
}

impl BatchClient {
    pub fn new(client: reqwest::Client, endpoint: Url, auth: Option<String>) -> Self {
        Self {
            client,
            endpoint,
            auth: std::sync::RwLock::new(auth),
            reference: None,
            refresh: None,
        }
    }

    /// Scope the batch to a ref, e.g. `refs/heads/main`.
    pub fn with_ref(mut self, name: impl Into<String>) -> Self {
        self.reference = Some(RefSpec { name: name.into() });
        self
    }

    /// Install the credential refresher consulted on a 401.
    pub fn with_auth_refresh(mut self, refresh: AuthRefresh) -> Self {
        self.refresh = Some(refresh);
        self
    }

    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub fn host(&self) -> &str {
        self.endpoint.host_str().unwrap_or("unknown host")
    }

    /// The current `Authorization` value, if any.
    pub fn auth(&self) -> Option<String> {
        // A poisoned lock means another thread panicked mid-refresh. Treating
        // that as "no credential" degrades to a 401 rather than propagating a
        // panic through the sync loop.
        self.auth.read().ok().and_then(|auth| auth.clone())
    }

    /// Ask the server where to fetch each object from.
    pub async fn download(&self, objects: &[ObjectId]) -> Result<Vec<ObjectSpec>> {
        self.batch(Operation::Download, objects).await
    }

    /// Ask the server where to send each object.
    pub async fn upload(&self, objects: &[ObjectId]) -> Result<Vec<ObjectSpec>> {
        self.batch(Operation::Upload, objects).await
    }

    /// This client's headers for an LFS JSON request.
    pub fn json_headers(&self) -> Result<HeaderMap> {
        json_headers(self.auth().as_deref(), self.host())
    }

    async fn batch(&self, operation: Operation, objects: &[ObjectId]) -> Result<Vec<ObjectSpec>> {
        if objects.is_empty() {
            return Ok(Vec::new());
        }

        let mut collected = Vec::with_capacity(objects.len());
        let mut batch_size = DEFAULT_BATCH_SIZE;
        let mut offset = 0usize;
        while offset < objects.len() {
            let end = (offset + batch_size).min(objects.len());
            match self.post_batch(operation, &objects[offset..end]).await? {
                BatchStep::Objects(mut specs) => {
                    collected.append(&mut specs);
                    offset = end;
                }
                BatchStep::Halve => {
                    if batch_size <= 1 {
                        return Err(SyncError::Config(format!(
                            "{} rejects even a single-object LFS batch",
                            self.host()
                        )));
                    }
                    batch_size /= 2;
                    tracing::debug!(
                        host = self.host(),
                        batch_size,
                        "LFS batch too large; halving and retrying"
                    );
                }
            }
        }
        Ok(collected)
    }

    async fn post_batch(&self, operation: Operation, objects: &[ObjectId]) -> Result<BatchStep> {
        let url = endpoint::sub_path(&self.endpoint, "objects/batch")?;
        let payload = serde_json::to_vec(&BatchRequest {
            operation,
            // Advertising only `basic` is not a simplification: it is the only
            // adapter Forgejo implements, and `tus`/`ssh`/`multipart` would
            // never be selected (research §6.3.7).
            transfers: &["basic"],
            reference: self.reference.clone(),
            objects,
            hash_algo: "sha256",
        })
        .map_err(|err| SyncError::Config(format!("cannot encode an LFS batch request: {err}")))?;

        // Exactly one retry per condition. Longer-horizon retry belongs to the
        // journal supervisor (AD-49), not to a second retry engine in here.
        let mut reauth_spent = false;
        let mut wait_spent = false;
        loop {
            let response = self
                .client
                .post(url.clone())
                .headers(self.json_headers()?)
                .body(payload.clone())
                .send()
                .await
                .map_err(|err| self.network_error(&err))?;

            let status = response.status().as_u16();
            let challenge = auth_challenge(response.headers());
            let retry_after = header_string(response.headers(), "retry-after");
            // Bounded, not `text()`: a batch answer is metadata (tens of
            // kilobytes for 100 objects), and an unbounded read of a broken or
            // hostile response is the one place this client could allocate
            // without limit. A truncation surfaces as a parse error below.
            let body = bounded_body(response, MAX_RESPONSE_BYTES).await;

            match classify_status(
                status,
                self.host(),
                challenge.as_deref(),
                retry_after.as_deref(),
                &body,
            ) {
                BatchAction::Proceed => {
                    let parsed: BatchResponse =
                        serde_json::from_str(&body).map_err(|err| SyncError::Network {
                            host: self.host().to_owned(),
                            // Line/column only; the body may quote a path.
                            reason: format!(
                                "malformed LFS batch response at line {} column {}",
                                err.line(),
                                err.column()
                            ),
                        })?;
                    let transfer = parsed.transfer();
                    if transfer != "basic" {
                        return Err(SyncError::Config(format!(
                            "{} chose the unsupported `{transfer}` LFS transfer adapter",
                            self.host()
                        )));
                    }
                    return Ok(BatchStep::Objects(parsed.objects));
                }
                BatchAction::HalveBatch => return Ok(BatchStep::Halve),
                BatchAction::Reauthenticate { challenge } => {
                    if reauth_spent {
                        return Err(SyncError::Auth {
                            host: self.host().to_owned(),
                        });
                    }
                    reauth_spent = true;
                    self.reauthenticate(challenge.as_deref())?;
                }
                BatchAction::RetryAfter(delay) => {
                    if wait_spent {
                        return Err(SyncError::Network {
                            host: self.host().to_owned(),
                            reason: "rate limited by the LFS server".to_owned(),
                        });
                    }
                    wait_spent = true;
                    tracing::info!(
                        host = self.host(),
                        delay_ms = delay.as_millis() as u64,
                        "LFS batch rate limited; honouring Retry-After"
                    );
                    tokio::time::sleep(delay).await;
                }
                BatchAction::Fail(err) => return Err(err),
            }
        }
    }

    /// Replace the stored credential from the refresher, or fail.
    fn reauthenticate(&self, challenge: Option<&str>) -> Result<()> {
        let auth_error = || SyncError::Auth {
            host: self.host().to_owned(),
        };
        let refreshed = self
            .refresh
            .as_ref()
            .and_then(|refresh| refresh(challenge))
            .ok_or_else(auth_error)?;
        let mut slot = self.auth.write().map_err(|_| auth_error())?;
        *slot = Some(refreshed);
        Ok(())
    }

    /// Build a `Network` error from a transport failure.
    ///
    /// The reason comes from the error's *kind*, never its `Display`: reqwest
    /// documents that its errors may embed the full URL, and an action href can
    /// carry a pre-signed token (AD-53).
    fn network_error(&self, err: &reqwest::Error) -> SyncError {
        SyncError::Network {
            host: self.host().to_owned(),
            reason: transport_reason(err).to_owned(),
        }
    }
}

/// One step of the halving loop.
enum BatchStep {
    Objects(Vec<ObjectSpec>),
    Halve,
}

/// A credential-safe description of a transport failure.
pub(crate) fn transport_reason(err: &reqwest::Error) -> &'static str {
    // `is_connect` is asked first because a connect timeout answers true to
    // both, and of the two answers "could not connect" is the one an operator
    // can act on. What remains under `is_timeout` is then the case worth naming
    // precisely: an established connection that stopped delivering bytes, which
    // `crate::http::READ_TIMEOUT` is what turns into an error at all.
    if err.is_connect() {
        "could not connect"
    } else if err.is_timeout() {
        "the connection went silent"
    } else if err.is_redirect() {
        "too many redirects"
    } else if err.is_decode() || err.is_body() {
        "the response body was truncated or malformed"
    } else if err.is_request() {
        "the request could not be sent"
    } else {
        "transport failure"
    }
}

/// Read a header as an owned `String`, dropping non-ASCII values.
///
/// Takes the name as a `&str` so no call site has to spell out `http`'s sealed
/// `AsHeaderName`; header lookup is case-insensitive either way.
pub(crate) fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

/// The authentication challenge a rejected LFS response carries, under
/// whichever of the two header names the server chose.
///
/// `LFS-Authenticate` is git-lfs's own: it mirrors `WWW-Authenticate` under a
/// custom key so a browser hitting the endpoint does not raise a password
/// prompt. It is **optional**, and Forgejo does not send it — `services/lfs/
/// server.go` answers a rejected batch with a plain
/// `WWW-Authenticate: Basic realm="gitea-lfs"`. Reading only the LFS spelling
/// meant the challenge was always `None` against the one server this engine
/// exists to talk to, so the whole re-authentication path was unreachable
/// there (Story 34.15).
///
/// The LFS-specific header still wins when both are present: a server that
/// bothers to send it is being explicit about what the *LFS* endpoint wants.
pub(crate) fn auth_challenge(headers: &HeaderMap) -> Option<String> {
    header_string(headers, "lfs-authenticate")
        .or_else(|| header_string(headers, "www-authenticate"))
}

/// Read at most `max` bytes of a response body, lossily, as UTF-8.
///
/// Never [`reqwest::Response::text`]: that has no ceiling, and this client's
/// standing promise is that it allocates in bounded chunks whatever a server
/// sends (NFR-23). A read error mid-way keeps what arrived — a truncated
/// diagnostic still beats none, and the HTTP status is what actually drives the
/// decision.
pub(crate) async fn bounded_body(mut response: reqwest::Response, max: usize) -> String {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < max {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let room = max - buf.len();
                let take = chunk.len().min(room);
                buf.extend_from_slice(&chunk[..take]);
            }
            Ok(None) | Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OID_A: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    fn client() -> BatchClient {
        BatchClient::new(
            reqwest::Client::new(),
            Url::parse("https://host.example/o/r.git/info/lfs").expect("endpoint"),
            Some("Basic dXNlcjpwYXNz".to_owned()),
        )
    }

    #[test]
    fn accept_puts_the_lfs_media_type_first_exactly_as_forgejo_checks() {
        let headers = client().json_headers().expect("headers");
        let accept = headers
            .get(ACCEPT)
            .and_then(|value| value.to_str().ok())
            .expect("Accept is always set; reqwest supplies no default");

        // This *is* Forgejo's check, `strings.Split(hdr, ";")[0]`.
        assert_eq!(accept.split(';').next(), Some(MEDIA_TYPE));
        // The shape that earns a 415, for contrast.
        assert_ne!(
            "*/*, application/vnd.git-lfs+json".split(';').next(),
            Some(MEDIA_TYPE)
        );

        assert_eq!(
            headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some(MEDIA_TYPE)
        );
        assert_eq!(
            headers.get(USER_AGENT).and_then(|v| v.to_str().ok()),
            Some(crate::AGENT)
        );
        assert!(headers
            .get(AUTHORIZATION)
            .is_some_and(reqwest::header::HeaderValue::is_sensitive));
    }

    #[test]
    fn request_body_matches_the_canonical_schema() {
        let objects = [ObjectId::new(OID_A, 123)];
        let request = BatchRequest {
            operation: Operation::Download,
            transfers: &["basic"],
            reference: Some(RefSpec {
                name: "refs/heads/main".to_owned(),
            }),
            objects: &objects,
            hash_algo: "sha256",
        };

        assert_eq!(
            serde_json::to_value(&request).expect("serialize"),
            json!({
                "operation": "download",
                "transfers": ["basic"],
                "ref": { "name": "refs/heads/main" },
                "objects": [{ "oid": OID_A, "size": 123 }],
                "hash_algo": "sha256"
            })
        );
    }

    #[test]
    fn request_objects_carry_only_oid_and_size() {
        // `additionalProperties: false` in the canonical schema — a stray field
        // is a 422 from a strict server.
        let value = serde_json::to_value(ObjectId::new(OID_A, 1)).expect("serialize");
        let object = value.as_object().expect("an object");
        assert_eq!(object.len(), 2, "{object:?}");
        assert!(object.contains_key("oid") && object.contains_key("size"));
    }

    #[test]
    fn the_ref_is_omitted_rather_than_sent_as_null() {
        let objects = [ObjectId::new(OID_A, 1)];
        let request = BatchRequest {
            operation: Operation::Upload,
            transfers: &["basic"],
            reference: None,
            objects: &objects,
            hash_algo: "sha256",
        };
        let value = serde_json::to_value(&request).expect("serialize");
        assert!(!value.as_object().is_some_and(|o| o.contains_key("ref")));
        assert_eq!(value["operation"], json!("upload"));
    }

    #[test]
    fn the_spec_response_body_deserializes_with_every_field_intact() {
        let body = json!({
            "transfer": "basic",
            "objects": [{
                "oid": OID_A,
                "size": 123,
                "authenticated": true,
                "actions": {
                    "download": {
                        "href": "https://some-download.com",
                        "header": { "Key": "value" },
                        "expires_in": 86400,
                        "expires_at": "2016-11-10T15:29:07Z"
                    }
                }
            }],
            "hash_algo": "sha256"
        })
        .to_string();

        let parsed: BatchResponse = serde_json::from_str(&body).expect("deserialize");
        assert_eq!(parsed.transfer(), "basic");
        assert_eq!(parsed.hash_algo.as_deref(), Some("sha256"));

        let object = parsed.objects.first().expect("one object");
        assert_eq!(object.oid, OID_A);
        assert_eq!(object.size, 123);
        assert!(object.authenticated);

        let action = object.action(Operation::Download).expect("download action");
        assert_eq!(action.href, "https://some-download.com");
        assert_eq!(action.header.get("Key").map(String::as_str), Some("value"));
        assert_eq!(
            object
                .disposition(Operation::Download, "host.example")
                .expect("an object with a download action can be transferred"),
            Disposition::Transfer
        );

        // And it survives a round trip, so nothing is silently dropped.
        let reserialized = serde_json::to_value(&parsed).expect("serialize");
        assert_eq!(
            reserialized,
            serde_json::from_str::<serde_json::Value>(&body).expect("reparse")
        );
    }

    #[test]
    fn a_missing_transfer_key_means_basic() {
        // Forgejo never sends `transfer`, so this is the common case, not an
        // edge case.
        let body = json!({ "objects": [] }).to_string();
        let parsed: BatchResponse = serde_json::from_str(&body).expect("deserialize");
        assert_eq!(parsed.transfer, None);
        assert_eq!(parsed.transfer(), "basic");
    }

    #[test]
    fn expires_in_beats_expires_at() {
        let both = Action {
            href: "https://x/".to_owned(),
            header: BTreeMap::new(),
            expires_in: Some(86_400),
            expires_at: Some("2016-11-10T15:29:07Z".to_owned()),
        };
        assert_eq!(both.expiry(), Expiry::In(86_400));

        let only_at = Action {
            expires_in: None,
            ..both.clone()
        };
        assert_eq!(
            only_at.expiry(),
            Expiry::At("2016-11-10T15:29:07Z".to_owned())
        );

        let neither = Action {
            expires_in: None,
            expires_at: None,
            ..both
        };
        assert_eq!(neither.expiry(), Expiry::Never);
    }

    #[test]
    fn neither_actions_nor_error_means_already_present_on_upload_only() {
        let bare = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 5,
            ..ObjectSpec::default()
        };

        // The server already holds it: success, and the object is skipped.
        assert_eq!(
            bare.disposition(Operation::Upload, "host.example")
                .expect("a bare upload object is a success"),
            Disposition::AlreadyPresent
        );

        // On a download the server owes us an href; silence is a failure.
        let err = bare
            .disposition(Operation::Download, "host.example")
            .expect_err("a download with no action cannot proceed");
        assert_eq!(err.code(), "config");
    }

    #[test]
    fn a_per_object_error_wins_over_any_action() {
        let spec = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 5,
            actions: Some(Actions {
                download: Some(Action {
                    href: "https://x/".to_owned(),
                    ..Action::default()
                }),
                ..Actions::default()
            }),
            error: Some(ObjectError {
                code: 404,
                message: "Object does not exist".to_owned(),
            }),
            ..ObjectSpec::default()
        };

        let err = spec
            .disposition(Operation::Download, "host.example")
            .expect_err("an errored object must not be transferred");
        assert_eq!(err.code(), "config");
        // The server's prose is never interpolated into our message.
        assert!(!err.to_string().contains("Object does not exist"), "{err}");
    }

    #[test]
    fn per_object_codes_map_onto_the_taxonomy() {
        let cases = [
            (404, "config"),
            (409, "config"),
            (410, "config"),
            (422, "config"),
            (503, "network"),
        ];
        for (code, expected) in cases {
            let error = ObjectError {
                code,
                message: String::new(),
            };
            assert_eq!(
                error.to_sync_error(OID_A, "host.example").code(),
                expected,
                "per-object code {code}"
            );
        }
    }

    #[test]
    fn every_documented_status_maps_to_the_right_action() {
        let host = "host.example";
        let classify = |status: u16, body: &str| classify_status(status, host, None, None, body);

        assert!(matches!(classify(200, ""), BatchAction::Proceed));

        match classify_status(401, host, Some("Basic realm=gitea-lfs"), None, "") {
            // The challenge is what the re-auth path reads to pick a scheme.
            BatchAction::Reauthenticate { challenge } => {
                assert_eq!(challenge.as_deref(), Some("Basic realm=gitea-lfs"));
            }
            other => panic!("401 produced {other:?}"),
        }

        for (status, code) in [
            // 403 is not 401: the token was accepted and then refused, and the
            // two must not read as the same sentence to the person holding it.
            (403u16, "forbidden"),
            (404, "config"),
            (406, "config"),
            (415, "config"),
            (422, "config"),
            (501, "config"),
            (507, "quota"),
            (509, "quota"),
            (500, "network"),
            (502, "network"),
            (503, "network"),
            (504, "network"),
            // An unexpected 4xx must be permanent, not retried forever.
            (418, "config"),
        ] {
            match classify(status, "") {
                BatchAction::Fail(err) => {
                    assert_eq!(err.code(), code, "status {status}");
                }
                other => panic!("status {status} produced {other:?}"),
            }
        }
    }

    #[test]
    fn a_413_is_told_apart_by_its_body_not_its_status() {
        // "Batch too large": halve and retry.
        assert!(matches!(
            classify_status(413, "host.example", None, None, "too many objects"),
            BatchAction::HalveBatch
        ));

        // Forgejo reuses 413 for a per-owner quota. Halving would loop forever
        // against a full account, so the body is the discriminator.
        match classify_status(413, "host.example", None, None, "Quota Exceeded") {
            BatchAction::Fail(err) => {
                assert_eq!(err.code(), "quota");
                assert!(err.needs_user_action());
            }
            other => panic!("quota 413 produced {other:?}"),
        }
    }

    #[test]
    fn retry_after_is_honoured_parsed_and_bounded() {
        match classify_status(429, "host.example", None, Some("120"), "") {
            BatchAction::RetryAfter(delay) => assert_eq!(delay, Duration::from_secs(120)),
            other => panic!("429 produced {other:?}"),
        }

        // An HTTP-date is legal but unparsed here; the fallback keeps us moving.
        assert_eq!(
            retry_after_delay(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            DEFAULT_RETRY_AFTER
        );
        assert_eq!(retry_after_delay(None), DEFAULT_RETRY_AFTER);
        // A hostile or broken value must not park the daemon indefinitely.
        assert_eq!(retry_after_delay(Some("999999999")), MAX_RETRY_AFTER);
    }

    /// The header the recovery path reads — which was the one header Forgejo
    /// never sends.
    #[test]
    fn a_challenge_is_read_from_either_header_name() {
        let mut headers = HeaderMap::new();

        // No challenge at all is the terse-server case, not a malformed one.
        assert_eq!(auth_challenge(&headers), None);

        // What Forgejo actually answers a rejected batch with. Reading only
        // `LFS-Authenticate` left this invisible, so the re-auth path could
        // never fire against the one server this engine targets.
        headers.insert(
            "www-authenticate",
            HeaderValue::from_static("Basic realm=\"gitea-lfs\",charset=\"UTF-8\""),
        );
        assert_eq!(
            auth_challenge(&headers).as_deref(),
            Some("Basic realm=\"gitea-lfs\",charset=\"UTF-8\"")
        );

        // A server that sends the LFS-specific one is being explicit about the
        // LFS endpoint, so it wins.
        headers.insert(
            "lfs-authenticate",
            HeaderValue::from_static("Basic realm=\"lfs\""),
        );
        assert_eq!(
            auth_challenge(&headers).as_deref(),
            Some("Basic realm=\"lfs\"")
        );
    }

    #[tokio::test]
    async fn an_empty_batch_never_reaches_the_network() {
        // Guards the caller contract: nothing is queued for a profile whose
        // scan found no LFS objects.
        let client = client();
        assert_eq!(client.download(&[]).await.expect("download"), Vec::new());
        assert_eq!(client.upload(&[]).await.expect("upload"), Vec::new());
    }

    #[test]
    fn debug_output_never_carries_the_credential() {
        // AD-53: `BatchClient` ends up in `tracing` spans.
        let rendered = format!("{:?}", client());
        assert!(!rendered.contains("dXNlcjpwYXNz"), "{rendered}");
        assert!(rendered.contains("authenticated: true"), "{rendered}");
    }
}
