//! A session room's briefs as the producer draws them, against a loopback
//! homeserver: the SDK's own store, timeline and member sync behind
//! [`forward_timeline`] (R114, R117). Each test drives `/sync` by hand and
//! holds, fails or serves `/members` to put the device where it needs to be.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use serde_json::{json, Value};

use super::{forward_timeline, open_timeline};
use crate::agents::approval_card::HostedRooms;
use crate::agents::delegation::{
    brief_content, DelegateCard, DelegateContent, DelegateFrom, DelegateLimits,
};
use crate::agents::events::{CONTENT_VERSION, SESSION_ROOM_TYPE};
use crate::agents::label::{Integrity, Label, Readers};
use crate::agents::room::{AgentIcons, AgentKinds, BriefVm};
use crate::forges::testing::{self, Reply, Seen};
use crate::vm::{TimelineBatch, TimelineItemVm, TimelineOp};

const ME: &str = "@tgorka:h";
const NIXI: &str = "@nixi:h";
const TOLA: &str = "@tola:h";
const MARTA: &str = "@marta:h";
const OZZY: &str = "@ozzy:h";
const ROOM: &str = "!brief:h";

/// What `/members` answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Members {
    /// Nothing yet: the request waits.
    Held,
    /// A server error.
    Fail,
    /// The person and both agents, joined.
    Serve,
}

struct Server {
    syncs: VecDeque<Value>,
    members: Members,
}

type Shared = Arc<(Mutex<Server>, Condvar)>;

