//! The bots driving adapter (Epic 61, AD-55/AD-56): every `bots_*` command.
//!
//! **No decisions live here.** The base-URL grammar, the bot-target grammar,
//! the request body, the SSE framing, the delta reassembly, the retry rule, the
//! silence budget, the health verdict and the persistence are all
//! `keeper_core::bots`. This module resolves a row, asks the secret port for a
//! token, assembles an [`Endpoint`], calls, and projects the answer — which is
//! what "the shell is a call site" means in practice.
//!
//! # Why there is no `#[cfg(desktop)]` in this file (Story 62.1)
//!
//! Every other feature module in this crate is desktop-only because it links
//! `keeper-sync`: a vault, a sessions zone and a task record all live in a
//! folder git syncs, and iOS has no such folder. A conversation does not. A
//! provider is a URL plus a credential behind the [`keeper_core::platform`]
//! port, and a conversation is two tables in `keeper.db` — the same database
//! the account registry already opens on every platform. So these commands
//! compile and run anywhere, and they are registered in the **shared**
//! literal in `lib.rs` for the reason `config_layers` is: a target answering
//! "Command bots_providers_list not found" would force the frontend to
//! special-case a call it can always make.
//!
//! That was true when this module was written, stopped being true when the
//! tool loop (Story 61.11) gave it `keeper_sync::bots_fs` through
//! `crate::bots_tools`, and is true again because the half that needs a drive
//! now lives in [`crate::bots_drive_ipc`]: grants, the audit log, the approval
//! answer, image staging and deliverable paths. **This file imports nothing
//! from `keeper_sync` and nothing from `crate::bots_tools`**, and the iOS
//! compile check is what enforces that sentence.
//!
//! What keeps the drive affordances off a phone is
//! [`crate::ipc::capabilities`]'s `botTools`, which is false there — the
//! grant bar, the audit list, the tool rows and the reveal control are all
//! absent, so nothing calls the drive half. That is AD-27's absence, rather
//! than a second code path that refuses.
//!
//! # The stream contract (FR-372, FR-373)
//!
//! [`bots_chat_send`] persists the assistant row **before** the request goes
//! out, marked partial, and rewrites it as deltas land. So the record on disk
//! is never more than one flush behind the record on screen, and a stream that
//! dies — a dropped socket, a pressed Stop, a killed process — leaves a row
//! marked partial rather than leaving nothing. Only the terminal
//! [`BotStreamEvent::Closed`] clears the flag, and it clears it only on a clean
//! finish.
//!
//! The lifecycle follows the tree's existing shape exactly
//! (`sessions_ipc::sessions_search`): the command returns a **string
//! subscription id**, the producer runs on a spawned task registered under that
//! id, and [`bots_chat_stop`] is idempotent — stopping an id that already
//! finished is a no-op, so a racing unmount is not an error. What this adds
//! over that precedent is that Stop is *cooperative* rather than an abort: it
//! fires `keeper_core::bots::chat::CancelHandle`, so the driver unwinds through
//! its own cancel path and writes the partial row, where a bare
//! `JoinHandle::abort` would drop the answer on the floor mid-write.
//!
//! # The tool loop, and the port the drive fills (Stories 61.10, 61.11, 62.1)
//!
//! A turn is `keeper_core::bots::tools::run_tool_loop_reporting` over a
//! [`ToolHost`], always — with no grant the offered `tools` array is empty and
//! the loop is one completion. What goes on offer, which drive files the model
//! is told about, and which profile an unqualified path means are three
//! `keeper-core` decisions [`arm_turn`] gathers the facts for; the shell adds
//! only where each event lands. A finished call is a
//! [`BotStreamEvent::ToolResult`] the moment it completes, and the context
//! bundle is a [`BotStreamEvent::Context`] right after `Opened`.
//!
//! Which host the loop runs over is the one thing this file does not decide:
//! it asks [`arm_drive`], a [`TurnHost`] port with two bodies. On desktop the
//! body is `bots_drive_ipc::arm_drive`, which reads the sync profiles and
//! later builds a `DriveToolHost`; everywhere else it is `NoDrive`, which
//! holds no profiles, loads no context and refuses every call by name. The
//! streaming code cannot tell which it got, so there is one code path and not
//! a plain one beside a tool-using one that drifts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keeper_core::bots::chat::{
    self, CancelHandle, ChatEvent, ChatMessage, ChatOptions, ChatRequest, Role,
};
use keeper_core::bots::context_files::ContextBundle;
use keeper_core::bots::tools::{
    self, ToolHost, ToolLoop, ToolLoopEvent, ToolLoopOptions, ToolOffer,
};
// Story 61.12's inbound direction — a staged image folded into the request —
// on its own line so the story that owns it is legible. The outbound
// direction, and the staging itself, are `bots_drive_ipc`.
use keeper_core::bots::deliverable;
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::Grant;
use keeper_core::bots::{discover, http, session, store, Bot, Endpoint, Provider, ProviderHealth};
use keeper_core::vm::{
    BotChatSendReq, BotConversationVm, BotMessageVm, BotModelVm, BotProbeVm, BotProviderSaveReq,
    BotProviderVm, BotRetryReq, BotSaveReq, BotSessionListVm, BotSessionQueryReq, BotSessionVm,
    BotStreamEvent, BotVm, IpcError, IpcErrorCode,
};
// Story 61.9's two, on their own line so the story that owns them is legible.
use keeper_core::vm::{BotCommandContextReq, BotCommandPreviewVm};
// Story 61.11's two — the tool row and the context disclosure — likewise.
use keeper_core::vm::{BotContextBundleVm, BotToolCallVm};
use tauri::ipc::Channel;
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// How many bytes of a growing answer may sit in memory before the partial row
/// on disk is rewritten.
///
/// Not a timer, and not every delta. Every delta would be one `UPDATE` per
/// token — a write per twenty bytes, on the database the account registry
/// shares. A timer would be a second clock in an app whose AD-62 already
/// refuses one. A byte threshold spends a bounded number of writes per answer
/// and bounds what a crash loses to the last half-kilobyte, which is a sentence
/// rather than an answer.
const FLUSH_BYTES: usize = 512;

// ---------------------------------------------------------------------------
// Errors and small helpers
//
// The ones marked `pub(crate)` are shared with `bots_drive_ipc` — the
// desktop-only half of this surface — and live here rather than there because
// this file compiles on every target and that one does not. One copy, read by
// both, so the two halves cannot drift on what "no such row" or "now" means.
// ---------------------------------------------------------------------------

/// Fold a bots-domain error into the one envelope the frontend understands.
///
/// The classification is the contract's, and it uses the closed taxonomy rather
/// than minting a code: a `401`/`403` is `invalidCredentials` because that is
/// the sentence the app already has for a credential the far side refused; a
/// transport failure or a silence timeout is `serverUnreachable` and retriable,
/// which is the vocabulary the app already uses for a remote it cannot reach
/// (research §8.9, and the epic's rule that an unreachable endpoint produces
/// the same words as an unreachable remote); a quirk-table refusal is
/// `unsupported`; everything else is `internal`.
///
/// Every message here has already been through `keeper_core::bots::error`,
/// which is what guarantees it carries no credential and no full URL.
fn bots_error(error: BotsError) -> IpcError {
    let (code, retriable) = match &error {
        BotsError::Unsupported { .. } => (IpcErrorCode::Unsupported, false),
        BotsError::Status { status, .. } if *status == 401 || *status == 403 => {
            (IpcErrorCode::InvalidCredentials, false)
        }
        BotsError::Transport { retryable, .. } => (IpcErrorCode::ServerUnreachable, *retryable),
        BotsError::Timeout { .. } => (IpcErrorCode::ServerUnreachable, true),
        BotsError::Status { .. } => (IpcErrorCode::Internal, error.is_retryable()),
        _ => (IpcErrorCode::Internal, false),
    };
    IpcError {
        code,
        message: error.to_string(),
        account_id: None,
        retriable,
    }
}

