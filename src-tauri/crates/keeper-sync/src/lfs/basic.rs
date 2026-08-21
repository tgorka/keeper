//! The `basic` transfer adapter (Story 25.3, AD-46).
//!
//! `basic` is the only adapter Forgejo implements and the only one universally
//! supported anywhere, so there is no adapter negotiation to build: downloads
//! are `GET`s that may be resumed with an HTTP `Range`, uploads are a single
//! `PUT` that cannot be resumed at all.
//!
//! Nothing here ever holds an object in memory. Downloads stream through
//! [`reqwest::Response::chunk`] into a `.part` file while a single `Sha256`
//! runs alongside; uploads stream out of a file through a body type that
//! advertises an exact length. A 40 GB video costs one 64 KiB buffer in each
//! direction (NFR-23).
//!
//! # The server behaviours encoded here
//!
//! Each is a real defect or quirk found in the field, and each carries a
//! comment at its handling site so it survives the next refactor:
//!
//! * Forgejo's `Content-Range` **complete-length is wrong** (`bytes {from}-{to}/{size-from}`),
//!   so only the *start* byte may be validated;
//! * Forgejo parses `Range` offsets with a **32-bit** `ParseInt`, so resume
//!   above 2 GiB silently returns the wrong bytes;
//! * a **`200`** in reply to a `Range` request means the server ignored it;
//! * `authenticated: true` means the href is **pre-signed** and re-attaching a
//!   credential makes S3 answer 400;
//! * `403` on a `PUT` means the token expired and is worth one retry, while
//!   `422` must never be retried.

use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, RANGE, USER_AGENT,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWriteExt};
use url::Url;

use crate::error::{Result, SyncError};
use crate::lfs::batch::{
    auth_challenge, bounded_body, header_string, json_headers, sensitive_auth, transport_reason,
    Action, AuthRefresh, Disposition, ObjectId, ObjectSpec, Operation, MAX_ERROR_BODY_BYTES,
};
use crate::lfs::store::{self, LfsStore};

/// `lfs.concurrenttransfers` default (research §5.10).
///
/// Applied by [`BasicTransfer::download_all`] and
/// [`BasicTransfer::upload_all`]; a caller driving single transfers owns its
/// own window.
pub const DEFAULT_CONCURRENT_TRANSFERS: usize = 8;

/// `lfs.transfer.maxretries` default (research §5.10). Bounds the per-object
/// attempt loop; longer-horizon retry belongs to the journal supervisor (AD-49).
pub const DEFAULT_MAX_RETRIES: u32 = 8;

/// Roughly one progress event per object per this interval.
///
/// Per *chunk* would be ~15 000 events/s for a fast download — enough to starve
/// the webview that is meant to render them.
pub const DEFAULT_PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Highest `Range` offset we will ask a server for.
///
/// Forgejo parses both `Range` offsets with `strconv.ParseInt(..., 10, 32)`, so
/// anything at or above 2 GiB parses as garbage and the server answers with the
/// wrong bytes. Restarting a 2 GiB download is expensive; being handed the
/// wrong bytes is worse, and we would only find out at the final digest check.
pub const RESUME_OFFSET_CEILING: u64 = 1 << 31;

/// I/O granularity in both directions.
const CHUNK_BYTES: usize = 64 * 1024;

/// Write buffer over the staged `.part` file.
const WRITE_BUFFER_BYTES: usize = 256 * 1024;

/// A transfer lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEvent {
    Started {
        oid: String,
        size: u64,
    },
    Progress {
        oid: String,
        bytes_done: u64,
    },
    Completed {
        oid: String,
    },
    Failed {
        oid: String,
        /// [`SyncError::code`] — the stable discriminant a UI can branch on.
        code: &'static str,
        error: String,
    },
}

/// A sink for transfer events. `false` means "stop producing", matching
/// [`crate::progress::ProgressSink`].
pub type TransferSink = Box<dyn Fn(TransferEvent) -> bool + Send + Sync>;

/// Rate-limits `Progress` events to at most one per interval.
///
/// Time is a parameter rather than a read of the clock, which is what makes the
/// coalescing testable at its boundaries.
#[derive(Debug, Clone)]
pub struct ProgressCoalescer {
    interval: Duration,
    last_emit: Option<Instant>,
}

impl ProgressCoalescer {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_emit: None,
        }
    }

    /// Whether a `Progress` event may be emitted at `now`.
    ///
    /// The first call always yields `true`, so a transfer reports something
    /// immediately instead of staying blank for the first interval.
    pub fn should_emit(&mut self, now: Instant) -> bool {
        let due = match self.last_emit {
            None => true,
            Some(last) => now.saturating_duration_since(last) >= self.interval,
        };
        if due {
            self.last_emit = Some(now);
        }
        due
    }
}

/// Shared progress plumbing.
///
/// Separate from [`BasicTransfer`] because the upload body needs it too, and
/// the body must be `'static` to go through [`reqwest::Body::wrap`].
struct Reporter {
    sink: Option<Arc<TransferSink>>,
    /// Set once a sink has answered `false`.
    detached: AtomicBool,
    interval: Duration,
}

impl Reporter {
    fn emit(&self, event: TransferEvent) {
        let Some(sink) = &self.sink else {
            return;
        };
        if self.detached.load(Ordering::Relaxed) {
            return;
        }
        if !sink(event) {
            // The consumer went away. The transfer itself continues: it is
            // journaled work (AD-49), not a UI subscription.
            self.detached.store(true, Ordering::Relaxed);
        }
    }

    fn coalescer(&self) -> ProgressCoalescer {
        ProgressCoalescer::new(self.interval)
    }
}

/// The `basic` transfer adapter.
pub struct BasicTransfer {
    client: reqwest::Client,
    store: LfsStore,
    reporter: Arc<Reporter>,
    refresh: Option<AuthRefresh>,
}

impl std::fmt::Debug for BasicTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicTransfer")
            .field("store", &self.store.root())
            .finish_non_exhaustive()
    }
}

impl BasicTransfer {
    pub fn new(client: reqwest::Client, store: LfsStore) -> Self {
        Self {
            client,
            store,
            reporter: Arc::new(Reporter {
                sink: None,
                detached: AtomicBool::new(false),
                interval: DEFAULT_PROGRESS_INTERVAL,
            }),
            refresh: None,
        }
    }

    pub fn with_sink(mut self, sink: TransferSink) -> Self {
        self.reporter = Arc::new(Reporter {
            sink: Some(Arc::new(sink)),
            detached: AtomicBool::new(false),
            interval: self.reporter.interval,
        });
        self
    }

    pub fn with_progress_interval(mut self, interval: Duration) -> Self {
        self.reporter = Arc::new(Reporter {
            sink: self.reporter.sink.clone(),
            detached: AtomicBool::new(false),
            interval,
        });
        self
    }

