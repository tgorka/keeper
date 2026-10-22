//! Opening a room as the producer does it, against a loopback homeserver
//! (102.6, R468): the window filled from older history in Rust — one page at
//! a time until it draws enough, never again after a page failed. A
//! `/messages` with a `from` is a page of the fill; one without is answered
//! once released (`release_search`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use matrix_sdk_ui::Timeline;
use serde_json::{json, Value};

use super::{forward_timeline, open_timeline, HistoryAsks};
use crate::forges::testing::{self, Reply, Seen};
use crate::vm::{
    PaginationState, PaginationStatusBatch, TimelineBatch, TimelineItemVm, TimelineOp,
};

const ROOM: &str = "!open:h";

const ME: &str = "@tgorka:h";
const NIXI: &str = "@nixi:h";

fn state(kind: &str, key: &str, sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": kind, "state_key": key, "sender": sender, "content": content,
        "event_id": format!("$s{n}:h"), "origin_server_ts": 1_000 + n,
    })
}

fn member(user: &str, by: &str, membership: &str, n: u64) -> Value {
    state(
        "m.room.member",
        user,
        by,
        json!({ "membership": membership }),
        n,
    )
}

/// The state of a room `creator` made, with the person in it.
fn room_state(creator: &str) -> Vec<Value> {
    vec![
        state(
            "m.room.create",
            "",
            creator,
            json!({ "creator": creator, "room_version": "11" }),
            1,
        ),
        member(creator, creator, "join", 3),
        member(ME, ME, "join", 4),
    ]
}

fn message(sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": "m.room.message", "sender": sender, "content": content,
        "event_id": format!("$m{n}:h"), "origin_server_ts": 2_000 + n,
    })
}

/// A sync carrying `room`'s `state` and `timeline`.
fn sync(n: u64, room: &str, state: Vec<Value>, timeline: Vec<Value>, limited: bool) -> Value {
    json!({
        "next_batch": format!("s{n}"),
        "rooms": { "join": { room: {
            "state": { "events": state },
            "timeline": { "events": timeline, "limited": limited, "prev_batch": format!("p{n}") },
        }}},
    })
}

/// What a producer sent, and the producer.
struct Drawn {
    batches: Arc<Mutex<Vec<TimelineBatch>>>,
    producer: tokio::task::JoinHandle<()>,
}

impl Drop for Drawn {
    fn drop(&mut self) {
        self.producer.abort();
    }
}

impl Drawn {
    /// Every batch's ops applied in order, as the frontend's store does.
    fn items(&self) -> Vec<TimelineItemVm> {
        let mut items: Vec<TimelineItemVm> = Vec::new();
        for batch in self.batches.lock().expect("batches").iter() {
            for op in &batch.ops {
                match op.clone() {
                    TimelineOp::Reset { items: all } => items = all,
                    TimelineOp::Append { items: more } => items.extend(more),
                    TimelineOp::Clear => items.clear(),
                    TimelineOp::PushFront { item } => items.insert(0, item),
                    TimelineOp::PushBack { item } => items.push(item),
                    TimelineOp::PopFront => {
                        items.remove(0);
                    }
                    TimelineOp::PopBack => {
                        items.pop();
                    }
                    TimelineOp::Insert { index, item } => items.insert(index as usize, item),
                    TimelineOp::Set { index, item } => items[index as usize] = item,
                    TimelineOp::Remove { index } => {
                        items.remove(index as usize);
                    }
                    TimelineOp::Truncate { length } => items.truncate(length as usize),
                }
            }
        }
        items
    }

