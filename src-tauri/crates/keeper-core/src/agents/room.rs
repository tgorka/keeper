//! An agent's room as a person's keeper draws it (AD-372, AD-373, AD-381).
//!
//! A room's create type says whether it is an agent room at all; in one, the
//! timeline keeps the SDK's own filter and also drops the claim and host
//! state the hosts renew every minute. The room's header — the status line,
//! the run badge, `agent@host`, the scope chip and the label chip — is read
//! beside the timeline from the room's event cache (ruling R33, option B: a
//! live run on Synapse showed the cache holds the agent's decrypted status
//! after a restart, before any sync).
//!
//! Status updates are a stream of `dev.keeper.agent.status` events, each
//! naming its anchor in `content.anchor` (R32). [`AgentRoomState`] reads them
//! as the host's `trail_of` does: only from the agent the content names, at
//! power 50 or more, never from the own user, the newest `origin_server_ts`
//! winning. A status keeper cannot read — a run or kind it does not know, a
//! newer `v` — is shown as *unreadable*, never dropped.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, RwLock};

use matrix_sdk::event_cache::RoomEventCacheUpdate;
use matrix_sdk::ruma::events::room::power_levels::RoomPowerLevels;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::room::RoomType;
use matrix_sdk::ruma::room_version_rules::RoomVersionRules;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{Int, OwnedRoomId, OwnedUserId, RoomId, UserId};
use matrix_sdk::Room;
use matrix_sdk_ui::eyeball_im::VectorDiff;
use matrix_sdk_ui::timeline::default_event_filter;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;
use ts_rs::TS;

use crate::agents::events::{
    RunState, StatusContent, CLAIM, CONTENT_VERSION, CONTROL_ROOM_TYPE, HOST, SCOPE,
    SESSION_ROOM_TYPE, STATUS, TURN,
};
use crate::agents::label::{Label, LabelVm};
use crate::agents::session::SessionKind;
use crate::bots::identity::parse_identity;

/// The power an agent user holds in its rooms (`events::power_levels`).
const AGENT_POWER: i64 = 50;

/// How far back the header looks for a status when the event cache holds
/// none: pages of [`HEADER_PAGE`] events, newest first.
const HEADER_PAGES: usize = 4;
const HEADER_PAGE: u32 = 50;

/// Which agent room a room is, from its `m.room.create` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRoomKind {
    /// A session room: a proxy's DM or conversation, or a session the person
    /// watches.
    Session,
    /// A principal's control room: never shown in a room list.
    Control,
}

impl AgentRoomKind {
    /// The kind of a room created with `room_type`, or `None` for every
    /// other room.
    pub fn of(room_type: Option<&RoomType>) -> Option<AgentRoomKind> {
        match room_type.map(RoomType::as_str) {
            Some(SESSION_ROOM_TYPE) => Some(AgentRoomKind::Session),
            Some(CONTROL_ROOM_TYPE) => Some(AgentRoomKind::Control),
            _ => None,
        }
    }
}

/// A room-list row's agent room (UX-DR132).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AgentRoomKindVm {
    /// A proxy conversation: the session's status says `main` or
    /// `conversation`. Lives in the Agents window.
    Proxy,
    /// A session the person watches: its status names any other kind.
    /// Lives in the Agents window.
    Session,
    /// A session room whose kind keeper has not read yet: no status in the
    /// loaded events. Lives in the Agents window, claiming neither.
    Unknown,
    /// A control room: in no window.
    Control,
}

impl AgentRoomKindVm {
    /// The row for a room of `kind` whose session kind is `session` when
    /// keeper has read one; an unread session kind is unknown, never
    /// guessed a proxy's or a watched session's.
    pub fn of(kind: AgentRoomKind, session: Option<SessionKind>) -> AgentRoomKindVm {
        match (kind, session) {
            (AgentRoomKind::Control, _) => AgentRoomKindVm::Control,
            (_, Some(SessionKind::Main | SessionKind::Conversation)) => AgentRoomKindVm::Proxy,
            (AgentRoomKind::Session, Some(_)) => AgentRoomKindVm::Session,
            (AgentRoomKind::Session, None) => AgentRoomKindVm::Unknown,
        }
    }
}

/// The timeline filter of a room of `kind` (ruling R33): the SDK's default
/// everywhere, and in an agent room also without the claim and host state
/// a host renews every minute, which would otherwise each add an item.
pub fn agent_event_filter(
    kind: Option<AgentRoomKind>,
) -> impl Fn(&AnySyncTimelineEvent, &RoomVersionRules) -> bool + Send + Sync + 'static {
    move |event, rules| {
        if kind.is_some() {
            if let AnySyncTimelineEvent::State(state) = event {
                // `StateEventType` has no `&str` view; this runs only for state
                // events in agent rooms.
                if matches!(state.event_type().to_string().as_str(), CLAIM | HOST) {
                    return false;
                }
            }
        }
        default_event_filter(event, rules)
    }
}

#[derive(Deserialize)]
struct TurnProbe {
    content: TurnProbeContent,
}

#[derive(Deserialize)]
struct TurnProbeContent {
    #[serde(rename = "dev.keeper.agent.turn")]
    turn: Option<IgnoredAny>,
}

/// Whether the event in `json` carries the `dev.keeper.agent.turn` marker in
/// its content. Anyone can write the marker; [`is_trusted_turn`] says whose
/// counts.
pub fn is_agent_turn(json: &str) -> bool {
    debug_assert_eq!(TURN, "dev.keeper.agent.turn");
    serde_json::from_str::<TurnProbe>(json).is_ok_and(|probe| probe.content.turn.is_some())
}

/// Whether a message is an agent's streamed answer: in a session room, from a
/// sender who is not the own user and holds an agent's power (`agent_power`),
/// with the turn marker in its content (`json`). Its edits are the answer
/// growing, not a correction (91.1 acceptance 4), and its `…` anchor is not a
/// message to notify about (R45). Anyone else's marker is an ordinary message,
/// as R30 has the device accept agent events only from the session's agent.
/// The marker is parsed last, so an ordinary room never reads the JSON.
pub fn is_trusted_turn(
    kind: Option<AgentRoomKind>,
    is_own: bool,
    agent_power: bool,
    json: Option<&str>,
) -> bool {
    kind == Some(AgentRoomKind::Session)
        && !is_own
        && agent_power
        && json.is_some_and(is_agent_turn)
}

/// Who may stream an answer in the room being drawn: its agent room kind and
/// its power levels as last read (`None` when unread, which trusts no one).
#[derive(Debug, Clone, Copy, Default)]
pub struct TurnTrust<'a> {
    pub kind: Option<AgentRoomKind>,
    pub levels: Option<&'a RoomPowerLevels>,
}

