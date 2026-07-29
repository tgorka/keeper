//! Tier 1 of the completeness gate — the event trigger (Story 26.2, AD-45).
//!
//! One [`FolderWatcher`] per profile watches that profile's root recursively
//! and emits coalesced [`WatchEvent`]s. It says *when* to look at a path; it
//! never says whether the path is finished. That judgement belongs entirely to
//! [`crate::stability`].
//!
//! # One watcher per profile, not per folder
//!
//! inotify instances are a scarce per-user resource: `max_user_instances` is
//! **128** on a typical host (`max_user_watches` is generous by comparison at
//! ~244 000). A watcher per synced *folder* would exhaust the instance limit
//! after 128 profiles' worth of subdirectories; a watcher per *profile*,
//! registered recursively, costs exactly one instance no matter how deep the
//! tree is.
//!
//! # Close-write is a Linux-only fast path, and never a proof
//!
//! On Linux, `EventKind::Access(AccessKind::Close(AccessMode::Write))` is
//! inotify's `IN_CLOSE_WRITE`: a writer that had the file open for writing let
//! go of it. That is enough to collapse the quiescence window to
//! [`crate::profile::CLOSE_WRITE_SETTLE_MS`], and not enough for anything else
//! — close-then-reopen is ordinary behaviour for git, `dd conv=notrunc`,
//! torrent clients and database WALs.
//!
//! **On macOS this event does not exist in any backend available to us.**
//! FSEvents has no notion of a close at all. kqueue's `EVFILT_VNODE` is
//! per-file precise but its flag set (`NOTE_DELETE`, `NOTE_WRITE`,
//! `NOTE_EXTEND`, `NOTE_ATTRIB`, `NOTE_LINK`, `NOTE_RENAME`, `NOTE_REVOKE`) has
//! no `NOTE_CLOSE*` — that is FreeBSD 11+, not XNU — and it costs **one file
//! descriptor per watched file**, which exhausts `RLIMIT_NOFILE` on a large
//! tree and blocks unmount while the descriptors are held. EndpointSecurity's
//! `ES_EVENT_TYPE_NOTIFY_CLOSE` is the only true signal and needs an
//! Apple-granted entitlement. So macOS keeps the FSEvents default and accepts
//! the full window.
//!
//! # Three mandatory companions
//!
//! A watcher on its own is wrong in three specific ways, each covered here:
//!
//! 1. **Events are silently dropped.** Linux raises `IN_Q_OVERFLOW` and macOS
//!    raises `MustScanSubDirs` when their queues overflow; both arrive as
//!    `notify`'s rescan flag, and both mean "you have missed changes". Covered
//!    by reacting to that flag *and* by an unconditional periodic full rescan,
//!    because a watcher that was never started, or a mount that emits no events
//!    at all (NFS, CIFS, some FUSE), raises no flag either.
//! 2. **Our own writes look like the user's.** A fetch that checks out 400
//!    files would re-commit all 400 as local changes. inotify has no
//!    self-filtering (FSEvents' `IgnoreSelf` only covers the same process, and
//!    not reliably), so the engine registers paths in an [`EchoSuppressor`]
//!    before writing and clears them after — Syncthing's `inProgress` set.
//! 3. **Some filesystems never notify.** Network mounts, FUSE and unowned
//!    macOS trees need [`notify::PollWatcher`], selected per profile through
//!    [`WatchConfig::force_poll`].

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify::{EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, new_debouncer_opt, DebounceEventResult, DebouncedEvent, Debouncer,
    RecommendedCache,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Result, SyncError};

/// Default debounce window. Comfortably below the shortest quiescence window
/// ([`crate::profile::CLOSE_WRITE_SETTLE_MS`], 1 s) so debouncing never becomes
/// the thing that decides when a file syncs — tier 2 must own that.
pub const DEFAULT_DEBOUNCE_MS: u64 = 500;

/// Default interval between unconditional full rescans.
///
/// This is the backstop for "the watcher told us nothing", and on most
/// platforms it is the *only* coverage: `Flag::Rescan` is raised from exactly
/// two places in `notify` — inotify's `IN_Q_OVERFLOW` and FSEvents'
/// `MustScanSubDirs`. Windows' `ReadDirectoryChanges`, kqueue and
/// `PollWatcher` never raise it, so on those backends a dropped or missed
/// change is invisible until this sweep runs. The same sweep covers a mount
/// that emits no events at all (NFS, CIFS, some FUSE) and a watcher that
/// failed to arm. Fifteen minutes bounds the worst-case detection latency
/// without making the rescan itself a load source on a 100 000-file profile.
pub const DEFAULT_RESCAN_INTERVAL_MS: u64 = 15 * 60 * 1_000;

