//! Push on Android, through the person's UnifiedPush distributor (Epic 98,
//! story 98.4 #1–#3, #8; AD-414, D-36, the epic's Q6, Q11, Q12).
//!
//! # What it is
//!
//! On Android a push reaches keeper through a *distributor* the person
//! installed — the ntfy app, pointed at a server they run. keeper registers
//! with a distributor only after the person confirmed that one, by name and
//! package, in Settings on this device ([`choose_distributor`]); the choice
//! is kept on this device and a different distributor asks again. The
//! distributor gives keeper an [`Endpoint`]: a URL on its server that
//! delivers whatever is posted to it to keeper on this device. A Matrix
//! homeserver cannot post to that URL itself; it posts to a *Matrix push
//! gateway*, which forwards to the endpoint. keeper never chooses that
//! gateway (D-1, D-4): it asks the endpoint's own server whether it is one —
//! the UnifiedPush convention is a `GET` of [`NOTIFY_PATH`] on the endpoint's
//! host answering `{"unifiedpush":{"gateway":"matrix"}}` ([`discover_gateway`])
//! — and when it is, that address, read through [`Gateway::parse`], is the
//! pusher's gateway ([`PusherSpec::unifiedpush`](crate::agents::push::PusherSpec::unifiedpush)).
//! A definite other answer is refused with a sentence naming the host, and
//! no pusher is registered; a server keeper cannot reach changes nothing
//! ([`pusher_change`]). So on Android, and only there, keeper itself
//! contacts the push server: once per endpoint, to ask that one question
//! ([`known_gateway`]; codemap §3 row 15).
//!
//! # What a push shows
//!
//! The gateway forwards the homeserver's `event_id_only` notification to the
//! endpoint, and the distributor hands its bytes to keeper's receiver
//! ([`pushed_event`]). keeper reads the one event with the account that knows
//! the room, in its own process, and the iPhone's classifier decides what
//! shows ([`crate::agents::push::shown_of`]) — one table for both phones.
//! [`phone_notification`] lays the answer out for Android's notification
//! manager: the categories' actions only from Android 12 (API 31), the first
//! that can require an unlocked device for an action (Q12); before it, no
//! actions — the notification opens keeper.
//!
//! The shell's `push_android.rs` and the Kotlin in `gen/android` carry these
//! answers and decide nothing (AD-55/AD-56).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::agents::push::{
    Category, Gateway, PushApproval, Shown, APPROVAL_INFO, FINISH_IN_KEEPER, GENERIC_BODY,
    GENERIC_TITLE, NOTIFY_PATH, PAYLOAD_ACCOUNT, SHOWN_INFO,
};

/// The pusher's `app_id` on Android: one app for every Android build, since
/// UnifiedPush has no signing environment.
pub const APP_ID: &str = "dev.tgorka.keeper.android";

/// The longest pushkey a homeserver keeps (Client-Server API, `POST
/// /pushers/set`: at most 512 bytes) — and so the longest endpoint keeper
/// registers.
pub const PUSHKEY_MAX: usize = 512;

/// The first Android API level whose notification actions can require an
/// unlocked device (`Notification.Action.Builder.setAuthenticationRequired`,
/// Android 12): below it a notification carries no actions (Q12).
pub const ACTIONS_NEED_UNLOCK_FROM: u32 = 31;

/// How long keeper waits for the endpoint's server to answer whether it is a
/// Matrix gateway.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long keeper waits, on Android, for the keys of a pushed event it
/// cannot decrypt yet: they travel to this device in the sync beside the
/// push.
pub const DECRYPT_BUDGET: Duration = Duration::from_secs(10);

/// How often the pushed event is read again while its keys are waited for.
pub const DECRYPT_POLL: Duration = Duration::from_millis(1500);

/// The most of an answer to the discovery question keeper reads; the answer
/// it accepts is a few dozen bytes.
const DISCOVERY_BODY_MAX: usize = 16 * 1024;

/// The Android notification channel keeper's pushes post in, and its name
/// and description in the system's notification settings.
pub const CHANNEL_ID: &str = "keeper-agents";
/// See [`CHANNEL_ID`].
pub const CHANNEL_NAME: &str = "Agent approvals and answers";
/// See [`CHANNEL_ID`].
pub const CHANNEL_DESCRIPTION: &str =
    "Your agents' approval requests and your own agent's answers, read on this device.";