impl TurnTrust<'_> {
    /// Whether `sender`'s message, read by `own`, with `json` its original
    /// event, is an agent's streamed answer ([`is_trusted_turn`]).
    pub fn is_turn(&self, own: &UserId, sender: &UserId, json: Option<&str>) -> bool {
        is_trusted_turn(
            self.kind,
            sender == own,
            holds_agent_power(self.levels, sender),
            json,
        )
    }
}

/// The newest answer anchor once `seen` is read after `held`: each trusted
/// anchor's `(origin ms, render key)`, in timeline order, replaces the one
/// held unless it is older, so of two in the same millisecond the later in
/// the timeline wins. An edit keeps its original's origin time.
pub fn newest_turn<'a>(
    held: Option<(u64, String)>,
    seen: impl IntoIterator<Item = (u64, &'a str)>,
) -> Option<(u64, String)> {
    seen.into_iter().fold(held, |held, (ts, key)| match held {
        Some((held_ts, held_key)) if ts < held_ts => Some((held_ts, held_key)),
        _ => Some((ts, key.to_owned())),
    })
}

/// What the run badge says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AgentRunVm {
    Idle,
    Running,
    Blocked,
    /// Waiting for the host named in `waiting`.
    Waiting,
    Done,
    /// keeper cannot read the status; `unreadable` says why.
    Unreadable,
}

impl From<RunState> for AgentRunVm {
    fn from(run: RunState) -> AgentRunVm {
        match run {
            RunState::Idle => AgentRunVm::Idle,
            RunState::Running => AgentRunVm::Running,
            RunState::Blocked => AgentRunVm::Blocked,
            RunState::Waiting => AgentRunVm::Waiting,
            RunState::Done => AgentRunVm::Done,
        }
    }
}

/// The status line of an agent room (UX-DR129).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentStatusVm {
    /// The agent's Matrix user id.
    pub agent: String,
    /// The agent's display name in the room, else its user id.
    pub agent_name: String,
    /// `nixi@electra`: the agent's localpart and the host that answers; the
    /// localpart alone when the host could not be read.
    pub handle: String,
    /// The mark from the agent's `SOUL.md` `icon` when its agents zone is on
    /// this device (AD-155, 91.1 acceptance 6), checked by the bot identity's
    /// own bound: one short literal mark or an icon name. Absent elsewhere
    /// (the phone), where the mark is the agent's first letter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// The host that wrote the status.
    pub host: Option<String>,
    /// The session's title.
    pub title: Option<String>,
    /// The session's kind (R25).
    pub kind: Option<SessionKind>,
    pub run: AgentRunVm,
    /// The host the session waits for, with `run: waiting`.
    pub waiting: Option<String>,
    /// The host's detail, drawn as sent: counts, never paths (S-16).
    pub detail: Option<String>,
    /// Why keeper cannot read the status, as a sentence; `None` when it can.
    pub unreadable: Option<String>,
}

/// A drive in the scope chip, in the scope event's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScopeDriveVm {
    pub id: String,
    pub title: String,
}

/// An agent room's header, beside its timeline (UX-DR129).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentRoomHeaderVm {
    /// The newest status; `None` before the agent sent one.
    pub status: Option<AgentStatusVm>,
    /// The drives in scope; `None` before a scope was read ("no scope yet")
    /// or when the newest one is unreadable.
    pub scope: Option<Vec<ScopeDriveVm>>,
    /// The label chip; `None` before a scope carried one.
    pub label: Option<LabelVm>,
    /// Why keeper cannot read the newest scope, as a sentence the scope chip
    /// says in place of the drives; `None` when it can.
    pub scope_unreadable: Option<String>,
    /// The render key of the newest answer's anchor while the run is
    /// `running`: the message that draws the growing caret.
    pub caret_key: Option<String>,
}

/// The sentence for a status from a newer keeper.
pub const NEWER_STATUS: &str = "This status is from a newer keeper. Update keeper to read it.";
/// The sentence for a status keeper cannot read.
pub const UNREADABLE_STATUS: &str = "keeper cannot read this status.";
/// The sentence for a scope from a newer keeper.
pub const NEWER_SCOPE: &str = "This scope is from a newer keeper. Update keeper to read it.";
/// The sentence for a scope keeper cannot read.
pub const UNREADABLE_SCOPE: &str = "keeper cannot read this scope.";

#[derive(Debug, Clone, PartialEq, Eq)]
enum StatusRead {
    Read(StatusContent),
    Unreadable {
        agent: OwnedUserId,
        host: Option<String>,
        title: Option<String>,
        waiting: Option<String>,
        detail: Option<String>,
        sentence: &'static str,
    },
}

impl StatusRead {
    fn of(content: &Value, agent: OwnedUserId) -> StatusRead {
        match content["v"].as_u64() {
            Some(v) if v == u64::from(CONTENT_VERSION) => {
                if let Ok(read) = StatusContent::deserialize(content) {
                    return StatusRead::Read(read);
                }
                StatusRead::lenient(content, agent, UNREADABLE_STATUS)
            }
            Some(v) if v > u64::from(CONTENT_VERSION) => StatusRead::Unreadable {
                agent,
                host: None,
                title: None,
                waiting: None,
                detail: None,
                sentence: NEWER_STATUS,
            },
            _ => StatusRead::lenient(content, agent, UNREADABLE_STATUS),
        }
    }

    fn lenient(content: &Value, agent: OwnedUserId, sentence: &'static str) -> StatusRead {
        let text = |key: &str| content[key].as_str().map(str::to_owned);
        StatusRead::Unreadable {
            agent,
            host: text("host"),
            title: text("title"),
            waiting: text("waiting"),
            detail: text("detail"),
            sentence,
        }
    }
}

/// A scope as read: its drives and label, or why keeper cannot read it — a
/// scope is never dropped for its version or shape, as a status is not (R32).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScopeRead {
    Read {
        drives: Vec<ScopeDriveVm>,
        label: Option<Label>,
    },
    Unreadable(&'static str),
}

impl ScopeRead {
    fn of(content: &Value) -> ScopeRead {
        if content["v"]
            .as_u64()
            .is_some_and(|v| v > u64::from(CONTENT_VERSION))
        {
            return ScopeRead::Unreadable(NEWER_SCOPE);
        }
        match ScopeWire::deserialize(content) {
            Ok(wire) if wire.v == CONTENT_VERSION => ScopeRead::Read {
                drives: wire
                    .drives
                    .into_iter()
                    .map(|drive| ScopeDriveVm {
                        id: drive.id,
                        title: drive.title,
                    })
                    .collect(),
                label: wire.label,
            },
            _ => ScopeRead::Unreadable(UNREADABLE_SCOPE),
        }
    }
}

