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

/// What each root markdown file of a **flat** pattern declares itself to be
/// (FR-268, AD-120).
///
/// The flat contract puts a file's kind in its frontmatter, so the question
/// "does this file travel into a new session" cannot be answered from the path
/// the way `prompts/**` answers it in the folder contract. The domain decides
/// what each kind means; this only reads the bytes it needs to ask (AD-108).
///
/// Bounded on purpose. Only root markdown is read — a flat session's pool is
/// however much prose the operator wrote, while `artifacts/` and `workspace/`
/// are decided by path and never opened, which is what keeps "make a session
/// like this one" from costing a walk of a folder holding a video render. An
/// unreadable file is simply absent from the map, and an absent kind is
/// `Loose`: it stays behind, which is the safe direction.
#[cfg(desktop)]
fn flat_kinds(
    dir: &std::path::Path,
    files: &[(String, bool)],
) -> std::collections::BTreeMap<String, keeper_core::sessions::shape::KindTag> {
    use keeper_core::sessions::pool::{read_one, PoolFile};

    files
        .iter()
        .filter(|(rel, is_dir)| !*is_dir && !rel.contains('/') && rel.ends_with(".md"))
        .filter_map(|(rel, _)| {
            let text = std::fs::read_to_string(dir.join(rel)).ok()?;
            let entry = read_one(PoolFile {
                rel,
                text: text.as_str(),
            });
            entry.kind.map(|kind| (rel.clone(), kind))
        })
        .collect()
}

/// The named templates a zone offers (FR-266): every `_template/<name>/` that
/// holds a record file, in name order.
///
/// Named rather than counted: what makes a directory under `_template/` a
/// template of its own and not a part of the skeleton is
/// [`keeper_core::sessions::pattern::is_named_template`]'s question, asked
/// against that directory's own top-level names. This reads them; the domain
/// decides (AD-108).
#[cfg(desktop)]
fn named_templates(zone: &std::path::Path) -> Vec<String> {
    use keeper_core::sessions::pattern;

    let template_dir = zone.join(keeper_core::sessions::model::TEMPLATE_DIR);
    let Ok(entries) = std::fs::read_dir(&template_dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            pattern::could_be_named_template(&name, is_dir)
        })
        .filter(|entry| {
            let top_level: Vec<String> = std::fs::read_dir(entry.path())
                .map(|inner| {
                    inner
                        .flatten()
                        .map(|child| child.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            pattern::is_named_template(&top_level)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // Sorted, because `read_dir` order is the filesystem's business and the
    // picker's rows must not move between two reads of an unchanged zone.
    out.sort();
    out
}

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
                        // Checked, like `sessions_space_files` and
                        // `sessions_template_entries`: a cast would wrap a
                        // future-dated file negative, and `max()` would then
                        // read the wrap as "oldest" and order the picker by it.
                        .map(|since| i64::try_from(since.as_millis()).unwrap_or(0))
                })
        })
        .max()
}

