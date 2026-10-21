//! The People sheet over real on-disk replicas seeded through `mutate`,
//! with no server: who is in a session, who may be added (only people of the
//! session's own database), the owner's writes, a member leaving, and the
//! changes that need the server refused while it is not there.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{add, leave, remove, set_reader, view, AgentMemberRoleVm};
use crate::hq::approvals::{self, Decisions};
use crate::hq::port::{Rejection, ReplicaError, ReplicaSpec, RowOp};
use crate::hq::sessions::{self, names, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, Opened};
use crate::hq::window_tests::Staged;

const ANA: &str = "ana";
const BEN: &str = "ben";
const CAL: &str = "cal";
const MARTA: &str = "marta";
const NIXIE: &str = "nixie";

fn open(dir: &std::path::Path, database: &str, actor: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("{database}-{actor}.sqlite")),
        client_id: Some(format!("{actor}.dev")),
        link: None,
        subscriptions: Vec::new(),
    })
    .expect("opens");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: actor.to_owned(),
        client: format!("{actor}.dev"),
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

fn member(session: &str, actor: &str, role: &str, reads: bool) -> RowOp {
    upsert(
        "session_members",
        json!({"id": format!("{session}/{actor}"), "session_id": session, "actor_id": actor,
               "role": role, "reads_trace": reads, "added_by_actor": ANA, "added_at_ms": 1,
               "read_until": null, "author_actor": "server", "author_client": "server"}),
    )
}

/// Ana's session `s.1` with Nixie in `alpha`, Ben invited and a member;
/// Cal is in alpha's directory and not in it. Marta belongs to beta alone,
/// and a stray row naming her as beta's is in alpha's replica too.
fn seed(source: &Source) {
    let at = sessions::now_ms() - 60_000;
    source
        .replica
        .mutate(vec![
            actor(ANA, "person", "Ana", "alpha"),
            actor(BEN, "person", "Ben", "alpha"),
            actor(CAL, "person", "Cal", "alpha"),
            actor(NIXIE, "agent", "Nixie", "alpha"),
            actor(MARTA, "person", "Marta", "beta"),
            upsert(
                "sessions",
                json!({"id": "s.1", "title": "plans", "status": "open", "agent_actor": NIXIE,
                       "created_by_actor": ANA, "created_by_client": "ana.dev",
                       "preferred_client": null, "parent_session": null,
                       "invite": json!([BEN]).to_string(), "archived": false,
                       "heard_until": null, "created_at_ms": at, "last_activity_ms": at,
                       "author_actor": ANA, "author_client": "ana.dev"}),
            ),
            member("s.1", ANA, "owner", true),
            member("s.1", NIXIE, "agent", true),
            member("s.1", BEN, "person", false),
            upsert(
                "messages",
                json!({"id": "m1", "session_id": "s.1",
                       "page_key": sessions::page_key("s.1", &sessions::month(at)),
                       "turn_id": null, "role": "person", "body": "hello from ben",
                       "body_blob": null, "metadata": null, "created_at_ms": at,
                       "author_actor": BEN, "author_client": "ben.dev"}),
            ),
        ])
        .expect("seed");
}

fn world(actor: &str) -> (tempfile::TempDir, Source) {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = open(dir.path(), "alpha", actor);
    seed(&source);
    (dir, source)
}

