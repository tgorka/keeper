//! The bots driving adapter (Epic 61, AD-55/AD-56): every `bots_*` command.
//!
//! **No decisions live here.** The base-URL grammar, the bot-target grammar,
//! the request body, the SSE framing, the delta reassembly, the retry rule, the
//! silence budget, the health verdict and the persistence are all
//! `keeper_core::bots`, and the turn loop itself — opening a turn, the tool
//! loop, the partial row, the stream — is `keeper_agent` (AD-367). This module
//! builds the turn's environment and sink from the app's ports
//! ([`crate::agent_ports`]), calls, and projects the answer — which is what
//! "the shell is a call site" means in practice.
//!
//! # Why there is no `#[cfg(desktop)]` in this file (Story 62.1)
//!
//! A conversation is a URL plus a credential behind the
//! [`keeper_core::platform`] port and two tables in `keeper.db`, so these
//! commands compile and run anywhere, and they are registered in the
//! **shared** literal in `lib.rs` for the reason `config_layers` is: a target
//! answering "Command bots_providers_list not found" would force the frontend
//! to special-case a call it can always make.
//!
//! The half that needs a drive — grants, the audit log, the approval answer,
//! image staging and deliverable paths — lives in [`crate::bots_drive_ipc`],
//! and a turn's drive is chosen at compile time by
//! [`crate::agent_ports::turn_env`]: the sync profiles, the notes vault and
//! the approval sheet on desktop, none on a phone, where `keeper_agent` arms
//! a host that refuses every tool by name. What keeps the drive affordances
//! off a phone is [`crate::ipc::capabilities`]'s `botTools`, which is false
//! there — AD-27's absence, rather than a second code path that refuses.
//!
//! # The stream contract (FR-372, FR-373)
//!
//! [`bots_chat_send`] returns a **string subscription id**; the producer runs
//! on a spawned task registered under that id, and [`bots_chat_stop`] is
//! idempotent and cooperative. The assistant row is stored partial before the
//! request goes out and rewritten as deltas land; `keeper_agent::turn` and
//! `keeper_agent::drive` state the contract.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_agent::drive;
use keeper_agent::ports::TurnSink;
use keeper_agent::turn::{self, new_id, now_ms, TurnEnv};
use keeper_core::agents::proxy::ProxyRoomVm;
use keeper_core::bots::error::BotsError;
use keeper_core::bots::follow;
use keeper_core::bots::remote::{self, SessionCapabilities};
// Epic 67's one (AD-206): where a spoken turn goes, decided in core.
use keeper_core::bots::voice_target;
use keeper_core::bots::{discover, http, session, store, Bot, Provider, ProviderHealth};
use keeper_core::registry;
use keeper_core::vm::{
    BotChatSendReq, BotConversationVm, BotFollowVm, BotMessageVm, BotModelVm, BotProbeVm,
    BotProviderSaveReq, BotProviderVm, BotRetryReq, BotSaveReq, BotSessionListVm,
    BotSessionQueryReq, BotSessionVm, BotStreamEvent, BotTranscriptSource, BotVm, IpcError,
    IpcErrorCode,
};
// Story 61.9's two, on their own line so the story that owns them is legible.
use keeper_core::vm::{BotCommandContextReq, BotCommandPreviewVm};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::agent_ports::{agent_error, plain_env, sink_for, ChannelSink, EventSink};
use crate::ipc::{to_ipc_error, AppState};

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
/// could not read would be the claim that lies. A provider set to use the
/// account has one by construction (Epic 82, AD-315).
fn has_provider_token(state: &AppState, provider_id: &str) -> bool {
    crate::account_ipc::provider_has_credential(state.platform.as_ref(), provider_id)
}

// ---------------------------------------------------------------------------
// What each endpoint said about sessions (Epic 63, Story 63.6, AD-176)
// ---------------------------------------------------------------------------

