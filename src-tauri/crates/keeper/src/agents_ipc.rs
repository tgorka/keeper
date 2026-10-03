//! The person's proxy beside the notes view (story 91.2, AD-382): the dock's
//! commands. On every target — the phone shows the proxy's rooms too (P5:
//! it is never a host, which is `agents_host`'s, desktop only).
//!
//! It decides nothing: which rooms are the proxy's, what a scope or a focus
//! event says and when it goes out are `keeper_core::agents::proxy`'s and
//! `AccountManager`'s; which drive and path a note is, `keeper_agent::surface`'s.

use keeper_agent::surface::drive_of;
use keeper_core::agents::events::Focus;
use keeper_core::agents::proxy::{AgentFocusReq, ProxyRoomVm};
use keeper_core::notes::outline::{heading_at, heading_in};
use keeper_core::vm::IpcError;
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// The person's proxy conversations on `account_id`, the DM first; each with
/// the drives its scope chip may offer where the proxy's agents zone is on
/// this device.
#[tauri::command]
pub async fn agent_rooms_list(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<ProxyRoomVm>, IpcError> {
    Ok(state.accounts.agent_rooms(&account_id).await)
}

/// Ask the proxy in `room_id` for `drives` in scope (the scope chip).
#[tauri::command]
pub async fn agent_scope_set(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    drives: Vec<String>,
) -> Result<(), IpcError> {
    state
        .accounts
        .agent_scope_set(&account_id, &room_id, drives)
        .await
        .map_err(to_ipc_error)
}

/// The docked note changed (`focus`), or the dock closed (`None`); `seq`
/// orders the webview's calls, so a focus that arrives after a later close
/// is dropped. The note is named only once it has been still a second
/// (`keeper_core::agents::focus`); a note in a folder that declares no
/// drive is, to the proxy, no note: it is sent as none.
#[tauri::command]
pub async fn agent_focus(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    seq: u64,
    focus: Option<AgentFocusReq>,
) -> Result<(), IpcError> {
    match focus {
        Some(req) => {
            let platform = std::sync::Arc::clone(&state.platform);
            state
                .accounts
                .agent_focus(&account_id, &room_id, seq, move || {
                    name_focus(platform, &req)
                })
                .await
        }
        None => {
            state
                .accounts
                .agent_focus_close(&account_id, &room_id, seq)
                .await
        }
    }
    .map_err(to_ipc_error)
}

/// The docked note by its drive: the vault's folder, the note's path in it
/// from the vault's index, and the heading above the caret from the
/// editor's buffer as the webview sent it — or, with no editor (Preview),
/// from the note on disk. Blocking: the engine's profiles and the note's
/// file.
fn name_focus(
    platform: std::sync::Arc<dyn keeper_core::platform::Platform>,
    req: &AgentFocusReq,
) -> Option<Focus> {
    let vault = crate::notes_vault::vault(&req.vault_id)?;
    let entry = crate::notes_vault::snapshot(&req.vault_id)?
        .by_id(&req.note_id)?
        .clone();
    let profile = crate::sync::engine(platform)
        .ok()?
        .list_profiles()
        .ok()?
        .into_iter()
        .find(|profile| profile.id == vault.id)?;
    let named = drive_of(&profile, &vault.config.subfolder, &entry.path)?;
    let heading = match &req.text {
        Some(text) => heading_in(text, req.line),
        None => crate::notes_vault::read_note(&vault, &entry.path)
            .ok()
            .and_then(|text| heading_at(&text, req.line)),
    };
    Some(Focus {
        drive: named.drive,
        path: named.path,
        heading,
    })
}

/// Ask the proxy, in its DM `room_id`, for a new conversation (R36); the
/// request's event id. The proxy's host makes the room and invites the
/// person; it appears in [`agent_rooms_list`] once its status arrives.
#[tauri::command]
pub async fn agent_conversation_new(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    title: Option<String>,
) -> Result<String, IpcError> {
    state
        .accounts
        .agent_conversation_new(&account_id, &room_id, title)
        .await
        .map_err(to_ipc_error)
}