/// The refusal for an id that names nothing.
///
/// `Internal` rather than a caller-input code, for `notes_ipc::notes_error`'s
/// reason: by the time a command runs, the id came from a view model keeper
/// itself produced, so an id that resolves to nothing is keeper's bug or a row
/// deleted underneath a stale render.
pub(crate) fn no_such(what: &str, id: &str) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!("no such {what}: {id}"),
        account_id: None,
        retriable: false,
    }
}

/// Now, in ms since the Unix epoch (UTC) — the only timestamp shape that
/// crosses this boundary.
pub(crate) fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        // Before 1970. Not reachable on a machine whose clock is sane, and a
        // saturating floor is a better record than a panic in a chat command.
        Err(_) => 0,
    }
}

/// A fresh opaque id, in the shape every other keeper record uses.
pub(crate) fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

/// Resolve the data directory through the platform port.
pub(crate) fn data_dir(state: &AppState) -> Result<PathBuf, IpcError> {
    state.platform.data_dir().map_err(to_ipc_error)
}

/// Read one provider row, or refuse.
fn provider_of(dir: &Path, provider_id: &str) -> Result<store::ProviderRow, IpcError> {
    store::get_provider(dir, provider_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("provider", provider_id))
}

/// Read one bot row, or refuse.
pub(crate) fn bot_of(dir: &Path, bot_id: &str) -> Result<Bot, IpcError> {
    store::get_bot(dir, bot_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("bot", bot_id))
}

/// Whether the secret port holds a provider's default credential.
///
/// A read rather than a stored flag, because the keychain is the authority and
/// a column mirroring it would be a second truth that goes stale the moment
/// somebody clears the entry in Keychain Access. A keychain failure reads as
/// "no credential", which is the honest floor: `has_token` drives a sentence
/// about a missing credential, and claiming one is present on a port keeper
/// could not read would be the claim that lies.
fn has_provider_token(state: &AppState, provider_id: &str) -> bool {
    keeper_core::bots::resolve_token(state.platform.as_ref(), provider_id, None)
        .ok()
        .flatten()
        .is_some()
}

/// Assemble the endpoint for one provider, optionally addressing one bot.
///
/// The token comes from `keeper_core::bots::resolve_token`, which knows the
/// bot-then-provider fallback order the far side demands; the join of base URL,
/// kind and profile prefix is `Endpoint::url`'s. Neither is re-derived here.
fn endpoint_of(
    state: &AppState,
    row: &store::ProviderRow,
    bot: Option<&str>,
) -> Result<Endpoint, IpcError> {
    let token = keeper_core::bots::resolve_token(state.platform.as_ref(), &row.provider.id, bot)
        .map_err(to_ipc_error)?;
    Ok(Endpoint::new(&row.provider, bot, token))
}

/// The silence budget for one provider: its override, or the policy default.
fn read_timeout_of(row: &store::ProviderRow) -> Duration {
    match row.read_timeout_ms {
        Some(ms) if ms > 0 => Duration::from_millis(ms.unsigned_abs()),
        _ => http::READ_TIMEOUT,
    }
}

// ---------------------------------------------------------------------------
// Providers (FR-369, FR-379)
// ---------------------------------------------------------------------------

/// Every configured provider, in insertion order (FR-369).
///
/// Rows this build cannot speak to are not returned: they are readable but not
/// speakable, and Story 61.1 keeps them so the epic's later honesty surface can
/// list them. This command answers the picker and the Settings section, both of
/// which offer verbs that would refuse.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_providers_list(state: State<'_, AppState>) -> Result<Vec<BotProviderVm>, IpcError> {
    let dir = data_dir(&state)?;
    let listing = store::list_providers(&dir).map_err(to_ipc_error)?;
    Ok(listing
        .rows
        .iter()
        .map(|row| BotProviderVm::compose(row, has_provider_token(&state, &row.provider.id)))
        .collect())
}

/// Add or edit one provider (FR-379, AD-C7 on the wire).
///
/// `req.id` absent adds, present rewrites. The base URL goes through
/// `keeper_core::bots::parse_base_url` and the **normalized** form is what is
/// stored, so `http://LOCALHOST:11434/` and `http://localhost:11434` are one
/// provider and one egress row rather than two.
///
/// A private or loopback host is accepted — the SSRF question is answered by
/// disclosure and an explicit user act, not by a blocklist — and the answer
/// carries `host` and `isPrivate` so the surface can say which side of the
/// network the bytes stay on.
///
/// The credential is written only when `req.token` is `Some`, and deleted only
/// when `req.clearToken` is set. An absent token means *unchanged*: the edit
/// form cannot render a stored token, so treating an empty field as a deletion
/// would unauthenticate a working provider every time somebody renamed it.
///
/// An edit does **not** carry the previous health snapshot forward, because
/// `store::update_provider` refuses to: the verdict was about an endpoint that
/// may no longer be this one. The surface re-probes, which is what the Test
/// control is for.
///
/// Rejects with: `internal` (a base URL the grammar refuses, or an unknown id).
#[tauri::command]
pub fn bots_provider_save(
    state: State<'_, AppState>,
    req: BotProviderSaveReq,
) -> Result<BotProviderVm, IpcError> {
    let dir = data_dir(&state)?;
    let parsed = keeper_core::bots::parse_base_url(&req.base_url)
        .map_err(|err| bots_error(BotsError::BaseUrl(err)))?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "a provider needs a name you will recognise in the picker".to_owned(),
            account_id: None,
            retriable: false,
        });
    }
    let id = req.id.clone().unwrap_or_else(new_id);
    let provider = Provider {
        id: id.clone(),
        kind: req.kind,
        name: name.to_owned(),
        base_url: parsed.normalized,
        created_ms: now_ms(),
    };
    match &req.id {
        None => store::insert_provider(&dir, &provider).map_err(to_ipc_error)?,
        Some(existing) => {
            if !store::update_provider(&dir, &provider).map_err(to_ipc_error)? {
                return Err(no_such("provider", existing));
            }
        }
    }
    if let Some(token) = req.token.as_deref() {
        keeper_core::bots::save_provider_token(state.platform.as_ref(), &id, token)
            .map_err(to_ipc_error)?;
    } else if req.clear_token {
        keeper_core::bots::delete_provider_token(state.platform.as_ref(), &id)
            .map_err(to_ipc_error)?;
    }
    let row = provider_of(&dir, &id)?;
    Ok(BotProviderVm::compose(
        &row,
        has_provider_token(&state, &id),
    ))
}

/// Remove one provider, its bots and its credential (FR-379).
///
/// Three effects in a deliberate order: the rows first, atomically — one
/// transaction, so a failure cannot leave bots whose provider is gone — then
/// the provider's own secret, then each bot's. The database is the thing a
/// surface reads, so a crash between the row delete and the keychain delete
/// leaves an orphaned secret nothing can reach rather than a provider keeper
/// can no longer authenticate.
///
/// Idempotent in `keeper-core`'s sense: deleting a provider that is already
/// gone is not an error.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_provider_remove(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    // Read the bots BEFORE the delete: afterwards there is nothing left to say
    // which secrets belonged to this provider.
    let bots = store::list_bots_for_provider(&dir, &provider_id).map_err(to_ipc_error)?;
    store::delete_provider(&dir, &provider_id).map_err(to_ipc_error)?;
    keeper_core::bots::delete_provider_token(state.platform.as_ref(), &provider_id)
        .map_err(to_ipc_error)?;
    for bot in &bots {
        keeper_core::bots::delete_bot_token(state.platform.as_ref(), &provider_id, &bot.target)
            .map_err(to_ipc_error)?;
    }
    Ok(())
}

/// Ask a provider whether it is there and what it is, and store the verdict
/// (FR-375).
///
/// Never an `Err` for a refusal: an endpoint that answered `401`, or never
/// answered at all, is a *fact about the endpoint* the surface has to print, so
/// it comes back as a [`BotProbeVm`]. The only errors are keeper's own — an
/// unknown id, an unreadable data dir, a credential the header grammar refuses.
///
/// The verdict is persisted through `discover::health_state`, so the card and
/// the picker read one answer rather than each remembering their own.
///
/// Rejects with: `internal`.
#[tauri::command]
pub async fn bots_provider_probe(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<BotProbeVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = provider_of(&dir, &provider_id)?;
    let endpoint = endpoint_of(&state, &row, None)?;
    let client = http::client(read_timeout_of(&row)).map_err(bots_error)?;
    let probe = discover::health(&client, &endpoint).await;
    let health = ProviderHealth {
        state: discover::health_state(&probe),
        checked_ms: Some(now_ms()),
        detail: probe.reason.clone(),
    };
    store::set_provider_health(&dir, &provider_id, &health).map_err(to_ipc_error)?;
    Ok(probe)
}

