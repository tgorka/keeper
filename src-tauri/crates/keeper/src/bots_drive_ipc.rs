//! The drive half of the Bots surface (Epic 62, Story 62.1): every `bots_*`
//! command that only makes sense where `keeper-sync` links.
//!
//! **No decisions live here** — the same rule as [`crate::bots_ipc`] (AD-55,
//! AD-56). This file exists because `keeper-sync` is not a dependency of the
//! shell crate on iOS or Android, and a phone can therefore hold a
//! conversation but not a folder: no grant, no audit, no deliverable path, no
//! image staging. Everything that reaches `keeper_sync` or
//! [`crate::bots_tools`] is here, the module is `#[cfg(desktop)]` in `lib.rs`,
//! and its commands are spliced into the desktop `$extra` beside the sync
//! surface they belong to. What keeps the affordances off a phone is
//! `CapabilitiesVm.botTools`, which is false there — absence rather than a
//! refusing twin (AD-27).
//!
//! # Where the seam is
//!
//! The streaming pair (`bots_chat_send`, `bots_message_retry`) stays in
//! `bots_ipc` and runs on every platform, and a turn is always one tool loop
//! over a [`ToolHost`]. This module fills the port that loop needs on a build
//! with a drive: [`arm_drive`] reads the sync profiles, loads the context
//! files a grant allows, and returns a [`DesktopDrive`] that later builds a
//! [`DriveToolHost`] over them once the channel and cancel signal exist. A
//! build without a drive fills the same port with `bots_ipc`'s `NoDrive`,
//! and the streaming code cannot tell which it got.
//!
//! # The approval round trip (Story 61.10)
//!
//! The approval a grant can demand (`GrantVerdict::Ask`) is a round trip the
//! `Channel` cannot carry alone: the turn sends
//! [`BotStreamEvent::ApprovalAsked`] and **blocks** on a one-shot sender
//! registered under the ask's id, and the pane answers through
//! [`bots_approval_answer`]. Stop releases a blocked ask as a refusal, and so
//! does a pane that went away — nothing but an explicit `true` is consent.
//! Both ends are in this file because an ask is only ever raised by the drive
//! host.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use keeper_core::bots::audit;
use keeper_core::bots::chat;
use keeper_core::bots::context_files;
use keeper_core::bots::deliverable;
use keeper_core::bots::grant::{self, Grant, GrantScope};
use keeper_core::bots::tools::{ToolCall, ToolHost};
use keeper_core::bots::{store, Bot};
use keeper_core::vm::{
    BotApprovalRequestVm, BotAttachmentVm, BotAuditRowVm, BotDeliverableVm, BotGrantListVm,
    BotGrantSaveReq, BotGrantVm, BotStreamEvent, IpcError, IpcErrorCode,
};
use keeper_sync::SyncProfile;
use tauri::ipc::Channel;
use tauri::State;

use crate::bots_ipc::{
    bot_of, data_dir, discovered_model, new_id, no_such, now_ms, ArmedDrive, Turn, TurnHost,
};
use crate::bots_tools::{Approver, DriveToolHost};
use crate::ipc::{to_ipc_error, AppState};

// ---------------------------------------------------------------------------
// The drive port for one turn (Story 62.1)
// ---------------------------------------------------------------------------

/// Every sync profile keeper holds, or none when the engine is unavailable.
///
/// No profiles means every tool call is refused as naming no folder and no
/// context file is read — no control, and a reason — which is the failure
/// direction `deliverable_roots` already takes.
fn sync_profiles(state: &AppState) -> Vec<SyncProfile> {
    let platform = Arc::clone(&state.platform);
    let Ok(engine) = crate::sync::engine(platform) else {
        return Vec::new();
    };
    engine.list_profiles().unwrap_or_default()
}

