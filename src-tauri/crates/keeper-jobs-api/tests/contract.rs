//! Contract tests for the jobs schema, scope patterns, kinds, and row shape.

use std::collections::BTreeSet;

use keeper_jobs_api::is_kind;
use keeper_jobs_api::model::Job;
use serde_json::{json, Value};

const MANIFEST: &str = include_str!("../../../../syncular/syncular.json");
const MIGRATION: &str = include_str!("../../../../syncular/migrations/0012_jobs/up.sql");

fn jobs_table_columns() -> BTreeSet<String> {
    let mut in_table = false;
    let mut columns = BTreeSet::new();
    for line in MIGRATION.lines() {
        let trimmed = line.trim();
        if trimmed == "CREATE TABLE jobs (" {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if trimmed.starts_with(");") {
            break;
        }
        if let Some(column) = trimmed.split_whitespace().next() {
            columns.insert(column.to_owned());
        }
    }
    columns
}

#[test]
fn no_scope_pattern_of_a_job_table_names_a_kind() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("syncular.json is valid JSON");
    let tables = manifest["tables"]
        .as_array()
        .expect("manifest has tables array");

    let mut seen = BTreeSet::new();
    for table in tables {
        let name = table["name"].as_str().expect("table has name");
        if !matches!(name, "jobs" | "job_claims" | "job_capabilities") {
            continue;
        }
        seen.insert(name);
        let scopes = table["scopes"]
            .as_array()
            .expect("job table has scopes array");
        assert!(!scopes.is_empty(), "{name} must declare at least one scope");
        for scope in scopes {
            let column = scope["column"]
                .as_str()
                .expect("job table scope is an object with column");
            let pattern = scope["pattern"]
                .as_str()
                .expect("job table scope is an object with pattern");
            assert!(
                column == "page_key" || column == "domain",
                "{name} scope uses unexpected column {column}"
            );
            assert_ne!(column, "kind");
            assert!(
                !pattern.contains("kind"),
                "{name} scope pattern {pattern} must not name kind"
            );
        }
    }

    let expected = BTreeSet::from(["jobs", "job_claims", "job_capabilities"]);
    assert_eq!(seen, expected);

    let subscriptions = manifest["subscriptions"]
        .as_array()
        .expect("manifest has subscriptions array");
    let jobs_page = subscriptions
        .iter()
        .find(|sub| sub["name"].as_str() == Some("jobsPage"))
        .expect("jobsPage subscription exists");
    assert_eq!(jobs_page["scopes"], json!({ "jobs_page": "{pageKey}" }));

    let jobs_nodes = subscriptions
        .iter()
        .find(|sub| sub["name"].as_str() == Some("jobsNodes"))
        .expect("jobsNodes subscription exists");
    assert_eq!(jobs_nodes["scopes"], json!({ "jobs_nodes": "{database}" }));
}

#[test]
fn the_contract_names_no_kind() {
    assert!(!MIGRATION.to_uppercase().contains("CHECK"));
    assert!(MIGRATION.contains("kind TEXT NOT NULL"));
    for forbidden in ["files.extract", "fixture.", "test.alpha", "test.beta"] {
        assert!(
            !MIGRATION.contains(forbidden),
            "migration must not name {forbidden}"
        );
        assert!(
            !MANIFEST.contains(forbidden),
            "manifest must not name {forbidden}"
        );
    }
}

#[test]
fn kind_is_any_namespaced_name() {
    for valid in ["files.extract", "test.alpha", "a.b.c", "x1.y_2"] {
        assert!(is_kind(valid), "{valid} should be a valid kind");
    }
    let too_long = format!("a.{}", "b".repeat(63));
    assert_eq!(too_long.len(), 65);
    for invalid in [
        "extract",
        "Files.extract",
        "files.",
        ".x",
        "files:extract",
        too_long.as_str(),
    ] {
        assert!(!is_kind(invalid), "{invalid} should be rejected as a kind");
    }
}

#[test]
fn the_job_columns_hold_any_kind() {
    let columns = jobs_table_columns();
    assert!(!columns.is_empty(), "CREATE TABLE jobs block not found");

    let blob_a = format!("sha256:{}", "a".repeat(64));
    let blob_b = format!("sha256:{}", "b".repeat(64));
    let digest_a = format!("sha256:{}", "1".repeat(64));
    let digest_b = format!("sha256:{}", "2".repeat(64));
    let beta_id = "01JD0000000000000000000002";

    let alpha_row = json!({
        "id": "01JD0000000000000000000001",
        "domain": "dom_alpha",
        "page_key": "dom_alpha/open",
        "kind": "test.alpha",
        "input": {
            "refs": [{ "type": "row", "table": "messages", "id": "m1" }],
            "params": { "lang": "en" }
        },
        "input_blob": null,
        "input_digest": digest_a,
        "runs_on": "server",
        "state": "queued",
        "attempt": 0,
        "max_attempts": 3,
        "not_before_ms": 1_700_000_000_000_u64,
        "plan_blob": null,
        "plan_attempt_id": null,
        "note": null,
        "result": null,
        "result_blob": null,
        "effects": null,
        "error_code": null,
        "error_sentence": null,
        "submitter_actor": "actor_1",
        "submitter_client": "client_1",
        "submitter_context": "01JD0000000000000000000010",
        "submitted_at_ms": 1_700_000_000_000_u64,
        "updated_at_ms": 1_700_000_000_000_u64,
        "ended_at_ms": null,
        "cancel_requested_at_ms": null,
        "revision": 1,
        "author_actor": "actor_1",
        "author_client": "client_1"
    });

    let beta_row = json!({
        "id": beta_id,
        "domain": "dom_beta",
        "page_key": "dom_beta/2026-11",
        "kind": "test.beta",
        "input": null,
        "input_blob": { "blobId": blob_a, "byteLength": 42 },
        "input_digest": digest_b,
        "runs_on": "node:gpu",
        "state": "succeeded",
        "attempt": 1,
        "max_attempts": 3,
        "not_before_ms": 1_700_000_000_000_u64,
        "plan_blob": { "blobId": blob_b, "byteLength": 128 },
        "plan_attempt_id": "01JD0000000000000000000003",
        "note": "done",
        "result": { "ok": true },
        "result_blob": null,
        "effects": [
            {
                "id": { "job_id": beta_id, "ordinal": 0 },
                "produced": { "type": "proposal", "proposal_id": "p1" }
            }
        ],
        "error_code": null,
        "error_sentence": null,
        "submitter_actor": "actor_2",
        "submitter_client": "client_2",
        "submitter_context": null,
        "submitted_at_ms": 1_700_000_000_000_u64,
        "updated_at_ms": 1_700_000_005_000_u64,
        "ended_at_ms": 1_700_000_005_000_u64,
        "cancel_requested_at_ms": null,
        "revision": 4,
        "author_actor": "actor_2",
        "author_client": "client_2"
    });

    for row in [alpha_row, beta_row] {
        let job: Job = serde_json::from_value(row.clone()).expect("job row deserializes");
        let round_trip = serde_json::to_value(&job).expect("job row serializes");
        assert_eq!(round_trip, row);

        let keys: BTreeSet<String> = round_trip
            .as_object()
            .expect("serialized job is an object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, columns);
    }
}