/// Poll interval used when [`WatchConfig::force_poll`] is set.
///
/// A polling watcher re-`stat`s the entire tree on every tick, so it is
/// deliberately lazy: this is the fallback for filesystems that cannot notify
/// at all, not a general-purpose mode. Matches `notify`'s own default.
const POLL_INTERVAL_MS: u64 = 30_000;

/// One coalesced change notification.
///
/// A `path` equal to the profile root means **rescan the whole tree**: that is
/// how a queue overflow and the periodic sweep are expressed, and it needs no
/// special case in the consumer because walking a directory is what a scanner
/// does with a directory path anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    pub path: PathBuf,
    /// A Linux `IN_CLOSE_WRITE` was seen for this path in this batch. Shortens
    /// the quiescence window; never authorizes a commit by itself.
    pub close_write: bool,
}

/// Per-profile watcher tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchConfig {
    pub debounce_ms: u64,
    /// `0` disables the periodic sweep. Only sensible in tests: in production
    /// the sweep is what covers a dropped-event queue.
    pub rescan_interval_ms: u64,
    /// Use [`notify::PollWatcher`] instead of the platform backend. Required
    /// for network and FUSE mounts, and for macOS trees the process does not
    /// own.
    pub force_poll: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            rescan_interval_ms: DEFAULT_RESCAN_INTERVAL_MS,
            force_poll: false,
        }
    }
}

