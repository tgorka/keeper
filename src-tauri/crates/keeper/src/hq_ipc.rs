//! Shell call sites of `keeper_core::hq` (AD-418, AD-422, AD-424): the
//! process's one [`Hq`], started once the settings layers are installed and
//! reconciled whenever a setting or a settings layer changes, its commands,
//! the pairing link a scanned QR opens, and the Agents window's streams.
//! Every decision — the address grammar, the pairing, which databases sync,
//! the list, its order and attention, every sentence — is core's; this holds
//! it and streams what it answers.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use keeper_core::hq::approvals::{AgentApprovalVm, AgentDecisionOutcomeVm, ShownApprovalVm};
use keeper_core::hq::default_agent::DefaultAgentVm;
use keeper_core::hq::grants::{AgentGrantsVm, AlwaysAllow};
use keeper_core::hq::instances::AgentInstancesVm;
use keeper_core::hq::live::{AgentViewPrefsPatch, AgentViewPrefsVm};
use keeper_core::hq::members::AgentSessionMembersVm;
use keeper_core::hq::push::PushTarget;
use keeper_core::hq::routes::HttpRoutes;
use keeper_core::hq::send::{AgentFocus, AgentNoteCaret};
use keeper_core::hq::sessions::{
    diff, AgentChoiceVm, AgentDefaultSessionVm, AgentNoteSessionVm, AgentSessionRef,
    AgentSessionsBatch, AgentSessionsOp,
};
use keeper_core::hq::signin::OrgAccount;
use keeper_core::hq::spoken::{FollowSlot, SpokenStep};
use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::timeline::AgentSessionViewVm;
use keeper_core::hq::vm::{HqEnrolmentVm, HqPairCodeVm, HqStatusVm};
use keeper_core::hq::{Hq, HqError};
use keeper_core::platform::Platform;
use keeper_core::vm::{IpcError, IpcErrorCode, VoiceAgentTargetVm};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

/// The event a pairing link raises, so the window opens Settings on it.
pub const PAIR_OFFER_EVENT: &str = "hq-pair-offer";

/// The process's agents server service, set once by [`init`].
static HQ: OnceLock<Arc<Hq>> = OnceLock::new();

/// A live stream: its task, and the session it holds open, if any.
struct Stream {
    task: JoinHandle<()>,
    session: Option<AgentSessionRef>,
}

/// The live subscriptions, so an unmounting webview can end its own.
static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<String, Stream>>> = LazyLock::new(Mutex::default);

/// Start a stream under a new id and keep it. The map is held while it
/// starts, so a stream that ends itself at once (`end(&id)`) finds itself.
fn keep(session: Option<AgentSessionRef>, start: impl FnOnce(String) -> JoinHandle<()>) -> String {
    let id = ulid::Ulid::new().to_string();
    let mut streams = SUBSCRIPTIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let task = start(id.clone());
    streams.insert(id.clone(), Stream { task, session });
    id
}

/// End a stream by id; an unknown id is a no-op. A session it held open is
/// let go.
fn end(subscription_id: &str) {
    let stream = SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(subscription_id);
    if let Some(stream) = stream {
        stream.task.abort();
        if let (Some(session), Some(hq)) = (stream.session, HQ.get()) {
            hq.close_session(&session);
        }
    }
}

/// The push target the shell asked for before the service existed.
static PUSH_TARGET: Mutex<Option<Option<PushTarget>>> = Mutex::new(None);

/// Build the service and open what this device holds. Called from setup
/// after the settings layers are installed, so `agents.server_url` is read
/// through them.
pub fn init(app: &tauri::App) {
    ensure(app.handle());
}

/// The service, built and started now if it is not yet: setup's, or — on
/// the iPhone, when an action on a notification launched keeper in the
/// background and no setup runs — the only thing that launch opens. Either
/// way the settings layers are installed first
/// ([`crate::install_settings_layers`], a no-op after setup's), so
/// `agents.server_url` is the effective setting — a file's address, never a
/// stored one it shadows — and not the SQLite row alone (R1066K-03).
pub fn ensure(app: &tauri::AppHandle) -> Arc<Hq> {
    if let Some(hq) = HQ.get() {
        return Arc::clone(hq);
    }
    let platform = Arc::clone(&app.state::<crate::ipc::AppState>().platform);
    crate::install_settings_layers(platform.as_ref());
    // The account's sign-in enrols this device (109.6); core decides when
    // and to which server, the shell only names the account and its client.
    let sign_in = Arc::new(OrgAccount::new(
        Arc::clone(&platform),
        crate::account_ipc::descriptor,
        || crate::account_ipc::http().cloned(),
    ));
    let hq = Hq::new(
        platform,
        Arc::new(SyncularOpener),
        Arc::new(HttpRoutes::new()),
        sign_in,
        "keeper".to_owned(),
        crate::account_ipc::device_host,
    );
    if HQ.set(Arc::clone(&hq)).is_err() {
        return HQ.get().map_or(hq, Arc::clone);
    }
    // An approved drive write is committed into this device's own checkout
    // as the person (109.27).
    hq.set_drive_writes(Arc::new(crate::hq_apply::ShellDriveWrites::app()));
    let started = Arc::clone(&hq);
    let pending = PUSH_TARGET
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    tauri::async_runtime::spawn(async move {
        if let Some(target) = pending {
            started.set_push_target(target);
        }
        if let Err(error) = started.start().await {
            tracing::warn!(%error, "the agents server could not be started");
        }
    });
    hq
}

