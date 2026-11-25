use std::collections::BTreeMap;
use std::sync::OnceLock;

use keeper_accounts_api::Role;
use keeper_files_api::caller::Caller;
use keeper_files_api::names::FileMode;
use keeper_files_api::refusal::{Code, Refusal};
use keeper_files_api::rows::{
    Drive, Entry, Head, Kind, Lifecycle, Proposal, ProposalReviewer, Review,
};
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
        (Value::Array(exp), Value::Array(act)) => {
            exp.len() == act.len() && exp.iter().zip(act.iter()).all(|(e, a)| subset(e, a))
        }
        _ => expected == actual,
    }
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
        ..Manifest::default()
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

struct ManifestSnapshot {
    entries: BTreeMap<String, Entry>,
    head: Option<Head>,
    proposals: BTreeMap<String, (Proposal, Caller, Vec<WireOp>)>,
    inbox: BTreeMap<String, ProposalReviewer>,
    reviews: BTreeMap<String, Review>,
}

impl ManifestSnapshot {
    fn capture(m: &Manifest) -> Self {
        let proposals = m
            .proposals
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    (v.row.clone(), v.proposer.clone(), v.ops.clone()),
                )
            })
            .collect();
        Self {
            entries: m.entries.clone(),
            head: m.head.clone(),
            proposals,
            inbox: m.inbox.clone(),
            reviews: m.reviews.clone(),
        }
    }

    fn assert_unchanged(&self, m: &Manifest, prefix: &str, failures: &mut Vec<String>) {
        if m.entries != self.entries {
            failures.push(format!("{prefix}: refused step mutated manifest entries"));
        }
        if m.head != self.head {
            failures.push(format!("{prefix}: refused step mutated manifest head"));
        }
        let cur_proposals: BTreeMap<String, (Proposal, Caller, Vec<WireOp>)> = m
            .proposals
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    (v.row.clone(), v.proposer.clone(), v.ops.clone()),
                )
            })
            .collect();
        if cur_proposals != self.proposals {
            failures.push(format!("{prefix}: refused step mutated manifest proposals"));
        }
        if m.inbox != self.inbox {
            failures.push(format!("{prefix}: refused step mutated manifest inbox"));
        }
        if m.reviews != self.reviews {
            failures.push(format!("{prefix}: refused step mutated manifest reviews"));
        }
    }
}

