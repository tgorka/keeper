//! Which instance answers, over real on-disk replicas seeded through
//! `mutate`, with no server: the rows of two databases that agree or differ,
//! an instance whose definition differs from its siblings', who holds a
//! session as its claim moves, and presence coming and going.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{
    agent_view, answering_vm, instances_of, list, online, set_order, AgentInstancePresenceVm,
    Instance, Online, Prefs, Shared,
};
use crate::hq::live::{Claim, Live, LEASE};
use crate::hq::port::{Refusal, Rejection, ReplicaSpec, RowOp, RowWrites};
use crate::hq::sessions::{names, object, Source};
use crate::hq::syncular::SyncularReplica;

const ANA: &str = "ana";
const NIXIE: &str = "nixie";
const A: &str = "nixie.electra";
const B: &str = "nixie.hesperia";

fn open(dir: &std::path::Path, database: &str) -> Source {
    open_as(dir, database, ANA)
}

fn open_as(dir: &std::path::Path, database: &str, person: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("{database}.sqlite")),
        client_id: Some(format!("{person}.dev")),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: person.to_owned(),
        client: format!("{person}.dev"),
    }
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

fn server(mut values: Value) -> Value {
    values["author_actor"] = json!("server");
    values["author_client"] = json!("server");
    values
}

fn actor(id: &str, kind: &str, name: &str, dir: &str) -> RowOp {
    upsert(
        "actors",
        server(
            json!({"id": id, "kind": kind, "display_name": name, "family": null, "dir": dir,
                      "created_at_ms": 1}),
        ),
    )
}

fn client(id: &str, name: &str, dir: &str, revoked: Option<i64>) -> RowOp {
    upsert(
        "clients",
        server(
            json!({"id": id, "actor_id": NIXIE, "kind": "instance", "name": name,
                      "host": name.split('@').nth(1), "created_at_ms": 1,
                      "revoked_at_ms": revoked, "dir": dir}),
        ),
    )
}

fn profile(id: &str, dir: &str, hash: &str, started: i64) -> RowOp {
    let mut values = json!({"id": id, "actor_id": NIXIE, "name": "Nixie",
                            "description": "tgorka's proxy", "avatar_blob": null,
                            "definition_hash": hash, "image_digest": "sha256:img",
                            "started_at_ms": started, "dir": dir, "tools": null});
    values["author_actor"] = json!(NIXIE);
    values["author_client"] = json!(id);
    upsert("agent_profiles", values)
}

fn prefs(default: &str, fallback: &[&str], updated: i64) -> RowOp {
    upsert(
        "instance_prefs",
        json!({"id": format!("{ANA}/{NIXIE}"), "person_actor": ANA, "agent_actor": NIXIE,
               "default_client": default, "fallback": json!(fallback).to_string(),
               "updated_at_ms": updated, "author_actor": ANA, "author_client": "ana.dev"}),
    )
}

/// One of Ana's databases with Nixie and her two instances, on one
/// definition.
fn seed(source: &Source) {
    let db = source.database.as_str();
    source
        .replica
        .mutate(vec![
            actor(ANA, "person", "Ana", db),
            actor(NIXIE, "agent", "Nixie", db),
            client(A, "nixie@electra", db, None),
            client(B, "nixie@hesperia", db, None),
            profile(A, db, "h-one", 10),
            profile(B, db, "h-one", 10),
        ])
        .expect("seed");
}

/// Ana's databases alpha and beta, Nixie in both.
fn world() -> (tempfile::TempDir, Vec<Source>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let sources = vec![open(dir.path(), "alpha"), open(dir.path(), "beta")];
    for source in &sources {
        seed(source);
    }
    (dir, sources)
}

fn present(clients: &[&str]) -> Online {
    Some(
        clients
            .iter()
            .map(|c| (*c).to_owned())
            .collect::<HashSet<_>>(),
    )
}

fn nobody(_: &Source) -> Online {
    Some(HashSet::new())
}

