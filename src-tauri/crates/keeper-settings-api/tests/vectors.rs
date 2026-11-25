//! `syncular/settings.vectors.json` holds for this crate, together with
//! `syncular/settings.travel.json` and `syncular/limits.json`. keeper-hq's
//! TypeScript reads the same files.

use std::collections::{BTreeMap, BTreeSet};

use keeper_settings_api::adoption::{check_adoption, Claim, DeviceRecord};
use keeper_settings_api::blob::{check_upload, read_value_blob, BlobAnswer, BlobReader};
use keeper_settings_api::config::{check_config_write, template_key, ConfigWriter, Role};
use keeper_settings_api::credential::{check_value, PATTERNS};
use keeper_settings_api::limits;
use keeper_settings_api::models::{
    canonical_text, check_model_upload, check_model_write, check_set, parse_key, read_config_blob,
    version_id, BlobRef, ConfigBlobReader, ModelFile, ModelWrite, VersionState,
};
use keeper_settings_api::record::{classify, parse_id, record_id, Device, OfferKind, RecordKey};
use keeper_settings_api::refusal::Refusal;
use keeper_settings_api::scope::{settings_owner_scope, CallerKind};
use keeper_settings_api::travel::{Travel, CREDENTIAL_CHOICES};
use keeper_settings_api::write::{
    check_device_write, check_value_write, Bound, DeviceRow, Domain, DomainKind, Op, ValueRow,
    Writer, WriterKind,
};
use serde_json::{json, Value};

const CORPUS: &str = include_str!("../../../../syncular/settings.vectors.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("vector JSON")
}

fn entries<'a>(corpus: &'a Value, key: &str) -> &'a [Value] {
    corpus[key].as_array().expect(key)
}

fn case_name(case: &Value) -> &str {
    case["name"].as_str().expect("case name")
}

fn parse_device(raw: &str) -> Device {
    if raw == "*" {
        Device::Person
    } else {
        Device::Device(raw.to_owned())
    }
}

fn device_str(device: &Device) -> &str {
    match device {
        Device::Person => "*",
        Device::Device(id) => id.as_str(),
    }
}

fn record_key_json(rk: &RecordKey) -> Value {
    match rk {
        RecordKey::Preference(key) => json!({
            "kind": "preference",
            "key": key,
        }),
        RecordKey::Pin(key) => json!({
            "kind": "pin",
            "key": key,
        }),
        RecordKey::Offer { kind, reference } => json!({
            "kind": "offer",
            "offer_kind": match kind {
                OfferKind::Drive => "drive",
                OfferKind::Provider => "provider",
                OfferKind::Matrix => "matrix",
            },
            "reference": reference,
        }),
        RecordKey::DriveRoles(reference) => json!({
            "kind": "drive_roles",
            "reference": reference,
        }),
    }
}

fn parse_writer_kind(raw: &str) -> WriterKind {
    match raw {
        "person" => WriterKind::Person,
        "agent" => WriterKind::Agent,
        "tool" => WriterKind::Tool,
        "service" => WriterKind::Service,
        other => panic!("unknown writer kind: {other}"),
    }
}

fn parse_caller_kind(raw: &str) -> CallerKind {
    match raw {
        "person" => CallerKind::Person,
        "agent" => CallerKind::Agent,
        "tool" => CallerKind::Tool,
        "service" => CallerKind::Service,
        other => panic!("unknown caller kind: {other}"),
    }
}

fn parse_domain_kind(raw: &str) -> DomainKind {
    match raw {
        "general" => DomainKind::General,
        "personal" => DomainKind::Personal,
        "group" => DomainKind::Group,
        other => panic!("unknown domain kind: {other}"),
    }
}

fn parse_op(raw: &str) -> Op {
    match raw {
        "upsert" => Op::Upsert,
        "delete" => Op::Delete,
        other => panic!("unknown op: {other}"),
    }
}

fn parse_role(raw: &str) -> Role {
    match raw {
        "reader" => Role::Reader,
        "editor" => Role::Editor,
        "reviewer" => Role::Reviewer,
        "owner" => Role::Owner,
        other => panic!("unknown role: {other}"),
    }
}