/// Every model this provider will accept as a chat request's `model` (FR-377).
///
/// Read from the route that actually knows the answer per kind, and the
/// capability flags are the tri-state `keeper-core` produced: `null` means the
/// endpoint did not say, and is never flattened to `false` here or anywhere.
///
/// `bot` addresses a Hermes profile prefix, so the models offered for a bot are
/// the models *that bot* answers to rather than the gateway's defaults.
///
/// Rejects with: `internal`, `invalidCredentials`, `serverUnreachable`,
/// `unsupported`.
#[tauri::command]
pub async fn bots_models_list(
    state: State<'_, AppState>,
    provider_id: String,
    bot: Option<String>,
) -> Result<Vec<BotModelVm>, IpcError> {
    let dir = data_dir(&state)?;
    let row = provider_of(&dir, &provider_id)?;
    let endpoint = endpoint_of(&state, &row, bot.as_deref())?;
    let client = http::client(read_timeout_of(&row)).map_err(bots_error)?;
    discover::models(&client, &endpoint)
        .await
        .map_err(bots_error)
}

// ---------------------------------------------------------------------------
// Bots (FR-376, FR-383)
// ---------------------------------------------------------------------------

/// Verify that a named bot is really there (FR-376).
///
/// Verification, not enumeration: the bearer API keeper is allowed through has
/// no profile roster, so a bot is named by the person who has one and this
/// confirms it exists before they rely on it. The three-way answer —
/// `exists` / `absent` / `unknown` — is `keeper-core`'s, and `unknown` is a real
/// answer rather than a failure: "keeper could not ask" is a different sentence
/// from "it is not there" for somebody about to retype a name that was right
/// all along.
///
/// Rejects with: `internal`.
#[tauri::command]
pub async fn bots_bot_probe(
    state: State<'_, AppState>,
    provider_id: String,
    target: String,
) -> Result<BotProbeVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = provider_of(&dir, &provider_id)?;
    let endpoint = endpoint_of(&state, &row, Some(&target))?;
    let client = http::client(read_timeout_of(&row)).map_err(bots_error)?;
    Ok(discover::probe_bot(&client, &endpoint, &target).await)
}

/// Every pinned bot, in the user's hand-set order (FR-383).
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_bots_list(state: State<'_, AppState>) -> Result<Vec<BotVm>, IpcError> {
    let dir = data_dir(&state)?;
    let bots = store::list_bots(&dir).map_err(to_ipc_error)?;
    Ok(bots.iter().map(BotVm::compose).collect())
}

/// Add or edit one bot (FR-376, AD-C7 on the wire).
///
/// The target goes through `keeper_core::bots::parse_bot_target`, so the person
/// typing gets the sentence that names what was wrong rather than a `404` from
/// a URL keeper composed out of it. A new bot lands at the end of the hand
/// order — Story 61.7 owns reordering.
///
/// Rejects with: `internal` (a target the grammar refuses, a duplicate
/// `(provider, target)`, an unknown id).
#[tauri::command]
pub fn bots_bot_save(state: State<'_, AppState>, req: BotSaveReq) -> Result<BotVm, IpcError> {
    let dir = data_dir(&state)?;
    let target = keeper_core::bots::parse_bot_target(&req.target).map_err(|err| IpcError {
        code: IpcErrorCode::Internal,
        message: err.to_string(),
        account_id: None,
        retriable: false,
    })?;
    let name = req.name.trim();
    let name = if name.is_empty() { target } else { name };
    // Refuse before writing: a bot whose provider does not exist is a row every
    // list, picker and grant would then have to disambiguate.
    let _ = provider_of(&dir, &req.provider_id)?;
    let id = match req.id {
        None => {
            let existing = store::list_bots(&dir).map_err(to_ipc_error)?;
            let pin_order = i64::try_from(existing.len()).unwrap_or(i64::MAX);
            let bot = Bot {
                id: new_id(),
                provider_id: req.provider_id.clone(),
                target: target.to_owned(),
                name: name.to_owned(),
                pin_order,
                identity: keeper_core::bots::BotIdentity::default(),
                created_ms: now_ms(),
            };
            store::insert_bot(&dir, &bot).map_err(to_ipc_error)?;
            bot.id
        }
        Some(existing) => {
            if !store::update_bot(&dir, &existing, target, name).map_err(to_ipc_error)? {
                return Err(no_such("bot", &existing));
            }
            existing
        }
    };
    if let Some(token) = req.token.as_deref() {
        keeper_core::bots::save_bot_token(state.platform.as_ref(), &req.provider_id, target, token)
            .map_err(to_ipc_error)?;
    } else if req.clear_token {
        keeper_core::bots::delete_bot_token(state.platform.as_ref(), &req.provider_id, target)
            .map_err(to_ipc_error)?;
    }
    let bot = bot_of(&dir, &id)?;
    Ok(BotVm::compose(&bot))
}

/// Remove one bot and its own credential (FR-383).
///
/// The bot's conversations are **not** deleted. A conversation is a record of
/// something that happened, and unpinning a bot is not a statement about the
/// past; Story 61.6 owns deleting one, with a confirmation that names what
/// happens to which object.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_bot_remove(state: State<'_, AppState>, bot_id: String) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    // Read before delete, for `bots_provider_remove`'s reason: afterwards
    // nothing says which secret belonged to this bot.
    let bot = store::get_bot(&dir, &bot_id).map_err(to_ipc_error)?;
    store::delete_bot(&dir, &bot_id).map_err(to_ipc_error)?;
    if let Some(bot) = bot {
        keeper_core::bots::delete_bot_token(state.platform.as_ref(), &bot.provider_id, &bot.target)
            .map_err(to_ipc_error)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Conversations (FR-381, FR-382)
// ---------------------------------------------------------------------------

/// Every conversation, newest activity first (FR-381).
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_sessions_list(
    state: State<'_, AppState>,
    include_archived: bool,
) -> Result<Vec<BotSessionVm>, IpcError> {
    let dir = data_dir(&state)?;
    let rows = session::list_sessions(&dir, include_archived).map_err(to_ipc_error)?;
    Ok(rows.iter().map(BotSessionVm::compose).collect())
}

/// One conversation and its messages, replayed from keeper's own store
/// (FR-382).
///
/// One command rather than two, so a header cannot render one conversation's
/// title over another's rows for a frame. Nothing is fetched from the remote:
/// keeper's store is the truth, and a Hermes `session_id` is a reference the
/// detail may show.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub fn bots_session_open(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<BotConversationVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = session::get_session(&dir, &session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", &session_id))?;
    let messages = session::list_messages(&dir, &session_id).map_err(to_ipc_error)?;
    Ok(BotConversationVm {
        session: BotSessionVm::compose(&row),
        messages: messages.iter().map(BotMessageVm::compose).collect(),
    })
}

/// One page of the conversation list, searched, scoped and bounded
/// (Story 61.6, FR-381).
///
/// Its own command beside [`bots_sessions_list`] rather than a widening of it:
/// that one answers "every conversation" for the pane's own refresh, and this
/// one answers a query a person typed, with the `total` a count line needs
/// beside the page it could otherwise miscount.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn bots_sessions_search(
    state: State<'_, AppState>,
    req: BotSessionQueryReq,
) -> Result<BotSessionListVm, IpcError> {
    let dir = data_dir(&state)?;
    let page = session::search_sessions(&dir, &req.to_query()).map_err(to_ipc_error)?;
    Ok(BotSessionListVm::compose(&page))
}