/// Fold every capable gateway's session list into `keeper.db` (AD-176).
///
/// For each provider whose endpoint has a session API, and each pinned bot on
/// it, read the gateway's list and `remote::reconcile` it: a session started
/// on another device becomes a row here, carrying the same id, so it can be
/// opened and continued. Every failure degrades silently to the local list —
/// a gateway that stopped answering is remembered as having no session API
/// until the provider is edited or re-tested, so a dead host costs one
/// connect timeout and not one per refresh.
async fn reconcile_remote(env: &TurnEnv, dir: &Path) {
    let Ok(listing) = store::list_providers(dir) else {
        return;
    };
    let now = now_ms();
    for row in &listing.rows {
        let caps = turn::session_caps(env, row).await;
        if !caps.session_api {
            continue;
        }
        let Ok(bots) = store::list_bots_for_provider(dir, &row.provider.id) else {
            continue;
        };
        let Ok(client) = discover::discovery_client() else {
            return;
        };
        for bot in &bots {
            let Ok(endpoint) = turn::endpoint_of(env, row, Some(&bot.target)).await else {
                continue;
            };
            match remote::list_sessions(&client, &endpoint).await {
                Ok(found) => {
                    if let Err(error) =
                        remote::reconcile(dir, &row.provider.id, &bot.id, &found, now)
                    {
                        tracing::warn!(%error, "bots: could not fold the gateway's sessions in");
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "bots: gateway session list not read; local list stands");
                    if matches!(
                        error,
                        BotsError::Transport { .. } | BotsError::Timeout { .. }
                    ) {
                        turn::capabilities().remember(
                            &row.provider.id,
                            &row.provider.base_url,
                            SessionCapabilities::NONE,
                        );
                    }
                    break;
                }
            }
        }
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
    // An edit may point at a different gateway; what the old one said about
    // sessions is not a fact about the new one.
    turn::capabilities().forget(&id);
    let row = provider_of(&dir, &id)?;
    // The provider travels in the person's `bots.toml` (Epic 84, AD-325).
    crate::account_ipc::note_local_change();
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
    turn::capabilities().forget(&provider_id);
    keeper_core::bots::delete_provider_token(state.platform.as_ref(), &provider_id)
        .map_err(to_ipc_error)?;
    for bot in &bots {
        keeper_core::bots::delete_bot_token(state.platform.as_ref(), &provider_id, &bot.target)
            .map_err(to_ipc_error)?;
    }
    crate::account_ipc::note_local_change();
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
    // Test is the person asking again, so the session-capability answer is
    // asked again too on the next list read — the way a gateway that came
    // back from a dead spell gets its session API noticed.
    turn::capabilities().forget(&provider_id);
    let endpoint = turn::endpoint_of(&plain_env(&state), &row, None)
        .await
        .map_err(agent_error)?;
    let client = http::client(turn::read_timeout_of(&row)).map_err(bots_error)?;
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
    let endpoint = turn::endpoint_of(&plain_env(&state), &row, bot.as_deref())
        .await
        .map_err(agent_error)?;
    let client = http::client(turn::read_timeout_of(&row)).map_err(bots_error)?;
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
    let endpoint = turn::endpoint_of(&plain_env(&state), &row, Some(&target))
        .await
        .map_err(agent_error)?;
    let client = http::client(turn::read_timeout_of(&row)).map_err(bots_error)?;
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
    crate::account_ipc::note_local_change();
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
    crate::account_ipc::note_local_change();
    Ok(())
}

// ---------------------------------------------------------------------------
// Conversations (FR-381, FR-382)
// ---------------------------------------------------------------------------

/// Every conversation, newest activity first (FR-381), after folding in
/// every capable gateway's own list (Epic 63, AD-176).
///
/// This is the read the pane makes on mount and after every send — its
/// **revision signal** for the searched list — so it is where the gateway is
/// asked: a session another device started becomes a row here before the
/// list that shows it is re-read. A gateway with no session API, or one that
/// does not answer, changes nothing about the answer; see
/// [`reconcile_remote`].
///
/// Rejects with: `internal`.
#[tauri::command]
pub async fn bots_sessions_list(
    state: State<'_, AppState>,
    include_archived: bool,
) -> Result<Vec<BotSessionVm>, IpcError> {
    let dir = data_dir(&state)?;
    reconcile_remote(&plain_env(&state), &dir).await;
    let rows = session::list_sessions(&dir, include_archived).map_err(to_ipc_error)?;
    Ok(rows.iter().map(BotSessionVm::compose).collect())
}

/// One conversation and its messages (FR-382; Epic 63, AD-176, AD-181).
///
/// One command rather than two, so a header cannot render one conversation's
/// title over another's rows for a frame.
///
/// Where the row names a gateway session and the gateway has a session API,
/// the transcript is **the gateway's** — `GET /api/sessions/{id}/messages`,
/// which is the one copy both devices write to — and the answer says so
/// (`transcript: remote`). Where the endpoint keeps no sessions, or where its
/// history cannot be read right now, the transcript is keeper's own copy and
/// the answer says that instead. Neither case is an error: a gateway that is
/// down does not make the local record wrong.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub async fn bots_session_open(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<BotConversationVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = session::get_session(&dir, &session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", &session_id))?;
    let local = session::list_messages(&dir, &session_id).map_err(to_ipc_error)?;
    if let Some(found) = remote_history(&plain_env(&state), &dir, &row).await {
        let messages = follow::merge(&local, &found, &row.id, &row.provider_id, row.updated_ms);
        return Ok(BotConversationVm {
            session: BotSessionVm::compose(&row),
            messages: messages.iter().map(BotMessageVm::compose).collect(),
            transcript: BotTranscriptSource::Remote,
        });
    }
    Ok(BotConversationVm {
        session: BotSessionVm::compose(&row),
        messages: local.iter().map(BotMessageVm::compose).collect(),
        transcript: BotTranscriptSource::Local,
    })
}

