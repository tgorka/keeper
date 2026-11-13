//! Measurement tooling for story 110.5's P1 (never shipped: an example of a library crate,
//! not built by `--bins`; `scripts/check-graph-boundary.py` pins that). It regenerates the
//! reviewed 110.1 reference corpus in the 110.5 worker protocol: manifest frames with the
//! authority and chained prefix digest, after-image-only event frames, `KG-Cypher/1` query
//! frames and typed inspection frames. Every input is deterministic (event ids come from a
//! fixed-seed generator), every input's SHA-256 is recorded, and every case names the 110.1
//! item it maps from.
//!
//! ```sh
//! cargo run -p keeper-graph-protocol --example rc_corpus -- <out-dir> [--large]
//! ```
//!
//! Writes `<out-dir>/corpus.json`, `<out-dir>/corpus.json.sha256` and the inputs under
//! `<out-dir>/inputs/`. The two large fixtures (the benchmark, ~90 MB, and the limits
//! fixture, ~65 MB) are hashed always and written only with `--large`; the runner
//! regenerates them with the same command and checks the recorded digest.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use keeper_graph_protocol::delta::{self, Op};
use keeper_graph_protocol::profile::{admit, MAX_PARAMETER_BYTES};
use keeper_graph_protocol::vocabulary::{EntityType, Operation};
use keeper_graph_protocol::worker::{
    chain, EventFrame, InspectEntity, InspectRequest, Manifest, ENGINE, GENESIS, MAX_EVENT_BYTES,
    PROJECTION_VERSION,
};
use keeper_graph_protocol::{Refusal, PROFILE, SCHEMA_VERSION};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

/// The fixed seed of the event-id generator ("KG1105R6").
const SEED: u64 = 0x4B47_3131_3035_5236;
const DOMAIN: &str = "personal";
const AUTHORITY_DIGEST: &str = "ab";
const VECTORS: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// splitmix64.
struct Seeded(u64);
impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A canonical ULID (first character `0`).
    fn ulid(&mut self) -> String {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let (a, b) = (self.next(), self.next());
        let mut out = String::from("0");
        for i in 0..25 {
            let word = if i < 12 {
                a >> (i * 5)
            } else {
                b >> ((i - 12) * 5)
            };
            out.push(char::from(ALPHABET[(word & 31) as usize]));
        }
        out
    }
}

/// The 110.1 ids: 26 decimal digits, which are canonical ULIDs.
fn id(n: u64) -> String {
    format!("{n:026}")
}

fn node(n: u64) -> Value {
    json!({"id": id(n), "kind": "Concept", "title": format!("node {n}"), "description": null,
           "description_source_id": null, "properties": {"score": 1.0}, "lifecycle": "active",
           "merged_into_id": null, "revision": "1", "lifecycle_revision": "1"})
}

fn edge(n: u64, from: u64, to: u64) -> Value {
    json!({"id": id(n), "from_id": id(from), "to_id": id(to), "type": "contains",
           "properties": {}, "lifecycle": "active", "revision": "1"})
}

fn tag(n: u64, text: &str) -> Value {
    json!({"id": id(n), "text": text, "label": null, "revision": "1"})
}

fn membership(n: u64, node: u64, tag: u64) -> Value {
    json!({"id": id(n), "node_id": id(node), "tag_id": id(tag)})
}

fn kind_of(image: &Value) -> EntityType {
    if image.get("node_id").is_some() {
        EntityType::NodeTag
    } else if image.get("from_id").is_some() {
        EntityType::Edge
    } else if image.get("kind").is_some() {
        EntityType::Node
    } else {
        EntityType::Tag
    }
}

/// An accepted operation: a create without `before`, else an update (or an archive or
/// unarchive when the lifecycle changes).
fn put(after: Value, before: Option<Value>) -> Op {
    let entity_type = kind_of(&after);
    let membership = entity_type == EntityType::NodeTag;
    let operation = match &before {
        None => Operation::Create,
        Some(b) if b["lifecycle"] != after["lifecycle"] && after["lifecycle"] == "archived" => {
            Operation::Archive
        }
        Some(b) if b["lifecycle"] != after["lifecycle"] => Operation::Unarchive,
        Some(_) => Operation::Update,
    };
    let revision = |v: &Value| v["revision"].as_str().map(str::to_owned);
    Op {
        ordinal: 0,
        entity_type,
        entity_id: after["id"].as_str().unwrap_or_default().to_owned(),
        operation,
        before_revision: if membership {
            None
        } else {
            before.as_ref().and_then(revision)
        },
        after_revision: if membership { None } else { revision(&after) },
        before_value: before,
        after_value: Some(after),
    }
}

fn remove(before: Value) -> Op {
    Op {
        ordinal: 0,
        entity_type: EntityType::NodeTag,
        entity_id: before["id"].as_str().unwrap_or_default().to_owned(),
        operation: Operation::Remove,
        before_revision: None,
        after_revision: None,
        before_value: Some(before),
        after_value: None,
    }
}

/// One input file: written (or only hashed) as it is produced.
struct Sink {
    name: String,
    file: Option<std::io::BufWriter<std::fs::File>>,
    hash: Sha256,
    bytes: u64,
}

impl Sink {
    fn new(dir: &Path, name: &str, write: bool) -> Result<Self> {
        Ok(Self {
            name: name.to_owned(),
            file: if write {
                Some(std::io::BufWriter::new(std::fs::File::create(
                    dir.join(name),
                )?))
            } else {
                None
            },
            hash: Sha256::new(),
            bytes: 0,
        })
    }
    fn put(&mut self, data: &[u8]) -> Result<()> {
        self.hash.update(data);
        self.bytes += data.len() as u64;
        if let Some(f) = &mut self.file {
            f.write_all(data)?;
        }
        Ok(())
    }
    fn finish(mut self, generated_only: bool) -> Result<Value> {
        if let Some(f) = &mut self.file {
            f.flush()?;
        }
        Ok(
            json!({"file": format!("inputs/{}", self.name), "sha256": hex(&self.hash.finalize()),
                  "bytes": self.bytes, "written": !generated_only}),
        )
    }
}