#[test]
fn the_owner_sees_every_member_and_may_add_only_people_of_this_database() {
    let (_dir, ana) = world(ANA);
    let vm = view(&ana, "s.1").expect("view");
    assert!(vm.owner && !vm.can_leave);
    let shown: Vec<(&str, AgentMemberRoleVm, bool, bool)> = vm
        .members
        .iter()
        .map(|m| (m.actor_id.as_str(), m.role, m.reads_trace, m.me))
        .collect();
    assert_eq!(
        shown,
        [
            (ANA, AgentMemberRoleVm::Owner, true, true),
            (BEN, AgentMemberRoleVm::Person, false, false),
            (NIXIE, AgentMemberRoleVm::Agent, true, false),
        ]
    );
    let ben = &vm.members[1];
    assert_eq!((ben.name.as_str(), ben.mark.as_str()), ("Ben", "B"));
    assert!(ben.can_set_reader && ben.can_remove);
    // The owner's own row and the agent's are not the owner's to change.
    assert!(vm
        .members
        .iter()
        .filter(|m| m.actor_id != BEN)
        .all(|m| !m.can_set_reader && !m.can_remove));
    // Cal alone: not the agent, not a member, never Marta of the other database.
    let addable: Vec<&str> = vm.addable.iter().map(|p| p.actor_id.as_str()).collect();
    assert_eq!(addable, [CAL]);
    let refused = add(&ana, "s.1", MARTA).expect_err("marta is beta's");
    assert!(refused.contains("alpha's directory"), "{refused}");
    assert!(
        add(&ana, "s.1", NIXIE).is_err(),
        "an agent is not added as a person"
    );
    assert!(add(&ana, "s.1", BEN).is_err(), "already in");
}

#[test]
fn adding_names_the_person_in_the_invite_and_shows_them_joining() {
    let (_dir, ana) = world(ANA);
    add(&ana, "s.1", CAL).expect("added");
    let rows = ana
        .replica
        .query("SELECT invite FROM sessions WHERE id = 's.1'", &[])
        .expect("row");
    assert_eq!(names(&rows[0], "invite"), [BEN, CAL]);
    let vm = view(&ana, "s.1").expect("view");
    let cal = vm.members.iter().find(|m| m.actor_id == CAL).expect("cal");
    assert!(cal.joining && !cal.reads_trace && !cal.can_set_reader && !cal.can_remove);
    assert!(vm.addable.is_empty());
    // The member row arrives (the server's reaction): no longer joining.
    ana.replica
        .mutate(vec![member("s.1", CAL, "person", false)])
        .expect("arrives");
    let vm = view(&ana, "s.1").expect("view");
    let cal = vm.members.iter().find(|m| m.actor_id == CAL).expect("cal");
    assert!(!cal.joining && cal.can_remove);
}

#[test]
fn a_member_who_is_not_the_owner_changes_nobody_and_may_leave() {
    let (_dir, ben) = world(BEN);
    let vm = view(&ben, "s.1").expect("view");
    assert!(!vm.owner && vm.can_leave);
    assert!(vm.addable.is_empty());
    assert!(vm
        .members
        .iter()
        .all(|m| !m.can_set_reader && !m.can_remove));
    assert!(add(&ben, "s.1", CAL)
        .expect_err("not the owner")
        .contains("owner"));
    assert!(set_reader(&ben, "s.1", ANA, false).is_err());
    assert!(remove(&ben, "s.1", ANA).is_err());

    let held = |source: &Source| -> HashMap<&'static str, Vec<String>> {
        sessions::plan(
            &sessions::held(source),
            &HashMap::new(),
            false,
            sessions::now_ms(),
        )
        .into_iter()
        .collect()
    };
    assert_eq!(held(&ben)["session_members"], ["s.1"]);
    leave(&ben, "s.1").expect("left");
    // The list and every window drop the session at once.
    assert!(sessions::list(std::slice::from_ref(&ben), sessions::now_ms()).is_empty());
    assert!(held(&ben).values().all(Vec::is_empty), "{:?}", held(&ben));
    assert!(view(&ben, "s.1").is_err());
}

#[test]
fn the_owner_does_not_leave_and_server_changes_wait_for_the_server() {
    let (_dir, ana) = world(ANA);
    assert!(leave(&ana, "s.1").expect_err("owner").contains("owner"));
    for refused in [
        set_reader(&ana, "s.1", BEN, true).expect_err("offline"),
        remove(&ana, "s.1", BEN).expect_err("offline"),
    ] {
        assert!(refused.contains("alpha is not syncing"), "{refused}");
    }
    // Asking for what already holds asks nobody.
    set_reader(&ana, "s.1", BEN, false).expect("already not a reader");
    assert!(
        set_reader(&ana, "s.1", NIXIE, false).is_err(),
        "the agent's flag is not the owner's"
    );
    assert!(remove(&ana, "s.1", CAL).is_err(), "not a member");
}