    async fn until(&self, what: &str, done: impl Fn(&Drawn) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !done(self) {
            assert!(
                std::time::Instant::now() < deadline,
                "never: {what}: {:#?}",
                self.items()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// What the fake homeserver answers.
struct Server {
    syncs: Vec<Value>,
    /// Whether the header's search may be answered yet.
    search_held: bool,
    /// The header's search's answer: its newest page.
    search: Value,
    /// The fill's pages, by the `from` they answer.
    pages: HashMap<String, Reply>,
    /// The pages held unanswered, by their `from`: a request for one waits
    /// while it is held and is then refused, keeping its page for the next.
    held: HashSet<String>,
    /// Whether a redaction is refused (403, unrecoverable) or accepted.
    refuse_redactions: bool,
}

type Shared = Arc<(Mutex<Server>, Condvar)>;

fn answer(shared: &Shared, seen: &Seen) -> Reply {
    let path = seen.path.as_str();
    if path.ends_with("/sync") {
        let mut server = shared.0.lock().expect("server");
        let next = if server.syncs.is_empty() {
            json!({"next_batch": "idle"})
        } else {
            server.syncs.remove(0)
        };
        return Reply::json(200, &next.to_string());
    }
    if path.contains("/redact/") {
        let refused = shared.0.lock().expect("server").refuse_redactions;
        return if refused {
            Reply::json(403, r#"{"errcode":"M_FORBIDDEN","error":"not yours"}"#)
        } else {
            Reply::json(200, r#"{"event_id":"$redaction:h"}"#)
        };
    }
    if let Some((_, txn)) = path.split_once("/send/m.room.message/") {
        return Reply::json(
            200,
            &json!({ "event_id": format!("$sent-{txn}:h") }).to_string(),
        );
    }
    if path.ends_with("/messages") {
        let (lock, wake) = &**shared;
        let mut server = lock.lock().expect("server");
        return match seen.query.get("from") {
            Some(from) if server.held.contains(from) => {
                while server.held.contains(from) {
                    server = wake.wait(server).expect("server");
                }
                Reply::json(504, r#"{"errcode":"M_UNKNOWN","error":"held"}"#)
            }
            Some(from) => server
                .pages
                .remove(from)
                .unwrap_or_else(|| Reply::json(200, r#"{"chunk":[],"start":"x"}"#)),
            None => {
                while server.search_held {
                    server = wake.wait(server).expect("server");
                }
                Reply::json(200, &server.search.to_string())
            }
        };
    }
    if path.ends_with("/members") {
        let chunk: Vec<Value> = [ME, NIXI]
            .iter()
            .enumerate()
            .map(|(n, user)| {
                state(
                    "m.room.member",
                    user,
                    user,
                    json!({"membership": "join"}),
                    900 + n as u64,
                )
            })
            .collect();
        return Reply::json(200, &json!({ "chunk": chunk }).to_string());
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

/// A custom state event the timeline never draws (an agent host's claim
/// renewal, in the rooms of keeper before 108.2).
fn claim(n: u64) -> Value {
    state(
        "dev.keeper.agent.claim",
        "",
        NIXI,
        json!({ "v": 1, "host": "electra", "agent": NIXI, "renewed_at": n }),
        3_000 + n,
    )
}

fn said(text: &str, n: u64) -> Value {
    message(NIXI, json!({ "msgtype": "m.text", "body": text }), n)
}

/// A page of the fill: `events` oldest first, continuing at `end`.
fn page(events: Vec<Value>, end: Option<&str>) -> Reply {
    let mut events = events;
    events.reverse();
    let mut body = json!({ "chunk": events, "start": "s" });
    if let Some(end) = end {
        body["end"] = json!(end);
    }
    Reply::json(200, &body.to_string())
}

struct Device {
    shared: Shared,
    fake: testing::Fake,
    client: Client,
}

impl Device {
    async fn new(pages: HashMap<String, Reply>) -> Device {
        Device::with_builder(pages, Client::builder()).await
    }

    async fn with_builder(
        pages: HashMap<String, Reply>,
        builder: matrix_sdk::ClientBuilder,
    ) -> Device {
        let shared: Shared = Arc::new((
            Mutex::new(Server {
                syncs: Vec::new(),
                search_held: true,
                search: json!({"chunk": [], "start": "s"}),
                pages,
                held: HashSet::new(),
                refuse_redactions: false,
            }),
            Condvar::new(),
        ));
        let handler = Arc::clone(&shared);
        let fake = testing::serve(move |seen| answer(&handler, seen));
        let client = builder
            .homeserver_url(&fake.base)
            .build()
            .await
            .expect("client");
        let session: matrix_sdk::authentication::matrix::MatrixSession = serde_json::from_value(
            json!({ "user_id": ME, "device_id": "PHONE", "access_token": "t" }),
        )
        .expect("session");
        client.restore_session(session).await.expect("restore");
        client.event_cache().subscribe().expect("event cache");
        Device {
            shared,
            fake,
            client,
        }
    }

    /// Sync `body`. The event cache takes it in on a task of its own, after
    /// `sync_once` returned, so a room opened right after may get its
    /// events through the stream rather than in the snapshot.
    async fn sync(&self, body: Value) {
        self.shared.0.lock().expect("server").syncs.push(body);
        self.client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
    }

    /// Sync `body` and wait until the room's event cache holds its timeline,
    /// for a test about what the snapshot (the first batch) carries.
    async fn sync_cached(&self, body: Value) {
        let carried = body["rooms"]["join"][ROOM]["timeline"]["events"]
            .as_array()
            .map_or(0, Vec::len);
        self.sync(body).await;
        let room_id: OwnedRoomId = ROOM.try_into().expect("room");
        let room = self.client.get_room(&room_id).expect("the synced room");
        let (cache, _handles) = room.event_cache().await.expect("room cache");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while cache.events().await.expect("cached events").len() < carried {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the cache never took the sync in"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn release_search(&self) {
        self.shared.0.lock().expect("server").search_held = false;
        self.shared.1.notify_all();
    }

    /// Answer the page at `from` with `reply` from now on.
    fn serve_page(&self, from: &str, reply: Reply) {
        self.shared
            .0
            .lock()
            .expect("server")
            .pages
            .insert(from.to_owned(), reply);
    }

    /// Hold the page at `from` unanswered, or answer it again.
    fn hold_page(&self, from: &str, held: bool) {
        let mut server = self.shared.0.lock().expect("server");
        if held {
            server.held.insert(from.to_owned());
        } else {
            server.held.remove(from);
        }
        drop(server);
        self.shared.1.notify_all();
    }

    /// How often the fill asked for the page at `from`.
    fn asked(&self, from: &str) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|seen| {
                seen.path.ends_with("/messages")
                    && seen.query.get("from").map(String::as_str) == Some(from)
            })
            .count()
    }

    async fn open(&self) -> Drawn {
        self.open_asking().await.0
    }

    /// Open the room, with where the person's asks for older history reach
    /// its producer and the timeline the person's own pagination pages.
    async fn open_asking(&self) -> (Drawn, HistoryAsks, Arc<Timeline>) {
        let room: OwnedRoomId = ROOM.try_into().expect("room");
        let open = open_timeline(&self.client, &room, "acct")
            .await
            .expect("timeline");
        let (asks, timeline) = (open.asks(), open.timeline());
        let batches: Arc<Mutex<Vec<TimelineBatch>>> = Arc::default();
        let sink = Arc::clone(&batches);
        let producer = tokio::spawn(forward_timeline(
            open,
            room,
            Box::new(move |batch| {
                sink.lock().expect("batches").push(batch);
                true
            }),
        ));
        (Drawn { batches, producer }, asks, timeline)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // A held search never outlives its test.
        self.release_search();
    }
}

fn messages(drawn: &Drawn) -> usize {
    drawn
        .items()
        .iter()
        .filter(|item| matches!(item, TimelineItemVm::Message { .. }))
        .count()
}

/// 102.6 #3: a window that draws nothing — the newest events are claim
/// renewals — pages back in Rust, page after page, and stops once it draws
/// a screen's start, without the webview asking.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_that_draws_nothing_fills_itself_and_stops_when_it_draws_enough() {
    let pages = HashMap::from([
        (
            "p1".to_owned(),
            page(
                (10..14).map(|n| said(&format!("m{n}"), n)).collect(),
                Some("q2"),
            ),
        ),
        (
            "q2".to_owned(),
            page(
                (20..28).map(|n| said(&format!("m{n}"), n)).collect(),
                Some("q3"),
            ),
        ),
        (
            "q3".to_owned(),
            page(
                (45..60).map(|n| said(&format!("m{n}"), n)).collect(),
                Some("q4"),
            ),
        ),
    ]);
    let device = Device::new(pages).await;
    device.release_search();
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            (1..=5).map(claim).collect(),
            true,
        ))
        .await;
    let drawn = device.open().await;
    drawn
        .until("a screen's start of messages", |drawn| {
            messages(drawn) >= 10
        })
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        (device.asked("p1"), device.asked("q2"), device.asked("q3")),
        (1, 1, 0),
        "two pages, and none once the window drew enough"
    );
    assert_eq!(messages(&drawn), 12);
}

/// 102.6 #3: a window that already draws enough — the SDK shows a room's
/// newest 20 items at first — asks the server for no older page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_that_draws_enough_asks_for_nothing_older() {
    let device = Device::new(HashMap::new()).await;
    device.release_search();
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            (1..=30).map(|n| said(&format!("m{n}"), n)).collect(),
            true,
        ))
        .await;
    let drawn = device.open().await;
    drawn
        .until("the first batch", |drawn| messages(drawn) >= 10)
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(device.asked("p1"), 0, "no page was asked for");
}

/// 102.6 #3, R381: a fill page the server refuses is said where the person
/// looks — the boundary's failed state, with its Retry — and the window is
/// not filled again on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_fill_page_is_said_once_and_not_asked_again() {
    let pages = HashMap::from([(
        "p1".to_owned(),
        Reply::json(403, r#"{"errcode":"M_FORBIDDEN","error":"no"}"#),
    )]);
    let device = Device::new(pages).await;
    device.release_search();
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            (1..=5).map(claim).collect(),
            true,
        ))
        .await;
    let drawn = device.open().await;
    drawn
        .until("the failed fill said", |drawn| {
            drawn.batches.lock().expect("batches").iter().any(|batch| {
                batch
                    .pagination
                    .is_some_and(|status| status.state == PaginationState::Failed)
            })
        })
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(device.asked("p1"), 1, "the failed page is not asked again");
}

/// The newest pagination status the producer sent.
fn status(drawn: &Drawn) -> Option<PaginationStatusBatch> {
    drawn
        .batches
        .lock()
        .expect("batches")
        .iter()
        .rev()
        .find_map(|batch| batch.pagination)
}

fn drawn_text(drawn: &Drawn, text: &str) -> bool {
    drawn
        .items()
        .iter()
        .any(|item| matches!(item, TimelineItemVm::Message { body, .. } if body == text))
}

fn failed(drawn: &Drawn) -> bool {
    status(drawn).is_some_and(|status| status.state == PaginationState::Failed)
}

/// R509/01: the room's start is the SDK's current word, not a lasting fact.
/// The fill reaches the start; a limited sync of nothing but claims then
/// unloads the window to its last chunk, and the fill pages again on its
/// own — no renderer asks — until the earlier message shows once more. The
/// header's search is held unanswered the whole time and is no obstacle.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_unloaded_after_reaching_its_start_fills_again() {
    let pages = HashMap::from([("p1".to_owned(), page(vec![said("early", 2)], None))]);
    let device = Device::new(pages).await;
    device
        .sync(sync(1, ROOM, room_state(NIXI), vec![claim(1)], true))
        .await;
    let drawn = device.open().await;
    drawn
        .until("the fill reaches the start", |drawn| {
            drawn_text(drawn, "early")
                && status(drawn)
                    .is_some_and(|status| status.state == PaginationState::Idle && status.hit_start)
        })
        .await;
    let before = drawn.batches.lock().expect("batches").len();

