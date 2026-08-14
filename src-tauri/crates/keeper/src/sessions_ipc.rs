//! The sessions driving adapter (Phase 7, AD-114): every `sessions_*`
//! command. No decisions live here — commands validate arguments against the
//! root registry and hand the work to `sessions_root` (effects) or
//! `keeper_core::sessions` (rules), exactly as `notes_ipc` does for notes.
//!
//! **Lists project; the event invalidates** (AD-114 at zone scale). A zone
//! holds tens of session folders, so the change surface is one payload-free
//! `keeper://sessions-changed` event and a re-read through `sessions_list` —
//! the `CAPTURE_WINDOWS_EVENT` pattern, chosen over the notes `NoteListOp`
//! diff channel because an index-diff protocol over a forty-row list buys
//! latency nobody can see and costs a second op-application code path.
//!
//! `#[cfg(not(desktop))]` twins live at the bottom so the `invoke_handler`
//! list is identical on every target and the iOS compile gate stays green.

#[cfg(desktop)]
use keeper_core::sessions::vm::{SessionRootVm, SessionRowVm};
use keeper_core::vm::{IpcError, IpcErrorCode};

/// The refusal every desktop-only twin returns on mobile — sessions ride the
/// sync capability, which is desktop-only, so these are unreachable from a UI
/// that gates on `CapabilitiesVm.sessions` (FR-223).
#[cfg(not(desktop))]
fn unsupported() -> IpcError {
    IpcError {
        code: IpcErrorCode::Unsupported,
        message: "sessions are a desktop surface".to_owned(),
        account_id: None,
        retriable: false,
    }
}

/// Every registered sessions root, for the board's switcher (FR-224).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_roots() -> Result<Vec<SessionRootVm>, IpcError> {
    Ok(crate::sessions_root::roots())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_roots() -> Result<Vec<()>, IpcError> {
    Err(unsupported())
}

/// The board rows for one root, newest record-change first, pinned first
/// within status (FR-228). An unindexed root answers an empty list with
/// `indexed: false` on its `SessionRootVm` — absent data is stated, never
/// invented.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_list(root_id: String) -> Result<Vec<SessionRowVm>, IpcError> {
    let rows = crate::sessions_root::rows(&root_id);
    match rows {
        Some(rows) => Ok(rows.as_ref().clone()),
        None => {
            // Not scanned yet, or an unknown root. Distinguish: an unknown id
            // is a caller bug and refuses; a known-but-cold root answers empty.
            if crate::sessions_root::known(&root_id) {
                Ok(Vec::new())
            } else {
                Err(IpcError {
                    code: IpcErrorCode::Internal,
                    message: format!("no such sessions root: {root_id}"),
                    account_id: None,
                    retriable: false,
                })
            }
        }
    }
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_list(root_id: String) -> Result<Vec<()>, IpcError> {
    let _ = root_id;
    Err(unsupported())
}

/// Rescan one root now — the sessions "Rebuild index" verb (FR-225). The
/// answer is acknowledgement of the request, not completion: the scan lands as
/// a `keeper://sessions-changed` event like every other change.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_rescan(root_id: String) -> Result<(), IpcError> {
    if crate::sessions_root::rescan(&root_id) {
        Ok(())
    } else {
        Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!("no such sessions root: {root_id}"),
            account_id: None,
            retriable: false,
        })
    }
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_rescan(root_id: String) -> Result<(), IpcError> {
    let _ = root_id;
    Err(unsupported())
}

