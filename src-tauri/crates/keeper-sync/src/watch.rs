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
//!    files delivers 400 events, and inotify has no self-filtering (FSEvents'
//!    `IgnoreSelf` only covers the same process, and not reliably). This
//!    module ships Syncthing's `inProgress` answer — [`EchoSuppressor`],
//!    honoured by `fold_batch` — but **the engine deliberately does not use
//!    it** (Story 34.9). An event is never on its own a reason to commit
//!    anything: it only tells the supervisor to look, and `git status` then
//!    decides what actually changed. A checkout leaves the tree clean against
//!    the new HEAD, so its echo costs at most one fruitless walk and cannot
//!    become a re-commit loop. Registration would buy that walk back at the
//!    price of a register/clear pair around every write the engine makes, any
//!    one of which — forgotten on an error path, or skipped by a `?` — would
//!    mute a real user change indefinitely. That trades a performance cost for
//!    a silent data-loss one, so it was declined.
//!
//!    Consequently [`EchoSuppressor`] has **no production registrar**:
//!    `Engine::fold_watch_events` never takes the
//!    [`FolderWatcher::suppressor`] handle, so the suppression branch in
//!    `fold_batch` cannot fire outside this module's own tests. It is a
//!    working helper with unit coverage and no live caller. Do not read a
//!    green test over it as coverage of the shipped engine, and do not wire it
//!    up without revisiting the reasoning above.
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
    new_debouncer_opt, DebounceEventResult, DebouncedEvent, Debouncer, NoCache,
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

/// How long a retirement may run before it is worth complaining about.
///
/// A healthy teardown costs at most one debouncer tick, so anything past this
/// is a backend that is stuck rather than one that is finishing. It is only a
/// log threshold: nothing waits on it, because nothing waits on a retirement at
/// all (see [`retire`]).
const RETIRE_WARN_AFTER: Duration = Duration::from_secs(5);

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

/// A set of paths a caller is writing itself, folded out of every batch.
///
/// Cloneable and shared: a registrar would register before a checkout and
/// clear after, while the watcher thread reads it on every batch. It is
/// Syncthing's `inProgress` set, and it works — but **nothing in this crate
/// registers with it**. `Engine::fold_watch_events` never takes the
/// [`FolderWatcher::suppressor`] handle, because an event there is only a
/// prompt to run `git status` and a checkout leaves the tree clean against the
/// new HEAD, so the echo costs one fruitless walk rather than the re-commit
/// loop this type was written to prevent. See the module documentation for the
/// full reasoning, and treat every `EchoSuppressor` test as a test of this
/// type alone.
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
    /// no-op, so a registrar's cleanup can be unconditional.
    pub fn clear(&self, path: &Path) {
        self.guard().remove(path);
    }

    /// Forget every registration. For a registrar tearing a profile down
    /// mid-write: a stale entry would silently mute a real user change
    /// forever.
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
/// Pure: no clock, no filesystem, no channel. Everything about *what an event
/// means* is decided here, which is what makes it testable without waiting for
/// a real inotify queue.
///
/// What an event is *worth* is not decided here. Tier 0 lives on the profile's
/// [`crate::stability::StabilityGate`], with the per-profile patterns the user
/// added, and `Engine::fold_watch_events` puts every drained path to it before
/// letting it arm a walk (Story 34.9). Do not add a second exclusion filter
/// here: this function has no profile and no `ExcludeSet`, so it could only
/// consult a hard-coded copy of the corpus, and a copy that drifts from the
/// gate's is a wake and a walk that disagree about what is worth looking at.
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
///
/// # `NoCache`, and never `RecommendedCache`
///
/// The file-id cache exists to correlate a remove with a create into a rename.
/// Nothing here reads that: [`fold_batch`] collapses every event to "this path
/// changed, and possibly closed", and the supervisor answers it with a walk. So
/// the cache buys this crate nothing — and it is not free. `FileIdMap::add_path`
/// is a `WalkDir` plus a `stat` per entry, run inside `watch()`, so arming a
/// watch walks the entire tree: on the folder that reported this, 154,765
/// entries on a USB volume, about ten minutes, holding the profile's
/// one-operation-at-a-time reservation with the journal full of ready work — at
/// every process start, and again on every dropped-event rescan.
///
/// `RecommendedCache` is the trap: it is `NoCache` on Linux and `FileIdMap`
/// everywhere else, so the dev box and the whole test suite saw the free half
/// and the shipped Mac app paid the other one. Naming the policy makes the two
/// platforms behave the same, which is the only way this stays fixed.
enum Backend {
    Native(Debouncer<RecommendedWatcher, NoCache>),
    Poll(Debouncer<PollWatcher, NoCache>),
}