/// Read the conversation another device may be writing, and say when to read
/// it again (Epic 63, Story 63.7, FR-425, FR-426, AD-177).
///
/// One history read of the route [`bots_session_open`] already uses — never
/// `GET /v1/runs/{id}/events`, whose queue is destroyed by a second reader —
/// folded under `keeper_core::bots::follow::merge`'s rule, with
/// `keeper_core::bots::follow::decide` saying whether the webview should ask
/// again and after how long. The webview owns the timer, so a conversation
/// that leaves the screen stops being read the moment it does, and nothing
/// here outlives the pane.
///
/// `owns_turn` is the shell's own answer, not the webview's: a turn this
/// process is streaming is in [`streams`], and a device that has the stream
/// does not read the transcript underneath it.
///
/// A transcript that could not be read — a local conversation, a gateway that
/// stopped answering — is [`BotFollowVm::UNREAD`]: what is on screen stands,
/// and the following stops rather than retrying against a host that is down.
///
/// Rejects with: `internal` (unknown id).
#[tauri::command]
pub async fn bots_session_follow(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<BotFollowVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = session::get_session(&dir, &session_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| no_such("conversation", &session_id))?;
    let Some(found) = remote_history(&plain_env(&state), &dir, &row).await else {
        return Ok(BotFollowVm::UNREAD);
    };
    let local = session::list_messages(&dir, &session_id).map_err(to_ipc_error)?;
    let messages = follow::merge(&local, &found, &row.id, &row.provider_id, row.updated_ms);
    let live = follow::turn_open(&found);
    let next = follow::decide(&follow::FollowSignals {
        now_ms: now_ms(),
        owns_turn: drive::owns_turn(&session_id),
        turn_open: live,
        newest_ms: follow::newest_ms(&found),
        last_active_ms: row.remote_last_active_ms,
        updated_ms: row.updated_ms,
    });
    Ok(BotFollowVm::compose(&messages, live, next))
}

