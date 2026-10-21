//! The People sheet over real on-disk replicas seeded through `mutate`,
//! with no server: who is in a session, who may be added (only people of the
//! session's own database), the owner's writes, a member leaving, and the
//! changes that need the server refused while it is not there. The server's
//! commands are answered by a stand-in — in place of the replica's network,
//! or over HTTP to the replica's own adapter.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{add, leave, remove, set_reader, view, AgentMemberRoleVm};
use crate::hq::approvals::{self, Decisions};
use crate::hq::port::{Answered, Rejection, ReplicaError, ReplicaSpec, RowOp};
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

/// A stand-in for the server's member commands: who is a member there and
/// the `added_at_ms` it last stamped for each (R-HQ-48), who reads the
/// trace, the request ids it applied (a repeat is answered from its record,
/// `cached`), and whether the next answer is lost on its way back (or, over
/// HTTP, arrives unreadable).
#[derive(Default)]
struct Server {
    members: std::collections::HashSet<String>,
    stamped: HashMap<String, i64>,
    readers: std::collections::HashSet<String>,
    applied: std::collections::HashSet<String>,
    lose_next_answer: bool,
    requests: Vec<String>,
}

/// What the server does with one request.
enum Outcome {
    Applied,
    Cached,
    NotMember,
}

impl Server {
    /// Ana's `alpha` as [`seed`] has it: Ana and Ben members, Ben's
    /// membership stamped at 1, nobody but Ana reading.
    fn seeded() -> Self {
        Server {
            members: [ANA, BEN].map(str::to_owned).into(),
            stamped: [(BEN.to_owned(), 1)].into(),
            readers: [ANA.to_owned()].into(),
            ..Server::default()
        }
    }

    fn run(&mut self, operation: &str, request: &str, actor: &str) -> Outcome {
        self.requests.push(request.to_owned());
        if self.applied.contains(request) {
            return Outcome::Cached;
        }
        if !self.members.contains(actor) {
            return Outcome::NotMember;
        }
        match operation {
            "remove_member" => {
                self.members.remove(actor);
                self.readers.remove(actor);
            }
            "grant_trace" => {
                self.readers.insert(actor.to_owned());
            }
            "revoke_trace" => {
                self.readers.remove(actor);
            }
            other => panic!("no command {other}"),
        }
        self.applied.insert(request.to_owned());
        Outcome::Applied
    }

    /// Invite `actor` again at the server's clock `now`: a new membership,
    /// on the server only, stamped as R-HQ-48 has it — `now`, or one past
    /// the last stamp when `now` is not later. The stamp.
    fn reinvite(server: &std::sync::Mutex<Server>, actor: &str, now: i64) -> i64 {
        let mut server = server.lock().expect("server");
        let at = server
            .stamped
            .get(actor)
            .map_or(now, |last| now.max(last + 1));
        server.stamped.insert(actor.to_owned(), at);
        server.members.insert(actor.to_owned());
        at
    }
}

fn answer(
    server: &Arc<std::sync::Mutex<Server>>,
    operation: &str,
    request: &str,
    params: &Value,
) -> Result<Answered, ReplicaError> {
    let mut server = server.lock().expect("server");
    let actor = params["actorId"].as_str().unwrap_or_default().to_owned();
    match server.run(operation, request, &actor) {
        Outcome::Cached => Ok(Answered::Cached),
        Outcome::NotMember => Err(ReplicaError::Remote {
            code: "operation.forbidden".to_owned(),
            message: "hq.remove_member.not_member".to_owned(),
            retryable: false,
        }),
        Outcome::Applied if std::mem::take(&mut server.lose_next_answer) => {
            Err(ReplicaError::Unheard {
                code: "transport.timeout".to_owned(),
                message: "the request timed out".to_owned(),
                retryable: true,
            })
        }
        Outcome::Applied => Ok(Answered::Applied),
    }
}

