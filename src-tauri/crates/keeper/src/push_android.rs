//! Push on Android, through the person's UnifiedPush distributor (Epic 98,
//! story 98.4; AD-414, D-36, the epic's Q11 and Q12).
//!
//! # What it is
//!
//! The app half of Android push, over `UnifiedPushPlugin.kt` and the two
//! receivers beside it in `gen/android`:
//!
//! - **The distributor and the endpoint.** At setup keeper asks the plugin
//!   which distributors are installed, by package and name;
//!   `keeper_core::agents::unifiedpush::choose_distributor` decides against
//!   the one the person confirmed on this device: that one registers, any
//!   other is asked about in Settings ([`confirm`]) and registers nothing
//!   until the person confirms it. The distributor answers with an endpoint,
//!   which keeper keeps (the egress list and a sign-out read it); keeper asks
//!   the endpoint's server once whether it is a Matrix push gateway
//!   (`unifiedpush::known_gateway`, `unifiedpush::discover_gateway`); a
//!   definite other answer is said with a sentence naming the host —
//!   Settings → Notifications shows it ([`status`], told on
//!   [`STATUS_EVENT`]); a server keeper cannot reach changes nothing
//!   (`unifiedpush::pusher_change`).
//! - **The old pusher removed.** Before 108.2 every account that held agent
//!   session rooms got an `event_id_only` Matrix pusher to that gateway; the
//!   agents left Matrix, so each new endpoint takes the pusher for it and for
//!   the endpoint before it off every account (`AccountManager::push_remove`,
//!   DW-E108-1) and none is registered.
//! - **The agents server** (106.6, AD-432). The same endpoint and gateway go
//!   to keeper-hq as this device's push target
//!   (`keeper_core::hq::push::PushTarget::unifiedpush`, through
//!   [`crate::hq_ipc::set_push_target`]); losing the endpoint or the gateway
//!   removes it. A push from the server names an item by ids; keeper reads
//!   that item with the device's pairing (`Hq::read_pushed`) and posts what
//!   `keeper_core::hq::push::classify` says, one notification per item
//!   (`unifiedpush::item_notification`); its actions write the decision row
//!   (`Hq::decide_pushed`) and a tap opens the session.
//! - **A push.** The distributor hands the gateway's notification to
//!   `PushReceiver.kt`, which passes it here; an agents server item is read
//!   as above. When keeper is not running — Android stopped it — the
//!   receiver cannot ask, and posts keeper's one generic notification
//!   itself.
//! - **An action.** *Approve once* or *Deny* (Android 12 and later, after
//!   the device is unlocked) reaches `PushActionReceiver.kt`, which passes
//!   it here: `keeper_core::hq::push::action_decision` derives the decision
//!   from what keeper posted, it is written within `push::ACTION_BUDGET`,
//!   and the notification is removed — or replaced by one asking the person
//!   to finish in keeper.
//! - **A tap** opens the session: [`take_open`] for the webview, told on
//!   [`PUSH_OPEN_EVENT`], as on the iPhone.
//!
//! Nothing here decides: the distributor, the gateway, what a notification
//! shows and offers, what an action sends and where a tap lands are
//! `keeper_core::agents::{push, unifiedpush}`'s (AD-55/AD-56). The endpoint
//! is a capability — anyone who has it can post to this device — so it is
//! never logged; only its host is.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock, PoisonError};

use keeper_core::agents::push::{self, PushOpenVm};
use keeper_core::agents::unifiedpush::{
    self as up, DiscoveryRefusal, Distributor, DistributorChoice, Endpoint, KeptEndpoint,
    PhoneNotification, PusherChange, UnifiedPushStatus, UnifiedPushVm,
};
use keeper_core::failure::FailureOperation;
use keeper_core::org_account::layout::DeviceClass;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::ipc::AppState;

/// The event that tells the webview a tapped notification is waiting to be
/// opened (`push_open_take`) — the iPhone's, so the front listens once.
pub const PUSH_OPEN_EVENT: &str = "push://open";

/// The event that tells Settings where push stands changed
/// (`unifiedpush_status`).
pub const STATUS_EVENT: &str = "unifiedpush://status";

