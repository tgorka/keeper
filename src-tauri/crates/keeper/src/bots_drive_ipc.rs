//! The drive half of the Bots surface (Epic 62, Story 62.1): every `bots_*`
//! command that only makes sense where a drive exists.
//!
//! **No decisions live here** — the same rule as [`crate::bots_ipc`] (AD-55,
//! AD-56). A phone holds a conversation but not a folder the bots surface may
//! reach: no grant, no audit, no deliverable path, no image staging. The
//! module is `#[cfg(desktop)]` in `lib.rs`, and its commands are spliced into
//! the desktop `$extra` beside the sync surface they belong to. What keeps the
//! affordances off a phone is `CapabilitiesVm.botTools`, which is false there
//! — absence rather than a refusing twin (AD-27).
//!
//! # Where the seam is
//!
//! The streaming pair (`bots_chat_send`, `bots_message_retry`) stays in
//! `bots_ipc` and runs on every platform, and a turn is always one
//! `keeper_agent` tool loop. On desktop its drive ports are
//! `crate::agent_ports`': the sync profiles, the notes vault and the approval
//! sheet whose answer arrives here.
//!
//! # The approval round trip (Story 61.10)
//!
//! The approval a grant can demand (`GrantVerdict::Ask`) is a round trip the
//! `Channel` cannot carry alone: the turn sends
//! [`BotStreamEvent::ApprovalAsked`] down its stream — the pane's channel, or
//! the spoken-stream event for a spoken turn — and **blocks** in
//! `keeper_agent::approval` on a one-shot sender registered under the ask's
//! id, and the pane answers through [`bots_approval_answer`]. Stop releases a
//! blocked ask as a refusal, and so does a pane that went away — nothing but
//! an explicit `true` is consent.
//!
//! [`BotStreamEvent::ApprovalAsked`]: keeper_core::vm::BotStreamEvent::ApprovalAsked

use std::path::{Path, PathBuf};

use keeper_agent::turn::{new_id, now_ms};
use keeper_core::bots::audit;
use keeper_core::bots::deliverable;
use keeper_core::bots::grant::{self, Grant, GrantScope};
use keeper_core::bots::{store, Bot};
use keeper_core::vm::{
    BotAttachmentVm, BotAuditRowVm, BotDeliverableVm, BotGrantListVm, BotGrantSaveReq, BotGrantVm,
    IpcError, IpcErrorCode,
};
use tauri::State;

use crate::bots_ipc::{bot_of, data_dir, no_such};
use crate::ipc::{to_ipc_error, AppState};

// ---------------------------------------------------------------------------
// The approval round trip (Story 61.10, FR-387)
// ---------------------------------------------------------------------------

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
    keeper_agent::approval::answer(&request_id, approved);
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
    keeper_agent::turn::discovered_model(&crate::agent_ports::plain_env(state), dir, bot, model)
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
    let Ok(engine) = crate::sync::engine(std::sync::Arc::clone(&state.platform)) else {
        return Vec::new();
    };
    engine
        .list_profiles()
        .unwrap_or_default()
        .into_iter()
        .map(|profile| deliverable::DeliverableRoot {
            profile_id: profile.id,
            local_path: profile.local_path,
        })
        .collect()
}