/// Rename one conversation (Story 61.6, FR-381).
///
/// The new name goes through [`session::mint_title`] — the same minter a first
/// message goes through — so a rename cannot install a title the list could
/// not draw: no newline, no emoji as chrome, no zero-width name, and the same
/// clamp. A name that leaves nothing quotable becomes the placeholder rather
/// than an empty row.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub fn bots_session_rename(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<BotSessionVm, IpcError> {
    let dir = data_dir(&state)?;
    let minted = session::mint_title(&title);
    if !session::set_session_title(&dir, &session_id, &minted, now_ms()).map_err(to_ipc_error)? {
        return Err(no_such("conversation", &session_id));
    }
    session_vm(&dir, &session_id)
}

/// Archive or unarchive one conversation (Story 61.6, FR-381).
///
/// One command with a flag rather than two verbs, for
/// `bots_sessions_search`'s converse reason: archiving and unarchiving are one
/// column with two values, and two commands would be two chances for them to
/// disagree about what else a filing changes.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub fn bots_session_archive(
    state: State<'_, AppState>,
    session_id: String,
    archived: bool,
) -> Result<BotSessionVm, IpcError> {
    let dir = data_dir(&state)?;
    if !session::set_session_archived(&dir, &session_id, archived, now_ms())
        .map_err(to_ipc_error)?
    {
        return Err(no_such("conversation", &session_id));
    }
    session_vm(&dir, &session_id)
}

/// Delete one conversation and every message in it (Story 61.6, FR-381).
///
/// **No remote request is made.** keeper's store is the record (AD-154), so a
/// delete is a local transaction; a Hermes `session_id` beside the row named
/// something on a server keeper never owned and cannot speak for.
///
/// It refuses an id that names nothing rather than reporting a delete that
/// deleted nothing, because the confirmation the user just read named an
/// object — and AD-27 forbids an affordance that claims an effect it did not
/// have.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub fn bots_session_delete(state: State<'_, AppState>, session_id: String) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    if session::get_session(&dir, &session_id)
        .map_err(to_ipc_error)?
        .is_none()
    {
        return Err(no_such("conversation", &session_id));
    }
    session::delete_session(&dir, &session_id).map_err(to_ipc_error)
}

/// Re-read one conversation after a write, or refuse.
///
/// Every mutating conversation command answers with the row as stored rather
/// than with the row it hoped for: the frontend renders what the database
/// says, which is what keeps a rename that was clamped from showing the
/// unclamped text until the next read.
fn session_vm(dir: &std::path::Path, session_id: &str) -> Result<BotSessionVm, IpcError> {
    let row = session::get_session(dir, session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", session_id))?;
    Ok(BotSessionVm::compose(&row))
}

// ---------------------------------------------------------------------------
// Streaming (FR-372, FR-373, FR-374)
// ---------------------------------------------------------------------------

/// One answer being streamed. Dropping it aborts the task, which is the
/// backstop; [`bots_chat_stop`] uses `cancel` instead, so the driver unwinds
/// through its own cancel path and gets to write the partial row.
struct LiveStream {
    cancel: CancelHandle,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Answers currently streaming, keyed by subscription id.
///
/// Several may run at once — the epic's surface is one pane, but a person who
/// starts an answer, switches conversation and starts another has two, and
/// killing the first would be the app deciding their question was stale.
fn streams() -> std::sync::MutexGuard<'static, HashMap<String, LiveStream>> {
    static STREAMS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, LiveStream>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
    STREAMS
        .lock()
        // A poisoned lock means a driver panicked mid-answer. The map holds
        // cancel handles and join handles and nothing else, so there is no torn
        // state to protect and refusing every later send would be the worse
        // failure — `sessions_ipc::scans`'s reasoning, verbatim.
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What one streaming turn needs, resolved before the task is spawned.
///
/// `pub(crate)` with its fields because [`TurnHost::host`] hands it to the
/// drive half, which reads the ids the audit rows and the approval sheet
/// name. Nothing outside this module constructs one.
pub(crate) struct Turn {
    pub(crate) dir: PathBuf,
    pub(crate) endpoint: Endpoint,
    pub(crate) request: ChatRequest,
    pub(crate) read_timeout: Duration,
    pub(crate) session_id: String,
    pub(crate) provider_id: String,
    pub(crate) bot_id: String,
    pub(crate) assistant_id: String,
    /// What the drive contributed while arming, and how to build the host
    /// over it once the channel and the cancel signal exist. The grants are
    /// **not** here: the host re-reads them per call (FR-386), and a copy on
    /// this struct would be an unrevocable grant.
    pub(crate) drive: Box<dyn TurnHost>,
    /// The profile an unqualified tool path is relative to, or empty when no
    /// grant names one — `keeper_core::bots::tools::default_profile_id`.
    pub(crate) default_profile_id: String,
}

// ---------------------------------------------------------------------------
// The drive port (Story 62.1)
// ---------------------------------------------------------------------------

/// What the drive contributes to one turn, decided while arming it.
///
/// Two builds fill it. Desktop (`bots_drive_ipc::arm_drive`) reads the sync
/// profiles, loads the context files a grant allows and later builds a
/// `DriveToolHost` over them; every other target — a phone has no
/// `keeper-sync` to link — fills it with `NoDrive`. The streaming pair does
/// not know which it got, which is what keeps one code path.
pub(crate) struct ArmedDrive {
    /// The profile ids a tool call may name, for [`tools::default_profile_id`].
    /// Empty where there is no drive.
    pub(crate) profile_ids: Vec<String>,
    /// The bundle the model is shown, when tools were offered and the drive
    /// could read one. `None` is "keeper does not know", never "none".
    pub(crate) context: Option<ContextBundle>,
    /// How to build the host once the turn's task exists.
    pub(crate) host: Box<dyn TurnHost>,
}

/// How a build makes the host one turn's tool calls run against.
///
/// A method rather than a value on [`ArmedDrive`] because the approval port a
/// desktop host carries needs the stream channel and the cancel signal, and
/// both exist only inside the spawned task — after arming, not during it.
pub(crate) trait TurnHost: Send + Sync {
    fn host(
        &self,
        turn: &Turn,
        channel: Channel<BotStreamEvent>,
        signal: chat::CancelSignal,
    ) -> Box<dyn ToolHost>;
}

/// The drive port on a build with no drive.
///
/// Holds no profiles, loads no context, and refuses every call by name —
/// which `tools::offer_tools` makes unreachable in practice, since a grant
/// cannot be created where `CapabilitiesVm.botTools` is false and a Hermes
/// bot is never offered tools. The refusal exists so that a model which
/// calls a tool anyway gets a sentence rather than a panic, and so the loop
/// records the call the way it records every other refusal.
///
/// Gated with the body that constructs it: on desktop nothing would, and a
/// never-constructed struct is a warning the clippy gate turns into an error.
#[cfg(not(desktop))]
struct NoDrive;

#[cfg(not(desktop))]
impl TurnHost for NoDrive {
    fn host(
        &self,
        _turn: &Turn,
        _channel: Channel<BotStreamEvent>,
        _signal: chat::CancelSignal,
    ) -> Box<dyn ToolHost> {
        Box::new(NoDrive)
    }
}

#[cfg(not(desktop))]
impl ToolHost for NoDrive {
    fn run(&self, call: &tools::ToolCall) -> Result<tools::ToolOutcome, BotsError> {
        Ok(tools::ToolOutcome::Refused {
            reason: format!(
                "{} needs the drive, and this build of keeper has none: the drive tools live on the Mac.",
                call.name.as_wire()
            ),
        })
    }
}

/// Arm the drive half of a turn, on the build that has one.
///
/// Two bodies, the way `served_as_lfs_filter` in `lib.rs` has two: the
/// desktop one is `bots_drive_ipc`'s, and every other target gets
/// `NoDrive`. A `cfg` pair rather than a port installed at startup because
/// a port nobody installed would fail silently at runtime, and this pair
/// fails at compile time.
#[cfg(desktop)]
fn arm_drive(state: &AppState, grants: &[Grant], offered: bool) -> ArmedDrive {
    crate::bots_drive_ipc::arm_drive(state, grants, offered)
}

/// The build without a drive: no profiles, no context, a refusing host.
#[cfg(not(desktop))]
fn arm_drive(_state: &AppState, _grants: &[Grant], _offered: bool) -> ArmedDrive {
    ArmedDrive {
        profile_ids: Vec::new(),
        context: None,
        host: Box::new(NoDrive),
    }
}

/// What [`arm_turn`] resolved: the request as the model will see it, the
/// bundle the pane is told about, and the drive half the task will run over.
struct Armed {
    request: ChatRequest,
    context: Option<ContextBundle>,
    drive: Box<dyn TurnHost>,
    default_profile_id: String,
}

/// Everything about one turn that `keeper-core` decides from the live grants,
/// gathered here and decided there.
///
/// Three reads and three decisions. The reads: the live grants for this
/// `(provider, bot)`, the drive's profiles through [`arm_drive`], and — only
/// where a grant exists and the provider is one keeper runs tools for —
/// whether the model states it can use tools. The decisions, all
/// `keeper-core`'s: [`tools::offer_tools`] for what goes in `tools`,
/// `context_files::context_targets` (inside the drive half) for which drive
/// files the model is told about, and [`tools::default_profile_id`] for the
/// profile an unqualified path means.
///
/// **The context bundle is built only when tools are offered.** The bundle is
/// FR-390's "rules for the files the model is about to touch", and a model
/// that cannot touch them has no call for their rules; for a Hermes bot in
/// particular, keeper has told the person a grant changes nothing there, and
/// sending that server the drive's `AGENTS.md` would be a disclosure the pane
/// never promised. A grant is still the precondition for reading a context
/// file at all — `context_targets` runs every candidate through `decide`.
///
/// An unreadable grant table reads as no grants: no tools, no context, and
/// the turn still runs as plain prose rather than failing the send.
async fn arm_turn(
    state: &AppState,
    dir: &Path,
    row: &store::ProviderRow,
    bot: &Bot,
    model: &str,
    messages: Vec<ChatMessage>,
) -> Armed {
    let grants = store::list_grants_for_bot(dir, &bot.provider_id, Some(&bot.id))
        .map(|listing| listing.live)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "bots: could not read the grants for this turn");
            Vec::new()
        });
    let kind = row.provider.kind;
    // The probe is a network round trip, spent only where its answer can
    // change the decision: `offer_tools` withholds for Hermes and for no
    // grant whatever the capability says.
    let tools_supported = if grants.is_empty() || kind == keeper_core::bots::ProviderKind::Hermes {
        None
    } else {
        discovered_model(state, dir, bot, model)
            .await
            .and_then(|found| found.tools)
    };
    let offer = tools::offer_tools(kind, tools_supported, &grants);
    if let ToolOffer::Withheld { reason } = &offer {
        tracing::debug!(reason, "bots: no tools offered this turn");
    }

    let drive = arm_drive(state, &grants, offer.is_offered());
    let profile_ids: Vec<&str> = drive.profile_ids.iter().map(String::as_str).collect();
    let default_profile_id = tools::default_profile_id(&grants, &profile_ids).unwrap_or_default();

    let mut prompted = Vec::with_capacity(messages.len() + 1);
    if let Some(system) = drive
        .context
        .as_ref()
        .and_then(ContextBundle::system_prompt)
    {
        prompted.push(ChatMessage::text(Role::System, system));
    }
    prompted.extend(messages);

    let request = ChatRequest {
        model: model.to_owned(),
        messages: prompted,
        tools: offer.specs(),
        ..ChatRequest::default()
    };
    Armed {
        request,
        context: drive.context,
        drive: drive.host,
        default_profile_id,
    }
}