/// Where the agents server's push reaction posts for this device (106.6):
/// the phone's push shell calls this with its gateway, token and app, or
/// `None` when the gateway is cleared. Kept until the service starts.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn set_push_target(target: Option<PushTarget>) {
    match HQ.get() {
        Some(hq) => {
            let hq = Arc::clone(hq);
            tauri::async_runtime::spawn(async move { hq.set_push_target(target) });
        }
        None => {
            *PUSH_TARGET.lock().unwrap_or_else(PoisonError::into_inner) = Some(target);
        }
    }
}

/// Read and classify the agents item a push names (Android, where the push
/// reaches keeper itself).
#[cfg(target_os = "android")]
pub async fn read_pushed(
    pushed: &keeper_core::hq::push::PushedItem,
) -> Result<keeper_core::hq::push::HqClassified, String> {
    let hq = HQ
        .get()
        .cloned()
        .ok_or_else(|| "the agents server is not started yet".to_owned())?;
    hq.read_pushed(pushed).await
}

/// Write the decision an action on an agents item's notification sends,
/// within `budget`; the service is started first when this launch has none.
/// Blocking: never on the async workers.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn decide_pushed(
    app: &tauri::AppHandle,
    decision: &keeper_core::hq::push::HqActionDecision,
    budget: std::time::Duration,
) -> Result<AgentDecisionOutcomeVm, String> {
    ensure(app)
        .decide_pushed(decision, budget)
        .map_err(|error| error.to_string())
}

/// The effective settings may have changed (a setting written, a settings
/// layer installed or cleared, a settings file pulled): follow
/// `agents.server_url` if it moved, and the view preference
/// (`agents.view_mode`). Before [`init`] there is
/// nothing to follow.
pub fn reconcile() {
    let Some(hq) = HQ.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = hq.reconcile().await {
            tracing::warn!(%error, "the agents server did not follow its setting");
        }
    });
}

/// The agents servers a request may still reach, for the egress list.
pub fn contacting() -> Vec<String> {
    HQ.get().map(|hq| hq.contacting()).unwrap_or_default()
}

/// A `keeper://agents/pair` link opened this app (a scanned QR): Rust checks
/// it and keeps it; the window opens Settings, where the person confirms.
pub fn link_arrived(app: &tauri::AppHandle, link: &str) {
    let Some(hq) = HQ.get() else {
        tracing::warn!("a pairing link arrived before the agents server started");
        return;
    };
    if let Err(error) = hq.offer_link(link) {
        tracing::info!(%error, "a pairing link was refused");
        return;
    }
    if let Err(error) = app.emit(PAIR_OFFER_EVENT, ()) {
        tracing::warn!(%error, "the pairing link could not be shown");
    }
    #[cfg(desktop)]
    crate::tray::show_main_window(app);
}

fn hq() -> Result<&'static Arc<Hq>, IpcError> {
    HQ.get().ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: "the agents server is not started yet".to_owned(),
        account_id: None,
        retriable: true,
    })
}

/// The service, once started: what the `keeper-hq-blob://` handler serves
/// through.
pub(crate) fn service() -> Option<Arc<Hq>> {
    HQ.get().cloned()
}

/// An hq refusal as the webview sees it: one still [`HqError::Arriving`] is
/// retriable — the caller asks again when the list or the status moves.
fn error(error: HqError) -> IpcError {
    let retriable = matches!(error, HqError::Arriving(_));
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable,
    }
}