/// The tag of the one notification keeper posts when a push arrives and keeper
/// cannot read it — Android had stopped keeper, or the event could not be
/// read: each such push replaces the last, so they never pile up.
pub const GENERIC_TAG: &str = "keeper-generic";

/// A UnifiedPush endpoint keeper will hand a homeserver as a pushkey:
/// `https://`, no user name or password, at most [`PUSHKEY_MAX`] bytes. Its
/// path and query are the distributor's (ntfy's `https://ntfy.example/upAbc?up=1`)
/// and are a capability: anyone who has the endpoint can post to this device.
/// So it is never logged or shown — only its host is ([`Endpoint::host`]).
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    url: url::Url,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Endpoint").field(&self.host()).finish()
    }
}

/// Why an endpoint the distributor gave is not one keeper registers. The
/// `Display` text names its host, never the endpoint itself.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointRefusal {
    /// Not a URL with a host.
    #[error("Your UnifiedPush distributor gave keeper an address that is not a web address, so no pusher is registered.")]
    NotAnAddress,
    /// A scheme other than `https`: the gateway on the same server would be
    /// reached unencrypted, and keeper registers no such gateway.
    #[error("Your UnifiedPush distributor's server {host} is reached over {scheme}://; keeper registers push only over https://, so no pusher is registered. Give that server an https:// address in the distributor.")]
    NotHttps {
        /// The endpoint's host.
        host: String,
        /// The scheme it carried.
        scheme: String,
    },
    /// A user name or password in the address, which the homeserver would
    /// keep.
    #[error("The address your UnifiedPush distributor gave keeper on {host} carries a user name or password; keeper does not hand that to your homeserver, so no pusher is registered.")]
    Credentials {
        /// The endpoint's host.
        host: String,
    },
    /// Longer than a homeserver keeps.
    #[error("The address your UnifiedPush distributor gave keeper on {host} is longer than a homeserver keeps (512 bytes), so no pusher is registered.")]
    TooLong {
        /// The endpoint's host.
        host: String,
    },
}

impl Endpoint {
    /// Read the endpoint as the distributor gave it.
    pub fn parse(raw: &str) -> Result<Endpoint, EndpointRefusal> {
        let raw = raw.trim();
        let url = url::Url::parse(raw).map_err(|_| EndpointRefusal::NotAnAddress)?;
        let host =
            crate::agents::mcp::url_host(url.as_str()).ok_or(EndpointRefusal::NotAnAddress)?;
        if url.scheme() != "https" {
            return Err(EndpointRefusal::NotHttps {
                host,
                scheme: url.scheme().to_owned(),
            });
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(EndpointRefusal::Credentials { host });
        }
        if url.as_str().len() > PUSHKEY_MAX {
            return Err(EndpointRefusal::TooLong { host });
        }
        Ok(Endpoint { url })
    }

    /// The whole endpoint: the pusher's pushkey. A capability — never log it.
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// The endpoint's host, and its port when it names one: what the egress
    /// list and Settings show.
    pub fn host(&self) -> String {
        crate::agents::mcp::url_host(self.url.as_str()).unwrap_or_default()
    }

    /// The address keeper asks whether the endpoint's server is a Matrix
    /// gateway: [`NOTIFY_PATH`] on the endpoint's own scheme, host and port.
    pub fn probe(&self) -> url::Url {
        let mut probe = self.url.clone();
        probe.set_path(NOTIFY_PATH);
        probe.set_query(None);
        probe.set_fragment(None);
        probe
    }

    /// The pusher's identity on a homeserver: the endpoint and [`APP_ID`].
    pub fn pusher_ids(&self) -> matrix_sdk::ruma::api::client::push::PusherIds {
        matrix_sdk::ruma::api::client::push::PusherIds::new(
            self.as_str().to_owned(),
            APP_ID.to_owned(),
        )
    }
}

