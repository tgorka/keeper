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

use keeper_core::notes::frontmatter::Frontmatter;
use keeper_core::notes::naming::title_from_body;
use keeper_core::sessions::model::{
    classify, freshness, lineage, Freshness, SessionStatus, ACTIVE_DIR, ARCHIVE_DIR, ARTIFACTS_DIR,
    README, WORKSPACE_DIR,
};
use keeper_core::sessions::pool::{log_candidates, read_one, PoolFile};
use keeper_core::sessions::shape::{
    shape as shape_of, KindTag, Shape, ABOUT, PROMPTS_DIR, REFS_DIR,
};
use keeper_core::sessions::spaces::{self, SessionSpace, SPACES_DIR};
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

/// The most workspace entries the freshness walk visits per session before it
/// stops and reports what it has (NFR-37). A workspace that blows the budget
/// shows freshness "at least T", and the UI never promises more precision than
/// was paid for (AD-110).
const WORKSPACE_WALK_BUDGET: usize = 2_000;

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

static TAP_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

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

/// Start the sessions subsystem: build the registry from the profile set, then
/// subscribe to the sync engine's watcher tap. Called from `setup()` after the
/// sync supervisor, beside `notes_vault::start`, and idempotent for the same
/// reasons.
pub fn start(app: &AppHandle) {
    tracing::info!("sessions: starting the root registry");
    refresh(app);
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
    guard.retain(|id, _| keep.contains(id.as_str()));
    for root in wanted {
        match guard.get_mut(&root.id) {
            // Same zone: adopt the name in place, keep the warm snapshot.
            Some(slot) if slot.root.root == root.root => {
                slot.root.name = root.name;
            }
            // New root, or a zone that moved: fresh slot, fresh scan task.
            _ => {
                let slot = spawn_scanner(app, root);
                guard.insert(slot.root.id.clone(), slot);
            }
        }
    }
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

/// One full pass over a zone: every `active/*` and `archive/YYYY/*` directory
/// becomes a row. Pure-ish — all decisions delegate to `keeper_core::sessions`;
/// this function only walks and reads.
fn scan_zone(zone: &Path) -> Vec<SessionRowVm> {
    let mut rows = Vec::new();
    for rel in session_dirs(zone) {
        let Some(status) = classify(&rel) else {
            continue;
        };
        let dir = zone.join(&rel);
        if let Some(row) = row_for(&dir, &rel, status) {
            rows.push(row);
        }
    }
    // Pinned first within status, then record-freshness desc — the board's
    // default order (FR-232, UX-DR85). Active before archived.
    rows.sort_by(|a, b| {
        let group = |r: &SessionRowVm| (r.status != "active", !r.pinned);
        group(a)
            .cmp(&group(b))
            .then(b.record_ms.unwrap_or(0).cmp(&a.record_ms.unwrap_or(0)))
    });
    rows
}

/// Zone-relative session directory candidates: `active/*` and `archive/*/*`.
fn session_dirs(zone: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in dir_names(&zone.join(ACTIVE_DIR)) {
        out.push(format!("{ACTIVE_DIR}/{name}"));
    }
    for year in dir_names(&zone.join(ARCHIVE_DIR)) {
        for name in dir_names(&zone.join(ARCHIVE_DIR).join(&year)) {
            out.push(format!("{ARCHIVE_DIR}/{year}/{name}"));
        }
    }
    out
}

/// Every entry name directly under `path` — files and directories both,
/// best-effort.
///
/// The shape signal and nothing else, which is why it stays a flat listing
/// while the markdown scan walks the tree: the signal is a *file* at the root
/// (`AGENTS.md` or `about.md`), so a directories-only listing would report
/// every flat session as folder-shaped, and a recursive one would let an
/// `about.md` somebody archived three folders down decide the contract.
/// [`dir_names`] answers the first of those wrongly and [`markdown_rels`] the
/// second, so neither can be reused here.
fn entry_names(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// The newest log file's date and first line, for a flat session.
///
/// A flat session has no `## Log` section to fold over, so the signal comes from
/// the pool — but the board draws every session in the zone, and reading every
/// markdown file in each of them to find one line would make opening it cost the
/// whole drive. [`log_candidates`] narrows the field by filename, and this reads
/// down that list until a file's own tags confirm it is a log, giving up after
/// [`LOG_PROBE_BUDGET`] files.
///
/// Ordered by the filename, which is where the clock is, by [`log_candidates`]
/// — the same comparator [`keeper_core::sessions::pool::group`] gives the log
/// view. One comparator with two callers, so the row's window and the detail's
/// Log section cannot name two different newest sittings: a path order would
/// fill this window with the sittings the operator filed into `log/` and none
/// of the ones keeper is still writing at the root.
///
/// Giving up means the row says nothing about its last log, which is what the
/// folder shape already does when its README has no `## Log`. An empty answer
/// here is "not cheaply knowable", never "no logs".
///
/// `names` is [`markdown_rels`]' walk — the same list the pool is read from
/// (FR-285), not the root listing [`entry_names`] returns. A row whose newest
/// log came from a narrower source than the log view's would announce one
/// sitting on the board and open on another, and the cheaper alternative
/// (probe the root only, and let the two disagree once the operator makes a
/// `log/`) buys nothing measurable: the row already walks this session's whole
/// subtree for freshness (see [`walk_freshness`]), including the `workspace/`
/// this walk skips, so the dirents are paid for either way.
fn last_log_flat(dir: &Path, names: &[String]) -> (String, String) {
    for name in log_candidates(names).into_iter().take(LOG_PROBE_BUDGET) {
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
            continue;
        };
        let entry = read_one(PoolFile {
            rel: name,
            text: &text,
        });
        if entry.kind != Some(KindTag::Log) {
            continue;
        }
        let line = entry
            .body(&text)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or_default()
            .to_owned();
        return (entry.date.clone(), line);
    }
    (String::new(), String::new())
}

/// How many stamped candidates a single row will open before giving up. Small
/// on purpose: this runs once per session on every zone scan, and the answer is
/// normally the first file.
const LOG_PROBE_BUDGET: usize = 8;

/// The directory names directly under `path`, `/`-clean, best-effort.
fn dir_names(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry.file_type().ok()?.is_dir().then_some(())?;
            Some(entry.file_name().to_string_lossy().into_owned())
        })
        .collect()
}