/// Ana's replica of `alpha` (as [`world`]) behind [`Staged`], reporting
/// version 1 for every row, whose member commands the stand-in server
/// answers, losing its first answer.
fn unheard_world(
    dir: &std::path::Path,
    database: &str,
) -> (Source, Arc<Staged>, Source, Arc<std::sync::Mutex<Server>>) {
    let plain = open(dir, database, ANA);
    seed(&plain);
    let (staged, ana) = Staged::over(&plain, Some(1));
    let server = Arc::new(std::sync::Mutex::new(Server {
        lose_next_answer: true,
        ..Server::seeded()
    }));
    let answering = Arc::clone(&server);
    *staged.server.lock().expect("server") = Some(Box::new(
        move |operation: &str, request: &str, params: &Value| {
            answer(&answering, operation, request, params)
        },
    ));
    (plain, staged, ana, server)
}

/// Ben's membership row in `source`'s replica, as a pull brings it.
fn ben_row(source: &Source, values: Value) {
    let mut values = object(values);
    values.insert("id".to_owned(), json!(format!("s.1/{BEN}")));
    source
        .replica
        .mutate(vec![RowOp::Patch {
            table: "session_members".to_owned(),
            values,
            base_version: None,
        }])
        .expect("Ben's row");
}

/// R1064K-02, R1064K-04, R1064K-07: a removal the server made whose answer
/// was lost is asked again under the same request id — even after an
/// update to Ben's same membership (his read point, a newer row version)
/// reached this device meanwhile — so the server replays it rather than
/// removing the newer membership it gave him when it invited him again, in
/// the very millisecond of the first.
#[test]
fn an_unheard_removal_is_asked_again_under_its_request_across_updates_to_the_membership() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (plain, staged, ana, server) = unheard_world(dir.path(), "alpha-unheard");
    let lost = remove(&ana, "s.1", BEN).expect_err("the answer was lost");
    assert!(lost.contains("did not hear"), "{lost}");
    // Removed on the server, then invited again there at the clock of his
    // first membership; this device has seen neither, only a delayed update
    // to his old membership.
    Server::reinvite(&server, BEN, 1);
    ben_row(
        &plain,
        json!({"read_until": 5, "author_actor": BEN, "author_client": "ben.dev"}),
    );
    staged.set_version(Some(2));
    remove(&ana, "s.1", BEN).expect("the server's record answers");
    let server = server.lock().expect("server");
    assert_eq!(server.requests.len(), 2);
    assert_eq!(server.requests[0], server.requests[1], "the same request");
    assert!(server.members.contains(BEN), "his newer membership stays");
}

/// R1064K-04, R1064K-07: while a removal's answer is still unheard, Ben is
/// invited again in the same millisecond as his first membership, and this
/// device sees it. The server stamps the new membership past the old one
/// (R-HQ-48), never the same, so Remove is a new removal of the membership
/// shown, not a replay of the first.
#[test]
fn an_unheard_removal_is_a_new_request_once_the_membership_is_new() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (plain, _staged, ana, server) = unheard_world(dir.path(), "alpha-reinvited");
    remove(&ana, "s.1", BEN).expect_err("the answer was lost");
    let stamped = Server::reinvite(&server, BEN, 1);
    plain
        .replica
        .mutate(vec![upsert(
            "session_members",
            json!({"id": format!("s.1/{BEN}"), "session_id": "s.1", "actor_id": BEN,
                   "role": "person", "reads_trace": false, "added_by_actor": ANA,
                   "added_at_ms": stamped, "read_until": null, "author_actor": "server",
                   "author_client": "server"}),
        )])
        .expect("re-invited");
    remove(&ana, "s.1", BEN).expect("a new removal");
    let server = server.lock().expect("server");
    assert_eq!(server.requests.len(), 2);
    assert_ne!(server.requests[1], server.requests[0], "a new request");
    assert!(!server.members.contains(BEN), "his new membership removed");
}

