//! The Agents window over real on-disk replicas, seeded through `mutate`
//! with no server: the list of two databases, the attention fold, a
//! message's turn state, the windows an archive shrinks (an outbox-pinned
//! row surviving it), new sessions and their database, and decisions.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::agents::attention::AgentAttentionVm;
use crate::hq::approvals::{self, AgentApprovalStateVm, AgentDecisionStateVm};
use crate::hq::port::{ReplicaSpec, RowOp};
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, AgentMessageStateVm, Opened};

const ANA: &str = "ana";
const BEN: &str = "ben";
const NIXIE: &str = "nixie";
const STEWARD: &str = "steward";

struct World {
    _dir: tempfile::TempDir,
    alpha: Source,
    beta: Source,
}

fn open(dir: &std::path::Path, database: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("{database}.sqlite")),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
    })
    .expect("opens");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: ANA.to_owned(),
        client: "dev1".to_owned(),
    }
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

fn actor(id: &str, kind: &str, name: &str, dir: &str) -> RowOp {
    upsert(
        "actors",
        json!({"id": id, "kind": kind, "display_name": name, "family": null, "dir": dir,
               "created_at_ms": 1, "author_actor": "server", "author_client": "server"}),
    )
}

/// A session of `agent` with ana as its owner, active at `at`.
fn session(id: &str, agent: &str, at: i64, archived: bool, read_until: Option<i64>) -> Vec<RowOp> {
    vec![
        upsert(
            "sessions",
            json!({"id": id, "title": format!("about {id}"), "status": "open",
                   "agent_actor": agent, "created_by_actor": ANA, "created_by_client": "dev1",
                   "preferred_client": null, "parent_session": null, "invite": "[]",
                   "archived": archived, "heard_until": null, "created_at_ms": at,
                   "last_activity_ms": at, "author_actor": ANA, "author_client": "dev1"}),
        ),
        upsert(
            "session_members",
            json!({"id": format!("{id}/{ANA}"), "session_id": id, "actor_id": ANA,
                   "role": "owner", "reads_trace": true, "added_by_actor": ANA,
                   "added_at_ms": at, "read_until": read_until,
                   "author_actor": ANA, "author_client": "dev1"}),
        ),
    ]
}

fn message(
    id: &str,
    session: &str,
    role: &str,
    at: i64,
    turn: Option<&str>,
    metadata: Value,
) -> RowOp {
    let author = if role == "agent" { NIXIE } else { ANA };
    let client = if role == "agent" { "nixie.box" } else { "dev1" };
    upsert(
        "messages",
        json!({"id": id, "session_id": session,
               "page_key": sessions::page_key(session, &sessions::month(at)),
               "turn_id": turn, "role": role, "body": format!("body of {id}"),
               "body_blob": null,
               "metadata": if metadata.is_null() { Value::Null } else { Value::from(metadata.to_string()) },
               "created_at_ms": at, "author_actor": author, "author_client": client}),
    )
}

fn approval(id: &str, session: &str, at: i64, approvers: &[&str], with_call: bool) -> Vec<RowOp> {
    let page = sessions::page_key(session, &sessions::month(at));
    let mut ops = Vec::new();
    if with_call {
        ops.push(upsert(
            "tool_calls",
            json!({"id": format!("call-{id}"), "session_id": session, "page_key": page,
                   "turn_id": "t", "name": "run", "args": "{\"argv\":[\"ls\"]}",
                   "status": "parked", "result": null, "result_blob": null,
                   "started_at_ms": at, "ended_at_ms": null, "created_at_ms": at,
                   "author_actor": NIXIE, "author_client": "nixie.box"}),
        ));
    }
    ops.push(upsert(
        "approvals",
        json!({"id": id, "session_id": session, "page_key": page,
               "tool_call_id": format!("call-{id}"), "tier": "T2",
               "binding_digest": format!("digest-{id}"), "summary": "list files",
               "approvers": serde_json::to_string(approvers).expect("json"),
               "state": "pending", "decided_by_actor": null, "decided_by_client": null,
               "consumed_by_client": null, "expires_at_ms": at + 3_600_000,
               "created_at_ms": at, "author_actor": NIXIE, "author_client": "nixie.box"}),
    ));
    ops
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    let beta = open(dir.path(), "beta");
    alpha
        .replica
        .mutate(vec![
            actor(NIXIE, "agent", "Nixie", "alpha"),
            actor(STEWARD, "agent", "Steward", "alpha"),
            actor(ANA, "person", "Ana", "alpha"),
        ])
        .expect("alpha directory");
    beta.replica
        .mutate(vec![
            actor(NIXIE, "agent", "Nixie", "beta"),
            actor(ANA, "person", "Ana", "beta"),
        ])
        .expect("beta directory");
    World {
        _dir: dir,
        alpha,
        beta,
    }
}