/// Project one pattern for the picker: the decision, applied and rendered.
///
/// `excluded` is the directories the source keeps but the copy does not — the
/// zone template's own named templates, and nothing else. It is subtracted
/// before `apply` rather than filtered out of the result, so the preview and
/// the plan are still the one value projected twice: a named template must not
/// appear under *Copies*, and it must not appear under *Leaves behind* either,
/// because it is a different pattern rather than a file this one refused.
#[cfg(desktop)]
fn pattern_vm(
    id: &str,
    kind: keeper_core::sessions::pattern::PatternKind,
    label: &str,
    detail: &str,
    dir: &std::path::Path,
    excluded: &[String],
) -> keeper_core::sessions::vm::SessionPatternVm {
    use keeper_core::sessions::pattern;
    use keeper_core::sessions::vm::{SessionPatternFileVm, SessionPatternSkipVm, SessionPatternVm};

    let files = pattern::without_dirs(&pattern_files(dir), excluded);
    let mtime_ms = newest_mtime_ms(dir, &files);
    // The same kinds the create path reads, for the same reason: the preview
    // and the plan are one value rendered twice, so a flat pattern previewed
    // without its kinds would promise to leave behind prompts the create then
    // copies. Whether it is flat is `apply_with_kinds`' own question; an empty
    // map for a folder-shaped pattern costs one `is_dir` scan and changes
    // nothing.
    let kinds = flat_kinds(dir, &files);
    let outcome = pattern::apply_with_kinds(kind, &files, |rel| kinds.get(rel).copied());
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

/// Everything a new session can be shaped from (FR-253, FR-266): the zone's
/// `_template/` first, then its named templates, then the sessions
/// themselves, newest first.
///
/// The board used to offer these as two unrelated verbs — *New session* on
/// the header and *New like this* on a row's menu — which made "start from
/// what I did last time" a thing you had to already know about. One list,
/// one question, and the preview each entry carries is the plan's own
/// decision rather than a second description of it (AD-116).
///
/// Named templates sit between the two halves because that is what they are
/// between: more deliberate than the zone default, more reusable than the
/// session you happened to run last. They are ordered by name rather than by
/// mtime — a template's identity is the name somebody gave it, and a list that
/// reshuffles because a file was touched is a list you cannot learn.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_patterns(
    root_id: String,
) -> Result<Vec<keeper_core::sessions::vm::SessionPatternVm>, IpcError> {
    use keeper_core::sessions::pattern::{self, PatternKind};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let mut out = Vec::new();
    let template_dir = zone.join(keeper_core::sessions::model::TEMPLATE_DIR);
    let named = named_templates(&zone);
    if template_dir.is_dir() {
        out.push(pattern_vm(
            pattern::TEMPLATE_ID,
            PatternKind::Template,
            pattern::TEMPLATE_LABEL,
            pattern::TEMPLATE_DETAIL,
            &template_dir,
            // The zone skeleton keeps its named templates on disk and leaves
            // them out of what it copies: a template that grew a sibling would
            // otherwise start putting that sibling in every new session.
            &named,
        ));
    }
    for name in &named {
        out.push(pattern_vm(
            &pattern::named_template_id(name),
            PatternKind::Template,
            name,
            pattern::NAMED_TEMPLATE_DETAIL,
            &template_dir.join(name),
            &[],
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
            &[],
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
/// out. `pattern_id` is `None` or `"_template"` for the zone's skeleton,
/// `"_template/<name>"` for one of its named templates (FR-266), and
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
    use keeper_core::sessions::{model, plan, spaces, template};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let title = title.trim().to_owned();
    // **One clock read for the whole create**, and not [`today`] plus
    // [`now_hhmm`], which are two. Two reads were harmless while the only
    // consumers were a folder name and a frontmatter line: a create spanning
    // midnight got a stamp from one day and a date from the next, in strings
    // nobody compares. A template's `{{date}}` and `{{time}}` are read by a
    // person, often in the same paragraph, so the two have to be one moment.
    // The two helpers stay for the verbs that need only one of the three.
    let now = chrono::Local::now();
    let now_local = now.to_rfc3339();
    let date = now.format("%Y-%m-%d").to_string();
    let stamp = format!("{date}-{}", now.format("%H%M"));
    let dir_name = model::session_dir_name(&title, &date, &taken_names(&zone));
    let id = crate::sync_ipc::new_ulid();

    // Which pattern, resolved to the one thing the plan needs: a zone-relative
    // directory to copy out of, and the kind that decides what travels. The
    // domain owns the id→source question (AD-108) — a `_template/<name>` id is
    // a template, not a session path, and an id keeper cannot join onto the
    // zone is refused here rather than reinterpreted downstream.
    let resolved = pattern::resolve(pattern_id.as_deref()).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "no such template: {}",
            pattern_id.as_deref().unwrap_or_default()
        ),
        account_id: None,
        retriable: false,
    })?;
    let (kind, pattern_root, source) = match &resolved {
        pattern::PatternSource::Template { root } => (PatternKind::Template, root.clone(), None),
        pattern::PatternSource::Session { id: source_id } => {
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
    // The zone skeleton does not carry its own named templates into a session.
    // Only the bare `_template` root can hold them, so nothing else pays for
    // the read.
    let excluded = if pattern_root == model::TEMPLATE_DIR {
        named_templates(&zone)
    } else {
        Vec::new()
    };

    // Which contract the new session is born into: the pattern's own. A flat
    // template begets a flat session and a folder-shaped one begets a folder —
    // the shape is a property of the thing being copied, never a preference
    // asked of the user, because a session whose files say one thing and whose
    // shape says another is unreadable by both readers.
    let pattern_files = pattern::without_dirs(&pattern_files(&pattern_dir), &excluded);
    let pattern_top: Vec<String> = pattern_files
        .iter()
        .filter(|(rel, _)| !rel.contains('/'))
        .map(|(rel, _)| rel.clone())
        .collect();
    let shape = keeper_core::sessions::shape::shape(&pattern_top);
    let flat = shape == keeper_core::sessions::shape::Shape::Flat;

    // The stamped record. Folder-shaped: the pattern's own headings, empty,
    // with the title and date in place — a template README that grows a section
    // grows it for every new session, and a continued session inherits the
    // shape it earned. Flat: the same idea against `about.md`, falling back to
    // the shipped default (FR-268) when the pattern has none to inherit from.
    let record_name = if flat {
        keeper_core::sessions::shape::ABOUT
    } else {
        model::README
    };
    let pattern_record = std::fs::read_to_string(pattern_dir.join(record_name)).ok();
    let body = match (&pattern_record, flat) {
        (Some(text), _) => {
            let (_, body_at) = keeper_core::notes::frontmatter::Frontmatter::parse(text);
            plan::skeleton_from(&text[body_at..], &title, &date)
        }
        // No record to inherit: the default template's own `about.md` body,
        // reached through the same renderer so the two cannot drift.
        (None, true) => template::about_only(&title, &date),
        (None, false) => plan::skeleton_from(
            "# <session title>\n\n## Summary\n\n## Log\n\n## Promote\n\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n",
            &title,
            &date,
        ),
    };
    // The record's own tag, so the About space finds it by what it declares
    // rather than by its filename (AD-120). Only the flat contract has kinds.
    let kind_line = if flat { "tags: [about]\n" } else { "" };
    let readme = match &source {
        // continues: baked into the new record's frontmatter at birth (AD-112).
        Some(row) => format!(
            "---\nid: {id}\ncreated: {date}\n{kind_line}keeper:\n  session-continues: [{}]\n---\n{body}",
            row.id
        ),
        None => format!("---\nid: {id}\ncreated: {date}\n{kind_line}---\n{body}"),
    };

    // What travels. In the flat contract a file's kind is a tag inside it, so
    // the decision needs the pool — read here, in the shell, because the domain
    // opens nothing (AD-108). Bounded by the same walk the preview already
    // pays for: root markdown only, and `artifacts/`/`workspace/` are decided
    // by path without being read.
    let kinds = if flat {
        flat_kinds(&pattern_dir, &pattern_files)
    } else {
        std::collections::BTreeMap::new()
    };
    let outcome = pattern::apply_with_kinds(kind, &pattern_files, |rel| kinds.get(rel).copied());
    let copies = outcome.copies;

    // What keeper composes rather than copies. Folder-shaped: the record alone.
    // Flat: the record, always the navigation contract, and — only for a
    // session with nothing to inherit — the two seed files (FR-268).
    //
    // The split is the rule stated once: `AGENTS.md` is a *contract*, so a flat
    // session without one is unreadable and keeper supplies it whenever the
    // pattern did not. The seed log and seed prompt are *examples*, and a
    // continuation is not short of examples — it was made from a session that
    // has real ones. Seeding it anyway would put a "Nothing has happened yet"
    // log at the top of a session continuing months of work.
    let mut stamped = vec![(record_name.to_owned(), readme.clone())];
    if flat {
        let carried: std::collections::BTreeSet<&str> =
            copies.iter().map(|(rel, _)| rel.as_str()).collect();
        // What the pattern already supplies, by KIND — not by filename. A seed
        // is named `YYYY-MM-DD-HHMM-opened.md`, so a template holding one and
        // keeper composing another produce two different names for the same
        // thing and a filename test never fires: the session lands with two
        // "Opened" logs, one of them stamped with a minute that has nothing to
        // do with it. The kind is what may not be duplicated, so the kind is
        // what is compared.
        let carried_kinds: std::collections::BTreeSet<keeper_core::sessions::shape::KindTag> =
            copies
                .iter()
                .filter_map(|(rel, _)| kinds.get(rel).copied())
                .collect();
        let ulids: Vec<String> = (0..3).map(|_| crate::sync_ipc::new_ulid()).collect();
        let seeds =
            template::default_template(&title, &date, &stamp, [&ulids[0], &ulids[1], &ulids[2]]);
        for file in seeds {
            let is_contract = file.name == keeper_core::sessions::shape::AGENTS;
            if file.name == keeper_core::sessions::shape::ABOUT
                || carried.contains(file.name.as_str())
                || file.kind.is_some_and(|kind| carried_kinds.contains(&kind))
                || (!is_contract && source.is_some())
            {
                continue;
            }
            stamped.push((file.name, file.content));
        }
    }

    // The placeholders a template's markdown carries. This side reads the
    // bytes and supplies the context — the clock and the ULID are the shell's
    // (AD-56) — and `pattern::expansions` decides everything else (AD-108), so
    // what a `{{title}}` becomes is provable on a host where this crate does
    // not build.
    //
    // The `expands` test is applied here too, as an optimisation rather than a
    // second rule: it is what stops a template's `.png` being read into memory
    // at all. A file keeper cannot read as UTF-8 is simply not offered, and
    // copies byte for byte as it always did.
    let ctx = keeper_core::notes::templates::TemplateCtx {
        title: title.clone(),
        id: id.clone(),
        now_local,
    };
    let markdown: Vec<(String, String)> = copies
        .iter()
        .filter(|(rel, is_dir)| !*is_dir && pattern::expands(rel))
        .filter_map(|(rel, _)| {
            Some((
                rel.clone(),
                std::fs::read_to_string(pattern_dir.join(rel)).ok()?,
            ))
        })
        .collect();
    let expanded = pattern::expansions(&markdown, &ctx);

    let mut compiled = match &source {
        None => plan::compile_create_shaped(&dir_name, &pattern_root, &copies, &expanded, &stamped),
        Some(row) => plan::compile_create_from_shaped(
            &dir_name,
            &row.path,
            &std::fs::read_to_string(pattern_dir.join(record_name)).unwrap_or_default(),
            record_name,
            &id,
            &copies,
            &expanded,
            &stamped,
        ),
    };
    // The spaces the template offers the ZONE (FR-291). Never the session:
    // AD-121 refused a per-session copy of a query, and `pattern::apply` keeps
    // these out of `copies` for that reason — `outcome.seeds` is non-empty only
    // for a template.
    //
    // **Only into a `_spaces/` that already exists.** An absent one is the
    // signal `sessions_spaces` reads to write the zone the five defaults it was
    // designed around ("the directory is the ledger"); a create that minted the
    // directory to drop one template space into it would consume that signal,
    // and the zone would never be offered the other four. So the create fills
    // holes and `sessions_spaces` digs the well — and the one zone this can
    // decline for says so rather than seeding nothing in silence.
    let space_seeds = if outcome.seeds.is_empty() {
        Vec::new()
    } else {
        let read = crate::sessions_root::zone_spaces(&root_id);
        let seeded = read.as_ref().is_some_and(|read| read.seeded);
        let existing = read.map(|read| read.spaces).unwrap_or_default();
        if seeded {
            let mut sources: Vec<(String, String)> = Vec::new();
            for rel in &outcome.seeds {
                match std::fs::read_to_string(pattern_dir.join(rel)) {
                    Ok(text) => sources.push((rel.clone(), text)),
                    // One space the zone does not gain, said out loud. Never a
                    // refusal: a create must not fail over a file it was only
                    // being offered.
                    Err(error) => tracing::warn!("{rel} was not seeded: {error}"),
                }
            }
            let borrowed: Vec<(&str, &str)> = sources
                .iter()
                .map(|(rel, text)| (rel.as_str(), text.as_str()))
                .collect();
            let planned = spaces::plan_template_spaces(&pattern_root, &borrowed, &existing);
            for sentence in &planned.skipped {
                tracing::warn!("{sentence}");
            }
            planned.seeds
        } else {
            tracing::warn!(
                "this zone has no {}/ yet, so the template's spaces were not seeded — they are offered to a zone that already has its own",
                spaces::SPACES_DIR
            );
            Vec::new()
        }
    };
    // Appended after every write into the new session, because the seed lands
    // OUTSIDE it: a crash before these steps leaves the zone exactly as it was
    // and the new session still readable, through the spaces the zone already
    // had. Inside the create's own plan rather than as a second `spaces-seed`
    // verb, so one press is one journal row and a resume finishes what it began
    // (AD-111).
    compiled
        .steps
        .extend(spaces::template_seed_steps(&space_seeds));
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
///
/// `new_file_kind` joins them for the same reason: the create verb's kind is
/// read off the query by the domain, so the surface is handed a kind or nothing
/// and never a DSL to parse. A query that does not parse gets `error: Some(_)`
/// and `None` here by construction — `creatable_kind` runs the same parser this
/// line does.
#[cfg(desktop)]
fn space_vm(
    space: &keeper_core::sessions::spaces::SessionSpace,
) -> keeper_core::sessions::vm::SessionSpaceVm {
    use keeper_core::notes::{query, sort};
    use keeper_core::sessions::spaces;

    keeper_core::sessions::vm::SessionSpaceVm {
        id: space.rel.clone(),
        name: space.name.clone(),
        query: space.query.clone(),
        sort: space.sort.clone(),
        sort_effective: sort::read(&space.sort).sort.canonical(),
        icon: space.icon.clone(),
        default_key: space.default_key.clone(),
        order: space.order,
        // Carried, not resolved: the fold's four layers are composed in the
        // surface because one of them is a cookie this process cannot read, and
        // the cap is a render cap the section applies to its own rows.
        folded: space.folded,
        rows: space.rows,
        warnings: space.warnings.clone(),
        error: query::parse(&space.query).err().map(|error| error.message),
        new_file_kind: spaces::creatable_kind(&space.query).map(|kind| kind.as_str().to_owned()),
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
    use keeper_core::sessions::shape;
    use keeper_core::sessions::spaces::{self, select, Candidate};
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

    // Which contract this session follows, from its own top-level names —
    // `shape()`'s own input, and `sessions_file_new_kind`'s reading of it
    // (`taken_in` + `shape::shape`), asked once here for every space in the
    // payload. One extra `read_dir` of the session root per read, which is what
    // it costs to stop TypeScript from owning a second copy of the mapping.
    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let root_names: Vec<String> = taken_in(&zone_root.join(&pool.path)).into_iter().collect();
    let session_shape = shape::shape(&root_names);

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
            // Why this space offers no create, and which verb applies instead —
            // both the domain's answers, and both worded there (Story 51.7).
            // This used to ask `creatable_kind` for a kind and then `kind_dir`
            // for a home, which meant a space that offered NO kind was asked
            // nothing and said nothing: the About space rendered neither a
            // button nor a reason, which is the defect the owner reported. One
            // call now, so the shell composes no refusal of its own and the
            // order the refusals are reported in is the domain's.
            let refused = spaces::create_refused(&space.query, session_shape);
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
                no_home: refused.why.map(|why| why.to_string()),
                open_record: refused.record,
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
        // Straight through from the form, which seeded them from `space_vm`.
        // `render_edit` replaces the whole `keeper:` map, so dropping either
        // here would delete the operator's answer on the next Save of anything
        // else — the one failure this story is arranged to prevent.
        folded: space.folded,
        rows: space.rows,
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

/// The zone-relative directory a *new* template name means — `_template` for
/// the zone's own, `_template/<slug>` for a named one — or the refusal for a
/// name with nothing in it a folder can be called after.
///
/// **Minting, not addressing.** This runs the name through
/// [`keeper_core::notes::naming::slug`] because the caller is about to create a
/// directory out of something a person typed; [`template_at`] is its twin for
/// the verbs that must find a directory somebody already has. Shared by install
/// and by rename's destination rather than copied into each: the refusal is one
/// sentence, and two copies of it are two chances for the two verbs to disagree
/// about which names a zone accepts.
#[cfg(desktop)]
fn template_mint(name: Option<&str>) -> Result<String, IpcError> {
    use keeper_core::sessions::{model, template};

    match name.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(model::TEMPLATE_DIR.to_owned()),
        Some(raw) => {
            // The typed name is tested BEFORE it is slugged, and that order is
            // the whole guard. `naming::slug` never answers empty — a note it
            // refused to name would be a note that was lost, so it substitutes
            // `untitled` — which made the old `slug(raw).is_empty()` test dead
            // code: `###` minted `_template/untitled` and a rename moved the
            // operator's template into a directory they never typed. Asking the
            // fold is the domain's own question (AD-108), not a re-derived
            // "contains a letter or digit" that would drift from it.
            if !template::nameable(raw) {
                return Err(IpcError {
                    code: IpcErrorCode::Internal,
                    message: format!(
                        "\"{raw}\" has nothing in it a folder can be named after — a named \
                         template needs letters or digits."
                    ),
                    account_id: None,
                    retriable: false,
                });
            }
            Ok(format!(
                "{}/{}",
                model::TEMPLATE_DIR,
                keeper_core::notes::naming::slug(raw)
            ))
        }
    }
}

/// The zone-relative directory an *existing* template name means, verbatim.
///
/// **Addressing, not minting** — and the difference is worth two functions. A
/// name that arrives here identifies a directory that is already on the drive,
/// and `_template/Interview Kit/` is a template these docs invite an operator to
/// make by hand. Slugging it would send the read to `_template/interview-kit`:
/// an empty room for a template with files in it, and — if both names existed —
/// a rename that moved the wrong directory.
///
/// The guard is the domain's own.
/// [`keeper_core::sessions::pattern::could_be_named_template`] answers
/// "may keeper join this segment onto a zone root under `_template/`", which is
/// the same predicate [`named_templates`] filtered the picker's rows by; so a row
/// the picker showed is a row these verbs can address, and a `..` from the
/// webview is refused before anything is opened.
///
/// The caller has this name without composing it: a named template's
/// `SessionPatternVm.label` **is** its folder name
/// ([`keeper_core::sessions::pattern::NAMED_TEMPLATE_DETAIL`]'s own note), so the
/// Templates list passes the label straight back and joins nothing (AD-65).
#[cfg(desktop)]
fn template_at(name: Option<&str>) -> Result<String, IpcError> {
    use keeper_core::sessions::{model, pattern};

    match name.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(model::TEMPLATE_DIR.to_owned()),
        Some(raw) if pattern::could_be_named_template(raw, true) => {
            Ok(format!("{}/{raw}", model::TEMPLATE_DIR))
        }
        Some(raw) => Err(IpcError {
            code: IpcErrorCode::Internal,
            // What `pattern::safe_segment` actually refuses, said in its own
            // order. The sentence used to promise "no dots", which is not the
            // rule — `v1.2` is a legal template name, and only a name that IS
            // `.` or `..`, or begins with a dot, is turned away. A refusal that
            // overstates the rule teaches the operator to avoid names keeper
            // accepts.
            message: format!(
                "\"{raw}\" is not a name keeper will look for under {}/ — a template's directory \
                 name carries no separators, is not \".\" or \"..\", and does not begin with a \
                 dot or an underscore.",
                model::TEMPLATE_DIR
            ),
            account_id: None,
            retriable: false,
        }),
    }
}

/// Write keeper's own template into this zone's `_template/` (FR-268).
///
/// **The zone's template is the operator's, and this verb says so out loud.** A
/// zone that has one is never touched by a create — keeper copies what it finds
/// and does not improve on it — so the only way to *adopt* an updated default is
/// to ask for it. Pressing this is that ask.
///
/// `name` is `None` for the zone's own `_template/` and `Some(slug)` for a named
/// one, which is what makes "keep my template, add keeper's as `flat`" possible
/// without a second command.
///
/// **A skeleton, not a rendered session**: the contract and an empty record, and
/// none of the seeds. There is no `title` parameter because a template has no
/// title — the one this used to take was frozen into every session ever created
/// from the result (see [`template::zone_skeleton`]).
///
/// Anything already there under one of the two names is trashed before it is
/// rewritten, so an `AGENTS.md` somebody improved by hand is recoverable in
/// `.keeper/trash/` rather than gone. Files the template does not name are left
/// alone — including seeds the operator added themselves, which a create then
/// carries in preference to keeper's: this replaces two files, it does not clear
/// a directory.
///
/// Rejects with: `internal` (unknown root, a bad name, a failed write),
/// `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_install(
    root_id: String,
    name: Option<String>,
) -> Result<String, IpcError> {
    use keeper_core::sessions::template;

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    // Minted, not addressed: the name came out of a text field, so it is
    // slugged before it becomes a directory (see [`template_mint`]).
    let dest = template_mint(name.as_deref())?;

    let date = today();
    // The skeleton, not a rendered session: `_template/` gets the contract and
    // an empty record, and keeper composes the seed log and seed prompt fresh
    // per create, with that session's own title.
    let files = template::zone_skeleton(&date, &crate::sync_ipc::new_ulid());
    // What is already there decides trash-then-write versus plain write, and
    // reading it is the shell's job — the domain opens nothing (AD-108).
    let present: Vec<String> = std::fs::read_dir(zone.join(&dest))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let compiled = template::compile_install(&dest, &files, &present, &crate::sync_ipc::new_ulid());
    let zone_root = zone.clone();
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-install task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(dest)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_install(
    root_id: String,
    name: Option<String>,
) -> Result<String, IpcError> {
    let _ = (root_id, name);
    Err(unsupported())
}

/// Every file **and folder** inside one template's directory (FR-269, FR-270).
///
/// The Templates list is a room the operator walks into, and a template's rows
/// are its files: pressing one opens it in the same editor a session's record
/// opens in. So this is a listing and nothing more — no parse, no kinds, no
/// tags. A template is a skeleton, and the only questions the list asks about it
/// are what is in here and which of it changed last.
///
/// `name` is `None` for the zone's own `_template/` and `Some(name)` for a named
/// one — the same argument [`sessions_template_install`] takes, so the two verbs
/// address a template the same way. The name is used verbatim
/// ([`template_at`]), because it identifies a directory that already exists.
///
/// **The same walk the picker previews from.** A template's rows are
/// [`pattern_files`] minus the directories the zone skeleton keeps and does not
/// copy — [`pattern_vm`]'s own composition — so "what is in this template" is
/// asked once. A second reader of the same directory is how the room and the
/// create came to disagree: this listing was non-recursive, so a folder-shaped
/// template's `prompts/*.md` were an empty room; it dropped `_`-prefixed files a
/// create carries; and its file test was lstat-shaped, so a symlinked file was
/// missing from the room and present in every session made from it.
///
/// The walk, and not the pattern's *decision* about it: the picker's *Copies*
/// list is this same walk put through
/// [`keeper_core::sessions::pattern::apply`], which stamps a record rather than
/// copying it, so `about.md` is a row here and never a row there. That is the
/// one intended difference between the two surfaces — the room is where a
/// template's record is edited, which is the whole of FR-270 — and it is a
/// difference in what the create does with a file, not in what the directory
/// holds. Only `.gitkeep` is dropped here: it holds an empty directory open and
/// there is nothing in it to read, and the directory it was holding open is now
/// a row in its own right.
///
/// **A folder is a row, and that is what makes it addressable.** A directory
/// that names no file — one `New folder` just made, or a skeleton's
/// `artifacts/` whose only content is the `.gitkeep` below — used to be dropped
/// here, so the room could not draw it, and a row the room cannot draw is one
/// nothing can rename, delete, or create into. The webview still derives the
/// *shape* from these paths (`templateTree`) rather than walking anything; what
/// it can no longer do is invent the existence of a folder from its contents.
///
/// Every `subpath` is profile-relative and composed HERE (AD-65): the zone
/// subfolder, the template directory and the file's template-relative path,
/// joined once in Rust exactly as [`sessions_space_files`] joins a space's rows.
/// The webview does not know the zone's subfolder, and a second joiner is how
/// the picker's path and this list's path start to disagree about the same file.
///
/// **A directory that is not there answers `Ok(vec![])`.** A template someone
/// removed in Finder under us is an empty room, not a fault: this list re-reads
/// after every write, and an error banner over a directory that is simply gone
/// would be keeper reporting the operator's own edit as a failure.
///
/// Rejects with: `internal` (unknown root, a name keeper will not join),
/// `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_template_entries(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    name: Option<String>,
) -> Result<Vec<keeper_core::sessions::vm::SessionTemplateEntryVm>, IpcError> {
    use keeper_core::sessions::{model, pattern, vm::SessionTemplateEntryVm};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let rel = template_at(name.as_deref())?;

    // The zone subfolder, so every row carries a path the frontend can open
    // without composing one (AD-65) — `sessions_space_files`' mechanism, and its
    // spelling.
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone_prefix = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let prefix = format!("{zone_prefix}/{rel}");

    // One walk, shared with the picker — see this command's own doc. `excluded`
    // is `pattern_vm`'s: the zone skeleton leaves its named templates out of
    // what it copies, so the room must leave them out of what it lists, and a
    // named template excludes nothing (a `prompts/` of its own is its own).
    //
    // An absent directory walks to nothing, so a template the operator removed
    // in Finder is still an empty room rather than an error banner.
    let dir = zone.join(&rel);
    let excluded = if rel == model::TEMPLATE_DIR {
        named_templates(&zone)
    } else {
        Vec::new()
    };
    let files = pattern::without_dirs(&pattern_files(&dir), &excluded);
    let mut out: Vec<SessionTemplateEntryVm> = files
        .iter()
        // A `.gitkeep` is not a row, for the picker's own reason: it holds an
        // empty directory open and there is nothing in it to read. The directory
        // IS a row — see this command's doc — carrying `is_dir` so the room draws
        // it rather than guessing at it from the files underneath.
        .filter(|(path, _)| !pattern::is_placeholder(path))
        .map(|(path, is_dir)| {
            // Checked, not `as i64` — `sessions_space_files`' spelling. A
            // future-dated file whose millisecond count does not fit wraps
            // negative under a cast and then sorts to the bottom of a
            // newest-first list, which is the one place the wrap would be read
            // as an answer rather than as a fault.
            let mtime_ms = std::fs::metadata(dir.join(path))
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(0));
            SessionTemplateEntryVm {
                subpath: format!("{prefix}/{path}"),
                // Template-relative rather than a basename, now that the walk
                // reaches into subdirectories: `prompts/hand-off.md` and
                // `refs/hand-off.md` are two rows, and labelling both
                // `hand-off.md` would make the room ambiguous about which file
                // a press opens. Still composed in Rust — slicing a path in the
                // webview is still a path operation (AD-65).
                name: path.clone(),
                mtime_ms,
                is_dir: *is_dir,
            }
        })
        .collect();
    // Newest first, ties broken by name — the sessions tree's own recent order,
    // because a template is edited one file at a time and the file touched last
    // is the one the operator came back for. A tie-break at all because two
    // files written in the same millisecond are not an order.
    out.sort_by(|a, b| {
        b.mtime_ms
            .cmp(&a.mtime_ms)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_entries(
    root_id: String,
    name: Option<String>,
) -> Result<Vec<()>, IpcError> {
    // The desktop twin takes `state` too; a mobile twin that refuses does not
    // need it, and asking for it would make the mobile build depend on
    // `AppState` for a function that never reads it — `sessions_space_files`'
    // stub, and its reason.
    let _ = (root_id, name);
    Err(unsupported())
}

/// Rename one named template (FR-271).
///
/// **Why a verb and not a text field over `std::fs::rename`.** The zone lives on
/// a synced drive whose history keeper owns, so a directory renamed behind
/// keeper's back is a write its watcher sees as somebody else's. This goes
/// through the same plan/journal/exec path every other lifecycle verb uses, and
/// the drive gets one commit with keeper's provenance on it.
///
/// `name` addresses the template as it is on disk ([`template_at`]); `new_name`
/// is minted, so it is slugged exactly as [`sessions_template_install`] slugs the
/// name it creates. Resolves to the new id, `_template/<slug>` — the spelling
/// `sessions_patterns` answers with after the rescan, so the caller can select
/// the row it just renamed without composing an id (AD-65).
///
/// **The zone's own `_template/` cannot be renamed**, and an empty `name` means
/// exactly that directory. Its name IS the contract:
/// [`keeper_core::sessions::model::TEMPLATE_DIR`] is what a create copies from
/// and what the scan skips, and a zone has one of it. Renaming it would not
/// produce a differently-named zone template; it would produce a zone with none.
///
/// **It refuses rather than merges.** Install may trash-then-write, because the
/// operator asked for keeper's skeleton and the displaced bytes land somewhere
/// recoverable. A rename has no such mandate: moving onto a name that is taken
/// would bury one operator's template under another's, so a collision is a
/// refusal and both directories stay where they are. "Taken" means a *different*
/// directory, decided by identity rather than by spelling — see the check
/// itself: on a case-insensitive volume the destination of a case-only rename
/// exists because it IS the source.
///
/// **An empty `new_name` is refused here**, before [`template_mint`] is asked.
/// Mint's "empty means the zone's own `_template/`" rule is right for install
/// and wrong for a destination: it would compute a move of a named template on
/// top of the zone contract, stopped only incidentally by the collision check
/// and reported under a directory name the operator never typed.
///
/// **Not idempotent, and the caller needs the shape of that.** A `new_name`
/// whose slug already IS the directory's own name resolves without writing — a
/// journal row and a commit for a no-op move would be noise on a synced drive.
/// A name that folds to something else is a real move even when it looks like
/// the name already there: a hand-made `_template/Interview Kit/` re-typed
/// verbatim slugs to `interview-kit` and moves. And a genuine double-submit
/// after a successful rename is refused, because the source it names has moved.
/// So the caller's answer to a rejection is to re-read the list, never to retry
/// the call.
///
/// Rejects with: `internal` (unknown root, the zone's own template, an empty
/// `new_name`, a name with nothing to slug, a source that is not a directory, a
/// destination that is a different directory already, a failed move),
/// `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_rename(
    root_id: String,
    name: String,
    new_name: String,
) -> Result<String, IpcError> {
    use keeper_core::sessions::{model, template};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    // An empty `name` is the zone's own `_template/`, and that is the one name
    // there is nothing to rename to.
    let named = name.trim();
    if named.is_empty() {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "the zone's own {}/ cannot be renamed — its name is the contract every create \
                 looks for, and a zone has exactly one of it. Rename a template inside it \
                 instead.",
                model::TEMPLATE_DIR
            ),
            account_id: None,
            retriable: false,
        });
    }
    // An empty `new_name` is refused before it is minted: `template_mint` reads
    // an empty name as the zone's own `_template/`, which is right for install
    // and would make this a move of a named template onto the zone contract.
    // The collision check below happens to stop that, and stops it while naming
    // a directory the operator never typed — so the missing name is said here,
    // where the sentence can be about the field that was left blank.
    let renamed = new_name.trim();
    if renamed.is_empty() {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "a rename needs a new name — the field was empty, so nothing was moved."
                .to_owned(),
            account_id: None,
            retriable: false,
        });
    }
    let from = template_at(Some(named))?;
    let to = template_mint(Some(renamed))?;
    // Reading what is on the drive is the shell's job — the domain opens nothing
    // (AD-108) — so both refusals below are decided here and the compiler is
    // handed a move it can just describe.
    let source = zone.join(&from);
    let target = zone.join(&to);
    if !source.is_dir() {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "there is no template at {from} in this zone, so there is nothing to rename."
            ),
            account_id: None,
            retriable: false,
        });
    }
    if to == from {
        return Ok(to);
    }
    // "Already exists" has to mean a *different* directory. `from` is verbatim
    // and `to` is slugged, so renaming a hand-made `_template/Interview/` to
    // `interview` gets past the equality above — and then `exists()` answers
    // true on APFS and NTFS about the source itself, refusing the one rename
    // that normalises a name somebody typed by hand.
    //
    // The same predicate `MoveDir` guards itself with
    // ([`crate::sessions_exec::same_directory`]), asked here so the operator
    // reads a sentence rather than an executor refusal — and asked from the one
    // definition, because two of them would be two chances for the edge and the
    // executor to disagree about which moves this zone accepts.
    if target.exists() && !crate::sessions_exec::same_directory(&target, &source) {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "{to} already exists — pick another name. A rename will not write over a \
                 template somebody else made."
            ),
            account_id: None,
            retriable: false,
        });
    }

    let compiled = template::compile_rename(&from, &to);
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-rename task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(to)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_rename(
    root_id: String,
    name: String,
    new_name: String,
) -> Result<String, IpcError> {
    let _ = (root_id, name, new_name);
    Err(unsupported())
}