fn substitute_placeholders(
    val: &mut Value,
    manifest: &Manifest,
    prefix: &str,
    failures: &mut Vec<String>,
) {
    match val {
        Value::String(s) => {
            if let Some(row_id) = s.strip_prefix("$digest:") {
                match manifest.proposals.get(row_id) {
                    Some(state) => *s = state.row.digest.clone(),
                    None => failures.push(format!(
                        "{prefix}: $digest:{row_id} not found in manifest.proposals"
                    )),
                }
            } else if let Some(event_id) = s.strip_prefix("$delta:") {
                match manifest.events.get(event_id) {
                    Some((event, _)) => *s = event.delta_digest.clone(),
                    None => failures.push(format!(
                        "{prefix}: $delta:{event_id} not found in manifest.events"
                    )),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute_placeholders(item, manifest, prefix, failures);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                substitute_placeholders(item, manifest, prefix, failures);
            }
        }
        _ => {}
    }
}

fn check_refusal(refusal: &Refusal, expect: &Value, prefix: &str, failures: &mut Vec<String>) {
    let exp_code: Code = serde_json::from_value(expect["code"].clone())
        .unwrap_or_else(|err| panic!("{prefix}: invalid expect.code: {err}"));
    if refusal.code != exp_code {
        failures.push(format!(
            "{prefix}: expected code {exp_code:?}, got {:?} (detail: {:?})",
            refusal.code, refusal.detail
        ));
    }
    if let Some(exp_detail) = expect.get("detail").filter(|d| !d.is_null()) {
        match &refusal.detail {
            Some(act_detail) if subset(exp_detail, act_detail) => {}
            other => failures.push(format!(
                "{prefix}: detail mismatch: expected subset {exp_detail}, got {other:?}"
            )),
        }
    }
}

fn check_post_state(manifest: &Manifest, expect: &Value, prefix: &str, failures: &mut Vec<String>) {
    if let Some(exp_rows) = expect.get("proposal_rows").and_then(Value::as_object) {
        for (row_id, exp_row) in exp_rows {
            if exp_row.is_null() {
                if let Some(act) = manifest.proposals.get(row_id) {
                    failures.push(format!(
                        "{prefix}: expected proposal row {row_id} absent, got {:?}",
                        act.row
                    ));
                }
                continue;
            }
            match manifest.proposals.get(row_id) {
                Some(act) => {
                    let act_val = serde_json::to_value(&act.row).expect("serialize Proposal");
                    if !subset(exp_row, &act_val) {
                        failures.push(format!(
                            "{prefix}: proposal row {row_id} mismatch:\n  expected subset: {exp_row}\n  actual: {act_val}"
                        ));
                    }
                }
                None => failures.push(format!("{prefix}: expected proposal row {row_id} missing")),
            }
        }
    }

    if let Some(exp_inbox) = expect.get("inbox").and_then(Value::as_object) {
        for (id, exp_row) in exp_inbox {
            match manifest.inbox.get(id) {
                Some(act) => {
                    let act_val = serde_json::to_value(act).expect("serialize ProposalReviewer");
                    if !subset(exp_row, &act_val) {
                        failures.push(format!(
                            "{prefix}: inbox {id} mismatch:\n  expected subset: {exp_row}\n  actual: {act_val}"
                        ));
                    }
                }
                None => failures.push(format!("{prefix}: expected inbox row {id} missing")),
            }
        }
    }

    if let Some(absent) = expect.get("inbox_absent").and_then(Value::as_array) {
        for id_val in absent {
            let id = id_val.as_str().expect("inbox_absent id string");
            if let Some(act) = manifest.inbox.get(id) {
                failures.push(format!(
                    "{prefix}: expected inbox row {id} absent, got {act:?}"
                ));
            }
        }
    }

    if let Some(exp_reviews) = expect.get("reviews").and_then(Value::as_object) {
        for (id, exp_row) in exp_reviews {
            match manifest.reviews.get(id) {
                Some(act) => {
                    let act_val = serde_json::to_value(act).expect("serialize Review");
                    if !subset(exp_row, &act_val) {
                        failures.push(format!(
                            "{prefix}: review {id} mismatch:\n  expected subset: {exp_row}\n  actual: {act_val}"
                        ));
                    }
                }
                None => failures.push(format!("{prefix}: expected review row {id} missing")),
            }
        }
    }

    if let Some(exp_events) = expect.get("events").and_then(Value::as_object) {
        for (event_id, exp_ev) in exp_events {
            match manifest.events.get(event_id) {
                Some((act_ev, _)) => {
                    let act_val = serde_json::to_value(act_ev).expect("serialize Event");
                    if !subset(exp_ev, &act_val) {
                        failures.push(format!(
                            "{prefix}: event {event_id} mismatch:\n  expected subset: {exp_ev}\n  actual: {act_val}"
                        ));
                    }
                }
                None => failures.push(format!("{prefix}: expected event {event_id} missing")),
            }
        }
    }

    if let Some(exp_entries) = expect.get("entries").and_then(Value::as_object) {
        for (entry_id, exp_entry) in exp_entries {
            if exp_entry.is_null() {
                if let Some(act_entry) = manifest.entries.get(entry_id) {
                    failures.push(format!(
                        "{prefix}: expected entry {entry_id} absent, got {act_entry:?}"
                    ));
                }
                continue;
            }
            match manifest.entries.get(entry_id) {
                Some(act_entry) => {
                    let act_val = serde_json::to_value(act_entry).expect("serialize Entry");
                    if !subset(exp_entry, &act_val) {
                        failures.push(format!(
                            "{prefix}: entry {entry_id} mismatch:\n  expected subset: {exp_entry}\n  actual: {act_val}"
                        ));
                    }
                }
                None => failures.push(format!("{prefix}: expected entry {entry_id} missing")),
            }
        }
    }

    if let Some(exp_head) = expect.get("head") {
        match &manifest.head {
            Some(act_head) => {
                let act_val = serde_json::to_value(act_head).expect("serialize Head");
                if !subset(exp_head, &act_val) {
                    failures.push(format!(
                        "{prefix}: head mismatch:\n  expected subset: {exp_head}\n  actual: {act_val}"
                    ));
                }
            }
            None if exp_head.is_null() => {}
            None => failures.push(format!("{prefix}: expected head {exp_head}, got None")),
        }
    }
}

fn check_read_result(
    actual_val: &Value,
    actual_ids: &[&str],
    expect: &Value,
    prefix: &str,
    failures: &mut Vec<String>,
) {
    if let Some(exp_read) = expect.get("read") {
        if !subset(exp_read, actual_val) {
            failures.push(format!(
                "{prefix}: read mismatch:\n  expected subset: {exp_read}\n  actual: {actual_val}"
            ));
        }
    }
    if let Some(exp_ids_val) = expect.get("read_ids").and_then(Value::as_array) {
        let exp_ids: Vec<&str> = exp_ids_val
            .iter()
            .map(|v| v.as_str().expect("read_ids entry must be string"))
            .collect();
        if actual_ids != exp_ids.as_slice() {
            failures.push(format!(
                "{prefix}: read_ids mismatch: expected {exp_ids:?}, got {actual_ids:?}"
            ));
        }
    }
}

fn apply_roles_map(manifest: &mut Manifest, roles_obj: &serde_json::Map<String, Value>, ctx: &str) {
    for (actor, role_val) in roles_obj {
        if role_val.is_null() {
            manifest.roles.remove(actor);
        } else {
            let role: Role = serde_json::from_value(role_val.clone())
                .unwrap_or_else(|err| panic!("{ctx}: invalid role for {actor}: {err}"));
            manifest.roles.insert(actor.clone(), role);
        }
    }
}

fn run_steps(case_id: &str, manifest: &mut Manifest, steps: &[Value], failures: &mut Vec<String>) {
    for (step_idx, step_val) in steps.iter().enumerate() {
        let prefix = format!("{case_id} step {step_idx}");
        if let Some(roles_obj) = step_val.get("roles").and_then(Value::as_object) {
            apply_roles_map(manifest, roles_obj, &prefix);
            continue;
        }

        let mut step: StepSpec = serde_json::from_value(step_val.clone())
            .unwrap_or_else(|err| panic!("{prefix}: parse error: {err}"));
        substitute_placeholders(&mut step.params, manifest, &prefix, failures);
        let snap = ManifestSnapshot::capture(manifest);

        match step.descriptor.as_str() {
            "files.propose" => {
                let server = step
                    .server
                    .as_ref()
                    .unwrap_or_else(|| panic!("{prefix}: missing server"));
                let result = rules::propose(manifest, &step.caller, &step.params, server);
                let outcome = step.expect["outcome"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{prefix}: missing expect.outcome"));
                match outcome {
                    "rejected" => match result {
                        Ok(detail) => failures.push(format!(
                            "{prefix}: expected propose refusal, got {detail:?}"
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    },
                    "proposed" => match result {
                        Ok(detail) => {
                            if let Some(exp_detail) = step.expect.get("detail") {
                                let act_val = serde_json::to_value(&detail)
                                    .expect("serialize ProposedDetail");
                                if !subset(exp_detail, &act_val) {
                                    failures.push(format!(
                                        "{prefix}: propose detail mismatch:\n  expected subset: {exp_detail}\n  actual: {act_val}"
                                    ));
                                }
                            }
                        }
                        Err(refusal) => {
                            failures.push(format!("{prefix}: expected proposed, got {refusal:?}"));
                            break;
                        }
                    },
                    other => failures.push(format!("{prefix}: unknown propose outcome {other:?}")),
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
            "files.decide" => {
                let server = step
                    .server
                    .as_ref()
                    .unwrap_or_else(|| panic!("{prefix}: missing server"));
                let result = rules::decide(manifest, &step.caller, &step.params, server);
                if let Some(exp_receipt) = step.expect.get("receipt") {
                    match result {
                        Ok(receipt) => {
                            let act_val =
                                serde_json::to_value(&receipt).expect("serialize DecideReceipt");
                            if !subset(exp_receipt, &act_val) {
                                failures.push(format!(
                                    "{prefix}: decide receipt mismatch:\n  expected subset: {exp_receipt}\n  actual: {act_val}"
                                ));
                            }
                        }
                        Err(refusal) => {
                            failures.push(format!(
                                "{prefix}: expected decide receipt, got {refusal:?}"
                            ));
                            break;
                        }
                    }
                } else if step.expect.get("outcome").and_then(Value::as_str) == Some("rejected") {
                    match result {
                        Ok(receipt) => failures.push(format!(
                            "{prefix}: expected whole-list decide refusal, got {receipt:?}"
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    }
                } else {
                    failures.push(format!(
                        "{prefix}: files.decide expect must have 'receipt' or outcome 'rejected'"
                    ));
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
            "files.proposal" => {
                let result = rules::proposal(manifest, &step.caller, &step.params);
                if step.expect.get("outcome").and_then(Value::as_str) == Some("rejected") {
                    match result {
                        Ok(rows) => failures.push(format!(
                            "{prefix}: expected files.proposal refusal, got {} row(s)",
                            rows.len()
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    }
                } else {
                    match result {
                        Ok(rows) => {
                            let act_val = serde_json::to_value(&rows).expect("serialize Proposal");
                            let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
                            check_read_result(&act_val, &ids, &step.expect, &prefix, failures);
                        }
                        Err(refusal) => failures.push(format!(
                            "{prefix}: expected files.proposal ok, got {refusal:?}"
                        )),
                    }
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
            "files.proposals" => {
                let now_ms = step.server.as_ref().map_or(0, |s| s.recorded_at_ms);
                let result = rules::proposals(manifest, &step.caller, &step.params, now_ms);
                if step.expect.get("outcome").and_then(Value::as_str) == Some("rejected") {
                    match result {
                        Ok(page) => failures.push(format!(
                            "{prefix}: expected files.proposals refusal, got {page:?}"
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    }
                } else {
                    match result {
                        Ok(page) => {
                            let act_val =
                                serde_json::to_value(&page).expect("serialize ProposalsPage");
                            let ids: Vec<&str> =
                                page.proposals.iter().map(|r| r.id.as_str()).collect();
                            check_read_result(&act_val, &ids, &step.expect, &prefix, failures);
                        }
                        Err(refusal) => failures.push(format!(
                            "{prefix}: expected files.proposals ok, got {refusal:?}"
                        )),
                    }
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
            "files.unreviewed" => {
                let result = rules::unreviewed(manifest, &step.caller, &step.params);
                if step.expect.get("outcome").and_then(Value::as_str) == Some("rejected") {
                    match result {
                        Ok(page) => failures.push(format!(
                            "{prefix}: expected files.unreviewed refusal, got {page:?}"
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    }
                } else {
                    match result {
                        Ok(page) => {
                            let act_val =
                                serde_json::to_value(&page).expect("serialize UnreviewedPage");
                            let ids: Vec<&str> =
                                page.events.iter().map(|i| i.event.id.as_str()).collect();
                            check_read_result(&act_val, &ids, &step.expect, &prefix, failures);
                        }
                        Err(refusal) => failures.push(format!(
                            "{prefix}: expected files.unreviewed ok, got {refusal:?}"
                        )),
                    }
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
            _ => {
                let server = step
                    .server
                    .as_ref()
                    .unwrap_or_else(|| panic!("{prefix}: missing server"));
                let result = rules::evaluate(
                    manifest,
                    &step.caller,
                    &step.descriptor,
                    &step.params,
                    server,
                );
                let outcome = step.expect["outcome"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{prefix}: missing expect.outcome"));
                match outcome {
                    "rejected" => match result {
                        Ok(accepted) => failures.push(format!(
                            "{prefix}: expected refusal, got accepted event {}",
                            accepted.event.id
                        )),
                        Err(refusal) => {
                            check_refusal(&refusal, &step.expect, &prefix, failures);
                            snap.assert_unchanged(manifest, &prefix, failures);
                        }
                    },
                    "accepted" => {
                        let accepted = match result {
                            Ok(accepted) => accepted,
                            Err(refusal) => {
                                failures
                                    .push(format!("{prefix}: expected accepted, got {refusal:?}"));
                                break;
                            }
                        };
                        if let Some(exp_event) = step.expect.get("event") {
                            let act_event =
                                serde_json::to_value(&accepted.event).expect("serialize Event");
                            if !subset(exp_event, &act_event) {
                                failures.push(format!(
                                    "{prefix}: event mismatch:\n  expected subset: {exp_event}\n  actual: {act_event}"
                                ));
                            }
                        }
                        if let Some(exp_count) = step.expect.get("op_count").and_then(Value::as_u64)
                        {
                            let exp_count = usize::try_from(exp_count).expect("op_count usize");
                            if accepted.ops.len() != exp_count {
                                failures.push(format!(
                                    "{prefix}: op_count expected {exp_count}, got ops.len()={}",
                                    accepted.ops.len()
                                ));
                            }
                            if accepted.event.op_count as usize != exp_count {
                                failures.push(format!(
                                    "{prefix}: event.op_count expected {exp_count}, got {}",
                                    accepted.event.op_count
                                ));
                            }
                        }
                        if let Some(exp_ops) = step.expect.get("ops").and_then(Value::as_array) {
                            if accepted.ops.len() < exp_ops.len() {
                                failures.push(format!(
                                    "{prefix}: expected at least {} ops, got {}",
                                    exp_ops.len(),
                                    accepted.ops.len()
                                ));
                            } else {
                                for (op_idx, exp_op) in exp_ops.iter().enumerate() {
                                    let act_op = serde_json::to_value(&accepted.ops[op_idx])
                                        .expect("serialize Op");
                                    if !subset(exp_op, &act_op) {
                                        failures.push(format!(
                                            "{prefix} op {op_idx}: mismatch:\n  expected subset: {exp_op}\n  actual: {act_op}"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    other => failures.push(format!("{prefix}: unknown outcome {other:?}")),
                }
                check_post_state(manifest, &step.expect, &prefix, failures);
            }
        }
    }
}

fn run_section(name: &str) {
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
        if let Some(roles_obj) = case.get("roles").and_then(Value::as_object) {
            apply_roles_map(&mut manifest, roles_obj, case_id);
        }
        run_steps(case_id, &mut manifest, steps, &mut failures);
    }
    assert_no_failures(name, &failures);
}

#[test]
fn proposals() {
    run_section("proposals");
}

#[test]
fn reviews() {
    run_section("reviews");
}

#[test]
fn outcomes() {
    run_section("outcomes");
}