    /// Install the credential refresher consulted on a 401/403.
    pub fn with_auth_refresh(mut self, refresh: AuthRefresh) -> Self {
        self.refresh = Some(refresh);
        self
    }

    pub fn store(&self) -> &LfsStore {
        &self.store
    }

    /// Fetch one object into the local store.
    pub async fn download(&self, spec: &ObjectSpec, auth: Option<&str>) -> Result<()> {
        let host = host_of(spec.action(Operation::Download).map(|a| a.href.as_str()));
        if self.store.contains(&spec.oid, spec.size) {
            // Already materialized — a re-driven journal unit, or two profiles
            // sharing content. Content addressing makes this unambiguous.
            return Ok(());
        }
        let action = match spec.disposition(Operation::Download, &host)? {
            Disposition::Transfer => spec
                .action(Operation::Download)
                .ok_or_else(|| missing_action(&spec.oid, &host))?,
            // `disposition` never reports this for a download; handled so the
            // match stays exhaustive without a catch-all that could hide a
            // future variant.
            Disposition::AlreadyPresent => return Ok(()),
        };

        self.store.ensure_layout()?;
        self.reporter.emit(TransferEvent::Started {
            oid: spec.oid.clone(),
            size: spec.size,
        });
        self.finish(
            &spec.oid,
            self.download_inner(spec, action, auth, &host).await,
        )
    }

    /// Send one object, then run its `verify` callback.
    ///
    /// `source` is the file holding the object's raw bytes — the LFS store's
    /// own copy, or the worktree file during a clean.
    pub async fn upload(&self, spec: &ObjectSpec, source: &Path, auth: Option<&str>) -> Result<()> {
        let host = host_of(spec.action(Operation::Upload).map(|a| a.href.as_str()));
        let action = match spec.disposition(Operation::Upload, &host)? {
            Disposition::Transfer => spec
                .action(Operation::Upload)
                .ok_or_else(|| missing_action(&spec.oid, &host))?,
            Disposition::AlreadyPresent => {
                // Neither `actions` nor `error`: the server already holds the
                // content. That is a success by the spec, not a malformed
                // response — and it is the only way an upload is ever skipped,
                // since Forgejo hashes the PUT body as proof of possession.
                self.reporter.emit(TransferEvent::Completed {
                    oid: spec.oid.clone(),
                });
                return Ok(());
            }
        };

        self.reporter.emit(TransferEvent::Started {
            oid: spec.oid.clone(),
            size: spec.size,
        });
        let outcome = self.upload_inner(spec, action, source, auth, &host).await;
        let outcome = match outcome {
            Ok(()) => self.verify(spec, auth, &host).await,
            Err(err) => Err(err),
        };
        self.finish(&spec.oid, outcome)
    }

    /// Download every object, at most [`DEFAULT_CONCURRENT_TRANSFERS`] at once.
    ///
    /// Returns one result per object in completion order. A failure never
    /// aborts its siblings: each object is a separate journal unit, and the
    /// scheduler decides what to re-drive.
    pub async fn download_all(
        self: Arc<Self>,
        specs: Vec<ObjectSpec>,
        auth: Option<String>,
    ) -> Vec<(String, Result<()>)> {
        self.run_all(specs, auth, None).await
    }

    /// Upload every object, at most [`DEFAULT_CONCURRENT_TRANSFERS`] at once.
    ///
    /// Sources come from the local store, which is where a cleaned worktree
    /// file already lives.
    pub async fn upload_all(
        self: Arc<Self>,
        specs: Vec<ObjectSpec>,
        auth: Option<String>,
    ) -> Vec<(String, Result<()>)> {
        let store = self.store.clone();
        self.run_all(specs, auth, Some(store)).await
    }

    async fn run_all(
        self: Arc<Self>,
        specs: Vec<ObjectSpec>,
        auth: Option<String>,
        upload_from: Option<LfsStore>,
    ) -> Vec<(String, Result<()>)> {
        type Outcome = (String, Result<()>);

        let mut results: Vec<Outcome> = Vec::with_capacity(specs.len());
        let mut pending = specs.into_iter();
        let mut running: tokio::task::JoinSet<Outcome> = tokio::task::JoinSet::new();

        let mut spawn_next = |running: &mut tokio::task::JoinSet<Outcome>| {
            let Some(spec) = pending.next() else {
                return false;
            };
            let transfer = Arc::clone(&self);
            let auth = auth.clone();
            let source = upload_from
                .as_ref()
                .map(|store| store.object_path(&spec.oid));
            running.spawn(async move {
                let outcome = match &source {
                    Some(path) => transfer.upload(&spec, path, auth.as_deref()).await,
                    None => transfer.download(&spec, auth.as_deref()).await,
                };
                (spec.oid, outcome)
            });
            true
        };

        for _ in 0..DEFAULT_CONCURRENT_TRANSFERS {
            if !spawn_next(&mut running) {
                break;
            }
        }
        // A plain `loop` rather than `while let`: the `join_next` future
        // borrows the set for the whole of a `while let` body, which would
        // collide with refilling the window from inside it.
        loop {
            let Some(joined) = running.join_next().await else {
                break;
            };
            match joined {
                Ok(entry) => results.push(entry),
                Err(err) => {
                    // A panicked task loses its oid. `unsafe_code = deny` plus
                    // the no-unwrap rule make this all but unreachable; it is
                    // recorded rather than swallowed so it cannot look like a
                    // silently skipped object.
                    tracing::error!(%err, "an LFS transfer task did not complete");
                    results.push((
                        String::new(),
                        Err(SyncError::Config(
                            "an LFS transfer task did not complete".to_owned(),
                        )),
                    ));
                }
            }
            spawn_next(&mut running);
        }
        results
    }

    /// Emit the terminal event for an object and pass the outcome through.
    fn finish(&self, oid: &str, outcome: Result<()>) -> Result<()> {
        match &outcome {
            Ok(()) => self.reporter.emit(TransferEvent::Completed {
                oid: oid.to_owned(),
            }),
            Err(err) => self.reporter.emit(TransferEvent::Failed {
                oid: oid.to_owned(),
                code: err.code(),
                error: err.to_string(),
            }),
        }
        outcome
    }

