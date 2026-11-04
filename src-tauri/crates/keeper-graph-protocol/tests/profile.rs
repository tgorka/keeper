use keeper_graph_protocol::{
    admit, custom_property, decode_symbol, encode_symbol, Refusal, SymbolKind, MAX_PARAMETER_BYTES,
    MAX_SOURCE_BYTES, PROFILE,
};

fn query(source: &str) -> keeper_graph_protocol::AdmittedQuery {
    admit(PROFILE, source, br#"{"p":1,"title":"hello"}"#).expect("admitted corpus")
}

#[test]
fn parameters_are_data_and_aliases_cannot_become_engine_tokens() {
    let q = admit(
        PROFILE,
        "MATCH (MATCH:Concept) WHERE MATCH.title = $title RETURN MATCH.id AS DELETE LIMIT 2",
        br#"{"title":"x') DELETE n; LOAD CSV FROM 'file:///secret'"}"#,
    )
    .expect("string is data");
    assert_eq!(
        q.engine_text(),
        "MATCH (v0:Concept) WHERE v0.title = $title RETURN v0.id AS a0 ORDER BY v0.id ASC LIMIT 2"
    );
    assert_eq!(q.aliases(), &["DELETE"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&q.parameters_json().expect("JSON"))
            .expect("valid JSON")["title"],
        "x') DELETE n; LOAD CSV FROM 'file:///secret'"
    );
}

#[test]
fn fixed_paths_tags_incoming_edges_and_ties_are_regenerated() {
    let q = query("MATCH (a:Concept)-[r:HAS_TAG]->(t:Tag)<-[:HAS_TAG]-(b) WHERE t.text = $title AND r.revision >= $p RETURN a.id AS id, r.id AS edge, b.title AS title ORDER BY title DESC, id LIMIT 1000");
    assert_eq!(q.engine_text(), "MATCH (v0:Concept)-[v1:HAS_TAG]->(v2:Tag)<-[v3:HAS_TAG]-(v4) WHERE v2.text = $title AND v1.revision >= $p RETURN v0.id AS a0, v1.id AS a1, v4.title AS a2 ORDER BY a2 DESC, a0 ASC, v0.id ASC, v1.id ASC, v2.id ASC, v3.id ASC, v4.id ASC LIMIT 1000");
}

#[test]
fn count_and_null_predicates_are_not_weakened() {
    assert_eq!(query("MATCH (a) WHERE a.description IS NULL AND a.merged_into_id IS NOT NULL RETURN count(a) AS total ORDER BY total DESC LIMIT 1").engine_text(), "MATCH (v0) WHERE v0.description IS NULL AND v0.merged_into_id IS NOT NULL RETURN count(v0) AS a0 ORDER BY a0 DESC LIMIT 1");
    assert_eq!(
        query("match (a) return COUNT(*) limit 1").engine_text(),
        "MATCH (v0) RETURN count(*) AS a0 LIMIT 1"
    );
    for op in ["=", "<>", "<", "<=", ">", ">="] {
        assert!(query(&format!(
            "MATCH (a) WHERE a.revision {op} $p RETURN a.id AS id LIMIT 1"
        ))
        .engine_text()
        .contains(&format!("v0.revision {op} $p")));
    }
}

#[test]
fn custom_symbols_round_trip_unicode_and_protect_structural_fields() {
    for (kind, name) in [
        (SymbolKind::Node, "work:żółć"),
        (SymbolKind::Edge, "work:depends_on"),
        (SymbolKind::Property, "score.α"),
    ] {
        let encoded = encode_symbol(kind, name).expect("mapped");
        assert_eq!(decode_symbol(kind, &encoded).expect("decoded"), name);
    }
    let label = encode_symbol(SymbolKind::Node, "work:Task").expect("label");
    let prop = custom_property("rating").expect("property");
    assert!(query(&format!(
        "MATCH (n:{label}) WHERE n.{prop} = $p RETURN n.{prop} AS rating LIMIT 1"
    ))
    .engine_text()
    .contains(&label));
    for key in ["id", "revision", "from_id", "lifecycle", "description"] {
        assert_eq!(custom_property(key), Err(Refusal::QueryUnsupported));
        let encoded = encode_symbol(SymbolKind::Property, key).expect("reversible encoding");
        assert!(admit(
            PROFILE,
            &format!("MATCH (n) RETURN n.{encoded} AS x LIMIT 1"),
            b"{}"
        )
        .is_err());
    }
    for invalid in ["K_", "K_ff", "K_776f726b3A5461736b", "K_666f6f", "K_123"] {
        assert!(
            decode_symbol(SymbolKind::Node, invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn numbers_normalize_and_duplicate_or_composite_parameters_refuse() {
    let source = "MATCH (n) WHERE n.revision = $p RETURN n.id AS id LIMIT 1";
    let normalized = |json: &[u8]| {
        admit(PROFILE, source, json)
            .expect("numeric parameter")
            .parameters_json()
            .expect("JSON")
    };
    assert_eq!(normalized(br#"{"p":1}"#), normalized(br#"{"p":1.0}"#));
    assert_eq!(normalized(br#"{"p":-0.0}"#), normalized(br#"{"p":0}"#));
    for json in [
        r#"{"p":9007199254740992}"#,
        r#"{"p":-9007199254740992}"#,
        r#"{"p":1e400}"#,
        r#"{"p":NaN}"#,
        r#"{"p":1,"p":2}"#,
        r#"{"p":[]}"#,
        r#"{"p":{}}"#,
        r#"{"p;DELETE":1}"#,
        r#"{} {}"#,
    ] {
        assert!(admit(PROFILE, source, json.as_bytes()).is_err(), "{json}");
    }
    for json in [
        br#"{"p":null}"#.as_slice(),
        br#"{"p":true}"#,
        br#"{"p":"1"}"#,
        br#"{"p":9007199254740991}"#,
    ] {
        assert!(admit(PROFILE, source, json).is_ok());
    }
}

#[test]
fn hostile_and_unlisted_grammar_never_reaches_renderer() {
    for source in [
        "LOAD CSV FROM $p AS x RETURN x LIMIT 1",
        "MATCH (n) CALL { DELETE n } RETURN n.id AS id LIMIT 1",
        "MATCH (n) RETURN n.id AS id LIMIT 1; DELETE n",
        "MATCH (n) /* comment */ RETURN n.id AS id LIMIT 1",
        "MATCH (n) // comment\nRETURN n.id AS id LIMIT 1",
        "MATCH (`n`) RETURN n.id AS id LIMIT 1",
        "MATCH (n:Unknown) RETURN n.id AS id LIMIT 1",
        "MATCH (n), (m) RETURN n.id AS id LIMIT 1",
        "MATCH (n)-[:contains*1..8]->(m) RETURN n.id AS id LIMIT 1",
        "MATCH (n)-[:contains]-(m) RETURN n.id AS id LIMIT 1",
        "MATCH (n) WHERE n.title = 'x' RETURN n.id AS id LIMIT 1",
        "MATCH (n) WHERE n.title = $p OR n.id = $p RETURN n.id AS id LIMIT 1",
        "MATCH (n) WHERE n.title = $missing RETURN n.id AS id LIMIT 1",
        "MATCH (n) RETURN n AS n LIMIT 1",
        "MATCH (n) RETURN n.id LIMIT 1",
        "MATCH (n) RETURN count(*), n.id AS id LIMIT 1",
        "MATCH (n) RETURN n.id AS id ORDER BY n.id LIMIT 1",
        "MATCH (n) RETURN n.id AS id SKIP 1 LIMIT 1",
        "MATCH (n) RETURN n.id AS id LIMIT $p",
        "MATCH (n) RETURN n.id AS id LIMIT 1 UNION MATCH (m) RETURN m.id AS id LIMIT 1",
        "MATCH (n) RETURN n.id AS id, n.title AS id LIMIT 1",
        "MATCH (n)-[n:contains]->(m) RETURN n.id AS id LIMIT 1",
        "MATCH (n) RETURN n.from_id AS id LIMIT 1",
        "MATCH (n) RETURN n.id AS id LIMIT 1 trailing",
        "MATCH (n)-é RETURN n.id AS id LIMIT 1",
    ] {
        assert!(admit(PROFILE, source, br#"{"p":1}"#).is_err(), "{source}");
    }
    assert!(admit("Cypher", "MATCH (n) RETURN count(*) LIMIT 1", b"{}").is_err());
}

#[test]
fn every_input_dimension_has_a_boundary() {
    let base = "MATCH (n) RETURN n.id AS id LIMIT 1";
    let source = format!("{base}{}", " ".repeat(MAX_SOURCE_BYTES - base.len()));
    assert!(admit(PROFILE, &source, b"{}").is_ok());
    assert_eq!(
        admit(PROFILE, &(source + " "), b"{}").err(),
        Some(Refusal::QueryLimit)
    );
    let parameters = format!("{{\"p\":\"{}\"}}", "x".repeat(MAX_PARAMETER_BYTES - 8));
    assert!(admit(PROFILE, base, parameters.as_bytes()).is_ok());
    assert_eq!(
        admit(PROFILE, base, (parameters + " ").as_bytes()).err(),
        Some(Refusal::QueryLimit)
    );
    for (limit, accepted) in [(0, false), (1, true), (1000, true), (1001, false)] {
        assert_eq!(
            admit(
                PROFILE,
                &format!("MATCH (n) RETURN count(*) LIMIT {limit}"),
                b"{}"
            )
            .is_ok(),
            accepted
        );
    }
    for (size, accepted) in [(8, true), (9, false)] {
        let chain = (0..size)
            .map(|i| format!("-[:contains]->(n{})", i + 1))
            .collect::<String>();
        assert_eq!(
            admit(
                PROFILE,
                &format!("MATCH (n0){chain} RETURN n0.id AS id LIMIT 1"),
                b"{}"
            )
            .is_ok(),
            accepted
        );
    }
    for (size, accepted) in [(16, true), (17, false)] {
        let returns = (0..size)
            .map(|i| format!("n.id AS a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            admit(
                PROFILE,
                &format!("MATCH (n) RETURN {returns} LIMIT 1"),
                b"{}"
            )
            .is_ok(),
            accepted
        );
    }
    for (size, accepted) in [(32, true), (33, false)] {
        let predicates = vec!["n.revision = $p"; size].join(" AND ");
        assert_eq!(
            admit(
                PROFILE,
                &format!("MATCH (n) WHERE {predicates} RETURN n.id AS id LIMIT 1"),
                br#"{"p":1}"#
            )
            .is_ok(),
            accepted
        );
        let params = (0..size)
            .map(|i| format!("\"p{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            admit(PROFILE, base, format!("{{{params}}}").as_bytes()).is_ok(),
            accepted
        );
    }
}

#[test]
fn normalized_parameters_remain_bounded_and_limits_stay_typed() {
    let source = "MATCH (n) RETURN n.id AS id LIMIT 1";
    let params = (0..33)
        .map(|i| format!("\"p{i}\":0"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        admit(PROFILE, source, format!("{{{params}}}").as_bytes()).err(),
        Some(Refusal::QueryLimit)
    );
    let overhead = "{\"x\":\"\",\"p\":1e3}".len();
    let params = format!(
        "{{\"x\":\"{}\",\"p\":1e3}}",
        "x".repeat(MAX_PARAMETER_BYTES - overhead)
    );
    assert_eq!(
        admit(PROFILE, source, params.as_bytes()).err(),
        Some(Refusal::QueryLimit)
    );
    assert_eq!(
        query("MATCH (n) RETURN n.id AS id ORDER BY id ASC, id DESC LIMIT 1").engine_text(),
        "MATCH (v0) RETURN v0.id AS a0 ORDER BY a0 ASC, a0 DESC, v0.id ASC LIMIT 1"
    );
}