fn parse_writer(v: &Value) -> Writer {
    Writer {
        kind: parse_writer_kind(v["kind"].as_str().expect("writer.kind")),
        actor: v["actor"].as_str().expect("writer.actor").to_owned(),
        client: v["client"].as_str().expect("writer.client").to_owned(),
    }
}

fn parse_domain(v: &Value) -> Domain {
    Domain {
        kind: parse_domain_kind(v["kind"].as_str().expect("domain.kind")),
        owner_actor: v["owner_actor"].as_str().map(str::to_owned),
    }
}

fn parse_bound(v: &Value) -> Option<Bound> {
    if v.is_null() {
        return None;
    }
    Some(Bound {
        owner_actor: v["owner_actor"]
            .as_str()
            .expect("bound.owner_actor")
            .to_owned(),
        client_id: v["client_id"].as_str().expect("bound.client_id").to_owned(),
    })
}

fn parse_value_row(v: &Value) -> ValueRow {
    ValueRow {
        id: v["id"].as_str().expect("row.id").to_owned(),
        owner_actor: v["owner_actor"]
            .as_str()
            .expect("row.owner_actor")
            .to_owned(),
        device: v["device"].as_str().expect("row.device").to_owned(),
        key: v["key"].as_str().expect("row.key").to_owned(),
        value: v["value"].as_str().map(str::to_owned),
        value_blob: v["value_blob"].as_bool().expect("row.value_blob"),
    }
}

fn parse_device_row(v: &Value) -> DeviceRow {
    DeviceRow {
        device_id: v["device_id"].as_str().expect("row.device_id").to_owned(),
        owner_actor: v["owner_actor"]
            .as_str()
            .expect("row.owner_actor")
            .to_owned(),
        client_id: v["client_id"].as_str().expect("row.client_id").to_owned(),
        machine: v["machine"].as_str().map(str::to_owned),
    }
}

fn parse_device_record(v: &Value) -> DeviceRecord {
    DeviceRecord {
        owner_actor: v["owner_actor"]
            .as_str()
            .expect("record.owner_actor")
            .to_owned(),
        device_class: v["device_class"]
            .as_str()
            .expect("record.device_class")
            .to_owned(),
        platform: v["platform"].as_str().expect("record.platform").to_owned(),
        machine: v["machine"].as_str().map(str::to_owned),
        untouched_days: v["untouched_days"].as_u64().expect("record.untouched_days"),
    }
}

fn parse_claim(v: &Value) -> Claim {
    Claim {
        actor: v["actor"].as_str().expect("claim.actor").to_owned(),
        writer_kind: parse_writer_kind(v["writer_kind"].as_str().expect("claim.writer_kind")),
        device_class: v["device_class"]
            .as_str()
            .expect("claim.device_class")
            .to_owned(),
        platform: v["platform"].as_str().expect("claim.platform").to_owned(),
        machine: v["machine"].as_str().map(str::to_owned),
    }
}

fn parse_config_writer(v: &Value) -> ConfigWriter {
    match v["kind"].as_str().expect("writer.kind") {
        "admin" => ConfigWriter::Admin,
        "person" => ConfigWriter::Person {
            role: v.get("role").and_then(Value::as_str).map(parse_role),
        },
        "agent" => ConfigWriter::Agent,
        "tool" => ConfigWriter::Tool,
        "service" => ConfigWriter::Service,
        other => panic!("unknown config writer kind: {other}"),
    }
}

fn parse_blob_reader(v: &Value) -> BlobReader {
    BlobReader {
        kind: parse_caller_kind(v["kind"].as_str().expect("reader.kind")),
        actor: v["actor"].as_str().expect("reader.actor").to_owned(),
    }
}

fn blob_answer_str(answer: BlobAnswer) -> &'static str {
    match answer {
        BlobAnswer::Granted => "granted",
        BlobAnswer::Absent => "absent",
    }
}

fn collect_refusals(value: &Value, seen: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "refusal" {
                    if let Some(code) = v.as_str() {
                        seen.insert(code.to_owned());
                    }
                } else {
                    collect_refusals(v, seen);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refusals(item, seen);
            }
        }
        _ => {}
    }
}