/// Project one session directory into its board row. `None` only when the
/// directory vanished mid-scan.
///
/// The row is read from whichever file this session's shape calls the record —
/// `README.md` for a folder session, `about.md` for a flat one. That branch is
/// what makes migration non-destructive: identity, title, tags, pinned state and
/// lineage all live in the record's frontmatter, so a row that kept reading
/// `README.md` after migration would silently unpin every migrated session and
/// drop it to a `path:` id, which the board would render as a *different*
/// session that had lost its history.
fn row_for(dir: &Path, rel: &str, status: SessionStatus) -> Option<SessionRowVm> {
    let folder_name = rel.rsplit('/').next().unwrap_or(rel);
    let names = entry_names(dir);
    let flat = shape_of(&names) == Shape::Flat;
    let record_name = if flat { ABOUT } else { README };
    let readme = std::fs::read_to_string(dir.join(record_name)).unwrap_or_default();
    let (fm, body_at) = Frontmatter::parse(&readme);
    let body = &readme[body_at..];

    // Identity: the frontmatter id when present; otherwise a path-derived
    // stand-in. Minting a real ULID into the file is the lifecycle layer's
    // job (it owns the one writer) — until then the row is honest about
    // indexing by path, exactly as notes treat a foreign id (FR-226).
    let id = fm
        .as_string("id")
        .map(str::to_owned)
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("path:{rel}"));

    let title = {
        let from_body = title_from_body(body);
        let fm_title = fm.as_string("title").unwrap_or_default().trim().to_owned();
        if !fm_title.is_empty() {
            fm_title
        } else if !from_body.is_empty() {
            from_body
        } else {
            folder_name.to_owned()
        }
    };

    let tags = fm.as_list("tags").unwrap_or_default();
    let pinned = fm.as_bool("pinned").unwrap_or(false);
    let line = lineage(&fm);
    let fresh = walk_freshness(dir);
    let (last_log_date, last_log_line) = if flat {
        // The pool's own source list, walked for paths only: the row reads at
        // most `LOG_PROBE_BUDGET` of these files, the pool reads down the same
        // list until its byte budget runs out. A walk that stopped short is not
        // reported on the row — the row is one line about the last sitting, and
        // "at least this" is what every other bounded signal here promises
        // (`walk_freshness`'s own rule).
        let (names, _truncated) = markdown_rels(dir, true);
        last_log_flat(dir, &names)
    } else {
        last_log(body)
    };

    let (status_str, archived_year) = match status {
        SessionStatus::Active => ("active", None),
        SessionStatus::Archived(year) => ("archived", Some(year)),
    };

    Some(SessionRowVm {
        id,
        path: rel.to_owned(),
        title,
        status: status_str.to_owned(),
        archived_year,
        workspace_ms: fresh.workspace_ms,
        record_ms: fresh.record_ms,
        last_log_date,
        last_log_line,
        snippet: section_snippet(body, "## Summary"),
        tags,
        pinned,
        // Unread and origin are the provenance projection's (Story 48.3);
        // until it lands the row claims nothing rather than guessing.
        unread: false,
        origin: String::new(),
        head_rev: String::new(),
        conflict: readme_conflict(dir),
        lineage: !line.continues.is_empty() || !line.continued_by.is_empty(),
    })
}

/// Fold the session's file mtimes into the two freshness signals, with the
/// workspace walk budgeted (NFR-37): the record side is small by the zone's
/// own contract, the scratch side can be anything, so the scratch walk stops
/// at [`WORKSPACE_WALK_BUDGET`] entries and reports what it has — freshness
/// "at least T", never a stalled scan.
fn walk_freshness(dir: &Path) -> Freshness {
    let mut facts: Vec<(String, i64)> = Vec::new();
    let mut workspace_budget = WORKSPACE_WALK_BUDGET;
    collect_mtimes(dir, "", &mut facts, &mut workspace_budget);
    freshness(facts.iter().map(|(p, m)| (p.as_str(), *m)))
}

/// Recursively record `(session-relative path, mtime_ms)`. `workspace_budget`
/// is decremented for every entry visited under `workspace/`; at zero the
/// walk stops descending there.
fn collect_mtimes(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, i64)>,
    workspace_budget: &mut usize,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == ".keeper" {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let in_workspace = rel == WORKSPACE_DIR || rel.starts_with("workspace/");
        if in_workspace {
            if *workspace_budget == 0 {
                continue;
            }
            *workspace_budget -= 1;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_mtimes(&entry.path(), &rel, out, workspace_budget);
        } else if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(since) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    out.push((rel, since.as_millis() as i64));
                }
            }
        }
    }
}

/// Whether a sync conflict copy sits beside the README (FR-228). The engine's
/// conflict copies carry `sync-conflict` in the name (AD-43).
fn readme_conflict(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .contains("sync-conflict")
    })
}

/// The newest `### YYYY-MM-DD — …` log entry's date and first content line.
/// "Newest last" is the zone's own convention, so the last heading wins.
fn last_log(body: &str) -> (String, String) {
    let mut date = String::new();
    let mut line_out = String::new();
    let mut in_entry = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### ") {
            let candidate = rest.trim();
            // A log heading starts with a date; other ### headings do not.
            if candidate.len() >= 10 && candidate.as_bytes()[4] == b'-' {
                date = candidate.chars().take(10).collect();
                line_out.clear();
                in_entry = true;
                // What follows the dash on the heading itself is the entry's
                // own summary line.
                if let Some((_, after)) = candidate.split_once('—') {
                    let after = after.trim();
                    if !after.is_empty() {
                        line_out = after.to_owned();
                    }
                }
                continue;
            }
        }
        if in_entry && line_out.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
            line_out = trimmed.to_owned();
        }
        if in_entry && trimmed.starts_with("## ") {
            in_entry = false;
        }
    }
    (date, line_out)
}

/// Compose one session's detail — header facts, properties, the rendered
/// log, and the file sections (FR-233). One directory read plus one record
/// parse; every field derivable from files alone (AD-110).
///
/// **Both contracts, one payload.** Which file holds the record and where the
/// log lives differ by shape, and *nothing downstream of here knows that*: the
/// header, the properties widget and the timeline render identically either
/// way. The shape is reported so the UI can decide what to **offer** — a
/// migrate button, a new-log button — never what a file means.
pub fn detail(
    root_id: &str,
    session_id: &str,
) -> Option<keeper_core::sessions::vm::SessionDetailVm> {
    use keeper_core::sessions::shape::ABOUT;
    use keeper_core::sessions::vm::{
        SessionDetailVm, SessionLogEntryVm, SessionPropertyVm, SessionTaskVm,
    };

    let zone = zone_of(root_id)?;
    let row = row_of(root_id, session_id)?;
    let dir = zone.join(&row.path);

    // One read of the session root, reused for everything: the shape, the pool
    // and the record. `ref_sources`' own scan is a separate call with a separate
    // budget, deliberately — the detail must not pay for `refs/`.
    let (sources, _truncated, shape) = read_ref_sources(&dir, DETAIL_SCAN_BUDGET);
    let flat = shape == Shape::Flat;

    // The record: `about.md` under the flat contract, `README.md` under the
    // folder one. Under the flat shape a missing `about.md` is ordinary — a
    // session may be nothing but logs — and an empty parse degrades exactly the
    // way an empty README already does.
    let record_name = if flat { ABOUT } else { README };
    let record = sources
        .iter()
        .find(|source| source.rel == record_name)
        .map(|source| source.text.clone())
        .unwrap_or_else(|| std::fs::read_to_string(dir.join(record_name)).unwrap_or_default());
    let readme = record;
    let (fm, body_at) = Frontmatter::parse(&readme);
    let body = &readme[body_at..];
    let line = lineage(&fm);

    // The pool, under both contracts, from that one scan.
    let pool = detail_pool(&sources, shape);

    // The properties widget (FR-227): user-tier keys only. keeper-owned keys
    // and the Obsidian-native `tags` are projected elsewhere on the header;
    // repeating them here would be two spellings of one fact.
    let owned = [
        "id",
        "created",
        "updated",
        "pinned",
        "archived",
        "keeper",
        "tags",
        "aliases",
        "cssclasses",
        "title",
    ];
    let properties: Vec<SessionPropertyVm> = fm
        .keys()
        .filter(|key| !owned.contains(key) && !key.starts_with("keeper."))
        .filter_map(|key| {
            fm.get(key).map(|value| SessionPropertyVm {
                key: key.to_owned(),
                value: value.index_string(),
            })
        })
        .collect();

    // The log, from whichever contract this session follows. `log_view` owns
    // that branch so the two readings cannot drift into two ideas of what an
    // entry is; the folder path stays byte-identical to what it always was
    // (parse `## Log`, reverse into review order — the FILE stays newest-last).
    let log: Vec<SessionLogEntryVm> = if flat {
        // Bodies come from the same texts the pool was parsed from, indexed in
        // step with `pool.logs`.
        let texts: Vec<&str> = pool
            .logs
            .iter()
            .map(|entry| {
                sources
                    .iter()
                    .find(|source| source.rel == entry.rel)
                    .map(|source| source.text.as_str())
                    .unwrap_or("")
            })
            .collect();
        keeper_core::sessions::pool::log_view_with_bodies(&pool, &texts)
    } else {
        keeper_core::sessions::pool::log_view(shape, body, &pool)
    }
    .into_iter()
    .map(|(date, title, entry_body)| SessionLogEntryVm {
        date,
        title,
        body: entry_body,
    })
    .collect();

    let tasks: Vec<SessionTaskVm> = pool
        .tasks
        .iter()
        .map(|entry| SessionTaskVm {
            id: entry.id.clone(),
            rel_path: entry.rel.clone(),
            title: entry.title.clone(),
            status: entry.status.map(|status| status.as_str().to_owned()),
            order: entry.order.value,
            order_is_own: entry.order.is_own(),
            tags: entry.tags.clone(),
            unstable_identity: entry.unstable_identity,
        })
        .collect();

    let unfiled: Vec<String> = pool.unfiled.iter().map(|entry| entry.rel.clone()).collect();

    let (status, archived_year) = (row.status.clone(), row.archived_year);
    Some(SessionDetailVm {
        id: row.id,
        path: row.path,
        title: row.title,
        status,
        archived_year,
        pinned: row.pinned,
        tags: row.tags,
        properties,
        continues: line.continues,
        continued_by: line.continued_by,
        summary: section_snippet(body, "## Summary"),
        log,
        shape: shape.as_str().to_owned(),
        unfiled,
        tasks,
    })
}