#[derive(Deserialize)]
struct ScopeWire {
    v: u32,
    drives: Vec<ScopeDriveWire>,
    #[serde(default)]
    label: Option<Label>,
}

#[derive(Deserialize)]
struct ScopeDriveWire {
    id: String,
    title: String,
}

/// An agent room's status and scope, folded from its events (R32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRoomState {
    own: OwnedUserId,
    status: Option<(u64, StatusRead)>,
    scope: Option<(u64, ScopeRead)>,
    /// Who sent the newest scope.
    scope_by: Option<OwnedUserId>,
}

impl AgentRoomState {
    /// An empty state for a room `own` reads.
    pub fn new(own: OwnedUserId) -> AgentRoomState {
        AgentRoomState {
            own,
            status: None,
            scope: None,
            scope_by: None,
        }
    }

    /// Fold one event (decrypted, as JSON). `trusted` answers whether a user
    /// holds an agent's power in the room. Returns whether the header changed.
    pub fn apply(&mut self, event: &Value, trusted: &dyn Fn(&UserId) -> bool) -> bool {
        let is_status = event["type"] == STATUS;
        if !is_status && event["type"] != SCOPE {
            return false;
        }
        let Some(sender) = event["sender"]
            .as_str()
            .and_then(|sender| UserId::parse(sender).ok())
        else {
            return false;
        };
        if sender == self.own || !trusted(&sender) {
            return false;
        }
        let Some(ts) = event["origin_server_ts"].as_u64() else {
            return false;
        };
        let content = &event["content"];
        if is_status {
            if content["agent"].as_str() != Some(sender.as_str()) {
                return false;
            }
            if self.status.as_ref().is_some_and(|(held, _)| *held > ts) {
                return false;
            }
            let read = StatusRead::of(content, sender);
            let changed = self.status.as_ref().map(|(_, held)| held) != Some(&read);
            self.status = Some((ts, read));
            changed
        } else {
            if self.scope.as_ref().is_some_and(|(held, _)| *held > ts) {
                return false;
            }
            let read = ScopeRead::of(content);
            let changed = self.scope.as_ref().map(|(_, held)| held) != Some(&read);
            self.scope = Some((ts, read));
            self.scope_by = Some(sender);
            changed
        }
    }

    /// Whether a status has been read.
    pub fn has_status(&self) -> bool {
        self.status.is_some()
    }

    /// The session's kind, from the newest readable status.
    pub fn kind(&self) -> Option<SessionKind> {
        match &self.status {
            Some((_, StatusRead::Read(status))) => Some(status.kind),
            _ => None,
        }
    }

    /// Whether the newest status says `running`.
    pub fn running(&self) -> bool {
        matches!(&self.status, Some((_, StatusRead::Read(s))) if s.run == RunState::Running)
    }

    /// The agent the newest readable status names.
    pub fn agent(&self) -> Option<&UserId> {
        match &self.status {
            Some((_, StatusRead::Read(status))) => Some(&status.agent),
            _ => None,
        }
    }

    /// The title the newest readable status says.
    pub fn title(&self) -> Option<&str> {
        match &self.status {
            Some((_, StatusRead::Read(status))) => Some(&status.title),
            _ => None,
        }
    }

    /// The drives `agent`'s own newest scope names (its host's echo);
    /// `None` when the newest scope is someone else's or unreadable, or none
    /// was read — the device then knows nothing of the agent's drives.
    pub fn scope_drives_of(&self, agent: &UserId) -> Option<Vec<String>> {
        match (&self.scope, &self.scope_by) {
            (Some((_, ScopeRead::Read { drives, .. })), Some(by)) if by == agent => {
                Some(drives.iter().map(|drive| drive.id.clone()).collect())
            }
            _ => None,
        }
    }

    /// The users the header names: the agent and the label's readers, for
    /// the caller to resolve to display names.
    pub fn named(&self) -> Vec<OwnedUserId> {
        let mut users = Vec::new();
        match &self.status {
            Some((_, StatusRead::Read(status))) => users.push(status.agent.clone()),
            Some((_, StatusRead::Unreadable { agent, .. })) => users.push(agent.clone()),
            None => {}
        }
        if let Some((
            _,
            ScopeRead::Read {
                label:
                    Some(Label {
                        readers: crate::agents::label::Readers::Only(set),
                        ..
                    }),
                ..
            },
        )) = &self.scope
        {
            users.extend(set.iter().cloned());
        }
        users
    }

    /// The header, naming users as `name` does, marking the agent with its
    /// soul's icon from `icons`, and drawing the caret on `newest_turn` while
    /// the run is `running`.
    pub fn header(
        &self,
        name: &dyn Fn(&UserId) -> String,
        icons: &AgentIcons,
        newest_turn: Option<&str>,
    ) -> AgentRoomHeaderVm {
        let status = self
            .status
            .as_ref()
            .map(|(_, read)| status_vm(read, name, icons));
        let (scope, label, scope_unreadable) = match &self.scope {
            Some((_, ScopeRead::Read { drives, label })) => (
                Some(drives.clone()),
                label.as_ref().map(|label| LabelVm::compose(label, name)),
                None,
            ),
            Some((_, ScopeRead::Unreadable(sentence))) => {
                (None, None, Some((*sentence).to_owned()))
            }
            None => (None, None, None),
        };
        AgentRoomHeaderVm {
            status,
            scope,
            label,
            scope_unreadable,
            caret_key: newest_turn.filter(|_| self.running()).map(str::to_owned),
        }
    }
}

fn handle(agent: &UserId, host: Option<&str>) -> String {
    match host {
        Some(host) => format!("{}@{host}", agent.localpart()),
        None => agent.localpart().to_owned(),
    }
}

fn status_vm(
    read: &StatusRead,
    name: &dyn Fn(&UserId) -> String,
    icons: &AgentIcons,
) -> AgentStatusVm {
    match read {
        StatusRead::Read(status) => AgentStatusVm {
            agent: status.agent.to_string(),
            agent_name: name(&status.agent),
            handle: handle(&status.agent, Some(&status.host)),
            icon: icons.get(&status.agent),
            host: Some(status.host.clone()),
            title: Some(status.title.clone()),
            kind: Some(status.kind),
            run: status.run.into(),
            waiting: status.waiting.clone(),
            detail: status.detail.clone(),
            unreadable: None,
        },
        StatusRead::Unreadable {
            agent,
            host,
            title,
            waiting,
            detail,
            sentence,
        } => AgentStatusVm {
            agent: agent.to_string(),
            agent_name: name(agent),
            handle: handle(agent, host.as_deref()),
            icon: icons.get(agent),
            host: host.clone(),
            title: title.clone(),
            kind: None,
            run: AgentRunVm::Unreadable,
            waiting: waiting.clone(),
            detail: detail.clone(),
            unreadable: Some((*sentence).to_owned()),
        },
    }
}

