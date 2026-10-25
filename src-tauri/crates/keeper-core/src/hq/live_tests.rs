//! The live answer: the fold under every order its two paths can arrive
//! in (property-style, over a writer that streams the way the agent's sink
//! does), and an opened session's view over a real on-disk replica — the
//! final row replacing the chunks, details absent where unreported,
//! thinking only for a reader who shows it, a stopped instance, the tool
//! switches and the trust label.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::hq::port::{Peer, ReplicaSpec, RowOp};
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, Opened};

const TURN: &str = "t1";
const INSTANCE: &str = "alpha.box";

/// A small deterministic generator, so a failing case names its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, in_ten: usize) -> bool {
        self.below(10) < in_ten
    }
}

fn session_doc(from_seq: i64, tail: &str) -> Peer {
    Peer {
        actor_id: "alpha".to_owned(),
        client_id: INSTANCE.to_owned(),
        doc: json!({"turn": TURN, "from_seq": from_seq, "text": tail, "tool": null}),
    }
}

/// What the agent's sink produced for one answer: its commits in order,
/// its session documents in publish order, and the whole answer.
struct Written {
    chunks: Vec<(i64, String)>,
    docs: Vec<(i64, String)>,
    answer: String,
}

/// Stream an answer the sink's way: the anchor at seq 0, a document on
/// every delta, and now and then the tail committed under its `from_seq`
/// and the next document starting empty at the seq after.
fn write(rng: &mut Rng) -> Written {
    let words = ["ana ", "ben ", "żółw ", "x", "\n", "ąę ", "🙂", "seq "];
    let mut chunks = vec![(0, String::new())];
    let mut docs = vec![(1, String::new())];
    let (mut seq, mut tail, mut answer) = (1, String::new(), String::new());
    for _ in 0..(5 + rng.below(40)) {
        let delta = words[rng.below(words.len())];
        tail.push_str(delta);
        answer.push_str(delta);
        docs.push((seq, tail.clone()));
        if rng.chance(2) {
            chunks.push((seq, std::mem::take(&mut tail)));
            seq += 1;
            docs.push((seq, String::new()));
        }
    }
    Written {
        chunks,
        docs,
        answer,
    }
}

/// What the viewer shows of the turn: the rule over the chunks it holds.
fn shown(live: &Live, held: &[(i64, String)]) -> String {
    let empty = Tails::default();
    fold(
        held.iter().map(|(seq, text)| (*seq, text.as_str())),
        0,
        live.turns.get(TURN).map_or(&empty, |turn| &turn.text),
    )
}

fn observe(live: &mut Live, doc: Option<&(i64, String)>) {
    let peers: Vec<Peer> = doc
        .map(|(from, tail)| session_doc(*from, tail))
        .into_iter()
        .collect();
    live.observe(&peers, &[], None, Instant::now(), 0);
}

/// One run: the chunks and the documents arrive interleaved at random, the
/// chunks themselves out of order when `shuffle_chunks`, the documents
/// coalesced (a viewer reads presence when it next draws, not on every
/// publish), reordered when `shuffle_docs`, and now and then absent (a
/// leave and a rejoin). After every arrival what is shown is a prefix of
/// the answer and extends what was shown before; once everything arrived
/// and the instance left, it is the whole answer.
fn run(seed: u64, shuffle_chunks: bool, shuffle_docs: bool) {
    let mut rng = Rng(seed);
    let written = write(&mut rng);
    let mut chunks = written.chunks.clone();
    let mut docs = written.docs.clone();
    if shuffle_chunks {
        for i in (1..chunks.len()).rev() {
            chunks.swap(i, rng.below(i + 1));
        }
    }
    if shuffle_docs {
        for i in (1..docs.len()).rev() {
            docs.swap(i, rng.below(i + 1));
        }
    }
    let mut live = Live::default();
    let mut held: Vec<(i64, String)> = Vec::new();
    let (mut next_chunk, mut next_doc) = (0, 0);
    let mut current: Option<&(i64, String)> = None;
    let mut before = String::new();
    while next_chunk < chunks.len() || next_doc < docs.len() {
        let take_chunk = next_doc == docs.len() || (next_chunk < chunks.len() && rng.chance(4));
        if take_chunk {
            held.push(chunks[next_chunk].clone());
            next_chunk += 1;
        } else {
            for _ in 0..=rng.below(3) {
                if next_doc < docs.len() {
                    current = Some(&docs[next_doc]);
                    next_doc += 1;
                }
            }
        }
        let present = if rng.chance(1) { None } else { current };
        observe(&mut live, present);
        let now = shown(&live, &held);
        assert!(
            written.answer.starts_with(&now),
            "seed {seed}: {now:?} is not a prefix of {:?}",
            written.answer
        );
        assert!(
            now.starts_with(&before),
            "seed {seed}: {before:?} was shown, then {now:?}"
        );
        before = now;
    }
    // The publishes end with the last tail; a shuffled run saw them all.
    observe(
        &mut live,
        docs.iter().max_by_key(|(from, tail)| (*from, tail.len())),
    );
    observe(&mut live, None);
    assert_eq!(
        shown(&live, &held),
        written.answer,
        "seed {seed}: everything arrived, and the instance left"
    );
}

