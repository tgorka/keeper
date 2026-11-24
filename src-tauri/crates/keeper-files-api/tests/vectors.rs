use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use keeper_accounts_api::grant::{FileItem, GrantMode};
use keeper_files_api::caller::{self, Caller, GrantHolder};
use keeper_files_api::ids;
use keeper_files_api::names::{self, FileMode};
use keeper_files_api::refusal::Code;
use keeper_files_api::rows::{Drive, Entry, Head, Kind, Lifecycle};
use keeper_files_api::rules::{self, Manifest, Server};
use keeper_files_api::wire::WireOp;
use serde::Deserialize;
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/files.vectors.json");

fn corpus() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(CORPUS).expect("valid files.vectors.json"))
}

fn section<'a>(name: &str) -> &'a [Value] {
    corpus()
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing array section '{name}' in files.vectors.json"))
}

fn subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(exp), Value::Object(act)) => exp
            .iter()
            .all(|(key, val)| act.get(key).is_some_and(|act_val| subset(val, act_val))),
        _ => expected == actual,
    }
}

fn move_value_keys_failure(op: &Value) -> Option<String> {
    if op.get("operation").and_then(Value::as_str) != Some("move") {
        return None;
    }
    let (Some(before), Some(after)) = (
        op.get("before_value").and_then(Value::as_object),
        op.get("after_value").and_then(Value::as_object),
    ) else {
        return Some(format!(
            "move op missing before_value/after_value object: {op}"
        ));
    };
    let before_keys: BTreeSet<&str> = before.keys().map(String::as_str).collect();
    let after_keys: BTreeSet<&str> = after.keys().map(String::as_str).collect();
    if before_keys != after_keys {
        return Some(format!(
            "move op key sets differ: before={before_keys:?}, after={after_keys:?}"
        ));
    }
    if before_keys == BTreeSet::from(["drive_key"]) {
        return None;
    }
    let mut expected = BTreeSet::from(["parent_id", "name", "name_fold"]);
    for opt in ["media_type", "drive_key"] {
        if before.contains_key(opt) {
            if before.get(opt) == after.get(opt) {
                return Some(format!(
                    "move op includes unchanged optional key '{opt}' (value={})",
                    before[opt]
                ));
            }
            expected.insert(opt);
        }
    }
    if before_keys != expected {
        return Some(format!(
            "move op value keys mismatch: expected {expected:?}, got {before_keys:?}"
        ));
    }
    None
}

