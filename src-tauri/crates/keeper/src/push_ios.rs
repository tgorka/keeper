//! Push on the iPhone (Epic 98, story 98.1; AD-412, UX-DR144, the codemap's
//! Q2 and Q6).
//!
//! # What it is
//!
//! The app half of push. The notification extension (`keeper-nse`,
//! `gen/apple/KeeperNotify`) decides what a push shows; this module does the
//! rest of the phone's part:
//!
//! - **The agents server's push target** (106.6, AD-432). The same token,
//!   app id and gateway go to keeper-hq as this device's push target
//!   (`keeper_core::hq::push::PushTarget::apns`, `PUT /clients/self/push-target`
//!   through [`crate::hq_ipc::set_push_target`]); a cleared gateway removes
//!   it. The server then posts an item's ids, and the extension reads the
//!   item (`keeper_nse::answer_item`).
//! - **The token, and the old pusher removed.** tao's app delegate hands the
//!   APNs device token to [`token_arrived`] (the `tools/tao` patch, NOTICE
//!   change 3). Before 108.2 every signed-in account that held agent session
//!   rooms got a Matrix pusher for this token; the agents left Matrix, so
//!   once keeper is set up that pusher is taken off every account
//!   (`AccountManager::push_remove`, DW-E108-1) and none is registered. The
//!   app id follows the build's own `aps-environment`
//!   ([`ApnsEnvironment::of_profile`] over `embedded.mobileprovision`).
//! - **The delegate.** `gen/apple/Sources/keeper/KeeperPush.swift` becomes
//!   `UNUserNotificationCenter`'s delegate, forwarding every notification that
//!   is not keeper's push to the notification plugin's own delegate, which
//!   drops push-triggered responses (codemap §0 item 1), and registers the two
//!   categories Rust describes (`push::registered_categories`).
//! - **An action.** *Approve once* or *Deny* on an agents server item's
//!   notification writes the decision row through the session's replica
//!   (`keeper_core::hq::push::action_decision`, `Hq::decide_pushed`); a
//!   background launch for it starts only that client
//!   ([`crate::hq_ipc::ensure`]), within [`push::ACTION_BUDGET`]; when it
//!   cannot, the answer is a notification asking the person to finish in
//!   keeper.
//! - **A tap.** [`keeper_push_open`] keeps the session to open and tells
//!   the webview, which takes it with `push_open_take` — on mount, for a tap
//!   that launched keeper, and on [`PUSH_OPEN_EVENT`] otherwise.
//!
//! Nothing here decides: what a notification offers, what an action sends and
//! where a tap lands are `keeper_core::agents::push`'s (AD-55/AD-56).
//!
//! # When it runs
//!
//! [`install`] runs right after `tauri::Builder::build`, before UIKit starts,
//! because iOS hands a notification's response to the delegate set before
//! launch finishes — and under the scene lifecycle an action that launches
//! keeper in the background connects no scene, so tao never emits the event
//! that runs `setup()` (`tools/tao/src/platform_impl/ios/app_state.rs`,
//! `did_finish_launching`). The plugins are initialised in `build`, so the
//! plugin's delegate is already there to be wrapped. [`set_up`] runs from
//! `setup()`: only then are old pushers removed, because removing one activates
//! every account, which a background launch for one action must not do.

use std::ffi::{c_char, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use keeper_core::agents::push::{self, ApnsEnvironment, PushOpenVm};
use keeper_core::failure::{FailureOperation, PUSH_NOT_REGISTERED};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::ipc::AppState;

/// The event that tells the webview a tapped notification is waiting to be
/// opened (`push_open_take`).
pub const PUSH_OPEN_EVENT: &str = "push://open";

/// The app, set once by [`install`].
static APP: OnceLock<AppHandle> = OnceLock::new();

/// This launch's APNs device token, once iOS has given one.
static TOKEN: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Whether `setup()` has run: the app is in front, its accounts are being
/// signed in, and the old pusher may be removed.
static SET_UP: AtomicBool = AtomicBool::new(false);

/// The session room the last tapped notification opens, until the webview
/// takes it.
static PENDING_OPEN: Mutex<Option<PushOpenVm>> = Mutex::new(None);

/// Take the delegate and ask iOS for the token: right after
/// `tauri::Builder::build`, before the event loop runs.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    tao::platform::ios::set_remote_notification_hooks(token_arrived, token_refused);
    bridge_install();
}

/// keeper is set up (`setup()`): hand the agents server this phone's push
/// target and remove its old Matrix pusher once the token is known, and make
/// sure the delegate is still keeper's.
pub fn set_up() {
    SET_UP.store(true, Ordering::SeqCst);
    bridge_install();
    register_soon();
}

