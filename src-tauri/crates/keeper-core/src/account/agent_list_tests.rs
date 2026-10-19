//! The Agents list against a loopback homeserver (story 102.5): the SDK's
//! own store and event cache behind the list worker, the room-list producer
//! and the seat finder. Each test drives `/sync` by hand and serves, holds
//! or fails `/messages` to put the device where it needs to be.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use serde_json::{json, Value};

use super::*;
use crate::account::lifetime::Pause;
use crate::agents::events::{CONTENT_VERSION, SESSION_ROOM_TYPE, STATUS};
use crate::agents::room::AgentIcons;
use crate::error::CoreError;
use crate::forges::testing::{self, FakePlatform, Reply, Seen as Request};
use crate::vm::{InboxBatch, InboxOp, NetworksSnapshot, SpacesSnapshot};

const ME: &str = "@tgorka:h";
const NIXI: &str = "@nixi:h";
const TOLA: &str = "@tola:h";
const ACCOUNT: &str = "acct";
/// The person's conversation with their proxy: its questions are placed
/// from every other room's reading.
const CONVERSATION: &str = "!r00:h";
/// How many agent session rooms the device has.
const ROOMS: usize = 30;

fn room_id(n: usize) -> String {
    format!("!r{n:02}:h")
}

/// What `/messages` does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Messages {
    /// Nothing yet: the request waits.
    Held,
    /// The room's history is empty before the sync's gap.
    Serve,
}

struct Server {
    syncs: VecDeque<Value>,
    messages: Messages,
    /// Rooms (`r07`) whose `/messages` fail.
    failing: HashSet<String>,
}

type Shared = Arc<(Mutex<Server>, Condvar)>;

/// The room tag (`r07`) a request's path names, if any.
fn tag_of(path: &str) -> Option<String> {
    let at = path.find("rooms/")? + "rooms/".len();
    let id = path[at..].split('/').next()?;
    let id = id.replace("%21", "!").replace("%3A", ":");
    Some(id.trim_start_matches('!').split(':').next()?.to_owned())
}

fn answer(shared: &Shared, seen: &Request) -> Reply {
    let path = seen.path.as_str();
    if path.ends_with("/sync") {
        let next = shared.0.lock().expect("server").syncs.pop_front();
        return Reply::json(
            200,
            &next
                .unwrap_or_else(|| json!({"next_batch": "idle"}))
                .to_string(),
        );
    }
    if path.ends_with("/messages") {
        let (lock, wake) = &**shared;
        let mut server = lock.lock().expect("server");
        while server.messages == Messages::Held {
            server = wake.wait(server).expect("server");
        }
        if tag_of(path).is_some_and(|tag| server.failing.contains(&tag)) {
            return Reply::json(500, r#"{"errcode":"M_UNKNOWN","error":"down"}"#);
        }
        return Reply::json(200, r#"{"chunk":[],"start":"t0"}"#);
    }
    if path.ends_with("/versions") {
        return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
    }
    if path.ends_with("/keys/upload") {
        return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
    }
    if path.ends_with("/keys/query") {
        return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
    }
    Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
}

fn state(kind: &str, key: &str, sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": kind, "state_key": key, "sender": sender, "content": content,
        "event_id": format!("$s{n}:h"), "origin_server_ts": 1_000 + n,
    })
}

/// An encrypted session room `agent` made and holds power in, with the
/// person in it.
fn room_state(agent: &str) -> Vec<Value> {
    vec![
        state(
            "m.room.create",
            "",
            agent,
            json!({ "creator": agent, "room_version": "11", "type": SESSION_ROOM_TYPE }),
            1,
        ),
        state(
            "m.room.power_levels",
            "",
            agent,
            json!({ "users": { agent: 100 }, "users_default": 0, "events_default": 0 }),
            2,
        ),
        state(
            "m.room.member",
            agent,
            agent,
            json!({ "membership": "join" }),
            3,
        ),
        state("m.room.member", ME, ME, json!({ "membership": "join" }), 4),
        state(
            "m.room.encryption",
            "",
            agent,
            json!({ "algorithm": "m.megolm.v1.aes-sha2" }),
            5,
        ),
    ]
}

/// `agent`'s status of a `kind` session, its run `run`.
fn status(agent: &str, kind: &str, run: &str, n: u64) -> Value {
    json!({
        "type": STATUS, "sender": agent, "event_id": format!("$t{n}:h"),
        "origin_server_ts": 2_000 + n,
        "content": {
            "v": CONTENT_VERSION, "session": "01J9ZQ3W4K5M6N7P8Q9R0SESSN", "kind": kind,
            "title": "plan", "agent": agent, "host": "electra", "epoch": 1, "run": run,
        },
    })
}

/// The first sync: the person's conversation with Nixi, their proxy
/// (`dev.keeper.agent.proxies`), and 29 sessions Tola runs that the person
/// watches. Each timeline is limited, so a room's history behind it is on
/// the server.
fn first_sync() -> Value {
    sync_of(1, ROOMS - 1)
}

/// A first sync of `conversations` of the person's with Nixi, their proxy,
/// each running, then `delegated` sessions Tola runs, idle.
fn sync_of(conversations: usize, delegated: usize) -> Value {
    let mut join = serde_json::Map::new();
    for n in 0..conversations + delegated {
        let (agent, kind, run) = if n < conversations {
            (NIXI, "conversation", "running")
        } else {
            (TOLA, "delegated", "idle")
        };
        join.insert(
            room_id(n),
            json!({
                "state": { "events": room_state(agent) },
                "timeline": {
                    "events": [status(agent, kind, run, n as u64)],
                    "limited": true, "prev_batch": format!("p{n}"),
                },
            }),
        );
    }
    json!({
        "next_batch": "s1",
        "rooms": { "join": join },
        "account_data": { "events": [{
            "type": "dev.keeper.agent.proxies",
            "content": { "v": CONTENT_VERSION, "agents": [NIXI] },
        }]},
    })
}

/// A homeserver's state: `sync` first, `/messages` as `messages` says.
fn server(sync: Value, messages: Messages) -> Shared {
    Arc::new((
        Mutex::new(Server {
            syncs: VecDeque::from([sync]),
            messages,
            failing: HashSet::new(),
        }),
        Condvar::new(),
    ))
}

struct Device {
    shared: Shared,
    fake: testing::Fake,
    client: Client,
    data: tempfile::TempDir,
}

impl Device {
    async fn new(messages: Messages) -> Device {
        Device::with(first_sync(), messages).await
    }

    async fn with(sync: Value, messages: Messages) -> Device {
        let shared = server(sync, messages);
        let handler = Arc::clone(&shared);
        let fake = testing::serve(move |seen| answer(&handler, seen));
        let client = Client::builder()
            .homeserver_url(&fake.base)
            .build()
            .await
            .expect("client");
        let session: matrix_sdk::authentication::matrix::MatrixSession = serde_json::from_value(
            json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }),
        )
        .expect("session");
        client.restore_session(session).await.expect("restore");
        client.event_cache().subscribe().expect("event cache");
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
        Device {
            shared,
            fake,
            client,
            data: tempfile::tempdir().expect("data"),
        }
    }

    fn set(&self, change: impl FnOnce(&mut Server)) {
        change(&mut self.shared.0.lock().expect("server"));
        self.shared.1.notify_all();
    }

    fn worker(&self, board: &Arc<AttentionBoard>) -> ListWorker {
        ListWorker {
            client: self.client.clone(),
            account_id: ACCOUNT.to_owned(),
            kinds: Arc::new(AgentKinds::default()),
            proxies: Arc::new(AgentProxies::default()),
            data_dir: Some(self.data.path().to_owned()),
            board: Arc::clone(board),
        }
    }

    fn asked_messages(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages"))
            .count()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // No request is left waiting on the fake's threads.
        self.set(|server| server.messages = Messages::Serve);
    }
}

fn room(id: &str) -> OwnedRoomId {
    OwnedRoomId::try_from(id).expect("room")
}

fn rooms_todo(rooms: &[&str]) -> Todo {
    Todo {
        rooms: rooms.iter().map(|id| room(id)).collect(),
        ..Todo::default()
    }
}

type Batches = Arc<Mutex<Vec<InboxBatch>>>;

/// A merger whose Agents window is captured into the returned batches; the
/// other windows are dropped.
fn merger() -> (InboxMerger, Batches) {
    let agents: Batches = Arc::default();
    let into = Arc::clone(&agents);
    let platform: Arc<dyn crate::platform::Platform> = Arc::new(FakePlatform::default());
    let merger = InboxMerger::new(
        Box::new(|_: InboxBatch| true),
        Box::new(|_: InboxBatch| true),
        Box::new(|_: InboxBatch| true),
        Box::new(|_: InboxBatch| true),
        Box::new(move |batch: InboxBatch| {
            into.lock().expect("agents").push(batch);
            true
        }),
        HashMap::new(),
        Box::new(|_: SpacesSnapshot| true),
        Box::new(|_: NetworksSnapshot| true),
        platform,
        Arc::new(crate::badge::BadgeConfig::new(
            crate::vm::DockBadgeMode::Off,
        )),
    );
    (merger, agents)
}