#[test]
fn a_leave_the_server_refuses_comes_back_with_its_reason() {
    // Ben's replica holds him as a person, so Leave is his; the server holds
    // him as the owner and refuses the delete (R-HQ-40). Its own database
    // name keeps the process-wide outcome maps apart from the other tests.
    let dir = tempfile::tempdir().expect("tempdir");
    let plain = open(dir.path(), "alpha-leave", BEN);
    seed(&plain);
    let (staged, ben) = Staged::over(&plain, None);
    assert!(view(&ben, "s.1").expect("view").can_leave);
    leave(&ben, "s.1").expect("written");
    let commit = staged
        .commits
        .lock()
        .expect("commits")
        .last()
        .cloned()
        .expect("the leave's commit");
    staged.refused.lock().expect("refused").insert(
        commit,
        Rejection::new("hq.v3.owner_cannot_leave", "hq.v3.owner_cannot_leave"),
    );
    // The refused delete comes back, as the server's rejection restores it.
    plain
        .replica
        .mutate(vec![member("s.1", BEN, "person", false)])
        .expect("restored");
    assert_eq!(
        view(&ben, "s.1").expect("view").notice.as_deref(),
        Some("The server refused your leaving: the session's owner cannot leave it.")
    );
}

#[test]
fn a_message_shows_the_author_the_server_stamped() {
    let (_dir, ana) = world(ANA);
    let (view, _) = timeline::view(
        &ana,
        "s.1",
        &Opened::default(),
        &HashMap::new(),
        sessions::now_ms(),
    )
    .expect("view");
    let m = &view.messages[0];
    assert_eq!(
        (m.author_actor.as_str(), m.author_name.as_str()),
        (BEN, "Ben")
    );
}

/// Ben's request `ap1` in `s.1`, its tool call (turn `t`, being written)
/// and Ben a member reading the trace.
fn trace_for_ben(source: &Source) {
    let at = sessions::now_ms() - 30_000;
    let page = sessions::page_key("s.1", &sessions::month(at));
    source
        .replica
        .mutate(vec![
            member("s.1", BEN, "person", true),
            upsert(
                "tool_calls",
                json!({"id": "call-ap1", "session_id": "s.1", "page_key": page, "turn_id": "t",
                       "name": "run", "args": "{\"argv\":[\"cat\",\"secret\"]}",
                       "status": "parked", "result": null, "result_blob": null,
                       "started_at_ms": at, "ended_at_ms": null, "created_at_ms": at,
                       "author_actor": NIXIE, "author_client": "nixie.box"}),
            ),
            upsert(
                "turn_chunks",
                json!({"id": "k0", "session_id": "s.1", "turn_id": "t", "seq": 0,
                       "text": "looking", "created_at_ms": at, "author_actor": NIXIE,
                       "author_client": "nixie.box"}),
            ),
            upsert(
                "approvals",
                json!({"id": "ap1", "session_id": "s.1", "page_key": page,
                       "tool_call_id": "call-ap1", "tier": "T2", "binding_digest": "digest-ap1",
                       "summary": "read the private file", "approvers": json!([BEN]).to_string(),
                       "state": "pending", "decided_by_actor": null, "decided_by_client": null,
                       "consumed_by_client": null, "expires_at_ms": at + 3_600_000,
                       "created_at_ms": at, "author_actor": NIXIE, "author_client": "nixie.box"}),
            ),
        ])
        .expect("trace");
}

