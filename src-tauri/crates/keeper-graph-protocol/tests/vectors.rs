//! The staged shared vector file must hold for this implementation.
use keeper_graph_protocol::wire::canonical_bytes;
use keeper_graph_protocol::{
    admit, custom_property, decode_symbol, encode_symbol, SymbolKind, PROFILE,
};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("../vectors/kg1-staged.vectors.json")).expect("vector JSON")
}
fn kind(name: &str) -> SymbolKind {
    match name {
        "node" => SymbolKind::Node,
        "edge" => SymbolKind::Edge,
        _ => SymbolKind::Property,
    }
}
#[test]
fn staged_vectors_hold() {
    let v = vectors();
    for s in v["symbols"].as_array().expect("symbols") {
        let (k, canonical, symbol) = (
            kind(s["kind"].as_str().expect("kind")),
            s["canonical"].as_str().expect("c"),
            s["symbol"].as_str().expect("s"),
        );
        assert_eq!(encode_symbol(k, canonical).expect("encode"), symbol);
        assert_eq!(decode_symbol(k, symbol).expect("decode"), canonical);
    }
    for key in v["reserved_property_refusals"]
        .as_array()
        .expect("reserved")
    {
        assert!(custom_property(key.as_str().expect("key")).is_err());
    }
    let source = "MATCH (n) WHERE n.revision = $p RETURN n.id AS id LIMIT 1";
    for p in v["parameters"].as_array().expect("parameters") {
        let result = admit(
            PROFILE,
            source,
            p["input"].as_str().expect("input").as_bytes(),
        );
        match p.get("normalized") {
            Some(expected) => {
                let actual: Value = serde_json::from_str(
                    &result.expect("admitted").parameters_json().expect("json"),
                )
                .expect("json");
                let expected: Value =
                    serde_json::from_str(expected.as_str().expect("normalized")).expect("json");
                assert_eq!(actual, expected);
            }
            None => assert_eq!(
                result.err().map(|e| e.to_string()).as_deref(),
                p["refusal"].as_str()
            ),
        }
    }
    for j in v["jcs"].as_array().expect("jcs") {
        let input: Value = serde_json::from_str(j["input"].as_str().expect("input")).expect("json");
        assert_eq!(
            canonical_bytes(&input).expect("canonical"),
            j["canonical"].as_str().expect("canonical").as_bytes(),
            "{}",
            j["source"]
        );
    }
    for r in v["renderer"].as_array().expect("renderer") {
        let q = admit(
            PROFILE,
            r["source"].as_str().expect("source"),
            br#"{"tag":"x"}"#,
        )
        .expect("admitted");
        assert_eq!(q.engine_text(), r["engine"].as_str().expect("engine"));
    }
}
