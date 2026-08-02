//! The notes driven adapter (AD-56, AD-57, AD-62) — every effect the notes
//! domain refuses to own.
//!
//! `keeper_core::notes` is pure: it takes bytes and returns values (AD-55). This
//! module is where the platform lives, and it is to notes what `recorder.rs` is
//! to recording (AD-33). It owns:
//!
//! * the **vault registry** — profile id → canonical root + `NotesConfig`,
//!   rebuilt whenever the profile set changes, because a vault *is* a
//!   notes-flagged `SyncProfile` and there is no second store to keep in step
//!   (AD-54);
//! * the **cold scan**, walking the vault through `keeper-sync`'s own
//!   [`ExcludeSet`] so keeper's exclusion rules mean the same thing to notes as
//!   to sync, and skipping `.obsidian/` **by name before descent**;
//! * the advisory `<vault>/.keeper/index.json` cache, which is allowed to be
//!   wrong: a mismatched `(size, mtime_ns, ino)` re-parses one note and anything
//!   else wrong discards the whole file and rescans (AD-57);
//! * **one reconciler task per vault**, the single mutator of that vault's
//!   `IndexBuilder`, publishing `Arc<IndexSnapshot>` over a [`watch`] channel so
//!   readers are never blocked by a write and need no lock;
//! * the subscriber on the engine's watcher tap with its 150 ms coalescer —
//!   rendering a half-written file is a repaint, committing one is corruption,
//!   so notes may be optimistic here where the four-tier stability gate may not;
//! * the reader and the atomic writer (temp + rename, echo-suppressed), the
//!   trash (NFR-30 — a delete is never an `unlink`), conflict-copy recognition,
//!   attachment import, and the provenance projection that fills
//!   `NoteRevisionVm` from the commit trailers `keeper-sync` already writes
//!   (AD-63);
//! * the **cadence** (AD-62), evaluated on the ~1 Hz supervisor tick that
//!   already exists. There is no second clock, no notes scheduler, no notes
//!   timer.
//!
//! One consequence of that last part is worth stating so nobody "fixes" it:
//! provenance is a git fact, so it arrives one publish *after* the index does.
//! For that window every note reads as `origin:local`, which is the documented
//! absent-value default rather than an accident — nothing is known yet, and a
//! space whose only term is `origin:agent` is honestly empty until the projection
//! lands.
//!
//! Nothing here decides anything the core can decide.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};
use keeper_core::notes::index::{
    link_key, IndexBuilder, IndexCache, IndexEntry, IndexSnapshot, NoteDelta, FIELD_DEVICE,
    FIELD_ORIGIN, INDEX_SCHEMA,
};
use keeper_core::notes::vm::{
    NoteAttachmentVm, NoteCadenceVm, NoteDiffVm, NoteHunkVm, NoteIndexProgressVm, NoteRevisionVm,
    NoteVaultVm,
};
use keeper_core::notes::{links, naming, tags, NotesError};
use keeper_core::platform::Platform;
use keeper_sync::exclude::ExcludeSet;
use keeper_sync::profile::{NotesCadence, NotesConfig, SyncProfile};
use keeper_sync::provenance::{Provenance, SyncSource};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, watch};

/// keeper's own per-vault cache directory. Tier-0 excluded by `keeper-sync`, so
/// nothing in it is ever staged, committed or listed as pending (FR-121).
pub const KEEPER_DIR: &str = ".keeper";

/// Obsidian's settings directory. Never read, never written, never walked
/// (FR-121). Named here exactly once, so the walk can refuse it by name.
pub const OBSIDIAN_DIR: &str = ".obsidian";

/// The vault-relative directory pasted and dropped assets land in (FR-110).
const ATTACHMENTS_DIR: &str = "attachments";

/// The coalescing window notes applies on top of the sync watcher's own 500 ms
/// debounce (AD-56). A sliding window, reset by each new path: a burst of writes
/// to one note costs one re-read, and the NFR-29 budget spends 150 of its 1000
/// ms here.
const COALESCE_WINDOW: Duration = Duration::from_millis(150);

/// Bodies larger than this are indexed head-only and flagged `oversize`. This
/// bounds the per-note cost of a pathological file rather than refusing to index
/// it — the note is the user's, the index is ours.
const MAX_INDEXED_BODY: usize = 1024 * 1024;

/// How many characters of body a row's snippet carries.
const SNIPPET_CHARS: usize = 240;

/// How many commits the provenance projection reads per vault (AD-63).
///
/// Unread state is about recent change: a note nobody has touched in two hundred
/// commits is read by definition, and an unbounded revwalk on every index
/// publish is not a steady state.
const PROVENANCE_COMMITS: u32 = 200;

/// Record and field separators for the `git log` projections. ASCII control
/// characters, so neither can appear in a sanitized trailer value or a subject.
const RECORD_SEP: &str = "\u{1e}";
const FIELD_SEP: &str = "\u{1f}";

/// The scan phases a progress message reports.
const PHASE_PARSING: &str = "parsing";
const PHASE_READY: &str = "ready";
const PHASE_SCANNING: &str = "scanning";

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// One registered vault: the profile it rides on, resolved once.
///
/// `root` is **canonicalized at registration** and never re-derived from a
/// caller's argument, which is what makes the `keeper-note://` containment check
/// one `canonicalize` plus one `starts_with` on the hot path (AD-59).
#[derive(Debug, Clone)]
pub struct Vault {
    /// The profile id. A vault has no identity of its own (AD-54).
    pub id: String,
    /// The profile's human label, shown in the switcher and the tray.
    pub name: String,
    /// Canonical vault root — `local_path/subfolder`.
    pub root: PathBuf,
    /// Canonical profile root. A file link may point anywhere inside it
    /// (FR-109), and nowhere else.
    pub local_path: PathBuf,
    pub config: NotesConfig,
    /// keeper's exclusion rules, compiled once and shared by the scan and the
    /// watcher tap so both agree on what a vault contains.
    pub excludes: Arc<ExcludeSet>,
}

impl Vault {
    /// The absolute path of a vault-relative note path.
    fn join(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// `<vault>/.keeper` — created lazily, never at flag time, because an empty
    /// scaffold in someone's existing vault is the "keeper moved my stuff"
    /// failure FR-121 forbids.
    fn keeper_dir(&self) -> PathBuf {
        self.root.join(KEEPER_DIR)
    }
}

/// The live state of one vault. The reconciler task owns the `IndexBuilder`; the
/// slot holds only what readers need.
struct Slot {
    vault: Vault,
    /// The published index. Readers clone an `Arc` and are never blocked.
    index: watch::Receiver<Arc<IndexSnapshot>>,
    /// Cold-scan / rescan progress, for `notes_subscribe_index`.
    progress: watch::Receiver<NoteIndexProgressVm>,
    /// Head provenance per vault-relative path, refreshed on every publish.
    /// Beside the snapshot rather than inside the pure index, because it is read
    /// from git and the core may not know git exists (AD-40).
    heads: Arc<HashMap<String, HeadRevision>>,
    /// Work handed to the reconciler. Dropping the slot ends the task.
    work: mpsc::UnboundedSender<Work>,
    /// The AD-62 cadence, evaluated by the ~1 Hz supervisor tick.
    cadence: Cadence,
}

/// What the reconciler is asked to do. Everything else it decides itself.
enum Work {
    /// These vault-relative paths may have changed on disk.
    Touched(Vec<String>),
    /// Discard everything and cold-scan — `notes_index_rebuild`, or a lagged
    /// watcher tap, where a burst that outran the channel degrades to a slower
    /// correct answer rather than a lost update.
    Rescan,
    /// The provenance projection came back. Origin is a *git* fact the pure
    /// index cannot know, so it arrives as a second pass over the entry set:
    /// the reconciler stamps `keeper.origin` and `keeper.device` onto each
    /// entry's fields (which is where `origin:` reads them) and republishes.
    Heads(HashMap<String, HeadRevision>),
}

/// The process-wide registry.
///
/// A `Mutex` rather than something lock-free because it is written only when the
/// profile set changes and read on every command; the index itself lives behind
/// a `watch` and takes no lock at all.
static REGISTRY: LazyLock<Mutex<HashMap<String, Slot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether the watcher-tap subscriber is running for this process.
static TAP_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

/// Lock the registry, recovering a poisoned lock — the map holds plain handles
/// with no invariant a mid-write panic could break, and notes must never be the
/// reason the app stops answering.
fn registry() -> MutexGuard<'static, HashMap<String, Slot>> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tap_flag() -> MutexGuard<'static, bool> {
    TAP_RUNNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start the notes subsystem: build the registry from the profile set, then
/// subscribe to the sync engine's watcher tap.
///
/// Called from `setup()` after the sync supervisor, because the tap is the
/// engine's and there is nothing to subscribe to before it exists. Idempotent
/// and best-effort: a machine with no usable `git` has no engine and therefore
/// no vaults, and this returns quietly — `CapabilitiesVm.notes` is already
/// `false` there.
pub fn start(app: &AppHandle) {
    refresh(app);
    start_tap();
}

/// Rebuild the registry from the current profile set.
///
/// Called at startup and after any profile write, which is the whole of "the
/// vault list *is* a filter over the profile list" (AD-54): flagging a folder
/// adds a vault, unflagging removes one and deletes nothing.
pub fn refresh(app: &AppHandle) {
    let Some(engine) = crate::sync::engine_if_open() else {
        return;
    };
    let profiles = match engine.list_profiles() {
        Ok(profiles) => profiles,
        Err(error) => {
            tracing::warn!(%error, "notes: could not read the profile set; registry unchanged");
            return;
        }
    };
    let wanted: Vec<Vault> = profiles.iter().filter_map(register_one).collect();
    let keep: HashSet<&str> = wanted.iter().map(|vault| vault.id.as_str()).collect();

    let mut guard = registry();
    // Dropping a slot drops its work sender, which ends its reconciler task —
    // the whole of "unflagging a vault costs nothing and deletes nothing".
    guard.retain(|id, _| keep.contains(id.as_str()));
    for vault in wanted {
        match guard.get_mut(&vault.id) {
            // Same root: adopt the new configuration in place, so a settings
            // save does not throw away a warm index.
            Some(slot) if slot.vault.root == vault.root => {
                slot.vault.name = vault.name;
                slot.vault.config = vault.config;
                slot.vault.excludes = vault.excludes;
            }
            // A vault whose root moved is a different vault: end the old
            // reconciler rather than leave one task holding a stale root for the
            // process lifetime.
            _ => {
                let slot = spawn_reconciler(app, vault);
                guard.insert(slot.vault.id.clone(), slot);
            }
        }
    }
    drop(guard);
    // A `git` repoint tears the engine down and closes the tap; re-arming here
    // means the first profile write after a repair puts notes back too.
    start_tap();
}

/// Resolve one profile into a vault, or `None` when it is not one.
///
/// The root is canonicalized here and nowhere else. A vault whose folder is
/// absent — an unmounted volume, a folder the user moved — canonicalizes to
/// nothing and is skipped **without discarding anything**: a missing folder is
/// not evidence of a deletion (AD-48), and the next refresh adopts it back.
fn register_one(profile: &SyncProfile) -> Option<Vault> {
    let config = profile.notes.clone()?;
    let root = profile.vault_root()?;
    let canonical = match root.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) => {
            tracing::info!(
                profile = %profile.id,
                path = %root.display(),
                %error,
                "notes: vault folder is not there right now; leaving it unregistered"
            );
            return None;
        }
    };
    let local_path = profile
        .local_path
        .canonicalize()
        .unwrap_or_else(|_| profile.local_path.clone());
    let excludes = match ExcludeSet::new(&profile.excludes) {
        Ok(set) => set,
        Err(error) => {
            // A malformed user pattern must not cost the vault its index:
            // keeper's built-in corpus alone is still correct, just less
            // specific. If even that refuses to compile there is nothing left to
            // fall back to, so the vault goes unregistered rather than unfiltered.
            tracing::warn!(profile = %profile.id, %error, "notes: falling back to built-in excludes");
            match ExcludeSet::new(&[]) {
                Ok(set) => set,
                Err(error) => {
                    tracing::error!(%error, "notes: the built-in exclude corpus did not compile");
                    return None;
                }
            }
        }
    };
    Some(Vault {
        id: profile.id.clone(),
        name: profile.name.clone(),
        root: canonical,
        local_path,
        config,
        excludes: Arc::new(excludes),
    })
}

