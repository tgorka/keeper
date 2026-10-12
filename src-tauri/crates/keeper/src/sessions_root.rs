//! The sessions-root registry and indexer (Phase 7, AD-108, AD-110).
//!
//! The shell half of the sessions domain: it owns every effect — the registry
//! of sessions-flagged profiles, the zone scan, the watcher-tap fan-out — and
//! hands `keeper_core::sessions` plain values. It is to sessions what
//! `notes_vault` is to notes, deliberately smaller: a zone holds tens of
//! session folders, not ten thousand notes, so the index here is "rescan the
//! zone" with a coalescing window rather than an incremental delta pipeline.
//! NFR-36's bar is a 2 s cold scan at 200 sessions; a full rescan is well
//! under it, and a simpler pipeline is one that cannot desync.
//!
//! **Files are the only truth** (AD-110): everything published here is
//! recomputed from disk on every scan. The only cache is advisory and lives in
//! the zone's `.keeper/`; deleting it costs one rescan.
//!
//! **Workspace is a read-only projection** (AD-113): the walk records
//! `workspace/**` mtimes for the freshness signal — depth- and entry-budgeted
//! — and nothing else about it: no text, no index rows, no change events.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use keeper_agent::sessions::scan::{
    read_ref_sources, read_session_pool, read_zone_spaces, scan_zone, RefSource, SessionPool,
    ZoneSpaces, REF_SCAN_BUDGET, WORKSPACE_WALK_BUDGET,
};
use keeper_core::sessions::model::WORKSPACE_DIR;
use keeper_core::sessions::vm::{SessionRootVm, SessionRowVm};
use keeper_sync::SyncProfile;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

/// The event the frontend listens on: "this root's session set changed, re-read
/// it". Payload is the root id and nothing else — the listener re-reads through
/// the command rather than trusting a payload, the `CAPTURE_WINDOWS_EVENT`
/// pattern, which at zone scale costs one list read and cannot drift.
pub const SESSIONS_CHANGED_EVENT: &str = "keeper://sessions-changed";

/// How long after the last watcher event a rescan runs. One agent write burst —
/// an editor saving three times, a tool writing file by file — costs one scan.
const COALESCE_WINDOW: Duration = Duration::from_millis(400);

/// One registered sessions root.
#[derive(Debug, Clone)]
struct Root {
    id: String,
    name: String,
    subfolder: String,
    root: PathBuf,
}

/// A root's slot: its identity plus the published snapshot.
struct Slot {
    root: Root,
    /// The last completed scan's rows, or `None` before the first.
    rows: Option<Arc<Vec<SessionRowVm>>>,
    /// Sender into the scan task: any message means "rescan soon".
    work: mpsc::UnboundedSender<()>,
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Slot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(desktop)]
static TAP_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

fn registry() -> MutexGuard<'static, HashMap<String, Slot>> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(desktop)]
fn tap_flag() -> MutexGuard<'static, bool> {
    TAP_RUNNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start the sessions subsystem: build the registry from the profile set, then
/// subscribe to the sync engine's watcher tap. Called from `setup()` after the
/// sync supervisor, beside `notes_vault::start`, and idempotent for the same
/// reasons.
///
/// On a phone (98.2, AD-413) there is no watcher to tap: a zone is rescanned
/// after each sync pass ([`phone_synced`]) and after a card move.
pub fn start(app: &AppHandle) {
    tracing::info!("sessions: starting the root registry");
    refresh(app);
    #[cfg(desktop)]
    start_tap(app);
}