#[test]
fn the_fold_shows_a_growing_prefix_whatever_order_the_chunks_and_tails_come_in() {
    for seed in 1..=400u64 {
        let seed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        run(seed, false, false);
        run(seed, true, false);
        run(seed, false, true);
        run(seed, true, true);
    }
}

#[test]
fn the_fold_by_cases() {
    let tails = |pairs: &[(i64, &str)]| {
        let mut tails = Tails::default();
        for (from, tail) in pairs {
            tails.see(*from, tail);
        }
        tails
    };
    // Overlap: the tail of a seq whose chunk is held is not drawn again.
    assert_eq!(
        fold([(0, ""), (1, "ab")], 0, &tails(&[(1, "ab"), (2, "c")])),
        "abc"
    );
    // Reordering: chunks in any order.
    assert_eq!(
        fold([(2, "c"), (0, ""), (1, "ab")], 0, &tails(&[(3, "d")])),
        "abcd"
    );
    // A missing chunk: nothing past the gap, not even its tail.
    assert_eq!(fold([(0, ""), (2, "c")], 0, &tails(&[(3, "d")])), "");
    assert_eq!(
        fold(
            [(0, ""), (1, "ab"), (3, "d")],
            0,
            &tails(&[(2, "c"), (4, "e")])
        ),
        "abc",
        "the tail kept for the missing seq, nothing after it"
    );
    // A tail older than the last chunk: its text is in the chunk already.
    assert_eq!(
        fold(
            [(0, ""), (1, "ab"), (2, "cd")],
            0,
            &tails(&[(1, "a"), (2, "c")])
        ),
        "abcd"
    );
    // A tail ahead of its chunks waits for them.
    assert_eq!(fold([(0, "")], 0, &tails(&[(2, "zz")])), "");
    // A stale copy of a tail does not shrink it; a different one replaces it.
    assert_eq!(fold([(0, "")], 0, &tails(&[(1, "abc"), (1, "ab")])), "abc");
    assert_eq!(fold([(0, "")], 0, &tails(&[(1, "abc"), (1, "xy")])), "xy");
}

#[test]
fn a_tail_is_kept_through_a_leave_until_its_chunk_arrives() {
    let mut live = Live::default();
    observe(&mut live, Some(&(1, "hello wor".to_owned())));
    observe(&mut live, None);
    assert_eq!(shown(&live, &[(0, String::new())]), "hello wor");
    // The chunk arrives later than the leave: it replaces the tail.
    assert_eq!(
        shown(&live, &[(0, String::new()), (1, "hello world".to_owned())]),
        "hello world"
    );
}