/// Why the endpoint's server is not a gateway keeper registers (98.4 #2).
/// Every sentence names the host, so the person knows which server to look
/// at; none suggests another server — keeper never substitutes one (D-4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryRefusal {
    /// The endpoint itself is refused.
    #[error(transparent)]
    Endpoint(#[from] EndpointRefusal),
    /// The server could not be reached, or did not answer in time: keeper
    /// does not know, so it changes nothing ([`pusher_change`]).
    #[error("keeper could not reach {host} to ask whether it is a Matrix push gateway, so it changed nothing: a pusher registered before stays. keeper asks again the next time it opens.")]
    Unreachable {
        /// The endpoint's host.
        host: String,
    },
    /// The server answered with a status other than 200 (a 404 is a server
    /// with no Matrix gateway; a redirect is not followed).
    #[error("{host} answered {status} when keeper asked whether it is a Matrix push gateway, so it is not one and no pusher is registered. Use a UnifiedPush server that is also a Matrix gateway (ntfy is).")]
    Status {
        /// The endpoint's host.
        host: String,
        /// The status it answered.
        status: u16,
    },
    /// The server answered something that is not the UnifiedPush answer.
    #[error("{host} did not answer as a Matrix push gateway when keeper asked, so no pusher is registered. Use a UnifiedPush server that is also a Matrix gateway (ntfy is).")]
    NotMatrix {
        /// The endpoint's host.
        host: String,
    },
    /// The answer is a UnifiedPush gateway of another kind.
    #[error("{host} says it is a UnifiedPush gateway for {kind}, not for Matrix, so no pusher is registered.")]
    OtherGateway {
        /// The endpoint's host.
        host: String,
        /// The kind it named.
        kind: String,
    },
}

/// The gateway an answer to the discovery question names (98.4 #2): a 200
/// whose body is JSON with `unifiedpush.gateway` equal to `"matrix"` makes
/// the endpoint's [`Endpoint::probe`] the gateway, read through
/// [`Gateway::parse`]; any other status, a body that is not that JSON, and a
/// gateway of another kind are refused with a sentence naming the host.
pub fn gateway_of_answer(
    endpoint: &Endpoint,
    status: u16,
    body: &[u8],
) -> Result<Gateway, DiscoveryRefusal> {
    let host = endpoint.host();
    if status != 200 {
        return Err(DiscoveryRefusal::Status { host, status });
    }
    let answer: Value = serde_json::from_slice(body)
        .map_err(|_| DiscoveryRefusal::NotMatrix { host: host.clone() })?;
    match answer["unifiedpush"]["gateway"].as_str() {
        Some("matrix") => {}
        Some(kind) => {
            return Err(DiscoveryRefusal::OtherGateway {
                host,
                kind: kind.to_owned(),
            })
        }
        None => return Err(DiscoveryRefusal::NotMatrix { host }),
    }
    gateway_at(endpoint).ok_or(DiscoveryRefusal::NotMatrix { host })
}

/// The gateway on `endpoint`'s own server: [`Endpoint::probe`] through
/// [`Gateway::parse`].
fn gateway_at(endpoint: &Endpoint) -> Option<Gateway> {
    Gateway::parse(endpoint.probe().as_str()).ok().flatten()
}

/// What keeper kept of the endpoint it registered with last: the endpoint,
/// the distributor that gave it, and whether its server answered as a Matrix
/// gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptEndpoint {
    /// The endpoint.
    pub endpoint: Endpoint,
    /// The distributor that gave it.
    pub distributor: Distributor,
    /// Its server answered as a Matrix gateway, and the pusher is registered
    /// to it.
    pub gateway: bool,
}

/// The gateway keeper already knows for `endpoint`, so it does not ask
/// again: the endpoint is the one it kept, and that one's server answered
/// as a Matrix gateway. A new endpoint, or one whose server did not, is
/// asked ([`discover_gateway`]) — so keeper asks once per endpoint.
pub fn known_gateway(kept: Option<&KeptEndpoint>, endpoint: &Endpoint) -> Option<Gateway> {
    kept.filter(|kept| kept.gateway && kept.endpoint == *endpoint)
        .and_then(|_| gateway_at(endpoint))
}

/// What the answer about the endpoint's server does to the pusher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PusherChange {
    /// Register the pusher to this gateway (removing the previous
    /// endpoint's).
    Register(Gateway),
    /// The server is definitely not a gateway keeper registers: remove the
    /// pusher, and say why.
    Remove(DiscoveryRefusal),
    /// keeper could not ask: change nothing — not the pusher, not the
    /// endpoint it kept — and say why.
    Keep(DiscoveryRefusal),
}

