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
    classify, freshness, lineage, Freshness, SessionStatus, ACTIVE_DIR, ARCHIVE_DIR, README,
    WORKSPACE_DIR,
};
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
fn row_for(dir: &Path, rel: &str, status: SessionStatus) -> Option<SessionRowVm> {
    let folder_name = rel.rsplit('/').next().unwrap_or(rel);
    let readme = std::fs::read_to_string(dir.join(README)).unwrap_or_default();
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
    let (last_log_date, last_log_line) = last_log(body);

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
}