#[test]
fn instance_label_uses_agent_and_host_not_client_nickname() {
    let (_dir, sources) = world();
    sources[0]
        .replica
        .mutate(vec![upsert(
            "clients",
            server(json!({
                "id": A, "actor_id": NIXIE, "kind": "instance", "name": "desk nickname",
                "host": "electra", "created_at_ms": 1, "revoked_at_ms": null, "dir": "alpha"
            })),
        )])
        .expect("client");
    let instances = instances_of(&sources[0], NIXIE);
    let instance = instances
        .iter()
        .find(|instance| instance.client_id == A)
        .expect("instance");
    assert_eq!(instance.label, "nixie@electra");
    assert_eq!(instance.client_id, A);
}

#[test]
fn rows_that_differ_show_the_newest_choice_and_say_how_each_database_differs() {
    let (_dir, sources) = world();
    sources[0]
        .replica
        .mutate(vec![prefs(A, &[B], 10)])
        .expect("alpha's row");
    sources[1]
        .replica
        .mutate(vec![prefs(B, &[A], 20)])
        .expect("beta's row");
    let vms = list(&sources, &nobody);
    assert_eq!(vms.len(), 1);
    let vm = &vms[0];
    assert_eq!(vm.databases, ["alpha", "beta"]);
    assert_eq!(
        vm.default_client.as_deref(),
        Some(B),
        "beta's row is the newer"
    );
    assert_eq!(vm.fallback, [A]);
    assert_eq!(
        vm.instances
            .iter()
            .map(|i| (i.client_id.as_str(), i.rank))
            .collect::<Vec<_>>(),
        [(B, Some(0)), (A, Some(1))]
    );
    let said = vm.disagreement.as_deref().expect("the rows differ");
    assert!(vm.reapply, "the order shown can be written into each again");
    assert!(
        said.contains("alpha asks nixie@electra, then nixie@hesperia"),
        "{said}"
    );
    assert!(
        said.contains("beta asks nixie@hesperia, then nixie@electra"),
        "{said}"
    );

    // A database with no row differs from one with a row.
    let (_dir, sources) = world();
    sources[0]
        .replica
        .mutate(vec![prefs(A, &[B], 10)])
        .expect("alpha's row");
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(vm.default_client.as_deref(), Some(A));
    assert!(vm
        .disagreement
        .as_deref()
        .is_some_and(|s| s.contains("beta has no choice")));

    // The same row in both: one choice, nothing to say.
    sources[1]
        .replica
        .mutate(vec![prefs(A, &[B], 10)])
        .expect("beta's row");
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(vm.disagreement, None);
    assert_eq!(
        vm.sentence,
        "New sessions go to nixie@electra; while it is offline, to nixie@hesperia."
    );
}

#[test]
fn a_choice_is_written_as_one_row_into_every_database_shared_with_the_agent() {
    let (dir, mut sources) = world();
    // gamma holds Ana but not Nixie: no row goes there.
    let gamma = open(dir.path(), "gamma");
    gamma
        .replica
        .mutate(vec![actor(ANA, "person", "Ana", "gamma")])
        .expect("gamma");
    sources.push(gamma);
    set_order(&sources, NIXIE, &[B.to_owned(), A.to_owned()], 42).expect("written");
    for source in &sources[..2] {
        let rows = source
            .replica
            .query(
                "SELECT id, person_actor, agent_actor, default_client, fallback, updated_at_ms, \
                 author_actor, author_client FROM instance_prefs",
                &[],
            )
            .expect("reads");
        assert_eq!(rows.len(), 1, "{}", source.database);
        let row = &rows[0];
        assert_eq!(row["id"], json!("ana/nixie"));
        assert_eq!(row["person_actor"], json!(ANA));
        assert_eq!(row["agent_actor"], json!(NIXIE));
        assert_eq!(row["default_client"], json!(B));
        assert_eq!(names(row, "fallback"), [A]);
        assert_eq!(row["author_client"], json!("ana.dev"));
    }
    let gamma_rows = sources[2]
        .replica
        .query("SELECT id FROM instance_prefs", &[])
        .expect("reads");
    assert!(gamma_rows.is_empty());
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(vm.default_client.as_deref(), Some(B));
    assert_eq!(vm.disagreement, None, "both rows are the same");

    for (order, refusal) in [
        (vec![], "Choose the instance that answers first."),
        (
            vec![A.to_owned(), A.to_owned()],
            "nixie.electra is named twice; each instance takes one place.",
        ),
        (
            vec!["dixi.electra".to_owned()],
            "dixi.electra is not an instance of nixie in your databases.",
        ),
    ] {
        assert_eq!(
            set_order(&sources, NIXIE, &order, 43),
            Err(refusal.to_owned())
        );
    }
    assert_eq!(
        set_order(&sources[2..], NIXIE, &[A.to_owned()], 43),
        Err("nixie is in no database this device holds.".to_owned())
    );
}