/// Arm the drive half of one turn on a build that has a drive.
///
/// Two reads, and one decision that is `keeper-core`'s: the sync profiles,
/// then — only when `offered` says tools went in the request — the context
/// files [`context_files::context_targets`] picks from the live grants,
/// loaded through [`crate::bots_tools::load_context`] and merged into the
/// bundle the model is shown. The profiles are kept by value on the returned
/// [`DesktopDrive`] because the host built from them holds them the same way.
pub(crate) fn arm_drive(state: &AppState, grants: &[Grant], offered: bool) -> ArmedDrive {
    let profiles = sync_profiles(state);
    let profile_ids: Vec<&str> = profiles.iter().map(|profile| profile.id.as_str()).collect();
    let context = offered.then(|| {
        let targets = context_files::context_targets(grants, &profile_ids);
        context_files::merge(crate::bots_tools::load_context(&profiles, &targets))
    });
    ArmedDrive {
        profile_ids: profile_ids.iter().map(|id| (*id).to_owned()).collect(),
        context,
        host: Box::new(DesktopDrive { profiles }),
    }
}

/// The drive port as a desktop build fills it: the profiles a tool call may
/// name, held until the turn's task exists and the host can be built.
///
/// The grants are **not** here: `DriveToolHost::run` re-reads them per call
/// (FR-386), and a copy on this struct would be an unrevocable grant.
struct DesktopDrive {
    profiles: Vec<SyncProfile>,
}

impl TurnHost for DesktopDrive {
    fn host(
        &self,
        turn: &Turn,
        channel: Channel<BotStreamEvent>,
        signal: chat::CancelSignal,
    ) -> Box<dyn ToolHost> {
        Box::new(DriveToolHost {
            data_dir: turn.dir.clone(),
            provider_id: turn.provider_id.clone(),
            bot_id: Some(turn.bot_id.clone()),
            session_id: turn.session_id.clone(),
            message_id: Some(turn.assistant_id.clone()),
            profiles: self.profiles.clone(),
            approve: Some(approver(
                channel,
                signal,
                turn.provider_id.clone(),
                turn.bot_id.clone(),
            )),
        })
    }
}

// ---------------------------------------------------------------------------
// The approval round trip (Story 61.10, FR-387)
// ---------------------------------------------------------------------------

/// Approvals waiting on a person, keyed by request id (Story 61.10, FR-387).
///
/// The other end of each sender is a tool call blocked inside a turn; the
/// answer arrives through [`bots_approval_answer`] from the sheet the
/// [`BotStreamEvent::ApprovalAsked`] event opened. An entry outlives nothing:
/// the asking side removes it when it has its answer, or when its turn was
/// stopped.
fn asks() -> std::sync::MutexGuard<'static, HashMap<String, SyncSender<bool>>> {
    static ASKS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, SyncSender<bool>>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
    // A poisoned lock means a driver panicked mid-answer. The map holds
    // senders and nothing else, so there is no torn state to protect and
    // refusing every later ask would be the worse failure —
    // `bots_ipc::streams`' reasoning, verbatim.
    ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How long a blocked approval waits between looks at its cancel signal.
///
/// The approval port is synchronous (`bots_tools.rs`'s `Approver`), so the
/// wait is a blocking receive; polling at this cadence is what lets Stop
/// release a turn that is waiting on a sheet nobody will answer.
const APPROVAL_POLL: Duration = Duration::from_millis(250);

/// The approval port for one turn: ask the pane, wait, and obey (Story 61.10,
/// FR-387).
///
/// The port is synchronous by `bots_tools.rs`'s design — a tool call is a
/// blocking act inside one round — so the wait is a blocking receive inside
/// `tokio::task::block_in_place`, which hands this worker's slot to another
/// thread for the duration rather than starving the runtime while a person
/// reads a sheet. The wait ends on the answer, on Stop (the cancel signal is
/// looked at every [`APPROVAL_POLL`]), or on a pane that went away — and every
/// way it ends other than an explicit `true` is a refusal. A missing answer
/// must never read as consent.
fn approver(
    channel: Channel<BotStreamEvent>,
    signal: chat::CancelSignal,
    provider_id: String,
    bot_id: String,
) -> Arc<Approver> {
    Arc::new(move |call: &ToolCall, reason: &str| -> bool {
        let request_id = new_id();
        let (answer, waiting) = std::sync::mpsc::sync_channel::<bool>(1);
        asks().insert(request_id.clone(), answer);
        let request =
            BotApprovalRequestVm::compose(&request_id, &provider_id, Some(&bot_id), call, reason);
        if channel
            .send(BotStreamEvent::ApprovalAsked {
                request: Box::new(request),
            })
            .is_err()
        {
            asks().remove(&request_id);
            return false;
        }
        let approved = tokio::task::block_in_place(|| loop {
            match waiting.recv_timeout(APPROVAL_POLL) {
                Ok(approved) => break approved,
                Err(RecvTimeoutError::Timeout) if !signal.is_cancelled() => {}
                Err(_) => break false,
            }
        });
        asks().remove(&request_id);
        approved
    })
}