#[test]
fn a_holder_that_stops_is_said_once_its_claim_stands_still_for_the_lease() {
    let claim = |renewed: i64, released: bool, holder: &str| Claim {
        holder_client: holder.to_owned(),
        epoch: 1,
        renewed_at_ms: renewed,
        released,
    };
    let start = Instant::now();
    let now_ms = 1_800_000_000_000;
    let mut live = Live::default();
    let doc = [session_doc(1, "half")];
    live.observe(
        &doc,
        &[],
        Some(claim(now_ms, false, INSTANCE)),
        start,
        now_ms,
    );
    assert!(!live.stopped(TURN, None), "present");
    live.observe(
        &[],
        &[],
        Some(claim(now_ms, false, INSTANCE)),
        start + Duration::from_secs(29),
        now_ms,
    );
    assert!(
        !live.stopped(TURN, None),
        "left, the claim still in its lease"
    );
    live.observe(
        &[],
        &[],
        Some(claim(now_ms, false, INSTANCE)),
        start + LEASE,
        now_ms,
    );
    assert!(live.stopped(TURN, None), "the lease passed unrenewed");
    // A renewal is a new row: the lease starts again from it.
    live.observe(
        &[],
        &[],
        Some(claim(now_ms + 10_000, false, INSTANCE)),
        start + LEASE,
        now_ms,
    );
    assert!(!live.stopped(TURN, None));
    live.observe(
        &[],
        &[],
        Some(claim(now_ms + 10_000, true, INSTANCE)),
        start + LEASE,
        now_ms,
    );
    assert!(live.stopped(TURN, None), "released");
    live.observe(
        &[],
        &[],
        Some(claim(now_ms, false, "beta.box")),
        start,
        now_ms,
    );
    assert!(live.stopped(TURN, None), "another instance took it");
    let mut late = Live::default();
    late.observe(
        &[],
        &[],
        Some(claim(now_ms - STALE_MS, false, INSTANCE)),
        start,
        now_ms,
    );
    assert!(
        late.stopped(TURN, Some(INSTANCE)),
        "opened long after its holder stopped"
    );
    // No claim row to judge by: a document away a whole lease is a stop too.
    let mut unclaimed = Live::default();
    unclaimed.observe(&doc, &[], None, start, now_ms);
    unclaimed.observe(&[], &[], None, start + Duration::from_secs(29), now_ms);
    assert!(
        !unclaimed.stopped(TURN, Some(INSTANCE)),
        "left, not yet a lease ago"
    );
    unclaimed.observe(&[], &[], None, start + LEASE, now_ms);
    assert!(
        unclaimed.stopped(TURN, Some(INSTANCE)),
        "away a whole lease with no claim"
    );
}

#[test]
fn a_document_that_leaves_takes_its_tool_with_it() {
    let calling = Peer {
        doc: json!({"turn": TURN, "from_seq": 1, "text": "half", "tool": "web_search"}),
        ..session_doc(1, "half")
    };
    let mut live = Live::default();
    let now = Instant::now();
    live.observe(&[calling], &[], None, now, 0);
    assert_eq!(
        live.turns.get(TURN).and_then(|t| t.tool.as_deref()),
        Some("web_search")
    );
    live.observe(&[], &[], None, now, 0);
    let turn = live.turns.get(TURN).expect("kept through the leave");
    assert_eq!(
        (
            turn.tool.as_deref(),
            turn.text.0.get(&1).map(String::as_str)
        ),
        (None, Some("half"))
    );
}

// ---------------------------------------------------------------------------
// The view over a real replica
// ---------------------------------------------------------------------------

struct World {
    _dir: tempfile::TempDir,
    source: Source,
    at: i64,
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

/// `ana` in session `dev1.1` with agent `alpha`, reading its trace or not.
fn world(reads_trace: bool) -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: dir.path().join("alpha.sqlite"),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    let source = Source {
        database: "alpha".to_owned(),
        replica: Arc::new(replica),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    };
    let at = sessions::now_ms() - 60_000;
    source
        .replica
        .mutate(vec![
            upsert(
                "actors",
                json!({"id": "alpha", "kind": "agent", "display_name": "Alpha", "family": null,
                       "dir": "alpha", "created_at_ms": 1, "author_actor": "server",
                       "author_client": "server"}),
            ),
            upsert(
                "sessions",
                json!({"id": "dev1.1", "title": "t", "status": "open", "agent_actor": "alpha",
                       "created_by_actor": "ana", "created_by_client": "dev1",
                       "preferred_client": null, "parent_session": null, "invite": "[]",
                       "archived": false, "heard_until": null, "created_at_ms": at,
                       "last_activity_ms": at, "trust": null, "author_actor": "ana",
                       "author_client": "dev1"}),
            ),
            upsert(
                "session_members",
                json!({"id": "dev1.1/ana", "session_id": "dev1.1", "actor_id": "ana",
                       "role": "person", "reads_trace": reads_trace, "added_by_actor": "ana",
                       "added_at_ms": at, "read_until": null, "author_actor": "ana",
                       "author_client": "dev1"}),
            ),
        ])
        .expect("seed");
    World {
        _dir: dir,
        source,
        at,
    }
}