/// Rebuild the registry from the current profile set. The root list *is* a
/// filter over the profile list (AD-107): flagging adds a root, unflagging
/// removes one and deletes nothing.
pub fn refresh(app: &AppHandle) {
    let Some(engine) = crate::sync::engine_if_open() else {
        tracing::info!("sessions: no sync engine yet, so no roots; the next refresh re-enters");
        return;
    };
    let profiles = match engine.list_profiles() {
        Ok(profiles) => profiles,
        Err(error) => {
            tracing::warn!(%error, "sessions: could not read the profile set; registry unchanged");
            return;
        }
    };
    let wanted: Vec<Root> = profiles.iter().filter_map(register_one).collect();
    tracing::info!(
        profiles = profiles.len(),
        roots = wanted.len(),
        "sessions: refreshing the root registry"
    );
    let keep: HashSet<&str> = wanted.iter().map(|root| root.id.as_str()).collect();

    let mut guard = registry();
    #[cfg(not(desktop))]
    let had_roots = guard.len();
    guard.retain(|id, _| keep.contains(id.as_str()));
    let mut fresh: Vec<(String, PathBuf)> = Vec::new();
    for root in wanted {
        match guard.get_mut(&root.id) {
            // Same zone: adopt the name in place, keep the warm snapshot.
            Some(slot) if slot.root.root == root.root => {
                slot.root.name = root.name;
            }
            // New root, or a zone that moved: a fresh slot and a fresh scan
            // task, and whatever plan a crash left in the zone finished below.
            _ => {
                fresh.push((root.id.clone(), root.root.clone()));
                let slot = spawn_scanner(app, root);
                guard.insert(slot.root.id.clone(), slot);
            }
        }
    }
    #[cfg(not(desktop))]
    let has_roots = guard.len();
    drop(guard);
    // A phone finishes a crashed card move before each sync pass instead
    // ([`finish_interrupted_before_pass`]), so a pass never commits half of
    // one; the sweep here would race the launch's first pass.
    #[cfg(desktop)]
    finish_interrupted(fresh);
    // On a phone `CapabilitiesVm.sessions` is whether a zone is here
    // ([`any`]), so a first zone or a last one gone says so to the webview,
    // which asks again — and only those (`zones_flip_the_surface`).
    #[cfg(not(desktop))]
    if keeper_agent::sessions::zones_flip_the_surface(had_roots, has_roots) {
        let _ = app.emit(crate::ipc::CAPABILITIES_CHANGED_EVENT, ());
    }
}

/// Whether any sessions zone is registered — on a phone, the sessions
/// capability (98.2 #6).
pub fn any() -> bool {
    !registry().is_empty()
}

/// Re-read the registry and rescan every zone after a phone sync pass (98.2,
/// NFR-57), as [`crate::notes_vault::phone_synced`] does for vaults: a clone
/// that just landed is a zone from then on, and a fast-forward that just
/// moved cards reaches the board — there is no watcher tap on a phone.
#[cfg(not(desktop))]
pub fn phone_synced(app: &AppHandle) {
    refresh(app);
    for slot in registry().values() {
        let _ = slot.work.send(());
    }
}

/// Finish whatever plan a crash left in the sessions zone of each of these
/// profiles, before a phone's sync pass runs (98.2, R346).
///
/// A card move writes its neighbours' `order:` first and the card last, under
/// a journal; a phone killed between the two leaves half a move on disk, and
/// the pass after it would commit and push that half. So every phone pass —
/// the launch's first one included — runs the same executor the move itself
/// runs, `resume_all`, over the zones first: the plan is finished whole, or
/// (when its guarded write refuses because the bytes changed) its journal is
/// cleared and nothing further is written. Awaited, not spawned: the point is
/// that the pass starts after it. Nothing else on a phone holds a zone lock
/// but keeper's own move, so the wait is bounded by that move.
#[cfg(not(desktop))]
pub async fn finish_interrupted_before_pass(profiles: &[SyncProfile]) {
    let zones: Vec<PathBuf> = profiles
        .iter()
        .filter_map(register_one)
        .map(|root| root.root)
        .collect();
    if zones.is_empty() {
        return;
    }
    let swept = tauri::async_runtime::spawn_blocking(move || {
        let unfinished = keeper_agent::sessions::resume_all(zones.iter().map(PathBuf::as_path));
        for (zone, error) in unfinished {
            tracing::warn!(
                zone = %zone.display(),
                %error,
                "sessions: an interrupted card move could not be finished"
            );
        }
    })
    .await;
    if let Err(error) = swept {
        tracing::warn!(%error, "sessions: the sweep before the phone's pass did not finish");
    }
}

