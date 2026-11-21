//! `syncular/accounts.vectors.json` holds for this crate, and
//! `syncular/vocabulary.json`'s `roles` is [`Role::ALL`]. keeper-hq's
//! TypeScript reads the same two files.

use keeper_accounts_api::admin::{AdminAnswer, AdminCommand, AdminRequest, Barrier, MapView};
use keeper_accounts_api::discovery::Discovery;
use keeper_accounts_api::grant::{AgentGrantRecord, CapabilityClass, GrantMode};
use keeper_accounts_api::refusal::Refusal;
use keeper_accounts_api::{DomainKind, FilesWrites, Role};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/accounts.vectors.json");
const VOCABULARY: &str = include_str!("../../../../syncular/vocabulary.json");
const GRAPH: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("vector JSON")
}

fn entries<'a>(corpus: &'a Value, key: &str) -> &'a [Value] {
    corpus[key].as_array().expect(key)
}

fn names<T: Serialize>(items: &[T]) -> Value {
    serde_json::to_value(items).expect("names")
}

/// Parses `json` as `T` and serialises it back.
fn round_trip<T: Serialize + DeserializeOwned>(json: &Value) -> Result<Value, serde_json::Error> {
    let parsed: T = serde_json::from_value(json.clone())?;
    Ok(serde_json::to_value(parsed).expect("serialise"))
}

#[test]
fn the_four_roles_are_the_vocabulary_s() {
    let vocabulary: Value = serde_json::from_str(VOCABULARY).expect("vocabulary JSON");
    assert_eq!(vocabulary["roles"], names(&Role::ALL));
    assert_eq!(corpus()["roles"], names(&Role::ALL));
    for role in Role::ALL {
        assert_eq!(serde_json::to_value(role).expect("role"), role.as_str());
    }
}

#[test]
fn closed_sets_agree() {
    let v = corpus();
    assert_eq!(
        v["kinds"],
        names(&[DomainKind::General, DomainKind::Personal, DomainKind::Group])
    );
    assert_eq!(
        v["files_writes"],
        names(&[FilesWrites::Shadow, FilesWrites::Open])
    );
    assert_eq!(
        v["barriers"],
        names(&[
            Barrier::Open,
            Barrier::Starting,
            Barrier::Reconciling,
            Barrier::Failed,
            Barrier::StaleRestore,
        ])
    );
    assert_eq!(v["capability_classes"], names(&CapabilityClass::ALL));
    assert_eq!(v["grant_modes"], names(&GrantMode::ALL));
    let graph: Value = serde_json::from_str(GRAPH).expect("graph vector JSON");
    assert_eq!(graph["approval_bases"]["modes"], v["grant_modes"]);
}

#[test]
fn a_barrier_state_admits_only_when_open_and_an_unknown_one_is_closed() {
    let v = corpus();
    for entry in entries(&v, "barrier_reads") {
        let state: Barrier = serde_json::from_value(entry["state"].clone()).expect("state");
        assert_eq!(state.is_open(), entry["open"], "{}", entry["state"]);
        assert_eq!(serde_json::to_value(&state).expect("state"), entry["state"]);
        let known = v["barriers"]
            .as_array()
            .expect("barriers")
            .contains(&entry["state"]);
        assert_eq!(
            !known,
            matches!(state, Barrier::Other(_)),
            "{}",
            entry["state"]
        );
    }
}