/// R1064K-06: Ana switches Ben's reading to `reads` and the answer is lost.
/// Her device then sees the switch made, and then undone from another
/// device of hers. Switching it again is a new decision: the unheard request
/// is replayed (`cached`) and this decision asked as its own, so the server
/// ends with her latest.
fn a_switch_after_its_unheard_one_was_undone(reads: bool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (plain, _staged, ana, server) = unheard_world(dir.path(), &format!("alpha-reader-{reads}"));
    let seen = |reading: bool| {
        ben_row(
            &plain,
            json!({"reads_trace": reading, "author_actor": ANA, "author_client": "ana.dev"}),
        );
    };
    // Ben starts the other way round, on the server and here.
    if !reads {
        server
            .lock()
            .expect("server")
            .readers
            .insert(BEN.to_owned());
        seen(true);
    }
    let lost = set_reader(&ana, "s.1", BEN, reads).expect_err("the answer was lost");
    assert!(lost.contains("did not hear"), "{lost}");
    assert_eq!(server.lock().expect("server").readers.contains(BEN), reads);
    seen(reads);
    let undo = if reads { "revoke_trace" } else { "grant_trace" };
    server
        .lock()
        .expect("server")
        .run(undo, "from-another-device", BEN);
    seen(!reads);
    set_reader(&ana, "s.1", BEN, reads).expect("her new decision");
    let server = server.lock().expect("server");
    assert_eq!(
        server.readers.contains(BEN),
        reads,
        "her latest decision holds"
    );
    assert_eq!(
        server.requests[2], server.requests[0],
        "the unheard one first"
    );
}

#[test]
fn a_withdrawal_after_its_unheard_one_was_undone_is_its_own_decision() {
    a_switch_after_its_unheard_one_was_undone(false);
}

#[test]
fn a_grant_after_its_unheard_one_was_undone_is_its_own_decision() {
    a_switch_after_its_unheard_one_was_undone(true);
}

/// R1064K-02 for a reader switch: asked again before this device has seen
/// anything, the unheard request is what is asked first — replayed, not run
/// twice — and the flag ends as asked.
#[test]
fn a_reader_switch_asked_again_unseen_ends_as_asked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_plain, _staged, ana, server) = unheard_world(dir.path(), "alpha-reader-again");
    set_reader(&ana, "s.1", BEN, true).expect_err("the answer was lost");
    set_reader(&ana, "s.1", BEN, true).expect("asked again");
    let server = server.lock().expect("server");
    assert!(server.readers.contains(BEN));
    assert_eq!(
        server.requests[1], server.requests[0],
        "the unheard one first"
    );
}

/// The protocol's tagged value encoding (`{"t": …, "v": …}`), both ways.
fn tagged(value: &Value) -> Value {
    match value {
        Value::Null => json!({"t": "null"}),
        Value::Bool(b) => json!({"t": "boolean", "v": b}),
        Value::Number(n) if n.is_i64() => json!({"t": "integer", "v": n.to_string()}),
        Value::Number(n) => json!({"t": "number", "v": n}),
        Value::String(s) => json!({"t": "string", "v": s}),
        Value::Array(items) => {
            json!({"t": "array", "v": items.iter().map(tagged).collect::<Vec<_>>()})
        }
        Value::Object(map) => json!({"t": "object",
            "v": map.iter().map(|(k, v)| json!([k, tagged(v)])).collect::<Vec<_>>()}),
    }
}