fn now() -> i64 {
    sessions::now_ms()
}

#[test]
fn both_databases_list_together_each_row_labelled_and_attention_is_rusts() {
    let w = world();
    let t = now() - 60_000;
    let mut alpha = Vec::new();
    // dev1.1: an approval naming ana waits.
    alpha.extend(session("dev1.1", NIXIE, t, false, None));
    alpha.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    // dev1.2: a turn is open.
    alpha.extend(session("dev1.2", STEWARD, t + 1, false, None));
    alpha.push(upsert(
        "turn_chunks",
        json!({"id": "k1", "session_id": "dev1.2", "turn_id": "m1", "seq": 0, "text": "",
               "created_at_ms": t, "author_actor": STEWARD, "author_client": "s.box"}),
    ));
    // dev1.3: answered after ana last read it.
    alpha.extend(session("dev1.3", NIXIE, t + 2, false, Some(t)));
    alpha.push(message(
        "m3",
        "dev1.3",
        "agent",
        t + 2,
        Some("x"),
        Value::Null,
    ));
    // dev1.4: the agent asks; dev1.5: ana answered its ask.
    alpha.extend(session("dev1.4", NIXIE, t + 3, false, Some(t + 10)));
    alpha.push(message(
        "m4",
        "dev1.4",
        "agent",
        t + 3,
        None,
        json!({"kind": "ask", "options": ["yes", "no"]}),
    ));
    alpha.extend(session("dev1.5", NIXIE, t + 4, false, Some(t + 10)));
    alpha.push(message(
        "m5a",
        "dev1.5",
        "agent",
        t + 4,
        None,
        json!({"kind": "ask"}),
    ));
    alpha.push(message("m5b", "dev1.5", "person", t + 5, None, Value::Null));
    w.alpha.replica.mutate(alpha).expect("alpha sessions");
    // The same id in beta is another session.
    w.beta
        .replica
        .mutate(session("dev1.1", NIXIE, t + 6, false, None))
        .expect("beta session");

    let list = sessions::list(&[w.alpha.clone(), w.beta.clone()], now());
    let keys: Vec<(&str, &str)> = list
        .iter()
        .map(|r| (r.database.as_str(), r.session_id.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("beta", "dev1.1"),
            ("alpha", "dev1.5"),
            ("alpha", "dev1.4"),
            ("alpha", "dev1.3"),
            ("alpha", "dev1.2"),
            ("alpha", "dev1.1"),
        ]
    );
    let attention: HashMap<(&str, &str), Option<AgentAttentionVm>> = list
        .iter()
        .map(|r| ((r.database.as_str(), r.session_id.as_str()), r.attention))
        .collect();
    assert_eq!(
        attention[&("alpha", "dev1.1")],
        Some(AgentAttentionVm::NeedsYou)
    );
    assert_eq!(
        attention[&("alpha", "dev1.2")],
        Some(AgentAttentionVm::Working)
    );
    assert_eq!(
        attention[&("alpha", "dev1.3")],
        Some(AgentAttentionVm::DoneUnread)
    );
    assert_eq!(
        attention[&("alpha", "dev1.4")],
        Some(AgentAttentionVm::NeedsYou)
    );
    assert_eq!(attention[&("alpha", "dev1.5")], None);
    assert_eq!(attention[&("beta", "dev1.1")], None);
    let steward = list.iter().find(|r| r.session_id == "dev1.2").expect("row");
    assert_eq!(
        (steward.agent_name.as_str(), steward.agent_mark.as_str()),
        ("Steward", "S")
    );
}

