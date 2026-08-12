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
