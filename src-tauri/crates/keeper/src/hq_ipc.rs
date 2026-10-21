//! Shell call sites of `keeper_core::hq` (AD-418, AD-422, AD-424): the
//! process's one [`Hq`], started once the settings layers are installed and
//! reconciled whenever a setting or a settings layer changes, its commands,
//! the pairing link a scanned QR opens, and the Agents window's streams.
//! Every decision — the address grammar, the pairing, which databases sync,
//! the list, its order and attention, every sentence — is core's; this holds
//! it and streams what it answers.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use keeper_core::hq::approvals::{AgentApprovalVm, AgentDecisionOutcomeVm};
use keeper_core::hq::live::{self, AgentViewPrefsPatch, AgentViewPrefsVm};
use keeper_core::hq::routes::HttpRoutes;
use keeper_core::hq::sessions::{
    diff, AgentChoiceVm, AgentDefaultSessionVm, AgentSessionRef, AgentSessionsBatch,
    AgentSessionsOp,
};
use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::timeline::AgentSessionViewVm;
use keeper_core::hq::vm::{HqPairCodeVm, HqStatusVm};
use keeper_core::hq::{Hq, HqError};
use keeper_core::vm::{IpcError, IpcErrorCode};
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

/// Build the service and open what this device holds. Called from setup
/// after the settings layers are installed, so `agents.server_url` is read
/// through them.
pub fn init(app: &tauri::App) {
    let platform = Arc::clone(&app.state::<crate::ipc::AppState>().platform);
    let hq = Hq::new(
        platform,
        Arc::new(SyncularOpener),
        Arc::new(HttpRoutes::new()),
        format!("keeper on {}", keeper_core::config::read_host_label()),
    );
    if HQ.set(Arc::clone(&hq)).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = hq.start().await {
            tracing::warn!(%error, "the agents server could not be started");
        }
    });
}

/// The effective settings may have changed (a setting written, a settings
/// layer installed or cleared, a settings file pulled): follow
/// `agents.server_url` if it moved, and the view preferences
/// (`agents.show_reasoning`, `agents.show_details`). Before [`init`] there is
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

fn error(error: HqError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
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
                // An answer still arriving is read again on its own, so one
                // whose instance stopped says so with nothing else moving.
                let arriving = shown
                    .as_ref()
                    .is_some_and(|view| view.live.iter().any(|turn| !turn.stopped));
                let woke = async {
                    tokio::select! {
                        changed = changes.changed() => changed.is_ok(),
                        changed = tails.changed() => changed.is_ok(),
                    }
                };
                if arriving {
                    if let Ok(false) = tokio::time::timeout(live::RECHECK, woke).await {
                        return;
                    }
                } else if !woke.await {
                    return;
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

/// The person scrolled to the top of an open session: earlier messages.
#[tauri::command]
pub fn agent_session_page_back(database: String, session_id: String) -> Result<(), IpcError> {
    hq()?
        .page_back(&session(database, session_id))
        .map_err(error)
}

/// Write a message into a session; it shows as waiting at once.
#[tauri::command]
pub async fn agent_send(
    database: String,
    session_id: String,
    body: String,
) -> Result<(), IpcError> {
    blocking(move |hq| hq.send(&session(database, session_id), &body)).await
}

/// Open a session with an agent in one database, with its first message.
#[tauri::command]
pub async fn agent_session_new(
    database: String,
    agent_actor: String,
    body: String,
) -> Result<AgentSessionRef, IpcError> {
    blocking(move |hq| hq.new_session(&database, &agent_actor, &body)).await
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