/// A build export: the manifest frame, then one after-image frame per event.
struct Export {
    sink: Sink,
    events: Vec<(Vec<Op>, Option<String>)>,
}

impl Export {
    fn new(sink: Sink) -> Self {
        Self {
            sink,
            events: Vec::new(),
        }
    }
    fn event(&mut self, ops: Vec<Op>) {
        self.events.push((ops, None));
    }
    /// An event whose delta digest is given (the shared digest refuses its operations).
    fn event_digest(&mut self, ops: Vec<Op>, digest: String) {
        self.events.push((ops, Some(digest)));
    }
    /// Write the export; `head` overrides the manifest's N and `tweak` edits frames.
    fn finish(
        mut self,
        rng: &mut Seeded,
        head: Option<u64>,
        tweak: &dyn Fn(u64, &mut EventFrame, &mut Vec<u8>),
        prefix: Option<String>,
        trailing: &[u8],
        generated_only: bool,
    ) -> Result<(Value, Manifest)> {
        let mut frames = Vec::new();
        let mut digest = GENESIS.to_owned();
        for (i, (mut ops, given)) in self.events.into_iter().enumerate() {
            for (o, op) in ops.iter_mut().enumerate() {
                op.ordinal = u32::try_from(o)?;
            }
            let seq = u64::try_from(i)? + 1;
            let delta = match given {
                Some(d) => d,
                None => delta::digest(&ops).map_err(|r| format!("event {seq}: {r}"))?,
            };
            digest = chain(&digest, seq, &delta);
            frames.push(EventFrame::new(seq, &rng.ulid(), &delta, &ops));
        }
        let manifest = Manifest {
            domain: DOMAIN.into(),
            authority_epoch: "1".into(),
            authority_digest: AUTHORITY_DIGEST.repeat(32),
            accepted_head: head.unwrap_or(frames.len() as u64),
            prefix_digest: prefix.unwrap_or(digest),
            sync_checkpoint: "1".into(),
            contract_schema: SCHEMA_VERSION,
            projection_version: PROJECTION_VERSION,
            engine: ENGINE.into(),
        };
        let mut line = serde_json::to_vec(&manifest)?;
        line.push(b'\n');
        self.sink.put(&line)?;
        let mut sizes = Vec::new();
        for mut frame in frames {
            let mut bytes = serde_jcs::to_vec(&frame)?;
            tweak(frame.graph_seq, &mut frame, &mut bytes);
            sizes.push(bytes.len());
            bytes.push(b'\n');
            self.sink.put(&bytes)?;
        }
        self.sink.put(trailing)?;
        let mut record = self.sink.finish(generated_only)?;
        record["frames"] = json!(sizes.len());
        record["max_frame_bytes"] = json!(sizes.iter().max().copied().unwrap_or(0));
        record["accepted_head"] = json!(manifest.accepted_head);
        record["prefix_digest"] = json!(manifest.prefix_digest);
        Ok((record, manifest))
    }
}

fn no_tweak(_: u64, _: &mut EventFrame, _: &mut Vec<u8>) {}

fn query_frame(source: &str, parameters: &str) -> String {
    // The raw parameter text is embedded as the frame's `parameters` value, exactly as the
    // worker's `QueryRequest` reads it.
    format!(
        "{{\"source\":{},\"parameters\":{parameters}}}\n",
        serde_json::to_string(source).unwrap_or_default()
    )
}

/// What the worker itself answers a query frame with, when the outcome is the profile's:
/// exit 0 (admitted), 75 (`query_limit`) or 74 (`query_unsupported`, or a frame it cannot read).
fn worker_admission(source: &str, parameters: &str) -> (i32, &'static str) {
    let Ok(value) = serde_json::from_str::<Value>(parameters) else {
        return (74, "frame refused: parameters are not JSON");
    };
    let Ok(bytes) = serde_json::to_vec(&value) else {
        return (74, "frame refused");
    };
    match admit(PROFILE, source, &bytes) {
        Ok(_) => (0, "admitted"),
        Err(Refusal::QueryLimit) => (75, "query_limit"),
        Err(_) => (74, "query_unsupported"),
    }
}

struct Corpus {
    dir: PathBuf,
    large: bool,
    rng: Seeded,
    fixtures: BTreeMap<String, Value>,
    inputs: BTreeMap<String, Value>,
    cases: Vec<Value>,
}

impl Corpus {
    fn sink(&self, name: &str, large: bool) -> Result<Sink> {
        Sink::new(&self.dir.join("inputs"), name, !large || self.large)
    }
    fn input(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut s = self.sink(name, false)?;
        s.put(bytes)?;
        self.inputs.insert(name.into(), s.finish(false)?);
        Ok(())
    }
    fn case(&mut self, case: Value) {
        self.cases.push(case);
    }
}