// ---------------------------------------------------------------------------
// Entry verbs (FR-284): make, rename and unmake what is INSIDE one template
// ---------------------------------------------------------------------------
//
// The session file verbs below cannot be pointed at a template, and the wall is
// an id lookup rather than a path guard: `sessions_file_new`,
// `sessions_file_new_kind` and `sessions_file_delete` all resolve their
// directory through `sessions_root::row_of(root_id, session_id)`, a lookup over
// the scan's rows, and `_template/` is never scanned as a session (FR-225). So
// these four address a template the way the three verbs above it do — by
// `(root_id, name)` through `template_at` — and take a template-relative `rel`
// for the entry itself.

/// The refusal for a template path the domain will not compile, in its words.
///
/// [`file_verb_error`]'s twin, over
/// [`keeper_core::sessions::template::EntryError`] rather than a session's
/// `FileVerbError` — see that type for why the two sets of refusals are not one
/// type.
#[cfg(desktop)]
fn entry_error(error: keeper_core::sessions::template::EntryError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// What a verb says about a destination that is already taken.
///
/// The words `sessions_template_rename` refuses a template collision with, one
/// level down: `WriteFile` overwrites by contract and `MoveFile`/`MoveDir` refuse,
/// so without this a *create* would silently write over a file somebody put in
/// the template while a *rename* onto the same name was refused — two answers to
/// one question.
#[cfg(desktop)]
fn entry_taken_error(rel: &str) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "{rel} is already in this template — pick another name. keeper will not write over a \
             file or a folder somebody put there."
        ),
        account_id: None,
        retriable: false,
    }
}