/// Ask a bot, streaming the answer over `channel`, and return the subscription
/// id (FR-372).
///
/// What has already happened by the time this resolves: the conversation exists
/// (created here when `req.sessionId` is absent, because the first message is
/// what mints the title), the user's message is stored, and the assistant row
/// is stored **empty and partial**. All three are on the first
/// [`BotStreamEvent::Opened`] event, which is emitted before the request goes
/// out — so the pane renders the pending answer rather than an optimistic
/// placeholder it would have to reconcile.
///
/// The whole conversation is replayed to the model from keeper's store, in
/// order, which is what makes "continue" mean replay rather than a server-side
/// resume keeper cannot verify.
///
/// Rejects with: `internal` (unknown bot, provider or conversation),
/// `unsupported` (a request this provider kind refuses),
/// `invalidCredentials`, `serverUnreachable`.
#[tauri::command]
pub async fn bots_chat_send(
    state: State<'_, AppState>,
    req: BotChatSendReq,
    channel: Channel<BotStreamEvent>,
) -> Result<String, IpcError> {
    let dir = data_dir(&state)?;
    let bot = bot_of(&dir, &req.bot_id)?;
    let row = provider_of(&dir, &bot.provider_id)?;
    let endpoint = endpoint_of(&state, &row, Some(&bot.target))?;
    let now = now_ms();

    // The conversation, created on the first message so a titled conversation
    // can never exist with nothing in it.
    let session_row = match req.session_id.as_deref() {
        Some(id) => session::get_session(&dir, id)
            .map_err(to_ipc_error)?
            .ok_or_else(|| no_such("conversation", id))?,
        None => {
            let created = session::BotSession {
                id: new_id(),
                bot_id: bot.id.clone(),
                provider_id: bot.provider_id.clone(),
                title: session::mint_title(&req.text),
                created_ms: now,
                updated_ms: now,
                archived: false,
                remote_session_id: None,
            };
            session::insert_session(&dir, &created).map_err(to_ipc_error)?;
            created
        }
    };

    let user = store_message(&dir, &session_row.id, "user", &req.text, None, false, now)?;
    let assistant = store_message(
        &dir,
        &session_row.id,
        "assistant",
        "",
        Some((&req.model, &bot.provider_id)),
        true,
        now,
    )?;
    session::touch_session(&dir, &session_row.id, now).map_err(to_ipc_error)?;

    let history = session::list_messages(&dir, &session_row.id).map_err(to_ipc_error)?;
    // Story 61.12: the pasted images of this turn become `data:` content parts
    // on the user message, here and nowhere else.
    let messages = attach_staged_images(&dir, replay(&history, &assistant.id), &req.attachment_ids);
    let armed = arm_turn(&state, &dir, &row, &bot, &req.model, messages).await;
    let context = armed.context;
    let turn = Turn {
        dir,
        endpoint,
        request: armed.request,
        read_timeout: read_timeout_of(&row),
        session_id: session_row.id.clone(),
        provider_id: bot.provider_id.clone(),
        bot_id: bot.id.clone(),
        assistant_id: assistant.id.clone(),
        drive: armed.drive,
        default_profile_id: armed.default_profile_id,
    };

    let subscription_id = new_id();
    let _ = channel.send(BotStreamEvent::Opened {
        subscription_id: subscription_id.clone(),
        session: Box::new(BotSessionVm::compose(&session_row)),
        user: Box::new(BotMessageVm::compose(&user)),
        assistant: Box::new(BotMessageVm::compose(&assistant)),
    });
    // After `Opened`, so the pane has the row the disclosure belongs to.
    emit_context(&channel, context.as_ref());
    Ok(spawn_turn(turn, subscription_id, channel))
}

