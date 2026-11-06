//! `syncular/knowledge-graph.vectors.json`'s query-side sections hold for
//! this implementation: every `queries[]` entry admits to its engine text,
//! normalised parameters, aliases and limit, or refuses with its code;
//! `prefix_chains[]` and `manifests[]` reproduce; `worker` names the
//! constants the consumer and its worker share.

use keeper_graph_protocol::profile::admit;
use keeper_graph_protocol::worker::{
    chain, EventFrame, Manifest, ENGINE, GENESIS, LINUX_WORKER_GLIBC_TUNABLES, PROJECTION_VERSION,
};
use keeper_graph_protocol::{delta, Refusal, PROFILE};
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("vector JSON")
}

fn text<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {entry}"))
}

/// `source` and `parameters` with the entry's fill applied to the one
/// `{FILL}` in its source, else in its parameters.
fn expanded(entry: &Value) -> (String, String) {
    let (mut source, mut parameters) = (
        text(entry, "source").to_owned(),
        text(entry, "parameters").to_owned(),
    );
    if let Some(fill) = entry.get("fill") {
        let filler = text(fill, "text").repeat(fill["count"].as_u64().expect("count") as usize);
        if source.contains("{FILL}") {
            source = source.replacen("{FILL}", &filler, 1);
        } else {
            parameters = parameters.replacen("{FILL}", &filler, 1);
        }
    }
    (source, parameters)
}

#[test]
fn query_vectors_admit_or_refuse() {
    let v = corpus();
    let entries = v["queries"].as_array().expect("queries");
    assert!(entries.len() >= 80, "the query corpus lost entries");
    for entry in entries {
        let id = text(entry, "id");
        let (source, parameters) = expanded(entry);
        let profile = entry
            .get("profile")
            .and_then(Value::as_str)
            .unwrap_or(PROFILE);
        let admitted = admit(profile, &source, parameters.as_bytes());
        match entry.get("refusal") {
            Some(code) => assert_eq!(
                admitted.err().map(Refusal::code),
                code.as_str(),
                "{id}: expected a refusal"
            ),
            None => {
                let query = admitted.unwrap_or_else(|r| panic!("{id}: refused {r}"));
                assert_eq!(query.engine_text(), text(entry, "engine"), "{id}");
                if let Some(normalized) = entry.get("normalized") {
                    assert_eq!(
                        query.parameters_json().expect("parameters"),
                        normalized.as_str().expect("normalized"),
                        "{id}"
                    );
                }
                if let Some(aliases) = entry.get("aliases") {
                    assert_eq!(
                        serde_json::to_value(query.aliases()).expect("aliases"),
                        *aliases,
                        "{id}"
                    );
                }
                if let Some(limit) = entry.get("limit") {
                    assert_eq!(Some(query.limit() as u64), limit.as_u64(), "{id}");
                }
            }
        }
    }
}

#[test]
fn prefix_chains_reproduce() {
    let v = corpus();
    assert_eq!(v["worker"]["prefix_genesis"], GENESIS);
    for entry in v["prefix_chains"].as_array().expect("prefix_chains") {
        let mut digest = GENESIS.to_owned();
        for event in entry["events"].as_array().expect("events") {
            let seq = event[0].as_u64().expect("graph_seq");
            digest = chain(&digest, seq, event[1].as_str().expect("delta_digest"));
        }
        assert_eq!(digest, text(entry, "prefix_digest"), "{}", entry["id"]);
    }
}

#[test]
fn manifest_vectors_validate_or_refuse() {
    let v = corpus();
    for entry in v["manifests"].as_array().expect("manifests") {
        let id = text(entry, "id");
        let manifest: Manifest =
            serde_json::from_value(entry["manifest"].clone()).expect("manifest shape");
        let got = manifest.validate().err().map(Refusal::code);
        assert_eq!(got, entry.get("refusal").and_then(Value::as_str), "{id}");
        if got.is_none() {
            let line = manifest.encode().expect("frame");
            assert_eq!(line.last(), Some(&b'\n'));
            let back: Manifest = serde_json::from_slice(&line).expect("frame reads back");
            assert_eq!(back, manifest, "{id}");
        }
    }
}

#[test]
fn worker_constants_agree() {
    let v = corpus();
    let w = &v["worker"];
    assert_eq!(w["engine"], ENGINE);
    assert_eq!(w["projection_version"], PROJECTION_VERSION);
    assert_eq!(w["linux_glibc_tunables"], LINUX_WORKER_GLIBC_TUNABLES);
    // OD-A H1: the three reviewed fields keep their bytes and order, and one
    // arena is appended.
    assert_eq!(
        LINUX_WORKER_GLIBC_TUNABLES,
        "glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912:glibc.malloc.arena_max=1"
    );
}

/// Every accepted `algebra[]` event (whole images, as the server writes
/// them) exports to a frame that checks at its position, carries no
/// before-image, and refuses at any other position or with its operation
/// count disturbed.
#[test]
fn accepted_events_export_to_checked_frames() {
    let v = corpus();
    let mut checked = 0;
    for entry in v["algebra"].as_array().expect("algebra") {
        if entry.get("fills").is_some() || entry.get("ops").is_none() {
            continue;
        }
        let ops: Vec<delta::Op> =
            serde_json::from_value(entry["ops"].clone()).expect("accepted ops");
        let digest = delta::digest(&ops).expect("accepted digest");
        let frame = EventFrame::new(7, "01JD0000000000000000000099", &digest, &ops);
        frame
            .check(7)
            .unwrap_or_else(|r| panic!("{}: {r}", entry["id"]));
        let line = frame.encode().expect("encode");
        assert!(!String::from_utf8_lossy(&line).contains("before_"));
        assert_eq!(frame.check(8), Err(Refusal::InvalidChange));
        let mut short = frame.clone();
        short.op_count += 1;
        assert_eq!(short.check(7), Err(Refusal::InvalidChange));
        checked += 1;
    }
    assert!(checked >= 5, "too few accepted events exported");
}