/// The soul's mark of every agent whose agents zone is on this device, by
/// Matrix user: the desktop's host replaces it on each scan; on the phone it
/// stays empty.
#[derive(Debug, Default)]
pub struct AgentIcons {
    marks: RwLock<BTreeMap<OwnedUserId, String>>,
    /// Bumped when the marks change, so an open room's header redraws
    /// without waiting for its next status.
    changes: watch::Sender<u64>,
}

impl AgentIcons {
    /// Replace every mark with `icons` (each a `SOUL.md` `icon`, as read),
    /// keeping only those the bot identity draws: a mark it would refuse is
    /// dropped, and that agent is drawn by its first letter.
    pub fn replace(&self, icons: BTreeMap<OwnedUserId, String>) {
        let drawable: BTreeMap<OwnedUserId, String> = icons
            .into_iter()
            .filter_map(|(user, icon)| {
                let mark = parse_identity(None, None, Some(&icon)).ok()?.mark?;
                Some((user, mark))
            })
            .collect();
        {
            let mut marks = self
                .marks
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *marks == drawable {
                return;
            }
            *marks = drawable;
        }
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// `user`'s mark, when its zone is on this device.
    pub fn get(&self, user: &UserId) -> Option<String> {
        self.marks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(user)
            .cloned()
    }

    /// A receiver that sees every change of the marks after now.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}

/// The session kind of every agent room an account has read a status in.
/// A session's kind never changes, so once read it is kept for the
/// process's life; the room list reads it here before the event cache.
#[derive(Debug, Default)]
pub struct AgentKinds {
    kinds: Mutex<HashMap<OwnedRoomId, SessionKind>>,
    /// Session rooms whose history was searched back for a status and held
    /// none: not searched again in this process, since a status sent later
    /// reaches the event cache.
    searched: Mutex<HashSet<OwnedRoomId>>,
}

impl AgentKinds {
    pub fn get(&self, room: &RoomId) -> Option<SessionKind> {
        self.kinds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(room)
            .copied()
    }

    pub fn insert(&self, room: &RoomId, kind: SessionKind) {
        self.kinds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(room.to_owned(), kind);
    }

    /// Whether `room`'s history was already searched and held no status.
    pub fn searched_empty(&self, room: &RoomId) -> bool {
        self.searched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(room)
    }

    /// Note that `room`'s history was searched and held no status.
    pub fn note_searched_empty(&self, room: &RoomId) {
        self.searched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(room.to_owned());
    }
}

/// Whether `user` holds an agent's power under `levels`: what a host asks
/// of an agent's event, a brief included (R53).
pub fn holds_agent_power(levels: Option<&RoomPowerLevels>, user: &UserId) -> bool {
    levels.is_some_and(|levels| levels.for_user(user) >= Int::from(AGENT_POWER as i32))
}

/// An event's `type`, read without parsing the rest of it.
fn event_type(raw: &Raw<AnySyncTimelineEvent>) -> Option<&str> {
    raw.get_field::<&str>("type").ok().flatten()
}

/// The event in `raw` as JSON when it is a status or a scope; no other event
/// is parsed.
fn header_event(raw: &Raw<AnySyncTimelineEvent>) -> Option<Value> {
    match event_type(raw) {
        Some(STATUS | SCOPE) => raw.deserialize_as::<Value>().ok(),
        _ => None,
    }
}

/// The room-list row's agent room: its create type, and for a session room
/// the session kind from `kinds` or else from the newest readable status in
/// the event cache's loaded events.
pub async fn room_kind_vm(room: &Room, kinds: &AgentKinds) -> Option<AgentRoomKindVm> {
    let kind = AgentRoomKind::of(room.room_type().as_ref())?;
    if kind == AgentRoomKind::Control {
        return Some(AgentRoomKindVm::Control);
    }
    if let Some(session) = kinds.get(room.room_id()) {
        return Some(AgentRoomKindVm::of(kind, Some(session)));
    }
    let levels = room.power_levels().await.ok();
    let trusted = |user: &UserId| holds_agent_power(levels.as_ref(), user);
    let mut state = AgentRoomState::new(room.own_user_id().to_owned());
    let session = match room.event_cache().await {
        Ok((cache, _handles)) => cache
            .rfind_map_event_in_memory_by(|event| {
                let value = header_event(event.raw())?;
                // Each status is read on its own, newest first: the newest
                // readable one names the kind.
                state.status = None;
                state.apply(&value, &trusted);
                state.kind()
            })
            .await
            .ok()
            .flatten(),
        Err(_) => None,
    };
    if let Some(session) = session {
        kinds.insert(room.room_id(), session);
    }
    Some(AgentRoomKindVm::of(kind, session))
}

/// A session room's header reader: the folded state and the room it reads.
pub struct HeaderReader {
    room: Room,
    state: AgentRoomState,
    levels: Option<RoomPowerLevels>,
    /// The display names the header resolved, until a member event.
    names: HashMap<OwnedUserId, String>,
}

impl HeaderReader {
    /// Read `room`'s status and scope from its event cache, and when the
    /// cache holds no status, from the server's history newest page first,
    /// as the host's `latest_status` does — once per process for a room
    /// whose history held none.
    pub async fn open(room: Room, kinds: &AgentKinds) -> HeaderReader {
        let mut reader = HeaderReader {
            state: AgentRoomState::new(room.own_user_id().to_owned()),
            levels: room.power_levels().await.ok(),
            room,
            names: HashMap::new(),
        };
        if let Ok((cache, _handles)) = reader.room.event_cache().await {
            if let Ok(events) = cache.events().await {
                for event in &events {
                    if let Some(value) = header_event(event.raw()) {
                        reader.apply(&value);
                    }
                }
            }
        }
        if !reader.state.has_status()
            && !kinds.searched_empty(reader.room.room_id())
            && reader.page_back().await
        {
            kinds.note_searched_empty(reader.room.room_id());
        }
        reader.remember(kinds);
        reader
    }

    /// Search the server's history back for a status; returns whether the
    /// search ran to its end and found none (a failed request is not that).
    async fn page_back(&mut self) -> bool {
        use matrix_sdk::room::MessagesOptions;
        let mut from: Option<String> = None;
        for _ in 0..HEADER_PAGES {
            let mut options = MessagesOptions::backward().from(from.as_deref());
            options.limit = HEADER_PAGE.into();
            let Ok(page) = self.room.messages(options).await else {
                return false;
            };
            let mut events: Vec<Value> = page
                .chunk
                .iter()
                .filter_map(|event| header_event(event.raw()))
                .collect();
            events.reverse();
            for event in &events {
                self.apply(event);
            }
            if self.state.has_status() {
                return false;
            }
            match page.end {
                Some(end) if !page.chunk.is_empty() => from = Some(end),
                _ => return true,
            }
        }
        true
    }

    fn apply(&mut self, event: &Value) -> bool {
        let levels = self.levels.as_ref();
        self.state
            .apply(event, &|user| holds_agent_power(levels, user))
    }

    fn remember(&self, kinds: &AgentKinds) {
        if let Some(kind) = self.state.kind() {
            kinds.insert(self.room.room_id(), kind);
        }
    }

    /// The room's power levels as last read; `None` when unread, which
    /// trusts no one.
    pub fn levels(&self) -> Option<&RoomPowerLevels> {
        self.levels.as_ref()
    }

    /// The room's folded status and scope.
    pub fn state(&self) -> &AgentRoomState {
        &self.state
    }

    /// Fold one event-cache update; returns whether the header may have
    /// changed.
    pub async fn update(&mut self, update: &RoomEventCacheUpdate, kinds: &AgentKinds) -> bool {
        let RoomEventCacheUpdate::UpdateTimelineEvents(diffs) = update else {
            return false;
        };
        self.levels = self.room.power_levels().await.ok();
        let mut changed = false;
        for diff in &diffs.diffs {
            let events: Vec<&matrix_sdk::deserialized_responses::TimelineEvent> = match diff {
                VectorDiff::Append { values } | VectorDiff::Reset { values } => {
                    values.iter().collect()
                }
                VectorDiff::PushFront { value }
                | VectorDiff::PushBack { value }
                | VectorDiff::Insert { value, .. }
                | VectorDiff::Set { value, .. } => vec![value],
                _ => Vec::new(),
            };
            for event in events {
                match event_type(event.raw()) {
                    Some(STATUS | SCOPE) => {
                        if let Ok(value) = event.raw().deserialize_as::<Value>() {
                            changed |= self.apply(&value);
                        }
                    }
                    // A member event may rename the agent or a reader.
                    Some("m.room.member") => {
                        self.names.clear();
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        self.remember(kinds);
        changed
    }

    /// The header now, with `newest_turn` as the caret's candidate and the
    /// agent's mark from `icons`.
    pub async fn header(
        &mut self,
        icons: &AgentIcons,
        newest_turn: Option<&str>,
    ) -> AgentRoomHeaderVm {
        for user in self.state.named() {
            if self.names.contains_key(&user) {
                continue;
            }
            let name = self
                .room
                .get_member_no_sync(&user)
                .await
                .ok()
                .flatten()
                .and_then(|member| member.display_name().map(str::to_owned))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| user.to_string());
            self.names.insert(user, name);
        }
        let names = &self.names;
        let name = |user: &UserId| names.get(user).cloned().unwrap_or_else(|| user.to_string());
        self.state.header(&name, icons, newest_turn)
    }
}

/// Power levels holding `users` at their levels, everyone else at 0.
#[cfg(test)]
pub(crate) fn power_levels_for(users: &[(&str, i32)]) -> RoomPowerLevels {
    use matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsEventContent;
    use matrix_sdk::ruma::room_version_rules::AuthorizationRules;
    let mut content = RoomPowerLevelsEventContent::new(&AuthorizationRules::V1);
    for (user, level) in users {
        content.users.insert(
            OwnedUserId::try_from(*user).expect("user"),
            Int::from(*level),
        );
    }
    RoomPowerLevels::new(
        content.into(),
        &AuthorizationRules::V1,
        Vec::<OwnedUserId>::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use serde_json::json;

    const NIXI: &str = "@nixi:example.org";
    const PERSON: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";

    fn own() -> OwnedUserId {
        OwnedUserId::try_from(PERSON).expect("user")
    }

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn agents(user: &UserId) -> bool {
        user.as_str() == NIXI || user.as_str() == "@tola:example.org"
    }

    fn status(id: &str, ts: u64, content: Value) -> Value {
        json!({
            "type": STATUS, "event_id": id, "sender": NIXI, "origin_server_ts": ts,
            "content": content,
        })
    }

    fn content(run: &str, host: &str, anchor: Option<&str>) -> Value {
        let mut content = json!({
            "v": 1, "session": "sessions/nixi/main", "kind": "main", "title": "Nixi",
            "agent": NIXI, "host": host, "epoch": 1, "run": run,
        });
        if let Some(anchor) = anchor {
            content["anchor"] = json!(anchor);
        }
        content
    }

    /// A surface request is acted on only over the drives the room's proxy
    /// declares; on a phone those are the proxy's own newest scope echo —
    /// never a scope another agent at 50 in the room sent — and none before
    /// one is read, which the device then cannot check.
    #[test]
    fn the_drives_a_proxy_declares_are_its_own_scopes() {
        let nixi = user(NIXI);
        let tola = user("@tola:example.org");
        let mut state = AgentRoomState::new(own());
        assert_eq!(state.scope_drives_of(&nixi), None, "nothing read yet");
        let scope = |sender: &str, ts: u64, drives: Value| {
            json!({
                "type": SCOPE, "event_id": format!("$s{ts}"), "sender": sender,
                "origin_server_ts": ts,
                "content": {"v": 1, "drives": drives, "set_by": PERSON},
            })
        };
        assert!(state.apply(
            &scope(NIXI, 10, json!([{"id": "tgdrive", "title": "tgdrive"}])),
            &agents
        ));
        assert_eq!(
            state.scope_drives_of(&nixi),
            Some(vec!["tgdrive".to_owned()])
        );
        // Tola, another agent of the principal, sends a newer scope: it is
        // the newest, and it is not Nixi's word.
        assert!(state.apply(
            &scope(
                "@tola:example.org",
                20,
                json!([{"id": "marta-diary", "title": "Diary"}])
            ),
            &agents
        ));
        assert_eq!(state.scope_drives_of(&nixi), None);
        assert_eq!(
            state.scope_drives_of(&tola),
            Some(vec!["marta-diary".to_owned()])
        );
    }

    fn header(state: &AgentRoomState) -> AgentRoomHeaderVm {
        let name = |user: &UserId| match user.as_str() {
            NIXI => "Nixi".to_owned(),
            MARTA => "Marta".to_owned(),
            other => other.to_owned(),
        };
        state.header(&name, &AgentIcons::default(), Some("turn-key"))
    }

    fn status_of(state: &AgentRoomState) -> AgentStatusVm {
        header(state).status.expect("a status")
    }

    fn event(json: Value) -> AnySyncTimelineEvent {
        Raw::<AnySyncTimelineEvent>::from_json_string(json.to_string())
            .expect("raw")
            .deserialize()
            .expect("an event")
    }

    /// A message, an edit, a reaction, a redaction, a custom message-like
    /// type, the agents' status and scope, their claim and host state and an
    /// ordinary state event, built as JSON (keeper-core has no
    /// `matrix-sdk-test`).
    fn fixtures() -> Vec<(&'static str, AnySyncTimelineEvent)> {
        let base = |kind: &str, content: Value| {
            json!({
                "type": kind, "event_id": format!("${kind}"), "sender": NIXI,
                "origin_server_ts": 1, "content": content,
            })
        };
        let state = |kind: &str, key: &str, content: Value| {
            let mut event = base(kind, content);
            event["state_key"] = json!(key);
            event
        };
        vec![
            (
                "message",
                event(base(
                    "m.room.message",
                    json!({"msgtype": "m.text", "body": "hi"}),
                )),
            ),
            (
                "edit",
                event(base(
                    "m.room.message",
                    json!({
                        "msgtype": "m.text", "body": "* hi",
                        "m.new_content": {"msgtype": "m.text", "body": "hi"},
                        "m.relates_to": {"rel_type": "m.replace", "event_id": "$a"},
                    }),
                )),
            ),
            (
                "reaction",
                event(base(
                    "m.reaction",
                    json!({"m.relates_to": {"rel_type": "m.annotation", "event_id": "$a", "key": "👍"}}),
                )),
            ),
            (
                "redaction",
                event(base("m.room.redaction", json!({"redacts": "$a"}))),
            ),
            ("custom", event(base("org.example.custom", json!({"x": 1})))),
            (
                "status",
                event(base(STATUS, content("idle", "electra", None))),
            ),
            (
                "scope",
                event(base(SCOPE, json!({"v": 1, "drives": [], "set_by": NIXI}))),
            ),
            (
                "surface.result",
                event(base("dev.keeper.agent.surface.result", json!({"v": 1}))),
            ),
            (
                "claim",
                event(state(CLAIM, "", json!({"v": 1, "epoch": 1}))),
            ),
            ("host", event(state(HOST, "electra", json!({"v": 1})))),
            (
                "topic",
                event(state("m.room.topic", "", json!({"topic": "t"}))),
            ),
        ]
    }

    #[test]
    fn a_room_kind_comes_from_the_create_type_only() {
        let typed = |name: &str| Some(RoomType::from(name));
        assert_eq!(
            AgentRoomKind::of(typed(SESSION_ROOM_TYPE).as_ref()),
            Some(AgentRoomKind::Session)
        );
        assert_eq!(
            AgentRoomKind::of(typed(CONTROL_ROOM_TYPE).as_ref()),
            Some(AgentRoomKind::Control)
        );
        assert_eq!(AgentRoomKind::of(Some(&RoomType::Space)), None);
        assert_eq!(AgentRoomKind::of(typed("dev.keeper.agent").as_ref()), None);
        assert_eq!(AgentRoomKind::of(None), None);
    }

    #[test]
    fn ordinary_rooms_filter_exactly_as_before() {
        let rules = RoomVersionRules::V11;
        let filter = agent_event_filter(None);
        for (name, event) in fixtures() {
            assert_eq!(
                filter(&event, &rules),
                default_event_filter(&event, &rules),
                "{name}"
            );
        }
    }

    #[test]
    fn agent_rooms_drop_claim_and_host_state_and_admit_no_agent_event() {
        let rules = RoomVersionRules::V11;
        for kind in [AgentRoomKind::Session, AgentRoomKind::Control] {
            let filter = agent_event_filter(Some(kind));
            for (name, event) in fixtures() {
                let expected = match name {
                    "claim" | "host" => false,
                    _ => default_event_filter(&event, &rules),
                };
                assert_eq!(filter(&event, &rules), expected, "{kind:?} {name}");
            }
            // Status, scope and surface results are read beside the stream,
            // never drawn as items (R33).
            for (name, event) in fixtures() {
                if matches!(
                    name,
                    "status" | "scope" | "surface.result" | "claim" | "host"
                ) {
                    assert!(!filter(&event, &rules), "{kind:?} {name}");
                }
            }
        }
    }

    #[test]
    fn a_status_anchor_and_its_updates_are_one_line() {
        let mut state = AgentRoomState::new(own());
        assert!(state.apply(&status("$a", 10, content("idle", "electra", None)), &agents));
        assert!(state.apply(
            &status("$b", 20, content("running", "electra", Some("$a"))),
            &agents
        ));
        assert!(state.apply(
            &status("$c", 30, content("blocked", "hesperia", Some("$a"))),
            &agents
        ));
        let mut last = content("done", "hesperia", Some("$a"));
        last["detail"] = json!("3 files read");
        last["kind"] = json!("conversation");
        assert!(state.apply(&status("$d", 40, last), &agents));
        let line = status_of(&state);
        assert_eq!(line.run, AgentRunVm::Done);
        assert_eq!(line.host.as_deref(), Some("hesperia"));
        assert_eq!(line.handle, "nixi@hesperia");
        assert_eq!(line.agent_name, "Nixi");
        assert_eq!(line.detail.as_deref(), Some("3 files read"));
        assert_eq!(line.kind, Some(SessionKind::Conversation));
        assert_eq!(line.unreadable, None);

        // A stale update arriving late does not replace a newer one.
        assert!(!state.apply(
            &status("$e", 35, content("running", "electra", Some("$a"))),
            &agents
        ));
        assert_eq!(status_of(&state).run, AgentRunVm::Done);
        // An update naming another anchor is read like any other: the newest
        // status in the room wins, as the host's `trail_of` reads it.
        assert!(state.apply(
            &status("$f", 50, content("idle", "electra", Some("$z"))),
            &agents
        ));
        assert_eq!(status_of(&state).run, AgentRunVm::Idle);
    }

    #[test]
    fn a_status_from_anyone_but_its_agent_is_ignored() {
        let mut state = AgentRoomState::new(own());
        // The person, even naming the agent.
        let mut forged = status("$p", 10, content("running", "electra", None));
        forged["sender"] = json!(PERSON);
        assert!(!state.apply(&forged, &|_| true));
        // A user without an agent's power.
        let mut weak = status("$w", 10, content("running", "electra", None));
        weak["sender"] = json!("@mallory:example.org");
        weak["content"]["agent"] = json!("@mallory:example.org");
        assert!(!state.apply(&weak, &agents));
        // Another agent sending a status that names Nixi.
        let mut other = status("$o", 10, content("running", "electra", None));
        other["sender"] = json!("@tola:example.org");
        assert!(!state.apply(&other, &agents));
        assert!(header(&state).status.is_none());
    }

    #[test]
    fn waiting_shows_the_host_it_waits_for() {
        let mut state = AgentRoomState::new(own());
        let mut waiting = content("waiting", "electra", None);
        waiting["waiting"] = json!("hesperia");
        state.apply(&status("$a", 10, waiting), &agents);
        let line = status_of(&state);
        assert_eq!(line.run, AgentRunVm::Waiting);
        assert_eq!(line.waiting.as_deref(), Some("hesperia"));
    }

    #[test]
    fn an_unknown_run_kind_or_version_is_unreadable_never_dropped() {
        for (field, value, sentence) in [
            ("run", json!("wat"), UNREADABLE_STATUS),
            ("kind", json!("party"), UNREADABLE_STATUS),
            ("v", json!(2), NEWER_STATUS),
        ] {
            let mut state = AgentRoomState::new(own());
            state.apply(&status("$a", 10, content("idle", "electra", None)), &agents);
            let mut odd = content("running", "electra", Some("$a"));
            odd[field] = value;
            assert!(state.apply(&status("$b", 20, odd), &agents), "{field}");
            let line = status_of(&state);
            assert_eq!(line.run, AgentRunVm::Unreadable, "{field}");
            assert_eq!(line.kind, None, "{field}");
            assert_eq!(line.unreadable.as_deref(), Some(sentence), "{field}");
            assert_eq!(line.agent, NIXI, "{field}");
            assert_eq!(state.kind(), None, "{field}");
        }
        // An unreadable run still names the host it came from.
        let mut state = AgentRoomState::new(own());
        state.apply(&status("$a", 10, content("wat", "electra", None)), &agents);
        assert_eq!(status_of(&state).handle, "nixi@electra");
    }

    #[test]
    fn the_scope_gives_the_drive_chips_in_order_and_the_label_chip() {
        let mut state = AgentRoomState::new(own());
        assert_eq!(header(&state).scope, None);
        let scope = json!({
            "type": SCOPE, "event_id": "$s", "sender": NIXI, "origin_server_ts": 10,
            "content": {
                "v": 1, "set_by": PERSON,
                "drives": [{"id": "tgdrive", "title": "tgdrive"}, {"id": "neura", "title": "Neura"}],
                "label": {"readers": [MARTA, "@ghost:example.org"], "integrity": "peer"},
            },
        });
        assert!(state.apply(&scope, &agents));
        let header = header(&state);
        let ids: Vec<&str> = header
            .scope
            .as_ref()
            .expect("scope")
            .iter()
            .map(|drive| drive.id.as_str())
            .collect();
        assert_eq!(ids, ["tgdrive", "neura"]);
        let label = header.label.expect("label");
        // BTreeSet order: the unknown reader is shown by its id, never hidden.
        assert_eq!(label.readers, ["@ghost:example.org", "Marta"]);
        assert_eq!(label.integrity, "peer");
        let named: BTreeSet<String> = state.named().iter().map(|u| u.to_string()).collect();
        assert!(named.contains(MARTA) && named.contains("@ghost:example.org"));

        // The person's own scope is not the agent's (R30 note).
        let mut person = scope.clone();
        person["sender"] = json!(PERSON);
        person["origin_server_ts"] = json!(20);
        person["content"]["drives"] = json!([]);
        assert!(!state.apply(&person, &|_| true));
    }

    #[test]
    fn the_caret_is_on_the_newest_turn_only_while_running() {
        let mut state = AgentRoomState::new(own());
        state.apply(&status("$a", 10, content("idle", "electra", None)), &agents);
        assert_eq!(header(&state).caret_key, None);
        state.apply(
            &status("$b", 20, content("running", "electra", Some("$a"))),
            &agents,
        );
        assert_eq!(header(&state).caret_key.as_deref(), Some("turn-key"));
        state.apply(
            &status("$c", 30, content("done", "electra", Some("$a"))),
            &agents,
        );
        assert_eq!(header(&state).caret_key, None);
    }

    #[test]
    fn the_newest_answer_anchor_holds_the_caret() {
        // A later origin wins; an older anchor seen after it does not.
        let held = newest_turn(None, [(10, "a"), (30, "b"), (20, "c")]);
        assert_eq!(held, Some((30, "b".to_owned())));
        // Of two in the same millisecond, the later in the timeline wins.
        assert_eq!(
            newest_turn(held.clone(), [(30, "d")]),
            Some((30, "d".to_owned()))
        );
        // An edit re-sends an older anchor with its original's time: the
        // caret stays where it is.
        assert_eq!(newest_turn(held.clone(), [(10, "a")]), held);
    }

    #[test]
    fn a_turn_marker_counts_only_from_an_agent_in_a_session_room() {
        let turn = json!({
            "type": "m.room.message", "sender": NIXI,
            "content": {"msgtype": "m.text", "body": "…", TURN: {"session": "s", "line": "l"}},
        })
        .to_string();
        let plain =
            json!({"type": "m.room.message", "content": {"msgtype": "m.text", "body": "hi"}})
                .to_string();
        let levels = power_levels_for(&[(NIXI, 50), (PERSON, 100)]);
        let session = TurnTrust {
            kind: Some(AgentRoomKind::Session),
            levels: Some(&levels),
        };
        let own = own();
        let nixi = user(NIXI);
        // The agent's anchor in its session room: an answer.
        assert!(session.is_turn(&own, &nixi, Some(&turn)));
        // A person at 0 writing the marker: an ordinary message.
        assert!(!session.is_turn(&own, &user(MARTA), Some(&turn)));
        // The own user, even at 100.
        assert!(!session.is_turn(&own, &own, Some(&turn)));
        // The agent without the marker, or with no original (a local echo).
        assert!(!session.is_turn(&own, &nixi, Some(&plain)));
        assert!(!session.is_turn(&own, &nixi, None));
        // Unread power levels trust no one.
        let unread = TurnTrust {
            kind: Some(AgentRoomKind::Session),
            levels: None,
        };
        assert!(!unread.is_turn(&own, &nixi, Some(&turn)));
        // A room with no create type, and a control room, hold no answers.
        for kind in [None, Some(AgentRoomKind::Control)] {
            let trust = TurnTrust {
                kind,
                levels: Some(&levels),
            };
            assert!(!trust.is_turn(&own, &nixi, Some(&turn)), "{kind:?}");
        }
    }

    #[test]
    fn a_scope_keeper_cannot_read_says_so_never_dropped() {
        let scope = |ts: u64, content: Value| {
            json!({
                "type": SCOPE, "event_id": format!("$s{ts}"), "sender": NIXI,
                "origin_server_ts": ts, "content": content,
            })
        };
        let readable = json!({
            "v": 1, "set_by": NIXI, "drives": [{"id": "tgdrive", "title": "tgdrive"}],
            "label": {"readers": [MARTA], "integrity": "peer"},
        });
        let mut state = AgentRoomState::new(own());
        for (ts, content, sentence) in [
            (20, json!({"v": 2, "drives": []}), NEWER_SCOPE),
            (40, json!({"v": 1, "drives": "tgdrive"}), UNREADABLE_SCOPE),
            (60, json!({"v": 0, "drives": []}), UNREADABLE_SCOPE),
        ] {
            assert!(
                state.apply(&scope(ts - 10, readable.clone()), &agents),
                "{ts}"
            );
            assert!(state.apply(&scope(ts, content), &agents), "{ts}");
            let header = header(&state);
            assert_eq!(header.scope, None, "{ts}");
            assert_eq!(header.label, None, "{ts}");
            assert_eq!(header.scope_unreadable.as_deref(), Some(sentence), "{ts}");
        }
        // A readable scope after it is read again.
        assert!(state.apply(&scope(70, readable), &agents));
        let header = header(&state);
        assert_eq!(header.scope_unreadable, None);
        assert_eq!(header.scope.map(|drives| drives.len()), Some(1));
    }

    #[test]
    fn a_change_of_marks_redraws_and_a_rescan_without_one_does_not() {
        let icons = AgentIcons::default();
        let mut changes = icons.subscribe();
        let marks = |icon: &str| BTreeMap::from([(user(NIXI), icon.to_owned())]);
        icons.replace(marks("N"));
        assert!(changes.has_changed().expect("open"));
        changes.borrow_and_update();
        // The next scan finds the same marks: nothing to redraw.
        icons.replace(marks("N"));
        assert!(!changes.has_changed().expect("open"));
        // A refused mark is no mark: a change from `N`, then none from none.
        icons.replace(marks("Nixie"));
        assert!(changes.has_changed().expect("open"));
        changes.borrow_and_update();
        icons.replace(BTreeMap::new());
        assert!(!changes.has_changed().expect("open"));
    }

    #[test]
    fn a_history_without_a_status_is_searched_once_per_room() {
        let kinds = AgentKinds::default();
        let quiet = OwnedRoomId::try_from("!quiet:example.org").expect("room");
        let other = OwnedRoomId::try_from("!other:example.org").expect("room");
        assert!(!kinds.searched_empty(&quiet));
        kinds.note_searched_empty(&quiet);
        assert!(kinds.searched_empty(&quiet));
        assert!(!kinds.searched_empty(&other));
    }

    #[test]
    fn the_agent_is_marked_with_its_souls_icon_where_its_zone_is() {
        let mut state = AgentRoomState::new(own());
        state.apply(&status("$a", 10, content("idle", "electra", None)), &agents);
        let icons = AgentIcons::default();
        let icon_of = |state: &AgentRoomState, icons: &AgentIcons| {
            state
                .header(&|user| user.to_string(), icons, None)
                .status
                .expect("a status")
                .icon
        };
        // The phone: no zone, no mark — the UI draws the first letter.
        assert_eq!(icon_of(&state, &icons), None);
        let nixi = OwnedUserId::try_from(NIXI).expect("user");
        let tola = OwnedUserId::try_from("@tola:example.org").expect("user");
        icons.replace(BTreeMap::from([
            (nixi.clone(), "N".to_owned()),
            (tola, "🜂".to_owned()),
        ]));
        assert_eq!(icon_of(&state, &icons).as_deref(), Some("N"));
        // An icon name is a mark too.
        icons.replace(BTreeMap::from([(nixi.clone(), "sparkles".to_owned())]));
        assert_eq!(icon_of(&state, &icons).as_deref(), Some("sparkles"));
        // A mark the bot identity refuses — too long, or not drawable — is
        // dropped, never cut or shown as text.
        for refused in ["Nixie", "N x", "  "] {
            icons.replace(BTreeMap::from([(nixi.clone(), refused.to_owned())]));
            assert_eq!(icon_of(&state, &icons), None, "{refused:?}");
        }
        // Another agent's mark is never this one's.
        icons.replace(BTreeMap::from([(
            OwnedUserId::try_from("@tola:example.org").expect("user"),
            "T".to_owned(),
        )]));
        assert_eq!(icon_of(&state, &icons), None);
        // An unreadable status still names its agent's mark.
        icons.replace(BTreeMap::from([(nixi, "N".to_owned())]));
        let mut odd = AgentRoomState::new(own());
        odd.apply(&status("$b", 10, content("wat", "electra", None)), &agents);
        assert_eq!(icon_of(&odd, &icons).as_deref(), Some("N"));
    }

    #[test]
    fn a_proxy_row_needs_a_read_main_or_conversation_kind() {
        let session = AgentRoomKind::Session;
        assert_eq!(
            AgentRoomKindVm::of(session, Some(SessionKind::Main)),
            AgentRoomKindVm::Proxy
        );
        assert_eq!(
            AgentRoomKindVm::of(session, Some(SessionKind::Conversation)),
            AgentRoomKindVm::Proxy
        );
        for kind in [
            SessionKind::Delegated,
            SessionKind::Scheduled,
            SessionKind::Workflow,
            SessionKind::Gate,
        ] {
            assert_eq!(
                AgentRoomKindVm::of(session, Some(kind)),
                AgentRoomKindVm::Session
            );
        }
        assert_eq!(AgentRoomKindVm::of(session, None), AgentRoomKindVm::Unknown);
        assert_eq!(
            AgentRoomKindVm::of(AgentRoomKind::Control, Some(SessionKind::Main)),
            AgentRoomKindVm::Control
        );
    }

    #[test]
    fn a_turn_marker_is_read_from_the_content_only() {
        let anchor = json!({
            "type": "m.room.message", "sender": NIXI,
            "content": {"msgtype": "m.text", "body": "…", TURN: {"session": "s", "line": "l"}},
        });
        assert!(is_agent_turn(&anchor.to_string()));
        let plain =
            json!({"type": "m.room.message", "content": {"msgtype": "m.text", "body": "hi"}});
        assert!(!is_agent_turn(&plain.to_string()));
        let outside = json!({"type": "m.room.message", TURN: {}, "content": {"body": "hi"}});
        assert!(!is_agent_turn(&outside.to_string()));
    }
}