/// Every registered vault, ordered by name so every surface lists them the same
/// way.
pub fn vaults() -> Vec<Vault> {
    let mut all: Vec<Vault> = registry().values().map(|slot| slot.vault.clone()).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// One vault by id, or `None` when the id names nothing.
///
/// Every command resolves its `vault_id` through here rather than trusting a
/// caller: the capture window's capability file is a floor, not a fence (AD-60),
/// so the registry is the real containment.
pub fn vault(id: &str) -> Option<Vault> {
    registry().get(id).map(|slot| slot.vault.clone())
}

/// The published index for a vault.
pub fn snapshot(id: &str) -> Option<Arc<IndexSnapshot>> {
    registry().get(id).map(|slot| slot.index.borrow().clone())
}

/// A receiver that wakes on every index publish, for the streaming surfaces.
pub fn subscribe_index(id: &str) -> Option<watch::Receiver<Arc<IndexSnapshot>>> {
    registry().get(id).map(|slot| slot.index.clone())
}

/// A receiver that wakes on every scan-progress change.
pub fn subscribe_progress(id: &str) -> Option<watch::Receiver<NoteIndexProgressVm>> {
    registry().get(id).map(|slot| slot.progress.clone())
}

/// The current scan progress for a vault.
pub fn progress(id: &str) -> Option<NoteIndexProgressVm> {
    registry()
        .get(id)
        .map(|slot| slot.progress.borrow().clone())
}

/// Head provenance per vault-relative path, for the row projection.
pub fn heads(id: &str) -> Option<Arc<HashMap<String, HeadRevision>>> {
    registry().get(id).map(|slot| Arc::clone(&slot.heads))
}

/// Whether this vault has finished its first scan.
pub fn is_indexed(id: &str) -> bool {
    registry()
        .get(id)
        .is_some_and(|slot| slot.progress.borrow().phase == PHASE_READY)
}

/// Ask a vault's reconciler to re-read these vault-relative paths.
///
/// Used after keeper's own write: that write is echo-suppressed for the
/// *watcher*, so without this the reconciler would learn about it only on the
/// next unrelated event.
pub fn touch(id: &str, paths: Vec<String>) {
    if let Some(slot) = registry().get(id) {
        let _ = slot.work.send(Work::Touched(paths));
    }
}

/// Drop the cache and cold-scan (`notes_index_rebuild`).
///
/// Deleting `.keeper/` by hand is the same repair, which is why this is not the
/// only way to get it.
pub fn rebuild(id: &str) -> Result<(), NotesError> {
    let guard = registry();
    let slot = guard
        .get(id)
        .ok_or_else(|| NotesError::VaultUnknown(id.to_owned()))?;
    let cache = slot.vault.keeper_dir().join("index.json");
    if let Err(error) = std::fs::remove_file(&cache) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, "notes: could not drop the index cache; rescanning anyway");
        }
    }
    let _ = slot.work.send(Work::Rescan);
    Ok(())
}

/// Project a vault into its view model.
pub fn vault_vm(vault: &Vault, unread: u32) -> NoteVaultVm {
    NoteVaultVm {
        id: vault.id.clone(),
        profile_id: vault.id.clone(),
        name: vault.name.clone(),
        subfolder: vault.config.subfolder.clone(),
        root: vault.root.to_string_lossy().into_owned(),
        indexed: is_indexed(&vault.id),
        note_count: snapshot(&vault.id).map_or(0, |snapshot| {
            u32::try_from(snapshot.len()).unwrap_or(u32::MAX)
        }),
        unread_count: unread,
        cadence: cadence_vm(&vault.config.cadence),
    }
}

/// The cadence as the settings form shows it — the values actually in force
/// (AD-34-8).
pub fn cadence_vm(cadence: &NotesCadence) -> NoteCadenceVm {
    NoteCadenceVm {
        commit_idle_ms: cadence.commit_idle_ms,
        push_interval_ms: cadence.push_interval_ms,
        push_on_blur: cadence.push_on_blur,
    }
}

// ---------------------------------------------------------------------------
// The watcher tap and its coalescer
// ---------------------------------------------------------------------------

/// Subscribe to the engine's watcher tap, once per process (AD-56).
///
/// `keeper-sync` already runs one recursive `notify` instance per profile, and
/// the host's inotify instance budget is a hard ceiling — so notes taps that
/// stream instead of opening a second watcher over the same subtree, which would
/// double every event and desynchronize two debouncers over one file.
///
/// It does **not** mute keeper's own writes here. The engine's `EchoSuppressor`
/// is reachable only from inside the engine, and hiding a note keeper wrote from
/// the sync watcher would also hide it from the thing that commits it. The echo
/// is suppressed one layer up instead, where it actually matters: a body
/// subscription compares the disk revision against the revision it last
/// delivered or wrote (`content_rev`), so keeper's own write reaches the index —
/// which is correct, the index must reflect it — and reaches no editor as an
/// external change.
fn start_tap() {
    let mut running = tap_flag();
    if *running {
        return;
    }
    let Some(engine) = crate::sync::engine_if_open() else {
        // No engine means no watcher to tap and no vaults either; the next
        // `refresh` re-enters here.
        return;
    };
    *running = true;
    let mut tap = engine.watch_tap();
    drop(running);
    tauri::async_runtime::spawn(async move {
        loop {
            match tap.recv().await {
                Ok((profile_id, path)) => fan_out(&profile_id, &path),
                // A burst that outran the channel is a correctness question, not
                // a performance one: rescan rather than guess which updates were
                // dropped.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::info!(missed, "notes: watcher tap lagged; rescanning every vault");
                    for slot in registry().values() {
                        let _ = slot.work.send(Work::Rescan);
                    }
                }
                // The sender is gone: the engine was torn down (a `git`
                // repoint). Release the flag so the next refresh re-arms.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    *tap_flag() = false;
                    return;
                }
            }
        }
    });
}

/// Route one tapped absolute path to the vault that contains it, if any.
///
/// A profile with no vault, and a change outside the vault subfolder, are
/// dropped here — the cheapest possible filter, before any `lstat`.
fn fan_out(profile_id: &str, path: &Path) {
    let guard = registry();
    let Some(slot) = guard.get(profile_id) else {
        return;
    };
    let Some(rel) = vault_relative(&slot.vault, path) else {
        return;
    };
    if is_internal(&rel) || slot.vault.excludes.is_excluded(Path::new(&rel)) {
        return;
    }
    slot.cadence.mark_dirty();
    let _ = slot.work.send(Work::Touched(vec![rel]));
}

/// A vault-relative, `/`-separated path for an absolute one inside the vault.
fn vault_relative(vault: &Vault, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(&vault.root).ok()?;
    let joined = rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    (!joined.is_empty()).then_some(joined)
}

/// Whether a vault-relative path is keeper's, git's or Obsidian's own business.
///
/// `.obsidian/` is refused here as well as skipped by the walk, because "we
/// never generate that path" is not the same as "that path cannot be requested".
fn is_internal(rel: &str) -> bool {
    rel.split('/')
        .any(|part| part == KEEPER_DIR || part == OBSIDIAN_DIR || part == ".git")
}

/// A sliding-window coalescer over changed vault-relative paths (AD-56, step 4).
///
/// Each new path pushes the deadline out, so a burst of writes to one note — an
/// editor saving three times in 400 ms, an agent appending line by line — costs
/// exactly one re-read. A `BTreeSet` because the set is small, dedup is the
/// whole point, and ordered output makes a batch deterministic.
#[derive(Debug, Default)]
struct Coalescer {
    pending: BTreeSet<String>,
    deadline: Option<Instant>,
}

impl Coalescer {
    /// Accept a path and (re)start the window.
    fn push(&mut self, rel: String, now: Instant) {
        self.pending.insert(rel);
        self.deadline = Some(now + COALESCE_WINDOW);
    }

    /// How long until the window closes, or `None` when nothing is pending.
    fn wait(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    /// Whether the window has closed on a non-empty batch.
    fn is_due(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline) && !self.pending.is_empty()
    }

    /// Take the batch and close the window.
    fn take(&mut self) -> Vec<String> {
        self.deadline = None;
        std::mem::take(&mut self.pending).into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// The reconciler
// ---------------------------------------------------------------------------

/// Spawn the one task that owns this vault's `IndexBuilder`.
///
/// Single mutator, so the index needs no lock: readers take an
/// `Arc<IndexSnapshot>` from the `watch` and are never blocked by a write
/// (AD-57).
fn spawn_reconciler(app: &AppHandle, vault: Vault) -> Slot {
    let (work_tx, work_rx) = mpsc::unbounded_channel();
    let (index_tx, index_rx) = watch::channel(Arc::new(IndexSnapshot::default()));
    let (progress_tx, progress_rx) = watch::channel(NoteIndexProgressVm {
        vault_id: vault.id.clone(),
        scanned: 0,
        total_estimate: 0,
        phase: PHASE_SCANNING.to_owned(),
    });
    let slot = Slot {
        vault: vault.clone(),
        index: index_rx,
        progress: progress_rx,
        heads: Arc::new(HashMap::new()),
        work: work_tx,
        cadence: Cadence::default(),
    };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        reconcile(handle, vault, work_rx, index_tx, progress_tx).await;
    });
    slot
}

/// The reconciler loop: cold start, then coalesced incremental applies.
async fn reconcile(
    app: AppHandle,
    vault: Vault,
    mut work: mpsc::UnboundedReceiver<Work>,
    index: watch::Sender<Arc<IndexSnapshot>>,
    progress: watch::Sender<NoteIndexProgressVm>,
) {
    let mut state = ReconcilerState::default();
    cold_start(&app, &vault, &mut state, &index, &progress).await;

    let mut coalescer = Coalescer::default();
    loop {
        // With a batch pending, wait only as long as its window has left; the
        // timeout expiring IS the flush.
        let received = match coalescer.wait(Instant::now()) {
            Some(remaining) => match tokio::time::timeout(remaining, work.recv()).await {
                Ok(received) => received,
                Err(_) => Some(Work::Touched(Vec::new())),
            },
            None => work.recv().await,
        };
        match received {
            Some(Work::Touched(paths)) => {
                let now = Instant::now();
                for rel in paths {
                    coalescer.push(rel, now);
                }
                if coalescer.is_due(Instant::now()) {
                    let batch = coalescer.take();
                    apply_batch(&app, &vault, &mut state, &batch, &index).await;
                }
            }
            Some(Work::Rescan) => {
                coalescer.take();
                state = ReconcilerState::default();
                cold_start(&app, &vault, &mut state, &index, &progress).await;
            }
            Some(Work::Heads(heads)) => stamp_heads(&vault, &mut state, heads, &index),
            // Every sender is gone: the vault was unflagged, or its root moved
            // and a fresh reconciler took over.
            None => return,
        }
    }
}

/// Everything the single mutator owns.
#[derive(Default)]
struct ReconcilerState {
    builder: IndexBuilder,
    /// The entry set as this task last built it, keyed by vault-relative path.
    /// The task's own mirror, so an incremental apply can diff against the
    /// previous version of the one note that changed.
    entries: HashMap<String, IndexEntry>,
    /// Which notes emit each link token, so `is:orphan` is a fact rather than a
    /// guess. Maintained incrementally: only the tokens the changed note gained
    /// or lost are touched.
    inbound: HashMap<String, HashSet<String>>,
}

/// Load the cache, walk the vault, parse whatever the cache could not vouch for,
/// and publish.
async fn cold_start(
    app: &AppHandle,
    vault: &Vault,
    state: &mut ReconcilerState,
    index: &watch::Sender<Arc<IndexSnapshot>>,
    progress: &watch::Sender<NoteIndexProgressVm>,
) {
    let cached = load_cache(vault);
    let seen = {
        let vault = vault.clone();
        // The walk is `read_dir` plus one `lstat` per entry — blocking IO, so it
        // belongs on the blocking pool and not on a runtime worker.
        match tokio::task::spawn_blocking(move || {
            let mut walker = DiskWalk {
                root: vault.root.clone(),
            };
            walk(&mut walker, vault.excludes.as_ref())
        })
        .await
        {
            Ok(seen) => seen,
            Err(error) => {
                tracing::warn!(%error, "notes: the vault walk failed; leaving the index empty");
                Vec::new()
            }
        }
    };

    let total = u32::try_from(seen.len()).unwrap_or(u32::MAX);
    let plan = plan_scan(cached, seen);
    let adopted = u32::try_from(plan.adopt.len()).unwrap_or(u32::MAX);
    let _ = progress.send(NoteIndexProgressVm {
        vault_id: vault.id.clone(),
        scanned: adopted,
        total_estimate: total,
        phase: PHASE_PARSING.to_owned(),
    });

    let mut entries = plan.adopt;
    entries.extend(read_and_parse(vault, plan.parse, progress, total, adopted).await);

    state.entries = entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    state.inbound = build_inbound(&state.entries);
    apply_orphan_flags(state);
    // The cold build seeds the builder from the whole entry set once, after the
    // orphan pass, so nothing needs re-projecting individually here.
    state.builder = IndexBuilder::from_entries(state.entries.values().cloned().collect());

    let snapshot = state.builder.snapshot();
    let _ = progress.send(NoteIndexProgressVm {
        vault_id: vault.id.clone(),
        scanned: u32::try_from(snapshot.len()).unwrap_or(u32::MAX),
        total_estimate: total,
        phase: PHASE_READY.to_owned(),
    });
    publish(app, vault, index, snapshot);
    write_cache(vault, state);
}