/// Re-ask the question one assistant row failed to answer (FR-372).
///
/// The failed row is **replaced**, not appended beside: two answers to one
/// question is a record nobody can read, and the epic's own retry boundary says
/// a partially-streamed completion is never resumed — a re-sent request samples
/// afresh, so splicing two halves would produce text the model never wrote.
///
/// `req.messageId` names the row explicitly rather than being inferred as "the
/// last one", so a Retry pressed on a stale render cannot delete a row that
/// arrived after it was drawn.
///
/// Rejects with: `internal` (unknown conversation or message, or a message that
/// is not an assistant row).
#[tauri::command]
pub async fn bots_message_retry(
    state: State<'_, AppState>,
    req: BotRetryReq,
    channel: Channel<BotStreamEvent>,
) -> Result<String, IpcError> {
    let dir = data_dir(&state)?;
    let session_row = session::get_session(&dir, &req.session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", &req.session_id))?;
    let bot = bot_of(&dir, &session_row.bot_id)?;
    let row = provider_of(&dir, &bot.provider_id)?;
    let endpoint = endpoint_of(&state, &row, Some(&bot.target))?;

    let history = session::list_messages(&dir, &req.session_id).map_err(to_ipc_error)?;
    let doomed = history
        .iter()
        .find(|message| message.id == req.message_id)
        .ok_or_else(|| no_such("message", &req.message_id))?;
    if doomed.role != "assistant" {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "only an answer can be retried; a question is not re-asked by keeper"
                .to_owned(),
            account_id: None,
            retriable: false,
        });
    }
    // Drop the failed answer and everything after it: replaying a later turn
    // over a re-sampled earlier one would build a conversation that never
    // happened.
    for message in history.iter().filter(|m| m.seq >= doomed.seq) {
        session::delete_message(&dir, &message.id).map_err(to_ipc_error)?;
    }

    let now = now_ms();
    let assistant = store_message(
        &dir,
        &req.session_id,
        "assistant",
        "",
        Some((&req.model, &bot.provider_id)),
        true,
        now,
    )?;
    session::touch_session(&dir, &req.session_id, now).map_err(to_ipc_error)?;

    let replayed = session::list_messages(&dir, &req.session_id).map_err(to_ipc_error)?;
    let armed = arm_turn(
        &state,
        &dir,
        &row,
        &bot,
        &req.model,
        replay(&replayed, &assistant.id),
    )
    .await;
    let context = armed.context;
    let turn = Turn {
        dir,
        endpoint,
        request: armed.request,
        read_timeout: read_timeout_of(&row),
        session_id: req.session_id.clone(),
        provider_id: bot.provider_id.clone(),
        bot_id: bot.id.clone(),
        assistant_id: assistant.id.clone(),
        drive: armed.drive,
        default_profile_id: armed.default_profile_id,
    };

    let subscription_id = new_id();
    let _ = channel.send(BotStreamEvent::Opened {
        subscription_id: subscription_id.clone(),
        session: Box::new(BotSessionVm::compose(&session_row)),
        // The question is unchanged, so the row the pane already holds is
        // echoed rather than re-minted: a Retry that re-emitted a new user
        // message would double the question on screen.
        user: Box::new(
            replayed
                .iter()
                .rfind(|m| m.role == "user")
                .map(BotMessageVm::compose)
                .unwrap_or_else(|| BotMessageVm::compose(&assistant)),
        ),
        assistant: Box::new(BotMessageVm::compose(&assistant)),
    });
    emit_context(&channel, context.as_ref());
    Ok(spawn_turn(turn, subscription_id, channel))
}

/// Stop a streaming answer by subscription id (FR-372).
///
/// Idempotent: an id that already finished, or one whose window closed, is a
/// no-op — a racing unmount has no way to know which happened and should not
/// have to.
///
/// It fires the cancel handle rather than aborting the task, and the difference
/// is the whole point: the driver wakes from a *silent* socket through its watch
/// channel, unwinds through its own cancel path, and writes what had arrived as
/// a partial row. An abort would drop the answer mid-write.
///
/// Rejects with: nothing.
#[tauri::command]
pub fn bots_chat_stop(subscription_id: String) -> Result<(), IpcError> {
    if let Some(live) = streams().get(&subscription_id) {
        live.cancel.cancel();
    }
    Ok(())
}

/// What one composer draft is: prose, a command, or a refusal (Story 61.9,
/// FR-385).
///
/// **The thinnest command in this file, and deliberately so.** It reads no row,
/// opens no database, touches no socket and holds no state: the whole answer is
/// `keeper_core::bots::commands` applied to a string and a context the caller
/// already knows. The registry, the resolution order, the refusal sentences and
/// the availability reasons are all decisions, and decisions live in the core
/// (AD-55/AD-56) — so the shell is one `compose` call, which is what makes the
/// rules testable without a shell that compiles.
///
/// Called as somebody types, the way `sync_task_schedule_preview` is, and it
/// carries that command's contract with it: the draft is **echoed back** and
/// the caller must compare it against the field's current value, because a slow
/// answer for a half-typed draft can land after a fast answer for the finished
/// one.
///
/// Rejects with: nothing. A refusal is data, never a rejection — a half-typed
/// command is the ordinary case rather than a fault.
#[tauri::command]
pub fn bots_command_preview(
    draft: String,
    context: BotCommandContextReq,
) -> Result<BotCommandPreviewVm, IpcError> {
    Ok(BotCommandPreviewVm::compose(&draft, &context.context()))
}

/// Store one message and return it as written, with the `seq` the store
/// assigned.
fn store_message(
    dir: &Path,
    session_id: &str,
    role: &str,
    content: &str,
    model: Option<(&str, &str)>,
    partial: bool,
    now: i64,
) -> Result<session::BotMessage, IpcError> {
    let mut message = session::BotMessage {
        id: new_id(),
        session_id: session_id.to_owned(),
        seq: 0,
        role: role.to_owned(),
        content: content.to_owned(),
        model: model.map(|(model, _)| model.to_owned()),
        provider_id: model.map(|(_, provider)| provider.to_owned()),
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        ttft_ms: None,
        duration_ms: None,
        finish_reason: None,
        request_id: None,
        tool_call_count: 0,
        partial,
        created_ms: now,
    };
    message.seq = session::append_message(dir, &message).map_err(to_ipc_error)?;
    Ok(message)
}

/// The conversation as the model is told it, excluding the empty assistant row
/// this turn is about to fill.
///
/// A partial row from an earlier failed turn is replayed as what it is — the
/// model saw those tokens, and hiding them would make the next turn answer a
/// question it has already half-answered.
fn replay(history: &[session::BotMessage], exclude_id: &str) -> Vec<ChatMessage> {
    history
        .iter()
        .filter(|message| message.id != exclude_id && !message.content.is_empty())
        .map(|message| {
            let role = match message.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                // Anything else — including a role a newer keeper wrote — is
                // replayed as the person's turn rather than dropped: a message
                // omitted from the replay is a question the model answers
                // without.
                _ => Role::User,
            };
            ChatMessage::text(role, message.content.clone())
        })
        .collect()
}

/// Spawn the driver for one turn and register it under `subscription_id`.
fn spawn_turn(turn: Turn, subscription_id: String, channel: Channel<BotStreamEvent>) -> String {
    let (cancel, signal) = chat::cancellation();
    let retire = subscription_id.clone();
    let task = tauri::async_runtime::spawn(async move {
        drive(turn, signal, channel).await;
        // Retire self. `remove` drops the `LiveStream`, whose `Drop` aborts a
        // task that has already finished — which aborts nothing.
        streams().remove(&retire);
    });
    streams().insert(subscription_id.clone(), LiveStream { cancel, task });
    subscription_id
}