/// Stream the agents server's status — the databases, the devices, a pending
/// pairing link — at once, then every change.
#[tauri::command]
pub fn hq_status_subscribe(channel: Channel<HqStatusVm>) -> Result<String, IpcError> {
    let mut status = hq()?.status();
    let current = status.borrow_and_update().clone();
    if let Err(error) = channel.send(current) {
        tracing::warn!(%error, "the agents server status subscription could not start");
    }
    let start = move |_: String| {
        tauri::async_runtime::spawn(async move {
            while status.changed().await.is_ok() {
                let current = status.borrow_and_update().clone();
                if channel.send(current).is_err() {
                    return;
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`hq_status_subscribe`] stream. An unknown id is a no-op.
#[tauri::command]
pub fn hq_status_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Set `agents.server_url`: blank clears it; a refused address is returned
/// as its sentence and nothing is stored.
#[tauri::command]
pub async fn hq_server_set(url: String) -> Result<HqStatusVm, IpcError> {
    hq()?.set_server(&url).await.map_err(error)
}

/// Forget the server: this device's copies of it, then its pairing.
#[tauri::command]
pub async fn hq_server_forget() -> Result<HqStatusVm, IpcError> {
    hq()?.forget().await.map_err(error)
}

/// A code (and its QR) another device of this person can redeem.
#[tauri::command]
pub async fn hq_pair_start() -> Result<HqPairCodeVm, IpcError> {
    hq()?.pair_start().await.map_err(error)
}

/// Redeem a code, or a pairing link, typed or pasted on this device.
#[tauri::command]
pub async fn hq_pair_redeem(code: String) -> Result<HqStatusVm, IpcError> {
    hq()?.pair_redeem(&code).await.map_err(error)
}

/// Pair with the server of the link a scanned QR opened.
#[tauri::command]
pub async fn hq_pair_offer_accept() -> Result<HqStatusVm, IpcError> {
    hq()?.accept_offer().await.map_err(error)
}

/// Forget the link a scanned QR opened.
#[tauri::command]
pub fn hq_pair_offer_dismiss() -> Result<(), IpcError> {
    hq()?.dismiss_offer();
    Ok(())
}

/// Remove one of the person's devices from the server; the status after.
#[tauri::command]
pub async fn hq_device_revoke(client_id: String) -> Result<HqStatusVm, IpcError> {
    hq()?.revoke(&client_id).await.map_err(error)
}

/// The person's newest "new device" notices from their other devices
/// (S-ID-8): signed in, adopted, or paired with a code.
#[tauri::command]
pub async fn hq_enrolments() -> Result<Vec<HqEnrolmentVm>, IpcError> {
    hq()?.enrolments().await.map_err(error)
}

/// Run a blocking core call off the async workers.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce(Arc<Hq>) -> Result<T, HqError> + Send + 'static,
) -> Result<T, IpcError> {
    let hq = Arc::clone(hq()?);
    tauri::async_runtime::spawn_blocking(move || call(hq))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: join.to_string(),
            account_id: None,
            retriable: true,
        })?
        .map_err(error)
}

fn session(database: String, session_id: String) -> AgentSessionRef {
    AgentSessionRef {
        database,
        session_id,
    }
}

/// The drive, and the path from its root, of the note `note_id` of the
/// vault `vault_id` (109.28): the vault's synced folder's `_drive.toml` and
/// the note's vault-relative path, joined by
/// [`keeper_agent::surface::drive_of`] (AD-65) — never by string arithmetic
/// here. `None` when the folder declares no drive, or the note is not a file
/// of it. Blocking: it reads the folder.
fn note_drive(
    platform: &Arc<dyn Platform>,
    vault_id: &str,
    note_id: &str,
) -> Option<keeper_agent::surface::DriveRef> {
    let vault = crate::notes_vault::vault(vault_id)?;
    let path = crate::notes_vault::snapshot(vault_id)?
        .by_id(note_id)?
        .path
        .clone();
    let profiles =
        match crate::sync::engine(Arc::clone(platform)).and_then(|engine| engine.list_profiles()) {
            Ok(profiles) => profiles,
            Err(error) => {
                tracing::debug!(%error, "the synced folders could not be read for a note's drive");
                return None;
            }
        };
    let profile = profiles.iter().find(|profile| profile.id == vault.id)?;
    keeper_agent::surface::drive_of(profile, &vault.config.subfolder, &path)
}

/// What a message sent from the Notes Assistant carries as
/// `metadata.focus` (109.28): the note's drive and path ([`note_drive`]) and
/// the editor's caret; none when the note is on no drive.
fn focus_of(platform: &Arc<dyn Platform>, caret: Option<&AgentNoteCaret>) -> Option<AgentFocus> {
    let caret = caret?;
    let note = note_drive(platform, &caret.vault_id, &caret.note_id)?;
    AgentFocus::new(&note.drive, &note.path, &caret.text, caret.line)
}

/// Stream the Agents window's rows — every held database merged, read from
/// the replicas — as a reset, then the changes.
#[tauri::command]
pub fn agents_sessions_subscribe(channel: Channel<AgentSessionsBatch>) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    let mut changes = hq.changes();
    let start = move |_: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown = Vec::new();
            let mut first = true;
            loop {
                let read = Arc::clone(&hq);
                let Ok(rows) = tauri::async_runtime::spawn_blocking(move || read.sessions()).await
                else {
                    return;
                };
                let ops = if first {
                    vec![AgentSessionsOp::Reset { rows: rows.clone() }]
                } else {
                    diff(&shown, &rows)
                };
                first = false;
                if !ops.is_empty() {
                    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
                    if channel.send(AgentSessionsBatch { ops, total }).is_err() {
                        return;
                    }
                }
                shown = rows;
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`agents_sessions_subscribe`] stream.
#[tauri::command]
pub fn agents_sessions_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Open a session: stream its view — messages, approvals, what is still
/// arriving — at once, then whole on every change.
#[tauri::command]
pub fn agent_session_open(
    database: String,
    session_id: String,
    channel: Channel<AgentSessionViewVm>,
) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    let key = session(database, session_id);
    hq.open_session(&key).map_err(error)?;
    let mut changes = hq.changes();
    let mut tails = hq.live_changes();
    let held = key.clone();
    let start = move |id: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown: Option<AgentSessionViewVm> = None;
            loop {
                let (read, at) = (Arc::clone(&hq), held.clone());
                let Ok(view) =
                    tauri::async_runtime::spawn_blocking(move || read.session_view(&at)).await
                else {
                    return;
                };
                match view {
                    Ok(view) if shown.as_ref() != Some(&view) => {
                        if channel.send(view.clone()).is_err() {
                            end(&id);
                            return;
                        }
                        shown = Some(view);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "an open agents session could not be read");
                    }
                }
                // Read again on its own when keeper-core says so: an answer
                // still arriving (one whose instance stopped says so), or a
                // claim whose lease lapses with nothing else moving.
                let recheck = hq.session_recheck(&held, shown.as_ref());
                let woke = async {
                    tokio::select! {
                        changed = changes.changed() => changed.is_ok(),
                        changed = tails.changed() => changed.is_ok(),
                    }
                };
                match recheck {
                    Some(wait) => {
                        if let Ok(false) = tokio::time::timeout(wait, woke).await {
                            return;
                        }
                    }
                    None => {
                        if !woke.await {
                            return;
                        }
                    }
                }
            }
        })
    };
    Ok(keep(Some(key), start))
}