/// The gateway's history for a conversation, or `None` when keeper reads its
/// own copy instead.
///
/// `None` covers every reason at once — no remote id, no session API, an
/// unknown bot or provider, a gateway that did not answer — because the caller
/// has exactly one alternative and it is the same for all of them. The
/// failure is logged at debug: it is the ordinary state of an endpoint that
/// lacks the feature, not a fault.
async fn remote_history(
    env: &TurnEnv,
    dir: &Path,
    row: &session::BotSession,
) -> Option<Vec<remote::RemoteMessage>> {
    let remote_id = row.remote_session_id.as_deref()?;
    let provider = store::get_provider(dir, &row.provider_id).ok().flatten()?;
    let caps = turn::session_caps(env, &provider).await;
    if remote::transcript_source(caps, row) != BotTranscriptSource::Remote {
        return None;
    }
    let bot = store::get_bot(dir, &row.bot_id).ok().flatten()?;
    let endpoint = turn::endpoint_of(env, &provider, Some(&bot.target))
        .await
        .ok()?;
    let client = discover::discovery_client().ok()?;
    match remote::fetch_messages(&client, &endpoint, remote_id).await {
        Ok(found) => Some(found),
        Err(error) => {
            tracing::debug!(%error, "bots: gateway transcript not read; replaying keeper's copy");
            None
        }
    }
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
    Ok(BotSessionListVm::compose(&page, |provider_id| {
        turn::cached_caps(&dir, provider_id)
    }))
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
/// **No remote request is made.** A delete is a local transaction; a Hermes
/// session id on the row names something on a server keeper never owned and
/// cannot speak for. What the delete does remember is the dismissal, so the
/// next list read does not adopt the gateway's copy straight back
/// (`session::delete_session`, Epic 63).
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
    session::delete_session(&dir, &session_id, now_ms()).map_err(to_ipc_error)
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
// Streaming (FR-372, FR-373, FR-374) — the loop is `keeper_agent`'s
// ---------------------------------------------------------------------------

/// Ask a bot, streaming the answer over `channel`, and return the subscription
/// id (FR-372).
///
/// What has already happened by the time this resolves: the conversation exists
/// (created here when `req.sessionId` is absent, because the first message is
/// what mints the title), the user's message is stored, and the assistant row
/// is stored **empty and partial**. All three are on the first
/// [`BotStreamEvent::Opened`] event, which is emitted before the request goes
/// out. Whether the turn is spoken is the voice turn's answer, read where
/// arming reaches it ([`crate::agent_ports::origin_of`]).
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
    let base: Arc<dyn TurnSink> = Arc::new(ChannelSink(channel));
    let env = crate::agent_ports::turn_env(&state, Some(Arc::clone(&base)));
    // Typed while a voice turn waits, the send is that turn's answer: its
    // question as the send arms.
    let question = crate::voice_ipc::question_now();
    let opened = turn::open_turn(&env, req, &crate::agent_ports::origin_of)
        .await
        .map_err(agent_error)?;
    let sink = sink_for(&opened, base, question);
    Ok(drive::spawn_turn(opened, sink))
}

/// Send what the voice turn heard as `question` (Epic 67, Story 67.1,
/// AD-205, AD-206).
///
/// Called by `voice_ipc::transition` when the turn hands out
/// `Effect::SendText`, with the question's generation. Where the question
/// goes is `keeper_core::bots::voice_target::resolve`'s answer over
/// `bots.voice_target`, the pinned bots, the conversation list and the
/// person's proxy conversations — never what is open on the screen. A bot
/// target's model is `voice_target::model_for`'s; this function gathers the
/// facts and then opens the turn exactly as a typed send would, so there is
/// one stream code path. The voice turn is in `Heard`, so the turn's origin
/// is spoken, which routes its close back into the voice turn. The stream
/// goes to the app-wide [`crate::agent_ports::SPOKEN_STREAM_EVENT`], for the
/// pane when there is one. An agent room target (`agent:<room id>`, AD-384)
/// is [`crate::agents_ipc::send_spoken`]'s: the question goes into that
/// room as the person's message and its answer is followed there.
///
/// A refusal — no bot to talk to, no model to send with, a chosen agent room
/// that is not the person's proxy conversation, a send that never opened —
/// ends the turn through `voice_ipc::answer_failed` with the sentence, which
/// puts it beside the switch (AD-190) and in the ring (AD-192). Nothing is
/// guessed and nothing is sent to a bot nobody chose. Every report names
/// `question`: one the person has since stopped or replaced moves nothing.
pub async fn send_spoken(app: &AppHandle, question: u64, text: String) {
    // A new question: an earlier one's answer is not this turn's to speak.
    crate::agents_ipc::drop_spoken_answer();
    let state = app.state::<AppState>();
    let outcome = match spoken_target_now(&state).await {
        Ok(SpokenTarget::Agent {
            account_id,
            room_id,
        }) => {
            crate::agents_ipc::send_spoken(app, question, account_id, room_id, text);
            return;
        }
        Ok(SpokenTarget::Bot {
            bot,
            session_id,
            history,
        }) => spoken_request(&state, bot, session_id, history, &text).await,
        Err(refusal) => Err(refusal),
    };
    let req = match outcome {
        Ok(req) => req,
        Err(refusal) => {
            tracing::warn!(message = %refusal.message, "bots: a spoken turn was refused");
            crate::voice_ipc::answer_failed(question, refusal.message);
            return;
        }
    };
    tracing::info!(bot = %req.bot_id, session = ?req.session_id, "bots: sending what the voice turn heard");
    // An ask goes down the spoken stream, where the pane's sheet answers it.
    let base: Arc<dyn TurnSink> = Arc::new(EventSink(app.clone()));
    let env = crate::agent_ports::turn_env(&state, Some(Arc::clone(&base)));
    match turn::open_turn(&env, req, &crate::agent_ports::origin_of).await {
        Ok(opened) => {
            let sink = sink_for(&opened, base, Some(question));
            drive::spawn_turn(opened, sink);
        }
        Err(error) => crate::voice_ipc::answer_failed(question, agent_error(error).message),
    }
}

