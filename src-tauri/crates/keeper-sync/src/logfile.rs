//! The log file both hosts write, and the filter both hosts start from
//! (Epic 70, Story 70.7, AD-234).
//!
//! Two facts from hesperia's `keeper.log` on 2026-09-08 decided this module's
//! shape. **1.9 M of its 4.35 M lines were two gitoxide targets** —
//! `gix_attributes::search::attributes` warning once per path per lookup about
//! one mis-stored `.gitattributes` pointer (1 314 669 lines) and
//! `gix_worktree_state::checkout::chunk` erroring once per path about one
//! checkout into a non-empty directory (588 408 lines) — and the file had
//! **no rotation at all**: 895 MB live beside a 1.33 GB sibling, every line
//! written whether or not the owner had opted into debug logging, with one
//! `open(2)`/`close(2)` pair per line.
//!
//! Here rather than in either host because both hosts write a log and neither
//! can compile on the other's machine: the shell crate is macOS-only, the
//! daemon is Linux, and a rotation written twice is a rotation tested once.
//! This crate is `tauri`-free and `keeper-core`-free (AD-40), so both can
//! link it, and a `tempdir` test on the Linux box proves the byte boundary
//! for the Mac.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Bytes a log file may reach before [`RotatingFile`] moves it aside.
///
/// 64 MiB. Keeper's own lines are one `status walk finished` per walk and one
/// anomaly per hour per profile — a few MB a month — so this bound is reached
/// only by a defect class of the kind above, and 64 MiB of it is more than
/// any bug report needs and less than any laptop minds. With two generations
/// kept the disk cost tops out at 192 MiB, against the 2.2 GB measured.
pub const LOG_ROTATE_BYTES: u64 = 64 * 1024 * 1024;

/// How many rotated generations survive beside the live file.
///
/// Two: `keeper.log.1` is the one that just filled and `keeper.log.2` the one
/// before it, so a problem reported a day after it happened is still on disk.
/// A third is deleted at the rotation that would create it.
pub const LOG_GENERATIONS: u32 = 2;

/// The per-target directives every default filter carries.
///
/// The two targets that wrote 1.9 M lines are held to `error` (their `WARN`
/// is per path per lookup and says the same thing every time; an `ERROR` is
/// still one line a person needs), and `gix_dir`, the directory walker, to
/// `warn` — it emits an `INFO` per pruned directory on a walk that visits
/// 155 626 entries. `rmcp`, the MCP client, is off: at every level it may
/// write a message an MCP server sent, whole and unredacted, and keeper's
/// own lines say what a person needs of each server, redacted. Keeper's own
/// targets are untouched, and `RUST_LOG`, when set, replaces the rest of
/// the string ([`filter`]): a person debugging gix wants gix.
pub const LOG_TARGET_DIRECTIVES: &str =
    "gix_attributes=error,gix_worktree_state=error,gix_dir=warn,rmcp=off";

/// What `rmcp` is held to whatever a person asks for.
const RMCP_OFF: &str = "rmcp=off";

/// The default `EnvFilter` directive for a host whose base level is `level`.
///
/// Only what a host starts from when nobody said otherwise ([`filter`]).
/// Keeper's own crates read at `level`; the gitoxide targets above are held
/// down.
pub fn default_filter(level: &str) -> String {
    format!("{level},{LOG_TARGET_DIRECTIVES}")
}

/// The `EnvFilter` directives a host installs: `rust_log` (its `RUST_LOG`)
/// when it is set, else [`default_filter`] at `level`. Either way `rmcp`
/// stays off — a directive of `rust_log` naming it is dropped, and so is a
/// span directive whose target could be rmcp's, since a matched span turns
/// on every event inside it past any target's `off` — since verbosity a
/// person asks for is not leave to write what an MCP server sent,
/// unredacted, into a log.
pub fn filter(rust_log: Option<&str>, level: &str) -> String {
    let Some(asked) = rust_log.filter(|asked| !asked.trim().is_empty()) else {
        return default_filter(level);
    };
    let mut kept: Vec<&str> = asked
        .split(',')
        .filter(|directive| {
            let directive = directive.trim();
            let target = directive.split(['[', '=']).next().unwrap_or_default();
            // A directive's target matches every target it begins.
            let names_rmcp = target.starts_with("rmcp");
            let spans_rmcp = directive.contains('[') && "rmcp".starts_with(target);
            !names_rmcp && !spans_rmcp
        })
        .collect();
    kept.push(RMCP_OFF);
    kept.join(",")
}

