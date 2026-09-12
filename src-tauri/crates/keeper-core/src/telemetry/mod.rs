//! Optional, local-consent observability. No app-log subscriber, account state,
//! arbitrary attributes, error messages or recording operations enter this module.
mod wire;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Notify, Semaphore};
use ts_rs::TS;

use crate::vm::{EgressEndpointVm, EgressKind};

const QUEUE_LIMIT: usize = 128;
const EVENTS_PER_MINUTE: usize = 60;
const RESPONSE_LIMIT: usize = 16 * 1024;
const REQUEST_LIMIT: usize = 32 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_DURATION_MS: f64 = 600_000.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryConsentVm {
    pub diagnostics: bool,
    pub product_analytics: bool,
    pub remote_config: bool,
}

impl TelemetryConsentVm {
    fn any(self) -> bool {
        self.diagnostics || self.product_analytics || self.remote_config
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryStatusVm {
    pub consent: TelemetryConsentVm,
    pub configured: bool,
    pub host: Option<String>,
    pub installation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TelemetryEventKind {
    AppReady,
    CommandPaletteOpened,
    SettingsOpened,
    FrontendError,
    Interaction,
}

impl TelemetryEventKind {
    fn diagnostic(self) -> bool {
        matches!(
            self,
            Self::AppReady | Self::FrontendError | Self::Interaction
        )
    }

    fn name(self) -> &'static str {
        match self {
            Self::AppReady => "keeper_app_ready",
            Self::CommandPaletteOpened => "keeper_command_palette_opened",
            Self::SettingsOpened => "keeper_settings_opened",
            Self::FrontendError => "keeper_frontend_error",
            Self::Interaction => "keeper_interaction",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryEventReq {
    pub kind: TelemetryEventKind,
    pub duration_ms: Option<f64>,
}

impl TelemetryEventReq {
    fn validate(&self) -> Result<(), TelemetryError> {
        if self
            .duration_ms
            .is_some_and(|ms| !ms.is_finite() || !(0.0..=MAX_DURATION_MS).contains(&ms))
        {
            return Err(TelemetryError::InvalidEvent);
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryRemoteConfigVm {
    pub support_message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryStudyConfigVm {
    pub host: String,
    pub project_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryStudyPreviewVm {
    pub configured: bool,
    pub host: Option<String>,
}

/// Only a public ingestion token and a pinned HTTPS origin. Never Debug-print it.
#[derive(Clone)]
pub struct PublicConfig {
    host: String,
    token: String,
}

impl PublicConfig {
    pub fn parse(host: &str, token: &str) -> Option<Self> {
        let url = url::Url::parse(host).ok()?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || host.len() > 256
            || !token.starts_with("phc_")
            || !(12..=200).contains(&token.len())
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }
        Some(Self {
            host: url.origin().ascii_serialization(),
            token: token.to_owned(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("The observability event is outside the allowed schema.")]
    InvalidEvent,
    #[error("Observability consent could not be saved. Collection is off for this session; saved consent may remain. Retry before restarting.")]
    Persistence,
    #[error("Observability state is unavailable. Collection is off.")]
    Unavailable,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredConsent {
    version: u8,
    consent: TelemetryConsentVm,
    installation_id: Option<String>,
}

impl StoredConsent {
    fn load(path: &Path) -> Self {
        let read = || -> Option<Self> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .ok()?
                .take(2049)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > 2048 {
                return None;
            }
            let state: Self = serde_json::from_slice(&bytes).ok()?;
            let valid_id = state.installation_id.as_ref().is_some_and(|id| {
                id.len() == 32
                    && id.bytes().all(|b| b.is_ascii_hexdigit())
                    && id.bytes().any(|b| b != b'0')
            });
            if state.version != 1
                || (state.consent.any() && !valid_id)
                || (!state.consent.any() && state.installation_id.is_some())
            {
                return None;
            }
            Some(state)
        };
        read().unwrap_or_default()
    }

    fn persist(&self, path: &Path) -> Result<(), TelemetryError> {
        let persist = || -> std::io::Result<()> {
            let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
            std::fs::create_dir_all(parent)?;
            let temporary = parent.join(format!(".telemetry-{}.tmp", random_id::<16>()));
            let result = (|| {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&temporary)?;
                serde_json::to_writer(&mut file, self)?;
                file.flush()?;
                file.sync_all()?;
                std::fs::rename(&temporary, path)?;
                #[cfg(unix)]
                std::fs::File::open(parent)?.sync_all()?;
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
            result
        };
        persist().map_err(|_| TelemetryError::Persistence)
    }
}

fn random_id<const N: usize>() -> String {
    let mut bytes = [0u8; N];
    rand::thread_rng().fill_bytes(&mut bytes);
    // W3C IDs cannot be all zero; set one bit rather than retrying.
    bytes[0] |= 1;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut id = String::with_capacity(N * 2);
    for byte in bytes {
        id.push(char::from(HEX[usize::from(byte >> 4)]));
        id.push(char::from(HEX[usize::from(byte & 15)]));
    }
    id
}

fn unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Copy)]
enum Operation {
    Frontend(TelemetryEventKind),
    RemoteConfig,
    FlagsRequest,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Frontend(kind) => kind.name(),
            Self::RemoteConfig => "keeper.remote_config",
            Self::FlagsRequest => "keeper.flags_request",
        }
    }
}

/// No string-valued source attributes exist. IDs are created inside this module.
struct Record {
    operation: Operation,
    duration_ms: Option<f64>,
    start: u64,
    end: u64,
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    outcome: u32,
}

impl Record {
    fn frontend(event: TelemetryEventReq) -> Self {
        let end = unix_nanos();
        let elapsed = event.duration_ms.unwrap_or(0.0) * 1_000_000.0;
        Self {
            operation: Operation::Frontend(event.kind),
            duration_ms: event.duration_ms,
            start: end.saturating_sub(elapsed as u64),
            end,
            trace_id: random_id::<16>(),
            span_id: random_id::<8>(),
            parent_span_id: None,
            outcome: u32::from(event.kind == TelemetryEventKind::FrontendError),
        }
    }

    fn diagnostic(&self) -> bool {
        match self.operation {
            Operation::Frontend(kind) => kind.diagnostic(),
            _ => true,
        }
    }
}

struct Inner {
    stored: StoredConsent,
    epoch: u64,
    queue: VecDeque<Record>,
    admitted: VecDeque<Instant>,
    remote_last: Option<Instant>,
    remote: TelemetryRemoteConfigVm,
    study_active: bool,
}

/// One process-wide instance shared by every window. One queued record runs at
/// once with at most three collector requests (plus one concurrent flags read),
/// no retries or disk spool. Revocation drops futures; sent bytes cannot be recalled.
pub struct Telemetry {
    config: Option<PublicConfig>,
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
    epoch: watch::Sender<u64>,
    wake: Notify,
    remote_slot: Semaphore,
    http: LazyLock<Option<reqwest::Client>>,
}

impl Telemetry {
    pub fn open(data_dir: Option<&Path>, config: Option<PublicConfig>) -> Arc<Self> {
        let path = data_dir.map(|dir| dir.join("telemetry-consent-v1.json"));
        let stored = path.as_deref().map(StoredConsent::load).unwrap_or_default();
        let (epoch, _) = watch::channel(0);
        Arc::new(Self {
            config,
            path,
            inner: Mutex::new(Inner {
                stored,
                epoch: 0,
                queue: VecDeque::new(),
                admitted: VecDeque::new(),
                remote_last: None,
                remote: TelemetryRemoteConfigVm::default(),
                study_active: false,
            }),
            epoch,
            wake: Notify::new(),
            remote_slot: Semaphore::new(1),
            http: LazyLock::new(client),
        })
    }

    pub fn study_preview(&self) -> TelemetryStudyPreviewVm {
        TelemetryStudyPreviewVm {
            configured: self.config.is_some(),
            host: self.config.as_ref().map(|config| config.host.clone()),
        }
    }

    pub fn status(&self) -> Result<TelemetryStatusVm, TelemetryError> {
        let inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
        Ok(TelemetryStatusVm {
            consent: inner.stored.consent,
            configured: self.config.is_some(),
            host: self.config.as_ref().map(|config| config.host.clone()),
            installation_id: inner.stored.installation_id.clone(),
        })
    }

    /// Persist separately from the layered/synced settings registry. Any write
    /// failure immediately leaves this process off, including on attempted enable.
    pub fn set_consent(
        &self,
        consent: TelemetryConsentVm,
    ) -> Result<TelemetryStatusVm, TelemetryError> {
        let mut inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
        inner.epoch = inner.epoch.wrapping_add(1);
        self.epoch.send_replace(inner.epoch);
        inner.queue.clear();
        inner.remote = TelemetryRemoteConfigVm::default();
        inner.remote_last = None;
        let stored = StoredConsent {
            version: 1,
            consent,
            installation_id: if consent.any() {
                Some(
                    inner
                        .stored
                        .installation_id
                        .clone()
                        .unwrap_or_else(random_id::<16>),
                )
            } else {
                None
            },
        };
        inner.stored = StoredConsent::default();
        let path = self.path.as_deref().ok_or(TelemetryError::Persistence)?;
        if stored.persist(path).is_err() {
            // Do not leave previously enabled consent behind when atomic replacement
            // fails (e.g. permissions changed). A missing or truncated file is off.
            if std::fs::remove_file(path).is_err() {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(path);
            }
            return Err(TelemetryError::Persistence);
        }
        inner.stored = stored;
        drop(inner);
        self.wake.notify_one();
        self.status()
    }

    pub fn study_config(&self) -> Result<Option<TelemetryStudyConfigVm>, TelemetryError> {
        let mut inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
        inner.study_active = self.config.is_some();
        Ok(self.config.as_ref().map(|config| TelemetryStudyConfigVm {
            host: config.host.clone(),
            project_token: config.token.clone(),
        }))
    }

    pub fn study_stop(&self) -> Result<(), TelemetryError> {
        self.inner
            .lock()
            .map_err(|_| TelemetryError::Unavailable)?
            .study_active = false;
        Ok(())
    }

    pub fn egress(&self) -> Option<EgressEndpointVm> {
        let config = self.config.as_ref()?;
        let inner = self.inner.lock().ok()?;
        (inner.stored.consent.any() || inner.study_active).then(|| EgressEndpointVm {
            url: config.host.clone(),
            kind: EgressKind::Telemetry,
            label: "PostHog — enabled observability or synthetic study".to_owned(),
        })
    }

    pub fn capture(&self, event: TelemetryEventReq) -> Result<(), TelemetryError> {
        event.validate()?;
        if self.config.is_none() {
            return Ok(());
        }
        // Contended persistence/export is not allowed to stall an app operation.
        let Ok(mut inner) = self.inner.try_lock() else {
            return Ok(());
        };
        let consent = inner.stored.consent;
        let allowed = if event.kind.diagnostic() {
            consent.diagnostics
        } else {
            consent.product_analytics
        };
        if !allowed || !admit(&mut inner) {
            return Ok(());
        }
        inner.queue.push_back(Record::frontend(event));
        drop(inner);
        self.wake.notify_one();
        Ok(())
    }

    /// Run once on the shell runtime. No HTTP client, task per event or flag
    /// request is initialized at boot. Empty/off installs only await this notify.
    pub async fn run(self: Arc<Self>) {
        loop {
            self.wake.notified().await;
            loop {
                let mut cancellation = self.epoch.subscribe();
                let next = self.inner.lock().ok().and_then(|mut inner| {
                    let record = inner.queue.pop_front()?;
                    let id = inner.stored.installation_id.clone()?;
                    Some((record, id, inner.epoch))
                });
                let Some((record, id, epoch)) = next else {
                    break;
                };
                if *cancellation.borrow_and_update() != epoch {
                    continue;
                }
                // A stalled/offline exporter cannot send stale interaction data
                // minutes later when connectivity returns.
                if unix_nanos().saturating_sub(record.end) > 60_000_000_000 {
                    continue;
                }
                let Some(config) = &self.config else {
                    continue;
                };
                tokio::select! {
                    biased;
                    _ = cancellation.changed() => {},
                    _ = tokio::time::timeout(REQUEST_TIMEOUT, export(self.http.as_ref(), config, &id, &record)) => {},
                }
            }
        }
    }

    pub async fn remote_config(&self) -> TelemetryRemoteConfigVm {
        let Some(config) = &self.config else {
            return TelemetryRemoteConfigVm::default();
        };
        let Ok(_permit) = self.remote_slot.try_acquire() else {
            return self.remote_cached();
        };
        let mut cancellation = self.epoch.subscribe();
        let context = self.inner.lock().ok().and_then(|mut inner| {
            if !inner.stored.consent.remote_config {
                return None;
            }
            if inner
                .remote_last
                .is_some_and(|last| last.elapsed() < REMOTE_INTERVAL)
            {
                return None;
            }
            inner.remote_last = Some(Instant::now());
            Some((
                inner.stored.installation_id.clone()?,
                inner.epoch,
                inner.stored.consent.diagnostics,
            ))
        });
        let Some((id, epoch, diagnostics)) = context else {
            return self.remote_cached();
        };
        if *cancellation.borrow_and_update() != epoch {
            return TelemetryRemoteConfigVm::default();
        }
        let start = unix_nanos();
        let result = tokio::select! {
            biased;
            _ = cancellation.changed() => return TelemetryRemoteConfigVm::default(),
            result = tokio::time::timeout(REQUEST_TIMEOUT, fetch_config(self.http.as_ref(), config, &id)) => result.ok().flatten(),
        };
        let end = unix_nanos();
        let Ok(mut inner) = self.inner.lock() else {
            return TelemetryRemoteConfigVm::default();
        };
        if inner.epoch != epoch || !inner.stored.consent.remote_config {
            return TelemetryRemoteConfigVm::default();
        }
        let outcome = u32::from(result.is_none());
        inner.remote = result.unwrap_or_default();
        // A real Rust operation with a child span and correlated records. Its
        // response text, endpoint, HTTP errors and installation ID aren't attrs.
        if diagnostics
            && inner.stored.consent.diagnostics
            && inner.queue.len() + 2 <= QUEUE_LIMIT
            && admit(&mut inner)
        {
            let trace_id = random_id::<16>();
            let root_id = random_id::<8>();
            for (operation, span_id, parent) in [
                (Operation::RemoteConfig, root_id.clone(), None),
                (Operation::FlagsRequest, random_id::<8>(), Some(root_id)),
            ] {
                inner.queue.push_back(Record {
                    operation,
                    span_id,
                    parent_span_id: parent,
                    trace_id: trace_id.clone(),
                    duration_ms: Some(end.saturating_sub(start) as f64 / 1_000_000.0),
                    start,
                    end,
                    outcome,
                });
            }
            self.wake.notify_one();
        }
        inner.remote.clone()
    }

    fn remote_cached(&self) -> TelemetryRemoteConfigVm {
        self.inner
            .lock()
            .ok()
            .filter(|inner| inner.stored.consent.remote_config)
            .map(|inner| inner.remote.clone())
            .unwrap_or_default()
    }
}

fn admit(inner: &mut Inner) -> bool {
    let now = Instant::now();
    while inner
        .admitted
        .front()
        .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60))
    {
        inner.admitted.pop_front();
    }
    if inner.queue.len() >= QUEUE_LIMIT || inner.admitted.len() >= EVENTS_PER_MINUTE {
        return false;
    }
    inner.admitted.push_back(now);
    true
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .https_only(true)
        .build()
        .ok()
}

async fn post(
    client: &reqwest::Client,
    config: &PublicConfig,
    path: &str,
    body: Vec<u8>,
    protobuf: bool,
) {
    if body.len() > REQUEST_LIMIT {
        return;
    }
    let mut request = client.post(format!("{}{path}", config.host));
    if protobuf {
        request = request.bearer_auth(&config.token);
    }
    // Never read an export response: errors/HTML bodies are neither captured nor
    // logged. No retry, redirect, proxy response body or remote code is consumed.
    let _ = request
        .header(
            "Content-Type",
            if protobuf {
                "application/x-protobuf"
            } else {
                "application/json"
            },
        )
        .body(body)
        .send()
        .await;
}

fn batch_payload(config: &PublicConfig, id: &str, record: &Record) -> Option<serde_json::Value> {
    let Operation::Frontend(kind) = record.operation else {
        return None;
    };
    let timestamp = chrono::DateTime::from_timestamp_nanos(i64::try_from(record.end).ok()?)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut properties = serde_json::json!({
        "distinct_id": id,
        "consent_category": if kind.diagnostic() { "diagnostics" } else { "productAnalytics" },
        "event_source": "frontend", "synthetic": false,
        "$process_person_profile": false, "$geoip_disable": true,
        "trace_id": record.trace_id, "span_id": record.span_id
    });
    if let Some(duration) = record.duration_ms {
        properties["duration_ms"] = duration.into();
    }
    let exception = matches!(kind, TelemetryEventKind::FrontendError).then(|| {
        // A fixed handled category, never the source exception, stack, query or URL.
        let mut error = properties.clone();
        error["$exception_list"] = serde_json::json!([{
            "type": "KeeperFrontendError",
            "value": "A frontend operation failed; private details are not collected",
            "mechanism": {"handled": true, "type": "generic"}
        }]);
        error["$exception_fingerprint"] = "keeper-frontend-error-v1".into();
        error["$exception_level"] = "error".into();
        serde_json::json!({"event": "$exception", "timestamp": timestamp, "properties": error})
    });
    let mut batch = vec![
        serde_json::json!({"event": kind.name(), "timestamp": timestamp, "properties": properties}),
    ];
    if let Some(exception) = exception {
        batch.push(exception);
    }
    Some(serde_json::json!({ "api_key": config.token, "batch": batch }))
}

async fn export(
    client: Option<&reqwest::Client>,
    config: &PublicConfig,
    id: &str,
    record: &Record,
) {
    let Some(client) = client else {
        return;
    };
    let events = async {
        if let Some(bytes) =
            batch_payload(config, id, record).and_then(|body| serde_json::to_vec(&body).ok())
        {
            post(client, config, "/batch/", bytes, false).await;
        }
    };
    let logs = async {
        if record.diagnostic() {
            post(client, config, "/i/v1/logs", wire::logs(record), true).await;
        }
    };
    let traces = async {
        if record.diagnostic() {
            post(client, config, "/i/v1/traces", wire::traces(record), true).await;
        }
    };
    // One slow collector cannot consume another collector's entire five-second budget.
    // Bounded to three requests for one queued record; revocation cancels all of them.
    tokio::join!(events, logs, traces);
}

async fn fetch_config(
    client: Option<&reqwest::Client>,
    config: &PublicConfig,
    id: &str,
) -> Option<TelemetryRemoteConfigVm> {
    let mut response = client?.post(format!("{}/flags?v=2", config.host))
        .json(&serde_json::json!({"api_key": config.token, "distinct_id": id, "flag_keys_to_evaluate": ["keeper-client-config"], "send_feature_flag_events": false}))
        .send().await.ok()?.error_for_status().ok()?;
    if response
        .content_length()
        .is_some_and(|size| size > RESPONSE_LIMIT as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    parse_remote(&body)
}

fn parse_remote(bytes: &[u8]) -> Option<TelemetryRemoteConfigVm> {
    if bytes.len() > RESPONSE_LIMIT {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if value
        .get("errorsWhileComputingFlags")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return None;
    }
    let flag = value.get("flags")?.get("keeper-client-config")?;
    if !flag.get("enabled")?.as_bool()? {
        return None;
    }
    let raw = flag.get("metadata")?.get("payload")?;
    let payload = match raw {
        serde_json::Value::String(text) => serde_json::from_str(text).ok()?,
        value => value.clone(),
    };
    let config: TelemetryRemoteConfigVm = serde_json::from_value(payload).ok()?;
    if let Some(text) = &config.support_message {
        // Narrow plain-text vocabulary intentionally excludes links, markup,
        // controls, bidi overrides and action-like punctuation. No interpolation.
        if text.is_empty() || text.chars().count() > 200 {
            return None;
        }
        let link_like = text
            .chars()
            .zip(text.chars().skip(1))
            .zip(text.chars().skip(2))
            .any(|((left, middle), right)| {
                middle == '.' && left.is_alphanumeric() && right.is_alphanumeric()
            });
        if link_like
            || text.chars().any(|c| {
                !c.is_alphanumeric()
                    && !matches!(c, ' ' | '.' | ',' | '!' | '?' | '-' | '\'' | '(' | ')')
            })
        {
            return None;
        }
    }
    Some(config)
}

#[cfg(test)]
mod tests;