/// Run one turn — every completion of it, tool rounds included — into the
/// channel and into the store.
///
/// Every write to the partial row happens here rather than in the command, so
/// there is exactly one writer per answer and the flush policy is stated once.
///
/// The turn is `keeper_core::bots::tools::run_tool_loop_reporting` whether or
/// not tools were offered: with an empty `tools` array it is one completion,
/// so there is one code path and not a plain one beside a tool-using one that
/// drifts. What the loop decides — how many rounds, how a refusal is worded,
/// what an exhausted budget does — is not restated here; what this adds is
/// where each event lands. A finished call becomes a
/// [`BotStreamEvent::ToolResult`] **as it completes**, and on a build with a
/// drive its audit row was written by `DriveToolHost` before its effect
/// (NFR-47), so a turn that dies mid-loop has shown and recorded every call
/// that ran.
async fn drive(turn: Turn, signal: chat::CancelSignal, channel: Channel<BotStreamEvent>) {
    let client = match http::client(turn.read_timeout) {
        Ok(client) => client,
        Err(error) => {
            close_failed(&turn, &channel, &error.to_string());
            return;
        }
    };
    let options = ChatOptions {
        read_timeout: turn.read_timeout,
        ..ChatOptions::default()
    };

    // The host is the port's to build, not this file's: `channel` and
    // `signal` are what an approval needs, and they exist only here.
    let host = turn.drive.host(&turn, channel.clone(), signal.clone());
    let context = ToolLoop {
        client: &client,
        endpoint: &turn.endpoint,
        host: host.as_ref(),
        default_profile_id: &turn.default_profile_id,
    };

    // Accumulated in the sink and flushed by byte count — see `FLUSH_BYTES`.
    // Across rounds, not per round: the text a model wrote before it called a
    // tool and the text it wrote after are one answer on screen, so they are
    // one answer in the row.
    let mut content = String::new();
    let mut unflushed = 0usize;
    let mut sink = |event: ToolLoopEvent| match event {
        ToolLoopEvent::Chat(ChatEvent::FirstToken { after_ms }) => {
            let _ = channel.send(BotStreamEvent::FirstToken { after_ms });
        }
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => {
            unflushed += text.len();
            content.push_str(&text);
            let _ = channel.send(BotStreamEvent::Delta { text });
            if unflushed >= FLUSH_BYTES {
                unflushed = 0;
                // A failed flush is not fatal to the answer on screen: the row
                // stays partial, which is exactly what it is.
                if let Err(error) =
                    session::set_message_content(&turn.dir, &turn.assistant_id, &content)
                {
                    tracing::warn!(%error, "bots: could not flush a partial answer");
                }
            }
        }
        ToolLoopEvent::Chat(ChatEvent::ReasoningDelta(text)) => {
            let _ = channel.send(BotStreamEvent::Reasoning { text });
        }
        ToolLoopEvent::Chat(ChatEvent::ToolCallDelta { name, .. }) => {
            // The name, as the fragments arrive: the row itself follows from
            // the reporter once the call has run.
            if let Some(name) = name {
                let _ = channel.send(BotStreamEvent::ToolCall { name });
            }
        }
        // Usage and the finish reason are written once, by the close below,
        // from the outcome — which carries them whether or not the sink saw
        // them.
        ToolLoopEvent::Chat(ChatEvent::Usage(_) | ChatEvent::Finished { .. }) => {}
        ToolLoopEvent::Chat(ChatEvent::Failed { error }) => {
            tracing::warn!(%error, "bots: a stream failed after it had produced bytes");
        }
        // A later round's prose starts on its own paragraph, on screen and in
        // the row alike, so two rounds' sentences do not run together.
        ToolLoopEvent::RoundStarted { round, .. } => {
            if round > 0 && !content.is_empty() && !content.ends_with('\n') {
                content.push_str("\n\n");
                let _ = channel.send(BotStreamEvent::Delta {
                    text: "\n\n".to_owned(),
                });
            }
        }
        // The row carries the refusal and `grantDenied`; the exhausted budget
        // answers every outstanding call with its own sentence, which the
        // rows show. Nothing here needs a second wording.
        ToolLoopEvent::ToolStarted { .. }
        | ToolLoopEvent::ToolFinished { .. }
        | ToolLoopEvent::GrantDenied { .. }
        | ToolLoopEvent::RoundsExhausted { .. } => {}
    };
    let mut report =
        |record: &tools::ToolCallRecord, wire: &chat::ToolCall, outcome: &tools::ToolOutcome| {
            let _ = channel.send(BotStreamEvent::ToolResult {
                call: Box::new(BotToolCallVm::compose(
                    record,
                    &tools::arguments_text(wire),
                    Some(outcome),
                )),
            });
        };

    let outcome = tools::run_tool_loop_reporting(
        &context,
        &turn.request,
        &options,
        &ToolLoopOptions::default(),
        signal.clone(),
        &mut sink,
        &mut report,
    )
    .await;

    match outcome {
        Ok(ran) => {
            let cancelled = signal.is_cancelled();
            let reason = match (&ran.final_outcome.finish_reason, cancelled) {
                (_, true) => Some("Stopped. What had arrived is kept.".to_owned()),
                (chat::FinishReason::Failed, false) => {
                    Some("The answer stopped before it finished.".to_owned())
                }
                _ => None,
            };
            close(
                &turn,
                &channel,
                &ran.final_outcome,
                &content,
                ran.calls.len(),
                reason,
            );
        }
        // No stream byte ever existed, so there is nothing to keep. The row
        // stays — marked partial, with the reason — rather than vanishing: a
        // question whose answer disappeared is a surface that lost a message.
        Err(error) => close_failed(&turn, &channel, &error.to_string()),
    }
}

/// Tell the pane what the model was told about the drive, where anything was
/// (Story 61.11, FR-391).
///
/// Absent when no bundle was built, which the pane draws as "keeper does not
/// know" — not as "none", because a turn with no read grant genuinely told the
/// model nothing and a turn keeper never armed is not the same fact (AD-27).
fn emit_context(channel: &Channel<BotStreamEvent>, context: Option<&ContextBundle>) {
    if let Some(bundle) = context {
        let _ = channel.send(BotStreamEvent::Context {
            bundle: Box::new(BotContextBundleVm::compose(bundle)),
        });
    }
}

/// Write the finished (or stopped) answer and emit the terminal event.
///
/// `content` is the whole turn's prose as the sink accumulated it, not the
/// final completion's alone: a tool-using turn is several completions and the
/// row holds what the person saw. `tool_call_count` is every call the loop
/// ran, for the same reason.
fn close(
    turn: &Turn,
    channel: &Channel<BotStreamEvent>,
    outcome: &chat::ChatOutcome,
    content: &str,
    tool_call_count: usize,
    reason: Option<String>,
) {
    let usage = outcome.usage.unwrap_or_default();
    let partial = reason.is_some();
    let finish_reason = finish_word(&outcome.finish_reason);
    let close = session::MessageClose {
        id: &turn.assistant_id,
        content,
        model: outcome.model.as_deref().or(Some(&turn.request.model)),
        prompt_tokens: usage.prompt_tokens.map(i64::from),
        completion_tokens: usage.completion_tokens.map(i64::from),
        total_tokens: usage.total_tokens.map(i64::from),
        ttft_ms: outcome
            .first_token_ms
            .map(|ms| i64::try_from(ms).unwrap_or(i64::MAX)),
        duration_ms: Some(i64::try_from(outcome.total_ms).unwrap_or(i64::MAX)),
        finish_reason: Some(&finish_reason),
        request_id: outcome.response_id.as_deref(),
        tool_call_count: i64::try_from(tool_call_count).unwrap_or(i64::MAX),
        partial,
    };
    if let Err(error) = session::close_message(&turn.dir, close) {
        tracing::warn!(%error, "bots: could not close an answer");
    }
    emit_closed(turn, channel, reason);
}

/// Write a failure that produced no usable outcome, keeping the row partial.
fn close_failed(turn: &Turn, channel: &Channel<BotStreamEvent>, reason: &str) {
    let close = session::MessageClose {
        id: &turn.assistant_id,
        content: "",
        model: Some(&turn.request.model),
        finish_reason: Some("failed"),
        partial: true,
        ..session::MessageClose::default()
    };
    if let Err(error) = session::close_message(&turn.dir, close) {
        tracing::warn!(%error, "bots: could not record a failed answer");
    }
    emit_closed(turn, channel, Some(reason.to_owned()));
}

/// Re-read the row and emit the one terminal event, so the surface renders what
/// was actually stored rather than what the producer believed it stored.
fn emit_closed(turn: &Turn, channel: &Channel<BotStreamEvent>, reason: Option<String>) {
    session::touch_session(&turn.dir, &turn.session_id, now_ms()).ok();
    let stored = session::list_messages(&turn.dir, &turn.session_id)
        .ok()
        .and_then(|rows| {
            rows.into_iter()
                .find(|row| row.id == turn.assistant_id)
                .as_ref()
                .map(BotMessageVm::compose)
        });
    let Some(message) = stored else {
        // The row is gone — the conversation was deleted while the answer was
        // in flight. Nothing to report to a surface that no longer shows it.
        tracing::warn!(
            provider = %turn.provider_id,
            "bots: an answer finished into a conversation that no longer exists"
        );
        return;
    };
    let _ = channel.send(BotStreamEvent::Closed {
        message: Box::new(message),
        reason,
    });
}