/// The registered `UnifiedPushPlugin`, set once by [`plugin`]'s setup.
static PLUGIN: OnceLock<PluginHandle<Wry>> = OnceLock::new();

/// The app, set once by [`set_up`].
static APP: OnceLock<AppHandle> = OnceLock::new();

/// The channel the plugin and its receivers send their events through, kept
/// for the life of the process.
static EVENTS: LazyLock<Channel<Value>> = LazyLock::new(|| {
    Channel::new(|body: InvokeResponseBody| {
        match body.deserialize::<Value>() {
            Ok(event) => received(event),
            Err(error) => tracing::warn!(%error, "push: an event keeper cannot read"),
        }
        Ok(())
    })
});

/// This device's API level, as the plugin reported it.
static SDK_INT: AtomicU32 = AtomicU32::new(0);

/// Where push stands this launch, for Settings.
static STATUS: Mutex<UnifiedPushStatus> = Mutex::new(UnifiedPushStatus::Waiting);

/// The distributor keeper registered with this launch — only ever one the
/// person confirmed. An endpoint is taken only while it is set.
static DISTRIBUTOR: Mutex<Option<Distributor>> = Mutex::new(None);

/// The session room the last tapped notification opens, until the webview
/// takes it.
static PENDING_OPEN: Mutex<Option<PushOpenVm>> = Mutex::new(None);

/// The Tauri plugin that registers the Kotlin `UnifiedPushPlugin` (class
/// `dev.tgorka.keeper.UnifiedPushPlugin` in the app module).
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("keeper-unifiedpush")
        .setup(|_app, api| {
            let handle = api.register_android_plugin("dev.tgorka.keeper", "UnifiedPushPlugin")?;
            let _ = PLUGIN.set(handle);
            Ok(())
        })
        .build()
}

/// keeper is set up (`setup()`): open the event channel and register with
/// the distributor, in the background.
pub fn set_up(app: &AppHandle) {
    let _ = APP.set(app.clone());
    tauri::async_runtime::spawn(async {
        if let Err(error) = start().await {
            crate::failure_notice::tell(
                FailureOperation::PushRegistration,
                None,
                "Push notifications could not start on this phone, so messages arrive only while keeper is open.",
                &error,
            );
        }
    });
}

/// Settings → Notifications' push status on this device.
pub fn status() -> UnifiedPushVm {
    let noun = match crate::android::device_class() {
        DeviceClass::Tablet => "tablet",
        _ => "phone",
    };
    UnifiedPushVm::of(&STATUS.lock().unwrap_or_else(PoisonError::into_inner), noun)
}

/// The webview's half of a tap: the room to open, once.
pub fn take_open() -> Option<PushOpenVm> {
    PENDING_OPEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// This device's pusher, as a homeserver knows it: the endpoint keeper kept
/// and the Android app id. What a sign-out removes first (R340).
pub fn pusher_ids(data_dir: &std::path::Path) -> Option<push::PusherIds> {
    up::stored_endpoint(data_dir).map(|endpoint| endpoint.pusher_ids())
}

fn set_status(status: UnifiedPushStatus) {
    *STATUS.lock().unwrap_or_else(PoisonError::into_inner) = status;
    if let Some(app) = APP.get() {
        if let Err(error) = app.emit(STATUS_EVENT, ()) {
            tracing::warn!(%error, "push: Settings was not told the status changed");
        }
    }
}

/// The data directory, where keeper keeps the confirmed distributor and the
/// endpoint.
fn data_dir() -> Option<std::path::PathBuf> {
    let app = APP.get()?;
    match app.state::<AppState>().platform.data_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            tracing::warn!(%error, "push: no data directory");
            None
        }
    }
}

/// What `UnifiedPushPlugin.start` answers.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    sdk_int: u32,
    distributors: Vec<Distributor>,
}

