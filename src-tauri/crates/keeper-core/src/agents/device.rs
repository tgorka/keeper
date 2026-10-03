//! The agents on the person's device (AD-383): each live account's surface
//! request handler and presence publisher.
//!
//! [`AgentDevice`] is the app's one: the shell tells it whether keeper is in
//! front and which primary view it shows, and reads the requests this device
//! admitted. [`register`] starts one account's pair at activation, before
//! sync starts, so a request in the first batch is not missed; dropping the
//! returned [`AccountAgents`] stops both.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use matrix_sdk::event_handler::EventHandlerDropGuard;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::{Client, Room};
use serde_json::Value;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::account::send_agent_event;
use crate::agents::events::{
    PresencePlatform, SurfaceOutcome, SurfaceResultContent, CONTENT_VERSION, PRESENCE,
    SURFACE_REQUEST,
};
use crate::agents::presence::{
    presence_content, DevicePresence, PresenceClock, RENEW_EVERY, UNKNOWN_VIEW,
};
use crate::agents::proxy::AgentOutbound;
use crate::agents::room::{holds_agent_power, AgentRoomKind};
use crate::agents::surface::{Admission, RequestEvent, SurfaceInbox, SurfaceRequestArrived};

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

/// One live account's agents: its surface request handler, its presence
/// publisher, and the requests it handed on. Dropping it stops both.
pub(crate) struct AccountAgents {
    _handler: EventHandlerDropGuard,
    presence: JoinHandle<()>,
    inbox: Arc<Mutex<SurfaceInbox>>,
}

impl Drop for AccountAgents {
    fn drop(&mut self) {
        self.presence.abort();
    }
}

impl AccountAgents {
    /// Whether the notes view may answer `request` in `room`; it takes it.
    pub(crate) fn answer(&self, room: &matrix_sdk::ruma::RoomId, request: &str) -> bool {
        lock(&self.inbox).answer(room, request)
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
pub(crate) fn register(client: &Client, account_id: &str, device: &AgentDevice) -> AccountAgents {
    let inbox = Arc::new(Mutex::new(SurfaceInbox::default()));
    let handled = Arc::clone(&inbox);
    let requests = device.requests.clone();
    let account_id = account_id.to_owned();
    let handle = client.add_event_handler(
        move |event: Raw<AnySyncTimelineEvent>, room: Room, client: Client| {
            let inbox = Arc::clone(&handled);
            let requests = requests.clone();
            let account_id = account_id.clone();
            async move {
                if event.get_field::<&str>("type").ok().flatten() != Some(SURFACE_REQUEST)
                    || AgentRoomKind::of(room.room_type().as_ref()) != Some(AgentRoomKind::Session)
                {
                    return;
                }
                let (Ok(value), Some(device)) =
                    (event.deserialize_as::<Value>(), client.device_id())
                else {
                    return;
                };
                let Some(sender) = value["sender"]
                    .as_str()
                    .and_then(|sender| matrix_sdk::ruma::UserId::parse(sender).ok())
                else {
                    return;
                };
                let levels = room.power_levels().await.ok();
                let admission = lock(&inbox).admit(RequestEvent {
                    event_id: value["event_id"].as_str().unwrap_or_default(),
                    room: room.room_id(),
                    content: &value["content"],
                    from_own_user: client.user_id() == Some(&*sender),
                    from_agent: holds_agent_power(levels.as_ref(), &sender),
                    this_device: device.as_str(),
                    now_ms: now_ms(),
                });
                match admission {
                    Admission::Forward(request) => {
                        // Nobody listening (no notes view yet) is the host's
                        // timeout to tell, not this device's.
                        let _ = requests.send(SurfaceRequestArrived {
                            account_id,
                            room_id: room.room_id().to_owned(),
                            request,
                        });
                    }
                    Admission::Expired(request) => {
                        let expired = SurfaceResultContent {
                            v: CONTENT_VERSION,
                            request: request.id,
                            device: device.to_string(),
                            outcome: SurfaceOutcome::Expired,
                            applied: None,
                            detail: None,
                        };
                        if let Err(error) =
                            send_agent_event(&room, AgentOutbound::SurfaceResult(expired)).await
                        {
                            tracing::warn!(room = %room.room_id(), %error, "agents: an expired surface request could not be answered");
                        }
                    }
                    Admission::Ignored(why) => {
                        tracing::debug!(room = %room.room_id(), why, "agents: a surface request is not this device's");
                    }
                }
            }
        },
    );
    let presence = tokio::spawn(publish_presence(
        client.clone(),
        device.presence.subscribe(),
    ));
    AccountAgents {
        _handler: client.event_handler_drop_guard(handle),
        presence,
        inbox,
    }
}

/// Publish this device's presence into every control room the account is
/// in, as [`PresenceClock`] says when, for as long as the account is live.
async fn publish_presence(client: Client, mut state: watch::Receiver<Option<DevicePresence>>) {
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
        let (Some(user), Some(device)) = (client.user_id(), client.device_id()) else {
            continue;
        };
        let content = presence_content(user, device.as_str(), &due, now_ms());
        let Ok(content) = serde_json::to_value(&content) else {
            continue;
        };
        for room in client.joined_rooms() {
            if AgentRoomKind::of(room.room_type().as_ref()) != Some(AgentRoomKind::Control) {
                continue;
            }
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
}