fn answer(shared: &Shared, seen: &Seen) -> Reply {
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
    if path.ends_with("/members") {
        let (lock, wake) = &**shared;
        let mut server = lock.lock().expect("server");
        while server.members == Members::Held {
            server = wake.wait(server).expect("server");
        }
        return match server.members {
            Members::Fail => Reply::json(500, r#"{"errcode":"M_UNKNOWN","error":"down"}"#),
            _ => {
                let chunk: Vec<Value> = [ME, NIXI, TOLA]
                    .iter()
                    .enumerate()
                    .map(|(n, user)| member(user, user, "join", 900 + n as u64))
                    .collect();
                Reply::json(200, &json!({ "chunk": chunk }).to_string())
            }
        };
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
    if path.ends_with("/messages") {
        return Reply::json(200, r#"{"chunk":[],"start":"t0"}"#);
    }
    Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
}

fn set_members(shared: &Shared, members: Members) {
    shared.0.lock().expect("server").members = members;
    shared.1.notify_all();
}

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

fn levels(users: Value, n: u64) -> Value {
    state(
        "m.room.power_levels",
        "",
        NIXI,
        json!({ "users": users, "users_default": 0, "events_default": 0 }),
        n,
    )
}

/// The state of a session room `creator` made, at 100, with the person and
/// Tola in it: what a lazy-loading sync hands over, not the whole list.
fn room_state(creator: &str) -> Vec<Value> {
    vec![
        state(
            "m.room.create",
            "",
            creator,
            json!({ "creator": creator, "room_version": "11", "type": SESSION_ROOM_TYPE }),
            1,
        ),
        levels(json!({ creator: 100, TOLA: 50 }), 2),
        member(creator, creator, "join", 3),
        member(ME, ME, "join", 4),
        member(TOLA, TOLA, "join", 5),
    ]
}

fn message(sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": "m.room.message", "sender": sender, "content": content,
        "event_id": format!("$m{n}:h"), "origin_server_ts": 2_000 + n,
    })
}

/// `from`'s delegation to Tola, saying `brief`, under a label `readers`
/// read.
fn delegation(from: &str, brief: &str, readers: &[&str]) -> DelegateContent {
    DelegateContent {
        v: CONTENT_VERSION,
        id: ulid::Ulid::new().to_string(),
        from: DelegateFrom {
            agent: from.try_into().expect("user"),
            drive: "tgdrive".to_owned(),
            session: "active/2026-10-04-chat".to_owned(),
            room: "!parent:h".try_into().expect("room"),
        },
        to: TOLA.try_into().expect("user"),
        brief: brief.to_owned(),
        drives: vec!["tgdrive".to_owned()],
        label: Label {
            readers: Readers::Only(
                readers
                    .iter()
                    .map(|r| (*r).try_into().expect("user"))
                    .collect(),
            ),
            integrity: Integrity::Agent,
            local_only: false,
        },
        hop: 1,
        limits: DelegateLimits {
            rounds_per_exchange: 3,
            tokens: 1_000,
        },
        card: Some(DelegateCard {
            title: "Sync chapter".to_owned(),
            schedule: None,
            workflow: None,
        }),
        dispatch_chain: Vec::new(),
    }
}

fn brief(from: &str, text: &str, readers: &[&str]) -> Value {
    brief_content(&delegation(from, text, readers))
}

/// A sync carrying `room`'s `state` and `timeline`, and the person's proxy
/// list when `listed` names any.
fn sync(
    n: u64,
    room: &str,
    state: Vec<Value>,
    timeline: Vec<Value>,
    limited: bool,
    listed: &[&str],
) -> Value {
    let mut body = json!({
        "next_batch": format!("s{n}"),
        "rooms": { "join": { room: {
            "state": { "events": state },
            "timeline": { "events": timeline, "limited": limited, "prev_batch": format!("p{n}") },
        }}},
    });
    if !listed.is_empty() {
        body["account_data"] = json!({ "events": [{
            "type": "dev.keeper.agent.proxies", "content": { "v": 1, "agents": listed },
        }]});
    }
    body
}

struct Device {
    shared: Shared,
    fake: testing::Fake,
    client: Client,
}

impl Device {
    async fn new(members: Members) -> Device {
        let shared: Shared = Arc::new((
            Mutex::new(Server {
                syncs: VecDeque::new(),
                members,
            }),
            Condvar::new(),
        ));
        let handler = Arc::clone(&shared);
        let fake = testing::serve(move |seen| answer(&handler, seen));
        let client = Client::builder()
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

    async fn sync(&self, body: Value) {
        self.shared.0.lock().expect("server").syncs.push_back(body);
        self.client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
    }

    fn asked_members(&self) -> bool {
        self.fake
            .requests()
            .iter()
            .any(|seen| seen.path.ends_with("/members"))
    }

    /// The room's timeline as the frontend holds it, fed by the producer.
    async fn open(&self, room: &str, icons: Arc<AgentIcons>) -> Drawn {
        let room: OwnedRoomId = room.try_into().expect("room");
        let open = open_timeline(&self.client, &room, "acct")
            .await
            .expect("timeline");
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
            icons,
            Arc::new(HostedRooms::default()),
        ));
        Drawn { batches, producer }
    }
}

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

    /// The message whose body is `body`, as drawn now: `Some(brief)`.
    fn message(&self, body: &str) -> Option<Option<BriefVm>> {
        self.items().into_iter().find_map(|item| match item {
            TimelineItemVm::Message {
                body: drawn, brief, ..
            } if drawn == body => Some(brief.map(|brief| *brief)),
            _ => None,
        })
    }

    /// Every way the message whose body is `body` has been drawn, batch by
    /// batch.
    fn every_drawing(&self, body: &str) -> Vec<Option<BriefVm>> {
        let batches = self.batches.lock().expect("batches");
        let mut drawn = Vec::new();
        for batch in batches.iter() {
            for op in &batch.ops {
                let items: Vec<&TimelineItemVm> = match op {
                    TimelineOp::Reset { items } | TimelineOp::Append { items } => {
                        items.iter().collect()
                    }
                    TimelineOp::PushFront { item }
                    | TimelineOp::PushBack { item }
                    | TimelineOp::Insert { item, .. }
                    | TimelineOp::Set { item, .. } => vec![item],
                    _ => Vec::new(),
                };
                for item in items {
                    if let TimelineItemVm::Message {
                        body: text, brief, ..
                    } = item
                    {
                        if text == body {
                            drawn.push(brief.as_deref().cloned());
                        }
                    }
                }
            }
        }
        drawn
    }

    async fn until(&self, what: &str, done: impl Fn(&Drawn) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done(self) {
            assert!(
                Instant::now() < deadline,
                "never: {what}: {:#?}",
                self.items()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

const SYNC_CHAPTER: &str = "Review the sync chapter.";

fn narrowed(drawn: &Option<Option<BriefVm>>) -> bool {
    matches!(drawn, Some(Some(brief)) if brief.narrowed && brief.title.is_none() && brief.drives.is_empty())
}

fn whole(drawn: &Option<Option<BriefVm>>) -> bool {
    matches!(drawn, Some(Some(brief)) if !brief.narrowed && brief.title.as_deref() == Some("Sync chapter"))
}

/// R117 (R3-07): a cached session room is drawn while its member list is
/// still being fetched — its brief narrowed, as nothing yet says who reads
/// the room — and the brief is drawn whole once the list arrives. On a
/// phone, the person's own proxy list is how it knows Nixi (R114).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_batch_does_not_wait_for_the_members() {
    let device = Device::new(Members::Held).await;
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![message(NIXI, brief(NIXI, SYNC_CHAPTER, &[ME]), 1)],
            false,
            &[NIXI],
        ))
        .await;
    let drawn = device.open(ROOM, Arc::new(AgentIcons::default())).await;
    drawn
        .until("the first batch", |drawn| {
            drawn.message(SYNC_CHAPTER).is_some()
        })
        .await;
    // The fetch is held: what is drawn came without it.
    assert!(
        narrowed(&drawn.message(SYNC_CHAPTER)),
        "{:?}",
        drawn.message(SYNC_CHAPTER)
    );
    drawn
        .until("the member fetch asked", |_| device.asked_members())
        .await;
    set_members(&device.shared, Members::Serve);
    drawn
        .until("the brief drawn whole", |drawn| {
            whole(&drawn.message(SYNC_CHAPTER))
        })
        .await;
}