/// Hand the plugin the event channel and the words it posts while keeper's
/// Rust half is not running — the generic notification for a push, "finish
/// in keeper" for an action (its tag set there) — then register with the
/// distributor the person confirmed, or ask about the installed ones.
async fn start() -> Result<(), String> {
    let plugin = PLUGIN
        .get()
        .ok_or("the UnifiedPush plugin is not registered")?;
    let started: Started = plugin
        .run_mobile_plugin_async(
            "start",
            json!({
                "events": &*EVENTS,
                "channelId": up::CHANNEL_ID,
                "channelName": up::CHANNEL_NAME,
                "channelDescription": up::CHANNEL_DESCRIPTION,
                "generic": up::generic_notification(),
                "finish": up::finish_in_keeper("", &Value::Null, ""),
            }),
        )
        .await
        .map_err(|error| format!("the plugin did not start: {error}"))?;
    SDK_INT.store(started.sdk_int, Ordering::SeqCst);
    let confirmed = data_dir().and_then(|dir| up::confirmed_distributor(&dir));
    match up::choose_distributor(confirmed.as_deref(), &started.distributors) {
        DistributorChoice::Use(distributor) => register(distributor).await?,
        DistributorChoice::None => forget(UnifiedPushStatus::NoDistributor).await,
        DistributorChoice::Ask(asked) => forget(UnifiedPushStatus::Asking(asked)).await,
    }
    Ok(())
}

/// Register with `distributor`, which the person confirmed: its endpoint
/// arrives as an `endpoint` event.
async fn register(distributor: Distributor) -> Result<(), String> {
    let plugin = PLUGIN
        .get()
        .ok_or("the UnifiedPush plugin is not registered")?;
    let package = distributor.package.clone();
    *DISTRIBUTOR.lock().unwrap_or_else(PoisonError::into_inner) = Some(distributor);
    plugin
        .run_mobile_plugin_async::<Value>("register", json!({ "distributor": package }))
        .await
        .map_err(|error| format!("the distributor was not asked: {error}"))?;
    Ok(())
}

/// The person confirmed the distributor `package` in Settings → Notifications
/// (R98UP1-02): when keeper asked about it, keep the choice on this device
/// and register with it. Answers where push stands now.
pub async fn confirm(package: &str) -> Result<UnifiedPushVm, String> {
    let asked = match &*STATUS.lock().unwrap_or_else(PoisonError::into_inner) {
        UnifiedPushStatus::Asking(asked) => asked.clone(),
        _ => return Err("keeper is not asking which distributor to use".to_owned()),
    };
    let distributor =
        up::confirmed_of(&asked, package).ok_or("keeper did not ask about that distributor")?;
    let dir = data_dir().ok_or("keeper has no data directory")?;
    up::confirm_distributor(&dir, &distributor)
        .map_err(|error| format!("the choice could not be kept: {error}"))?;
    set_status(UnifiedPushStatus::Waiting);
    register(distributor).await?;
    Ok(status())
}

/// An event from the plugin or a receiver.
fn received(event: Value) {
    let text = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
    match event["kind"].as_str() {
        Some("endpoint") => {
            tauri::async_runtime::spawn(new_endpoint(text("endpoint")));
        }
        Some("failed") => {
            tauri::async_runtime::spawn(forget(UnifiedPushStatus::RegistrationFailed));
        }
        Some("unregistered") => {
            tauri::async_runtime::spawn(forget(UnifiedPushStatus::Unregistered));
        }
        Some("message") => {
            tauri::async_runtime::spawn(message(text("body")));
        }
        Some("action") => {
            tauri::async_runtime::spawn(action(
                text("tag"),
                text("category"),
                text("action"),
                text("userInfo"),
            ));
        }
        Some("open") => open(&text("userInfo")),
        other => tracing::warn!(?other, "push: an event of a kind keeper does not know"),
    }
}