/// The rows of the last Agents window emitted.
fn last_window(agents: &Batches) -> Vec<crate::vm::InboxRoomVm> {
    let batches = agents.lock().expect("agents");
    match batches.last().and_then(|batch| batch.ops.first()) {
        Some(InboxOp::Reset { rooms }) => rooms.clone(),
        _ => Vec::new(),
    }
}

/// Wait up to `bound` for `ready` to hold.
async fn until(bound: Duration, ready: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < bound {
        if ready() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ready()
}

/// 102.5 #2, #4 (R468, DW-1496): with every room's history held on the
/// server, the room list's first diff still draws all 30 agent rows at once
/// — each *Unknown*, none read — and the launch's readiness is marked;
/// once the reads land, the worker's states redraw the rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_agents_list_paints_before_any_room_is_read() {
    let device = Device::new(Messages::Held).await;
    let board: Arc<AttentionBoard> = Arc::default();
    let (merger, agents) = merger();
    merger.register_account(ACCOUNT, 0).await;
    let ready = crate::agents::launch::LaunchReadiness::default();
    ready.expect([ACCOUNT.to_owned()]);

    let (due, due_rx) = mpsc::unbounded_channel();
    let (changed_tx, changed) = broadcast::channel(CHANGED_BACKLOG);
    let worker = tokio::spawn(run_attention_worker(
        device.worker(&board),
        Kept::default(),
        due_rx,
        changed_tx,
    ));
    let items: matrix_sdk_ui::eyeball_im::Vector<matrix_sdk_ui::room_list_service::RoomListItem> =
        (0..ROOMS)
            .filter_map(|n| device.client.get_room(&room(&room_id(n))))
            .map(Into::into)
            .collect();
    assert_eq!(items.len(), ROOMS);
    let diffs =
        futures_util::stream::iter(vec![vec![matrix_sdk_ui::eyeball_im::VectorDiff::Reset {
            values: items,
        }]])
        .chain(futures_util::stream::pending());
    let loading = futures_util::stream::iter(vec![
        matrix_sdk_ui::room_list_service::RoomListLoadingState::Loaded {
            maximum_number_of_rooms: Some(ROOMS as u32),
        },
    ])
    .chain(futures_util::stream::pending());
    let (producer_merger, producer_board, producer_ready) =
        (merger.clone(), Arc::clone(&board), ready.clone());
    let producer = tokio::spawn(async move {
        let (notify, kinds, icons) = (
            crate::notify::NotifyConfig::new(false),
            AgentKinds::default(),
            AgentIcons::default(),
        );
        super::super::drive_inbox(
            diffs,
            matrix_sdk_ui::room_list_service::RoomListLoadingState::NotLoaded,
            loading,
            super::super::InboxRows {
                merger: producer_merger,
                account_id: ACCOUNT,
                notify: &notify,
                kinds: &kinds,
                launch_ready: &producer_ready,
                icons: &icons,
                board: &producer_board,
                worker: Some((due, changed)),
                keep: None,
                owner: super::super::inbox_owners::Owner::live(ACCOUNT),
            },
        )
        .await;
    });

    assert!(
        until(Duration::from_secs(2), || last_window(&agents).len()
            == ROOMS)
        .await,
        "every agent row is drawn while no room's history has come back"
    );
    let first = last_window(&agents);
    assert!(
        first
            .iter()
            .all(|row| row.agent_attention == Some(AgentAttentionVm::Unknown)),
        "a room not read yet is Unknown, never nothing pending"
    );
    assert!(
        ready.ready().is_some(),
        "the launch's readiness waited on no read"
    );
    assert!(
        until(Duration::from_secs(2), || device.asked_messages() > 0).await,
        "the worker is reading: its first read waits on the server"
    );

    device.set(|server| server.messages = Messages::Serve);
    assert!(
        until(Duration::from_secs(20), || {
            last_window(&agents).iter().any(|row| {
                row.room_id == CONVERSATION
                    && row.agent_attention == Some(AgentAttentionVm::Working)
            })
        })
        .await,
        "the worker's reading redraws the row"
    );
    producer.abort();
    worker.abort();
}

/// 102.5 #2, #3 (R453, R468): a first pass reads each of the 30 rooms once;
/// a room read in part makes the conversation whose questions it may hold
/// *Unknown*. A change in that room alone reads that room alone, and the
/// conversation's row follows from the kept readings without being read
/// again; a claim renewal elsewhere reads only its room, and a changed seat
/// reads none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pass_reads_each_changed_room_once_and_places_questions_in_memory() {
    let device = Device::new(Messages::Serve).await;
    let failing = "r07";
    device.set(|server| {
        server.failing.insert(failing.to_owned());
    });
    let board: Arc<AttentionBoard> = Arc::default();
    let worker = device.worker(&board);
    let mut kept = Kept::default();

    worker
        .pass(
            &mut kept,
            Todo {
                read_all: true,
                ..Todo::default()
            },
        )
        .await;
    assert_eq!(kept.reads(), ROOMS, "each room read once");
    let conversation = room(CONVERSATION);
    assert_eq!(
        board.said(ACCOUNT, &conversation),
        Some(AgentAttentionVm::Unknown),
        "a room a question may wait in was read in part"
    );

    device.set(|server| {
        server.failing.clear();
    });
    let changed = worker.pass(&mut kept, rooms_todo(&["!r07:h"])).await;
    assert_eq!(kept.reads(), ROOMS + 1, "only the changed room is read");
    assert_eq!(
        board.said(ACCOUNT, &conversation),
        Some(AgentAttentionVm::Working),
        "the conversation's row follows without its room being read again"
    );
    assert!(changed.contains(&conversation));

    let changed = worker.pass(&mut kept, rooms_todo(&["!r12:h"])).await;
    assert_eq!(kept.reads(), ROOMS + 2, "a renewal reads its own room");
    assert!(
        changed.is_empty(),
        "and redraws nothing that did not change"
    );

    worker
        .pass(
            &mut kept,
            Todo {
                rows_all: true,
                ..Todo::default()
            },
        )
        .await;
    assert_eq!(kept.reads(), ROOMS + 2, "a changed seat reads no room");
}

/// 102.5 #2 (R501, review R102L5-03): thirty conversations of one proxy
/// beside ten rooms it delegated. A pass reads each room once and places
/// every folded question by visiting each (proxy, room) pair at most once —
/// never every room once per conversation — and so does a renewal's pass;
/// each conversation's row is placed from the readings.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn many_conversations_of_one_proxy_are_placed_in_one_visit_per_room() {
    const CONVERSATIONS: usize = 30;
    const DELEGATED: usize = 10;
    let rooms = CONVERSATIONS + DELEGATED;
    let device = Device::with(sync_of(CONVERSATIONS, DELEGATED), Messages::Serve).await;
    let board: Arc<AttentionBoard> = Arc::default();
    let worker = device.worker(&board);
    let mut kept = Kept::default();

    worker
        .pass(
            &mut kept,
            Todo {
                read_all: true,
                ..Todo::default()
            },
        )
        .await;
    assert_eq!(kept.reads(), rooms, "each room read once");
    assert!(
        kept.visits() <= rooms,
        "{} placement visits for {rooms} rooms and one proxy",
        kept.visits()
    );
    for n in 0..CONVERSATIONS {
        assert_eq!(
            board.said(ACCOUNT, &room(&room_id(n))),
            Some(AgentAttentionVm::Working),
            "conversation {n}"
        );
    }

    let before = kept.visits();
    worker
        .pass(&mut kept, rooms_todo(&[&room_id(CONVERSATIONS + 3)]))
        .await;
    assert_eq!(kept.reads(), rooms + 1, "a renewal reads its own room");
    assert!(
        kept.visits() - before <= rooms,
        "{} placement visits for a renewal",
        kept.visits() - before
    );
}

/// The note of the last Agents batch.
fn last_note(agents: &Batches) -> Option<String> {
    agents
        .lock()
        .expect("agents")
        .last()
        .and_then(|batch| batch.note.clone())
}

/// 102.5 #1 (R501, R381; review R102L5-04): a kept list that cannot be
/// read, or cannot be kept, is said above the Agents list, not only logged;
/// the sentence of a list not kept ends once a later snapshot is kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_list_that_cannot_be_read_or_kept_is_said_above_the_list() {
    let dir = tempfile::tempdir().expect("dir");
    let db = dir.path().join("keeper.db");
    std::fs::create_dir(&db).expect("a directory where keeper.db goes");
    let store = ListRowStore::new(dir.path());
    let board = AttentionBoard::default();
    let (merger, agents) = merger();
    merger.register_account(ACCOUNT, 0).await;
    let lives = crate::account::lifetime::AccountLifetimes::default();
    let writer = store
        .writer(&lives.begin(ACCOUNT).expect("activated"))
        .expect("writer");

    draw_kept_rows(&store, &board, &merger, ACCOUNT).await;
    let unread = AgentListTrouble::KeptUnread.sentence();
    assert_eq!(last_note(&agents).as_deref(), Some(unread));

    keep_rows(&writer, &merger, ACCOUNT, Vec::new())
        .await
        .expect("join");
    assert_eq!(
        last_note(&agents),
        Some(format!(
            "{unread} {}",
            AgentListTrouble::KeptUnwritten.sentence()
        ))
    );

    std::fs::remove_dir(&db).expect("keeper.db can be made again");
    keep_rows(&writer, &merger, ACCOUNT, Vec::new())
        .await
        .expect("join");
    assert_eq!(
        last_note(&agents).as_deref(),
        Some(unread),
        "kept again; the account is still not live"
    );
}

