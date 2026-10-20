//! Optional, consented observability (Epic 71; R461/R462).
//!
//! Two kinds of record leave: closed, content-free events (readiness, the two
//! counted surfaces, interactions, the microphone-loss cause) and **problems**
//! — every WARN/ERROR line the shell logs ([`LogExport`]) and every error the
//! webview reports ([`Telemetry::capture_frontend_error`]) — each a
//! `$exception` whose message went through [`redact`] first. Lines
//! under an agent target ([`is_agent_target`]) leave without their message.
//!
//! Diagnostics and product-analytics consent are account-wide settings keys
//! (`telemetry.diagnostics`, `telemetry.product_analytics`) that settings sync
//! carries to every device; remote configuration and the random installation
//! id stay in this installation's `telemetry-consent-v1.json`.
mod layer;
pub mod redact;
mod wire;

pub use layer::{export_filter, log_subscriber, LogExport};

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// How many problems of one fingerprint may leave per minute (R461).
const PROBLEMS_PER_FINGERPRINT_PER_MINUTE: u32 = 10;
/// How many problems one run of keeper may send at all (R461's session cap).
const PROBLEMS_PER_SESSION: u32 = 500;
/// How often, at most, the count of refused problems is reported.
const DROP_REPORT_INTERVAL: Duration = Duration::from_secs(60);
/// The per-installation store: remote configuration and the installation id.
const STORE_FILE: &str = "telemetry-consent-v1.json";
/// The target a webview's reported error is logged and exported under.
pub const FRONTEND_TARGET: &str = "keeper::frontend";
/// The most characters of a webview's report that are read at all: a longer
/// one is [`redact::bound`]ed — its cut token dropped — before redaction.
pub const FRONTEND_INPUT_CAP: usize = 8_000;
/// What an agent-target line says in place of its message (R461).
const AGENT_MESSAGE_WITHHELD: &str = "An agent-host line; its message stays on this device.";

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
    /// A settings file (`keeper.toml`, the account's own included) sets
    /// diagnostics or product analytics, so a switch here is saved but does
    /// not decide: `consent` is what the file says (R495 06).
    pub consent_pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TelemetryEventKind {
    AppReady,
    CommandPaletteOpened,
    SettingsOpened,
    Interaction,
}

impl TelemetryEventKind {
    fn diagnostic(self) -> bool {
        matches!(self, Self::AppReady | Self::Interaction)
    }

    fn name(self) -> &'static str {
        match self {
            Self::AppReady => "keeper_app_ready",
            Self::CommandPaletteOpened => "keeper_command_palette_opened",
            Self::SettingsOpened => "keeper_settings_opened",
            Self::Interaction => "keeper_interaction",
        }
    }
}

/// Which declared surface a `keeper_interaction` belongs to (Epic 100,
/// R381, 100.1 #9). A closed set of content-free names: never a command, a
/// room, an agent or anything typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TelemetrySurface {
    /// The chat composer's slash menu opened.
    SlashMenuOpened,
    /// A command was chosen from the slash menu.
    SlashMenuChosen,
    /// keeper opened at launch on the default agent's conversation.
    LaunchDefaultAgent,
}

impl TelemetrySurface {
    /// The value exported as the event's `surface` property.
    fn name(self) -> &'static str {
        match self {
            Self::SlashMenuOpened => "slash_menu_opened",
            Self::SlashMenuChosen => "slash_menu_chosen",
            Self::LaunchDefaultAgent => "launch_default_agent",
        }
    }
}

/// Why a recording lost its microphone, exported as the `cause` property of
/// `keeper_recording_mic_lost`. Raised by the shell, never the webview, so it
/// has no serde or TypeScript form. A closed set: never the device name, the
/// sidecar's message or anything about the recording itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicLossCause {
    /// The microphone went away (unplugged, or its fallback failed).
    Removed,
    /// The microphone stayed attached but stopped delivering audio.
    Stalled,
    /// The microphone delivered only digital silence.
    Silent,
}

impl MicLossCause {
    /// Maps a recording sidecar warning code to its cause. Any other code
    /// (camera, echo cancellation, segments, unknown) is not a microphone loss.
    pub fn from_warning_code(code: &str) -> Option<Self> {
        match code {
            "micLost" => Some(Self::Removed),
            "micStalled" => Some(Self::Stalled),
            "micSilent" => Some(Self::Silent),
            _ => None,
        }
    }

