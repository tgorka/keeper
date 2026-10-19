//! The Agents list's states, read off the list's path (story 102.5, R468;
//! DW-1496 closed).
//!
//! Before this story every agent row's state was read inside the room-list
//! diff, one row after another, before anything was drawn; each reading
//! read every session room again for the questions answered in the row's
//! room, and a room with no status in reach searched the server's history.
//! A claim renewal re-ran it for every row showing questions: O(N²) reads
//! per renewal, and the launch's readiness waited behind them.
//!
//! Now:
//! - a row is drawn from what the [`AttentionBoard`] says of its room — the
//!   last reading, the state kept from the last run, or *Unknown* for a room
//!   not read yet (R436) — and never waits;
//! - one worker per account ([`run_attention_worker`]) reads the rooms that
//!   changed, each **once** per pass ([`agent_attention::read`]: its own
//!   [`Facts`](agent_attention::Facts), the questions folded from it per
//!   proxy, the person's messages in it), keeps each reading, and places the
//!   questions of every row from the kept readings in memory, by 101.3's one
//!   projection, built once per pass ([`place_all`]: `waiting::fold`,
//!   `waiting::answered_in`, `waiting::answer_state` — R453, no second copy;
//!   proxies × rooms visits, R501). A claim renewal reads the one room it
//!   landed in;
//! - nothing on a listing path asks the server: a row is read from the event
//!   cache and the seat kept for it ([`HeaderReader::open_row`]), and a room
//!   whose status is out of the cache's reach is looked for by one finder per
//!   account ([`run_seat_finder`]) that goes further back at each look, with
//!   backoff ([`finder_backoff`]) — R416's "look again later", bounded per
//!   room per hour;
//! - the worker and the finder live with the account, from its activation
//!   to its teardown ([`AgentLists`], R501): the Agents list's producers
//!   attach to them while the Chats view is open, and the notes dock and the
//!   launch read what they wrote whatever view is open.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use matrix_sdk::{Client, Room};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::Instrument;

use super::{known_proxies, proxy_row, MessageKind, ASKS_SETTLE};
use crate::agents::attention::{self as agent_attention, AgentAttentionVm, Asks, RowAttention};
use crate::agents::list_rows::{Keep, ListRowStore, RowWriter};
use crate::agents::proxy::{self, AgentProxies, KnownProxies, ProxyRoomRow};
use crate::agents::room::{holds_agent_power, AgentKinds, AgentRoomKind, Found, HeaderReader};
use crate::agents::session::SessionKind;
use crate::agents::waiting::{
    self as agent_waiting, AgentAskVm, AgentAsksVm, Mine, Reach, RoomAsks, Seen, WaitingAsk,
};
use crate::inbox::{AgentListTrouble, InboxMerger};
use crate::registry;
use crate::vm::RoomVm;

/// What a sign-out says when the account's kept Agents rows could not be
/// removed (R501, R381): it stops before the session goes, so it can be
/// tried again.
pub(crate) const KEPT_ROWS_NOT_REMOVED: &str = "keeper could not remove the agents it kept for this account's next launch, so the account is still signed in on this device. Sign out again to remove them.";

/// What every account's agent rows say of their rooms now (story 102.5):
/// written by each account's worker, seeded with the rows kept from the
/// last run, read by the rows as they are drawn and by the notes dock.
#[derive(Debug, Default)]
pub struct AttentionBoard {
    said: std::sync::Mutex<HashMap<(String, OwnedRoomId), Option<AgentAttentionVm>>>,
}

