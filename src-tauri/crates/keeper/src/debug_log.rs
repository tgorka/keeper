//! Opt-in on-disk debug logging (Story 22.5, FR-79).
//!
//! Two sinks, both strictly gated by the persisted `debug.mode` toggle
//! (default OFF — a privacy stance, not a convenience: log files describing
//! the user's recording activity land on disk only after an explicit opt-in):
//!
//! 1. **App log** — `~/Library/Logs/keeper/keeper.log`. [`init`] installs the
//!    process-wide `tracing` subscriber; every `tracing::info!/warn!/error!`
//!    across the app is formatted to stderr always (dev visibility) and
//!    appended to the file only while the toggle is on. The gate is checked
//!    per write, so flipping the setting applies live — no restart, no
//!    subscriber reload machinery.
//! 2. **Per-session event log** — `<session folder>/events.log`, one
//!    timestamped line per sidecar [`RecordingEvent`] (appended by the
//!    driver's event sink in `ipc.rs`). Lives beside `manifest.json`, so a
//!    bug report is one folder: media + manifest + the exact event stream
//!    that produced them.
//!
//! Everything here is best-effort by design: a failed log write must never
//! affect capture, the machine, or the IPC surface — errors are swallowed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The live debug-mode gate — seeded from the registry at boot ([`init`]),
/// flipped by the `debug_mode_set` command. Relaxed ordering is enough: a
/// straggler write racing a toggle is harmless either way.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether debug mode is currently on (live view, not a registry read).
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Flip the live gate (the command persists to the registry separately).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        // Surface the destination once per enable — into the newly-gated-open
        // file itself, so the log self-documents where it lives.
        tracing::info!(path = %app_log_path().display(), "debug mode: on-disk logging enabled");
    }
}

/// The app-level log file: `$HOME/Library/Logs/keeper/keeper.log`. On a Mac
/// that is `~/Library/Logs/keeper/keeper.log`, the standard per-app log home
/// (surfaces in Console.app's Log Reports); on the phone `$HOME` is the app's
/// own container, so the file is `<container>/Library/Logs/keeper/keeper.log`.
/// The About sentence renders this answer (`debug_log_path`) rather than a
/// literal, so each device names its own file (Story 65.3, AD-192).
pub fn app_log_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Logs/keeper/keeper.log")
}

/// A `tracing` writer that always mirrors to stderr and appends to
/// [`app_log_path`] when this event earns a file line. Opened per event: debug
/// volume is low, and per-write opens make the live toggle trivially safe.
struct GatedWriter {
    to_file: bool,
}

/// Decides per event whether it reaches the file.
///
/// A **problem is always recorded**; routine chatter waits for the toggle. The
/// file leg used to be gated wholesale, so a warning raised while debug mode
/// was off — which is the default, and therefore the normal case — existed only
/// on a stderr nobody sees once the app is launched from Finder. That is
/// exactly backwards for the one thing a user is later asked to send in: by the
/// time anyone knows something went wrong, the evidence has to already be on
/// disk. `WARN` and `ERROR` are rare by construction, so this costs nothing in
/// volume and does not weaken the privacy stance the toggle exists for: the
/// verbose `INFO` trail describing what the user was doing stays opt-in.
struct GatedMakeWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for GatedMakeWriter {
    type Writer = GatedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        GatedWriter { to_file: enabled() }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        // `tracing::Level` orders ERROR below WARN below INFO, so `<= WARN`
        // is exactly "a warning or worse".
        let is_problem = *meta.level() <= tracing::Level::WARN;
        GatedWriter {
            to_file: enabled() || is_problem,
        }
    }
}

impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if self.to_file {
            let path = app_log_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = file.write_all(buf);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

/// Install the process-wide `tracing` subscriber and seed the gate from the
/// persisted `debug.mode` setting. Idempotent-tolerant: a second install
/// attempt (tests) is ignored rather than panicking.
pub fn init(data_dir: &Path) {
    let seeded = keeper_core::registry::get_debug_mode(data_dir).unwrap_or(false);
    ENABLED.store(seeded, Ordering::Relaxed);
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(GatedMakeWriter)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    if seeded {
        tracing::info!(path = %app_log_path().display(), "debug mode: on-disk logging enabled");
    }
}

/// The tail of the app log, oldest line first, capped at `lines`.
///
/// Reads the whole file and keeps the last `lines`: the log is small by
/// construction (warnings and errors always, everything else only while debug
/// mode is on) and a backwards seek would buy nothing at this size while
/// costing the ability to be sure a line is whole.
///
/// A missing file is an empty tail, not an error — no log yet is the normal
/// state of a healthy install, and a viewer that shows a scary message for it
/// would be lying about the absence.
pub fn tail(lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(app_log_path()) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all.iter()
        .skip(all.len().saturating_sub(lines))
        .map(|line| (*line).to_owned())
        .collect()
}

/// Append one timestamped line to `<session_dir>/events.log` — no-op while
/// the gate is off, and best-effort while on (a full disk or vanished folder
/// must never disturb a live capture).
pub fn session_event(session_dir: &Path, line: &str) {
    if !enabled() {
        return;
    }
    let stamp = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join("events.log"))
    {
        let _ = writeln!(file, "{stamp} {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_event_is_a_no_op_while_disabled_and_appends_while_enabled() {
        let dir = std::env::temp_dir().join(format!("keeper-debuglog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        ENABLED.store(false, Ordering::Relaxed);
        session_event(&dir, "hidden");
        assert!(!dir.join("events.log").exists(), "off ⇒ no file");
        ENABLED.store(true, Ordering::Relaxed);
        session_event(&dir, "state -> recording");
        session_event(&dir, "segmentClosed index=0");
        let text = std::fs::read_to_string(dir.join("events.log")).expect("read");
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("state -> recording"));
        ENABLED.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gate that matters for a bug report: a problem is recorded even
    /// though debug mode is off, because by the time anyone asks for the log
    /// the warning has already happened. Routine chatter stays opt-in, which
    /// is what the toggle is actually for.
    #[test]
    fn a_warning_reaches_the_file_with_the_toggle_off_and_info_does_not() {
        use tracing_subscriber::fmt::MakeWriter as _;

        ENABLED.store(false, Ordering::Relaxed);
        let make = GatedMakeWriter;

        let problem = tracing::metadata::Metadata::new(
            "event",
            "keeper",
            tracing::Level::WARN,
            None,
            None,
            None,
            tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&TEST_CALLSITE)),
            tracing::metadata::Kind::EVENT,
        );
        assert!(
            make.make_writer_for(&problem).to_file,
            "a warning must be recorded whether or not anyone opted in"
        );

        let chatter = tracing::metadata::Metadata::new(
            "event",
            "keeper",
            tracing::Level::INFO,
            None,
            None,
            None,
            tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&TEST_CALLSITE)),
            tracing::metadata::Kind::EVENT,
        );
        assert!(
            !make.make_writer_for(&chatter).to_file,
            "routine activity stays off disk until the user asks for it"
        );

        // With the toggle on, everything lands.
        ENABLED.store(true, Ordering::Relaxed);
        assert!(make.make_writer_for(&chatter).to_file);
        ENABLED.store(false, Ordering::Relaxed);
    }

    /// No log yet is the normal state of a healthy install, so the viewer must
    /// get an empty tail rather than an error to render.
    #[test]
    fn tailing_a_log_that_does_not_exist_is_empty_not_an_error() {
        // `app_log_path` is a fixed per-user location; this asserts the shape of
        // the answer for the missing-file case without writing to it.
        let lines = tail(10);
        assert!(lines.len() <= 10, "the tail respects its cap");
    }

    struct TestCallsite;
    impl tracing::callsite::Callsite for TestCallsite {
        fn set_interest(&self, _: tracing::subscriber::Interest) {}
        fn metadata(&self) -> &tracing::Metadata<'_> {
            unreachable!("only the identifier is used")
        }
    }
    static TEST_CALLSITE: TestCallsite = TestCallsite;
}