/// The refusal for a create whose parent folder is not on the drive — the shell
/// half of "this verb addresses a folder, it never mints one".
///
/// Both create verbs fold only the LAST segment of what was typed
/// (`template::entry_name`, through `rejoin`), because the segments in front of
/// it address directories that already exist — a hand-made `Interview Kit/` is
/// addressable exactly as it is spelled. A verb that *created* a missing parent
/// would spell it verbatim, so `Interview Kit/notes.md` minted a folder
/// `New folder` could never have made, whose name folds to `interview-kit`. The
/// refusal has to live here for [`entry_taken_error`]'s reason: the domain opens
/// nothing (AD-108), and `atomic_write`'s own `create_dir_all` would otherwise
/// make the parent whatever the plan said.
#[cfg(desktop)]
fn entry_parent_error(parent: &str) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "there is no folder {parent} in this template. Make it with New folder first — a \
             create names the file, and keeper will not invent the folder around it under a name \
             it would have folded."
        ),
        account_id: None,
        retriable: false,
    }
}

/// Whether the folder a template-relative create lands in is already there.
///
/// One question for both create verbs, so *New file* and *New folder* cannot
/// answer it differently — see [`entry_parent_error`] for why it is asked at all.
/// A root-level `rel` has no parent to ask about: the template's own directory is
/// proved by [`template_dir`].
#[cfg(desktop)]
fn entry_parent_present(zone: &std::path::Path, dir: &str, rel: &str) -> Result<(), IpcError> {
    let Some((parent, _)) = rel.rsplit_once('/') else {
        return Ok(());
    };
    if zone.join(format!("{dir}/{parent}")).is_dir() {
        return Ok(());
    }
    Err(entry_parent_error(parent))
}

/// The template directory a `(root_id, name)` pair addresses, and the zone root
/// it sits in.
///
/// One resolver for all four verbs, so "unknown root" and "no such template" are
/// each one sentence. The template is required to *be* a directory here rather
/// than assumed: `sessions_template_entries` answers an absent one with an empty
/// room on purpose — a template removed in Finder is not a fault to a reader —
/// but a *write* into a directory that is not there would create it, and a
/// template keeper invented is not one the operator named.
#[cfg(desktop)]
fn template_dir(
    root_id: &str,
    name: Option<&str>,
) -> Result<(std::path::PathBuf, String), IpcError> {
    let zone = crate::sessions_root::zone_of(root_id).ok_or_else(|| root_error(root_id))?;
    let rel = template_at(name)?;
    if !zone.join(&rel).is_dir() {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "there is no template at {rel} in this zone, so there is nothing to change \
                 inside it."
            ),
            account_id: None,
            retriable: false,
        });
    }
    Ok((zone, rel))
}

/// The profile-relative subpath of one zone-relative path, composed here and
/// only here (AD-65).
///
/// `sessions_template_entries`' own composition, so the path a create answers
/// with is the same string the row for that file will carry when the room
/// re-reads — and the webview opens both through the one file target without
/// joining anything.
#[cfg(desktop)]
fn template_subpath(
    state: &tauri::State<'_, crate::ipc::AppState>,
    root_id: &str,
    zone_rel: &str,
) -> Result<String, IpcError> {
    let profile = crate::sync_ipc::sessions_profile(state, root_id)?;
    let zone_prefix = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    Ok(format!("{zone_prefix}/{zone_rel}"))
}

/// Whether the entry a rename or a delete names is a file or a folder — the one
/// fact the domain cannot work out for itself (AD-108).
///
/// A path with nothing at it is refused here rather than left to the executor: a
/// missing source is a stale list, and "read the list again" is a different
/// instruction from "that name is taken".
#[cfg(desktop)]
fn entry_kind(
    source: &std::path::Path,
    rel: &str,
) -> Result<keeper_core::sessions::template::EntryKind, IpcError> {
    use keeper_core::sessions::template::EntryKind;

    if source.is_dir() {
        return Ok(EntryKind::Dir);
    }
    if source.is_file() {
        return Ok(EntryKind::File);
    }
    Err(IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "there is nothing at {rel} in this template — it moved or was removed. Read the \
             list again rather than trying again."
        ),
        account_id: None,
        retriable: false,
    })
}

/// Make one file inside a template (FR-284), and answer with the path that opens
/// it.
///
/// `rel` is template-relative and carries the filename: `notes.md` at the
/// template's root, `refs/inputs.md` in a folder that is already there. The last
/// segment is folded to a slug and its extension kept — `Kick Off.md` lands as
/// `kick-off.md`, never `kick-off-md` — while the directories in front of it
/// travel verbatim, because they address folders that already exist
/// (`template_at`'s rule one level down).
///
/// **A folder in front of the filename is addressed, never created.** It used to
/// be created, in the same plan and under the name as typed, which minted a
/// directory *New folder* could not have made: `Interview Kit/notes.md` left an
/// `Interview Kit/` on the drive while the same words through
/// `sessions_template_dir_new` fold to `interview-kit`. So a parent that is not
/// there is refused here (`entry_parent_error`), and every directory keeper
/// mints in a template is minted by the folder verb, through the one fold.
///
/// **The file lands empty.** A template is copied into every session made from
/// it, so frontmatter with an `id` in it would hand every one of those sessions
/// the same identity — the freeze `template::zone_skeleton` exists to prevent. A
/// `.json` gets `{}` because an empty file is not valid JSON.
///
/// Rejects with: `internal` (unknown root, no such template, a path that leaves
/// the template, a dotfile, an extension outside `.md`/`.csv`/`.json`, a name
/// that folds to nothing, a folder in the path that is not there, a destination
/// that exists, a failed write),
/// `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_file_new(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<String, IpcError> {
    use keeper_core::sessions::template;

    let (zone, dir) = template_dir(&root_id, name.as_deref())?;
    let compiled = template::compile_file_new(&dir, &rel).map_err(entry_error)?;
    // A create names the FILE. The folder in front of it must already be on the
    // drive, because only the last segment went through the fold — see
    // `entry_parent_error`. Asked before the collision test: "there is no folder
    // refs" is a truer answer about `refs/inputs.md` than anything about the
    // file's own name.
    entry_parent_present(&zone, &dir, &compiled.rel)?;
    let landed = format!("{dir}/{}", compiled.rel);
    // Reading the drive is the shell's job (AD-108), and this refusal has to be
    // made here: `WriteFile` overwrites by contract, so nothing further down
    // would stop a create from taking a file somebody wrote.
    if zone.join(&landed).exists() {
        return Err(entry_taken_error(&compiled.rel));
    }
    let subpath = template_subpath(&state, &root_id, &landed)?;
    let plan = compiled.plan;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, plan))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-file-new task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(subpath)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_file_new(
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<String, IpcError> {
    // No `state`: the desktop twin needs it to compose a profile-relative path,
    // and a mobile twin that refuses would only be making the mobile build depend
    // on `AppState` for a function that never reads it (`sessions_template_entries`'
    // stub, and its reason).
    let _ = (root_id, name, rel);
    Err(unsupported())
}

/// Make one folder inside a template (FR-284).
///
/// `rel` is template-relative: `artifacts` at the root, `refs/inputs` inside a
/// folder that is already there. **Idempotent** — a folder that is
/// already there succeeds and changes nothing, because `MkDir` says so and
/// because the four skeleton directories are exactly the names somebody types
/// without checking first. A *file* already at that path is refused, since that
/// is the one collision `mkdir` cannot absorb.
///
/// A template's `workspace/` may be created and trashed like any other folder,
/// unlike a session's: `files::check_dir` refuses scratch because AD-113 fences
/// every write out of a live session's workspace, and a template's `workspace/`
/// is a skeleton directory a create copies rather than scratch anything is
/// writing into.
///
/// Rejects with: `internal` (unknown root, no such template, a path that leaves
/// the template, a dotfile, a name that folds to nothing, a folder in the path
/// that is not there, a file at that path, a failed write), `unsupported`
/// (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_dir_new(
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<(), IpcError> {
    use keeper_core::sessions::template;

    let (zone, dir) = template_dir(&root_id, name.as_deref())?;
    let compiled = template::compile_dir_new(&dir, &rel).map_err(entry_error)?;
    // The folder verb mints its LAST segment and no other, for the reason
    // `sessions_template_file_new` refuses the same thing: `create_dir_all` would
    // spell an absent `Interview Kit/` verbatim, and that name is one this room
    // folds when it is typed as a name of its own.
    entry_parent_present(&zone, &dir, &compiled.rel)?;
    if zone.join(format!("{dir}/{}", compiled.rel)).is_file() {
        return Err(entry_taken_error(&compiled.rel));
    }
    let plan = compiled.plan;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, plan))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-dir-new task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_dir_new(
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<(), IpcError> {
    let _ = (root_id, name, rel);
    Err(unsupported())
}

/// Rename one file or folder inside a template (FR-284), and answer with the
/// path that opens the result.
///
/// **Why this is offered here and refused for a session's files.**
/// `docs/sessions.md`'s refusal is about link identity: a hand-written file has
/// no `id` and is identified by its path, so renaming it breaks every pin
/// pointing at it. A template has no such graph — nothing pins a template's
/// files, and a create *copies* them rather than referencing them — and the room
/// already renames a whole template directory, which moves every file inside it
/// at once. The relaxation is recorded rather than smuggled: see the Templates
/// section of `docs/sessions.md`.
///
/// `new_name` is a name a person typed: its stem folds to a slug and its
/// extension survives, and a file whose typed name carries no extension keeps the
/// one it has — a rename renames, it does not decide what kind of file this is.
/// The entry stays in its own folder; only the last segment changes.
///
/// **Not a move between folders**, and not idempotent in the useful direction: a
/// name that folds to the one already there writes nothing and answers, while a
/// name that folds to anything else is a real move even when it looks like the
/// name on screen. A collision means a *different* file, by identity rather than
/// by spelling — on a case-insensitive volume the destination of a case-only
/// rename exists because it IS the source.
///
/// Rejects with: `internal` (unknown root, no such template, a path that leaves
/// the template, a dotfile, the template root itself, nothing at that path, a
/// name that folds to nothing, a destination that is a different entry, a failed
/// move), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_rename_entry(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    name: Option<String>,
    rel: String,
    new_name: String,
) -> Result<String, IpcError> {
    use keeper_core::sessions::template;

    let (zone, dir) = template_dir(&root_id, name.as_deref())?;
    // Normalised before it is joined onto a zone root, because the next lines
    // stat it. The guard is the domain's own and the compiler asks it again — a
    // guard the caller can skip is not a guard (`files::check_rel`'s pattern).
    let rel = template::entry_rel(&rel).map_err(entry_error)?;
    let source = zone.join(&dir).join(&rel);
    let kind = entry_kind(&source, &rel)?;
    let compiled =
        template::compile_entry_rename(&dir, &rel, &new_name, kind).map_err(entry_error)?;
    let landed = format!("{dir}/{}", compiled.rel);
    let subpath = template_subpath(&state, &root_id, &landed)?;
    if compiled.rel == rel {
        // A journal row and a commit for a move that is not one would be noise on
        // a synced drive — `sessions_template_rename`'s rule, one level down.
        return Ok(subpath);
    }
    let target = zone.join(&landed);
    // The same predicate the plan step guards itself with
    // ([`crate::sessions_exec::same_directory`]), asked here so the operator reads
    // a sentence rather than an executor refusal, and asked from the one
    // definition so the two layers cannot disagree about which renames a template
    // accepts.
    if target.exists() && !crate::sessions_exec::same_directory(&target, &source) {
        return Err(entry_taken_error(&compiled.rel));
    }
    let plan = compiled.plan;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, plan))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-entry-rename task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(subpath)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_rename_entry(
    root_id: String,
    name: Option<String>,
    rel: String,
    new_name: String,
) -> Result<String, IpcError> {
    let _ = (root_id, name, rel, new_name);
    Err(unsupported())
}