#[test]
fn a_message_is_waiting_until_the_server_has_it_and_an_ask_closes_on_an_answer() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.push(message(
        "m1",
        "dev1.1",
        "agent",
        t,
        None,
        json!({"kind": "ask", "options": ["go", "stop"]}),
    ));
    w.alpha.replica.mutate(ops).expect("seed");
    let (view, _) = timeline::view(&w.alpha, "dev1.1", &Opened::default(), now()).expect("view");
    let ask = view.messages[0].ask.clone().expect("an ask");
    assert_eq!(
        (ask.options, ask.open),
        (vec!["go".to_owned(), "stop".to_owned()], true)
    );

    crate::hq::send::send(&w.alpha, "dev1.1", "go").expect("sent");
    let (view, newest) =
        timeline::view(&w.alpha, "dev1.1", &Opened::default(), now()).expect("view");
    let mine = view.messages.last().expect("the person's message");
    assert_eq!(
        (mine.body.as_str(), mine.state),
        ("go", Some(AgentMessageStateVm::Waiting))
    );
    assert_eq!(view.messages[0].ask.as_ref().map(|a| a.open), Some(false));
    assert_eq!(
        view.messages[0].state, None,
        "an agent's message has no turn state"
    );
    assert_eq!(newest, Some(mine.created_at_ms));
    assert!(!view.read_only && !view.has_earlier);
}

#[test]
fn an_archived_session_takes_no_message_and_is_read_through_the_server() {
    let w = world();
    let t = now() - 60_000;
    w.alpha
        .replica
        .mutate(session("dev1.1", NIXIE, t, true, None))
        .expect("seed");
    assert_eq!(
        crate::hq::send::send(&w.alpha, "dev1.1", "hi").map_err(|e| e.to_string()),
        Err("This conversation is archived; unarchive it to write in it.".to_owned())
    );
    // No server: the remote read is refused with the database's sentence.
    let row = timeline::session_row(&w.alpha, "dev1.1").expect("row");
    let error =
        timeline::fetch_page(&w.alpha, "dev1.1", &row, &Default::default()).expect_err("no server");
    assert_eq!(
        timeline::fetch_sentence("alpha", &error),
        "This conversation is archived on the server; keeper will fetch it when alpha is reachable."
    );
}

#[test]
fn archiving_shrinks_the_windows_and_an_outbox_pinned_row_survives() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(session("dev1.2", NIXIE, t, false, None));
    ops.push(message("m1", "dev1.1", "person", t, None, Value::Null));
    w.alpha.replica.mutate(ops).expect("seed");
    let apply = |source: &Source| {
        for (table, units) in sessions::plan(&sessions::held(source), &HashMap::new(), now()) {
            source
                .replica
                .set_window(&sessions::window(table), &units)
                .expect("window");
        }
    };
    apply(&w.alpha);
    let month = sessions::month(t);
    let messages = sessions::window("messages");
    let held = w
        .alpha
        .replica
        .window_state(&messages)
        .expect("state")
        .units;
    assert!(
        held.contains(&sessions::page_key("dev1.1", &month)),
        "{held:?}"
    );

    sessions::set_archived(&w.alpha, "dev1.1", true).expect("archived");
    apply(&w.alpha);
    let held = w
        .alpha
        .replica
        .window_state(&messages)
        .expect("state")
        .units;
    assert!(!held.iter().any(|u| u.starts_with("dev1.1/")), "{held:?}");
    assert!(
        held.contains(&sessions::page_key("dev1.2", &month)),
        "{held:?}"
    );
    let sessions_held = w
        .alpha
        .replica
        .window_state(&sessions::window("sessions"))
        .expect("state")
        .units;
    assert_eq!(
        sessions_held,
        vec!["dev1.1", "dev1.2"],
        "the list stays local"
    );
    // m1 was never sent: the outbox pins it through the eviction.
    let count = w
        .alpha
        .replica
        .query(
            "SELECT count(*) AS n FROM messages WHERE session_id = 'dev1.1'",
            &[],
        )
        .expect("count");
    assert_eq!(count[0]["n"], json!(1));
    let row = sessions::list(std::slice::from_ref(&w.alpha), now())
        .into_iter()
        .find(|r| r.session_id == "dev1.1")
        .expect("still listed");
    assert!(row.archived);
}