/// The request a spoken turn sends to `bot`: its conversation (or none, for
/// a new one) and the model, resolved from the store.
async fn spoken_request(
    state: &AppState,
    bot: Bot,
    session_id: Option<String>,
    history: Vec<session::BotMessage>,
    text: &str,
) -> Result<BotChatSendReq, IpcError> {
    let dir = data_dir(state)?;
    let kind = provider_of(&dir, &bot.provider_id)?.provider.kind;
    // The endpoint's list is asked only when neither the conversation nor
    // the provider names a model of its own (AD-217's first two rungs) — a
    // network round trip spent where it can change the answer, `arm_turn`'s
    // rule.
    let offered = match voice_target::model_for(&bot, kind, &history, &[]) {
        Ok(_) => Vec::new(),
        Err(_) => offered_models(&plain_env(state), &dir, &bot).await,
    };
    let model = voice_target::model_for(&bot, kind, &history, &offered)
        .map_err(|refusal| refused(refusal.message()))?;
    Ok(BotChatSendReq {
        session_id,
        bot_id: bot.id,
        model,
        text: text.to_owned(),
        attachment_ids: Vec::new(),
    })
}

/// Where a spoken turn goes, with what sending there needs.
#[derive(Debug)]
enum SpokenTarget {
    /// A pinned bot, its conversation (none: a new one) and that
    /// conversation's messages.
    Bot {
        bot: Bot,
        session_id: Option<String>,
        history: Vec<session::BotMessage>,
    },
    /// One of the person's proxy conversations, on the account that reads
    /// it as theirs.
    Agent { account_id: String, room_id: String },
}

/// [`spoken_target`] over the store and, when the choice names an agent
/// room, every live account's proxy conversations — listed only then.
async fn spoken_target_now(state: &AppState) -> Result<SpokenTarget, IpcError> {
    let dir = data_dir(state)?;
    let chosen = registry::get_bots_voice_target(&dir).map_err(to_ipc_error)?;
    let rooms = match chosen.as_deref().and_then(voice_target::agent_room_of) {
        Some(_) => state.accounts.agent_rooms_everywhere().await,
        None => Vec::new(),
    };
    spoken_target(&dir, &rooms)
}

/// Where a spoken turn goes, read from the store: `bots.voice_target`, the
/// pinned bots, the live conversations newest first and `rooms` — the
/// person's proxy conversations by account — go to `voice_target::resolve`;
/// a bot comes back with its row and the target conversation's messages
/// (empty for a new one). A refusal is the sentence, as an `IpcError` the
/// caller hands to the turn.
fn spoken_target(dir: &Path, rooms: &[(String, ProxyRoomVm)]) -> Result<SpokenTarget, IpcError> {
    let chosen = registry::get_bots_voice_target(dir).map_err(to_ipc_error)?;
    let bots = store::list_bots(dir).map_err(to_ipc_error)?;
    let sessions = session::list_sessions(dir, false).map_err(to_ipc_error)?;
    let room_ids: Vec<String> = rooms.iter().map(|(_, room)| room.room_id.clone()).collect();
    match voice_target::resolve(chosen.as_deref(), &bots, &sessions, &room_ids)
        .map_err(|refusal| refused(refusal.message()))?
    {
        voice_target::VoiceTarget::Agent { room_id } => {
            let (account_id, _) = rooms
                .iter()
                .find(|(_, room)| room.room_id == room_id)
                .ok_or_else(|| refused(voice_target::AGENT_ROOM_GONE_SENTENCE.to_owned()))?;
            Ok(SpokenTarget::Agent {
                account_id: account_id.clone(),
                room_id,
            })
        }
        voice_target::VoiceTarget::Bot { bot_id, session_id } => {
            let bot = bot_of(dir, &bot_id)?;
            let history = match &session_id {
                Some(id) => session::list_messages(dir, id).map_err(to_ipc_error)?,
                None => Vec::new(),
            };
            Ok(SpokenTarget::Bot {
                bot,
                session_id,
                history,
            })
        }
    }
}