fn oracle(c: &mut Corpus) -> Result<()> {
    let mut a = node(1);
    a["properties"] =
        json!({"score": 1.0, "removed": "gone", "changed": "old", "nil": null, "literal": "1"});
    let mut b = node(2);
    b["kind"] = json!("work:Item");
    b["properties"] = json!({"score": -0.0});
    let mut archived = node(3);
    archived["lifecycle"] = json!("archived");
    let first = vec![
        put(a.clone(), None),
        put(b, None),
        put(archived, None),
        put(tag(4, "research"), None),
        put(edge(11, 1, 2), None),
        put(edge(12, 1, 2), None),
        put(edge(13, 1, 3), None),
        put(membership(14, 1, 4), None),
    ];
    let mut changed = a.clone();
    changed["revision"] = json!("2");
    changed["properties"] = json!({"score": 1, "changed": true, "nil": null, "literal": "1"});
    let second = vec![put(changed, Some(a))];

    let mut e = Export::new(c.sink("oracle.export", false)?);
    e.event(first.clone());
    e.event(second.clone());
    let (record, _) = e.finish(&mut c.rng, None, &no_tweak, None, b"", false)?;
    c.fixtures.insert("oracle".into(), record);
    c.case(
        json!({"id": "oracle_build", "class": "oracle", "role": "build", "stdin": "fixture:oracle",
                  "expect": {"exit": 0, "published": true},
                  "maps_from": "110.1 driver oracle_export + build_export"}),
    );

    // The 22 oracle queries with their exact rows (110.1 driver `oracle`).
    let q: Vec<(&str, Value, Value)> = vec![
        ("MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, e.id AS edge ORDER BY edge LIMIT 10", json!({}), json!([{"node":id(1),"edge":id(11)},{"node":id(1),"edge":id(12)}])),
        ("MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, m.id AS target ORDER BY node DESC LIMIT 1", json!({}), json!([{"node":id(1),"target":id(2)}])),
        ("MATCH (n:Concept)-[e:contains]->(m) RETURN e.id AS edge ORDER BY edge DESC LIMIT 1", json!({}), json!([{"edge":id(12)}])),
        ("MATCH (n:Concept)-[e:contains]->(m:K_776f726b3a4974656d) RETURN e.id AS edge LIMIT 1", json!({}), json!([{"edge":id(11)}])),
        ("MATCH (n)-[e:HAS_TAG]->(t:Tag) WHERE t.text = $tag RETURN n.id AS node, t.id AS tag LIMIT 10", json!({"tag":"research"}), json!([{"node":id(1),"tag":id(4)}])),
        ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", json!({"v":1.0}), json!([{"id":id(1)}])),
        ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", json!({"v":1}), json!([{"id":id(1)}])),
        ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", json!({"v":-0.0}), json!([{"id":id(2)}])),
        ("MATCH (n) WHERE n.P_73636f7265 > $v RETURN n.id AS id LIMIT 10", json!({"v":0.5}), json!([{"id":id(1)}])),
        ("MATCH (n) WHERE n.P_73636f7265 < $v RETURN n.id AS id LIMIT 10", json!({"v":0.5}), json!([{"id":id(2)}])),
        ("MATCH (n) WHERE n.P_73636f7265 <= $v RETURN n.id AS id LIMIT 10", json!({"v":0}), json!([{"id":id(2)}])),
        ("MATCH (n) WHERE n.P_73636f7265 >= $v RETURN n.id AS id LIMIT 10", json!({"v":1.0}), json!([{"id":id(1)}])),
        ("MATCH (n) WHERE n.P_73636f7265 <> $v RETURN n.id AS id LIMIT 10", json!({"v":1}), json!([{"id":id(2)}])),
        ("MATCH (n) WHERE n.P_73636f7265 >= $lo AND n.P_73636f7265 <= $hi RETURN n.id AS id ORDER BY id DESC LIMIT 10", json!({"lo":0,"hi":1}), json!([{"id":id(2)},{"id":id(1)}])),
        ("MATCH (n) WHERE n.P_6c69746572616c = $v RETURN n.id AS id LIMIT 10", json!({"v":1}), json!([])),
        ("MATCH (n) WHERE n.P_6c69746572616c = $v RETURN n.id AS id LIMIT 10", json!({"v":"1"}), json!([{"id":id(1)}])),
        ("MATCH (n:Concept) RETURN n.P_72656d6f766564 AS removed, n.P_6368616e676564 AS changed, n.P_6e696c AS nil, n.P_6c69746572616c AS literal LIMIT 1", json!({}), json!([{"removed":null,"changed":true,"nil":null,"literal":"1"}])),
        ("MATCH (n:K_776f726b3a4974656d) RETURN n.id AS id LIMIT 1", json!({}), json!([{"id":id(2)}])),
        ("MATCH (n)-[e:contains]->(m) RETURN count(e) AS count LIMIT 1", json!({}), json!([{"count":2.0}])),
        ("MATCH (n) RETURN count(*) AS total LIMIT 1", json!({}), json!([{"total":3.0}])),
        ("MATCH (n:Concept) WHERE n.P_72656d6f766564 IS NULL AND n.P_6e696c IS NULL RETURN n.id AS id LIMIT 1", json!({}), json!([{"id":id(1)}])),
        ("MATCH (n:Concept) WHERE n.P_6368616e676564 IS NOT NULL RETURN n.id AS id LIMIT 5", json!({}), json!([{"id":id(1)}])),
    ];
    for (i, (source, params, rows)) in q.into_iter().enumerate() {
        let name = format!("oracle-q{:02}.query", i + 1);
        c.input(&name, query_frame(source, &params.to_string()).as_bytes())?;
        c.case(
            json!({"id": format!("oracle_q{:02}", i + 1), "class": "oracle query", "role": "query",
                      "generation": "oracle", "stdin": format!("input:{name}"),
                      "expect": {"exit": 0, "rows": rows},
                      "maps_from": format!("110.1 driver oracle corpus[{i}] (both clients)")}),
        );
    }
    // F3: a <=64 KiB parameter of all-escaped bytes crosses the frame; one beyond is query_limit.
    let escaped = json!({"s": "\\".repeat(32_000)}).to_string();
    let over = json!({"s": "\\".repeat(32_800)}).to_string();
    let boundary = "MATCH (n:Concept) WHERE n.title = $s RETURN n.id AS id LIMIT 1";
    c.input(
        "oracle-escaped-param.query",
        query_frame(boundary, &escaped).as_bytes(),
    )?;
    c.input(
        "oracle-escaped-param-over.query",
        query_frame(boundary, &over).as_bytes(),
    )?;
    c.case(json!({"id": "f3_escaped_parameter_at_limit", "class": "frame bound", "role": "query", "generation": "oracle",
                  "stdin": "input:oracle-escaped-param.query", "expect": {"exit": 0, "rows": []},
                  "maps_from": "110.1 driver oracle F3 boundary"}));
    c.case(json!({"id": "f3_escaped_parameter_over_limit", "class": "frame bound", "role": "query", "generation": "oracle",
                  "stdin": "input:oracle-escaped-param-over.query", "expect": {"exit": 75, "stdout_bytes": 0},
                  "maps_from": "110.1 driver oracle F3 over (parent refused query_limit; here the worker's own admission refuses)"}));
    for (i, source) in [
        "MATCH (n) RETURN n.id AS id LIMIT 1; MATCH (m) DETACH DELETE m",
        "LOAD CSV FROM 'file:///etc/passwd' AS r RETURN r LIMIT 1",
    ]
    .iter()
    .enumerate()
    {
        let name = format!("oracle-hostile-{}.query", i + 1);
        c.input(&name, query_frame(source, "{}").as_bytes())?;
        c.case(json!({"id": format!("hostile_{}", i + 1), "class": "grammar refusal", "role": "query", "generation": "oracle",
                      "stdin": format!("input:{name}"), "expect": {"exit": 74, "stdout_bytes": 0},
                      "maps_from": format!("110.1 driver oracle hostile[{i}]")}));
    }
    // Typed inspection (every role is exercised: build, query and inspect).
    let inspections = [
        (
            "inspect-node1",
            InspectEntity::Node,
            id(1),
            false,
            json!({"rows": 1, "properties": {"score": 1.0, "changed": true, "nil": null, "literal": "1"}, "tags": ["research"]}),
            "110.1 driver oracle inspect id(1): typed null kept, removed key gone",
        ),
        (
            "inspect-archived-hidden",
            InspectEntity::Node,
            id(3),
            false,
            json!({"rows": 0}),
            "110.1 driver oracle inspect id(3) include_archived=false",
        ),
        (
            "inspect-archived-shown",
            InspectEntity::Node,
            id(3),
            true,
            json!({"rows": 1, "lifecycle": "archived"}),
            "110.1 driver oracle inspect id(3) include_archived=true",
        ),
        (
            "inspect-edge-to-archived",
            InspectEntity::Edge,
            id(13),
            false,
            json!({"rows": 0}),
            "110.5 addition: edge inspection hides an edge whose endpoint is archived",
        ),
        (
            "inspect-edge-active",
            InspectEntity::Edge,
            id(11),
            false,
            json!({"rows": 1}),
            "110.5 addition: typed edge inspection",
        ),
    ];
    for (name, entity, eid, include_archived, expect, from) in inspections {
        let frame = InspectRequest {
            entity,
            id: eid,
            include_archived,
        }
        .encode()?;
        let file = format!("{name}.inspect");
        c.input(&file, &frame)?;
        c.case(
            json!({"id": name, "class": "inspect", "role": "inspect", "generation": "oracle",
                      "stdin": format!("input:{file}"), "expect": {"exit": 0, "answer": expect},
                      "maps_from": from}),
        );
    }
    // The count query and inspection frame the B0 (preload) cases feed every role.
    c.input(
        "count.query",
        query_frame("MATCH (n:Concept) RETURN count(*) AS total LIMIT 1", "{}").as_bytes(),
    )?;

    // Publication faults: each export is refused by the builder and publishes nothing.
    let a1 = node(1);
    let mut a2 = a1.clone();
    a2["revision"] = json!("2");
    let faults: Vec<(&str, &str)> = vec![
        ("truncated", "110.1 fault truncated: the manifest names 2 events, the export holds 1"),
        ("chain", "110.1 fault digest: the event's digest differs, so the chained prefix digest no longer ends at the manifest's (110.5 frames carry no before-image; the 110.1 before-image fault is the parent's, keeper-core `a_partial_or_corrupt_history_is_stale_or_refused`)"),
        ("dangling", "110.1 fault dangling: an edge to an entity no event created"),
        ("gap", "110.1 fault gap: event 3 follows event 1"),
        ("trailing", "110.1 fault trailing: bytes after the last frame"),
        ("type-change", "110.5 addition: an entity id reused with another entity type"),
    ];
    for (kind, from) in faults {
        let name = format!("fault-{kind}.export");
        let mut e = Export::new(c.sink(&name, false)?);
        e.event(vec![put(a1.clone(), None)]);
        let mut head = None;
        let mut trailing: &[u8] = b"";
        let mut prefix = None;
        match kind {
            "truncated" => head = Some(2),
            "chain" => {
                e.event(vec![put(a2.clone(), Some(a1.clone()))]);
                prefix = Some("0".repeat(63) + "1");
            }
            "dangling" => e.event(vec![put(edge(20, 1, 99), None)]),
            "gap" => e.event(vec![put(a2.clone(), Some(a1.clone()))]),
            "trailing" => {
                e.event(vec![put(a2.clone(), Some(a1.clone()))]);
                trailing = b"{\"extra\":true}\n";
            }
            _ => e.event(vec![put(tag(1, "clash"), None)]),
        }
        let gap = |seq: u64, f: &mut EventFrame, b: &mut Vec<u8>| {
            if seq == 2 {
                f.graph_seq = 3;
                *b = serde_jcs::to_vec(f).unwrap_or_default();
            }
        };
        let tweak: &dyn Fn(u64, &mut EventFrame, &mut Vec<u8>) =
            if kind == "gap" { &gap } else { &no_tweak };
        let (record, _) = e.finish(&mut c.rng, head, tweak, prefix, trailing, false)?;
        c.inputs.insert(name.clone(), record);
        c.case(json!({"id": format!("fault_{kind}"), "class": "publication fault", "role": "build",
                      "stdin": format!("input:{name}"), "expect": {"exit": 74, "published": false, "stdout_bytes": 0},
                      "maps_from": from}));
    }
    Ok(())
}