/// R117 (R3-02): a member fetch that fails leaves the roster unknown. A
/// later read of the store — which holds only the members a sync carried,
/// here exactly the label's readers — never counts as the whole list, so
/// the brief stays narrowed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_member_fetch_never_widens_a_brief() {
    let device = Device::new(Members::Fail).await;
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![message(NIXI, brief(NIXI, SYNC_CHAPTER, &[ME]), 1)],
            false,
            &[NIXI],
        ))
        .await;
    let drawn = device.open(ROOM, Arc::new(AgentIcons::default())).await;
    drawn
        .until("the member fetch asked", |_| device.asked_members())
        .await;
    // The room changes: the store is read again.
    device
        .sync(sync(
            2,
            ROOM,
            Vec::new(),
            vec![message(
                NIXI,
                json!({"msgtype": "m.text", "body": "Working."}),
                2,
            )],
            false,
            &[],
        ))
        .await;
    drawn
        .until("the next message", |drawn| {
            drawn.message("Working.").is_some()
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let every = drawn.every_drawing(SYNC_CHAPTER);
    assert!(!every.is_empty());
    for drawing in every {
        assert!(narrowed(&Some(drawing.clone())), "{drawing:?}");
    }
}

/// R117 (R3-01, R3-02): a loaded brief is drawn again when what it rests on
/// changes, with nothing else arriving: an invite in the room's state alone
/// narrows it, the outsider leaving widens it again; a gap (a limited sync)
/// forgets the whole member list, so the brief its window brings back is
/// narrowed until the members are fetched again; its creator demoted below
/// an agent's power, in state alone, makes it an ordinary message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_of_members_or_power_draws_a_loaded_brief_again() {
    let device = Device::new(Members::Serve).await;
    let held_brief = message(NIXI, brief(NIXI, SYNC_CHAPTER, &[ME]), 1);
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![held_brief.clone()],
            false,
            &[NIXI],
        ))
        .await;
    let drawn = device.open(ROOM, Arc::new(AgentIcons::default())).await;
    drawn
        .until("the brief drawn whole", |drawn| {
            whole(&drawn.message(SYNC_CHAPTER))
        })
        .await;

    // Marta invited, in state alone.
    device
        .sync(sync(
            2,
            ROOM,
            vec![member(MARTA, NIXI, "invite", 10)],
            Vec::new(),
            false,
            &[],
        ))
        .await;
    drawn
        .until("narrowed by the invite", |drawn| {
            narrowed(&drawn.message(SYNC_CHAPTER))
        })
        .await;

    // Marta leaves; the list is whole again.
    device
        .sync(sync(
            3,
            ROOM,
            vec![member(MARTA, MARTA, "leave", 11)],
            Vec::new(),
            false,
            &[],
        ))
        .await;
    drawn
        .until("whole again", |drawn| whole(&drawn.message(SYNC_CHAPTER)))
        .await;

    // A gap: the member list is no longer known whole, and its fetch waits.
    // The limited sync's window holds the brief again, so the timeline,
    // which drops what came before a gap, draws it anew.
    set_members(&device.shared, Members::Held);
    device
        .sync(sync(
            4,
            ROOM,
            Vec::new(),
            vec![
                held_brief,
                message(
                    NIXI,
                    json!({"msgtype": "m.text", "body": "After the gap."}),
                    4,
                ),
            ],
            true,
            &[],
        ))
        .await;
    drawn
        .until("narrowed by the gap", |drawn| {
            drawn.message("After the gap.").is_some() && narrowed(&drawn.message(SYNC_CHAPTER))
        })
        .await;
    // Its fetch done, the list is whole again.
    set_members(&device.shared, Members::Serve);
    drawn
        .until("whole after the fetch", |drawn| {
            whole(&drawn.message(SYNC_CHAPTER))
        })
        .await;

    // Nixi demoted, in state alone: the brief is an ordinary message.
    device
        .sync(sync(
            5,
            ROOM,
            vec![levels(json!({ TOLA: 50 }), 12)],
            Vec::new(),
            false,
            &[],
        ))
        .await;
    drawn
        .until("demoted", |drawn| {
            matches!(drawn.message(SYNC_CHAPTER), Some(None))
        })
        .await;
}