fn untagged(value: &Value) -> Value {
    match value["t"].as_str() {
        Some("integer") => json!(value["v"].as_str().and_then(|v| v.parse::<i64>().ok())),
        Some("array") => Value::Array(
            value["v"]
                .as_array()
                .map(|items| items.iter().map(untagged).collect())
                .unwrap_or_default(),
        ),
        Some("object") => Value::Object(
            value["v"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|e| {
                            (
                                e[0].as_str().unwrap_or_default().to_owned(),
                                untagged(&e[1]),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("null") | None => Value::Null,
        Some(_) => value["v"].clone(),
    }
}

/// A server on a local port whose `/operations` is [`Server`]'s
/// `remove_member` in the protocol's own encoding — its first applied
/// answer sent back truncated, unreadable — and which answers every sync
/// round 503. The port.
fn operations_server(server: Arc<std::sync::Mutex<Server>>) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 8192];
            let body_at = loop {
                let Ok(n) = stream.read(&mut chunk) else {
                    break None;
                };
                if n == 0 {
                    break None;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(at + 4);
                }
            };
            let Some(body_at) = body_at else { continue };
            let head = String::from_utf8_lossy(&bytes[..body_at]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while bytes.len() < body_at + length {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                }
            }
            let (status, body) = if head.starts_with("post /alpha/operations ") {
                let request: Value = serde_json::from_slice(&bytes[body_at..])
                    .map(|v| untagged(&v))
                    .unwrap_or_default();
                let id = request["requestId"].as_str().unwrap_or_default().to_owned();
                let actor = request["params"]["actorId"].as_str().unwrap_or_default();
                let mut server = server.lock().expect("server");
                let reply = |status: &str| {
                    json!({"revision": 1, "kind": "command", "operationId": "remove_member",
                           "requestId": id, "status": status, "results": []})
                };
                let answer = match server.run("remove_member", &id, actor) {
                    Outcome::Applied => reply("applied"),
                    Outcome::Cached => reply("cached"),
                    Outcome::NotMember => json!({"revision": 1, "kind": "error",
                        "code": "operation.forbidden", "message": "hq.remove_member.not_member",
                        "retryable": false}),
                };
                let mut body = tagged(&answer).to_string().into_bytes();
                if std::mem::take(&mut server.lose_next_answer) {
                    body.truncate(body.len() / 2);
                }
                ("200 OK", body)
            } else {
                ("503 Service Unavailable", Vec::new())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/vnd.syncular.operations.v1+json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    port
}

/// R1064K-05, at the replica adapter over HTTP: the server applies Ben's
/// removal and its answer arrives unreadable (the client's
/// `client.invalid_host_response`, which the client marks not retryable).
/// keeper does not take that for a refusal: it keeps the request, does not
/// send it again by itself, and when Ana asks again — Ben meanwhile invited
/// again on the server — the same request is answered from the server's
/// record (`cached`) and his newer membership stays.
#[test]
fn an_unreadable_answer_keeps_its_request_and_a_retry_replays_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = Arc::new(std::sync::Mutex::new(Server {
        lose_next_answer: true,
        ..Server::seeded()
    }));
    let port = operations_server(Arc::clone(&server));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: dir.path().join("alpha-linked.sqlite"),
        client_id: Some("ana.dev".to_owned()),
        link: Some(crate::hq::port::Link {
            base_url: format!("http://127.0.0.1:{port}/alpha"),
            token: "0123456789abcdef".to_owned(),
        }),
        subscriptions: Vec::new(),
    })
    .expect("opens");
    let ana = Source {
        database: "alpha".to_owned(),
        replica: Arc::new(replica),
        actor: ANA.to_owned(),
        client: "ana.dev".to_owned(),
    };
    seed(&ana);
    let unread = remove(&ana, "s.1", BEN).expect_err("the answer could not be read");
    assert!(unread.contains("could not read"), "{unread}");
    assert!(unread.contains("client.invalid_host_response"), "{unread}");
    {
        let server = server.lock().expect("server");
        assert_eq!(server.requests.len(), 1, "not sent again by itself");
        assert!(!server.members.contains(BEN), "the server removed him");
    }
    Server::reinvite(&server, BEN, 1);
    remove(&ana, "s.1", BEN).expect("the server's record answers");
    let server = server.lock().expect("server");
    assert_eq!(server.requests.len(), 2);
    assert_eq!(server.requests[0], server.requests[1], "the same request");
    assert!(server.members.contains(BEN), "his newer membership stays");
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