/// At-cap values, the R6 premise unchanged: each pair is admitted at the cap and refused one
/// past it (exit 74, nothing published).
fn values(c: &mut Corpus) -> Result<()> {
    let caps: Vec<(&str, Value, bool, &str)> = {
        let mut wide_ok = serde_json::Map::new();
        let mut wide_over = serde_json::Map::new();
        // Short distinct keys (printable ASCII without `"` and `\`, one then two
        // characters), so 8,192 entries stay inside the 64 KiB properties bound and only
        // the entry count decides.
        let keys: Vec<String> = {
            let alphabet: Vec<char> = (0x21u8..0x7f)
                .map(char::from)
                .filter(|c| *c != '"' && *c != '\\')
                .collect();
            let mut k: Vec<String> = alphabet.iter().map(|c| c.to_string()).collect();
            for a in &alphabet {
                for b in &alphabet {
                    k.push(format!("{a}{b}"));
                }
            }
            k
        };
        for key in keys.iter().take(8192) {
            wide_ok.insert(key.clone(), json!(0));
        }
        for key in keys.iter().take(8193) {
            wide_over.insert(key.clone(), json!(0));
        }
        let nest = |levels: usize| {
            let mut v = json!(0);
            for _ in 0..levels {
                v = json!([v]);
            }
            v
        };
        vec![
            (
                "string-65536",
                json!({"title": "x".repeat(65_536)}),
                true,
                "string at 65,536 bytes",
            ),
            (
                "string-65537",
                json!({"title": "x".repeat(65_537)}),
                false,
                "string at 65,537 bytes",
            ),
            (
                "array-8192",
                json!({"properties": {"a": vec![0; 8192]}}),
                true,
                "array at 8,192 entries",
            ),
            (
                "array-8193",
                json!({"properties": {"a": vec![0; 8193]}}),
                false,
                "array at 8,193 entries",
            ),
            (
                "object-8192",
                json!({"properties": {"o": Value::Object(wide_ok)}}),
                true,
                "object at 8,192 keys",
            ),
            (
                "object-8193",
                json!({"properties": {"o": Value::Object(wide_over)}}),
                false,
                "object at 8,193 keys",
            ),
            (
                "key-8192",
                json!({"properties": {"k": {"k".repeat(8192): 0}}}),
                true,
                "key at 8,192 bytes",
            ),
            (
                "key-8193",
                json!({"properties": {"k": {"k".repeat(8193): 0}}}),
                false,
                "key at 8,193 bytes",
            ),
            (
                "number-max",
                json!({"properties": {"n": 9_007_199_254_740_991u64}}),
                true,
                "number 2^53-1",
            ),
            (
                "number-over",
                json!({"properties": {"n": 9_007_199_254_740_992u64}}),
                false,
                "number 2^53",
            ),
            (
                "nesting-8",
                json!({"properties": {"d": nest(7)}}),
                true,
                "property nesting 8 (properties object counted)",
            ),
            (
                "nesting-9",
                json!({"properties": {"d": nest(9)}}),
                false,
                "property nesting past 8",
            ),
            (
                "image-128k",
                json!({"title": "t".repeat(65_536), "description": "d".repeat(65_000)}),
                true,
                "after-image within 128 KiB",
            ),
            (
                "image-over-128k",
                json!({"title": "t".repeat(65_536), "description": "d".repeat(65_536)}),
                false,
                "after-image over 128 KiB",
            ),
        ]
    };
    for (name, patch, ok, what) in caps {
        let mut image = node(1);
        for (k, v) in patch.as_object().cloned().unwrap_or_default() {
            image[k] = v;
        }
        let file = format!("value-{name}.export");
        let mut e = Export::new(c.sink(&file, false)?);
        let ops = vec![put(image, None)];
        match delta::digest(&ops) {
            Ok(_) => e.event(ops),
            Err(_) => {
                let digest = sha256(&serde_json::to_vec(&ops)?);
                e.event_digest(ops, digest);
            }
        }
        let (record, _) = e.finish(&mut c.rng, None, &no_tweak, None, b"", false)?;
        c.inputs.insert(file.clone(), record);
        c.case(json!({"id": format!("value_{name}"), "class": "at-cap value", "role": "build",
                      "stdin": format!("input:{file}"),
                      "expect": if ok { json!({"exit": 0, "published": true}) } else { json!({"exit": 74, "published": false, "stdout_bytes": 0}) },
                      "maps_from": format!("110.1 worker validate_value / validate_entity: {what}")}));
    }
    Ok(())
}

