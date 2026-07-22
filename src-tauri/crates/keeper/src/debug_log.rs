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

/// The app-level log file: `~/Library/Logs/keeper/keeper.log` — the standard
/// macOS per-app log home (surfaces in Console.app's Log Reports).
pub fn app_log_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Logs/keeper/keeper.log")
}

/// A `tracing` writer that always mirrors to stderr and, while the gate is
/// on, appends to [`app_log_path`]. Opened per event: debug volume is low,
/// and per-write opens make the live toggle trivially safe.
struct GatedWriter;

impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if enabled() {
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
        .with_writer(|| GatedWriter)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    if seeded {
        tracing::info!(path = %app_log_path().display(), "debug mode: on-disk logging enabled");
    }
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
}