/// What of the trace reaches Ben's webview: the session view's approvals and
/// tool steps, and the decisions panel.
fn shown_to(ben: &Source) -> (usize, usize, usize) {
    let (view, _) = timeline::view(
        ben,
        "s.1",
        &Opened::default(),
        &HashMap::new(),
        sessions::now_ms(),
    )
    .expect("view");
    let steps = view.live.iter().map(|turn| turn.steps.len()).sum();
    let waiting = approvals::waiting(
        std::slice::from_ref(ben),
        &HashMap::new(),
        sessions::now_ms(),
    );
    (view.approvals.len(), steps, waiting.len())
}

/// R1064K-01: withdrawing Ben's reader flag, then removing him, leaves the
/// approval, its tool call and his own decision (pinned by the outbox) on
/// his device until the window evicts them; none of it reaches a view, and
/// he cannot decide it.
#[test]
fn a_withdrawn_or_removed_reader_is_shown_no_approval_and_cannot_decide() {
    let (dir, ben) = world(BEN);
    trace_for_ben(&ben);
    let decisions = Decisions::load(dir.path().join("decisions.json"));
    // A reader: the request, its exact action, his to decide.
    let (shown, steps, waiting) = shown_to(&ben);
    assert_eq!((shown, steps, waiting), (1, 1, 1));
    // He denies it while offline: the decision waits in the outbox.
    approvals::decide(
        &ben,
        &decisions,
        "s.1",
        "ap1",
        "digest-ap1",
        false,
        sessions::now_ms(),
    )
    .expect("written");
    // The owner withdraws his flag: the membership changes, the rows stay.
    ben.replica
        .mutate(vec![member("s.1", BEN, "person", false)])
        .expect("withdrawn");
    let held = ben
        .replica
        .query("SELECT id FROM approvals WHERE id = 'ap1'", &[])
        .expect("rows");
    assert_eq!(held.len(), 1, "the row is still on this device");
    assert_eq!(shown_to(&ben), (0, 0, 0));
    let refused = approvals::decide(
        &ben,
        &decisions,
        "s.1",
        "ap1",
        "digest-ap1",
        true,
        sessions::now_ms(),
    )
    .expect_err("not a reader");
    assert_eq!(refused.0, approvals::NOT_A_READER);
    // Granted again, then removed: no membership reads nothing.
    trace_for_ben(&ben);
    assert_eq!(shown_to(&ben).0, 1);
    ben.replica
        .mutate(vec![RowOp::Delete {
            table: "session_members".to_owned(),
            row_id: format!("s.1/{BEN}"),
            base_version: None,
        }])
        .expect("removed");
    let (view, _) = timeline::view(
        &ben,
        "s.1",
        &Opened::default(),
        &HashMap::new(),
        sessions::now_ms(),
    )
    .expect("the session row is still here");
    assert!(view.approvals.is_empty() && !view.reads_trace);
    assert!(view.live.iter().all(|turn| turn.steps.is_empty()));
    assert!(approvals::waiting(
        std::slice::from_ref(&ben),
        &HashMap::new(),
        sessions::now_ms()
    )
    .is_empty());
}

/// A decision the server refuses with `hq.v8.not_reader` reads in keeper's
/// words; a code keeper does not know is shown as the server said it.
#[test]
fn a_decision_refused_for_not_reading_says_so() {
    assert_eq!(
        approvals::refused_because(&Rejection::new("hq.v8.not_reader", "hq.v8.not_reader")),
        "you no longer read this session's thinking and tool calls, so its requests are not \
         yours to decide."
    );
    assert_eq!(
        approvals::refused_because(&Rejection::new("hq.v8.expired", "hq.v8.expired")),
        "hq.v8.expired"
    );
}

/// A stand-in for the server's `remove_member`: who is a member there, the
/// request ids it applied (a repeat is answered from its record, `cached`),
/// and whether the next answer is lost on its way back.
#[derive(Default)]
struct Server {
    members: std::collections::HashSet<String>,
    applied: std::collections::HashSet<String>,
    lose_next_answer: bool,
    requests: Vec<String>,
}