    device.serve_page("p2", page(vec![said("early", 2), claim(1)], Some("p1")));
    device
        .sync(sync(
            2,
            ROOM,
            Vec::new(),
            (10..16).map(claim).collect(),
            true,
        ))
        .await;
    drawn
        .until("the window unloaded by the limited sync", |drawn| {
            drawn.batches.lock().expect("batches")[before..]
                .iter()
                .flat_map(|batch| &batch.ops)
                .any(|op| matches!(op, TimelineOp::Clear | TimelineOp::Reset { .. }))
        })
        .await;
    drawn
        .until("the earlier message drawn again", |drawn| {
            drawn_text(drawn, "early")
        })
        .await;
    assert!(device.asked("p2") >= 1, "the fill asked past the new gap");
}

/// R509/02: the person's Retry reaches the fill's owner. A failed fill page
/// is tried again; its page brings only claims, and the fill pages on
/// until the older message shows, with no second ask.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_after_a_failed_fill_pages_on_until_something_shows() {
    let pages = HashMap::from([(
        "p1".to_owned(),
        Reply::json(403, r#"{"errcode":"M_FORBIDDEN","error":"no"}"#),
    )]);
    let device = Device::new(pages).await;
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            (1..=5).map(claim).collect(),
            true,
        ))
        .await;
    let (drawn, asks, _timeline) = device.open_asking().await;
    drawn.until("the failed fill said", failed).await;

    device.serve_page("p1", page((20..30).map(claim).collect(), Some("q2")));
    device.serve_page("q2", page(vec![said("older", 3)], Some("q3")));
    asks.ask();
    drawn
        .until("the older message after one Retry", |drawn| {
            drawn_text(drawn, "older") && !failed(drawn)
        })
        .await;
}