/// One session's *record*: header facts, the user-tier properties widget and
/// the rendered log, newest first — the review order (FR-233). Composed from
/// one README parse; nothing stored.
///
/// The session's files are [`sessions_tree`], read separately (FR-254): the
/// tree costs a directory walk and one `Engine::pending` query, and a log
/// re-read should not pay for either.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_detail(
    root_id: String,
    session_id: String,
) -> Result<keeper_core::sessions::vm::SessionDetailVm, IpcError> {
    crate::sessions_root::detail(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_detail(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Why a folder has no Delete (FR-262).
///
/// Not a rule [`keeper_core::sessions::files`] states, because it is not a rule
/// about *paths*: that module's verbs take one file, and a folder delete is a
/// different, recursive thing whose blast radius is whatever happens to be
/// inside. Saying so on the row is what keeps this from reading as an oversight
/// — the operator has Finder for the day they mean it.
#[cfg(desktop)]
const SESSION_TREE_DIR_UNDELETABLE: &str =
    "keeper deletes one file at a time. Removing a folder takes everything inside it with it, \
     which is a bigger promise than this tree makes — do it in Finder.";

/// One session's own file tree (FR-254, AD-117) — the session as the small
/// workspace it is.
///
/// **Why the whole subtree in one call, and not a folder per expand.** The
/// Files tab browses lazily because a synced folder is unbounded and each
/// level costs one `Engine::pending` query. A session is bounded by its own
/// contract — four shallow sections — and its five sections open together, so
/// lazy browsing would trade one git query for five. AD-114 already decided
/// this for the board; the tree is the same decision one level down.
///
/// **The sync mark is the Files tab's, not a second opinion.** `pending` is
/// asked once for the whole tree and every entry is classified through
/// [`keeper_sync::browse::status_of`] — the same function the listing and the
/// delete confirmation go through — then worded by the same five sentences.
/// A session file that the Files pane calls excluded is called excluded here,
/// in those words, because it is one fact asked from two places.
///
/// **The lock is the write fence, asked** (AD-113). `workspace/` entries carry
/// [`keeper_sync::files_write::WriteRefusal::SessionWorkspace`]'s own sentence
/// rather than a UI convention that could drift from what a write would
/// actually do. `undeletable` is the same trick for the Delete verb: the row
/// carries [`keeper_core::sessions::files::check_deletable`]'s refusal, so the
/// button the tree draws and the command it would call cannot disagree.
///
/// Rejects with: `internal` (unknown root or session, an unreadable profile
/// exclude pattern), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_tree(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
) -> Result<keeper_core::sessions::vm::SessionTreeVm, IpcError> {
    use keeper_core::sessions::files;
    use keeper_core::sessions::vm::{SessionEntryVm, SessionTreeVm};
    use keeper_core::vm::FileSizeVm;
    use keeper_sync::browse;
    use keeper_sync::exclude::ExcludeSet;

    let (session_path, raw, truncated) =
        tokio::task::block_in_place(|| crate::sessions_root::tree(&root_id, &session_id))
            .ok_or_else(|| IpcError {
                code: IpcErrorCode::Internal,
                message: format!("no such session: {session_id}"),
                account_id: None,
                retriable: false,
            })?;

    // A sessions root IS a sync profile (AD-107), so the engine, the excludes
    // and the write scope all come off the profile the root id already names.
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let excludes = ExcludeSet::new(&profile.excludes).map_err(|error| IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    })?;
    let (_vault, scope) = crate::sync_ipc::sessions_scope(&profile);

    // Once for the whole tree, exactly as a listing asks once for a whole
    // directory. An engine that cannot answer does not fail the tree: the
    // files still come back, marked unknown with the engine's own words.
    let (pending, unavailable) = match crate::sync_ipc::sessions_pending(&state, &root_id).await {
        Ok(files) => (browse::PendingView::from_pending(files), None),
        Err(error) => (browse::PendingView::Unavailable, Some(error)),
    };

    let entries = raw
        .into_iter()
        .map(|entry| {
            // Composed here and only here (AD-65): the frontend receives a
            // path it can hand straight to a file target and never joins one.
            let subpath = format!("{zone}/{session_path}/{}", entry.rel_path);
            // The Delete button's own answer, from the predicate the command
            // runs (FR-262) rather than from a rule re-stated here. A directory
            // is refused up front: `check_deletable` takes a file path and
            // would happily accept `notes` as one, and removing a folder is a
            // recursive verb this module does not offer.
            //
            // Asked before the struct literal because `rel_path` moves into it,
            // and a `clone()` to keep asking later would be a copy taken for
            // the sake of statement order.
            let undeletable = if entry.is_dir {
                Some(SESSION_TREE_DIR_UNDELETABLE.to_owned())
            } else {
                files::check_deletable(&entry.rel_path)
                    .err()
                    .map(|refusal| refusal.to_string())
            };
            let status = browse::status_of(
                &profile.local_path,
                &subpath,
                entry.is_dir,
                &excludes,
                &pending,
            );
            SessionEntryVm {
                name: entry.name,
                rel_path: entry.rel_path,
                parent: entry.parent,
                depth: entry.depth,
                is_dir: entry.is_dir,
                sync: crate::sync_ipc::sessions_sync_mark(&status, unavailable.as_deref()),
                locked: scope
                    .session_workspace_lock(&subpath, entry.is_dir)
                    .then(|| {
                        keeper_sync::files_write::WriteRefusal::SessionWorkspace {
                            subpath: subpath.clone(),
                        }
                        .to_string()
                    }),
                absolute_path: profile
                    .local_path
                    .join(&subpath)
                    .to_string_lossy()
                    .into_owned(),
                undeletable,
                subpath,
                size: (!entry.is_dir).then(|| FileSizeVm::new(entry.size)),
                mtime_ms: entry.mtime_ms,
            }
        })
        .collect();

    Ok(SessionTreeVm { entries, truncated })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_tree(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// What one session points at (FR-255, AD-118) — the other half of "a session
/// folder is a small workspace".
///
/// [`sessions_tree`] lists what a session *holds*. This lists what it *names*,
/// which is a different set on purpose: the zone's own contract says big files
/// live in their zone and a session references them by repo-root-relative path,
/// so the thing that breaks is the pointer, and until now nothing said so.
///
/// **Six kinds, six existing resolvers, asked rather than restated.** A note is
/// what [`keeper_core::notes::index::IndexSnapshot::resolve_link`] answers —
/// the same function backlinks are built from, so a link cannot open one note
/// here and appear under another there. A recording is a note whose frontmatter
/// carries `session:` ([`keeper_core::notes::recording_note::is_recording_note`],
/// as the vault's own `recording` flag is computed) and **never** a media file
/// extension: a loose `.m4a` in a session is a file. A file exists on disk. A
/// session is a file that turned out to be another session's folder, asked of
/// the board's own rows. External is `http(s)`, opened by the system browser
/// and never probed — keeper does not make network requests to colour a row.
/// Missing is what is left, and the word is the export receipt's.
///
/// **The vault is asked only when the target could name a note.** A sessions
/// root and a notes vault are both the profile (AD-90, AD-107), and the zone
/// and the vault can never overlap ([`keeper_sync::SessionsConfig`]'s own
/// validation), so a `60-sessions/…` path is structurally not a note. Asking
/// anyway would let a stem match answer for an unrelated file.
///
/// Rejects with: `internal` (unknown root or session), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_refs(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
) -> Result<keeper_core::sessions::vm::SessionReferencesVm, IpcError> {
    use keeper_core::panels::PanelTargetVm;
    use keeper_core::sessions::refs::{self, NoteHit, RefKind, RefProbe, RefTarget, SessionHit};
    use keeper_core::sessions::vm::{SessionReferenceVm, SessionReferencesVm};

    let sources =
        crate::sessions_root::ref_sources(&root_id, &session_id).ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("no such session: {session_id}"),
            account_id: None,
            retriable: false,
        })?;

    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let prefix = format!("{zone}/{}", sources.path);

    /// The three questions, answered from the registries that already hold
    /// them. The pure half took a trait rather than a filesystem, so this is
    /// the only place any of them touches disk.
    struct Probe<'a> {
        root_id: &'a str,
        zone: &'a str,
        local_path: &'a std::path::Path,
        snapshot: Option<std::sync::Arc<keeper_core::notes::index::IndexSnapshot>>,
    }

    impl RefProbe for Probe<'_> {
        fn note(&self, target: &str) -> Option<NoteHit> {
            let snapshot = self.snapshot.as_ref()?;
            let entry = snapshot.resolve_link(target)?;
            Some(NoteHit {
                note_id: entry.id.clone(),
                title: entry.title.clone(),
                // The vault's own `recording` flag, as `notes_vault` computes
                // it from `is_recording_note` — not a second predicate here.
                recording: entry.flags.iter().any(|flag| flag == "recording"),
            })
        }

        fn exists(&self, subpath: &str) -> bool {
            // A profile-relative path, joined against the profile root and
            // nowhere else: `..` in a link would otherwise probe outside the
            // synced folder, and a reference widget is not a filesystem prober.
            if subpath.split('/').any(|part| part == "..") {
                return false;
            }
            self.local_path.join(subpath).exists()
        }

        fn session(&self, subpath: &str) -> Option<SessionHit> {
            let inside = subpath.strip_prefix(&format!("{}/", self.zone))?;
            crate::sessions_root::session_at(self.root_id, inside).map(|title| SessionHit { title })
        }
    }

    let probe = Probe {
        root_id: &root_id,
        zone: &zone,
        local_path: &profile.local_path,
        // A profile that is not also a vault answers no notes, which is the
        // honest answer rather than an error: a sessions zone in a folder with
        // no vault flag has files and no note index (AD-90).
        snapshot: crate::notes_vault::snapshot(&profile.id),
    };

    let found: Vec<(String, refs::RawRef)> = sources
        .files
        .iter()
        .flat_map(|source| {
            refs::scan(&source.text)
                .into_iter()
                .map(move |raw| (source.rel.clone(), raw))
        })
        .collect();

    let rows: Vec<SessionReferenceVm> = refs::plan(&found, &prefix, &probe)
        .into_iter()
        .map(|row| {
            let (panel_target, url, notice) = match &row.open {
                RefTarget::Note { note_id } => (
                    Some(PanelTargetVm::Note {
                        // A vault id IS the profile id (AD-90) — composed here,
                        // where that identity is known, rather than in the pure
                        // module, which should not be asserting it.
                        vault_id: profile.id.clone(),
                        note_id: note_id.clone(),
                    }),
                    None,
                    None,
                ),
                RefTarget::File { subpath } => (
                    Some(PanelTargetVm::File {
                        profile_id: profile.id.clone(),
                        relative_path: subpath.clone(),
                    }),
                    None,
                    None,
                ),
                RefTarget::External { url } => (None, Some(url.clone()), None),
                RefTarget::Missing { looked } => {
                    (None, None, Some(refs::missing_notice(&row.target, looked)))
                }
            };
            SessionReferenceVm {
                kind: row.kind.as_str().to_owned(),
                target: row.target,
                label: row.label,
                source: row.source,
                panel_target,
                url,
                notice,
            }
        })
        .collect();

    let missing = rows
        .iter()
        .filter(|row| row.kind == RefKind::Missing.as_str())
        .count() as u32;

    Ok(SessionReferencesVm {
        refs: rows,
        missing,
        truncated: sources.truncated,
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_refs(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

// ---------------------------------------------------------------------------
// Lifecycle verbs (FR-238..FR-248, AD-111, AD-112)
// ---------------------------------------------------------------------------

/// Today, as the zone's folder-name prefix wants it.
#[cfg(desktop)]
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(desktop)]
fn root_error(root_id: &str) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such sessions root: {root_id}"),
        account_id: None,
        retriable: false,
    }
}

