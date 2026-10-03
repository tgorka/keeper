//! The agents on the person's device (AD-383): each live account's surface
//! request handler and presence publisher.
//!
//! [`AgentDevice`] is the app's one: the shell tells it whether keeper is in
//! front and which primary view it shows, and reads the requests this device
//! admitted. [`register`] starts one account's pair at activation, before
//! sync starts, so a request in the first batch is not missed; dropping the
//! returned [`AccountAgents`] stops both, after [`AccountAgents::goodbye`]
//! has said the device is no longer in front.
//!
//! The device acts only for its own proxy, in that proxy's own conversation
//! ([`proxy::room_proxy`]), and publishes where it is only into control rooms
//! its own proxies made ([`proxy::own_proxies`]): a person may be a member
//! of another principal's rooms, whose agents hold the same power there, and
//! neither their requests nor "is tgorka at his keyboard" travel between them.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use matrix_sdk::event_handler::EventHandlerDropGuard;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::{Client, Room};
use serde_json::Value;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::account::{proxy_row, send_agent_event};
use crate::agents::events::{
    PresencePlatform, SurfaceOutcome, SurfaceResultContent, CONTENT_VERSION, PRESENCE,
    SURFACE_REQUEST,
};
use crate::agents::presence::{
    presence_content, DevicePresence, PresenceClock, RENEW_EVERY, UNKNOWN_VIEW,
};
use crate::agents::proxy::{self, AgentOutbound, AgentProxies};
use crate::agents::room::{holds_agent_power, AgentKinds, AgentRoomKind};
use crate::agents::surface::{
    Admission, RequestEvent, SurfaceInbox, SurfaceRequestArrived, Waiting, CANNOT_SHOW,
};

/// The app's agents state on this device, shared by every account.
#[derive(Debug)]
pub struct AgentDevice {
    requests: broadcast::Sender<SurfaceRequestArrived>,
    presence: watch::Sender<Option<DevicePresence>>,
}

impl Default for AgentDevice {
    fn default() -> Self {
        AgentDevice {
            requests: broadcast::channel(16).0,
            presence: watch::channel(None).0,
        }
    }
}

impl AgentDevice {
    /// Every surface request a live account admits from now on.
    pub fn requests(&self) -> broadcast::Receiver<SurfaceRequestArrived> {
        self.requests.subscribe()
    }

    /// keeper came to the front (`true`) or left it, on `platform`.
    pub fn focus(&self, platform: PresencePlatform, focused: bool) {
        self.presence.send_modify(|state| {
            let view = state
                .as_ref()
                .map_or_else(|| UNKNOWN_VIEW.to_owned(), |state| state.view.clone());
            *state = Some(DevicePresence {
                platform,
                focused,
                view,
            });
        });
    }

    /// The primary view shown now; nothing is published before the shell
    /// has said whether keeper is in front.
    pub fn view(&self, view: String) {
        self.presence.send_modify(|state| {
            if let Some(state) = state {
                state.view = view;
            }
        });
    }
}

/// What this device says as it goes (quit, sign-out): no longer in front —
/// else a host keeps sending it calls, as the device in front renewed last,
/// until its presence's TTL. Nothing where the shell never said whether
/// keeper was in front: nothing was published to take back.
pub fn farewell(state: Option<DevicePresence>) -> Option<DevicePresence> {
    state.map(|state| DevicePresence {
        focused: false,
        ..state
    })
}

/// One live account's agents: its surface request handler, its presence
/// publisher, and the requests it handed on. Dropping it stops both.
pub(crate) struct AccountAgents {
    _handler: EventHandlerDropGuard,
    presence: JoinHandle<()>,
    inbox: Arc<Mutex<SurfaceInbox>>,
    publisher: Arc<Publisher>,
    state: watch::Receiver<Option<DevicePresence>>,
}

impl Drop for AccountAgents {
    fn drop(&mut self) {
        self.presence.abort();
    }
}

impl AccountAgents {
    /// The notes view answers `request` in `room`: take it, when this device
    /// was handed it and has not answered it yet.
    pub(crate) fn take(&self, room: &RoomId, request: &str) -> Option<Waiting> {
        lock(&self.inbox).take(room, request)
    }

    /// The answer taken for `request` did not leave: it waits again.
    pub(crate) fn restore(&self, request: String, waiting: Waiting) {
        lock(&self.inbox).restore(request, waiting);
    }

    /// Say this device is no longer in front ([`farewell`]) while the client
    /// can still send, within `bound`: a homeserver that does not answer
    /// never holds a quit or a sign-out.
    pub(crate) async fn goodbye(&self, bound: Duration) {
        let Some(state) = farewell(self.state.borrow().clone()) else {
            return;
        };
        if tokio::time::timeout(bound, self.publisher.publish(&state))
            .await
            .is_err()
        {
            tracing::debug!("agents: this device's goodbye presence did not land in time");
        }
    }
}