    async fn download_inner(
        &self,
        spec: &ObjectSpec,
        action: &Action,
        auth: Option<&str>,
        host: &str,
    ) -> Result<()> {
        let mut credential = auth.map(str::to_owned);
        let mut reauth_spent = false;
        let mut wait_spent = false;
        let mut discard_prefix = false;

        for _ in 0..DEFAULT_MAX_RETRIES {
            if discard_prefix {
                self.store.remove_partial(&spec.oid)?;
                discard_prefix = false;
            }

            let (staged, primed) = self.staged_prefix(&spec.oid).await?;
            let decision = resume_decision(staged, spec.size);
            let (start, hasher) = match decision {
                RangeDecision::Resume { from, .. } => (from, primed),
                RangeDecision::Whole | RangeDecision::Restart => {
                    self.store.remove_partial(&spec.oid)?;
                    (0, Sha256::new())
                }
            };

            let mut request = self.client.get(&action.href).headers(self.transfer_headers(
                spec,
                action,
                credential.as_deref(),
                host,
                reauth_spent,
            )?);
            if let RangeDecision::Resume { from, last } = decision {
                // Explicit end, never open-ended, exactly as upstream: we know
                // the length from the batch response, so we may as well pin it.
                request = request.header(RANGE, header_value(&format!("bytes={from}-{last}"))?);
            }

            let response = request
                .send()
                .await
                .map_err(|err| network(host, transport_reason(&err)))?;

            let status = response.status().as_u16();
            let retry_after = header_string(response.headers(), "retry-after");
            let challenge = auth_challenge(response.headers());
            let content_range = header_string(response.headers(), "content-range");

            if response.status().is_success() {
                // A 2xx body is the object. It is streamed, never read into a
                // string, which is why success is handled before the shared
                // classifier (that one wants an error body).
                if status == 206 {
                    let advertised = content_range.as_deref().and_then(parse_content_range_start);
                    // Validate the START byte and nothing else. Forgejo emits
                    // `bytes {from}-{to}/{size-from}` — its complete-length is
                    // the remaining length, not the object size, so a client
                    // that checks the total rejects every Forgejo resume.
                    //
                    // A matching start of 0 is accepted rather than rejected:
                    // a server that answers an unranged GET with 206 is being
                    // odd, but its body is still the whole object, and looping
                    // instead would burn the entire retry budget on it.
                    if advertised != Some(start) {
                        tracing::debug!(
                            oid = %spec.oid,
                            start,
                            ?advertised,
                            "unusable 206 Content-Range; restarting the download from zero"
                        );
                        discard_prefix = true;
                        continue;
                    }
                    return self
                        .stream_into_part(response, spec, start, hasher, host)
                        .await;
                }
                // A 200 in reply to a `Range` request means the server ignored
                // it. Consume this body from byte 0 instead of paying another
                // round trip to ask again.
                if start > 0 {
                    self.store.remove_partial(&spec.oid)?;
                }
                return self
                    .stream_into_part(response, spec, 0, Sha256::new(), host)
                    .await;
            }

            // 413 needs the body to tell a full account from an oversized
            // request. Bounded, because a hostile server's "error" body is
            // still a body.
            let body = bounded_body(response, MAX_ERROR_BODY_BYTES).await;
            match classify_transfer_status(status, host, retry_after.as_deref(), &body) {
                TransferAction::Unsatisfiable => {
                    // 416: the staged prefix is past the end of the object.
                    discard_prefix = true;
                }
                TransferAction::Reauthenticate => {
                    if reauth_spent {
                        return Err(SyncError::Auth {
                            host: host.to_owned(),
                        });
                    }
                    reauth_spent = true;
                    credential = self.refreshed_credential(challenge.as_deref(), host)?;
                }
                TransferAction::RetryAfter(delay) => {
                    if wait_spent {
                        return Err(network(host, "rate limited by the LFS server"));
                    }
                    wait_spent = true;
                    tokio::time::sleep(delay).await;
                }
                TransferAction::Fail(err) => return Err(err),
                // Unreachable: both are 2xx, handled above.
                TransferAction::Whole | TransferAction::Partial => {
                    return Err(network(host, "inconsistent LFS download response"))
                }
            }
        }

        Err(network(
            host,
            "the LFS download did not complete within its retry budget",
        ))
    }

    /// Hash whatever is already staged for `oid`.
    ///
    /// On `spawn_blocking` because it reads and hashes the whole `.part` file —
    /// gigabytes of synchronous I/O that would otherwise stall a reactor thread
    /// for seconds.
    async fn staged_prefix(&self, oid: &str) -> Result<(u64, Sha256)> {
        let store = self.store.clone();
        let owned = oid.to_owned();
        let path = self.store.incomplete_path(oid);
        tokio::task::spawn_blocking(move || store.resume_offset(&owned))
            .await
            .map_err(|err| {
                SyncError::io(
                    "hash the staged LFS prefix",
                    path,
                    std::io::Error::other(err.to_string()),
                )
            })?
    }

    /// Stream a response body into the `.part` file, hashing as it goes.
    async fn stream_into_part(
        &self,
        mut response: reqwest::Response,
        spec: &ObjectSpec,
        start: u64,
        mut hasher: Sha256,
        host: &str,
    ) -> Result<()> {
        let part = self.store.incomplete_path(&spec.oid);
        let mut options = tokio::fs::OpenOptions::new();
        options.create(true);
        if start == 0 {
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        let file = options
            .open(&part)
            .await
            .map_err(|err| SyncError::io("open lfs partial", &part, err))?;
        let mut writer = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_BYTES, file);

        let mut written = start;
        let mut coalescer = self.reporter.coalescer();
        // `chunk()` rather than `bytes_stream()`: consuming a `Stream` needs
        // `futures_util::StreamExt`, which this crate deliberately does not
        // depend on. Both poll one body frame at a time; neither buffers.
        // The loop BREAKS with its outcome rather than returning through `?`,
        // because an interrupted transfer has something worth keeping: the
        // hasher, primed with every byte that did reach the disk. Returning
        // early drops it, and the next attempt then re-reads the whole partial
        // to rebuild what this process already had — fourteen seconds, on a
        // 970 MB partial, before it asks for a single new byte.
        let interrupted: Option<SyncError> = loop {
            let chunk = match response.chunk().await {
                Ok(chunk) => chunk,
                Err(err) => break Some(network(host, transport_reason(&err))),
            };
            let Some(chunk) = chunk else {
                break None;
            };
            let bytes: &[u8] = &chunk;
            hasher.update(bytes);
            if let Err(err) = writer.write_all(bytes).await {
                break Some(SyncError::io("write lfs partial", &part, err));
            }
            written += bytes.len() as u64;
            if coalescer.should_emit(Instant::now()) {
                self.reporter.emit(TransferEvent::Progress {
                    oid: spec.oid.clone(),
                    bytes_done: written,
                });
            }
        };

        writer
            .flush()
            .await
            .map_err(|err| SyncError::io("flush lfs partial", &part, err))?;
        drop(writer);

        if let Some(err) = interrupted {
            // Flushed first, deliberately: the remembered offset is only usable
            // while it equals the file's length, so state saved ahead of the
            // flush would be discarded by the very check that makes it safe.
            store::remember_prefix(&spec.oid, written, &hasher);
            return Err(err);
        }

        let digest = hex::encode(hasher.finalize());
        if digest != spec.oid || written != spec.size {
            // Hard failure, and the staged bytes go with it. Resuming from a
            // prefix we already know is poisoned would reproduce the same
            // mismatch on every subsequent attempt, forever.
            self.store.remove_partial(&spec.oid)?;
            store::forget_prefix(&spec.oid);
            return Err(SyncError::Integrity {
                subject: format!("lfs object {}", spec.oid),
                expected: format!("{}/{}B", spec.oid, spec.size),
                actual: format!("{digest}/{written}B"),
            });
        }

        // A rename; microseconds, so it stays on the reactor.
        store::forget_prefix(&spec.oid);
        self.store.commit_partial(&spec.oid)
    }