#[test]
fn an_instance_whose_definition_differs_from_its_siblings_is_marked() {
    let (_dir, sources) = world();
    // A third instance on an older image, in alpha only.
    sources[0]
        .replica
        .mutate(vec![
            client("nixie.laptop", "nixie@laptop", "alpha", None),
            profile("nixie.laptop", "alpha", "h-old", 5),
        ])
        .expect("third");
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(vm.instances.len(), 3);
    for instance in &vm.instances {
        if instance.client_id == "nixie.laptop" {
            assert_eq!(
                instance.definition_differs.as_deref(),
                Some(
                    "Runs a different definition from nixie@electra and nixie@hesperia: one of \
                     them runs an older or a newer image."
                )
            );
            assert_eq!(instance.databases, ["alpha"]);
        } else {
            assert_eq!(
                instance.definition_differs, None,
                "{} runs the majority's",
                instance.client_id
            );
        }
    }

    // Two instances that differ: neither is the majority, so both say so.
    let (_dir, sources) = world();
    sources[1]
        .replica
        .mutate(vec![profile(B, "beta", "h-two", 20)])
        .expect("restarted on a new image in beta");
    let vm = &list(&sources, &nobody)[0];
    let a = vm.instances.iter().find(|i| i.client_id == A).expect("a");
    let b = vm.instances.iter().find(|i| i.client_id == B).expect("b");
    assert_eq!(
        b.definition.as_deref(),
        Some("h-two"),
        "the newest profile row"
    );
    assert!(a
        .definition_differs
        .as_deref()
        .is_some_and(|s| s.contains("nixie@hesperia")));
    assert!(b
        .definition_differs
        .as_deref()
        .is_some_and(|s| s.contains("nixie@electra")));
}

#[test]
fn presence_is_online_offline_or_unknown_and_a_revoked_instance_is_gone() {
    let (_dir, sources) = world();
    sources[1]
        .replica
        .mutate(vec![client(B, "nixie@hesperia", "beta", Some(99))])
        .expect("revoked in beta");
    // alpha syncs and sees A; beta does not sync here.
    let online = |source: &Source| {
        if source.database == "alpha" {
            present(&[A])
        } else {
            None
        }
    };
    let vm = &list(&sources, &online)[0];
    let state = |client: &str| {
        vm.instances
            .iter()
            .find(|i| i.client_id == client)
            .map(|i| (i.presence, i.databases.clone()))
    };
    assert_eq!(
        state(A),
        Some((
            AgentInstancePresenceVm::Online,
            vec!["alpha".to_owned(), "beta".to_owned()]
        ))
    );
    // B is gone from alpha's presence (its socket closed, or the server let
    // its presence expire); its beta client is revoked, so beta lists it no
    // more.
    assert_eq!(
        state(B),
        Some((AgentInstancePresenceVm::Offline, vec!["alpha".to_owned()]))
    );
    // Nothing syncs here: nobody knows.
    let vm = &list(&sources, &|_: &Source| None)[0];
    assert!(vm
        .instances
        .iter()
        .all(|i| i.presence == AgentInstancePresenceVm::Unknown));
}

fn instance(id: &str, label: &str) -> Instance {
    Instance {
        client_id: id.to_owned(),
        label: label.to_owned(),
        ..Instance::default()
    }
}

fn claim(holder: &str, epoch: i64, released: bool) -> Claim {
    Claim {
        holder_client: holder.to_owned(),
        epoch,
        renewed_at_ms: 1,
        released,
    }
}