/// The refusal for an id the registry does not hold — [`root_error`]'s twin.
///
/// The same eight lines are still written inline in the older commands above;
/// this is not a sweep of them, only the shape new ones should use.
#[cfg(desktop)]
fn session_error(session_id: &str) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    }
}

#[cfg(desktop)]
fn exec_error(error: crate::sessions_exec::ExecError) -> IpcError {
    use crate::sessions_exec::ExecError;
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        // A refusal re-plans and may succeed; a disk failure is the OS's word.
        retriable: matches!(error, ExecError::Refused(_)),
    }
}

/// The `(dir-relative path, is_dir)` facts a pattern copy needs — one walk,
/// used for the zone's `_template/` and for a source session alike, because
/// [`keeper_core::sessions::pattern::apply`] is what tells them apart.
#[cfg(desktop)]
fn pattern_files(dir: &std::path::Path) -> Vec<(String, bool)> {
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, bool)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".gitkeep" {
                continue;
            }
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                out.push((rel.clone(), true));
                walk(&entry.path(), &rel, out);
            } else {
                out.push((rel, false));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out
}

/// The pattern id the zone's own `_template/` answers to.
#[cfg(desktop)]
const TEMPLATE_PATTERN_ID: &str = "_template";

/// Newest mtime under a directory, ms since epoch — what orders the picker.
#[cfg(desktop)]
fn newest_mtime_ms(dir: &std::path::Path, files: &[(String, bool)]) -> Option<i64> {
    files
        .iter()
        .filter(|(_, is_dir)| !*is_dir)
        .filter_map(|(rel, _)| {
            std::fs::metadata(dir.join(rel))
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| {
                    time.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|since| since.as_millis() as i64)
                })
        })
        .max()
}

/// Project one pattern for the picker: the decision, applied and rendered.
#[cfg(desktop)]
fn pattern_vm(
    id: &str,
    kind: keeper_core::sessions::pattern::PatternKind,
    label: &str,
    detail: &str,
    dir: &std::path::Path,
) -> keeper_core::sessions::vm::SessionPatternVm {
    use keeper_core::sessions::pattern;
    use keeper_core::sessions::vm::{SessionPatternFileVm, SessionPatternSkipVm, SessionPatternVm};

    let files = pattern_files(dir);
    let mtime_ms = newest_mtime_ms(dir, &files);
    let outcome = pattern::apply(kind, &files);
    SessionPatternVm {
        id: id.to_owned(),
        kind: kind.as_str().to_owned(),
        label: label.to_owned(),
        detail: detail.to_owned(),
        mtime_ms,
        // Placeholders travel but are never shown: an empty `refs/` that
        // advertised a file would be true about bytes and false about meaning.
        copies: outcome
            .copies
            .iter()
            .filter(|(rel, _)| !pattern::is_placeholder(rel))
            .map(|(rel, is_dir)| SessionPatternFileVm {
                rel_path: rel.clone(),
                is_dir: *is_dir,
            })
            .collect(),
        skips: outcome
            .skips
            .iter()
            .filter(|(rel, _)| !pattern::is_placeholder(rel))
            .map(|(rel, reason)| SessionPatternSkipVm {
                rel_path: rel.clone(),
                reason: reason.sentence().to_owned(),
            })
            .collect(),
    }
}