/// The stored spelling of a finish reason.
///
/// The provider's own word where it invented one, for [`BotMessageVm`]'s
/// reason: a caption that prints what the endpoint said is more use than one
/// that prints keeper's guess.
fn finish_word(reason: &chat::FinishReason) -> String {
    match reason {
        chat::FinishReason::Stop => "stop".to_owned(),
        chat::FinishReason::Length => "length".to_owned(),
        chat::FinishReason::ContentFilter => "content_filter".to_owned(),
        chat::FinishReason::ToolCalls => "tool_calls".to_owned(),
        chat::FinishReason::Cancelled => "cancelled".to_owned(),
        chat::FinishReason::Failed => "failed".to_owned(),
        chat::FinishReason::Other(word) => word.clone(),
    }
}

/// Read whether an answer shows its metadata caption (Story 61.8, FR-384).
///
/// No mobile twin, for the reason no command in this file has one: it runs
/// on every target, and a phone's pane calls it like a desktop's. The value
/// itself is an ordinary `settings` row and the layer stack in front of it
/// means a `keeper.toml` can set it, which is why the read goes through the
/// account manager rather than the table.
#[tauri::command]
pub fn bots_message_details_get(state: State<'_, AppState>) -> Result<bool, IpcError> {
    state
        .accounts
        .bots_message_details_get(&state.platform)
        .map_err(to_ipc_error)
}

/// Write whether an answer shows its metadata caption (Story 61.8, FR-384).
///
/// The pane's toggle and the palette entry both land here, so the two cannot
/// drift into two preferences that look like one.
#[tauri::command]
pub fn bots_message_details_set(state: State<'_, AppState>, shown: bool) -> Result<(), IpcError> {
    state
        .accounts
        .bots_message_details_set(&state.platform, shown)
        .map_err(to_ipc_error)
}

/// Write one bot's chosen identity — shape, colour token, mark (Story 61.7,
/// FR-383).
///
/// Validation is `keeper_core::bots::identity::parse_identity` and not this
/// file's business: the closed shape set, the bounded palette and the rule that
/// a colour needs a shape beside it are decisions, and decisions live in
/// `keeper-core`. The shell reads the row back afterwards so the caller gets
/// what was stored rather than what it sent.
///
/// Rejects with: `internal` (an unknown shape or colour, a colour with no
/// shape, a mark that will not draw, an unknown bot).
#[tauri::command]
pub fn bots_bot_identity_save(
    state: State<'_, AppState>,
    bot_id: String,
    shape: Option<String>,
    colour: Option<String>,
    mark: Option<String>,
) -> Result<BotVm, IpcError> {
    let dir = data_dir(&state)?;
    let identity = keeper_core::bots::identity::parse_identity(
        shape.as_deref(),
        colour.as_deref(),
        mark.as_deref(),
    )
    .map_err(|err| IpcError {
        code: IpcErrorCode::Internal,
        message: err.to_string(),
        account_id: None,
        retriable: false,
    })?;
    if !store::set_bot_identity(&dir, &bot_id, &identity).map_err(to_ipc_error)? {
        return Err(no_such("bot", &bot_id));
    }
    let bot = bot_of(&dir, &bot_id)?;
    Ok(BotVm::compose(&bot))
}

/// Rewrite the whole hand order (Story 61.7, FR-383).
///
/// `order` is every bot id, in the order the strip should draw them.
/// `keeper_core::bots::identity::plan_reorder` refuses anything that is not a
/// permutation of what exists BEFORE the write, because the write rewrites the
/// whole sequence: a partial order would renumber some rows and leave the rest
/// at their old positions, which is `registry::reorder_pins`' own lesson and
/// the reason the pins strip disables its drag while a filter is on.
///
/// The write itself is one `BEGIN IMMEDIATE` transaction in
/// `store::reorder_bots`, so it commits as a unit or not at all.
///
/// Rejects with: `internal` (an unknown id, a duplicate, a partial order).
#[tauri::command]
pub fn bots_bots_reorder(
    state: State<'_, AppState>,
    order: Vec<String>,
) -> Result<Vec<BotVm>, IpcError> {
    let dir = data_dir(&state)?;
    let known: Vec<String> = store::list_bots(&dir)
        .map_err(to_ipc_error)?
        .into_iter()
        .map(|bot| bot.id)
        .collect();
    let plan =
        keeper_core::bots::identity::plan_reorder(&known, &order).map_err(|err| IpcError {
            code: IpcErrorCode::Internal,
            message: err.to_string(),
            account_id: None,
            retriable: false,
        })?;
    store::reorder_bots(&dir, &plan).map_err(to_ipc_error)?;
    let bots = store::list_bots(&dir).map_err(to_ipc_error)?;
    Ok(bots.iter().map(BotVm::compose).collect())
}

// ---------------------------------------------------------------------------
// Story 61.12 — the inbound half: a staged image becomes a content part
// (FR-392). The paste that stages it and the reveal that resolves a path are
// `bots_drive_ipc`, because the second reads the sync profiles and the first
// is an affordance the phone does not offer.
// ---------------------------------------------------------------------------

/// This model as the endpoint describes it right now, or `None` when keeper
/// could not ask or the endpoint does not list it.
///
/// One read behind both the drive half's `vision_of` and the turn's
/// tool-capability check, so the tri-state every capability carries — `None`
/// is "did not say", never `false` — is produced by one route (FR-377).
pub(crate) async fn discovered_model(
    state: &AppState,
    dir: &Path,
    bot: &Bot,
    model: &str,
) -> Option<BotModelVm> {
    let row = provider_of(dir, &bot.provider_id).ok()?;
    let endpoint = endpoint_of(state, &row, Some(&bot.target)).ok()?;
    let client = http::client(read_timeout_of(&row)).ok()?;
    let models = discover::models(&client, &endpoint).await.ok()?;
    models.into_iter().find(|candidate| candidate.id == model)
}

/// Fold the staged images of this message into its user turn (FR-392).
///
/// The one place a pasted image becomes a `data:` URI, and it happens inside
/// the outbound request body — which is the shape Ollama's OpenAI layer
/// documents and the only shape it accepts. A staged image that cannot be read
/// back is skipped rather than fatal: the question is still worth asking, and
/// the alternative is losing a typed message to a missing temp file.
///
/// Each image is discarded once its bytes are in the request, so the staging
/// folder holds only pastes that have not been sent.
fn attach_staged_images(
    dir: &Path,
    mut messages: Vec<ChatMessage>,
    attachment_ids: &[String],
) -> Vec<ChatMessage> {
    if attachment_ids.is_empty() {
        return messages;
    }
    let Some(last) = messages.iter_mut().rev().find(|m| m.role == Role::User) else {
        return messages;
    };
    for id in attachment_ids {
        match deliverable::read_staged(dir, id) {
            Ok(bytes) => {
                let mime =
                    deliverable::staged_mime(dir, id).unwrap_or_else(|| "image/png".to_owned());
                last.content
                    .push(deliverable::image_content_part(&mime, &bytes));
                deliverable::discard_staged(dir, id);
            }
            Err(error) => {
                tracing::info!(%error, "bots: a staged image could not be read back");
            }
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replay excludes the row this turn is about to fill, and keeps a
    /// partial answer from an earlier turn.
    #[test]
    fn the_replay_excludes_the_row_being_filled() {
        let message = |id: &str, role: &str, content: &str, seq: i64| session::BotMessage {
            id: id.to_owned(),
            session_id: "s1".to_owned(),
            seq,
            role: role.to_owned(),
            content: content.to_owned(),
            model: None,
            provider_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            ttft_ms: None,
            duration_ms: None,
            finish_reason: None,
            request_id: None,
            tool_call_count: 0,
            partial: false,
            created_ms: 0,
        };
        let history = vec![
            message("m1", "user", "hello", 0),
            message("m2", "assistant", "half an ans", 1),
            message("m3", "user", "again", 2),
            message("m4", "assistant", "", 3),
        ];
        let replayed = replay(&history, "m4");
        assert_eq!(replayed.len(), 3, "the empty row being filled is excluded");
        assert_eq!(replayed[1].role, Role::Assistant);
    }
}