#[test]
fn refusals() {
    let c = corpus();
    let expected: Vec<&str> = entries(&c, "refusals")
        .iter()
        .map(|v| v.as_str().expect("refusal string"))
        .collect();
    let actual: Vec<&str> = Refusal::ALL.iter().map(|r| r.code()).collect();
    assert_eq!(actual, expected);
    for refusal in Refusal::ALL {
        assert_eq!(
            serde_json::to_value(refusal).expect("serialize refusal"),
            refusal.code()
        );
    }
}

#[test]
fn credential_patterns() {
    let c = corpus();
    let expected: Vec<(&str, &str)> = entries(&c, "credentialPatterns")
        .iter()
        .map(|v| {
            (
                v["name"].as_str().expect("pattern name"),
                v["regex"].as_str().expect("pattern regex"),
            )
        })
        .collect();
    assert_eq!(PATTERNS, expected.as_slice());
}

#[test]
fn credential_choices() {
    let c = corpus();
    let expected: Vec<&str> = entries(&c, "credentialChoices")
        .iter()
        .map(|v| v.as_str().expect("credential choice"))
        .collect();
    assert_eq!(CREDENTIAL_CHOICES.as_slice(), expected.as_slice());
}

#[test]
fn limits_match_corpus() {
    let c = corpus();
    let lim = limits();
    assert_eq!(
        lim.setting_value_max_bytes,
        c["limits"]["settingValueMaxBytes"]
            .as_u64()
            .expect("settingValueMaxBytes")
    );
    assert_eq!(
        lim.attachment_max_bytes,
        c["limits"]["attachmentMaxBytes"]
            .as_u64()
            .expect("attachmentMaxBytes")
    );
    assert_eq!(
        lim.model_file_max_bytes,
        c["limits"]["modelFileMaxBytes"]
            .as_u64()
            .expect("modelFileMaxBytes")
    );
}

#[test]
fn ids() {
    let c = corpus();
    for case in entries(&c, "ids") {
        let name = case_name(case);
        let id = case["id"].as_str().expect("id");
        let expected_refusal = case["refusal"].as_str();
        let result = parse_id(id);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        if let Ok((device, key)) = result {
            assert_eq!(
                device_str(&device),
                case["device"].as_str().expect("expected device"),
                "case {name}"
            );
            assert_eq!(
                key,
                case["key"].as_str().expect("expected key"),
                "case {name}"
            );
            assert_eq!(record_id(&device, &key), id, "case {name}");
        }
    }
}

#[test]
fn keys() {
    let c = corpus();
    let travel = Travel::contract();
    for case in entries(&c, "keys") {
        let name = case_name(case);
        let device = parse_device(case["device"].as_str().expect("device"));
        let key = case["key"].as_str().expect("key");
        let expected_refusal = case["refusal"].as_str();
        let result = classify(&travel, &device, key);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        if let Ok(record_key) = result {
            assert_eq!(record_key_json(&record_key), case["record"], "case {name}");
        } else {
            assert!(case["record"].is_null(), "case {name}");
        }
    }
}