/// Answer a tool call waiting on a person (Story 61.10, FR-387).
///
/// The one direction a `Channel` cannot carry: the sheet the
/// [`BotStreamEvent::ApprovalAsked`] event opened answers here, by the
/// `requestId` it was given. `approved` is `true` for "just this once" and
/// for "always for this folder" alike — the latter has already saved its
/// grant through `bots_grant_save` before it answers, so the *next* call to
/// that subtree is allowed by the grant rather than by this answer.
///
/// Idempotent, for `bots_chat_stop`'s reason: an id nobody is waiting on —
/// answered twice, or belonging to a turn Stop already released — is a no-op.
/// The default for an ask nobody answers is a refusal, in the waiting side.
///
/// Rejects with: nothing.
#[tauri::command]
pub fn bots_approval_answer(request_id: String, approved: bool) -> Result<(), IpcError> {
    if let Some(answer) = asks().remove(&request_id) {
        // A receiver that is gone was a turn that stopped waiting; the answer
        // then changes nothing, which is what a late answer should change.
        let _ = answer.send(approved);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Grants and the audit log (Story 61.10, FR-386, FR-387, FR-388, NFR-47)
// ---------------------------------------------------------------------------

/// Every grant, live and revoked, with the rows this build cannot act on
/// (FR-386).
///
/// One list, deliberately: "what can it change?" is answered by grants and
/// their state, never by a history of clicks, so a revoked grant is a row with
/// `revokedMs` set rather than a row that vanished.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_grants_list(state: State<'_, AppState>) -> Result<BotGrantListVm, IpcError> {
    let dir = data_dir(&state)?;
    let listing = store::list_grants(&dir).map_err(to_ipc_error)?;
    Ok(BotGrantListVm::compose(&listing))
}

/// Create or rewrite one grant (FR-386, AD-C7 on the wire).
///
/// `req.id` absent creates, present rewrites. The subtree goes through
/// `keeper_core::bots::grant::parse_subpath`, so the person typing gets the
/// sentence naming what was wrong rather than a scope that silently never
/// matches anything; the **normalized** form is stored, so `notes/` and `notes`
/// are one grant.
///
/// A rewrite clears `revoked_ms`: granting again is what the surface just did,
/// and a row listed as present while dead on every check is the affordance
/// AD-27 forbids.
///
/// **This is the only writer of a grant** (NFR-48). No tool result, file
/// content or model message reaches it, which is what stops a file from
/// widening the access of the model reading it.
///
/// Rejects with: `internal` (a subtree the grammar refuses, an unknown
/// provider or bot).
#[tauri::command]
pub fn bots_grant_save(
    state: State<'_, AppState>,
    req: BotGrantSaveReq,
) -> Result<BotGrantVm, IpcError> {
    let dir = data_dir(&state)?;
    let scope = match req.scope {
        GrantScope::Subtree {
            profile_id,
            subpath,
        } => GrantScope::Subtree {
            profile_id,
            subpath: grant::parse_subpath(&subpath).map_err(|err| IpcError {
                code: IpcErrorCode::Internal,
                message: err.to_string(),
                account_id: None,
                retriable: false,
            })?,
        },
        other => other,
    };
    let existing = match &req.id {
        Some(id) => store::get_grant(&dir, id).map_err(to_ipc_error)?,
        None => None,
    };
    let id = req.id.clone().unwrap_or_else(new_id);
    let created_ms = existing.map_or_else(now_ms, |row| row.grant.created_ms);
    let saved = Grant {
        id,
        provider_id: req.provider_id,
        bot_id: req.bot_id,
        scope,
        mode: req.mode,
        created_ms,
    };
    store::save_grant(&dir, &saved).map_err(to_ipc_error)?;
    let row = store::get_grant(&dir, &saved.id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("grant", &saved.id))?;
    Ok(BotGrantVm::compose(&row))
}

/// Revoke one grant in one act (FR-386).
///
/// The row survives with `revoked_ms` set, so every audit line that names it
/// still resolves. It permits nothing from the next tool call onward —
/// `keeper_core::bots::grant::check` re-reads the table on every call, so a
/// conversation mid-sequence is stopped rather than finishing under a
/// permission that has been taken away.
///
/// Revoking a grant that is already revoked, or one that never existed, is a
/// no-op rather than an error: a racing double-click has no way to know which
/// happened.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_grant_revoke(state: State<'_, AppState>, grant_id: String) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    store::revoke_grant(&dir, &grant_id, now_ms()).map_err(to_ipc_error)?;
    Ok(())
}