/// Everything a new session can be shaped from (FR-253): the zone's
/// `_template/` first, then the sessions themselves, newest first.
///
/// The board used to offer these as two unrelated verbs — *New session* on
/// the header and *New like this* on a row's menu — which made "start from
/// what I did last time" a thing you had to already know about. One list,
/// one question, and the preview each entry carries is the plan's own
/// decision rather than a second description of it (AD-116).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_patterns(
    root_id: String,
) -> Result<Vec<keeper_core::sessions::vm::SessionPatternVm>, IpcError> {
    use keeper_core::sessions::pattern::PatternKind;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let mut out = Vec::new();
    let template_dir = zone.join(keeper_core::sessions::model::TEMPLATE_DIR);
    if template_dir.is_dir() {
        out.push(pattern_vm(
            TEMPLATE_PATTERN_ID,
            PatternKind::Template,
            "Zone template",
            "the zone's own skeleton — copied whole",
            &template_dir,
        ));
    }
    // Sessions as patterns, newest record change first — the same order the
    // board sorts by, so the picker agrees with the list behind it.
    for row in crate::sessions_root::rows(&root_id)
        .map(|rows| rows.as_ref().clone())
        .unwrap_or_default()
    {
        let detail = if row.status == "active" {
            "continues this session".to_owned()
        } else {
            match row.archived_year {
                Some(year) => format!("continues this session — archived {year}"),
                None => "continues this session — archived".to_owned(),
            }
        };
        out.push(pattern_vm(
            &row.id,
            PatternKind::Session,
            &row.title,
            &detail,
            &zone.join(&row.path),
        ));
    }
    Ok(out)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_patterns(root_id: String) -> Result<Vec<()>, IpcError> {
    let _ = root_id;
    Err(unsupported())
}

/// The folder names already taken in `active/`, for the collision counter.
#[cfg(desktop)]
fn taken_names(zone: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(zone.join("active"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Create a session from a pattern (FR-238, FR-239, FR-253, AD-112, AD-116).
///
/// Two questions in — the title, and what to shape it from — and one folder
/// out. `pattern_id` is `None` or `"_template"` for the zone's skeleton, and
/// a session's ULID to continue that session: structure only, with
/// `continues`/`continued-by` written into BOTH READMEs, an archived source
/// included, because files are truth and a lineage only the index knew would
/// be invisible to `cat`, to Obsidian and to the agent.
///
/// One command rather than the pair it replaces: which pattern a session
/// starts from is an *argument*, not a different verb, and keeping it as two
/// commands is what let the template path and the continuation path drift
/// apart in the first place.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_create(
    root_id: String,
    title: String,
    pattern_id: Option<String>,
) -> Result<keeper_core::sessions::vm::SessionRefVm, IpcError> {
    use keeper_core::sessions::pattern::{self, PatternKind};
    use keeper_core::sessions::{model, plan};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let title = title.trim().to_owned();
    let date = today();
    let dir_name = model::session_dir_name(&title, &date, &taken_names(&zone));
    let id = crate::sync_ipc::new_ulid();

    // Which pattern, resolved to the one thing the plan needs: a zone-relative
    // directory to copy out of, and the kind that decides what travels.
    let source_id = pattern_id.filter(|value| value != TEMPLATE_PATTERN_ID);
    let (kind, pattern_root, source) = match &source_id {
        None => (PatternKind::Template, model::TEMPLATE_DIR.to_owned(), None),
        Some(source_id) => {
            let row =
                crate::sessions_root::row_of(&root_id, source_id).ok_or_else(|| IpcError {
                    code: IpcErrorCode::Internal,
                    message: format!("no such session: {source_id}"),
                    account_id: None,
                    retriable: false,
                })?;
            (PatternKind::Session, row.path.clone(), Some(row))
        }
    };
    let pattern_dir = zone.join(&pattern_root);

    // The stamped README: the pattern's own headings, empty, with the title
    // and date in place. A template README that grows a section grows it for
    // every new session; a continued session inherits the shape it earned.
    let pattern_readme = std::fs::read_to_string(pattern_dir.join(model::README))
        .unwrap_or_else(|_| "# <session title>\n\n## Summary\n\n## Log\n\n## Promote\n\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n".to_owned());
    let (_, body_at) = keeper_core::notes::frontmatter::Frontmatter::parse(&pattern_readme);
    let body = plan::skeleton_from(&pattern_readme[body_at..], &title, &date);
    let readme = match &source {
        // continues: baked into the new README's frontmatter at birth (AD-112).
        Some(row) => format!(
            "---\nid: {id}\ncreated: {date}\nkeeper:\n  session-continues: [{}]\n---\n{body}",
            row.id
        ),
        None => format!("---\nid: {id}\ncreated: {date}\n---\n{body}"),
    };

    let copies = pattern::apply(kind, &pattern_files(&pattern_dir)).copies;
    let mut compiled = match &source {
        None => plan::compile_create(&dir_name, &pattern_root, &copies, &readme),
        Some(row) => {
            let source_readme =
                std::fs::read_to_string(pattern_dir.join(model::README)).unwrap_or_default();
            plan::compile_create_from(&dir_name, &row.path, &source_readme, &id, &copies, &readme)
        }
    };
    compiled.verb = if source.is_some() {
        "create-from".to_owned()
    } else {
        "create".to_owned()
    };
    let session_path = compiled.session.clone();

    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("create task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(keeper_core::sessions::vm::SessionRefVm {
        root_id,
        id,
        path: session_path,
        title,
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_create(
    root_id: String,
    title: String,
    pattern_id: Option<String>,
) -> Result<(), IpcError> {
    let _ = (root_id, title, pattern_id);
    Err(unsupported())
}

/// Append (or find) today's log entry in a session's README (FR-240). The
/// answer carries the README's profile-relative path so the caller opens it
/// in the one editor; a same-day second call is a no-op that still answers.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_log_today(
    root_id: String,
    session_id: String,
) -> Result<keeper_core::sessions::vm::SessionRefVm, IpcError> {
    use keeper_core::sessions::plan;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })?;
    let readme_path = zone.join(&row.path).join("README.md");
    let readme = std::fs::read_to_string(&readme_path).unwrap_or_default();
    if let Some((compiled, _caret)) = plan::compile_log_today(&row.path, &readme, &today()) {
        let zone_for_run = zone.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::sessions_exec::run(&zone_for_run, compiled)
        })
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("log-today task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
        crate::sessions_root::rescan(&root_id);
    }
    Ok(keeper_core::sessions::vm::SessionRefVm {
        root_id,
        id: session_id,
        path: row.path,
        title: row.title,
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_log_today(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Read one session for migration: what shape it is, and every file the
/// compiler needs to plan the conversion.
///
/// Shared by the preview and the run so the two can never disagree about what
/// they are looking at. It is bounded the way every other session read is: the
/// carried files are `refs/` and `prompts/` markdown, which the zone's own
/// contract keeps small, and nothing descends into `artifacts/` or `workspace/`.
#[cfg(desktop)]
fn migrate_input(
    zone: &std::path::Path,
    session_rel: &str,
) -> keeper_core::sessions::migrate::MigrateInput {
    use keeper_core::sessions::migrate::{MigrateFile, MigrateInput};

    let dir = zone.join(session_rel);
    let top_level: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();

    let mut carried = Vec::new();
    for kind in ["refs", "prompts"] {
        let Ok(entries) = std::fs::read_dir(dir.join(kind)) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.to_ascii_lowercase().ends_with(".md"))
            .collect();
        // Sorted so a preview and the run that follows it list the same files in
        // the same order — `read_dir` order is the filesystem's business.
        names.sort();
        for name in names {
            let Ok(text) = std::fs::read_to_string(dir.join(kind).join(&name)) else {
                continue;
            };
            carried.push(MigrateFile {
                rel: format!("{kind}/{name}"),
                text,
            });
        }
    }

    let mut input = MigrateInput {
        session: session_rel.to_owned(),
        top_level,
        readme: std::fs::read_to_string(dir.join("README.md")).unwrap_or_default(),
        carried,
        ids: Vec::new(),
        today: today(),
    };
    input.ids = (0..keeper_core::sessions::migrate::id_count(&input))
        .map(|_| crate::sync_ipc::new_ulid())
        .collect();
    input
}

/// What migrating this session would do — every path, before any of them
/// happens (FR-257).
///
/// Pure in the only sense that matters here: it reads the session and writes
/// nothing. The ids it mints are thrown away with the preview, and the run
/// mints its own — which is correct, because the operator may preview twice and
/// run once, and the ids that end up in the files must be the ones the journal
/// recorded.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_migrate_preview(
    root_id: String,
    session_id: String,
) -> Result<keeper_core::sessions::vm::SessionMigrationVm, IpcError> {
    use keeper_core::sessions::plan::PlanStep;
    use keeper_core::sessions::vm::SessionMigrationVm;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;

    let input = migrate_input(&zone, &row.path);
    let Some(plan) = keeper_core::sessions::migrate::compile_migrate(&input) else {
        return Ok(SessionMigrationVm {
            needed: false,
            creates: Vec::new(),
            rewrites: Vec::new(),
            trashes: Vec::new(),
        });
    };

    // Session-relative, because that is what the operator is looking at: the
    // zone prefix is the same on every row and would only be noise.
    let strip = |path: &str| {
        path.strip_prefix(&format!("{}/", row.path))
            .unwrap_or(path)
            .to_owned()
    };
    let mut vm = SessionMigrationVm {
        needed: true,
        creates: Vec::new(),
        rewrites: Vec::new(),
        trashes: Vec::new(),
    };
    for step in &plan.steps {
        match step {
            PlanStep::WriteFile { path, .. } => vm.creates.push(strip(path)),
            PlanStep::GuardedWrite { path, .. } => vm.rewrites.push(strip(path)),
            PlanStep::TrashDir { path, .. } => vm.trashes.push(strip(path)),
            _ => {}
        }
    }
    Ok(vm)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_migrate_preview(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Convert one folder-shaped session to the flat contract (FR-257).
///
/// Journaled and idempotent like every other lifecycle verb: a crash mid-run
/// resumes from the journal, and a completed migration compiles to no plan at
/// all, so pressing the button twice is not a second migration. **Never
/// automatic** — a scan that migrated what it read would turn opening the board
/// into a commit against the operator's drive.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_migrate(root_id: String, session_id: String) -> Result<(), IpcError> {
    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;

    let input = migrate_input(&zone, &row.path);
    let Some(compiled) = keeper_core::sessions::migrate::compile_migrate(&input) else {
        // Already flat. Not an error: the operator asked for an outcome that
        // already holds.
        return Ok(());
    };

    let zone_for_run = zone.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::sessions_exec::run(&zone_for_run, compiled)
    })
    .await
    .map_err(|join| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("migrate task failed: {join}"),
        account_id: None,
        retriable: false,
    })?
    .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_migrate(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Pin or unpin a session (FR-232): one frontmatter boolean through the one
/// byte-preserving writer.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_set_pinned(
    root_id: String,
    session_id: String,
    pinned: bool,
) -> Result<(), IpcError> {
    use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })?;
    let readme_path = zone.join(&row.path).join("README.md");
    let readme = std::fs::read_to_string(&readme_path).map_err(|error| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("could not read the session README: {error}"),
        account_id: None,
        retriable: false,
    })?;
    let updated = if pinned {
        Frontmatter::set_in(&readme, "pinned", FieldValue::Bool(true))
    } else {
        Frontmatter::remove_in(&readme, "pinned")
    };
    if updated != readme {
        std::fs::write(&readme_path, updated).map_err(|error| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("could not write the session README: {error}"),
            account_id: None,
            retriable: false,
        })?;
        crate::sessions_root::rescan(&root_id);
    }
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_set_pinned(
    root_id: String,
    session_id: String,
    pinned: bool,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, pinned);
    Err(unsupported())
}