/// Re-read a coalesced batch of paths and publish the result.
///
/// One `lstat` per path in the window — the whole of NFR-28's steady-state cost.
/// A path whose `(size, mtime_ns, ino)` still matches the index is dropped
/// before anything is read.
async fn apply_batch(
    app: &AppHandle,
    vault: &Vault,
    state: &mut ReconcilerState,
    batch: &[String],
    index: &watch::Sender<Arc<IndexSnapshot>>,
) {
    if batch.is_empty() {
        return;
    }
    let now = now_ms();
    let mut changed = false;
    for rel in batch {
        // Attachments and other vault files are not notes. They still sync; they
        // are simply not index entries.
        if !rel.ends_with(".md") {
            continue;
        }
        let absolute = vault.join(rel);
        // `symlink_metadata`: a symlink is followed by nothing here, so a link
        // pointing out of the vault is a link and never a door.
        match std::fs::symlink_metadata(&absolute) {
            Ok(meta) if meta.is_file() => {
                let stat = file_stat(&meta);
                if state
                    .entries
                    .get(rel)
                    .is_some_and(|entry| cache_hit(entry, &stat))
                {
                    continue;
                }
                let Some(text) = read_bounded(&absolute) else {
                    continue;
                };
                upsert(state, parse_note(rel, &stat, &text, now));
                changed = true;
            }
            // Gone, or replaced by something that is not a regular file.
            _ => changed |= remove(state, rel),
        }
    }
    if !changed {
        return;
    }
    let moved = apply_orphan_flags(state);
    reapply(state, &moved);
    publish(app, vault, index, state.builder.snapshot());
    write_cache(vault, state);
}

/// Absorb one changed note, keeping the inbound-link map true.
fn upsert(state: &mut ReconcilerState, entry: IndexEntry) {
    if let Some(previous) = state.entries.get(&entry.path) {
        retract_links(&mut state.inbound, previous);
    }
    record_links(&mut state.inbound, &entry);
    state
        .builder
        .apply(NoteDelta::Upsert(Box::new(entry.clone())));
    state.entries.insert(entry.path.clone(), entry);
}

/// Absorb a removal, reporting whether anything was actually there.
fn remove(state: &mut ReconcilerState, rel: &str) -> bool {
    let Some(previous) = state.entries.remove(rel) else {
        return false;
    };
    retract_links(&mut state.inbound, &previous);
    state.builder.apply(NoteDelta::Remove {
        path: rel.to_owned(),
    });
    true
}

/// Publish a snapshot and refresh the head-provenance projection beside it.
fn publish(
    app: &AppHandle,
    vault: &Vault,
    index: &watch::Sender<Arc<IndexSnapshot>>,
    snapshot: Arc<IndexSnapshot>,
) {
    let _ = index.send(snapshot);
    refresh_heads(app, vault);
}

/// Stamp the provenance projection onto the entry set and republish.
///
/// `origin:` is evaluated by the pure query layer over `IndexEntry.fields`, and
/// the two keys it reads are namespaced (`keeper.origin`, `keeper.device`) so
/// they cannot collide with a user's own frontmatter. They are written here
/// rather than in `parse_note` because they are a *git* fact: the bytes of a note
/// do not say who committed it.
///
/// This republishes without asking for another projection, which is what keeps
/// the two passes from chasing each other forever.
fn stamp_heads(
    vault: &Vault,
    state: &mut ReconcilerState,
    heads: HashMap<String, HeadRevision>,
    index: &watch::Sender<Arc<IndexSnapshot>>,
) {
    let mut moved = Vec::new();
    for (path, entry) in &mut state.entries {
        // A note with no commit yet matches `origin:local`, which is what the
        // predicate table says: absent reads as local.
        let (origin, device) = heads.get(path).map_or(("local", ""), |head| {
            (head.origin.as_str(), head.device.as_str())
        });
        let changed =
            replace_field(entry, FIELD_ORIGIN, origin) | replace_field(entry, FIELD_DEVICE, device);
        if changed {
            moved.push(path.clone());
        }
    }
    if let Some(slot) = registry().get_mut(&vault.id) {
        slot.heads = Arc::new(heads);
    }
    if moved.is_empty() {
        return;
    }
    reapply(state, &moved);
    let _ = index.send(state.builder.snapshot());
}

/// Set one reserved field, reporting whether it actually moved.
fn replace_field(entry: &mut IndexEntry, key: &str, value: &str) -> bool {
    if entry.fields.get(key).map(String::as_str) == Some(value) {
        return false;
    }
    entry.fields.insert(key.to_owned(), value.to_owned());
    true
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// The three numbers that decide whether a cached parse is still true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    pub mtime_ns: i128,
    pub ino: u64,
}

/// One entry the walk saw.
#[derive(Debug, Clone)]
struct WalkEntry {
    name: String,
    is_dir: bool,
    stat: FileStat,
}

/// A note the walk found, with the `lstat` it already paid for.
#[derive(Debug, Clone)]
struct Seen {
    rel: String,
    stat: FileStat,
}

/// The directory listing the walk needs, and nothing else.
///
/// A trait rather than a direct `read_dir` because the FR-121 promise about
/// `.obsidian/` is a **negative** — a syscall that never happened — and a
/// negative cannot be asserted against a real filesystem. The test implements
/// this with a map and records every directory the walk asked for.
trait VaultWalk {
    /// List `rel` (`""` is the vault root).
    fn list(&mut self, rel: &str) -> std::io::Result<Vec<WalkEntry>>;
}

/// The real filesystem.
struct DiskWalk {
    root: PathBuf,
}

impl VaultWalk for DiskWalk {
    fn list(&mut self, rel: &str) -> std::io::Result<Vec<WalkEntry>> {
        let dir = if rel.is_empty() {
            self.root.clone()
        } else {
            self.root.join(rel)
        };
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            // `DirEntry::metadata` does not follow a symlink, which is what
            // keeps a link out of the vault from being walked as a directory.
            let meta = entry.metadata()?;
            out.push(WalkEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                stat: file_stat(&meta),
            });
        }
        Ok(out)
    }
}

/// Walk the vault, returning every markdown note with its `lstat`.
///
/// `.obsidian/` is skipped **by name, before descent** — the directory is never
/// listed, so nothing inside it is opened or stat'd. So are `.keeper/`, which is
/// keeper's own cache rather than vault content, and `.git/`. Everything else
/// goes through keeper's own [`ExcludeSet`], so an exclusion rule means the same
/// thing to notes as it does to sync.
fn walk(fs: &mut dyn VaultWalk, excludes: &ExcludeSet) -> Vec<Seen> {
    let mut out = Vec::new();
    let mut queue = vec![String::new()];
    while let Some(dir) = queue.pop() {
        let entries = match fs.list(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                // An unreadable directory is one the user cannot read either;
                // the rest of the vault still indexes.
                tracing::debug!(dir, %error, "notes: could not list a vault directory");
                continue;
            }
        };
        for entry in entries {
            // Refused on the NAME, before the path is even built.
            if entry.is_dir && is_refused_dir(&entry.name) {
                continue;
            }
            let rel = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir}/{}", entry.name)
            };
            if excludes.is_excluded(Path::new(&rel)) {
                continue;
            }
            if entry.is_dir {
                queue.push(rel);
            } else if rel.ends_with(".md") {
                out.push(Seen {
                    rel,
                    stat: entry.stat,
                });
            }
        }
    }
    out
}

/// Directory names the walk never descends into.
fn is_refused_dir(name: &str) -> bool {
    name == OBSIDIAN_DIR || name == KEEPER_DIR || name == ".git"
}

/// The stat fields, from a `Metadata` the caller already paid for.
#[cfg(unix)]
fn file_stat(meta: &std::fs::Metadata) -> FileStat {
    use std::os::unix::fs::MetadataExt as _;
    FileStat {
        size: meta.len(),
        // Nanosecond resolution matters: a one-second mtime cannot tell a note
        // edited twice in the same second from one edited once.
        mtime_ns: i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
        ino: meta.ino(),
    }
}

/// No inode on this platform, so revalidation rests on size + mtime alone. That
/// is weaker, and the honest consequence is a slightly higher chance of adopting
/// a stale parse — never of losing a note, because the cache is advisory (AD-57).
#[cfg(not(unix))]
fn file_stat(meta: &std::fs::Metadata) -> FileStat {
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|when| when.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| i128::from(since.as_nanos()));
    FileStat {
        size: meta.len(),
        mtime_ns,
        ino: 0,
    }
}

// ---------------------------------------------------------------------------
// The advisory cache
// ---------------------------------------------------------------------------

/// Read `<vault>/.keeper/index.json`, or an empty set to cold-scan.
fn load_cache(vault: &Vault) -> Vec<IndexEntry> {
    let path = vault.keeper_dir().join("index.json");
    let Ok(bytes) = std::fs::read(&path) else {
        // Absent or unreadable — the ordinary first-run case, and a read-only
        // mount. Not an error; just a slower start.
        return Vec::new();
    };
    adopt_cache(&bytes, &vault.id).unwrap_or_else(|| {
        tracing::info!(
            vault = %vault.id,
            "notes: index cache discarded; rescanning (deleting .keeper/ is a supported repair)"
        );
        Vec::new()
    })
}

/// The pure half of the cache load: bytes in, entries or `None` out.
///
/// A bad schema, a mismatched `vault_id` (a cache copied to another machine), a
/// truncated file and a wrong-shaped document all take **exactly one branch** —
/// discard and rescan. Never an error, never a user-visible failure (AD-57).
fn adopt_cache(bytes: &[u8], vault_id: &str) -> Option<Vec<IndexEntry>> {
    let cache: IndexCache = serde_json::from_slice(bytes).ok()?;
    if cache.schema != INDEX_SCHEMA || cache.vault_id != vault_id {
        return None;
    }
    Some(cache.entries)
}

/// Whether a cached entry still describes the file the walk just stat'd.
fn cache_hit(entry: &IndexEntry, stat: &FileStat) -> bool {
    entry.size == stat.size && entry.mtime_ns == stat.mtime_ns && entry.ino == stat.ino
}

/// What a cache load owes the scan.
#[derive(Debug, Default)]
struct ScanPlan {
    /// Cached parses the `lstat` vouched for.
    adopt: Vec<IndexEntry>,
    /// Notes that must be read: new, or changed since the cache was written.
    parse: Vec<Seen>,
}

/// Split what the walk saw against what the cache claims. Pure, so the
/// adopt / re-parse / drop rule is asserted without a vault.
///
/// A file present on disk and absent from the cache is parsed; a cache entry
/// whose file is gone is dropped; a matching `(size, mtime_ns, ino)` adopts the
/// cached parse and a mismatch re-parses that one note.
fn plan_scan(cached: Vec<IndexEntry>, seen: Vec<Seen>) -> ScanPlan {
    let mut by_path: HashMap<String, IndexEntry> = cached
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let mut plan = ScanPlan::default();
    for note in seen {
        match by_path.remove(&note.rel) {
            Some(entry) if cache_hit(&entry, &note.stat) => plan.adopt.push(entry),
            _ => plan.parse.push(note),
        }
    }
    // Whatever is left in `by_path` describes a file that is no longer there, so
    // it is simply not carried forward.
    plan
}

/// Write the cache, best-effort.
///
/// A failure is a `warn` and never a surfaced error: a vault on a read-only
/// mount must still work, just slower to start.
fn write_cache(vault: &Vault, state: &ReconcilerState) {
    let dir = vault.keeper_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%error, "notes: could not create .keeper/; no warm start for this vault");
        return;
    }
    let cache = IndexCache {
        schema: INDEX_SCHEMA,
        vault_id: vault.id.clone(),
        built_ms: now_ms(),
        entries: state.entries.values().cloned().collect(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        tracing::warn!("notes: could not serialise the index cache");
        return;
    };
    if let Err(error) = atomic_write(&dir.join("index.json"), &bytes) {
        tracing::warn!(%error, "notes: could not write the index cache");
    }
}