fn lock(inbox: &Mutex<SurfaceInbox>) -> MutexGuard<'_, SurfaceInbox> {
    // The inbox holds ids and nothing else: a panic mid-update tears nothing.
    inbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

/// Start `account_id`'s surface request handler and presence publisher.
pub(crate) fn register(
    client: &Client,
    account_id: &str,
    device: &AgentDevice,
    kinds: Arc<AgentKinds>,
    proxies: Arc<AgentProxies>,
) -> AccountAgents {
    let inbox = Arc::new(Mutex::new(SurfaceInbox::default()));
    let handled = Arc::clone(&inbox);
    let requests = device.requests.clone();
    let account_id = account_id.to_owned();
    let (handler_kinds, handler_proxies) = (Arc::clone(&kinds), Arc::clone(&proxies));
    let handle = client.add_event_handler(
        move |event: Raw<AnySyncTimelineEvent>, room: Room, client: Client| {
            let inbox = Arc::clone(&handled);
            let requests = requests.clone();
            let account_id = account_id.clone();
            let kinds = Arc::clone(&handler_kinds);
            let proxies = Arc::clone(&handler_proxies);
            async move {
                if event.get_field::<&str>("type").ok().flatten() != Some(SURFACE_REQUEST)
                    || AgentRoomKind::of(room.room_type().as_ref()) != Some(AgentRoomKind::Session)
                {
                    return;
                }
                let (Ok(value), Some(device), Some(me)) = (
                    event.deserialize_as::<Value>(),
                    client.device_id(),
                    client.user_id(),
                ) else {
                    return;
                };
                let Some(sender) = value["sender"]
                    .as_str()
                    .and_then(|sender| matrix_sdk::ruma::UserId::parse(sender).ok())
                else {
                    return;
                };
                let levels = room.power_levels().await.ok();
                // The room as one of this person's proxy conversations, and
                // the proxy whose it is: the one sender acted on here.
                let row = proxy_row(room.clone(), &kinds).await;
                let proxy = proxy::room_proxy(&row, me, &proxies.snapshot());
                let now = now_ms();
                let admission = lock(&inbox).admit(RequestEvent {
                    event_id: value["event_id"].as_str().unwrap_or_default(),
                    room: room.room_id(),
                    content: &value["content"],
                    sender: &sender,
                    from_own_user: *me == *sender,
                    from_agent: holds_agent_power(levels.as_ref(), &sender),
                    proxy: proxy.as_ref(),
                    this_device: device.as_str(),
                    sent_ms: value["origin_server_ts"].as_u64().unwrap_or(now),
                    age_ms: value["unsigned"]["age"].as_u64(),
                    now_ms: now,
                });
                match admission {
                    Admission::Forward {
                        request,
                        deadline_ms,
                    } => {
                        // Nobody listening (no notes view yet) is the host's
                        // timeout to tell, not this device's.
                        let _ = requests.send(SurfaceRequestArrived {
                            account_id,
                            room_id: room.room_id().to_owned(),
                            request,
                            deadline_ms,
                            drives: proxy.and_then(|proxy| proxy.drives),
                        });
                    }
                    Admission::Expired(request) => {
                        answer(&room, device.as_str(), request.id, SurfaceOutcome::Expired, None)
                            .await;
                    }
                    Admission::Unavailable(request) => {
                        // The reason stays here: the room is told one fixed
                        // sentence, never which drives this device keeps.
                        tracing::debug!(room = %room.room_id(), drive = %request.args.drive, "agents: a surface request names a drive the room's proxy does not declare");
                        answer(
                            &room,
                            device.as_str(),
                            request.id,
                            SurfaceOutcome::Unavailable,
                            Some(CANNOT_SHOW),
                        )
                        .await;
                    }
                    Admission::Ignored(why) => {
                        tracing::debug!(room = %room.room_id(), why, "agents: a surface request is not this device's");
                    }
                }
            }
        },
    );
    let publisher = Arc::new(Publisher {
        client: client.clone(),
        kinds,
        proxies,
    });
    let presence = tokio::spawn(publish_presence(
        Arc::clone(&publisher),
        device.presence.subscribe(),
    ));
    AccountAgents {
        _handler: client.event_handler_drop_guard(handle),
        presence,
        inbox,
        publisher,
        state: device.presence.subscribe(),
    }
}

/// Answer a request this device will not show, from here: `expired`, or
/// `unavailable` with [`CANNOT_SHOW`].
async fn answer(
    room: &Room,
    device: &str,
    request: String,
    outcome: SurfaceOutcome,
    detail: Option<&str>,
) {
    let result = SurfaceResultContent {
        v: CONTENT_VERSION,
        request,
        device: device.to_owned(),
        outcome,
        applied: None,
        detail: detail.map(ToOwned::to_owned),
    };
    if let Err(error) = send_agent_event(room, AgentOutbound::SurfaceResult(result)).await {
        tracing::warn!(room = %room.room_id(), %error, "agents: a surface request this device will not show could not be answered");
    }
}