/// Archive a session (FR-245, AD-111): the compiled checklist decision —
/// promotes to run, whether to empty the workspace — executed with the move
/// last, journaled, resumable.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_archive(
    root_id: String,
    session_id: String,
    promotes: Vec<(String, String)>,
    empty_workspace: bool,
) -> Result<(), IpcError> {
    use keeper_core::sessions::plan;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })?;
    if row.status != "active" {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "only an active session can be archived".to_owned(),
            account_id: None,
            retriable: false,
        });
    }
    let year = today()[..4].parse::<i32>().unwrap_or(1970);
    let compiled = plan::compile_archive(
        &row.path,
        &plan::ArchiveDecision {
            promotes,
            empty_workspace,
            year,
        },
    );
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("archive task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_archive(
    root_id: String,
    session_id: String,
    promotes: Vec<(String, String)>,
    empty_workspace: bool,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, promotes, empty_workspace);
    Err(unsupported())
}

/// Delete a session into the zone's trash (FR-246, FR-247): recoverable,
/// never an unlink, workspace included (the trash is its only afterlife).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_delete(root_id: String, session_id: String) -> Result<(), IpcError> {
    use keeper_core::sessions::plan;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })?;
    let compiled = plan::compile_delete(&row.path, &session_id);
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("delete task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_delete(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Move an archived session back to `active/` (FR-248). Lineage is never
/// rewritten (AD-112); the UI offers continuation first and says why.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_unarchive(root_id: String, session_id: String) -> Result<(), IpcError> {
    use keeper_core::sessions::plan;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {session_id}"),
        account_id: None,
        retriable: false,
    })?;
    if row.status != "archived" {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "only an archived session can be unarchived".to_owned(),
            account_id: None,
            retriable: false,
        });
    }
    let compiled = plan::compile_unarchive(&row.path);
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("unarchive task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_unarchive(root_id: String, session_id: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// One zone's space definitions, in rail order (FR-261).
///
/// Seeds on first sight: a zone with **no** `_spaces/` gets the five defaults
/// written before the list answers, so the operator's first look at a session is
/// the grouped one rather than an empty rail with a button on it. A zone that
/// has the directory is theirs — an empty one stays empty, which is what makes a
/// deleted space stay deleted without a ledger file (AD-121).
///
/// The seed is a write inside a read, which is worth flagging: it happens once
/// per zone, ever, and the alternative is a first-run state where every session
/// looks broken until someone finds the restore button.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_spaces(
    root_id: String,
) -> Result<Vec<keeper_core::sessions::vm::SessionSpaceVm>, IpcError> {
    use keeper_core::sessions::spaces;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let mut read =
        crate::sessions_root::zone_spaces(&root_id).ok_or_else(|| root_error(&root_id))?;
    if !read.seeded {
        let defaults = spaces::plan(spaces::SeedMode::FirstRun, None);
        let ids: Vec<String> = defaults
            .iter()
            .map(|_| crate::sync_ipc::new_ulid())
            .collect();
        let compiled = spaces::compile_seed(&defaults, &ids, &today());
        let zone_for_run = zone.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::sessions_exec::run(&zone_for_run, compiled)
        })
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("seed task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
        read = crate::sessions_root::zone_spaces(&root_id).ok_or_else(|| root_error(&root_id))?;
    }
    Ok(read.spaces.iter().map(space_vm).collect())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_spaces(root_id: String) -> Result<Vec<()>, IpcError> {
    let _ = root_id;
    Err(unsupported())
}