/// A kept Agents row of Nixi's in `room`, last active at `at`.
fn kept_row(room: &str, at: i64, attention: Option<AgentAttentionVm>) -> RoomVm {
    RoomVm {
        room_id: room.to_owned(),
        display_name: format!("Nixi — {room}"),
        last_message: None,
        timestamp: Some(at),
        avatar_url: None,
        is_unread: false,
        mention_count: 0,
        is_archived: false,
        is_favourite: false,
        is_space: false,
        network: None,
        network_id: None,
        mute_state: crate::vm::MuteState::None,
        agent_room: Some(crate::agents::room::AgentRoomKindVm::Proxy),
        agent_avatar: None,
        agent_mark: Some("N".to_owned()),
        agent_attention: attention,
    }
}

/// 102.5 #1, #5: the rows kept from the last run are the Agents window's
/// first emission, from a warm `keeper.db` in under 100 ms, with their
/// states on the board — a kept *nothing pending* as *Unknown* (R436).
#[tokio::test]
async fn the_kept_rows_are_drawn_first_and_fast() {
    let dir = tempfile::tempdir().expect("dir");
    let store = ListRowStore::new(dir.path());
    let rows: Vec<RoomVm> = (0..ROOMS)
        .map(|n| {
            kept_row(
                &room_id(n),
                n as i64,
                (n == 3).then_some(AgentAttentionVm::NeedsYou),
            )
        })
        .collect();
    store.replace(ACCOUNT, &rows).expect("keep");
    // Warm: the database file exists and has been read once.
    assert_eq!(store.of(ACCOUNT).expect("read").len(), ROOMS);

    let board = AttentionBoard::default();
    let (merger, agents) = merger();
    merger.register_account(ACCOUNT, 0).await;
    let start = Instant::now();
    draw_kept_rows(&store, &board, &merger, ACCOUNT).await;
    let took = start.elapsed();

    let window = last_window(&agents);
    assert_eq!(window.len(), ROOMS, "the kept rows are drawn");
    assert!(took < Duration::from_millis(100), "drawn in {took:?}");
    assert_eq!(
        board.said(ACCOUNT, &room(&room_id(3))),
        Some(AgentAttentionVm::NeedsYou)
    );
    assert_eq!(
        board.said(ACCOUNT, &room(&room_id(4))),
        Some(AgentAttentionVm::Unknown)
    );
}

/// The seat finder's schedule (R416, R468): a room whose status is not
/// found is looked at again later, at most five times in its first hour
/// and once an hour after.
#[test]
fn the_finder_looks_again_later_and_at_most_hourly() {
    let hour = Duration::from_secs(3600);
    let mut at = Duration::ZERO;
    let mut in_first_hour = 0;
    for looks in 0..10 {
        at += finder_backoff(looks);
        if at < hour {
            in_first_hour += 1;
        }
    }
    assert!(
        in_first_hour <= 5,
        "{in_first_hour} looks in the first hour"
    );
    assert!(
        finder_backoff(1) > Duration::ZERO,
        "a look is never repeated at once"
    );
    assert_eq!(finder_backoff(50), hour, "then hourly");
}

/// A room whose status lies `deep` pages back: its history is pages of
/// fifty notices, the status on page `deep`.
fn buried_history(seen: &Request, deep: usize) -> Reply {
    let page: usize = seen
        .query
        .get("from")
        .and_then(|from| from.strip_prefix('q'))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let mut chunk: Vec<Value> = (0..50)
        .map(|n| {
            json!({
                "type": "m.room.message", "sender": NIXI,
                "event_id": format!("$p{page}n{n}:h"),
                "origin_server_ts": 1_000_000 - (page * 50 + n) as u64,
                "content": { "msgtype": "m.notice", "body": "claim" },
            })
        })
        .collect();
    if page == deep {
        chunk.push(status(NIXI, "conversation", "idle", 9));
    }
    Reply::json(
        200,
        &json!({ "chunk": chunk, "start": format!("q{page}"), "end": format!("q{}", page + 1) })
            .to_string(),
    )
}

/// 102.5 #3 (R416, R468): a listing never asks the server, even for a room
/// with no seat and no status in its cache; the finder looks further back
/// at each look, going on from where the last stopped, and keeps the seat
/// it finds — which a listing then reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_never_asks_the_server_and_the_finder_goes_further_each_look() {
    let deep = FINDER_PAGES + 4;
    let fake = testing::serve(move |seen| {
        let path = seen.path.as_str();
        if path.ends_with("/sync") {
            let mut join = serde_json::Map::new();
            join.insert(
                CONVERSATION.to_owned(),
                json!({
                    "state": { "events": room_state(NIXI) },
                    "timeline": { "events": [], "limited": true, "prev_batch": "q0" },
                }),
            );
            return Reply::json(
                200,
                &json!({ "next_batch": "s1", "rooms": { "join": join } }).to_string(),
            );
        }
        if path.ends_with("/messages") {
            return buried_history(seen, deep);
        }
        if path.ends_with("/versions") {
            return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
        }
        if path.ends_with("/keys/upload") {
            return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
        }
        if path.ends_with("/keys/query") {
            return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
        }
        Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
    });
    let client = Client::builder()
        .homeserver_url(&fake.base)
        .build()
        .await
        .expect("client");
    let session: matrix_sdk::authentication::matrix::MatrixSession =
        serde_json::from_value(json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }))
            .expect("session");
    client.restore_session(session).await.expect("restore");
    client.event_cache().subscribe().expect("event cache");
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("sync");
    let messages = || {
        fake.requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages"))
            .map(|seen| seen.query.get("from").cloned())
            .collect::<Vec<_>>()
    };
    let joined = client.get_room(&room(CONVERSATION)).expect("joined");
    let kinds = AgentKinds::default();

    let row = proxy_row(joined.clone(), &kinds).await;
    assert_eq!(row.kind, None, "out of the cache's reach");
    assert!(messages().is_empty(), "a listing asked the server");

    let first = HeaderReader::find(joined.clone(), &kinds, None, FINDER_PAGES).await;
    let Found::Further(resume) = first else {
        panic!("the first look stops at its budget: {first:?}");
    };
    assert_eq!(messages().len(), FINDER_PAGES);
    assert!(kinds.seat(joined.own_user_id(), joined.room_id()).is_none());

    let second =
        HeaderReader::find(joined.clone(), &kinds, Some(resume.clone()), FINDER_PAGES).await;
    assert_eq!(second, Found::Seat);
    assert_eq!(
        messages()[FINDER_PAGES],
        Some(resume.from),
        "the second look goes on where the first stopped"
    );
    let asked = messages().len();
    let row = proxy_row(joined, &kinds).await;
    assert_eq!(row.kind, Some(SessionKind::Conversation), "the kept seat");
    assert_eq!(
        messages().len(),
        asked,
        "read from the seat, not the server"
    );
}

/// The account the manager tests sign in, and one whose session cannot be
/// restored.
const LIVE: &str = "acct-live";
const BROKEN: &str = "acct-broken";