// ---------------------------------------------------------------------------
// Reading and parsing
// ---------------------------------------------------------------------------

/// Read and parse `to_parse` on the blocking pool, bounded.
///
/// Bounded because an unbounded fan-out over 10 000 files exhausts file
/// descriptors and thrashes the page cache; the lane count is
/// `min(8, available_parallelism())`, so at most that many reads are ever in
/// flight (NFR-28).
async fn read_and_parse(
    vault: &Vault,
    to_parse: Vec<Seen>,
    progress: &watch::Sender<NoteIndexProgressVm>,
    total: u32,
    already: u32,
) -> Vec<IndexEntry> {
    if to_parse.is_empty() {
        return Vec::new();
    }
    let lanes = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8);
    let per_lane = to_parse.len().div_ceil(lanes);
    let now = now_ms();
    let mut set = tokio::task::JoinSet::new();
    for chunk in to_parse.chunks(per_lane) {
        let chunk: Vec<Seen> = chunk.to_vec();
        let root = vault.root.clone();
        set.spawn_blocking(move || {
            let mut out = Vec::with_capacity(chunk.len());
            for note in chunk {
                let Some(text) = read_bounded(&root.join(&note.rel)) else {
                    continue;
                };
                out.push(parse_note(&note.rel, &note.stat, &text, now));
            }
            out
        });
    }
    let mut entries = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(chunk) => entries.extend(chunk),
            Err(error) => {
                tracing::warn!(%error, "notes: a parse lane failed; its notes are absent");
            }
        }
        let _ = progress.send(NoteIndexProgressVm {
            vault_id: vault.id.clone(),
            scanned: already.saturating_add(u32::try_from(entries.len()).unwrap_or(u32::MAX)),
            total_estimate: total,
            phase: PHASE_PARSING.to_owned(),
        });
    }
    entries
}

/// Read a note, truncated at [`MAX_INDEXED_BODY`].
///
/// Lossy UTF-8 rather than a refusal: a note with one bad byte is still the
/// user's note, and refusing to index it would violate the spirit of NFR-30.
fn read_bounded(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let capped = bytes.get(..MAX_INDEXED_BODY).unwrap_or(bytes.as_slice());
    Some(String::from_utf8_lossy(capped).into_owned())
}

/// Turn bytes into an [`IndexEntry`] through the pure core. No IO here.
fn parse_note(rel: &str, stat: &FileStat, text: &str, now_ms: i64) -> IndexEntry {
    let (fm, body_offset) = Frontmatter::parse(text);
    let body = text.get(body_offset..).unwrap_or("");
    let mut flags: Vec<String> = Vec::new();

    // A note whose frontmatter keeper cannot parse is indexed, not skipped: the
    // note is the user's and the index is ours (AD-55). Its body stays
    // searchable and the properties panel shows the located complaint.
    if fm.unparsed().is_some() {
        flags.push("unparsed".to_owned());
    }
    if fm.as_bool("pinned").unwrap_or(false) {
        flags.push("pinned".to_owned());
    }
    if fm.as_bool("archived").unwrap_or(false) {
        flags.push("archived".to_owned());
    }
    if conflict_origin(rel).is_some() {
        flags.push("conflict".to_owned());
    }
    if rel.starts_with("templates/") {
        flags.push("template".to_owned());
    }
    if rel.starts_with("spaces/") {
        flags.push("space".to_owned());
    }
    if rel.starts_with("journal/") {
        flags.push("journal".to_owned());
    }
    if is_capture(&fm) {
        flags.push("capture".to_owned());
    }
    if usize::try_from(stat.size).unwrap_or(usize::MAX) > MAX_INDEXED_BODY {
        flags.push("oversize".to_owned());
    }

    // A note that already carries a non-ULID `id` keeps it — keeper does not
    // rewrite frontmatter it did not author (FR-121) — and is indexed under a
    // path-derived identity instead, flagged so the UI can say that its pins and
    // unread marks will not survive a rename.
    let id = match fm.as_string("id") {
        Some(id) if is_ulid(id) => id.to_owned(),
        _ => {
            flags.push("unstable_identity".to_owned());
            format!("path:{rel}")
        }
    };

    let mut fields = BTreeMap::new();
    for key in fm.keys() {
        if let Some(value) = fm.get(key) {
            fields.insert(key.to_owned(), value.index_string());
        }
    }
    let created_ms = fields
        .get("created")
        .and_then(|raw| parse_timestamp_ms(raw))
        .unwrap_or_else(|| mtime_ms(stat, now_ms));
    let updated_ms = fields
        .get("updated")
        .and_then(|raw| parse_timestamp_ms(raw))
        .unwrap_or_else(|| mtime_ms(stat, now_ms));

    IndexEntry {
        id,
        path: rel.to_owned(),
        title: note_title(&fm, body, rel),
        size: stat.size,
        mtime_ns: stat.mtime_ns,
        ino: stat.ino,
        created_ms,
        updated_ms,
        tags: tags::note_tags(&fm, body),
        fields,
        links: links::extract(body)
            .into_iter()
            .map(|link| link.target)
            .collect(),
        flags,
        snippet: snippet(body),
    }
}

/// Whether a note was born in the capture panel — `keeper.capture: true`, the
/// reserved namespace's one-level nesting, so the inbox lens can find unfiled
/// thoughts.
fn is_capture(fm: &Frontmatter) -> bool {
    match fm.get("keeper") {
        Some(FieldValue::Map(pairs)) => pairs
            .iter()
            .any(|(key, value)| key == "capture" && matches!(value, FieldValue::Bool(true))),
        _ => false,
    }
}

/// A note's title: an explicit frontmatter `title`, else the first heading or
/// line of the body, else the filename stem — which is what Obsidian shows.
fn note_title(fm: &Frontmatter, body: &str, rel: &str) -> String {
    if let Some(title) = fm.as_string("title") {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    let from_body = naming::title_from_body(body);
    if from_body.trim().is_empty() {
        stem(rel).to_owned()
    } else {
        from_body
    }
}

/// The first [`SNIPPET_CHARS`] characters of body, whitespace folded.
fn snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(SNIPPET_CHARS)
        .collect()
}

/// The filename stem of a vault-relative path.
fn stem(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.strip_suffix(".md").unwrap_or(name)
}

/// Whether a string is a ULID: 26 Crockford base32 characters.
///
/// Checked rather than assumed, because `id` is a top-level frontmatter key that
/// an agent or another tool may already be using for something else entirely.
fn is_ulid(value: &str) -> bool {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    value.len() == 26
        && value
            .bytes()
            .all(|byte| ALPHABET.contains(&byte.to_ascii_uppercase()))
}

/// An RFC-3339 timestamp as epoch milliseconds, or a bare `YYYY-MM-DD` at local
/// midnight. `None` for anything else — a malformed date is not an error.
fn parse_timestamp_ms(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if let Ok(when) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(when.timestamp_millis());
    }
    let naive = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    Some(
        naive
            .and_local_timezone(chrono::Local)
            .earliest()
            .map_or_else(
                || naive.and_utc().timestamp_millis(),
                |local| local.timestamp_millis(),
            ),
    )
}

/// The file's mtime in milliseconds, falling back to `now` for a clock that
/// predates the epoch.
fn mtime_ms(stat: &FileStat, now_ms: i64) -> i64 {
    i64::try_from(stat.mtime_ns / 1_000_000)
        .ok()
        .filter(|ms| *ms > 0)
        .unwrap_or(now_ms)
}

/// Wall-clock milliseconds since the epoch, clamped rather than panicking.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Local wall clock as epoch milliseconds — what the query DSL's `today` and
/// `-14d` specs resolve against.
///
/// `keeper-core` owns no timezone database and no clock, so the offset is applied
/// here: pass raw UTC and a user in UTC+13 gets a space whose meaning changes at
/// 11:00.
pub fn local_now_ms() -> i64 {
    let offset_ms = i64::from(chrono::Local::now().offset().local_minus_utc()) * 1_000;
    now_ms().saturating_add(offset_ms)
}

// ---------------------------------------------------------------------------
// Orphans and the inbound-link map
// ---------------------------------------------------------------------------

/// Which notes emit each link token.
///
/// Tokens are normalised by the core's own `link_key`, so this map and
/// `IndexSnapshot::resolve_link` cannot disagree about what names a note — a
/// second folding rule here would be exactly the drift that makes `is:orphan`
/// and `backlink:` answer differently about the same pair.
fn build_inbound(entries: &HashMap<String, IndexEntry>) -> HashMap<String, HashSet<String>> {
    let mut inbound: HashMap<String, HashSet<String>> = HashMap::new();
    for entry in entries.values() {
        record_links(&mut inbound, entry);
    }
    inbound
}

/// Record that `entry` emits each of its outbound link tokens.
fn record_links(inbound: &mut HashMap<String, HashSet<String>>, entry: &IndexEntry) {
    for link in &entry.links {
        inbound
            .entry(link_key(link))
            .or_default()
            .insert(entry.path.clone());
    }
}

fn retract_links(inbound: &mut HashMap<String, HashSet<String>>, entry: &IndexEntry) {
    for link in &entry.links {
        let token = link_key(link);
        if let Some(sources) = inbound.get_mut(&token) {
            sources.remove(&entry.path);
            if sources.is_empty() {
                inbound.remove(&token);
            }
        }
    }
}

/// Set or clear the `orphan` flag across the entry set, returning the paths whose
/// flag actually moved.
///
/// `is:orphan` is a whole-index fact and `query::eval` sees one entry at a time,
/// so the flag is where the answer has to live. A note that links to itself is
/// still an orphan — a self-reference is not an inbound link from anywhere.
///
/// The changed paths are returned rather than applied here because the
/// `IndexBuilder` owns the posting lists: every entry this touches has to go back
/// through an `Upsert`, and only the ones that moved should pay for it.
fn apply_orphan_flags(state: &mut ReconcilerState) -> Vec<String> {
    let inbound = &state.inbound;
    let mut moved = Vec::new();
    for entry in state.entries.values_mut() {
        let linked = entry.link_keys().into_iter().any(|key| {
            inbound
                .get(&key)
                .is_some_and(|sources| sources.iter().any(|source| source != &entry.path))
        });
        let flagged = entry.flags.iter().any(|flag| flag == "orphan");
        if linked && flagged {
            entry.flags.retain(|flag| flag != "orphan");
            moved.push(entry.path.clone());
        } else if !linked && !flagged {
            entry.flags.push("orphan".to_owned());
            moved.push(entry.path.clone());
        }
    }
    moved
}

/// Re-project the named entries through the builder.
///
/// The builder owns the tag posting lists and the backlink map, so an entry
/// mutated in the mirror has to be handed back as an `Upsert` — editing a
/// snapshot around the builder would leave those indexes describing a vault that
/// no longer exists.
fn reapply(state: &mut ReconcilerState, paths: &[String]) {
    for path in paths {
        if let Some(entry) = state.entries.get(path).cloned() {
            state.builder.apply(NoteDelta::Upsert(Box::new(entry)));
        }
    }
}

// ---------------------------------------------------------------------------
// Reading and writing notes
// ---------------------------------------------------------------------------

/// Read a note's full text — uncapped, because the editor gets what is on disk.
pub fn read_note(vault: &Vault, rel: &str) -> Result<String, NotesError> {
    let path = contained(vault, rel)?;
    std::fs::read_to_string(&path).map_err(|error| NotesError::NotFound(format!("{rel}: {error}")))
}

/// Write a note atomically.
///
/// Temp + rename in the same directory, under the `.keeper.<ulid>.tmp` name that
/// is already a tier-0 exclusion — so a torn temp file can never be staged, and
/// a `kill -9` between write and rename leaves no partial note in the vault
/// (NFR-30).
///
/// **Where the echo is suppressed.** The write is deliberately NOT hidden from
/// the sync watcher: hiding it would also hide it from the thing that commits
/// it, and the engine's own `EchoSuppressor` is reachable only from inside the
/// engine. So keeper's write does come back as a watcher event, the reconciler
/// re-reads the file (correct — the index must reflect it), and the echo is
/// stopped one layer up, in `notes_ipc`: a body subscription compares the disk
/// revision against the revision it last delivered or wrote, so an echo reaches
/// no editor as an external change. That comparison is strictly safer than
/// muting a path, because a real external write that lands inside the muting
/// window is not swallowed by it.
pub fn write_note(vault: &Vault, rel: &str, text: &str) -> Result<(), NotesError> {
    let path = contained(vault, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| NotesError::Name(format!("{rel}: {error}")))?;
    }
    atomic_write(&path, text.as_bytes())
        .map_err(|error| NotesError::Name(format!("{rel}: {error}")))?;
    touch(&vault.id, vec![rel.to_owned()]);
    mark_dirty(&vault.id);
    Ok(())
}

