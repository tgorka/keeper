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

/// The `(template-relative path, is_dir)` facts a template copy needs, walked
/// shallowly enough for a skeleton (depth-capped at the template's own shape).
#[cfg(desktop)]
fn template_files(zone: &std::path::Path) -> Vec<(String, bool)> {
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
    walk(&zone.join("_template"), "", &mut out);
    out
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

/// Create a session from the zone's `_template/` (FR-238): one question in
/// (the title), one folder out — copied verbatim, README stamped with the
/// date line and a minted id, and the caret's home returned as the ref.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_create(
    root_id: String,
    title: String,
) -> Result<keeper_core::sessions::vm::SessionRefVm, IpcError> {
    use keeper_core::sessions::{model, plan};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let title = title.trim().to_owned();
    let date = today();
    let dir_name = model::session_dir_name(&title, &date, &taken_names(&zone));
    let id = crate::sync_ipc::new_ulid();
    // The stamped README: template prose replaced by the canonical skeleton
    // with the title and date in place. The template's own README stays the
    // pattern for what sections exist — read it, keep its headings.
    let template_readme = std::fs::read_to_string(zone.join("_template/README.md"))
        .unwrap_or_else(|_| "# <session title>\n\n## Summary\n\n## Log\n\n## Promote\n\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n".to_owned());
    let body = plan::skeleton_from(&template_readme, &title, &date);
    let readme = format!("---\nid: {id}\ncreated: {date}\n---\n{body}");
    let compiled = plan::compile_create(&dir_name, &template_files(&zone), &readme);
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
pub fn sessions_create(root_id: String, title: String) -> Result<(), IpcError> {
    let _ = (root_id, title);
    Err(unsupported())
}

/// Create a session continuing another (FR-239, AD-112): structure-only copy
/// of the source's shape, lineage written on BOTH ends — including into an
/// archived source, because files are truth.
#[cfg(desktop)]
#[tauri::command]
pub async fn sessions_create_from(
    root_id: String,
    source_id: String,
    title: String,
) -> Result<keeper_core::sessions::vm::SessionRefVm, IpcError> {
    use keeper_core::sessions::{model, plan};

    let zone = crate::sessions_root::zone_of(&root_id).ok_or_else(|| root_error(&root_id))?;
    let source = crate::sessions_root::row_of(&root_id, &source_id).ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such session: {source_id}"),
        account_id: None,
        retriable: false,
    })?;
    let title = title.trim().to_owned();
    let date = today();
    let dir_name = model::session_dir_name(&title, &date, &taken_names(&zone));
    let id = crate::sync_ipc::new_ulid();

    let source_dir = zone.join(&source.path);
    let source_readme = std::fs::read_to_string(source_dir.join("README.md")).unwrap_or_default();
    let (_, body_at) = keeper_core::notes::frontmatter::Frontmatter::parse(&source_readme);
    let skeleton = plan::skeleton_from(&source_readme[body_at..], &title, &date);
    // continues: baked into the new README's frontmatter at birth (AD-112).
    let readme = format!(
        "---\nid: {id}\ncreated: {date}\nkeeper:\n  session-continues: [{source_id}]\n---\n{skeleton}"
    );

    // The structural copy: the source's prompts and ref pointers, walked here
    // and filtered by the pure rule.
    let mut source_files = Vec::new();
    {
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
        walk(&source_dir, "", &mut source_files);
    }
    let copies_rel = plan::pattern_copies(&source_files);
    // pattern copies name paths relative to the SOURCE session; the compile
    // function copies from `_template/` — so re-point the copy sources by
    // building the plan by hand from compile_create's shape, with the source
    // session as the origin for the copied files.
    let mut compiled = plan::compile_create(&dir_name, &copies_rel, &readme);
    for step in &mut compiled.steps {
        if let keeper_core::sessions::plan::PlanStep::CopyFile { from, .. } = step {
            if let Some(rest) = from.strip_prefix("_template/") {
                *from = format!("{}/{rest}", source.path);
            }
        }
    }
    let with_lineage =
        plan::compile_create_from(&dir_name, &source.path, &source_readme, &id, &[], &readme);
    // Take the guarded source-side lineage write from the canonical compile
    // and append it to the re-pointed structural plan.
    if let Some(lineage_step) = with_lineage.steps.last().cloned() {
        compiled.steps.push(lineage_step);
    }
    compiled.verb = "create-from".to_owned();
    let session_path = compiled.session.clone();

    tauri::async_runtime::spawn_blocking(move || crate::sessions_exec::run(&zone, compiled))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("create-from task failed: {join}"),
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
pub fn sessions_create_from(
    root_id: String,
    source_id: String,
    title: String,
) -> Result<(), IpcError> {
    let _ = (root_id, source_id, title);
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
