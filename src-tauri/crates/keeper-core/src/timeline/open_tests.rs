//! Opening a room as the producer does it, against a loopback homeserver
//! (102.6, R468): the first batch from what the cache holds before the
//! header's search of the server, and the window filled from older history
//! in Rust — one page at a time until it draws enough, never again after a
//! page failed. `/messages` without a `from` is the header's search; with
//! one, a page of the fill.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use matrix_sdk_ui::Timeline;
use serde_json::{json, Value};

use super::brief_tests::{message, room_state, state, sync, Drawn, ME, NIXI};
use super::{forward_timeline, open_timeline, paginate_backwards, HistoryAsks};
use crate::agents::approval_card::HostedRooms;
use crate::agents::events::{CLAIM, STATUS};
use crate::agents::room::{AgentIcons, AgentKinds, Found, HeaderReader};
use crate::forges::testing::{self, Reply, Seen};
use crate::vm::{
    PaginationState, PaginationStatusBatch, TimelineBatch, TimelineItemVm, TimelineOp,
};

const ROOM: &str = "!open:h";

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

/// Nixi's status for the session, far enough back that the cache lacks it.
fn status_event(n: u64) -> Value {
    json!({
        "type": STATUS, "sender": NIXI, "event_id": format!("$status{n}:h"),
        "origin_server_ts": 500 + n,
        "content": {
            "v": 1, "session": "sessions/nixi/main", "kind": "main", "title": "Nixi",
            "agent": NIXI, "host": "electra", "epoch": 1, "run": "idle",
        },
    })
}