/// What `answer` does to the pusher: a gateway registers it; a refusal that
/// is an answer — a status, a body that is not a Matrix gateway's, another
/// gateway kind, an endpoint keeper refuses — removes it; an unreachable
/// server is not an answer and keeps whatever is registered (R98UP1-01).
pub fn pusher_change(answer: Result<Gateway, DiscoveryRefusal>) -> PusherChange {
    match answer {
        Ok(gateway) => PusherChange::Register(gateway),
        Err(refusal @ DiscoveryRefusal::Unreachable { .. }) => PusherChange::Keep(refusal),
        Err(refusal) => PusherChange::Remove(refusal),
    }
}

/// The client discovery asks through: no redirects followed (a redirect to
/// another host would be a gateway the person did not name), and
/// [`DISCOVERY_TIMEOUT`].
pub fn discovery_client() -> reqwest::Result<reqwest::Client> {
    discovery_client_from(reqwest::Client::builder())
}

/// [`discovery_client`]'s policy over `builder` — for a caller that must
/// trust another root, as the discovery tests' local server does.
pub fn discovery_client_from(builder: reqwest::ClientBuilder) -> reqwest::Result<reqwest::Client> {
    builder
        .redirect(reqwest::redirect::Policy::none())
        .timeout(DISCOVERY_TIMEOUT)
        .build()
}

/// Ask the endpoint's server whether it is a Matrix push gateway, and read
/// its answer ([`gateway_of_answer`]). keeper asks nothing else and sends
/// nothing but the `GET`; no default gateway is ever used.
pub async fn discover_gateway(
    http: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<Gateway, DiscoveryRefusal> {
    let unreachable = || DiscoveryRefusal::Unreachable {
        host: endpoint.host(),
    };
    let mut response = http
        .get(endpoint.probe())
        .send()
        .await
        .map_err(|_| unreachable())?;
    let status = response.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unreachable())? {
        body.extend_from_slice(&chunk);
        if body.len() > DISCOVERY_BODY_MAX {
            return Err(DiscoveryRefusal::NotMatrix {
                host: endpoint.host(),
            });
        }
    }
    gateway_of_answer(endpoint, status, &body)
}

/// The event a push names: the homeserver's notification as the gateway
/// forwarded it (Push Gateway API, `{"notification": {"room_id",
/// "event_id", …}}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushedEvent {
    pub room_id: matrix_sdk::ruma::OwnedRoomId,
    pub event_id: matrix_sdk::ruma::OwnedEventId,
}

/// The room and event a push's bytes name; `None` for anything else — a
/// count-only notification, or bytes that are not the gateway's JSON.
pub fn pushed_event(message: &[u8]) -> Option<PushedEvent> {
    let message: Value = serde_json::from_slice(message).ok()?;
    let notification = &message["notification"];
    Some(PushedEvent {
        room_id: matrix_sdk::ruma::RoomId::parse(notification["room_id"].as_str()?).ok()?,
        event_id: matrix_sdk::ruma::EventId::parse(notification["event_id"].as_str()?).ok()?,
    })
}

/// A UnifiedPush distributor installed on this device: its package, and the
/// name Android shows for it. Any app can declare itself a distributor, so
/// both are shown whenever one is named — a name alone can be borrowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distributor {
    /// Its Android package (`io.heckel.ntfy`).
    pub package: String,
    /// Its name on this device (`ntfy`).
    pub label: String,
}

impl Distributor {
    /// How keeper names it: `ntfy (io.heckel.ntfy)`.
    pub fn named(&self) -> String {
        format!("{} ({})", self.label, self.package)
    }
}

/// Which distributor keeper registers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributorChoice {
    /// The one the person confirmed on this device; it is still installed.
    Use(Distributor),
    /// No distributor is installed: no push.
    None,
    /// None of these was confirmed on this device — none was, or the one
    /// that was is gone: keeper registers with none until the person
    /// confirms one in Settings, and never picks for them (D-4, R98UP1-02).
    Ask(Vec<Distributor>),
}

/// The distributor keeper registers with: the one the person confirmed on
/// this device (`confirmed`, a package) while it is installed; else it asks
/// about every installed one — the only one too; else none.
pub fn choose_distributor(confirmed: Option<&str>, installed: &[Distributor]) -> DistributorChoice {
    if let Some(one) =
        confirmed.and_then(|confirmed| installed.iter().find(|one| one.package == confirmed))
    {
        return DistributorChoice::Use(one.clone());
    }
    match installed {
        [] => DistributorChoice::None,
        asked => DistributorChoice::Ask(asked.to_vec()),
    }
}