impl World {
    fn put(&self, ops: Vec<RowOp>) {
        self.source.replica.mutate(ops).expect("written");
    }
    fn chunk(&self, seq: i64, text: &str) -> RowOp {
        upsert(
            "turn_chunks",
            json!({"id": format!("k{seq}"), "session_id": "dev1.1", "turn_id": TURN,
                   "seq": seq, "text": text, "created_at_ms": self.at + seq,
                   "author_actor": "alpha", "author_client": INSTANCE}),
        )
    }
    fn page(&self) -> String {
        sessions::page_key("dev1.1", &sessions::month(self.at))
    }
    fn reasoning(&self, id: &str, seq: i64, compacted: bool, text: &str) -> RowOp {
        upsert(
            "reasoning",
            json!({"id": id, "session_id": "dev1.1", "page_key": self.page(), "turn_id": TURN,
                   "seq": seq, "compacted": compacted, "text": text, "created_at_ms": self.at,
                   "author_actor": "alpha", "author_client": INSTANCE}),
        )
    }
    fn tool_call(&self, status: &str, result: Option<&str>) -> RowOp {
        upsert(
            "tool_calls",
            json!({"id": "c1", "session_id": "dev1.1", "page_key": self.page(),
                   "turn_id": TURN, "name": "web_search", "args": "{}", "status": status,
                   "result": result, "result_blob": null, "started_at_ms": self.at,
                   "ended_at_ms": result.map(|_| self.at + 5), "created_at_ms": self.at,
                   "author_actor": "alpha", "author_client": INSTANCE}),
        )
    }
    fn answer(&self, body: &str, metadata: Value) -> RowOp {
        upsert(
            "messages",
            json!({"id": "m-final", "session_id": "dev1.1", "page_key": self.page(),
                   "turn_id": TURN, "role": "agent", "body": body, "body_blob": null,
                   "metadata": metadata.to_string(), "created_at_ms": self.at + 100,
                   "author_actor": "alpha", "author_client": INSTANCE}),
        )
    }
    fn profile(&self, id: &str, tools: Option<&[&str]>) -> RowOp {
        upsert(
            "agent_profiles",
            json!({"id": id, "actor_id": "alpha", "name": "Alpha", "description": "",
                   "avatar_blob": null, "definition_hash": "h", "image_digest": "d",
                   "started_at_ms": self.at, "dir": "alpha",
                   "tools": tools.map(|t| serde_json::to_string(t).expect("json")),
                   "author_actor": "alpha", "author_client": id}),
        )
    }
    fn claim(&self, holder: &str) -> RowOp {
        upsert(
            "claims",
            json!({"session_id": "dev1.1", "holder_actor": "alpha", "holder_client": holder,
                   "epoch": 1, "renewed_at_ms": sessions::now_ms(), "released": false,
                   "author_actor": "alpha", "author_client": holder}),
        )
    }
    fn view(&self, opened: &Opened) -> timeline::AgentSessionViewVm {
        timeline::view(
            &self.source,
            "dev1.1",
            opened,
            &HashMap::new(),
            sessions::now_ms(),
        )
        .expect("view")
        .0
    }
}

fn opened(prefs: AgentViewPrefsVm, session: &[Peer], trace: &[Peer]) -> Opened {
    let mut opened = Opened {
        prefs,
        ..Opened::default()
    };
    opened
        .live
        .observe(session, trace, None, Instant::now(), sessions::now_ms());
    opened
}

const SHOW_ALL: AgentViewPrefsVm = AgentViewPrefsVm {
    show_reasoning: true,
    show_details: true,
};