/// The person's session as keeper keeps it in the keychain.
fn session_blob() -> String {
    format!(r#"{{"kind":"Password","user_id":"{ME}","device_id":"MAC","access_token":"t"}}"#)
}

/// What a homeserver answers the sliding sync an account's sync service
/// asks for: nothing — the manager tests sync by hand.
fn sliding_unanswered(seen: &Request) -> Option<Reply> {
    seen.path
        .contains("msc3575")
        .then(|| Reply::json(404, r#"{"errcode":"M_UNRECOGNIZED","error":"no"}"#))
}

/// Wait up to `bound` for `ready` to resolve true.
async fn eventually<F, Ready>(bound: Duration, mut ready: F) -> bool
where
    F: FnMut() -> Ready,
    Ready: Future<Output = bool>,
{
    let start = Instant::now();
    while start.elapsed() < bound {
        if ready().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ready().await
}

/// An account manager on a scratch platform, as the shell holds one.
struct Signed {
    platform: Arc<FakePlatform>,
    manager: super::super::AccountManager,
}

impl Signed {
    fn new() -> Signed {
        let platform = Arc::new(FakePlatform::default());
        let manager = super::super::AccountManager::new(platform.as_ref(), &platform.data_dir);
        Signed { platform, manager }
    }

    fn platform(&self) -> Arc<dyn crate::platform::Platform> {
        self.platform.clone()
    }

    /// Sign `account` in as the person against `homeserver`, `session` in
    /// the keychain.
    fn sign_in(&self, account: &str, homeserver: &str, session: &str) {
        self.sign_in_at(account, homeserver, session, 1);
    }

    /// [`Self::sign_in`], the account added at `created`: accounts are
    /// started in the order they were added.
    fn sign_in_at(&self, account: &str, homeserver: &str, session: &str, created: i64) {
        use crate::platform::Platform;
        crate::registry::insert_account(
            &self.platform.data_dir,
            account,
            ME,
            homeserver,
            "MAC",
            created,
            0,
            "password",
        )
        .expect("registry");
        self.platform
            .keychain_set(&crate::auth::session_keychain_key(account), session)
            .expect("keychain");
    }

    /// Inbox producer tasks of `account` running now, whichever
    /// subscription started them.
    fn running(&self, account: &str) -> usize {
        self.manager.inbox_owners.running(account)
    }

    /// How many rooms the palette finds now.
    async fn palette_rooms(&self) -> usize {
        self.manager.palette.lock().await.len()
    }

    /// The inbox subscription that owns the merged inbox now.
    async fn inbox_owner(&self) -> Option<u64> {
        self.manager
            .inbox
            .lock()
            .await
            .as_ref()
            .map(|handle| handle.subscription_id)
    }

    /// What one `/sync` of `homeserver` brings, already in `account`'s
    /// store, as a launch finds it.
    async fn warm(&self, account: &str, homeserver: &str) {
        let sdk = self
            .platform
            .data_dir
            .join("accounts")
            .join(account)
            .join("sdk");
        let client = Client::builder()
            .homeserver_url(homeserver)
            .sqlite_store(&sdk, None)
            .build()
            .await
            .expect("client");
        let session: matrix_sdk::authentication::matrix::MatrixSession = serde_json::from_value(
            json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }),
        )
        .expect("session");
        client.restore_session(session).await.expect("restore");
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
    }

    /// Subscribe the merged inbox, as the Chats view does, its Agents window
    /// captured into `agents`.
    async fn subscribe(&self, agents: &Batches) -> u64 {
        let into = Arc::clone(agents);
        self.manager
            .subscribe_inbox(
                &self.platform(),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: InboxBatch| true),
                Box::new(move |batch: InboxBatch| {
                    into.lock().expect("agents").push(batch);
                    true
                }),
                Box::new(|_: SpacesSnapshot| true),
                Box::new(|_: NetworksSnapshot| true),
            )
            .await
            .expect("subscribed")
    }

    /// `account`'s live client, its sync service stopped: these tests sync
    /// by hand.
    async fn live(&self, account: &str) -> Client {
        let (client, sync) = {
            let accounts = self.manager.accounts.lock().await;
            let handle = accounts.get(account).expect("live");
            (handle.client.clone(), Arc::clone(&handle.sync))
        };
        sync.stop().await;
        client
    }

    /// What the notes dock says of `room` on `account` (`agentRoomsList`):
    /// `None` when it does not list it.
    async fn dock(&self, account: &str, room: &str) -> Option<Option<AgentAttentionVm>> {
        self.manager
            .agent_rooms_attended(account)
            .await
            .into_iter()
            .find(|vm| vm.room_id == room)
            .map(|vm| vm.attention)
    }
}

/// 102.5 #1 (R501, review R102L5-01; DW-1550 closed): a snapshot of an
/// account's Agents rows handed over before its sign-out, and still queued
/// when the sign-out completes, lands after it and brings no row back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_row_write_queued_before_a_sign_out_brings_no_row_back() {
    let signed = Signed::new();
    signed.sign_in(LIVE, "https://example.invalid", &session_blob());
    let store = signed.manager.agent_list_rows.clone();
    store
        .replace(LIVE, &[kept_row("!kept:h", 1, None)])
        .expect("kept from the last run");
    let life = signed.manager.lifetimes.begin(LIVE).expect("activated");
    let writer = store.writer(&life).expect("a live account's writer");

    let held = store.hold_writes();
    let queued = writer.keep(vec![kept_row("!late:h", 2, None)]);
    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    assert!(store.of(LIVE).expect("read").is_empty(), "the rows went");
    drop(held);

    assert_eq!(queued.await.expect("join").expect("keep"), Keep::Fenced);
    assert!(
        store.of(LIVE).expect("read").is_empty(),
        "the write queued before the sign-out brought no row back"
    );
}

/// R501, R381, R516 (reviews R102L5-01, R102L6): a sign-out that cannot
/// remove the account's kept rows — the fault in that one table, nothing
/// else — says so and stops before the session goes, the rows still there;
/// once the fault is gone, signing out again removes the rows and the
/// session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sign_out_that_cannot_remove_the_kept_rows_says_so_and_the_next_one_does() {
    use crate::platform::Platform;
    let signed = Signed::new();
    signed.sign_in(LIVE, "https://example.invalid", &session_blob());
    let store = signed.manager.agent_list_rows.clone();
    store
        .replace(LIVE, &[kept_row("!kept:h", 1, None)])
        .expect("kept from the last run");
    let db = rusqlite::Connection::open(signed.platform.data_dir.join("keeper.db")).expect("db");
    db.execute(
        "CREATE TRIGGER kept_rows_stay BEFORE DELETE ON agent_list_rows \
         BEGIN SELECT RAISE(ABORT, 'the disk refused'); END",
        [],
    )
    .expect("a fault in removing the kept rows alone");
    let session = || {
        signed
            .platform
            .keychain_get(&crate::auth::session_keychain_key(LIVE))
            .expect("keychain")
    };
    let kept = || -> Vec<String> {
        store
            .of(LIVE)
            .expect("read")
            .into_iter()
            .map(|row| row.room_id)
            .collect()
    };

    let refused = signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect_err("the rows could not be removed");
    assert!(
        matches!(&refused, CoreError::Internal(said) if said == KEPT_ROWS_NOT_REMOVED),
        "{refused:?}"
    );
    assert!(session().is_some(), "still signed in");
    assert_eq!(kept(), ["!kept:h"], "the rows are still kept");

    db.execute("DROP TRIGGER kept_rows_stay", [])
        .expect("the fault is gone");
    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out the second time");
    assert!(kept().is_empty(), "the rows went");
    assert!(session().is_none(), "the session went");
    assert!(
        crate::registry::get_account(&signed.platform.data_dir, LIVE)
            .expect("registry")
            .is_none(),
        "the account went"
    );
}

/// A homeserver for the lifecycle tests: the first sync's 30 rooms, no
/// sliding sync.
fn plain_homeserver() -> testing::Fake {
    queued_homeserver().0
}

/// [`plain_homeserver`], and where a test queues what its next `/sync`
/// brings.
fn queued_homeserver() -> (testing::Fake, Shared) {
    let shared = server(first_sync(), Messages::Serve);
    let handler = Arc::clone(&shared);
    let fake = testing::serve(move |seen| {
        sliding_unanswered(seen).unwrap_or_else(|| answer(&handler, seen))
    });
    (fake, shared)
}

/// A second account of the person's, added after [`LIVE`].
const SECOND: &str = "acct-second";

/// Subscribe the merged inbox on its own task.
fn subscribing(signed: &Arc<Signed>, agents: &Batches) -> tokio::task::JoinHandle<u64> {
    let (signed, agents) = (Arc::clone(signed), Arc::clone(agents));
    tokio::spawn(async move { signed.subscribe(&agents).await })
}

/// What a subscription registered of `account` once published: whether
/// any producer of it is registered or runs, and whether its rows are in
/// the Agents window last drawn.
async fn registered(signed: &Signed, agents: &Batches, account: &str) -> (bool, bool) {
    let producing =
        signed.manager.inbox_owners.registered(account) > 0 || signed.running(account) > 0;
    let drawn = last_window(agents)
        .iter()
        .any(|row| row.account_id == account);
    (producing, drawn)
}

/// R516 (review R102L6-01): a subscription holding an account's room list,
/// its writer not taken yet, while the account signs out: once it goes on,
/// it gets no writer, registers no producer and draws none of its rows, and
/// no kept row comes back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscription_that_resumes_after_a_sign_out_registers_nothing_of_it() {
    let fake = plain_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in(LIVE, &fake.base, &session_blob());
    let store = signed.manager.agent_list_rows.clone();
    store
        .replace(LIVE, &[kept_row(CONVERSATION, 1, None)])
        .expect("kept from the last run");
    let paused = signed.manager.lifetimes.pause_at(Pause::TakingWriter);
    let agents = Batches::default();
    let subscription = subscribing(&signed, &agents);
    paused
        .reached
        .await
        .expect("the subscription holds the room list");

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    let _ = paused.release.send(());
    subscription.await.expect("subscribed");

    assert_eq!(
        registered(&signed, &agents, LIVE).await,
        (false, false),
        "(a producer, its rows drawn) of the signed-out account"
    );
    tokio::time::sleep(crate::account::KEEP_SETTLE + Duration::from_secs(1)).await;
    assert!(
        store.of(LIVE).expect("read").is_empty(),
        "a kept row came back"
    );
}

/// R516 (review R102L6-01): a subscription that built an account's
/// producers and has not published them when the account signs out stops
/// them as it publishes: none runs, none of the rows is drawn or kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscription_that_publishes_after_a_sign_out_stops_what_it_built() {
    let fake = plain_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in(LIVE, &fake.base, &session_blob());
    let paused = signed.manager.lifetimes.pause_at(Pause::Publishing);
    let agents = Batches::default();
    let subscription = subscribing(&signed, &agents);
    paused
        .reached
        .await
        .expect("the subscription built its producers");

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    let _ = paused.release.send(());
    subscription.await.expect("subscribed");

    assert_eq!(
        registered(&signed, &agents, LIVE).await,
        (false, false),
        "(a producer, its rows drawn) of the signed-out account"
    );
    tokio::time::sleep(crate::account::KEEP_SETTLE + Duration::from_secs(1)).await;
    assert!(
        signed
            .manager
            .agent_list_rows
            .of(LIVE)
            .expect("read")
            .is_empty(),
        "a kept row came back"
    );
}