/// The distributor the person confirmed gave keeper an endpoint: ask its
/// server whether it is a Matrix gateway — unless keeper already knows
/// (`unifiedpush::known_gateway`) — and, on an answer, keep the endpoint,
/// hand the agents server its push target, and take the old Matrix pusher
/// for this endpoint and the one before it off every account (DW-E108-1).
/// A server keeper cannot reach changes nothing
/// (`unifiedpush::pusher_change`).
async fn new_endpoint(raw: String) {
    let Some(distributor) = DISTRIBUTOR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    else {
        tracing::warn!("push: an endpoint from a distributor keeper did not register with");
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    let state = app.state::<AppState>();
    let Some(data_dir) = data_dir() else {
        return;
    };
    let endpoint = match Endpoint::parse(&raw) {
        Ok(endpoint) => endpoint,
        Err(refusal) => {
            tracing::warn!(%refusal, "push: the distributor's endpoint is refused");
            forget(UnifiedPushStatus::Refused(refusal.into())).await;
            return;
        }
    };
    let kept = up::kept_endpoint(&data_dir);
    let answer = match up::known_gateway(kept.as_ref(), &endpoint) {
        Some(gateway) => Ok(gateway),
        None => match up::discovery_client() {
            Ok(http) => up::discover_gateway(&http, &endpoint).await,
            Err(_) => Err(DiscoveryRefusal::Unreachable {
                host: endpoint.host(),
            }),
        },
    };
    let (gateway, status) = match up::pusher_change(answer) {
        PusherChange::Keep(refusal) => {
            tracing::warn!(%refusal, "push: the pusher is left as it was");
            set_status(UnifiedPushStatus::Refused(refusal));
            return;
        }
        PusherChange::Register(gateway) => (
            Some(gateway),
            UnifiedPushStatus::Registered {
                host: endpoint.host(),
                distributor: distributor.clone(),
            },
        ),
        PusherChange::Remove(refusal) => {
            tracing::warn!(%refusal, "push: no pusher is registered");
            (None, UnifiedPushStatus::Refused(refusal))
        }
    };
    crate::hq_ipc::set_push_target(
        gateway.as_ref().map(|gateway| {
            keeper_core::hq::push::PushTarget::unifiedpush(gateway.clone(), &endpoint)
        }),
    );
    let now = KeptEndpoint {
        endpoint: endpoint.clone(),
        distributor,
        gateway: gateway.is_some(),
    };
    if let Err(error) = up::keep_endpoint(&data_dir, Some(&now)) {
        tracing::warn!(%error, "push: the endpoint could not be kept");
    }
    let outcomes = state
        .accounts
        .push_remove(
            &state.platform,
            endpoint.pusher_ids(),
            kept.map(|kept| kept.endpoint.pusher_ids()),
        )
        .await;
    log_outcomes(outcomes);
    set_status(status);
}

/// No endpoint any more (no distributor, none confirmed — keeper asks —, a
/// refused endpoint, a refused or dropped registration): remove the old
/// Matrix pusher of the one keeper kept from every account, forget it, and
/// say why.
async fn forget(why: UnifiedPushStatus) {
    set_status(why);
    crate::hq_ipc::set_push_target(None);
    let Some(app) = APP.get() else {
        return;
    };
    let state = app.state::<AppState>();
    let Some(data_dir) = data_dir() else {
        return;
    };
    let Some(kept) = up::stored_endpoint(&data_dir) else {
        return;
    };
    let outcomes = state
        .accounts
        .push_remove(&state.platform, kept.pusher_ids(), None)
        .await;
    log_outcomes(outcomes);
    if let Err(error) = up::keep_endpoint(&data_dir, None) {
        tracing::warn!(%error, "push: the old endpoint could not be forgotten");
    }
}

/// Each account's removal outcome, for the log.
type Outcomes =
    Result<Vec<(String, Result<(), keeper_core::error::CoreError>)>, keeper_core::error::CoreError>;

/// The old pusher's removal, per account: a failure is a WARN line and is
/// tried again with the next endpoint or launch (DW-E108-1).
fn log_outcomes(outcomes: Outcomes) {
    match outcomes {
        Ok(outcomes) => {
            for (account, outcome) in outcomes {
                match outcome {
                    Ok(()) => tracing::info!(account, "push: the old Matrix pusher is removed"),
                    Err(error) => tracing::warn!(
                        account,
                        %error,
                        "push: the old Matrix pusher could not be removed"
                    ),
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "push: the old Matrix pushers could not be removed")
        }
    }
}

/// A push arrived while keeper runs: read the agents server item it names
/// and post what the classifier says shows; keeper's generic words when it
/// cannot be read. A push of the agents' old Matrix path (an event id,
/// before 108.2) names nothing keeper reads any more and shows nothing.
async fn message(body: String) {
    if let Some(pushed) = keeper_core::hq::push::PushedItem::of_notify(body.as_bytes()) {
        match crate::hq_ipc::read_pushed(&pushed).await {
            Ok(keeper_core::hq::push::HqClassified::Show(shown)) => {
                post(up::item_notification(
                    &shown,
                    SDK_INT.load(Ordering::SeqCst),
                ))
                .await;
            }
            Ok(keeper_core::hq::push::HqClassified::Drop) => {}
            Err(why) => {
                tracing::warn!(target: keeper_core::telemetry::SHELL_AGENTS_TARGET, why, "push: the agents item could not be read");
                post(up::generic_notification()).await;
            }
        }
        return;
    }
    tracing::info!("push: a push of the agents' old Matrix path, shown as nothing");
}

/// The person picked an action on the notification `tag`: write exactly the
/// decision `keeper_core::hq::push::action_decision` derives from what
/// keeper posted, then remove the notification — or replace it with one
/// asking to finish in keeper. A notification keeper did not compose decides
/// nothing and is answered with nothing.
async fn action(tag: String, category: String, action: String, user_info: String) {
    let Some(app) = APP.get() else {
        return;
    };
    let user_info: Value = match serde_json::from_str(&user_info) {
        Ok(info) => info,
        Err(error) => {
            tracing::warn!(%error, "push: an action keeper cannot read");
            return;
        }
    };
    let tap = push::ActionTap {
        category,
        action,
        user_info,
    };
    if keeper_core::hq::push::is_composed(&tap.user_info) {
        act_on_item(app, &tag, &tap).await;
        return;
    }
    tracing::warn!("push: an action on a notification keeper did not compose");
}

/// The decision an action on an agents item's notification `tag` sends,
/// written through the session's replica: the notification is removed once
/// the server took it, else replaced by one asking to finish in keeper.
async fn act_on_item(app: &AppHandle, tag: &str, tap: &push::ActionTap) {
    let decision = match keeper_core::hq::push::action_decision(tap) {
        Ok(decision) => decision,
        Err(push::ActionRefusal::NoApproval) => {
            tracing::warn!("push: an action on a notification keeper did not compose");
            return;
        }
        Err(refusal) => {
            post(up::finish_in_keeper(
                tag,
                &tap.user_info,
                &refusal.to_string(),
            ))
            .await;
            return;
        }
    };
    let deciding = app.clone();
    let sent = tauri::async_runtime::spawn_blocking(move || {
        crate::hq_ipc::decide_pushed(&deciding, &decision, push::ACTION_BUDGET)
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|sent| sent);
    match sent {
        Ok(_) => {
            if let Some(plugin) = PLUGIN.get() {
                if let Err(error) = plugin
                    .run_mobile_plugin_async::<Value>("cancel", json!({ "tag": tag }))
                    .await
                {
                    tracing::warn!(%error, "push: the decided notification stays");
                }
            }
        }
        Err(why) => {
            tracing::warn!(
                target: keeper_core::telemetry::SHELL_AGENTS_TARGET,
                why,
                "push: a decision from a notification was not sent"
            );
            post(up::finish_in_keeper(tag, &tap.user_info, &why)).await;
        }
    }
}

/// Keep where the tapped notification whose `user_info` this is opens, and
/// tell the webview.
fn open(user_info: &str) {
    let Some(target) = serde_json::from_str::<Value>(user_info)
        .ok()
        .and_then(|info| push::open_target(&info))
    else {
        return;
    };
    *PENDING_OPEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(target);
    if let Some(app) = APP.get() {
        if let Err(error) = app.emit(PUSH_OPEN_EVENT, ()) {
            tracing::warn!(%error, "push: could not tell the webview a notification was tapped");
        }
    }
}

/// Post `notification` through the plugin.
async fn post(notification: PhoneNotification) {
    let Some(plugin) = PLUGIN.get() else {
        return;
    };
    if let Err(error) = plugin
        .run_mobile_plugin_async::<Value>("notify", notification)
        .await
    {
        tracing::warn!(%error, "push: the notification was not posted");
    }
}