#[test]
fn the_header_names_the_holder_as_the_claim_moves_and_presence_comes_and_goes() {
    let instances = [instance(A, "nixie@electra"), instance(B, "nixie@hesperia")];
    let order = [B.to_owned(), A.to_owned()];
    let header = |claim: Option<&Claim>, online: Online| {
        answering_vm("Nixie", &instances, claim, false, Some(&order), &online).expect("a header")
    };

    // Nobody holds it: the first of the order that is online takes it next.
    let vm = header(None, present(&[A, B]));
    assert_eq!(vm.holder, None);
    assert_eq!(
        vm.sentence,
        "No instance holds this conversation yet; nixie@hesperia takes it next."
    );
    let vm = header(None, present(&[A]));
    assert_eq!(
        vm.sentence,
        "No instance holds this conversation yet; nixie@electra takes it next."
    );
    let vm = header(None, present(&[]));
    assert_eq!(
        vm.sentence,
        "None of the instances you chose is online; this conversation waits for one."
    );

    // B holds it: named first, and it stays B's while A comes and goes.
    let held = claim(B, 1, false);
    for online in [present(&[A, B]), present(&[B])] {
        let vm = header(Some(&held), online);
        assert_eq!(vm.holder.as_deref(), Some(B));
        assert_eq!(vm.sentence, "nixie@hesperia is answering.");
        assert_eq!(vm.instances[0].client_id, B);
        assert!(vm.instances[0].holds);
    }
    // B's presence expired: still the holder until another takes over.
    let vm = header(Some(&held), present(&[A]));
    assert_eq!(vm.holder.as_deref(), Some(B));
    assert_eq!(
        vm.sentence,
        "nixie@hesperia holds this conversation and is offline; another instance takes it over \
         once its lease runs out."
    );
    let states: Vec<(&str, AgentInstancePresenceVm, bool)> = vm
        .instances
        .iter()
        .map(|s| (s.client_id.as_str(), s.presence, s.holds))
        .collect();
    assert_eq!(
        states,
        [
            (B, AgentInstancePresenceVm::Offline, true),
            (A, AgentInstancePresenceVm::Online, false)
        ]
    );
    // A took it over (epoch 2): the header names A.
    let vm = header(Some(&claim(A, 2, false)), present(&[A]));
    assert_eq!(vm.holder.as_deref(), Some(A));
    assert_eq!(vm.sentence, "nixie@electra is answering.");
    // This device is offline: who holds it is said, not whether it is up.
    let vm = header(Some(&claim(A, 2, false)), None);
    assert_eq!(vm.sentence, "nixie@electra holds this conversation.");
    assert!(vm
        .instances
        .iter()
        .all(|s| s.presence == AgentInstancePresenceVm::Unknown));
    // Released: nobody holds it.
    let vm = header(Some(&claim(A, 2, true)), present(&[A]));
    assert_eq!(vm.holder, None);
    assert!(vm.instances.iter().all(|s| !s.holds));

    // No order known here (another person opened it): any online instance.
    let vm = answering_vm("Nixie", &instances, None, false, None, &present(&[A])).expect("header");
    assert_eq!(
        vm.sentence,
        "No instance holds this conversation yet; one that is online takes it next."
    );
    let vm = answering_vm("Nixie", &instances, None, false, None, &present(&[])).expect("header");
    assert_eq!(
        vm.sentence,
        "No instance of Nixie is online; this conversation waits for one."
    );
    // A holder the directory does not list yet is still named.
    let vm = answering_vm(
        "Nixie",
        &[],
        Some(&claim("nixie.new", 3, false)),
        false,
        None,
        &None,
    )
    .expect("header");
    assert_eq!(vm.instances.len(), 1);
    assert_eq!(vm.sentence, "nixie.new holds this conversation.");
    // No instance and no claim: no header line at all.
    assert_eq!(answering_vm("Nixie", &[], None, false, None, &None), None);
}