#[test]
fn answer_context_survives_replica_projection_only_for_trace_readers() {
    for reader in [true, false] {
        let w = world(reader);
        let parts: Vec<_> = [
            "persona",
            "memory",
            "tools",
            "mcp",
            "history",
            "attachments",
            "tool_results",
        ]
        .into_iter()
        .map(|part| json!({"part": part, "tokens": 10, "estimated": true}))
        .collect();
        let mut metadata = json!({"context": {"used": 70, "window": 200, "parts": parts},
            "usage": {"prompt_tokens": 9000}});
        w.put(vec![w.answer("answer", metadata.clone())]);
        let view = w.view(&opened(AgentViewPrefsVm::default(), &[], &[]));
        assert_eq!(view.context.as_ref().map(|c| c.used), reader.then_some(70));
        assert_eq!(
            view.context.as_ref().and_then(|c| c.percent.as_deref()),
            reader.then_some("35%")
        );
        metadata["context"]["window"] = Value::Null;
        w.put(vec![w.answer("answer", metadata.clone())]);
        let unknown = w.view(&opened(AgentViewPrefsVm::default(), &[], &[]));
        if reader {
            assert_eq!(
                unknown.context.expect("unknown window").summary,
                "used ≈ 70 tokens"
            );
        } else {
            assert!(unknown.context.is_none());
        }
        metadata["context"]["parts"][0]["tokens"] = json!(71);
        w.put(vec![w.answer("answer", metadata)]);
        assert!(w.view(&opened(SHOW_ALL, &[], &[])).context.is_none());
        w.put(vec![w.answer(
            "old answer",
            json!({"usage": {"prompt_tokens": 9000}}),
        )]);
        assert!(w.view(&opened(SHOW_ALL, &[], &[])).context.is_none());
    }
}

#[test]
fn a_late_opener_sees_everything_so_far_and_the_final_row_replaces_it_byte_for_byte() {
    let w = world(true);
    w.put(vec![
        w.chunk(0, ""),
        w.chunk(1, "Ahoj, "),
        w.chunk(2, "ana"),
    ]);
    let docs = [Peer {
        actor_id: "alpha".to_owned(),
        client_id: INSTANCE.to_owned(),
        doc: json!({"turn": TURN, "from_seq": 3, "text": " i ben", "tool": "web_search"}),
    }];
    let view = w.view(&opened(AgentViewPrefsVm::default(), &docs, &[]));
    assert_eq!(view.live.len(), 1);
    let turn = &view.live[0];
    assert_eq!(
        (turn.text.as_str(), turn.tool.as_deref(), turn.stopped),
        ("Ahoj, ana i ben", Some("web_search"), false)
    );
    // The end: one commit writes the message and deletes the chunks.
    w.put(vec![
        w.answer("Ahoj, ana i ben.", json!({"model": "m"})),
        RowOp::Delete {
            table: "turn_chunks".to_owned(),
            row_id: "k0".to_owned(),
            base_version: None,
        },
        RowOp::Delete {
            table: "turn_chunks".to_owned(),
            row_id: "k1".to_owned(),
            base_version: None,
        },
        RowOp::Delete {
            table: "turn_chunks".to_owned(),
            row_id: "k2".to_owned(),
            base_version: None,
        },
    ]);
    // The document is still present until it is cleared: the row wins.
    let view = w.view(&opened(AgentViewPrefsVm::default(), &docs, &[]));
    assert!(view.live.is_empty(), "{:?}", view.live);
    assert_eq!(
        view.messages.last().map(|m| m.body.as_str()),
        Some("Ahoj, ana i ben.")
    );
}