/// Remove one file or folder from a template (FR-284), recoverably.
///
/// A trash move into the zone's `.keeper/trash/<id>/`, never an unlink and never
/// a `remove_dir_all`: a template is a thing somebody wrote, and a folder delete
/// that erases bytes is one nobody presses without making a copy first. A
/// directory goes whole, which is what makes it recoverable whole.
///
/// The session tree's own refusal — *"keeper deletes one file at a time. Removing
/// a folder takes everything inside it with it"* — is not repeated here, and that
/// is a judgement rather than an oversight: it is about a live session's
/// directories, which hold work, while a template's hold a skeleton the operator
/// put there and a create copies. The trash is what makes it safe either way.
///
/// `rel` is template-relative. The template **root** is refused
/// (`template::entry_rel`), because deleting a whole template is a different verb
/// with a different confirmation.
///
/// Rejects with: `internal` (unknown root, no such template, a path that leaves
/// the template, a dotfile, the template root itself, nothing at that path, a
/// failed move), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_template_delete_entry(
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<(), IpcError> {
    use keeper_core::sessions::template;

    let (zone, dir) = template_dir(&root_id, name.as_deref())?;
    let rel = template::entry_rel(&rel).map_err(entry_error)?;
    let source = zone.join(&dir).join(&rel);
    let kind = entry_kind(&source, &rel)?;
    let compiled = template::compile_entry_delete(&dir, &rel, kind, &crate::sync_ipc::new_ulid())
        .map_err(entry_error)?;
    let plan = compiled.plan;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, plan))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("template-entry-delete task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_template_delete_entry(
    root_id: String,
    name: Option<String>,
    rel: String,
) -> Result<(), IpcError> {
    let _ = (root_id, name, rel);
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

/// The refusal for a properties block that cannot be spliced, in core's own
/// words — [`file_verb_error`]'s twin, and here for its reason: the Stale
/// sentence carries the Re-read advice the surface acts on, so a wording
/// invented here would be a second answer to one question.
#[cfg(desktop)]
fn properties_refusal(error: keeper_core::file_properties::PropertiesRefusal) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// Which session a profile-relative subpath is inside, and where in it —
/// [`resolve_session_file`]'s inverse (Story 51.6).
///
/// **Here rather than in the webview, and that is AD-65 read backwards.** The
/// rule is that Rust composes paths; the corollary nobody had needed until now is
/// that Rust also *decomposes* them. The properties panel addresses a session file
/// by `(profile id, subpath)` because a session's `README.md` has no note id
/// (Story 50.4), so a session verb reachable from that panel has to start from
/// that address — and the split needs the zone subfolder and the session folder
/// set, neither of which the frontend holds.
///
/// **Asked of the scanned rows**, and the deepest match wins, exactly as
/// [`crate::sessions_root::session_at`] does it: a path inside a session names
/// that session and not an ancestor that happens to share its prefix. A second
/// definition of "is this a session" is the drift `model::classify` exists to
/// prevent.
///
/// # Errors
/// `internal` when the root is unknown, when the subpath is not under the zone,
/// or when it is not inside any session the last scan found — three states with
/// one honest answer, because the caller's next move is the same for all three.
#[cfg(desktop)]
fn session_of_subpath(
    state: &tauri::State<'_, crate::ipc::AppState>,
    root_id: &str,
    subpath: &str,
) -> Result<(String, String), IpcError> {
    let elsewhere = || IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "{subpath} is not inside a session of this zone, so keeper has no session to rename \
             it in. Reopen the file from the board if the zone has moved."
        ),
        account_id: None,
        retriable: false,
    };

    // The profile's own value rather than the registry's copy of it: the zone the
    // subpath was composed with is the one `resolve_session_file` will compose it
    // with again, and one source cannot disagree with itself.
    let profile = crate::sync_ipc::sessions_profile(state, root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let zone_relative = if zone.is_empty() {
        subpath
    } else {
        subpath
            .strip_prefix(&format!("{zone}/"))
            .ok_or_else(elsewhere)?
    };

    let rows = crate::sessions_root::rows(root_id).ok_or_else(|| root_error(root_id))?;
    let row = rows
        .iter()
        .filter(|row| zone_relative.starts_with(&format!("{}/", row.path)))
        .max_by_key(|row| row.path.len())
        .ok_or_else(elsewhere)?;
    let rel = zone_relative
        .get(row.path.len() + 1..)
        .filter(|rel| !rel.is_empty())
        .ok_or_else(elsewhere)?;
    Ok((row.id.clone(), rel.to_owned()))
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

/// Make one folder inside a session (FR-287).
///
/// `rel` is session-relative and is a **name**, or a path whose last segment is
/// one: the last segment folds to a slug in
/// [`keeper_core::sessions::files::dir_rel`] (`Interview Kit` → `interview-kit`)
/// and the ones in front of it address directories already on the drive. Nothing
/// here composes a name (AD-65) and nothing here composes a zone path — the
/// domain joins the session's own folder onto the folded path.
///
/// **Idempotent.** `MkDir` succeeds on a directory that is already there and
/// creates parents, so a second press changes nothing and `a/b/c` is one plan
/// and one journal row. Unlike the Templates room's twin, this does not refuse a
/// **file** sitting at that path: the executor's `create_dir_all` fails on it and
/// says so, and a pre-flight `is_file` here would be a second answer to a
/// question the write already answers. The room's verb needs one because it also
/// refuses an absent parent, which this deliberately does not.
///
/// **Two fences, both asked.** `files::dir_rel` refuses `workspace/`, traversal,
/// an absolute path and any dotted segment on shape grounds with no knowledge of
/// zones; then this asks
/// [`keeper_sync::files_write::WriteScope::in_session_workspace`] about the
/// profile-relative subpath, which is the fence the product is measured against
/// (AD-113). [`resolve_session_file`] is the same pair for a *file* and cannot be
/// reused: it leads with `check_rel`, which requires one of three extensions, and
/// a folder has none.
///
/// Rejects with: `internal` (unknown root or session, a name that folds to
/// nothing, `workspace/`, a path that leaves the session, a dotted segment, a
/// failed write), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_dir_new(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    rel: String,
) -> Result<(), IpcError> {
    use keeper_core::sessions::files;

    // Folded first, and everything after it is about the folded path: `Workspace`
    // becomes `workspace`, and a fence asked about what was typed would have
    // missed it.
    let rel = files::dir_rel(&rel).map_err(file_verb_error)?;

    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
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

    let compiled = files::compile_dir_new(&row.path, &rel).map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("dir-new task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

/// Make a correctly-named, correctly-tagged file of one kind, **where this
/// session's shape keeps that kind** (FR-277).
///
/// **[`sessions_log_today`]'s twin, not a rival.** That command appends a dated
/// heading to a folder-shaped session's `README.md`, which is where its log
/// lives; a flat session has no `## Log` section to append to, and its log is a
/// *file*. Same verb, same button, two contracts — which is why the frontend
/// picks between them on `detail.shape` rather than offering both, and why a
/// `log` asked of THIS command on a folder-shaped session is refused with a
/// sentence pointing at that one rather than growing a second log writer.
///
/// **Where it writes is the shape's answer, not this command's**
/// ([`keeper_core::sessions::shape::kind_dir`]). A flat session keeps every
/// kind at its root; a folder-shaped one keeps references in `refs/` and
/// prompts in `prompts/`, which are exactly the directories its pool reads back
/// (`crate::sessions_root::read_ref_sources`). Until Story 50.1 this always
/// wrote the root, so on a folder-shaped session the file landed somewhere no
/// space and no *Unfiled* notice could ever see — which is why the surface
/// suppressed the button instead of fixing the write.
///
/// The name is `YYYY-MM-DD-HHMM-<slug>.md` and the tag is written into
/// frontmatter, because those two together are what decide whether the zone's
/// spaces will ever list the file. The directory is *not* the third half of
/// that: [`keeper_core::sessions::pool::read_one`] derives a kind from tags
/// alone (AD-120), so a file in `refs/` without `tags: [ref]` is unfiled no
/// matter which folder it is in.
///
/// Rejects with: `internal` (unknown root or session, an unknown kind tag, a
/// kind this session's shape has no home for, a refused path, a failed write),
/// `unsupported` (mobile).
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
    use keeper_core::sessions::shape::{self, KINDS};

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
    let root = zone_root.join(&row.path);

    // Which contract this session follows, from its own top-level names —
    // `shape()`'s input, and the same listing a name collision is checked
    // against, from ONE read of the directory. The row does not carry the
    // shape and putting it there would make every board row pay for a fact one
    // write verb needs.
    let root_names = taken_in(&root);
    let session_shape = shape::shape(&root_names.iter().cloned().collect::<Vec<_>>());
    // `about` included: the record is one per session under both contracts, and
    // a second one would give `shape()` two answers. The refusal is the
    // domain's sentence rather than one written here, so the rule and the
    // wording it is explained with cannot drift apart.
    let subdir = shape::kind_dir(session_shape, tag).map_err(|no_home| IpcError {
        code: IpcErrorCode::Internal,
        message: no_home.to_string(),
        account_id: None,
        retriable: false,
    })?;
    // The containment rule is asked of the directory in its own right, exactly
    // as `sessions_file_new` asks it of the `parent` it is handed: `check_dir`
    // refuses `workspace/`, traversal and dotfolders, and a rule that only
    // holds because the mapping happened to return a safe constant is not a
    // rule (`files::check_dir`'s own argument). No second guard is written
    // here; this is that one, asked.
    if let Some(subdir) = subdir {
        files::check_dir(subdir).map_err(file_verb_error)?;
    }

    let today = today();
    // The collision set is the DESTINATION's, not the session root's: two
    // references created in the same minute collide with each other, and a
    // stamped name that dodged a root-level file it will never sit beside
    // would be avoiding the wrong collision.
    let taken = match subdir {
        Some(subdir) => taken_in(&root.join(subdir)),
        None => root_names,
    };
    let name = files::new_stamped(&title, &today, &now_hhmm(), &taken);
    let rel = match subdir {
        Some(subdir) => format!("{subdir}/{name}"),
        None => name,
    };

    let (zone_root, session_path, subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;
    let content = files::render_new(
        files::NewFileKind::Markdown,
        Some(tag),
        &title,
        &crate::sync_ipc::new_ulid(),
        &today,
    );
    // `compile_new` leads with `MkDir` whenever `rel` has a parent, so a
    // session whose `refs/` does not exist yet gets it created in the same
    // journaled plan rather than in a step somebody has to remember.
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

/// Rename one session file to follow its title, and rewrite what pointed at it
/// (FR-295, FR-296).
///
/// **One command, because it is one act.** The person changed a title; that the
/// filename derives from it, and that three kinds of pointer name that filename,
/// is keeper's business and not theirs. So the title write, the move and the
/// pointer rewrites are one [`keeper_core::sessions::files::compile_rename`] plan
/// and one journal row — either all of it landed or none of it did (NFR-38).
/// Calling `sync_write_frontmatter` and then a rename would leave a window in
/// which the file says `Kick Off` and is still called `untitled`, which is
/// exactly the *"half of it would be worse than none"* `docs/sessions.md` refused
/// a rename over.
///
/// **The new title is read out of `next_block`, not passed beside it.** The block
/// is the thing being written, so taking the name from anywhere else would let a
/// caller rename a file after a title the file will not carry. `block` is the
/// block the surface was editing, and it is the guard: a concurrent edit to the
/// properties refuses with
/// [`keeper_core::file_properties::PropertiesRefusal::Stale`] and its Re-read
/// sentence, exactly as `sync_write_frontmatter` does (Story 50.4).
///
/// **The pool is the rewrite's scope, and that is a fact about the reader rather
/// than a list kept here.** `session_pool` walks the session's markdown and
/// enters neither `workspace/` nor `artifacts/` nor any dotted directory
/// (`sessions_root::UNSCANNED_DIRS`), so a pointer inside scratch or inside a
/// deliverable is never rewritten — and `files::check_rewritable` refuses both
/// again as the plan compiles, because two predicates that must agree should both
/// run.
///
/// **Addressed by `(profile_id, subpath)`, which is the properties panel's own
/// address.** Story 50.4 made a session file's properties reachable through
/// `(profile id, subpath)` precisely because a session's `README.md` is not a
/// note and has no id; a rename verb that took `(root, session, session-relative
/// path)` instead would be reachable from the row menu and unreachable from the
/// panel, and the panel is where the owner reported this. Splitting the subpath
/// back into a session and a path inside it happens in Rust
/// ([`session_of_subpath`]) rather than in the webview, which is AD-65 in the one
/// direction it had not been asked in yet.
///
/// Answers with the file's new profile-relative subpath, so the caller
/// re-addresses its panel without joining a path (AD-65).
///
/// Rejects with: `internal` (a path in no session of this root, a file that has
/// left the session, a title that names nothing, a collision, a refused path, a
/// stale properties block, a failed write), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_file_rename(
    state: tauri::State<'_, crate::ipc::AppState>,
    profile_id: String,
    subpath: String,
    block: String,
    next_block: String,
) -> Result<String, IpcError> {
    use keeper_core::notes::frontmatter::Frontmatter;
    use keeper_core::sessions::{files, refs};

    let root_id = profile_id;
    let (session_id, rel) = session_of_subpath(&state, &root_id, &subpath)?;
    files::check_rewritable(&rel).map_err(file_verb_error)?;
    // Recomposed rather than trusted, and the recomposition is the round trip's
    // proof: `resolve_session_file` builds the subpath from the profile's zone and
    // the row's path, so a `rel` the split got wrong could not come back as the
    // subpath that was asked about. It is also where the real write fence is asked
    // (AD-113).
    let (zone_root, session_path, subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;

    // Read once, and rewrite from that read: the pool carries every file's bytes,
    // which is both what the title splice needs and what every pointer rewrite
    // needs. A second read per pointer file would be a second answer to "what
    // does this session say now".
    let pool = crate::sessions_root::session_pool(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let text = pool
        .files
        .iter()
        .find(|(candidate, _, _)| *candidate == rel)
        .map(|(_, text, _)| text.as_str())
        .ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "{rel} is not in this session any more — someone moved or deleted it while its \
                 properties were open. Reopen the session to see where its files are now."
            ),
            account_id: None,
            retriable: false,
        })?;

    // The title write first, so the rewrite below sees the offsets it will
    // actually be written at: the splice changes the frontmatter's length, and a
    // pointer rewrite computed against the pre-splice bytes would carry spans
    // into a body that had moved.
    let titled = keeper_core::file_properties::replace_block(text, &block, &next_block, &subpath)
        .map_err(properties_refusal)?;
    let (fm, _body_at) = Frontmatter::parse(&next_block);
    let title = fm.as_string("title").unwrap_or_default();

    // Where it lands. The record and the contract file keep their names whatever
    // their title says — `shape()` reads those names — so `renames` is asked and
    // the title still gets written either way.
    let to = if files::renames(&rel) {
        // The collision set is the DESTINATION folder's, which for a rename is
        // the folder the file is already in — a retitle does not move a file
        // between directories, because a directory is not something a title says.
        // Split on `/` rather than through `Path`: `rel` is a session-relative,
        // `/`-joined logical path, and `Path`'s separators are the platform's.
        let dir = rel.rsplit_once('/').map_or("", |(dir, _)| dir);
        let taken = taken_in(&zone_root.join(&session_path).join(dir));
        files::rename_target(&rel, title, &taken).map_err(file_verb_error)?
    } else {
        rel.clone()
    };
    // The destination goes past the real write fence too, not only past the
    // domain's containment rule: `resolve_session_file` is where
    // `WriteScope::in_session_workspace` is asked (AD-113), and a rename has two
    // ends.
    let (_, _, new_subpath) = resolve_session_file(&state, &root_id, &session_id, &to)?;

    // The renamed file's own bytes carry both edits when it also pointed at
    // itself; every other pool file carries only the rewrite, and a file with no
    // pointer to it is left out of the plan rather than written back unchanged.
    let mut rewrites = vec![files::Rewrite {
        rel: rel.clone(),
        expect_len: text.len(),
        content: refs::rewrite_pointers(&titled, &rel, &to).unwrap_or(titled),
    }];
    rewrites.extend(pool.files.iter().filter_map(|(candidate, body, _)| {
        if *candidate == rel {
            return None;
        }
        refs::rewrite_pointers(body, &rel, &to).map(|content| files::Rewrite {
            rel: candidate.clone(),
            expect_len: body.len(),
            content,
        })
    }));

    let compiled =
        files::compile_rename(&session_path, &rel, &to, &rewrites).map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("file-rename task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(new_subpath)
}

/// Where one file of this zone is on this machine, absolute — the argument
/// *Reveal in Finder* and *Copy path* take (FR-297).
///
/// **Asked when the verb runs, rather than carried on every row.** AD-65 forbids
/// the webview joining a path, and
/// [`keeper_core::sessions::vm::SessionSpaceFileVm`] carries the profile-relative
/// `subpath` that *opens* a file and nothing more. Widening every row of every
/// space with an absolute path — a string nine rows in ten will never be asked
/// for — to serve two items in a menu is the wrong trade; so is reading it out of
/// [`sessions_tree`], which would pay for a whole tree walk and a sync-engine
/// pending query per right-click.
///
/// **Resolved rather than joined**, through the same `browse::resolve` every read
/// on this side of the app goes through: it refuses a subpath that leaves the
/// profile and answers `None` for a file that is not there, so *Reveal* cannot be
/// handed a location that does not exist. A `local_path.join(subpath)` would have
/// been shorter and would have said yes to both.
///
/// Here rather than in `sync_ipc` because its caller and its wording are this
/// surface's: a session space row is the only thing that asks, and the day a
/// second surface needs it, moving it is a rename.
///
/// Rejects with: `internal` (unknown profile, a path that leaves it, a file that
/// is gone), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_file_path(
    state: tauri::State<'_, crate::ipc::AppState>,
    profile_id: String,
    subpath: String,
) -> Result<String, IpcError> {
    let profile = crate::sync_ipc::sessions_profile(&state, &profile_id)?;
    let gone = || IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "{subpath} is not on this disk any more, so there is no location to show. Reopen \
             the session to see what it holds now."
        ),
        account_id: None,
        retriable: false,
    };
    let resolved = keeper_sync::browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| IpcError {
            code: IpcErrorCode::Internal,
            message: refusal.to_string(),
            account_id: None,
            retriable: false,
        })?
        .ok_or_else(gone)?;
    Ok(resolved.to_string_lossy().into_owned())
}