/// R519 (review R102L7-01): a subscription that started an account's
/// producers and waits on another account's room list when the first
/// account signs out. The sign-out stops and awaits every producer of it
/// before it returns — before the subscription goes on — and takes its
/// rooms out of the palette; once the subscription goes on, nothing of the
/// account runs or comes back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_out_while_the_inbox_waits_on_another_account_stops_every_producer_of_it() {
    let fake = plain_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, &session_blob(), 1);
    signed.sign_in_at(SECOND, &fake.base, &session_blob(), 2);
    signed.warm(LIVE, &fake.base).await;
    let waiting = signed
        .manager
        .lifetimes
        .pause_at_for(Pause::Acquiring, SECOND);
    let agents = Batches::default();
    let subscription = subscribing(&signed, &agents);
    waiting
        .reached
        .await
        .expect("the subscription waits on the second account");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the first account's three producers run, its rooms in the palette: {} running",
        signed.running(LIVE)
    );

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    assert_eq!(
        (signed.running(LIVE), signed.palette_rooms().await),
        (0, 0),
        "(its producers running, its rooms in the palette) once the sign-out returned"
    );
    assert_eq!(registered(&signed, &agents, LIVE).await, (false, false));

    let _ = waiting.release.send(());
    subscription.await.expect("subscribed");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(SECOND) == 3
        })
        .await,
        "the subscription went on with the other account"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        registered(&signed, &agents, LIVE).await,
        (false, false),
        "(a producer, its rows drawn) of the signed-out account"
    );
    assert_eq!(
        signed.palette_rooms().await,
        0,
        "the palette finds a room of the signed-out account"
    );
}

/// R519 (review R102L7-01): two inbox subscriptions overlap and the older
/// one finishes last. The newer one stopped and awaited the older one's
/// producers when it began, and stays the owner; a teardown then stops
/// every producer there is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_older_inbox_subscription_finishing_last_leaves_the_newer_one_in_charge() {
    let fake = plain_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, &session_blob(), 1);
    signed.sign_in_at(SECOND, &fake.base, &session_blob(), 2);
    signed.warm(LIVE, &fake.base).await;
    let waiting = signed
        .manager
        .lifetimes
        .pause_at_for(Pause::Acquiring, SECOND);
    let (older_agents, newer_agents) = (Batches::default(), Batches::default());
    let older = subscribing(&signed, &older_agents);
    waiting
        .reached
        .await
        .expect("the older subscription waits on the second account");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3
        })
        .await,
        "the older subscription's producers of the first account run"
    );

    let newer = signed.subscribe(&newer_agents).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3
        })
        .await,
        "the older subscription's producers were stopped: only the newer one's run, not {}",
        signed.running(LIVE)
    );
    let _ = waiting.release.send(());
    let older = older.await.expect("the older subscription finished");

    assert_ne!(older, newer);
    assert_eq!(
        signed.inbox_owner().await,
        Some(newer),
        "the newer subscription owns the inbox"
    );
    assert!(
        eventually(Duration::from_secs(5), || async {
            (signed.running(LIVE), signed.running(SECOND)) == (3, 3)
        })
        .await,
        "only the newer subscription's producers run: ({}, {})",
        signed.running(LIVE),
        signed.running(SECOND)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        (signed.running(LIVE), signed.running(SECOND)),
        (3, 3),
        "the older subscription started nothing more"
    );

    signed.manager.shutdown(LIVE).await;
    signed.manager.shutdown(SECOND).await;
    assert_eq!(
        (signed.running(LIVE), signed.running(SECOND)),
        (0, 0),
        "a teardown stops every producer of its account"
    );
}

/// R519 (review R102L7-02): an old subscription held while its account
/// shut down, came back, and was subscribed afresh. When the old one
/// finishes, its token is refused and it cleans up only its own: the fresh
/// subscription keeps the inbox, its producers, the launch's readiness and
/// the palette.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_old_subscription_finishing_after_a_new_activation_leaves_the_new_one_intact() {
    let fake = plain_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in(LIVE, &fake.base, &session_blob());
    signed.warm(LIVE, &fake.base).await;
    let paused = signed.manager.lifetimes.pause_at(Pause::TakingWriter);
    let (old_agents, fresh_agents) = (Batches::default(), Batches::default());
    let old = subscribing(&signed, &old_agents);
    paused
        .reached
        .await
        .expect("the old subscription holds the room list");

    signed.manager.shutdown(LIVE).await;
    let fresh = signed.subscribe(&fresh_agents).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the fresh subscription's producers run, its rooms in the palette"
    );
    let ready = signed.manager.launch_ready.ready();
    let mut readiness = signed.manager.launch_ready.subscribe();
    readiness.mark_unchanged();

    let _ = paused.release.send(());
    old.await.expect("the old subscription finished");
    assert_eq!(signed.inbox_owner().await, Some(fresh));
    assert!(
        !readiness.has_changed().expect("readiness"),
        "the old subscription changed the launch's readiness"
    );
    assert_eq!(signed.manager.launch_ready.ready(), ready);
    assert_eq!(
        signed.palette_rooms().await,
        ROOMS,
        "the fresh subscription's rooms left the palette"
    );
    assert_eq!(signed.running(LIVE), 3, "the fresh producers run");
}

/// R519 (review R102L7-01): between the moment an account's teardown
/// begins and the moment its producers are stopped, a producer that hears
/// of a change has no effect and stops: the palette does not learn a room
/// of an account that is going.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_producer_has_no_effect_once_its_account_began_closing() {
    let (fake, homeserver) = queued_homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in(LIVE, &fake.base, &session_blob());
    signed.warm(LIVE, &fake.base).await;
    let agents = Batches::default();
    let _chats = signed.subscribe(&agents).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the producers run, the rooms in the palette"
    );
    let client = signed.live(LIVE).await;

    let stopping = signed.manager.lifetimes.pause_at(Pause::StoppingProducers);
    let shutdown = {
        let signed = Arc::clone(&signed);
        tokio::spawn(async move { signed.manager.shutdown(LIVE).await })
    };
    stopping
        .reached
        .await
        .expect("the teardown began, its producers not stopped yet");
    homeserver.0.lock().expect("server").syncs.push_back(json!({
        "next_batch": "s2",
        "rooms": { "join": { "!new:h": {
            "state": { "events": [
                state("m.room.create", "", ME, json!({ "creator": ME, "room_version": "11" }), 90),
                state("m.room.member", ME, ME, json!({ "membership": "join" }), 91),
            ]},
            "timeline": { "events": [], "limited": false },
        }}},
    }));
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("a new room");
    assert!(client.get_room(&room("!new:h")).is_some(), "the room came");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) <= 1
        })
        .await,
        "the producers that heard of the room went on: {} running",
        signed.running(LIVE)
    );
    assert_eq!(
        signed.palette_rooms().await,
        ROOMS,
        "the palette learned a room of an account that is going"
    );

    let _ = stopping.release.send(());
    shutdown.await.expect("shut down");
    assert_eq!(signed.running(LIVE), 0);
    assert_eq!(signed.palette_rooms().await, 0);
}

/// A homeserver whose one session room's status lies deeper than a look of
/// the finder reaches: every look asks `/messages`.
fn buried_homeserver() -> testing::Fake {
    let deep = FINDER_PAGES + 4;
    testing::serve(move |seen| {
        if let Some(reply) = sliding_unanswered(seen) {
            return reply;
        }
        let path = seen.path.as_str();
        if path.ends_with("/sync") {
            let mut join = serde_json::Map::new();
            join.insert(
                CONVERSATION.to_owned(),
                json!({
                    "state": { "events": room_state(NIXI) },
                    "timeline": { "events": [], "limited": true, "prev_batch": "q0" },
                }),
            );
            return Reply::json(
                200,
                &json!({ "next_batch": "s1", "rooms": { "join": join } }).to_string(),
            );
        }
        if path.ends_with("/messages") {
            return buried_history(seen, deep);
        }
        if path.ends_with("/versions") {
            return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
        }
        if path.ends_with("/keys/upload") {
            return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
        }
        if path.ends_with("/keys/query") {
            return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
        }
        Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
    })
}