/// Close an [`agent_session_open`] stream.
#[tauri::command]
pub fn agent_session_close(subscription_id: String) {
    end(&subscription_id);
}

/// Change what this viewer sees: only the fields present are written; the
/// answer is what holds after (a settings file keeps a preference it sets),
/// and every open session is drawn again with it (`AgentSessionViewVm.prefs`).
#[tauri::command]
pub async fn agent_view_prefs_set(
    patch: AgentViewPrefsPatch,
) -> Result<AgentViewPrefsVm, IpcError> {
    blocking(move |hq| hq.set_view_prefs(patch)).await
}

/// Switch a tool the agent offers off or back on for one session.
#[tauri::command]
pub async fn agent_session_tool_set(
    database: String,
    session_id: String,
    name: String,
    on: bool,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_session_tool(&session(database, session_id), &name, on)).await
}

/// Tick a drive for the session's agent to read, or untick it; keeper-core
/// decides whether this person may and writes `[]`, never a delete.
#[tauri::command]
pub async fn agent_session_drive_set(
    database: String,
    session_id: String,
    drive: String,
    tick: bool,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_session_drive(&session(database, session_id), &drive, tick)).await
}

/// Change a session's model or effort, or reset both to the agent's
/// defaults (109.18). Answers what was written (the device's last pick for
/// the agent) and the sentence a change made while a turn runs owes the
/// person; a later server refusal arrives with the session's view
/// (`AgentSessionViewVm.settings.write`).
#[tauri::command]
pub async fn agent_session_settings_change(
    database: String,
    session_id: String,
    change: keeper_core::hq::settings::AgentSettingsChange,
) -> Result<keeper_core::hq::settings::AgentSettingsChangedVm, IpcError> {
    blocking(move |hq| hq.change_session_settings(&session(database, session_id), change)).await
}

/// The person scrolled to the top of an open session: earlier messages.
#[tauri::command]
pub fn agent_session_page_back(database: String, session_id: String) -> Result<(), IpcError> {
    hq()?
        .page_back(&session(database, session_id))
        .map_err(error)
}

/// Write a message into a session; it shows as waiting at once. `focus` is
/// the note open in front of the person in the Notes Assistant (109.28).
#[tauri::command]
pub async fn agent_send(
    state: tauri::State<'_, crate::ipc::AppState>,
    database: String,
    session_id: String,
    body: String,
    declines: Vec<ShownApprovalVm>,
    focus: Option<AgentNoteCaret>,
) -> Result<(), IpcError> {
    let platform = Arc::clone(&state.platform);
    blocking(move |hq| {
        let focus = focus_of(&platform, focus.as_ref());
        hq.send(
            &session(database, session_id),
            &body,
            &declines,
            focus.as_ref(),
        )
    })
    .await
}

/// The composer's line (109.24): `@<handle> …` starts that agent's thread
/// from this session; any other line is a message, as [`agent_send`],
/// carrying `focus`. Either declines first what the banner showed
/// (`declines`, Q109-12 a).
#[tauri::command]
pub async fn agent_send_line(
    state: tauri::State<'_, crate::ipc::AppState>,
    database: String,
    session_id: String,
    line: String,
    declines: Vec<ShownApprovalVm>,
    focus: Option<AgentNoteCaret>,
) -> Result<(), IpcError> {
    let platform = Arc::clone(&state.platform);
    blocking(move |hq| {
        let focus = focus_of(&platform, focus.as_ref());
        hq.send_line(
            &session(database, session_id),
            &line,
            &declines,
            focus.as_ref(),
        )
    })
    .await
}

/// What an `@` at the start of a session's draft offers (109.24): Rust's
/// rows, the same shape as the mid-text menu's.
#[tauri::command]
pub fn agent_mention_preview(
    database: String,
    session_id: String,
    before: String,
    after: String,
) -> Result<keeper_core::vm::ChatInlinePreviewVm, IpcError> {
    hq()?
        .mention_preview(&session(database, session_id), &before, &after)
        .map_err(error)
}

/// Rust decides both menu rows and how Enter interprets an agents draft.
#[tauri::command]
pub fn agent_command_preview(
    database: String,
    session_id: String,
    draft: String,
) -> Result<keeper_core::vm::ChatCommandPreviewVm, IpcError> {
    hq()?
        .command_preview(&session(database, session_id), &draft)
        .map_err(error)
}

/// Revalidate and execute an agents command against the current replica.
#[tauri::command]
pub async fn agent_command(
    database: String,
    session_id: String,
    draft: String,
    pick: Option<keeper_core::hq::settings::AgentSettingsPick>,
    declines: Vec<ShownApprovalVm>,
) -> Result<keeper_core::hq::commands::AgentCommandResultVm, IpcError> {
    blocking(move |hq| {
        hq.command(
            &session(database, session_id),
            &draft,
            pick.as_ref(),
            &declines,
        )
    })
    .await
}

/// Stage the files at `paths` (the paperclip, drag-and-drop) for a message
/// of a session: Rust reads each file, so no bytes cross IPC. A handle for
/// each, in order; a file over the contract's bound, or one keeper cannot
/// read, refuses them all with its sentence. Nothing is written until
/// [`agent_attach_commit`].
#[tauri::command]
pub async fn agent_attach_path(
    database: String,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<String>, IpcError> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    blocking(move |hq| hq.stage_paths(&session(database, session_id), &paths)).await
}