/// Move one task card: which column it lands in, and where in that column.
///
/// `status` is one of the four the closed [`TaskStatus`] set names, and `index`
/// is the position among the cards **already in that column, with the moved card
/// removed** — `0` is the top, past the end is the bottom. The frontend sends
/// the index it rendered rather than two neighbour ids, because the column it is
/// looking at is the column the operator dropped into; resolving that to a
/// number is [`keeper_core::sessions::tasks::compile_move`]'s job (AD-65).
///
/// **The column is re-read here, not trusted from the drag.** A board that has
/// been open for ten minutes is a board an agent has had ten minutes to write
/// tasks into, and placing a card between two neighbours that have since moved
/// is how a drop lands somewhere nobody chose. The read is the same
/// [`crate::sessions_root::session_pool`] scan the spaces list already does.
///
/// Rejects with: `internal` (unknown root or session, an unknown status, a card
/// that is not in the pool, a refused path, a failed write), `unsupported`
/// (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_task_move(
    root_id: String,
    session_id: String,
    rel: String,
    status: String,
    index: u32,
) -> Result<(), IpcError> {
    use keeper_core::sessions::pool::{read_pool, PoolFile};
    use keeper_core::sessions::shape::TaskStatus;
    use keeper_core::sessions::tasks::{self, TaskFile};

    let status = TaskStatus::parse(&status).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "{status} is not one of this board's columns. A card's status is one of \
             in-preparation, todo, done or deferred — a fifth column would be a column no \
             session can show."
        ),
        account_id: None,
        retriable: false,
    })?;

    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let pool_read =
        crate::sessions_root::session_pool(&root_id, &session_id).ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("no such session: {session_id}"),
            account_id: None,
            retriable: false,
        })?;
    let pool = read_pool(
        &pool_read
            .files
            .iter()
            .map(|(rel, text, _)| PoolFile { rel, text })
            .collect::<Vec<_>>(),
    );

    // The moved card's own bytes, from the same read: the write splices against
    // what is on disk now, and a text the frontend still held would silently
    // revert whatever was edited in the meantime (FR-121 is about preserving
    // bytes, which means the *current* ones).
    let text = pool_read
        .files
        .iter()
        .find(|(candidate, _, _)| *candidate == rel)
        .map(|(_, text, _)| text.as_str())
        .ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!(
                "{rel} is not in this session any more — someone moved or deleted it while the \
                 board was open. Reopen the session to see where its cards are now."
            ),
            account_id: None,
            retriable: false,
        })?;

    // The target column as it stands, in rendered order, without the card being
    // moved — which is exactly what `compile_move` documents as its `column`.
    let column: Vec<TaskFile<'_>> = pool
        .tasks
        .iter()
        .filter(|entry| entry.status == Some(status) && entry.rel != rel)
        .map(|entry| TaskFile {
            rel: &entry.rel,
            text: pool_read
                .files
                .iter()
                .find(|(candidate, _, _)| *candidate == entry.rel)
                .map_or("", |(_, text, _)| text.as_str()),
            order: entry.order.value,
        })
        .collect();

    let compiled =
        tasks::compile_move(&pool_read.path, &rel, text, status, &column, index as usize)
            .map_err(file_verb_error)?;
    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("task-move task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_task_move(
    root_id: String,
    session_id: String,
    rel: String,
    status: String,
    index: u32,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, rel, status, index);
    Err(unsupported())
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
pub fn sessions_dir_new(root_id: String, session_id: String, rel: String) -> Result<(), IpcError> {
    let _ = (root_id, session_id, rel);
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

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_file_rename(
    profile_id: String,
    subpath: String,
    block: String,
    next_block: String,
) -> Result<String, IpcError> {
    let _ = (profile_id, subpath, block, next_block);
    Err(unsupported())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_file_path(profile_id: String, subpath: String) -> Result<String, IpcError> {
    let _ = (profile_id, subpath);
    Err(unsupported())
}

// ---------------------------------------------------------------------------
// Adding a reference (FR-265, AD-118)
// ---------------------------------------------------------------------------

/// How many candidates one picker call will return.
///
/// A vault holds tens of thousands of notes and a session's `workspace/` can
/// hold a checkout. The picker is a list somebody scrolls, so the honest design
/// is a bounded list plus a `truncated` flag that says so — [`sessions_tree`]'s
/// rule, and the reason `query` is filtered in Rust rather than in React: a
/// frontend filtering a prefix of the vault would be filtering the wrong 500.
#[cfg(desktop)]
const CANDIDATE_BUDGET: usize = 500;

/// Everything the operator could reference from this session (FR-265).
///
/// Three sources, one list, ordered session-files-first: a reference is most
/// often to something the sitting just produced, and a picker that opens on the
/// vault's oldest note is a picker that opens on the wrong answer.
///
/// **`query` is applied here, not in the webview.** Filtering after truncation
/// would search a prefix, and a `tag:` term is the tag hierarchy's question — it
/// belongs beside the index that answers it (AD-7, AD-65).
///
/// **The workspace fence is asked, not guessed.** Whether a candidate is
/// promotable comes from the same [`keeper_sync::files_write::WriteScope`] the
/// write path enforces, so the offer cannot appear on a file keeper would then
/// refuse to copy.
///
/// Rejects with: `internal` (unknown root or session), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_ref_candidates(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    query: String,
) -> Result<keeper_core::sessions::vm::SessionRefCandidatesVm, IpcError> {
    use keeper_core::sessions::add_ref::{self, DEFAULT_REF_FILE};
    use keeper_core::sessions::pool::{read_pool, PoolFile};
    use keeper_core::sessions::refs::RefKind;
    use keeper_core::sessions::shape::{self, KindTag};
    use keeper_core::sessions::vm::{SessionRefCandidateVm, SessionRefCandidatesVm};

    // Asked for its refusal as well as its path: every reader below degrades to
    // an empty list for an unknown session, and an empty picker is a worse
    // answer than "no such session" for the one case that is actually a bug.
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();
    let (_vault, scope) = crate::sync_ipc::sessions_scope(&profile);

    let query = query.trim();
    let mut candidates: Vec<SessionRefCandidateVm> = Vec::new();
    let mut truncated = false;

    // The session's own files first, in the tree's own order — which already
    // puts `artifacts/` before `workspace/`, so the promoted output a sitting
    // meant to reference is above the scratch it happened to leave behind.
    if let Some((session_path, entries, tree_truncated)) =
        crate::sessions_root::tree(&root_id, &session_id)
    {
        truncated |= tree_truncated;
        for entry in entries.iter().filter(|entry| !entry.is_dir) {
            let subpath = format!("{zone}/{session_path}/{}", entry.rel_path);
            let label = entry.name.clone();
            if !add_ref::matches(query, &label, &entry.rel_path, &[]) {
                continue;
            }
            if candidates.len() >= CANDIDATE_BUDGET {
                truncated = true;
                break;
            }
            candidates.push(SessionRefCandidateVm {
                kind: RefKind::File.as_str().to_owned(),
                target: subpath.clone(),
                label,
                detail: entry.rel_path.clone(),
                tags: Vec::new(),
                mtime_ms: entry.mtime_ms,
                promotable: scope.in_session_workspace(&subpath),
            });
        }
    }

    // Then the vault, newest first. A profile that is not also a vault answers
    // nothing here, which is the honest answer rather than an error (AD-90).
    if let Some(snapshot) = crate::notes_vault::snapshot(&profile.id) {
        let mut notes: Vec<&keeper_core::notes::index::IndexEntry> = snapshot
            .entries()
            .iter()
            .filter(|entry| {
                let folder = entry.path.rsplit_once('/').map_or("", |(dir, _)| dir);
                add_ref::matches(query, &entry.title, folder, &entry.tags)
            })
            .collect();
        notes.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.path.cmp(&b.path)));
        for entry in notes {
            if candidates.len() >= CANDIDATE_BUDGET {
                truncated = true;
                break;
            }
            let recording = entry.flags.iter().any(|flag| flag == "recording");
            candidates.push(SessionRefCandidateVm {
                kind: if recording {
                    RefKind::Recording.as_str().to_owned()
                } else {
                    RefKind::Note.as_str().to_owned()
                },
                // The title, because a wikilink addresses a note by name — which
                // is what makes the written reference survive the note moving.
                target: entry.title.clone(),
                label: entry.title.clone(),
                detail: entry.path.clone(),
                tags: entry.tags.clone(),
                mtime_ms: entry.updated_ms,
                // A note is not in the session's workspace by construction: a
                // zone and a vault cannot overlap (`SessionsConfig`'s own rule).
                promotable: false,
            });
        }
    }

    // Where a reference could go: the session's `ref`-tagged markdown first,
    // then its other markdown. Composed in Rust so the picker offers a list and
    // never a path it built itself (AD-65).
    let targets = match crate::sessions_root::session_pool(&root_id, &session_id) {
        Some(pool_read) => {
            let pool = read_pool(
                &pool_read
                    .files
                    .iter()
                    .map(|(rel, text, _)| PoolFile { rel, text })
                    .collect::<Vec<_>>(),
            );
            let mut targets: Vec<String> =
                pool.refs.iter().map(|entry| entry.rel.clone()).collect();
            for entry in pool
                .about
                .iter()
                .chain(pool.prompts.iter())
                .chain(pool.logs.iter())
                .chain(pool.unfiled.iter())
            {
                if !targets.contains(&entry.rel) {
                    targets.push(entry.rel.clone());
                }
            }
            targets
        }
        None => Vec::new(),
    };

    // The default is the constant whether or not the file exists: an existing
    // references file is the file the operator meant, and dodging it into
    // `references-2.md` would split one list in two. `new_named`'s collision
    // rule is for files a *title* names; this is a fixed name that keeps
    // accumulating.
    //
    // WHERE that constant sits is the shape's answer, from the same mapping a
    // space's create asks (Story 50.1, FR-279). Until then this was the bare
    // name under both contracts, so on a folder-shaped session `Add reference`
    // wrote `references.md` into the session ROOT — a real file, on the
    // operator's disk, that the folder pool never reads
    // (`sessions_root::read_ref_sources` takes `README.md` by name and then
    // walks `refs/` and `prompts/`), invisible to every space and to the
    // *Unfiled* notice alike. A write into a blind spot, and the very failure
    // the spaces surface had suppressed its own create button to avoid.
    //
    // `Ref` has a home under both contracts, so the refusal arm is unreachable
    // today; the session root is the honest answer if that ever changes,
    // because this command only OFFERS a destination and `sessions_ref_add`
    // re-checks whatever it is handed.
    let top_level: Vec<String> = taken_in(&zone_root.join(&row.path)).into_iter().collect();
    let default_target = match shape::kind_dir(shape::shape(&top_level), KindTag::Ref) {
        Ok(Some(dir)) => format!("{dir}/{DEFAULT_REF_FILE}"),
        Ok(None) | Err(_) => DEFAULT_REF_FILE.to_owned(),
    };

    Ok(SessionRefCandidatesVm {
        candidates,
        targets,
        default_target,
        truncated,
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_ref_candidates(
    root_id: String,
    session_id: String,
    query: String,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, query);
    Err(unsupported())
}

/// Write one reference into one of the session's markdown files (FR-265).
///
/// The line is composed in Rust — the syntax a reference is written in is the
/// syntax [`keeper_core::sessions::refs::scan`] reads back, and a frontend
/// composing markdown would be the second author of that contract (AD-65).
///
/// **Guarded, because an agent may be writing the same file.** The target's
/// current bytes are read here and the plan's write refuses if they changed
/// underneath it, which turns a race into a retry rather than a lost line.
///
/// Rejects with: `internal` (unknown root or session, a refused pick, a target
/// that is not markdown, a failed write), `unsupported` (mobile).
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_ref_add(
    state: tauri::State<'_, crate::ipc::AppState>,
    root_id: String,
    session_id: String,
    req: keeper_core::sessions::vm::SessionRefAddReq,
) -> Result<keeper_core::sessions::vm::SessionRefAddedVm, IpcError> {
    use keeper_core::sessions::add_ref::{self, Pick};
    use keeper_core::sessions::vm::SessionRefAddedVm;

    let pick = Pick::parse(&req.kind, &req.target).map_err(add_ref_error)?;

    let zone_root = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let row = crate::sessions_root::row_of(&root_id, &session_id)
        .ok_or_else(|| session_error(&session_id))?;
    let profile = crate::sync_ipc::sessions_profile(&state, &root_id)?;
    let zone = profile
        .sessions
        .as_ref()
        .map(|sessions| sessions.subfolder.trim().to_owned())
        .unwrap_or_default();

    // The promotion is computed against what `artifacts/` holds *now*, not
    // against a name the picker precomputed: the list may be minutes old and a
    // stale destination is how two promotions land on one file.
    let promotion = match (&pick, req.promote) {
        (Pick::Path { subpath }, true) => add_ref::promotion(
            &zone,
            &row.path,
            subpath,
            &taken_in(&zone_root.join(&row.path).join("artifacts")),
        ),
        _ => None,
    };

    let line = add_ref::line(
        &zone,
        &row.path,
        &pick,
        req.label.as_deref(),
        promotion.as_ref(),
    )
    .map_err(add_ref_error)?;

    let rel = req.file.trim().to_owned();
    let (zone_root, session_path, _subpath) =
        resolve_session_file(&state, &root_id, &session_id, &rel)?;

    // Read the target's current bytes, or `None` when it does not exist yet —
    // which is the create case and the only one that is not guarded.
    let existing = std::fs::read_to_string(zone_root.join(&session_path).join(&rel)).ok();
    let (existing, line_written) = match existing {
        Some(text) => (Some(text), line.clone()),
        // A brand-new references file is seeded with frontmatter and the `ref`
        // tag, so the References space lists it the moment it lands rather than
        // after somebody notices it is untagged.
        //
        // The DESTINATION, not a title composed here. `seeded` folds the path
        // down to its basename, and it does so in the domain because the rule
        // is about what `render_new` writes: until Story 50.1's review this
        // call handed it `refs/references.md` minus the extension and the new
        // file was titled `refs/references` — in its frontmatter, in its H1,
        // and therefore in every space that lists it.
        None => (
            None,
            add_ref::seeded(&rel, &crate::sync_ipc::new_ulid(), &today(), &line),
        ),
    };

    let compiled = add_ref::compile_add(
        &session_path,
        &rel,
        existing.as_deref(),
        &line_written,
        promotion.as_ref(),
    )
    .map_err(add_ref_error)?;

    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone_root, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("ref-add task failed: {join}"),
            account_id: None,
            retriable: false,
        })?
        .map_err(exec_error)?;
    crate::sessions_root::rescan(&root_id);

    Ok(SessionRefAddedVm {
        file: rel,
        line,
        promoted: promotion.map(|promotion| promotion.rel),
    })
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_ref_add(
    root_id: String,
    session_id: String,
    req: keeper_core::sessions::vm::SessionRefAddReq,
) -> Result<(), IpcError> {
    let _ = (root_id, session_id, req);
    Err(unsupported())
}