/// Frames at the 2 MiB event limit in both roles: the 110.1 adversarial frames (refused),
/// plus valid near-limit falsifiers (admitted) and the inspect frame bound.
fn frames(c: &mut Corpus) -> Result<()> {
    let limit = MAX_EVENT_BYTES - 1; // 2,097,151 content bytes + newline
                                     // Adversarial build frame: one node whose after-image carries ~524k one-byte strings,
                                     // padded with JSON whitespace to exactly the limit.
    let pad_to = |bytes: &mut Vec<u8>, size: usize| {
        let tail = bytes.split_off(bytes.len() - 1);
        bytes.resize(size - tail.len(), b' ');
        bytes.extend_from_slice(&tail);
    };
    let skeleton = {
        let mut image = node(1);
        image["properties"] = json!({"p": []});
        let ops = vec![put(image, None)];
        serde_jcs::to_vec(&EventFrame::new(1, &id(9), &"0".repeat(64), &ops))?.len()
    };
    let strings = (limit - skeleton) / 4;
    let mut image = node(1);
    image["properties"] = json!({"p": vec!["a"; strings]});
    let file = "frame-adversarial.export";
    let mut e = Export::new(c.sink(file, false)?);
    let ops = vec![put(image, None)];
    let digest = sha256(&serde_json::to_vec(&ops)?);
    e.event_digest(ops, digest);
    let pad = move |_: u64, _: &mut EventFrame, b: &mut Vec<u8>| pad_to(b, limit);
    let (record, _) = e.finish(&mut c.rng, None, &pad, None, b"", false)?;
    c.inputs.insert(file.into(), record);
    c.case(json!({"id": "frame_adversarial_build_2097151", "class": "2 MiB frame", "role": "build",
                  "stdin": format!("input:{file}"), "one_byte_strings": strings,
                  "expect": {"exit": 74, "published": false, "stdout_bytes": 0},
                  "maps_from": "110.1 graph-proof adversarial_frames_at_event_limit_both_roles (build). 110.1 exit 75 (its validate_value returned query_limit); 110.5's restored validate_value refuses the same frame with a plain error, exit 74. Same parse premise: the whole frame is parsed before the value admission"}));
    // Valid near-limit build frame: ~30 nodes of ~66 KB text, the frame just under the limit.
    let mut ops = Vec::new();
    let mut n = 100;
    loop {
        let mut image = node(n);
        image["description"] = json!("v".repeat(65_000));
        ops.push(put(image, None));
        let size = serde_jcs::to_vec(&EventFrame::new(1, &id(9), &"0".repeat(64), &ops))?.len();
        if size > limit - 70_000 {
            break;
        }
        n += 1;
    }
    let file = "frame-valid-near-limit.export";
    let mut e = Export::new(c.sink(file, false)?);
    e.event(ops);
    let pad = move |_: u64, _: &mut EventFrame, b: &mut Vec<u8>| pad_to(b, limit);
    let (record, _) = e.finish(&mut c.rng, None, &pad, None, b"", false)?;
    c.inputs.insert(file.into(), record);
    c.case(
        json!({"id": "frame_valid_build_2097151", "class": "2 MiB frame", "role": "build",
                  "stdin": format!("input:{file}"), "expect": {"exit": 0, "published": true},
                  "maps_from": "R6 review: valid near-frame-limit falsifier (build role)"}),
    );
    // Query role: the 110.1 adversarial frame, refused by the 81,984-byte query bound.
    let mut frame = b"{\"source\":\"".to_vec();
    frame.resize(limit - 2, b'a');
    frame.extend_from_slice(b"\"}\n");
    c.input("frame-adversarial.query", &frame)?;
    c.case(json!({"id": "frame_adversarial_query_2097151", "class": "2 MiB frame", "role": "query", "generation": "oracle",
                  "stdin": "input:frame-adversarial.query", "expect": {"exit": 74, "stdout_bytes": 0},
                  "maps_from": "110.1 graph-proof adversarial_frames_at_event_limit_both_roles (query)"}));
    // Valid near-limit query frame: source 8 KiB and parameters 64 KiB, admitted.
    let base = "MATCH (n) WHERE n.title = $p RETURN n.id AS id LIMIT 1";
    let source = format!("{base}{}", " ".repeat(8192 - base.len()));
    let params = format!("{{\"p\":\"{}\"}}", "x".repeat(MAX_PARAMETER_BYTES - 8));
    c.input(
        "frame-valid-near-limit.query",
        query_frame(&source, &params).as_bytes(),
    )?;
    c.case(json!({"id": "frame_valid_query_at_bounds", "class": "2 MiB frame", "role": "query", "generation": "oracle",
                  "stdin": "input:frame-valid-near-limit.query", "expect": {"exit": 0, "rows": []},
                  "maps_from": "R6 review: valid near-limit falsifier (query role; source 8 KiB, parameters 64 KiB)"}));
    // Inspect role: the 8 KiB inspect frame bound.
    let inspect = |padding: usize| -> Vec<u8> {
        let mut f = format!(
            "{{\"entity\":\"node\",\"id\":\"{}\",\"include_archived\":false",
            id(1)
        )
        .into_bytes();
        f.resize(f.len() + padding, b' ');
        f.extend_from_slice(b"}\n");
        f
    };
    let base_len = inspect(0).len() - 1;
    c.input("frame-valid-near-limit.inspect", &inspect(8192 - base_len))?;
    c.input("frame-over-limit.inspect", &inspect(8193 - base_len))?;
    c.case(json!({"id": "frame_valid_inspect_8192", "class": "2 MiB frame", "role": "inspect", "generation": "oracle",
                  "stdin": "input:frame-valid-near-limit.inspect", "expect": {"exit": 0, "answer": {"rows": 1}},
                  "maps_from": "R6 review: valid near-limit falsifier (inspect role, 8 KiB frame)"}));
    c.case(json!({"id": "frame_over_inspect_8193", "class": "2 MiB frame", "role": "inspect", "generation": "oracle",
                  "stdin": "input:frame-over-limit.inspect", "expect": {"exit": 74, "stdout_bytes": 0},
                  "maps_from": "R6 review: malformed near-limit falsifier (inspect role)"}));
    Ok(())
}

