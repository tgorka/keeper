//! The person's proxy beside the notes view (stories 91.2–91.4; AD-382,
//! AD-383, AD-384): the dock's commands, the surface requests this device
//! executes, its presence, and a spoken question sent to the proxy with its
//! answer spoken as it grows. On every target — the phone shows the proxy's
//! rooms, answers surface requests and speaks to it too (P5: it is never a
//! host, which is `agents_host`'s, desktop only).
//!
//! It decides nothing: which rooms are the proxy's, which agent and which
//! drives a request may come from, what a scope, a focus, a presence or a
//! surface result says and when it goes out are `keeper_core::agents`' and
//! `AccountManager`'s; which drive and path a note is, and which note or
//! file a request names, `keeper_agent::surface`'s.

use std::sync::Mutex;

use keeper_agent::surface::{drive_of, locate, Located};
use keeper_core::agents::approval_card::ApprovalDecideReq;
use keeper_core::agents::events::{Focus, PresencePlatform, SurfaceOutcome};
use keeper_core::agents::proxy::{AgentFocusReq, ProxyRoomVm};
use keeper_core::agents::spoken::{FollowSlot, SpokenStep};
use keeper_core::agents::surface::{
    SurfaceAnswerReq, SurfaceRequestArrived, SurfaceRequestVm, CANNOT_SHOW,
};
use keeper_core::bots::voice_target;
use keeper_core::notes::outline::{heading_at, heading_in};
use keeper_core::panels::PanelTargetVm;
use keeper_core::vm::{IpcError, VoiceAgentTargetVm};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::ipc::{to_ipc_error, AppState};

/// The platform a presence names.
#[cfg(target_os = "ios")]
const PLATFORM: PresencePlatform = PresencePlatform::Ios;
#[cfg(target_os = "android")]
const PLATFORM: PresencePlatform = PresencePlatform::Android;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
const PLATFORM: PresencePlatform = PresencePlatform::Macos;

/// keeper came to the front or left it: any of its windows' focus on the
/// desktop ([`any_window_focused`]), the app's lifecycle on the phone
/// (`lib.rs`). Every live account's presence follows after a second of
/// stillness.
pub fn presence_focus(app: &AppHandle, focused: bool) {
    app.state::<AppState>()
        .accounts
        .agent_presence_focus(PLATFORM, focused);
}

/// Whether any of keeper's windows is focused — the main window, the draft
/// window, the voice pill: typing in the draft window blurs main while
/// keeper is plainly in front. A window that is not focused yet when another
/// blurs is caught by its own `Focused(true)` a moment later, within the
/// second of stillness a presence waits.
#[cfg(desktop)]
pub fn any_window_focused(app: &AppHandle) -> bool {
    app.webview_windows()
        .values()
        .any(|window| window.is_focused().unwrap_or(false))
}

/// The primary view keeper shows now (`notes`, `chats`): a view id, never a
/// note.
#[tauri::command]
pub fn agent_presence_view(state: State<'_, AppState>, view: String) -> Result<(), IpcError> {
    state
        .accounts
        .agent_presence_view(&view)
        .map_err(to_ipc_error)
}