/// A host's claim renewal: room state the timeline never draws.
fn claim(n: u64) -> Value {
    state(
        CLAIM,
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

    /// A device whose event cache pauses where `gate` says.
    async fn gated(pages: HashMap<String, Reply>, gate: Arc<Gate>) -> Device {
        use matrix_sdk::config::StoreConfig;
        use matrix_sdk::cross_process_lock::CrossProcessLockConfig;
        let store = StoreConfig::new(CrossProcessLockConfig::multi_process("main"))
            .event_cache_store(GatedStore {
                inner: matrix_sdk_base::event_cache::store::MemoryStore::new(),
                gate,
            });
        Device::with_builder(pages, Client::builder().store_config(store)).await
    }

    async fn with_builder(
        pages: HashMap<String, Reply>,
        builder: matrix_sdk::ClientBuilder,
    ) -> Device {
        let shared: Shared = Arc::new((
            Mutex::new(Server {
                syncs: Vec::new(),
                search_held: true,
                search: json!({"chunk": [status_event(900)], "start": "s"}),
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

    /// Answer the header's search with `chunk`, newest first, continuing at
    /// `end`, from now on.
    fn serve_search(&self, chunk: Vec<Value>, end: Option<&str>) {
        let mut search = json!({ "chunk": chunk, "start": "s" });
        if let Some(end) = end {
            search["end"] = json!(end);
        }
        self.shared.0.lock().expect("server").search = search;
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

    /// How often a search asked for the newest page (no `from`).
    fn asked_newest(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/messages") && !seen.query.contains_key("from"))
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
            Arc::new(AgentKinds::default()),
            Arc::new(AgentIcons::default()),
            Arc::new(HostedRooms::default()),
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

/// 102.6 #1 (R468): the session room's status is out of the cache's reach,
/// and the server has not answered the header's search yet — the first
/// batch is sent anyway, from the cache, carrying the pagination status;
/// the status the search finds comes afterwards as a header of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_batch_is_sent_before_the_header_searches_the_server() {
    let device = Device::new(HashMap::new()).await;
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![said("the last word", 1)],
            true,
            &[NIXI],
        ))
        .await;
    let drawn = device.open().await;
    drawn
        .until("the first batch while the search waits", |drawn| {
            !drawn.batches.lock().expect("batches").is_empty()
        })
        .await;
    let first = drawn.batches.lock().expect("batches")[0].clone();
    let TimelineOp::Reset { items } = &first.ops[0] else {
        panic!("the first batch is a reset: {first:?}");
    };
    assert!(
        items.iter().any(
            |item| matches!(item, TimelineItemVm::Message { body, .. } if body == "the last word")
        ),
        "the cached message is on the first batch: {items:?}"
    );
    let header = first.header.as_ref().expect("a header on the first batch");
    assert!(header.status.is_none(), "the cache holds no status");
    assert!(
        first.pagination.is_some(),
        "the pagination status rides on the first batch"
    );

    device.release_search();
    drawn
        .until("the status the search found", |drawn| {
            drawn.batches.lock().expect("batches").iter().any(|batch| {
                batch
                    .header
                    .as_ref()
                    .is_some_and(|header| header.status.is_some())
            })
        })
        .await;
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
            &[NIXI],
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
            &[NIXI],
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
            &[NIXI],
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
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![claim(1)],
            true,
            &[NIXI],
        ))
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
            &[],
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
            &[NIXI],
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
            &[NIXI],
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

/// R509/03, R510: the header's search reads past the room's event cache.
/// With it held unanswered, the person's own pagination pages the window;
/// once it answers, what it read is in the header and nowhere in the cache.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_header_search_never_meets_the_persons_pagination() {
    let pages = HashMap::from([("p1".to_owned(), page(vec![said("older", 3)], None))]);
    let device = Device::new(pages).await;
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            (100..112).map(|n| said(&format!("m{n}"), n)).collect(),
            true,
            &[NIXI],
        ))
        .await;
    let (drawn, _asks, timeline) = device.open_asking().await;
    drawn
        .until("the first batch", |drawn| messages(drawn) == 12)
        .await;
    let paged = tokio::time::timeout(Duration::from_secs(10), paginate_backwards(&timeline, 40))
        .await
        .expect("the person's page never came back while the search waited");
    assert!(paged.is_ok(), "the page: {paged:?}");
    drawn
        .until("the older message", |drawn| drawn_text(drawn, "older"))
        .await;

    device.release_search();
    drawn
        .until("the status the search found", |drawn| {
            drawn.batches.lock().expect("batches").iter().any(|batch| {
                batch
                    .header
                    .as_ref()
                    .is_some_and(|header| header.status.is_some())
            })
        })
        .await;
    let room_id: OwnedRoomId = ROOM.try_into().expect("room");
    let room = device.client.get_room(&room_id).expect("room");
    let (cache, _handles) = room.event_cache().await.expect("room cache");
    let status_id: matrix_sdk::ruma::OwnedEventId = "$status900:h".try_into().expect("event id");
    assert!(
        cache.find_event(&status_id).await.expect("cache").is_none(),
        "the search saved what it read into the event cache"
    );
}

/// R509/03 × 102.5 (lane `102stack`): the Agents list's seat finder looks
/// the way the header searches, past the room's event cache. With its look
/// held unanswered, the person opens the same room and pages it; once the
/// look answers, the status it read is the room's seat and nowhere in the
/// cache.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_seat_finder_never_meets_the_persons_pagination() {
    let pages = HashMap::from([("p1".to_owned(), page(vec![said("older", 3)], None))]);
    let device = Device::new(pages).await;
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            (100..112).map(|n| said(&format!("m{n}"), n)).collect(),
            true,
            &[NIXI],
        ))
        .await;
    let room_id: OwnedRoomId = ROOM.try_into().expect("room");
    let room = device.client.get_room(&room_id).expect("room");
    let kinds = Arc::new(AgentKinds::default());
    let finder = {
        let (room, kinds) = (room.clone(), Arc::clone(&kinds));
        // One look of the finder, from the newest page, at its page budget.
        tokio::spawn(async move { HeaderReader::find(room, &kinds, None, 20).await })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while device.asked_newest() == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the finder never asked the server"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let (drawn, _asks, timeline) = device.open_asking().await;
    drawn
        .until("the first batch", |drawn| messages(drawn) == 12)
        .await;
    let paged = tokio::time::timeout(Duration::from_secs(10), paginate_backwards(&timeline, 40))
        .await
        .expect("the person's page never came back while the finder looked");
    assert!(paged.is_ok(), "the page: {paged:?}");
    drawn
        .until("the older message", |drawn| drawn_text(drawn, "older"))
        .await;

    device.release_search();
    let found = tokio::time::timeout(Duration::from_secs(10), finder)
        .await
        .expect("the finder's look never ended")
        .expect("the finder's task");
    assert_eq!(found, Found::Seat);
    assert!(
        kinds.seat(room.own_user_id(), room.room_id()).is_some(),
        "the status the look read is not the room's seat"
    );
    let (cache, _handles) = room.event_cache().await.expect("room cache");
    let status_id: matrix_sdk::ruma::OwnedEventId = "$status900:h".try_into().expect("event id");
    assert!(
        cache.find_event(&status_id).await.expect("cache").is_none(),
        "the finder saved what it read into the event cache"
    );
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
            &[NIXI],
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

/// The session's status as the host's device seals it, under a room key it
/// shares with nobody, and that device — which can hand the key over.
async fn sealed_status(
    room_id: &matrix_sdk::ruma::RoomId,
) -> (Value, matrix_sdk_crypto::OlmMachine) {
    use matrix_sdk::ruma::events::AnyMessageLikeEventContent;
    use matrix_sdk::ruma::serde::Raw;
    use matrix_sdk::ruma::{OwnedDeviceId, OwnedUserId};
    use matrix_sdk_crypto::{EncryptionSettings, OlmMachine};

    let nixi: OwnedUserId = NIXI.try_into().expect("user");
    let host_device: OwnedDeviceId = "HOST".into();
    let host = OlmMachine::new(&nixi, &host_device).await;
    host.share_room_key(room_id, std::iter::empty(), EncryptionSettings::default())
        .await
        .expect("a session");
    let status = status_event(900);
    let content: Raw<AnyMessageLikeEventContent> =
        Raw::from_json(serde_json::value::to_raw_value(&status["content"]).expect("content"));
    let sealed = host
        .encrypt_room_event_raw(room_id, STATUS, &content)
        .await
        .expect("sealed");
    let event = json!({
        "type": "m.room.encrypted", "sender": NIXI, "event_id": "$sealed:h",
        "origin_server_ts": 1_400, "content": sealed.content,
    });
    (event, host)
}

/// A device with the session room synced, its status not in the cache.
async fn synced_session_room() -> (Device, matrix_sdk::Room) {
    let device = Device::new(HashMap::new()).await;
    device.release_search();
    device
        .sync_cached(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![said("the last word", 1)],
            false,
            &[NIXI],
        ))
        .await;
    let room_id: OwnedRoomId = ROOM.try_into().expect("room");
    let room = device.client.get_room(&room_id).expect("the synced room");
    (device, room)
}