impl AttentionBoard {
    fn said_all(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<(String, OwnedRoomId), Option<AgentAttentionVm>>> {
        self.said.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What `account`'s row of `room` says: the last reading, else the kept
    /// state, else *Unknown* — a room not read yet is never *nothing
    /// pending* (R436).
    pub fn said(&self, account: &str, room: &RoomId) -> Option<AgentAttentionVm> {
        self.said_all()
            .get(&(account.to_owned(), room.to_owned()))
            .copied()
            .unwrap_or(Some(AgentAttentionVm::Unknown))
    }

    fn put(&self, account: &str, room: &RoomId, state: Option<AgentAttentionVm>) {
        self.said_all()
            .insert((account.to_owned(), room.to_owned()), state);
    }

    /// The states `rows` — kept from the last run — say, for the rooms not
    /// read yet in this one.
    pub fn seed(&self, account: &str, rows: &[RoomVm]) {
        let mut said = self.said_all();
        for row in rows {
            if let Ok(room) = OwnedRoomId::try_from(row.room_id.as_str()) {
                said.entry((account.to_owned(), room))
                    .or_insert(row.agent_attention);
            }
        }
    }

    /// Forget `account`'s rows: it signed out.
    pub fn forget(&self, account: &str) {
        self.said_all().retain(|(held, _), _| held != account);
    }
}

/// Draw `account_id`'s kept Agents rows ([`ListRowStore`]) as the Agents
/// window's first emission, and their states on the board, before the
/// account is activated (story 102.5 #1). A store that cannot be read draws
/// nothing, with a WARN and a sentence above the list (R381, R501): the live
/// rows come as before.
pub(crate) async fn draw_kept_rows(
    store: &ListRowStore,
    board: &AttentionBoard,
    merger: &InboxMerger,
    account_id: &str,
) {
    match store.of(account_id) {
        Ok(rows) if rows.is_empty() => {}
        Ok(rows) => {
            board.seed(account_id, &rows);
            merger.seed_agents(account_id, rows).await;
        }
        Err(error) => {
            tracing::warn!(account_id = %account_id, %error, "agents: the Agents list kept from the last run could not be read; it is drawn once the account is live");
            merger
                .trouble(account_id, AgentListTrouble::KeptUnread, true)
                .await;
        }
    }
}

/// Keep `rows` as `account_id`'s Agents rows for the next run through
/// `writer` — in the order they were taken, and never after a sign-out
/// ([`RowWriter`], R501) — off the producer's path. A failed write is a
/// WARN and said above the list until a later snapshot is kept (R381); a
/// snapshot superseded or fenced changes nothing.
pub(crate) fn keep_rows(
    writer: &RowWriter,
    merger: &InboxMerger,
    account_id: &str,
    rows: Vec<RoomVm>,
) -> JoinHandle<()> {
    let write = writer.keep(rows);
    let (merger, account_id) = (merger.clone(), account_id.to_owned());
    tokio::spawn(async move {
        let failed = match write.await {
            Ok(Ok(Keep::Written)) => false,
            Ok(Ok(Keep::Superseded | Keep::Fenced)) => return,
            Ok(Err(error)) => {
                tracing::warn!(account_id = %account_id, %error, "agents: the Agents list could not be kept for the next launch");
                true
            }
            Err(error) => {
                tracing::warn!(account_id = %account_id, %error, "agents: the Agents list's write stopped before it was kept for the next launch");
                true
            }
        };
        merger
            .trouble(&account_id, AgentListTrouble::KeptUnwritten, failed)
            .await;
    })
}

/// One room's events as a pass read them: how far back, the questions
/// folded from them for each proxy, and the person's messages among them.
struct EventsRead {
    reach: Reach,
    folds: HashMap<OwnedUserId, RoomAsks>,
    mine: Vec<Mine>,
}

/// One reading of a session room ([`read_one`]).
struct Reading {
    facts: agent_attention::Facts,
    /// `None`: the room's events could not be read.
    events: Option<EventsRead>,
    levels_read: bool,
}

/// A session room as a pass knows it: the room, its row as a listing reads
/// it ([`proxy_row`]) and its last reading, `None` until it is read.
struct Known {
    room: Room,
    row: ProxyRoomRow,
    reading: Option<Reading>,
}

/// The person's own text messages among `seen`, as
/// [`agent_waiting::answer_state`] reads them.
fn mine_of(seen: &[Seen], me: &UserId) -> Vec<Mine> {
    seen.iter()
        .filter_map(|seen| {
            let event = &seen.value;
            (event["type"] == "m.room.message" && event["sender"] == me.as_str()).then_some(())?;
            let content = &event["content"];
            (content["msgtype"] == "m.text" && content["m.relates_to"]["rel_type"] != "m.replace")
                .then_some(())?;
            Some(Mine {
                body: content["body"].as_str()?.to_owned(),
                at: event["origin_server_ts"].as_u64()?,
                held: false,
            })
        })
        .collect()
}

/// Read `room` once: its facts, and from the same events the questions
/// folded for each of `proxies` and the person's messages.
async fn read_one(room: &Room, me: &UserId, proxies: &BTreeSet<OwnedUserId>) -> Reading {
    let read = agent_attention::read(room).await;
    let levels = read.levels.as_ref();
    let agent = |user: &UserId| holds_agent_power(levels, user);
    let events = read.events.map(|(seen, reach)| EventsRead {
        reach,
        folds: proxies
            .iter()
            .map(|proxy| {
                let folded = agent_waiting::fold(seen.iter().cloned(), me, proxy, &agent);
                (proxy.clone(), folded)
            })
            .collect(),
        mine: mine_of(&seen, me),
    });
    Reading {
        facts: read.facts,
        events,
        levels_read: levels.is_some(),
    }
}

/// Every joined agent session room of `client`, each with its row as a
/// listing reads it, none read yet.
async fn session_rooms(client: &Client, kinds: &AgentKinds) -> HashMap<OwnedRoomId, Known> {
    let mut rooms = HashMap::new();
    for room in client.joined_rooms() {
        if AgentRoomKind::of(room.room_type().as_ref()) != Some(AgentRoomKind::Session) {
            continue;
        }
        let row = proxy_row(room.clone(), kinds).await;
        rooms.insert(
            room.room_id().to_owned(),
            Known {
                room,
                row,
                reading: None,
            },
        );
    }
    rooms
}

/// Why a reading of the questions is in part, per room: logged where the
/// panel reads them, and where a row turns *Unknown* (R381).
type Why = Vec<(OwnedRoomId, &'static str)>;

/// What [`Placement::take`] logs for a room whose status is unread.
const STATUS_UNREAD: &str =
    "agents: a room's status is unread, so the questions answered there could not be read";

/// Where the questions answered in a room stand.
enum Placed {
    /// The room is no proxy conversation of the person's: none is answered
    /// there.
    NotAsked,
    /// The room's status is unread: whether it is one is not known.
    Unread,
    /// The questions answered there, each with the room it was asked in,
    /// and whether a room one may wait in was read only in part.
    Asks {
        waiting: Vec<(WaitingAsk, Room)>,
        partial: bool,
    },
}

/// Where every question the readings of a pass folded is answered (101.3's
/// projection, R430, R431, R453; R501, review R102L5-03): built once per
/// pass, then taken room by room.
struct Placement {
    /// Per proxy room, its proxy and where its questions stand; per room
    /// whose status is unread, [`Placed::Unread`].
    placed: HashMap<OwnedRoomId, (Option<OwnedUserId>, Placed)>,
    /// Per proxy, why the readings of its rooms' questions are in part.
    why: HashMap<OwnedUserId, Why>,
    /// How many (proxy, room) pairs were visited to build it: at most the
    /// proxies times the rooms.
    visits: usize,
}

impl Placement {
    /// The questions answered in `here`, and its proxy when it is a proxy
    /// room.
    fn take(&mut self, here: &RoomId) -> (Placed, Option<OwnedUserId>) {
        match self.placed.remove(here) {
            Some((proxy, placed)) => (placed, proxy),
            None => (Placed::NotAsked, None),
        }
    }

    /// Why the readings of `proxy`'s rooms' questions are in part, once.
    fn why(&mut self, proxy: &UserId) -> Why {
        self.why.remove(proxy).unwrap_or_default()
    }
}

/// Place every question folded in `rooms`' readings (101.3's projection,
/// R430, R431, R453) in the one room that answers it: only the person's
/// proxy rooms ([`proxy::admits`]) answer any — a room whose status keeper
/// has not read may be one, so it answers none, read in part. For each
/// proxy, its rooms are indexed once; then every other session room is
/// visited once: its questions, folded for that proxy when it was read
/// ([`agent_waiting::fold`]), go to the room the host puts them to the
/// person in ([`agent_waiting::answered_in`] over the proxy's rooms), and a
/// room not read, read in part, whose power levels were not read, or whose
/// asks could not be placed makes every one of the proxy's rooms read in
/// part. `P` proxies and `N` rooms cost at most `P × N` visits, however
/// many conversations a proxy has (R501, review R102L5-03).
fn place_all(rooms: &HashMap<OwnedRoomId, Known>, me: &UserId, known: &KnownProxies) -> Placement {
    let mut own: HashMap<OwnedUserId, HashMap<OwnedRoomId, SessionKind>> = HashMap::new();
    let mut placed = HashMap::new();
    for (id, known_room) in rooms {
        match proxy::admits(&known_room.row, me, known) {
            Ok(admitted) => {
                own.entry(admitted.agent.to_owned())
                    .or_default()
                    .insert(id.clone(), admitted.kind);
            }
            Err(proxy::Refusal::Unread) => {
                placed.insert(id.clone(), (None, Placed::Unread));
            }
            Err(_) => {}
        }
    }
    let mut why = HashMap::new();
    let mut visits = 0;
    for (proxy, proxy_rooms) in own {
        let main = proxy_rooms
            .iter()
            .find(|(_, kind)| **kind == SessionKind::Main)
            .map(|(room, _)| room.clone());
        let proxy_kind = |room: &RoomId| proxy_rooms.get(room).copied();
        let mut said = Why::new();
        let mut partial = false;
        let mut waiting: HashMap<OwnedRoomId, Vec<(WaitingAsk, Room)>> = HashMap::new();
        for (id, other) in rooms {
            if proxy_rooms.contains_key(id) {
                continue;
            }
            visits += 1;
            let Some((reading, events)) = other
                .reading
                .as_ref()
                .and_then(|reading| Some((reading, reading.events.as_ref()?)))
            else {
                said.push((id.clone(), "agents: a room's asks could not be read"));
                partial = true;
                continue;
            };
            // An ask behind a failed read-back may be waiting (R453).
            partial |= events.reach == Reach::Part;
            let whole = events.reach == Reach::Start;
            if !reading.levels_read {
                said.push((
                    id.clone(),
                    "agents: a room's power levels could not be read for its asks",
                ));
                partial = true;
                continue;
            }
            let Some(asks) = events.folds.get(&proxy) else {
                said.push((id.clone(), "agents: a room's asks could not be read"));
                partial = true;
                continue;
            };
            if asks.waiting.is_empty() {
                continue;
            }
            match agent_waiting::answered_in(
                other.row.kind,
                asks,
                whole,
                &proxy_kind,
                main.as_deref(),
            ) {
                Some(target) if proxy_rooms.contains_key(&target) => {
                    waiting.entry(target).or_default().extend(
                        asks.waiting
                            .iter()
                            .cloned()
                            .map(|ask| (ask, other.room.clone())),
                    );
                }
                Some(_) => {}
                None => {
                    said.push((id.clone(), "agents: a room's asks could not be placed"));
                    partial = true;
                }
            }
        }
        for id in proxy_rooms.keys() {
            placed.insert(
                id.clone(),
                (
                    Some(proxy.clone()),
                    Placed::Asks {
                        waiting: waiting.remove(id).unwrap_or_default(),
                        partial,
                    },
                ),
            );
        }
        why.insert(proxy, said);
    }
    Placement {
        placed,
        why,
        visits,
    }
}

/// The person's own text messages in `here` and whether every source was
/// read: the room's events as its reading holds them (one read back only in
/// part is not whole), the send queue's local echoes that have not failed,
/// and the sends held for Undo-Send in `data_dir`'s outbox (`None`:
/// unreadable). A source that cannot be read is logged at WARN and left out.
async fn mine_in(here: &Known, data_dir: Option<&Path>, account_id: &str) -> (Vec<Mine>, bool) {
    let room = &here.room;
    let mut mine = Vec::new();
    let mut whole = true;
    match here
        .reading
        .as_ref()
        .and_then(|reading| reading.events.as_ref())
    {
        Some(events) => {
            whole &= events.reach != Reach::Part;
            mine.extend(events.mine.iter().cloned());
        }
        None => {
            tracing::warn!(room = %room.room_id(), "agents: the person's answers could not be read");
            whole = false;
        }
    }
    let text = |content: &serde_json::Value| {
        (content["msgtype"] == "m.text" && content["m.relates_to"]["rel_type"] != "m.replace")
            .then(|| content["body"].as_str().map(str::to_owned))
            .flatten()
    };
    match room.send_queue().subscribe().await {
        Ok((echoes, _)) => mine.extend(echoes.iter().filter_map(|echo| {
            let matrix_sdk::send_queue::LocalEchoContent::Event {
                serialized_event,
                send_handle,
                send_error: None,
                ..
            } = &echo.content
            else {
                return None;
            };
            let (raw, event_type) = serialized_event.raw();
            (event_type == "m.room.message").then_some(())?;
            // Dated by when it was queued: written before the ask, it answers
            // nothing however long it waits (R447).
            Some(Mine {
                body: text(&raw.deserialize_as::<serde_json::Value>().ok()?)?,
                at: u64::from(send_handle.created_at.0),
                held: false,
            })
        })),
        Err(error) => {
            tracing::warn!(room = %room.room_id(), %error, "agents: the send queue could not be read for the person's answers");
            whole = false;
        }
    }
    match data_dir.map(|dir| registry::list_outbox_rows_for_account(dir, account_id)) {
        Some(Ok(rows)) => mine.extend(
            rows.into_iter()
                .filter(|row| {
                    row.room_id == room.room_id().as_str() && row.kind == MessageKind::Text
                })
                .filter_map(|row| {
                    Some(Mine {
                        at: u64::try_from(row.held_at_ts).ok()?,
                        body: row.body,
                        held: true,
                    })
                }),
        ),
        Some(Err(error)) => {
            tracing::warn!(room = %room.room_id(), %error, "agents: the held sends could not be read for the person's answers");
            whole = false;
        }
        None => {
            tracing::warn!(room = %room.room_id(), "agents: no data folder, so the held sends could not be read for the person's answers");
            whole = false;
        }
    }
    (mine, whole)
}

/// The panel's questions for `here` as `placed` found them, each with where
/// its answer stands ([`agent_waiting::answer_state`]); `None` for a room no
/// question is answered in, and a reading in part says
/// [`agent_waiting::ASKS_PARTIAL`] beside them — never a complete, empty
/// answer (R453).
async fn asks_vm(
    here: &Known,
    placed: Placed,
    data_dir: Option<&Path>,
    account_id: &str,
) -> Option<AgentAsksVm> {
    let (waiting, mut partial) = match placed {
        Placed::NotAsked => return None,
        Placed::Unread => (Vec::new(), true),
        Placed::Asks { waiting, partial } => (waiting, partial),
    };
    let mut asks = Vec::new();
    if !waiting.is_empty() {
        let (mine, read_whole) = mine_in(here, data_dir, account_id).await;
        partial |= !read_whole;
        for (ask, room) in waiting {
            let asked_by = room
                .get_member_no_sync(&ask.asker)
                .await
                .ok()
                .flatten()
                .and_then(|member| member.display_name().map(str::to_owned))
                .unwrap_or_else(|| ask.asker.localpart().to_owned());
            let state = agent_waiting::answer_state(&ask, &mine);
            asks.push(AgentAskVm {
                id: ask.ask.id,
                room_id: room.room_id().to_string(),
                asked_by,
                question: ask.ask.question,
                choices: ask.ask.choices,
                default: ask.ask.default,
                asked_at: ask.at,
                state,
            });
        }
        asks.sort_by_key(|ask| ask.asked_at);
    }
    Some(AgentAsksVm {
        asks,
        note: partial.then(|| agent_waiting::ASKS_PARTIAL.to_owned()),
    })
}

fn log_why(why: Why) {
    for (room, what) in why {
        tracing::warn!(room = %room, "{what}");
    }
}

/// The questions waiting for this person that are answered in `here`
/// (Story 101.3, R430, R431; story 101.6, R453), oldest first: what the
/// decisions panel reads, by the same projection the rows' states are
/// placed by ([`place_all`]). Every other session room this account reads is
/// read back once to [`agent_waiting::ASK_READ_BACK`] events. `None` for a
/// room no question is answered in; a reading in part is logged at WARN and
/// said beside the panel ([`agent_waiting::ASKS_PARTIAL`], R381).
pub(crate) async fn asks_answered_in(
    here: &Room,
    data_dir: Option<&Path>,
    account_id: &str,
    kinds: &AgentKinds,
    proxies: &AgentProxies,
) -> Option<AgentAsksVm> {
    if AgentRoomKind::of(here.room_type().as_ref()) != Some(AgentRoomKind::Session) {
        return None;
    }
    let client = here.client();
    let me = here.own_user_id().to_owned();
    let known = known_proxies(&client, proxies).await;
    let mut rooms = session_rooms(&client, kinds).await;
    let proxy = rooms.get(here.room_id()).and_then(|this| {
        proxy::admits(&this.row, &me, &known)
            .ok()
            .map(|admitted| admitted.agent.to_owned())
    });
    if let Some(proxy) = proxy {
        let folds = BTreeSet::from([proxy.clone()]);
        for (id, other) in rooms.iter_mut() {
            let own = proxy::admits(&other.row, &me, &known)
                .is_ok_and(|admitted| admitted.agent == &*proxy);
            if !own || id == here.room_id() {
                other.reading = Some(read_one(&other.room, &me, &folds).await);
            }
        }
    }
    let mut placement = place_all(&rooms, &me, &known);
    let (placed, proxy) = placement.take(here.room_id());
    match (&placed, proxy) {
        (Placed::Unread, _) => log_why(vec![(here.room_id().to_owned(), STATUS_UNREAD)]),
        (Placed::Asks { partial: true, .. }, Some(proxy)) => log_why(placement.why(&proxy)),
        _ => {}
    }
    asks_vm(rooms.get(here.room_id())?, placed, data_dir, account_id).await
}

/// What a producer hands the worker to read again: the rooms the room list
/// redrew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Due {
    /// These rooms changed: their rows and events are read again.
    Rooms(Vec<OwnedRoomId>),
    /// A card in this room expires at this time: what a sealed approval
    /// card says, which a loopback homeserver cannot seal (tests only).
    #[cfg(test)]
    Expires(OwnedRoomId, chrono::DateTime<chrono::Utc>),
}

/// What the next pass reads.
#[derive(Debug, Default)]
struct Todo {
    read_all: bool,
    rows_all: bool,
    rooms: HashSet<OwnedRoomId>,
}

impl Todo {
    fn any(&self) -> bool {
        self.read_all || self.rows_all || !self.rooms.is_empty()
    }

    fn add(&mut self, due: Due, kept: &Kept) {
        match due {
            Due::Rooms(rooms) => self.rooms.extend(rooms),
            #[cfg(test)]
            Due::Expires(room, at) => kept.noted.expires(&room, at),
        }
        let _ = kept;
    }
}

/// One account's list worker (story 102.5): what it reads with, and where
/// its states go.
#[derive(Clone)]
pub(crate) struct ListWorker {
    pub client: Client,
    pub account_id: String,
    pub kinds: Arc<AgentKinds>,
    pub proxies: Arc<AgentProxies>,
    pub data_dir: Option<PathBuf>,
    pub board: Arc<AttentionBoard>,
}

/// What the worker keeps between passes: every session room's row and last
/// reading, what each row said and when to read it again, which proxies the
/// readings folded questions for, how many rooms it has read and how many
/// (proxy, room) pairs its placements visited.
#[derive(Default)]
pub(crate) struct Kept {
    rooms: HashMap<OwnedRoomId, Known>,
    noted: RowAttention,
    folded: BTreeSet<OwnedUserId>,
    reads: Arc<AtomicUsize>,
    #[cfg(test)]
    visits: usize,
}

#[cfg(test)]
impl Kept {
    /// How many room readings the worker has made.
    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    /// The count of [`Kept::reads`], read while a running worker owns it.
    pub(crate) fn reads_counter(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.reads)
    }

    /// How many (proxy, room) pairs the placements visited.
    pub(crate) fn visits(&self) -> usize {
        self.visits
    }
}

impl ListWorker {
    /// One pass: the rows of `todo`'s rooms (or all) read again from the
    /// cache and the seats, each room whose events changed read once, then
    /// every row's state placed from the kept readings in memory. Returns
    /// the rooms whose row's state changed, written to the board.
    async fn pass(&self, kept: &mut Kept, todo: Todo) -> Vec<OwnedRoomId> {
        let Some(me) = self.client.user_id().map(ToOwned::to_owned) else {
            return Vec::new();
        };
        let known = known_proxies(&self.client, &self.proxies).await;
        let session: Vec<Room> = self
            .client
            .joined_rooms()
            .into_iter()
            .filter(|room| {
                AgentRoomKind::of(room.room_type().as_ref()) == Some(AgentRoomKind::Session)
            })
            .collect();
        let here: HashSet<OwnedRoomId> = session
            .iter()
            .map(|room| room.room_id().to_owned())
            .collect();
        kept.rooms.retain(|id, _| here.contains(id));
        for room in session {
            let id = room.room_id().to_owned();
            let stale = todo.read_all || todo.rooms.contains(&id);
            match kept.rooms.get_mut(&id) {
                Some(known_room) if stale || todo.rows_all => {
                    known_room.row = proxy_row(room, &self.kinds).await;
                    if stale {
                        known_room.reading = None;
                    }
                }
                Some(_) => {}
                None => {
                    let row = proxy_row(room.clone(), &self.kinds).await;
                    kept.rooms.insert(
                        id,
                        Known {
                            room,
                            row,
                            reading: None,
                        },
                    );
                }
            }
        }
        // The questions are folded per proxy when a room is read: a proxy
        // no reading folded for means every room is read again.
        let proxies: BTreeSet<OwnedUserId> = kept
            .rooms
            .values()
            .filter_map(|known_room| {
                proxy::admits(&known_room.row, &me, &known)
                    .ok()
                    .map(|admitted| admitted.agent.to_owned())
            })
            .collect();
        if proxies != kept.folded {
            for known_room in kept.rooms.values_mut() {
                known_room.reading = None;
            }
            kept.folded = proxies;
        }
        for known_room in kept.rooms.values_mut() {
            if known_room.reading.is_none() {
                known_room.reading = Some(read_one(&known_room.room, &me, &kept.folded).await);
                kept.reads.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut placement = place_all(&kept.rooms, &me, &known);
        tracing::debug!(account_id = %self.account_id, rooms = kept.rooms.len(), visits = placement.visits, "agents: a list pass placed its questions");
        #[cfg(test)]
        {
            kept.visits += placement.visits;
        }
        let ids: Vec<OwnedRoomId> = kept.rooms.keys().cloned().collect();
        let mut changed = Vec::new();
        let mut unknown: BTreeSet<OwnedUserId> = BTreeSet::new();
        for id in ids {
            let (placed, proxy) = placement.take(&id);
            let unread = matches!(placed, Placed::Unread);
            let Some(known_room) = kept.rooms.get(&id) else {
                continue;
            };
            let asks = asks_vm(
                known_room,
                placed,
                self.data_dir.as_deref(),
                &self.account_id,
            )
            .await;
            let Some(reading) = &known_room.reading else {
                continue;
            };
            let attention = reading.facts.with(asks.as_ref().map(Asks::of));
            if kept.noted.note(&id, &attention) {
                if attention.state == Some(AgentAttentionVm::Unknown) {
                    if unread {
                        log_why(vec![(id.clone(), STATUS_UNREAD)]);
                    }
                    unknown.extend(proxy);
                    tracing::warn!(room = %id, "agents: keeper could not read all it needs to say nothing waits in a room, so its row says Unknown");
                }
                self.board.put(&self.account_id, &id, attention.state);
                changed.push(id);
            }
        }
        // Why a proxy's rooms turned Unknown, once per proxy, not per room.
        for proxy in unknown {
            log_why(placement.why(&proxy));
        }
        changed
    }
}

/// Resolves when a room's card expires and it must be read again.
async fn recheck(noted: &RowAttention) {
    match noted.next_recheck() {
        Some(at) => {
            let wait = (at - chrono::Utc::now()).to_std().unwrap_or_default();
            tokio::time::sleep(wait).await;
        }
        None => std::future::pending().await,
    }
}

/// Whether `room_id` is one of `client`'s agent session rooms.
fn is_session(client: &Client, room_id: &RoomId) -> bool {
    client.get_room(room_id).is_some_and(|room| {
        AgentRoomKind::of(room.room_type().as_ref()) == Some(AgentRoomKind::Session)
    })
}

/// Run `worker` for its account (story 102.5 #2; R501, review R102L5-02):
/// a first pass reads every session room; then each change — a room the
/// room list redrew (`due`) or whose event cache updated, a seat, a card's
/// expiry — is settled for [`ASKS_SETTLE`] with whatever else arrives, and
/// read in one pass. The rooms whose row's state changed go to `changed`,
/// for whichever producers are drawing the account's rows now — none while
/// the person is elsewhere, when the board alone is written: the notes dock
/// and the launch read it. Runs until its account's teardown aborts it, or
/// `due`'s every sender is gone.
pub(crate) async fn run_attention_worker(
    worker: ListWorker,
    mut kept: Kept,
    mut due: mpsc::UnboundedReceiver<Due>,
    changed: broadcast::Sender<Vec<OwnedRoomId>>,
) {
    let mut updates = worker
        .client
        .event_cache()
        .subscribe_to_room_generic_updates();
    let mut events_open = true;
    let mut seats = worker.kinds.subscribe();
    let mut seats_open = true;
    let mut todo = Todo {
        read_all: true,
        ..Todo::default()
    };
    loop {
        if todo.any() {
            while let Ok(more) = due.try_recv() {
                todo.add(more, &kept);
            }
            loop {
                match updates.try_recv() {
                    Ok(update) if is_session(&worker.client, &update.room_id) => {
                        todo.rooms.insert(update.room_id);
                    }
                    Ok(_) => {}
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        todo.read_all = true;
                    }
                    Err(_) => break,
                }
            }
            let redraw = worker.pass(&mut kept, std::mem::take(&mut todo)).await;
            if !redraw.is_empty() {
                // No producer listening is the person elsewhere, not an end.
                let _ = changed.send(redraw);
            }
            continue;
        }
        tokio::select! {
            more = due.recv() => match more {
                Some(more) => todo.add(more, &kept),
                None => break,
            },
            update = updates.recv(), if events_open => match update {
                Ok(update) => {
                    if is_session(&worker.client, &update.room_id) {
                        todo.rooms.insert(update.room_id);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => todo.read_all = true,
                Err(broadcast::error::RecvError::Closed) => events_open = false,
            },
            seat = seats.changed(), if seats_open => match seat {
                // A seat changed: every row is read again from the cache and
                // the seats, and no room's events are.
                Ok(()) => todo.rows_all = true,
                Err(_) => seats_open = false,
            },
            () = recheck(&kept.noted) => {
                todo.rooms.extend(kept.noted.due(chrono::Utc::now()));
            }
        }
        if todo.any() {
            tokio::time::sleep(ASKS_SETTLE).await;
        }
    }
}

/// How many rounds of changed rooms a producer may fall behind before it
/// redraws every agent row instead.
const CHANGED_BACKLOG: usize = 64;

/// One account's list worker and seat finder (story 102.5), alive from the
/// account's activation to its teardown, whatever surface reads what they
/// write (R501, review R102L5-02): the Agents list's producers come and go
/// with the Chats view, while the notes dock and the launch read the board
/// and the seats the whole time. Dropping it stops both.
pub(crate) struct AgentList {
    due: mpsc::UnboundedSender<Due>,
    changed: broadcast::Sender<Vec<OwnedRoomId>>,
    worker: JoinHandle<()>,
    finder: JoinHandle<()>,
}

impl Drop for AgentList {
    fn drop(&mut self) {
        self.worker.abort();
        self.finder.abort();
    }
}

impl AgentList {
    /// Stop both and wait until they are gone: each holds the account's
    /// `Client`, which a sign-out must release before it deletes the store.
    pub(crate) async fn stop(mut self) {
        self.worker.abort();
        self.finder.abort();
        let _ = (&mut self.worker).await;
        let _ = (&mut self.finder).await;
    }
}

/// Every live account's [`AgentList`], by account id.
#[derive(Default)]
pub(crate) struct AgentLists {
    running: std::sync::Mutex<HashMap<String, AgentList>>,
}

impl AgentLists {
    fn running(&self) -> std::sync::MutexGuard<'_, HashMap<String, AgentList>> {
        // Handles and channel ends only: a panic mid-update tears nothing.
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start `worker`'s account's list worker and seat finder: at its
    /// activation. One already running for the account (activated again
    /// without a teardown) is stopped.
    pub(crate) fn start(&self, worker: ListWorker) {
        let account_id = worker.account_id.clone();
        let (due, due_rx) = mpsc::unbounded_channel();
        let (changed, _) = broadcast::channel(CHANGED_BACKLOG);
        let finder = tokio::spawn(
            run_seat_finder(worker.client.clone(), Arc::clone(&worker.kinds))
                .instrument(tracing::info_span!("agent_seat_finder", account_id = %account_id)),
        );
        let worker = tokio::spawn(
            run_attention_worker(worker, Kept::default(), due_rx, changed.clone())
                .instrument(tracing::info_span!("agent_list_worker", account_id = %account_id)),
        );
        let list = AgentList {
            due,
            changed,
            worker,
            finder,
        };
        let replaced = self.running().insert(account_id, list);
        drop(replaced);
    }

    /// What a producer drawing `account_id`'s rows hands its worker (the
    /// rooms it redrew) and hears from it (the rooms whose state changed);
    /// `None` for an account with no worker.
    pub(crate) fn attach(
        &self,
        account_id: &str,
    ) -> Option<(
        mpsc::UnboundedSender<Due>,
        broadcast::Receiver<Vec<OwnedRoomId>>,
    )> {
        self.running()
            .get(account_id)
            .map(|list| (list.due.clone(), list.changed.subscribe()))
    }

    /// Take `account_id`'s list out, at its teardown: dropping it stops it,
    /// [`AgentList::stop`] waits for that.
    pub(crate) fn take(&self, account_id: &str) -> Option<AgentList> {
        self.running().remove(account_id)
    }
}

/// How many pages of [`HeaderReader::find`] one look of the finder reads.
pub(crate) const FINDER_PAGES: usize = 20;

/// How long one look may take.
pub(crate) const FINDER_WAIT: Duration = Duration::from_secs(30);

/// How often the finder looks for rooms that need a look, when none is due.
const FINDER_RESCAN: Duration = Duration::from_secs(60);

/// How long the finder waits before its next look at a room it looked at
/// `looks` times without finding its status (R416's "look again later"):
/// at once, then 1, 5 and 30 minutes, then hourly — at most five looks in
/// a room's first hour, one an hour after.
pub(crate) fn finder_backoff(looks: u32) -> Duration {
    const MINUTE: u64 = 60;
    Duration::from_secs(match looks {
        0 => 0,
        1 => MINUTE,
        2 => 5 * MINUTE,
        3 => 30 * MINUTE,
        _ => 60 * MINUTE,
    })
}

/// The finder's next look at one room.
struct Look {
    at: tokio::time::Instant,
    looks: u32,
    /// Where the last look stopped, to go on from.
    from: Option<String>,
}

/// Look for the statuses of `client`'s session rooms that have no seat and
/// none in the cache (story 102.5 #3, R416, R468): one look at a time, each
/// [`HeaderReader::find`] going [`FINDER_PAGES`] further back than the last
/// under [`FINDER_WAIT`], the next after [`finder_backoff`]. A status found
/// is kept as the room's seat, which redraws its row; a history searched
/// to its start is not searched again.
pub(crate) async fn run_seat_finder(client: Client, kinds: Arc<AgentKinds>) {
    let mut finder = SeatFinder::new(client, kinds);
    while let Some(next) = finder.look().await {
        tokio::time::sleep_until(next).await;
    }
}

/// The finder's state between rounds: each room's next look.
pub(crate) struct SeatFinder {
    client: Client,
    kinds: Arc<AgentKinds>,
    looks: HashMap<OwnedRoomId, Look>,
}

impl SeatFinder {
    pub(crate) fn new(client: Client, kinds: Arc<AgentKinds>) -> SeatFinder {
        SeatFinder {
            client,
            kinds,
            looks: HashMap::new(),
        }
    }

    /// One round: look at every room whose look is due, then say when the
    /// next round is — the soonest look still due later, at most
    /// [`FINDER_RESCAN`] away. A room that is no longer one to look at — left,
    /// seated, searched to its start — has its look dropped first, so its
    /// past deadline never makes the next round come at once (R516, review
    /// R102L6-03); a room still to look at keeps its look, where it stopped
    /// and its backoff. `None` once the client has no user.
    pub(crate) async fn look(&mut self) -> Option<tokio::time::Instant> {
        let me = self.client.user_id().map(ToOwned::to_owned)?;
        let mut eligible = HashSet::new();
        for room in self.client.joined_rooms() {
            if AgentRoomKind::of(room.room_type().as_ref()) != Some(AgentRoomKind::Session) {
                continue;
            }
            let id = room.room_id().to_owned();
            if self.kinds.seat(&me, &id).is_some() || self.kinds.searched_empty(&me, &id) {
                continue;
            }
            eligible.insert(id.clone());
            let (at, from) = match self.looks.get(&id) {
                Some(look) => (look.at, look.from.clone()),
                None => (tokio::time::Instant::now(), None),
            };
            if at > tokio::time::Instant::now() {
                continue;
            }
            let found = tokio::time::timeout(
                FINDER_WAIT,
                HeaderReader::find(room, &self.kinds, from.clone(), FINDER_PAGES),
            )
            .await;
            let (looked, from) = match found {
                Ok(Found::Seat | Found::Empty) => {
                    eligible.remove(&id);
                    continue;
                }
                Ok(Found::Further(further)) => (true, Some(further)),
                Ok(Found::Failed) | Err(_) => (false, from),
            };
            let made = self.looks.get(&id).map_or(0, |look| look.looks) + 1;
            if !looked {
                tracing::warn!(room = %id, "agents: keeper could not look back for an agent room's status; it looks again later");
            }
            self.looks.insert(
                id,
                Look {
                    at: tokio::time::Instant::now() + finder_backoff(made),
                    looks: made,
                    from,
                },
            );
        }
        self.looks.retain(|id, _| eligible.contains(id));
        let rescan = tokio::time::Instant::now() + FINDER_RESCAN;
        Some(
            self.looks
                .values()
                .map(|look| look.at)
                .min()
                .map_or(rescan, |at| at.min(rescan)),
        )
    }
}

#[cfg(test)]
#[path = "agent_list_tests.rs"]
mod tests;
