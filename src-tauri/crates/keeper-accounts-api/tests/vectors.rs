//! `syncular/accounts.vectors.json` holds for this crate, and
//! `syncular/vocabulary.json`'s `roles` is [`Role::ALL`]. keeper-hq's
//! TypeScript reads the same two files.

use keeper_accounts_api::admin::{AdminAnswer, AdminRequest, Barrier, MapView};
use keeper_accounts_api::discovery::Discovery;
use keeper_accounts_api::refusal::Refusal;
use keeper_accounts_api::{DomainKind, FilesWrites, Role};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/accounts.vectors.json");
const VOCABULARY: &str = include_str!("../../../../syncular/vocabulary.json");

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