/// Write `bytes` to `path` through a temp file in the same directory.
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = dir.join(format!(".keeper.{}.tmp", crate::sync_ipc::new_ulid()));
    std::fs::write(&temp, bytes)?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // A failed rename must not leave the temp behind: it is excluded
            // from sync, but it is still litter in the user's vault.
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Move a note into `<vault>/.keeper/trash/<ulid>/<original-path>` (NFR-30).
///
/// Never an `unlink`. The bytes stay recoverable locally *and* from history, and
/// the removal is what the next cadence tick stages — so the commit that deletes
/// the note is preceded by one that still holds it.
pub fn trash_note(vault: &Vault, rel: &str) -> Result<PathBuf, NotesError> {
    let path = contained(vault, rel)?;
    let grave = vault
        .keeper_dir()
        .join("trash")
        .join(crate::sync_ipc::new_ulid())
        .join(rel);
    if let Some(parent) = grave.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| NotesError::Name(format!("trash {rel}: {error}")))?;
    }
    let moved = std::fs::rename(&path, &grave).or_else(|_| {
        // `.keeper/` is inside the vault, so a cross-device rename is unusual —
        // but copy-then-remove keeps the promise either way.
        std::fs::copy(&path, &grave).and_then(|_| std::fs::remove_file(&path))
    });
    moved.map_err(|error| NotesError::Name(format!("trash {rel}: {error}")))?;
    touch(&vault.id, vec![rel.to_owned()]);
    mark_dirty(&vault.id);
    Ok(grave)
}

/// Rename a note inside the vault, keeping its bytes.
///
/// A rename is not a delete: nothing is lost, so this needs no trash copy — the
/// note's ULID `id` is what keeps its links, pins and unread marks intact across
/// the new filename (FR-97), and git follows the content.
///
/// Refuses to overwrite an existing note. The caller resolves a free name first
/// (`naming::note_filename` over the sibling set), so a collision here means two
/// renames raced, and silently clobbering the loser would lose a note.
pub fn rename_note(vault: &Vault, from_rel: &str, to_rel: &str) -> Result<(), NotesError> {
    let from = contained(vault, from_rel)?;
    let to = contained(vault, to_rel)?;
    if from == to {
        return Ok(());
    }
    if to.exists() {
        return Err(NotesError::Name(format!("{to_rel} already exists")));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| NotesError::Name(format!("{to_rel}: {error}")))?;
    }
    std::fs::rename(&from, &to)
        .map_err(|error| NotesError::Name(format!("{from_rel} -> {to_rel}: {error}")))?;
    touch(&vault.id, vec![from_rel.to_owned(), to_rel.to_owned()]);
    mark_dirty(&vault.id);
    Ok(())
}

/// Resolve a vault-relative path, refusing anything that leaves the vault.
///
/// The same containment rule the `keeper-note://` handler applies (AD-59), for
/// the same reason: a note is agent-authored text, and `../../../../etc/passwd`
/// is one line an autonomous writer can emit by accident. This is the LEXICAL
/// half — every component must be a plain name — because a create resolves a
/// path that does not exist yet and cannot be canonicalized. Reads that can be
/// canonicalized are contained again in `note_protocol`, which is where a
/// symlink out of the vault is caught.
pub fn contained(vault: &Vault, rel: &str) -> Result<PathBuf, NotesError> {
    let relative = Path::new(rel);
    if rel.is_empty() || rel.contains('\0') || relative.is_absolute() {
        return Err(NotesError::Name(format!("not a vault path: {rel}")));
    }
    // `Component::Normal` excludes `..`, `.`, a root and a Windows prefix in one
    // rule, so no separator or encoding trick reaches the join.
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(NotesError::Name(format!("not a vault path: {rel}")));
    }
    if is_internal(rel) {
        return Err(NotesError::Name(format!("keeper does not touch {rel}")));
    }
    Ok(vault.root.join(relative))
}

/// Import a file into `attachments/`, returning what the editor should insert.
///
/// The bytes never cross IPC in either direction (AD-58): the webview hands over
/// a path Tauri's own drag-drop event gave it, or nothing at all for a clipboard
/// paste, and Rust does the reading.
pub fn import_attachment(vault: &Vault, source: &Path) -> Result<NoteAttachmentVm, NotesError> {
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| NotesError::Name("attachment has no file name".to_owned()))?;
    let dir = vault.root.join(ATTACHMENTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| NotesError::Name(format!("attachments/: {error}")))?;
    let chosen = unique_name(&name, &siblings(vault, ATTACHMENTS_DIR));
    let rel = format!("{ATTACHMENTS_DIR}/{chosen}");
    let target = dir.join(&chosen);
    std::fs::copy(source, &target).map_err(|error| NotesError::Name(format!("{rel}: {error}")))?;
    mark_dirty(&vault.id);
    Ok(NoteAttachmentVm {
        markdown: attachment_markdown(&rel, &chosen),
        url: asset_url(&vault.id, &rel),
        rel_path: rel,
    })
}

/// A collision-free name inside a directory: `shot.png`, `shot-2.png`, …
///
/// Pure over the sibling set, exactly like `naming::note_filename` — an
/// attachment is not a note, so it gets the same rule and not the same function.
fn unique_name(name: &str, taken: &[String]) -> String {
    let is_taken = |candidate: &str| {
        taken
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(candidate))
    };
    if !is_taken(name) {
        return name.to_owned();
    }
    let (stem, extension) = match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    };
    for counter in 2..10_000 {
        let candidate = format!("{stem}-{counter}{extension}");
        if !is_taken(&candidate) {
            return candidate;
        }
    }
    // Ten thousand files of one name is not a real vault, but a name is still
    // owed; a ULID cannot collide with the counters above.
    format!("{stem}-{}{extension}", crate::sync_ipc::new_ulid())
}

/// The markdown an imported attachment inserts: an embed for an image, an
/// ordinary link otherwise.
fn attachment_markdown(rel: &str, name: &str) -> String {
    if is_image(rel) {
        format!("![{name}]({rel})")
    } else {
        format!("[{name}]({rel})")
    }
}

fn is_image(rel: &str) -> bool {
    matches!(
        extension(rel).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "bmp")
    )
}

/// A path's lowercased extension.
pub fn extension(rel: &str) -> Option<String> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let index = name.rfind('.')?;
    (index > 0 && index + 1 < name.len()).then(|| name[index + 1..].to_lowercase())
}

/// The characters that survive a path segment unescaped: RFC 3986's unreserved
/// set. Keeping `.`, `-`, `_` and `~` legible is what stops every ordinary
/// filename turning into `a%2Db%2Epng` in the DOM and in a log line.
const ASSET_SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The `keeper-note://` URL for a vault-relative asset.
///
/// Segment by segment, so a `/` in the path stays a separator and everything
/// else is escaped. The one exception is a dot segment: `.` and `..` are encoded
/// whole, because they are the only two names whose meaning a URL resolver would
/// change. The scheme handler refuses a traversal on resolution regardless — this
/// is the belt to that pair of braces, and it means a traversal attempt reaches
/// the log as visible text rather than as a path that already collapsed.
pub fn asset_url(vault_id: &str, rel: &str) -> String {
    let encoded: Vec<String> = rel
        .split('/')
        .map(|segment| {
            if segment == "." || segment == ".." {
                return segment.replace('.', "%2E");
            }
            percent_encoding::utf8_percent_encode(segment, ASSET_SEGMENT).to_string()
        })
        .collect();
    format!("keeper-note://{vault_id}/{}", encoded.join("/"))
}

/// The sibling file names of a vault-relative directory, for the collision
/// counter `naming::note_filename` applies.
pub fn siblings(vault: &Vault, rel_dir: &str) -> Vec<String> {
    let dir = if rel_dir.is_empty() {
        vault.root.clone()
    } else {
        vault.root.join(rel_dir)
    };
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Today's journal path for a vault, from its configured template.
pub fn journal_rel(vault: &Vault) -> String {
    let today = chrono::Local::now().date_naive();
    let template = if vault.config.journal_template.trim().is_empty() {
        naming::DEFAULT_JOURNAL_TEMPLATE
    } else {
        vault.config.journal_template.as_str()
    };
    naming::journal_path(
        template,
        chrono::Datelike::year(&today),
        chrono::Datelike::month(&today),
        chrono::Datelike::day(&today),
    )
}

/// An opaque content fingerprint, used as a document revision.
///
/// It answers exactly one question — "is what is on disk still what we handed
/// out?" — and it is FNV-1a over the bytes with the length mixed in, not a
/// cryptographic digest. It is deliberately not a security boundary: an agent
/// that wants to lose your bytes already has write access to the file, and what
/// covers that is the conflict copy, not this.
pub fn content_rev(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:x}-{hash:016x}", text.len())
}

// ---------------------------------------------------------------------------
// Conflict copies
// ---------------------------------------------------------------------------

/// The canonical note a conflict copy belongs to, or `None` for an ordinary
/// note.
///
/// Recognised by **name shape** — `<stem>.sync-conflict-<timestamp>-<device>.md`
/// — because a conflict copy is an ordinary tracked file that arrives from the
/// sync engine (AD-43) or from another tool that spells conflicts the same way.
/// A hyphen in a note's own name is not a conflict: the marker is a
/// dot-delimited component beginning `sync-conflict-`.
pub fn conflict_origin(rel: &str) -> Option<String> {
    let (dir, name) = match rel.rfind('/') {
        Some(index) => (&rel[..=index], &rel[index + 1..]),
        None => ("", rel),
    };
    let mut parts = name.split('.');
    let stem = parts.next()?;
    if stem.is_empty() {
        // A dotfile: the leading dot is not a separator.
        return None;
    }
    if !parts.next()?.starts_with("sync-conflict-") {
        return None;
    }
    // `conflict_name` preserves the extension when there was one; a copy of a
    // file that had none is still a note as far as the vault is concerned.
    let extension = parts.next().unwrap_or("md");
    Some(format!("{dir}{stem}.{extension}"))
}

/// Every conflict copy currently in a vault, paired with its canonical note.
pub fn conflicts(vault: &Vault) -> Vec<(String, String)> {
    let Some(snapshot) = snapshot(&vault.id) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = snapshot
        .entries()
        .iter()
        .filter_map(|entry| {
            conflict_origin(&entry.path).map(|canonical| (canonical, entry.path.clone()))
        })
        .collect();
    out.sort();
    out
}