/// Depth and containment: a 64-level `contains` chain and the 8-segment traversal bound.
fn containment(c: &mut Corpus) -> Result<()> {
    let mut ops: Vec<Op> = (0..=64).map(|i| put(node(1000 + i), None)).collect();
    ops.extend((0..64).map(|i| put(edge(2000 + i, 1000 + i, 1001 + i), None)));
    let file = "containment-64.export";
    let mut e = Export::new(c.sink(file, false)?);
    for chunk in ops.chunks(100) {
        e.event(chunk.to_vec());
    }
    let (record, _) = e.finish(&mut c.rng, None, &no_tweak, None, b"", false)?;
    c.fixtures.insert("containment".into(), record);
    c.case(json!({"id": "containment_build", "class": "depth/containment", "role": "build", "stdin": "fixture:containment",
                  "expect": {"exit": 0, "published": true}, "maps_from": "110.1 containment depth class (64 levels)"}));
    let eight = (0..8)
        .map(|i| format!("-[:contains]->(n{})", i + 1))
        .collect::<String>();
    let nine = (0..9)
        .map(|i| format!("-[:contains]->(n{})", i + 1))
        .collect::<String>();
    for (name, source, expect, from) in [
        (
            "containment-8hop.query",
            format!("MATCH (n0){eight} RETURN count(*) AS paths LIMIT 1"),
            json!({"exit": 0, "rows": [{"paths": 57.0}]}),
            "8 fixed segments: 57 paths in a 65-node chain",
        ),
        (
            "containment-9hop.query",
            format!("MATCH (n0){nine} RETURN count(*) AS paths LIMIT 1"),
            json!({"exit": 75, "stdout_bytes": 0}),
            "9 segments: query_limit",
        ),
    ] {
        c.input(name, query_frame(&source, "{}").as_bytes())?;
        c.case(json!({"id": name.trim_end_matches(".query").replace('-', "_"), "class": "depth/containment", "role": "query",
                      "generation": "containment", "stdin": format!("input:{name}"), "expect": expect,
                      "maps_from": format!("110.1 profile segment bound: {from}")}));
    }
    Ok(())
}