    async fn upload_inner(
        &self,
        spec: &ObjectSpec,
        action: &Action,
        source: &Path,
        auth: Option<&str>,
        host: &str,
    ) -> Result<()> {
        let mut credential = auth.map(str::to_owned);
        let mut reauth_spent = false;
        let mut wait_spent = false;

        for _ in 0..DEFAULT_MAX_RETRIES {
            // Uploads are not resumable — upstream's own adapter says so — and
            // every attempt therefore reopens the file and sends from zero.
            let file = tokio::fs::File::open(source)
                .await
                .map_err(|err| SyncError::io("open lfs upload source", source, err))?;
            let length = file
                .metadata()
                .await
                .map_err(|err| SyncError::io("stat lfs upload source", source, err))?
                .len();
            if length != spec.size {
                // An exact `Content-Length` is a promise about bytes we have
                // not sent yet. Discovering the shortfall mid-body would hang
                // the connection until the server gave up.
                return Err(SyncError::Integrity {
                    subject: format!("lfs upload source for {}", spec.oid),
                    expected: format!("{}B", spec.size),
                    actual: format!("{length}B"),
                });
            }

            let mut headers =
                self.transfer_headers(spec, action, credential.as_deref(), host, reauth_spent)?;
            // Default per the spec; an action header pinning a type wins.
            headers
                .entry(CONTENT_TYPE)
                .or_insert(HeaderValue::from_static("application/octet-stream"));
            // Belt and braces: `SizedFileBody::size_hint` already makes reqwest
            // emit this, but stating it here is what a reader looks for, and it
            // survives a future change of body type.
            headers.insert(CONTENT_LENGTH, HeaderValue::from(spec.size));

            // What the watchdog below reads. The body is inside `reqwest` by
            // the time anybody wants to ask it how far it has got.
            let moved = Arc::new(AtomicU64::new(0));
            let body = reqwest::Body::wrap(SizedFileBody::new(
                file,
                spec.size,
                spec.oid.clone(),
                Arc::clone(&self.reporter),
                Arc::clone(&moved),
            ));
            let sending = self
                .client
                .put(&action.href)
                .headers(headers)
                .body(body)
                .send();
            // The upload is guarded by PROGRESS, not by silence.
            //
            // While a body is being sent the server says nothing, because that
            // is correct — so a read timeout is a duration cap wearing a
            // silence costume, and an object that needs longer than the cap can
            // never finish however good the link is. Eight objects on a real
            // folder retried every 61 seconds for eighteen hours against a
            // 60-second read timeout, and the sizes never mattered.
            //
            // This asks the only question that distinguishes a slow upload from
            // a dead one: did any byte move in the last window?
            tokio::pin!(sending);
            let mut last_seen = 0u64;
            let response = loop {
                tokio::select! {
                    finished = &mut sending => {
                        break finished.map_err(|err| network(host, transport_reason(&err)))?;
                    }
                    () = tokio::time::sleep(UPLOAD_STALL_WINDOW) => {
                        let now = moved.load(Ordering::Relaxed);
                        if now == last_seen {
                            return Err(network(host, "the upload stopped moving"));
                        }
                        last_seen = now;
                    }
                }
            };

            let status = response.status().as_u16();
            let retry_after = header_string(response.headers(), "retry-after");
            let challenge = auth_challenge(response.headers());
            let body = if response.status().is_success() {
                // A successful PUT answers with nothing we need; do not read it.
                String::new()
            } else {
                bounded_body(response, MAX_ERROR_BODY_BYTES).await
            };

            match classify_transfer_status(status, host, retry_after.as_deref(), &body) {
                TransferAction::Whole | TransferAction::Partial => return Ok(()),
                TransferAction::Reauthenticate => {
                    // A 403 here is an expired token, not a permissions
                    // decision — Forgejo echoes the batch `Authorization` into
                    // the action headers and it can age out mid-transfer.
                    if reauth_spent {
                        return Err(SyncError::Auth {
                            host: host.to_owned(),
                        });
                    }
                    reauth_spent = true;
                    credential = self.refreshed_credential(challenge.as_deref(), host)?;
                }
                TransferAction::RetryAfter(delay) => {
                    if wait_spent {
                        return Err(network(host, "rate limited by the LFS server"));
                    }
                    wait_spent = true;
                    tokio::time::sleep(delay).await;
                }
                // 416 is meaningless for a PUT; treat it as the failure it is
                // rather than looping.
                TransferAction::Unsatisfiable => {
                    return Err(network(host, "the LFS server rejected the upload range"))
                }
                // 422 lands here and is never retried: `SyncError::Config` is
                // `Retriability::Permanent`.
                TransferAction::Fail(err) => return Err(err),
            }
        }

        Err(network(
            host,
            "the LFS upload did not complete within its retry budget",
        ))
    }

    /// `POST` the `verify` action, if the server asked for one.
    async fn verify(&self, spec: &ObjectSpec, auth: Option<&str>, host: &str) -> Result<()> {
        let Some(action) = spec.verify_action() else {
            return Ok(());
        };
        let payload = serde_json::to_vec(&ObjectId::new(spec.oid.clone(), spec.size))
            .map_err(|err| SyncError::Config(format!("cannot encode an LFS verify body: {err}")))?;

        // The LFS `Accept` header is mandatory here too. Forgejo force-injects
        // it into its own verify actions as a workaround for git-lfs#3662; a
        // client that omits it earns a 415.
        //
        // Our credential is attached unconditionally, unlike on the data
        // transfer: `authenticated` describes the *content* href, and under
        // `SERVE_DIRECT` that one points at S3 while `verify` still points back
        // at the LFS API, which does want authenticating. An `Authorization`
        // supplied in the action header still overrides it below.
        let mut headers = json_headers(auth, host)?;
        apply_action_headers(&mut headers, action);

        let response = self
            .client
            .post(&action.href)
            .headers(headers)
            .body(payload)
            .send()
            .await
            .map_err(|err| network(host, transport_reason(&err)))?;

        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status().as_u16();
        let retry_after = header_string(response.headers(), "retry-after");
        let body = bounded_body(response, MAX_ERROR_BODY_BYTES).await;
        match classify_transfer_status(status, host, retry_after.as_deref(), &body) {
            TransferAction::Fail(err) => Err(err),
            _ => Err(network(
                host,
                "the LFS server did not confirm the uploaded object",
            )),
        }
    }

