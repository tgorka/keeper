use super::*;

fn history(d: &Device<'_>, entry_id: &str) -> Value {
    d.ok(&["history", "--entity", entry_id])
}

fn only_item(page: &Value) -> &Value {
    let [item] = rows(page, "events") else {
        panic!("expected exactly one history item")
    };
    item
}

// Read the unfiltered ledger at the receipt's sequence. A review-only event
// has no entry operation and cannot be found by the --entity filter.
fn receipt_item(d: &Device<'_>, receipt: &Value) -> Value {
    let seq: u64 = text(receipt, "seq").parse().expect("receipt sequence");
    let page = d.ok(&[
        "history",
        "--after",
        &seq.checked_sub(1).expect("positive sequence").to_string(),
        "--limit",
        "1",
    ]);
    let item = only_item(&page);
    assert!(text(&item["event"], "seq") == text(receipt, "seq"));
    assert!(text(&item["event"], "id") == text(receipt, "event_id"));
    item.clone()
}

fn check_review_op(op: &Value, event: &Value, new_event: &str, actor: &str, kind: &str) {
    assert!(text(op, "event_id") == new_event);
    assert!(text(op, "entity_type") == "review");
    assert!(text(op, "operation") == "create");
    let image = &op["after_value"];
    assert!(text(image, "id") == text(op, "entity_id"));
    assert!(text(image, "target_event_id") == text(event, "id"));
    assert!(text(image, "target_delta_digest") == text(event, "delta_digest"));
    assert!(text(image, "reviewer_actor") == actor);
    assert!(text(image, "kind") == kind);
}

// Put a witness *after* the action; absence in the same folder listing as
// this witness cannot be a pass from an empty or pre-action replica.
fn check_absent(editor: &Device<'_>, reader: &Device<'_>, path: &str) {
    let leaf = path.strip_prefix("A/").expect("scratch path");
    let witness = unique("visible");
    let put = editor.put(&format!("A/{witness}"), witness.as_bytes());
    let listing = reader.ok(&["ls", "--drive", &reader.cfg.drive, "A"]);
    assert!(text(entry(&listing, &witness), "id") == text(&put, "entry_id"));
    assert!(!rows(&listing, "entries")
        .iter()
        .any(|v| text(v, "name") == leaf));
    // This not_found describes path resolution, not download-grant policy.
    assert!(
        text(
            &reader.call(&["get", "--drive", &reader.cfg.drive, path], 2, false),
            "code"
        ) == "not_found"
    );
}

// Anchor at the sequence immediately before the event: a full earlier page may
// not be used as evidence of absence. Runs on a dedicated serial scratch domain.
fn is_unreviewed(d: &Device<'_>, event: &Value) -> bool {
    let seq: u64 = text(event, "seq").parse().expect("event sequence");
    let page = d.ok(&["unreviewed", "--after", &(seq - 1).to_string()]);
    assert!(
        text(&page, "scanned_through")
            .parse::<u64>()
            .expect("cursor")
            >= seq
    );
    rows(&page, "events")
        .iter()
        .any(|item| text(&item["event"], "id") == text(event, "id"))
}

#[test]
#[ignore = "needs editor and reviewer scratch credentials"]
fn direct_create_and_replace_are_not_unreviewed() {
    let cfg = live();
    let editor = Device::new(&cfg, "EDITOR");
    let reviewer = Device::new(&cfg, "REVIEWER");
    let path = unique("direct");
    let first = editor.put(&path, b"one");
    let second = editor.put(&path, b"two");
    let id = text(&first, "entry_id");
    assert!(text(&second, "entry_id") == id);
    let page = history(&editor, id);
    let events = rows(&page, "events");
    assert!(events.len() == 2);
    assert!(text(&events[0]["event"], "id") != text(&events[1]["event"], "id"));
    for ((item, put), action) in events
        .iter()
        .zip([&first, &second])
        .zip(["create", "replace"])
    {
        let event = &item["event"];
        let [op] = rows(item, "ops") else {
            panic!("expected one direct entry operation")
        };
        assert!(text(op, "operation") == action);
        assert!(text(op, "entity_type") == "entry");
        assert!(text(op, "entity_id") == id);
        assert!(text(op, "event_id") == text(event, "id"));
        assert!(text(op, "after_content_id") == text(put, "content_id"));
        assert!(text(event, "request_id") == text(put, "request_id"));
        assert!(event["actor_id"] == editor.actor);
        assert!(event["origin_class"] == "human");
        assert!(event["approval_basis"]["basis"] == "direct");
        assert!(!is_unreviewed(&reviewer, event));
    }
}