#[test]
fn credentials() {
    let c = corpus();
    for case in entries(&c, "credentials") {
        let name = case_name(case);
        let key = case["key"].as_str().expect("key");
        let value = case["value"].as_str().expect("value");
        let expected_refusal = case["refusal"].as_str();
        let result = check_value(key, value);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn value_writes() {
    let c = corpus();
    let travel = Travel::contract();
    let lim = limits();
    for case in entries(&c, "valueWrites") {
        let name = case_name(case);
        let writer = parse_writer(&case["writer"]);
        let domain = parse_domain(&case["domain"]);
        let op = parse_op(case["op"].as_str().expect("op"));
        let row = parse_value_row(&case["row"]);
        let device_record = parse_bound(&case["device_record"]);
        let expected_refusal = case["refusal"].as_str();
        let result = check_value_write(
            &travel,
            &lim,
            &writer,
            &domain,
            op,
            &row,
            device_record.as_ref(),
        );
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn device_writes() {
    let c = corpus();
    for case in entries(&c, "deviceWrites") {
        let name = case_name(case);
        let writer = parse_writer(&case["writer"]);
        let domain = parse_domain(&case["domain"]);
        let op = parse_op(case["op"].as_str().expect("op"));
        let row = parse_device_row(&case["row"]);
        let stored = parse_bound(&case["stored"]);
        let expected_refusal = case["refusal"].as_str();
        let result = check_device_write(&writer, &domain, op, &row, stored.as_ref());
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn adoptions() {
    let c = corpus();
    for case in entries(&c, "adoptions") {
        let name = case_name(case);
        let record = parse_device_record(&case["record"]);
        let claim = parse_claim(&case["claim"]);
        let expected_refusal = case["refusal"].as_str();
        let result = check_adoption(&record, &claim);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn scopes() {
    let c = corpus();
    for case in entries(&c, "scopes") {
        let name = case_name(case);
        let caller = parse_caller_kind(case["caller"].as_str().expect("caller"));
        let actor = case["actor"].as_str().expect("actor");
        let domain = parse_domain(&case["domain"]);
        let expected_refusal = case["refusal"].as_str();
        let result = settings_owner_scope(caller, actor, &domain);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        if let Ok(allowed) = result {
            let expected: Vec<&str> = case["allowed"]
                .as_array()
                .expect("allowed")
                .iter()
                .map(|v| v.as_str().expect("allowed entry"))
                .collect();
            assert_eq!(allowed, expected, "case {name}");
        }
    }
}

#[test]
fn config_writes() {
    let c = corpus();
    let lim = limits();
    for case in entries(&c, "configWrites") {
        let name = case_name(case);
        let writer = parse_config_writer(&case["writer"]);
        let domain_kind = parse_domain_kind(case["domain_kind"].as_str().expect("domain_kind"));
        let key = case["key"].as_str().expect("key");
        let value = case["value"].as_str().expect("value");
        let domains: Vec<&str> = case["domains"]
            .as_array()
            .expect("domains")
            .iter()
            .map(|v| v.as_str().expect("domain entry"))
            .collect();
        let expected_refusal = case["refusal"].as_str();
        let result = check_config_write(&writer, domain_kind, key, value, &domains, &lim);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn template_keys() {
    let c = corpus();
    for case in entries(&c, "templateKeys") {
        let name = case_name(case);
        let path = case["path"].as_str().expect("path");
        let expected_refusal = case["refusal"].as_str();
        let result = template_key(path);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        if let Ok(key) = result {
            assert_eq!(
                key,
                case["key"].as_str().expect("expected key"),
                "case {name}"
            );
        }
    }
}

#[test]
fn blob_reads() {
    let c = corpus();
    for case in entries(&c, "blobReads") {
        let name = case_name(case);
        let owner_actor = case["owner_actor"].as_str().expect("owner_actor");
        let reader = parse_blob_reader(&case["reader"]);
        let expected = case["answer"].as_str().expect("answer");
        let actual = blob_answer_str(read_value_blob(owner_actor, &reader));
        assert_eq!(actual, expected, "case {name}");
    }
}

#[test]
fn blob_uploads() {
    let c = corpus();
    let lim = limits();
    for case in entries(&c, "blobUploads") {
        let name = case_name(case);
        let size = case["size"].as_u64().expect("size");
        let expected_refusal = case["refusal"].as_str();
        let result = check_upload(size, &lim);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn every_refusal_is_exercised_in_corpus() {
    let c = corpus();
    let mut seen = BTreeSet::new();
    collect_refusals(&c, &mut seen);
    for refusal in Refusal::ALL {
        assert!(
            seen.contains(refusal.code()),
            "refusal {} never appears as an expected refusal in corpus",
            refusal.code()
        );
    }
}

#[test]
fn model_keys() {
    let c = corpus();
    for case in entries(&c, "modelKeys") {
        let name = case_name(case);
        let key = case["key"].as_str().expect("key");
        let expected_refusal = case["refusal"].as_str();
        let result = parse_key(key);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        let actual_parsed = result
            .ok()
            .map(|k| serde_json::to_value(k).expect("serialize ModelKey"));
        if case["parsed"].is_null() {
            assert_eq!(actual_parsed, None, "case {name}");
        } else {
            assert_eq!(actual_parsed.as_ref(), Some(&case["parsed"]), "case {name}");
        }
    }
}

#[test]
fn model_versions() {
    let c = corpus();
    let mut by_name: BTreeMap<&str, String> = BTreeMap::new();
    let mut distinct: BTreeSet<String> = BTreeSet::new();
    for case in entries(&c, "modelVersions") {
        let name = case_name(case);
        let files: Vec<ModelFile> =
            serde_json::from_value(case["files"].clone()).expect("model files");
        let models_toml = case["models_toml"].as_str().expect("models_toml");
        let expected_refusal = case["refusal"].as_str();

        let canonical = canonical_text(&files, models_toml);
        assert_eq!(
            canonical.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name} canonical"
        );
        let version = version_id(&files, models_toml);
        assert_eq!(
            version.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name} version"
        );

        if let (Ok(canon), Ok(ver)) = (canonical, version) {
            assert_eq!(
                canon,
                case["canonical"].as_str().expect("expected canonical"),
                "case {name} canonical text"
            );
            assert_eq!(
                ver,
                case["version"].as_str().expect("expected version"),
                "case {name} version id"
            );
            if let Some(other) = case["same_as"].as_str() {
                let other_id = by_name
                    .get(other)
                    .unwrap_or_else(|| panic!("case {name}: unknown same_as {other}"));
                assert_eq!(&ver, other_id, "case {name} same_as {other}");
            } else {
                assert!(
                    distinct.insert(ver.clone()),
                    "case {name}: duplicate version id {ver}"
                );
            }
            by_name.insert(name, ver);
        }
    }
}

#[test]
fn model_sets() {
    let c = corpus();
    let lim = limits();
    for case in entries(&c, "modelSets") {
        let name = case_name(case);
        let files: Vec<ModelFile> =
            serde_json::from_value(case["files"].clone()).expect("model files");
        let models_toml = case["models_toml"].as_str().expect("models_toml");
        let expected_refusal = case["refusal"].as_str();
        let result = check_set(&files, models_toml, &lim);
        assert_eq!(
            result.as_ref().err().map(|r| r.code()),
            expected_refusal,
            "case {name}"
        );
        if let Ok(ver) = result {
            assert_eq!(
                ver,
                case["version"].as_str().expect("expected version"),
                "case {name}"
            );
        }
    }
}

#[test]
fn model_uploads() {
    let c = corpus();
    let lim = limits();
    for case in entries(&c, "modelUploads") {
        let name = case_name(case);
        let writer = parse_config_writer(&case["writer"]);
        let domain_kind = parse_domain_kind(case["domain_kind"].as_str().expect("domain_kind"));
        let size = case["size"].as_u64().expect("size");
        let sha256 = case["sha256"].as_str().expect("sha256");
        let expected_refusal = case["refusal"].as_str();
        let result = check_model_upload(&writer, domain_kind, size, sha256, &lim);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn model_writes() {
    let c = corpus();
    let lim = limits();
    for case in entries(&c, "modelWrites") {
        let name = case_name(case);
        let writer = parse_config_writer(&case["writer"]);
        let domain_kind = parse_domain_kind(case["domain_kind"].as_str().expect("domain_kind"));
        let key = case["key"].as_str().expect("key");
        let blob: Option<BlobRef> = serde_json::from_value(case["blob"].clone()).expect("blob");
        let state: VersionState = serde_json::from_value(case["state"].clone()).expect("state");
        let domains: Vec<&str> = case["domains"]
            .as_array()
            .expect("domains")
            .iter()
            .map(|v| v.as_str().expect("domain entry"))
            .collect();
        let expected_refusal = case["refusal"].as_str();
        let write = ModelWrite {
            key,
            value: &case["value"],
            blob: blob.as_ref(),
        };
        let result = check_model_write(&writer, domain_kind, &write, &state, &domains, &lim);
        assert_eq!(
            result.err().map(Refusal::code),
            expected_refusal,
            "case {name}"
        );
    }
}

#[test]
fn config_blob_reads() {
    let c = corpus();
    for case in entries(&c, "configBlobReads") {
        let name = case_name(case);
        let reader_val = &case["reader"];
        let reader = ConfigBlobReader {
            kind: parse_caller_kind(reader_val["kind"].as_str().expect("reader.kind")),
            member: reader_val["member"].as_bool().expect("reader.member"),
        };
        let expected = case["answer"].as_str().expect("answer");
        let actual = blob_answer_str(read_config_blob(&reader));
        assert_eq!(actual, expected, "case {name}");
    }
}