#[test]
fn discovery_bodies_parse_to_what_they_want() {
    let v = corpus();
    for entry in entries(&v, "discovery") {
        let got = round_trip::<Discovery>(&entry["json"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["want"], "{}", entry["name"]);
    }
    for entry in entries(&v, "discovery_refusals") {
        assert!(
            serde_json::from_value::<Discovery>(entry["json"].clone()).is_err(),
            "{} parsed",
            entry["name"]
        );
    }
}

#[test]
fn admin_requests_round_trip_and_malformed_ones_do_not_parse() {
    let v = corpus();
    for entry in entries(&v, "admin_requests") {
        let got = round_trip::<AdminRequest>(&entry["json"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["json"], "{}", entry["name"]);
    }
    for entry in entries(&v, "admin_request_refusals") {
        assert!(
            serde_json::from_value::<AdminRequest>(entry["json"].clone()).is_err(),
            "{} parsed",
            entry["name"]
        );
        assert_eq!(
            entry["refusal"],
            Refusal::Malformed.code(),
            "{}",
            entry["name"]
        );
    }
}

#[test]
fn admin_answers_and_map_views_round_trip() {
    let v = corpus();
    for entry in entries(&v, "admin_answers") {
        let got = round_trip::<AdminAnswer>(&entry["json"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["json"], "{}", entry["name"]);
    }
    for entry in entries(&v, "map_views") {
        let got = round_trip::<MapView>(&entry["json"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["json"], "{}", entry["name"]);
    }
}

#[test]
fn map_commands_parse_and_name_the_contract_s_codes() {
    let v = corpus();
    let fixture = &v["map"];
    for key in ["domains", "people", "agents"] {
        let names = fixture[key].as_array().expect(key);
        assert!(names.iter().all(Value::is_string), "map.{key}");
    }
    for entry in entries(&v, "command_refusals") {
        let got = round_trip::<AdminCommand>(&entry["command"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["command"], "{}", entry["name"]);
        let code = entry["refusal"].as_str().expect("refusal");
        assert!(Refusal::from_code(code).is_some(), "{}", entry["name"]);
    }
    for entry in entries(&v, "command_applications") {
        let got = round_trip::<AdminCommand>(&entry["command"])
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(got, entry["command"], "{}", entry["name"]);
        for side in ["before", "after"] {
            let got = round_trip::<Vec<AgentGrantRecord>>(&entry[side])
                .unwrap_or_else(|e| panic!("{} {side}: {e}", entry["name"]));
            assert_eq!(got, entry[side], "{} {side}", entry["name"]);
        }
    }
}

#[test]
fn a_listed_grant_s_id_and_modes_are_what_its_events_carry() {
    let v = corpus();
    let views = entries(&v, "map_views");
    let grants: Vec<AgentGrantRecord> = views
        .iter()
        .filter_map(|entry| entry["json"].get("agent_grants"))
        .flat_map(|grants| {
            serde_json::from_value::<Vec<AgentGrantRecord>>(grants.clone()).expect("grants")
        })
        .collect();
    let graph: Value = serde_json::from_str(GRAPH).expect("graph vector JSON");
    let events = graph["approval_bases"]["events"]
        .as_array()
        .expect("events");
    let granted: Vec<&Value> = events
        .iter()
        .filter(|e| e["event"]["approval_basis"]["basis"] == "grant")
        .collect();
    assert!(granted.iter().any(|e| e.get("root").is_some()));
    assert!(granted.iter().any(|e| e.get("root").is_none()));
    for event in granted {
        let name = &event["name"];
        let basis = &event["event"]["approval_basis"];
        let grant = grants
            .iter()
            .find(|g| g.grant_id() == basis["grant_id"] && g.revision == basis["grant_revision"])
            .unwrap_or_else(|| panic!("{name}: no listed grant {basis}"));
        assert_eq!(
            event["event"]["actor_id"],
            grant.agent_id.as_str(),
            "{name}"
        );
        let class = if event["event"]["descriptor"] == "graph.reverse" {
            CapabilityClass::ReverseOwn
        } else {
            CapabilityClass::Content
        };
        assert!(grant.capability.holds(class), "{name}: no {class:?}");
        let mode = match event.get("root") {
            // An agent instance's change: the mode its grant gives the root.
            Some(root) => grant.capability.mode_of(root.as_str().expect("root")),
            // A tool's: no mode per root, its whole grant lands `apply`.
            None => {
                assert_eq!(grant.capability.fields().root_modes, None, "{name}");
                GrantMode::Apply
            }
        };
        assert_eq!(
            basis["mode"],
            serde_json::to_value(mode).expect("mode"),
            "{name}"
        );
    }
}

#[test]
fn refusal_codes_and_statuses_are_the_vectors() {
    let v = corpus();
    let listed = entries(&v, "refusals");
    assert_eq!(listed.len(), Refusal::ALL.len());
    for (refusal, entry) in Refusal::ALL.into_iter().zip(listed) {
        assert_eq!(entry["code"], refusal.code());
        assert_eq!(
            entry["status"],
            serde_json::to_value(refusal.status()).expect("status")
        );
        assert_eq!(Refusal::from_code(refusal.code()), Some(refusal));
    }
}