/// Write the current disk bytes aside as an AD-43-shaped conflict copy, before
/// an overwrite that would otherwise lose them.
///
/// The copy is an ordinary tracked file, so it becomes a conflict row (FR-116)
/// and a commit on the next cadence tick — which is the concrete answer to
/// "where does a diverged save put the other side": beside the note, and in
/// history, never nowhere.
pub fn write_conflict_copy(vault: &Vault, rel: &str, theirs: &str) -> Option<String> {
    let device = crate::sync::engine_if_open()
        .map_or_else(|| "device".to_owned(), |engine| engine.device().label);
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let name = keeper_sync::git::conflict::conflict_name(Path::new(rel), &stamp, &device);
    let copy_rel = match rel.rfind('/') {
        Some(index) => format!("{}{name}", &rel[..=index]),
        None => name,
    };
    match write_note(vault, &copy_rel, theirs) {
        Ok(()) => Some(copy_rel),
        Err(error) => {
            tracing::warn!(%error, "notes: could not write the conflict copy");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Provenance (AD-63)
// ---------------------------------------------------------------------------

/// The head commit that touched one note, projected from the trailers
/// `keeper-sync` already writes on every commit.
#[derive(Debug, Clone)]
pub struct HeadRevision {
    pub rev: String,
    pub when_ms: i64,
    pub device: String,
    /// `local` | `agent` | `remote` | `unknown` — the `origin:` predicate's
    /// vocabulary, resolved here because only the shell knows which device this
    /// machine is.
    pub origin: String,
    pub source: String,
    pub subject: String,
}

impl HeadRevision {
    /// Project into the view model.
    pub fn vm(&self) -> NoteRevisionVm {
        NoteRevisionVm {
            rev: self.rev.clone(),
            when_ms: self.when_ms,
            device: self.device.clone(),
            origin: self.origin.clone(),
            source: self.source.clone(),
            subject: self.subject.clone(),
        }
    }

    /// Whether this revision came from somewhere the user is not.
    ///
    /// An agent's commit, or any device that is not this one. A commit with no
    /// trailers is `unknown` and deliberately does **not** count as local: a
    /// hand-made `git commit` on another machine is exactly the case the unread
    /// mark exists for.
    pub fn is_foreign(&self) -> bool {
        self.origin != "local"
    }
}

/// Refresh the head-provenance map for a vault, off the reconciler's thread.
fn refresh_heads(app: &AppHandle, vault: &Vault) {
    let app = app.clone();
    let vault = vault.clone();
    tauri::async_runtime::spawn(async move {
        let Ok((vault_id, heads)) =
            tokio::task::spawn_blocking(move || read_heads(&app, &vault)).await
        else {
            return;
        };
        // Hand it to the reconciler rather than storing it here: the entry set
        // is the single mutator's, and stamping `keeper.origin` onto it from a
        // second thread would be the one lock this design does not have.
        if let Some(slot) = registry().get(&vault_id) {
            let _ = slot.work.send(Work::Heads(heads));
        }
    });
}

/// Read the last [`PROVENANCE_COMMITS`] commits touching the vault subfolder and
/// project one head revision per path.
///
/// **One** `git` invocation per publish, not one per note: a revwalk per file
/// over a 10 000-note vault is 10 000 process spawns, and unread state only ever
/// concerns notes that changed recently.
fn read_heads(app: &AppHandle, vault: &Vault) -> (String, HashMap<String, HeadRevision>) {
    let mut heads = HashMap::new();
    let Some(output) = git_out(
        app,
        &vault.local_path,
        &[
            "log".to_owned(),
            "--no-color".to_owned(),
            format!("-n{PROVENANCE_COMMITS}"),
            "--name-only".to_owned(),
            format!("--format={RECORD_SEP}%H{FIELD_SEP}%ct{FIELD_SEP}%s{FIELD_SEP}%B{FIELD_SEP}"),
            "--".to_owned(),
            vault.config.subfolder.clone(),
        ],
    ) else {
        // A vault whose profile has never committed has an honest empty history,
        // not an error (AD-63).
        return (vault.id.clone(), heads);
    };
    let this_device = crate::sync::engine_if_open().map(|engine| engine.device().label);
    let prefix = format!("{}/", vault.config.subfolder);
    for record in output.split(RECORD_SEP).skip(1) {
        let mut fields = record.split(FIELD_SEP);
        let (Some(rev), Some(when), Some(subject), Some(body), Some(paths)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let head = revision_of(rev, when, subject, body, this_device.as_deref());
        for path in paths.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let Some(rel) = path.strip_prefix(&prefix) else {
                continue;
            };
            // First record wins: `git log` is newest-first, so the first commit
            // naming a path IS that path's head.
            heads.entry(rel.to_owned()).or_insert_with(|| head.clone());
        }
    }
    (vault.id.clone(), heads)
}

/// Assemble one revision from the four `git log` fields.
fn revision_of(
    rev: &str,
    when: &str,
    subject: &str,
    body: &str,
    this_device: Option<&str>,
) -> HeadRevision {
    let provenance = Provenance::parse(body);
    HeadRevision {
        rev: rev.trim().to_owned(),
        // `%ct` is committer time in whole seconds.
        when_ms: when
            .trim()
            .parse::<i64>()
            .unwrap_or(0)
            .saturating_mul(1_000),
        device: provenance
            .as_ref()
            .map(|p| p.device_label.clone())
            .unwrap_or_default(),
        origin: origin_of(provenance.as_ref(), this_device),
        source: provenance
            .as_ref()
            .map_or("unknown", |p| p.source.as_str())
            .to_owned(),
        subject: subject.trim().to_owned(),
    }
}

/// The `origin:` vocabulary for one commit's trailers.
fn origin_of(provenance: Option<&Provenance>, this_device: Option<&str>) -> String {
    let Some(provenance) = provenance else {
        // No keeper trailers at all: a hand-made commit. Unknown, not local —
        // claiming a commit we cannot attribute belongs to this machine would
        // make the unread mark lie in exactly the case it exists for.
        return "unknown".to_owned();
    };
    if provenance.source == SyncSource::Bot {
        return "agent".to_owned();
    }
    match this_device {
        Some(device) if device == provenance.device_label => "local".to_owned(),
        Some(_) => "remote".to_owned(),
        None => "unknown".to_owned(),
    }
}

/// Per-note history, rename-following, newest first (FR-114).
///
/// A revwalk per call rather than a cached store: "who changed this note" is a
/// git question and git already has the answer (AD-63).
pub fn revisions(app: &AppHandle, vault: &Vault, rel: &str, limit: u32) -> Vec<NoteRevisionVm> {
    let Some(output) = git_out(
        app,
        &vault.local_path,
        &[
            "log".to_owned(),
            "--no-color".to_owned(),
            // `--follow` is what makes a note's history survive the filename
            // changes its ULID identity already survives (FR-97).
            "--follow".to_owned(),
            format!("-n{}", limit.clamp(1, 1_000)),
            format!("--format={RECORD_SEP}%H{FIELD_SEP}%ct{FIELD_SEP}%s{FIELD_SEP}%B"),
            "--".to_owned(),
            format!("{}/{rel}", vault.config.subfolder),
        ],
    ) else {
        return Vec::new();
    };
    let this_device = crate::sync::engine_if_open().map(|engine| engine.device().label);
    output
        .split(RECORD_SEP)
        .skip(1)
        .filter_map(|record| {
            let mut fields = record.split(FIELD_SEP);
            let (Some(rev), Some(when), Some(subject), Some(body)) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return None;
            };
            Some(revision_of(rev, when, subject, body, this_device.as_deref()).vm())
        })
        .collect()
}

/// The diff of one note between two revisions, or between a revision and the
/// working tree (`to_rev: None`).
pub fn diff(
    app: &AppHandle,
    vault: &Vault,
    rel: &str,
    from_rev: &str,
    to_rev: Option<&str>,
) -> NoteDiffVm {
    let mut args = vec![
        "diff".to_owned(),
        "--no-color".to_owned(),
        "--unified=3".to_owned(),
        from_rev.to_owned(),
    ];
    if let Some(to_rev) = to_rev {
        args.push(to_rev.to_owned());
    }
    args.push("--".to_owned());
    args.push(format!("{}/{rel}", vault.config.subfolder));
    NoteDiffVm {
        hunks: git_out(app, &vault.local_path, &args)
            .map(|output| parse_hunks(&output))
            .unwrap_or_default(),
        from_rev: from_rev.to_owned(),
        to_rev: to_rev.map(str::to_owned),
    }
}

/// Parse the hunks of a unified diff. Pure over the text `git diff` printed.
fn parse_hunks(diff: &str) -> Vec<NoteHunkVm> {
    let mut hunks: Vec<NoteHunkVm> = Vec::new();
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(header) {
                hunks.push(hunk);
            }
            continue;
        }
        // Everything between two headers is the hunk body; the file headers
        // (`diff --git`, `index`, `---`, `+++`) precede the first one and so
        // reach no hunk at all.
        if let Some(current) = hunks.last_mut() {
            current.text.push_str(line);
            current.text.push('\n');
        }
    }
    hunks
}

/// `-12,4 +12,6 @@ trailing context` → the four numbers.
fn parse_hunk_header(header: &str) -> Option<NoteHunkVm> {
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let (old_start, old_lines) = split_span(old)?;
    let (new_start, new_lines) = split_span(new)?;
    Some(NoteHunkVm {
        old_start,
        old_lines,
        new_start,
        new_lines,
        text: String::new(),
    })
}

/// `12,4` → `(12, 4)`. A bare `12` means one line, which is how unified diff
/// spells a single-line span.
fn split_span(span: &str) -> Option<(u32, u32)> {
    match span.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((span.parse().ok()?, 1)),
    }
}

/// Run the resolved `git` in `repo` and return stdout, or `None` on any failure.
///
/// The shell resolves `git` already (`sync::git_resolution`), and history is a
/// **read**: there is no state to corrupt and nothing to journal, so this needs
/// no new engine API and `keeper` needs no gitoxide dependency of its own. A
/// failure — no git, no repository, a path with no history — is `None`, because
/// an empty history is an honest answer and a missing one is not an error.
fn git_out(app: &AppHandle, repo: &Path, args: &[String]) -> Option<String> {
    let platform = Arc::clone(&app.state::<crate::ipc::AppState>().platform);
    let program = crate::sync::git_resolution(platform.as_ref())
        .program()
        .ok()?;
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(repo)
        // git must never try to ask a human anything from inside keeper.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        tracing::debug!(code = ?output.status.code(), "notes: a git read returned non-zero");
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ---------------------------------------------------------------------------
// The cadence (AD-62)
// ---------------------------------------------------------------------------

/// Where a vault is in the cadence state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    /// Nothing local to commit.
    #[default]
    Idle,
    /// Local changes are settling towards `commit_idle_ms`.
    Dirty,
    /// Committed locally, waiting for its push deadline.
    Ahead,
}

/// One vault's cadence.
///
/// A `Mutex` around four scalars rather than atomics, because the transitions
/// have to be read and written together: a tick that saw `Dirty` and a change
/// that arrived between the read and the write must not lose either fact.
#[derive(Debug, Default)]
struct Cadence {
    inner: Mutex<CadenceState>,
}

#[derive(Debug, Clone, Copy, Default)]
struct CadenceState {
    phase: Phase,
    /// When the vault last changed. The debounce is measured from the LAST
    /// change, not the first, so a typing burst produces one commit.
    last_change_ms: i64,
    /// When a push becomes due.
    push_deadline_ms: i64,
    /// Whether a commit or push is in flight for this vault. One at a time: the
    /// engine already serializes per profile, and asking twice only queues work
    /// behind itself.
    in_flight: bool,
}

impl Cadence {
    fn lock(&self) -> MutexGuard<'_, CadenceState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A vault path changed.
    fn mark_dirty(&self) {
        let mut state = self.lock();
        state.phase = Phase::Dirty;
        state.last_change_ms = now_ms();
    }

    /// An action finished. `ahead` means the vault now has local commits its
    /// remote does not, which is what schedules the push.
    fn finish(&self, ahead: bool, push_interval_ms: u64) {
        let mut state = self.lock();
        state.in_flight = false;
        // A change that arrived while the action ran left the phase `Dirty`;
        // that must survive, or the note that arrived mid-commit waits for the
        // next unrelated edit.
        if state.phase == Phase::Dirty {
            return;
        }
        if ahead {
            state.phase = Phase::Ahead;
            state.push_deadline_ms =
                now_ms().saturating_add(i64::try_from(push_interval_ms).unwrap_or(i64::MAX));
        } else {
            state.phase = Phase::Idle;
        }
    }
}

/// What a tick owes a vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Nothing due.
    None,
    /// The idle window closed: ask the engine to notice and stage what settled.
    /// The four-tier gate still decides whether a path may be staged — the
    /// cadence only decides when keeper asks (AD-62).
    Commit,
    /// The push deadline passed, or a flush was forced.
    Push,
}

/// The pure cadence decision, separated from the side effect so the state
/// machine is testable against a fixed clock rather than a real one.
fn decide(state: &CadenceState, cadence: &NotesCadence, now_ms: i64, forced: bool) -> Action {
    if state.in_flight {
        return Action::None;
    }
    match state.phase {
        Phase::Dirty => {
            let idle_ms = now_ms.saturating_sub(state.last_change_ms);
            if forced || idle_ms >= i64::try_from(cadence.commit_idle_ms).unwrap_or(i64::MAX) {
                Action::Commit
            } else {
                Action::None
            }
        }
        Phase::Ahead if forced || now_ms >= state.push_deadline_ms => Action::Push,
        Phase::Ahead | Phase::Idle => Action::None,
    }
}

/// Mark a vault dirty from outside the watcher tap — keeper's own writes.
pub fn mark_dirty(vault_id: &str) {
    if let Some(slot) = registry().get(vault_id) {
        slot.cadence.mark_dirty();
    }
}

/// Evaluate the cadence for every vault.
///
/// Called from the ~1 Hz tick that already drives the tray, which is the whole
/// of AD-62: a 1 Hz tick has exactly the resolution a 2 s idle-commit needs, and
/// two schedulers over one git repository is how you get concurrent index locks.
pub fn cadence_tick() {
    dispatch_cadence(false);
}