fn answer(
    server: &Arc<std::sync::Mutex<Server>>,
    request: &str,
    params: &Value,
) -> Result<(), ReplicaError> {
    let mut server = server.lock().expect("server");
    server.requests.push(request.to_owned());
    if server.applied.contains(request) {
        return Ok(());
    }
    let actor = params["actorId"].as_str().unwrap_or_default().to_owned();
    if !server.members.remove(&actor) {
        return Err(ReplicaError::Remote {
            code: "operation.forbidden".to_owned(),
            message: "hq.remove_member.not_member".to_owned(),
            retryable: false,
        });
    }
    server.applied.insert(request.to_owned());
    if std::mem::take(&mut server.lose_next_answer) {
        return Err(ReplicaError::Remote {
            code: "transport.timeout".to_owned(),
            message: "the request timed out".to_owned(),
            retryable: true,
        });
    }
    Ok(())
}

/// R1064K-02: a removal the server made whose answer was lost is asked
/// again under the same request id, so the server replays it rather than
/// removing Ben's newer membership (he was invited again meanwhile); once
/// this device has seen him re-invited, Remove is a new removal.
#[test]
fn a_removal_whose_answer_was_lost_is_asked_again_under_the_same_request() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plain = open(dir.path(), "alpha-unheard", ANA);
    seed(&plain);
    let (staged, ana) = Staged::over(&plain, None);
    let server = Arc::new(std::sync::Mutex::new(Server {
        members: [ANA, BEN].map(str::to_owned).into(),
        lose_next_answer: true,
        ..Server::default()
    }));
    let answering = Arc::clone(&server);
    *staged.server.lock().expect("server") = Some(Box::new(
        move |operation: &str, request: &str, params: &Value| {
            assert_eq!(operation, "remove_member");
            answer(&answering, request, params)
        },
    ));
    let lost = remove(&ana, "s.1", BEN).expect_err("the answer was lost");
    assert!(lost.contains("did not hear"), "{lost}");
    // Removed on the server; then invited again there, before this device
    // has synced either change.
    server
        .lock()
        .expect("server")
        .members
        .insert(BEN.to_owned());
    remove(&ana, "s.1", BEN).expect("the server's record answers");
    {
        let server = server.lock().expect("server");
        assert_eq!(server.requests[0], server.requests[1], "the same request");
        assert!(server.members.contains(BEN), "his newer membership stays");
    }
    // This device sees him re-invited: a membership written again.
    plain
        .replica
        .mutate(vec![upsert(
            "session_members",
            json!({"id": format!("s.1/{BEN}"), "session_id": "s.1", "actor_id": BEN,
                   "role": "person", "reads_trace": false, "added_by_actor": ANA,
                   "added_at_ms": 2, "read_until": null, "author_actor": "server",
                   "author_client": "server"}),
        )])
        .expect("re-invited");
    remove(&ana, "s.1", BEN).expect("a new removal");
    let server = server.lock().expect("server");
    assert_ne!(server.requests[2], server.requests[0], "a new request");
    assert!(!server.members.contains(BEN), "removed again");
}

/// `remove_member`'s refusals are matched by their exact reason.
#[test]
fn a_refused_removal_says_why_by_its_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plain = open(dir.path(), "alpha-refused", ANA);
    seed(&plain);
    let (staged, ana) = Staged::over(&plain, None);
    for (reason, sentence) in [
        (
            "hq.remove_member.not_member",
            "The server refused: they are not a member of this session.",
        ),
        (
            "hq.remove_member.not_owner",
            "The server refused: only the session's owner changes who is in it.",
        ),
        (
            "hq.remove_member.not_owner_of_anything",
            "The server refused this change to the session's people.",
        ),
    ] {
        *staged.server.lock().expect("server") =
            Some(Box::new(move |_: &str, _: &str, _: &Value| {
                Err(ReplicaError::Remote {
                    code: "operation.forbidden".to_owned(),
                    message: reason.to_owned(),
                    retryable: false,
                })
            }));
        assert_eq!(remove(&ana, "s.1", BEN).expect_err("refused"), sentence);
    }
}