/// Project one definition for the rail and the editor.
///
/// `notes_ipc::notes_spaces`' projection, key for key — the query parsed only to
/// find out whether it parses, and the sort resolved once here so the form never
/// has to work out what `bananas` falls back to (that rule lives in Rust and is
/// tested there).
#[cfg(desktop)]
fn space_vm(
    space: &keeper_core::sessions::spaces::SessionSpace,
) -> keeper_core::sessions::vm::SessionSpaceVm {
    use keeper_core::notes::{query, sort};

    keeper_core::sessions::vm::SessionSpaceVm {
        id: space.rel.clone(),
        name: space.name.clone(),
        query: space.query.clone(),
        sort: space.sort.clone(),
        sort_effective: sort::read(&space.sort).sort.canonical(),
        icon: space.icon.clone(),
        default_key: space.default_key.clone(),
        order: space.order,
        warnings: space.warnings.clone(),
        error: query::parse(&space.query).err().map(|error| error.message),
    }
}

/// What every space in the zone selected out of one session (FR-261).
///
/// One payload rather than one call per space: the session's pool is read once
/// and evaluated N times, which is the same trade AD-114 already made for the
/// board. N round trips would each re-read the same files off the drive to
/// answer a different query about them.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_space_files(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
) -> Result<Vec<keeper_core::sessions::vm::SessionSpaceFilesVm>, IpcError> {
    use keeper_core::sessions::pool::{read_one as read_pool_one, PoolFile};
    use keeper_core::sessions::spaces::{select, Candidate};
    use keeper_core::sessions::vm::{SessionSpaceFileVm, SessionSpaceFilesVm};

    let read = crate::sessions_root::zone_spaces(&root_id).ok_or_else(|| root_error(&root_id))?;
    let pool = crate::sessions_root::session_pool(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;

    // The zone subfolder, so every row carries a path the frontend can open
    // without composing one (AD-65) — `sessions_tree`'s rule, and its spelling.
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone_prefix = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let prefix = format!("{zone_prefix}/{}", pool.path);

    let entries: Vec<_> = pool
        .files
        .iter()
        .map(|(rel, text, _)| read_pool_one(PoolFile { rel, text }))
        .collect();
    let candidates: Vec<Candidate<'_>> = entries
        .iter()
        .zip(&pool.files)
        .map(|(entry, (_, text, mtime_ns))| Candidate {
            entry,
            mtime_ns: *mtime_ns,
            text,
        })
        .collect();
    let now = chrono::Local::now().timestamp_millis();

    Ok(read
        .spaces
        .iter()
        .map(|space| {
            let selection = select(space, &candidates, now);
            SessionSpaceFilesVm {
                space_id: space.rel.clone(),
                files: selection
                    .picked
                    .iter()
                    .filter_map(|index| candidates.get(*index))
                    .map(|candidate| SessionSpaceFileVm {
                        id: candidate.entry.id.clone(),
                        rel_path: candidate.entry.rel.clone(),
                        subpath: format!("{prefix}/{}", candidate.entry.rel),
                        title: candidate.entry.title.clone(),
                        tags: candidate.entry.tags.clone(),
                        // Milliseconds on the wire, nanoseconds in the domain:
                        // the sort needs the precision, a rendered "2 hours ago"
                        // does not, and an i64 of milliseconds is what every
                        // other VM here already carries.
                        mtime_ms: i64::try_from(candidate.mtime_ns / 1_000_000).unwrap_or(0),
                        unstable_identity: candidate.entry.unstable_identity,
                    })
                    .collect(),
                error: selection.error,
            }
        })
        .collect())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_space_files(root_id: String, session_id: String) -> Result<Vec<()>, IpcError> {
    // The desktop twin takes `state` too; a mobile twin that refuses does not
    // need it, and asking for it would make the mobile build depend on
    // `AppState` for a function that never reads it.
    let _ = (root_id, session_id);
    Err(unsupported())
}

/// Create or rewrite one space (FR-261).
///
/// A broken query is refused **at the edge**, exactly as `notes_space_save`
/// refuses one and for the same reason: a stored space that selects nothing
/// silently is worse than a save that says no. The editor's own refusal to save
/// an empty chip set sits on top of this; this is the backstop under it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_space_save(
    root_id: String,
    space: keeper_core::sessions::vm::SessionSpaceReq,
) -> Result<String, IpcError> {
    use keeper_core::notes::query;
    use keeper_core::sessions::spaces;

    query::parse(&space.query).map_err(|error| IpcError {
        code: IpcErrorCode::Internal,
        message: error.message,
        account_id: None,
        retriable: false,
    })?;
    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let read = crate::sessions_root::zone_spaces(&root_id).ok_or_else(|| root_error(&root_id))?;

    let edit = spaces::SpaceEdit {
        name: space.name.trim().to_owned(),
        query: space.query.clone(),
        sort: space.sort.clone(),
        icon: space.icon.clone(),
        order: space.order,
    };
    // An id names a file that must already be there. A save against one that is
    // gone — deleted in another window, or on the far side of a sync — is
    // refused rather than quietly recreated: recreating it would resurrect a
    // space the operator threw away, in the one directory whose contents *are*
    // the ledger.
    let (rel, source) = match space.id {
        Some(id) => {
            if !spaces::is_space_path(&id) {
                return Err(IpcError {
                    code: IpcErrorCode::Internal,
                    message: format!("not a space path: {id}"),
                    account_id: None,
                    retriable: false,
                });
            }
            let Some(source) = read.sources.get(&id).cloned() else {
                return Err(IpcError {
                    code: IpcErrorCode::Internal,
                    message: "that space is no longer there; nothing was written".to_owned(),
                    account_id: None,
                    retriable: false,
                });
            };
            (id, Some(source))
        }
        None => {
            let taken = read.sources.keys().cloned().collect();
            (spaces::rel_for_new(&edit.name, &taken), None)
        }
    };

    let compiled = spaces::compile_save(
        &rel,
        source.as_deref(),
        &edit,
        &crate::sync_ipc::new_ulid(),
        &today(),
    );
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("space-save task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(rel)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_space_save(root_id: String, space: ()) -> Result<String, IpcError> {
    let _ = (root_id, space);
    Err(unsupported())
}

/// Remove one space, recoverably (FR-261).
///
/// The path is checked against [`keeper_core::sessions::spaces::is_space_path`]
/// before anything is compiled. The executor's own check only proves a path
/// cannot escape the zone, which would still happily accept a session's
/// `about.md`.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_space_delete(root_id: String, space_id: String) -> Result<(), IpcError> {
    use keeper_core::sessions::spaces;

    if !spaces::is_space_path(&space_id) {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!("not a space path: {space_id}"),
            account_id: None,
            retriable: false,
        });
    }
    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let compiled = spaces::compile_delete(&space_id, &crate::sync_ipc::new_ulid());
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("space-delete task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_space_delete(root_id: String, space_id: String) -> Result<(), IpcError> {
    let _ = (root_id, space_id);
    Err(unsupported())
}