impl WatchConfig {
    /// Reject settings the debouncer would reject at runtime.
    ///
    /// `notify-debouncer-full` derives its tick rate as `timeout / 4` and
    /// returns a runtime error for a zero timeout, so catching it here turns a
    /// confusing "generic" error into a configuration message.
    pub fn validate(&self) -> Result<()> {
        if self.debounce_ms == 0 {
            return Err(SyncError::Config(
                "watch debounce must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

/// The set of paths the engine is currently writing itself.
///
/// Cloneable and shared: the engine registers before a checkout and clears
/// after, while the watcher thread reads it on every batch. Without this a
/// fetch that materializes 400 files produces 400 "local changes" and the next
/// push commits them straight back — a self-sustaining loop that looks exactly
/// like a busy sync.
///
/// Suppression is **subtree-wide**: registering a directory suppresses
/// everything written beneath it, which is what a checkout actually does.
#[derive(Debug, Clone, Default)]
pub struct EchoSuppressor {
    inner: Arc<Mutex<HashSet<PathBuf>>>,
}

impl EchoSuppressor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Suppress `path` and everything beneath it until [`Self::clear`].
    pub fn register(&self, path: impl Into<PathBuf>) {
        self.guard().insert(path.into());
    }

    /// Stop suppressing `path`. Clearing a path that was never registered is a
    /// no-op, so the engine's cleanup can be unconditional.
    pub fn clear(&self, path: &Path) {
        self.guard().remove(path);
    }

    /// Forget every registration. Used when a profile is torn down mid-write:
    /// a stale entry would silently mute a real user change forever.
    pub fn clear_all(&self) {
        self.guard().clear();
    }

    pub fn is_suppressed(&self, path: &Path) -> bool {
        let set = self.guard();
        if set.is_empty() {
            return false;
        }
        path.ancestors().any(|ancestor| set.contains(ancestor))
    }

    pub fn len(&self) -> usize {
        self.guard().len()
    }

    pub fn is_empty(&self) -> bool {
        self.guard().is_empty()
    }

    /// A poisoned lock still holds a perfectly usable set — the worst a panic
    /// mid-mutation can leave behind is a stale path — and propagating the
    /// poison would take down the watcher thread over nothing.
    fn guard(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// What one `notify` event means to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interest {
    /// Not a change we act on.
    Ignore,
    /// Something happened to this path; re-examine it.
    Touch,
    /// A writer closed the file. The tier-2 fast path.
    CloseWrite,
}

/// Classify one event kind.
fn classify(kind: &EventKind) -> Interest {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => Interest::CloseWrite,
        // Opens and reads are pure noise: on a tree being scanned by Spotlight
        // or a backup agent they arrive continuously and mean nothing.
        EventKind::Access(_) => Interest::Ignore,
        // An atime bump is what *reading* a file looks like. Acting on it would
        // make our own verify-on-read pass retrigger the gate it just passed.
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => Interest::Ignore,
        // Everything else — create, remove, rename, data and permission
        // changes, and the deliberately vague `Any`/`Other` that FSEvents
        // produces — is a reason to look again.
        _ => Interest::Touch,
    }
}

/// Collapses a burst into at most one event per path.
///
/// The debouncer already merges duplicates within its window, but a single
/// batch still routinely carries `Create`, several `Modify`s and a `Close` for
/// the same file. Emitting five events would run the stability gate five times
/// for one write.
#[derive(Debug, Default)]
struct Coalescer {
    /// path -> whether a close-write was seen for it in this batch.
    seen: HashMap<PathBuf, bool>,
}

impl Coalescer {
    fn push(&mut self, path: &Path, close_write: bool) {
        if let Some(flag) = self.seen.get_mut(path) {
            // Sticky: one close-write anywhere in the burst is enough, and a
            // later plain modify must not downgrade it.
            *flag |= close_write;
            return;
        }
        self.seen.insert(path.to_path_buf(), close_write);
    }

    /// Emit in path order so a batch is reproducible; arrival order inside a
    /// debounce window is not meaningful anyway.
    fn drain(self) -> Vec<WatchEvent> {
        let mut events: Vec<WatchEvent> = self
            .seen
            .into_iter()
            .map(|(path, close_write)| WatchEvent { path, close_write })
            .collect();
        events.sort_by(|a, b| a.path.cmp(&b.path));
        events
    }
}

/// Fold one debounced batch into the events it implies.
///
/// Pure: no clock, no filesystem, no channel. Everything interesting about the
/// watcher's behaviour is decided here, which is what makes it testable without
/// waiting for a real inotify queue.
fn fold_batch(
    events: &[DebouncedEvent],
    root: &Path,
    suppressor: &EchoSuppressor,
) -> Vec<WatchEvent> {
    let mut coalescer = Coalescer::default();
    let mut overflowed = false;

    for event in events {
        // `IN_Q_OVERFLOW` (Linux) and `MustScanSubDirs` (macOS) both surface
        // here: the kernel is telling us it dropped events, so nothing in this
        // batch can be trusted to be the whole story.
        //
        // Checked before the kind match and before touching `paths`, because
        // Linux's overflow event is `EventKind::Other` with an **empty** path
        // list. A handler that filtered on kind, or that iterated paths first,
        // would never see an overflow on Linux at all. The debouncer also
        // keeps only one rescan event and restarts its timer on each new one,
        // so a sustained storm arrives as a single flag — one more reason the
        // periodic sweep is not optional.
        if event.need_rescan() {
            overflowed = true;
        }
        let interest = classify(&event.kind);
        if interest == Interest::Ignore {
            continue;
        }
        for path in &event.paths {
            if suppressor.is_suppressed(path) {
                continue;
            }
            coalescer.push(path, interest == Interest::CloseWrite);
        }
    }

    if overflowed {
        tracing::debug!(root = %root.display(), "watch queue overflowed; requesting rescan");
        coalescer.push(root, false);
    }
    coalescer.drain()
}

/// The debouncer, in whichever flavour this profile asked for.
///
/// Two variants rather than a boxed trait object because `Debouncer` is generic
/// over its watcher and the poll fallback is a genuinely different type;
/// `Watcher` is not object-safe in a way that would help here.
enum Backend {
    Native(Debouncer<RecommendedWatcher, RecommendedCache>),
    Poll(Debouncer<PollWatcher, RecommendedCache>),
}

impl Backend {
    fn watch(&mut self, root: &Path) -> notify::Result<()> {
        match self {
            Self::Native(debouncer) => debouncer.watch(root, RecursiveMode::Recursive),
            Self::Poll(debouncer) => debouncer.watch(root, RecursiveMode::Recursive),
        }
    }

    /// Joins the debouncer's worker thread. `Debouncer`'s own `Drop` only sets
    /// a stop flag and returns immediately, so events can still be delivered
    /// for up to one tick after it — which is not what "stopped" should mean
    /// when a profile is being torn down.
    fn stop(self) {
        match self {
            Self::Native(debouncer) => debouncer.stop(),
            Self::Poll(debouncer) => debouncer.stop(),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Native(_) => "native",
            Self::Poll(_) => "poll",
        }
    }
}

/// A recursive watcher over one profile root.
pub struct FolderWatcher {
    root: PathBuf,
    backend: Option<Backend>,
    suppressor: EchoSuppressor,
    /// Dropping this sender wakes the rescan thread out of its timed wait, so
    /// teardown never has to outlive one rescan interval.
    rescan_stop: Option<std::sync::mpsc::Sender<()>>,
    rescan_thread: Option<std::thread::JoinHandle<()>>,
}

impl fmt::Debug for FolderWatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderWatcher")
            .field("root", &self.root)
            .field(
                "backend",
                &self.backend.as_ref().map_or("stopped", Backend::label),
            )
            .field("suppressed", &self.suppressor.len())
            .finish()
    }
}

impl FolderWatcher {
    /// Arm a recursive watch over `root`, emitting into `tx`.
    ///
    /// The channel is unbounded on purpose. The alternative — a bounded channel
    /// with `try_send` — would either block the debouncer's thread (which is
    /// how the kernel queue overflows in the first place, so the cure causes
    /// the disease) or silently drop a change and wait up to a full rescan
    /// interval to notice. Each batch is already coalesced to at most one event
    /// per changed path, and the consumer is our own supervisor.
    pub fn start(
        root: impl Into<PathBuf>,
        config: WatchConfig,
        tx: UnboundedSender<WatchEvent>,
    ) -> Result<Self> {
        config.validate()?;
        let root = root.into();
        let suppressor = EchoSuppressor::new();

        let handler = {
            let root = root.clone();
            let suppressor = suppressor.clone();
            let tx = tx.clone();
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    for event in fold_batch(&events, &root, &suppressor) {
                        if tx.send(event).is_err() {
                            // The supervisor is gone; the watcher is about to
                            // be torn down with it. Nothing to report.
                            return;
                        }
                    }
                }
                Err(errors) => {
                    for err in errors {
                        tracing::warn!(root = %root.display(), %err, "watch error");
                    }
                }
            }
        };

        let timeout = Duration::from_millis(config.debounce_ms);
        let mut backend = if config.force_poll {
            let notify_config = notify::Config::default()
                .with_poll_interval(Duration::from_millis(POLL_INTERVAL_MS))
                // Content comparison would read every byte of every file on
                // every tick. The completeness gate hashes files anyway, and
                // only the ones it is about to commit.
                .with_compare_contents(false);
            Backend::Poll(
                new_debouncer_opt::<_, PollWatcher, RecommendedCache>(
                    timeout,
                    None,
                    handler,
                    RecommendedCache::new(),
                    notify_config,
                )
                .map_err(|err| map_notify_error("start watcher", &root, err))?,
            )
        } else {
            Backend::Native(
                new_debouncer(timeout, None, handler)
                    .map_err(|err| map_notify_error("start watcher", &root, err))?,
            )
        };

        backend
            .watch(&root)
            .map_err(|err| map_notify_error("watch folder", &root, err))?;
        tracing::info!(
            root = %root.display(),
            backend = backend.label(),
            debounce_ms = config.debounce_ms,
            "folder watch armed"
        );

        let (rescan_stop, rescan_thread) = if config.rescan_interval_ms > 0 {
            let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
            let interval = Duration::from_millis(config.rescan_interval_ms);
            let rescan_root = root.clone();
            let handle = std::thread::Builder::new()
                .name("keeper-sync-rescan".to_owned())
                .spawn(move || loop {
                    match stop_rx.recv_timeout(interval) {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            let event = WatchEvent {
                                path: rescan_root.clone(),
                                close_write: false,
                            };
                            if tx.send(event).is_err() {
                                return;
                            }
                        }
                        // Either a stop was requested or the sender was
                        // dropped with the watcher. Both mean "go away".
                        _ => return,
                    }
                })
                .map_err(|err| SyncError::io("spawn rescan thread", &root, err))?;
            (Some(stop_tx), Some(handle))
        } else {
            (None, None)
        };