/// The 110.1 limits fixture (hub 1 -> 10,000 leaves, 1,000 nodes with a 60 KiB text) and
/// its work-limit cases.
fn limits(c: &mut Corpus) -> Result<()> {
    let mut ops = Vec::new();
    let mut hub = node(1);
    hub["properties"] = json!({});
    ops.push(put(hub, None));
    ops.extend((2..=10_001u64).map(|n| put(node(n), None)));
    ops.extend((0..10_000u64).map(|n| put(edge(20_000 + n, 1, 2 + n), None)));
    let blob = "x".repeat(60 * 1024);
    let blobs: Vec<Op> = (0..1000u64)
        .map(|n| {
            let mut b = node(40_000 + n);
            b["properties"] = json!({"blob": blob});
            put(b, None)
        })
        .collect();
    let mut e = Export::new(c.sink("limits.export", true)?);
    for chunk in ops.chunks(100) {
        e.event(chunk.to_vec());
    }
    for chunk in blobs.chunks(30) {
        e.event(chunk.to_vec());
    }
    let (mut record, _) = e.finish(&mut c.rng, None, &no_tweak, None, b"", !c.large)?;
    record["regenerate"] =
        json!("cargo run -p keeper-graph-protocol --example rc_corpus -- <out> --large");
    c.fixtures.insert("limits".into(), record);
    c.case(json!({"id": "limits_build", "class": "work limit", "role": "build", "stdin": "fixture:limits",
                  "expect": {"exit": 0, "published": true}, "maps_from": "110.1 driver limits_export"}));
    let hub = json!({"id": id(1)}).to_string();
    let wide = (0..16)
        .map(|i| format!("n.P_626c6f62 AS b{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let cases = [
        ("limits-control.query", "MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN count(*) AS total LIMIT 1".to_owned(), hub.clone(),
         json!({"exit": 0, "rows": [{"total": 10000.0}]}), "110.1 limits control"),
        ("limits-materialization.query", format!("MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN {wide} LIMIT 1000"), "{}".to_owned(),
         json!({"exit": 72, "stdout_bytes": 0}), "110.1 native_materialization_budget (charged budget R exhausted)"),
        ("limits-result-cap.query", "MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 100".to_owned(), "{}".to_owned(),
         json!({"exit": 75, "stdout_bytes": 0}), "110.1 encoded_result_cap"),
        ("limits-deadline.query", "MATCH (x)<-[e1:contains]-(h:Concept)-[e2:contains]->(y)<-[e3:contains]-(z) WHERE h.id = $id RETURN count(*) AS total LIMIT 1".to_owned(), hub,
         json!({"outcome": "query_limit", "by": "the supervisor's 3 s deadline (the worker is killed and reaped; run through the spawner or the supervisor, not standalone)"}), "110.1 wall_deadline"),
    ];
    for (name, source, params, expect, from) in cases {
        c.input(name, query_frame(&source, &params).as_bytes())?;
        c.case(json!({"id": name.trim_end_matches(".query").replace('-', "_"), "class": "work limit", "role": "query",
                      "generation": "limits", "stdin": format!("input:{name}"), "expect": expect, "maps_from": from}));
    }
    Ok(())
}

/// The 110.1 benchmark fixture: 100k nodes, 300k edges, 10,000 events with 2,000
/// update/archive/tag changes, and its four timed queries.
fn benchmark(c: &mut Corpus) -> Result<()> {
    let mut e = Export::new(c.sink("benchmark.export", true)?);
    let mut ops = Vec::with_capacity(50);
    for n in 1..=400_000u64 {
        let op = if n <= 99_998 {
            put(node(n), None)
        } else if n <= 100_000 {
            put(tag(n, &format!("tag {n}")), None)
        } else if n <= 399_998 {
            let j = n - 100_000;
            let from = if j <= 10_000 { 1 } else { j % 99_998 + 1 };
            let to = (j * 17) % 99_998 + 1;
            put(edge(n, from, to), None)
        } else {
            put(membership(n, 1, n - 300_000), None)
        };
        ops.push(op);
        if ops.len() == 50 {
            e.event(std::mem::replace(&mut ops, Vec::with_capacity(50)));
        }
    }
    let mut first = node(1);
    let mut second = node(2);
    let held = membership(399_999, 1, 99_999);
    let mut tagged = true;
    for i in 0..2000u64 {
        let op = match i % 4 {
            0 => {
                let before = first.clone();
                first["title"] = json!(format!("updated {i}"));
                first["revision"] = json!((i + 2).to_string());
                put(first.clone(), Some(before))
            }
            1 | 2 => {
                let before = second.clone();
                second["lifecycle"] = json!(if i % 4 == 1 { "archived" } else { "active" });
                second["revision"] = json!((i + 2).to_string());
                second["lifecycle_revision"] = json!((i + 2).to_string());
                put(second.clone(), Some(before))
            }
            _ => {
                let op = if tagged {
                    remove(held.clone())
                } else {
                    put(held.clone(), None)
                };
                tagged = !tagged;
                op
            }
        };
        e.event(vec![op]);
    }
    let (mut record, _) = e.finish(&mut c.rng, None, &no_tweak, None, b"", !c.large)?;
    record["regenerate"] =
        json!("cargo run -p keeper-graph-protocol --example rc_corpus -- <out> --large");
    c.fixtures.insert("benchmark".into(), record);
    c.case(json!({"id": "benchmark_build", "class": "benchmark", "role": "build", "stdin": "fixture:benchmark",
                  "expect": {"exit": 0, "published": true}, "maps_from": "110.1 driver benchmark_export (cold generation)"}));
    let node_p = json!({"id": id(1)}).to_string();
    let tag_p = json!({"tag": id(99_999)}).to_string();
    for (name, source, params, target_us) in [
        ("bench-node-lookup.query", "MATCH (n:Concept) WHERE n.id = $id RETURN n.id AS id, n.title AS title LIMIT 1", &node_p, 250_000),
        ("bench-tag-lookup.query", "MATCH (n:Concept)-[e:HAS_TAG]->(t:Tag) WHERE t.id = $tag RETURN n.id AS id LIMIT 10", &tag_p, 250_000),
        ("bench-traversal-1hop.query", "MATCH (n:Concept)-[e:contains]->(m) WHERE n.id = $id RETURN e.id AS edge, m.id AS target ORDER BY edge LIMIT 10", &node_p, 250_000),
        ("bench-traversal-2hop.query", "MATCH (a:Concept)-[e1:contains]->(b)-[e2:contains]->(c) WHERE a.id = $id RETURN c.id AS id ORDER BY id LIMIT 10", &node_p, 1_000_000),
    ] {
        c.input(name, query_frame(source, params).as_bytes())?;
        c.case(json!({"id": name.trim_end_matches(".query").replace('-', "_"), "class": "benchmark", "role": "query",
                      "generation": "benchmark", "stdin": format!("input:{name}"), "iterations": 45,
                      "expect": {"exit": 0, "p95_us_at_most": target_us},
                      "maps_from": "110.1 driver benchmark queries (45 iterations, p95 target)"}));
    }
    Ok(())
}

/// The shared `queries[]` vectors, run by the worker against the oracle generation. The
/// worker's own outcome is pinned (its frame carries parameters as a JSON value, so raw-byte
/// refusals that only the parent sees are recorded as such).
fn vectors(c: &mut Corpus) -> Result<()> {
    let doc: Value = serde_json::from_str(VECTORS)?;
    let mut parent_only = Vec::new();
    for v in doc["queries"].as_array().ok_or("queries")? {
        let vid = v["id"].as_str().ok_or("id")?;
        let mut source = v["source"].as_str().ok_or("source")?.to_owned();
        let mut params = v["parameters"].as_str().ok_or("parameters")?.to_owned();
        if let Some(fill) = v.get("fill") {
            let text = fill["text"]
                .as_str()
                .unwrap_or_default()
                .repeat(fill["count"].as_u64().unwrap_or(0) as usize);
            if source.contains("{FILL}") {
                source = source.replacen("{FILL}", &text, 1);
            } else {
                params = params.replacen("{FILL}", &text, 1);
            }
        }
        if v.get("profile").is_some() {
            parent_only.push(json!({"vector": vid, "why": "the profile name is the parent's input; the worker always admits KG-Cypher/1"}));
            continue;
        }
        let (exit, outcome) = worker_admission(&source, &params);
        let vector = v
            .get("refusal")
            .and_then(Value::as_str)
            .unwrap_or("admitted");
        let frame = if serde_json::from_str::<Value>(&params).is_ok_and(|p| p.is_object()) {
            query_frame(&source, &params)
        } else {
            // Not a JSON object: carried as a string, which the worker refuses as a frame.
            query_frame(&source, &serde_json::to_string(&params)?)
        };
        let name = format!("vector-{vid}.query");
        c.input(&name, frame.as_bytes())?;
        let mut case = json!({"id": format!("vector_{}", vid.replace('-', "_")), "class": "query vector", "role": "query",
                              "generation": "oracle", "stdin": format!("input:{name}"),
                              "expect": if exit == 0 { json!({"exit": 0}) } else { json!({"exit": exit, "stdout_bytes": 0}) },
                              "vector_outcome": vector, "worker_outcome": outcome,
                              "maps_from": format!("syncular/knowledge-graph.vectors.json queries[{vid}]")});
        if (vector == "admitted") != (exit == 0) || (vector != "admitted" && vector != outcome) {
            case["note"] = json!("parent-level refusal: the raw parameter bytes are refused by the parent; the worker reads parameters as one JSON value");
        }
        c.case(case);
    }
    c.fixtures
        .insert("_vector_parent_only".into(), Value::Array(parent_only));
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("usage: rc_corpus <out-dir> [--large]")?);
    let large = args.any(|a| a == "--large");
    std::fs::create_dir_all(dir.join("inputs"))?;
    let mut c = Corpus {
        dir: dir.clone(),
        large,
        rng: Seeded(SEED),
        fixtures: BTreeMap::new(),
        inputs: BTreeMap::new(),
        cases: Vec::new(),
    };
    oracle(&mut c)?;
    values(&mut c)?;
    frames(&mut c)?;
    containment(&mut c)?;
    limits(&mut c)?;
    benchmark(&mut c)?;
    vectors(&mut c)?;
    let parent_only = c
        .fixtures
        .remove("_vector_parent_only")
        .unwrap_or(Value::Null);
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for case in &c.cases {
        *classes
            .entry(case["class"].as_str().unwrap_or("").to_owned())
            .or_default() += 1;
    }
    let corpus = json!({
        "schema": "kg1-1105-r6-corpus/1",
        "generator": "cargo run -p keeper-graph-protocol --example rc_corpus -- <out-dir> [--large]",
        "seed": format!("{SEED:#018x}"),
        "worker_protocol": {"engine": ENGINE, "projection_version": PROJECTION_VERSION, "contract_schema": SCHEMA_VERSION,
                            "max_event_bytes": MAX_EVENT_BYTES, "domain": DOMAIN, "authority_epoch": "1",
                            "authority_digest": AUTHORITY_DIGEST.repeat(32)},
        "runner_contract": {
            "build": "worker build <fresh staging file> < fixture or input; exit and whether _kg_manifest exists afterwards (published)",
            "query": "worker query <generation> < input; generation = the published file of the named fixture's build case",
            "inspect": "worker inspect <generation> < input",
            "environment": "exactly GQL_STMT_CACHE=0 and GLIBC_TUNABLES=<worker.linux_glibc_tunables>; no_new_privs set",
            "rows": "expect.rows compares the response's rows array exactly (numbers as JSON numbers)",
            "answer": "inspect: rows = number of rows; other keys compare the first row's image fields or tags"
        },
        "coverage_classes": classes,
        "fixtures": c.fixtures,
        "inputs": c.inputs,
        "cases": c.cases,
        "vectors_parent_only": parent_only,
    });
    let bytes = serde_json::to_vec_pretty(&corpus)?;
    std::fs::write(dir.join("corpus.json"), &bytes)?;
    let digest = sha256(&bytes);
    std::fs::write(
        dir.join("corpus.json.sha256"),
        format!("{digest}  corpus.json\n"),
    )?;
    println!(
        "corpus.json {digest} cases={} inputs={}",
        c.cases.len(),
        c.inputs.len()
    );
    Ok(())
}