#[test]
#[ignore = "needs scratch propose-only tool, reviewer and real object store"]
fn tool_proposals_do_not_publish_until_decided() {
    let cfg = live();
    let tool = Device::new(&cfg, "PROPOSER");
    let reviewer = Device::new(&cfg, "REVIEWER");
    let reader = Device::new(&cfg, "EDITOR");
    for approve in [true, false] {
        let name = unique("proposal");
        let path = format!("A/{name}");
        let bytes = path.as_bytes();
        let file = tool.body(bytes);
        let cid = hash_file(Path::new(&file)).1;
        let proposed = tool.ok(&[
            "put",
            "--file",
            &file,
            "--drive",
            &cfg.drive,
            &path,
            "--propose",
            "--reviewer",
            &reviewer.actor,
        ]);
        assert!(proposed["outcome"] == "proposed");
        let id = text(&proposed, "proposal_id");
        let revision = text(&proposed, "proposal_revision");
        let listed = reviewer.ok(&["proposals"]);
        assert!(rows(&listed, "proposals")
            .iter()
            .any(|p| p["proposal_id"] == id && p["revision"] == revision && p["status"] == "open"));
        check_absent(&reader, &reader, &path);
        let opened = reviewer.ok(&["proposal", id, "--open"]);
        let [item] = rows(&opened, "opened") else {
            panic!("one proposed file")
        };
        assert!(item["fetched"] == true);
        assert!(item["content_id"] == cid);
        assert!(hash_file(Path::new(text(item, "file"))).1 == cid);
        let entry_id = text(item, "entry_id");
        assert!(rows(&history(&reviewer, entry_id), "events").is_empty());
        let decided = reviewer.ok(&[
            "decide",
            &format!("{id}/{revision}"),
            if approve { "--approve" } else { "--reject" },
        ]);
        let [decision] = rows(&decided, "items") else {
            panic!("one decision")
        };
        let own = tool.ok(&["proposals", "--mine"]);
        let rev = rows(&own, "proposals")
            .iter()
            .find(|p| p["proposal_id"] == id && p["revision"] == revision)
            .expect("own revision");
        if approve {
            assert!(rev["status"] == "approved");
            let event_id = text(decision, "event_id");
            assert!(rev["decided_event_id"] == event_id);
            let fetched = reader.get(&path, bytes);
            assert!(fetched["fetched"] == true);
            assert!(fetched["entry"]["id"] == entry_id);
            let page = history(&reader, entry_id);
            let event = &only_item(&page)["event"];
            assert!(text(event, "id") == event_id);
            assert!(event["actor_id"] == reviewer.actor);
            assert!(event["origin_class"] == "human");
            let basis = &event["approval_basis"];
            assert!(basis["basis"] == "proposal");
            assert!(basis["proposal_id"] == id);
            assert!(basis["proposal_revision"] == revision);
            assert!(basis["proposer_actor"] == tool.actor);
        } else {
            assert!(rev["status"] == "rejected");
            assert!(decision.get("event_id") == Some(&Value::Null));
            assert!(rows(&history(&reviewer, entry_id), "events").is_empty());
            check_absent(&reader, &reader, &path);
        }
    }
}

#[test]
#[ignore = "needs scratch apply and propose tools and a reviewer"]
fn applied_changes_can_be_acknowledged_or_reversed() {
    let cfg = live();
    let apply = Device::new(&cfg, "APPLIER");
    let propose = Device::new(&cfg, "PROPOSER");
    let reviewer = Device::new(&cfg, "REVIEWER");
    let editor = Device::new(&cfg, "EDITOR");
    for ack in [true, false] {
        let path = format!("A/{}", unique("apply"));
        let put = apply.put(&path, path.as_bytes());
        let entry_id = text(&put, "entry_id");
        let original_page = history(&reviewer, entry_id);
        let original = only_item(&original_page);
        let event = &original["event"];
        let id = text(event, "id");
        let [create] = rows(original, "ops") else {
            panic!("expected a single create")
        };
        assert!(text(create, "operation") == "create");
        assert!(text(create, "entity_id") == entry_id);
        assert!(event["actor_id"] == apply.actor);
        assert!(event["origin_class"] == "tool");
        assert!(event["approval_basis"]["basis"] == "grant");
        assert!(event["approval_basis"]["mode"] == "apply");
        assert!(is_unreviewed(&reviewer, event));
        if ack {
            let ack = reviewer.ok(&["acknowledge", id]);
            assert!(ack["outcome"] == "accepted");
            let acknowledged = receipt_item(&reviewer, &ack);
            let [op] = rows(&acknowledged, "ops") else {
                panic!("expected one review operation")
            };
            check_review_op(
                op,
                event,
                text(&ack, "event_id"),
                &reviewer.actor,
                "acknowledgement",
            );
            assert!(reviewer.call(&["acknowledge", id], 2, false)["code"] == "invalid_change");
            let after = history(&reviewer, entry_id);
            assert!(
                only_item(&after) == original,
                "original history item changed after acknowledgement"
            );
            assert!(reviewer.ok(&["preview-reversal", id])["status"] == "ready");
        } else {
            let preview = reviewer.ok(&["preview-reversal", id]);
            assert!(preview["target_event_id"] == id);
            assert!(preview["status"] == "ready");
            let [planned] = rows(&preview, "plan") else {
                panic!("expected one inverse for file creation")
            };
            assert!(text(planned, "op") == "delete");
            assert!(text(planned, "id") == entry_id);
            assert!(text(planned, "expected_revision") == text(create, "after_revision"));
            let reversed = reviewer.ok(&["reverse", id]);
            assert!(reversed["outcome"] == "accepted");
            let result = receipt_item(&reviewer, &reversed);
            assert!(text(&result["event"], "reverses_event_id") == id);
            let [inverse, review] = rows(&result, "ops") else {
                panic!("expected inverse and review")
            };
            assert!(text(inverse, "event_id") == text(&reversed, "event_id"));
            assert!(text(inverse, "entity_id") == entry_id);
            assert!(text(inverse, "entity_type") == "entry");
            assert!(text(inverse, "operation") == "delete");
            check_review_op(
                review,
                event,
                text(&reversed, "event_id"),
                &reviewer.actor,
                "reversal",
            );
            check_absent(&editor, &reviewer, &path);
        }
        assert!(!is_unreviewed(&reviewer, event));
    }
    let file = propose.body(b"must stay unpublished");
    let path = format!("A/{}", unique("plain"));
    let refused = propose.call(
        &["put", "--drive", &cfg.drive, &path, "--file", &file],
        2,
        false,
    );
    assert!(refused["code"] == "propose_required");
    check_absent(&editor, &reviewer, &path);
}