    /// Headers for a data transfer, honouring the pre-signing rule.
    ///
    /// `fresh_credential` says whether `auth` was just re-issued after a
    /// rejection. It has to exist: Forgejo echoes the batch request's
    /// `Authorization` into every action header, so after a 403 the stale token
    /// is still sitting in `action.header` and would win the retry, producing a
    /// second 403 and a pointless round trip.
    fn transfer_headers(
        &self,
        spec: &ObjectSpec,
        action: &Action,
        auth: Option<&str>,
        host: &str,
        fresh_credential: bool,
    ) -> Result<HeaderMap> {
        let mut headers = HeaderMap::with_capacity(3 + action.header.len());
        headers.insert(USER_AGENT, HeaderValue::from_static(crate::AGENT));
        apply_action_headers(&mut headers, action);

        // `authenticated: true` means the href is pre-signed. Re-attaching a
        // credential makes S3 answer **400** — this is Forgejo's `SERVE_DIRECT`
        // path with MinIO/S3 behind it, and it holds even for a fresh token.
        if spec.authenticated {
            return Ok(headers);
        }
        // Otherwise the server's own action header is authoritative — Forgejo
        // puts the working credential there deliberately — until we have proof
        // it expired.
        let Some(auth) = auth else {
            return Ok(headers);
        };
        if fresh_credential || !headers.contains_key(reqwest::header::AUTHORIZATION) {
            headers.insert(reqwest::header::AUTHORIZATION, sensitive_auth(auth, host)?);
        }
        Ok(headers)
    }

    fn refreshed_credential(&self, challenge: Option<&str>, host: &str) -> Result<Option<String>> {
        match self.refresh.as_ref().and_then(|refresh| refresh(challenge)) {
            Some(fresh) => Ok(Some(fresh)),
            // No refresher, or it had nothing: a second attempt with the same
            // credential would only earn a second rejection, and retry storms
            // against a git host get accounts locked.
            None => Err(SyncError::Auth {
                host: host.to_owned(),
            }),
        }
    }
}

/// Copy an action's headers onto a request, skipping any the server malformed.
fn apply_action_headers(headers: &mut HeaderMap, action: &Action) {
    for (name, value) in &action.header {
        let (Ok(parsed_name), Ok(mut parsed_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) else {
            // A server that emits an unrepresentable header should not cost us
            // the transfer; the value is never logged, only the name.
            tracing::debug!(header = %name, "skipping a malformed LFS action header");
            continue;
        };
        if parsed_name == reqwest::header::AUTHORIZATION {
            parsed_value.set_sensitive(true);
        }
        headers.insert(parsed_name, parsed_value);
    }
}

/// What a data-transfer response status tells the client to do.
#[derive(Debug)]
pub enum TransferAction {
    /// 2xx other than 206. For a download this is a complete body from byte 0
    /// (the server ignored our `Range`); for an upload it is success.
    Whole,
    /// 206 Partial Content — the `Content-Range` start must still be checked.
    Partial,
    /// 416 — the staged prefix is unusable; truncate and re-request whole.
    Unsatisfiable,
    /// 401/403 — refresh the credential and retry exactly once.
    Reauthenticate,
    /// 429 — wait, then retry.
    RetryAfter(Duration),
    /// Surface it; [`SyncError::retriability`] decides what happens next.
    Fail(SyncError),
}

/// Map a data-transfer response status onto a client action.
///
/// Pure, so the whole table is testable without a server.
pub fn classify_transfer_status(
    status: u16,
    host: &str,
    retry_after: Option<&str>,
    body: &str,
) -> TransferAction {
    match status {
        206 => TransferAction::Partial,
        200..=299 => TransferAction::Whole,
        // Both mean "this credential will not do". On a `PUT` a 403 is an
        // expired token rather than a policy decision, which is why it is worth
        // exactly one retry instead of being terminal.
        401 | 403 => TransferAction::Reauthenticate,
        404 | 410 => TransferAction::Fail(SyncError::Config(format!(
            "{host} no longer serves this LFS object"
        ))),
        // Capacity refusals. Forgejo returns 413 for a per-owner quota and for
        // an oversized object alike; both need a human, and `Quota` is the
        // variant that raises an actionable notice.
        413 | 507 | 509 => TransferAction::Fail(SyncError::Quota {
            host: host.to_owned(),
        }),
        416 => TransferAction::Unsatisfiable,
        // Never retried. Forgejo answers 422 when the PUT body fails its
        // proof-of-possession hash or size check, and re-sending the same bytes
        // cannot change that. `Config` is `Retriability::Permanent`.
        422 => TransferAction::Fail(SyncError::Config(format!(
            "{host} rejected the object content as invalid"
        ))),
        429 => TransferAction::RetryAfter(crate::lfs::batch::retry_after_delay(retry_after)),
        500..=599 => TransferAction::Fail(SyncError::Network {
            host: host.to_owned(),
            reason: if body.to_ascii_lowercase().contains("quota exceeded") {
                "the remote is out of storage".to_owned()
            } else {
                format!("server error {status}")
            },
        }),
        other => TransferAction::Fail(SyncError::Config(format!(
            "{host} answered an LFS transfer with an unexpected status {other}"
        ))),
    }
}

/// What to ask for, given what is already staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeDecision {
    /// Nothing staged: request the whole object.
    Whole,
    /// Discard the staged prefix, then request the whole object.
    Restart,
    /// Ask for `bytes=<from>-<last>`.
    Resume { from: u64, last: u64 },
}

/// Decide whether a staged prefix is worth resuming from.
pub fn resume_decision(part_len: u64, size: u64) -> RangeDecision {
    if part_len == 0 {
        return RangeDecision::Whole;
    }
    if size == 0 {
        // Bytes staged for an object that has none. Nothing to resume into.
        return RangeDecision::Restart;
    }
    // `from >= size - 1` would produce an empty or backwards range. Upstream
    // truncates and restarts rather than emit one, and so do we.
    if part_len >= size - 1 {
        return RangeDecision::Restart;
    }
    // Forgejo parses `Range` offsets with a 32-bit `ParseInt`; see
    // `RESUME_OFFSET_CEILING`.
    if part_len >= RESUME_OFFSET_CEILING {
        return RangeDecision::Restart;
    }
    RangeDecision::Resume {
        from: part_len,
        last: size - 1,
    }
}

/// Extract the **start** byte of a `Content-Range`.
///
/// Only the start, deliberately. Forgejo's `services/lfs/server.go:104` emits
/// `bytes {from}-{to}/{size-from}` — the complete-length is the *remaining*
/// length, not the object size. Upstream git-lfs only ever matched the start
/// (`bytes (\d+)-.*`) so it never noticed, and a client that validates the
/// total against the batch `size` rejects every single Forgejo resume.
pub fn parse_content_range_start(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?;
    let rest = rest.trim_start();
    // `bytes 0-99/100` is the response form; tolerate the `bytes=` request
    // spelling too, since some proxies echo it back.
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    let (start, _) = rest.split_once('-')?;
    start.trim().parse::<u64>().ok()
}