        Ok(Self {
            root,
            backend: Some(backend),
            suppressor,
            rescan_stop,
            rescan_thread,
        })
    }

    /// The watched root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A handle the engine registers its own writes with, before it makes them.
    pub fn suppressor(&self) -> EchoSuppressor {
        self.suppressor.clone()
    }

    /// Tear the watcher down and wait for both threads to finish.
    ///
    /// Deterministic by contract: after this returns, no further `WatchEvent`
    /// can be produced. `Drop` does the same thing, so a forgotten `stop` is a
    /// tidiness problem rather than a correctness one.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        // Dropping the sender is what wakes the rescan thread out of
        // `recv_timeout`; without it teardown would block for up to a full
        // rescan interval.
        drop(self.rescan_stop.take());
        if let Some(handle) = self.rescan_thread.take() {
            if handle.join().is_err() {
                tracing::warn!(root = %self.root.display(), "rescan thread panicked");
            }
        }
        if let Some(backend) = self.backend.take() {
            backend.stop();
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `Send` is load-bearing rather than incidental: since AD-34-11 the engine
/// holds one watcher per profile inside a `Mutex` on a value shared across tokio
/// tasks, which makes `Mutex<..>: Sync` — and therefore this — a requirement.
/// It comes from `notify`'s own `unsafe impl Send` for the platform backends, so
/// it is asserted here, where those types live, rather than discovered as a
/// baffling error about `Engine` after a dependency bump.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<FolderWatcher>();
};