/// R513: the session's whole history is one status, encrypted under a room
/// key this device was never given. The header's search reads to the
/// room's beginning and finds no status it can read — incomplete, never
/// "searched empty" — so once the key arrives the room's next open searches
/// again and finds it. A history read whole without a status is empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_status_whose_key_arrives_late_is_searched_for_again() {
    use matrix_sdk::ruma::OwnedUserId;

    let (device, room) = synced_session_room().await;
    let room_id = room.room_id().to_owned();
    let me: OwnedUserId = ME.try_into().expect("user");
    let (sealed, host) = sealed_status(&room_id).await;
    device.serve_search(vec![sealed], None);

    let kinds = AgentKinds::default();
    let reader = HeaderReader::open(room.clone(), &kinds).await;
    assert!(
        !reader.state().has_status(),
        "a status read without its key"
    );
    assert!(
        !kinds.searched_empty(&me, &room_id),
        "an undecryptable history was taken for one with no status"
    );

    // The key arrives.
    let keys = host
        .store()
        .export_room_keys(|_| true)
        .await
        .expect("the host's keys");
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("keys.txt");
    let export = matrix_sdk_crypto::encrypt_room_key_export(&keys, "pass", 1).expect("export");
    std::fs::write(&file, export).expect("written");
    let imported = device
        .client
        .encryption()
        .import_room_keys(file, "pass")
        .await
        .expect("imported");
    assert_eq!(imported.imported_count, 1);

    let reader = HeaderReader::open(room.clone(), &kinds).await;
    assert!(
        reader.state().has_status(),
        "the status was not searched for again once its key arrived"
    );
    assert!(kinds.seat(&me, &room_id).is_some(), "its seat kept");

    // The same room's history readable whole and status-free.
    device.serve_search(vec![said("nothing else", 2)], None);
    let fresh = AgentKinds::default();
    HeaderReader::open(room, &fresh).await;
    assert!(
        fresh.searched_empty(&me, &room_id),
        "a readable history with no status is searched empty"
    );
}