#[test]
fn an_open_session_carries_its_header_from_its_own_database() {
    use crate::hq::timeline::{self, Opened};
    let (_dir, sources) = world();
    let alpha = &sources[0];
    let at = crate::hq::sessions::now_ms();
    alpha
        .replica
        .mutate(vec![
            prefs(B, &[A], 10),
            upsert(
                "sessions",
                json!({"id": "s.1", "title": "plans", "status": "open", "agent_actor": NIXIE,
                       "created_by_actor": ANA, "created_by_client": "ana.dev",
                       "preferred_client": null, "parent_session": null, "invite": "[]",
                       "archived": false, "heard_until": null, "created_at_ms": at,
                       "last_activity_ms": at, "author_actor": ANA, "author_client": "ana.dev"}),
            ),
            upsert(
                "session_members",
                json!({"id": "s.1/ana", "session_id": "s.1", "actor_id": ANA, "role": "owner",
                       "reads_trace": true, "added_by_actor": ANA, "added_at_ms": at,
                       "read_until": null, "author_actor": ANA, "author_client": "ana.dev"}),
            ),
            upsert(
                "claims",
                json!({"session_id": "s.1", "holder_actor": NIXIE, "holder_client": A,
                       "epoch": 1, "renewed_at_ms": at, "released": false,
                       "author_actor": NIXIE, "author_client": A}),
            ),
        ])
        .expect("seed");
    let (view, _) =
        timeline::view(alpha, "s.1", &Opened::default(), &Default::default(), at).expect("a view");
    let answering = view.answering.expect("the header");
    assert_eq!(answering.holder.as_deref(), Some(A));
    // alpha does not sync in this test: who holds it, never "offline".
    assert_eq!(answering.sentence, "nixie@electra holds this conversation.");
    assert_eq!(answering.instances.len(), 2);
}