/// R509/04: a window that draws enough is not filled on its own, but the
/// person's "Load earlier messages" pages it until something older shows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn load_earlier_brings_something_older_into_a_full_window() {
    let device = Device::new(HashMap::new()).await;
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            (100..112).map(|n| said(&format!("m{n}"), n)).collect(),
            true,
        ))
        .await;
    let (drawn, asks, _timeline) = device.open_asking().await;
    drawn
        .until("the window at rest, more to load", |drawn| {
            messages(drawn) == 12
                && status(drawn).is_some_and(|status| {
                    status.state == PaginationState::Idle && !status.hit_start
                })
        })
        .await;
    assert_eq!(device.asked("p1"), 0, "a full window is not filled");

    device.serve_page("p1", page(vec![said("older", 3)], Some("q2")));
    asks.ask();
    drawn
        .until("the older message", |drawn| drawn_text(drawn, "older"))
        .await;
}

/// R509/03, R381: a fill page the server never answers is a failure the
/// person can retry once its deadline passes, not a window waiting for
/// good; the retried page then fills it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_fill_page_is_a_failure_the_person_can_retry() {
    let device = Device::new(HashMap::new()).await;
    device.release_search();
    device.hold_page("p1", true);
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            (1..=5).map(claim).collect(),
            true,
        ))
        .await;
    let (drawn, asks, _timeline) = device.open_asking().await;
    drawn.until("the stalled page said as failed", failed).await;

    device.serve_page("p1", page(vec![said("older", 3)], None));
    device.hold_page("p1", false);
    asks.ask();
    drawn
        .until("the older message after Retry", |drawn| {
            drawn_text(drawn, "older") && !failed(drawn)
        })
        .await;
}