/// R513: a page of the search holding nothing the device can decrypt is
/// not the room's beginning — the search reads on past it to the status.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_reads_on_past_a_page_it_cannot_decrypt() {
    let (device, room) = synced_session_room().await;
    let (sealed, _host) = sealed_status(room.room_id()).await;
    device.serve_search(vec![sealed], Some("older"));
    device.serve_page("older", page(vec![status_event(800)], None));

    let reader = HeaderReader::open(room, &AgentKinds::default()).await;
    assert!(
        reader.state().has_status(),
        "the search stopped at the page it could not decrypt"
    );
}

/// R513 × 102.5 (lane `102stack`): a look of the Agents list's seat finder
/// that meets an event it cannot decrypt never searches the room empty — not
/// when its own page reaches the beginning, and not when the next look, going
/// on from where it stopped, reads the rest of the history whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finder_look_past_an_undecryptable_event_never_searches_the_room_empty() {
    use matrix_sdk::ruma::OwnedUserId;

    let (device, room) = synced_session_room().await;
    let room_id = room.room_id().to_owned();
    let me: OwnedUserId = ME.try_into().expect("user");
    let (sealed, _host) = sealed_status(&room_id).await;
    let kinds = AgentKinds::default();

    device.serve_search(vec![sealed.clone()], None);
    assert_eq!(
        HeaderReader::find(room.clone(), &kinds, None, 20).await,
        Found::Unreadable
    );
    assert!(
        !kinds.searched_empty(&me, &room_id),
        "an undecryptable history was taken for one with no status"
    );

    device.serve_search(vec![sealed], Some("older"));
    device.serve_page("older", page(vec![said("nothing else", 2)], None));
    let first = HeaderReader::find(room.clone(), &kinds, None, 1).await;
    let Found::Further(resume) = first else {
        panic!("one page is the look's budget: {first:?}");
    };
    assert_eq!(
        HeaderReader::find(room, &kinds, Some(resume), 1).await,
        Found::Unreadable,
        "the look that read the rest whole forgot the page it could not decrypt"
    );
    assert!(!kinds.searched_empty(&me, &room_id));
}

/// Where the event cache's store pauses: once armed, the next duplicate
/// check of [`ROOM`]'s events — which the SDK runs while it holds the
/// cache's one lock, every room's, to conclude a page from the server —
/// waits until the gate opens.
#[derive(Default)]
struct Gate {
    armed: std::sync::atomic::AtomicBool,
    entered: tokio::sync::watch::Sender<bool>,
    open: tokio::sync::watch::Sender<bool>,
}

impl Gate {
    fn arm(&self) {
        self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn entered(&self) {
        let _ = self.entered.subscribe().wait_for(|entered| *entered).await;
    }

    fn open(&self) {
        self.open.send_replace(true);
    }

    async fn pass(&self) {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.entered.send_replace(true);
            let _ = self.open.subscribe().wait_for(|open| *open).await;
        }
    }
}