/// R516 (review R102L6-02): a shutdown that begins while an activation
/// holds the accounts lock leaves no list worker or seat finder of it, and
/// nothing of it asks the server after — whether the activation checked
/// its lifetime before the shutdown began (it publishes, and the teardown
/// takes it out) or after (it is refused). A new activation then starts
/// one worker and finder again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_racing_an_activation_leaves_no_worker_behind() {
    let fake = buried_homeserver();
    let asked = || {
        fake.requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages"))
            .count()
    };
    let signed = Arc::new(Signed::new());
    signed.sign_in(LIVE, &fake.base, &session_blob());
    signed.warm(LIVE, &fake.base).await;
    let lifetimes = signed.manager.lifetimes.clone();
    let activating = || {
        let signed = Arc::clone(&signed);
        tokio::spawn(async move {
            signed
                .manager
                .ensure_live(&signed.platform(), LIVE)
                .await
                .map(drop)
        })
    };
    let shutting = || {
        let signed = Arc::clone(&signed);
        tokio::spawn(async move { signed.manager.shutdown(LIVE).await })
    };
    let quiet = || async {
        // A request already sent when the finder stopped lands first.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = asked();
        tokio::time::sleep(Duration::from_secs(1)).await;
        asked() == before
    };

    // The activation checked its lifetime; its worker is not started yet
    // when the shutdown begins and comes to take the account out.
    let started = lifetimes.pause_at(Pause::StartingList);
    let detaching = lifetimes.pause_at(Pause::Detaching);
    let activation = activating();
    started
        .reached
        .await
        .expect("the activation checked its lifetime");
    let shutdown = shutting();
    detaching
        .reached
        .await
        .expect("the shutdown is about to take the account out");
    let _ = detaching.release.send(());
    let _ = started.release.send(());
    activation
        .await
        .expect("join")
        .expect("published before the teardown took the lock");
    shutdown.await.expect("shut down");
    assert!(
        signed.manager.agent_lists.attach(LIVE).is_none(),
        "a worker and finder outlived the teardown"
    );
    assert!(!signed.manager.accounts.lock().await.contains_key(LIVE));
    assert!(
        quiet().await,
        "the old finder asked the server after its teardown"
    );

    // The activation had not checked its lifetime when the shutdown began.
    let checking = lifetimes.pause_at(Pause::CheckingLife);
    let activation = activating();
    checking
        .reached
        .await
        .expect("the activation built the account");
    let shutdown = shutting();
    assert!(
        until(Duration::from_secs(5), || lifetimes.is_closing(LIVE)).await,
        "the shutdown began"
    );
    let _ = checking.release.send(());
    let refused = activation.await.expect("join");
    assert!(
        refused.is_err(),
        "an activation published into a closing account"
    );
    shutdown.await.expect("shut down");
    assert!(
        signed.manager.agent_lists.attach(LIVE).is_none(),
        "a worker and finder started in a closing account"
    );
    assert!(
        quiet().await,
        "something of the refused activation asked the server"
    );

    // A new activation starts the account's one worker and finder.
    let before = asked();
    activating().await.expect("join").expect("activated again");
    assert!(signed.manager.agent_lists.attach(LIVE).is_some());
    assert!(
        until(Duration::from_secs(30), || asked() > before).await,
        "the new finder looks"
    );
}

/// R516 (review R102L6-03): a room left while its next look is pending is
/// no longer looked for, and its passed deadline does not bring the next
/// round at once — the finder waits; a room still to look at keeps its
/// look: where it stopped and its backoff.
#[tokio::test]
async fn the_finder_waits_after_a_room_with_a_pending_look_was_left() {
    let left_room: Arc<Mutex<Option<String>>> = Arc::default();
    let leave = Arc::clone(&left_room);
    let deep = FINDER_PAGES + 4;
    let fake = testing::serve(move |seen| {
        let path = seen.path.as_str();
        if path.ends_with("/sync") {
            if let Some(left) = leave.lock().expect("left").clone() {
                let mut gone = serde_json::Map::new();
                gone.insert(
                    left,
                    json!({ "timeline": { "events": [state(
                        "m.room.member", ME, ME, json!({ "membership": "leave" }), 90,
                    )], "limited": false } }),
                );
                return Reply::json(
                    200,
                    &json!({ "next_batch": "s2", "rooms": { "leave": gone } }).to_string(),
                );
            }
            let mut join = serde_json::Map::new();
            for n in 0..2 {
                join.insert(
                    room_id(n),
                    json!({
                        "state": { "events": room_state(NIXI) },
                        "timeline": { "events": [], "limited": true, "prev_batch": "q0" },
                    }),
                );
            }
            return Reply::json(
                200,
                &json!({ "next_batch": "s1", "rooms": { "join": join } }).to_string(),
            );
        }
        if path.ends_with("/messages") {
            return buried_history(seen, deep);
        }
        if path.ends_with("/versions") {
            return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
        }
        if path.ends_with("/keys/upload") {
            return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
        }
        if path.ends_with("/keys/query") {
            return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
        }
        Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
    });
    let client = Client::builder()
        .homeserver_url(&fake.base)
        .build()
        .await
        .expect("client");
    let session: matrix_sdk::authentication::matrix::MatrixSession =
        serde_json::from_value(json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }))
            .expect("session");
    client.restore_session(session).await.expect("restore");
    client.event_cache().subscribe().expect("event cache");
    let sync = || client.sync_once(SyncSettings::default().timeout(Duration::ZERO));
    sync().await.expect("sync");
    let asked = || {
        fake.requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages"))
            .count()
    };

    // The first round looks at both rooms; neither status is found, so each
    // is looked at again a minute after its look.
    let mut finder = SeatFinder::new(client.clone(), Arc::new(AgentKinds::default()));
    finder.look().await.expect("a user");
    assert_eq!(asked(), 2 * FINDER_PAGES, "both rooms looked at");
    let mut pending: Vec<(OwnedRoomId, tokio::time::Instant)> = finder
        .looks
        .iter()
        .map(|(id, look)| (id.clone(), look.at))
        .collect();
    pending.sort_by_key(|(_, at)| *at);
    let [(first, first_at), (second, second_at)] = pending.as_slice() else {
        panic!("two pending looks: {pending:?}");
    };
    assert!(first_at < second_at, "looked one after the other");

    // The person leaves the room looked at first; then time passes its
    // deadline, not the other's.
    *left_room.lock().expect("left") = Some(first.to_string());
    sync().await.expect("sync");
    assert!(client
        .get_room(first)
        .is_some_and(|room| room.state() == matrix_sdk::RoomState::Left));
    tokio::time::pause();
    let now = tokio::time::Instant::now();
    tokio::time::advance(*first_at - now + (*second_at - *first_at) / 2).await;

    let next = finder.look().await.expect("a user");
    assert!(
        next > tokio::time::Instant::now(),
        "the finder would look again at once, and again, for a room it left"
    );
    assert_eq!(next, *second_at, "the next round is the kept room's look");
    assert_eq!(asked(), 2 * FINDER_PAGES, "nothing was looked at");
    let kept = finder.looks.get(second).expect("the kept room's look");
    assert_eq!(
        (
            kept.looks,
            kept.from.as_ref().map(|resume| resume.from.as_str())
        ),
        (1, Some(format!("q{FINDER_PAGES}").as_str())),
        "its backoff and where it stopped"
    );
    assert!(!finder.looks.contains_key(first), "the left room's look");
}

/// 102.5 (R501, review R102L5-02): an account's list worker lives with the
/// account, not with the Agents list. With the Chats view closed — its inbox
/// unsubscribed, as opening Notes does — a card's expiry and then a remote
/// change still reach what the notes dock lists.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_notes_dock_hears_the_worker_after_the_chats_view_closed() {
    let shared = server(first_sync(), Messages::Serve);
    shared
        .0
        .lock()
        .expect("server")
        .failing
        .insert("r07".to_owned());
    let handler = Arc::clone(&shared);
    let fake = testing::serve(move |seen| {
        sliding_unanswered(seen).unwrap_or_else(|| answer(&handler, seen))
    });
    let signed = Signed::new();
    signed.sign_in(LIVE, &fake.base, &session_blob());
    let agents = Batches::default();
    let chats = signed.subscribe(&agents).await;
    let client = signed.live(LIVE).await;
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("sync");
    let says = |state: AgentAttentionVm| {
        let signed = &signed;
        move || async move { signed.dock(LIVE, CONVERSATION).await == Some(Some(state)) }
    };
    assert!(
        eventually(Duration::from_secs(20), says(AgentAttentionVm::Unknown)).await,
        "a room a question may wait in was read in part"
    );

    signed.manager.unsubscribe_inbox(chats).await;
    assert!(
        signed.manager.inbox.lock().await.is_none(),
        "no Agents list is drawn"
    );
    shared.0.lock().expect("server").failing.clear();
    let (due, _) = signed
        .manager
        .agent_lists
        .attach(LIVE)
        .expect("the account's worker");
    due.send(Due::Expires(room("!r07:h"), chrono::Utc::now()))
        .expect("the worker is running");
    assert!(
        eventually(Duration::from_secs(20), says(AgentAttentionVm::Working)).await,
        "the card's expiry reached the dock"
    );

    let mut join = serde_json::Map::new();
    join.insert(
        CONVERSATION.to_owned(),
        json!({ "timeline": {
            "events": [status(NIXI, "conversation", "blocked", 90)], "limited": false,
        }}),
    );
    shared
        .0
        .lock()
        .expect("server")
        .syncs
        .push_back(json!({ "next_batch": "s2", "rooms": { "join": join } }));
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("sync");
    assert!(
        eventually(Duration::from_secs(20), says(AgentAttentionVm::Failed)).await,
        "the remote change reached the dock"
    );
}