/// A pasted attachment as `agent_attach_bytes` receives it, read and
/// checked: its body copied out of the request only once its length is
/// within the contract's bound, so an oversized paste is refused with
/// core's sentence before a second copy of it is allocated.
#[derive(Debug)]
struct RawAttachment {
    database: String,
    session_id: String,
    mime: String,
    name: String,
    bytes: Vec<u8>,
}

fn raw_attachment(
    body: &tauri::ipc::InvokeBody,
    headers: &tauri::http::HeaderMap,
) -> Result<RawAttachment, IpcError> {
    let tauri::ipc::InvokeBody::Raw(borrowed) = body else {
        return Err(IpcError {
            code: IpcErrorCode::Internal,
            message: "a pasted attachment must be sent as a raw binary body".to_owned(),
            account_id: None,
            retriable: false,
        });
    };
    let name =
        crate::ipc::decode_header(headers, "x-filename").unwrap_or_else(|| "pasted".to_owned());
    keeper_core::hq::attachments::within_bound(&name, borrowed.len() as u64)
        .map_err(|refused| error(HqError::Sentence(refused.to_string())))?;
    let database = crate::ipc::required_header(headers, "x-database")?;
    let session_id = crate::ipc::required_header(headers, "x-session-id")?;
    let mime = crate::ipc::required_header(headers, "x-mime").unwrap_or_default();
    Ok(RawAttachment {
        database,
        session_id,
        mime,
        name,
        bytes: borrowed.clone(),
    })
}

/// Stage a pasted file for a message of a session; its handle. Its bytes
/// ride as a raw IPC body (`InvokeBody::Raw`, never base64 or JSON), as
/// `send_attachment_bytes`'s do; `x-database`, `x-session-id`, `x-mime` and
/// the percent-encoded `x-filename` ride in headers.
#[tauri::command]
pub async fn agent_attach_bytes(request: tauri::ipc::Request<'_>) -> Result<String, IpcError> {
    let RawAttachment {
        database,
        session_id,
        mime,
        name,
        bytes,
    } = raw_attachment(request.body(), request.headers())?;
    blocking(move |hq| hq.stage_bytes(&session(database, session_id), &name, &mime, bytes)).await
}

/// The composer's one Send: every staged file `handles` name, `caption` its
/// text, committed as one new message of the session — or nothing, with the
/// sentence. The handles are spent either way. `focus` is the note open in
/// front of the person, as [`agent_send`] carries it.
#[tauri::command]
pub async fn agent_attach_commit(
    state: tauri::State<'_, crate::ipc::AppState>,
    database: String,
    session_id: String,
    handles: Vec<String>,
    caption: Option<String>,
    declines: Vec<ShownApprovalVm>,
    focus: Option<AgentNoteCaret>,
) -> Result<(), IpcError> {
    let platform = Arc::clone(&state.platform);
    blocking(move |hq| {
        let focus = focus_of(&platform, focus.as_ref());
        hq.commit_attachments(
            &session(database, session_id),
            &handles,
            caption.as_deref().unwrap_or(""),
            &declines,
            focus.as_ref(),
        )
    })
    .await
}

/// The composer let go of staged files without sending them.
#[tauri::command]
pub fn agent_attach_release(
    database: String,
    session_id: String,
    handles: Vec<String>,
) -> Result<(), IpcError> {
    hq()?.release_attachments(&session(database, session_id), &handles);
    Ok(())
}

/// Open a session with an agent in one database, with its first message;
/// `pick` is this device's last model and effort for the agent (109.18).
/// `focus` is the note in front of the person when the Notes Assistant
/// starts it (109.28), as [`agent_send`] carries it.
#[tauri::command]
pub async fn agent_session_new(
    state: tauri::State<'_, crate::ipc::AppState>,
    database: String,
    agent_actor: String,
    body: String,
    pick: Option<keeper_core::hq::settings::AgentSettingsPick>,
    focus: Option<AgentNoteCaret>,
) -> Result<AgentSessionRef, IpcError> {
    let platform = Arc::clone(&state.platform);
    blocking(move |hq| {
        let focus = focus_of(&platform, focus.as_ref());
        hq.new_session(
            &database,
            &agent_actor,
            &body,
            pick.as_ref(),
            focus.as_ref(),
        )
    })
    .await
}

/// Rename the title without changing the session id.
#[tauri::command]
pub async fn agent_session_rename(
    database: String,
    session_id: String,
    title: String,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.rename_session(&session(database, session_id), &title)).await
}

/// Archive a session: its messages leave this device.
#[tauri::command]
pub async fn agent_session_archive(database: String, session_id: String) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_archived(&session(database, session_id), true)).await
}

/// Unarchive a session: its newest messages come back.
#[tauri::command]
pub async fn agent_session_unarchive(database: String, session_id: String) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_archived(&session(database, session_id), false)).await
}

/// The agents a new session can be opened with, and where it would go.
#[tauri::command]
pub async fn agents_choices() -> Result<Vec<AgentChoiceVm>, IpcError> {
    blocking(|hq| hq.choices()).await
}

/// The default agent's main conversation, for launch and the docks.
#[tauri::command]
pub async fn agent_session_default() -> Result<Option<AgentDefaultSessionVm>, IpcError> {
    blocking(|hq| hq.default_session()).await
}