fn assert_no_failures(section_name: &str, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{section_name}: {} failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[derive(Deserialize)]
struct FixtureHead {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    seq: String,
    event_id: String,
    #[serde(default)]
    digest: Option<String>,
    drives: Vec<Drive>,
}

#[derive(Deserialize)]
struct BulkSpec {
    parent_id: String,
    drive_key: String,
    count: usize,
    id_prefix: String,
    name_prefix: String,
    content_id: String,
    size: u64,
}

#[derive(Deserialize)]
struct FixtureSpec {
    domain: String,
    #[serde(default)]
    objects: Vec<String>,
    #[serde(default)]
    head: Option<FixtureHead>,
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    bulk: Vec<BulkSpec>,
}

fn build_fixture(name: &str) -> Manifest {
    let raw = corpus()
        .get("fixtures")
        .and_then(Value::as_object)
        .and_then(|map| map.get(name))
        .unwrap_or_else(|| panic!("unknown fixture '{name}'"));
    let spec: FixtureSpec = serde_json::from_value(raw.clone())
        .unwrap_or_else(|err| panic!("fixture '{name}' parse failed: {err}"));

    let head = spec.head.map(|h| Head {
        id: h.id.unwrap_or_else(|| spec.domain.clone()),
        domain: h.domain.unwrap_or_else(|| spec.domain.clone()),
        seq: h.seq,
        event_id: h.event_id,
        digest: h.digest.unwrap_or_default(),
        drives: h.drives,
    });

    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    for entry in spec.entries {
        let prev = entries.insert(entry.id.clone(), entry);
        assert!(prev.is_none(), "fixture '{name}' has duplicate entry id");
    }

    for bulk in spec.bulk {
        let head_ref = head
            .as_ref()
            .unwrap_or_else(|| panic!("fixture '{name}' uses bulk without head"));
        let pad = 26usize
            .checked_sub(bulk.id_prefix.len())
            .unwrap_or_else(|| panic!("fixture '{name}' bulk id_prefix exceeds 26 chars"));
        for n in 1..=bulk.count {
            let id = format!("{}{n:0pad$}", bulk.id_prefix);
            assert_eq!(
                id.len(),
                26,
                "fixture '{name}' bulk id '{id}' is not 26 chars"
            );
            let entry_name = format!("{}{n}.md", bulk.name_prefix);
            let entry = Entry {
                id: id.clone(),
                domain: spec.domain.clone(),
                drive_key: bulk.drive_key.clone(),
                parent_id: bulk.parent_id.clone(),
                name_fold: entry_name.to_lowercase(),
                name: entry_name,
                kind: Kind::File,
                content_id: Some(bulk.content_id.clone()),
                size: Some(bulk.size),
                media_type: Some("text/markdown".to_owned()),
                mtime_ms: None,
                mode: FileMode::Normal,
                revision: "1".to_owned(),
                lifecycle: Lifecycle::Live,
                created_event_id: head_ref.event_id.clone(),
                updated_event_id: head_ref.event_id.clone(),
                updated_seq: head_ref.seq.clone(),
            };
            let prev = entries.insert(id, entry);
            assert!(prev.is_none(), "fixture '{name}' bulk collides on entry id");
        }
    }

    Manifest {
        domain: spec.domain,
        entries,
        head,
        events: BTreeMap::new(),
        objects: spec.objects.into_iter().collect(),
        bindings: BTreeMap::new(),
    }
}

#[derive(Deserialize)]
struct StepSpec {
    caller: Caller,
    descriptor: String,
    params: Value,
    #[serde(default)]
    server: Option<Server>,
    expect: Value,
}

#[test]
fn names() {
    let mut failures = Vec::new();
    for item in section("names") {
        let id = item["id"].as_str().expect("names[].id");
        let name = item["name"].as_str().expect("names[].name");
        let admitted = item["admitted"].as_bool().expect("names[].admitted");
        match names::admit_name(name) {
            Ok(()) => {
                if !admitted {
                    failures.push(format!("{id}: expected refusal for {name:?}, got Ok"));
                    continue;
                }
                if let Some(exp_fold) = item.get("fold").and_then(Value::as_str) {
                    let got_fold = names::fold(name);
                    if got_fold != exp_fold {
                        failures.push(format!(
                            "{id}: fold({name:?}) expected {exp_fold:?}, got {got_fold:?}"
                        ));
                    }
                }
            }
            Err(refusal) => {
                if admitted {
                    failures.push(format!("{id}: expected Ok for {name:?}, got {refusal:?}"));
                    continue;
                }
                if let Some(exp_code_val) = item.get("code") {
                    let exp_code: Code = serde_json::from_value(exp_code_val.clone())
                        .unwrap_or_else(|err| panic!("{id}: invalid code: {err}"));
                    if refusal.code != exp_code {
                        failures.push(format!(
                            "{id}: expected code {exp_code:?}, got {:?}",
                            refusal.code
                        ));
                    }
                }
            }
        }
    }
    assert_no_failures("names", &failures);
}

#[test]
fn folds() {
    let mut failures = Vec::new();
    for item in section("folds") {
        let id = item["id"].as_str().expect("folds[].id");
        let a = item["a"].as_str().expect("folds[].a");
        let b = item["b"].as_str().expect("folds[].b");
        let same = item["same"].as_bool().expect("folds[].same");
        let got = names::fold(a) == names::fold(b);
        if got != same {
            failures.push(format!(
                "{id}: fold({a:?})==fold({b:?}) expected {same}, got {got}"
            ));
        }
    }
    assert_no_failures("folds", &failures);
}

#[test]
fn media_types() {
    let mut failures = Vec::new();
    for item in section("media_types") {
        let id = item["id"].as_str().expect("media_types[].id");
        let name = item["name"].as_str().expect("media_types[].name");
        let expected = item["media_type"]
            .as_str()
            .expect("media_types[].media_type");
        let got = names::media_type(name);
        if got != expected {
            failures.push(format!(
                "{id}: media_type({name:?}) expected {expected:?}, got {got:?}"
            ));
        }
    }
    assert_no_failures("media_types", &failures);
}

#[test]
fn renames() {
    let mut failures = Vec::new();
    for item in section("renames") {
        let id = item["id"].as_str().expect("renames[].id");
        let from = item["from"].as_str().expect("renames[].from");
        let to = item["to"].as_str().expect("renames[].to");
        let before = item["media_type_before"]
            .as_str()
            .expect("renames[].media_type_before");
        let after = item["media_type_after"]
            .as_str()
            .expect("renames[].media_type_after");
        let got_before = names::media_type(from);
        let got_after = names::media_type(to);
        if got_before != before {
            failures.push(format!(
                "{id}: media_type({from:?}) expected {before:?}, got {got_before:?}"
            ));
        }
        if got_after != after {
            failures.push(format!(
                "{id}: media_type({to:?}) expected {after:?}, got {got_after:?}"
            ));
        }
    }
    assert_no_failures("renames", &failures);
}

#[test]
fn git_modes() {
    let mut failures = Vec::new();
    for item in section("git_modes") {
        let id = item["id"].as_str().expect("git_modes[].id");
        let git_mode = item["git_mode"].as_str().expect("git_modes[].git_mode");
        let result = FileMode::from_git(git_mode);
        if let Some(exp_mode_val) = item.get("mode") {
            let exp_mode: FileMode = serde_json::from_value(exp_mode_val.clone())
                .unwrap_or_else(|err| panic!("{id}: invalid mode: {err}"));
            if result != Ok(exp_mode) {
                failures.push(format!(
                    "{id}: from_git({git_mode:?}) expected Ok({exp_mode:?}), got {result:?}"
                ));
            }
        } else if let Some(exp_code_val) = item.get("code") {
            let exp_code: Code = serde_json::from_value(exp_code_val.clone())
                .unwrap_or_else(|err| panic!("{id}: invalid code: {err}"));
            match result {
                Err(refusal) if refusal.code == exp_code => {}
                other => failures.push(format!(
                    "{id}: from_git({git_mode:?}) expected Err({exp_code:?}), got {other:?}"
                )),
            }
        } else {
            failures.push(format!("{id}: git_modes item has neither mode nor code"));
        }
    }
    assert_no_failures("git_modes", &failures);
}

#[test]
fn content_ids() {
    let mut failures = Vec::new();
    for item in section("content_ids") {
        let id = item["id"].as_str().expect("content_ids[].id");
        let text = item["text"].as_str().expect("content_ids[].text");
        let valid = item["valid"].as_bool().expect("content_ids[].valid");
        let got = ids::is_content_id(text);
        if got != valid {
            failures.push(format!(
                "{id}: is_content_id({text:?}) expected {valid}, got {got}"
            ));
        }
    }
    assert_no_failures("content_ids", &failures);
}

#[test]
fn page_keys() {
    let mut failures = Vec::new();
    for item in section("page_keys") {
        let id = item["id"].as_str().expect("page_keys[].id");
        let domain = item["domain"].as_str().expect("page_keys[].domain");
        let recorded_at_ms = item["recorded_at_ms"]
            .as_i64()
            .expect("page_keys[].recorded_at_ms");
        let import = item["import"].as_bool().expect("page_keys[].import");
        let expected = item["page_key"].as_str().expect("page_keys[].page_key");
        let got = ids::page_key(domain, recorded_at_ms, import);
        if got != expected {
            failures.push(format!("{id}: page_key expected {expected:?}, got {got:?}"));
        }
    }
    assert_no_failures("page_keys", &failures);
}

#[test]
fn locators() {
    let mut failures = Vec::new();
    for item in section("locators") {
        let id = item["id"].as_str().expect("locators[].id");
        let domain = item["domain"].as_str().expect("locators[].domain");
        let entry_id = item["entry_id"].as_str().expect("locators[].entry_id");
        let expected = item["locator"].as_str().expect("locators[].locator");
        let got = ids::locator(domain, entry_id);
        if got != expected {
            failures.push(format!("{id}: locator expected {expected:?}, got {got:?}"));
        }
    }
    assert_no_failures("locators", &failures);
}

#[test]
fn default_modes() {
    let mut failures = Vec::new();
    for item in section("default_modes") {
        let id = item["id"].as_str().expect("default_modes[].id");
        let holder: GrantHolder = serde_json::from_value(item["holder"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid holder: {err}"));
        let expected: GrantMode = serde_json::from_value(item["mode"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid mode: {err}"));
        let got = caller::default_mode(holder);
        if got != expected {
            failures.push(format!(
                "{id}: default_mode({holder:?}) expected {expected:?}, got {got:?}"
            ));
        }
    }
    assert_no_failures("default_modes", &failures);
}

#[test]
fn modes() {
    let mut failures = Vec::new();
    for item in section("modes") {
        let id = item["id"].as_str().expect("modes[].id");
        let word = item["word"].as_str().expect("modes[].word");
        let result = caller::parse_mode(word);
        if let Some(exp_mode_val) = item.get("mode") {
            let exp_mode: GrantMode = serde_json::from_value(exp_mode_val.clone())
                .unwrap_or_else(|err| panic!("{id}: invalid mode: {err}"));
            if result != Ok(exp_mode) {
                failures.push(format!(
                    "{id}: parse_mode({word:?}) expected Ok({exp_mode:?}), got {result:?}"
                ));
            }
        } else if let Some(exp_code_val) = item.get("code") {
            let exp_code: Code = serde_json::from_value(exp_code_val.clone())
                .unwrap_or_else(|err| panic!("{id}: invalid code: {err}"));
            match result {
                Err(refusal) if refusal.code == exp_code => {}
                other => failures.push(format!(
                    "{id}: parse_mode({word:?}) expected Err({exp_code:?}), got {other:?}"
                )),
            }
        } else {
            failures.push(format!("{id}: modes item has neither mode nor code"));
        }
    }
    assert_no_failures("modes", &failures);
}

#[test]
fn event_rows() {
    let mut failures = Vec::new();
    for item in section("event_rows") {
        let id = item["id"].as_str().expect("event_rows[].id");
        let valid = item["valid"].as_bool().expect("event_rows[].valid");
        let result = rules::check_event(&item["event"], &item["ops"]);
        match result {
            Ok(()) => {
                if !valid {
                    failures.push(format!("{id}: expected invalid event row, got Ok"));
                }
            }
            Err(refusal) => {
                if valid {
                    failures.push(format!("{id}: expected valid event row, got {refusal:?}"));
                    continue;
                }
                if let Some(exp_code_val) = item.get("code") {
                    let exp_code: Code = serde_json::from_value(exp_code_val.clone())
                        .unwrap_or_else(|err| panic!("{id}: invalid code: {err}"));
                    if refusal.code != exp_code {
                        failures.push(format!(
                            "{id}: expected code {exp_code:?}, got {:?}",
                            refusal.code
                        ));
                    }
                }
            }
        }
    }
    assert_no_failures("event_rows", &failures);
}

#[test]
fn facts_reserved() {
    let mut failures = Vec::new();
    for item in section("facts_reserved") {
        let id = item["id"].as_str().expect("facts_reserved[].id");
        let content_id = item["content_id"]
            .as_str()
            .expect("facts_reserved[].content_id");
        let extractor = item["extractor"]
            .as_str()
            .expect("facts_reserved[].extractor");
        let extractor_version = item["extractor_version"]
            .as_str()
            .expect("facts_reserved[].extractor_version");
        let expected_id = item["facts_id"]
            .as_str()
            .expect("facts_reserved[].facts_id");
        let row_carries_trust = item["row_carries_trust"]
            .as_bool()
            .expect("facts_reserved[].row_carries_trust");
        let entries = item["entries"]
            .as_array()
            .expect("facts_reserved[].entries");

        if row_carries_trust {
            failures.push(format!("{id}: row_carries_trust must be false"));
        }
        if entries.len() < 2 {
            failures.push(format!(
                "{id}: expected at least 2 entries sharing one facts_id, got {}",
                entries.len()
            ));
        }

        let ids_for_entries: BTreeSet<String> = entries
            .iter()
            .map(|_| ids::facts_id(content_id, extractor, extractor_version))
            .collect();
        if ids_for_entries.len() != 1 || !ids_for_entries.contains(expected_id) {
            failures.push(format!(
                "{id}: expected single facts_id {expected_id:?}, got {ids_for_entries:?}"
            ));
        }
    }
    assert_no_failures("facts_reserved", &failures);
}

#[test]
fn inside() {
    let mut failures = Vec::new();
    for item in section("inside") {
        let id = item["id"].as_str().expect("inside[].id");
        let fixture_name = item["fixture"].as_str().expect("inside[].fixture");
        let entry_id = item["entry_id"].as_str().expect("inside[].entry_id");
        let root_id = item["root_id"].as_str().expect("inside[].root_id");
        let expected = item["inside"].as_bool().expect("inside[].inside");

        let mut manifest = build_fixture(fixture_name);
        let mut step_failed = false;
        if let Some(steps) = item.get("after_steps").and_then(Value::as_array) {
            for (idx, step_val) in steps.iter().enumerate() {
                let caller: Caller = serde_json::from_value(step_val["caller"].clone())
                    .unwrap_or_else(|err| panic!("{id} after_step {idx} caller: {err}"));
                let descriptor = step_val["descriptor"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{id} after_step {idx} descriptor"));
                let server: Server = serde_json::from_value(step_val["server"].clone())
                    .unwrap_or_else(|err| panic!("{id} after_step {idx} server: {err}"));
                if let Err(refusal) = rules::evaluate(
                    &mut manifest,
                    &caller,
                    descriptor,
                    &step_val["params"],
                    &server,
                ) {
                    failures.push(format!(
                        "{id}: after_step {idx} must be accepted, got {refusal:?}"
                    ));
                    step_failed = true;
                    break;
                }
            }
        }
        if step_failed {
            continue;
        }

        let got = rules::is_inside(&manifest, entry_id, root_id);
        if got != expected {
            failures.push(format!(
                "{id}: is_inside({entry_id}, {root_id}) expected {expected}, got {got}"
            ));
        }
    }
    assert_no_failures("inside", &failures);
}

#[test]
fn narrower() {
    let mut failures = Vec::new();
    for item in section("narrower") {
        let id = item["id"].as_str().expect("narrower[].id");
        let fixture_name = item["fixture"].as_str().expect("narrower[].fixture");
        let held_domain = item["held_domain"]
            .as_str()
            .expect("narrower[].held_domain");
        let given_domain = item["given_domain"]
            .as_str()
            .expect("narrower[].given_domain");
        let held: Vec<FileItem> = serde_json::from_value(item["held"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid held items: {err}"));
        let given: Vec<FileItem> = serde_json::from_value(item["given"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid given items: {err}"));

        let manifest = build_fixture(fixture_name);
        let result = rules::narrower_or_equal(&manifest, given_domain, &given, held_domain, &held);

        if let Some(expected) = item.get("narrower_or_equal").and_then(Value::as_bool) {
            if result != Ok(expected) {
                failures.push(format!(
                    "{id}: narrower_or_equal expected Ok({expected}), got {result:?}"
                ));
            }
        } else if let Some(exp_code_val) = item.get("code") {
            let exp_code: Code = serde_json::from_value(exp_code_val.clone())
                .unwrap_or_else(|err| panic!("{id}: invalid code: {err}"));
            match result {
                Err(refusal) if refusal.code == exp_code => {}
                other => failures.push(format!(
                    "{id}: narrower_or_equal expected Err({exp_code:?}), got {other:?}"
                )),
            }
        } else {
            failures.push(format!(
                "{id}: narrower item has neither narrower_or_equal nor code"
            ));
        }
    }
    assert_no_failures("narrower", &failures);
}

fn run_steps(case_id: &str, manifest: &mut Manifest, steps: &[Value], failures: &mut Vec<String>) {
    for (step_idx, step_val) in steps.iter().enumerate() {
        let step: StepSpec = serde_json::from_value(step_val.clone())
            .unwrap_or_else(|err| panic!("{case_id} step {step_idx}: parse error: {err}"));
        let before_entries = manifest.entries.clone();
        let before_head = manifest.head.clone();

        if step.descriptor == "files.preview_reversal" {
            let result = rules::preview_reversal(manifest, &step.caller, &step.params);
            if let Some(exp_preview) = step.expect.get("preview") {
                match result {
                    Ok(preview) => {
                        if let Some(exp_plan_val) = exp_preview.get("plan") {
                            let exp_plan: Vec<WireOp> = serde_json::from_value(
                                exp_plan_val.clone(),
                            )
                            .unwrap_or_else(|err| {
                                panic!(
                                    "{case_id} step {step_idx}: invalid expect.preview.plan: {err}"
                                )
                            });
                            if preview.plan != exp_plan {
                                failures.push(format!(
                                    "{case_id} step {step_idx}: preview.plan mismatch:\n  expected: {exp_plan:?}\n  actual: {:?}",
                                    preview.plan
                                ));
                            }
                        }
                        let mut rest = exp_preview
                            .as_object()
                            .unwrap_or_else(|| {
                                panic!(
                                    "{case_id} step {step_idx}: expect.preview must be an object"
                                )
                            })
                            .clone();
                        rest.remove("plan");
                        if !rest.is_empty() {
                            let act_preview =
                                serde_json::to_value(&preview).expect("serialize Preview");
                            let exp_rest = Value::Object(rest);
                            if !subset(&exp_rest, &act_preview) {
                                failures.push(format!(
                                    "{case_id} step {step_idx}: preview mismatch:\n  expected subset: {exp_rest}\n  actual: {act_preview}"
                                ));
                            }
                        }
                    }
                    Err(refusal) => failures.push(format!(
                        "{case_id} step {step_idx}: expected preview, got {refusal:?}"
                    )),
                }
            } else if step.expect.get("outcome").and_then(Value::as_str) == Some("rejected")
                || step.expect.get("code").is_some()
            {
                let exp_code: Code = serde_json::from_value(step.expect["code"].clone())
                    .unwrap_or_else(|err| {
                        panic!("{case_id} step {step_idx}: invalid expect.code: {err}")
                    });
                match result {
                    Ok(preview) => failures.push(format!(
                        "{case_id} step {step_idx}: expected preview refusal {exp_code:?}, got {preview:?}"
                    )),
                    Err(refusal) => {
                        if refusal.code != exp_code {
                            failures.push(format!(
                                "{case_id} step {step_idx}: expected code {exp_code:?}, got {:?} (detail: {:?})",
                                refusal.code, refusal.detail
                            ));
                        }
                        if let Some(exp_detail) =
                            step.expect.get("detail").filter(|d| !d.is_null())
                        {
                            match &refusal.detail {
                                Some(act_detail) if subset(exp_detail, act_detail) => {}
                                other => failures.push(format!(
                                    "{case_id} step {step_idx}: detail mismatch: expected subset {exp_detail}, got {other:?}"
                                )),
                            }
                        }
                    }
                }
            } else {
                failures.push(format!(
                    "{case_id} step {step_idx}: files.preview_reversal expect missing 'preview' or 'code'"
                ));
            }
            if manifest.entries != before_entries {
                failures.push(format!(
                    "{case_id} step {step_idx}: preview step mutated manifest entries"
                ));
            }
            if manifest.head != before_head {
                failures.push(format!(
                    "{case_id} step {step_idx}: preview step mutated manifest head"
                ));
            }
            continue;
        }

        let server = step
            .server
            .as_ref()
            .unwrap_or_else(|| panic!("{case_id} step {step_idx}: missing server"));
        let result = rules::evaluate(
            manifest,
            &step.caller,
            &step.descriptor,
            &step.params,
            server,
        );
        let outcome = step.expect["outcome"]
            .as_str()
            .unwrap_or_else(|| panic!("{case_id} step {step_idx}: missing expect.outcome"));

        match outcome {
            "rejected" => {
                let exp_code: Code = serde_json::from_value(step.expect["code"].clone())
                    .unwrap_or_else(|err| {
                        panic!("{case_id} step {step_idx}: invalid expect.code: {err}")
                    });
                match result {
                    Ok(accepted) => failures.push(format!(
                        "{case_id} step {step_idx}: expected rejection {exp_code:?}, got accepted event {}",
                        accepted.event.id
                    )),
                    Err(refusal) => {
                        if refusal.code != exp_code {
                            failures.push(format!(
                                "{case_id} step {step_idx}: expected code {exp_code:?}, got {:?} (detail: {:?})",
                                refusal.code, refusal.detail
                            ));
                        }
                        if let Some(exp_detail) =
                            step.expect.get("detail").filter(|d| !d.is_null())
                        {
                            match &refusal.detail {
                                Some(act_detail) if subset(exp_detail, act_detail) => {}
                                other => failures.push(format!(
                                    "{case_id} step {step_idx}: detail mismatch: expected subset {exp_detail}, got {other:?}"
                                )),
                            }
                        }
                    }
                }
                if manifest.entries != before_entries {
                    failures.push(format!(
                        "{case_id} step {step_idx}: rejected step mutated manifest entries"
                    ));
                }
                if manifest.head != before_head {
                    failures.push(format!(
                        "{case_id} step {step_idx}: rejected step mutated manifest head"
                    ));
                }
            }
            "accepted" => {
                let accepted = match result {
                    Ok(accepted) => accepted,
                    Err(refusal) => {
                        failures.push(format!(
                            "{case_id} step {step_idx}: expected accepted, got {refusal:?}"
                        ));
                        break;
                    }
                };

                if let Some(exp_event) = step.expect.get("event") {
                    let act_event = serde_json::to_value(&accepted.event).expect("serialize Event");
                    if !subset(exp_event, &act_event) {
                        failures.push(format!(
                            "{case_id} step {step_idx}: event mismatch:\n  expected subset: {exp_event}\n  actual: {act_event}"
                        ));
                    }
                }

                if let Some(exp_count) = step.expect.get("op_count").and_then(Value::as_u64) {
                    let exp_count = usize::try_from(exp_count).expect("op_count fits usize");
                    if accepted.ops.len() != exp_count {
                        failures.push(format!(
                            "{case_id} step {step_idx}: op_count expected {exp_count}, got ops.len()={}",
                            accepted.ops.len()
                        ));
                    }
                    if accepted.event.op_count as usize != exp_count {
                        failures.push(format!(
                            "{case_id} step {step_idx}: event.op_count expected {exp_count}, got {}",
                            accepted.event.op_count
                        ));
                    }
                }

                for (op_idx, op) in accepted.ops.iter().enumerate() {
                    let act_op = serde_json::to_value(op).expect("serialize Op");
                    if let Some(msg) = move_value_keys_failure(&act_op) {
                        failures.push(format!("{case_id} step {step_idx} op {op_idx}: {msg}"));
                    }
                }

                if let Some(exp_ops) = step.expect.get("ops").and_then(Value::as_array) {
                    if accepted.ops.len() < exp_ops.len() {
                        failures.push(format!(
                            "{case_id} step {step_idx}: expected at least {} ops, got {}",
                            exp_ops.len(),
                            accepted.ops.len()
                        ));
                    } else {
                        for (op_idx, exp_op) in exp_ops.iter().enumerate() {
                            let act_op =
                                serde_json::to_value(&accepted.ops[op_idx]).expect("serialize Op");
                            if !subset(exp_op, &act_op) {
                                failures.push(format!(
                                    "{case_id} step {step_idx} op {op_idx}: mismatch:\n  expected subset: {exp_op}\n  actual: {act_op}"
                                ));
                            }
                        }
                    }
                }

                if let Some(exp_entries) = step.expect.get("entries").and_then(Value::as_object) {
                    for (entry_id, exp_entry) in exp_entries {
                        if exp_entry.is_null() {
                            if let Some(act_entry) = manifest.entries.get(entry_id) {
                                failures.push(format!(
                                    "{case_id} step {step_idx}: expected entry {entry_id} absent, got {act_entry:?}"
                                ));
                            }
                            continue;
                        }
                        match manifest.entries.get(entry_id) {
                            Some(act_entry) => {
                                let act_val =
                                    serde_json::to_value(act_entry).expect("serialize Entry");
                                if !subset(exp_entry, &act_val) {
                                    failures.push(format!(
                                        "{case_id} step {step_idx}: entry {entry_id} mismatch:\n  expected subset: {exp_entry}\n  actual: {act_val}"
                                    ));
                                }
                            }
                            None => failures.push(format!(
                                "{case_id} step {step_idx}: expected entry {entry_id} missing"
                            )),
                        }
                    }
                }

                if let Some(unchanged) = step.expect.get("unchanged").and_then(Value::as_array) {
                    for id_val in unchanged {
                        let entry_id = id_val.as_str().expect("unchanged entry id string");
                        if manifest.entries.get(entry_id) != before_entries.get(entry_id) {
                            failures.push(format!(
                                "{case_id} step {step_idx}: entry {entry_id} changed when expected unchanged"
                            ));
                        }
                    }
                }

                if let Some(exp_head) = step.expect.get("head") {
                    match &manifest.head {
                        Some(act_head) => {
                            let act_val = serde_json::to_value(act_head).expect("serialize Head");
                            if !subset(exp_head, &act_val) {
                                failures.push(format!(
                                    "{case_id} step {step_idx}: head mismatch:\n  expected subset: {exp_head}\n  actual: {act_val}"
                                ));
                            }
                        }
                        None if exp_head.is_null() => {}
                        None => failures.push(format!(
                            "{case_id} step {step_idx}: expected head {exp_head}, got None"
                        )),
                    }
                }

                if let Some(listings) = step.expect.get("listing").and_then(Value::as_object) {
                    for (parent_id, exp_names_val) in listings {
                        let exp_names: Vec<String> = serde_json::from_value(exp_names_val.clone())
                            .expect("listing names array");
                        let got_names: Vec<String> = rules::list(manifest, parent_id)
                            .into_iter()
                            .map(|entry| entry.name.clone())
                            .collect();
                        if got_names != exp_names {
                            failures.push(format!(
                                "{case_id} step {step_idx}: listing[{parent_id}] expected {exp_names:?}, got {got_names:?}"
                            ));
                        }
                    }
                }

                if let Some(paths) = step.expect.get("paths").and_then(Value::as_object) {
                    for (entry_id, exp_path_val) in paths {
                        let got = rules::path_of(manifest, entry_id);
                        let expected = exp_path_val.as_str();
                        if got.as_deref() != expected {
                            failures.push(format!(
                                "{case_id} step {step_idx}: path_of({entry_id}) expected {expected:?}, got {got:?}"
                            ));
                        }
                    }
                }
            }
            other => failures.push(format!(
                "{case_id} step {step_idx}: unknown expect.outcome {other:?}"
            )),
        }
    }
}

fn run_case_section(name: &str) {
    let mut failures = Vec::new();
    for case in section(name) {
        let case_id = case["id"].as_str().unwrap_or_else(|| panic!("{name}[].id"));
        let fixture_name = case["fixture"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}[].fixture"));
        let steps = case["steps"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}[].steps"));
        let mut manifest = build_fixture(fixture_name);
        run_steps(case_id, &mut manifest, steps, &mut failures);
    }
    assert_no_failures(name, &failures);
}

#[test]
fn cases() {
    run_case_section("cases");
}

#[test]
fn reversals() {
    run_case_section("reversals");
}

#[test]
fn inverses() {
    let mut failures = Vec::new();
    for case in section("inverses") {
        let case_id = case["id"].as_str().expect("inverses[].id");
        let fixture_name = case["fixture"].as_str().expect("inverses[].fixture");
        let steps = case["steps"].as_array().expect("inverses[].steps");
        let target_event_id = case["target_event_id"]
            .as_str()
            .expect("inverses[].target_event_id");
        let expect = &case["expect"];
        let mut manifest = build_fixture(fixture_name);

        let before_failures = failures.len();
        run_steps(case_id, &mut manifest, steps, &mut failures);
        if failures.len() != before_failures {
            continue;
        }

        let result = rules::inverse(&manifest, target_event_id);
        if let Some(exp_plan_val) = expect.get("plan") {
            let exp_plan: Vec<WireOp> = serde_json::from_value(exp_plan_val.clone())
                .unwrap_or_else(|err| panic!("{case_id}: invalid expect.plan: {err}"));
            match result {
                Ok(act_plan) => {
                    if act_plan != exp_plan {
                        failures.push(format!(
                            "{case_id}: inverse plan mismatch:\n  expected: {exp_plan:?}\n  actual: {act_plan:?}"
                        ));
                    }
                }
                Err(refusal) => {
                    failures.push(format!("{case_id}: expected inverse plan, got {refusal:?}"))
                }
            }
        } else {
            let exp_code: Code = serde_json::from_value(expect["code"].clone())
                .unwrap_or_else(|err| panic!("{case_id}: invalid expect.code: {err}"));
            match result {
                Ok(act_plan) => failures.push(format!(
                    "{case_id}: expected inverse refusal {exp_code:?}, got plan {act_plan:?}"
                )),
                Err(refusal) => {
                    if refusal.code != exp_code {
                        failures.push(format!(
                            "{case_id}: expected code {exp_code:?}, got {:?} (detail: {:?})",
                            refusal.code, refusal.detail
                        ));
                    }
                    if let Some(exp_detail) = expect.get("detail").filter(|d| !d.is_null()) {
                        match &refusal.detail {
                            Some(act_detail) if subset(exp_detail, act_detail) => {}
                            other => failures.push(format!(
                                "{case_id}: detail mismatch: expected subset {exp_detail}, got {other:?}"
                            )),
                        }
                    }
                }
            }
        }
    }
    assert_no_failures("inverses", &failures);
}

#[test]
fn history() {
    let mut failures = Vec::new();
    for case in section("history") {
        let case_id = case["id"].as_str().expect("history[].id");
        let fixture_name = case["fixture"].as_str().expect("history[].fixture");
        let steps = case["steps"].as_array().expect("history[].steps");
        let queries = case["queries"].as_array().expect("history[].queries");
        let mut manifest = build_fixture(fixture_name);

        let before_failures = failures.len();
        run_steps(case_id, &mut manifest, steps, &mut failures);
        if failures.len() != before_failures {
            continue;
        }

        for (query_idx, query) in queries.iter().enumerate() {
            let params = &query["params"];
            let expect = &query["expect"];
            let result = rules::history(&manifest, params);

            if let Some(exp_code_val) = expect.get("code").filter(|c| !c.is_null()) {
                let exp_code: Code =
                    serde_json::from_value(exp_code_val.clone()).unwrap_or_else(|err| {
                        panic!("{case_id} query {query_idx}: invalid expect.code: {err}")
                    });
                match result {
                    Ok(page) => failures.push(format!(
                        "{case_id} query {query_idx}: expected refusal {exp_code:?}, got {} event(s)",
                        page.events.len()
                    )),
                    Err(refusal) => {
                        if refusal.code != exp_code {
                            failures.push(format!(
                                "{case_id} query {query_idx}: expected code {exp_code:?}, got {:?} (detail: {:?})",
                                refusal.code, refusal.detail
                            ));
                        }
                        if let Some(exp_detail) = expect.get("detail").filter(|d| !d.is_null()) {
                            match &refusal.detail {
                                Some(act_detail) if subset(exp_detail, act_detail) => {}
                                other => failures.push(format!(
                                    "{case_id} query {query_idx}: detail mismatch: expected subset {exp_detail}, got {other:?}"
                                )),
                            }
                        }
                    }
                }
                continue;
            }

            let page = match result {
                Ok(page) => page,
                Err(refusal) => {
                    failures.push(format!(
                        "{case_id} query {query_idx}: expected history page, got {refusal:?}"
                    ));
                    continue;
                }
            };

            let exp_event_ids: Vec<String> = serde_json::from_value(expect["event_ids"].clone())
                .unwrap_or_else(|err| {
                    panic!("{case_id} query {query_idx}: invalid expect.event_ids: {err}")
                });
            let got_event_ids: Vec<String> = page
                .events
                .iter()
                .map(|item| item.event.id.clone())
                .collect();
            if got_event_ids != exp_event_ids {
                failures.push(format!(
                    "{case_id} query {query_idx}: event_ids expected {exp_event_ids:?}, got {got_event_ids:?}"
                ));
            }

            let exp_scanned = expect["scanned_through"].as_str().unwrap_or_else(|| {
                panic!("{case_id} query {query_idx}: missing expect.scanned_through")
            });
            if page.scanned_through != exp_scanned {
                failures.push(format!(
                    "{case_id} query {query_idx}: scanned_through expected {exp_scanned:?}, got {:?}",
                    page.scanned_through
                ));
            }

            if let Some(exp_ops_map) = expect.get("ops").and_then(Value::as_object) {
                let got_ops_map: BTreeMap<&str, Vec<u32>> = page
                    .events
                    .iter()
                    .map(|item| {
                        (
                            item.event.id.as_str(),
                            item.ops.iter().map(|op| op.ordinal).collect(),
                        )
                    })
                    .collect();
                for (event_id, exp_ordinals_val) in exp_ops_map {
                    let exp_ordinals: Vec<u32> = serde_json::from_value(exp_ordinals_val.clone())
                        .unwrap_or_else(|err| {
                            panic!(
                                "{case_id} query {query_idx}: invalid expect.ops[{event_id}]: {err}"
                            )
                        });
                    match got_ops_map.get(event_id.as_str()) {
                        Some(got_ordinals) if *got_ordinals == exp_ordinals => {}
                        other => failures.push(format!(
                            "{case_id} query {query_idx}: ops[{event_id}] expected {exp_ordinals:?}, got {other:?}"
                        )),
                    }
                }
            }
        }
    }
    assert_no_failures("history", &failures);
}

#[test]
fn conflict_copies() {
    let mut failures = Vec::new();
    for item in section("conflict_copies") {
        let id = item["id"].as_str().expect("conflict_copies[].id");
        let name = item["name"].as_str().expect("conflict_copies[].name");
        let now_utc = item["now_utc"].as_str().expect("conflict_copies[].now_utc");
        let device = item["device"].as_str().expect("conflict_copies[].device");
        let taken: Vec<String> = serde_json::from_value(item["taken"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid taken: {err}"));
        let expected: Option<String> = serde_json::from_value(item["expect"].clone())
            .unwrap_or_else(|err| panic!("{id}: invalid expect: {err}"));

        let got = names::conflict_copy_name(name, now_utc, device, &taken);
        if got != expected {
            failures.push(format!(
                "{id}: conflict_copy_name({name:?}, {now_utc:?}, {device:?}, {taken:?}) expected {expected:?}, got {got:?}"
            ));
        }
    }
    assert_no_failures("conflict_copies", &failures);
}

#[test]
fn media_table_is_the_data_file() {
    let data: Value =
        serde_json::from_str(names::MEDIA_TYPES_JSON).expect("valid files.media-types.json");
    let file_default = data["default"]
        .as_str()
        .expect("files.media-types.json default");
    assert_eq!(names::DEFAULT_MEDIA_TYPE, file_default);

    let file_extensions: BTreeMap<String, String> = data["extensions"]
        .as_object()
        .expect("files.media-types.json extensions")
        .iter()
        .map(|(ext, media)| {
            (
                ext.clone(),
                media.as_str().expect("media type string").to_owned(),
            )
        })
        .collect();

    let mut code_extensions: BTreeMap<String, String> = BTreeMap::new();
    for &(ext, media) in names::MEDIA_TYPES {
        let prev = code_extensions.insert(ext.to_owned(), media.to_owned());
        assert!(prev.is_none(), "duplicate extension '{ext}' in MEDIA_TYPES");
    }

    assert_eq!(code_extensions, file_extensions);
}

fn collect_codes(value: &Value, seen: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(code) = map.get("code").and_then(Value::as_str) {
                seen.insert(code.to_owned());
            }
            for child in map.values() {
                collect_codes(child, seen);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_codes(item, seen);
            }
        }
        _ => {}
    }
}

#[test]
fn every_refusal_code_has_a_vector() {
    let mut seen = BTreeSet::new();
    collect_codes(corpus(), &mut seen);

    let expected: BTreeSet<String> = Code::ALL
        .iter()
        .map(|code| {
            serde_json::to_value(code)
                .expect("serialize Code")
                .as_str()
                .expect("Code serializes as string")
                .to_owned()
        })
        .collect();

    let missing: Vec<&String> = expected.difference(&seen).collect();
    let unexpected: Vec<&String> = seen.difference(&expected).collect();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "refusal code coverage mismatch: missing={missing:?}, unexpected={unexpected:?}"
    );
}

#[test]
fn every_vector_id_is_unique() {
    let root = corpus().as_object().expect("corpus root object");
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut failures = Vec::new();

    for (section_name, val) in root {
        let Some(items) = val.as_array() else {
            continue;
        };
        for (idx, item) in items.iter().enumerate() {
            match item.get("id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => {
                    if let Some(prev_section) = seen.insert(id.to_owned(), section_name.clone()) {
                        failures.push(format!(
                            "duplicate id '{id}' in {section_name}[{idx}] (first seen in {prev_section})"
                        ));
                    }
                }
                _ => failures.push(format!("{section_name}[{idx}]: missing or empty id")),
            }
        }
    }

    assert!(!seen.is_empty(), "corpus has no vector items");
    assert_no_failures("every_vector_id_is_unique", &failures);
}