/// The in-memory event-cache store, paused by its [`Gate`].
#[derive(Debug)]
struct GatedStore {
    inner: matrix_sdk_base::event_cache::store::MemoryStore,
    gate: Arc<Gate>,
}

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gate")
    }
}

mod gated_store {
    use matrix_sdk::cross_process_lock::CrossProcessLockGeneration;
    use matrix_sdk::linked_chunk::{
        ChunkIdentifier, ChunkIdentifierGenerator, ChunkMetadata, LinkedChunkId, Position,
        RawChunk, Update,
    };
    use matrix_sdk::ruma::events::relation::RelationType;
    use matrix_sdk::ruma::{EventId, OwnedEventId, RoomId};
    use matrix_sdk_base::event_cache::store::{EventCacheStore, EventCacheStoreError};
    use matrix_sdk_base::event_cache::thread::ThreadInfo;
    use matrix_sdk_base::event_cache::{Event, Gap};

    use super::{GatedStore, ROOM};

    type Result<T> = std::result::Result<T, EventCacheStoreError>;

    #[matrix_sdk::async_trait]
    impl EventCacheStore for GatedStore {
        type Error = EventCacheStoreError;

        async fn try_take_leased_lock(
            &self,
            lease_duration_ms: u32,
            key: &str,
            holder: &str,
        ) -> Result<Option<CrossProcessLockGeneration>> {
            self.inner
                .try_take_leased_lock(lease_duration_ms, key, holder)
                .await
        }

        async fn handle_linked_chunk_updates(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
            updates: Vec<Update<Event, Gap>>,
        ) -> Result<()> {
            self.inner
                .handle_linked_chunk_updates(linked_chunk_id, updates)
                .await
        }

        async fn load_all_chunks(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
        ) -> Result<Vec<RawChunk<Event, Gap>>> {
            self.inner.load_all_chunks(linked_chunk_id).await
        }

        async fn load_all_chunks_metadata(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
        ) -> Result<Vec<ChunkMetadata>> {
            self.inner.load_all_chunks_metadata(linked_chunk_id).await
        }

        async fn load_last_chunk(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
        ) -> Result<(Option<RawChunk<Event, Gap>>, ChunkIdentifierGenerator)> {
            self.inner.load_last_chunk(linked_chunk_id).await
        }

        async fn load_previous_chunk(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
            before_chunk_identifier: ChunkIdentifier,
        ) -> Result<Option<RawChunk<Event, Gap>>> {
            self.inner
                .load_previous_chunk(linked_chunk_id, before_chunk_identifier)
                .await
        }

        async fn load_thread_info(
            &self,
            room_id: &RoomId,
            thread_id: &EventId,
        ) -> Result<ThreadInfo> {
            self.inner.load_thread_info(room_id, thread_id).await
        }

        async fn update_thread_info(
            &self,
            room_id: &RoomId,
            thread_id: &EventId,
            thread_info: &ThreadInfo,
        ) -> Result<()> {
            self.inner
                .update_thread_info(room_id, thread_id, thread_info)
                .await
        }

        async fn clear_all_events(&self, room_id: Option<&RoomId>) -> Result<()> {
            self.inner.clear_all_events(room_id).await
        }

        async fn filter_duplicated_events(
            &self,
            linked_chunk_id: LinkedChunkId<'_>,
            events: Vec<OwnedEventId>,
        ) -> Result<Vec<(OwnedEventId, Position)>> {
            if matches!(linked_chunk_id, LinkedChunkId::Room(room) if room.as_str() == ROOM) {
                self.gate.pass().await;
            }
            self.inner
                .filter_duplicated_events(linked_chunk_id, events)
                .await
        }

        async fn find_event(&self, room_id: &RoomId, event_id: &EventId) -> Result<Option<Event>> {
            self.inner.find_event(room_id, event_id).await
        }

        async fn find_event_relations(
            &self,
            room_id: &RoomId,
            event_id: &EventId,
            filter: Option<&[RelationType]>,
        ) -> Result<Vec<(Event, Option<Position>)>> {
            self.inner
                .find_event_relations(room_id, event_id, filter)
                .await
        }