/// The redacted stub of `key`'s item, by its send state, if it is one.
fn redacted(drawn: &Drawn, key: &str) -> Option<Option<crate::vm::SendState>> {
    drawn.items().into_iter().find_map(|item| match item {
        TimelineItemVm::Redacted {
            key: k, send_state, ..
        } if k == key => Some(send_state),
        _ => None,
    })
}

/// How many requests to `segment` the server has seen.
fn requests_to(device: &Device, segment: &str) -> usize {
    device
        .fake
        .requests()
        .iter()
        .filter(|seen| seen.path.contains(segment))
        .count()
}

/// matrix-sdk-ui 0.19 queues a deletion (#6428), and a refused request
/// holds the room's later sends behind it (#6843). The stub shows a refused
/// deletion as failed, never as deleted; Cancel restores the message and the
/// held send goes out; Retry queues the deletion again, and once the server
/// takes it the stub is deleted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_deletion_says_so_and_cancel_or_retry_resolve_it() {
    use crate::send::{self, MessageKind, SendTrigger};
    use crate::vm::SendState;

    let device = Device::new(HashMap::new()).await;
    device.release_search();
    device.shared.0.lock().expect("server").refuse_redactions = true;
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![
                said("theirs", 1),
                message(ME, json!({ "msgtype": "m.text", "body": "mine" }), 2),
            ],
            false,
        ))
        .await;
    let (drawn, _asks, timeline) = device.open_asking().await;
    drawn
        .until("my message", |drawn| drawn_text(drawn, "mine"))
        .await;
    let key = drawn
        .items()
        .into_iter()
        .find_map(|item| match item {
            TimelineItemVm::Message { key, body, .. } if body == "mine" => Some(key),
            _ => None,
        })
        .expect("my message's key");

    send::redact(&timeline, &key, None).await.expect("queued");
    drawn
        .until("the refused deletion", |drawn| {
            redacted(drawn, &key) == Some(Some(SendState::Failed))
        })
        .await;
    send::submit(
        &timeline,
        "after",
        MessageKind::Text,
        SendTrigger::ComposerSend,
    )
    .await
    .expect("queued");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        requests_to(&device, "/send/m.room.message/"),
        0,
        "a later send waits behind the refused deletion"
    );

    send::cancel(&timeline, &key).await.expect("cancelled");
    drawn
        .until("my message back, and the held send gone out", |drawn| {
            redacted(drawn, &key).is_none()
                && drawn_text(drawn, "mine")
                && requests_to(&device, "/send/m.room.message/") == 1
        })
        .await;

    send::redact(&timeline, &key, None).await.expect("queued");
    drawn
        .until("refused again", |drawn| {
            redacted(drawn, &key) == Some(Some(SendState::Failed))
        })
        .await;
    let refused = requests_to(&device, "/redact/");
    device.shared.0.lock().expect("server").refuse_redactions = false;
    send::retry(&timeline, &key).await.expect("retried");
    drawn
        .until("deleted once the server took it", |drawn| {
            matches!(redacted(drawn, &key), Some(None | Some(SendState::Sent)))
                && requests_to(&device, "/redact/") > refused
        })
        .await;
}