/// The drive of the note `note_id` of the vault `vault_id` ([`note_drive`]),
/// or `None` when its folder declares none. Reads only: the Notes Assistant
/// asks it first, so a note on the drive it already follows changes nothing
/// (R605-02).
#[tauri::command]
pub async fn agent_note_drive(
    state: tauri::State<'_, crate::ipc::AppState>,
    vault_id: String,
    note_id: String,
) -> Result<Option<String>, IpcError> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        note_drive(&platform, &vault_id, &note_id).map(|note| note.drive)
    })
    .await
    .map_err(|join| IpcError {
        code: IpcErrorCode::Internal,
        message: join.to_string(),
        account_id: None,
        retriable: true,
    })
}

/// The Notes Assistant opened on a note (109.28, R-HQ-86): the note's drive
/// ([`note_drive`]), then core's answer — the default agent's chat in the
/// database named after that drive, started with `pick` when it has none,
/// and the drive ticked in it as the person, or why not.
#[tauri::command]
pub async fn agent_session_for_note(
    state: tauri::State<'_, crate::ipc::AppState>,
    vault_id: String,
    note_id: String,
    pick: Option<keeper_core::hq::settings::AgentSettingsPick>,
) -> Result<Option<AgentNoteSessionVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    blocking(move |hq| {
        let drive = note_drive(&platform, &vault_id, &note_id).map(|note| note.drive);
        hq.session_for_note(drive.as_deref(), pick.as_ref())
    })
    .await
}

/// Stream the approvals waiting for this person, in every held database.
#[tauri::command]
pub fn agent_approvals_subscribe(
    channel: Channel<Vec<AgentApprovalVm>>,
) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    let mut changes = hq.changes();
    let start = move |_: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown: Option<Vec<AgentApprovalVm>> = None;
            loop {
                let read = Arc::clone(&hq);
                let Ok(waiting) =
                    tauri::async_runtime::spawn_blocking(move || read.approvals_waiting()).await
                else {
                    return;
                };
                if shown.as_ref() != Some(&waiting) {
                    if channel.send(waiting.clone()).is_err() {
                        return;
                    }
                    shown = Some(waiting);
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`agent_approvals_subscribe`] stream.
#[tauri::command]
pub fn agent_approvals_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Decide an approval on the action and digest the person was shown; a
/// refusal is the server's sentence.
#[tauri::command]
pub async fn agent_session_approval_decide(
    database: String,
    session_id: String,
    approval_id: String,
    binding_digest: String,
    approve: bool,
) -> Result<AgentDecisionOutcomeVm, IpcError> {
    blocking(move |hq| {
        hq.decide(
            &session(database, session_id),
            &approval_id,
            &binding_digest,
            approve,
        )
    })
    .await
}

/// A card's *Always allow …* or *This session only* (109.11): the request
/// approved and the grant it offers written, the scope named by its id —
/// Rust derives it again from the request's own call and decides whether it
/// is offered at all.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_approval_always_allow(
    database: String,
    session_id: String,
    approval_id: String,
    binding_digest: String,
    scope: String,
    session_only: bool,
    expires_in_days: Option<u32>,
) -> Result<AgentDecisionOutcomeVm, IpcError> {
    blocking(move |hq| {
        hq.always_allow(
            &database,
            &AlwaysAllow {
                session_id,
                approval_id,
                binding_digest,
                scope,
                session_only,
                expires_in_days,
            },
        )
    })
    .await
}

/// Stream Settings › Agents › *Always allowed* at once, then on every
/// change.
#[tauri::command]
pub fn agent_grants_subscribe(channel: Channel<AgentGrantsVm>) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    // A grant made on another device about an agent that only started
    // serving this person lists once the server is asked again.
    hq.recheck_grants();
    let mut changes = hq.changes();
    let start = move |id: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown: Option<AgentGrantsVm> = None;
            loop {
                let read = Arc::clone(&hq);
                let Ok(grants) = tauri::async_runtime::spawn_blocking(move || read.grants()).await
                else {
                    return;
                };
                if shown.as_ref() != Some(&grants) {
                    if channel.send(grants.clone()).is_err() {
                        end(&id);
                        return;
                    }
                    shown = Some(grants);
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`agent_grants_subscribe`] stream.
#[tauri::command]
pub fn agent_grants_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Revoke one of this person's grants: new uses stop; approvals it already
/// issued still run.
#[tauri::command]
pub async fn agent_grant_revoke(database: String, grant_id: String) -> Result<(), IpcError> {
    blocking(move |hq| hq.revoke_grant(&database, &grant_id)).await
}

/// Stream a session's People sheet — its members and who the owner may
/// add — at once, then on every change. The change feed is registered
/// before the first sheet is read, so a change landing in between is never
/// missed. The stream does not hold the session open; the session view does.
#[tauri::command]
pub fn agent_session_members_subscribe(
    database: String,
    session_id: String,
    channel: Channel<AgentSessionMembersVm>,
) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    let key = session(database, session_id);
    let (first, mut changes) = hq.session_members_watched(&key).map_err(error)?;
    let start = move |id: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown = first;
            if channel.send(shown.clone()).is_err() {
                end(&id);
                return;
            }
            while changes.changed().await.is_ok() {
                let (read, at) = (Arc::clone(&hq), key.clone());
                let Ok(sheet) =
                    tauri::async_runtime::spawn_blocking(move || read.session_members(&at)).await
                else {
                    return;
                };
                match sheet {
                    Ok(sheet) if sheet != shown => {
                        if channel.send(sheet.clone()).is_err() {
                            end(&id);
                            return;
                        }
                        shown = sheet;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "an agents session's people could not be read");
                    }
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`agent_session_members_subscribe`] stream.
#[tauri::command]
pub fn agent_session_members_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Add a person of the session's database; they join once the server has
/// written their membership.
#[tauri::command]
pub async fn agent_session_member_add(
    database: String,
    session_id: String,
    actor_id: String,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.add_member(&session(database, session_id), &actor_id)).await
}