/// A file body that advertises an **exact** length.
///
/// This is the one genuinely non-obvious piece of the upload path, and the
/// reason `http-body` is a dependency of this crate at all.
/// [`reqwest::Body::wrap_stream`] and `Body::from(File)` both produce an
/// unknown-size body, so hyper falls back to `Transfer-Encoding: chunked`.
/// Forgejo tolerates chunked, but **pre-signed S3 `PUT`s reject it**, as do
/// many reverse proxies. Implementing [`http_body::Body`] with a `size_hint`
/// of [`http_body::SizeHint::with_exact`] makes reqwest set `CONTENT_LENGTH`
/// from `body.content_length()`, and both `Body::wrap`'s `IntoBytesBody` and
/// its `BoxBody` delegate `size_hint`, so the exact length survives the boxing.
///
/// Reads go through `tokio::fs`, so a multi-gigabyte upload never blocks the
/// reactor inside `poll_frame`.
/// How long an upload may make no progress at all before it is a failure.
///
/// The guard on an upload is that BYTES ARE MOVING, not that the socket is
/// talking: while the client sends a body the server is silent because that is
/// correct, so silence says nothing. A read timeout asked the wrong question and
/// answered it every sixty seconds — see `http::TRANSFER_READ_TIMEOUT`.
///
/// Ninety seconds rather than something tighter because a file on a slow or
/// sleeping external disk can genuinely take a while to yield its next chunk,
/// and turning that into a failure would trade a rare hang for a retry storm on
/// exactly the folders this matters for.
pub const UPLOAD_STALL_WINDOW: Duration = Duration::from_secs(90);

struct SizedFileBody {
    file: tokio::fs::File,
    /// Bytes still to send. Also the `size_hint`, which is what the trait's
    /// contract asks for and what makes the `Content-Length` correct.
    remaining: u64,
    buf: bytes::BytesMut,
    sent: u64,
    /// The same figure the watchdog reads. Shared rather than returned, because
    /// the body is inside `reqwest` by the time anybody wants to ask.
    moved: Arc<AtomicU64>,
    oid: String,
    reporter: Arc<Reporter>,
    coalescer: ProgressCoalescer,
}

impl SizedFileBody {
    fn new(
        file: tokio::fs::File,
        size: u64,
        oid: String,
        reporter: Arc<Reporter>,
        moved: Arc<AtomicU64>,
    ) -> Self {
        let coalescer = reporter.coalescer();
        Self {
            file,
            remaining: size,
            buf: bytes::BytesMut::with_capacity(CHUNK_BYTES),
            sent: 0,
            moved,
            oid,
            reporter,
            coalescer,
        }
    }
}

impl http_body::Body for SizedFileBody {
    type Data = bytes::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.remaining == 0 {
            return Poll::Ready(None);
        }

        // Never read more than is left: over-reading would send bytes the
        // `Content-Length` did not promise.
        let want = if this.remaining < CHUNK_BYTES as u64 {
            this.remaining as usize
        } else {
            CHUNK_BYTES
        };
        this.buf.clear();
        this.buf.resize(want, 0);