/// Translate a `notify` failure into the engine's taxonomy.
///
/// `MaxFilesWatch` gets its own message because it is the one watcher failure a
/// user can actually fix, and the fix (a sysctl, or this profile's `forcePoll`)
/// is not guessable from "inotify error".
fn map_notify_error(operation: &'static str, path: &Path, err: notify::Error) -> SyncError {
    let rendered = err.to_string();
    match err.kind {
        notify::ErrorKind::MaxFilesWatch => SyncError::Config(format!(
            "{operation}: the inotify watch limit is exhausted for {} — raise \
             fs.inotify.max_user_watches (or max_user_instances), or enable \
             forcePoll for this profile",
            path.display()
        )),
        notify::ErrorKind::Io(source) => SyncError::io(operation, path, source),
        _ => SyncError::Config(format!(
            "{operation} failed for {}: {rendered}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, RemoveKind};
    use notify::Event;
    use std::time::Instant;

    fn debounced(kind: EventKind, paths: &[&str]) -> DebouncedEvent {
        let mut event = Event::new(kind);
        for path in paths {
            event = event.add_path(PathBuf::from(path));
        }
        DebouncedEvent::new(event, Instant::now())
    }

    const CLOSE_WRITE: EventKind = EventKind::Access(AccessKind::Close(AccessMode::Write));
    const CONTENT: EventKind = EventKind::Modify(ModifyKind::Data(DataChange::Content));

    #[test]
    fn the_echo_suppressor_suppresses_a_registered_path_and_stops_after_clearing() {
        let s = EchoSuppressor::new();
        let path = Path::new("/w/notes.md");
        assert!(!s.is_suppressed(path), "nothing is suppressed by default");

        s.register(path);
        assert!(s.is_suppressed(path));
        assert_eq!(s.len(), 1);
        // An unrelated sibling must still get through, or a checkout would mute
        // every concurrent user edit in the profile.
        assert!(!s.is_suppressed(Path::new("/w/other.md")));

        s.clear(path);
        assert!(!s.is_suppressed(path));
        assert!(s.is_empty());
    }

    #[test]
    fn the_echo_suppressor_covers_a_registered_subtree() {
        let s = EchoSuppressor::new();
        // A checkout registers the directory it is about to materialize; every
        // file it writes underneath must be muted, not just the directory.
        s.register("/w/docs");
        assert!(s.is_suppressed(Path::new("/w/docs")));
        assert!(s.is_suppressed(Path::new("/w/docs/a/b/c.md")));
        assert!(!s.is_suppressed(Path::new("/w/docsy/c.md")));
        assert!(!s.is_suppressed(Path::new("/w")));

        s.clear_all();
        assert!(!s.is_suppressed(Path::new("/w/docs/a/b/c.md")));
    }

    #[test]
    fn clearing_an_unregistered_path_is_a_no_op() {
        let s = EchoSuppressor::new();
        s.register("/w/a");
        s.clear(Path::new("/w/never-registered"));
        assert!(
            s.is_suppressed(Path::new("/w/a")),
            "unrelated clear must not disturb the set"
        );
    }

    #[test]
    fn a_close_write_event_maps_to_close_write_and_an_ordinary_modify_does_not() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();

        let events = fold_batch(&[debounced(CLOSE_WRITE, &["/w/a.txt"])], root, &s);
        assert_eq!(
            events,
            vec![WatchEvent {
                path: PathBuf::from("/w/a.txt"),
                close_write: true,
            }]
        );

        let events = fold_batch(&[debounced(CONTENT, &["/w/a.txt"])], root, &s);
        assert_eq!(
            events,
            vec![WatchEvent {
                path: PathBuf::from("/w/a.txt"),
                close_write: false,
            }]
        );
    }

    #[test]
    fn a_burst_for_one_path_collapses_into_a_single_event() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        // A single `cp` produces exactly this shape: create, several writes,
        // then the close. Five events would run the stability gate five times.
        let batch = [
            debounced(EventKind::Create(CreateKind::File), &["/w/a.txt"]),
            debounced(CONTENT, &["/w/a.txt"]),
            debounced(CONTENT, &["/w/a.txt"]),
            debounced(CLOSE_WRITE, &["/w/a.txt"]),
        ];
        let events = fold_batch(&batch, root, &s);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].path, PathBuf::from("/w/a.txt"));
        assert!(
            events[0].close_write,
            "one close-write in the burst must survive the collapse"
        );
    }

    #[test]
    fn a_later_plain_modify_does_not_downgrade_an_earlier_close_write() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        let batch = [
            debounced(CLOSE_WRITE, &["/w/a.txt"]),
            debounced(CONTENT, &["/w/a.txt"]),
        ];
        let events = fold_batch(&batch, root, &s);
        assert_eq!(events.len(), 1);
        assert!(events[0].close_write);
    }

    #[test]
    fn distinct_paths_are_kept_apart_and_emitted_in_a_stable_order() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        let batch = [
            debounced(CONTENT, &["/w/z.txt", "/w/a.txt"]),
            debounced(CLOSE_WRITE, &["/w/m.txt"]),
        ];
        let events = fold_batch(&batch, root, &s);
        let paths: Vec<_> = events.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/w/a.txt"),
                PathBuf::from("/w/m.txt"),
                PathBuf::from("/w/z.txt"),
            ]
        );
        assert!(events[1].close_write, "only m.txt was closed for writing");
        assert!(!events[0].close_write);
    }

    #[test]
    fn suppressed_paths_never_reach_the_channel() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        s.register("/w/checkout");
        let batch = [
            debounced(CONTENT, &["/w/checkout/a.txt"]),
            debounced(CLOSE_WRITE, &["/w/checkout/b/c.txt"]),
            debounced(CONTENT, &["/w/mine.txt"]),
        ];
        let events = fold_batch(&batch, root, &s);
        assert_eq!(
            events,
            vec![WatchEvent {
                path: PathBuf::from("/w/mine.txt"),
                close_write: false,
            }],
            "our own checkout writes must not become commits"
        );

        // ...and once the checkout is done, the same events flow again.
        s.clear(Path::new("/w/checkout"));
        assert_eq!(fold_batch(&batch, root, &s).len(), 3);
    }

    #[test]
    fn a_dropped_event_queue_requests_a_full_rescan_of_the_root() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();

        // The exact shape Linux produces (`inotify.rs`: `IN_Q_OVERFLOW` =>
        // `Event::new(EventKind::Other).set_flag(Flag::Rescan)`): kind `Other`
        // and **no paths at all**. Gating the rescan check behind a path
        // filter or a kind match would silently discard it, which is the whole
        // bug this test exists to prevent.
        let overflow = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert!(
            overflow.paths.is_empty(),
            "the Linux overflow event is pathless"
        );
        let batch = [DebouncedEvent::new(overflow, Instant::now())];
        assert_eq!(
            fold_batch(&batch, root, &s),
            vec![WatchEvent {
                path: PathBuf::from("/w"),
                close_write: false,
            }],
            "an overflow must escalate to a rescan of the root"
        );

        // macOS `MustScanSubDirs` carries one path — the fsevent batch
        // directory — alongside the flag. Both the path and the root must be
        // scanned; the root subsumes it, but dropping either would be wrong.
        let must_scan = Event::new(EventKind::Other)
            .set_flag(notify::event::Flag::Rescan)
            .add_path(PathBuf::from("/w/deep"));
        let batch = [DebouncedEvent::new(must_scan, Instant::now())];
        let events = fold_batch(&batch, root, &s);
        assert!(events.iter().any(|e| e.path == Path::new("/w")));
        assert!(events.iter().any(|e| e.path == Path::new("/w/deep")));
    }

    #[test]
    fn reads_and_atime_bumps_are_ignored() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        let batch = [
            debounced(EventKind::Access(AccessKind::Read), &["/w/a.txt"]),
            debounced(
                EventKind::Access(AccessKind::Open(AccessMode::Read)),
                &["/w/a.txt"],
            ),
            debounced(
                EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
                &["/w/a.txt"],
            ),
        ];
        assert!(
            fold_batch(&batch, root, &s).is_empty(),
            "reading a file is not a change; our own verify-on-read pass would \
             otherwise retrigger the gate it just passed"
        );
    }

    #[test]
    fn removals_and_renames_are_changes() {
        let root = Path::new("/w");
        let s = EchoSuppressor::new();
        // A delete has to propagate, and a rename arrives as two paths.
        let batch = [
            debounced(EventKind::Remove(RemoveKind::File), &["/w/gone.txt"]),
            debounced(
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
                &["/w/from.txt", "/w/to.txt"],
            ),
        ];
        let events = fold_batch(&batch, root, &s);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|e| !e.close_write));
    }

    #[test]
    fn classify_covers_every_kind_we_depend_on() {
        assert_eq!(classify(&CLOSE_WRITE), Interest::CloseWrite);
        assert_eq!(classify(&CONTENT), Interest::Touch);
        // A close after *reading* is not the fast path — that distinction is
        // the whole value of the variant.
        assert_eq!(
            classify(&EventKind::Access(AccessKind::Close(AccessMode::Read))),
            Interest::Ignore
        );
        // FSEvents' deliberately vague kinds must not be discarded, or macOS
        // would sync nothing at all.
        assert_eq!(classify(&EventKind::Any), Interest::Touch);
        assert_eq!(classify(&EventKind::Other), Interest::Touch);
    }

    #[test]
    fn a_zero_debounce_is_rejected_before_the_debouncer_sees_it() {
        let config = WatchConfig {
            debounce_ms: 0,
            ..WatchConfig::default()
        };
        let err = config
            .validate()
            .expect_err("a zero timeout makes the debouncer's timeout/4 tick rate invalid");
        assert!(matches!(err, SyncError::Config(_)), "got {err:?}");

        assert!(WatchConfig::default().validate().is_ok());
        assert_eq!(WatchConfig::default().debounce_ms, DEFAULT_DEBOUNCE_MS);
        // Debouncing must never be the thing that decides when a file syncs —
        // the quiescence gate owns that. A const assertion keeps the two in
        // order even if someone retunes one of them.
        const _: () = assert!(DEFAULT_DEBOUNCE_MS < crate::profile::CLOSE_WRITE_SETTLE_MS);
    }

    #[test]
    fn start_arms_a_real_watcher_and_stop_tears_it_down() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        // The rescan sweep is disabled so this test owns no timers: it proves
        // the backend arms and tears down, not that events arrive.
        let config = WatchConfig {
            debounce_ms: 50,
            rescan_interval_ms: 0,
            force_poll: false,
        };
        let watcher = FolderWatcher::start(dir.path(), config, tx).expect("watcher arms");
        assert_eq!(watcher.root(), dir.path());
        assert!(watcher.suppressor().is_empty());
        watcher.stop();
    }

    #[test]
    fn the_poll_backend_arms_for_filesystems_that_cannot_notify() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let config = WatchConfig {
            debounce_ms: 50,
            rescan_interval_ms: 0,
            force_poll: true,
        };
        let watcher = FolderWatcher::start(dir.path(), config, tx).expect("poll watcher arms");
        assert!(format!("{watcher:?}").contains("poll"));
        watcher.stop();
    }

    #[test]
    fn the_suppressor_handle_is_shared_with_the_watcher_thread() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            dir.path(),
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx,
        )
        .expect("watcher arms");

        // The engine registers through its own handle; the watcher must see it,
        // otherwise self-echo suppression silently does nothing.
        let handle = watcher.suppressor();
        handle.register(dir.path().join("staged"));
        assert_eq!(watcher.suppressor().len(), 1);
        assert!(watcher
            .suppressor()
            .is_suppressed(&dir.path().join("staged/deep/file.bin")));
        watcher.stop();
    }
}