#[test]
fn a_new_session_takes_the_next_id_and_its_database_is_the_only_one_or_the_last_chosen() {
    let w = world();
    let sources = [w.alpha.clone(), w.beta.clone()];
    let first = sessions::create(&w.alpha, NIXIE, "plan the week\nthen more").expect("first");
    let second = sessions::create(&w.alpha, NIXIE, "again").expect("second");
    assert_eq!((first.as_str(), second.as_str()), ("dev1.1", "dev1.2"));
    let row = timeline::session_row(&w.alpha, &first).expect("row");
    assert_eq!(
        (row.title.as_str(), row.agent_actor.as_str()),
        ("plan the week", NIXIE)
    );
    let (view, _) = timeline::view(&w.alpha, &first, &Opened::default(), now()).expect("view");
    assert_eq!(view.messages.len(), 1);

    let choices = sessions::choices(&sources, &HashMap::new());
    let nixie = choices
        .iter()
        .find(|c| c.agent_actor == NIXIE)
        .expect("nixie");
    assert_eq!(nixie.databases, vec!["alpha", "beta"]);
    assert!(nixie.ask);
    assert_eq!(nixie.database, None);
    let steward = choices
        .iter()
        .find(|c| c.agent_actor == STEWARD)
        .expect("steward");
    assert_eq!(
        (steward.ask, steward.database.as_deref()),
        (false, Some("alpha"))
    );
    let last = HashMap::from([(NIXIE.to_owned(), "beta".to_owned())]);
    let chosen = sessions::choices(&sources, &last);
    let nixie = chosen
        .iter()
        .find(|c| c.agent_actor == NIXIE)
        .expect("nixie");
    assert_eq!(nixie.database.as_deref(), Some("beta"));

    // The default agent opens its newest active session.
    let default = sessions::default_session(&sources, Some("@nixie:example.org"), &last, now())
        .expect("a default");
    assert_eq!(
        (default.database.as_deref(), default.session_id.as_deref()),
        (Some("alpha"), Some("dev1.2"))
    );
    let fresh = sessions::default_session(&sources, Some("@steward:example.org"), &last, now())
        .expect("a default");
    assert_eq!(
        (fresh.database.as_deref(), fresh.session_id),
        (Some("alpha"), None)
    );
    assert_eq!(
        sessions::default_session(&sources, Some("@nobody:x"), &last, now()),
        None
    );
}

#[test]
fn a_decision_is_bound_to_the_digest_shown_and_to_the_approvers() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    ops.extend(approval("ap2", "dev1.1", t + 1, &[BEN], true));
    ops.extend(approval("ap3", "dev1.1", t + 2, &[ANA], false));
    w.alpha.replica.mutate(ops).expect("seed");

    let waiting = approvals::waiting(std::slice::from_ref(&w.alpha), now());
    assert_eq!(
        waiting.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["ap1"]
    );
    assert!(waiting[0]
        .action
        .as_deref()
        .is_some_and(|a| a.contains("\"ls\"")));
    let all = approvals::of_session(&w.alpha, "dev1.1", &sessions::directory(&w.alpha), now());
    assert_eq!(
        all[2].sentence,
        "keeper does not hold this request's action yet, so it cannot be approved here."
    );
    assert!(!all[1].can_decide && all[1].sentence.starts_with("Waiting for"));

    let refused = |id: &str, digest: &str| {
        approvals::decide(&w.alpha, "dev1.1", id, digest, true, now())
            .expect_err("refused")
            .to_string()
    };
    assert_eq!(
        refused("ap1", "stale"),
        "This request changed since it was shown; look at it again before deciding."
    );
    assert_eq!(
        refused("ap2", "digest-ap2"),
        "This request is not yours to decide."
    );
    assert!(refused("ap3", "digest-ap3").contains("cannot be approved here"));

    let outcome =
        approvals::decide(&w.alpha, "dev1.1", "ap1", "digest-ap1", true, now()).expect("decided");
    assert_eq!(
        outcome.state,
        AgentDecisionStateVm::Waiting,
        "no server: it waits here"
    );
    let decided = approvals::of_session(&w.alpha, "dev1.1", &sessions::directory(&w.alpha), now());
    assert_eq!(decided[0].state, AgentApprovalStateVm::Approved);
    assert_eq!(decided[0].sentence, "Approved by Ana.");
    assert_eq!(
        refused("ap1", "digest-ap1"),
        "This request was already decided."
    );
}