/// The distributor the person confirmed in Settings: `package`, when it is
/// one keeper asked about (`asked`); anything else confirms nothing.
pub fn confirmed_of(asked: &[Distributor], package: &str) -> Option<Distributor> {
    asked.iter().find(|one| one.package == package).cloned()
}

/// One action on an Android notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneAction {
    /// The action's id (`push::NotificationAction::id`).
    pub id: String,
    pub label: String,
    /// *Deny*: drawn as the destructive one where Android draws that.
    pub destructive: bool,
}

/// A notification as Android's notification manager posts it: what
/// `UnifiedPushPlugin.kt` builds and nothing more. Every action requires an
/// unlocked device; `user_info` rides along in keeper's own intents and comes
/// back with an action or a tap, as `userInfo` does on the iPhone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneNotification {
    /// Replaces an earlier notification with the same tag.
    pub tag: String,
    /// Notifications of one room group together.
    pub group: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    /// The category it is shown with (`push::Category::id`); `None` with no
    /// actions.
    pub category: Option<String>,
    pub actions: Vec<PhoneAction>,
    pub user_info: Value,
}

/// What `shown` — read for `account` from the event `event_id` of `room_id`
/// — posts on a device of API level `sdk_int`: the iPhone's words; its
/// category's actions only from [`ACTIONS_NEED_UNLOCK_FROM`] — below that no
/// category and no actions, and the record it carries says so, so no action
/// can name one; `user_info` the account, the room, the record and
/// [`SHOWN_INFO`], which only a notification keeper composed carries
/// (`push::action_decision`, `push::tap_target`). One notification per
/// approval, per answer (its edits replace it) or else per event.
pub fn phone_notification(
    account: &str,
    room_id: &str,
    event_id: &str,
    shown: &Shown,
    sdk_int: u32,
) -> PhoneNotification {
    let actionable = sdk_int >= ACTIONS_NEED_UNLOCK_FROM;
    let category = shown.category.filter(|_| actionable);
    let approval = shown.approval.clone().map(|approval| PushApproval {
        category: approval.category.filter(|_| actionable),
        ..approval
    });
    let tag = approval
        .as_ref()
        .map(|approval| approval.id.clone())
        .or_else(|| shown.answer.clone())
        .unwrap_or_else(|| event_id.to_owned());
    let mut user_info = json!({
        PAYLOAD_ACCOUNT: account,
        SHOWN_INFO: true,
        "room_id": room_id,
    });
    if let Some(approval) = &approval {
        user_info[APPROVAL_INFO] = json!(approval);
    }
    PhoneNotification {
        tag,
        group: shown.thread_id.clone(),
        title: shown.title.clone(),
        subtitle: shown.subtitle.clone(),
        body: shown.body.clone(),
        category: category.map(|category| category.id().to_owned()),
        actions: category
            .map(Category::actions)
            .unwrap_or_default()
            .iter()
            .map(|action| PhoneAction {
                id: action.id().to_owned(),
                label: action.label().to_owned(),
                destructive: action.destructive(),
            })
            .collect(),
        user_info,
    }
}

/// The notification keeper posts when a push arrived and it cannot say what
/// it was: keeper's own words, no actions, opening keeper; one at a time
/// ([`GENERIC_TAG`]).
pub fn generic_notification() -> PhoneNotification {
    PhoneNotification {
        tag: GENERIC_TAG.to_owned(),
        group: GENERIC_TAG.to_owned(),
        title: GENERIC_TITLE.to_owned(),
        subtitle: None,
        body: GENERIC_BODY.to_owned(),
        category: None,
        actions: Vec::new(),
        user_info: json!({}),
    }
}

