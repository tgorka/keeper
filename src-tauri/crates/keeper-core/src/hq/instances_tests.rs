//! Which instance answers, over real on-disk replicas seeded through
//! `mutate`, with no server: the rows of two databases that agree or differ,
//! an instance whose definition differs from its siblings', who holds a
//! session as its claim moves, and presence coming and going.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{answering_vm, list, set_order, AgentInstancePresenceVm, Instance, Online};
use crate::hq::live::Claim;
use crate::hq::port::{ReplicaSpec, RowOp};
use crate::hq::sessions::{names, object, Source};
use crate::hq::syncular::SyncularReplica;

const ANA: &str = "ana";
const NIXIE: &str = "nixie";
const A: &str = "nixie.electra";
const B: &str = "nixie.hesperia";

fn open(dir: &std::path::Path, database: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("{database}.sqlite")),
        client_id: Some("ana.dev".to_owned()),
        link: None,
        subscriptions: Vec::new(),
    })
    .expect("opens");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: ANA.to_owned(),
        client: "ana.dev".to_owned(),
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
        answering_vm("Nixie", &instances, claim, Some(&order), &online).expect("a header")
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
    let vm = answering_vm("Nixie", &instances, None, None, &present(&[A])).expect("header");
    assert_eq!(
        vm.sentence,
        "No instance holds this conversation yet; one that is online takes it next."
    );
    let vm = answering_vm("Nixie", &instances, None, None, &present(&[])).expect("header");
    assert_eq!(
        vm.sentence,
        "No instance of Nixie is online; this conversation waits for one."
    );
    // A holder the directory does not list yet is still named.
    let vm = answering_vm(
        "Nixie",
        &[],
        Some(&claim("nixie.new", 3, false)),
        None,
        &None,
    )
    .expect("header");
    assert_eq!(vm.instances.len(), 1);
    assert_eq!(vm.sentence, "nixie.new holds this conversation.");
    // No instance and no claim: no header line at all.
    assert_eq!(answering_vm("Nixie", &[], None, None, &None), None);
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
