#[cfg(any(target_os = "linux", target_os = "macos"))]
mod proof {
    use keeper_core::graph::{Cancellation, Supervisor};
    use keeper_graph_protocol::wire::{
        operation_digest, Change, EntityType, Event, Manifest, Operation, ENGINE,
    };
    use serde_json::{json, Value};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::Instant;
    type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;
    fn id(n: u64) -> String {
        format!("{n:026}")
    }
    fn node(n: u64) -> Value {
        json!({"id":id(n),"kind":"Concept","title":format!("node {n}"),"description":null,"lifecycle":"active","lifecycle_revision":"1","revision":"1","properties":{"score":1.0}})
    }
    fn edge(n: u64, from: u64, to: u64) -> Value {
        json!({"id":id(n),"from_id":id(from),"to_id":id(to),"type":"contains","lifecycle":"active","revision":"1","properties":{}})
    }
    fn put(kind: EntityType, after: Value, before: Option<Value>) -> Operation {
        Operation {
            ordinal: 0,
            entity_type: kind,
            entity_id: after["id"].as_str().unwrap_or_default().to_owned(),
            operation: Change::Put,
            before,
            after: Some(after),
        }
    }
    fn event(
        file: &mut impl Write,
        sequence: u64,
        mut operations: Vec<Operation>,
        sizes: &mut Vec<usize>,
    ) -> Result<()> {
        for (i, op) in operations.iter_mut().enumerate() {
            op.ordinal = u32::try_from(i)?;
        }
        let digest = operation_digest(&operations)?;
        let value = Event {
            sequence,
            operations,
            digest,
        };
        let bytes = serde_json::to_vec(&value)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        sizes.push(bytes.len());
        Ok(())
    }
    fn manifest(domain: &str, head: u64) -> Manifest {
        Manifest {
            domain: domain.into(),
            authority_epoch: "1".into(),
            accepted_head: head,
            sync_checkpoint: head.to_string(),
            schema_version: 1,
            projection_version: 1,
            engine: ENGINE.into(),
        }
    }
    fn header(file: &mut impl Write, value: &Manifest) -> Result<()> {
        serde_json::to_writer(&mut *file, value)?;
        file.write_all(b"\n")?;
        Ok(())
    }
    fn oracle_export(path: &Path, domain: &str) -> Result<Manifest> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let m = manifest(domain, 2);
        header(&mut file, &m)?;
        let mut a = node(1);
        a["properties"] =
            json!({"score":1.0,"removed":"gone","changed":"old","nil":null,"literal":"1"});
        let mut b = node(2);
        b["kind"] = json!("work:Item");
        b["properties"] = json!({"score":-0.0});
        let mut archived = node(3);
        archived["lifecycle"] = json!("archived");
        let tag = json!({"id":id(4),"text":"research"});
        let membership = json!({"id":id(14),"node_id":id(1),"tag_id":id(4)});
        let mut sizes = Vec::new();
        event(
            &mut file,
            1,
            vec![
                put(EntityType::Node, a.clone(), None),
                put(EntityType::Node, b, None),
                put(EntityType::Node, archived, None),
                put(EntityType::Tag, tag, None),
                put(EntityType::Edge, edge(11, 1, 2), None),
                put(EntityType::Edge, edge(12, 1, 2), None),
                put(EntityType::Edge, edge(13, 1, 3), None),
                put(EntityType::NodeTag, membership, None),
            ],
            &mut sizes,
        )?;
        let mut changed = a.clone();
        changed["revision"] = json!("2");
        changed["properties"] = json!({"score":1,"changed":true,"nil":null,"literal":"1"});
        event(
            &mut file,
            2,
            vec![put(EntityType::Node, changed, Some(a))],
            &mut sizes,
        )?;
        file.flush()?;
        Ok(m)
    }
    fn benchmark_export(path: &Path, domain: &str) -> Result<(Manifest, Vec<usize>)> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let m = manifest(domain, 10000);
        header(&mut file, &m)?;
        let mut sizes = Vec::with_capacity(10000);
        let mut ops = Vec::with_capacity(50);
        let mut sequence = 0;
        for n in 1..=400000u64 {
            let op = if n <= 99998 {
                put(EntityType::Node, node(n), None)
            } else if n <= 100000 {
                put(
                    EntityType::Tag,
                    json!({"id":id(n),"text":format!("tag {n}")}),
                    None,
                )
            } else if n <= 399998 {
                let j = n - 100000;
                let from = if j <= 10000 { 1 } else { j % 99998 + 1 };
                let to = (j * 17) % 99998 + 1;
                put(EntityType::Edge, edge(n, from, to), None)
            } else {
                put(
                    EntityType::NodeTag,
                    json!({"id":id(n),"node_id":id(1),"tag_id":id(n-300000)}),
                    None,
                )
            };
            ops.push(op);
            if ops.len() == 50 {
                sequence += 1;
                event(
                    &mut file,
                    sequence,
                    std::mem::replace(&mut ops, Vec::with_capacity(50)),
                    &mut sizes,
                )?;
            }
        }
        let mut first = node(1);
        let mut second = node(2);
        let membership = json!({"id":id(399999),"node_id":id(1),"tag_id":id(99999)});
        let mut tagged = true;
        for i in 0..2000u64 {
            let op = match i % 4 {
                0 => {
                    let before = first.clone();
                    first["title"] = json!(format!("updated {i}"));
                    first["revision"] = json!((i + 2).to_string());
                    put(EntityType::Node, first.clone(), Some(before))
                }
                1 | 2 => {
                    let before = second.clone();
                    second["lifecycle"] = json!(if i % 4 == 1 { "archived" } else { "active" });
                    second["revision"] = json!((i + 2).to_string());
                    second["lifecycle_revision"] = json!((i + 2).to_string());
                    put(EntityType::Node, second.clone(), Some(before))
                }
                _ => {
                    let op = if tagged {
                        Operation {
                            ordinal: 0,
                            entity_type: EntityType::NodeTag,
                            entity_id: id(399999),
                            operation: Change::Remove,
                            before: Some(membership.clone()),
                            after: None,
                        }
                    } else {
                        put(EntityType::NodeTag, membership.clone(), None)
                    };
                    tagged = !tagged;
                    op
                }
            };
            event(&mut file, 8001 + i, vec![op], &mut sizes)?;
        }
        file.flush()?;
        Ok((m, sizes))
    }
    fn percentile(values: &mut [u128]) -> u128 {
        values.sort_unstable();
        values[(values.len() * 95).div_ceil(100).saturating_sub(1)]
    }
    pub async fn run() -> Result<()> {
        let args: Vec<String> = std::env::args().collect();
        let mode = args.get(1).ok_or("oracle or benchmark required")?;
        if mode != "oracle" && mode != "benchmark" {
            return Err("unknown proof mode".into());
        }
        let worker = PathBuf::from(args.get(2).ok_or("worker executable required")?);
        let root = PathBuf::from(args.get(3).ok_or("owned scratch directory required")?);
        if root.exists() {
            return Err("proof scratch must be new".into());
        }
        std::fs::create_dir_all(&root)?;
        let supervisor = Supervisor::new(&worker, &root.join("generations"), true)?;
        for domain in ["fixture-personal", "fixture-shared"] {
            let export = root.join(format!("{domain}.ndjson"));
            let start = Instant::now();
            let (m, sizes) = if mode == "benchmark" {
                benchmark_export(&export, domain)?
            } else {
                (oracle_export(&export, domain)?, Vec::new())
            };
            let export_ms = start.elapsed().as_millis();
            let start = Instant::now();
            let generation = supervisor.build_export(&export, &m).await?;
            let build_ms = start.elapsed().as_millis();
            let first = supervisor.session("client-a", "context-a", domain)?;
            let second = supervisor.session("client-b", "context-b", domain)?;
            let cancel = Cancellation::default();
            if mode == "oracle" {
                let corpus=[
                ("MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, e.id AS edge ORDER BY edge LIMIT 10","{}".to_owned(),json!([{"node":id(1),"edge":id(11)},{"node":id(1),"edge":id(12)}])),
                ("MATCH (n)-[e:HAS_TAG]->(t:Tag) WHERE t.text = $tag RETURN n.id AS node, t.id AS tag LIMIT 10",json!({"tag":"research"}).to_string(),json!([{"node":id(1),"tag":id(4)}])),
                ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10","{\"v\":1.0}".to_owned(),json!([{"id":id(1)}])),
                ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10","{\"v\":-0.0}".to_owned(),json!([{"id":id(2)}])),
                ("MATCH (n:Concept) RETURN n.P_72656d6f766564 AS removed, n.P_6368616e676564 AS changed, n.P_6e696c AS nil, n.P_6c69746572616c AS literal LIMIT 1","{}".to_owned(),json!([{"removed":null,"changed":true,"nil":null,"literal":"1"}])),
                ("MATCH (n:K_776f726b3a4974656d) RETURN n.id AS id LIMIT 1","{}".to_owned(),json!([{"id":id(2)}])),
                ("MATCH (n)-[e:contains]->(m) RETURN count(e) AS count LIMIT 1","{}".to_owned(),json!([{"count":2.0}])),
                ("MATCH (n:Concept) WHERE n.P_72656d6f766564 IS NULL AND n.P_6e696c IS NULL RETURN n.id AS id LIMIT 1","{}".to_owned(),json!([{"id":id(1)}])),
            ];
                for (source, parameters, expected) in corpus {
                    let (a, b) = tokio::join!(
                        first.query(&generation, source, parameters.as_bytes(), &cancel),
                        second.query(&generation, source, parameters.as_bytes(), &cancel)
                    );
                    let a = a?;
                    let b = b?;
                    if a.rows != b.rows || Value::Array(a.rows.clone()) != expected {
                        return Err(format!("native oracle mismatch: query={source}; actual={:?}; expected={expected}",a.rows).into());
                    }
                }
                // Truncated accepted prefix must never replace or damage the prior generation.
                let bad = root.join(format!("{domain}-incomplete.ndjson"));
                std::fs::write(&bad, format!("{}\n", serde_json::to_string(&m)?))?;
                if supervisor.build_export(&bad, &m).await.is_ok() {
                    return Err("incomplete prefix published".into());
                }
                let rows = first
                    .query(
                        &generation,
                        "MATCH (n) RETURN count(*) AS total LIMIT 1",
                        b"{}",
                        &cancel,
                    )
                    .await?;
                if rows.rows != vec![json!({"total":3.0})] {
                    return Err("prior generation changed".into());
                }
                let inspected = first.inspect(&generation, &id(1), false, &cancel).await?;
                let properties = inspected.rows.first().and_then(|row|row.get("properties")).ok_or("typed inspection missing")?;
                if properties.get("nil") != Some(&Value::Null) || properties.get("removed").is_some() { return Err("typed null/missing distinction lost".into()); }
                if !first.inspect(&generation, &id(3), false, &cancel).await?.rows.is_empty() || first.inspect(&generation, &id(3), true, &cancel).await?.rows.len() != 1 { return Err("typed archive inspection mismatch".into()); }
            } else {
                let parameters = json!({"id":id(1)}).to_string();
                let mut lookup = Vec::new();
                let mut traversal = Vec::new();
                let mut charged = 0;
                for _ in 0..40 {
                    for (query,times) in [("MATCH (n:Concept) WHERE n.id = $id RETURN n.id AS id LIMIT 1",&mut lookup),("MATCH (n:Concept)-[e:contains]->(m) WHERE n.id = $id RETURN e.id AS edge, m.id AS target ORDER BY edge LIMIT 10",&mut traversal)]{
                    let start=Instant::now();let response=first.query(&generation,query,parameters.as_bytes(),&cancel).await?;times.push(start.elapsed().as_micros());charged=charged.max(response.allocated_bytes);
                }
                }
                let lookup_p95 = percentile(&mut lookup);
                let traversal_p95 = percentile(&mut traversal);
                let mut sizes: Vec<u128> = sizes.into_iter().map(|n| n as u128).collect();
                let delta_p95 = percentile(&mut sizes);
                serde_json::to_writer(
                    std::io::stdout(),
                    &json!({"domain":domain,"nodes":100000,"relationships":300000,"retained_events":10000,"update_archive_tag_events":2000,"export_ms":export_ms,"bootstrap_ms":build_ms,"export_bytes":std::fs::metadata(&export)?.len(),"delta_min":sizes.first(),"delta_p95":delta_p95,"delta_max":sizes.last(),"lookup_p95_us":lookup_p95,"traversal_p95_us":traversal_p95,"charged_bytes":charged}),
                )?;
                std::io::stdout().write_all(b"\n")?;
                if lookup_p95 > 250000 || traversal_p95 > 1000000 {
                    return Err("performance envelope failed".into());
                }
            }
            serde_json::to_writer(
                std::io::stdout(),
                &json!({"mode":mode,"domain":domain,"complete":true,"bootstrap_ms":build_ms}),
            )?;
            std::io::stdout().write_all(b"\n")?;
        }
        Ok(())
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    proof::run().await
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