    /// The value exported as the event's `cause` property.
    pub fn name(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Stalled => "stalled",
            Self::Silent => "silent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TelemetryEventReq {
    pub kind: TelemetryEventKind,
    pub duration_ms: Option<f64>,
    /// Only on [`TelemetryEventKind::Interaction`]; absent everywhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub surface: Option<TelemetrySurface>,
}

impl TelemetryEventReq {
    fn validate(&self) -> Result<(), TelemetryError> {
        if self
            .duration_ms
            .is_some_and(|ms| !ms.is_finite() || !(0.0..=MAX_DURATION_MS).contains(&ms))
        {
            return Err(TelemetryError::InvalidEvent);
        }
        if self.surface.is_some() && self.kind != TelemetryEventKind::Interaction {
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

/// This installation's half of the state (R462): remote configuration and the
/// random id records are sent under. Diagnostics and product analytics are
/// not here — they are the account's settings keys — except an epic-71 "on"
/// still waiting to be adopted ([`Telemetry::account_settled`]).
#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredInstallation {
    version: u8,
    remote_config: bool,
    installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adopt: Option<LegacyConsent>,
}

/// What an epic-71 store had on, kept until the account's settings are known
/// here and it can be adopted where the account has no value (R495 07). It
/// is never in force by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyConsent {
    diagnostics: bool,
    product_analytics: bool,
}

/// What a settings sync established about one consent key on the account,
/// which is all an epic-71 "on" may be adopted on (R511, P2-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKey {
    /// No device of the account has a value: the reconciled file holds
    /// none and nothing about the key failed. The old "on" is adopted.
    Absent,
    /// The account has a value and it was applied here: it decides, and
    /// the old "on" is dropped.
    Present,
    /// Not established: its apply failed or a local change overtook it, or
    /// the file holds a value this build cannot read. The old "on" waits
    /// for a sync that establishes it.
    Unknown,
}

/// [`AccountKey`] for each of the two account-wide consents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountConsentKnown {
    pub diagnostics: AccountKey,
    pub product_analytics: AccountKey,
}

impl AccountConsentKnown {
    /// An installation with no account: its own table is all there is.
    pub const NO_ACCOUNT: Self = Self {
        diagnostics: AccountKey::Absent,
        product_analytics: AccountKey::Absent,
    };

    /// What one reconciled shared settings file establishes: `file` as the
    /// merge left it, `not_applied` the keys whose apply failed or was
    /// overtaken by a change made here meanwhile.
    pub fn from_sync(
        file: &crate::org_account::settings_sync::Values,
        not_applied: &dyn Fn(&str) -> bool,
    ) -> Self {
        let key = |name: &str| {
            if not_applied(name) || file.preserved.contains_key(name) {
                AccountKey::Unknown
            } else if file.values.contains_key(name) {
                AccountKey::Present
            } else {
                AccountKey::Absent
            }
        };
        Self {
            diagnostics: key(crate::registry::TELEMETRY_DIAGNOSTICS_KEY),
            product_analytics: key(crate::registry::TELEMETRY_PRODUCT_ANALYTICS_KEY),
        }
    }
}

/// The epic-71 store, read once to migrate it (R462).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredV1 {
    version: u8,
    consent: TelemetryConsentVm,
    installation_id: Option<String>,
}

fn valid_installation_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) && id.bytes().any(|b| b != b'0')
}

impl StoredInstallation {
    /// The store, an epic-71 file migrated to this shape in memory (its
    /// consent kept as [`StoredInstallation::adopt`], never in force).
    /// Anything unreadable, oversized or inconsistent is the default: off.
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
            if let Ok(state) = serde_json::from_slice::<Self>(&bytes) {
                let id_ok = state
                    .installation_id
                    .as_deref()
                    .is_none_or(valid_installation_id);
                return (state.version == 2 && id_ok).then_some(state);
            }
            let old: StoredV1 = serde_json::from_slice(&bytes).ok()?;
            let valid_id = old
                .installation_id
                .as_deref()
                .is_some_and(valid_installation_id);
            if old.version != 1
                || (old.consent.any() && !valid_id)
                || (!old.consent.any() && old.installation_id.is_some())
            {
                return None;
            }
            let on = old.consent.diagnostics || old.consent.product_analytics;
            Some(Self {
                version: 2,
                remote_config: old.consent.remote_config,
                installation_id: old.installation_id,
                adopt: on.then_some(LegacyConsent {
                    diagnostics: old.consent.diagnostics,
                    product_analytics: old.consent.product_analytics,
                }),
            })
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
    MicLost(MicLossCause),
    /// A WARN/ERROR line the shell logged (R461): one `$exception`.
    Log,
    /// An error the webview reported: `keeper_frontend_error` and its
    /// `$exception`.
    FrontendError,
    /// How many problems the limits refused since the last such record.
    ProblemsDropped(u64),
    RemoteConfig,
    FlagsRequest,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Frontend(kind) => kind.name(),
            Self::MicLost(_) => "keeper_recording_mic_lost",
            Self::Log => "$exception",
            Self::FrontendError => "keeper_frontend_error",
            Self::ProblemsDropped(_) => "keeper_problems_dropped",
            Self::RemoteConfig => "keeper.remote_config",
            Self::FlagsRequest => "keeper.flags_request",
        }
    }
}

/// Where a log event was raised (R461): its level, target and callsite.
#[derive(Debug, Clone, Copy)]
pub struct LogSite<'a> {
    pub level: tracing::Level,
    pub target: &'a str,
    pub file: Option<&'a str>,
    pub line: Option<u32>,
}

/// Agent content stays home (R461): a line under `keeper_agent…` or
/// `keeper_core::agents…` leaves with its level, target, callsite and
/// fingerprint, never its message.
pub fn is_agent_target(target: &str) -> bool {
    target.starts_with("keeper_agent") || target.starts_with("keeper_core::agents")
}