/// The notification that replaces one whose action could not be sent: asks
/// the person to finish in keeper (`why`, when known, says why), no actions,
/// and a tap opens where the first one did — the agents server session, or
/// the Matrix session room ([`crate::agents::push::reposted_info`]) —
/// `user_info` carries [`SHOWN_INFO`] only when that one did.
pub fn finish_in_keeper(tag: &str, user_info: &Value, why: &str) -> PhoneNotification {
    let body = if why.is_empty() {
        FINISH_IN_KEEPER.to_owned()
    } else {
        format!("{FINISH_IN_KEEPER} {why}")
    };
    PhoneNotification {
        tag: tag.to_owned(),
        group: crate::agents::push::reposted_group(user_info)
            .unwrap_or_else(|| GENERIC_TAG.to_owned()),
        title: GENERIC_TITLE.to_owned(),
        subtitle: None,
        body,
        category: None,
        actions: Vec::new(),
        user_info: crate::agents::push::reposted_info(user_info).unwrap_or_else(|| json!({})),
    }
}

/// What `shown` — an agents server item read in keeper — posts on a device
/// of API level `sdk_int`: the iPhone's words; its category's actions only
/// from [`ACTIONS_NEED_UNLOCK_FROM`] — below that no category and no
/// actions, and the request it carries says so, so no action can name one;
/// `user_info` keeper's composed keys ([`crate::hq::push::composed_info`]).
/// One notification per item: its tag is the item's, so a second delivery
/// replaces the first.
pub fn item_notification(shown: &crate::hq::push::HqShown, sdk_int: u32) -> PhoneNotification {
    let actionable = sdk_int >= ACTIONS_NEED_UNLOCK_FROM;
    let category = shown.category.filter(|_| actionable);
    let mut shown = shown.clone();
    if let Some(approval) = shown.approval.as_mut() {
        approval.category = approval.category.filter(|_| actionable);
    }
    PhoneNotification {
        tag: shown.item.tag(),
        group: shown.thread_id.clone(),
        title: shown.title.clone(),
        subtitle: shown.subtitle.clone(),
        body: shown.body.clone(),
        category: category.map(|category| category.id().to_owned()),
        actions: category
            .map(Category::actions)
            .unwrap_or_default()
            .iter()
            .map(|action| PhoneAction {
                id: action.id().to_owned(),
                label: action.label().to_owned(),
                destructive: action.destructive(),
            })
            .collect(),
        user_info: crate::hq::push::composed_info(&shown),
    }
}

/// Where push on this device stands, for Settings → Notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedPushStatus {
    /// keeper has not heard from the distributor yet this launch.
    Waiting,
    /// No distributor is installed.
    NoDistributor,
    /// keeper asks the person which of these to use: none was confirmed on
    /// this device, or the one that was is gone.
    Asking(Vec<Distributor>),
    /// The distributor refused to register keeper.
    RegistrationFailed,
    /// The distributor dropped keeper's registration.
    Unregistered,
    /// The endpoint or its server is refused.
    Refused(DiscoveryRefusal),
    /// The pusher is registered: the gateway's host, on the server of the
    /// distributor the person confirmed.
    Registered {
        /// The gateway's host.
        host: String,
        /// The distributor whose server it is.
        distributor: Distributor,
    },
}

impl UnifiedPushStatus {
    /// The sentence Settings shows, for a device called `noun`.
    pub fn sentence(&self, noun: &str) -> String {
        match self {
            UnifiedPushStatus::Waiting => {
                "Waiting for your UnifiedPush distributor to give keeper an address.".to_owned()
            }
            UnifiedPushStatus::NoDistributor => format!(
                "No UnifiedPush distributor is installed on this {noun}, so nothing reaches it while keeper is closed. Install one — the ntfy app, pointed at a server you run — and open keeper again."
            ),
            UnifiedPushStatus::Asking(asked) => format!(
                "keeper registers for push on this {noun} only through a UnifiedPush distributor you confirm here. Installed: {}. The one you confirm gives keeper an address on its server; keeper asks that server once whether it is a Matrix push gateway, and your homeserver then sends it the id of every message that would notify on accounts that hold agent rooms, never its words.",
                asked.iter().map(Distributor::named).collect::<Vec<_>>().join(", ")
            ),
            UnifiedPushStatus::RegistrationFailed => {
                "Your UnifiedPush distributor refused to register keeper, so no pusher is registered.".to_owned()
            }
            UnifiedPushStatus::Unregistered => {
                "Your UnifiedPush distributor removed keeper's registration, so keeper removed its pusher.".to_owned()
            }
            UnifiedPushStatus::Refused(why) => why.to_string(),
            UnifiedPushStatus::Registered { host, distributor } => format!(
                "Push reaches this {noun} through {host}, the server of {}, the UnifiedPush distributor you confirmed; it answers as a Matrix push gateway. Your homeserver sends it the id of every message that would notify on accounts that hold agent rooms, never its words.",
                distributor.named()
            ),
        }
    }
}

