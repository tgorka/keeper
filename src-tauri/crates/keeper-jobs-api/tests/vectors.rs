//! `syncular/jobs.vectors.json` holds for this crate: every envelope check,
//! and every command sequence over a job and its claim, read as keeper-hq's
//! server tests read them (122.1-AC1, AC2, AC3, AC6).

use keeper_jobs_api::command::{check, Descriptor, Payload};
use keeper_jobs_api::model::{AttemptId, JobCapabilities, JobState};
use keeper_jobs_api::transition::{declare, judge, moves, Caller, Context, Judged, Rows};
use keeper_jobs_api::{Refusal, CONTRACT_VERSION, LIMITS};
use serde_json::{json, Map, Value};

const CORPUS: &str = include_str!("../../../../syncular/jobs.vectors.json");
const LIMITS_JSON: &str = include_str!("../../../../syncular/limits.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("vector JSON")
}

fn entries<'a>(corpus: &'a Value, key: &str) -> &'a [Value] {
    corpus[key].as_array().expect(key)
}

fn outcome_of(result: Result<(), Refusal>) -> String {
    match result {
        Ok(()) => "accepted".to_owned(),
        Err(refusal) => refusal.code().to_owned(),
    }
}

#[test]
fn closed_sets_and_bounds_agree() {
    let v = corpus();
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
    let descriptors: Vec<&str> = Descriptor::ALL.iter().map(|d| d.as_str()).collect();
    assert_eq!(v["descriptors"], json!(descriptors));
    for d in Descriptor::ALL {
        assert_eq!(serde_json::to_value(d).expect("descriptor"), d.as_str());
        assert_eq!(Descriptor::parse(d.as_str()), Some(d));
    }
    assert_eq!(
        v["states"],
        serde_json::to_value(JobState::ALL).expect("states")
    );
    let codes: Vec<&str> = Refusal::ALL.iter().map(|r| r.code()).collect();
    assert_eq!(v["refusals"], json!(codes));
    for refusal in Refusal::ALL {
        assert_eq!(serde_json::to_value(refusal).expect("code"), refusal.code());
    }
    // The vectors' bounds are limits.json's job bounds, which the crate compiled in.
    let limits: Value = serde_json::from_str(LIMITS_JSON).expect("limits JSON");
    let jobs: Map<String, Value> = limits
        .as_object()
        .expect("limits object")
        .iter()
        .filter(|(key, _)| key.starts_with("job"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    assert_eq!(v["limits"], Value::Object(jobs));
    let compiled = json!({
        "jobRowMaxBytes": LIMITS.row_max_bytes,
        "jobInlineMaxBytes": LIMITS.inline_max_bytes,
        "jobNoteMaxBytes": LIMITS.note_max_bytes,
        "jobRefsMax": LIMITS.refs_max,
        "jobEffectsMax": LIMITS.effects_max,
        "jobPlanMaxBytes": LIMITS.plan_max_bytes,
        "jobOpenMaxPerDomain": LIMITS.open_max_per_domain,
        "jobAttemptsMax": LIMITS.attempts_max,
        "jobLeaseMinMs": LIMITS.lease_min_ms,
        "jobLeaseMaxMs": LIMITS.lease_max_ms,
    });
    assert_eq!(v["limits"], compiled);
}

fn bytes_of(entry: &Value) -> Vec<u8> {
    match entry.get("envelope_text") {
        Some(text) => text.as_str().expect("envelope_text").as_bytes().to_vec(),
        None => serde_json::to_vec(&entry["envelope"]).expect("encode"),
    }
}

#[test]
fn every_envelope_is_checked_as_the_vectors_say() {
    let v = corpus();
    let checks = entries(&v, "checks");
    assert!(checks.len() >= 45, "the corpus holds the boundary cases");
    for entry in checks {
        let name = entry["name"].as_str().expect("name");
        let descriptor: Descriptor =
            serde_json::from_value(entry["descriptor"].clone()).expect("descriptor");
        let result = check(descriptor, &bytes_of(entry), &LIMITS);
        let got = outcome_of(result.as_ref().map(|_| ()).map_err(|r| *r));
        assert_eq!(got, entry["outcome"], "{name}");
        if let (Ok(command), Some(digest)) = (&result, entry.get("digest")) {
            assert_eq!(command.digest().expect("digest"), *digest, "{name}");
        }
    }
}

const REQUEST_ID: &str = "01JD00000000000000000000R0";

#[test]
fn every_sequence_judges_as_the_vectors_say() {
    let v = corpus();
    for sequence in entries(&v, "sequences") {
        let name = sequence["name"].as_str().expect("name");
        let mut rows = Rows::default();
        let mut capabilities: Option<JobCapabilities> = None;
        for (index, step) in entries(sequence, "steps").iter().enumerate() {
            let at = format!("{name} step {index}");
            let caller: Caller = serde_json::from_value(step["caller"].clone()).expect("caller");
            let descriptor: Descriptor =
                serde_json::from_value(step["descriptor"].clone()).expect("descriptor");
            let envelope = json!({
                "version": CONTRACT_VERSION,
                "request_id": REQUEST_ID,
                "context_id": null,
                "payload": step["payload"],
            });
            let bytes = serde_json::to_vec(&envelope).expect("encode");
            let minted: Option<AttemptId> = step
                .get("minted_attempt_id")
                .map(|id| serde_json::from_value(id.clone()).expect("minted attempt id"));
            let ctx = Context {
                domain: "d",
                caller: &caller,
                now_ms: step["now_ms"].as_u64().expect("now_ms"),
                minted_attempt_id: minted.as_ref(),
                open_jobs: step.get("open_jobs").and_then(Value::as_u64).unwrap_or(0),
                limits: &LIMITS,
            };
            let command = match check(descriptor, &bytes, &LIMITS) {
                Ok(command) => command,
                Err(refusal) => {
                    assert_eq!(
                        refusal.code(),
                        step["outcome"],
                        "{at}: refused by the check"
                    );
                    continue;
                }
            };
            if let Payload::DeclareCapability(payload) = &command.payload {
                let row = declare(payload, capabilities.as_ref(), &ctx);
                assert_eq!(step["outcome"], "accepted", "{at}");
                if let Some(expect) = step.get("expect").and_then(|e| e.get("capabilities")) {
                    subset(&serde_json::to_value(&row).expect("row"), expect, &at);
                }
                capabilities = Some(row);
                continue;
            }
            let before = rows.clone();
            match judge(&command.payload, &rows, &ctx) {
                Ok(Judged { rows: after, code }) => {
                    let expected = code.map_or("accepted", Refusal::code);
                    assert_eq!(step["outcome"], expected, "{at}");
                    if let Some(expect) = step.get("expect") {
                        expect_rows(&after, expect, &at);
                    }
                    let moved = moves(&before, &after);
                    if let (Some(b), Some(a)) = (&before.job, &after.job) {
                        assert_eq!(moved, b.page_key != a.page_key, "{at}");
                    }
                    if let (Some(job), Some(claim)) = (&after.job, &after.claim) {
                        assert_eq!(job.page_key, claim.page_key, "{at}: the rows move together");
                    }
                    rows = after;
                }
                Err(refusal) => {
                    assert_eq!(refusal.code(), step["outcome"], "{at}");
                    assert!(
                        step.get("expect").is_none(),
                        "{at}: a refusal writes nothing"
                    );
                    // A refusal writes nothing: `rows` stays as it was.
                }
            }
            if let Some(held) = step.get("held_by").and_then(Value::as_object) {
                let page = rows.job.as_ref().map(|job| job.page_key.as_str());
                for (unit, expected) in held {
                    assert_eq!(
                        Value::Bool(page == Some(unit.as_str())),
                        *expected,
                        "{at}: held by {unit}"
                    );
                }
            }
        }
    }
}

fn expect_rows(after: &Rows, expect: &Value, at: &str) {
    if let Some(job) = expect.get("job") {
        let row = serde_json::to_value(after.job.as_ref().expect("a job row")).expect("job");
        subset(&row, job, at);
    }
    match expect.get("claim") {
        Some(Value::Null) => assert!(after.claim.is_none(), "{at}: no claim"),
        Some(claim) => {
            let row =
                serde_json::to_value(after.claim.as_ref().expect("a claim row")).expect("claim");
            subset(&row, claim, at);
        }
        None => {}
    }
}

fn subset(row: &Value, expect: &Value, at: &str) {
    for (key, value) in expect.as_object().expect("expect object") {
        assert_eq!(row.get(key), Some(value), "{at}: {key}");
    }
}

#[test]
fn the_corpus_covers_every_command_and_refusal() {
    let v = corpus();
    let mut seen_descriptors = std::collections::BTreeSet::new();
    let mut seen_codes = std::collections::BTreeSet::new();
    for entry in entries(&v, "checks") {
        seen_descriptors.insert(entry["descriptor"].as_str().expect("d").to_owned());
        seen_codes.insert(entry["outcome"].as_str().expect("o").to_owned());
    }
    for sequence in entries(&v, "sequences") {
        for step in entries(sequence, "steps") {
            seen_descriptors.insert(step["descriptor"].as_str().expect("d").to_owned());
            seen_codes.insert(step["outcome"].as_str().expect("o").to_owned());
        }
    }
    for d in Descriptor::ALL {
        assert!(seen_descriptors.contains(d.as_str()), "{}", d.as_str());
    }
    // `idempotency_mismatch` is the server's record, not a pure judgement.
    for refusal in Refusal::ALL {
        if refusal != Refusal::IdempotencyMismatch {
            assert!(seen_codes.contains(refusal.code()), "{}", refusal.code());
        }
    }
}