/// The pool the DETAIL reads, out of one markdown scan (FR-286).
///
/// Split out of [`detail`] so a test can hand it a scan of a folder rather than
/// having to register a root, and so the exclusion below has one reader instead
/// of being a line inside a 150-line projection.
///
/// **Both contracts read a pool now** (Story 51.7). The folder shape got one in
/// Story 51.1 — its root markdown is in [`read_ref_sources`]' walk — and this
/// was the last reader still answering as though it had none, which is what left
/// a folder-shaped session's `task`-tagged file out of the board and its
/// untagged root markdown out of *Unfiled*.
///
/// **The record is left out, and only under the folder contract.** `README.md`
/// declares no kind, so feeding it in would report the one file keeper reads the
/// session's identity, title, tags and lineage out of as *unfiled* — an
/// accusation against the file that is doing its job. A flat session's
/// `about.md` needs no such exclusion: it carries `tags: [about]`, which is the
/// flat contract's whole premise, so that pool is byte-for-byte what it was.
fn detail_pool(sources: &[RefSource], shape: Shape) -> keeper_core::sessions::pool::Pool {
    use keeper_core::sessions::pool::{read_pool, PoolFile};

    let files: Vec<PoolFile<'_>> = sources
        .iter()
        .filter(|source| shape == Shape::Flat || source.rel != README)
        .map(|source| PoolFile {
            rel: &source.rel,
            text: &source.text,
        })
        .collect();
    read_pool(&files)
}

/// The most markdown one session's *detail* reads, in bytes.
///
/// Its own constant rather than a share of [`REF_SCAN_BUDGET`], because the two
/// scans answer different questions and are triggered at different rates: the
/// detail re-reads on every open and every log write, the reference scan only
/// when the references widget asks. Same ceiling today; separate so that
/// tightening one never silently truncates the other.
const DETAIL_SCAN_BUDGET: usize = 10 * 1024 * 1024;

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

/// One markdown file a session's references were read from.
pub struct RefSource {
    /// Session-relative path, as a row reports it: `README.md`,
    /// `refs/inputs.md`. This is the "where it was written" a reader needs to
    /// go and fix a broken pointer, so it is carried rather than derived.
    pub rel: String,
    /// The file's text, already read — the scan parses it, so reading it twice
    /// would double the one cost this walk actually has.
    pub text: String,
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

/// One zone's `_spaces/` on disk: the definitions, and whether the directory
/// exists at all.
///
/// The `Option` is the whole seeding rule in a type (FR-261). `None` means the
/// directory has never been created and keeper may write its five defaults;
/// `Some(vec![])` means the operator has one and emptied it, and keeper adds
/// nothing to it uninvited. Collapsing the two into an empty vector is exactly
/// the ambiguity `_spaces/` was chosen to avoid having a ledger file for.
pub struct ZoneSpaces {
    pub spaces: Vec<SessionSpace>,
    /// The raw bytes of each, keyed by zone-relative path — what a save splices
    /// against, so the edit path does not re-read a file the list just read.
    pub sources: HashMap<String, String>,
    /// Whether `_spaces/` exists.
    pub seeded: bool,
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

/// [`zone_spaces`] over a plain directory, so a test can hand it a folder.
fn read_zone_spaces(zone: &Path) -> ZoneSpaces {
    let dir = zone.join(SPACES_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // Unreadable and absent are one answer here on purpose: both mean
        // keeper has no definitions to show, and a permission error on a
        // directory keeper itself creates is not a case worth a second path.
        return ZoneSpaces {
            spaces: Vec::new(),
            sources: HashMap::new(),
            seeded: false,
        };
    };
    let mut sources: HashMap<String, String> = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.to_lowercase().ends_with(".md") {
            continue;
        }
        if !entry.path().is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        sources.insert(format!("{SPACES_DIR}/{name}"), text);
    }
    let pairs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(rel, text)| (rel.as_str(), text.as_str()))
        .collect();
    let spaces = spaces::read_all(&pairs);
    drop(pairs);
    ZoneSpaces {
        spaces,
        sources,
        seeded: true,
    }
}

/// One session's markdown pool with the two facts a space needs and the pool
/// does not carry: when each file changed, and its bytes.
///
/// Returned as owned strings rather than as [`keeper_core::sessions::spaces::Candidate`]s
/// because those borrow, and the caller has to hold the texts alive anyway to
/// evaluate a `text:` term against them.
pub struct SessionPool {
    /// Session path, zone-relative — what a `relPath` is joined onto.
    pub path: String,
    /// `(session-relative path, text, mtime_ns)` in reading order — a path
    /// rather than a name, because markdown is read wherever it sits (FR-285).
    pub files: Vec<(String, String, i128)>,
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
pub fn session_pool(root_id: &str, session_id: &str) -> Option<SessionPool> {
    let row = row_of(root_id, session_id)?;
    let zone = zone_of(root_id)?;
    let dir = zone.join(&row.path);
    let (sources, _truncated, _shape) = read_ref_sources(&dir, DETAIL_SCAN_BUDGET);
    let files = sources
        .into_iter()
        .map(|source| {
            let mtime_ns = std::fs::metadata(dir.join(&source.rel))
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos() as i128);
            (source.rel, source.text, mtime_ns)
        })
        .collect();
    Some(SessionPool {
        path: row.path,
        files,
    })
}

/// The most markdown one session's reference scan reads, in bytes. Ten
/// megabytes of prose is far past any session a person writes and far under
/// anything that would be felt.
const REF_SCAN_BUDGET: usize = 10 * 1024 * 1024;

/// Where references are read from, after the root, in reading order.
///
/// The domain's own constants, not a third spelling of two folder names: this
/// array is the READER that `shape::kind_dir` writes for, and the day the two
/// disagree a create lands where the pool does not look — the exact defect the
/// mapping was made public to prevent.
const REF_DIRS: [&str; 2] = [REFS_DIR, PROMPTS_DIR];