/// Grant or withdraw a member's reading of thinking and tool results.
#[tauri::command]
pub async fn agent_session_member_reader_set(
    database: String,
    session_id: String,
    actor_id: String,
    reads: bool,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_member_reader(&session(database, session_id), &actor_id, reads)).await
}

/// Remove a member from a session.
#[tauri::command]
pub async fn agent_session_member_remove(
    database: String,
    session_id: String,
    actor_id: String,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.remove_member(&session(database, session_id), &actor_id)).await
}

/// Leave a session this person does not own.
#[tauri::command]
pub async fn agent_session_leave(database: String, session_id: String) -> Result<(), IpcError> {
    blocking(move |hq| hq.leave_session(&session(database, session_id))).await
}

/// Stream every agent's instances — who is online, the definition each
/// runs — and the person's choice of which answers, at once, then on every
/// change of a row or of who is present.
#[tauri::command]
pub fn agent_instances_subscribe(
    channel: Channel<Vec<AgentInstancesVm>>,
) -> Result<String, IpcError> {
    let hq = Arc::clone(hq()?);
    let mut changes = hq.changes();
    let mut present = hq.live_changes();
    let start = move |id: String| {
        tauri::async_runtime::spawn(async move {
            let mut shown: Option<Vec<AgentInstancesVm>> = None;
            loop {
                let read = Arc::clone(&hq);
                let Ok(agents) =
                    tauri::async_runtime::spawn_blocking(move || read.agent_instances()).await
                else {
                    return;
                };
                if shown.as_ref() != Some(&agents) {
                    if channel.send(agents.clone()).is_err() {
                        end(&id);
                        return;
                    }
                    shown = Some(agents);
                }
                let woke = tokio::select! {
                    changed = changes.changed() => changed.is_ok(),
                    changed = present.changed() => changed.is_ok(),
                };
                if !woke {
                    return;
                }
            }
        })
    };
    Ok(keep(None, start))
}

/// End an [`agent_instances_subscribe`] stream.
#[tauri::command]
pub fn agent_instances_unsubscribe(subscription_id: String) {
    end(&subscription_id);
}

/// Choose which instance of an agent answers: `order`'s first, then the
/// rest while it is offline — one row in each database shared with it.
#[tauri::command]
pub async fn agent_instances_set(agent_actor: String, order: Vec<String>) -> Result<(), IpcError> {
    blocking(move |hq| hq.set_agent_instances(&agent_actor, &order)).await
}

/// Said to the webview when the default agent as keeper reads it moved
/// (story 102.7, DW-1572, R488): a synced settings row, or the account's
/// layers installed or cleared — never a row written beneath a file that
/// keeps deciding it. The default agent's marker and menus read it again.
/// Payload-free. Mirrors `AGENTS_DEFAULT_CHANGED_EVENT` in `client.ts`.
pub const AGENTS_DEFAULT_CHANGED_EVENT: &str = "keeper://agents-default-changed";

/// Tell the webview the default agent moved. Whether it did is
/// `default_agent::DefaultAgentSeen::moved`'s answer, asked by the caller. A
/// failure is a WARN line: the marker then follows at the next read.
pub(crate) fn default_agent_applied(app: &tauri::AppHandle) {
    if let Err(error) = app.emit(AGENTS_DEFAULT_CHANGED_EVENT, ()) {
        tracing::warn!(%error, "agents: the webview was not told the default agent changed");
    }
}

/// The default agent as Settings › Agents and the launch's chooser show it,
/// from what the replicas hold now (never waits); a failure is reported by
/// the webview.
#[tauri::command]
pub async fn agents_default_get() -> Result<DefaultAgentVm, IpcError> {
    blocking(|hq| hq.default_agent()).await
}

/// Make `agent` (an actor id) the default agent, or clear it with `None`,
/// and answer with the default agent as it now reads. Every failure is a
/// WARN line (R381), the core's.
#[tauri::command]
pub async fn agents_default_set(agent: Option<String>) -> Result<DefaultAgentVm, IpcError> {
    let hq = Arc::clone(hq()?);
    tauri::async_runtime::spawn_blocking(move || hq.set_default_agent(agent.as_deref()))
        .await
        .map_err(|join| IpcError {
            code: IpcErrorCode::Internal,
            message: join.to_string(),
            account_id: None,
            retriable: true,
        })?
        .map_err(|refusal| IpcError {
            code: IpcErrorCode::Internal,
            message: refusal.to_string(),
            account_id: None,
            retriable: false,
        })
}

/// The person put the launch's "Choose the agent keeper opens when it
/// starts" away (story 102.7, R463): not shown again until keeper is
/// updated. A failure is a WARN line (R381), the core's, and the webview
/// says it.
#[tauri::command]
pub async fn agents_default_chooser_dismiss() -> Result<(), IpcError> {
    blocking(|hq| hq.dismiss_default_chooser()).await
}