/// One open handle on a log file that rotates itself past
/// [`LOG_ROTATE_BYTES`].
///
/// Held open across writes — `O_APPEND`, one `write_all` per event — where
/// the shell used to open and close the file per line. The size is tracked
/// in memory from the position the file was opened at plus what this handle
/// has written, so a write costs no `fstat`; a second writer on the same
/// file (the app and the daemon share nothing here, but a person's `tee`
/// might) merely rotates a little late.
///
/// # Rotation is delete, rename, rename — in that order
///
/// `keeper.log.2` is removed, `.1` becomes `.2`, the live file becomes `.1`
/// and a fresh live file is opened. The delete comes first because a rename
/// onto an existing name replaces it on Unix and fails on Windows; deleting
/// makes the two agree. Each step is best-effort and the next runs
/// regardless: a rename that fails leaves a bigger file, never a lost one,
/// and a log that could not rotate is still a log. A rename within one
/// directory is atomic on every filesystem keeper ships to, so no reader
/// ever sees a half-moved file.
///
/// # Failure is silence, never a panic
///
/// This sits under `tracing`, which is under everything. A full disk, a
/// vanished directory or a permissions change must not take the process
/// with it, so every error here is swallowed and the next write tries again.
pub struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    rotate_at: u64,
}

impl RotatingFile {
    /// Open (creating if needed) the log at `path`, rotating past
    /// [`LOG_ROTATE_BYTES`].
    pub fn open(path: PathBuf) -> Self {
        Self::open_rotating_at(path, LOG_ROTATE_BYTES)
    }

    /// [`Self::open`] with the boundary chosen by the caller.
    ///
    /// Public so a host can test rotation without writing 64 MiB; production
    /// callers use [`Self::open`].
    pub fn open_rotating_at(path: PathBuf, rotate_at: u64) -> Self {
        let mut this = Self {
            path,
            file: None,
            written: 0,
            rotate_at,
        };
        this.reopen();
        this
    }

    /// Where the live file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `n`th rotated generation beside the live file: `keeper.log.1`.
    pub fn generation(path: &Path, n: u32) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    fn reopen(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = self
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map_or(0, |meta| meta.len());
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = std::fs::remove_file(Self::generation(&self.path, LOG_GENERATIONS));
        for n in (1..LOG_GENERATIONS).rev() {
            let _ = std::fs::rename(
                Self::generation(&self.path, n),
                Self::generation(&self.path, n + 1),
            );
        }
        let _ = std::fs::rename(&self.path, Self::generation(&self.path, 1));
        self.reopen();
    }
}