        let mut read_buf = tokio::io::ReadBuf::new(&mut this.buf[..]);
        match Pin::new(&mut this.file).poll_read(cx, &mut read_buf) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
            Poll::Ready(Ok(())) => {}
        }
        let filled = read_buf.filled().len();
        if filled == 0 {
            // The source shrank while we were sending it. Under an exact
            // `Content-Length` a short body just stalls the connection until
            // the server times out, so fail loudly instead.
            return Poll::Ready(Some(Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "LFS upload source shrank while it was being sent",
            ))));
        }

        this.remaining -= filled as u64;
        this.sent += filled as u64;
        // Every chunk, not every progress event: the coalescer exists to keep
        // the UI quiet and the watchdog needs the truth.
        this.moved.store(this.sent, Ordering::Relaxed);
        if this.coalescer.should_emit(Instant::now()) {
            this.reporter.emit(TransferEvent::Progress {
                oid: this.oid.clone(),
                bytes_done: this.sent,
            });
        }
        // `split_to` hands out an owned view; no copy of the payload.
        let chunk = this.buf.split_to(filled).freeze();
        Poll::Ready(Some(Ok(http_body::Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> http_body::SizeHint {
        // The whole reason this type exists.
        http_body::SizeHint::with_exact(self.remaining)
    }
}

fn network(host: &str, reason: &str) -> SyncError {
    SyncError::Network {
        host: host.to_owned(),
        reason: reason.to_owned(),
    }
}

fn missing_action(oid: &str, host: &str) -> SyncError {
    SyncError::Config(format!(
        "{host} offered no transfer action for LFS object {oid}"
    ))
}

/// Host of an action href, for error messages.
///
/// Only the host: a pre-signed href carries its credential in the query string
/// and must never reach a log line (AD-53).
fn host_of(href: Option<&str>) -> String {
    href.and_then(|href| Url::parse(href).ok())
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown host".to_owned())
}

fn header_value(raw: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(raw)
        .map_err(|_| SyncError::Config(format!("cannot build the `{raw}` header value")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lfs::batch::Actions;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const OID_A: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    #[test]
    fn content_range_start_is_read_even_when_the_total_is_wrong() {
        // Forgejo's actual output for a resume of a 1000-byte object from 400:
        // the complete-length is `size - from`, which is simply wrong. Reading
        // only the start byte is what makes resume work against it.
        assert_eq!(parse_content_range_start("bytes 400-999/600"), Some(400));
        assert_eq!(parse_content_range_start("bytes 0-999/1000"), Some(0));
        // Tolerated spellings.
        assert_eq!(parse_content_range_start("bytes=400-999/1000"), Some(400));
        assert_eq!(parse_content_range_start("  bytes 12-34/56  "), Some(12));

        // Unusable forms.
        assert_eq!(parse_content_range_start("bytes */1000"), None);
        assert_eq!(parse_content_range_start("items 0-9/10"), None);
        assert_eq!(parse_content_range_start(""), None);
        assert_eq!(parse_content_range_start("bytes"), None);
        assert_eq!(parse_content_range_start("bytes abc-def/10"), None);
    }

    #[test]
    fn resume_decision_covers_every_boundary() {
        // Nothing staged.
        assert_eq!(resume_decision(0, 100), RangeDecision::Whole);
        assert_eq!(resume_decision(0, 0), RangeDecision::Whole);

        // The ordinary resume: explicit end, never open-ended.
        assert_eq!(
            resume_decision(50, 100),
            RangeDecision::Resume { from: 50, last: 99 }
        );
        assert_eq!(
            resume_decision(1, 100),
            RangeDecision::Resume { from: 1, last: 99 }
        );

        // `from >= size - 1` would be an empty or backwards range.
        assert_eq!(
            resume_decision(98, 100),
            RangeDecision::Resume { from: 98, last: 99 }
        );
        assert_eq!(resume_decision(99, 100), RangeDecision::Restart);
        assert_eq!(resume_decision(100, 100), RangeDecision::Restart);
        // An over-long leftover is junk, not a resume point.
        assert_eq!(resume_decision(150, 100), RangeDecision::Restart);
        // Staged bytes for an empty object.
        assert_eq!(resume_decision(5, 0), RangeDecision::Restart);

        // The 2 GiB ceiling: Forgejo parses Range offsets with 32 bits.
        let huge = 8 * 1024 * 1024 * 1024u64;
        assert_eq!(
            resume_decision(RESUME_OFFSET_CEILING - 1, huge),
            RangeDecision::Resume {
                from: RESUME_OFFSET_CEILING - 1,
                last: huge - 1
            }
        );
        assert_eq!(
            resume_decision(RESUME_OFFSET_CEILING, huge),
            RangeDecision::Restart
        );
        assert_eq!(
            resume_decision(RESUME_OFFSET_CEILING + 1, huge),
            RangeDecision::Restart
        );
    }

    #[test]
    fn transfer_statuses_map_onto_the_documented_actions() {
        let host = "host.example";
        let classify = |status| classify_transfer_status(status, host, None, "");

        assert!(matches!(classify(206), TransferAction::Partial));
        // A 200 answering a Range request means the server ignored it.
        assert!(matches!(classify(200), TransferAction::Whole));
        assert!(matches!(classify(201), TransferAction::Whole));
        assert!(matches!(classify(416), TransferAction::Unsatisfiable));
        // 403 on a PUT is an expired token, worth exactly one retry.
        assert!(matches!(classify(401), TransferAction::Reauthenticate));
        assert!(matches!(classify(403), TransferAction::Reauthenticate));

        for (status, code) in [
            (404u16, "config"),
            (410, "config"),
            (413, "quota"),
            (507, "quota"),
            (509, "quota"),
            (500, "network"),
            (503, "network"),
            (418, "config"),
        ] {
            match classify(status) {
                TransferAction::Fail(err) => assert_eq!(err.code(), code, "status {status}"),
                other => panic!("status {status} produced {other:?}"),
            }
        }
    }

    #[test]
    fn a_422_is_permanent_so_the_same_bytes_are_never_resent() {
        match classify_transfer_status(422, "host.example", None, "") {
            TransferAction::Fail(err) => {
                assert_eq!(
                    err.retriability(),
                    crate::error::Retriability::Permanent,
                    "422 must never be retried"
                );
            }
            other => panic!("422 produced {other:?}"),
        }
    }

    #[test]
    fn a_429_honours_retry_after() {
        match classify_transfer_status(429, "host.example", Some("30"), "") {
            TransferAction::RetryAfter(delay) => assert_eq!(delay, Duration::from_secs(30)),
            other => panic!("429 produced {other:?}"),
        }
    }

    #[test]
    fn the_coalescer_emits_at_most_once_per_interval() {
        let interval = Duration::from_millis(100);
        let mut coalescer = ProgressCoalescer::new(interval);
        let start = Instant::now();

        // The first sample always fires, so a transfer is never blank.
        assert!(coalescer.should_emit(start));
        assert!(!coalescer.should_emit(start + Duration::from_millis(1)));
        assert!(!coalescer.should_emit(start + Duration::from_millis(99)));
        assert!(coalescer.should_emit(start + Duration::from_millis(100)));
        assert!(!coalescer.should_emit(start + Duration::from_millis(150)));
        assert!(coalescer.should_emit(start + Duration::from_millis(201)));

        // The contract that matters: over a 1 s run sampled every millisecond —
        // which is far slower than a real chunk loop — at most 11 events escape.
        let mut coalescer = ProgressCoalescer::new(interval);
        let emitted = (0..=1_000)
            .filter(|ms| coalescer.should_emit(start + Duration::from_millis(*ms)))
            .count();
        assert_eq!(emitted, 11, "one immediate event plus one per 100 ms");
    }

    #[test]
    fn a_pre_signed_href_never_gets_a_credential_re_attached() {
        // Forgejo's `SERVE_DIRECT` path: S3 answers 400 if we add one.
        let transfer = BasicTransfer::new(reqwest::Client::new(), store());
        let action = Action {
            href: "https://s3.example/bucket/obj?X-Amz-Signature=abc".to_owned(),
            ..Action::default()
        };
        let spec = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 10,
            authenticated: true,
            ..ObjectSpec::default()
        };

        let headers = transfer
            .transfer_headers(
                &spec,
                &action,
                Some("Basic dXNlcjpwYXNz"),
                "s3.example",
                false,
            )
            .expect("headers");
        assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());
        assert_eq!(
            headers.get(USER_AGENT).and_then(|v| v.to_str().ok()),
            Some(crate::AGENT)
        );
    }

    #[test]
    fn an_unauthenticated_href_gets_our_credential_but_the_action_header_wins() {
        let transfer = BasicTransfer::new(reqwest::Client::new(), store());
        let spec = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 10,
            ..ObjectSpec::default()
        };

        // No action header: our own credential is attached.
        let plain = Action {
            href: "https://host.example/o".to_owned(),
            ..Action::default()
        };
        let headers = transfer
            .transfer_headers(&spec, &plain, Some("Basic ours"), "host.example", false)
            .expect("headers");
        assert_eq!(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Basic ours")
        );

        // Forgejo echoes the batch credential into the action headers; that one
        // is authoritative for the data transfer.
        let echoed = Action {
            href: "https://host.example/o".to_owned(),
            header: BTreeMap::from([(
                "Authorization".to_owned(),
                "Bearer server-issued".to_owned(),
            )]),
            ..Action::default()
        };
        let headers = transfer
            .transfer_headers(&spec, &echoed, Some("Basic ours"), "host.example", false)
            .expect("headers");
        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .expect("action header applied");
        assert_eq!(auth.to_str().ok(), Some("Bearer server-issued"));
        // And it is marked sensitive so the `http` stack redacts it.
        assert!(auth.is_sensitive());
    }

    #[test]
    fn a_refreshed_credential_displaces_the_stale_echoed_one() {
        // The retry after a 403 must not resend the token that just expired.
        // Forgejo copies the batch `Authorization` into every action header, so
        // without this the stale value would win and the retry would be wasted.
        let transfer = BasicTransfer::new(reqwest::Client::new(), store());
        let spec = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 10,
            ..ObjectSpec::default()
        };
        let echoed = Action {
            href: "https://host.example/o".to_owned(),
            header: BTreeMap::from([("Authorization".to_owned(), "Bearer expired".to_owned())]),
            ..Action::default()
        };

        let headers = transfer
            .transfer_headers(&spec, &echoed, Some("Bearer minted"), "host.example", true)
            .expect("headers");
        assert_eq!(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer minted")
        );

        // A pre-signed href still gets nothing, fresh credential or not.
        let presigned = ObjectSpec {
            authenticated: true,
            ..spec
        };
        let headers = transfer
            .transfer_headers(
                &presigned,
                &echoed,
                Some("Bearer minted"),
                "host.example",
                true,
            )
            .expect("headers");
        assert_eq!(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer expired"),
            "the server's own action header is still applied verbatim"
        );
    }

    #[test]
    fn malformed_action_headers_are_skipped_rather_than_failing_the_transfer() {
        let mut headers = HeaderMap::new();
        let action = Action {
            href: "https://host.example/o".to_owned(),
            header: BTreeMap::from([
                ("X-Good".to_owned(), "fine".to_owned()),
                ("Bad Name".to_owned(), "value".to_owned()),
                ("X-Bad-Value".to_owned(), "line\nbreak".to_owned()),
            ]),
            ..Action::default()
        };
        apply_action_headers(&mut headers, &action);

        assert_eq!(
            headers.get("x-good").and_then(|v| v.to_str().ok()),
            Some("fine")
        );
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn the_sink_sees_started_then_completed_and_stops_when_it_detaches() {
        let seen: Arc<Mutex<Vec<TransferEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let sink: TransferSink = Box::new(move |event| {
            let mut log = recorder.lock().expect("sink lock");
            log.push(event);
            // Detach after the second event.
            log.len() < 2
        });
        let reporter = Reporter {
            sink: Some(Arc::new(sink)),
            detached: AtomicBool::new(false),
            interval: DEFAULT_PROGRESS_INTERVAL,
        };

        reporter.emit(TransferEvent::Started {
            oid: OID_A.to_owned(),
            size: 3,
        });
        reporter.emit(TransferEvent::Progress {
            oid: OID_A.to_owned(),
            bytes_done: 1,
        });
        // The sink said `false`; nothing more reaches it.
        reporter.emit(TransferEvent::Completed {
            oid: OID_A.to_owned(),
        });

        let log = seen.lock().expect("sink lock");
        assert_eq!(log.len(), 2, "{log:?}");
        assert!(matches!(log[0], TransferEvent::Started { size: 3, .. }));
        assert!(matches!(
            log[1],
            TransferEvent::Progress { bytes_done: 1, .. }
        ));
    }

    #[tokio::test]
    async fn an_upload_body_advertises_an_exact_length_and_streams_the_file() {
        use http_body::Body as _;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("object.bin");
        // Two full chunks plus a remainder, so the framing loop is exercised.
        let content = vec![7u8; CHUNK_BYTES * 2 + 13];
        std::fs::write(&path, &content).expect("write source");

        let file = tokio::fs::File::open(&path).await.expect("open");
        let mut body = SizedFileBody::new(
            file,
            content.len() as u64,
            OID_A.to_owned(),
            Arc::new(Reporter {
                sink: None,
                detached: AtomicBool::new(false),
                interval: DEFAULT_PROGRESS_INTERVAL,
            }),
            Arc::new(AtomicU64::new(0)),
        );

        // This is what reqwest reads to set `Content-Length`; without it hyper
        // falls back to chunked and pre-signed S3 PUTs break.
        assert_eq!(body.size_hint().exact(), Some(content.len() as u64));
        assert!(!body.is_end_stream());

        let mut streamed = Vec::new();
        let mut frames = 0usize;
        while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await
        {
            let frame = frame.expect("frame");
            let data = frame.into_data().unwrap_or_default();
            // Nothing larger than one chunk is ever resident.
            assert!(data.len() <= CHUNK_BYTES, "{} bytes", data.len());
            streamed.extend_from_slice(&data);
            frames += 1;
        }

        assert_eq!(streamed, content, "the body must send the file verbatim");
        // `tokio::fs` may short-read, so the exact frame count is not a
        // contract; that it framed at all — rather than materializing the whole
        // file — is.
        assert!(frames >= 3, "{frames} frames for {} bytes", content.len());
        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
    }

    #[tokio::test]
    async fn an_upload_body_reports_a_shrinking_source_instead_of_stalling() {
        use http_body::Body as _;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("short.bin");
        std::fs::write(&path, b"only four").expect("write source");

        let file = tokio::fs::File::open(&path).await.expect("open");
        // Claim more than the file holds, as a truncation mid-flight would.
        let mut body = SizedFileBody::new(
            file,
            1_000,
            OID_A.to_owned(),
            Arc::new(Reporter {
                sink: None,
                detached: AtomicBool::new(false),
                interval: DEFAULT_PROGRESS_INTERVAL,
            }),
            Arc::new(AtomicU64::new(0)),
        );

        let mut last = None;
        while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await
        {
            if frame.is_err() {
                last = Some(frame);
                break;
            }
        }
        let Some(Err(err)) = last else {
            panic!("a source shorter than its declared size must raise an error");
        };
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn an_object_already_in_the_store_is_not_re_downloaded() {
        let store = store();
        let content = b"already here";
        let oid = hex::encode(Sha256::digest(content));
        store
            .insert_verified(&oid, &content[..], content.len() as u64)
            .expect("seed the store");

        // The href is unreachable on purpose: reaching the network at all would
        // fail this test.
        let transfer = BasicTransfer::new(reqwest::Client::new(), store);
        let spec = ObjectSpec {
            oid: oid.clone(),
            size: content.len() as u64,
            actions: Some(Actions {
                download: Some(Action {
                    href: "https://127.0.0.1:1/never".to_owned(),
                    ..Action::default()
                }),
                ..Actions::default()
            }),
            ..ObjectSpec::default()
        };

        transfer
            .download(&spec, None)
            .await
            .expect("no-op download");
    }

    #[tokio::test]
    async fn an_upload_with_no_action_is_a_skip_not_a_failure() {
        // Neither `actions` nor `error` on an upload batch: the server already
        // holds the content.
        let transfer = BasicTransfer::new(reqwest::Client::new(), store());
        let spec = ObjectSpec {
            oid: OID_A.to_owned(),
            size: 4,
            ..ObjectSpec::default()
        };

        transfer
            .upload(&spec, Path::new("/definitely/not/here"), None)
            .await
            .expect("an already-present object is a success");
    }

    /// A store rooted in a leaked temp dir.
    ///
    /// Leaking is deliberate: these tests never write more than a few bytes,
    /// and keeping a `TempDir` guard alive through every helper would clutter
    /// each case for no protection the OS does not already give.
    fn store() -> LfsStore {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = LfsStore::in_git_dir(dir.keep());
        store.ensure_layout().expect("layout");
        store
    }
}