/// The webview's half of a tap: the room to open, once.
pub fn take_open() -> Option<PushOpenVm> {
    PENDING_OPEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

/// iOS gave this launch its token (tao's app delegate, main thread).
fn token_arrived(token: &[u8]) {
    if token.is_empty() {
        tracing::warn!("push: iOS answered an empty device token");
        return;
    }
    *TOKEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(token.to_vec());
    if SET_UP.load(Ordering::SeqCst) {
        register_soon();
    }
}

/// iOS gave no token: a build without `aps-environment` (every free
/// Personal Team) or no network. No push target is handed over.
fn token_refused(why: &str) {
    tracing::warn!(
        why,
        "push: iOS gave keeper no device token, so no push target is handed over"
    );
}

/// The environment this build's token belongs to, from the bundle's
/// `embedded.mobileprovision` (none: an App Store build); `None` when the
/// profile grants no push, and when it exists but cannot be read — logged,
/// because then neither the sandbox nor production can be named
/// (`ApnsEnvironment::of_profile_read`, R343).
fn environment() -> Option<ApnsEnvironment> {
    let read = std::env::current_exe().and_then(|exe| {
        let bundle = exe
            .parent()
            .ok_or_else(|| std::io::Error::other("the executable has no bundle directory"))?;
        std::fs::read(bundle.join("embedded.mobileprovision"))
    });
    match ApnsEnvironment::of_profile_read(read) {
        Ok(environment) => environment,
        Err(error) => {
            tracing::warn!(
                %error,
                "push: this build's provisioning profile could not be read, so whether its \
                 token is the sandbox's (development) or production's is unknown; no push \
                 target is handed over"
            );
            None
        }
    }
}

/// This phone's pusher, as a homeserver knows it, once iOS has given this
/// launch a token and the build has an environment: what a sign-out
/// removes first (`ipc::sign_out`, R340).
pub fn pusher_ids() -> Option<push::PusherIds> {
    let token = TOKEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()?;
    Some(push::pusher_ids(environment()?, &token))
}

/// Hand the agents server this phone's push target, and take this phone's
/// old Matrix pusher off every account (DW-E108-1), in the background: after
/// setup once the token is known, and when the person changes the gateway
/// (`ipc::push_gateway_set`).
pub fn register_soon() {
    if !SET_UP.load(Ordering::SeqCst) {
        return;
    }
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let Some(token) = TOKEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    else {
        return;
    };
    let Some(environment) = environment() else {
        tracing::info!("push: this build has no APNs environment; no push target is handed over");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let stored = match state
            .platform
            .data_dir()
            .and_then(|dir| keeper_core::registry::get_push_gateway_url(&dir))
        {
            Ok(stored) => stored,
            Err(error) => {
                crate::failure_notice::tell(
                    FailureOperation::PushRegistration,
                    None,
                    PUSH_NOT_REGISTERED,
                    &error,
                );
                return;
            }
        };
        // The agents server's push target: the same gateway, token and app.
        crate::hq_ipc::set_push_target(keeper_core::hq::push::PushTarget::apns(
            stored.as_deref(),
            environment,
            &token,
        ));
        crate::failure_notice::recovered(FailureOperation::PushRegistration, None);
        // The Matrix pusher this phone registered before 108.2, taken off
        // every account: a failure is a WARN line and is tried again at the
        // next launch (DW-E108-1).
        match state
            .accounts
            .push_remove(&state.platform, push::pusher_ids(environment, &token), None)
            .await
        {
            Ok(outcomes) => {
                for (account, outcome) in outcomes {
                    match outcome {
                        Ok(()) => tracing::info!(account, "push: the old Matrix pusher is removed"),
                        Err(error) => tracing::warn!(
                            account,
                            __omp_magic("", "error,")
                            "push: the old Matrix pusher could not be removed"
                        ),
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "push: the old Matrix pushers could not be removed")
            }
        }
    });
}

/// The decision an action sends, or the notification to post when it was
/// not sent: `None` once the decision is written, and for a notification
/// that names no agents server item keeper composed — a push as the gateway
/// sent it (`push::SHOWN_INFO`, R343), or one of the agents' old Matrix
/// rooms from before 108.2, is not answered with keeper's words either.
fn act(request: &str) -> Option<Value> {
    let tap: push::ActionTap = match serde_json::from_str(request) {
        Ok(tap) => tap,
        Err(error) => {
            tracing::warn!(%error, "push: an action keeper cannot read");
            return None;
        }
    };
    if keeper_core::hq::push::is_composed(&tap.user_info) {
        return act_on_item(&tap);
    }
    tracing::warn!("push: an action on a notification keeper did not compose");
    None
}

/// The decision an action on an agents server item's notification sends,
/// written through the session's replica (106.6): `None` once the server
/// took it, else the notification asking the person to finish in keeper.
fn act_on_item(tap: &push::ActionTap) -> Option<Value> {
    let decision = match keeper_core::hq::push::action_decision(tap) {
        Ok(decision) => decision,
        Err(push::ActionRefusal::NoApproval) => {
            tracing::warn!("push: an action on a notification keeper did not compose");
            return None;
        }
        Err(refusal) => return Some(unfinished(&tap.user_info, &refusal.to_string())),
    };
    let Some(app) = APP.get() else {
        return Some(unfinished(&tap.user_info, ""));
    };
    match crate::hq_ipc::decide_pushed(app, &decision, push::ACTION_BUDGET) {
        Ok(_) => None,
        Err(why) => {
            tracing::warn!(why, "push: a lock-screen decision was not sent");
            Some(unfinished(&tap.user_info, &why))
        }
    }
}

/// The notification that asks the person to finish a decision in keeper,
/// tapping which opens where the action's notification did — the session.
/// keeper composes it, so it carries `push::SHOWN_INFO`
/// — what the delegate and `push::open_target` route a tap by.
fn unfinished(user_info: &Value, why: &str) -> Value {
    let body = if why.is_empty() {
        push::FINISH_IN_KEEPER.to_owned()
    } else {
        format!("{} {why}", push::FINISH_IN_KEEPER)
    };
    json!({
        "title": push::GENERIC_TITLE,
        "body": body,
        "thread_id": push::reposted_group(user_info),
        "user_info": push::reposted_info(user_info),
    })
}

/// A Rust string handed to Swift, which releases it with
/// [`keeper_push_free`].
fn handed_over(text: String) -> *mut c_char {
    CString::new(text).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// A C string from Swift, or empty for null.
///
/// # Safety
/// `text` is null or a NUL-terminated buffer valid for the call.
#[allow(unsafe_code)]
unsafe fn received(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract: non-null, NUL-terminated, valid for
    // the call; it is copied out before returning.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// The person picked an action on a keeper notification
/// (`KeeperPush.swift`, a background queue, never the main thread: this
/// blocks up to [`push::ACTION_BUDGET`]). `request` is the JSON of
/// [`push::ActionTap`]. Returns null once the decision is sent, else the
/// JSON of the notification to post — `{title, body, thread_id, user_info}`
/// — which the caller releases with [`keeper_push_free`].
///
/// # Safety
/// `request` is null or a NUL-terminated buffer valid for the call.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn keeper_push_action(request: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from this function's own contract.
    let request = unsafe { received(request) };
    // A panic must not unwind into Swift: it is the request not sent.
    match std::panic::catch_unwind(|| act(&request))
        .unwrap_or_else(|_| Some(unfinished(&Value::Null, "")))
    {
        None => std::ptr::null_mut(),
        Some(repost) => handed_over(repost.to_string()),
    }
}

/// The person tapped a keeper notification (`KeeperPush.swift`, the main
/// thread). `user_info` is the notification's `userInfo` as JSON; the
/// session or room it opens ([`push::open_target`]) waits for the webview,
/// which is told.
///
/// # Safety
/// `user_info` is null or a NUL-terminated buffer valid for the call.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn keeper_push_open(user_info: *const c_char) {
    // SAFETY: forwarded from this function's own contract.
    let user_info = unsafe { received(user_info) };
    // A panic must not unwind into Swift: it is a tap that opens nothing.
    let _ = std::panic::catch_unwind(|| open(&user_info));
}

/// Keep where the tapped notification whose `userInfo` this is opens, and
/// tell the webview.
fn open(user_info: &str) {
    let Some(target) = serde_json::from_str::<Value>(user_info)
        .ok()
        .and_then(|info| push::open_target(&info))
    else {
        return;
    };
    *PENDING_OPEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(target);
    if let Some(app) = APP.get() {
        if let Err(error) = app.emit(PUSH_OPEN_EVENT, ()) {
            tracing::warn!(%error, "push: could not tell the webview a notification was tapped");
        }
    }
}

/// Release a string [`keeper_push_action`] returned.
///
/// # Safety
/// `text` is null or a pointer [`keeper_push_action`] returned, released
/// once.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn keeper_push_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    // SAFETY: by the contract above it came from `CString::into_raw` in
    // `handed_over` and is released exactly once.
    drop(unsafe { CString::from_raw(text) });
}

// The bridge, `gen/apple/Sources/keeper/KeeperPush.swift`, compiled into the
// app target: `keeper_push_install` takes the categories as JSON, becomes
// the notification center's delegate (wrapping whatever delegate is there)
// and, the first time, asks UIKit for remote notifications once its main
// loop runs. (A plain comment: rustdoc has no page for an extern block.)
extern "C" {
    fn keeper_push_install(categories: *const c_char);
}

/// Install the delegate and the categories through the bridge.
#[allow(unsafe_code)]
fn bridge_install() {
    let Ok(categories) = CString::new(push::registered_categories().to_string()) else {
        return;
    };
    // SAFETY: a NUL-terminated buffer that outlives the call; the bridge
    // copies it into a Swift string before returning and keeps no pointer.
    unsafe { keeper_push_install(categories.as_ptr()) };
}