impl Write for RotatingFile {
    /// Append `buf` whole, rotating first if the file would cross the
    /// boundary. `Ok(buf.len())` always, whatever the disk said — see the
    /// type's doc.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.file.is_none() {
            self.reopen();
        }
        if self.written > 0 && self.written.saturating_add(buf.len() as u64) > self.rotate_at {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(buf).is_ok() {
                self.written = self.written.saturating_add(buf.len() as u64);
            } else {
                // Reopen on the next write: the descriptor may be gone with
                // its directory, and a fresh open is the only repair.
                self.file = None;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

/// The longest panic message a log line carries. A message is whatever the
/// panicking code formatted; the location and thread are what a bug report
/// needs, and a runaway message must not fill the file. A message cut here
/// loses the token the cut went through (`bound_panic_message`).
pub const PANIC_MESSAGE_MAX: usize = 500;

/// What one panic's log line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicRecord {
    /// `file:line:column`, when the panic carries a location.
    pub location: Option<String>,
    /// The panicking thread's name, or `unnamed`.
    pub thread: String,
    /// The panic's message, bounded to [`PANIC_MESSAGE_MAX`] characters by
    /// `bound_panic_message`.
    pub message: String,
}

impl PanicRecord {
    /// The record of the panic `info` describes, on the current thread.
    pub fn of(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("a panic without a message");
        let bounded = bound_panic_message(message);
        Self {
            location: info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
            message: bounded,
        }
    }
}

/// `message` cut to at most [`PANIC_MESSAGE_MAX`] characters. The same line
/// is `keeper.log`'s and, through the shell's export layer, PostHog's, and
/// the redactor only ever sees what this leaves (R461, R511 P2-04): a cut
/// through a token — a hex or base64 secret, an identifier, a path — would
/// leave a fragment shorter than any class that redaction recognises. So a
/// cut message drops its trailing token whole, and the one the cut may have
/// ended at, since a cut on a boundary cannot be told from one inside a
/// token. keeper-core's `telemetry::redact::bound` applies the same rule to
/// the webview's reports; this crate is core-free (AD-40), hence the copy.
fn bound_panic_message(message: &str) -> String {
    if message.chars().count() <= PANIC_MESSAGE_MAX {
        return message.to_owned();
    }
    let cut: String = message.chars().take(PANIC_MESSAGE_MAX).collect();
    let kept = cut.trim_end_matches(|c: char| !c.is_whitespace());
    format!("{kept}…")
}

/// Install the process's panic hook (Epic 100, R381 as amended by R387,
/// 100.1 #1): every panic is written to the `tracing` log at `ERROR` with its
/// location and thread — so it lands in `keeper.log`, which records `WARN`
/// and above always — then the hook that was installed before this one runs,
/// so the default stderr report and any test harness still see it.
///
/// That line is all the hook writes. Where the shell's subscriber carries the
/// telemetry export layer, the line then leaves for PostHog like every other
/// `ERROR`, redacted, under diagnostics consent (R461, replacing R387's
/// "never counted"). The hook does nothing that could itself panic beyond
/// formatting the line — no lock it would wait on, no random number, no queue.
///
/// Here rather than in a host because both hosts could install it and
/// neither compiles on the other's machine (see this module's header).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let record = PanicRecord::of(info);
        tracing::error!(
            target: "keeper::panic",
            location = record.location.as_deref().unwrap_or("unknown"),
            thread = %record.thread,
            reason = %record.message,
            "keeper panicked"
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Past the boundary the live file is moved to `.1`, `.1` to `.2`, and
    /// the third generation is deleted — proven at a 1 000-byte boundary
    /// with 100-byte lines, so each generation is exactly ten lines and the
    /// arithmetic is the same as 64 MiB with less waiting.
    #[test]
    fn a_log_past_the_boundary_rotates_and_the_third_generation_is_deleted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("logs").join("keeper.log");
        let boundary = 1000u64;
        let mut log = RotatingFile::open_rotating_at(path.clone(), boundary);
        // Ten lines fill a generation exactly; the eleventh rotates. Each
        // batch is lettered so its journey through the generations can be
        // followed.
        let mut fill = |letter: u8| {
            for _ in 0..10 {
                log.write_all(&[letter; 100]).expect("infallible");
            }
        };

        fill(b'a');
        assert_eq!(std::fs::metadata(&path).expect("live").len(), 1000);
        assert!(
            !RotatingFile::generation(&path, 1).exists(),
            "nothing rotated yet"
        );

        fill(b'b');
        assert_eq!(
            std::fs::read(&path).expect("live"),
            vec![b'b'; 1000],
            "the first `b` crossed the boundary and opened a fresh file"
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'a'; 1000],
            "the full file moved aside whole"
        );

        fill(b'c');
        assert_eq!(std::fs::read(&path).expect("live"), vec![b'c'; 1000]);
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'b'; 1000]
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 2)).expect(".2"),
            vec![b'a'; 1000],
            "the oldest generation is the highest number"
        );

        fill(b'd');
        assert_eq!(std::fs::read(&path).expect("live"), vec![b'd'; 1000]);
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'c'; 1000]
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 2)).expect(".2"),
            vec![b'b'; 1000]
        );
        assert!(
            !RotatingFile::generation(&path, 3).exists(),
            "the `a` generation was deleted rather than becoming `.3`"
        );
    }

    /// Reopening an existing file counts its bytes, so a restart does not
    /// reset the boundary and let the file grow to twice the bound.
    #[test]
    fn an_existing_file_is_measured_on_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("keeper.log");
        std::fs::write(&path, vec![b'x'; 900]).expect("seed");
        let mut log = RotatingFile::open_rotating_at(path.clone(), 1000);
        log.write_all(&[b'y'; 200]).expect("infallible");
        assert_eq!(
            std::fs::read(&path).expect("live"),
            vec![b'y'; 200],
            "the write that would cross the bound rotated the seeded bytes out"
        );
        assert_eq!(
            std::fs::metadata(RotatingFile::generation(&path, 1))
                .expect("rotated")
                .len(),
            900
        );
    }

    /// A vanished directory is not a panic and not a lost log: once the
    /// descriptor is dropped, the next write recreates the directory and the
    /// file. (Unix keeps an unlinked file's descriptor writable, so the drop
    /// is what a failed write does in production — see `Write::write`.)
    #[test]
    fn a_removed_directory_is_recreated_on_the_next_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("gone").join("keeper.log");
        let mut log = RotatingFile::open(path.clone());
        log.write_all(b"one\n").expect("infallible");
        std::fs::remove_dir_all(dir.path().join("gone")).expect("remove");
        log.file = None;
        log.write_all(b"two\n").expect("infallible");
        assert_eq!(std::fs::read(&path).expect("the log came back"), b"two\n");
    }

    /// Whether `directives`, as a host installs them, let an event of
    /// `target` at `level` through.
    fn lets_through(directives: &str, target: &'static str, level: tracing::Level) -> bool {
        use tracing_subscriber::layer::SubscriberExt as _;

        let filter: tracing_subscriber::EnvFilter =
            directives.parse().expect("a valid directive set");
        let registry = tracing_subscriber::registry().with(filter);
        struct Callsite;
        impl tracing::callsite::Callsite for Callsite {
            fn set_interest(&self, _: tracing::subscriber::Interest) {}
            fn metadata(&self) -> &tracing::Metadata<'_> {
                unreachable!("only the identifier is used")
            }
        }
        static CALLSITE: Callsite = Callsite;
        let meta = tracing::Metadata::new(
            "event",
            target,
            level,
            None,
            None,
            None,
            tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&CALLSITE)),
            tracing::metadata::Kind::EVENT,
        );
        tracing::Subscriber::enabled(&registry, &meta)
    }

    /// The daemon's and the app's default filter holds the two chattering
    /// gitoxide targets down while keeper's own `INFO` still passes
    /// (Epic 70, F-db-8).
    #[test]
    fn the_default_filter_silences_gix_attributes_warnings_and_keeps_keepers_info() {
        let directives = default_filter("info");
        let enabled = |target, level| lets_through(&directives, target, level);
        assert!(
            !enabled("gix_attributes::search::attributes", tracing::Level::WARN),
            "the 1 314 669-line target is held to error"
        );
        assert!(enabled(
            "gix_attributes::search::attributes",
            tracing::Level::ERROR
        ));
        assert!(!enabled(
            "gix_worktree_state::checkout::chunk",
            tracing::Level::WARN
        ));
        assert!(!enabled("gix_dir::walk", tracing::Level::INFO));
        assert!(enabled("gix_dir::walk", tracing::Level::WARN));
        assert!(
            enabled("keeper_sync::engine", tracing::Level::INFO),
            "keeper's own lines are untouched"
        );
        assert!(!enabled("keeper_sync::engine", tracing::Level::DEBUG));
        // An MCP server's message, as rmcp would log it, reaches no log.
        assert!(!enabled("rmcp::service", tracing::Level::ERROR));
    }

    /// The targets of the two `INFO` events `directives` let through, both
    /// inside the span rmcp serves a connection in (`serve_inner`): rmcp's,
    /// as it logs a server's notification whole, and keeper's own.
    fn through_in_rmcps_span(directives: &str) -> Vec<String> {
        use tracing_subscriber::layer::SubscriberExt as _;

        #[derive(Clone, Default)]
        struct Seen(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Seen {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _: tracing_subscriber::layer::Context<'_, S>,
            ) {
                self.0
                    .lock()
                    .expect("seen")
                    .push(event.metadata().target().to_owned());
            }
        }
        let seen = Seen::default();
        let filter: tracing_subscriber::EnvFilter =
            directives.parse().expect("a valid directive set");
        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(seen.clone());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(target: "rmcp::service", "serve_inner");
            let _in = span.enter();
            tracing::info!(target: "rmcp::service", "a server's notification");
            tracing::info!(target: "keeper_agent::mcp", "keeper's own line");
        });
        let through = seen.0.lock().expect("seen").clone();
        through
    }

    /// R225, R238: a person's `RUST_LOG` replaces the default — gix at
    /// trace if they ask — but never lets `rmcp` write an MCP server's
    /// messages: not when it names `rmcp`, nor through a span directive
    /// that matches rmcp's own span.
    #[test]
    fn rust_log_never_turns_rmcp_on() {
        for asked in [
            "info",
            "trace",
            "rmcp=trace,info",
            "info,rmcp::service=info",
            "info,rmcp[serve_inner]=info",
            "info,[serve_inner]=trace",
            "info,rm[serve_inner]=trace",
        ] {
            let directives = filter(Some(asked), "warn");
            assert_eq!(
                through_in_rmcps_span(&directives),
                ["keeper_agent::mcp"],
                "{asked}: {directives}"
            );
        }
        assert!(lets_through(
            &filter(Some("gix_attributes=trace,info"), "warn"),
            "gix_attributes::search::attributes",
            tracing::Level::TRACE
        ));
    }

    /// Set in the child process [`a_panic_on_another_thread_is_an_error_line_with_where_and_which_thread`]
    /// re-executes: the directory its log and witness files go to.
    const PANIC_CHILD_DIR: &str = "KEEPER_PANIC_HOOK_CHILD_DIR";

    /// Where a test subscriber writes: a real rotating log file, read back.
    #[derive(Clone)]
    struct FileSink(std::sync::Arc<std::sync::Mutex<RotatingFile>>);
    impl Write for FileSink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("sink").write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.lock().expect("sink").flush()
        }
    }

    /// The child half: install a previous hook that leaves a witness file,
    /// then the panic hook, then panic on a named thread with the log going
    /// to a real rotating file. Its own process, so no other test's hook can
    /// see this panic or replace this hook.
    fn panic_in_this_process(dir: &Path) {
        let witness = dir.join("previous-hook-ran");
        std::panic::set_hook(Box::new(move |_| {
            let _ = std::fs::write(&witness, b"ran");
        }));
        install_panic_hook();
        let sink = FileSink(std::sync::Arc::new(std::sync::Mutex::new(
            RotatingFile::open(dir.join("keeper.log")),
        )));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("the child's only subscriber");
        let joined = std::thread::Builder::new()
            .name("sync-worker".to_owned())
            .spawn(|| panic!("the index is {} past its end", 3))
            .expect("spawn")
            .join();
        assert!(joined.is_err(), "the thread panicked");
    }

    /// R381 / 100.1 #1: a panic on a spawned thread is an `ERROR` line in the
    /// log file with its location, thread and message, and the hook installed
    /// before still runs. The panic happens in a re-executed copy of this
    /// test binary (R387, FIX 09): a panic hook is process-global, and the
    /// parallel test runner's other hook-replacing tests would race it here.
    #[test]
    fn a_panic_on_another_thread_is_an_error_line_with_where_and_which_thread() {
        if let Some(dir) = std::env::var_os(PANIC_CHILD_DIR) {
            panic_in_this_process(Path::new(&dir));
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let status = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            .args([
                "--exact",
                "logfile::tests::a_panic_on_another_thread_is_an_error_line_with_where_and_which_thread",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(PANIC_CHILD_DIR, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("the child runs");
        assert!(status.success(), "the child process: {status}");

        let log =
            std::fs::read_to_string(dir.path().join("keeper.log")).expect("the line is on disk");
        let line = log
            .lines()
            .find(|line| line.contains("keeper panicked"))
            .unwrap_or_else(|| panic!("no panic line in {log:?}"));
        assert!(line.contains("ERROR"), "{line}");
        assert!(line.contains("keeper::panic"), "{line}");
        assert!(line.contains("thread=sync-worker"), "{line}");
        assert!(line.contains("logfile.rs:"), "{line}");
        assert!(
            line.contains("reason=the index is 3 past its end"),
            "{line}"
        );
        assert_eq!(log.matches("keeper panicked").count(), 1, "one line: {log}");
        assert!(
            dir.path().join("previous-hook-ran").exists(),
            "the previous hook ran"
        );
    }
}