fn row_of(source: &Source) -> Option<(String, Vec<String>)> {
    let rows = source
        .replica
        .query(
            "SELECT default_client, fallback FROM instance_prefs WHERE id = ?1",
            &[json!(format!("{}/{NIXIE}", source.actor))],
        )
        .expect("reads");
    rows.first().map(|row| {
        (
            row["default_client"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            names(row, "fallback"),
        )
    })
}

#[test]
fn each_database_gets_the_order_less_the_instances_it_does_not_list() {
    const LAPTOP: &str = "nixie.laptop";
    let (_dir, sources) = world();
    // The laptop instance is in alpha's directory only.
    sources[0]
        .replica
        .mutate(vec![
            client(LAPTOP, "nixie@laptop", "alpha", None),
            profile(LAPTOP, "alpha", "h-one", 10),
        ])
        .expect("laptop");
    let order = [LAPTOP.to_owned(), A.to_owned(), B.to_owned()];
    set_order(&sources, NIXIE, &order, 42).expect("written");
    assert_eq!(
        row_of(&sources[0]),
        Some((LAPTOP.to_owned(), vec![A.to_owned(), B.to_owned()]))
    );
    assert_eq!(
        row_of(&sources[1]),
        Some((A.to_owned(), vec![B.to_owned()])),
        "beta never receives a client id its directory does not list"
    );
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(
        vm.default_client.as_deref(),
        Some(LAPTOP),
        "one choice shown"
    );
    assert_eq!(vm.fallback, [A, B]);
    assert_eq!(
        vm.disagreement, None,
        "each row is the choice, as its database goes"
    );
    assert!(!vm.reapply);
    // The two orders differ, so each is shown in its own terms.
    let shown: Vec<(&str, &[String], &str)> = vm
        .by_database
        .iter()
        .map(|db| {
            (
                db.database.as_str(),
                db.order.as_slice(),
                db.sentence.as_str(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (
                "alpha",
                &order[..],
                "alpha asks nixie@laptop, then nixie@electra, then nixie@hesperia; saved on this \
                 device, waiting to sync."
            ),
            (
                "beta",
                &order[1..],
                "beta asks nixie@electra, then nixie@hesperia; saved on this device, waiting to \
                 sync."
            ),
        ]
    );

    // Only the laptop: beta lists none of it, so its row is not touched and
    // the view says so in beta's terms.
    set_order(&sources, NIXIE, &[LAPTOP.to_owned()], 43).expect("written");
    assert_eq!(row_of(&sources[0]), Some((LAPTOP.to_owned(), vec![])));
    assert_eq!(
        row_of(&sources[1]),
        Some((A.to_owned(), vec![B.to_owned()])),
        "beta keeps its row"
    );
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(vm.default_client.as_deref(), Some(LAPTOP));
    assert_eq!(vm.disagreement, None);
    assert_eq!(
        vm.by_database[1].sentence,
        "None of the instances you chose is in beta, so its row is unchanged: it asks \
         nixie@electra, then nixie@hesperia."
    );

    // A database with no row and none of the order: nothing is written there.
    let (_dir, sources) = world();
    sources[0]
        .replica
        .mutate(vec![client(LAPTOP, "nixie@laptop", "alpha", None)])
        .expect("laptop");
    set_order(&sources, NIXIE, &[LAPTOP.to_owned()], 44).expect("written");
    assert_eq!(row_of(&sources[1]), None);
    let vm = &list(&sources, &nobody)[0];
    assert!(vm.by_database[1]
        .sentence
        .ends_with("its row is unchanged: it has no choice, so any online instance answers."));

    // One instance is a whole order: written into each database it is in.
    let (_dir, sources) = world();
    set_order(&sources, NIXIE, &[B.to_owned()], 45).expect("written");
    for source in &sources {
        assert_eq!(row_of(source), Some((B.to_owned(), vec![])));
    }
}

fn shared(database: &str, order: Option<&[&str]>, writes: RowWrites) -> Shared {
    Shared {
        database: database.to_owned(),
        instances: vec![instance(A, "nixie@electra"), instance(B, "nixie@hesperia")],
        prefs: order.map(|order| Prefs {
            default_client: order[0].to_owned(),
            fallback: order[1..].iter().map(|c| (*c).to_owned()).collect(),
            updated_ms: 42,
        }),
        online: None,
        writes,
    }
}

fn waiting() -> RowWrites {
    RowWrites {
        pending: vec!["c-waiting".to_owned()],
        refused: Vec::new(),
    }
}

#[test]
fn a_choice_one_database_took_and_another_still_waits_for_is_not_shown_as_agreement() {
    // alpha's server took the choice; beta's is offline, its write waiting.
    let vm = agent_view(
        NIXIE,
        "Nixie",
        &[
            shared("alpha", Some(&[B, A]), RowWrites::default()),
            shared("beta", Some(&[B, A]), waiting()),
        ],
    );
    assert_eq!(vm.disagreement, None, "the rows are the same here");
    assert_eq!(
        vm.delivery.as_deref(),
        Some("Saved in alpha; waiting to sync to beta.")
    );
    assert_eq!(
        vm.by_database
            .iter()
            .map(|db| (db.database.as_str(), db.pending))
            .collect::<Vec<_>>(),
        [("alpha", false), ("beta", true)]
    );
    assert!(!vm.reapply);

    // beta's server refused it; its row is back to what the server holds.
    let vm = agent_view(
        NIXIE,
        "Nixie",
        &[
            shared("alpha", Some(&[B, A]), RowWrites::default()),
            shared(
                "beta",
                Some(&[A, B]),
                RowWrites {
                    pending: Vec::new(),
                    refused: vec![Refusal {
                        commit_id: "c-1".to_owned(),
                        rejection: Rejection::new("hq.v9.person_actor", "not yours"),
                    }],
                },
            ),
        ],
    );
    assert_eq!(vm.delivery, None, "nothing waits any more");
    assert_eq!(
        vm.notice.as_deref(),
        Some("The server refused your choice in beta: hq.v9.person_actor: not yours.")
    );
    assert_eq!(
        vm.by_database[1].refused.as_deref(),
        Some("hq.v9.person_actor: not yours")
    );
    assert!(vm.disagreement.is_some(), "beta still asks its old order");
    assert!(vm.reapply, "the order shown can be written again");

    // Settled in both: nothing per database to say.
    let vm = agent_view(
        NIXIE,
        "Nixie",
        &[
            shared("alpha", Some(&[B, A]), RowWrites::default()),
            shared("beta", Some(&[B, A]), RowWrites::default()),
        ],
    );
    assert_eq!(
        (vm.delivery, vm.notice, vm.disagreement),
        (None, None, None)
    );
    assert!(vm.by_database.is_empty());
    assert!(!vm.reapply);
}

/// Record, as a round would, beta's server refusing a commit that wrote
/// `row_id` — straight into the replica file's outcome journal.
fn journal_refusal(path: &std::path::Path, commit: &str, row_id: &str) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let operations = vec![CommitOperation {
        table: "instance_prefs".to_owned(),
        row_id: row_id.to_owned(),
        op: "upsert".to_owned(),
        base_version: None,
        values: serde_json::json!({ "id": row_id, "default_client": "nixie.electra" })
            .as_object()
            .cloned(),
    }];
    let results = vec![CommitOperationOutcome::Error {
        rejection: RejectionRecord {
            client_commit_id: commit.to_owned(),
            op_index: 0,
            code: "hq.v9.person_actor".to_owned(),
            message: "hq.v9.person_actor".to_owned(),
            retryable: false,
            details: None,
            operation: None,
        },
    }];
    let conn = rusqlite::Connection::open(path).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    conn.execute(
        "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
         results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
        rusqlite::params![
            commit,
            serde_json::to_string(&results).expect("results"),
            serde_json::to_string(&operations).expect("operations")
        ],
    )
    .expect("records");
}

fn restart(dir: &std::path::Path, sources: Vec<Source>) -> Vec<Source> {
    let names: Vec<(String, String)> = sources
        .iter()
        .map(|s| (s.database.clone(), s.actor.clone()))
        .collect();
    for source in &sources {
        source.replica.close();
        source.replica.join();
    }
    drop(sources);
    names
        .iter()
        .map(|(database, actor)| open_as(dir, database, actor))
        .collect()
}

#[test]
fn what_waits_and_what_was_refused_survives_a_restart_and_belongs_to_its_person() {
    let (dir, sources) = world();
    set_order(&sources, NIXIE, &[B.to_owned(), A.to_owned()], 42).expect("written offline");
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(
        vm.delivery.as_deref(),
        Some("Saved on this device; waiting to sync to alpha and beta.")
    );

    // Quit and reopen: nothing of the last process but the files.
    let sources = restart(dir.path(), sources);
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(
        vm.delivery.as_deref(),
        Some("Saved on this device; waiting to sync to alpha and beta."),
        "the outbox says what still waits"
    );
    assert_eq!(vm.notice, None);

    // beta's server refuses the queued write after the restart.
    journal_refusal(&dir.path().join("beta.sqlite"), "c-beta", "ana/nixie");
    // Another person's refusal in the same file is not Ana's.
    journal_refusal(&dir.path().join("alpha.sqlite"), "c-kuba", "kuba/nixie");
    let vm = &list(&sources, &nobody)[0];
    assert_eq!(
        vm.notice.as_deref(),
        Some("The server refused your choice in beta: hq.v9.person_actor.")
    );
    assert!(vm.reapply);
    assert_eq!(
        vm.by_database[0].refused, None,
        "alpha refused nothing of Ana's"
    );

    // Kuba, on another server, with the same agent and database names, sees
    // none of it.
    let other = tempfile::tempdir().expect("tempdir");
    let kuba: Vec<Source> = ["alpha", "beta"]
        .iter()
        .map(|db| open_as(other.path(), db, "kuba"))
        .collect();
    for source in &kuba {
        seed(source);
    }
    let vm = &list(&kuba, &nobody)[0];
    assert_eq!((vm.notice.as_deref(), vm.delivery.as_deref()), (None, None));

    // Choosing again sets the refusal aside, and that too survives a restart.
    set_order(&sources, NIXIE, &[B.to_owned(), A.to_owned()], 43).expect("written");
    assert_eq!(list(&sources, &nobody)[0].notice, None);
    let sources = restart(dir.path(), sources);
    assert_eq!(list(&sources, &nobody)[0].notice, None);
}

#[test]
fn a_connected_socket_is_not_a_complete_view_of_who_is_online() {
    use crate::hq::port::{fake::FakeOpener, LinkState, Opener};
    let opener = FakeOpener::default();
    opener
        .open(ReplicaSpec {
            database: "alpha".to_owned(),
            path: std::path::PathBuf::from("alpha.sqlite"),
            client_id: Some("ana.dev".to_owned()),
            link: Some(crate::hq::port::Link {
                base_url: "https://hq.example.org/alpha".to_owned(),
                token: "t".to_owned(),
            }),
            subscriptions: Vec::new(),
            watch_presence: Some("dir:alpha".to_owned()),
        })
        .expect("opens");
    let replica = opener.all().remove(0);
    let source = Source {
        database: "alpha".to_owned(),
        replica: replica.clone(),
        actor: ANA.to_owned(),
        client: "ana.dev".to_owned(),
    };
    // Connected, the directory's peers still in flight: nobody knows yet.
    replica.set(LinkState::Connected);
    assert_eq!(online(&source), None);
    // The peers already present have arrived — here none: an established
    // empty directory, everyone offline.
    replica.set_presence_known(true);
    assert_eq!(online(&source), Some(HashSet::new()));
    // Reconnecting starts over.
    replica.set(LinkState::Connecting);
    replica.set_presence_known(false);
    assert_eq!(online(&source), None);
    replica.set(LinkState::Connected);
    assert_eq!(
        online(&source),
        None,
        "the new socket's peers are not here yet"
    );
}

#[test]
fn a_holder_whose_lease_ran_out_is_named_as_having_held_it_never_as_answering() {
    let instances = [instance(A, "nixie@electra"), instance(B, "nixie@hesperia")];
    let order = [B.to_owned(), A.to_owned()];
    let now_ms = crate::hq::sessions::now_ms();
    let t0 = std::time::Instant::now();
    let held = Claim {
        holder_client: B.to_owned(),
        epoch: 1,
        renewed_at_ms: now_ms,
        released: false,
    };
    let mut live = Live::default();
    live.observe(&[], &[], Some(held.clone()), t0, now_ms);
    assert!(!live.claim_lapsed(&held, now_ms));
    assert_eq!(
        live.lapses_in(t0, now_ms),
        Some(LEASE),
        "the header is drawn again when the lease would lapse"
    );
    // B stops renewing while its presence stays up: the same row, a lease
    // later, with no new row to wake anything.
    let later = t0 + LEASE;
    live.observe(&[], &[], Some(held.clone()), later, now_ms + 30_000);
    assert!(live.claim_lapsed(&held, now_ms + 30_000));
    assert_eq!(live.lapses_in(later, now_ms + 30_000), None);
    let vm = answering_vm(
        "Nixie",
        &instances,
        Some(&held),
        true,
        Some(&order),
        &present(&[A, B]),
    )
    .expect("a header");
    assert_eq!(vm.holder, None, "nobody is answering");
    assert_eq!(
        vm.sentence,
        "nixie@hesperia held this conversation; its lease ran out, so nixie@electra takes it \
         over next."
    );
    assert_eq!(
        vm.instances
            .iter()
            .map(|s| (s.client_id.as_str(), s.holds, s.lapsed))
            .collect::<Vec<_>>(),
        [(B, false, true), (A, false, false)]
    );
    // Every instance stopped, and none takes it over: no promise of expiry.
    let vm = answering_vm(
        "Nixie",
        &instances,
        Some(&held),
        true,
        Some(&order),
        &present(&[]),
    )
    .expect("a header");
    assert_eq!(
        vm.sentence,
        "nixie@hesperia held this conversation; its lease ran out; none of the other instances \
         you chose is online, so it waits for one."
    );
    assert!(!vm.sentence.contains("once its lease runs out"));
    // A takes it over: a new row, a current holder again.
    let taken = Claim {
        holder_client: A.to_owned(),
        epoch: 2,
        renewed_at_ms: now_ms + 31_000,
        released: false,
    };
    live.observe(&[], &[], Some(taken.clone()), later, now_ms + 31_000);
    assert!(!live.claim_lapsed(&taken, now_ms + 31_000));
    let vm = answering_vm(
        "Nixie",
        &instances,
        Some(&taken),
        false,
        Some(&order),
        &present(&[A]),
    )
    .expect("a header");
    assert_eq!(vm.holder.as_deref(), Some(A));
    assert_eq!(vm.sentence, "nixie@electra is answering.");
    // A session opened long after its holder stopped: a claim never watched
    // here is judged by its own time.
    let ancient = Claim {
        renewed_at_ms: 1,
        ..held
    };
    assert!(Live::default().claim_lapsed(&ancient, now_ms));
}