/// Every model the bot's endpoint lists right now, or none when keeper
/// could not ask — `discovered_model`'s read, whole.
async fn offered_models(env: &TurnEnv, dir: &Path, bot: &Bot) -> Vec<BotModelVm> {
    let Ok(row) = provider_of(dir, &bot.provider_id) else {
        return Vec::new();
    };
    let Ok(endpoint) = turn::endpoint_of(env, &row, Some(&bot.target)).await else {
        return Vec::new();
    };
    let Ok(client) = http::client(turn::read_timeout_of(&row)) else {
        return Vec::new();
    };
    discover::models(&client, &endpoint)
        .await
        .unwrap_or_default()
}

/// A spoken turn's refusal is the person's to act on, so the message is the
/// point; `internal` only because the taxonomy has no better word
/// (`voice_ipc::refused`'s reasoning).
fn refused(message: String) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message,
        account_id: None,
        retriable: false,
    }
}

/// Re-ask the question one assistant row failed to answer (FR-372).
///
/// The failed row is **replaced**, not appended beside, and `req.messageId`
/// names the row explicitly rather than being inferred as "the last one", so
/// a Retry pressed on a stale render cannot delete a row that arrived after it
/// was drawn. A row read from the gateway's transcript rather than written
/// here (Epic 63, AD-181) cannot be retried from this device.
///
/// Rejects with: `internal` (unknown conversation or message, or a message that
/// is not an assistant row), `unsupported` (an answer another device holds).
#[tauri::command]
pub async fn bots_message_retry(
    state: State<'_, AppState>,
    req: BotRetryReq,
    channel: Channel<BotStreamEvent>,
) -> Result<String, IpcError> {
    let base: Arc<dyn TurnSink> = Arc::new(ChannelSink(channel));
    let env = crate::agent_ports::turn_env(&state, Some(Arc::clone(&base)));
    let question = crate::voice_ipc::question_now();
    let opened = turn::open_retry(&env, req, &crate::agent_ports::origin_of)
        .await
        .map_err(agent_error)?;
    let sink = sink_for(&opened, base, question);
    Ok(drive::spawn_turn(opened, sink))
}

