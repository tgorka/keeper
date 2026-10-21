//! The People sheet over real on-disk replicas seeded through `mutate`,
//! with no server: who is in a session, who may be added (only people of the
//! session's own database), the owner's writes, a member leaving, and the
//! changes that need the server refused while it is not there.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{add, leave, remove, set_reader, view, AgentMemberRoleVm};
use crate::hq::port::{ReplicaSpec, RowOp};
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
    staged
        .refused
        .lock()
        .expect("refused")
        .insert(commit, "hq.v3.owner_cannot_leave".to_owned());
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
