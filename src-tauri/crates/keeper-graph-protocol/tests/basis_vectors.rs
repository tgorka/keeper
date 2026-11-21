//! `approval_bases` in `syncular/knowledge-graph.vectors.json` holds for
//! [`ApprovalBasis`] (story 114.6), and a `{basis: "grant"}` event is read
//! by the event header clients have shipped with.

use keeper_graph_protocol::basis::{grant_id, ApprovalBasis, GrantMode};
use keeper_graph_protocol::vocabulary::OriginClass;
use keeper_graph_protocol::wire::EventHeader;
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn section() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vector JSON");
    corpus["approval_bases"].clone()
}

fn entries<'a>(section: &'a Value, key: &str) -> &'a [Value] {
    section[key].as_array().expect(key)
}

#[test]
fn the_modes_are_the_vectors() {
    assert_eq!(
        section()["modes"],
        serde_json::to_value(GrantMode::ALL).expect("modes")
    );
}

#[test]
fn accepted_bases_round_trip_and_refused_ones_do_not_parse() {
    let v = section();
    for entry in entries(&v, "accepted") {
        let basis: ApprovalBasis = serde_json::from_value(entry["json"].clone())
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(
            serde_json::to_value(&basis).expect("serialise"),
            entry["json"],
            "{}",
            entry["name"]
        );
    }
    for entry in entries(&v, "refused") {
        assert!(
            serde_json::from_value::<ApprovalBasis>(entry["json"].clone()).is_err(),
            "{} parsed",
            entry["name"]
        );
    }
}

#[test]
fn a_grant_basis_names_its_grant_by_domain_and_actor() {
    let v = section();
    for entry in entries(&v, "accepted") {
        if let Ok(ApprovalBasis::Grant { grant_id: id, .. }) =
            serde_json::from_value(entry["json"].clone())
        {
            let (domain, actor) = id.split_once('/').expect("slash");
            assert_eq!(grant_id(domain, actor), id, "{}", entry["name"]);
        }
    }
}

fn bases(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(basis @ Value::Object(_)) = map.get("approval_basis") {
                out.push(basis.clone());
            }
            map.values().for_each(|v| bases(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| bases(v, out)),
        _ => {}
    }
}

#[test]
fn every_basis_the_vectors_record_is_one_of_the_forms() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vector JSON");
    let mut found = Vec::new();
    bases(&corpus, &mut found);
    assert!(found.len() > 60);
    for basis in found {
        let parsed: ApprovalBasis =
            serde_json::from_value(basis.clone()).unwrap_or_else(|e| panic!("{basis}: {e}"));
        assert_eq!(serde_json::to_value(parsed).expect("serialise"), basis);
    }
}

#[test]
fn a_shipped_header_reads_a_grant_event_as_an_agent_s() {
    let v = section();
    let events = entries(&v, "events");
    assert!(!events.is_empty());
    for entry in events {
        let header: EventHeader = serde_json::from_value(entry["event"].clone())
            .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert_eq!(header.origin_class, OriginClass::Agent, "{}", entry["name"]);
        assert!(header.context_id.is_some(), "{}", entry["name"]);
        assert_eq!(
            serde_json::to_value(&header).expect("serialise"),
            entry["event"],
            "{}",
            entry["name"]
        );
        let basis: ApprovalBasis =
            serde_json::from_value(header.approval_basis.clone().expect("basis"))
                .unwrap_or_else(|e| panic!("{}: {e}", entry["name"]));
        assert!(
            matches!(
                basis,
                ApprovalBasis::Grant {
                    mode: GrantMode::Apply,
                    ..
                }
            ),
            "{}",
            entry["name"]
        );
    }
}