/// Stop a streaming answer by subscription id (FR-372).
///
/// Idempotent: an id that already finished, or one whose window closed, is a
/// no-op — a racing unmount has no way to know which happened and should not
/// have to. It fires the cancel handle rather than aborting the task, so the
/// driver writes what had arrived as a partial row.
///
/// Rejects with: nothing.
#[tauri::command]
pub fn bots_chat_stop(subscription_id: String) -> Result<(), IpcError> {
    drive::stop(&subscription_id);
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
    // Shape, colour and mark travel in the person's `bots.toml`.
    crate::account_ipc::note_local_change();
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
    crate::account_ipc::note_local_change();
    let bots = store::list_bots(&dir).map_err(to_ipc_error)?;
    Ok(bots.iter().map(BotVm::compose).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A scratch data dir no other test can land in (the store tests' helper,
    /// verbatim: pid, nanosecond stamp and a counter).
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-spoken-test-{}-{}-{n}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    fn pin(dir: &Path, id: &str) {
        store::insert_bot(
            dir,
            &Bot {
                id: id.to_owned(),
                provider_id: "p1".to_owned(),
                target: id.to_owned(),
                name: format!("bot {id}"),
                pin_order: 0,
                identity: keeper_core::bots::BotIdentity::default(),
                created_ms: 0,
            },
        )
        .expect("pin a bot");
    }

    fn talk(dir: &Path, session_id: &str, bot_id: &str, updated_ms: i64, model: &str) {
        session::insert_session(
            dir,
            &session::BotSession {
                id: session_id.to_owned(),
                bot_id: bot_id.to_owned(),
                provider_id: "p1".to_owned(),
                title: session_id.to_owned(),
                created_ms: updated_ms,
                updated_ms,
                archived: false,
                remote_session_id: None,
                remote_last_active_ms: None,
                remote_source: None,
            },
        )
        .expect("insert a conversation");
        turn::store_message(dir, session_id, "user", "hello", None, false, updated_ms).expect("q");
        turn::store_message(
            dir,
            session_id,
            "assistant",
            "hi",
            Some((model, "p1")),
            false,
            updated_ms,
        )
        .expect("a");
    }

    /// Epic 67 (AD-206): the spoken turn's target is read from the store the
    /// way the conversation list reads it — newest activity first, pinned
    /// bots only, the chosen bot when one is set — and the refusal when there
    /// is none is core's sentence.
    #[test]
    fn the_spoken_target_is_read_from_the_store_and_refused_when_there_is_none() {
        let dir = temp_dir();
        store::insert_provider(
            &dir,
            &Provider {
                id: "p1".to_owned(),
                kind: keeper_core::bots::ProviderKind::Ollama,
                name: "local".to_owned(),
                base_url: "http://localhost:11434".to_owned(),
                created_ms: 0,
            },
        )
        .expect("provider");

        // Nothing pinned, nothing talked to: the sentence, and no bot.
        let refused = spoken_target(&dir, &[]).expect_err("nothing to talk to");
        assert_eq!(
            refused.message,
            keeper_core::bots::voice_target::NO_TARGET_SENTENCE
        );

        pin(&dir, "a");
        pin(&dir, "b");
        talk(&dir, "s1", "a", 10, "llama4:8b");
        talk(&dir, "s2", "b", 20, "qwen3");

        let bot_of_target = |target: SpokenTarget| match target {
            SpokenTarget::Bot {
                bot,
                session_id,
                history,
            } => (bot, session_id, history),
            SpokenTarget::Agent { .. } => panic!("a bot target"),
        };

        // Unset: the pinned bot most recently talked to, with its
        // conversation and the model that answered there.
        let (bot, session_id, history) =
            bot_of_target(spoken_target(&dir, &[]).expect("b was talked to last"));
        assert_eq!(bot.id, "b");
        assert_eq!(session_id.as_deref(), Some("s2"));
        assert_eq!(
            voice_target::model_for(&bot, keeper_core::bots::ProviderKind::Ollama, &history, &[]),
            Ok("qwen3".to_owned())
        );

        // Chosen: that bot, its own conversation.
        registry::set_bots_voice_target(&dir, Some("a")).expect("choose a");
        let (bot, session_id, history) =
            bot_of_target(spoken_target(&dir, &[]).expect("a is chosen"));
        assert_eq!(bot.id, "a");
        assert_eq!(session_id.as_deref(), Some("s1"));
        assert_eq!(
            voice_target::model_for(&bot, keeper_core::bots::ProviderKind::Ollama, &history, &[]),
            Ok("llama4:8b".to_owned())
        );

        // A chosen bot that was unpinned since is no choice: back to b.
        store::delete_bot(&dir, "a").expect("unpin a");
        let (bot, _, _) = bot_of_target(spoken_target(&dir, &[]).expect("b remains"));
        assert_eq!(bot.id, "b");

        // A chosen agent room goes to the account that reads it as its
        // proxy's; one no account lists is refused, never sent to b.
        registry::set_bots_voice_target(&dir, Some("agent:!dm:example.org")).expect("choose");
        let rooms = [(
            "acct".to_owned(),
            ProxyRoomVm {
                room_id: "!dm:example.org".to_owned(),
                name: "Nixi".to_owned(),
                kind: keeper_core::agents::session::SessionKind::Main,
                agent: "@nixi:example.org".to_owned(),
                allowed: None,
            },
        )];
        match spoken_target(&dir, &rooms).expect("the proxy room") {
            SpokenTarget::Agent {
                account_id,
                room_id,
            } => assert_eq!(
                (account_id.as_str(), room_id.as_str()),
                ("acct", "!dm:example.org")
            ),
            other => panic!("not the agent room: {other:?}"),
        }
        let refused = spoken_target(&dir, &[]).expect_err("no account lists it");
        assert_eq!(
            refused.message,
            keeper_core::bots::voice_target::AGENT_ROOM_GONE_SENTENCE
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