/// 102.5 #3 (R416, R501, review R102L5-02): the seat finder lives with the
/// account. The Agents list subscribed and unsubscribed again and again
/// while a room's search backs off does not look again before its time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_finder_keeps_its_backoff_across_subscriptions() {
    let deep = FINDER_PAGES + 4;
    let fake = testing::serve(move |seen| {
        if let Some(reply) = sliding_unanswered(seen) {
            return reply;
        }
        let path = seen.path.as_str();
        if path.ends_with("/sync") {
            let mut join = serde_json::Map::new();
            join.insert(
                CONVERSATION.to_owned(),
                json!({
                    "state": { "events": room_state(NIXI) },
                    "timeline": { "events": [], "limited": true, "prev_batch": "q0" },
                }),
            );
            return Reply::json(
                200,
                &json!({ "next_batch": "s1", "rooms": { "join": join } }).to_string(),
            );
        }
        if path.ends_with("/messages") {
            return buried_history(seen, deep);
        }
        if path.ends_with("/versions") {
            return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
        }
        if path.ends_with("/keys/upload") {
            return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
        }
        if path.ends_with("/keys/query") {
            return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
        }
        Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
    });
    // A look of the finder reads twenty pages, its last from `q19`, and the
    // next one goes on from `q20`; the worker's own read-back stops after
    // six pages (300 events), so neither page is ever the worker's.
    let looks = || {
        let (last, resume) = (format!("q{}", FINDER_PAGES - 1), format!("q{FINDER_PAGES}"));
        let pages: Vec<Option<String>> = fake
            .requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages"))
            .map(|seen| seen.query.get("from").cloned())
            .collect();
        let from = |page: &str| {
            pages
                .iter()
                .filter(|from| from.as_deref() == Some(page))
                .count()
        };
        (from(&last), from(&resume))
    };
    let signed = Signed::new();
    signed.sign_in(LIVE, &fake.base, &session_blob());
    signed.warm(LIVE, &fake.base).await;
    let agents = Batches::default();
    let mut chats = signed.subscribe(&agents).await;

    // The first look reads its twenty pages; the room's next look is a
    // minute away.
    let start = Instant::now();
    while looks().0 == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the finder never finished its first look"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(looks(), (1, 0), "one look so far");

    for _ in 0..3 {
        signed.manager.unsubscribe_inbox(chats).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        chats = signed.subscribe(&agents).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        looks(),
        (1, 0),
        "no subscription made the finder look again, from the start or from where it stopped"
    );
}

/// 102.5 #1 (R501, R381; review R102L5-04): an account whose session cannot
/// be restored keeps the Agents rows kept from its last run, beside another
/// account whose live list replaced its own, and the list says why.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_account_that_cannot_start_keeps_its_agents_beside_one_that_did() {
    let shared = server(first_sync(), Messages::Serve);
    let handler = Arc::clone(&shared);
    let fake = testing::serve(move |seen| {
        sliding_unanswered(seen).unwrap_or_else(|| answer(&handler, seen))
    });
    let signed = Signed::new();
    signed.sign_in(LIVE, &fake.base, &session_blob());
    signed.sign_in(BROKEN, &fake.base, "not a session");
    let store = signed.manager.agent_list_rows.clone();
    store
        .replace(LIVE, &[kept_row("!live-kept:h", 10, None)])
        .expect("kept");
    store
        .replace(BROKEN, &[kept_row("!broken-kept:h", 20, None)])
        .expect("kept");
    let agents = Batches::default();
    signed.subscribe(&agents).await;
    let client = signed.live(LIVE).await;
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("sync");

    let merger = signed
        .manager
        .inbox
        .lock()
        .await
        .as_ref()
        .expect("subscribed")
        .merger
        .clone();
    assert!(
        eventually(Duration::from_secs(20), || {
            let merger = merger.clone();
            async move { merger.live_agent_rows(LIVE).await.is_some() }
        })
        .await,
        "the healthy account's list went live"
    );
    let window: Vec<String> = last_window(&agents)
        .into_iter()
        .map(|row| row.room_id)
        .collect();
    assert!(
        !window.iter().any(|room| room == "!live-kept:h"),
        "the live list replaced the healthy account's kept rows: {window:?}"
    );
    assert!(
        window.iter().any(|room| room == "!broken-kept:h"),
        "the account that could not start keeps its rows: {window:?}"
    );
    assert_eq!(
        last_note(&agents).as_deref(),
        Some(AgentListTrouble::NotStarted.sentence())
    );
}

/// The client's side of one HTTP/1.1 connection, read as requests (R501,
/// review R102L5-05): a request is counted by its target — the path before
/// any query — once its whole head has arrived, however the bytes were split
/// between reads, and its body is skipped by its `Content-Length`, whatever
/// the body says.
#[derive(Default)]
struct Requests {
    unread: Vec<u8>,
    body_left: usize,
}

impl Requests {
    /// Read `bytes`, the next stretch of the connection: how many requests
    /// for `/messages` were completed in it.
    fn read(&mut self, bytes: &[u8]) -> usize {
        self.unread.extend_from_slice(bytes);
        let mut messages = 0;
        loop {
            if self.body_left > 0 {
                let skip = self.body_left.min(self.unread.len());
                self.unread.drain(..skip);
                self.body_left -= skip;
                if self.body_left > 0 {
                    return messages;
                }
            }
            let Some(end) = self.unread.windows(4).position(|w| w == b"\r\n\r\n") else {
                return messages;
            };
            let head: Vec<u8> = self.unread.drain(..end + 4).collect();
            let head = String::from_utf8_lossy(&head);
            let mut lines = head.split("\r\n");
            let target = lines
                .next()
                .and_then(|line| line.split(' ').nth(1))
                .unwrap_or_default();
            if target
                .split('?')
                .next()
                .is_some_and(|path| path.ends_with("/messages"))
            {
                messages += 1;
            }
            self.body_left = lines
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse().ok())
                .unwrap_or(0);
        }
    }
}

/// The relay's request reading (R501, review R102L5-05): a `/messages`
/// target split across reads is counted once, when its head is whole; a
/// body that says `/messages` is not a request; keep-alive requests are
/// each counted, a query is not part of the path.
#[test]
fn the_relay_counts_requests_not_bytes() {
    let mut requests = Requests::default();
    assert_eq!(requests.read(b"GET /_matrix/client/v3/rooms/!r:h/mess"), 0);
    assert_eq!(
        requests.read(b"ages?dir=b&from=t1 HTTP/1.1\r\nHost: h\r"),
        0
    );
    assert_eq!(
        requests.read(b"\n\r\n"),
        1,
        "counted once its head is whole"
    );
    let body = "{\"note\":\"/messages /messages\"}";
    let post = format!(
        "POST /_matrix/client/v3/rooms/!r:h/send/m.room.message/1 HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (start, rest) = post.split_at(post.len() - 12);
    assert_eq!(requests.read(start.as_bytes()), 0);
    assert_eq!(
        requests.read(
            format!("{rest}GET /_matrix/client/v3/rooms/!r:h/messages HTTP/1.1\r\n\r\n").as_bytes()
        ),
        1,
        "the body is no request; the next request is"
    );
    assert_eq!(
        requests.read(b"GET /_matrix/client/v3/sync?filter=/messages HTTP/1.1\r\n\r\n"),
        0,
        "a query is not the path"
    );
}

/// A loopback relay to `upstream` (`host:port`) that counts the `/messages`
/// requests a client sends through it: the live tests' view of what a
/// listing and a renewal ask the server.
async fn counting_relay(upstream: String) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            let (upstream, counted) = (upstream.clone(), Arc::clone(&counted));
            tokio::spawn(async move {
                let Ok(outbound) = tokio::net::TcpStream::connect(&upstream).await else {
                    return;
                };
                let (mut from_client, mut to_client) = inbound.into_split();
                let (mut from_server, mut to_server) = outbound.into_split();
                let up = async {
                    let mut buf = vec![0_u8; 16 * 1024];
                    let mut requests = Requests::default();
                    while let Ok(n) = from_client.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        counted.fetch_add(requests.read(&buf[..n]), Ordering::SeqCst);
                        if to_server.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    let _ = to_server.shutdown().await;
                };
                let down = async {
                    let _ = tokio::io::copy(&mut from_server, &mut to_client).await;
                    let _ = to_client.shutdown().await;
                };
                tokio::join!(up, down);
            });
        }
    });
    (base, count)
}

/// The live homeserver and its secrets, as `tests/agents_matrix_live.rs`
/// reads them (never printed).
struct Live {
    homeserver: String,
    secrets: HashMap<String, String>,
}

impl Live {
    fn from_env() -> Live {
        let homeserver = std::env::var("KEEPER_AGENTS_SMOKE_HOMESERVER")
            .expect("KEEPER_AGENTS_SMOKE_HOMESERVER names the test homeserver");
        let path = std::env::var("KEEPER_AGENTS_SMOKE_SECRETS")
            .expect("KEEPER_AGENTS_SMOKE_SECRETS names the secrets file");
        let text = std::fs::read_to_string(path).expect("the secrets file is readable");
        Live {
            homeserver: homeserver.trim_end_matches('/').to_owned(),
            secrets: text
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .collect(),
        }
    }