/// A callsite's file as it may leave: a workspace path as compiled
/// (`crates/keeper/src/ipc.rs`: `file!()` relative to the workspace, public
/// source, kept whole), a dependency's path from its crate directory
/// (`matrix-sdk-0.18.0/src/…`, never the builder's home), any other absolute
/// path through the redactor.
fn callsite_file(file: &str) -> String {
    for (marker, skip) in [
        ("/registry/src/", 1),
        ("/git/checkouts/", 2),
        ("/rustc/", 1),
    ] {
        if let Some(at) = file.find(marker) {
            let mut rest = &file[at + marker.len()..];
            for _ in 0..skip {
                rest = rest.split_once('/').map_or("", |(_, after)| after);
            }
            return rest.to_owned();
        }
    }
    let absolute = file.starts_with(['/', '\\', '~']) || file.as_bytes().get(1) == Some(&b':');
    if absolute {
        redact::redact(file)
    } else {
        file.to_owned()
    }
}

/// A stable fingerprint of `parts` (FNV-1a, 64 bits): the same callsite gives
/// the same issue in PostHog on every build and every device.
fn fingerprint(parts: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// What a problem carries to PostHog (R461).
struct Exception {
    level: tracing::Level,
    target: String,
    file: Option<String>,
    line: Option<u32>,
    /// `$exception_list[0].type`: the target, or a webview error's source.
    kind: String,
    /// Redacted; `None` for an agent target.
    message: Option<String>,
    fingerprint: String,
}

/// No caller string reaches a record except a problem's, and that one only
/// through [`redact::redact`]. IDs are created inside this module.
struct Record {
    operation: Operation,
    duration_ms: Option<f64>,
    /// A declared interaction surface (closed set), never content.
    surface: Option<TelemetrySurface>,
    start: u64,
    end: u64,
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    outcome: u32,
    exception: Option<Box<Exception>>,
}

impl Record {
    fn frontend(event: TelemetryEventReq) -> Self {
        let end = unix_nanos();
        let elapsed = event.duration_ms.unwrap_or(0.0) * 1_000_000.0;
        Self {
            operation: Operation::Frontend(event.kind),
            duration_ms: event.duration_ms,
            surface: event.surface,
            start: end.saturating_sub(elapsed as u64),
            end,
            trace_id: random_id::<16>(),
            span_id: random_id::<8>(),
            parent_span_id: None,
            outcome: 0,
            exception: None,
        }
    }

    /// A count with no duration: the span has zero length and no outcome
    /// error, since a lost microphone is a warning the recording survives.
    fn instant(operation: Operation, exception: Option<Exception>) -> Self {
        let end = unix_nanos();
        Self {
            operation,
            duration_ms: None,
            surface: None,
            start: end,
            end,
            trace_id: random_id::<16>(),
            span_id: random_id::<8>(),
            parent_span_id: None,
            outcome: 0,
            exception: exception.map(Box::new),
        }
    }

    fn mic_lost(cause: MicLossCause) -> Self {
        Self::instant(Operation::MicLost(cause), None)
    }

    fn diagnostic(&self) -> bool {
        match self.operation {
            Operation::Frontend(kind) => kind.diagnostic(),
            _ => true,
        }
    }

    /// Whether the OTLP log and trace collectors receive this record too. A
    /// problem leaves only as the `$exception` event.
    fn otlp(&self) -> bool {
        self.diagnostic()
            && !matches!(
                self.operation,
                Operation::Log | Operation::FrontendError | Operation::ProblemsDropped(_)
            )
    }
}

struct Inner {
    /// Effective consent: the account's two keys and this installation's
    /// remote-configuration choice.
    consent: TelemetryConsentVm,
    installation_id: Option<String>,
    epoch: u64,
    queue: VecDeque<Record>,
    admitted: VecDeque<Instant>,
    remote_last: Option<Instant>,
    remote: TelemetryRemoteConfigVm,
    study_active: bool,
    /// Per problem fingerprint: the current minute's start and its count.
    fingerprints: HashMap<u64, (Instant, u32)>,
    problems_admitted: u32,
    dropped_reported: Option<Instant>,
    /// An epic-71 "on" not adopted yet (R495 07).
    adopt: Option<LegacyConsent>,
    /// The store file no longer says what `adopt` says.
    store_stale: bool,
}

/// One process-wide instance shared by every window. One queued record runs at
/// once with at most three collector requests (plus one concurrent flags read),
/// no retries or disk spool. Revocation drops futures; sent bytes cannot be recalled.
pub struct Telemetry {
    config: Option<PublicConfig>,
    /// This installation's store file.
    path: Option<PathBuf>,
    /// The data directory whose `settings` table holds the account's consent.
    settings: Option<PathBuf>,
    inner: Mutex<Inner>,
    /// `inner.consent.diagnostics`, readable without the lock by the log layer.
    diagnostics: AtomicBool,
    /// Problems refused since the last [`Operation::ProblemsDropped`] record.
    dropped: AtomicU64,
    /// Held while a transition reads the effective consent and installs it
    /// (a local save, a reload, an adoption) — never across a write, so the
    /// settings observer a write runs never waits on it. Installs therefore
    /// happen in the order of their reads, and every transition reads after
    /// its own write: the last install answers the last write, whichever
    /// transition began first (R511, P2-02).
    settling: Mutex<()>,
    epoch: watch::Sender<u64>,
    wake: Notify,
    remote_slot: Semaphore,
    http: LazyLock<Option<reqwest::Client>>,
    /// Where every request runs: the exporter's own threads (R511 P2-05).
    transport: LazyLock<layer::Transport>,
}

impl Telemetry {
    /// Open the installation's store and read the account's consent from
    /// `data_dir`'s settings table. An epic-71 "on" is kept, not adopted:
    /// that waits for [`Telemetry::account_settled`] (R495 07).
    pub fn open(data_dir: Option<&Path>, config: Option<PublicConfig>) -> Arc<Self> {
        let path = data_dir.map(|dir| dir.join(STORE_FILE));
        let stored = path
            .as_deref()
            .map(StoredInstallation::load)
            .unwrap_or_default();
        let (epoch, _) = watch::channel(0);
        let telemetry = Arc::new(Self {
            config,
            path,
            settings: data_dir.map(Path::to_path_buf),
            inner: Mutex::new(Inner {
                consent: TelemetryConsentVm {
                    remote_config: stored.remote_config,
                    ..TelemetryConsentVm::default()
                },
                installation_id: stored.installation_id,
                epoch: 0,
                queue: VecDeque::new(),
                admitted: VecDeque::new(),
                remote_last: None,
                remote: TelemetryRemoteConfigVm::default(),
                study_active: false,
                fingerprints: HashMap::new(),
                problems_admitted: 0,
                dropped_reported: None,
                adopt: stored.adopt,
                store_stale: false,
            }),
            diagnostics: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            settling: Mutex::new(()),
            epoch,
            wake: Notify::new(),
            remote_slot: Semaphore::new(1),
            http: LazyLock::new(client),
            transport: LazyLock::new(layer::Transport::new),
        });
        let _ = telemetry.reload_consent();
        telemetry
    }

    pub fn study_preview(&self) -> TelemetryStudyPreviewVm {
        TelemetryStudyPreviewVm {
            configured: self.config.is_some(),
            host: self.config.as_ref().map(|config| config.host.clone()),
        }
    }

    pub fn status(&self) -> Result<TelemetryStatusVm, TelemetryError> {
        let consent_pinned = crate::registry::telemetry_consent_pinned();
        let inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
        Ok(TelemetryStatusVm {
            consent: inner.consent,
            configured: self.config.is_some(),
            host: self.config.as_ref().map(|config| config.host.clone()),
            installation_id: inner.installation_id.clone(),
            consent_pinned,
        })
    }

    /// The account's effective consent, `(diagnostics, product_analytics)`:
    /// a settings file's pin before the table (R462). Unreadable is off.
    fn effective_consent(&self) -> (bool, bool) {
        self.settings
            .as_deref()
            .and_then(|dir| crate::registry::get_telemetry_consent(dir).ok())
            .unwrap_or_default()
    }

    /// Re-read the account's effective consent and make it this process's —
    /// at boot, after every synced apply of one of the two keys, and after
    /// every account-layer install or clear (R496). A change discards what
    /// was queued and cancels what is in flight.
    pub fn reload_consent(&self) -> Result<TelemetryStatusVm, TelemetryError> {
        let settled = self.settle_effective(None, false);
        self.wake.notify_one();
        settled?;
        self.status()
    }

    /// A person's choice: diagnostics and product analytics go to the
    /// account's settings (and from there to every device, R462), remote
    /// configuration to this installation's store. What takes effect is the
    /// EFFECTIVE answer read back after the write — a settings file that pins
    /// a key wins, and the status says so (`consent_pinned`, R495 06). Any
    /// write failure leaves this process off, including on an attempted
    /// enable.
    pub fn set_consent(
        &self,
        consent: TelemetryConsentVm,
    ) -> Result<TelemetryStatusVm, TelemetryError> {
        // Outside every lock of this type: the settings observer runs on
        // this thread, and the transition that settles is serialized only
        // from its read on ([`Telemetry::settle_effective`]).
        #[cfg(test)]
        transition_point(TransitionPoint::BeforeWrite);
        let written = self.settings.as_deref().is_some_and(|dir| {
            crate::registry::set_telemetry_consent(
                dir,
                consent.diagnostics,
                consent.product_analytics,
            )
            .is_ok()
        });
        let settled = if written {
            self.settle_effective(Some(consent.remote_config), true)
        } else {
            let settling = self
                .settling
                .lock()
                .map_err(|_| TelemetryError::Unavailable)?;
            let mut inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
            self.discard_pending(&mut inner);
            self.switch_off(&mut inner);
            drop((inner, settling));
            Err(TelemetryError::Persistence)
        };
        self.wake.notify_one();
        settled?;
        self.status()
    }

    /// What the account's settings say is known on this device: it has no
    /// account, or a settings sync with the account has settled (its pulled
    /// values applied, its layers installed). Only now is an epic-71 "on"
    /// adopted, and per key only where `known` proves the account has no
    /// value ([`AccountKey::Absent`]) and this device has none either — no
    /// row, no layer file's pin. A key the account has a value for drops the
    /// old "on"; a key not established (its apply failed or was overtaken)
    /// keeps it pending for the next sync — so an upgrade never pushes an
    /// old installation's "on" over an account-wide "off" (R462, R495 07,
    /// R511 P2-03).
    pub fn account_settled(
        &self,
        known: AccountConsentKnown,
    ) -> Result<TelemetryStatusVm, TelemetryError> {
        let pending = self
            .inner
            .lock()
            .map_err(|_| TelemetryError::Unavailable)?
            .adopt;
        if let (Some(adopt), Some(dir)) = (pending, self.settings.as_deref()) {
            #[cfg(test)]
            transition_point(TransitionPoint::BeforeAdopt);
            // A failed adoption keeps the pending "on" for the next try.
            crate::registry::adopt_installation_telemetry_consent(
                dir,
                adopt.diagnostics && known.diagnostics == AccountKey::Absent,
                adopt.product_analytics && known.product_analytics == AccountKey::Absent,
            )
            .map_err(|_| TelemetryError::Persistence)?;
            let rest = LegacyConsent {
                diagnostics: adopt.diagnostics && known.diagnostics == AccountKey::Unknown,
                product_analytics: adopt.product_analytics
                    && known.product_analytics == AccountKey::Unknown,
            };
            let rest = (rest.diagnostics || rest.product_analytics).then_some(rest);
            let mut inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
            if inner.adopt != rest {
                inner.adopt = rest;
                inner.store_stale = true;
            }
        }
        self.reload_consent()
    }

    /// Read the account's effective consent — a settings file's pin before
    /// the table — and make it this process's, holding
    /// [`Telemetry::settling`] from the read to the install. `remote_config`
    /// is a person's new choice for this installation, `None` to keep it;
    /// `discard` drops what was queued even when nothing changes (a save).
    /// Any change discards what was queued and cancels what is in flight.
    fn settle_effective(
        &self,
        remote_config: Option<bool>,
        discard: bool,
    ) -> Result<(), TelemetryError> {
        #[cfg(test)]
        if matches!(
            self.settling.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ) {
            transition_point(TransitionPoint::Contended);
        }
        let _settling = self
            .settling
            .lock()
            .map_err(|_| TelemetryError::Unavailable)?;
        let (diagnostics, product_analytics) = self.effective_consent();
        #[cfg(test)]
        transition_point(TransitionPoint::AfterRead);
        let mut inner = self.inner.lock().map_err(|_| TelemetryError::Unavailable)?;
        let consent = TelemetryConsentVm {
            diagnostics,
            product_analytics,
            remote_config: remote_config.unwrap_or(inner.consent.remote_config),
        };
        if discard || consent != inner.consent {
            self.discard_pending(&mut inner);
        }
        self.settle(&mut inner, consent)
    }

    /// Revocation semantics for any consent change: queued records and the
    /// remote message go, and in-flight exports are cancelled.
    fn discard_pending(&self, inner: &mut Inner) {
        inner.epoch = inner.epoch.wrapping_add(1);
        self.epoch.send_replace(inner.epoch);
        inner.queue.clear();
        inner.remote = TelemetryRemoteConfigVm::default();
        inner.remote_last = None;
    }

    fn switch_off(&self, inner: &mut Inner) {
        inner.consent = TelemetryConsentVm::default();
        inner.installation_id = None;
        self.diagnostics.store(false, Ordering::Relaxed);
    }

    /// Make `consent` effective: an installation id exists exactly while any
    /// category is on (a new one after every full revocation), persisted with
    /// the remote-configuration choice. A failed write is off.
    fn settle(&self, inner: &mut Inner, consent: TelemetryConsentVm) -> Result<(), TelemetryError> {
        let installation_id = consent.any().then(|| {
            inner
                .installation_id
                .clone()
                .unwrap_or_else(random_id::<16>)
        });
        let stored = StoredInstallation {
            version: 2,
            remote_config: consent.remote_config,
            installation_id,
            adopt: inner.adopt,
        };
        let unchanged = !inner.store_stale
            && stored.remote_config == inner.consent.remote_config
            && stored.installation_id == inner.installation_id;
        if !unchanged {
            let persisted = self
                .path
                .as_deref()
                .ok_or(TelemetryError::Persistence)
                .and_then(|path| stored.persist(path));
            if persisted.is_err() {
                // Do not leave an enabled store behind when the atomic
                // replacement fails: a missing or truncated file is off.
                if let Some(path) = self.path.as_deref() {
                    if std::fs::remove_file(path).is_err() {
                        let _ = std::fs::OpenOptions::new()
                            .write(true)
                            .truncate(true)
                            .open(path);
                    }
                }
                self.switch_off(inner);
                return Err(TelemetryError::Persistence);
            }
        }
        inner.consent = consent;
        inner.installation_id = stored.installation_id;
        inner.store_stale = false;
        self.diagnostics
            .store(consent.diagnostics, Ordering::Relaxed);
        Ok(())
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
        (inner.consent.any() || inner.study_active).then(|| EgressEndpointVm {
            url: config.host.clone(),
            kind: EgressKind::Telemetry,
            label: "PostHog — enabled observability or synthetic study".to_owned(),
        })
    }

    pub fn capture(&self, event: TelemetryEventReq) -> Result<(), TelemetryError> {
        event.validate()?;
        self.enqueue(event.kind.diagnostic(), || Record::frontend(event));
        Ok(())
    }

    /// The shell's microphone-loss count, under the same diagnostics consent,
    /// rate limit and queue as a webview event. Never blocks; dropped silently
    /// like any other event when not admitted.
    pub fn capture_mic_lost(&self, cause: MicLossCause) {
        self.enqueue(true, || Record::mic_lost(cause));
    }

    /// The one admission path for every closed event. The record is built
    /// only once admitted, so a refused event costs no IDs.
    fn enqueue(&self, diagnostic: bool, record: impl FnOnce() -> Record) {
        if self.config.is_none() {
            return;
        }
        // Contended persistence/export is not allowed to stall an app operation.
        let Ok(mut inner) = self.inner.try_lock() else {
            return;
        };
        let consent = inner.consent;
        let allowed = if diagnostic {
            consent.diagnostics
        } else {
            consent.product_analytics
        };
        if !allowed || !admit(&mut inner) {
            return;
        }
        inner.queue.push_back(record());
        drop(inner);
        self.wake.notify_one();
    }

    /// Whether a problem would be considered at all — built with a
    /// destination and diagnostics on. Lock-free: the log layer asks before it
    /// formats anything.
    pub fn wants_problems(&self) -> bool {
        self.config.is_some() && self.diagnostics.load(Ordering::Relaxed)
    }

    /// The `/batch/` body of every record waiting to leave, as text: exactly
    /// what the exporter would send. For the tests of a crate that links a
    /// real producer beside this boundary (the panic hook in keeper-sync,
    /// tested from keeper-agent); nothing in the app reads it.
    #[doc(hidden)]
    pub fn queued_batch_payloads(&self) -> Vec<String> {
        let (Some(config), Ok(inner)) = (&self.config, self.inner.lock()) else {
            return Vec::new();
        };
        let id = inner.installation_id.clone().unwrap_or_default();
        inner
            .queue
            .iter()
            .filter_map(|record| batch_payload(config, &id, record))
            .map(|payload| payload.to_string())
            .collect()
    }

    /// One WARN/ERROR line (R461): level, target, callsite and fingerprint,
    /// and `message` redacted then capped — or withheld, under an agent
    /// target. Never blocks: a contended lock is a dropped problem, counted.
    pub fn capture_log(&self, site: LogSite<'_>, message: String) {
        self.capture_line(site, move || redact::redact(&message));
    }

    /// [`Telemetry::capture_log`] for text already redacted part by part
    /// ([`LogExport`]); `message` is asked for only once the problem is
    /// admitted, and never under an agent target.
    pub(crate) fn capture_line(&self, site: LogSite<'_>, message: impl FnOnce() -> String) {
        if site.level > tracing::Level::WARN {
            return;
        }
        let file = site.file.map(callsite_file);
        let line = site.line.map(|line| line.to_string()).unwrap_or_default();
        let id = fingerprint(&[site.target, file.as_deref().unwrap_or(""), &line]);
        self.capture_problem(id, |fingerprint| {
            let message = (!is_agent_target(site.target)).then(message);
            Record::instant(
                Operation::Log,
                Some(Exception {
                    level: site.level,
                    target: site.target.to_owned(),
                    file,
                    line: site.line,
                    kind: site.target.to_owned(),
                    message,
                    fingerprint,
                }),
            )
        });
    }

    /// An error the webview reported (`reportFrontendError`, the window's
    /// error and unhandled-rejection events, the crash boundary, a hooked
    /// `console.error`/`console.warn`): `keeper_frontend_error` with a
    /// `$exception` carrying the redacted message, fingerprinted by its
    /// source and that message. `message` arrives uncut (the webview no
    /// longer caps it, R495 04); past [`FRONTEND_INPUT_CAP`] it is bounded
    /// — its cut token dropped — then redacted, then capped.
    pub fn capture_frontend_error(&self, source: &str, message: &str, warning: bool) {
        if !self.wants_problems() {
            return;
        }
        let source = redact::redact(&redact::bound(source, 40));
        let message = redact::redact(&redact::bound(message, FRONTEND_INPUT_CAP));
        let id = fingerprint(&[FRONTEND_TARGET, &source, &message]);
        self.capture_problem(id, |fingerprint| {
            Record::instant(
                Operation::FrontendError,
                Some(Exception {
                    level: if warning {
                        tracing::Level::WARN
                    } else {
                        tracing::Level::ERROR
                    },
                    target: FRONTEND_TARGET.to_owned(),
                    file: None,
                    line: None,
                    kind: source,
                    message: Some(message),
                    fingerprint,
                }),
            )
        });
    }

    /// The admission path for problems: diagnostics consent, the session
    /// cap, the fingerprint's minute and the queue bound. The record is built
    /// (and the message redacted) outside the lock, only once admitted.
    fn capture_problem(&self, id: u64, build: impl FnOnce(String) -> Record) {
        if !self.wants_problems() {
            return;
        }
        let Ok(mut inner) = self.inner.try_lock() else {
            self.note_dropped();
            return;
        };
        if !inner.consent.diagnostics {
            return;
        }
        if !admit_problem(&mut inner, id, Instant::now()) {
            drop(inner);
            self.note_dropped();
            return;
        }
        let epoch = inner.epoch;
        drop(inner);
        let record = build(format!("{id:016x}"));
        let Ok(mut inner) = self.inner.try_lock() else {
            self.note_dropped();
            return;
        };
        // Consent changed meanwhile: the record belongs to a revoked epoch.
        if inner.epoch != epoch || !inner.consent.diagnostics {
            return;
        }
        if inner.queue.len() >= QUEUE_LIMIT {
            drop(inner);
            self.note_dropped();
            return;
        }
        inner.queue.push_back(record);
        drop(inner);
        self.wake.notify_one();
    }

    /// Count one refused problem; the worker is woken only by the first, and
    /// reports the count at most once per [`DROP_REPORT_INTERVAL`].
    fn note_dropped(&self) {
        if self.dropped.fetch_add(1, Ordering::Relaxed) == 0 {
            self.wake.notify_one();
        }
    }

    /// When the next dropped-count record may go, while any count waits.
    fn drop_report_at(&self) -> Option<Instant> {
        if self.dropped.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let last = self.inner.lock().ok()?.dropped_reported;
        Some(last.map_or_else(Instant::now, |at| at + DROP_REPORT_INTERVAL))
    }

    /// At most once per [`DROP_REPORT_INTERVAL`]: one record counting every
    /// problem refused since the last one (R461). A count with diagnostics
    /// off is discarded with the records it counted.
    fn report_dropped(&self, now: Instant) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner
            .dropped_reported
            .is_some_and(|at| now.saturating_duration_since(at) < DROP_REPORT_INTERVAL)
        {
            return;
        }
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped == 0 || self.config.is_none() || !inner.consent.diagnostics {
            return;
        }
        if inner.queue.len() >= QUEUE_LIMIT {
            self.dropped.fetch_add(dropped, Ordering::Relaxed);
            return;
        }
        inner.dropped_reported = Some(now);
        inner
            .queue
            .push_back(Record::instant(Operation::ProblemsDropped(dropped), None));
    }

    /// Run once on the shell runtime. No HTTP client, task per event or flag
    /// request is initialized at boot. Empty/off installs only await this notify.
    pub async fn run(self: Arc<Self>) {
        loop {
            match self.drop_report_at() {
                Some(at) => {
                    tokio::select! {
                        _ = self.wake.notified() => {},
                        _ = tokio::time::sleep_until(at.into()) => {},
                    }
                }
                None => self.wake.notified().await,
            }
            self.report_dropped(Instant::now());
            loop {
                let mut cancellation = self.epoch.subscribe();
                let next = self.inner.lock().ok().and_then(|mut inner| {
                    let record = inner.queue.pop_front()?;
                    let id = inner.installation_id.clone()?;
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
                    _ = tokio::time::timeout(REQUEST_TIMEOUT, self.transport.run(export((*self.http).clone(), config.clone(), id, record))) => {},
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
            if !inner.consent.remote_config {
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
                inner.installation_id.clone()?,
                inner.epoch,
                inner.consent.diagnostics,
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
            result = tokio::time::timeout(REQUEST_TIMEOUT, self.transport.run(fetch_config((*self.http).clone(), config.clone(), id))) => result.ok().flatten().flatten(),
        };
        let end = unix_nanos();
        let Ok(mut inner) = self.inner.lock() else {
            return TelemetryRemoteConfigVm::default();
        };
        if inner.epoch != epoch || !inner.consent.remote_config {
            return TelemetryRemoteConfigVm::default();
        }
        let outcome = u32::from(result.is_none());
        inner.remote = result.unwrap_or_default();
        // A real Rust operation with a child span and correlated records. Its
        // response text, endpoint, HTTP errors and installation ID aren't attrs.
        if diagnostics
            && inner.consent.diagnostics
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
                    surface: None,
                    span_id,
                    parent_span_id: parent,
                    trace_id: trace_id.clone(),
                    duration_ms: Some(end.saturating_sub(start) as f64 / 1_000_000.0),
                    start,
                    end,
                    outcome,
                    exception: None,
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
            .filter(|inner| inner.consent.remote_config)
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

/// A problem's own limits, separate from [`admit`]'s so a burst of warnings
/// cannot starve the closed events (or the reverse): the session cap, then at
/// most [`PROBLEMS_PER_FINGERPRINT_PER_MINUTE`] per fingerprint per minute.
fn admit_problem(inner: &mut Inner, id: u64, now: Instant) -> bool {
    if inner.problems_admitted >= PROBLEMS_PER_SESSION || inner.queue.len() >= QUEUE_LIMIT {
        return false;
    }
    let minute = inner.fingerprints.entry(id).or_insert((now, 0));
    if now.saturating_duration_since(minute.0) >= Duration::from_secs(60) {
        *minute = (now, 0);
    }
    if minute.1 >= PROBLEMS_PER_FINGERPRINT_PER_MINUTE {
        return false;
    }
    minute.1 += 1;
    inner.problems_admitted += 1;
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
    let (name, diagnostic, source) = match record.operation {
        Operation::Frontend(kind) => (kind.name(), kind.diagnostic(), "frontend"),
        Operation::MicLost(_) => (record.operation.name(), true, "shell"),
        Operation::Log | Operation::ProblemsDropped(_) => (record.operation.name(), true, "log"),
        Operation::FrontendError => (record.operation.name(), true, "frontend"),
        Operation::RemoteConfig | Operation::FlagsRequest => return None,
    };
    let timestamp = chrono::DateTime::from_timestamp_nanos(i64::try_from(record.end).ok()?)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut properties = serde_json::json!({
        "distinct_id": id,
        "consent_category": if diagnostic { "diagnostics" } else { "productAnalytics" },
        "event_source": source, "synthetic": false,
        "$process_person_profile": false, "$geoip_disable": true,
        "trace_id": record.trace_id, "span_id": record.span_id
    });
    if let Some(duration) = record.duration_ms {
        properties["duration_ms"] = duration.into();
    }
    if let Some(surface) = record.surface {
        properties["surface"] = surface.name().into();
    }
    if let Operation::MicLost(cause) = record.operation {
        properties["cause"] = cause.name().into();
    }
    if let Operation::ProblemsDropped(dropped) = record.operation {
        properties["dropped"] = dropped.into();
    }
    let exception = record.exception.as_deref().map(|problem| {
        let mut error = properties.clone();
        let value = problem
            .message
            .as_deref()
            .filter(|message| !message.is_empty())
            .unwrap_or(if problem.message.is_none() {
                AGENT_MESSAGE_WITHHELD
            } else {
                "(no message)"
            });
        error["$exception_list"] = serde_json::json!([{
            "type": problem.kind,
            "value": value,
            "mechanism": {"handled": true, "type": "generic"}
        }]);
        error["$exception_fingerprint"] = problem.fingerprint.clone().into();
        error["$exception_level"] = if problem.level == tracing::Level::ERROR {
            "error"
        } else {
            "warning"
        }
        .into();
        error["log_level"] = problem.level.as_str().into();
        error["log_target"] = problem.target.clone().into();
        if let Some(file) = &problem.file {
            error["log_file"] = file.clone().into();
        }
        if let Some(line) = problem.line {
            error["log_line"] = line.into();
        }
        error["os"] = std::env::consts::OS.into();
        error["app_version"] = env!("CARGO_PKG_VERSION").into();
        serde_json::json!({"event": "$exception", "timestamp": timestamp, "properties": error})
    });
    let mut batch = Vec::with_capacity(2);
    if !matches!(record.operation, Operation::Log) {
        batch.push(
            serde_json::json!({"event": name, "timestamp": timestamp, "properties": properties}),
        );
    }
    batch.extend(exception);
    Some(serde_json::json!({ "api_key": config.token, "batch": batch }))
}

/// Owns what it sends: it runs on the exporter's own runtime
/// ([`layer::Transport`]).
async fn export(client: Option<reqwest::Client>, config: PublicConfig, id: String, record: Record) {
    let Some(client) = client.as_ref() else {
        return;
    };
    let (config, id, record) = (&config, id.as_str(), &record);
    let events = async {
        if let Some(bytes) =
            batch_payload(config, id, record).and_then(|body| serde_json::to_vec(&body).ok())
        {
            post(client, config, "/batch/", bytes, false).await;
        }
    };
    let logs = async {
        if record.otlp() {
            post(client, config, "/i/v1/logs", wire::logs(record), true).await;
        }
    };
    let traces = async {
        if record.otlp() {
            post(client, config, "/i/v1/traces", wire::traces(record), true).await;
        }
    };
    // One slow collector cannot consume another collector's entire five-second budget.
    // Bounded to three requests for one queued record; revocation cancels all of them.
    tokio::join!(events, logs, traces);
}

/// Owns what it sends, like [`export`].
async fn fetch_config(
    client: Option<reqwest::Client>,
    config: PublicConfig,
    id: String,
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

/// Where a test may hold a consent transition on its own thread, to force
/// the interleavings R511 (P2-02) is about.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPoint {
    /// A local save, before it writes the account's rows.
    BeforeWrite,
    /// Any transition, between its effective read and its install.
    AfterRead,
    /// A transition about to wait for another's read-to-install to finish.
    Contended,
    /// An adoption that decided to adopt, before it writes.
    BeforeAdopt,
}

#[cfg(test)]
type TransitionHook = Box<dyn FnMut(TransitionPoint)>;

#[cfg(test)]
thread_local! {
    static TRANSITION_HOOK: std::cell::RefCell<Option<TransitionHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn transition_point(point: TransitionPoint) {
    TRANSITION_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(point);
        }
    });
}

#[cfg(test)]
mod tests;