/// One refusal, in the domain's own words (UX-DR43).
#[cfg(desktop)]
fn add_ref_error(error: keeper_core::sessions::add_ref::AddRefError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.message(),
        account_id: None,
        retriable: false,
    }
}

// ---------------------------------------------------------------------------
// Search (FR-267)
// ---------------------------------------------------------------------------

/// Live zone scans, keyed by root — at most one per root, because a second
/// scan of the same zone is always a newer query for the same field.
///
/// Aborting the previous one is the point: notes leaves its superseded scan
/// running and relies on the client to ignore late batches, which works but
/// spends a whole zone read on results nobody will ever see. Here the older
/// task is dropped, and dropping a [`ScanTask`] aborts it.
#[cfg(desktop)]
fn scans() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, ScanTask>> {
    use std::sync::{Mutex, OnceLock};
    static SCANS: OnceLock<Mutex<std::collections::HashMap<String, ScanTask>>> = OnceLock::new();
    SCANS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        // A poisoned lock means a scan panicked mid-walk. The map holds join
        // handles and nothing else, so there is no torn state to protect and
        // refusing every later search would be the worse failure.
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One running scan. Dropping it aborts the task, which is what makes
/// supersession free rather than cooperative.
#[cfg(desktop)]
struct ScanTask {
    id: String,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[cfg(desktop)]
impl Drop for ScanTask {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// How many hits accumulate before a batch is flushed to the client.
///
/// Small enough that the first results appear while the walk is still going —
/// which is the whole reason this streams rather than returning a `Vec` — and
/// large enough that a zone of short files does not become one message per hit.
#[cfg(desktop)]
const SEARCH_BATCH: usize = 20;

/// Bounded content scan across one zone, streamed as found (FR-267).
///
/// The sessions twin of `notes_search` (AD-114), and a separate command rather
/// than a flag on that one because the two read different folders: a subfolder
/// that is both a vault and a zone is refused at profile validation, so no id
/// can name both and no single scan could serve both.
///
/// Returns the scan's id. Any scan already running for this root is aborted
/// first, so a fast typist funds one walk rather than one per keystroke.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_search(
    root_id: String,
    req: keeper_core::sessions::vm::SessionSearchReq,
    channel: tauri::ipc::Channel<keeper_core::sessions::vm::SessionSearchBatch>,
) -> Result<String, IpcError> {
    if !crate::sessions_root::known(&root_id) {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!("no such sessions root: {root_id}"),
            account_id: None,
            retriable: false,
        });
    }
    let id = crate::sync_ipc::new_ulid();
    let scan_id = id.clone();
    let scan_root = root_id.clone();
    let task = tauri::async_runtime::spawn(async move {
        run_zone_search(&scan_root, &req.text, req.limit as usize, &channel).await;
        // Retire self, but only if a newer scan has not already taken the slot
        // — otherwise a finishing scan would abort its own successor on drop.
        let mut live = scans();
        if live.get(&scan_root).is_some_and(|held| held.id == scan_id) {
            // The task is finished, so this drop aborts nothing.
            live.remove(&scan_root);
        }
    });
    // Inserting replaces — and dropping the replaced value aborts it.
    scans().insert(
        root_id,
        ScanTask {
            id: id.clone(),
            task,
        },
    );
    Ok(id)
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_search(
    root_id: String,
    req: keeper_core::sessions::vm::SessionSearchReq,
) -> Result<String, IpcError> {
    let _ = (root_id, req);
    Err(unsupported())
}

/// Stop a scan by id. An id that names nothing — already finished, or already
/// superseded — is a no-op, so a racing unmount is not an error.
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_search_cancel(subscription_id: String) -> Result<(), IpcError> {
    let mut live = scans();
    let holder = live
        .iter()
        .find(|(_, held)| held.id == subscription_id)
        .map(|(root, _)| root.clone());
    if let Some(root) = holder {
        live.remove(&root);
    }
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_search_cancel(subscription_id: String) -> Result<(), IpcError> {
    let _ = subscription_id;
    Err(unsupported())
}

/// The walk behind [`sessions_search`]: every session of the zone, newest
/// first, every markdown file of each, in name order.
///
/// Reuses `session_pool` — the same read the spaces evaluator uses — rather
/// than walking the zone itself, so "the files of a session" has one definition
/// and a file the board cannot see is a file search cannot find either.
#[cfg(desktop)]
async fn run_zone_search(
    root_id: &str,
    needle: &str,
    limit: usize,
    channel: &tauri::ipc::Channel<keeper_core::sessions::vm::SessionSearchBatch>,
) {
    use keeper_core::sessions::search;
    use keeper_core::sessions::vm::SessionSearchBatch;

    if !search::searchable(needle) {
        let _ = channel.send(SessionSearchBatch {
            done: true,
            hits: Vec::new(),
        });
        return;
    }
    let Some(rows) = crate::sessions_root::rows(root_id) else {
        // Cold zone: the scan has not landed yet. An empty done-batch says "no
        // results" honestly; the client re-searches when the changed event
        // lands, exactly as every other sessions surface does.
        let _ = channel.send(SessionSearchBatch {
            done: true,
            hits: Vec::new(),
        });
        return;
    };
    let Some(zone) = crate::sessions_root::subfolder_of(root_id) else {
        let _ = channel.send(SessionSearchBatch {
            done: true,
            hits: Vec::new(),
        });
        return;
    };
    let mut scan = search::Scan::new(limit);
    for row in rows.iter() {
        if scan.exhausted() {
            break;
        }
        let Some(pool) = crate::sessions_root::session_pool(root_id, &row.id) else {
            continue;
        };
        // `<zone>/<session path>` — the same prefix every other session row's
        // `subpath` is built from, so a hit opens through the one file target.
        let prefix = format!("{zone}/{}", pool.path);
        let session = search::Session {
            id: &row.id,
            title: &row.title,
            prefix: &prefix,
        };
        for (rel, text, _mtime) in &pool.files {
            scan.push_file(session, rel, text, needle);
            if scan.exhausted() {
                break;
            }
        }
        if scan.pending() >= SEARCH_BATCH
            && channel
                .send(SessionSearchBatch {
                    done: false,
                    hits: scan.take(),
                })
                .is_err()
        {
            // The client is gone. Nothing left to send it to.
            return;
        }
        // One session's worth of blocking reads, then yield: a zone of forty
        // sessions must not starve the runtime it shares with the editor.
        tokio::task::yield_now().await;
    }
    let _ = channel.send(SessionSearchBatch {
        done: true,
        hits: scan.take(),
    });
}

/// The two name helpers, which are pure `&str -> Result<String>` and therefore
/// the only part of this module a test can reach without a zone, a registry and
/// a Tauri handle. They are also where the story's refusals are decided, so the
/// matrix rows about names live here rather than in a UI test that can only see
/// the sentence.
#[cfg(all(test, desktop))]
mod tests {
    use super::{template_at, template_mint};

    /// Row 5's naming half: what a person types becomes keeper's folding of it,
    /// which is the spelling `sessions_patterns` will answer with afterwards.
    #[test]
    fn a_typed_name_is_minted_as_keepers_own_folding() {
        assert_eq!(
            template_mint(Some("Kick Off")).expect("a name with letters mints"),
            "_template/kick-off"
        );
        assert_eq!(
            template_mint(Some("  Kick Off  ")).expect("surrounding space is trimmed, not refused"),
            "_template/kick-off"
        );
        // A legal interior dot survives the fold as a separator, which is what
        // makes `v1.2` a template name rather than a refusal.
        assert_eq!(
            template_mint(Some("v1.2")).expect("an interior dot is a legal name"),
            "_template/v1-2"
        );
    }

    /// Row 6, and the bug it was written for: `naming::slug` answers `untitled`
    /// for a name with nothing in it, so a guard that read the *slug* never
    /// fired and `###` minted `_template/untitled` — a directory the operator
    /// never typed, which rename then moved their template into.
    #[test]
    fn a_name_with_no_alphanumerics_is_refused_rather_than_called_untitled() {
        let refusal = template_mint(Some("###")).expect_err("### is not a folder name");
        assert_eq!(
            refusal.message,
            "\"###\" has nothing in it a folder can be named after — a named template needs \
             letters or digits."
        );
        assert!(
            !refusal.retriable,
            "a re-typed name is a new call, not a retry"
        );
        assert!(template_mint(Some("🎉")).is_err());
    }

    /// Row 8's Rust half. An absent name is the zone's own `_template/` on both
    /// helpers — that is the rule install is built on — which is exactly why
    /// `sessions_template_rename` refuses an empty `name` and an empty
    /// `new_name` of its own before it asks either of these: to a rename, "the
    /// zone contract" is not a name, it is the thing that must not be moved.
    #[test]
    fn an_absent_name_is_the_zones_own_template_on_both_helpers() {
        assert_eq!(
            template_mint(None).expect("no name is the zone template"),
            "_template"
        );
        assert_eq!(
            template_at(None).expect("no name is the zone template"),
            "_template"
        );
        assert_eq!(
            template_mint(Some("   ")).expect("whitespace trims to no name"),
            "_template"
        );
        assert_eq!(
            template_at(Some("   ")).expect("whitespace trims to no name"),
            "_template"
        );
    }

    /// Addressing is verbatim: a template the operator made by hand is found
    /// under the name they gave it, because slugging the address would send the
    /// read to an empty room — or, if both spellings existed, move the wrong
    /// directory.
    #[test]
    fn an_existing_name_is_addressed_exactly_as_it_is_on_disk() {
        assert_eq!(
            template_at(Some("Interview Kit")).expect("a hand-made name is addressed verbatim"),
            "_template/Interview Kit"
        );
        assert_eq!(
            template_at(Some("v1.2")).expect("an interior dot addresses"),
            "_template/v1.2"
        );
    }

    /// A name from the webview is joined onto the zone root, so the traversal
    /// cases are refused before anything is opened.
    #[test]
    fn a_name_that_could_escape_the_template_directory_is_refused() {
        for name in ["..", ".", "a/b", "a\\b", "_house", ".hidden"] {
            assert!(
                template_at(Some(name)).is_err(),
                "{name} must not address a directory under _template/"
            );
        }
    }

    /// The refusal has to describe the rule `pattern::safe_segment` actually
    /// has. It used to promise "no dots" while accepting `v1.2`, which teaches
    /// the operator to avoid names keeper takes.
    #[test]
    fn the_addressing_refusal_states_the_rule_it_actually_enforces() {
        let refusal = template_at(Some("..")).expect_err(".. is not addressable");
        assert!(
            refusal
                .message
                .contains("does not begin with a dot or an underscore"),
            "the sentence must name the leading-character rule: {}",
            refusal.message
        );
        assert!(
            !refusal.message.contains("no dots"),
            "an interior dot is legal; the sentence must not forbid it: {}",
            refusal.message
        );
    }
}