/// Put back whichever defaults are missing (FR-261).
///
/// Fills holes; never overwrites. A default already present **by key or by
/// folded name** is left exactly as the operator left it, so pressing this after
/// renaming Tasks to "Backlog" does not hand them a second Tasks.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_spaces_restore(
    root_id: String,
) -> Result<keeper_core::sessions::vm::SessionSpacesRestoredVm, IpcError> {
    use keeper_core::sessions::spaces;
    use keeper_core::sessions::vm::SessionSpacesRestoredVm;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let read = crate::sessions_root::zone_spaces(&root_id).ok_or_else(|| root_error(&root_id))?;
    let existing = read.seeded.then_some(read.spaces.as_slice());
    let missing = spaces::plan(spaces::SeedMode::Restore, existing);
    if missing.is_empty() {
        return Ok(SessionSpacesRestoredVm { names: Vec::new() });
    }
    let names: Vec<String> = missing.iter().map(|space| space.name.to_owned()).collect();
    let ids: Vec<String> = missing
        .iter()
        .map(|_| crate::sync_ipc::new_ulid())
        .collect();
    let compiled = spaces::compile_seed(&missing, &ids, &today());
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("restore task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(SessionSpacesRestoredVm { names })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_spaces_restore(root_id: String) -> Result<(), IpcError> {
    let _ = root_id;
    Err(unsupported())
}

// ---------------------------------------------------------------------------
// File verbs (FR-262): make and unmake one file inside a session
// ---------------------------------------------------------------------------

/// Now, as the flat contract's filename clock wants it: `HHMM`.
///
/// [`today`]'s companion, and separate from it because the two are used
/// separately — a session folder is named by day, a log file by minute. Local
/// time, like `today`, because the name is read by a person looking at their own
/// Finder and a UTC stamp would put an evening's work on tomorrow.
#[cfg(desktop)]
fn now_hhmm() -> String {
    chrono::Local::now().format("%H%M").to_string()
}

/// The refusal for a path this session will not write, in core's own words.
#[cfg(desktop)]
fn file_verb_error(error: keeper_core::sessions::files::FileVerbError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// Resolve a session and ask the **real** write fence about a path in it.
///
/// The fence is `WriteScope`'s, not a copy: `files::check_rel` refuses a
/// `workspace/` path on shape grounds with no knowledge of zones, and this asks
/// [`keeper_sync::files_write::WriteScope::in_session_workspace`] — the same
/// predicate `sessions_tree` renders its lock from (AD-113). Two predicates that
/// must agree both run; a second "third segment is workspace" test written here
/// would be the one that gets edited alone and drifts.
///
/// Returns the zone root, the session's zone-relative path, and the file's
/// profile-relative subpath — composed here and only here (AD-65).
#[cfg(desktop)]
fn resolve_session_file(
    state: &tauri::State<'_, crate::ipc::AppState>,
    root_id: &str,
    session_id: &str,
    rel: &str,
) -> Result<(std::path::PathBuf, String, String), IpcError> {
    keeper_core::sessions::files::check_rel(rel).map_err(file_verb_error)?;

    let zone_root = crate::sessions_root::zone_of(root_id).ok_or_else(|| root_error(root_id))?;
    let row = crate::sessions_root::row_of(root_id, session_id)
        .ok_or_else(|| session_error(session_id))?;
    let profile = crate::sync_ipc::sessions_profile(state, root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let subpath = format!("{zone}/{}/{rel}", row.path);
    let (_vault, scope) = crate::sync_ipc::sessions_scope(&profile);
    if scope.in_session_workspace(&subpath) {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: keeper_sync::files_write::WriteRefusal::SessionWorkspace { subpath }
                .to_string(),
            account_id: None,
            retriable: false,
        });
    }
    Ok((zone_root, row.path, subpath))
}

/// Every `.md`/`.csv`/`.json` name already taken in one folder of a session.
///
/// Read fresh rather than passed in from the frontend: the name a new file gets
/// has to dodge what is on disk *now*, and a listing the UI fetched when the
/// detail opened is a listing an agent has had minutes to invalidate. Case is
/// folded by the namers, not here — this returns what the directory says.
#[cfg(desktop)]
fn taken_in(dir: &std::path::Path) -> std::collections::BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // A folder that does not exist yet holds no names. The plan's `MkDir`
        // is what creates it, so this is the ordinary first-file case and not
        // an error worth failing a create over.
        return std::collections::BTreeSet::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// Make one file inside a session, and answer with the path that opens it.
///
/// `parent` is session-relative and `""` for the session's own root — the pool,
/// which is where a flat session's markdown belongs. `title` is what the
/// operator typed; the *filename* is derived from it here (AD-65), because a
/// frontend that composed one would be the second namer and the two would
/// disagree about collisions.
///
/// The answer is the profile-relative subpath, so the caller opens the new file
/// through the one file target (AD-109) without joining anything.
///
/// Rejects with: `internal` (unknown root or session, a refused path, a failed
/// write), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_file_new(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    parent: String,
    title: String,
    kind: String,
) -> Result<String, IpcError> {
    use keeper_core::sessions::files;

    let kind = files::NewFileKind::parse(&kind).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "keeper creates .md, .csv and .json files — {kind} is none of those. Anything else \
             belongs in artifacts/, put there by the tool that made it."
        ),
        account_id: None,
        retriable: false,
    })?;
    let parent = parent.trim().trim_matches('/').to_owned();
    // The parent is checked as a path in its own right before a filename is
    // appended to it: `workspace/` must be refused whatever the file is called,
    // and a traversal must not be smuggled in through the folder half.
    if !parent.is_empty() {
        files::check_dir(&parent).map_err(file_verb_error)?;
    }

    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let dir = if parent.is_empty() {
        zone_root.join(&row.path)
    } else {
        zone_root.join(&row.path).join(&parent)
    };
    let name = files::new_named(&title, kind, &taken_in(&dir));
    let rel = if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    };

    let (zone_root, session_path, subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;
    let content = files::render_new(
        kind,
        None,
        title.trim(),
        &crate::sync_ipc::new_ulid(),
        &today(),
    );
    let compiled = files::compile_new(&session_path, &rel, &content).map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("file-new task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(subpath)
}

/// Make a correctly-named, correctly-tagged log or prompt in the session's pool.
///
/// **[`sessions_log_today`]'s flat twin, not a rival.** That command appends a
/// dated heading to a folder-shaped session's `README.md`, which is where its
/// log lives; a flat session has no `## Log` section to append to, and its log
/// is a *file*. Same verb, same button, two contracts — which is why the
/// frontend picks between them on `detail.shape` rather than offering both.
///
/// The name is `YYYY-MM-DD-HHMM-<slug>.md` and the tag is written into
/// frontmatter, because those two together are what decide whether the zone's
/// spaces will ever list the file. A log the operator named freehand is a log
/// that no space selects — the flat shape's one real failure mode, and the whole
/// reason these two verbs exist beside the general one.
///
/// Rejects with: `internal` (unknown root or session, an unknown kind tag, a
/// failed write), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_file_new_kind(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    kind: String,
    title: String,
) -> Result<String, IpcError> {
    use keeper_core::sessions::files;
    use keeper_core::sessions::shape::{KindTag, KINDS};

    let tag = KINDS
        .iter()
        .copied()
        .find(|candidate| candidate.as_str() == kind.trim())
        .ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("not a session file kind: {kind}"),
            account_id: None,
            retriable: false,
        })?;
    // `about` is the session's record, one per session, written by the template
    // and edited in place — a second one would give `shape()` two answers.
    if tag == KindTag::About {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "a session has one about.md — open it rather than making a second.".to_owned(),
            account_id: None,
            retriable: false,
        });
    }

    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let title = if title.trim().is_empty() {
        // An untitled log still needs a slug, and "untitled" is what the
        // migration already writes for an entry whose heading was left blank.
        "untitled".to_owned()
    } else {
        title.trim().to_owned()
    };
    let dir = zone_root.join(&row.path);
    let today = today();
    let rel = files::new_stamped(&title, &today, &now_hhmm(), &taken_in(&dir));

    let (zone_root, session_path, subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;
    let content = files::render_new(
        files::NewFileKind::Markdown,
        Some(tag),
        &title,
        &crate::sync_ipc::new_ulid(),
        &today,
    );
    let compiled = files::compile_new(&session_path, &rel, &content).map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("file-new-kind task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(subpath)
}

/// Remove one file from a session, recoverably.
///
/// A trash move into the zone's `.keeper/trash/<id>/`, never an unlink: a file
/// in a session is something somebody wrote, and a delete button that erases
/// bytes is one nobody presses without making a copy first.
///
/// `about.md` and `AGENTS.md` are refused by
/// [`keeper_core::sessions::files::check_deletable`] — they are the two names
/// `shape()` reads, so deleting one turns a flat session back into a
/// folder-shaped one and hides every log behind a section that no longer exists.
///
/// Rejects with: `internal` (unknown root or session, a refused path, a failed
/// move), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_file_delete(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    rel: String,
) -> Result<(), IpcError> {
    use keeper_core::sessions::files;

    files::check_deletable(&rel).map_err(file_verb_error)?;
    let (zone_root, session_path, _subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;
    let compiled = files::compile_delete(&session_path, &rel, &crate::sync_ipc::new_ulid())
        .map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("file-delete task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_file_new(
    root_id: String,
    session_id: String,
    parent: String,
    title: String,
    kind: String,
) -> Result<String, IpcError> {
    let _ = (root_id, session_id, parent, title, kind);
    Err(unsupported())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_file_new_kind(
    root_id: String,
    session_id: String,
    kind: String,
    title: String,
) -> Result<String, IpcError> {
    let _ = (root_id, session_id, kind, title);
    Err(unsupported())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_file_delete(
    root_id: String,
    session_id: String,
    rel: String,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, rel);
    Err(unsupported())
}
