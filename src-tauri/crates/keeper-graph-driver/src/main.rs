#[cfg(any(target_os = "linux", target_os = "macos"))]
mod proof {
    use keeper_core::graph::{Cancellation, Supervisor};
    use keeper_graph_protocol::wire::{
        operation_digest, Change, EntityType, Event, Manifest, Operation, ENGINE,
    };
    use serde_json::{json, Value};
    use std::future::Future;
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
    fn percentile(values: &mut [u128], pct: usize) -> u128 {
        values.sort_unstable();
        values[(values.len() * pct).div_ceil(100).saturating_sub(1)]
    }
    fn emit(value: Value) -> Result<()> {
        serde_json::to_writer(std::io::stdout(), &value)?;
        std::io::stdout().write_all(b"\n")?;
        Ok(())
    }
    /// A reaped worker must be gone: `kill -0` fails for an absent PID.
    fn absent(pid: Option<u32>) -> Result<u32> {
        let pid = pid.ok_or("no worker was reaped")?;
        let alive = std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(std::process::Stdio::null())
            .status()?
            .success();
        if alive {
            return Err(format!("worker {pid} still present after supervisor returned").into());
        }
        Ok(pid)
    }
    fn generation_files(root: &Path) -> Result<Vec<String>> {
        Ok(generation_files_in(&root.join("generations")))
    }
    fn generation_files_in(directory: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // The supervisor's exclusive lock file is not a generation artifact.
            if !name.starts_with('.') {
                names.push(name);
            }
        }
        names.sort();
        names
    }
    /// Exports that must be refused without publishing anything.
    fn fault_export(path: &Path, domain: &str, kind: &str) -> Result<Manifest> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let m = manifest(domain, 2);
        header(&mut file, &m)?;
        let mut sizes = Vec::new();
        let a = node(1);
        event(
            &mut file,
            1,
            vec![put(EntityType::Node, a.clone(), None)],
            &mut sizes,
        )?;
        let mut changed = a.clone();
        changed["revision"] = json!("2");
        match kind {
            "digest" => {
                let ops = vec![put(EntityType::Node, changed, Some(a))];
                let value = Event {
                    sequence: 2,
                    operations: ops,
                    digest: "0".repeat(64),
                };
                serde_json::to_writer(&mut file, &value)?;
                file.write_all(b"\n")?;
            }
            "before-image" => {
                let mut stale = a.clone();
                stale["title"] = json!("not the accepted value");
                event(
                    &mut file,
                    2,
                    vec![put(EntityType::Node, changed, Some(stale))],
                    &mut sizes,
                )?;
            }
            "dangling" => event(
                &mut file,
                2,
                vec![put(EntityType::Edge, edge(20, 1, 99), None)],
                &mut sizes,
            )?,
            "gap" => event(
                &mut file,
                3,
                vec![put(EntityType::Node, changed, Some(a))],
                &mut sizes,
            )?,
            "trailing" => {
                event(
                    &mut file,
                    2,
                    vec![put(EntityType::Node, changed, Some(a))],
                    &mut sizes,
                )?;
                file.write_all(b"{\"extra\":true}\n")?;
            }
            "truncated" => {}
            _ => return Err("unknown fault".into()),
        }
        file.flush()?;
        Ok(m)
    }
    fn limits_export(path: &Path, domain: &str) -> Result<Manifest> {
        // Hub 1 → 10,000 leaves (fan-out) plus 1,000 nodes with a 60 KiB text.
        let mut ops = Vec::new();
        let mut node1 = node(1);
        node1["properties"] = json!({});
        ops.push(put(EntityType::Node, node1, None));
        for n in 2..=10001u64 {
            ops.push(put(EntityType::Node, node(n), None));
        }
        for n in 0..10000u64 {
            ops.push(put(EntityType::Edge, edge(20000 + n, 1, 2 + n), None));
        }
        let blob = "x".repeat(60 * 1024);
        let mut events: Vec<Vec<Operation>> = ops.chunks(100).map(<[Operation]>::to_vec).collect();
        let mut blobs = Vec::new();
        for n in 0..1000u64 {
            let mut b = node(40000 + n);
            b["properties"] = json!({"blob": blob});
            blobs.push(put(EntityType::Node, b, None));
        }
        events.extend(blobs.chunks(30).map(<[Operation]>::to_vec));
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let m = manifest(domain, u64::try_from(events.len())?);
        header(&mut file, &m)?;
        let mut sizes = Vec::new();
        for (i, ops) in events.into_iter().enumerate() {
            event(&mut file, u64::try_from(i + 1)?, ops, &mut sizes)?;
        }
        file.flush()?;
        Ok(m)
    }
    async fn oracle(supervisor: &Supervisor, root: &Path, domain: &str) -> Result<()> {
        let export = root.join(format!("{domain}.ndjson"));
        let m = oracle_export(&export, domain)?;
        let cancel = Cancellation::default();
        let generation = supervisor.build_export(&export, &m, &cancel).await?;
        let first = supervisor.session("client-a", "context-a", domain)?;
        let second = supervisor.session("client-b", "context-b", domain)?;
        let corpus = [
            ("MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, e.id AS edge ORDER BY edge LIMIT 10", "{}".to_owned(), json!([{"node":id(1),"edge":id(11)},{"node":id(1),"edge":id(12)}])),
            // Tie on `node`: canonical edge IDs order the parallel edges deterministically.
            ("MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, m.id AS target ORDER BY node DESC LIMIT 1", "{}".to_owned(), json!([{"node":id(1),"target":id(2)}])),
            ("MATCH (n:Concept)-[e:contains]->(m) RETURN e.id AS edge ORDER BY edge DESC LIMIT 1", "{}".to_owned(), json!([{"edge":id(12)}])),
            ("MATCH (n:Concept)-[e:contains]->(m:K_776f726b3a4974656d) RETURN e.id AS edge LIMIT 1", "{}".to_owned(), json!([{"edge":id(11)}])),
            ("MATCH (n)-[e:HAS_TAG]->(t:Tag) WHERE t.text = $tag RETURN n.id AS node, t.id AS tag LIMIT 10", json!({"tag":"research"}).to_string(), json!([{"node":id(1),"tag":id(4)}])),
            ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", "{\"v\":1.0}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", "{\"v\":1}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 10", "{\"v\":-0.0}".to_owned(), json!([{"id":id(2)}])),
            ("MATCH (n) WHERE n.P_73636f7265 > $v RETURN n.id AS id LIMIT 10", "{\"v\":0.5}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n) WHERE n.P_73636f7265 < $v RETURN n.id AS id LIMIT 10", "{\"v\":0.5}".to_owned(), json!([{"id":id(2)}])),
            ("MATCH (n) WHERE n.P_73636f7265 <= $v RETURN n.id AS id LIMIT 10", "{\"v\":0}".to_owned(), json!([{"id":id(2)}])),
            ("MATCH (n) WHERE n.P_73636f7265 >= $v RETURN n.id AS id LIMIT 10", "{\"v\":1.0}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n) WHERE n.P_73636f7265 <> $v RETURN n.id AS id LIMIT 10", "{\"v\":1}".to_owned(), json!([{"id":id(2)}])),
            ("MATCH (n) WHERE n.P_73636f7265 >= $lo AND n.P_73636f7265 <= $hi RETURN n.id AS id ORDER BY id DESC LIMIT 10", "{\"lo\":0,\"hi\":1}".to_owned(), json!([{"id":id(2)},{"id":id(1)}])),
            ("MATCH (n) WHERE n.P_6c69746572616c = $v RETURN n.id AS id LIMIT 10", "{\"v\":1}".to_owned(), json!([])),
            ("MATCH (n) WHERE n.P_6c69746572616c = $v RETURN n.id AS id LIMIT 10", "{\"v\":\"1\"}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n:Concept) RETURN n.P_72656d6f766564 AS removed, n.P_6368616e676564 AS changed, n.P_6e696c AS nil, n.P_6c69746572616c AS literal LIMIT 1", "{}".to_owned(), json!([{"removed":null,"changed":true,"nil":null,"literal":"1"}])),
            ("MATCH (n:K_776f726b3a4974656d) RETURN n.id AS id LIMIT 1", "{}".to_owned(), json!([{"id":id(2)}])),
            ("MATCH (n)-[e:contains]->(m) RETURN count(e) AS count LIMIT 1", "{}".to_owned(), json!([{"count":2.0}])),
            ("MATCH (n) RETURN count(*) AS total LIMIT 1", "{}".to_owned(), json!([{"total":3.0}])),
            ("MATCH (n:Concept) WHERE n.P_72656d6f766564 IS NULL AND n.P_6e696c IS NULL RETURN n.id AS id LIMIT 1", "{}".to_owned(), json!([{"id":id(1)}])),
            ("MATCH (n:Concept) WHERE n.P_6368616e676564 IS NOT NULL RETURN n.id AS id LIMIT 5", "{}".to_owned(), json!([{"id":id(1)}])),
        ];
        for (source, parameters, expected) in &corpus {
            let (a, b) = tokio::join!(
                first.query(&generation, source, parameters.as_bytes(), &cancel),
                second.query(&generation, source, parameters.as_bytes(), &cancel)
            );
            let (a, b) = (a?, b?);
            if a.rows != b.rows || &Value::Array(a.rows.clone()) != expected {
                return Err(format!(
                    "native oracle mismatch: query={source}; actual={:?}; expected={expected}",
                    a.rows
                )
                .into());
            }
        }
        // F3: a valid <=64 KiB parameter whose every byte needs JSON escaping must
        // cross the parent/worker frame and execute natively, not fail transport.
        let escaped = json!({ "s": "\\".repeat(32_000) }).to_string();
        let boundary_source = "MATCH (n:Concept) WHERE n.title = $s RETURN n.id AS id LIMIT 1";
        let boundary = first
            .query(&generation, boundary_source, escaped.as_bytes(), &cancel)
            .await?;
        if !boundary.rows.is_empty() {
            return Err("escaped boundary parameter matched a node".into());
        }
        let over = json!({ "s": "\\".repeat(32_800) }).to_string();
        if !matches!(
            first
                .query(&generation, boundary_source, over.as_bytes(), &cancel)
                .await,
            Err(keeper_core::graph::Error::Limit)
        ) {
            return Err("parameter beyond 64 KiB normalized bytes was not query_limit".into());
        }
        // Typed refusals reach no worker.
        for source in [
            "MATCH (n) RETURN n.id AS id LIMIT 1; MATCH (m) DETACH DELETE m",
            "LOAD CSV FROM 'file:///etc/passwd' AS r RETURN r LIMIT 1",
        ] {
            if !matches!(
                first.query(&generation, source, b"{}", &cancel).await,
                Err(keeper_core::graph::Error::Unsupported)
            ) {
                return Err(format!("hostile query not refused: {source}").into());
            }
        }
        let inspected = first.inspect(&generation, &id(1), false, &cancel).await?;
        let properties = inspected
            .rows
            .first()
            .and_then(|row| row.get("properties"))
            .ok_or("typed inspection missing")?;
        if properties.get("nil") != Some(&Value::Null) || properties.get("removed").is_some() {
            return Err("typed null/missing distinction lost".into());
        }
        if !first
            .inspect(&generation, &id(3), false, &cancel)
            .await?
            .rows
            .is_empty()
            || first
                .inspect(&generation, &id(3), true, &cancel)
                .await?
                .rows
                .len()
                != 1
        {
            return Err("typed archive inspection mismatch".into());
        }
        // Failed publication never replaces or alters the selected generation.
        let before_files = generation_files(root)?;
        let mut refused = Vec::new();
        for kind in [
            "truncated",
            "digest",
            "before-image",
            "dangling",
            "gap",
            "trailing",
        ] {
            let bad = root.join(format!("{domain}-{kind}.ndjson"));
            let m = fault_export(&bad, domain, kind)?;
            match supervisor.build_export(&bad, &m, &cancel).await {
                Ok(_) => return Err(format!("faulted export published: {kind}").into()),
                Err(e) => refused.push(json!({"fault":kind,"error":e.to_string(),"reaped_pid":absent(supervisor.last_reaped())?})),
            }
        }
        if generation_files(root)? != before_files {
            return Err("failed publication left staging, sidecar or generation files".into());
        }
        let again = first
            .query(&generation, corpus[0].0, b"{}", &cancel)
            .await?;
        if Value::Array(again.rows) != corpus[0].2 {
            return Err("prior generation changed after failed publication".into());
        }
        emit(
            json!({"mode":"oracle","domain":domain,"corpus":corpus.len(),"clients":2,"publication_faults_refused":refused,"generation_files":before_files}),
        )
    }
    async fn benchmark(
        supervisor: &Supervisor,
        root: &Path,
        domain: &str,
        cold: bool,
    ) -> Result<()> {
        let export = root.join(format!("{domain}.ndjson"));
        let start = Instant::now();
        let (m, sizes) = benchmark_export(&export, domain)?;
        let export_ms = start.elapsed().as_millis();
        let cancel = Cancellation::default();
        let start = Instant::now();
        let generation = supervisor.build_export(&export, &m, &cancel).await?;
        let build_ms = start.elapsed().as_millis();
        let session = supervisor.session("bench", "bench", domain)?;
        let node_parameters = json!({"id":id(1)}).to_string();
        let tag_parameters = json!({"tag":id(99999)}).to_string();
        let queries = [
            ("node_lookup", "MATCH (n:Concept) WHERE n.id = $id RETURN n.id AS id, n.title AS title LIMIT 1", &node_parameters),
            ("tag_lookup", "MATCH (n:Concept)-[e:HAS_TAG]->(t:Tag) WHERE t.id = $tag RETURN n.id AS id LIMIT 10", &tag_parameters),
            ("traversal_1hop", "MATCH (n:Concept)-[e:contains]->(m) WHERE n.id = $id RETURN e.id AS edge, m.id AS target ORDER BY edge LIMIT 10", &node_parameters),
            ("traversal_2hop", "MATCH (a:Concept)-[e1:contains]->(b)-[e2:contains]->(c) WHERE a.id = $id RETURN c.id AS id ORDER BY id LIMIT 10", &node_parameters),
        ];
        let mut results = serde_json::Map::new();
        let mut charged = 0;
        let mut residual = 0;
        let mut worker_rss = 0;
        for (name, query, parameters) in queries {
            let mut times = Vec::new();
            let mut rows = 0;
            for iteration in 0..45 {
                let start = Instant::now();
                let response = session
                    .query(&generation, query, parameters.as_bytes(), &cancel)
                    .await?;
                if iteration >= 5 {
                    times.push(start.elapsed().as_micros());
                }
                rows = response.rows.len();
                charged = charged.max(response.peak_allocated_bytes);
                residual = residual.max(response.allocated_bytes);
                worker_rss = worker_rss.max(response.peak_rss_bytes);
            }
            let p95 = percentile(&mut times, 95);
            let p50 = percentile(&mut times, 50);
            let budget = if name.starts_with("traversal") {
                1_000_000
            } else {
                250_000
            };
            results.insert(name.into(), json!({"p50_us":p50,"p95_us":p95,"rows":rows,"target_us":budget,"pass":p95 <= budget}));
        }
        let mut sizes: Vec<u128> = sizes.into_iter().map(|n| n as u128).collect();
        let generation_bytes = std::fs::metadata(
            root.join("generations").join(
                generation_files(root)?
                    .into_iter()
                    .rfind(|n| n.ends_with(".db"))
                    .ok_or("no generation")?,
            ),
        )?
        .len();
        let pass = results.values().all(|r| r["pass"] == json!(true));
        emit(
            json!({"mode":"benchmark","domain":domain,"first_build_in_process":cold,"nodes":100000,"relationships":300000,"hub_out_degree":10000,
            "retained_events":10000,"update_archive_tag_events":2000,"export_ms":export_ms,"bootstrap_build_publish_ms":build_ms,
            "export_bytes":std::fs::metadata(&export)?.len(),"latest_generation_bytes":generation_bytes,
            "delta_bytes":{"min":percentile(&mut sizes,0),"p50":percentile(&mut sizes,50),"p95":percentile(&mut sizes,95),"max":percentile(&mut sizes,100)},
            "warm_queries":results,"max_query_peak_charged_bytes":charged,"max_query_residual_charged_bytes_at_exit":residual,"max_query_worker_peak_rss_bytes":worker_rss,
            "builder_peak_rss_bytes":generation.build_peak_rss_bytes,"builder_peak_charged_bytes":generation.build_peak_allocated_bytes,
            "builder_residual_charged_bytes_at_exit":generation.build_final_allocated_bytes,"envelope_pass":pass}),
        )?;
        if !pass {
            return Err("performance envelope failed".into());
        }
        Ok(())
    }
    async fn limits(supervisor: &Supervisor, root: &Path, fake: &Path) -> Result<()> {
        let domain = "fixture-limits";
        let export = root.join("limits.ndjson");
        let m = limits_export(&export, domain)?;
        let cancel = Cancellation::default();
        let generation = supervisor.build_export(&export, &m, &cancel).await?;
        let published = generation_files(root)?;
        let generation_bytes = std::fs::read(root.join("generations").join(&published[0]))?;
        let session = supervisor.session("limits", "limits", domain)?;
        let hub = json!({"id":id(1)}).to_string();
        let control =
            "MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN count(*) AS total LIMIT 1";
        let baseline = session
            .query(&generation, control, hub.as_bytes(), &cancel)
            .await?
            .rows;
        let mut outcomes = Vec::new();
        // Native materialization: 1,000 rows × 16 cells × 60 KiB exceeds the 256 MiB budget.
        let wide = (0..16)
            .map(|i| format!("n.P_626c6f62 AS b{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let materialize =
            format!("MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN {wide} LIMIT 1000");
        let fanout = "MATCH (x)<-[e1:contains]-(h:Concept)-[e2:contains]->(y)<-[e3:contains]-(z) WHERE h.id = $id RETURN count(*) AS total LIMIT 1";
        let cases: [(&str, String, &str); 3] = [
            ("native_materialization_budget", materialize, "{}"),
            ("encoded_result_cap", "MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 100".into(), "{}"),
            ("wall_deadline", fanout.into(), hub.as_str()),
        ];
        for (name, query, parameters) in cases {
            let start = Instant::now();
            let result = session
                .query(&generation, &query, parameters.as_bytes(), &cancel)
                .await;
            let elapsed = start.elapsed().as_millis();
            let pid = absent(supervisor.last_reaped())?;
            if !matches!(result, Err(keeper_core::graph::Error::Limit)) {
                return Err(format!(
                    "{name} did not fail with query_limit: {:?}",
                    result.map(|r| r.rows.len())
                )
                .into());
            }
            outcomes.push(json!({"case":name,"error":"query_limit","elapsed_ms":elapsed,"reaped_pid":pid,"partial_rows":0}));
        }
        // Cancellation kills and reaps the native worker mid-computation.
        let cancelled = Cancellation::default();
        let trigger = cancelled.clone();
        let start = Instant::now();
        let (result, _) = tokio::join!(
            session.query(&generation, fanout, hub.as_bytes(), &cancelled),
            async {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                trigger.cancel();
            }
        );
        let elapsed = start.elapsed().as_millis();
        if !matches!(result, Err(keeper_core::graph::Error::Cancelled)) || elapsed > 2000 {
            return Err(format!("cancellation not honoured in {elapsed} ms").into());
        }
        outcomes.push(json!({"case":"cancel","error":"query_cancelled","elapsed_ms":elapsed,"reaped_pid":absent(supervisor.last_reaped())?}));
        // Cancelled build: nothing published, staging removed.
        let build_cancel = Cancellation::default();
        let trigger = build_cancel.clone();
        let (built, _) = tokio::join!(supervisor.build_export(&export, &m, &build_cancel), async {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            trigger.cancel();
        });
        if !matches!(built, Err(keeper_core::graph::Error::Cancelled))
            || generation_files(root)? != published
        {
            return Err("cancelled build published or left files".into());
        }
        outcomes.push(json!({"case":"cancel_build","error":"query_cancelled","reaped_pid":absent(supervisor.last_reaped())?,"generation_files":published}));
        // Bounded admission: the queue admits 16; extra concurrent requests are refused.
        let sessions: Vec<_> = (0..3)
            .map(|i| supervisor.session("limits-agent", &format!("ctx-{i}"), domain))
            .collect::<std::result::Result<_, _>>()?;
        let results = poll_together(
            (0..18).map(|i| sessions[i % 3].query(&generation, control, hub.as_bytes(), &cancel)),
        )
        .await;
        let refused = results
            .iter()
            .filter(|r| matches!(r, Err(keeper_core::graph::Error::Limit)))
            .count();
        let completed = results
            .iter()
            .filter(|r| r.as_ref().is_ok_and(|r| r.rows == baseline))
            .count();
        if refused != 2 || completed != 16 || supervisor.peak_workers() > 2 {
            return Err(format!(
                "admission bounds violated: refused={refused} completed={completed} peak={}",
                supervisor.peak_workers()
            )
            .into());
        }
        outcomes.push(json!({"case":"admission","requests":18,"refused_query_limit":refused,"completed":completed,"peak_live_workers":supervisor.peak_workers()}));
        // A worker killed by a signal mid-query is a typed failure, never a result.
        let crash = supervisor.session("limits", "crash", domain)?;
        let watcher = supervisor.clone();
        let start = Instant::now();
        let (result, signalled) = tokio::join!(
            crash.query(&generation, fanout, hub.as_bytes(), &cancel),
            async move {
                for _ in 0..300 {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    if let Some(pid) = watcher.live_workers().first().copied() {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        let sent = std::process::Command::new("kill")
                            .args(["-ABRT", &pid.to_string()])
                            .status()
                            .is_ok_and(|s| s.success());
                        return sent.then_some(pid);
                    }
                }
                None
            }
        );
        let elapsed = start.elapsed().as_millis();
        if signalled.is_none()
            || !matches!(result, Err(keeper_core::graph::Error::Unavailable))
            || elapsed > 3_500
        {
            return Err(format!(
                "aborted worker not a typed failure: {:?} after {elapsed} ms",
                result.map(|r| r.rows.len())
            )
            .into());
        }
        outcomes.push(json!({"case":"abort_mid_query","signal":"SIGABRT","error":"projection_unavailable","elapsed_ms":elapsed,"reaped_pid":absent(supervisor.last_reaped())?}));
        // Builder admission: concurrent builds beyond MAX_BUILDERS are refused before
        // any staging file exists; retained generations are bounded.
        let small = root.join("small.ndjson");
        let small_m = oracle_export(&small, domain)?;
        let builds =
            poll_together((0..3).map(|_| supervisor.build_export(&small, &small_m, &cancel))).await;
        let started = builds.iter().filter(|r| r.is_ok()).count();
        let refused = builds
            .iter()
            .filter(|r| matches!(r, Err(keeper_core::graph::Error::Limit)))
            .count();
        if started != keeper_core::graph::MAX_BUILDERS || refused != 3 - started {
            return Err(format!("builder admission: started={started} refused={refused}").into());
        }
        let retained = builds.into_iter().find_map(|r| r.ok()).ok_or("built")?;
        // Two referenced (limits + retained): a third publication is refused.
        let over = supervisor.build_export(&small, &small_m, &cancel).await;
        if !matches!(over, Err(keeper_core::graph::Error::Limit)) {
            return Err("retained generation bound not enforced".into());
        }
        drop(retained);
        let mut on_disk = Vec::new();
        for _ in 0..4 {
            let next = supervisor.build_export(&small, &small_m, &cancel).await?;
            let artifacts = supervisor.artifacts()?;
            if artifacts.len() > keeper_core::graph::MAX_RETAINED_GENERATIONS {
                return Err(format!("repeated publication grew storage: {artifacts:?}").into());
            }
            on_disk.push(artifacts.len());
            drop(next);
        }
        outcomes.push(json!({"case":"builder_and_retention_bounds","concurrent_builds":3,"started":started,"refused_query_limit":refused,
            "third_retained_refused":true,"artifacts_after_each_of_4_publications":on_disk}));
        // A real rename failure: the generations directory becomes read-only after the
        // staging file exists. Nothing is published; the unremovable staging leftover
        // is reclaimed by the next build.
        let directory = root.join("generations");
        let staged = directory.clone();
        let (built, _) = tokio::join!(supervisor.build_export(&export, &m, &cancel), async move {
            for _ in 0..4000 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                if generation_files_in(&staged)
                    .iter()
                    .any(|n| n.ends_with(".staging"))
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o555));
                    return;
                }
            }
        });
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))?;
        }
        let left = generation_files(root)?;
        if !matches!(built, Err(keeper_core::graph::Error::Storage(_)))
            || left.iter().filter(|n| n.ends_with(".db")).count() != 1
        {
            return Err(
                format!("rename failure published or was not a storage error: {left:?}").into(),
            );
        }
        let reclaimed = supervisor.build_export(&small, &small_m, &cancel).await?;
        let after_reclaim = generation_files(root)?;
        if after_reclaim.iter().any(|n| n.ends_with(".staging")) {
            return Err("failed-publication leftover was not reclaimed".into());
        }
        drop(reclaimed);
        outcomes.push(json!({"case":"rename_failure","error":"storage_unavailable","left_after_failure":left,"after_next_build":after_reclaim}));
        // Misbehaving worker executables: malformed, oversized, wrong-PID output, a
        // process that never answers and an abort. Each is a typed failure, reaped;
        // the hang is bounded by the deadline counted from spawn.
        for behaviour in ["malformed", "oversized", "wrong-pid", "hang", "abort"] {
            let binary = root.join(format!("fake-{behaviour}"));
            std::fs::copy(fake, &binary)?;
            let fake_supervisor = Supervisor::new(
                &binary,
                &root.join(format!("fake-{behaviour}-generations")),
                true,
            )?;
            let fake_generation = fake_supervisor
                .build_export(&small, &small_m, &cancel)
                .await?;
            let session = fake_supervisor.session("fake", "fake", domain)?;
            let start = Instant::now();
            let result = session
                .query(
                    &fake_generation,
                    "MATCH (n) RETURN count(*) AS total LIMIT 1",
                    b"{}",
                    &cancel,
                )
                .await;
            let elapsed = start.elapsed().as_millis();
            let expected = if behaviour == "hang" {
                matches!(result, Err(keeper_core::graph::Error::Limit)) && elapsed < 3_500
            } else {
                matches!(result, Err(keeper_core::graph::Error::Unavailable))
            };
            if !expected {
                return Err(format!(
                    "fake {behaviour}: {:?} after {elapsed} ms",
                    result.map(|r| r.rows.len())
                )
                .into());
            }
            outcomes.push(json!({"case":format!("worker_{behaviour}"),"error":result.err().map(|e| e.to_string()),"elapsed_ms":elapsed,
                "reaped_pid":absent(fake_supervisor.last_reaped())?}));
        }
        // Projection intact after every limit fired.
        let after = session
            .query(&generation, control, hub.as_bytes(), &cancel)
            .await?
            .rows;
        let unchanged =
            std::fs::read(root.join("generations").join(&published[0]))? == generation_bytes;
        if after != baseline || !unchanged {
            return Err("published projection changed".into());
        }
        emit(
            json!({"mode":"limits","outcomes":outcomes,"baseline":baseline,"projection_bytes_unchanged":unchanged}),
        )
    }
    /// Polls every future in the same pass so all admissions race together.
    async fn poll_together<F: Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
        let mut pending: Vec<Option<std::pin::Pin<Box<F>>>> =
            futures.map(|f| Some(Box::pin(f))).collect();
        let mut outputs = Vec::with_capacity(pending.len());
        std::future::poll_fn(|cx| {
            let mut done = true;
            for slot in pending.iter_mut() {
                if let Some(future) = slot.as_mut() {
                    match future.as_mut().poll(cx) {
                        std::task::Poll::Ready(output) => {
                            outputs.push(output);
                            *slot = None;
                        }
                        std::task::Poll::Pending => done = false,
                    }
                }
            }
            if done {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
        outputs
    }
    pub async fn run() -> Result<()> {
        let args: Vec<String> = std::env::args().collect();
        let mode = args
            .get(1)
            .ok_or("oracle, benchmark or limits required")?
            .clone();
        let worker = PathBuf::from(args.get(2).ok_or("worker executable required")?);
        let root = PathBuf::from(args.get(3).ok_or("owned scratch directory required")?);
        if root.exists() {
            return Err("proof scratch must be new".into());
        }
        std::fs::create_dir_all(&root)?;
        let supervisor = Supervisor::new(&worker, &root.join("generations"), true)?;
        match mode.as_str() {
            "oracle" => {
                for domain in ["fixture-personal", "fixture-shared"] {
                    oracle(&supervisor, &root, domain).await?;
                }
            }
            "benchmark" => {
                for (i, domain) in ["fixture-personal", "fixture-shared"]
                    .into_iter()
                    .enumerate()
                {
                    benchmark(&supervisor, &root, domain, i == 0).await?;
                }
            }
            "limits" => {
                let fake = PathBuf::from(args.get(4).ok_or("fake worker executable required")?);
                limits(&supervisor, &root, &fake).await?
            }
            _ => return Err("unknown proof mode".into()),
        }
        emit(json!({"mode":mode,"complete":true,"peak_live_workers":supervisor.peak_workers()}))
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