/// R114 (R3-03, R3-04, R3-05), through the SDK's own items: in the room
/// Nixi made, only its original `m.text` saying what it hands on is a
/// brief — not a notice, an emote, an image around the delegation, a body
/// that says something else, or a brief edited since. On a Mac whose zone
/// holds Nixi it is drawn with no proxy list; a room an agent this device
/// does not know made, at 100, draws its brief as an ordinary message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_an_original_text_brief_from_a_known_agent_is_drawn_as_one() {
    let device = Device::new(Members::Serve).await;
    let with_delegate = |msgtype: &str, text: &str| {
        let mut content = brief(NIXI, text, &[ME]);
        content["msgtype"] = json!(msgtype);
        if msgtype == "m.image" {
            content["url"] = json!("mxc://h/picture");
        }
        content
    };
    let mut other_text = brief(NIXI, "Review the inbox.", &[ME]);
    other_text["body"] = json!("Something else.");
    let edit = json!({
        "msgtype": "m.text", "body": "* Delete the archive.",
        "m.new_content": { "msgtype": "m.text", "body": "Delete the archive." },
        "m.relates_to": { "rel_type": "m.replace", "event_id": "$m7:h" },
    });
    device
        .sync(sync(
            1,
            ROOM,
            room_state(NIXI),
            vec![
                message(NIXI, brief(NIXI, SYNC_CHAPTER, &[ME]), 1),
                message(NIXI, with_delegate("m.notice", "A notice."), 2),
                message(NIXI, with_delegate("m.emote", "An emote."), 3),
                message(NIXI, with_delegate("m.image", "A picture."), 4),
                message(NIXI, other_text, 5),
                message(NIXI, brief(NIXI, "Sort the inbox.", &[ME]), 7),
                message(NIXI, edit, 8),
            ],
            false,
            &[],
        ))
        .await;
    let icons = Arc::new(AgentIcons::default());
    icons.replace([(NIXI.try_into().expect("user"), None)].into());
    let drawn = device.open(ROOM, Arc::clone(&icons)).await;
    drawn
        .until("the genuine brief whole", |drawn| {
            whole(&drawn.message(SYNC_CHAPTER))
        })
        .await;
    for body in [
        "A notice.",
        "An emote.",
        "Something else.",
        "Delete the archive.",
    ] {
        assert_eq!(drawn.message(body), Some(None), "{body}");
    }
    // The image, drawn by its attachment (its body is a file name, not a
    // caption): an ordinary media message.
    let media: Vec<bool> = drawn
        .items()
        .into_iter()
        .filter_map(|item| match item {
            TimelineItemVm::Message {
                media: Some(_),
                brief,
                ..
            } => Some(brief.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(media, [false]);

    // Ozzy, no agent of this device's, made its room and holds 100 there.
    let ozzys = "!ozzy:h";
    device
        .sync(sync(
            2,
            ozzys,
            room_state(OZZY),
            vec![message(OZZY, brief(OZZY, "Hand me the keys.", &[ME]), 20)],
            false,
            &[],
        ))
        .await;
    let theirs = device.open(ozzys, Arc::clone(&icons)).await;
    theirs
        .until("Ozzy's message", |drawn| {
            drawn.message("Hand me the keys.").is_some()
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(theirs
        .every_drawing("Hand me the keys.")
        .iter()
        .all(Option::is_none));
}