/// The tool-call audit log, newest first, optionally for one conversation
/// (FR-388).
///
/// Every row names the path a person reads, because the reader of this log is a
/// person. A row whose `outcome` is `pending` with no `finishedMs` is a call
/// that was recorded and never closed — after a restart, one that was in flight
/// when the process stopped (NFR-47) — and the surface says so rather than
/// rendering it as a success.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_audit_list(
    state: State<'_, AppState>,
    session_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<BotAuditRowVm>, IpcError> {
    let dir = data_dir(&state)?;
    let rows = audit::list_audit(&dir, session_id.as_deref(), limit).map_err(to_ipc_error)?;
    Ok(rows.iter().map(BotAuditRowVm::compose).collect())
}

// ---------------------------------------------------------------------------
// Story 61.12 — an image you can paste, a path you can open (FR-392, FR-393)
// ---------------------------------------------------------------------------

/// Read an ASCII header this command requires.
///
/// A local twin of `ipc.rs`'s helper rather than a widened visibility: the two
/// map their absence onto different error codes, because a missing header on a
/// bots paste is not a Matrix send failure.
fn bots_required_header(headers: &tauri::http::HeaderMap, name: &str) -> Result<String, IpcError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("the paste is missing its {name} header"),
            account_id: None,
            retriable: false,
        })
}

/// Read a header whose value the caller percent-encoded, because it may hold
/// non-ASCII an ASCII-only header value cannot carry verbatim.
fn bots_decoded_header(headers: &tauri::http::HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(name)?.to_str().ok()?;
    percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .ok()
        .map(std::borrow::Cow::into_owned)
        .filter(|value| !value.is_empty())
}

/// Stage a pasted clipboard image for the next message (FR-392, AD-58).
///
/// **The bytes ride as `InvokeBody::Raw`** — ~1× size, never base64 inside a
/// JSON payload — with the file name, the MIME and the capability context in
/// request headers. That is the same sanctioned exception `send_attachment_bytes`
/// takes for a Matrix paste, and the reason is the same: a clipboard image has
/// no OS path for Rust to read from.
///
/// Every decision belongs to `keeper_core::bots::deliverable` (AD-55/AD-56):
/// this reads the request, asks [`deliverable::accept_image`] whether the model
/// may be shown it, and asks [`deliverable::stage_image`] to write it. The gate
/// runs here as well as in the composer because a check that exists only in the
/// webview is not a check.
///
/// Rejects with: `internal` — carrying the refusal sentence verbatim, so the
/// pane prints what `keeper-core` worded rather than a second wording.
#[tauri::command]
pub async fn bots_image_paste(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<BotAttachmentVm, IpcError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "a pasted image must be sent as a raw binary body".to_owned(),
            account_id: None,
            retriable: false,
        });
    };
    let headers = request.headers();
    let mime = bots_required_header(headers, "x-mime")?;
    let filename =
        bots_decoded_header(headers, "x-filename").unwrap_or_else(|| "pasted-image".to_owned());
    let model = bots_decoded_header(headers, "x-model").unwrap_or_else(|| "this model".to_owned());
    let attached: usize = headers
        .get("x-attached")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);

    let dir = data_dir(&state)?;
    let bot_id = bots_required_header(headers, "x-bot-id")?;
    let bot = bot_of(&dir, &bot_id)?;
    // The model's own vision answer, re-read here rather than trusted from the
    // webview: `unknown` offers with a warning, `false` refuses by name, and
    // which of the three it is must not be decidable by the caller.
    let vision = vision_of(&state, &dir, &bot, &model).await;
    deliverable::accept_image(vision, &model, &mime, bytes.len(), attached).map_err(|reason| {
        IpcError {
            code: IpcErrorCode::Internal,
            message: reason,
            account_id: None,
            retriable: false,
        }
    })?;
    let staged = deliverable::stage_image(&dir, &mime, bytes).map_err(to_ipc_error)?;
    Ok(BotAttachmentVm {
        id: staged.id,
        filename,
        mime: staged.mime,
        byte_len: staged.byte_len as i64,
    })
}