impl Backend {
    fn watch(&mut self, root: &Path) -> notify::Result<()> {
        match self {
            Self::Native(debouncer) => debouncer.watch(root, RecursiveMode::Recursive),
            Self::Poll(debouncer) => debouncer.watch(root, RecursiveMode::Recursive),
        }
    }

    /// Joins the debouncer's worker thread, so that after this returns the
    /// debouncer really has stopped. `Debouncer`'s own `Drop` only sets a stop
    /// flag and returns, leaving events deliverable for up to one more tick.
    ///
    /// Only ever called from [`retire`]'s thread: the join is bounded but the
    /// backend drop that follows it is not, on macOS, so no caller of ours is
    /// allowed to be the one waiting here.
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
                new_debouncer_opt::<_, PollWatcher, NoCache>(
                    timeout,
                    None,
                    handler,
                    NoCache::new(),
                    notify_config,
                )
                .map_err(|err| map_notify_error("start watcher", &root, err))?,
            )
        } else {
            Backend::Native(
                // `new_debouncer` would pick `RecommendedCache` — see `Backend`
                // for the ten minutes that costs on macOS.
                new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
                    timeout,
                    None,
                    handler,
                    NoCache::new(),
                    notify::Config::default(),
                )
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

    /// This watcher's [`EchoSuppressor`], for a caller that wants to fold its
    /// own writes out of the batches this watcher emits.
    ///
    /// **No production caller takes this handle.** `Engine::fold_watch_events`
    /// deliberately does not register — see the module documentation — so in
    /// the shipped engine this returns a set that is always empty and the
    /// suppression branch in `fold_batch` never fires.
    pub fn suppressor(&self) -> EchoSuppressor {
        self.suppressor.clone()
    }

    /// Retire the watcher, without waiting for its threads to finish.
    ///
    /// Returns as soon as the teardown has been handed to [`retire`]. `Drop`
    /// does exactly the same thing, so a forgotten `stop` is a tidiness problem
    /// rather than a correctness one.
    ///
    /// This deliberately does **not** promise that no further [`WatchEvent`] can
    /// be produced, because on macOS that promise cannot be kept in bounded time
    /// — see [`retire`] for why. It does not need to. Events are sent to a
    /// channel whose receiver the caller drops along with this watcher, so a
    /// late batch finds a closed channel and `start`'s handler discards it.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        // Dropping the sender is what wakes the rescan thread out of
        // `recv_timeout`; without it the join inside `retire` would sit there
        // for up to a full rescan interval.
        drop(self.rescan_stop.take());
        let rescan = self.rescan_thread.take();
        if let Some(backend) = self.backend.take() {
            retire(&self.root, backend, rescan);
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Tear a retired watcher down on a thread of its own, and never wait for it.
///
/// Dropping a platform backend is not reliably prompt, and on macOS it is not
/// reliably *finite*. `notify`'s FSEvents teardown spins
///
/// ```text
/// while CFRunLoopIsWaiting(runloop) == 0 { thread::yield_now(); }
/// ```
///
/// (`notify-8.2.0/src/fsevent.rs:338`) with no deadline and no sleep, and it is
/// reached from `FsEventWatcher`'s own `Drop`. A run loop that is not parked
/// when we ask is therefore waited for forever: one that has not yet entered
/// `CFRunLoopRun` — the handle is published to us *before* the loop is entered
/// (`fsevent.rs:477-495`) — or one that has already left it, for which
/// `CFRunLoopIsWaiting` is false permanently. Linux hides all of this, which is
/// why a green Linux run says nothing about it: `INotifyWatcher::drop`
/// (`inotify.rs:606`) posts a shutdown message, wakes the poll, and returns
/// without joining anything.
///
/// Nothing the engine does needs a finished teardown, and three things it does
/// need must never wait for one: the supervisor tick replaces and retains
/// watchers, the graceful shutdown stops all of them, and `remove_profile`
/// drops one from an IPC thread. Blocking any of those on a backend that may
/// never let go is how a folder watch takes the whole app down with it, which
/// AD-34-11 forbids in as many words.
///
/// So the teardown gets its own named thread and nobody joins it. A wedged
/// backend then wedges exactly one thread, and `sample`/`ps -T` finds it by
/// name instead of the app simply stopping.
fn retire(root: &Path, backend: Backend, rescan: Option<std::thread::JoinHandle<()>>) {
    let label = backend.label();
    let owned_root = root.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("keeper-sync-retire".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            if let Some(handle) = rescan {
                if handle.join().is_err() {
                    tracing::warn!(root = %owned_root.display(), "rescan thread panicked");
                }
            }
            backend.stop();
            let elapsed = started.elapsed();
            if elapsed >= RETIRE_WARN_AFTER {
                tracing::warn!(
                    root = %owned_root.display(),
                    backend = label,
                    elapsed_ms = elapsed.as_millis(),
                    "folder watch took far too long to tear down"
                );
            } else {
                tracing::debug!(
                    root = %owned_root.display(),
                    backend = label,
                    "folder watch torn down"
                );
            }
        });
    if let Err(err) = spawned {
        // The closure was dropped with the backend inside it, so the teardown
        // has already happened inline on this thread — the one case where a
        // caller does wait. Saying so is the point: a process that cannot
        // create a thread has a larger problem than this watcher, and silence
        // here would make the resulting stall unattributable.
        tracing::error!(
            root = %root.display(),
            %err,
            "could not spawn a watch-retirement thread; the watch was torn down inline"
        );
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
        // An unrelated sibling must still get through, or a registrar's
        // checkout would mute every concurrent user edit in the profile.
        assert!(!s.is_suppressed(Path::new("/w/other.md")));

        s.clear(path);
        assert!(!s.is_suppressed(path));
        assert!(s.is_empty());
    }

    #[test]
    fn the_echo_suppressor_covers_a_registered_subtree() {
        let s = EchoSuppressor::new();
        // A registrar would register the directory it is about to materialize;
        // every file it writes underneath must be muted, not just the
        // directory. No registrar exists today — see [`EchoSuppressor`].
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

    /// A unit test of [`EchoSuppressor`] alone — **not** of any shipped
    /// behaviour.
    ///
    /// Nothing in production registers a path with an `EchoSuppressor`:
    /// `Engine::fold_watch_events` never takes [`FolderWatcher::suppressor`],
    /// so in the shipped engine the set this exercises is permanently empty
    /// and the branch below is unreachable. That is the deliberate choice
    /// recorded in the module documentation (Story 34.9): an event is only a
    /// prompt to run `git status`, so a checkout echo costs a fruitless walk
    /// and never a re-commit.
    ///
    /// The test is kept because the type is kept: if a future registrar
    /// appears, subtree-wide suppression and its release are the two
    /// properties it will rely on. It is named for what it actually proves so
    /// that nobody reads it as evidence that the engine suppresses its own
    /// writes — it is not, and the engine does not.
    #[test]
    fn a_registered_subtree_is_folded_out_of_a_batch_and_flows_again_once_cleared() {
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
            "a registered directory suppresses everything beneath it, and nothing else"
        );

        // ...and clearing the registration lets the same batch through again,
        // which is the half a registrar would get wrong by forgetting.
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
    fn retiring_a_watcher_does_not_wait_for_its_backend() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        // A four-second debounce gives the debouncer a one-second tick
        // (`notify-debouncer-full` derives `tick = timeout / 4` and sleeps a
        // whole tick per iteration), so a teardown that JOINS that thread takes
        // up to a second on every platform — while a teardown that hands the
        // backend to `retire` costs a thread spawn. That gap is the contract,
        // and it is the one a re-added `join()` in `shutdown` would break, on
        // Linux as well as on the macOS backend that cannot be bounded at all.
        let config = WatchConfig {
            debounce_ms: 4_000,
            rescan_interval_ms: 0,
            force_poll: false,
        };
        let watcher = FolderWatcher::start(dir.path(), config, tx).expect("watcher arms");

        let started = Instant::now();
        watcher.stop();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(250),
            "stop() waited {elapsed:?} for the backend; the supervisor, the \
             shutdown path and remove_profile all sit on this call"
        );
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