/// Stream the surface requests this device admits into `channel`, each
/// naming the note or file it is about. A request whose drive or path this
/// device cannot name is answered `unavailable` with one fixed sentence
/// (`CANNOT_SHOW`) and not streamed; the reason stays in the log, so an
/// answer never says which drives, folders or paths this device keeps. The
/// relay ends when the channel closes.
#[tauri::command]
pub fn agent_surface_subscribe(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: Channel<SurfaceRequestVm>,
) -> Result<(), IpcError> {
    let mut requests = state.accounts.surface_requests();
    tauri::async_runtime::spawn(async move {
        loop {
            let arrived = match requests.recv().await {
                Ok(arrived) => arrived,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let platform = std::sync::Arc::clone(&app.state::<AppState>().platform);
            let named = arrived.clone();
            let target = tokio::task::spawn_blocking(move || name_target(platform, &named))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            match target {
                Ok(target) => {
                    if channel
                        .send(SurfaceRequestVm::new(&arrived, target))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(reason) => {
                    tracing::debug!(room = %arrived.room_id, %reason, "agents: a surface request names what this device cannot show");
                    let answer = SurfaceAnswerReq {
                        request_id: arrived.request.id.clone(),
                        outcome: SurfaceOutcome::Unavailable,
                        applied: None,
                        detail: Some(CANNOT_SHOW.to_owned()),
                    };
                    let sent = app
                        .state::<AppState>()
                        .accounts
                        .agent_surface_result(&arrived.account_id, arrived.room_id.as_str(), answer)
                        .await;
                    if let Err(error) = sent {
                        tracing::warn!(%error, "agents: a surface request this device cannot name could not be answered");
                    }
                }
            }
        }
    });
    Ok(())
}

/// The note or file `arrived` names on this device: a note of a vault by
/// its id (the editor), any other file of a synced folder by its path (the
/// Files preview) — within the drives the room's proxy declares, where this
/// device knows them, never every folder it syncs. Blocking: the engine's
/// profiles. The `Err` is the reason, for the log only.
fn name_target(
    platform: std::sync::Arc<dyn keeper_core::platform::Platform>,
    arrived: &SurfaceRequestArrived,
) -> Result<PanelTargetVm, String> {
    let args = &arrived.request.args;
    if arrived
        .drives
        .as_ref()
        .is_some_and(|drives| !drives.contains(&args.drive))
    {
        return Err(format!(
            "{} is not a drive the room's proxy declares.",
            args.drive
        ));
    }
    let profiles = crate::sync::engine(platform)
        .map_err(|error| error.to_string())?
        .list_profiles()
        .map_err(|error| error.to_string())?;
    let folders: Vec<_> = profiles
        .into_iter()
        .map(|profile| {
            let vault =
                crate::notes_vault::vault(&profile.id).map(|vault| vault.config.subfolder.clone());
            (profile, vault)
        })
        .collect();
    match locate(&folders, &args.drive, &args.path)? {
        Located::Note {
            vault_id,
            note_path,
        } => {
            let note_id = crate::notes_vault::snapshot(&vault_id)
                .and_then(|index| index.by_path(&note_path).map(|entry| entry.id.clone()))
                .ok_or_else(|| format!("{} is not in the notes vault's index yet.", args.path))?;
            Ok(PanelTargetVm::Note { vault_id, note_id })
        }
        Located::File {
            profile_id,
            relative_path,
        } => Ok(PanelTargetVm::File {
            profile_id,
            relative_path,
        }),
    }
}

/// The notes view's answer to the surface request it executed, sent into
/// the room it came from — only for a request this device handed it.
#[tauri::command]
pub async fn agent_surface_result(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    answer: SurfaceAnswerReq,
) -> Result<(), IpcError> {
    state
        .accounts
        .agent_surface_result(&account_id, &room_id, answer)
        .await
        .map_err(to_ipc_error)
}

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

/// The person's decision on the approval card `req` names in the session
/// room `room_id` (93.3), sent from this device. Refused at once, with the
/// card's sentence, where the card shows no decide buttons; whether it
/// counts is the owning host's (`keeper_core`'s `agent_approval_decide`).
#[tauri::command]
pub async fn agent_approval_decide(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    req: ApprovalDecideReq,
) -> Result<(), IpcError> {
    state
        .accounts
        .agent_approval_decide(&account_id, &room_id, req)
        .await
        .map_err(to_ipc_error)
}

/// The attached action of the approval `id` in the session room `room_id`,
/// as pretty-printed JSON, fetched, decrypted and checked against its
/// digest; refused with a sentence when it is not the action sent for
/// approval (R186, `keeper_core`'s `agent_approval_payload`). Once shown,
/// it may be approved from this app.
#[tauri::command]
pub async fn agent_approval_payload(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    id: String,
) -> Result<String, IpcError> {
    state
        .accounts
        .agent_approval_payload(&account_id, &room_id, &id)
        .await
        .map_err(to_ipc_error)
}

/// `account_id`'s own master key, in groups of four, for the person to
/// compare with `keeper-agentd status`; `None` while it publishes none.
#[tauri::command]
pub async fn agent_own_fingerprint(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<String>, IpcError> {
    state
        .accounts
        .agent_own_fingerprint(&account_id)
        .await
        .map_err(to_ipc_error)
}

/// The proxy conversations a spoken turn may go to, on every signed-in
/// account, for the voice target picker ("Speak to", AD-384): each with the
/// value `voice_target_set` stores to choose it.
#[tauri::command]
pub async fn voice_agent_targets(
    state: State<'_, AppState>,
) -> Result<Vec<VoiceAgentTargetVm>, IpcError> {
    Ok(voice_target::agent_targets(
        state.accounts.agent_rooms_everywhere().await,
    ))
}

/// The send and then the following of the answer to the voice turn's
/// current question, when it went to the person's agent: only that
/// question's is ever kept ([`FollowSlot`]).
static SPOKEN_ANSWER: Mutex<FollowSlot<tauri::async_runtime::JoinHandle<()>>> =
    Mutex::new(FollowSlot::new());

fn spoken_answer(
) -> std::sync::MutexGuard<'static, FollowSlot<tauri::async_runtime::JoinHandle<()>>> {
    SPOKEN_ANSWER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A new question was heard: an earlier one's send or answer is not the
/// new turn's to speak (R44: the agent itself is not told; it finishes its
/// answer in the room).
pub fn drop_spoken_answer() {
    let current = crate::voice_ipc::question_now();
    if let Some(earlier) = spoken_answer().keep_only(current) {
        earlier.abort();
    }
}

/// Send what the voice turn heard as `question` into `room_id`, the
/// person's proxy conversation on `account_id` (AD-384, R31), and speak the
/// answer as it grows. Called by `bots_ipc::send_spoken` for an agent room
/// target. Only the text leaves the device, as the person's own message;
/// which room is allowed and what of the answer is heard when are
/// `keeper_core::account::spoken_send`'s and `keeper_core::agents::spoken`'s.
/// The voice turn hears the send leave (`note_sent`, with the agent's name),
/// the first words, each sentence and the end — or the refusal or failure
/// as its sentence — each for `question`. The work is in the slot before it
/// starts: a question that is no longer the voice turn's never sends, and
/// a newer one stops it.
pub fn send_spoken(
    app: &AppHandle,
    question: u64,
    account_id: String,
    room_id: String,
    text: String,
) {
    let app = app.clone();
    let work = tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut answer = match state
            .accounts
            .agent_spoken_send(&account_id, &room_id, &text)
            .await
        {
            Ok(answer) => answer,
            Err(error) => {
                let refusal = to_ipc_error(error);
                tracing::warn!(message = %refusal.message, "agents: a spoken question was not sent");
                crate::voice_ipc::answer_failed(question, refusal.message);
                return;
            }
        };
        tracing::info!(room = %room_id, "agents: sent what the voice turn heard to the person's agent");
        crate::voice_ipc::note_sent(question, answer.agent_name());
        while let Some(step) = answer.next().await {
            match step {
                SpokenStep::FirstText { after_ms } => {
                    crate::voice_ipc::note_answer_chunk(question, after_ms);
                }
                SpokenStep::Sentence(sentence) => {
                    crate::voice_ipc::answer_sentence(question, sentence);
                }
                SpokenStep::Complete(rest) => crate::voice_ipc::answer_complete(question, rest),
                SpokenStep::Failed(why) => crate::voice_ipc::answer_failed(question, why),
            }
        }
    });
    let current = crate::voice_ipc::question_now();
    if let Some(stopped) = spoken_answer().install(question, current, work) {
        stopped.abort();
    }
}