/// Finish whatever plan a crash left in each of these zones (AD-368), off the
/// calling thread and outside the registry: the zone lock waits without bound
/// for whoever holds it — `keeper-agentd` mid-plan, or a process stopped while
/// holding it — and `refresh` runs from `setup()`. Best-effort: the next verb
/// on a zone finishes its plan under the lock before reading anything
/// (`keeper_agent::sessions::exec::hold`), so nothing waits on this having
/// run. Each zone is rescanned afterwards, so the board shows a finished plan.
#[cfg(desktop)]
fn finish_interrupted(zones: Vec<(String, PathBuf)>) {
    use keeper_agent::sessions::exec::ExecError;

    if zones.is_empty() {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        for (id, zone) in zones {
            for (zone, error) in keeper_agent::sessions::resume_all([zone.as_path()]) {
                match &error {
                    ExecError::Failed { verb, .. } if verb == "lock" => {
                        tracing::warn!(zone = %zone.display(), %error, "sessions: could not lock the zone to finish an interrupted plan");
                    }
                    _ => {
                        tracing::warn!(zone = %zone.display(), %error, "sessions: an interrupted plan could not be finished");
                    }
                }
            }
            rescan(&id);
        }
    });
}

/// A `Root` for a sessions-flagged profile whose zone exists on disk right
/// now. Adopt-only (FR-222): a missing zone leaves the root unregistered — and
/// logged — rather than scaffolded.
fn register_one(profile: &SyncProfile) -> Option<Root> {
    profile.sessions.as_ref()?;
    let root = profile.sessions_root()?;
    let canonical = match root.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) => {
            tracing::info!(
                profile = %profile.id,
                path = %root.display(),
                %error,
                "sessions: zone folder is not there right now; leaving it unregistered"
            );
            return None;
        }
    };
    Some(Root {
        id: profile.id.clone(),
        name: profile.name.clone(),
        subfolder: profile
            .sessions
            .as_ref()
            .map(|s| s.subfolder.trim().to_owned())
            .unwrap_or_default(),
        root: canonical,
    })
}

/// Spawn the scan task for one root: an immediate cold scan, then one rescan
/// per coalesced burst of work messages.
fn spawn_scanner(app: &AppHandle, root: Root) -> Slot {
    let (work, mut inbox) = mpsc::unbounded_channel::<()>();
    let id = root.id.clone();
    let zone = root.root.clone();
    let app = app.clone();
    // Prime the channel so the task's first iteration scans without waiting.
    let _ = work.send(());
    tauri::async_runtime::spawn(async move {
        while inbox.recv().await.is_some() {
            // Coalesce the burst: keep draining until the window stays quiet.
            loop {
                match tokio::time::timeout(COALESCE_WINDOW, inbox.recv()).await {
                    Ok(Some(())) => continue,
                    Ok(None) => return,
                    Err(_elapsed) => break,
                }
            }
            let rows = tokio::task::block_in_place(|| scan_zone(&zone));
            let rows = Arc::new(rows);
            if let Some(slot) = registry().get_mut(&id) {
                slot.rows = Some(Arc::clone(&rows));
            }
            let _ = app.emit(SESSIONS_CHANGED_EVENT, id.clone());
        }
    });
    Slot {
        root,
        rows: None,
        work,
    }
}

/// Subscribe to the engine's watcher tap and mark the owning root dirty for
/// any change under its zone. Workspace changes count — they move the
/// freshness signal — but they enter the same coalesced rescan as everything
/// else; nothing about workspace content is read beyond `lstat` (AD-113).
#[cfg(desktop)]
fn start_tap(app: &AppHandle) {
    let mut running = tap_flag();
    if *running {
        return;
    }
    let Some(engine) = crate::sync::engine_if_open() else {
        return;
    };
    *running = true;
    let mut tap = engine.watch_tap();
    drop(running);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match tap.recv().await {
                Ok((profile_id, path)) => {
                    let guard = registry();
                    if let Some(slot) = guard.get(&profile_id) {
                        if path.starts_with(&slot.root.root) {
                            let _ = slot.work.send(());
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::info!(
                        missed,
                        "sessions: watcher tap lagged; rescanning every root"
                    );
                    for slot in registry().values() {
                        let _ = slot.work.send(());
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    *tap_flag() = false;
                    let _ = app; // keep the handle alive to here
                    return;
                }
            }
        }
    });
}