/// The directories a markdown scan of a session never enters, under either
/// shape.
///
/// `artifacts/` — promoted output is a deliverable, and a reference inside it is
/// a reference from the artifact, not from the session. `workspace/` — scratch
/// that dies with the session (AD-113), so a task or a log filed there would be
/// a card that vanishes, and a broken pointer in a file nobody keeps is not
/// worth reporting.
///
/// Public, with [`scans_markdown`], because it is the one list: a directory
/// keeper offers to create but never reads is a directory whose files are in no
/// pool, no space and not even *Unfiled* — the exact defect this scan was
/// widened to fix. Two lists is how the create side and the read side come to
/// disagree, and the disagreement is silent.
pub const UNSCANNED_DIRS: [&str; 2] = [ARTIFACTS_DIR, WORKSPACE_DIR];

/// Whether the markdown scan enters a session subdirectory of this name.
///
/// Dotted names are furniture — `.git`, `.obsidian`, the zone's own `.keeper` —
/// and are refused here rather than listed in [`UNSCANNED_DIRS`] because the
/// rule is a prefix, not a name: the same rule the dotfile filter applies one
/// level down.
///
/// **Folded, because the drive folds.** `Artifacts/` and `artifacts/` are one
/// directory on the volume keeper ships on, so an exclusion that held for only
/// one spelling of a name would put promoted output and `workspace/` scratch —
/// scratch that dies with the session (AD-113) — into the pool, into every
/// space evaluated over it, and into the *Unfiled* notice. The same reason
/// `sessions::files::check_dir` folds its own fence.
pub fn scans_markdown(dir_name: &str) -> bool {
    !dir_name.starts_with('.')
        && !UNSCANNED_DIRS
            .iter()
            .any(|excluded| dir_name.eq_ignore_ascii_case(excluded))
}

/// The most directory entries one session's markdown walk visits before it
/// stops and says so.
///
/// [`WORKSPACE_WALK_BUDGET`]'s reason, applied to the side of the session that
/// is walked now too: the record side is small by the zone's own contract, but
/// "small" is the operator's word and nothing stops him keeping a checkout in a
/// folder he made. The byte budget cannot stand in for this one — it is spent in
/// [`read_ref_sources`]'s `take`, *after* the walk has already materialised
/// every path — and this walk runs once per board row on top of
/// [`walk_freshness`], so an unbounded one would allocate a string per markdown
/// file in the zone to read at most [`LOG_PROBE_BUDGET`] of them per session.
const MARKDOWN_WALK_BUDGET: usize = 2_000;

/// Every `.md` under `dir`, session-relative and in reading order: each
/// directory's own files in folded name order, then its subdirectories in the
/// same order. `descend` false stops at `dir` itself — the folder contract's
/// listing, whose subdirectories are named by `shape::kind_dir` and read
/// explicitly rather than discovered.
///
/// **Paths only, no bytes.** Two callers with two appetites read down one list:
/// the pool takes text until its byte budget runs out, the board's newest-log
/// probe opens at most [`LOG_PROBE_BUDGET`] of them. One walk, so the row and
/// the pool cannot disagree about what the session contains.
///
/// **Bounded, and the bound is reported.** The second value is `true` when this
/// list is a prefix of what is there: [`MARKDOWN_WALK_BUDGET`] entries were
/// visited, or a directory could not be listed at all. Two causes, one fact —
/// *this is not everything* — and it reaches the caller because the pool is what
/// every space is evaluated over, so a card that quietly left a board with
/// nothing anywhere saying so is the worst shape a failure has.
///
/// **Iterative, and no-follow.** An explicit worklist rather than recursion, so
/// what bounds the walk is the budget above and not the stack. The type comes
/// from `read_dir` — one syscall for the directory instead of the stat-per-name
/// the root listing used to do — so a symlinked directory reports as a symlink,
/// is neither a directory to enter nor a file to read, and is therefore refused.
/// Following one is the version that reads somebody's whole home directory
/// because they linked `~/notes` into a session.
fn markdown_rels(dir: &Path, descend: bool) -> (Vec<String>, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut budget = MARKDOWN_WALK_BUDGET;
    let mut truncated = false;
    // Session-relative prefixes still to visit. Popped from the end, with each
    // directory's children pushed in reverse, this is a depth-first walk in
    // name order without a recursive call.
    let mut pending: Vec<String> = vec![String::new()];
    while let Some(prefix) = pending.pop() {
        if budget == 0 {
            // Whatever is still pending is unread, and the files already found
            // in the directory the budget ran out in are kept: a prefix that
            // says it is a prefix beats an empty answer.
            truncated = true;
            break;
        }
        let here = if prefix.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(&prefix)
        };
        let Ok(entries) = std::fs::read_dir(&here) else {
            // A directory keeper cannot list is not a directory with no
            // markdown: every file under it is missing from the pool, from
            // every space evaluated over it and from the *Unfiled* notice, so
            // the caller is told rather than handed a short list that looks
            // complete.
            truncated = true;
            continue;
        };
        let mut files: Vec<String> = Vec::new();
        let mut dirs: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            if budget == 0 {
                truncated = true;
                break;
            }
            budget -= 1;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if descend && scans_markdown(&name) {
                    dirs.push(name);
                }
            } else if name.to_lowercase().ends_with(".md")
                && (file_type.is_file() || entry.path().is_file())
            {
                // The type `read_dir` already handed back answers for every
                // regular file, which is every file in practice. `is_file`
                // follows, deliberately and only for what that type called
                // neither a file nor a directory: a symlinked markdown file is
                // one bounded file and was already read before this walk
                // existed. A symlinked *directory* fails both arms.
                files.push(name);
            }
        }
        // Folded, then the name itself. A key that is not injective would leave
        // two entries that fold equal — `README.md` beside `readme.md`, both
        // legal on ext4 — in `read_dir`'s order, which is a hash order that is
        // not stable between runs, so which of the two survived the byte budget
        // would change between launches. `pool::cmp_stamped`'s tie-break, for
        // its reason.
        files.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
        for name in files {
            out.push(join_rel(&prefix, &name));
        }
        dirs.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
        for name in dirs.into_iter().rev() {
            pending.push(join_rel(&prefix, &name));
        }
    }
    (out, truncated)
}