#[test]
fn details_show_what_the_provider_reported_and_nothing_it_did_not() {
    let w = world(true);
    w.put(vec![w.answer(
        "done",
        json!({"model": "gpt-x", "usage": {"prompt_tokens": 12, "completion_tokens": null,
               "total_tokens": null}, "ttft_ms": 210, "duration_ms": null,
               "finish": "stop", "tool_calls": 0}),
    )]);
    let hidden = w.view(&opened(AgentViewPrefsVm::default(), &[], &[]));
    assert_eq!(hidden.messages[0].details, None, "off by default");
    let shown = w.view(&opened(SHOW_ALL, &[], &[]));
    assert_eq!(
        shown.messages[0].details,
        Some(AgentMessageDetailsVm {
            model: Some("gpt-x".to_owned()),
            prompt_tokens: Some(12),
            ttft_ms: Some(210),
            finish: Some("stop".to_owned()),
            ..AgentMessageDetailsVm::default()
        })
    );
    assert_eq!(
        details(&json!({"usage": {}})),
        None,
        "nothing reported is no caption"
    );
    assert_eq!(
        details(&json!({"error": "the provider refused"})).and_then(|d| d.error),
        Some("the provider refused".to_owned())
    );
}

#[test]
fn thinking_streams_only_for_a_reader_who_shows_it_and_stays_after_the_end() {
    let w = world(true);
    w.put(vec![
        w.chunk(0, ""),
        w.reasoning("r1", 1, false, "think "),
        w.tool_call("started", None),
    ]);
    let trace = [Peer {
        actor_id: "alpha".to_owned(),
        client_id: INSTANCE.to_owned(),
        doc: json!({"turn": TURN, "from_seq": 2, "reasoning": "harder"}),
    }];
    let off = w.view(&opened(AgentViewPrefsVm::default(), &[], &trace));
    assert!(off.reads_trace);
    assert_eq!(
        off.live[0].reasoning, None,
        "Show thinking is off by default"
    );
    assert_eq!(off.live[0].steps[0].status, AgentToolStepStatusVm::Started);
    let on = w.view(&opened(SHOW_ALL, &[], &trace));
    assert_eq!(on.live[0].reasoning.as_deref(), Some("think harder"));

    w.put(vec![
        w.tool_call("done", Some("3 results")),
        w.answer("answer", json!({})),
        RowOp::Delete {
            table: "turn_chunks".to_owned(),
            row_id: "k0".to_owned(),
            base_version: None,
        },
        RowOp::Delete {
            table: "reasoning".to_owned(),
            row_id: "r1".to_owned(),
            base_version: None,
        },
        w.reasoning("r-all", 0, true, "think harder"),
    ]);
    let ended = w.view(&opened(SHOW_ALL, &[], &[]));
    let answer = ended.messages.last().expect("the answer");
    assert_eq!(answer.reasoning.as_deref(), Some("think harder"));
    let step = &answer.steps.as_ref().expect("steps")[0];
    assert_eq!(
        (
            step.status,
            step.result.as_deref(),
            step.ended_at_ms.is_some()
        ),
        (AgentToolStepStatusVm::Done, Some("3 results"), true)
    );
    let hidden = w.view(&opened(AgentViewPrefsVm::default(), &[], &[]));
    assert_eq!(
        hidden.messages.last().and_then(|m| m.reasoning.clone()),
        None
    );
}

#[test]
fn a_member_who_does_not_read_the_trace_is_shown_none_of_it() {
    let w = world(false);
    // Rows a non-reader's replica would never hold, written here to prove
    // the view does not lean on the server alone.
    w.put(vec![
        w.chunk(0, ""),
        w.chunk(1, "text"),
        w.reasoning("r1", 1, false, "secret"),
        w.tool_call("done", Some("secret result")),
    ]);
    let trace = [Peer {
        actor_id: "alpha".to_owned(),
        client_id: INSTANCE.to_owned(),
        doc: json!({"turn": TURN, "from_seq": 2, "reasoning": "more secret"}),
    }];
    let view = w.view(&opened(SHOW_ALL, &[], &trace));
    assert!(!view.reads_trace);
    let turn = &view.live[0];
    assert_eq!(turn.text, "text");
    assert_eq!(turn.reasoning, None);
    assert!(turn.steps.is_empty());
}

#[test]
fn a_turn_whose_instance_stopped_keeps_its_text_and_says_it_stopped() {
    let w = world(true);
    w.put(vec![
        w.chunk(0, ""),
        w.chunk(1, "half an ans"),
        w.claim(INSTANCE),
    ]);
    let mut opened = opened(AgentViewPrefsVm::default(), &[], &[]);
    let start = Instant::now();
    let claim = live_claim(&w);
    opened
        .live
        .observe(&[], &[], claim.clone(), start, sessions::now_ms());
    assert!(!w.view(&opened).live[0].stopped);
    opened
        .live
        .observe(&[], &[], claim, start + LEASE, sessions::now_ms());
    let turn = &w.view(&opened).live[0];
    assert_eq!((turn.text.as_str(), turn.stopped), ("half an ans", true));
}

