//! The `tracing` layer that hands every WARN and ERROR event to
//! [`Telemetry`] (R461). The shell builds its one subscriber with
//! [`log_subscriber`] (`debug_log::init`) before [`Telemetry`] exists, and
//! [`LogExport::attach`]es the layer once it does.
//!
//! The layer has its own level policy ([`export_filter`]: WARN and above on
//! every target), independent of the `keeper.log` writer's `EnvFilter` and of
//! `RUST_LOG` (R495 08, R498). Only `rmcp` is held off, as in `keeper.log`: at
//! every level it may carry what an MCP server sent, whole.
//!
//! It never blocks and never recurses: consent is an atomic read, the queue a
//! `try_lock`, and an event raised while this thread is already forwarding
//! one, or on the exporter's own threads ([`Transport`]: its HTTP requests
//! and every task and blocking call they spawn), or by this module, is not
//! forwarded. Anything else noisy is bounded by the per-fingerprint budget,
//! not excluded.

use std::cell::Cell;
use std::fmt::{self, Write as _};
use std::future::Future;
use std::sync::{Arc, OnceLock, Weak};

use tracing::field::{Field, Visit};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::EnvFilter;

use super::{is_agent_target, redact, LogSite, Telemetry, FRONTEND_TARGET};

/// Targets never forwarded, each with its reason: this module (the
/// exporter's own lines), and the webview's reports (`keeper::frontend`),
/// which leave as their own record.
const NEVER_FORWARDED: &[&str] = &["keeper_core::telemetry", FRONTEND_TARGET];

fn never_forwarded(target: &str) -> bool {
    NEVER_FORWARDED.iter().any(|prefix| {
        target
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    })
}

thread_local! {
    /// Whether this thread is inside [`LogExport::on_event`] already.
    static FORWARDING: Cell<bool> = const { Cell::new(false) };
    /// Whether this thread is one of the exporter's own ([`Transport`]).
    static EXPORTER_THREAD: Cell<bool> = const { Cell::new(false) };
}

fn inside_export() -> bool {
    EXPORTER_THREAD.with(Cell::get)
}

/// The exporter's own dispatch: a runtime of one worker thread, created on
/// the first request, on which every export and flags request runs. Its
/// threads — the worker and every blocking thread it starts (DNS) — are
/// marked as the exporter's, so whatever the HTTP stack logs there is the
/// export's own failure and never an export (R498, R511 P2-05). A marker
/// on the request's future would not reach what the client spawns:
/// reqwest runs hyper-util's `TokioExecutor`, whose `tokio::spawn` puts
/// each HTTP/2 connection's dispatcher on its own task — here, on this
/// runtime, so on a marked thread. Requests on any other runtime (a sync,
/// a forge) are unmarked, and their warnings are problems like any other.
pub(super) struct Transport {
    runtime: Option<tokio::runtime::Runtime>,
}

impl Transport {
    pub(super) fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(2)
            .thread_name("keeper-telemetry")
            .on_thread_start(|| EXPORTER_THREAD.with(|marked| marked.set(true)))
            .enable_all()
            .build()
            .ok();
        Self { runtime }
    }

    /// Run `work` on the exporter's runtime. Dropping the returned future
    /// (a revocation, a timeout) aborts the work and so its requests;
    /// `None` when there is no runtime or the work did not finish.
    pub(super) async fn run<F>(&self, work: F) -> Option<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let mut task = AbortOnDrop(self.runtime.as_ref()?.spawn(work));
        (&mut task.0).await.ok()
    }
}

impl Drop for Transport {
    /// Never blocks, so the last [`Telemetry`] may go on a runtime thread.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Clears [`FORWARDING`] on the way out, a panic included.
struct Forwarding;

impl Drop for Forwarding {
    fn drop(&mut self) {
        FORWARDING.with(|forwarding| forwarding.set(false));
    }
}

/// The export layer's own level policy: WARN and above on every target, and
/// nothing from `rmcp`, whatever `RUST_LOG` or `keeper.log`'s filter says.
pub fn export_filter() -> Targets {
    Targets::new()
        .with_default(LevelFilter::WARN)
        .with_target("rmcp", LevelFilter::OFF)
}

/// The shell's one subscriber (`debug_log::init`): `keeper.log`'s writer
/// under `filter` (the `EnvFilter` it always had) and the export layer under
/// [`export_filter`] — each filter its own layer's, so neither narrows the
/// other.
pub fn log_subscriber<W>(
    filter: EnvFilter,
    writer: W,
    export: LogExport,
) -> impl tracing::Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::Layer as _;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(filter),
        )
        .with(export.with_filter(export_filter()))
}

/// The layer. Cloning shares the attachment, so the shell keeps one handle to
/// attach later while the subscriber owns another.
#[derive(Clone, Default)]
pub struct LogExport {
    sink: Arc<OnceLock<Weak<Telemetry>>>,
}

impl LogExport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Point the layer at the process's telemetry. The first call wins; until
    /// it, events are not forwarded.
    pub fn attach(&self, telemetry: &Arc<Telemetry>) {
        let _ = self.sink.set(Arc::downgrade(telemetry));
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogExport {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        if *meta.level() > tracing::Level::WARN {
            return;
        }
        let Some(telemetry) = self.sink.get().and_then(Weak::upgrade) else {
            return;
        };
        if !telemetry.wants_problems() || inside_export() {
            return;
        }
        if FORWARDING.with(|forwarding| forwarding.replace(true)) {
            return;
        }
        let _forwarding = Forwarding;
        let mut line = Line::default();
        // An agent line's fields are never even formatted (R461).
        if !is_agent_target(meta.target()) {
            event.record(&mut line);
        }
        // A `log`-crate record arrives under target `log` with its real
        // target and callsite as fields.
        let target = line.log_target.as_deref().unwrap_or(meta.target());
        if never_forwarded(target) {
            return;
        }
        let site = LogSite {
            level: *meta.level(),
            target,
            file: line.log_file.as_deref().or(meta.file()),
            line: line.log_line.or(meta.line()),
        };
        let Line {
            message, fields, ..
        } = line;
        // Each part is redacted already; only the cap is left (R495 04).
        telemetry.capture_line(site, move || redact::cap(format!("{message}{fields}")));
    }
}

/// An event formatted the way `keeper.log` shows it — the message, then
/// ` name=value` per field — with each part redacted from its raw value
/// before anything quotes it (R495 01).
#[derive(Default)]
struct Line {
    message: String,
    fields: String,
    log_target: Option<String>,
    log_file: Option<String>,
    log_line: Option<u32>,
}

impl Visit for Line {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message.push_str(&redact::redact_uncapped(value)),
            "log.target" => self.log_target = Some(value.to_owned()),
            "log.file" => self.log_file = Some(value.to_owned()),
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(
                    self.fields,
                    " {name}={:?}",
                    redact::redact_field(name, value)
                );
            }
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "log.line" {
            self.log_line = u32::try_from(value).ok();
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => self
                .message
                .push_str(&redact::redact_uncapped(&format!("{value:?}"))),
            name if name.starts_with("log.") => {}
            name => {
                let raw = format!("{value:?}");
                let _ = write!(self.fields, " {name}={}", redact::redact_field(name, &raw));
            }
        }
    }
}