/// Settings → Notifications on Android: where push stands, what push on
/// Android brings and what it does not (`disclosure::android_push_line`),
/// and the distributors the person is asked to confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UnifiedPushVm {
    /// Where push stands ([`UnifiedPushStatus::sentence`]).
    pub status: String,
    /// What push brings on this device.
    pub note: String,
    /// One per distributor keeper asks about ([`UnifiedPushStatus::Asking`]);
    /// empty otherwise.
    pub offers: Vec<DistributorOfferVm>,
}

/// One distributor Settings asks the person to confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DistributorOfferVm {
    /// Its package: what `unifiedpush_confirm` is given.
    pub package: String,
    /// The button's words, naming it by name and package.
    pub confirm: String,
}

impl UnifiedPushVm {
    /// The view of `status` on a device called `noun`.
    pub fn of(status: &UnifiedPushStatus, noun: &str) -> UnifiedPushVm {
        let offers = match status {
            UnifiedPushStatus::Asking(asked) => asked
                .iter()
                .map(|one| DistributorOfferVm {
                    package: one.package.clone(),
                    confirm: format!("Use {} for push", one.named()),
                })
                .collect(),
            _ => Vec::new(),
        };
        UnifiedPushVm {
            status: status.sentence(noun),
            note: crate::disclosure::android_push_line(noun),
            offers,
        }
    }
}

/// Where keeper keeps the endpoint the distributor last gave it
/// ([`KeptEndpoint`]), under the data directory: the egress list and a
/// sign-out read it while the distributor has not said it again this launch,
/// and the next launch does not ask its server again ([`known_gateway`]).
const ENDPOINT_FILE: &str = "unifiedpush-endpoint";

/// Where keeper keeps the distributor the person confirmed, under the data
/// directory — on this device only; nothing syncs it.
const DISTRIBUTOR_FILE: &str = "unifiedpush-distributor";

/// How [`KeptEndpoint`] is written.
#[derive(Serialize, Deserialize)]
struct KeptFile {
    endpoint: String,
    distributor: Distributor,
    gateway: bool,
}

/// What keeper kept of the endpoint it registered with last, if it kept one
/// that still parses.
pub fn kept_endpoint(data_dir: &std::path::Path) -> Option<KeptEndpoint> {
    let raw = std::fs::read(data_dir.join(ENDPOINT_FILE)).ok()?;
    let kept: KeptFile = serde_json::from_slice(&raw).ok()?;
    Some(KeptEndpoint {
        endpoint: Endpoint::parse(&kept.endpoint).ok()?,
        distributor: kept.distributor,
        gateway: kept.gateway,
    })
}

/// The endpoint keeper kept ([`kept_endpoint`]).
pub fn stored_endpoint(data_dir: &std::path::Path) -> Option<Endpoint> {
    kept_endpoint(data_dir).map(|kept| kept.endpoint)
}

/// Keep `kept` as the endpoint keeper registers with, or forget it (`None`).
pub fn keep_endpoint(
    data_dir: &std::path::Path,
    kept: Option<&KeptEndpoint>,
) -> std::io::Result<()> {
    let path = data_dir.join(ENDPOINT_FILE);
    match kept {
        Some(kept) => std::fs::write(
            path,
            serde_json::to_vec(&KeptFile {
                endpoint: kept.endpoint.as_str().to_owned(),
                distributor: kept.distributor.clone(),
                gateway: kept.gateway,
            })?,
        ),
        None => match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

/// The package of the distributor the person confirmed on this device.
pub fn confirmed_distributor(data_dir: &std::path::Path) -> Option<String> {
    let raw = std::fs::read(data_dir.join(DISTRIBUTOR_FILE)).ok()?;
    serde_json::from_slice::<Distributor>(&raw)
        .ok()
        .map(|one| one.package)
}

/// The person confirmed `distributor` on this device: keep it, replacing any
/// other.
pub fn confirm_distributor(
    data_dir: &std::path::Path,
    distributor: &Distributor,
) -> std::io::Result<()> {
    std::fs::write(
        data_dir.join(DISTRIBUTOR_FILE),
        serde_json::to_vec(distributor)?,
    )
}