        async fn get_room_events(
            &self,
            room_id: &RoomId,
            event_type: Option<&str>,
            session_id: Option<&str>,
        ) -> Result<Vec<Event>> {
            self.inner
                .get_room_events(room_id, event_type, session_id)
                .await
        }

        async fn save_event(&self, room_id: &RoomId, event: Event) -> Result<()> {
            self.inner.save_event(room_id, event).await
        }

        async fn close(&self) -> Result<()> {
            self.inner.close().await
        }

        async fn reopen(&self) -> Result<()> {
            self.inner.reopen().await
        }

        async fn optimize(&self) -> Result<()> {
            self.inner.optimize().await
        }

        async fn get_size(&self) -> Result<Option<usize>> {
            self.inner.get_size().await
        }
    }
}

const OTHER: &str = "!other:h";

/// How many events `room`'s cache holds.
async fn cached(client: &Client, room: &str) -> usize {
    let room_id: OwnedRoomId = room.try_into().expect("room");
    let room = client.get_room(&room_id).expect("room");
    let (cache, _handles) = room.event_cache().await.expect("room cache");
    cache.events().await.expect("cached events").len()
}

/// matrix-sdk 0.19 keeps one event-cache state for every room (#6629) and
/// holds its lock across its store reads while it concludes a page. The
/// producer keeps its fill page unpolled while it is busy in a branch that
/// reads the cache — an approval refresh — so a page paused there must go
/// on by itself once the store answers: the refresh ends, another room's
/// cache is read (as admitting a room does), and the page lands in the
/// timeline. It does because the SDK runs each back-pagination on a task
/// of its own (`event_cache/caches/pagination.rs`); this fails if that stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fill_page_paused_in_the_cache_goes_on_while_its_producer_reads_the_cache() {
    use crate::agents::room::approvals_of;

    let gate = Arc::new(Gate::default());
    let pages = HashMap::from([("p1".to_owned(), page(vec![said("older", 3)], None))]);
    let device = Device::gated(pages, Arc::clone(&gate)).await;
    device.release_search();
    let mut body = sync(
        1,
        ROOM,
        room_state(NIXI),
        (100..112).map(|n| said(&format!("m{n}"), n)).collect(),
        true,
        &[NIXI],
    );
    body["rooms"]["join"][OTHER] = sync(
        1,
        OTHER,
        room_state(NIXI),
        vec![said("elsewhere", 1)],
        false,
        &[],
    )["rooms"]["join"][OTHER]
        .clone();
    device.sync_cached(body).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while cached(&device.client, OTHER).await < 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the other room never cached"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let room_id: OwnedRoomId = ROOM.try_into().expect("room");
    let other_id: OwnedRoomId = OTHER.try_into().expect("room");
    let open = open_timeline(&device.client, &room_id, "acct")
        .await
        .expect("timeline");
    let timeline = open.timeline();

    gate.arm();
    let mut page = super::fill_page(&timeline);
    tokio::select! {
        paged = &mut page => panic!("the page ended before its store read: {paged:?}"),
        () = gate.entered() => {}
    }
    // The page holds the cache's lock and the producer, in another branch,
    // reads the cache; `page` is not polled from here on, as the producer
    // does not poll it meanwhile.
    let (room, other) = (
        device.client.get_room(&room_id).expect("room"),
        device.client.get_room(&other_id).expect("other room"),
    );
    let refresh = tokio::spawn(async move {
        approvals_of(&room).await;
        approvals_of(&other).await;
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !refresh.is_finished(),
        "the page is paused under the cache's lock, so the refresh waits"
    );
    gate.open();
    tokio::time::timeout(Duration::from_secs(10), refresh)
        .await
        .expect("the cache stayed locked by a page nobody polled")
        .expect("the refresh");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while cached(&device.client, ROOM).await < 13 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the page never landed while nobody polled it"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(page.await.is_ok(), "the page");
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
            &[NIXI],
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
