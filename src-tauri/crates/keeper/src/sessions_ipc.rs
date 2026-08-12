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
/// actually do.
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