/// Force every vault's outstanding work forward: the main window hiding, the
/// capture panel hiding after a capture, or a blur on a profile that asked for
/// `push_on_blur`.
///
/// The user walking away is the strongest available signal that the other
/// machine wants these bytes.
pub fn flush() {
    dispatch_cadence(true);
}

/// Run the cadence over every vault, optionally forcing.
///
/// Takes no app handle on purpose: the vault registry and the sync engine are
/// both process-global, so threading one through would be decoration that the
/// next reader has to check.
fn dispatch_cadence(forced: bool) {
    let now = now_ms();
    let mut due: Vec<(String, Action, u64)> = Vec::new();
    {
        let guard = registry();
        for slot in guard.values() {
            let cadence = &slot.vault.config.cadence;
            let mut state = slot.cadence.lock();
            // A profile that asked not to push on blur is not forced into a
            // push; a forced tick still commits, because a local commit costs
            // nothing and needs no network.
            let force_this = forced && (cadence.push_on_blur || state.phase != Phase::Ahead);
            let action = decide(&state, cadence, now, force_this);
            if action != Action::None {
                state.in_flight = true;
                due.push((slot.vault.id.clone(), action, cadence.push_interval_ms));
            }
        }
    }
    for (vault_id, action, push_interval_ms) in due {
        tauri::async_runtime::spawn(async move {
            let ahead = run_cadence_action(&vault_id, action).await;
            if let Some(slot) = registry().get(&vault_id) {
                slot.cadence.finish(ahead, push_interval_ms);
            }
        });
    }
}

/// Perform one cadence action, reporting whether the vault is now ahead of its
/// remote.
async fn run_cadence_action(vault_id: &str, action: Action) -> bool {
    let Some(engine) = crate::sync::engine_if_open() else {
        return false;
    };
    match action {
        Action::Commit => {
            // `rescan` is how the engine is asked to notice *now*: its own next
            // pass walks the tree, applies the stability gate and commits what
            // settled. No second committer, and no second scheduler.
            if let Err(error) = engine.rescan(vault_id) {
                tracing::debug!(%error, "notes: could not request a rescan for the cadence");
                return false;
            }
            true
        }
        Action::Push => {
            if let Err(error) = engine.sync_once(vault_id, SyncSource::Watch).await {
                // Offline is the ordinary case, not a fault: the commit already
                // happened locally, and the push is a journaled unit with
                // backoff (AD-49). Staying `Ahead` is what retries it.
                tracing::debug!(%error, "notes: cadence push deferred");
                return true;
            }
            false
        }
        Action::None => false,
    }
}

/// The bound a quit flush gets. Long enough for a local commit on a warm
/// repository, short enough that quit is never stuck.
const QUIT_FLUSH_BOUND: Duration = Duration::from_secs(3);

/// Force every vault's work forward on the quit path, bounded.
///
/// Runs inside the existing graceful finalize (AD-39/AD-52) and before the
/// supervisor is signalled to stop. A flush that exceeds the bound loses
/// nothing: the commit already happened locally, and an outstanding push is a
/// journal row the next launch drains (AD-49).
pub fn flush_for_quit() {
    let vault_ids: Vec<String> = registry().keys().cloned().collect();
    if vault_ids.is_empty() {
        return;
    }
    let Some(engine) = crate::sync::engine_if_open() else {
        return;
    };
    let flush = async {
        tokio::time::timeout(QUIT_FLUSH_BOUND, async {
            for vault_id in vault_ids {
                if let Err(error) = engine.sync_once(&vault_id, SyncSource::Manual).await {
                    tracing::debug!(%error, "notes: quit flush left work journaled");
                }
            }
        })
        .await
    };
    if tauri::async_runtime::block_on(flush).is_err() {
        tracing::warn!("notes: quit flush exceeded its bound; the journal covers the remainder");
    }
}

// ---------------------------------------------------------------------------
// Unread state (AD-63)
// ---------------------------------------------------------------------------

/// Whether a note is unread: its head revision came from somewhere the user is
/// not, and the user has not acknowledged that revision.
///
/// `Accept` is what clears the mark (FR-113) — not opening the note — so the
/// acknowledged *revision* is what is compared, never a timestamp.
pub fn is_unread(platform: &dyn Platform, head: Option<&HeadRevision>, note_id: &str) -> bool {
    let Some(head) = head else {
        return false;
    };
    if !head.is_foreign() {
        return false;
    }
    let Ok(data_dir) = platform.data_dir() else {
        return false;
    };
    match keeper_core::registry::notes_read_mark_get(&data_dir, note_id) {
        Ok(acknowledged) => acknowledged.as_deref() != Some(head.rev.as_str()),
        Err(error) => {
            // Losing the mark marks things unread rather than losing data, which
            // is the right direction for an advisory record.
            tracing::warn!(%error, "notes: could not read the acknowledged revision");
            true
        }
    }
}