/// *Speak to*'s agents (AD-384, 108.2): every agent of every database this
/// device holds, each with the value `voice_target_set` stores to choose
/// it.
#[tauri::command]
pub async fn voice_agent_targets() -> Result<Vec<VoiceAgentTargetVm>, IpcError> {
    blocking(|hq| hq.voice_agent_targets()).await
}

/// The agents a spoken turn may go to, by actor id — refused while the
/// replicas on disk are still opening (`Hq::spoken_agents`).
pub(crate) async fn spoken_agents() -> Result<Vec<String>, IpcError> {
    blocking(|hq| hq.spoken_agents()).await
}

/// The send and then the following of the answer to the voice turn's
/// current question, when it went to one of the person's agents: only that
/// question's is ever kept ([`FollowSlot`]).
static SPOKEN_ANSWER: Mutex<FollowSlot<JoinHandle<()>>> = Mutex::new(FollowSlot::new());

fn spoken_answer() -> std::sync::MutexGuard<'static, FollowSlot<JoinHandle<()>>> {
    SPOKEN_ANSWER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A new question was heard: an earlier one's send or answer is not the
/// new turn's to speak (R44: the agent's turn is not cancelled; it finishes
/// its answer in its session — where the person stopped listening is
/// `voice_ipc`'s heard row, already on its way).
pub(crate) fn drop_spoken_answer() {
    let current = crate::voice_ipc::question_now();
    if let Some(earlier) = spoken_answer().keep_only(current) {
        earlier.abort();
    }
}

/// Send what the voice turn heard as `question` to `agent`'s main
/// conversation (AD-384, 108.2) and speak the answer as it grows. Called by
/// `voice_ipc::send_spoken` once *Speak to* resolves to an agent. Only the text leaves the
/// device, as the person's own message row; where it goes and what of the
/// answer is heard when are `keeper_core::hq::spoken`'s. The voice turn
/// hears the send leave (`note_sent`, with the agent's name), the first
/// words, the answer's start (for where the person stops listening,
/// AD-411), each sentence with its end and the end — or the refusal or
/// failure as its sentence — each for `question`. The work is in the slot
/// before it starts: a question that is no longer the voice turn's never
/// sends, and a newer one stops it.
pub(crate) fn send_spoken(question: u64, agent: String, text: String) {
    let work = tauri::async_runtime::spawn(async move {
        let sent = blocking(move |hq| {
            let ask = hq.spoken_send(&agent, &text)?;
            let answer = hq.spoken_answer(&ask)?;
            Ok((ask, answer))
        })
        .await;
        let (ask, mut answer) = match sent {
            Ok(sent) => sent,
            Err(refusal) => {
                tracing::warn!(message = %refusal.message, "agents: a spoken question was not sent");
                crate::voice_ipc::answer_failed(question, refusal.message);
                return;
            }
        };
        tracing::info!(database = %ask.key.database, "agents: sent what the voice turn heard to the person's agent");
        crate::voice_ipc::note_sent(question, answer.agent_name());
        while let Some(step) = answer.next().await {
            match step {
                SpokenStep::FirstText { after_ms } => {
                    crate::voice_ipc::note_answer_chunk(question, after_ms);
                }
                SpokenStep::Anchored => {
                    crate::voice_ipc::answer_anchored(
                        question,
                        ask.key.clone(),
                        ask.question.clone(),
                    );
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

/// Write where the person stopped hearing a spoken answer in `key` — the
/// heard row whose `metadata` is [`keeper_core::hq::spoken::HeardMark`]'s
/// (R-HQ-68). Called by `voice_ipc` by name, so no `voice*.rs` reaches the
/// network.
pub(crate) async fn spoken_heard(
    key: AgentSessionRef,
    metadata: serde_json::Value,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.spoken_heard(&key, metadata)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::hq::attachments::{too_large, ATTACHMENT_MAX_BYTES};

    fn headers(pairs: &[(&'static str, &str)]) -> tauri::http::HeaderMap {
        let mut headers = tauri::http::HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, value.parse().expect("a header value"));
        }
        headers
    }

    #[test]
    fn a_paste_over_the_bound_is_refused_with_cores_sentence_and_a_valid_one_is_read_whole() {
        let over = usize::try_from(ATTACHMENT_MAX_BYTES).expect("fits") + 1;
        let body = tauri::ipc::InvokeBody::Raw(vec![0u8; over]);
        // Non-ASCII, percent-encoded: "café.zip". No database header: the
        // bound is checked before anything else is read.
        let refused = raw_attachment(&body, &headers(&[("x-filename", "caf%C3%A9.zip")]))
            .expect_err("over the bound");
        assert_eq!(
            refused.message,
            too_large("café.zip", ATTACHMENT_MAX_BYTES + 1)
        );

        let body = tauri::ipc::InvokeBody::Raw(b"tea,3\n".to_vec());
        let read = raw_attachment(
            &body,
            &headers(&[
                ("x-database", "alpha"),
                ("x-session-id", "dev1.1"),
                ("x-mime", "text/csv"),
                ("x-filename", "pr%C3%ADces.csv"),
            ]),
        )
        .expect("within the bound");
        assert_eq!(read.database, "alpha");
        assert_eq!(read.session_id, "dev1.1");
        assert_eq!(read.mime, "text/csv");
        assert_eq!(read.name, "príces.csv");
        assert_eq!(read.bytes, b"tea,3\n");

        let json = tauri::ipc::InvokeBody::Json(serde_json::json!({}));
        assert!(
            raw_attachment(&json, &headers(&[])).is_err(),
            "not a raw body"
        );
    }
}