/// Where one account publishes this device's presence: the control rooms
/// its own proxies made.
struct Publisher {
    client: Client,
    kinds: Arc<AgentKinds>,
    proxies: Arc<AgentProxies>,
}

impl Publisher {
    /// The joined control rooms whose only creator is one of this person's
    /// own proxies ([`proxy::own_proxies`]): a shared principal's control
    /// room, which the person merely belongs to, is not among them.
    async fn own_control_rooms(&self) -> Vec<Room> {
        let Some(me) = self.client.user_id() else {
            return Vec::new();
        };
        let (mut sessions, mut controls) = (Vec::new(), Vec::new());
        for room in self.client.joined_rooms() {
            match AgentRoomKind::of(room.room_type().as_ref()) {
                Some(AgentRoomKind::Session) => sessions.push(room),
                Some(AgentRoomKind::Control) => controls.push(room),
                _ => {}
            }
        }
        let mut rows = Vec::with_capacity(sessions.len());
        for room in sessions {
            rows.push(proxy_row(room, &self.kinds).await);
        }
        let own = proxy::own_proxies(&rows, me, &self.proxies.snapshot());
        controls
            .retain(|room| proxy::is_own_control_room(&room.creators().unwrap_or_default(), &own));
        controls
    }

    /// Publish `state` as this device's presence into each own control room.
    async fn publish(&self, state: &DevicePresence) {
        let (Some(user), Some(device)) = (self.client.user_id(), self.client.device_id()) else {
            return;
        };
        let content = presence_content(user, device.as_str(), state, now_ms());
        let Ok(content) = serde_json::to_value(&content) else {
            return;
        };
        for room in self.own_control_rooms().await {
            if let Err(error) = room
                .send_state_event_raw(PRESENCE, device.as_str(), content.clone())
                .await
            {
                // A control room made before R37 refuses a person's state
                // until its creator's host brings it up to date.
                tracing::warn!(room = %room.room_id(), %error, "agents: this device's presence could not be published");
            }
        }
    }
}

/// Publish this device's presence into the account's own control rooms, as
/// [`PresenceClock`] says when, for as long as the account is live.
async fn publish_presence(
    publisher: Arc<Publisher>,
    mut state: watch::Receiver<Option<DevicePresence>>,
) {
    let mut clock = PresenceClock::default();
    if let Some(now) = state.borrow_and_update().clone() {
        clock.change(now, Instant::now());
    }
    loop {
        let wake = clock
            .next_due()
            .unwrap_or_else(|| Instant::now() + RENEW_EVERY);
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                if let Some(now) = state.borrow_and_update().clone() {
                    clock.change(now, Instant::now());
                }
                continue;
            }
            () = tokio::time::sleep_until(wake.into()) => {}
        }
        let Some(due) = clock.due(Instant::now()) else {
            continue;
        };
        publisher.publish(&due).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_published_before_keeper_says_whether_it_is_in_front() {
        let device = AgentDevice::default();
        let state = device.presence.subscribe();
        device.view("notes".to_owned());
        assert_eq!(*state.borrow(), None, "a view alone says nothing");
        device.focus(PresencePlatform::Ios, true);
        device.view("chats".to_owned());
        assert_eq!(
            *state.borrow(),
            Some(DevicePresence {
                platform: PresencePlatform::Ios,
                focused: true,
                view: "chats".to_owned(),
            })
        );
        device.focus(PresencePlatform::Ios, false);
        assert_eq!(
            state
                .borrow()
                .as_ref()
                .map(|s| (s.focused, s.view.as_str())),
            Some((false, "chats"))
        );
    }

    /// A Mac quit while in front would stay the device in front for up to
    /// its presence's TTL, and a call routed to it would end `expired`
    /// rather than reach the phone in hand.
    #[test]
    fn a_quitting_device_says_it_is_no_longer_in_front() {
        let in_front = DevicePresence {
            platform: PresencePlatform::Macos,
            focused: true,
            view: "notes".to_owned(),
        };
        assert_eq!(
            farewell(Some(in_front.clone())),
            Some(DevicePresence {
                focused: false,
                ..in_front.clone()
            })
        );
        // Already out of front: said again, so the newest word is the true one.
        let away = DevicePresence {
            focused: false,
            ..in_front
        };
        assert_eq!(farewell(Some(away.clone())), Some(away));
        // The shell never said whether keeper was in front: nothing was
        // published, so there is nothing to take back.
        assert_eq!(farewell(None), None);
    }
}