/// How many notes in a vault are unread.
pub fn unread_count(platform: &dyn Platform, vault_id: &str) -> u32 {
    let Some(snapshot) = snapshot(vault_id) else {
        return 0;
    };
    let heads = heads(vault_id).unwrap_or_default();
    let count = snapshot
        .entries()
        .iter()
        .filter(|entry| is_unread(platform, heads.get(&entry.path), &entry.id))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Acknowledge a revision, clearing the note's unread mark (FR-113).
pub fn mark_read(platform: &dyn Platform, note_id: &str, rev: &str) -> Result<(), NotesError> {
    let data_dir = platform
        .data_dir()
        .map_err(|error| NotesError::Name(error.to_string()))?;
    keeper_core::registry::notes_read_mark_set(&data_dir, note_id, rev)
        .map_err(|error| NotesError::Name(error.to_string()))
}

// ---------------------------------------------------------------------------
// The active vault
// ---------------------------------------------------------------------------

/// The active vault id, or `None` when there is nothing to be active.
///
/// A stored id that is no longer a flagged profile answers `None`: a vault that
/// lost its flag must not stay "active", or the tray would write into a folder
/// the user unflagged. With exactly one vault and no selection, that vault is
/// the answer — so the tray works before anyone visits the switcher.
pub fn active_vault(platform: &dyn Platform) -> Option<String> {
    if let Ok(data_dir) = platform.data_dir() {
        if let Ok(Some(stored)) = keeper_core::registry::get_active_vault(&data_dir) {
            if vault(&stored).is_some() {
                return Some(stored);
            }
        }
    }
    let mut all = vaults();
    (all.len() == 1).then(|| all.remove(0).id)
}

/// Select the active vault.
pub fn set_active_vault(platform: &dyn Platform, vault_id: &str) -> Result<(), NotesError> {
    if vault(vault_id).is_none() {
        return Err(NotesError::VaultUnknown(vault_id.to_owned()));
    }
    let data_dir = platform
        .data_dir()
        .map_err(|error| NotesError::Name(error.to_string()))?;
    keeper_core::registry::set_active_vault(&data_dir, vault_id)
        .map_err(|error| NotesError::Name(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault rooted at a real, canonical scratch directory. `tempfile` is not
    /// a dependency of this crate; `std::env::temp_dir()` plus the pid is the
    /// convention here (see `sync.rs`).
    fn test_vault(tag: &str) -> Vault {
        let root = std::env::temp_dir().join(format!(
            "keeper-notes-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).expect("vault root");
        let root = root.canonicalize().expect("canonical vault root");
        Vault {
            id: "01VAULT".to_owned(),
            name: "mind".to_owned(),
            root: root.clone(),
            local_path: root,
            config: NotesConfig::default(),
            excludes: Arc::new(ExcludeSet::new(&[]).expect("built-in excludes")),
        }
    }

    fn stat(size: u64, mtime_ns: i128, ino: u64) -> FileStat {
        FileStat {
            size,
            mtime_ns,
            ino,
        }
    }

    fn entry(path: &str, stat: FileStat) -> IndexEntry {
        IndexEntry {
            id: format!("path:{path}"),
            path: path.to_owned(),
            title: path.to_owned(),
            size: stat.size,
            mtime_ns: stat.mtime_ns,
            ino: stat.ino,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: BTreeMap::new(),
            links: Vec::new(),
            flags: Vec::new(),
            snippet: String::new(),
        }
    }

    /// A directory tree in memory that records every directory the walk listed.
    ///
    /// The record is the point: `.obsidian/` being skipped *before descent* is a
    /// syscall that never happens, and a real filesystem cannot be asked about a
    /// call nobody made.
    struct FakeWalk {
        tree: HashMap<String, Vec<WalkEntry>>,
        listed: Vec<String>,
    }

    impl FakeWalk {
        fn new(dirs: &[(&str, &[(&str, bool)])]) -> Self {
            let tree = dirs
                .iter()
                .map(|(dir, entries)| {
                    let entries = entries
                        .iter()
                        .map(|(name, is_dir)| WalkEntry {
                            name: (*name).to_owned(),
                            is_dir: *is_dir,
                            stat: stat(1, 1, 1),
                        })
                        .collect();
                    ((*dir).to_owned(), entries)
                })
                .collect();
            Self {
                tree,
                listed: Vec::new(),
            }
        }
    }

    impl VaultWalk for FakeWalk {
        fn list(&mut self, rel: &str) -> std::io::Result<Vec<WalkEntry>> {
            self.listed.push(rel.to_owned());
            Ok(self.tree.get(rel).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn obsidian_is_never_listed_and_its_notes_never_reach_the_index() {
        let mut fs = FakeWalk::new(&[
            (
                "",
                &[
                    ("note.md", false),
                    (OBSIDIAN_DIR, true),
                    (KEEPER_DIR, true),
                    ("journal", true),
                ],
            ),
            ("journal", &[("2026-08-02.md", false)]),
            // Populated precisely so a walk that descended would find these and
            // the assertions below would fail loudly.
            (
                OBSIDIAN_DIR,
                &[("workspace.json", false), ("plugins.md", false)],
            ),
            (KEEPER_DIR, &[("index.json", false)]),
        ]);
        let excludes = ExcludeSet::new(&[]).expect("built-in excludes");
        let found = walk(&mut fs, &excludes);

        assert!(
            !fs.listed.iter().any(|dir| dir == OBSIDIAN_DIR),
            "the walk listed .obsidian/, so it was opened before it was refused: {:?}",
            fs.listed
        );
        assert!(
            !fs.listed.iter().any(|dir| dir == KEEPER_DIR),
            "the walk listed .keeper/, which is keeper's cache and not vault content"
        );
        let paths: Vec<&str> = found.iter().map(|seen| seen.rel.as_str()).collect();
        assert!(paths.contains(&"note.md"), "a root note is indexed");
        assert!(
            paths.contains(&"journal/2026-08-02.md"),
            "an ordinary subdirectory is still descended into"
        );
        assert!(
            !paths.iter().any(|path| path.contains(OBSIDIAN_DIR)),
            "a note inside .obsidian/ reached the index: {paths:?}"
        );
    }

    #[test]
    fn a_matching_stat_adopts_the_cached_parse_and_a_mismatch_re_parses() {
        let cached = vec![
            entry("kept.md", stat(10, 100, 7)),
            entry("changed.md", stat(10, 100, 7)),
            entry("gone.md", stat(10, 100, 7)),
        ];
        let seen = vec![
            Seen {
                rel: "kept.md".to_owned(),
                stat: stat(10, 100, 7),
            },
            // One byte longer: the same note, re-parsed.
            Seen {
                rel: "changed.md".to_owned(),
                stat: stat(11, 100, 7),
            },
            Seen {
                rel: "new.md".to_owned(),
                stat: stat(4, 200, 9),
            },
        ];
        let plan = plan_scan(cached, seen);

        let adopted: Vec<&str> = plan.adopt.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            adopted,
            ["kept.md"],
            "only the vouched-for entry is adopted"
        );
        let mut parsed: Vec<&str> = plan.parse.iter().map(|s| s.rel.as_str()).collect();
        parsed.sort_unstable();
        assert_eq!(
            parsed,
            ["changed.md", "new.md"],
            "a changed note and an unknown one are both read"
        );
        assert!(
            !plan.adopt.iter().any(|e| e.path == "gone.md"),
            "an entry whose file is gone is dropped, not carried forward"
        );
    }

    #[test]
    fn a_mismatched_inode_re_parses_even_when_size_and_mtime_agree() {
        // A rename-into-place from another editor can keep size and mtime and
        // change the inode. Without the inode the cache would adopt a parse of
        // bytes that are no longer there.
        let plan = plan_scan(
            vec![entry("note.md", stat(10, 100, 7))],
            vec![Seen {
                rel: "note.md".to_owned(),
                stat: stat(10, 100, 8),
            }],
        );
        assert!(plan.adopt.is_empty());
        assert_eq!(plan.parse.len(), 1);
    }

    #[test]
    fn a_corrupt_or_foreign_cache_takes_the_discard_and_rescan_branch() {
        let good = serde_json::to_vec(&IndexCache {
            schema: INDEX_SCHEMA,
            vault_id: "01VAULT".to_owned(),
            built_ms: 1,
            entries: vec![entry("note.md", stat(1, 1, 1))],
        })
        .expect("a cache serializes");
        assert!(
            adopt_cache(&good, "01VAULT").is_some(),
            "a well-formed cache for this vault is adopted"
        );

        // Truncated to half its bytes: rebuilt silently.
        assert!(adopt_cache(&good[..good.len() / 2], "01VAULT").is_none());
        // Not JSON at all.
        assert!(adopt_cache(b"", "01VAULT").is_none());
        assert!(adopt_cache(b"not json", "01VAULT").is_none());
        // The right shape, a schema this build does not know.
        let stale = serde_json::to_vec(&IndexCache {
            schema: INDEX_SCHEMA + 1,
            vault_id: "01VAULT".to_owned(),
            built_ms: 1,
            entries: Vec::new(),
        })
        .expect("a cache serializes");
        assert!(adopt_cache(&stale, "01VAULT").is_none());
        // A cache copied from another machine names another vault.
        assert!(
            adopt_cache(&good, "01OTHERVAULT").is_none(),
            "a cache for a different vault is rejected rather than trusted"
        );
        // The right type with a missing field.
        assert!(adopt_cache(br#"{"schema":1}"#, "01VAULT").is_none());
    }

    #[test]
    fn a_conflict_copy_is_recognised_and_an_ordinary_hyphenated_note_is_not() {
        assert_eq!(
            conflict_origin("Vault as a lens.sync-conflict-20260802-120000-laptop.md").as_deref(),
            Some("Vault as a lens.md")
        );
        assert_eq!(
            conflict_origin("journal/2026-08-02.sync-conflict-20260802-120000-mini.md").as_deref(),
            Some("journal/2026-08-02.md"),
            "the canonical note keeps its directory"
        );
        // The shape the engine actually writes, built by the engine's own
        // function so this cannot drift from AD-43.
        let name = keeper_sync::git::conflict::conflict_name(
            Path::new("note.md"),
            "20260802-120000",
            "laptop",
        );
        assert_eq!(conflict_origin(&name).as_deref(), Some("note.md"));

        // Ordinary notes, including ones with hyphens, dates and the words
        // themselves in the name.
        assert!(conflict_origin("2026-08-02-sync-conflict-notes.md").is_none());
        assert!(conflict_origin("journal/2026-08-02.md").is_none());
        assert!(conflict_origin("my-sync-conflict-policy.md").is_none());
        assert!(conflict_origin("note.md").is_none());
        assert!(conflict_origin(".hidden.md").is_none());
    }

    #[test]
    fn the_coalescer_emits_once_for_a_burst_on_one_path() {
        let mut coalescer = Coalescer::default();
        let start = Instant::now();
        for step in 0..5 {
            coalescer.push(
                "note.md".to_owned(),
                start + Duration::from_millis(step * 20),
            );
        }
        let last_push = start + Duration::from_millis(80);
        assert!(
            !coalescer.is_due(last_push),
            "every push slid the window, so it is not due at the last push"
        );
        assert!(coalescer.is_due(last_push + COALESCE_WINDOW));
        assert_eq!(
            coalescer.take(),
            vec!["note.md".to_owned()],
            "five writes to one note cost exactly one re-read"
        );
        assert!(
            !coalescer.is_due(start + Duration::from_secs(10)),
            "a taken batch closes the window"
        );
    }

    #[test]
    fn the_coalescer_keeps_every_distinct_path_in_one_batch() {
        let mut coalescer = Coalescer::default();
        let start = Instant::now();
        coalescer.push("a.md".to_owned(), start);
        coalescer.push("b.md".to_owned(), start);
        coalescer.push("a.md".to_owned(), start);
        assert_eq!(coalescer.take(), vec!["a.md".to_owned(), "b.md".to_owned()]);
    }

    #[test]
    fn containment_refuses_traversal_an_absolute_path_and_an_escaping_symlink() {
        let vault = test_vault("contain");

        // `..` in any position.
        assert!(contained(&vault, "../outside.md").is_err());
        assert!(contained(&vault, "notes/../../outside.md").is_err());
        // An absolute path.
        assert!(contained(&vault, "/etc/passwd").is_err());
        // keeper's and Obsidian's own directories.
        assert!(contained(&vault, ".obsidian/workspace.json").is_err());
        assert!(contained(&vault, ".keeper/index.json").is_err());
        // A NUL, which some filesystems truncate at.
        assert!(contained(&vault, "note\0.md").is_err());
        assert!(contained(&vault, "").is_err());
        // An ordinary note resolves.
        assert_eq!(
            contained(&vault, "journal/2026-08-02.md").expect("a vault path resolves"),
            vault.root.join("journal/2026-08-02.md")
        );

        // A symlink inside the vault pointing out of it passes the lexical check
        // — every component is a plain name — and is refused by the
        // canonicalizing check the protocol handler applies.
        #[cfg(unix)]
        if let Some(parent) = vault.root.parent() {
            let outside = parent.join(format!("outside-{}.md", std::process::id()));
            std::fs::write(&outside, b"not yours").expect("write the file outside");
            let link = vault.root.join("escape.md");
            std::os::unix::fs::symlink(&outside, &link).expect("symlink");
            let lexical = contained(&vault, "escape.md").expect("lexically fine");
            let canonical = lexical.canonicalize().expect("the link resolves");
            assert!(
                !canonical.starts_with(&vault.root),
                "a symlink out of the vault must not canonicalize inside it"
            );
            assert!(
                crate::note_protocol::contained_read(&vault, "escape.md").is_none(),
                "the protocol handler's containment check must refuse an escaping symlink"
            );
            let _ = std::fs::remove_file(&outside);
        }
        let _ = std::fs::remove_dir_all(&vault.root);
    }

    #[test]
    fn the_cadence_debounces_from_the_last_change_and_never_runs_two_at_once() {
        let cadence = NotesCadence {
            commit_idle_ms: 2_000,
            push_interval_ms: 30_000,
            push_on_blur: true,
        };
        let dirty = CadenceState {
            phase: Phase::Dirty,
            last_change_ms: 10_000,
            push_deadline_ms: 0,
            in_flight: false,
        };
        // Still typing.
        assert_eq!(decide(&dirty, &cadence, 11_500, false), Action::None);
        // Quiet for the whole debounce.
        assert_eq!(decide(&dirty, &cadence, 12_000, false), Action::Commit);
        // A forced flush does not wait for the debounce — hiding the window IS
        // the signal.
        assert_eq!(decide(&dirty, &cadence, 10_100, true), Action::Commit);
        // One action in flight at a time.
        assert_eq!(
            decide(
                &CadenceState {
                    in_flight: true,
                    ..dirty
                },
                &cadence,
                99_999,
                true
            ),
            Action::None
        );

        let ahead = CadenceState {
            phase: Phase::Ahead,
            last_change_ms: 10_000,
            push_deadline_ms: 40_000,
            in_flight: false,
        };
        assert_eq!(decide(&ahead, &cadence, 39_000, false), Action::None);
        assert_eq!(decide(&ahead, &cadence, 40_000, false), Action::Push);
        assert_eq!(
            decide(&ahead, &cadence, 11_000, true),
            Action::Push,
            "a forced flush brings the push deadline forward"
        );
        assert_eq!(
            decide(&CadenceState::default(), &cadence, 99_999, true),
            Action::None,
            "an idle vault has nothing to flush"
        );
    }

    #[test]
    fn a_change_during_a_commit_survives_it() {
        // The coalesce rule from AD-62's state machine: changes arriving while
        // committing accumulate into the next Dirty window rather than being
        // finished away into Idle.
        let cadence = Cadence::default();
        cadence.lock().in_flight = true;
        cadence.mark_dirty();
        cadence.finish(false, 30_000);
        assert_eq!(cadence.lock().phase, Phase::Dirty);
        assert!(!cadence.lock().in_flight);
    }

    #[test]
    fn an_attachment_name_collides_into_a_counter() {
        assert_eq!(unique_name("shot.png", &[]), "shot.png");
        assert_eq!(
            unique_name("shot.png", &["shot.png".to_owned()]),
            "shot-2.png"
        );
        assert_eq!(
            unique_name(
                "shot.png",
                &["shot.png".to_owned(), "shot-2.png".to_owned()]
            ),
            "shot-3.png"
        );
        // The vault syncs to case-insensitive filesystems, so a differently
        // cased sibling is still a collision.
        assert_eq!(
            unique_name("Shot.png", &["shot.png".to_owned()]),
            "Shot-2.png"
        );
        // A dotfile has no extension to preserve.
        assert_eq!(
            unique_name(".gitkeep", &[".gitkeep".to_owned()]),
            ".gitkeep-2"
        );
    }

    #[test]
    fn a_unified_diff_parses_into_hunks() {
        let diff = concat!(
            "diff --git a/notes/a.md b/notes/a.md\n",
            "index 1111111..2222222 100644\n",
            "--- a/notes/a.md\n",
            "+++ b/notes/a.md\n",
            "@@ -1,3 +1,4 @@\n",
            " first\n",
            "-second\n",
            "+second edited\n",
            "+third\n",
            "@@ -10 +11,2 @@ context\n",
            "+appended\n",
        );
        let hunks = parse_hunks(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(
            (
                hunks[0].old_start,
                hunks[0].old_lines,
                hunks[0].new_start,
                hunks[0].new_lines
            ),
            (1, 3, 1, 4)
        );
        assert!(hunks[0].text.contains("+second edited"));
        assert!(
            !hunks[0].text.contains("diff --git"),
            "the file header belongs to no hunk"
        );
        // A bare `-10` is a one-line span.
        assert_eq!((hunks[1].old_start, hunks[1].old_lines), (10, 1));
        assert_eq!((hunks[1].new_start, hunks[1].new_lines), (11, 2));
    }

    #[test]
    fn only_a_real_ulid_gives_a_note_a_stable_identity() {
        assert!(is_ulid("01J8ZQ4M7T5R9V3XK2B6C0DFGH"));
        assert!(!is_ulid("not-a-ulid"));
        assert!(!is_ulid(""));
        // `U`, `I`, `L` and `O` are not in Crockford base32.
        assert!(!is_ulid("01J8ZQ4M7T5R9V3XK2B6C0DFGU"));
    }

    #[test]
    fn a_head_revision_from_another_device_or_an_agent_is_foreign() {
        let mut head = HeadRevision {
            rev: "abc".to_owned(),
            when_ms: 0,
            device: "mini".to_owned(),
            origin: "local".to_owned(),
            source: "watch".to_owned(),
            subject: "notes(mind): 1".to_owned(),
        };
        assert!(!head.is_foreign());
        for origin in ["agent", "remote", "unknown"] {
            head.origin = origin.to_owned();
            assert!(head.is_foreign(), "{origin} must count as foreign");
        }
    }

    #[test]
    fn an_untrailered_commit_is_unknown_rather_than_local() {
        assert_eq!(origin_of(None, Some("mini")), "unknown");
        let mine = Provenance::new("01P", "mini", "01D", "mini.local", SyncSource::Watch);
        assert_eq!(origin_of(Some(&mine), Some("mini")), "local");
        assert_eq!(origin_of(Some(&mine), Some("laptop")), "remote");
        let bot = Provenance::new("01P", "mini", "01D", "mini.local", SyncSource::Bot);
        assert_eq!(
            origin_of(Some(&bot), Some("mini")),
            "agent",
            "an agent's commit is an agent's commit even on this machine"
        );
    }

    #[test]
    fn a_content_rev_tracks_the_bytes_and_nothing_else() {
        assert_eq!(content_rev("hello"), content_rev("hello"));
        assert_ne!(content_rev("hello"), content_rev("hello "));
        assert_ne!(content_rev(""), content_rev("a"));
    }

    #[test]
    fn an_asset_url_percent_encodes_every_segment_and_keeps_the_separators() {
        assert_eq!(
            asset_url("01VAULT", "attachments/a b.png"),
            "keeper-note://01VAULT/attachments/a%20b.png"
        );
        assert_eq!(
            asset_url("01VAULT", "attachments/../etc/passwd"),
            "keeper-note://01VAULT/attachments/%2E%2E/etc/passwd",
            "a traversal attempt survives as text and is refused on resolution"
        );
    }
}