/// Every registered root, projected for the board's switcher (FR-224).
pub fn roots() -> Vec<SessionRootVm> {
    let guard = registry();
    let mut out: Vec<SessionRootVm> = guard
        .values()
        .map(|slot| {
            let rows = slot.rows.as_deref();
            SessionRootVm {
                id: slot.root.id.clone(),
                name: slot.root.name.clone(),
                subfolder: slot.root.subfolder.clone(),
                root: slot.root.root.to_string_lossy().into_owned(),
                indexed: rows.is_some(),
                active_count: rows
                    .map(|rows| rows.iter().filter(|r| r.status == "active").count() as u32)
                    .unwrap_or(0),
                unread_count: rows
                    .map(|rows| rows.iter().filter(|r| r.unread).count() as u32)
                    .unwrap_or(0),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The last completed scan's rows for one root, or `None` before it.
pub fn rows(root_id: &str) -> Option<Arc<Vec<SessionRowVm>>> {
    registry().get(root_id).and_then(|slot| slot.rows.clone())
}

/// Whether a root id is registered at all — what tells "cold" from "unknown".
pub fn known(root_id: &str) -> bool {
    registry().contains_key(root_id)
}

/// One root's zone path, for the lifecycle executor.
pub fn zone_of(root_id: &str) -> Option<PathBuf> {
    registry().get(root_id).map(|slot| slot.root.root.clone())
}

/// One root's zone **subfolder** — the profile-relative prefix every session
/// path is composed against (AD-65).
///
/// The registry's own copy, taken from the same `SessionsConfig::subfolder` the
/// commands read off the profile, so the two cannot disagree. It exists for the
/// callers that have a root id and no `AppState` to reach a profile through — a
/// spawned scan, for instance, which outlives the command that started it.
pub fn subfolder_of(root_id: &str) -> Option<String> {
    registry()
        .get(root_id)
        .map(|slot| slot.root.subfolder.clone())
}

/// One row by session id, from the last scan.
pub fn row_of(root_id: &str, session_id: &str) -> Option<SessionRowVm> {
    registry()
        .get(root_id)?
        .rows
        .as_ref()?
        .iter()
        .find(|row| row.id == session_id)
        .cloned()
}

/// Ask one root to rescan now (FR-225's rebuild verb).
pub fn rescan(root_id: &str) -> bool {
    registry()
        .get(root_id)
        .map(|slot| slot.work.send(()).is_ok())
        .unwrap_or(false)
}

/// One session's detail and its board ([`keeper_agent::sessions::board::detail`],
/// the one projection every target shows), for a registered root's row.
pub fn detail(
    root_id: &str,
    session_id: &str,
) -> Option<keeper_core::sessions::vm::SessionDetailVm> {
    let zone = zone_of(root_id)?;
    let row = row_of(root_id, session_id)?;
    Some(keeper_agent::sessions::board::detail(&zone, row))
}

/// One raw entry of a session's own tree, before anything is said about sync.
///
/// The walk deliberately knows nothing about profiles, excludes or the write
/// fence: it reads dirents. `sessions_ipc` is what turns these into
/// [`keeper_core::sessions::vm::SessionEntryVm`], because that is where the
/// engine and the scope already are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub name: String,
    /// Session-relative, `/`-joined.
    pub rel_path: String,
    /// Session-relative parent, `""` at the top level.
    pub parent: String,
    /// 1 at the top level.
    pub depth: u32,
    pub is_dir: bool,
    pub size: u64,
    pub mtime_ms: i64,
}

/// Walk one session folder, in the order the tree renders (FR-254, AD-117).
///
/// **The zone's own order, not the alphabet.** `artifacts/`, `refs/`,
/// `prompts/` and `workspace/` come first, in the zone's own sequence, because
/// that sequence is what the zone contract teaches and re-sorting it here
/// would make keeper's tree disagree with the operator's own documentation.
/// Anything else follows, folders before files, name-insensitively — the
/// Files-pane rule for entries keeper has no opinion about.
///
/// **Within a section, newest first.** A session's sections are review
/// surfaces; the file you want is the one that just changed. The four ordering
/// rules are all "what is this list for", which is why they differ.
///
/// The budget is [`WORKSPACE_WALK_BUDGET`], shared with the freshness signal —
/// a session's `workspace/` is the one subtree that can hold a `node_modules`,
/// and the caller is told when the walk stopped rather than being handed a
/// prefix that looks complete.
pub fn tree(root_id: &str, session_id: &str) -> Option<(String, Vec<RawEntry>, bool)> {
    let row = row_of(root_id, session_id)?;
    let zone = zone_of(root_id)?;
    let dir = zone.join(&row.path);
    let mut out = Vec::new();
    let mut budget = WORKSPACE_WALK_BUDGET;
    walk_tree(&dir, "", 1, &mut out, &mut budget);
    Some((row.path, out, budget == 0))
}

/// What [`ref_sources`] found.
///
/// A named struct rather than the triple it started as: three anonymous fields
/// where two are strings is a call site that reads `.0` and `.2` and means
/// nothing to the next person.
pub struct RefSources {
    /// The session's zone-relative folder, e.g. `active/2026-08-10-keeper` —
    /// the prefix a relative reference is resolved against.
    pub path: String,
    /// The markdown to scan, in reading order.
    pub files: Vec<RefSource>,
    /// Whether the byte budget stopped the scan before every file was read.
    pub truncated: bool,
}

/// Every pointer written in one session's markdown, with the file it was
/// written in (FR-255, AD-118).
///
/// **Which files, under the folder contract.** The README, then every other
/// `.md` at the session root, then every `.md` under `refs/` and `prompts/` —
/// the README first because it is the record, the rest of the root next because
/// that is where it sits, and `refs/` after because the zone's own contract says
/// that is where inputs worth keeping are listed. Root markdown is read
/// (FR-286) because the create verb wrote there before Story 50.1 filed by
/// kind, and a `references.md` left behind was in no reader at all: not this
/// one, not a space, not even *Unfiled*.
///
/// **Which files, under the flat contract.** Every `.md` in the session's tree,
/// each directory's own files before its subdirectories, in name order
/// (FR-285). The flat shape's premise is that kind is a tag, so a pointer's
/// file is not distinguishable by location and all of them must be read — and
/// a file the operator moved into a `spaces/` or a `log/` he made is still one
/// of them.
///
/// **What is excluded, in both.** [`UNSCANNED_DIRS`] and dotted directories,
/// through [`scans_markdown`] — the one list, for the reasons stated there.
///
/// **A byte budget, not an entry budget.** The tree's budget counts dirents
/// because that is what a `node_modules` inflates; here the cost is parsing
/// markdown, so the ceiling is total bytes read. A `refs/` somebody filled with
/// a crawl stops the scan and says so.
pub fn ref_sources(root_id: &str, session_id: &str) -> Option<RefSources> {
    let row = row_of(root_id, session_id)?;
    let zone = zone_of(root_id)?;
    // The shape is discarded here and only here: a reference is a reference
    // whichever contract wrote it, and the widget renders the same rows either
    // way. `detail` and `migrate` call the reader directly for the shape.
    let (files, truncated, _shape) = read_ref_sources(&zone.join(&row.path), REF_SCAN_BUDGET);
    Some(RefSources {
        path: row.path,
        files,
        truncated,
    })
}

/// Read a zone's `_spaces/` (FR-261).
///
/// Unbudgeted, unlike every other scan here, and the asymmetry is deliberate: a
/// session's pool is however much prose the operator wrote, but `_spaces/` holds
/// a handful of files keeper's own editor writes, each a frontmatter block and a
/// heading. A budget would be a ceiling nothing can reach that still has to be
/// explained in the failure text.
pub fn zone_spaces(root_id: &str) -> Option<ZoneSpaces> {
    let zone = zone_of(root_id)?;
    Some(read_zone_spaces(&zone))
}

/// Read one session's pool for space evaluation (FR-261).
///
/// Reuses [`read_ref_sources`]'s scan — the walk that also decides the shape —
/// and adds a stat per file. A folder-shaped session returns its `README.md`,
/// its other root markdown and its `refs/`+`prompts/` files, which is what makes
/// the spaces list *work* rather than sit empty on a session nobody has migrated
/// yet: a `tag:ref` query over an unmigrated session finds whatever those files
/// declare, and finds nothing when they declare nothing, which is the honest
/// answer either way.
///
/// The shape is discarded here — a space's selection is its query's, not its
/// session's contract's — but the truncation is not: see
/// [`SessionPool::truncated`].
pub fn session_pool(root_id: &str, session_id: &str) -> Option<SessionPool> {
    let row = row_of(root_id, session_id)?;
    let zone = zone_of(root_id)?;
    Some(read_session_pool(&zone.join(&row.path), row.path))
}

/// Whether a profile-relative path is inside a session folder of this root,
/// and what that session is called — the [`keeper_core::sessions::refs::RefProbe`]
/// question a path answers only against the zone.
///
/// Asked of the scanned rows rather than of the filesystem: the board already
/// knows every session in the zone by folder path, and a second definition of
/// "is this a session" is exactly the drift
/// [`keeper_core::sessions::model::classify`] exists to prevent.
pub fn session_at(root_id: &str, zone_relative: &str) -> Option<String> {
    let rows = rows(root_id)?;
    rows.iter()
        .filter(|row| {
            zone_relative == row.path || zone_relative.starts_with(&format!("{}/", row.path))
        })
        // The deepest match wins, so a path inside a session names that
        // session rather than an ancestor that happens to share its prefix.
        .max_by_key(|row| row.path.len())
        .map(|row| row.title.clone())
}

/// The recursive half. `budget` counts down across the whole walk, so one
/// enormous section cannot starve the ones after it silently — it exhausts the
/// budget, and `truncated` says so.
fn walk_tree(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<RawEntry>, budget: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut listed: Vec<RawEntry> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dotfiles are furniture here exactly as they are in the Files
            // pane: `.gitkeep` is the zone's own placeholder and `.keeper/` is
            // keeper's, and neither is a file anybody opened this tree to see.
            if name.starts_with('.') {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_millis() as i64)
                .unwrap_or(0);
            let rel_path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            Some(RawEntry {
                name,
                rel_path,
                parent: prefix.to_owned(),
                depth,
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                mtime_ms,
            })
        })
        .collect();

    if prefix.is_empty() {
        // The session root: the zone's four standard directories in the zone's
        // own order, then everything else folders-first-by-name.
        listed.sort_by(|a, b| {
            let rank = |entry: &RawEntry| {
                SECTION_ORDER
                    .iter()
                    .position(|section| *section == entry.name)
                    .unwrap_or(SECTION_ORDER.len())
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| b.is_dir.cmp(&a.is_dir))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    } else {
        // Inside a section: newest first, the review order.
        listed.sort_by(|a, b| {
            b.mtime_ms
                .cmp(&a.mtime_ms)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    for entry in listed {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let is_dir = entry.is_dir;
        // `dir` is already this level; the child is one `name` below it, not
        // one `rel_path` — `rel_path` is measured from the session root.
        let child = dir.join(&entry.name);
        let rel_path = entry.rel_path.clone();
        out.push(entry);
        if is_dir {
            walk_tree(&child, &rel_path, depth + 1, out, budget);
        }
    }
}

/// The zone's own section order at a session's root (`60-sessions` contract).
///
/// `README.md` is deliberately absent: it sorts with everything else, after
/// the four sections. It is not hidden — a session's record has a sync story
/// like any other file — but it does not get promoted above the sections,
/// because the header already opens it with its own verb.
const SECTION_ORDER: [&str; 4] = ["artifacts", "refs", "prompts", WORKSPACE_DIR];

#[cfg(test)]
mod tests {
    use super::*;

    /// One session folder, walked (FR-254).
    fn walk(dir: &Path, budget: usize) -> (Vec<RawEntry>, bool) {
        let mut out = Vec::new();
        let mut left = budget;
        walk_tree(dir, "", 1, &mut out, &mut left);
        (out, left == 0)
    }

    /// The tree renders in the zone's own order — the four contract sections
    /// first, in the contract's sequence, then everything else — and each
    /// section's subtree follows it rather than being appended at the end.
    #[test]
    fn the_walk_orders_by_the_zone_contract_and_nests_each_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in ["workspace", "prompts", "refs", "artifacts", "scratch"] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        std::fs::write(session.join("README.md"), "# s\n").expect("write");
        std::fs::write(session.join("artifacts/report.md"), "r").expect("write");
        std::fs::write(session.join("workspace/iter.md"), "i").expect("write");

        let (entries, truncated) = walk(session, WORKSPACE_WALK_BUDGET);
        assert!(!truncated, "eight entries do not exhaust the budget");

        let order: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "artifacts",
                "artifacts/report.md",
                "refs",
                "prompts",
                "workspace",
                "workspace/iter.md",
                "scratch",
                "README.md",
            ],
            "contract sections in contract order, each followed by its own \
             subtree; unknown entries after them, folders before files"
        );

        let report = entries
            .iter()
            .find(|e| e.rel_path == "artifacts/report.md")
            .expect("the artifact");
        assert_eq!(
            report.parent, "artifacts",
            "nesting is carried, not implied"
        );
        assert_eq!(report.depth, 2, "aria-level starts at 1 for the sections");
        assert!(!report.is_dir);
        assert_eq!(report.size, 1);

        let artifacts = &entries[0];
        assert_eq!(artifacts.parent, "", "a section's parent is the session");
        assert_eq!(artifacts.depth, 1);
        assert!(artifacts.is_dir);
    }

    /// Inside a section the newest file is first, because a session's sections
    /// are review surfaces and the file you want is the one that just changed.
    #[test]
    fn a_section_lists_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("artifacts")).expect("mkdir");
        // Explicit mtimes: two writes a millisecond apart are not an order.
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let stamp = |name: &str, when: std::time::SystemTime| {
            let path = session.join("artifacts").join(name);
            std::fs::write(&path, "x").expect("write");
            std::fs::File::options()
                .write(true)
                .open(&path)
                .expect("open")
                .set_modified(when)
                .expect("mtime");
        };
        stamp("old.md", old);
        stamp("new.md", old + std::time::Duration::from_secs(86_400));

        let (entries, _) = walk(session, WORKSPACE_WALK_BUDGET);
        let inside: Vec<&str> = entries
            .iter()
            .filter(|e| e.parent == "artifacts")
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(inside, vec!["new.md", "old.md"]);
    }

    /// Dotfiles are furniture, and a walk that runs out of budget says so
    /// rather than handing back a prefix that looks complete.
    #[test]
    fn dotfiles_are_skipped_and_an_exhausted_budget_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        std::fs::write(session.join(".keeper-marker"), "x").expect("write");
        for index in 0..8 {
            std::fs::write(session.join("workspace").join(format!("f{index}.md")), "x")
                .expect("write");
        }

        let (all, _) = walk(session, WORKSPACE_WALK_BUDGET);
        assert!(
            all.iter().all(|e| !e.name.starts_with('.')),
            "the tree is not where dotfiles are read"
        );

        let (clipped, truncated) = walk(session, 4);
        assert_eq!(clipped.len(), 4);
        assert!(truncated, "the caller is told the walk stopped");
    }
}