/// What the endpoint says about this model's vision, or `None` when keeper
/// could not read it (FR-377, FR-392).
///
/// A discovery failure is `None` and never `false`: a capability keeper could
/// not read is unknown, and the paste is then offered with a warning rather
/// than refused on the strength of a network error.
async fn vision_of(state: &AppState, dir: &Path, bot: &Bot, model: &str) -> Option<bool> {
    discovered_model(state, dir, bot, model)
        .await
        .and_then(|found| found.vision)
}

/// Drop a staged image that was never sent (FR-392). Idempotent.
#[tauri::command]
pub async fn bots_image_discard(
    state: State<'_, AppState>,
    attachment_id: String,
) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    deliverable::discard_staged(&dir, &attachment_id);
    Ok(())
}

/// Resolve the paths an assistant reply named against the drive and the live
/// grants (FR-393, AD-160).
///
/// The grants are re-read on every call, for [`grant::check`]'s reason: a grant
/// set cached anywhere is an unrevocable grant, and a reveal control drawn from
/// a stale read is a button that opens a folder the person closed.
///
/// Rejects with: `internal`.
#[tauri::command]
pub async fn bots_deliverable_paths(
    state: State<'_, AppState>,
    session_id: String,
    body: String,
) -> Result<Vec<BotDeliverableVm>, IpcError> {
    let dir = data_dir(&state)?;
    let session_row = keeper_core::bots::session::get_session(&dir, &session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", &session_id))?;
    let bot = bot_of(&dir, &session_row.bot_id)?;
    // Only the live half: a revoked grant reveals nothing, which is the same
    // rule `grant::check` applies to a tool call.
    let grants = store::list_grants_for_bot(&dir, &bot.provider_id, Some(&bot.id))
        .map_err(to_ipc_error)?
        .live;
    let roots = deliverable_roots(&state);
    // `HOME` rather than a platform port: `~` in a reply is the shell's own
    // spelling of the login home, and keeper has no other notion of it. An
    // unset `HOME` leaves a `~` path unexpanded, which then matches no root and
    // renders with the outside-the-drive sentence — the honest outcome.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let items = deliverable::resolve_deliverables(
        &body,
        home.as_deref(),
        &roots,
        &grants,
        &|path: &Path| path.exists(),
    );
    Ok(items.iter().map(BotDeliverableVm::compose).collect())
}

/// Every sync profile, as `(id, local_path)` pairs.
///
/// An unavailable sync engine yields no roots, so every mentioned path lands on
/// the outside-the-drive sentence — the same failure direction the rest of this
/// story takes: no control, and a reason.
fn deliverable_roots(state: &AppState) -> Vec<deliverable::DeliverableRoot> {
    sync_profiles(state)
        .into_iter()
        .map(|profile| deliverable::DeliverableRoot {
            profile_id: profile.id,
            local_path: profile.local_path,
        })
        .collect()
}