/// `prefix/name`, or `name` at the session root.
fn join_rel(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// [`ref_sources`] over a plain directory — the half that is about files rather
/// than about which session, so a test can hand it a folder.
///
/// The `read_dir` of the session root that decides the shape is its own: the
/// shape is a question about names at one level, and the walk that follows needs
/// each entry's type. Threading one listing through both would save a cached
/// directory read and cost the shape signal its independence from the walk's
/// exclusions.
fn read_ref_sources(dir: &Path, budget: usize) -> (Vec<RefSource>, bool, Shape) {
    let top_level: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let shape = shape_of(&top_level);

    let mut out: Vec<RefSource> = Vec::new();
    let mut budget = budget;
    // A walk that stopped short and a budget that ran out are the same fact to
    // the caller, so they arrive as one flag.
    let mut incomplete = false;

    let mut take = |rel: String, path: &Path, budget: &mut usize| {
        let Ok(text) = std::fs::read_to_string(path) else {
            // Unreadable or not UTF-8 — a binary in `refs/` is ordinary, and a
            // file keeper cannot read is a file with no references, not an
            // error worth failing the whole widget over.
            return;
        };
        if text.len() > *budget {
            *budget = 0;
            return;
        }
        *budget -= text.len();
        out.push(RefSource { rel, text });
    };

    if shape == Shape::Flat {
        // Name order, which for logs is also date order — the filename carries
        // `YYYY-MM-DD-HHMM` precisely so that a plain sort is chronological —
        // and each directory before the ones inside it, so a session still
        // reads as its root plus whatever the operator filed away.
        let (rels, walk_truncated) = markdown_rels(dir, true);
        incomplete |= walk_truncated;
        for rel in rels {
            if budget == 0 {
                break;
            }
            let path = dir.join(&rel);
            take(rel, &path, &mut budget);
        }
        return (out, incomplete || budget == 0, shape);
    }

    take(README.to_owned(), &dir.join(README), &mut budget);
    // The rest of the root, beside the record (FR-286). Skipping the record
    // itself is the whole of "not duplicated": one file is one entry, which is
    // what the flat shape's `about.md` gets too. Folded, because a case-
    // insensitive drive answers `README.md` with whatever spelling it holds and
    // the record must not come back a second time under it.
    let (root_names, root_truncated) = markdown_rels(dir, false);
    incomplete |= root_truncated;
    for name in root_names {
        if budget == 0 {
            break;
        }
        if name.eq_ignore_ascii_case(README) {
            continue;
        }
        let path = dir.join(&name);
        take(name, &path, &mut budget);
    }

    for section in REF_DIRS {
        // The same listing as the root's, one directory down — a missing
        // section is an empty one. Name order inside a section: `prompts/` is
        // numbered by the zone's own convention (`01-…`, `02-…`), and mtime
        // order would scramble it.
        let (section_names, section_truncated) = markdown_rels(&dir.join(section), false);
        // A missing section is not an unreadable one: `read_dir` on a path that
        // does not exist is the flag's other cause, and a folder session with no
        // `prompts/` is ordinary rather than short. Only a section that IS there
        // and cannot be listed should say so — which is what `is_dir` asks.
        incomplete |= section_truncated && dir.join(section).is_dir();
        for name in section_names {
            if budget == 0 {
                break;
            }
            let rel = format!("{section}/{name}");
            let path = dir.join(&rel);
            take(rel, &path, &mut budget);
        }
    }

    (out, incomplete || budget == 0, shape)
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

/// First non-empty prose line under a `## <name>` heading.
fn section_snippet(body: &str, heading: &str) -> String {
    let mut in_section = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == heading {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("## ") {
                return String::new();
            }
            if !trimmed.is_empty() && !trimmed.starts_with("<!--") {
                return trimmed.to_owned();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan over a zone shaped exactly like the live drives: template
    /// skipped, active and archived found, freshness split, log read.
    #[test]
    fn a_zone_shaped_like_the_live_drives_scans_to_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zone = dir.path();
        let mk = |rel: &str| std::fs::create_dir_all(zone.join(rel)).expect("mkdir");
        mk("_template/workspace");
        mk("active/2026-08-10-keeper/workspace");
        mk("active/2026-08-10-keeper/artifacts");
        mk("archive/2025/2025-03-01-taxes/artifacts");
        mk("active/.hidden");
        std::fs::write(
            zone.join("active/2026-08-10-keeper/README.md"),
            "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\npinned: true\ntags:\n  - project/keeper\n---\n# keeper — rolling work session\n\n## Summary\n\nState as of opening.\n\n## Log\n\n### 2026-08-10 — opened\n\n### 2026-08-11 — shipped 0.6.5\n",
        )
        .expect("write");
        std::fs::write(zone.join("active/2026-08-10-keeper/workspace/iter.md"), "x")
            .expect("write");
        std::fs::write(
            zone.join("archive/2025/2025-03-01-taxes/README.md"),
            "# taxes\n",
        )
        .expect("write");

        let rows = scan_zone(zone);
        assert_eq!(rows.len(), 2, "template and dotdirs are not sessions");

        let keeper = &rows[0];
        assert_eq!(keeper.id, "01J5AAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(keeper.status, "active");
        assert_eq!(keeper.title, "keeper — rolling work session");
        assert!(keeper.pinned);
        assert_eq!(keeper.tags, vec!["project/keeper"]);
        assert_eq!(keeper.last_log_date, "2026-08-11");
        assert_eq!(keeper.last_log_line, "shipped 0.6.5");
        assert_eq!(keeper.snippet, "State as of opening.");
        assert!(keeper.workspace_ms.is_some(), "scratch moved the ws signal");
        assert!(keeper.record_ms.is_some());

        let taxes = &rows[1];
        assert_eq!(taxes.status, "archived");
        assert_eq!(taxes.archived_year, Some(2025));
        assert_eq!(
            taxes.id, "path:archive/2025/2025-03-01-taxes",
            "no frontmatter id indexes by path, honestly"
        );
        assert_eq!(taxes.title, "taxes");
    }

    /// The log reader follows the zone's newest-last convention and reads the
    /// heading's own summary when one is on the dash.
    #[test]
    fn the_last_log_entry_wins_and_the_dash_line_is_the_subtitle() {
        let (date, line) = last_log(
            "## Log\n\n### 2026-08-01 — first\n\ntext\n\n### 2026-08-09\n\nthe body line\n",
        );
        assert_eq!(date, "2026-08-09");
        assert_eq!(line, "the body line");
    }

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

    /// The reference scan reads the record and the inputs, and deliberately
    /// does NOT read the deliverables or the scratch (FR-255).
    #[test]
    fn references_are_read_from_the_record_and_the_inputs_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in ["refs", "prompts", "artifacts", "workspace"] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("README.md", "the record");
        write("refs/inputs.md", "the inputs");
        // `prompts/` is numbered by the zone's own convention, so 02 must not
        // sort before 10 by mtime or by anything else.
        write("prompts/10-later.md", "later");
        write("prompts/02-earlier.md", "earlier");
        write("refs/.hidden.md", "furniture");
        write("refs/clip.m4a", "not markdown");
        write("artifacts/report.md", "the deliverable's own references");
        write("workspace/iter-3.md", "scratch that dies with the session");

        let (files, truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert!(!truncated);
        assert_eq!(
            shape,
            Shape::Folder,
            "a README with refs/ and prompts/ is the original contract"
        );
        let read: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(
            read,
            vec![
                "README.md",
                "refs/inputs.md",
                "prompts/02-earlier.md",
                "prompts/10-later.md",
            ],
            "record, then inputs, then prompts in the zone's own numbering — \
             artifacts and workspace are not the session's references, and a \
             dotfile and a media file are not markdown"
        );
        assert_eq!(
            files[0].text, "the record",
            "the text is carried, not re-read"
        );
    }

    /// The budget is bytes, because the cost here is parsing markdown — and an
    /// exhausted budget is reported rather than handed back as a prefix.
    #[test]
    fn the_reference_budget_counts_bytes_and_says_when_it_runs_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("refs")).expect("mkdir");
        std::fs::write(session.join("README.md"), "x".repeat(40)).expect("write");
        std::fs::write(session.join("refs/big.md"), "y".repeat(400)).expect("write");

        let (files, truncated, _shape) = read_ref_sources(session, 100);
        assert_eq!(
            files.len(),
            1,
            "the README fits; the file that would blow the budget is not read"
        );
        assert!(
            truncated,
            "and the caller is told, rather than shown a prefix"
        );
    }

    /// Under the flat contract the pool IS the source list: every root `.md`,
    /// in name order, and still nothing from the two directories that are not
    /// markdown (FR-256).
    #[test]
    fn a_flat_session_reads_every_root_md_and_still_skips_artifacts_and_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in ["artifacts", "workspace"] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("AGENTS.md", "how to read this folder");
        write("about.md", "the record");
        write("2026-08-12-1400-second.md", "later sitting");
        write("2026-08-12-0900-first.md", "earlier sitting");
        write("01-prompt.md", "reusable text");
        write(".hidden.md", "furniture");
        write("clip.m4a", "not markdown");
        write("artifacts/report.md", "the deliverable's own references");
        write("workspace/iter-3.md", "scratch that dies with the session");

        let (files, truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert_eq!(shape, Shape::Flat, "AGENTS.md declares the flat contract");
        assert!(!truncated);
        let read: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(
            read,
            vec![
                "01-prompt.md",
                "2026-08-12-0900-first.md",
                "2026-08-12-1400-second.md",
                "about.md",
                "AGENTS.md",
            ],
            "every root .md in name order — which for logs is date order, since \
             the filename carries the clock; artifacts and workspace are still \
             not the session's references, and a dotfile and a media file are \
             still not markdown. The sort folds case, as the prompts/ sort and \
             the pool's own already do, because APFS and NTFS do not \
             distinguish the two spellings either"
        );
    }

    /// The detail composes from whichever contract it finds, and the payload
    /// it produces is the same shape either way (FR-256).
    #[test]
    fn a_flat_sessions_log_and_board_come_from_its_tagged_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("AGENTS.md", "---\ntags: []\n---\nhow to read this folder\n");
        write("about.md", "---\ntags: [about]\n---\n# The session\n");
        write(
            "2026-08-11-0900-opened.md",
            "---\ntags: [log]\n---\n# opened\n\nfirst sitting\n",
        );
        write(
            "2026-08-12-1700-closed.md",
            "---\ntags: [log]\n---\n# closed\n\nwrapped up\n",
        );
        write(
            "ship-it.md",
            "---\ntags: [task]\nstatus: todo\norder: 1.5\n---\n# Ship it\n",
        );
        write("README.md", "left over from a half-finished migration\n");

        let (sources, _truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert_eq!(shape, Shape::Flat);
        let pool = keeper_core::sessions::pool::read_pool(
            &sources
                .iter()
                .map(|source| keeper_core::sessions::pool::PoolFile {
                    rel: &source.rel,
                    text: &source.text,
                })
                .collect::<Vec<_>>(),
        );

        let log = keeper_core::sessions::pool::log_view(shape, "", &pool);
        assert_eq!(
            log.iter()
                .map(|(_, title, _)| title.as_str())
                .collect::<Vec<_>>(),
            vec!["closed", "opened"],
            "newest first, the review order — same as the folder contract's"
        );
        assert_eq!(log[0].0, "2026-08-12", "the date comes from the filename");

        assert_eq!(pool.tasks.len(), 1);
        assert_eq!(pool.tasks[0].title, "Ship it");
        assert_eq!(pool.tasks[0].order.value, 1.5);

        assert_eq!(
            pool.unfiled
                .iter()
                .map(|e| e.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["AGENTS.md", "README.md"],
            "the navigation file declares no kind, and the residual README is \
             visible rather than merely survivable"
        );
    }

    /// Markdown is legible wherever it sits (FR-285). A file the operator moved
    /// into a directory he made is in the pool, carries that directory in its
    /// `rel`, and is listed by whatever its tag says — and the three exclusions
    /// are the only places the walk does not go.
    #[test]
    fn markdown_in_a_flat_sessions_subdirectories_is_read_and_the_exclusions_are_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in [
            "spaces",
            "log",
            "log/older",
            "artifacts",
            "workspace",
            ".hidden",
        ] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("AGENTS.md", "---\ntags: []\n---\nhow to read this folder\n");
        write("about.md", "---\ntags: [about]\n---\n# The session\n");
        write(
            "spaces/plan.md",
            "---\ntags: [task]\nstatus: todo\n---\n# Plan it\n",
        );
        write(
            "log/2026-08-16-0900-note.md",
            "---\ntags: [log]\n---\n# note\n\nthe sitting he filed\n",
        );
        write(
            "log/older/2026-08-01-0900-first.md",
            "---\ntags: [log]\n---\n# first\n",
        );
        write("artifacts/report.md", "the deliverable's own references");
        write("workspace/scratch.md", "scratch that dies with the session");
        write(".hidden/x.md", "furniture");
        write("spaces/.draft.md", "furniture one level down");

        let (files, truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert_eq!(shape, Shape::Flat, "about.md declares the flat contract");
        assert!(!truncated);
        let read: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(
            read,
            vec![
                "about.md",
                "AGENTS.md",
                "log/2026-08-16-0900-note.md",
                "log/older/2026-08-01-0900-first.md",
                "spaces/plan.md",
            ],
            "each directory's own files before the directories inside it, in \
             folded name order; `artifacts/`, `workspace/` and a dotted \
             directory are not read, and a dotfile is furniture at any depth"
        );

        let pool = keeper_core::sessions::pool::read_pool(
            &files
                .iter()
                .map(|source| keeper_core::sessions::pool::PoolFile {
                    rel: &source.rel,
                    text: &source.text,
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            pool.tasks
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["spaces/plan.md"],
            "the Tasks space lists it, and `rel` carries the subdirectory — so \
             a `spaces/plan.md` and a root `plan.md` are two entries"
        );
        // Order, not merely membership. The log view is newest-first by the
        // FILENAME, so a sitting filed one directory deeper is dated by its own
        // name and not by the folder it sits in — and sorting this vector before
        // asserting is exactly what used to hide `pool::group` ordering the
        // whole `rel`. The board row keys on the same comparator, so the two
        // surfaces are asserted together below.
        assert_eq!(
            pool.logs
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec![
                "log/2026-08-16-0900-note.md",
                "log/older/2026-08-01-0900-first.md",
            ],
            "a `log/` the operator made is a real home: both sittings are in \
             the log view rather than in no reader at all, newest first"
        );
        let (names, _truncated) = markdown_rels(session, true);
        assert_eq!(
            last_log_flat(session, &names),
            ("2026-08-16".to_owned(), "the sitting he filed".to_owned()),
            "and the row announces the sitting the log view lists first — one \
             walk, one order, so the board and the detail cannot disagree"
        );
    }

    /// The folder shape reads the markdown beside its record (FR-286) — the
    /// owner's `references.md`, written at the root before Story 50.1 filed by
    /// kind, becomes visible to References and to every space. The record stays
    /// the record: one file, one entry.
    #[test]
    fn a_folder_sessions_root_markdown_is_read_and_the_record_is_not_duplicated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in ["refs", "prompts", "artifacts", "workspace"] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("README.md", "---\ntags: []\n---\n# The session\n");
        write(
            "references.md",
            "---\ntags: [ref]\n---\n# What this session points at\n",
        );
        write("inputs.md", "---\ntags: [ref]\n---\n# Root inputs\n");
        write("refs/inputs.md", "---\ntags: [ref]\n---\n# Filed inputs\n");
        write("prompts/01-house.md", "reusable text");
        write("artifacts/report.md", "the deliverable's own references");
        write("workspace/iter-3.md", "scratch that dies with the session");

        let (files, truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert_eq!(shape, Shape::Folder, "a README with refs/ and prompts/");
        assert!(!truncated);
        let read: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(
            read,
            vec![
                "README.md",
                "inputs.md",
                "references.md",
                "refs/inputs.md",
                "prompts/01-house.md",
            ],
            "the record first, then the rest of the root because that is where \
             it sits, then the contract's own sections in the contract's order"
        );
        assert_eq!(
            read.iter().filter(|rel| **rel == README).count(),
            1,
            "the record is taken once — by name, as the record — and the root \
             listing does not hand it back a second time"
        );

        let pool = keeper_core::sessions::pool::read_pool(
            &files
                .iter()
                .map(|source| keeper_core::sessions::pool::PoolFile {
                    rel: &source.rel,
                    text: &source.text,
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            pool.refs
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["inputs.md", "references.md", "refs/inputs.md"],
            "`tag:ref` finds the orphan at the root, and a root `inputs.md` and \
             a `refs/inputs.md` are two entries a space can both list"
        );
    }

    /// Rows 4 and 7 of Story 51.7: the detail's own pool, under the folder
    /// contract.
    ///
    /// A `task`-tagged file at a folder-shaped session's root is a board card —
    /// this reader is what the board is drawn from, and it used to hand back
    /// `Pool::default()` for this shape, so the owner's board was hidden with the
    /// reason "a folder-shaped one has no pool to tag". Story 51.1 made that
    /// false.
    ///
    /// And the record is not accused: `README.md` declares no kind, so a pool
    /// that took it would name the session's own identity file as *unfiled*.
    #[test]
    fn the_detail_pool_finds_a_folder_sessions_tasks_and_leaves_its_record_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        for rel in ["refs", "prompts"] {
            std::fs::create_dir_all(session.join(rel)).expect("mkdir");
        }
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write(
            "README.md",
            "# The session\n\n## Log\n\n### 2026-08-16 first\n",
        );
        write(
            "ship-it.md",
            "---\ntags: [task]\nstatus: todo\norder: 1.5\n---\n# Ship it\n",
        );
        write("notes.md", "# Something nobody filed\n");
        write("refs/inputs.md", "---\ntags: [ref]\n---\n# Filed inputs\n");

        let (sources, _truncated, shape) = read_ref_sources(session, DETAIL_SCAN_BUDGET);
        assert_eq!(shape, Shape::Folder);
        let pool = detail_pool(&sources, shape);

        assert_eq!(
            pool.tasks
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["ship-it.md"],
            "the board's cards, on a shape whose board was hidden because it \
             was said to have no pool to tag"
        );
        assert_eq!(
            pool.unfiled
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["notes.md"],
            "root markdown declaring no kind is reported in this shape too, and \
             the record is not in the list: it is the file keeper reads the \
             session out of, not a file nobody filed"
        );
        assert!(
            pool.about.is_empty(),
            "and it is not carried in as an ordinary entry either"
        );
    }

    /// The other half of the same reader: a flat session's pool is what it was.
    /// Its record carries `tags: [about]`, so it needs no exclusion — and adding
    /// one would have taken `about.md` out of the About space.
    #[test]
    fn the_detail_pool_leaves_a_flat_sessions_record_in_the_pool_where_it_was() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("about.md", "---\ntags: [about]\n---\n# The session\n");
        write("AGENTS.md", "how to read this folder\n");
        write(
            "ship-it.md",
            "---\ntags: [task]\nstatus: todo\n---\n# Ship it\n",
        );

        let (sources, _truncated, shape) = read_ref_sources(session, DETAIL_SCAN_BUDGET);
        assert_eq!(shape, Shape::Flat);
        let pool = detail_pool(&sources, shape);

        assert_eq!(
            pool.about
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["about.md"]
        );
        assert_eq!(pool.tasks.len(), 1, "the board is unchanged");
        assert_eq!(
            pool.unfiled
                .iter()
                .map(|entry| entry.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["AGENTS.md"],
            "and so is the unfiled list, `AGENTS.md` included"
        );
    }

    /// The board row's newest-log line follows the pool: the probe reads down
    /// the same walk, and orders it by the clock the filename carries rather
    /// than by the directory the file sits in.
    #[test]
    fn the_board_rows_newest_log_follows_the_pool_into_a_log_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("log")).expect("mkdir");
        let write = |rel: &str, body: &str| {
            std::fs::write(session.join(rel), body).expect("write");
        };
        write("AGENTS.md", "how to read this folder\n");
        write("about.md", "---\ntags: [about]\n---\n# The session\n");
        write(
            "log/2026-08-15-0900-filed.md",
            "---\ntags: [log]\n---\n# filed\n\nthe sitting he moved\n",
        );
        write(
            "2026-08-17-0900-latest.md",
            "---\ntags: [log]\n---\n# latest\n\nthe newest sitting\n",
        );

        let row = row_for(session, "active/2026-08-10-keeper", SessionStatus::Active)
            .expect("a session directory projects to a row");
        assert_eq!(
            (row.last_log_date.as_str(), row.last_log_line.as_str()),
            ("2026-08-17", "the newest sitting"),
            "the newest sitting wins wherever the older one was filed — a path \
             sort would have announced the one in `log/`"
        );

        // And a session whose logs are ALL filed away still has a row that says
        // so, which is the half the root-only probe used to get wrong.
        let second = tempfile::tempdir().expect("tempdir");
        let filed = second.path();
        std::fs::create_dir_all(filed.join("log")).expect("mkdir");
        std::fs::write(filed.join("about.md"), "---\ntags: [about]\n---\n# S\n").expect("write");
        std::fs::write(
            filed.join("log/2026-08-16-1200-only.md"),
            "---\ntags: [log]\n---\n# only\n\nthe only sitting\n",
        )
        .expect("write");

        let row = row_for(filed, "active/2026-08-16-filed", SessionStatus::Active)
            .expect("a session directory projects to a row");
        assert_eq!(
            (row.last_log_date.as_str(), row.last_log_line.as_str()),
            ("2026-08-16", "the only sitting"),
        );
    }

    /// A walk that runs out of budget stops and says so: a truncated pool is
    /// reported, never silently short.
    #[test]
    fn a_subtree_that_blows_the_budget_is_reported_rather_than_silently_short() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("notes/deeper/deepest")).expect("mkdir");
        std::fs::write(session.join("AGENTS.md"), "x".repeat(40)).expect("write");
        std::fs::write(
            session.join("notes/deeper/deepest/long.md"),
            "y".repeat(400),
        )
        .expect("write");

        let (files, truncated, shape) = read_ref_sources(session, 100);
        assert_eq!(shape, Shape::Flat);
        assert_eq!(
            files.len(),
            1,
            "the navigation file fits; the one below it that would blow the \
             budget is not read"
        );
        assert!(truncated, "and the caller is told, not handed a prefix");
    }

    /// A session is not a vault index: 200 files across 12 directories is one
    /// walk, inside the budget the scan always had, in a defined order.
    #[test]
    fn two_hundred_files_across_twelve_directories_are_one_walk_within_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::write(session.join("AGENTS.md"), "---\ntags: []\n---\nnav\n").expect("write");
        let (folders, per_folder) = (12, 17);
        for folder in 0..folders {
            let sub = session.join(format!("d{folder:02}"));
            std::fs::create_dir_all(&sub).expect("mkdir");
            for file in 0..per_folder {
                std::fs::write(
                    sub.join(format!("f{file:02}.md")),
                    "---\ntags: [ref]\n---\n# a pointer\n",
                )
                .expect("write");
            }
        }

        let (files, truncated, _shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert!(!truncated, "204 short files are nowhere near ten megabytes");
        assert_eq!(files.len(), folders * per_folder + 1);
        assert_eq!(files[0].rel, "AGENTS.md", "the root before the tree");
        assert_eq!(files[1].rel, "d00/f00.md");
        assert_eq!(
            files.last().expect("a non-empty scan").rel,
            "d11/f16.md",
            "depth-first in folded name order, so the last directory's last \
             file is last"
        );
    }

    /// A symlinked directory would take the walk outside the session, so the
    /// walk does not follow one: `read_dir` reports it as a symlink, which is
    /// neither a directory to enter nor a file to read. A symlinked markdown
    /// *file* is one bounded file and is still read, exactly as the root
    /// listing read it before the walk existed.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_is_not_followed_out_of_the_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("private")).expect("mkdir");
        std::fs::write(outside.join("private/secret.md"), "not the session's").expect("write");
        std::fs::write(outside.join("shared.md"), "linked in on purpose").expect("write");
        let session = dir.path().join("session");
        std::fs::create_dir_all(&session).expect("mkdir");
        std::fs::write(session.join("AGENTS.md"), "how to read this folder\n").expect("write");
        std::os::unix::fs::symlink(outside.join("private"), session.join("elsewhere"))
            .expect("symlink");
        std::os::unix::fs::symlink(outside.join("shared.md"), session.join("shared.md"))
            .expect("symlink");

        let (files, _truncated, _shape) = read_ref_sources(&session, REF_SCAN_BUDGET);
        assert_eq!(
            files
                .iter()
                .map(|file| file.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["AGENTS.md", "shared.md"],
            "the linked directory is refused; the linked file is one file"
        );
    }

    /// The one exclusion list, said once, so a verb that offers to create a
    /// directory can ask the reader whether the reader will look in it.
    #[test]
    fn the_scan_names_the_directories_it_does_not_enter() {
        assert!(!scans_markdown(ARTIFACTS_DIR), "output, not the session");
        assert!(!scans_markdown(WORKSPACE_DIR), "scratch, AD-113");
        assert!(!scans_markdown(".obsidian"), "furniture");
        assert!(
            !scans_markdown("Artifacts") && !scans_markdown("Workspace"),
            "and folded: on the volume keeper ships on `Artifacts` IS \
             `artifacts`, so an exclusion that held for one spelling would put \
             scratch and promoted output in the pool"
        );
        assert!(
            scans_markdown("spaces") && scans_markdown("log"),
            "a directory the operator makes is a real home"
        );
        assert_eq!(
            UNSCANNED_DIRS.len(),
            2,
            "two names and a prefix rule — a third name would need its own reason"
        );
    }

    /// A directory keeper cannot list is not a directory with no markdown.
    /// Every file under it is missing from the pool, from every space evaluated
    /// over it and from the *Unfiled* notice, so the scan reports itself short
    /// rather than handing back a list that looks whole.
    #[cfg(unix)]
    #[test]
    fn a_directory_the_scan_cannot_list_is_reported_rather_than_silently_short() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::create_dir_all(session.join("spaces")).expect("mkdir");
        std::fs::write(session.join("about.md"), "---\ntags: [about]\n---\n# S\n").expect("write");
        std::fs::write(
            session.join("spaces/plan.md"),
            "---\ntags: [task]\n---\n# Plan\n",
        )
        .expect("write");
        let locked = session.join("spaces");
        let restore = |mode: u32| {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(mode))
                .expect("chmod");
        };
        restore(0o000);
        // uid 0 ignores the mode bits, so on a machine that runs its tests as
        // root there is no unreadable directory to observe. Put it back and say
        // nothing, rather than assert a property this machine cannot have.
        if std::fs::read_dir(&locked).is_ok() {
            restore(0o755);
            return;
        }

        let (files, truncated, shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        restore(0o755);

        assert_eq!(shape, Shape::Flat);
        assert_eq!(
            files
                .iter()
                .map(|file| file.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["about.md"],
            "the task under the unreadable directory is in no pool"
        );
        assert!(
            truncated,
            "and the caller is told, so a board that lost a card can say why"
        );
    }

    /// Two names in one directory that differ only in case are two entries, and
    /// their order is the walk's rather than `read_dir`'s — which decides which
    /// of the two survives the byte budget on a session near it.
    ///
    /// Under a sort key that folds both to one string the tie is left in hash
    /// order, and this assertion becomes a coin flip on ext4.
    #[test]
    fn two_names_that_differ_only_in_case_are_ordered_by_the_walk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::write(session.join("about.md"), "---\ntags: [about]\n---\n# S\n").expect("write");
        std::fs::write(session.join("Plan.md"), "---\ntags: [task]\n---\n# Upper\n")
            .expect("write");
        // The volume keeper ships on folds case and cannot hold both: there the
        // second write IS the first file, under the name the directory already
        // holds, and one entry is the right answer rather than a skipped test.
        let folds = session.join("plan.md").exists();
        std::fs::write(session.join("plan.md"), "---\ntags: [task]\n---\n# Lower\n")
            .expect("write");

        let (rels, truncated) = markdown_rels(session, true);
        assert!(!truncated);
        if folds {
            assert_eq!(
                rels,
                vec!["about.md".to_owned(), "Plan.md".to_owned()],
                "one file, one entry, under the spelling the drive holds"
            );
            return;
        }
        assert_eq!(
            rels,
            vec![
                "about.md".to_owned(),
                "Plan.md".to_owned(),
                "plan.md".to_owned(),
            ],
            "folded equal, so the raw name breaks the tie the same way on every \
             run and on every machine"
        );
    }

    /// The walk is bounded by entries as well as by bytes, and says when the
    /// bound is what stopped it. Ten megabytes of byte budget is no answer to a
    /// tree of short files: the budget is spent after the walk has already
    /// materialised every path, once per board row.
    #[test]
    fn a_tree_past_the_entry_budget_stops_and_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::write(session.join("about.md"), "---\ntags: [about]\n---\n# S\n").expect("write");
        // Spread wide enough that the stop lands mid-walk rather than at the
        // root, and one folder past the budget: dirents are what is counted, so
        // the directories count too.
        let per_folder = 200;
        let folders = MARKDOWN_WALK_BUDGET / per_folder + 1;
        for folder in 0..folders {
            let sub = session.join(format!("d{folder:03}"));
            std::fs::create_dir_all(&sub).expect("mkdir");
            for file in 0..per_folder {
                std::fs::write(sub.join(format!("f{file:03}.md")), "x").expect("write");
            }
        }

        let (rels, truncated) = markdown_rels(session, true);
        assert!(truncated, "a walk that stopped short says that it stopped");
        assert!(
            rels.len() < folders * per_folder,
            "and it stopped rather than reading the whole tree — {} of {}",
            rels.len(),
            folders * per_folder + 1
        );
        assert_eq!(rels[0], "about.md", "what it did read is in reading order");

        let (_files, reported, _shape) = read_ref_sources(session, REF_SCAN_BUDGET);
        assert!(
            reported,
            "and the pool carries the fact: bytes to spare, and the list is \
             still a prefix"
        );
    }

    /// Depth is not a limit the walk imposes on the operator's own tree: the
    /// worklist is on the heap and the only bound is the entry budget, so a file
    /// at the bottom of a chain he made is in the pool, in reading order.
    #[test]
    fn a_deeply_nested_file_is_reached_in_reading_order() {
        const DEPTH: usize = 150;

        let dir = tempfile::tempdir().expect("tempdir");
        let session = dir.path();
        std::fs::write(session.join("about.md"), "---\ntags: [about]\n---\n# S\n").expect("write");
        let mut deep = session.to_path_buf();
        for _ in 0..DEPTH {
            deep = deep.join("d");
        }
        std::fs::create_dir_all(&deep).expect("mkdir");
        std::fs::write(deep.join("bottom.md"), "---\ntags: [ref]\n---\n# Deep\n").expect("write");

        let (rels, truncated) = markdown_rels(session, true);
        assert!(
            !truncated,
            "a chain of {DEPTH} directories is well inside the entry budget"
        );
        assert_eq!(
            rels,
            vec![
                "about.md".to_owned(),
                format!("{}/bottom.md", ["d"; DEPTH].join("/")),
            ],
            "the root's own markdown first, then the file at the bottom"
        );
    }
}