    fn secret(&self, key: &str) -> &str {
        self.secrets
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("{key} in the secrets file"))
    }

    fn user(&self, localpart: &str) -> matrix_sdk::ruma::OwnedUserId {
        matrix_sdk::ruma::OwnedUserId::try_from(format!(
            "@{localpart}:{}",
            self.secret("SERVER_NAME")
        ))
        .expect("user")
    }

    async fn admin(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.homeserver))
            .bearer_auth(self.secret("ADMIN_TOKEN"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("admin call");
        assert!(response.status().is_success(), "admin {path}");
        response.json::<Value>().await.unwrap_or(Value::Null)
    }

    async fn clear_devices(&self, localpart: &str) {
        let user = self.user(localpart);
        let listed = self
            .admin(
                reqwest::Method::GET,
                &format!("/_synapse/admin/v2/users/{user}/devices"),
                None,
            )
            .await;
        let devices: Vec<Value> = listed["devices"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|device| device["device_id"].clone())
            .collect();
        if !devices.is_empty() {
            self.admin(
                reqwest::Method::POST,
                &format!("/_synapse/admin/v2/users/{user}/delete_devices"),
                Some(json!({ "devices": devices })),
            )
            .await;
        }
    }

    async fn device(
        &self,
        homeserver: &str,
        dir: &std::path::Path,
        localpart: &str,
        password_key: &str,
    ) -> crate::agents::matrix::AgentClient {
        let client = crate::agents::matrix::AgentClient::open(homeserver, dir, "smoke-passphrase")
            .await
            .expect("client");
        client
            .login(localpart, self.secret(password_key), None, localpart)
            .await
            .expect("login");
        // The event cache holds what the first sync brings, as keeper's
        // own activation subscribes it before syncing.
        client
            .client()
            .event_cache()
            .subscribe()
            .expect("event cache");
        client.sync_once().await.expect("first sync");
        client
    }
}

/// 102.5 #3, #5 (R468; R501, review R102L5-05), live on delectra's Synapse:
/// eight session rooms, four of them with their status buried under 220
/// claim renewals, read by a fresh device through a relay that counts the
/// `/messages` requests it parses (calibrated first with one request split
/// inside its target). Listing the agent rooms asks the server nothing; the
/// running worker reads every room and settles; the one claim renewal sent —
/// waited for by its event id — reaches the running worker through its event
/// cache, which reads that room alone and asks the server at most once.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn live_a_listing_and_a_renewal_ask_the_server_next_to_nothing() {
    use crate::agents::events::CLAIM;
    use crate::agents::matrix::RoomKind;
    use std::sync::atomic::Ordering;

    let live = Live::from_env();
    for localpart in ["nixi-smoke", "tgorka-smoke"] {
        live.clear_devices(localpart).await;
    }
    let root = tempfile::tempdir().expect("root");
    let agent = live.user("nixi-smoke");
    live.admin(
        reqwest::Method::POST,
        &format!("/_synapse/admin/v1/users/{agent}/override_ratelimit"),
        Some(json!({ "messages_per_second": 0, "burst_count": 0 })),
    )
    .await;
    let nixi = live
        .device(
            &live.homeserver,
            &root.path().join("nixi"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
        )
        .await;
    let person = live
        .device(
            &live.homeserver,
            &root.path().join("tgorka"),
            "tgorka-smoke",
            "TGORKA_SMOKE_PASSWORD",
        )
        .await;
    // The rooms earlier runs left the person in are left, so this run's
    // device holds this run's rooms alone.
    for room in person.client().joined_rooms() {
        let _ = room.leave().await;
    }
    person.sync_once().await.expect("sync");
    const LIVE_ROOMS: usize = 8;
    let mut rooms = Vec::new();
    for n in 0..LIVE_ROOMS {
        let room = nixi
            .create_room(
                RoomKind::Session(SessionKind::Conversation),
                &format!("nixi 102.5 {n}"),
                vec![live.user("tgorka-smoke")],
                &[],
            )
            .await
            .expect("create room");
        let deadline = Instant::now() + Duration::from_secs(60);
        while person.client().get_room(&room).is_none() {
            assert!(Instant::now() < deadline, "the invite never arrived");
            person.sync_once().await.expect("sync");
        }
        person.join(&room).await.expect("join");
        nixi.send(
            &room,
            STATUS,
            json!({
                "v": CONTENT_VERSION, "session": format!("sessions/nixi/{n}"),
                "kind": "conversation", "title": format!("live {n}"), "agent": agent,
                "host": "electra", "epoch": 1, "run": "idle",
            }),
            None,
        )
        .await
        .expect("status");
        rooms.push(room);
    }
    let claim = |at: chrono::DateTime<chrono::Utc>| {
        json!({
            "v": 1, "host": "electra", "device": "ELECTRA", "agent": agent, "epoch": 1,
            "acquired_at": at.to_rfc3339(), "renewed_at": at.to_rfc3339(),
            "expires_at": at.to_rfc3339(), "released": false,
        })
    };
    let start = chrono::Utc::now();
    for room in rooms.iter().take(LIVE_ROOMS / 2) {
        for n in 0..220_i64 {
            nixi.send_state(
                room,
                CLAIM,
                "",
                &claim(start + chrono::Duration::seconds(n)),
            )
            .await
            .expect("a renewal");
        }
    }

    let upstream = live
        .homeserver
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .to_owned();
    let (relay, asked) = counting_relay(upstream).await;
    // The relay counts requests, not bytes: one `/messages` request written
    // in two halves, split inside its target, is counted once.
    let calibrated = asked.load(Ordering::SeqCst);
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(relay.trim_start_matches("http://"))
            .await
            .expect("relay");
        stream
            .write_all(b"GET /_matrix/client/v3/rooms/%21calibrate%3Ah/mess")
            .await
            .expect("write");
        stream.flush().await.expect("flush");
        tokio::time::sleep(Duration::from_millis(100)).await;
        stream
            .write_all(b"ages?dir=b HTTP/1.1\r\nHost: relay\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut answered = Vec::new();
        let _ = stream.read_to_end(&mut answered).await;
        assert!(!answered.is_empty(), "the homeserver answered");
    }
    assert_eq!(
        asked.load(Ordering::SeqCst) - calibrated,
        1,
        "one request split inside its target is counted once"
    );
    let fresh = live
        .device(
            &relay,
            &root.path().join("fresh"),
            "tgorka-smoke",
            "TGORKA_SMOKE_PASSWORD",
        )
        .await;
    fresh.sync_once().await.expect("sync");
    let session_rooms = fresh
        .client()
        .joined_rooms()
        .iter()
        .filter(|room| AgentRoomKind::of(room.room_type().as_ref()) == Some(AgentRoomKind::Session))
        .count();
    assert_eq!(session_rooms, LIVE_ROOMS, "this run's rooms alone");
    let (kinds, proxies) = (
        Arc::new(AgentKinds::default()),
        Arc::new(AgentProxies::default()),
    );

    let before = asked.load(Ordering::SeqCst);
    let (listed, _) = super::super::agent_rooms_of(fresh.client(), ACCOUNT, &kinds, &proxies).await;
    let listing = asked.load(Ordering::SeqCst) - before;
    println!("/messages for one listing of {LIVE_ROOMS} rooms: {listing}");
    assert_eq!(
        listing,
        0,
        "a listing asks the server nothing ({} listed)",
        listed.len()
    );

    // The worker runs as an account's does: it hears the rooms that change
    // from its event cache, nothing chosen for it here.
    let data = tempfile::tempdir().expect("data");
    let worker = ListWorker {
        client: fresh.client().clone(),
        account_id: ACCOUNT.to_owned(),
        kinds,
        proxies,
        data_dir: Some(data.path().to_owned()),
        board: Arc::default(),
    };
    let kept = Kept::default();
    let reads = kept.reads_counter();
    let (due, due_rx) = mpsc::unbounded_channel();
    let (changed, _) = broadcast::channel(CHANGED_BACKLOG);
    let before = asked.load(Ordering::SeqCst);
    let running = tokio::spawn(run_attention_worker(worker, kept, due_rx, changed));
    assert!(
        until(Duration::from_secs(120), || reads.load(Ordering::SeqCst)
            >= LIVE_ROOMS)
        .await,
        "the first pass never read every room"
    );
    // The fresh device's event cache is still taking in its first sync
    // when the worker starts, so a room may be read again as its updates
    // land: wait until the worker has been idle for three seconds.
    let mut seen = reads.load(Ordering::SeqCst);
    let settling = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let now = reads.load(Ordering::SeqCst);
        if now == seen {
            break;
        }
        assert!(
            settling.elapsed() < Duration::from_secs(120),
            "the worker never settled: {now} readings"
        );
        seen = now;
    }
    println!(
        "worker start: {seen} readings of {LIVE_ROOMS} rooms, /messages {}",
        asked.load(Ordering::SeqCst) - before
    );

    let renewed = rooms[0].clone();
    let renewal = nixi
        .send_state(&renewed, CLAIM, "", &claim(chrono::Utc::now()))
        .await
        .expect("a renewal");
    let (before, read_before) = (asked.load(Ordering::SeqCst), reads.load(Ordering::SeqCst));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fresh.sync_once().await.expect("sync");
        let landed = fresh
            .client()
            .get_room(&renewed)
            .expect("room")
            .get_state_event(CLAIM.into(), "")
            .await
            .ok()
            .flatten()
            .is_some_and(|raw| match raw {
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Sync(raw) => {
                    raw.get_field::<String>("event_id")
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some(renewal.as_str())
                }
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Stripped(_) => false,
            });
        if landed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the renewal {renewal} never reached the fresh device"
        );
    }
    assert!(
        until(Duration::from_secs(60), || reads.load(Ordering::SeqCst)
            > read_before)
        .await,
        "the running worker never read the renewal"
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    let asked_for_renewal = asked.load(Ordering::SeqCst) - before;
    println!("/messages for one claim renewal: {asked_for_renewal}");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        read_before + 1,
        "the renewal is read in its own room alone"
    );
    assert!(
        asked_for_renewal <= 1,
        "a renewal asked the server {asked_for_renewal} times"
    );
    drop(due);
    running.abort();
}