/// R1063-10: a session open longer than a lease, then a new answer whose
/// chunks arrive before its claim or any document. Its lease runs from
/// when this device first saw it, not from when the session was opened;
/// with neither a claim nor a document for a whole lease it stopped.
#[test]
fn a_new_unclaimed_turn_gets_its_own_lease_however_long_the_session_was_open() {
    let w = world(true);
    let mut held = Opened::default();
    // What a stream does: observe, draw, settle what that drawing held.
    let draw = |held: &mut Opened, at: Instant| {
        held.live.observe(&[], &[], None, at, sessions::now_ms());
        let view = w.view(held);
        let seen = held.live.clone();
        held.live.settle(&seen, &view.live);
        view.live.first().map(|turn| turn.stopped)
    };
    let start = Instant::now();
    assert_eq!(draw(&mut held, start), None, "nothing being written");
    let begun = start + LEASE * 3;
    w.put(vec![w.chunk(0, ""), w.chunk(1, "a new ans")]);
    assert_eq!(draw(&mut held, begun), Some(false), "just begun");
    assert_eq!(
        draw(&mut held, begun + Duration::from_secs(29)),
        Some(false),
        "within its own lease"
    );
    assert_eq!(
        draw(&mut held, begun + LEASE),
        Some(true),
        "a lease with neither a claim nor a document"
    );
}

fn live_claim(w: &World) -> Option<Claim> {
    claim(&w.source, "dev1.1")
}

#[test]
fn a_session_offers_only_what_the_agent_offers_and_a_switch_only_narrows() {
    let w = world(true);
    assert!(
        w.view(&Opened::default()).tools.is_empty(),
        "no instance has said"
    );
    w.put(vec![
        w.profile("alpha.one", Some(&["web_search", "fetch", "run"])),
        w.profile("alpha.two", Some(&["fetch", "web_search"])),
        w.profile("alpha.old", None),
    ]);
    let names = |w: &World| -> Vec<(String, bool)> {
        w.view(&Opened::default())
            .tools
            .into_iter()
            .map(|tool| (tool.name, tool.on))
            .collect()
    };
    assert_eq!(
        names(&w),
        [("web_search".to_owned(), true), ("fetch".to_owned(), true)],
        "before a claim: what every instance that said offers"
    );
    w.put(vec![w.claim("alpha.one")]);
    assert_eq!(names(&w).len(), 3, "the holder's own offer");

    let now = sessions::now_ms();
    set_tool(&w.source, "dev1.1", "alpha", "run", false, now).expect("off");
    set_tool(&w.source, "dev1.1", "alpha", "run", false, now).expect("off twice is once");
    assert_eq!(switched_off(&w.source, "dev1.1").0, ["run"]);
    assert_eq!(
        names(&w),
        [
            ("web_search".to_owned(), true),
            ("fetch".to_owned(), true),
            ("run".to_owned(), false)
        ]
    );
    let refused =
        set_tool(&w.source, "dev1.1", "alpha", "drive_write", true, now).expect_err("not offered");
    assert!(
        refused.0.contains("not one of the tools offered"),
        "{refused}"
    );
    assert_eq!(
        switched_off(&w.source, "dev1.1").0,
        ["run"],
        "nothing written"
    );
    set_tool(&w.source, "dev1.1", "alpha", "run", true, now).expect("on");
    assert!(switched_off(&w.source, "dev1.1").0.is_empty());
}

#[test]
fn a_session_that_read_outside_content_is_marked() {
    let w = world(true);
    assert!(!w.view(&Opened::default()).untrusted);
    w.put(vec![RowOp::Patch {
        table: "sessions".to_owned(),
        values: object(json!({"id": "dev1.1", "trust": "untrusted"})),
        base_version: None,
    }]);
    assert!(w.view(&Opened::default()).untrusted);
}
